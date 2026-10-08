//! Private local Git storage for authenticated API handlers.
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{process::Command, sync::Mutex};

/// Errors from repository validation, filesystem access, and Git execution.
#[derive(Debug)]
pub enum GitError {
    InvalidInput(&'static str),
    InvalidData(&'static str),
    Io(io::Error),
    Timeout,
    CommandFailed {
        status: std::process::ExitStatus,
        stderr: String,
    },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) | Self::InvalidData(message) => f.write_str(message),
            Self::Io(error) => write!(f, "Git I/O error: {error}"),
            Self::Timeout => f.write_str("Git operation timed out"),
            Self::CommandFailed { status, stderr } => {
                f.write_fmt(format_args!("Git command failed ({status}): {stderr}"))
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for GitError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone)]
pub struct GitClient {
    repository_root: PathBuf,
    repository_locks: Arc<Mutex<HashMap<i32, Arc<Mutex<()>>>>>,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct GitRef {
    pub commit_sha: String,
    pub name: String,
}

impl GitClient {
    /// Prepare private local storage; no server or SSH keys are needed.
    pub fn from_env() -> Result<Self, GitError> {
        let storage = std::env::var_os("GIT_STORAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../storage/git"));
        Self::configured(storage)
    }

    pub(crate) fn configured(storage: PathBuf) -> Result<Self, GitError> {
        if !storage.is_absolute() {
            return Err(GitError::InvalidInput("GIT_STORAGE_DIR must be absolute"));
        }
        let repository_root = storage.join("projects");
        reject_symlinks(&repository_root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&repository_root)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&repository_root)?;
        Ok(Self {
            repository_root: repository_root.canonicalize()?,
            repository_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    fn repository_path(&self, project_id: i32) -> Result<PathBuf, GitError> {
        validate_project_id(project_id)?;
        let repository = self.repository_root.join(format!("{project_id}.git"));
        reject_symlinks(&repository)?;
        Ok(repository)
    }

    /// Create a bare repository, reusing it if it already exists.
    /// Callers must first authorize access to the database project.
    pub async fn create_project_repository(&self, project_id: i32) -> Result<PathBuf, GitError> {
        validate_project_id(project_id)?;
        let lock = self
            .repository_locks
            .lock()
            .await
            .entry(project_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;
        let repository = self.repository_path(project_id)?;
        if !repository.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new().mode(0o700).create(&repository)?;
            }
            #[cfg(not(unix))]
            std::fs::create_dir(&repository)?;
        }
        if !repository.join("HEAD").exists() {
            let mut command = self.command();
            command
                .args([
                    "init",
                    "--bare",
                    "--quiet",
                    "--template=",
                    "--object-format=sha1",
                ])
                .arg(&repository);
            run(command).await?;
        }
        let mut command = self.command();
        command
            .arg("--git-dir")
            .arg(&repository)
            .args(["rev-parse", "--is-bare-repository"]);
        if run(command).await?.as_slice() != b"true\n" {
            return Err(GitError::InvalidInput(
                "Project storage must be a bare Git repository",
            ));
        }
        Ok(repository)
    }

    /// Create storage exclusively for a new project; never adopt existing storage.
    pub async fn create_new_project_repository(&self, project_id: i32) -> Result<(), GitError> {
        let repository = self.repository_path(project_id)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&repository)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir(&repository)?;
        if let Err(error) = self.create_project_repository(project_id).await {
            if let Err(cleanup) = self.remove_project_repository(project_id) {
                eprintln!("Could not clean up repository for project {project_id}: {cleanup}");
            }
            return Err(error);
        }
        Ok(())
    }

    /// Caller must establish that no committed project owns this repository.
    pub fn remove_project_repository(&self, project_id: i32) -> Result<(), GitError> {
        let repository = self.repository_path(project_id)?;
        match std::fs::remove_dir_all(repository) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// List permanently retained snapshots directly from local storage.
    pub async fn list_snapshot_refs(&self, project_id: i32) -> Result<Vec<GitRef>, GitError> {
        let mut command = self.command();
        command
            .arg("--git-dir")
            .arg(self.repository_path(project_id)?)
            .args([
                "for-each-ref",
                "--format=%(objectname)%09%(refname)",
                "refs/snapshots/",
            ]);
        let output = run(command).await?;
        let output = String::from_utf8(output)
            .map_err(|_| GitError::InvalidData("Invalid Git reference output"))?;
        output
            .lines()
            .map(|line| {
                let (commit_sha, name) = line
                    .split_once('\t')
                    .ok_or_else(|| GitError::InvalidData("Invalid Git reference output"))?;
                Ok(GitRef {
                    commit_sha: commit_sha.to_owned(),
                    name: name.to_owned(),
                })
            })
            .collect()
    }

    /// Import a commit from a local repository and publish an immutable snapshot.
    /// Callers must authorize the project and supply an API-owned source repository.
    pub async fn publish_snapshot(
        &self,
        project_id: i32,
        snapshot_id: &str,
        source: &Path,
        commit_sha: &str,
    ) -> Result<(), GitError> {
        let valid_id = !snapshot_id.is_empty()
            && snapshot_id
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
            && snapshot_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !valid_id || commit_sha.len() != 40 || !commit_sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(GitError::InvalidInput("Invalid snapshot ID or commit SHA"));
        }
        reject_symlinks(source)?;
        let source = source.canonicalize()?;
        let repository = self.repository_path(project_id)?;
        let mut fetch = self.command();
        fetch
            .arg("--git-dir")
            .arg(&repository)
            .args(["fetch", "--quiet", "--no-tags", "--no-write-fetch-head"])
            .arg(source)
            .arg(commit_sha);
        run(fetch).await?;
        let mut check = self.command();
        check
            .arg("--git-dir")
            .arg(&repository)
            .args(["cat-file", "-t", commit_sha]);
        if run(check).await? != b"commit\n" {
            return Err(GitError::InvalidInput("Snapshots must reference commits"));
        }
        // Git locks the ref and checks that it does not exist atomically, even
        // across API processes. No operation here can replace a published ref.
        let mut publish = self.command();
        publish.arg("--git-dir").arg(repository).args([
            "update-ref",
            &format!("refs/snapshots/{snapshot_id}"),
            commit_sha,
            "0000000000000000000000000000000000000000",
        ]);
        run(publish).await?;
        Ok(())
    }

