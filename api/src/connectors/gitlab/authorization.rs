use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{env, error::Error, time::Duration};

pub struct GitlabAuthorization {
    client: Client,
    base_url: Url,
    cipher: Aes256Gcm,
}

#[derive(Deserialize)]
struct TokenDetails {
    user_id: i64,
    active: bool,
    revoked: bool,
    scopes: Vec<String>,
    expires_at: Option<String>,
}

#[derive(Deserialize)]
struct GitlabUser {
    id: i64,
    username: String,
}

#[derive(Serialize)]
pub struct ConnectionResponse {
    pub provider: &'static str,
    pub external_account_id: String,
    pub external_username: String,
}

pub struct AuthorizedConnection {
    pub response: ConnectionResponse,
    pub access_token_encrypted: String,
    pub expires_at: Option<String>,
    pub granted_permissions: String,
}

#[derive(Debug, PartialEq)]
pub enum AuthorizationError {
    InvalidToken,
    MissingReadPermissions,
    ProviderUnavailable,
    Internal,
}

impl GitlabAuthorization {
    /// Without an encryption key the connector is disabled.
    pub fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        let key = match env::var("CONNECTOR_TOKEN_KEY") {
            Ok(key) => key,
            Err(env::VarError::NotPresent) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let base = env::var("GITLAB_BASE_URL").unwrap_or_else(|_| "https://gitlab.com".into());
        Ok(Some(Self::new(&base, &key)?))
    }

