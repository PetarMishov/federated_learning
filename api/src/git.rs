//! Private local Git storage for authenticated API handlers.
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command, sync::Mutex};

/// Errors from repository validation, filesystem access, and Git execution.
#[derive(Debug)]
pub enum GitError {
    InvalidInput(&'static str),
    InvalidData(&'static str),
    Io(io::Error),
    Timeout,
    NotFound,
    UnsupportedFile,
    FileTooLarge {
        size_bytes: usize,
    },
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
            Self::NotFound => f.write_str("Snapshot path not found"),
            Self::UnsupportedFile => f.write_str("Only regular UTF-8 text files can be viewed"),
            Self::FileTooLarge { .. } => f.write_str("File exceeds the editor preview size limit"),
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

/// Maximum size of a complete text preview; does not limit snapshot storage.
pub const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GitEntryKind {
    Directory,
    File,
    Symlink,
    Submodule,
}

#[derive(Debug, serde::Serialize)]
pub struct GitTreeEntry {
    pub name: String,
    pub path: String,
    pub kind: GitEntryKind,
}

#[derive(Debug, serde::Serialize)]
pub struct GitTree {
    pub path: String,
    pub entries: Vec<GitTreeEntry>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitFile {
    pub path: String,
    pub content: String,
}

pub struct GitSnapshotFile {
    pub path: String,
    pub content: Vec<u8>,
    pub executable: bool,
}

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitSnapshotOperation {
    Delete { path: String },
    Move { path: String, to: String },
}

struct CaptureDirectory(PathBuf);

impl Drop for CaptureDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("Could not clean snapshot capture temporary files: {error}");
        }
    }
}

struct TreeObject {
    name: String,
    oid: String,
    kind: GitEntryKind,
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