    fn command(&self) -> Command {
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "protocol.ext.allow=never",
            ])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true);
        command
    }

    #[cfg(test)]
    pub(crate) fn test_config() -> Self {
        Self {
            repository_root: PathBuf::from("/unused/projects"),
            repository_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn validate_project_id(project_id: i32) -> Result<(), GitError> {
    if project_id <= 0 {
        return Err(GitError::InvalidInput("Project ID must be positive"));
    }
    Ok(())
}

fn reject_symlinks(path: &Path) -> Result<(), GitError> {
    for ancestor in path.ancestors() {
        match ancestor.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(GitError::InvalidInput(
                    "Git storage must not contain symlinks",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn run(mut command: Command) -> Result<Vec<u8>, GitError> {
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| GitError::Timeout)??;
    if !output.status.success() {
        return Err(GitError::CommandFailed {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("fl-git-{}", uuid::Uuid::new_v4())))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn local_snapshots_are_retained_and_concurrent_publication_is_atomic() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        let repository = client.create_project_repository(7).await.unwrap();
        assert_eq!(
            client.create_project_repository(7).await.unwrap(),
            repository
        );
        assert!(client.list_snapshot_refs(7).await.unwrap().is_empty());
        assert!(matches!(
            client.create_project_repository(0).await,
            Err(GitError::InvalidInput(_))
        ));
        assert!(client.list_snapshot_refs(-1).await.is_err());
        assert!(matches!(
            client.list_snapshot_refs(8).await,
            Err(GitError::CommandFailed { .. })
        ));

        let source = fixture.0.join("source");
        let mut init = client.command();
        init.args(["init", "--quiet"]).arg(&source);
        run(init).await.unwrap();
        std::fs::write(source.join("train.py"), "print('snapshot')\n").unwrap();
        let mut add = client.command();
        add.arg("-C").arg(&source).args(["add", "train.py"]);
        run(add).await.unwrap();
        let mut commit = client.command();
        commit.arg("-C").arg(&source).args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "--quiet",
            "-m",
            "Snapshot",
        ]);
        run(commit).await.unwrap();
        let mut head = client.command();
        head.arg("-C").arg(&source).args(["rev-parse", "HEAD"]);
        let sha = String::from_utf8(run(head).await.unwrap()).unwrap();
        let sha = sha.trim();
        let (first, second) = tokio::join!(
            client.publish_snapshot(7, "v1", &source, sha),
            client.publish_snapshot(7, "v1", &source, sha)
        );
        assert_ne!(first.is_ok(), second.is_ok());
        let failure = first.err().or_else(|| second.err()).unwrap();
        assert!(matches!(failure, GitError::CommandFailed { .. }));
        client
            .publish_snapshot(7, "v2", &source, sha)
            .await
            .unwrap();
        let refs = client.list_snapshot_refs(7).await.unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs[0],
            GitRef {
                commit_sha: sha.into(),
                name: "refs/snapshots/v1".into()
            }
        );
        assert!(
            client
                .publish_snapshot(7, "../bad", &source, sha)
                .await
                .is_err()
        );
        assert!(
            client
                .publish_snapshot(7, "v3", &source, "HEAD")
                .await
                .is_err()
        );
        let mut tree = client.command();
        tree.arg("-C")
            .arg(&source)
            .args(["rev-parse", "HEAD^{tree}"]);
        let tree = String::from_utf8(run(tree).await.unwrap()).unwrap();
        assert!(
            client
                .publish_snapshot(7, "tree", &source, tree.trim())
                .await
                .is_err()
        );
        std::fs::remove_dir_all(&source).unwrap();
        let mut show = client.command();
        show.arg("--git-dir")
            .arg(repository)
            .args(["show", "refs/snapshots/v1:train.py"]);
        assert_eq!(run(show).await.unwrap(), b"print('snapshot')\n");
        assert_eq!(client.list_snapshot_refs(7).await.unwrap(), refs);
    }

    #[test]
    fn rejects_relative_storage() {
        assert!(GitClient::configured(PathBuf::from("relative/storage")).is_err());
    }

    #[tokio::test]
    async fn new_project_storage_is_exclusive_and_can_be_cleaned_up() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_new_project_repository(42).await.unwrap();
        let repository = client.repository_path(42).unwrap();
        assert!(repository.join("HEAD").is_file());
        assert!(client.list_snapshot_refs(42).await.unwrap().is_empty());
        std::fs::write(repository.join("sentinel"), b"keep").unwrap();
        assert!(client.create_new_project_repository(42).await.is_err());
        assert_eq!(std::fs::read(repository.join("sentinel")).unwrap(), b"keep");
        client.remove_project_repository(42).unwrap();
        assert!(!repository.exists());
        client.remove_project_repository(42).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_storage_and_repositories() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        symlink(client.repository_root(), fixture.0.join("link")).unwrap();
        assert!(GitClient::configured(fixture.0.join("link")).is_err());
        symlink(
            fixture.0.join("missing"),
            client.repository_root().join("7.git"),
        )
        .unwrap();
        assert!(client.create_project_repository(7).await.is_err());
        assert!(client.list_snapshot_refs(7).await.is_err());
    }
}
