use crate::{
    db::users::{Claims, verify_user_token},
    state::AppState,
};
use axum::http::{HeaderMap, StatusCode};

pub async fn verify_user_credentials(
    headers: HeaderMap,
    state: &AppState,
) -> Result<i32, (StatusCode, &'static str)> {
    let (user_id, _) = authenticated_claims(headers, state).await?;
    Ok(user_id)
}

pub async fn authenticated_claims(
    headers: HeaderMap,
    state: &AppState,
) -> Result<(i32, Claims), (StatusCode, &'static str)> {
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
    if claims.jti.is_empty() {
        return Err(unauthorized);
    }
    let revoked = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM revoked_tokens WHERE jti = $1)",
    )
    .bind(&claims.jti)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not verify session.",
        )
    })?;
    if revoked {
        return Err(unauthorized);
    }
    Ok((user_id, claims))
}
