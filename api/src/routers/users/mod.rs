mod types;
mod verify_user;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn users_router() -> Router<AppState> {
    Router::new().route("/verify", post(verify_user::verify_login_request))
}
