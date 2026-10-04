mod db;
mod routers;
mod state;

use axum::Router;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env"))?;
    let secret = std::env::var("WEBTOKEN_SECRET")
        .map_err(|_| "WEBTOKEN_SECRET must be set before starting the API")?;
    let encoding_key = state::signing_key(&secret)?;
    let decoding_key = state::decoding_key(&secret)?;
    let options = db::connection_options()?;
    let pool = db::create_pool(options).await?;

    let router: Router = Router::new()
        .nest("/users", routers::users_router())
        .fallback(not_found)
        .with_state(state::AppState {
            pool: pool.clone(),
            encoding_key: encoding_key.clone(),
            decoding_key: decoding_key.clone(),
        });

    let listener: TcpListener = TcpListener::bind("0.0.0.0:3000").await?;
    axum::serve(listener, router).await?;

    pool.close().await;
    Ok(())
}

async fn not_found() -> (axum::http::StatusCode, &'static str) {
    (axum::http::StatusCode::NOT_FOUND, "not found")
}
