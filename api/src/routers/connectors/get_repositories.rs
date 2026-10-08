use crate::{
    auth::verify_user_credentials,
    connectors::repositories::{Provider, RepositoryClient, RepositoryError, RepositoryList},
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
pub struct RepositoryQuery {
    #[serde(default = "first_page")]
    page: u32,
}
fn first_page() -> u32 {
    1
}

pub async fn get_repositories_request(
    State(state): State<AppState>,
    Extension(client): Extension<Option<Arc<RepositoryClient>>>,
    headers: HeaderMap,
    Path(provider): Path<Provider>,
    Query(query): Query<RepositoryQuery>,
) -> Result<(HeaderMap, Json<RepositoryList>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    if query.page == 0 || query.page > 10000 {
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
        .list(provider, user_id, &token, query.page)
        .await
        .map_err(|error| match error {
            RepositoryError::InvalidCredential => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read repository connection.",
            ),
            RepositoryError::Rejected => (
                StatusCode::FORBIDDEN,
                "Provider token was rejected. Replace it with a valid read token.",
            ),
            RepositoryError::Unavailable => (
                StatusCode::BAD_GATEWAY,
                "Could not load repositories from the provider.",
            ),
        })?;
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok((headers, Json(repositories)))
}
