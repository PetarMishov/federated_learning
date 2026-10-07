use crate::{
    auth::verify_user_credentials,
    db::{projects::create_project, types::Project},
    routers::types::CreateNameRequest,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

pub async fn create_project_request(
    State(state): State<AppState>,
    Path(org_id): Path<i32>,
    headers: HeaderMap,
    Json(request): Json<CreateNameRequest>,
) -> Result<(StatusCode, Json<Project>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let name = request.validated_name()?;
    let project = create_project(&state.pool, org_id, user_id, name)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create project.",
            )
        })?
        .ok_or((
            StatusCode::FORBIDDEN,
            "Only the organization owner can create projects.",
        ))?;
    Ok((StatusCode::CREATED, Json(project)))
}
