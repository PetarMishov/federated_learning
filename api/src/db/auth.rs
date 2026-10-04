use argon2::{
    Argon2,
    password_hash::{Error, PasswordHash, PasswordVerifier},
};

pub fn verify(password: &str, stored_hash: &str) -> Result<bool, Error> {
    let parsed_hash = PasswordHash::new(stored_hash)?;
    match Argon2::default().verify_password(password.as_bytes(), &parsed_hash) {
        Ok(()) => Ok(true),
        Err(Error::Password) => Ok(false),
        Err(error) => Err(error),
    }
}
