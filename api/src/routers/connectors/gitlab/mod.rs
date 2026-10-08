use axum::Router;

use crate::state::AppState;

pub fn gitlab_router() -> Router<AppState> {
    Router::new()
}
