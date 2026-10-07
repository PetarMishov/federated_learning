use crate::{
    auth::verify_user_credentials,
    db::{organizations::create_organization, types::Organization},
    routers::types::CreateNameRequest,
    state::AppState,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};

pub async fn create_organization_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateNameRequest>,
) -> Result<(StatusCode, Json<Organization>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let name = request.validated_name()?;
    let organization = create_organization(&state.pool, user_id, name)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create organization.",
            )
        })?;
    Ok((StatusCode::CREATED, Json(organization)))
}
