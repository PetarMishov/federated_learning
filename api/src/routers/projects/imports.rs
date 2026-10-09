use crate::{
    auth::verify_user_credentials,
    connectors::repositories::{Provider, RepositoryClient},
    db::{connectors::get_connection_token, snapshots::can_save_snapshot, types::Snapshot},
    git::{GitFile, GitTree, validate_import_path},
    imports::{ImportJob, ImportManager, ImportPhase, fetch_provider, import_error},
    snapshots::{
        access::snapshot_file_error,
        save::save_import,
        types::{MAX_SNAPSHOT_REQUEST_BYTES, SaveSnapshotRequest, SnapshotPath},
    },
    state::AppState,
};
use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    sync::{Arc, atomic::Ordering},
};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/projects/{project_id}/import-limits", get(limits))
        .route("/projects/{project_id}/imports", post(create))
        .route(
            "/projects/{project_id}/imports/{import_id}",
            get(status).delete(discard),
        )
        .route(
            "/projects/{project_id}/imports/{import_id}/files",
            post(upload).layer(DefaultBodyLimit::disable()),
        )
        .route("/projects/{project_id}/imports/{import_id}/tree", get(tree))
        .route("/projects/{project_id}/imports/{import_id}/file", get(file))
        .route(
            "/projects/{project_id}/imports/{import_id}/snapshot",
            post(save).layer(DefaultBodyLimit::max(MAX_SNAPSHOT_REQUEST_BYTES)),
        )
}

async fn access(
    state: &AppState,
    project_id: i32,
    headers: HeaderMap,
) -> Result<i32, (StatusCode, &'static str)> {
    let user = verify_user_credentials(headers, state).await?;
    if !can_save_snapshot(&state.pool, project_id, user)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not verify project access.",
            )
        })?
    {
        return Err((
            StatusCode::NOT_FOUND,
            "Project not found or editing is not allowed.",
        ));
    }
    Ok(user)
}
fn job(
    manager: &ImportManager,
    id: Uuid,
    project: i32,
    user: i32,
) -> Result<Arc<ImportJob>, (StatusCode, &'static str)> {
    manager
        .get(id, project, user)
        .ok_or((StatusCode::NOT_FOUND, "Draft not found or expired."))
}
fn no_store<T: IntoResponse>(body: T) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], body).into_response()
}

async fn limits(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path(project): Path<i32>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, &'static str)> {
    access(&state, project, headers).await?;
    Ok(no_store(Json(manager.limits)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    source: String,
    repository: Option<String>,
    branch: Option<String>,
    commit_sha: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Extension(client): Extension<Option<Arc<RepositoryClient>>>,
    Path(project): Path<i32>,
    headers: HeaderMap,
    Json(request): Json<ImportRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    let user = access(&state, project, headers).await?;
    if !matches!(request.source.as_str(), "local" | "github" | "gitlab") {
        return Err((StatusCode::BAD_REQUEST, "Invalid import source."));
    }
    if request
        .branch
        .as_ref()
        .is_some_and(|branch| branch.len() > 255)
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid branch."));
    }
    let remote = if request.source != "local" {
        let sha = request
            .commit_sha
            .as_deref()
            .ok_or((StatusCode::BAD_REQUEST, "Select a commit to load."))?;
        if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err((
                StatusCode::BAD_REQUEST,
                "Enter a full 40-character commit SHA.",
            ));
        }
        let repository = request
            .repository
            .as_deref()
            .filter(|repository| !repository.is_empty() && repository.len() <= 512)
            .ok_or((StatusCode::BAD_REQUEST, "Select a repository."))?;
        let provider = if request.source == "github" {
            Provider::Github
        } else {
            Provider::Gitlab
        };
        let client = client.ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "Provider connections are not configured.",
        ))?;
        let token = get_connection_token(&state.pool, user, provider.name())
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not read provider connection.",
                )
            })?
            .ok_or((
                StatusCode::NOT_FOUND,
                "Add a provider token before loading code.",
            ))?;
        Some(
            client
                .import_source(provider, user, &token, repository)
                .await
                .map_err(|_| {
                    (
                        StatusCode::BAD_GATEWAY,
                        "Could not access the selected repository. Check your token permissions.",
                    )
                })?,
        )
    } else {
        if request.repository.is_some() || request.branch.is_some() || request.commit_sha.is_some()
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Local imports cannot specify provider fields.",
            ));
        }
        None
    };
    let draft = manager
        .create(
            project,
            user,
            request.source,
            request.branch,
            request.commit_sha.map(|sha| sha.to_ascii_lowercase()),
        )
        .await
        .map_err(|message| (StatusCode::TOO_MANY_REQUESTS, message))?;
    let initial = draft.status();
    if let Some((url, token)) = remote {
        tokio::spawn(fetch_provider(draft, url, token, manager.limits));
    }
    Ok(no_store((StatusCode::ACCEPTED, Json(initial))))
}

