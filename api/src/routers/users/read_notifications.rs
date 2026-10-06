use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};

use crate::db::notifications::read_user_notifications;
use crate::{auth::verify_user_credentials, state::AppState};

pub async fn read_notifications_request(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;

    read_user_notifications(&state.pool, user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error."))?;
    Ok(StatusCode::NO_CONTENT)
}
