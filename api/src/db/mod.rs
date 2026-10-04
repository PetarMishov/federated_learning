mod auth;
pub mod notifications;
pub mod organizations;
pub mod types;
pub mod users;

use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{env, error::Error};

pub fn connection_options() -> Result<PgConnectOptions, Box<dyn Error>> {
    let username = env::var("POSTGRES_USER")?;
    let password = env::var("POSTGRES_PASSWORD")?;
    let database = env::var("POSTGRES_DB")?;
    let port = env::var("POSTGRES_PORT")?.parse::<u16>()?;

    Ok(PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username(&username)
        .password(&password)
        .database(&database))
}

pub async fn create_pool(options: PgConnectOptions) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
}
