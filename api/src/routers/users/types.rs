use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct VerifyLoginRequest {
    pub username: String,
    pub password: String,
}
