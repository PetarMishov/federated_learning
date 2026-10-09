use super::types::SnapshotPreviewTooLarge;
use crate::{
    auth::verify_user_credentials,
    db::snapshots::get_snapshot,
    git::{GitError, MAX_PREVIEW_BYTES},
    state::AppState,
};
use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

pub async fn authorized_snapshot_commit(
    state: &AppState,
    project_id: i32,
    snapshot_id: i32,
    headers: HeaderMap,
) -> Result<String, (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, state).await?;
    let snapshot = get_snapshot(&state.pool, project_id, snapshot_id, user_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load snapshot.",
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "Snapshot not found."))?;
    Ok(snapshot.git_commit_sha)
}

pub fn snapshot_read_error(error: GitError) -> (StatusCode, &'static str) {
    match error {
        GitError::InvalidInput(_) => (
            StatusCode::BAD_REQUEST,
            "Path must be relative to the snapshot root.",
        ),
        GitError::NotFound => (StatusCode::NOT_FOUND, "Snapshot path not found."),
        GitError::UnsupportedFile => (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Only regular UTF-8 text files can be viewed.",
        ),
        GitError::FileTooLarge { .. } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "File is too large to preview.",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read snapshot storage.",
        ),
    }
}

pub fn snapshot_file_error(error: GitError) -> Response {
    match error {
        GitError::FileTooLarge { size_bytes } => (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(SnapshotPreviewTooLarge {
                error: "preview_too_large",
                size_bytes,
                max_preview_bytes: MAX_PREVIEW_BYTES,
            }),
        )
            .into_response(),
        error => snapshot_read_error(error).into_response(),
    }
}
