use crate::{
    auth::authenticated_claims,
    connectors::gitlab::authorization::{
        AuthorizationError, ConnectionResponse, GitlabAuthorization,
    },
    db::connectors::save_gitlab_connection,
    state::AppState,
};
use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationRequest {
    token: String,
}

impl AuthorizationRequest {
    fn validated_token(&self) -> Result<&str, (StatusCode, &'static str)> {
        let token = self.token.trim();
        if token.is_empty()
            || token.len() > 4096
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "A nonempty GitLab access token is required.",
            ));
        }
        Ok(token)
    }
}

pub async fn authorize_request(
    State(state): State<AppState>,
    Extension(gitlab): Extension<Option<Arc<GitlabAuthorization>>>,
    headers: HeaderMap,
    Json(request): Json<AuthorizationRequest>,
) -> Result<(HeaderMap, Json<ConnectionResponse>), (StatusCode, &'static str)> {
    let (user_id, claims) = authenticated_claims(headers, &state).await?;
    let token = request.validated_token()?;
    let gitlab = gitlab.ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "GitLab authorization is not configured.",
    ))?;
    let connection = gitlab
        .check_connection_authorization(user_id, token)
        .await
        .map_err(api_error)?;
    let saved = save_gitlab_connection(&state.pool, user_id, &claims, &connection)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not save GitLab connection.",
            )
        })?;
    if !saved {
        return Err((StatusCode::UNAUTHORIZED, "Valid session required."));
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    Ok((headers, Json(connection.response)))
}

fn api_error(error: AuthorizationError) -> (StatusCode, &'static str) {
    match error {
        AuthorizationError::InvalidToken => (
            StatusCode::BAD_REQUEST,
            "GitLab token is invalid, expired, or revoked.",
        ),
        AuthorizationError::MissingReadPermissions => (
            StatusCode::BAD_REQUEST,
            "GitLab token requires read_api and read_repository permissions, or equivalent read access.",
        ),
        AuthorizationError::ProviderUnavailable => {
            (StatusCode::BAD_GATEWAY, "Could not verify GitLab token.")
        }
        AuthorizationError::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not authorize GitLab connection.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_tokens_and_rejects_identity_fields_in_the_request() {
        assert_eq!(
            AuthorizationRequest {
                token: "  glpat-example  ".into()
            }
            .validated_token()
            .unwrap(),
            "glpat-example"
        );
        for token in [
            "".into(),
            "   ".into(),
            "bad\ntoken".into(),
            "bad token".into(),
            "x".repeat(4097),
        ] {
            assert!(AuthorizationRequest { token }.validated_token().is_err());
        }
        assert!(
            serde_json::from_str::<AuthorizationRequest>(r#"{"token":"example","user_id":99}"#)
                .is_err()
        );
    }
}
