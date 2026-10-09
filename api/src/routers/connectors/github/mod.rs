pub mod authorization;
use crate::state::AppState;
use axum::{Router, routing::post};

pub fn github_router() -> Router<AppState> {
    Router::new().route(
        "/connectors/github/authorize",
        post(authorization::authorize_request),
    )
}
