use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}

#[derive(Deserialize, Serialize)]
pub struct VerifyLoginRequest {
    pub username: String,
    pub password: String,
}
