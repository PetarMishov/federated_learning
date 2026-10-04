use super::{auth, types::DBError};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::state::AppState;

#[derive(Serialize)]
pub struct UserData {
    pub id: i64,
    pub email: String,
    pub username: String,
}

#[derive(Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

pub async fn verify_user_password(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> Result<Option<i64>, DBError> {
    let record = sqlx::query_as::<_, (i64, String)>(
        "SELECT id::bigint, password_hash FROM users WHERE username = $1",
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

pub async fn get_user_by_id(pool: &PgPool, id: i64) -> Result<UserData, DBError> {
    let (id, email, username) = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT id, email, username FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(UserData {
        id,
        email,
        username,
    })
}
