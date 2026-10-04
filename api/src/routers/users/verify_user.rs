use super::types::{Claims, VerifyLoginRequest};
use crate::db::users::{verify_user_password, verify_user_token};
use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
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
        };
        let token =
            jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &state.encoding_key)
                .map_err(|_e| (StatusCode::INTERNAL_SERVER_ERROR, "Could not verify user."))?;
        return Ok((StatusCode::OK, Json(token)));
    }
    Ok((StatusCode::UNAUTHORIZED, Json("".to_string())))
}

pub fn verify_user_credentials(
    headers: HeaderMap,
    state: &AppState,
) -> Result<i32, (StatusCode, &'static str)> {
    let unauthorized = (StatusCode::UNAUTHORIZED, "Valid bearer token required.");
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(unauthorized)?;
    let mut parts = authorization.split_whitespace();
    let scheme = parts.next().ok_or(unauthorized)?;
    let token = parts.next().ok_or(unauthorized)?;
    if !scheme.eq_ignore_ascii_case("Bearer") || parts.next().is_some() {
        return Err(unauthorized);
    }

    let claims = verify_user_token(token, &state).map_err(|_| unauthorized)?;
    let user_id = claims.sub.parse::<i32>().map_err(|_| unauthorized)?;
    if user_id <= 0 {
        return Err(unauthorized);
    }
    Ok(user_id)
}
