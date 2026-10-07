use crate::{
    auth::verify_user_credentials,
    db::{projects::get_project_members, types::MemberList},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

pub async fn get_project_members_request(
    State(state): State<AppState>,
    Path(proj_id): Path<i32>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<MemberList>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let members = get_project_members(&state.pool, proj_id, user_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load project members.",
            )
        })?;
    Ok((StatusCode::OK, Json(members)))
}
