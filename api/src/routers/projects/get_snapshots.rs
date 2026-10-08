use crate::{
    auth::verify_user_credentials,
    db::{snapshots::get_snapshots, types::SnapshotList},
    snapshots::types::SnapshotPage,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};

pub async fn get_snapshots_request(
    State(state): State<AppState>,
    Path(project_id): Path<i32>,
    Query(page): Query<SnapshotPage>,
    headers: HeaderMap,
) -> Result<Json<SnapshotList>, (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    if !(1..=100).contains(&page.limit) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Snapshot limit must be between 1 and 100.",
        ));
    }
    let list = get_snapshots(&state.pool, project_id, user_id, page.limit, page.offset)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load snapshots.",
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "Project not found."))?;
    Ok(Json(list))
}
