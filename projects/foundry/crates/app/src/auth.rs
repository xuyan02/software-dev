use crate::error::ServiceError;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn hash_password(password: &str) -> Result<String, ServiceError> {
    if password.is_empty() {
        return Err(ServiceError::Validation(
            "password must not be empty".to_string(),
        ));
    }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| ServiceError::Internal(format!("password hashing failed: {error}")))
}

pub fn verify_password(password: &str, password_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(password_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Two UUIDv7 values give enough randomness for a session token. The token is
/// returned once and only its hash is stored.
pub fn new_session_token() -> String {
    format!("{}{}", Uuid::now_v7().simple(), Uuid::now_v7().simple())
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn short_password_is_accepted() {
        let hash = hash_password("short").unwrap();
        assert!(verify_password("short", &hash));
    }

    #[test]
    fn empty_password_is_rejected() {
        assert!(hash_password("").is_err());
    }

    #[test]
    fn token_hash_is_stable_and_not_the_token() {
        let token = new_session_token();
        assert_eq!(hash_token(&token), hash_token(&token));
        assert_ne!(hash_token(&token), token);
    }
}
