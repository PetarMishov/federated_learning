use crate::git::{GitClient, GitError};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, sync::Mutex as AsyncMutex};
use uuid::Uuid;

#[derive(Clone, Copy, Serialize)]
pub struct ImportLimits {
    pub max_bytes: u64,
    pub max_files: usize,
}

impl ImportLimits {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        fn value(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
            let value = match std::env::var(name) {
                Ok(value) => value.parse()?,
                Err(std::env::VarError::NotPresent) => default,
                Err(error) => return Err(error.into()),
            };
            if value == 0 {
                return Err(format!("{name} must be positive").into());
            }
            Ok(value)
        }
        Ok(Self {
            max_bytes: value("PROJECT_IMPORT_MAX_BYTES", 100 * 1024 * 1024)?,
            max_files: usize::try_from(value("PROJECT_IMPORT_MAX_FILES", 10_000)?)?,
        })
    }
}

#[derive(Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImportPhase {
    Uploading,
    Downloading,
    Preparing,
    Ready,
    Failed,
    Cancelled,
}

#[derive(Clone, Serialize)]
pub struct ImportStatus {
    pub id: Uuid,
    pub phase: ImportPhase,
    pub progress_percent: Option<u8>,
    pub bytes: u64,
    pub files: usize,
    pub error: Option<&'static str>,
}

pub struct ImportJob {
    pub project_id: i32,
    pub user_id: i32,
    pub source: String,
    pub source_branch: Option<String>,
    pub source_commit_sha: Option<String>,
    pub git: GitClient,
    pub directory: PathBuf,
    _storage: Arc<ImportStorage>,
    pub cancelled: AtomicBool,
    pub busy: AsyncMutex<()>,
    pub commit: Mutex<Option<String>>,
    status: Mutex<ImportStatus>,
    last_used: Mutex<Instant>,
}

impl ImportJob {
    pub fn status(&self) -> ImportStatus {
        self.status.lock().unwrap().clone()
    }
    pub fn update(&self, phase: ImportPhase, progress: Option<u8>, bytes: u64, files: usize) {
        let mut status = self.status.lock().unwrap();
        if self.cancelled.load(Ordering::Relaxed) {
            status.phase = ImportPhase::Cancelled;
            return;
        }
        status.phase = phase;
        status.progress_percent = progress;
        status.bytes = bytes;
        status.files = files;
    }
    pub fn fail(&self, error: &'static str) {
        let mut status = self.status.lock().unwrap();
        status.phase = ImportPhase::Failed;
        status.error = Some(error);
    }
    pub fn ready_commit(&self) -> Option<String> {
        if self.status().phase != ImportPhase::Ready || self.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        self.commit.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.status.lock().unwrap().phase = ImportPhase::Cancelled;
    }
}