async fn status(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, &'static str)> {
    let user = access(&state, project, headers).await?;
    Ok(no_store(Json(job(&manager, id, project, user)?.status())))
}
async fn discard(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    // Allow cleanup after edit permission is revoked, while still enforcing owner.
    let user = verify_user_credentials(headers, &state).await?;
    if let Some(draft) = manager.get(id, project, user) {
        let _guard = if draft.status().phase == ImportPhase::Ready {
            Some(
                draft
                    .busy
                    .try_lock()
                    .map_err(|_| (StatusCode::CONFLICT, "Draft is being saved."))?,
            )
        } else {
            None
        };
        manager.remove(id, project, user);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn upload(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Response, (StatusCode, &'static str)> {
    let user = access(&state, project, headers).await?;
    let draft = job(&manager, id, project, user)?;
    let _guard = draft
        .busy
        .try_lock()
        .map_err(|_| (StatusCode::CONFLICT, "Draft is busy."))?;
    if draft.source != "local" || draft.status().phase != ImportPhase::Uploading {
        return Err((StatusCode::CONFLICT, "Draft cannot accept files."));
    }
    let directory = draft.directory.join("files");
    tokio::fs::create_dir(&directory).await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not stage uploaded files.",
        )
    })?;
    let result = async {
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut bytes = 0u64;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid folder upload."))?
        {
            if draft.cancelled.load(Ordering::Relaxed) {
                return Err((StatusCode::CONFLICT, "Import cancelled."));
            }
            let path = field
                .name()
                .ok_or((StatusCode::BAD_REQUEST, "Missing file path."))?
                .to_owned();
            let path = percent_encoding::percent_decode_str(&path)
                .decode_utf8()
                .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid file path encoding."))?
                .into_owned();
            validate_import_path(&path)
                .map_err(|error| (StatusCode::BAD_REQUEST, import_error(error)))?;
            if !seen.insert(path.clone()) {
                return Err((StatusCode::BAD_REQUEST, "Duplicate uploaded path."));
            }
            if names.len() >= manager.limits.max_files {
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Project exceeds the import file limit.",
                ));
            }
            let mut output = tokio::fs::File::create(directory.join(names.len().to_string()))
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not stage uploaded files.",
                    )
                })?;
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| (StatusCode::BAD_REQUEST, "Folder upload was interrupted."))?
            {
                if draft.cancelled.load(Ordering::Relaxed) {
                    return Err((StatusCode::CONFLICT, "Import cancelled."));
                }
                bytes = bytes.checked_add(chunk.len() as u64).ok_or((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Project exceeds the import size limit.",
                ))?;
                if bytes > manager.limits.max_bytes {
                    return Err((
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Project exceeds the import size limit.",
                    ));
                }
                output.write_all(&chunk).await.map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Could not stage uploaded files.",
                    )
                })?;
                draft.update(ImportPhase::Uploading, None, bytes, names.len());
            }
            names.push((path, false));
        }
        Ok::<_, (StatusCode, &'static str)>((names, bytes))
    }
    .await;
    let (names, bytes) = match result {
        Ok(result) => result,
        Err(error) => {
            draft.fail(error.1);
            return Err(error);
        }
    };
    draft.update(ImportPhase::Preparing, None, bytes, names.len());
    let initial = draft.status();
    let draft = draft.clone();
    let limits = manager.limits;
    tokio::spawn(async move {
        let _guard = draft.busy.lock().await;
        let result = async {
            if draft.cancelled.load(Ordering::Relaxed) {
                return Err("Import cancelled.");
            }
            let commit = draft
                .git
                .capture_staged_files(project, &directory, &names)
                .await
                .map_err(import_error)?;
            let (bytes, files) = draft
                .git
                .inspect_import(project, &commit, limits.max_bytes, limits.max_files)
                .await
                .map_err(import_error)?;
            draft
                .git
                .retain_snapshot(project, "draft", &commit)
                .await
                .map_err(import_error)?;
            *draft.commit.lock().unwrap() = Some(commit);
            draft.update(ImportPhase::Ready, Some(100), bytes, files);
            Ok::<_, &'static str>(())
        }
        .await;
        let _ = tokio::fs::remove_dir_all(directory).await;
        if let Err(error) = result
            && !draft.cancelled.load(Ordering::Relaxed)
        {
            draft.fail(error);
        }
    });
    Ok(no_store((StatusCode::ACCEPTED, Json(initial))))
}

async fn tree(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    Query(query): Query<SnapshotPath>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, &'static str)> {
    let user = access(&state, project, headers).await?;
    let draft = job(&manager, id, project, user)?;
    let commit = draft
        .ready_commit()
        .ok_or((StatusCode::CONFLICT, "Draft is not ready."))?;
    let tree: GitTree = draft
        .git
        .snapshot_tree(project, &commit, &query.path)
        .await
        .map_err(crate::snapshots::access::snapshot_read_error)?;
    Ok(no_store(Json(tree)))
}
async fn file(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    Query(query): Query<SnapshotPath>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user = access(&state, project, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let draft = job(&manager, id, project, user).map_err(IntoResponse::into_response)?;
    let commit = draft
        .ready_commit()
        .ok_or_else(|| (StatusCode::CONFLICT, "Draft is not ready.").into_response())?;
    let file: GitFile = draft
        .git
        .snapshot_file(project, &commit, &query.path)
        .await
        .map_err(snapshot_file_error)?;
    Ok(no_store(Json(file)))
}
async fn save(
    State(state): State<AppState>,
    Extension(manager): Extension<Arc<ImportManager>>,
    Path((project, id)): Path<(i32, Uuid)>,
    headers: HeaderMap,
    Json(request): Json<SaveSnapshotRequest>,
) -> Result<Response, (StatusCode, &'static str)> {
    let user = access(&state, project, headers).await?;
    let draft = job(&manager, id, project, user)?;
    let _guard = draft
        .busy
        .try_lock()
        .map_err(|_| (StatusCode::CONFLICT, "Draft is busy."))?;
    let snapshot: Snapshot = save_import(&state, &draft, request, manager.limits).await?;
    manager.remove(id, project, user);
    Ok(no_store((StatusCode::CREATED, Json(snapshot))))
}
