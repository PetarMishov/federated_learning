mod routers;

use axum::Router;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let router: Router = Router::new().nest("/users", routers::users_router());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, router).await
}
