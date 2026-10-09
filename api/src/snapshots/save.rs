use super::types::{MAX_SNAPSHOT_FILES, SaveSnapshotRequest};
use crate::{
    db::{snapshots, types::Snapshot},
    git::{GitError, GitSnapshotFile},
    state::AppState,
};
use axum::http::StatusCode;
use base64::{Engine, engine::general_purpose::STANDARD};

const SNAPSHOT_CREATION_LOCK: i32 = 0x464c534e;
const SAVE_FAILED: (StatusCode, &str) = (
    StatusCode::INTERNAL_SERVER_ERROR,
    "Could not save snapshot.",
);
const NO_ACCESS: (StatusCode, &str) = (
    StatusCode::NOT_FOUND,
    "Project not found or editing is not allowed.",
);

pub async fn save_snapshot(
    state: &AppState,
    project_id: i32,
    user_id: i32,
    request: SaveSnapshotRequest,
) -> Result<Snapshot, (StatusCode, &'static str)> {
    if !snapshots::can_save_snapshot(&state.pool, project_id, user_id)
        .await
        .map_err(|_| SAVE_FAILED)?
    {
        return Err(NO_ACCESS);
    }
    let base_commit = match request.base_snapshot_id {
        Some(id) => Some(
            snapshots::get_snapshot(&state.pool, project_id, id, user_id)
                .await
                .map_err(|_| SAVE_FAILED)?
                .ok_or((StatusCode::NOT_FOUND, "Base snapshot not found."))?
                .git_commit_sha,
        ),
        None => None,
    };
    save_version(
        state,
        project_id,
        user_id,
        request,
        base_commit.as_deref(),
        None,
        None,
    )
    .await
}

pub async fn save_import(
    state: &AppState,
    job: &crate::imports::ImportJob,
    request: SaveSnapshotRequest,
    limits: crate::imports::ImportLimits,
) -> Result<Snapshot, (StatusCode, &'static str)> {
    if request.base_snapshot_id.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "An import cannot use a saved snapshot as its base.",
        ));
    }
    if !snapshots::can_save_snapshot(&state.pool, job.project_id, job.user_id)
        .await
        .map_err(|_| SAVE_FAILED)?
    {
        return Err(NO_ACCESS);
    }
    let commit = job
        .ready_commit()
        .ok_or((StatusCode::CONFLICT, "Import is not ready."))?;
    let source = job
        .git
        .repository_path(job.project_id)
        .map_err(|_| SAVE_FAILED)?;
    state
        .git
        .import_commit(job.project_id, &source, &commit)
        .await
        .map_err(|_| SAVE_FAILED)?;
    save_version(
        state,
        job.project_id,
        job.user_id,
        request,
        Some(&commit),
        Some(job),
        Some(limits),
    )
    .await
}

async fn save_version(
    state: &AppState,
    project_id: i32,
    user_id: i32,
    request: SaveSnapshotRequest,
    base_commit: Option<&str>,
    import: Option<&crate::imports::ImportJob>,
    limits: Option<crate::imports::ImportLimits>,
) -> Result<Snapshot, (StatusCode, &'static str)> {
    let (source, branch, source_commit) = import.map_or(("local", None, None), |job| {
        (
            job.source.as_str(),
            job.source_branch.as_deref(),
            job.source_commit_sha.as_deref(),
        )
    });
    if request.operations.len() > MAX_SNAPSHOT_FILES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "Too many tree operations."));
    }
    let operations = request.operations.clone();
    let files = decode_files(request)?;
    let captured = if operations.is_empty() {
        match base_commit {
            Some(base) => {
                state
                    .git
                    .capture_snapshot_changes(project_id, Some(base), files)
                    .await
            }
            None => state.git.capture_snapshot(project_id, files).await,
        }
    } else {
        state
            .git
            .capture_snapshot_patch(project_id, base_commit, files, operations)
            .await
    };
    let commit_sha = captured.map_err(|error| match error {
        GitError::NotFound => (
            StatusCode::BAD_REQUEST,
            "Edited path not found in base snapshot.",
        ),
        GitError::InvalidInput(message) => (StatusCode::BAD_REQUEST, message),
        _ => SAVE_FAILED,
    })?;
    if let Some(limits) = limits {
        state
            .git
            .inspect_import(project_id, &commit_sha, limits.max_bytes, limits.max_files)
            .await
            .map_err(|error| match error {
                GitError::InvalidInput(message) if message.contains("exceeds") => {
                    (StatusCode::PAYLOAD_TOO_LARGE, message)
                }
                GitError::InvalidInput(message) => (StatusCode::BAD_REQUEST, message),
                _ => SAVE_FAILED,
            })?;
    }
    let mut transaction = state.pool.begin().await.map_err(|_| SAVE_FAILED)?;
    lock_project(&mut transaction, project_id)
        .await
        .map_err(|_| SAVE_FAILED)?;
    let snapshot = if source == "local" && branch.is_none() && source_commit.is_none() {
        snapshots::create_snapshot(&mut *transaction, project_id, user_id, &commit_sha).await
    } else {
        snapshots::create_imported_snapshot(
            &mut *transaction,
            project_id,
            user_id,
            &commit_sha,
            source,
            branch,
            source_commit,
        )
        .await
    }
    .map_err(|_| SAVE_FAILED)?
    .ok_or(NO_ACCESS)?;
    state
        .git
        .retain_snapshot(project_id, &snapshot.id.to_string(), &commit_sha)
        .await
        .map_err(|_| SAVE_FAILED)?;
    if transaction.commit().await.is_err() {
        // Wait for the original transaction to finish before resolving a lost
        // COMMIT response. Unknown outcomes retain Git storage for reconciliation.
        let mut check = state.pool.begin().await.map_err(|_| SAVE_FAILED)?;
        lock_project(&mut check, project_id)
            .await
            .map_err(|_| SAVE_FAILED)?;
        let saved =
            snapshots::snapshot_was_saved(&mut *check, project_id, snapshot.id, &commit_sha)
                .await
                .map_err(|_| SAVE_FAILED)?;
        check.rollback().await.map_err(|_| SAVE_FAILED)?;
        if !saved {
            return Err(SAVE_FAILED);
        }
    }
    Ok(snapshot)
}

fn decode_files(
    request: SaveSnapshotRequest,
) -> Result<Vec<GitSnapshotFile>, (StatusCode, &'static str)> {
    if request.files.len() > MAX_SNAPSHOT_FILES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "Snapshots may contain at most 10000 files.",
        ));
    }
    request
        .files
        .into_iter()
        .map(|file| {
            let content = match (file.content, file.content_base64) {
                (Some(text), None) => text.into_bytes(),
                (None, Some(encoded)) => STANDARD.decode(encoded).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        "File content_base64 must contain valid base64.",
                    )
                })?,
                _ => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "Each file must provide exactly one of content or content_base64.",
                    ));
                }
            };
            Ok(GitSnapshotFile {
                path: file.path,
                content,
                executable: file.executable,
            })
        })
        .collect()
}

async fn lock_project(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(SNAPSHOT_CREATION_LOCK)
        .bind(project_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}
