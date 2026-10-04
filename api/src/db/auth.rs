use argon2::{
    Argon2,
    password_hash::{
        Error, PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};

pub fn hash(password: &str) -> Result<String, Error> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string())
}

pub fn verify(password: &str, stored_hash: &str) -> Result<bool, Error> {
    let parsed_hash = PasswordHash::new(stored_hash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(Error::Password) => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_salted_and_passwords_can_be_verified() {
        let first = hash("test password").unwrap();
        let second = hash("test password").unwrap();
        assert!(first.starts_with("$argon2id$"));
        assert_ne!(first, second);
        assert!(verify("test password", &first).unwrap());
        assert!(!verify("wrong password", &first).unwrap());
    }

    #[test]
    fn malformed_hash_is_an_error() {
        assert!(verify("test password", "invalid hash").is_err());
    }
}
