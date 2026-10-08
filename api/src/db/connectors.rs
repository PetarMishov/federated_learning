use crate::{connectors::gitlab::authorization::AuthorizedConnection, db::users::Claims};
use sqlx::PgPool;

/// Reconnecting replaces only the authenticated user's credentials. Check the
/// session again because the provider requests may have taken several seconds.
pub async fn save_gitlab_connection(
    pool: &PgPool,
    user_id: i32,
    claims: &Claims,
    connection: &AuthorizedConnection,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO provider_connections
            (user_id, provider, external_account_id, external_username,
             access_token_encrypted, token_expires_at, granted_permissions)
         SELECT $1, 'gitlab', $2, $3, $4, ($5::text::date::timestamp AT TIME ZONE 'UTC'), $6
         WHERE to_timestamp($7::double precision) > clock_timestamp()
           AND NOT EXISTS (SELECT 1 FROM revoked_tokens WHERE jti = $8)
         ON CONFLICT (user_id, provider) DO UPDATE SET
            external_account_id = EXCLUDED.external_account_id,
            external_username = EXCLUDED.external_username,
            access_token_encrypted = EXCLUDED.access_token_encrypted,
            refresh_token_encrypted = NULL,
            token_expires_at = EXCLUDED.token_expires_at,
            granted_permissions = EXCLUDED.granted_permissions,
            updated_at = CURRENT_TIMESTAMP, invalidated_at = NULL",
    )
    .bind(user_id)
    .bind(&connection.response.external_account_id)
    .bind(&connection.response.external_username)
    .bind(&connection.access_token_encrypted)
    .bind(&connection.expires_at)
    .bind(&connection.granted_permissions)
    .bind(claims.exp as f64)
    .bind(&claims.jti)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Credentials are always scoped to the authenticated user and usable connection.
pub async fn get_connection_token(
    pool: &PgPool,
    user_id: i32,
    provider: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT access_token_encrypted FROM provider_connections WHERE user_id = $1 AND provider = $2::text::provider_kind AND invalidated_at IS NULL AND (token_expires_at IS NULL OR token_expires_at > clock_timestamp())")
        .bind(user_id).bind(provider).fetch_optional(pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::gitlab::authorization::ConnectionResponse;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL and permission to create a temporary schema"]
    async fn token_connections_preserve_other_users_and_handle_expiry_and_revocation() {
        let url = std::env::var("TEST_DATABASE_URL").unwrap();
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("fl_gitlab_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let search_path = format!("SET search_path TO {schema}");
        let pool = PgPoolOptions::new()
            .after_connect(move |connection, _| {
                let search_path = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&search_path).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        let check_pool = pool.clone();
        let result = tokio::spawn(async move {
            let schema_sql = include_str!("../../../db/schema.sql")
                .replace("\\ir deployment_rules.sql", include_str!("../../../db/deployment_rules.sql"))
                .lines().filter(|line| !line.starts_with('\\')).collect::<Vec<_>>().join("\n");
            sqlx::raw_sql(&schema_sql).execute(&check_pool).await.unwrap();
            let ids = sqlx::query_scalar::<_, i32>("INSERT INTO users (username, password_hash) VALUES ('first', 'unused'), ('second', 'unused') RETURNING id")
                .fetch_all(&check_pool).await.unwrap();
            let mut claims = Claims { sub: ids[0].to_string(), exp: jsonwebtoken::get_current_timestamp() + 900, jti: "test-session".into() };
            let mut connection = AuthorizedConnection {
                response: ConnectionResponse { provider: "gitlab", external_account_id: "same-account".into(), external_username: "external-user".into() },
                access_token_encrypted: "v1:encrypted-original".into(),
                expires_at: Some("2099-01-01".into()),
                granted_permissions: "read_user read_repository".into(),
            };
            for id in &ids {
                assert!(save_gitlab_connection(&check_pool, *id, &claims, &connection).await.unwrap());
            }
            let expiry: String = sqlx::query_scalar("SELECT to_char(token_expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') FROM provider_connections WHERE user_id = $1")
                .bind(ids[0]).fetch_one(&check_pool).await.unwrap();
            assert_eq!(expiry, "2099-01-01 00:00:00");
            sqlx::query("UPDATE provider_connections SET invalidated_at = CURRENT_TIMESTAMP, refresh_token_encrypted = 'old-oauth-token' WHERE user_id = $1")
                .bind(ids[0]).execute(&check_pool).await.unwrap();
            connection.access_token_encrypted = "v1:encrypted-replacement".into();
            connection.expires_at = None;
            assert!(save_gitlab_connection(&check_pool, ids[0], &claims, &connection).await.unwrap());
            let rows: Vec<(i32, String, bool)> = sqlx::query_as("SELECT user_id, access_token_encrypted, invalidated_at IS NULL AND refresh_token_encrypted IS NULL AND token_expires_at IS NULL FROM provider_connections ORDER BY user_id")
                .fetch_all(&check_pool).await.unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0], (ids[0], "v1:encrypted-replacement".into(), true));
            assert_eq!(rows[1].1, "v1:encrypted-original");
            claims.exp = 0;
            assert!(!save_gitlab_connection(&check_pool, ids[0], &claims, &connection).await.unwrap());
            claims.exp = jsonwebtoken::get_current_timestamp() + 900;
            sqlx::query("INSERT INTO revoked_tokens (jti, expires_at) VALUES ($1, CURRENT_TIMESTAMP + interval '1 hour')")
                .bind(&claims.jti).execute(&check_pool).await.unwrap();
            assert!(!save_gitlab_connection(&check_pool, ids[0], &claims, &connection).await.unwrap());
        }).await;
        pool.close().await;
        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        result.unwrap();
    }
}
