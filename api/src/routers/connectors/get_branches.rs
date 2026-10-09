use crate::{
    auth::verify_user_credentials,
    connectors::repositories::{BranchList, Provider, RepositoryClient, RepositoryError},
    db::connectors::get_connection_token,
    state::AppState,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct BranchQuery {
    #[serde(default = "first_page")]
    page: u32,
    repository: String,
}
fn first_page() -> u32 {
    1
}

pub async fn get_branches_request(
    State(state): State<AppState>,
    Extension(client): Extension<Option<Arc<RepositoryClient>>>,
    headers: HeaderMap,
    Path(provider): Path<Provider>,
    Query(query): Query<BranchQuery>,
) -> Result<(HeaderMap, Json<BranchList>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    if query.page == 0
        || query.page > 10000
        || query.repository.is_empty()
        || query.repository.len() > 512
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid repository page."));
    }
    let client = client.ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "Repository connections are not configured.",
    ))?;
    let token = get_connection_token(&state.pool, user_id, provider.name())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load repository connection.",
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            "Add a valid provider token before selecting a repository.",
        ))?;
    let repositories = client
        .branches(provider, user_id, &token, &query.repository, query.page)
        .await
        .map_err(|error| match error {
            RepositoryError::InvalidCredential => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read repository connection.",
            ),
            RepositoryError::Rejected => (
                StatusCode::FORBIDDEN,
                "Repository unavailable or token lacks permission to read branches.",
            ),
            RepositoryError::Unavailable => (
                StatusCode::BAD_GATEWAY,
                "Could not load branches from the provider.",
            ),
        })?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok((headers, Json(repositories)))
}