impl Drop for ImportJob {
    fn drop(&mut self) {
        let directory = self.directory.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || {
                let _ = std::fs::remove_dir_all(directory);
            });
        } else {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

// The root remains alive until the manager and all background jobs are gone.
struct ImportStorage {
    directory: PathBuf,
    _lease: std::fs::File,
}
impl Drop for ImportStorage {
    fn drop(&mut self) {
        let directory = self.directory.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || {
                let _ = std::fs::remove_dir_all(directory);
            });
        } else {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

pub struct ImportManager {
    pub limits: ImportLimits,
    root: Arc<ImportStorage>,
    jobs: Mutex<HashMap<Uuid, Arc<ImportJob>>>,
}

impl ImportManager {
    pub fn new(git: &GitClient, limits: ImportLimits) -> Result<Arc<Self>, GitError> {
        let root = git
            .repository_root()
            .parent()
            .ok_or(GitError::InvalidData("Invalid storage root"))?
            .join(format!(".imports-{}", Uuid::new_v4()));
        cleanup_abandoned(root.parent().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&root)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir(&root)?;
        let lease = std::fs::File::create(root.join("lease"))?;
        lease.lock()?;
        let manager = Arc::new(Self {
            limits,
            root: Arc::new(ImportStorage {
                directory: root,
                _lease: lease,
            }),
            jobs: Mutex::new(HashMap::new()),
        });
        let weak = Arc::downgrade(&manager);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let Some(manager) = weak.upgrade() else {
                    break;
                };
                let parent = manager.root.directory.parent().unwrap().to_path_buf();
                tokio::task::spawn_blocking(move || cleanup_abandoned(&parent));
                manager.jobs.lock().unwrap().retain(|_, job| {
                    if job.last_used.lock().unwrap().elapsed() > Duration::from_secs(30 * 60) {
                        job.cancel();
                        false
                    } else {
                        true
                    }
                });
            }
        });
        Ok(manager)
    }

    pub async fn create(
        &self,
        project_id: i32,
        user_id: i32,
        source: String,
        branch: Option<String>,
        commit: Option<String>,
    ) -> Result<Arc<ImportJob>, &'static str> {
        let id = Uuid::new_v4();
        let directory = self.root.directory.join(id.to_string());
        let git = GitClient::configured(directory.clone())
            .map_err(|_| "Could not prepare import storage.")?;
        let phase = if source == "local" {
            ImportPhase::Uploading
        } else {
            ImportPhase::Downloading
        };
        let job = Arc::new(ImportJob {
            project_id,
            user_id,
            source,
            source_branch: branch,
            source_commit_sha: commit,
            git,
            directory,
            _storage: self.root.clone(),
            cancelled: AtomicBool::new(false),
            busy: AsyncMutex::new(()),
            commit: Mutex::new(None),
            status: Mutex::new(ImportStatus {
                id,
                phase,
                progress_percent: None,
                bytes: 0,
                files: 0,
                error: None,
            }),
            last_used: Mutex::new(Instant::now()),
        });
        // Bound concurrent work and temporary storage across users.
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.len() >= 8 || jobs.values().filter(|job| job.user_id == user_id).count() >= 2 {
                return Err("Too many active imports. Discard an existing draft and try again.");
            }
            jobs.insert(id, job.clone());
        }
        if job.git.create_project_repository(project_id).await.is_err() {
            self.remove(id, project_id, user_id);
            return Err("Could not prepare import storage.");
        }
        Ok(job)
    }

    pub fn get(&self, id: Uuid, project_id: i32, user_id: i32) -> Option<Arc<ImportJob>> {
        let job = self.jobs.lock().unwrap().get(&id)?.clone();
        if job.project_id != project_id
            || job.user_id != user_id
            || job.cancelled.load(Ordering::Relaxed)
        {
            return None;
        }
        *job.last_used.lock().unwrap() = Instant::now();
        Some(job)
    }
    pub fn remove(&self, id: Uuid, project_id: i32, user_id: i32) {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs
            .get(&id)
            .is_some_and(|job| job.project_id == project_id && job.user_id == user_id)
            && let Some(job) = jobs.remove(&id)
        {
            job.cancel();
        }
    }
}

impl Drop for ImportManager {
    fn drop(&mut self) {
        for job in self.jobs.get_mut().unwrap().values() {
            job.cancel();
        }
    }
}

// Leases distinguish abandoned staging from another live server instance.
fn cleanup_abandoned(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name
            .strip_prefix(".imports-")
            .and_then(|id| Uuid::parse_str(id).ok())
            .is_none()
            || !entry.file_type().is_ok_and(|kind| kind.is_dir())
        {
            continue;
        }
        // Give a newly-created root time to acquire its lease before inspecting it.
        if !entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|time| {
                time.elapsed()
                    .is_ok_and(|age| age > Duration::from_secs(60))
            })
        {
            continue;
        }
        let path = entry.path().join("lease");
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => {
                if let Ok(lease) = std::fs::File::options().read(true).write(true).open(path)
                    && lease.try_lock().is_ok()
                {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::remove_dir_all(entry.path());
            }
            _ => {}
        }
    }
}

fn disk_bytes(path: &Path) -> std::io::Result<u64> {
    let mut bytes = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            bytes = bytes.saturating_add(disk_bytes(&entry.path())?);
        } else {
            bytes = bytes.saturating_add(metadata.len());
        }
    }
    Ok(bytes)
}

