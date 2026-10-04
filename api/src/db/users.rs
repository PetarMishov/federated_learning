use super::auth;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::state::AppState;

pub type UserError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
#[repr(i32)]
pub enum UserType {
    Basic = 1,
    Admin = 2,
    TrueAdmin = 3,
}

impl TryFrom<i32> for UserType {
    type Error = std::io::Error;

    fn try_from(id: i32) -> Result<Self, Self::Error> {
        match id {
            1 => Ok(Self::Basic),
            2 => Ok(Self::Admin),
            3 => Ok(Self::TrueAdmin),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Unknown user type ID",
            )),
        }
    }
}

#[derive(Serialize)]
pub struct UserData {
    pub id: i64,
    pub email: String,
    pub username: String,
    pub user_type: UserType,
}

#[derive(Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

pub async fn create_user(
    pool: &PgPool,
    email: &str,
    username: &str,
    password: &str,
    user_type: UserType,
) -> Result<UserData, UserError> {
    let password = password.to_owned();
    let password_hash = tokio::task::spawn_blocking(move || auth::hash(&password)).await??;
    let (id, email, username, user_type) = sqlx::query_as::<_, (i64, String, String, i32)>(
        "INSERT INTO users (email, username, password_hash, user_type)
         VALUES ($1, $2, $3, $4)
         RETURNING id, email, username, user_type",
    )
    .bind(email)
    .bind(username)
    .bind(password_hash)
    .bind(user_type as i32)
    .fetch_one(pool)
    .await?;

    Ok(UserData {
        id,
        email,
        username,
        user_type: UserType::try_from(user_type)?,
    })
}

pub async fn verify_user_password(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<Option<i64>, UserError> {
    let record = sqlx::query_as::<_, (i64, String)>(
        "SELECT id, password_hash FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    let Some((user_id, stored_hash)) = record else {
        return Ok(None);
    };
    let password = password.to_owned();
    let matches =
        tokio::task::spawn_blocking(move || auth::verify(&password, &stored_hash)).await??;
    Ok(if matches { Some(user_id) } else { None })
}

pub fn verify_user_token(
    token: &str,
    app_state: &AppState,
) -> Result<Claims, jsonwebtoken::errors::Error> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.set_required_spec_claims(&["sub", "exp"]);
    validation.leeway = 0;
    let data = jsonwebtoken::decode::<Claims>(token, &app_state.decoding_key, &validation)?;
    Ok(data.claims)
}

pub async fn get_user_by_id(pool: &PgPool, id: i64) -> Result<UserData, UserError> {
    let (id, email, username, user_type) = sqlx::query_as::<_, (i64, String, String, i32)>(
        "SELECT id, email, username, user_type FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(UserData {
        id,
        email,
        username,
        user_type: UserType::try_from(user_type)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_types_match_database_ids_and_enforce_creation_permissions() {
        assert_eq!(UserType::try_from(1).unwrap(), UserType::Basic);
        assert_eq!(UserType::try_from(2).unwrap(), UserType::Admin);
        assert_eq!(UserType::try_from(3).unwrap(), UserType::TrueAdmin);
        assert!(UserType::try_from(0).is_err());
        let roles = [UserType::Basic, UserType::Admin, UserType::TrueAdmin];
        let allowed = [
            [false, false, false],
            [true, false, false],
            [true, true, false],
        ];
        for (i, creator) in roles.iter().enumerate() {
            for (j, target) in roles.iter().enumerate() {
                assert_eq!(creator > target, allowed[i][j]);
            }
        }
        assert_eq!(
            serde_json::to_string(&UserType::TrueAdmin).unwrap(),
            "\"true_admin\""
        );
    }
    use jsonwebtoken::{
        Algorithm, DecodingKey, EncodingKey, Header, encode, get_current_timestamp,
    };

    fn test_state() -> AppState {
        let secret = b"test-only-secret-with-at-least-32-bytes";
        AppState {
            pool: PgPool::connect_lazy("postgres://localhost/test").unwrap(),
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        }
    }

    #[tokio::test]
    async fn accepts_valid_token_and_rejects_wrong_signature() {
        let state = test_state();
        let claims = Claims {
            sub: "42".into(),
            exp: get_current_timestamp() + 900,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &state.encoding_key).unwrap();
        assert_eq!(verify_user_token(&token, &state).unwrap().sub, "42");

        let wrong_token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(b"different-test-only-signing-secret"),
        )
        .unwrap();
        assert!(verify_user_token(&wrong_token, &state).is_err());
    }

    #[tokio::test]
    async fn rejects_expired_malformed_and_wrong_algorithm_tokens() {
        let state = test_state();
        let claims = Claims {
            sub: "42".into(),
            exp: get_current_timestamp() - 1,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &state.encoding_key).unwrap();
        assert!(verify_user_token(&token, &state).is_err());
        assert!(verify_user_token("invalid token", &state).is_err());

        let claims = Claims {
            exp: get_current_timestamp() + 900,
            ..claims
        };
        let token = encode(&Header::new(Algorithm::HS384), &claims, &state.encoding_key).unwrap();
        assert!(verify_user_token(&token, &state).is_err());
    }
}
