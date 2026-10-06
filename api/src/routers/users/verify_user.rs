use super::types::VerifyLoginRequest;
use crate::db::users::Claims;
use crate::db::users::verify_user_password;
use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode};
use jsonwebtoken::{Algorithm, Header};

pub async fn verify_login_request(
    State(state): State<AppState>,
    Json(request): Json<VerifyLoginRequest>,
) -> Result<(StatusCode, Json<String>), (StatusCode, &'static str)> {
    if request.username.trim().is_empty() || request.password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Username and password are required",
        ));
    }
    let verified_user_id = verify_user_password(&state.pool, &request.username, &request.password)
        .await
        .map_err(|_e| (StatusCode::INTERNAL_SERVER_ERROR, "Could not verify user."))?;

    if let Some(user_id) = verified_user_id {
        let claims = Claims {
            sub: user_id.to_string(),
            exp: jsonwebtoken::get_current_timestamp() + 15 * 60,
            jti: uuid::Uuid::new_v4().to_string(),
        };
        let token =
            jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &state.encoding_key)
                .map_err(|_e| (StatusCode::INTERNAL_SERVER_ERROR, "Could not verify user."))?;
        return Ok((StatusCode::OK, Json(token)));
    }
    Ok((StatusCode::UNAUTHORIZED, Json("".to_string())))
}