/// Fetch one immutable revision, without checkout, history, hooks, or submodules.
pub async fn fetch_provider(
    job: Arc<ImportJob>,
    url: reqwest::Url,
    token: String,
    limits: ImportLimits,
) {
    let _guard = job.busy.lock().await;
    let result = async {
        let repository = job.git.repository_path(job.project_id).map_err(|_| "Could not prepare import storage.")?;
        let sha = job.source_commit_sha.as_deref().ok_or("Select a commit to load.")?;
        let authorization = STANDARD.encode(format!("{}:{token}", if job.source == "github" { "x-access-token" } else { "oauth2" }));
        let mut command = job.git.command();
        // Credentials stay in the process environment, never its URL/arguments or logs.
        command.arg("--git-dir").arg(&repository)
            .args(["-c", "http.followRedirects=false", "-c", "fetch.fsckObjects=true", "-c", "protocol.file.allow=never", "fetch", "--depth=1", "--no-tags", "--no-recurse-submodules", "--no-write-fetch-head", "--progress"])
            .arg(url.as_str()).arg(format!("{sha}:refs/import/source"))
            .env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env("GIT_CONFIG_VALUE_0", format!("Authorization: Basic {authorization}"))
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|_| "Could not start repository download.")?;
        let mut stderr = child.stderr.take().ok_or("Could not monitor repository download.")?;
        let mut buffer = [0u8; 2048];
        let mut partial = String::new();
        let mut ticks = tokio::time::interval(Duration::from_millis(250));
        let deadline = tokio::time::sleep(Duration::from_secs(300)); tokio::pin!(deadline);
        let mut eof = false;
        let mut disk_tick = 0;
        loop {
            tokio::select! {
                status = child.wait() => { if !status.map_err(|_| "Repository download failed.")?.success() { return Err("Could not download the selected commit. Check repository access and commit SHA."); } break; }
                read = stderr.read(&mut buffer), if !eof => {
                    let count = read.map_err(|_| "Repository download failed.")?;
                    if count == 0 { eof = true; continue; }
                    partial.push_str(&String::from_utf8_lossy(&buffer[..count]));
                    while let Some(index) = partial.find(['\r', '\n']) {
                        let line = partial[..index].to_owned(); partial.drain(..=index);
                        if line.contains("Receiving objects:") {
                            let percent = line.split('%').next().and_then(|value| value.split_whitespace().last()).and_then(|value| value.parse::<u8>().ok());
                            job.update(ImportPhase::Downloading, percent, 0, 0);
                        }
                    }
                    if partial.len() > 4096 { partial.clear(); }
                }
                _ = ticks.tick() => {
                    if job.cancelled.load(Ordering::Relaxed) { let _ = child.kill().await; return Err("Import cancelled."); }
                    disk_tick += 1;
                    if disk_tick % 4 == 0 {
                        let path = repository.clone();
                        let bytes = tokio::task::spawn_blocking(move || disk_bytes(&path)).await.map_err(|_| "Could not monitor import size.")?.map_err(|_| "Could not monitor import size.")?;
                        if bytes > limits.max_bytes.saturating_mul(2).saturating_add(16 * 1024 * 1024) { let _ = child.kill().await; return Err("Repository download exceeds the import storage limit."); }
                    }
                }
                _ = &mut deadline => { let _ = child.kill().await; return Err("Repository download timed out. Please try again."); }
            }
        }
        job.update(ImportPhase::Preparing, None, 0, 0);
        let (bytes, files) = job.git.inspect_import(job.project_id, sha, limits.max_bytes, limits.max_files).await.map_err(import_error)?;
        let captured = job.git.import_tree_commit(job.project_id, sha).await.map_err(import_error)?;
        job.git.retain_snapshot(job.project_id, "draft", &captured).await.map_err(import_error)?;
        *job.commit.lock().unwrap() = Some(captured);
        job.update(ImportPhase::Ready, Some(100), bytes, files);
        Ok::<_, &'static str>(())
    }.await;
    if let Err(error) = result
        && !job.cancelled.load(Ordering::Relaxed)
    {
        job.fail(error);
    }
}

