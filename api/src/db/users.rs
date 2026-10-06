use super::{
    auth,
    types::{DBError, Organization, OrganizationList},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::state::AppState;

#[derive(Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
    pub jti: String,
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
    validation.set_required_spec_claims(&["sub", "exp", "jti"]);
    validation.leeway = 0;
    let data = jsonwebtoken::decode::<Claims>(token, &app_state.decoding_key, &validation)?;
    Ok(data.claims)
}

pub async fn get_user_organizations(
    pool: &PgPool,
    user_id: i32,
) -> Result<OrganizationList, DBError> {
    let rows = sqlx::query_as::<_, (i32, String, i32)>(
        "SELECT o.id, o.name, o.owner_user_id
         FROM organizations AS o
         JOIN user_organization AS membership ON membership.org_id = o.id
         WHERE membership.user_id = $1
         ORDER BY o.name, o.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(OrganizationList {
        organizations: rows
            .into_iter()
            .map(|(id, name, owner_user_id)| Organization {
                id,
                name,
                owner_user_id,
            })
            .collect(),
    })
}
