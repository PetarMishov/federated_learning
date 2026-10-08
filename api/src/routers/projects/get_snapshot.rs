use crate::{
    auth::verify_user_credentials,
    db::{snapshots::get_snapshot, types::Snapshot},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

pub async fn get_snapshot_request(
    State(state): State<AppState>,
    Path((proj_id, snapshot_id)): Path<(i32, i32)>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Snapshot>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let snapshot = get_snapshot(&state.pool, proj_id, snapshot_id, user_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load snapshot.",
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "Snapshot not found."))?;
    Ok((StatusCode::OK, Json(snapshot)))
}