    fn new(base: &str, key: &str) -> Result<Self, Box<dyn Error>> {
        let base_url = Url::parse(base)?;
        let local_http = base_url.scheme() == "http"
            && matches!(
                base_url.host_str(),
                Some("localhost" | "127.0.0.1" | "[::1]")
            );
        if !(base_url.scheme() == "https" || local_http)
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || base_url.path() != "/"
        {
            return Err("GITLAB_BASE_URL must be an HTTPS origin (HTTP allowed on loopback), without credentials, path, query or fragment".into());
        }
        let key = STANDARD.decode(key)?;
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| "CONNECTOR_TOKEN_KEY must decode to exactly 32 bytes")?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
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
        let details: TokenDetails = self
            .get("api/v4/personal_access_tokens/self", token)
            .await?;
        if !details.active || details.revoked {
            return Err(AuthorizationError::InvalidToken);
        }
        let has = |scope: &str| details.scopes.iter().any(|granted| granted == scope);
        // Broader existing scopes are accepted, but no write scope is required.
        if !(has("api")
            || ((has("read_user") || has("read_api"))
                && (has("read_repository") || has("write_repository"))))
        {
            return Err(AuthorizationError::MissingReadPermissions);
        }
        let user: GitlabUser = self.get("api/v4/user", token).await?;
        if user.id <= 0 || user.id != details.user_id || user.username.is_empty() {
            return Err(AuthorizationError::ProviderUnavailable);
        }
        Ok(AuthorizedConnection {
            response: ConnectionResponse {
                provider: "gitlab",
                external_account_id: user.id.to_string(),
                external_username: user.username,
            },
            access_token_encrypted: self.encrypt(user_id, token)?,
            expires_at: details.expires_at,
            granted_permissions: details.scopes.join(" "),
        })
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
    ) -> Result<T, AuthorizationError> {
        let response = self
            .client
            .get(
                self.base_url
                    .join(path)
                    .map_err(|_| AuthorizationError::Internal)?,
            )
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|_| AuthorizationError::ProviderUnavailable)?;
        if matches!(response.status().as_u16(), 400 | 401 | 403) {
            return Err(AuthorizationError::InvalidToken);
        }
        if !response.status().is_success() {
            return Err(AuthorizationError::ProviderUnavailable);
        }
        response
            .json()
            .await
            .map_err(|_| AuthorizationError::ProviderUnavailable)
    }

    fn encrypt(&self, user_id: i32, token: &str) -> Result<String, AuthorizationError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let associated = format!("gitlab:{user_id}:access");
        let encrypted = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: token.as_bytes(),
                    aad: associated.as_bytes(),
                },
            )
            .map_err(|_| AuthorizationError::Internal)?;
        let mut bytes = nonce.to_vec();
        bytes.extend(encrypted);
        Ok(format!("v1:{}", STANDARD.encode(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::Nonce;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };

    #[test]
    fn encryption_is_randomized_and_bound_to_the_local_user() {
        let service =
            GitlabAuthorization::new("https://gitlab.com", &STANDARD.encode([7; 32])).unwrap();
        let first = service.encrypt(12, "secret-token").unwrap();
        assert_ne!(first, service.encrypt(12, "secret-token").unwrap());
        let mut bytes = STANDARD.decode(first.strip_prefix("v1:").unwrap()).unwrap();
        let nonce = Nonce::from(<[u8; 12]>::try_from(&bytes[..12]).unwrap());
        let decrypt = |bytes: &[u8], aad: &[u8]| {
            service.cipher.decrypt(
                &nonce,
                Payload {
                    msg: &bytes[12..],
                    aad,
                },
            )
        };
        assert_eq!(
            decrypt(&bytes, b"gitlab:12:access").unwrap(),
            b"secret-token"
        );
        assert!(decrypt(&bytes, b"gitlab:13:access").is_err());
        *bytes.last_mut().unwrap() ^= 1;
        assert!(decrypt(&bytes, b"gitlab:12:access").is_err());
    }

    #[test]
    fn rejects_unsafe_configuration() {
        for base in [
            "http://gitlab.example",
            "https://user:secret@gitlab.com",
            "https://gitlab.com/path",
            "https://gitlab.com?query=x",
        ] {
            assert!(GitlabAuthorization::new(base, &STANDARD.encode([7; 32])).is_err());
        }
        assert!(GitlabAuthorization::new("https://gitlab.com", &STANDARD.encode([7; 16])).is_err());
    }

    async fn mock_provider(
        status: StatusCode,
        scopes: Vec<&str>,
        active: bool,
        revoked: bool,
    ) -> (GitlabAuthorization, tokio::task::JoinHandle<()>) {
        let scopes: Vec<String> = scopes.into_iter().map(str::to_owned).collect();
        let app = Router::new()
            .route("/api/v4/personal_access_tokens/self", get(move |headers: HeaderMap| async move {
                assert_eq!(headers["private-token"], "submitted-token");
                (status, Json(serde_json::json!({"user_id": 42, "active": active, "revoked": revoked, "scopes": scopes, "expires_at": "2099-01-01"})))
            }))
            .route("/api/v4/user", get(|headers: HeaderMap| async move {
                assert_eq!(headers["private-token"], "submitted-token");
                Json(serde_json::json!({"id": 42, "username": "gitlab-user"}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            GitlabAuthorization::new(&base, &STANDARD.encode([7; 32])).unwrap(),
            task,
        )
    }

    #[tokio::test]
    async fn accepts_read_permissions_without_requiring_write_permissions() {
        for scopes in [
            vec!["read_user", "read_repository"],
            vec!["read_api", "read_repository"],
            vec!["api"],
        ] {
            let (service, task) = mock_provider(StatusCode::OK, scopes, true, false).await;
            let connection = service
                .check_connection_authorization(12, "submitted-token")
                .await
                .unwrap();
            assert_eq!(connection.response.external_account_id, "42");
            assert_eq!(connection.response.external_username, "gitlab-user");
            assert_eq!(connection.expires_at.as_deref(), Some("2099-01-01"));
            assert!(connection.access_token_encrypted.starts_with("v1:"));
            assert!(
                !serde_json::to_string(&connection.response)
                    .unwrap()
                    .contains("submitted-token")
            );
            task.abort();
        }
    }

    #[tokio::test]
    async fn rejects_invalid_expired_revoked_and_insufficient_tokens_and_reports_outages() {
        for (status, scopes, active, revoked, expected) in [
            (
                StatusCode::UNAUTHORIZED,
                vec!["api"],
                true,
                false,
                AuthorizationError::InvalidToken,
            ),
            (
                StatusCode::OK,
                vec!["api"],
                false,
                false,
                AuthorizationError::InvalidToken,
            ),
            (
                StatusCode::OK,
                vec!["api"],
                true,
                true,
                AuthorizationError::InvalidToken,
            ),
            (
                StatusCode::OK,
                vec!["read_user"],
                true,
                false,
                AuthorizationError::MissingReadPermissions,
            ),
            (
                StatusCode::OK,
                vec!["read_repository"],
                true,
                false,
                AuthorizationError::MissingReadPermissions,
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                vec!["api"],
                true,
                false,
                AuthorizationError::ProviderUnavailable,
            ),
        ] {
            let (service, task) = mock_provider(status, scopes, active, revoked).await;
            assert_eq!(
                service
                    .check_connection_authorization(12, "submitted-token")
                    .await
                    .err(),
                Some(expected)
            );
            task.abort();
        }
    }
}
