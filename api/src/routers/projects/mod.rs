use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

mod create_project;
mod get_project_deployments;
mod get_project_members;

pub fn projects_router() -> Router<AppState> {
    Router::new()
        .route(
            "/organizations/{org_id}/projects",
            post(create_project::create_project_request),
        )
        .route(
            "/projects/{proj_id}/deployments",
            get(get_project_deployments::get_project_deployments_request),
        )
        .route(
            "/projects/{proj_id}/members",
            get(get_project_members::get_project_members_request),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, EncodingKey};
    use sqlx::PgPool;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
    };

    #[tokio::test]
    async fn deployments_route_requires_authentication() {
        let secret = b"deployments-test-secret-at-least-32-bytes";
        let state = AppState {
            pool: PgPool::connect_lazy("postgres://localhost/unused").unwrap(),
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            git: crate::git::GitClient::test_config(),
        };
        let app = Router::new().merge(projects_router()).with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut connection = TcpStream::connect(address).await.unwrap();
        connection
            .write_all(
                b"GET /projects/1/deployments HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).await.unwrap();
        server.abort();

        assert!(response.starts_with("HTTP/1.1 401"), "{response}");
        assert!(response.contains("Valid bearer token required."));
    }
}
