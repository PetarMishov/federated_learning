use crate::{
    git::GitFile,
    snapshots::{
        access::{authorized_snapshot_commit, snapshot_file_error},
        types::SnapshotPath,
    },
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
};

pub async fn get_snapshot_file_request(
    State(state): State<AppState>,
    Path((project_id, snapshot_id)): Path<(i32, i32)>,
    Query(query): Query<SnapshotPath>,
    headers: HeaderMap,
) -> Result<Json<GitFile>, Response> {
    let commit = authorized_snapshot_commit(&state, project_id, snapshot_id, headers)
        .await
        .map_err(IntoResponse::into_response)?;
    let file = state
        .git
        .snapshot_file(project_id, &commit, &query.path)
        .await
        .map_err(snapshot_file_error)?;
    Ok(Json(file))
}
