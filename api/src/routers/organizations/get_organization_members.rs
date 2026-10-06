use crate::{
    auth::verify_user_credentials,
    db::{organizations::get_organization_members, types::MemberList},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

pub async fn get_organization_members_request(
    State(state): State<AppState>,
    Path(org_id): Path<i32>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<MemberList>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let members = get_organization_members(&state.pool, org_id, user_id)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Could not load members."))?;
    Ok((StatusCode::OK, Json(members)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, EncodingKey};
    use sqlx::PgPool;

    #[tokio::test]
    async fn rejects_missing_credentials_before_accessing_members() {
        let secret = b"members-test-secret-at-least-32-bytes";
        let state = AppState {
            pool: PgPool::connect_lazy("postgres://localhost/unused").unwrap(),
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
        };
        let result =
            get_organization_members_request(State(state), Path(1), HeaderMap::new()).await;
        assert!(matches!(result, Err((StatusCode::UNAUTHORIZED, _))));
    }
}
