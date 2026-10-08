use jsonwebtoken::{DecodingKey, EncodingKey};
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub encoding_key: EncodingKey,
    pub decoding_key: DecodingKey,
    pub git: crate::git::GitClient,
}

pub fn signing_key(secret: &str) -> Result<EncodingKey, std::io::Error> {
    if secret.trim().len() < 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WEBTOKEN_SECRET must contain at least 32 bytes; use a randomly generated secret",
        ));
    }
    Ok(EncodingKey::from_secret(secret.as_bytes()))
}

pub fn decoding_key(secret: &str) -> Result<DecodingKey, std::io::Error> {
    if secret.trim().len() < 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "WEBTOKEN_SECRET must contain at least 32 bytes; use a randomly generated secret",
        ));
    }
    Ok(DecodingKey::from_secret(secret.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_short_and_whitespace_secrets() {
        for secret in ["", "short", "                                "] {
            assert!(signing_key(secret).is_err());
        }
    }
}
