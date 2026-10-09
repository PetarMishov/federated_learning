mod get_branches;
mod get_repositories;
mod github;
mod gitlab;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn connectors_router() -> Router<AppState> {
    Router::new()
        .merge(github::github_router())
        .route(
            "/connectors/gitlab/authorize",
            post(gitlab::authorization::authorize_request),
        )
        .route(
            "/connectors/{provider}/repositories",
            get(get_repositories::get_repositories_request),
        )
        .route(
            "/connectors/{provider}/branches",
            get(get_branches::get_branches_request),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::{
        github::authorization::GithubAuthorization, gitlab::authorization::GitlabAuthorization,
        repositories::RepositoryClient,
    };
    use axum::Extension;
    use jsonwebtoken::{DecodingKey, EncodingKey};
    use sqlx::PgPool;
    use std::sync::Arc;

    #[tokio::test]
    async fn gitlab_routes_require_local_authentication_and_remove_oauth_callback() {
        let secret = b"connector-test-secret-at-least-thirty-two-bytes";
        let app = connectors_router()
            .layer(Extension(None::<Arc<GitlabAuthorization>>))
            .layer(Extension(None::<Arc<GithubAuthorization>>))
            .layer(Extension(None::<Arc<RepositoryClient>>))
            .with_state(AppState {
                pool: PgPool::connect_lazy("postgres://localhost/unused").unwrap(),
                encoding_key: EncodingKey::from_secret(secret),
                decoding_key: DecodingKey::from_secret(secret),
                git: crate::git::GitClient::test_config(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        for provider in ["github", "gitlab"] {
            for token in [None, Some("forged")] {
                let mut request = client
                    .post(format!("http://{address}/connectors/{provider}/authorize"))
                    .json(&serde_json::json!({"token": "submitted-token"}));
                if let Some(token) = token {
                    request = request.bearer_auth(token);
                }
                assert_eq!(
                    request.send().await.unwrap().status(),
                    axum::http::StatusCode::UNAUTHORIZED
                );
            }
        }
        for provider in ["github", "gitlab"] {
            let response = client
                .get(format!(
                    "http://{address}/connectors/{provider}/repositories?page=1"
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        }
        for provider in ["github", "gitlab"] {
            let response = client
                .get(format!(
                    "http://{address}/connectors/{provider}/branches?repository=42&page=1"
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::UNAUTHORIZED);
        }
        let callback = client
            .get(format!(
                "http://{address}/connectors/gitlab/callback?state=forged&code=forged"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(callback.status(), axum::http::StatusCode::NOT_FOUND);
        task.abort();
    }
}
