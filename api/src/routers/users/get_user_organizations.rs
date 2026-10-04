use crate::db::{organizations::get_user_organizations, types::OrganizationList};
use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};

use super::verify_user::verify_user_credentials;

pub async fn get_user_organizations_request(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<OrganizationList>, (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;

    // Identity comes exclusively from the verified token, never request input.
    let organizations = get_user_organizations(&state.pool, user_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load organizations.",
            )
        })?;
    Ok(Json(organizations))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::users::Claims;
    use jsonwebtoken::{
        Algorithm, DecodingKey, EncodingKey, Header, encode, get_current_timestamp,
    };
    use sqlx::PgPool;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to the schema with demo fixtures"]
    async fn authenticated_users_receive_only_their_own_memberships() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let secret = b"organization-test-secret-at-least-32-bytes";
        let state = AppState {
            pool,
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        };
        for (username, expected) in [
            (
                "demo",
                vec!["Central Hospital", "Medical Network", "Research Lab"],
            ),
            ("other-user", vec!["Other User Organization"]),
            ("no-memberships", vec![]),
        ] {
            let (user_id,) =
                sqlx::query_as::<_, (i32,)>("SELECT id FROM users WHERE username = $1")
                    .bind(username)
                    .fetch_one(&state.pool)
                    .await
                    .unwrap();
            let token = encode(
                &Header::new(Algorithm::HS256),
                &Claims {
                    sub: user_id.to_string(),
                    exp: get_current_timestamp() + 900,
                    jti: uuid::Uuid::new_v4().to_string(),
                },
                &state.encoding_key,
            )
            .unwrap();
            let mut headers = HeaderMap::new();
            headers.insert(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            // An unrelated identity supplied by the caller must have no effect.
            headers.insert("x-user-id", "999".parse().unwrap());
            let Json(result) = get_user_organizations_request(State(state.clone()), headers)
                .await
                .unwrap();
            let names: Vec<_> = result
                .organizations
                .iter()
                .map(|org| org.name.as_str())
                .collect();
            assert_eq!(names, expected);
        }
        state.pool.close().await;
    }

    #[tokio::test]
    async fn rejects_missing_malformed_expired_and_forged_credentials_before_querying() {
        let secret = b"organization-test-secret-at-least-32-bytes";
        let state = AppState {
            pool: PgPool::connect_lazy("postgres://localhost/unused").unwrap(),
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        };
        let sign = |sub: &str, exp, key: &EncodingKey| {
            encode(
                &Header::new(Algorithm::HS256),
                &Claims {
                    sub: sub.into(),
                    exp,
                    jti: uuid::Uuid::new_v4().to_string(),
                },
                key,
            )
            .unwrap()
        };
        let expired = sign("1", get_current_timestamp() - 60, &state.encoding_key);
        let forged = sign(
            "1",
            get_current_timestamp() + 900,
            &EncodingKey::from_secret(b"wrong-secret"),
        );
        let invalid_subject = sign(
            "not-an-id",
            get_current_timestamp() + 900,
            &state.encoding_key,
        );
        let negative_subject = sign("-1", get_current_timestamp() + 900, &state.encoding_key);
        for authorization in [
            None,
            Some("Basic credentials".into()),
            Some("Bearer invalid".into()),
            Some(format!("Bearer {expired}")),
            Some(format!("Bearer {forged}")),
            Some(format!("Bearer {invalid_subject}")),
            Some(format!("Bearer {negative_subject}")),
        ] {
            let mut headers = HeaderMap::new();
            if let Some(value) = authorization {
                headers.insert(axum::http::header::AUTHORIZATION, value.parse().unwrap());
            }
            let result = get_user_organizations_request(State(state.clone()), headers).await;
            match result {
                Err((status, _)) => assert_eq!(status, StatusCode::UNAUTHORIZED),
                Ok(_) => panic!("Invalid authentication was accepted"),
            }
        }
    }
}
