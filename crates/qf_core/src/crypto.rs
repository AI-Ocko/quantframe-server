use std::sync::OnceLock;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use utils::{get_location, Error};

pub struct SecretKey([u8; 32]);

static KEY: OnceLock<Option<SecretKey>> = OnceLock::new();

impl SecretKey {
    pub fn from_hex(hex_key: &str) -> Result<Self, Error> {
        let bytes = hex::decode(hex_key.trim()).map_err(|e| {
            Error::new("Crypto:Key", format!("Secret key is not valid hex: {}", e), get_location!())
        })?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| {
            Error::new("Crypto:Key", "Secret key must be 32 bytes (64 hex characters)", get_location!())
        })?;
        Ok(Self(key))
    }

    fn cipher(&self) -> Result<Aes256Gcm, Error> {
        Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| Error::new("Crypto:Cipher", format!("{:?}", e), get_location!()))
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce)
            .map_err(|e| Error::new("Crypto:Encrypt", format!("{:?}", e), get_location!()))?;
        let ciphertext = self
            .cipher()?
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|e| Error::new("Crypto:Encrypt", format!("{:?}", e), get_location!()))?;
        Ok((ciphertext, nonce.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, Error> {
        if nonce.len() != 12 {
            return Err(Error::new("Crypto:Decrypt", "Nonce must be 12 bytes", get_location!()));
        }
        self.cipher()?
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| {
                Error::new(
                    "Crypto:Decrypt",
                    "Failed to decrypt; the secret key does not match the stored data",
                    get_location!(),
                )
            })
    }
}

pub fn init_key(key: Option<SecretKey>) {
    let _ = KEY.set(key);
}

pub fn key() -> Result<&'static SecretKey, Error> {
    KEY.get().and_then(|k| k.as_ref()).ok_or_else(|| {
        Error::new(
            "Crypto:Key",
            "No secret key configured (QF_SECRET_KEY_FILE); warframe.market sign-in is disabled",
            get_location!(),
        )
    })
}

/// Reads the `exp` claim of a JWT. Accepts an optional `JWT ` or `Bearer ` prefix.
pub fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let token = token.trim_start_matches("JWT ").trim_start_matches("Bearer ");
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    DateTime::from_timestamp(claims.get("exp")?.as_i64()?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn encrypt_then_decrypt_roundtrips() {
        let key = SecretKey::from_hex(KEY).unwrap();
        let (ct, nonce) = key.encrypt(b"jwt-token").unwrap();
        assert_ne!(ct, b"jwt-token");
        assert_eq!(key.decrypt(&ct, &nonce).unwrap(), b"jwt-token");
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let key = SecretKey::from_hex(KEY).unwrap();
        let other = SecretKey::from_hex(&"ab".repeat(32)).unwrap();
        let (ct, nonce) = key.encrypt(b"jwt-token").unwrap();
        assert!(other.decrypt(&ct, &nonce).is_err());
    }

    #[test]
    fn short_key_is_rejected() {
        assert!(SecretKey::from_hex("abcd").is_err());
    }

    #[test]
    fn jwt_expiry_reads_exp_claim_with_optional_prefix() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"exp":1794614400,"sub":"x"}"#);
        let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
        assert_eq!(jwt_expiry(&token).unwrap().timestamp(), 1794614400);
        assert_eq!(jwt_expiry(&format!("JWT {token}")).unwrap().timestamp(), 1794614400);
        assert!(jwt_expiry("not-a-jwt").is_none());
    }
}
