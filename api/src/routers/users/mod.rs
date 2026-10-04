use axum::Router;
use std::todo;

pub fn users_router() -> Router {
    Router::new().route("temp", todo!())
}