pub fn import_error(error: GitError) -> &'static str {
    match error {
        GitError::InvalidInput(message) => message,
        _ => "Could not prepare imported files.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, Uri},
        response::Response,
        routing::get,
    };
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn cleanup_removes_abandoned_staging_and_preserves_live_managers() {
        let directory = std::env::temp_dir().join(format!("fl-import-cleanup-{}", Uuid::new_v4()));
        let git = GitClient::configured(directory.clone()).unwrap();
        let limits = ImportLimits {
            max_bytes: 1024,
            max_files: 10,
        };
        let live = ImportManager::new(&git, limits).unwrap();
        let abandoned = directory.join(format!(".imports-{}", Uuid::new_v4()));
        std::fs::create_dir(&abandoned).unwrap();
        std::fs::File::create(abandoned.join("lease")).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        for path in [&abandoned, &live.root.directory] {
            std::fs::File::open(path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(old))
                .unwrap();
        }
        cleanup_abandoned(&directory);
        assert!(!abandoned.exists());
        assert!(live.root.directory.exists());
        drop(live);
        let _ = tokio::fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn provider_fetch_uses_authenticated_shallow_git_and_publishes_only_a_private_draft() {
        let directory = std::env::temp_dir().join(format!("fl-import-test-{}", Uuid::new_v4()));
        let provider = GitClient::configured(directory.join("provider")).unwrap();
        provider.create_project_repository(1).await.unwrap();
        let sha = provider
            .capture_snapshot(
                1,
                vec![crate::git::GitSnapshotFile {
                    path: "README.md".into(),
                    content: b"complete project".to_vec(),
                    executable: false,
                }],
            )
            .await
            .unwrap();
        provider.retain_snapshot(1, "source", &sha).await.unwrap();
        async fn git_http(
            State(root): State<PathBuf>,
            uri: Uri,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            assert_eq!(
                headers["authorization"],
                format!("Basic {}", STANDARD.encode("x-access-token:private-token"))
            );
            let mut command = tokio::process::Command::new("git");
            command
                .arg("http-backend")
                .env("GIT_PROJECT_ROOT", root)
                .env("GIT_HTTP_EXPORT_ALL", "1")
                .env("PATH_INFO", uri.path())
                .env("QUERY_STRING", uri.query().unwrap_or(""))
                .env(
                    "REQUEST_METHOD",
                    if body.is_empty() { "GET" } else { "POST" },
                )
                .env(
                    "CONTENT_TYPE",
                    headers
                        .get("content-type")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or(""),
                )
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = command.spawn().unwrap();
            child.stdin.take().unwrap().write_all(&body).await.unwrap();
            let output = child.wait_with_output().await.unwrap();
            assert!(output.status.success());
            let separator = output
                .stdout
                .windows(4)
                .position(|bytes| bytes == b"\r\n\r\n")
                .unwrap();
            let mut response = Response::builder();
            for line in std::str::from_utf8(&output.stdout[..separator])
                .unwrap()
                .split("\r\n")
            {
                let (name, value) = line.split_once(':').unwrap();
                response = response.header(name, value.trim());
            }
            response
                .body(axum::body::Body::from(
                    output.stdout[separator + 4..].to_vec(),
                ))
                .unwrap()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = reqwest::Url::parse(&format!("http://{}/1.git", listener.local_addr().unwrap()))
            .unwrap();
        let app = Router::new()
            .route("/{*path}", get(git_http).post(git_http))
            .with_state(provider.repository_root().to_path_buf());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let storage = GitClient::configured(directory.join("storage")).unwrap();
        storage.create_project_repository(2).await.unwrap();
        let limits = ImportLimits {
            max_bytes: 1024,
            max_files: 10,
        };
        let manager = ImportManager::new(&storage, limits).unwrap();
        let job = manager
            .create(
                2,
                10,
                "github".into(),
                Some("main".into()),
                Some(sha.clone()),
            )
            .await
            .unwrap();
        let id = job.status().id;
        assert!(manager.get(id, 2, 11).is_none());
        assert!(manager.get(id, 3, 10).is_none());
        fetch_provider(job.clone(), url, "private-token".into(), limits).await;
        let status = job.status();
        assert!(
            status.phase == ImportPhase::Ready,
            "{}",
            serde_json::to_string(&status).unwrap()
        );
        assert_eq!((status.bytes, status.files), (16, 1));
        assert!(storage.list_snapshot_refs(2).await.unwrap().is_empty());
        let commit = job.ready_commit().unwrap();
        storage
            .import_commit(2, &job.git.repository_path(2).unwrap(), &commit)
            .await
            .unwrap();
        assert_eq!(
            storage
                .snapshot_file(2, &commit, "README.md")
                .await
                .unwrap()
                .content,
            "complete project"
        );
        manager.remove(id, 2, 10);
        assert!(manager.get(id, 2, 10).is_none());
        server.abort();
        drop(job);
        drop(manager);
        let _ = tokio::fs::remove_dir_all(directory).await;
    }
}
