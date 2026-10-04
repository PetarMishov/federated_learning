use super::verify_user::authenticated_claims;
use crate::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};

pub async fn logout_request(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, ()), (StatusCode, &'static str)> {
    let (_, claims) = authenticated_claims(headers, &state).await?;
    sqlx::query(
        "INSERT INTO revoked_tokens (jti, expires_at)
         VALUES ($1, to_timestamp($2::double precision))
         ON CONFLICT (jti) DO NOTHING",
    )
    .bind(&claims.jti)
    .bind(claims.exp as f64)
    .execute(&state.pool)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not log out."))?;
    Ok((StatusCode::NO_CONTENT, ()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routers::users::{
        types::VerifyLoginRequest,
        verify_user::{verify_login_request, verify_user_credentials},
    };
    use axum::Json;
    use jsonwebtoken::{DecodingKey, EncodingKey};
    use sqlx::PgPool;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL with demo fixtures"]
    async fn logout_revokes_only_the_current_token_and_persists_across_state_recreation() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let secret = b"logout-test-secret-at-least-thirty-two-bytes";
        let state = AppState {
            pool,
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        };
        let login = || VerifyLoginRequest {
            username: "demo".into(),
            password: "demo-password".into(),
        };
        let (_, Json(first)) = verify_login_request(State(state.clone()), Json(login()))
            .await
            .unwrap();
        let (_, Json(second)) = verify_login_request(State(state.clone()), Json(login()))
            .await
            .unwrap();
        assert_ne!(first, second);
        let headers = |token: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            headers
        };
        assert!(
            verify_user_credentials(headers(&first), &state)
                .await
                .is_ok()
        );
        assert_eq!(
            logout_request(State(state.clone()), headers(&first))
                .await
                .unwrap(),
            (StatusCode::NO_CONTENT, ())
        );
        let restarted = AppState {
            pool: state.pool.clone(),
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        };
        assert_eq!(
            verify_user_credentials(headers(&first), &restarted)
                .await
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert!(
            verify_user_credentials(headers(&second), &restarted)
                .await
                .is_ok()
        );
        assert_eq!(
            logout_request(State(restarted), headers(&first))
                .await
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
        state.pool.close().await;
    }
}
