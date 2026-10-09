mod auth;
mod connectors;
mod db;
pub mod git;
mod routers;
mod snapshots;
mod state;

use axum::{Extension, Router};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env"))?;
    let secret = std::env::var("WEBTOKEN_SECRET")
        .map_err(|_| "WEBTOKEN_SECRET must be set before starting the API")?;
    let encoding_key = state::signing_key(&secret)?;
    let decoding_key = state::decoding_key(&secret)?;
    let git = git::GitClient::from_env()?;
    let github = connectors::github::authorization::GithubAuthorization::from_env()?.map(Arc::new);
    let gitlab = connectors::gitlab::authorization::GitlabAuthorization::from_env()?.map(Arc::new);
    let repositories = connectors::repositories::RepositoryClient::from_env()?.map(Arc::new);
    let options = db::connection_options()?;
    let pool = db::create_pool(options).await?;
    let app_state = state::AppState {
        pool: pool.clone(),
        encoding_key,
        decoding_key,
        git,
    };
    eprintln!(
        "Private Git repositories: {}",
        app_state.git.repository_root().display()
    );

    let router: Router = Router::new()
        .merge(routers::users_router())
        .merge(routers::organizations_router())
        .merge(routers::projects_router())
        .merge(routers::connectors_router())
        .fallback(not_found)
        .layer(Extension(gitlab))
        .layer(Extension(github))
        .layer(Extension(repositories))
        .with_state(app_state);

    let listener: TcpListener = TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, router).await?;

    pool.close().await;
    Ok(())
}

async fn not_found() -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::NOT_FOUND, "not found")
}
