use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};

pub struct TokenCipher(Aes256Gcm);

impl TokenCipher {
    pub fn new(encoded_key: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let key = STANDARD.decode(encoded_key)?;
        Ok(Self(Aes256Gcm::new_from_slice(&key).map_err(
            |_| "CONNECTOR_TOKEN_KEY must decode to exactly 32 bytes",
        )?))
    }

    pub fn encrypt(&self, provider: &str, user_id: i32, token: &str) -> Result<String, ()> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let associated = format!("{provider}:{user_id}:access");
        let ciphertext = self
            .0
            .encrypt(
                &nonce,
                Payload {
                    msg: token.as_bytes(),
                    aad: associated.as_bytes(),
                },
            )
            .map_err(|_| ())?;
        let mut bytes = nonce.to_vec();
        bytes.extend(ciphertext);
        Ok(format!("v1:{}", STANDARD.encode(bytes)))
    }

    pub fn decrypt(&self, provider: &str, user_id: i32, encoded: &str) -> Result<String, ()> {
        let bytes = STANDARD
            .decode(encoded.strip_prefix("v1:").ok_or(())?)
            .map_err(|_| ())?;
        let nonce = Nonce::from(<[u8; 12]>::try_from(bytes.get(..12).ok_or(())?).map_err(|_| ())?);
        let associated = format!("{provider}:{user_id}:access");
        let plaintext = self
            .0
            .decrypt(
                &nonce,
                Payload {
                    msg: &bytes[12..],
                    aad: associated.as_bytes(),
                },
            )
            .map_err(|_| ())?;
        String::from_utf8(plaintext).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_only_for_the_correct_provider_user_and_key() {
        let cipher = TokenCipher::new(&STANDARD.encode([7; 32])).unwrap();
        for provider in ["github", "gitlab"] {
            let token = cipher.encrypt(provider, 12, "secret-token").unwrap();
            assert_ne!(token, cipher.encrypt(provider, 12, "secret-token").unwrap());
            assert_eq!(
                cipher.decrypt(provider, 12, &token).unwrap(),
                "secret-token"
            );
            assert!(cipher.decrypt(provider, 13, &token).is_err());
            assert!(cipher.decrypt("other-provider", 12, &token).is_err());
            assert!(
                TokenCipher::new(&STANDARD.encode([8; 32]))
                    .unwrap()
                    .decrypt(provider, 12, &token)
                    .is_err()
            );
        }
        for token in ["", "v2:invalid", "v1:aA==", "v1:not-base64"] {
            assert!(cipher.decrypt("gitlab", 12, token).is_err());
        }
    }
}
