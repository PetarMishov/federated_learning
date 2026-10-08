use crate::{
    git::GitTree,
    snapshots::{
        access::{SnapshotResponseError, authorized_snapshot_commit, snapshot_read_error},
        types::SnapshotPath,
    },
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};

pub async fn get_snapshot_tree_request(
    State(state): State<AppState>,
    Path((project_id, snapshot_id)): Path<(i32, i32)>,
    Query(query): Query<SnapshotPath>,
    headers: HeaderMap,
) -> Result<Json<GitTree>, SnapshotResponseError> {
    let commit = authorized_snapshot_commit(&state, project_id, snapshot_id, headers).await?;
    let tree = state
        .git
        .snapshot_tree(project_id, &commit, &query.path)
        .await
        .map_err(snapshot_read_error)?;
    Ok(Json(tree))
}
