use axum::Router;

use crate::state::AppState;

pub fn github_router() -> Router<AppState> {
    Router::new()
}
