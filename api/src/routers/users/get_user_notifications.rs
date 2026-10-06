use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};

use crate::db::{notifications::get_user_notifications, types::NotificationList};
use crate::{auth::verify_user_credentials, state::AppState};

pub async fn get_user_notifications_request(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<NotificationList>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;

    let notifications = get_user_notifications(&state.pool, user_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load notifications.",
            )
        })?;
    Ok((StatusCode::OK, Json(notifications)))
}
