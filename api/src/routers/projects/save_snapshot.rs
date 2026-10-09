use crate::{
    auth::verify_user_credentials,
    db::types::Snapshot,
    snapshots::{save::save_snapshot, types::SaveSnapshotRequest},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
};

pub async fn save_snapshot_request(
    State(state): State<AppState>,
    Path(project_id): Path<i32>,
    headers: HeaderMap,
    payload: Result<Json<SaveSnapshotRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Snapshot>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let Json(request) = payload.map_err(|error| (error.status(), "Invalid snapshot upload."))?;
    let snapshot = save_snapshot(&state, project_id, user_id, request).await?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}