    pub(crate) fn repository_path(&self, project_id: i32) -> Result<PathBuf, GitError> {
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

    /// Read one directory from an authorized snapshot's fixed commit.
    pub async fn snapshot_tree(
        &self,
        project_id: i32,
        sha: &str,
        path: &str,
    ) -> Result<GitTree, GitError> {
        validate_snapshot_path(path, true)?;
        let oid = self.directory_oid(project_id, sha, path).await?;
        let mut entries: Vec<_> = self
            .tree_objects(project_id, &oid)
            .await?
            .into_iter()
            .map(|entry| GitTreeEntry {
                path: if path.is_empty() {
                    entry.name.clone()
                } else {
                    let mut entry_path = path.to_owned();
                    entry_path.push('/');
                    entry_path.push_str(&entry.name);
                    entry_path
                },
                name: entry.name,
                kind: entry.kind,
            })
            .collect();
        entries.sort_by(|a, b| {
            (a.kind != GitEntryKind::Directory)
                .cmp(&(b.kind != GitEntryKind::Directory))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(GitTree {
            path: path.to_owned(),
            entries,
        })
    }

    /// Read regular text files only. Never follow symlinks or render binary data.
    pub async fn snapshot_file(
        &self,
        project_id: i32,
        sha: &str,
        path: &str,
    ) -> Result<GitFile, GitError> {
        validate_snapshot_path(path, false)?;
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let oid = self.directory_oid(project_id, sha, parent).await?;
        let entry = self
            .tree_objects(project_id, &oid)
            .await?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or(GitError::NotFound)?;
        if entry.kind != GitEntryKind::File {
            return Err(GitError::UnsupportedFile);
        }
        let size = self
            .object_command(project_id, &["cat-file", "-s", &entry.oid])
            .await?;
        let size = std::str::from_utf8(&size)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .ok_or(GitError::InvalidData("Invalid Git file size"))?;
        if size > MAX_PREVIEW_BYTES {
            return Err(GitError::FileTooLarge { size_bytes: size });
        }
        let content = self
            .object_command(project_id, &["cat-file", "blob", &entry.oid])
            .await?;
        if content.contains(&0) {
            return Err(GitError::UnsupportedFile);
        }
        let content = String::from_utf8(content).map_err(|_| GitError::UnsupportedFile)?;
        Ok(GitFile {
            path: path.to_owned(),
            content,
        })
    }

    async fn directory_oid(
        &self,
        project_id: i32,
        sha: &str,
        path: &str,
    ) -> Result<String, GitError> {
        if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(GitError::InvalidData("Invalid stored snapshot commit"));
        }
        let root = self
            .object_command(
                project_id,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{sha}^{{commit}}^{{tree}}"),
                ],
            )
            .await?;
        let mut oid = String::from_utf8(root)
            .map_err(|_| GitError::InvalidData("Invalid Git tree"))?
            .trim()
            .to_owned();
        if !path.is_empty() {
            for component in path.split('/') {
                let entry = self
                    .tree_objects(project_id, &oid)
                    .await?
                    .into_iter()
                    .find(|entry| entry.name == component && entry.kind == GitEntryKind::Directory)
                    .ok_or(GitError::NotFound)?;
                oid = entry.oid;
            }
        }
        Ok(oid)
    }

    async fn tree_objects(&self, project_id: i32, oid: &str) -> Result<Vec<TreeObject>, GitError> {
        // NUL records preserve spaces and tabs in names. Resolve components by
        // exact name rather than passing user paths as Git revision/pathspec syntax.
        let output = self
            .object_command(project_id, &["ls-tree", "-z", oid])
            .await?;
        output
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .map(|record| {
                let record = std::str::from_utf8(record)
                    .map_err(|_| GitError::InvalidData("Invalid Git filename"))?;
                let (metadata, name) = record
                    .split_once('\t')
                    .ok_or(GitError::InvalidData("Invalid Git tree output"))?;
                let mut fields = metadata.split_whitespace();
                let mode = fields
                    .next()
                    .ok_or(GitError::InvalidData("Invalid Git mode"))?;
                let _object_type = fields
                    .next()
                    .ok_or(GitError::InvalidData("Invalid Git object type"))?;
                let oid = fields
                    .next()
                    .ok_or(GitError::InvalidData("Invalid Git object ID"))?;
                let kind = match mode {
                    "040000" => GitEntryKind::Directory,
                    "100644" | "100755" => GitEntryKind::File,
                    "120000" => GitEntryKind::Symlink,
                    "160000" => GitEntryKind::Submodule,
                    _ => return Err(GitError::InvalidData("Unsupported Git entry mode")),
                };
                Ok(TreeObject {
                    name: name.to_owned(),
                    oid: oid.to_owned(),
                    kind,
                })
            })
            .collect()
    }

    async fn object_command(&self, project_id: i32, args: &[&str]) -> Result<Vec<u8>, GitError> {
        let mut command = self.command();
        command
            .arg("--git-dir")
            .arg(self.repository_path(project_id)?)
            .args(args);
        run(command).await
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
        validate_snapshot_reference(snapshot_id, commit_sha)?;
        reject_symlinks(source)?;
        self.import_commit(project_id, source, commit_sha).await?;
        self.retain_snapshot(project_id, snapshot_id, commit_sha)
            .await
    }

    pub(crate) async fn import_commit(
        &self,
        project_id: i32,
        source: &Path,
        commit_sha: &str,
    ) -> Result<(), GitError> {
        validate_object_sha(commit_sha)?;
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
        Ok(())
    }

    pub(crate) async fn import_tree_commit(
        &self,
        project_id: i32,
        sha: &str,
    ) -> Result<String, GitError> {
        let tree = self.directory_oid(project_id, sha, "").await?;
        let mut command = self.command();
        command
            .arg("--git-dir")
            .arg(self.repository_path(project_id)?)
            .args(["commit-tree", &tree])
            .env("GIT_AUTHOR_NAME", "Snapshot storage")
            .env("GIT_AUTHOR_EMAIL", "snapshots@localhost")
            .env("GIT_COMMITTER_NAME", "Snapshot storage")
            .env("GIT_COMMITTER_EMAIL", "snapshots@localhost");
        parse_object_sha(run_with_input(command, b"Imported project draft\n").await?)
    }

    /// Validate an imported tree before exposing it as an editable draft.
    pub(crate) async fn inspect_import(
        &self,
        project_id: i32,
        commit: &str,
        max_bytes: u64,
        max_files: usize,
    ) -> Result<(u64, usize), GitError> {
        validate_object_sha(commit)?;
        let output = self
            .object_command(project_id, &["ls-tree", "-r", "-l", "-z", commit])
            .await?;
        let mut bytes = 0u64;
        let mut files = 0usize;
        for record in output
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
        {
            let record = std::str::from_utf8(record)
                .map_err(|_| GitError::InvalidData("Invalid imported filename"))?;
            let (metadata, path) = record
                .split_once('\t')
                .ok_or(GitError::InvalidData("Invalid imported tree"))?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(GitError::InvalidData("Invalid imported tree"));
            }
            if fields[0] == "160000" {
                return Err(GitError::InvalidInput(
                    "Repositories containing submodules cannot be loaded. Include the code in the selected repository.",
                ));
            }
            validate_import_path(path)?;
            let size = fields[3]
                .parse::<u64>()
                .map_err(|_| GitError::InvalidData("Invalid imported object size"))?;
            bytes = bytes.checked_add(size).ok_or(GitError::InvalidInput(
                "Project exceeds the import size limit.",
            ))?;
            files += 1;
            if bytes > max_bytes {
                return Err(GitError::InvalidInput(
                    "Project exceeds the import size limit.",
                ));
            }
            if files > max_files {
                return Err(GitError::InvalidInput(
                    "Project exceeds the import file limit.",
                ));
            }
        }
        Ok((bytes, files))
    }

