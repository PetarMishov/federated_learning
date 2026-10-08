mod github;
mod gitlab;

use axum::Router;

use crate::state::AppState;

pub fn connectors_router() -> Router<AppState> {
    Router::new()
        .merge(github::github_router())
        .merge(gitlab::gitlab_router())
}
