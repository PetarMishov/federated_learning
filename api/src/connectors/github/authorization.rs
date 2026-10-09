use crate::connectors::credentials::TokenCipher;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::{env, error::Error, time::Duration};

pub struct GithubAuthorization {
    client: Client,
    base_url: Url,
    cipher: TokenCipher,
}

#[derive(Deserialize)]
struct GithubUser {
    id: i64,
    login: String,
}

use crate::connectors::gitlab::authorization::{
    AuthorizationError, AuthorizedConnection, ConnectionResponse,
};

impl GithubAuthorization {
    /// Without an encryption key the connector is disabled.
    pub fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        let key = match env::var("CONNECTOR_TOKEN_KEY") {
            Ok(key) => key,
            Err(env::VarError::NotPresent) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let base = super::configured_base_url()?;
        Ok(Some(Self::new(&base, &key)?))
    }

    fn new(base: &str, key: &str) -> Result<Self, Box<dyn Error>> {
        let base_url = super::api_base_url(base)?;
        let cipher = TokenCipher::new(key)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .user_agent("federated-learning")
            .build()?;
        Ok(Self {
            client,
            base_url,
            cipher,
        })
    }

    pub async fn check_connection_authorization(
        &self,
        user_id: i32,
        token: &str,
    ) -> Result<AuthorizedConnection, AuthorizationError> {
        let response = self.get("user", token).await?;
        let granted_permissions = response
            .headers()
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        // Fine-grained tokens do not expose scopes; GitHub enforces their
        // selected repository permissions on each subsequent request.
        let expires_at = response
            .headers()
            .get("github-authentication-token-expiration")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let user: GithubUser = response
            .json()
            .await
            .map_err(|_| AuthorizationError::ProviderUnavailable)?;
        if user.id <= 0 || user.login.is_empty() {
            return Err(AuthorizationError::ProviderUnavailable);
        }
        let repositories = self.get("user/repos?per_page=1", token).await?;
        repositories
            .json::<Vec<serde_json::Value>>()
            .await
            .map_err(|_| AuthorizationError::ProviderUnavailable)?;
        Ok(AuthorizedConnection {
            response: ConnectionResponse {
                provider: "github",
                external_account_id: user.id.to_string(),
                external_username: user.login,
            },
            access_token_encrypted: self.encrypt(user_id, token)?,
            expires_at,
            granted_permissions,
        })
    }

    async fn get(&self, path: &str, token: &str) -> Result<reqwest::Response, AuthorizationError> {
        let response = self
            .client
            .get(
                self.base_url
                    .join(path)
                    .map_err(|_| AuthorizationError::Internal)?,
            )
            .bearer_auth(token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|_| AuthorizationError::ProviderUnavailable)?;
        match response.status().as_u16() {
            400 | 401 => Err(AuthorizationError::InvalidToken),
            403 if response.headers().contains_key("retry-after")
                || response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .is_some_and(|value| value == "0") =>
            {
                Err(AuthorizationError::ProviderUnavailable)
            }
            403 => Err(AuthorizationError::MissingReadPermissions),
            200..=299 => Ok(response),
            _ => Err(AuthorizationError::ProviderUnavailable),
        }
    }

    fn encrypt(&self, user_id: i32, token: &str) -> Result<String, AuthorizationError> {
        self.cipher
            .encrypt("github", user_id, token)
            .map_err(|_| AuthorizationError::Internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use base64::{Engine, engine::general_purpose::STANDARD};

    #[tokio::test]
    async fn verifies_identity_repository_access_and_encrypts_for_github() {
        for status in [
            StatusCode::OK,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let app = Router::new()
                .route(
                    "/api/v3/user",
                    get(|headers: HeaderMap| async move {
                        assert_eq!(headers["authorization"], "Bearer submitted-token");
                        assert!(headers.contains_key("user-agent"));
                        Json(serde_json::json!({"id": 42, "login": "alice"}))
                    }),
                )
                .route(
                    "/api/v3/user/repos",
                    get(move || async move { (status, Json(serde_json::json!([]))) }),
                );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}/api/v3", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let key = STANDARD.encode([7; 32]);
            let service = GithubAuthorization::new(&base, &key).unwrap();
            let result = service
                .check_connection_authorization(12, "submitted-token")
                .await;
            if status == StatusCode::OK {
                let connection = result.unwrap();
                assert_eq!(connection.response.provider, "github");
                assert_eq!(connection.response.external_username, "alice");
                assert_eq!(
                    TokenCipher::new(&key)
                        .unwrap()
                        .decrypt("github", 12, &connection.access_token_encrypted)
                        .unwrap(),
                    "submitted-token"
                );
                assert!(
                    TokenCipher::new(&key)
                        .unwrap()
                        .decrypt("gitlab", 12, &connection.access_token_encrypted)
                        .is_err()
                );
                assert!(
                    !serde_json::to_string(&connection.response)
                        .unwrap()
                        .contains("submitted-token")
                );
            } else {
                assert_eq!(
                    result.err(),
                    Some(match status {
                        StatusCode::UNAUTHORIZED => AuthorizationError::InvalidToken,
                        StatusCode::FORBIDDEN => AuthorizationError::MissingReadPermissions,
                        _ => AuthorizationError::ProviderUnavailable,
                    })
                );
            }
            task.abort();
        }
    }
}