    /// Retain a commit already captured in this project's repository.
    pub async fn retain_snapshot(
        &self,
        project_id: i32,
        snapshot_id: &str,
        commit_sha: &str,
    ) -> Result<(), GitError> {
        validate_snapshot_reference(snapshot_id, commit_sha)?;
        let repository = self.repository_path(project_id)?;
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

    /// Capture raw uploaded bytes without checking out paths or invoking filters.
    /// Callers must authorize project editing before writing Git objects.
    pub async fn capture_snapshot(
        &self,
        project_id: i32,
        files: Vec<GitSnapshotFile>,
    ) -> Result<String, GitError> {
        self.capture_snapshot_changes(project_id, None, files).await
    }

    /// Apply replacements to an immutable base tree, preserving untouched objects and modes.
    pub async fn capture_snapshot_changes(
        &self,
        project_id: i32,
        base_commit: Option<&str>,
        files: Vec<GitSnapshotFile>,
    ) -> Result<String, GitError> {
        self.capture_snapshot_patch(project_id, base_commit, files, vec![])
            .await
    }

    pub async fn capture_snapshot_patch(
        &self,
        project_id: i32,
        base_commit: Option<&str>,
        mut files: Vec<GitSnapshotFile>,
        operations: Vec<GitSnapshotOperation>,
    ) -> Result<String, GitError> {
        validate_upload_paths(&files)?;
        if base_commit.is_none() && !operations.is_empty() {
            return Err(GitError::InvalidInput(
                "Tree operations require a base snapshot",
            ));
        }
        let mut base = std::collections::BTreeMap::new();
        if let Some(sha) = base_commit {
            let tree = self.directory_oid(project_id, sha, "").await?;
            let output = self
                .object_command(project_id, &["ls-tree", "-r", "-z", &tree])
                .await?;
            for record in output
                .split(|byte| *byte == 0)
                .filter(|record| !record.is_empty())
            {
                let record = std::str::from_utf8(record)
                    .map_err(|_| GitError::InvalidData("Invalid Git filename"))?;
                let (metadata, path) = record
                    .split_once('\t')
                    .ok_or(GitError::InvalidData("Invalid Git tree"))?;
                let fields = metadata.split_whitespace().collect::<Vec<_>>();
                if fields.len() != 3 {
                    return Err(GitError::InvalidData("Invalid Git tree"));
                }
                base.insert(
                    path.to_owned(),
                    (fields[0].to_owned(), fields[2].to_owned()),
                );
            }
        }
        for operation in operations {
            let (path, destination) = match operation {
                GitSnapshotOperation::Delete { path } => (path, None),
                GitSnapshotOperation::Move { path, to } => (path, Some(to)),
            };
            validate_upload_paths(&[GitSnapshotFile {
                path: path.clone(),
                content: vec![],
                executable: false,
            }])?;
            let affected = base
                .keys()
                .filter(|key| path_contains(&path, key))
                .cloned()
                .collect::<Vec<_>>();
            if affected.is_empty() {
                return Err(GitError::InvalidInput(
                    "Tree operation source does not exist",
                ));
            }
            if let Some(to) = destination {
                validate_upload_paths(&[GitSnapshotFile {
                    path: to.clone(),
                    content: vec![],
                    executable: false,
                }])?;
                if path_contains(&path, &to) {
                    return Err(GitError::InvalidInput("Cannot move a path into itself"));
                }
                if base
                    .keys()
                    .any(|key| path_contains(&to, key) || path_contains(key, &to))
                {
                    return Err(GitError::InvalidInput(
                        "Move destination already exists or has a non-directory parent",
                    ));
                }
                for source in affected {
                    let target = format!("{to}{}", &source[path.len()..]);
                    validate_upload_paths(&[GitSnapshotFile {
                        path: target.clone(),
                        content: vec![],
                        executable: false,
                    }])?;
                    let object = base.remove(&source).unwrap();
                    base.insert(target, object);
                }
            } else {
                for source in affected {
                    base.remove(&source);
                }
            }
        }
        for file in &mut files {
            if base.keys().any(|path| {
                path != &file.path
                    && (path_contains(&file.path, path) || path_contains(path, &file.path))
            }) {
                return Err(GitError::InvalidInput(
                    "A file conflicts with an existing path",
                ));
            }
            if let Some((mode, _)) = base.get(&file.path) {
                if mode != "100644" && mode != "100755" {
                    return Err(GitError::InvalidInput("Only regular files can be edited"));
                }
                file.executable = mode == "100755";
            }
        }
        if self
            .object_command(project_id, &["rev-parse", "--is-bare-repository"])
            .await?
            != b"true\n"
        {
            return Err(GitError::InvalidInput(
                "Project storage must be a bare Git repository",
            ));
        }
        let lock = self
            .repository_locks
            .lock()
            .await
            .entry(project_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;
        let directory = self
            .repository_root
            .parent()
            .ok_or(GitError::InvalidData("Invalid Git storage root"))?
            .join(format!(".capture-{}", uuid::Uuid::new_v4()));
        let (capture, names) = tokio::task::spawn_blocking(move || {
            reject_symlinks(&directory)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
            }
            #[cfg(not(unix))]
            std::fs::create_dir(&directory)?;
            let capture = CaptureDirectory(directory);
            let mut names = Vec::with_capacity(files.len());
            for (index, file) in files.into_iter().enumerate() {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut output = options.open(capture.0.join(index.to_string()))?;
                std::io::Write::write_all(&mut output, &file.content)?;
                names.push((file.path, file.executable));
            }
            Ok::<_, GitError>((capture, names))
        })
        .await
        .map_err(|_| GitError::InvalidData("Snapshot capture task failed"))??;
        self.commit_staged_files(project_id, base, &capture.0, &names)
            .await
    }

    /// Hash numbered private staging files without loading their contents in memory.
    pub(crate) async fn capture_staged_files(
        &self,
        project_id: i32,
        directory: &Path,
        names: &[(String, bool)],
    ) -> Result<String, GitError> {
        let metadata = names
            .iter()
            .map(|(path, executable)| GitSnapshotFile {
                path: path.clone(),
                content: vec![],
                executable: *executable,
            })
            .collect::<Vec<_>>();
        validate_upload_paths(&metadata)?;
        self.commit_staged_files(
            project_id,
            std::collections::BTreeMap::new(),
            directory,
            names,
        )
        .await
    }

    async fn commit_staged_files(
        &self,
        project_id: i32,
        base: std::collections::BTreeMap<String, (String, String)>,
        directory: &Path,
        names: &[(String, bool)],
    ) -> Result<String, GitError> {
        let repository = self.repository_path(project_id)?;
        let mut index_info = Vec::new();
        for (path, (mode, oid)) in base {
            index_info.extend_from_slice(format!("{mode} {oid}\t{path}\0").as_bytes());
        }
        // Numbered private temporary files avoid interpreting uploaded names as
        // host paths. --no-filters preserves even .gitattributes-controlled bytes.
        for (batch, chunk) in names.chunks(128).enumerate() {
            let mut hash = self.command();
            hash.arg("--git-dir").arg(&repository).args([
                "hash-object",
                "-w",
                "--no-filters",
                "--",
            ]);
            for index in 0..chunk.len() {
                hash.arg(directory.join((batch * 128 + index).to_string()));
            }
            let hashes = run(hash).await?;
            let hashes = std::str::from_utf8(&hashes)
                .map_err(|_| GitError::InvalidData("Invalid uploaded blob hashes"))?
                .lines()
                .collect::<Vec<_>>();
            if hashes.len() != chunk.len() {
                return Err(GitError::InvalidData("Missing uploaded blob hashes"));
            }
            for ((path, executable), sha) in chunk.iter().zip(hashes) {
                validate_object_sha(sha)?;
                index_info.extend_from_slice(if *executable { b"100755 " } else { b"100644 " });
                index_info.extend_from_slice(sha.as_bytes());
                index_info.push(b'\t');
                index_info.extend_from_slice(path.as_bytes());
                index_info.push(0);
            }
        }
        let index = directory.join("index");
        let mut empty = self.command();
        empty
            .arg("--git-dir")
            .arg(&repository)
            .arg("read-tree")
            .arg("--empty")
            .env("GIT_INDEX_FILE", &index);
        run(empty).await?;
        let mut update = self.command();
        update
            .arg("--git-dir")
            .arg(&repository)
            .args(["update-index", "-z", "--index-info"])
            .env("GIT_INDEX_FILE", &index);
        run_with_input(update, &index_info).await?;
        let mut tree = self.command();
        tree.arg("--git-dir")
            .arg(&repository)
            .arg("write-tree")
            .env("GIT_INDEX_FILE", &index);
        let tree = parse_object_sha(run(tree).await?)?;
        let mut commit = self.command();
        commit
            .arg("--git-dir")
            .arg(repository)
            .args(["commit-tree", &tree])
            .env("GIT_AUTHOR_NAME", "Snapshot storage")
            .env("GIT_AUTHOR_EMAIL", "snapshots@localhost")
            .env("GIT_COMMITTER_NAME", "Snapshot storage")
            .env("GIT_COMMITTER_EMAIL", "snapshots@localhost");
        parse_object_sha(run_with_input(commit, b"Saved snapshot\n").await?)
    }

    pub(crate) fn command(&self) -> Command {
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

fn validate_object_sha(sha: &str) -> Result<(), GitError> {
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GitError::InvalidData("Invalid Git object hash"));
    }
    Ok(())
}

fn parse_object_sha(output: Vec<u8>) -> Result<String, GitError> {
    let sha = std::str::from_utf8(&output)
        .map_err(|_| GitError::InvalidData("Invalid Git object hash"))?
        .trim();
    validate_object_sha(sha)?;
    Ok(sha.to_owned())
}

fn validate_snapshot_reference(snapshot_id: &str, commit_sha: &str) -> Result<(), GitError> {
    let valid_id = snapshot_id
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && snapshot_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !valid_id || validate_object_sha(commit_sha).is_err() {
        return Err(GitError::InvalidInput("Invalid snapshot ID or commit SHA"));
    }
    Ok(())
}

pub(crate) fn validate_import_path(path: &str) -> Result<(), GitError> {
    validate_upload_paths(&[GitSnapshotFile {
        path: path.into(),
        content: vec![],
        executable: false,
    }])
}

fn validate_upload_paths(files: &[GitSnapshotFile]) -> Result<(), GitError> {
    let mut paths = HashSet::with_capacity(files.len());
    for file in files {
        validate_snapshot_path(&file.path, false)?;
        if file.path.len() > 4096
            || file.path.chars().any(char::is_control)
            || file.path.split('/').any(|part| {
                part.len() > 255
                    || part.contains(':')
                    || part
                        .trim_end_matches([' ', '.'])
                        .eq_ignore_ascii_case(".git")
            })
        {
            return Err(GitError::InvalidInput(
                "File paths must exclude Git metadata, control characters, and oversized names.",
            ));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(GitError::InvalidInput("File paths must be unique."));
        }
    }
    for file in files {
        for (index, _) in file.path.match_indices('/') {
            if paths.contains(&file.path[..index]) {
                return Err(GitError::InvalidInput(
                    "A path cannot be both a file and a directory.",
                ));
            }
        }
    }
    Ok(())
}

fn validate_snapshot_path(path: &str, allow_root: bool) -> Result<(), GitError> {
    if path.is_empty() && allow_root {
        return Ok(());
    }
    if path.is_empty()
        || path.contains('\0')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(GitError::InvalidInput(
            "Path must be relative to the snapshot root",
        ));
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

async fn run_with_input(mut command: Command, input: &[u8]) -> Result<Vec<u8>, GitError> {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = tokio::time::timeout(Duration::from_secs(30), async {
        let mut child = command.spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Git stdin unavailable"))?;
        let write = async {
            stdin.write_all(input).await?;
            drop(stdin);
            Ok::<_, io::Error>(())
        };
        let (_, output) = tokio::try_join!(write, child.wait_with_output())?;
        Ok::<_, io::Error>(output)
    })
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

fn path_contains(parent: &str, path: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn staged_import_supports_100_mib_and_enforces_content_and_file_limits() {
        use std::io::Write;
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let staging = fixture.0.join("upload");
        std::fs::create_dir(&staging).unwrap();
        let mut file = std::fs::File::create(staging.join("0")).unwrap();
        let mut state = 0x123456789abcdef_u64;
        let chunk = (0..1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect::<Vec<_>>();
        for _ in 0..100 {
            file.write_all(&chunk).unwrap();
        }
        drop(file);
        let started = std::time::Instant::now();
        let commit = client
            .capture_staged_files(42, &staging, &[("data.bin".into(), false)])
            .await
            .unwrap();
        assert_eq!(
            client
                .inspect_import(42, &commit, 100 * 1024 * 1024, 1)
                .await
                .unwrap(),
            (100 * 1024 * 1024, 1)
        );
        assert!(matches!(
            client
                .inspect_import(42, &commit, 100 * 1024 * 1024 - 1, 1)
                .await,
            Err(GitError::InvalidInput(_))
        ));
        let saved = client
            .capture_snapshot_patch(42, Some(&commit), vec![], vec![])
            .await
            .unwrap();
        assert_eq!(
            client.directory_oid(42, &commit, "").await.unwrap(),
            client.directory_oid(42, &saved, "").await.unwrap()
        );
        assert!(matches!(
            client.snapshot_file(42, &saved, "data.bin").await,
            Err(GitError::FileTooLarge { .. })
        ));
        eprintln!(
            "100 MiB staging, validation and unedited save: {:?}",
            started.elapsed()
        );
        std::fs::write(staging.join("1"), b"small").unwrap();
        let commit = client
            .capture_staged_files(
                42,
                &staging,
                &[("data.bin".into(), false), ("small".into(), true)],
            )
            .await
            .unwrap();
        assert!(matches!(
            client.inspect_import(42, &commit, u64::MAX, 1).await,
            Err(GitError::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn provider_import_keeps_exact_blobs_and_links_rejects_submodules_and_detaches_history() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let source = client
            .capture_snapshot(
                42,
                vec![GitSnapshotFile {
                    path: "keep.txt".into(),
                    content: b"$Format:%H$".to_vec(),
                    executable: true,
                }],
            )
            .await
            .unwrap();
        let repository = client.repository_path(42).unwrap();
        let blob = client
            .object_command(42, &["rev-parse", &format!("{source}:keep.txt")])
            .await
            .unwrap();
        let blob = String::from_utf8(blob).unwrap();
        let mut command = client.command();
        command.arg("--git-dir").arg(&repository).arg("mktree");
        let tree = parse_object_sha(
            run_with_input(
                command,
                format!(
                    "100755 blob {}\tkeep.txt\n120000 blob {}\tlink\n",
                    blob.trim(),
                    blob.trim()
                )
                .as_bytes(),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let mut command = client.command();
        command
            .arg("--git-dir")
            .arg(&repository)
            .args(["commit-tree", &tree, "-p", &source])
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@localhost")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@localhost");
        let revision =
            parse_object_sha(run_with_input(command, b"provider\n").await.unwrap()).unwrap();
        assert_eq!(
            client
                .inspect_import(42, &revision, 1024, 10)
                .await
                .unwrap(),
            (22, 2)
        );
        let imported = client.import_tree_commit(42, &revision).await.unwrap();
        assert_eq!(
            client
                .object_command(42, &["rev-list", "--count", &imported])
                .await
                .unwrap(),
            b"1\n"
        );
        assert_eq!(
            client.directory_oid(42, &revision, "").await.unwrap(),
            client.directory_oid(42, &imported, "").await.unwrap()
        );
        let mut command = client.command();
        command.arg("--git-dir").arg(&repository).arg("mktree");
        let tree = parse_object_sha(
            run_with_input(
                command,
                format!("160000 commit {source}\tsubmodule\n").as_bytes(),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert!(
            matches!(client.inspect_import(42, &tree, 1024, 10).await, Err(GitError::InvalidInput(message)) if message.contains("submodules"))
        );
    }

    #[tokio::test]
    async fn tree_operations_move_binary_subtrees_preserve_modes_and_keep_old_snapshots() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let file = |path: &str, content: &[u8], executable| GitSnapshotFile {
            path: path.into(),
            content: content.to_vec(),
            executable,
        };
        let first = client
            .capture_snapshot(
                42,
                vec![
                    file("src/run.sh", b"old", true),
                    file("src/data.bin", &[0, 255], false),
                    file("remove.txt", b"gone", false),
                    file("keep.txt", b"keep", false),
                ],
            )
            .await
            .unwrap();
        let edited = client
            .capture_snapshot_patch(
                42,
                Some(&first),
                vec![file("lib/run.sh", b"edited", false)],
                vec![
                    GitSnapshotOperation::Move {
                        path: "src".into(),
                        to: "lib".into(),
                    },
                    GitSnapshotOperation::Delete {
                        path: "remove.txt".into(),
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .snapshot_file(42, &edited, "lib/run.sh")
                .await
                .unwrap()
                .content,
            "edited"
        );
        assert_eq!(
            client
                .object_command(42, &["show", &format!("{edited}:lib/data.bin")])
                .await
                .unwrap(),
            [0, 255]
        );
        assert!(
            client
                .object_command(42, &["ls-tree", &edited, "lib/run.sh"])
                .await
                .unwrap()
                .starts_with(b"100755 blob ")
        );
        assert!(matches!(
            client.snapshot_file(42, &edited, "remove.txt").await,
            Err(GitError::NotFound)
        ));
        assert_eq!(
            client
                .snapshot_file(42, &first, "src/run.sh")
                .await
                .unwrap()
                .content,
            "old"
        );
        assert_eq!(
            client
                .snapshot_file(42, &first, "remove.txt")
                .await
                .unwrap()
                .content,
            "gone"
        );
        for (path, to) in [
            ("src", "src/inside"),
            ("src", "keep.txt"),
            ("keep.txt", "src"),
            ("src", "../outside"),
            ("missing", "new"),
        ] {
            assert!(matches!(
                client
                    .capture_snapshot_patch(
                        42,
                        Some(&first),
                        vec![],
                        vec![GitSnapshotOperation::Move {
                            path: path.into(),
                            to: to.into()
                        }]
                    )
                    .await,
                Err(GitError::InvalidInput(_))
            ));
        }
        let emptied = client
            .capture_snapshot_patch(
                42,
                Some(&first),
                vec![],
                vec![GitSnapshotOperation::Delete { path: "src".into() }],
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .snapshot_tree(42, &emptied, "")
                .await
                .unwrap()
                .entries
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn snapshot_additions_preserve_base_files_and_reject_path_collisions() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let file = |path: &str, content: &[u8]| GitSnapshotFile {
            path: path.into(),
            content: content.to_vec(),
            executable: false,
        };
        let first = client
            .capture_snapshot(
                42,
                vec![file("keep.txt", b"original"), file("src/main.py", b"code")],
            )
            .await
            .unwrap();
        let added = client
            .capture_snapshot_changes(
                42,
                Some(&first),
                vec![
                    file("models/nested/new.py", b"new"),
                    file("src/empty.txt", b""),
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .snapshot_file(42, &added, "keep.txt")
                .await
                .unwrap()
                .content,
            "original"
        );
        assert_eq!(
            client
                .snapshot_file(42, &added, "models/nested/new.py")
                .await
                .unwrap()
                .content,
            "new"
        );
        assert_eq!(
            client
                .snapshot_file(42, &added, "src/empty.txt")
                .await
                .unwrap()
                .content,
            ""
        );
        assert!(matches!(
            client
                .snapshot_file(42, &first, "models/nested/new.py")
                .await,
            Err(GitError::NotFound)
        ));
        for path in ["src", "keep.txt/child.txt"] {
            assert!(matches!(
                client
                    .capture_snapshot_changes(42, Some(&first), vec![file(path, b"bad")])
                    .await,
                Err(GitError::InvalidInput(_))
            ));
        }
    }

    #[tokio::test]
    async fn editing_a_snapshot_preserves_unopened_objects_and_executable_modes() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let first = client
            .capture_snapshot(
                42,
                vec![
                    GitSnapshotFile {
                        path: "run.sh".into(),
                        content: b"old".to_vec(),
                        executable: true,
                    },
                    GitSnapshotFile {
                        path: "binary.bin".into(),
                        content: vec![0, 255, 1],
                        executable: false,
                    },
                ],
            )
            .await
            .unwrap();
        let edited = client
            .capture_snapshot_changes(
                42,
                Some(&first),
                vec![GitSnapshotFile {
                    path: "run.sh".into(),
                    content: b"new".to_vec(),
                    executable: false,
                }],
            )
            .await
            .unwrap();
        assert_eq!(
            client
                .snapshot_file(42, &edited, "run.sh")
                .await
                .unwrap()
                .content,
            "new"
        );
        assert_eq!(
            client
                .snapshot_file(42, &first, "run.sh")
                .await
                .unwrap()
                .content,
            "old"
        );
        assert_eq!(
            client
                .object_command(42, &["show", &format!("{edited}:binary.bin")])
                .await
                .unwrap(),
            [0, 255, 1]
        );
        assert!(
            client
                .object_command(42, &["ls-tree", &edited, "run.sh"])
                .await
                .unwrap()
                .starts_with(b"100755 blob ")
        );
        assert!(
            client
                .capture_snapshot_changes(
                    42,
                    Some(&first),
                    vec![GitSnapshotFile {
                        path: "missing".into(),
                        content: vec![],
                        executable: false
                    },]
                )
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn uploaded_snapshots_preserve_raw_bytes_modes_and_complete_trees() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let file = |path: &str, content: &[u8], executable| GitSnapshotFile {
            path: path.into(),
            content: content.to_vec(),
            executable,
        };
        let first = client
            .capture_snapshot(
                42,
                vec![
                    file(
                        ".gitattributes",
                        b"*.py text eol=lf\n*.bin filter=anything\n",
                        false,
                    ),
                    file(".gitignore", b"ignored.py\n", false),
                    file("ignored.py", b"print('kept')\r\n", false),
                    file("src/space name.py", b"print('nested')\n", false),
                    file("src/run.sh", b"#!/bin/sh\necho ok\n", true),
                    file("binary.bin", &[0, 255, 1], false),
                    file("empty.txt", b"", false),
                ],
            )
            .await
            .unwrap();
        // Capture does not publish until the database has allocated a snapshot ID.
        assert!(client.list_snapshot_refs(42).await.unwrap().is_empty());
        client.retain_snapshot(42, "1", &first).await.unwrap();
        assert_eq!(
            client
                .snapshot_file(42, &first, "ignored.py")
                .await
                .unwrap()
                .content,
            "print('kept')\r\n"
        );
        assert_eq!(
            client
                .snapshot_file(42, &first, "src/space name.py")
                .await
                .unwrap()
                .content,
            "print('nested')\n"
        );
        assert_eq!(
            client
                .object_command(42, &["show", &format!("{first}:binary.bin")])
                .await
                .unwrap(),
            [0, 255, 1]
        );
        let mode = client
            .object_command(42, &["ls-tree", &first, "src/run.sh"])
            .await
            .unwrap();
        assert!(mode.starts_with(b"100755 blob "));
        let second = client
            .capture_snapshot(42, vec![file("new.py", b"new version\n", false)])
            .await
            .unwrap();
        client.retain_snapshot(42, "2", &second).await.unwrap();
        assert_eq!(
            client
                .snapshot_tree(42, &second, "")
                .await
                .unwrap()
                .entries
                .len(),
            1
        );
        assert!(matches!(
            client.snapshot_file(42, &second, "ignored.py").await,
            Err(GitError::NotFound)
        ));
        assert!(client.retain_snapshot(42, "1", &second).await.is_err());
        assert_eq!(
            client
                .snapshot_file(42, &first, "ignored.py")
                .await
                .unwrap()
                .content,
            "print('kept')\r\n"
        );
        let empty = client.capture_snapshot(42, vec![]).await.unwrap();
        client.retain_snapshot(42, "3", &empty).await.unwrap();
        assert!(
            client
                .snapshot_tree(42, &empty, "")
                .await
                .unwrap()
                .entries
                .is_empty()
        );
        assert!(
            std::fs::read_dir(fixture.0.join("storage/git"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".capture-"))
        );
    }

    #[tokio::test]
    async fn uploaded_snapshots_reject_invalid_paths_before_writing_objects() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let files = |paths: &[&str]| {
            paths
                .iter()
                .map(|path| GitSnapshotFile {
                    path: (*path).into(),
                    content: vec![],
                    executable: false,
                })
                .collect()
        };
        for path in [
            "",
            "/etc/passwd",
            "../file",
            "a/../file",
            "a//file",
            "a\\file",
            "a\0file",
            ".git/config",
            "a/.Git/config",
            ".git./config",
            "C:/file",
            "a\nfile",
        ] {
            assert!(
                matches!(
                    client.capture_snapshot(42, files(&[path])).await,
                    Err(GitError::InvalidInput(_))
                ),
                "{path:?}"
            );
        }
        for paths in [
            vec!["a.py", "a.py"],
            vec!["a", "a.b", "a/file"],
            vec!["a/file", "a"],
        ] {
            assert!(matches!(
                client.capture_snapshot(42, files(&paths)).await,
                Err(GitError::InvalidInput(_))
            ));
        }
        assert!(client.list_snapshot_refs(42).await.unwrap().is_empty());
        assert!(
            std::fs::read_dir(fixture.0.join("storage/git"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".capture-"))
        );
    }

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

    #[tokio::test]
    async fn snapshot_browsing_reads_exact_commits_and_rejects_unsafe_entries() {
        let fixture = Fixture::new();
        let client = GitClient::configured(fixture.0.join("storage/git")).unwrap();
        client.create_project_repository(42).await.unwrap();
        let source = fixture.0.join("source");
        let mut init = client.command();
        init.args(["init", "--quiet"]).arg(&source);
        run(init).await.unwrap();
        std::fs::create_dir(source.join("src")).unwrap();
        std::fs::write(source.join("src/space name.txt"), "old contents\n").unwrap();
        std::fs::write(source.join("literal[1]*.txt"), "literal").unwrap();
        std::fs::write(source.join("tab\tname.txt"), "tab").unwrap();
        std::fs::write(source.join("binary.bin"), [0, 1, 2]).unwrap();
        std::fs::write(source.join("invalid.bin"), [255]).unwrap();
        std::fs::write(source.join("empty.txt"), "").unwrap();
        std::fs::write(source.join("boundary.py"), vec![b'x'; MAX_PREVIEW_BYTES]).unwrap();
        std::fs::write(source.join("large.txt"), vec![b'x'; MAX_PREVIEW_BYTES + 1]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/passwd", source.join("link")).unwrap();
        async fn commit(client: &GitClient, source: &Path) -> String {
            let mut add = client.command();
            add.arg("-C").arg(source).args(["add", "--all"]);
            run(add).await.unwrap();
            let mut commit = client.command();
            commit.arg("-C").arg(source).args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "--quiet",
                "-m",
                "Files",
            ]);
            run(commit).await.unwrap();
            let mut head = client.command();
            head.arg("-C").arg(source).args(["rev-parse", "HEAD"]);
            String::from_utf8(run(head).await.unwrap())
                .unwrap()
                .trim()
                .to_owned()
        }
        let first = commit(&client, &source).await;
        client
            .publish_snapshot(42, "1", &source, &first)
            .await
            .unwrap();
        std::fs::write(source.join("src/space name.txt"), "new contents\n").unwrap();
        let second = commit(&client, &source).await;
        client
            .publish_snapshot(42, "2", &source, &second)
            .await
            .unwrap();
        std::fs::remove_dir_all(source).unwrap();
        let root = client.snapshot_tree(42, &first, "").await.unwrap();
        assert_eq!(root.entries[0].path, "src");
        assert_eq!(root.entries[0].kind, GitEntryKind::Directory);
        let nested = client.snapshot_tree(42, &first, "src").await.unwrap();
        assert_eq!(nested.entries[0].path, "src/space name.txt");
        assert_eq!(
            client
                .snapshot_file(42, &first, "src/space name.txt")
                .await
                .unwrap()
                .content,
            "old contents\n"
        );
        assert_eq!(
            client
                .snapshot_file(42, &second, "src/space name.txt")
                .await
                .unwrap()
                .content,
            "new contents\n"
        );
        for (path, expected) in [
            ("literal[1]*.txt", "literal"),
            ("tab\tname.txt", "tab"),
            ("empty.txt", ""),
        ] {
            assert_eq!(
                client
                    .snapshot_file(42, &first, path)
                    .await
                    .unwrap()
                    .content,
                expected
            );
        }
        for path in ["binary.bin", "invalid.bin", "src"] {
            assert!(matches!(
                client.snapshot_file(42, &first, path).await,
                Err(GitError::UnsupportedFile)
            ));
        }
        let boundary = client
            .snapshot_file(42, &first, "boundary.py")
            .await
            .unwrap();
        assert_eq!(boundary.content.len(), MAX_PREVIEW_BYTES);
        assert!(boundary.content.bytes().all(|byte| byte == b'x'));
        assert!(matches!(
            client.snapshot_file(42, &first, "large.txt").await,
            Err(GitError::FileTooLarge { size_bytes }) if size_bytes == MAX_PREVIEW_BYTES + 1
        ));
        for path in [
            "../secret",
            "/etc/passwd",
            "src/../secret",
            "src//file",
            ".",
            "src\\file",
            "nul\0",
        ] {
            assert!(matches!(
                client.snapshot_tree(42, &first, path).await,
                Err(GitError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            client.snapshot_tree(42, &first, "missing").await,
            Err(GitError::NotFound)
        ));
        assert!(matches!(
            client.snapshot_file(42, &first, "missing").await,
            Err(GitError::NotFound)
        ));
        #[cfg(unix)]
        {
            assert!(
                root.entries
                    .iter()
                    .any(|entry| entry.path == "link" && entry.kind == GitEntryKind::Symlink)
            );
            assert!(matches!(
                client.snapshot_file(42, &first, "link").await,
                Err(GitError::UnsupportedFile)
            ));
            assert!(matches!(
                client.snapshot_tree(42, &first, "link").await,
                Err(GitError::NotFound)
            ));
        }
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
