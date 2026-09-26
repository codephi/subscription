use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};

#[derive(Clone)]
pub struct CredentialVault {
    cipher: Aes256Gcm,
}

impl CredentialVault {
    pub fn from_environment() -> ApiResult<Option<Self>> {
        let Ok(encoded_key) = std::env::var("BILLING_CREDENTIAL_ENCRYPTION_KEY") else {
            return Ok(None);
        };
        let key = STANDARD.decode(&encoded_key).map_err(|_| invalid_key())?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| invalid_key())?;
        Ok(Some(Self { cipher }))
    }

    pub fn seal(
        &self,
        workspace_id: Uuid,
        connection_id: Uuid,
        purpose: &str,
        secret: &str,
    ) -> ApiResult<String> {
        let mut nonce = [0_u8; 12];
        rand::rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: secret.as_bytes(),
                    aad: &associated_data(workspace_id, connection_id, purpose),
                },
            )
            .map_err(|_| vault_error("credential_encrypt_failed"))?;
        Ok(format!(
            "v1:{}",
            STANDARD.encode([nonce.as_slice(), &ciphertext].concat())
        ))
    }

    pub fn open(
        &self,
        workspace_id: Uuid,
        connection_id: Uuid,
        purpose: &str,
        sealed: &str,
    ) -> ApiResult<String> {
        let encoded = sealed
            .strip_prefix("v1:")
            .ok_or_else(|| vault_error("credential_ciphertext_invalid"))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| vault_error("credential_ciphertext_invalid"))?;
        if bytes.len() <= 12 {
            return Err(vault_error("credential_ciphertext_invalid"));
        }
        let (nonce, ciphertext) = bytes.split_at(12);
        let nonce_bytes: [u8; 12] = nonce
            .try_into()
            .map_err(|_| vault_error("credential_ciphertext_invalid"))?;
        let plaintext = self
            .cipher
            .decrypt(
                &Nonce::from(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: &associated_data(workspace_id, connection_id, purpose),
                },
            )
            .map_err(|_| vault_error("credential_decrypt_failed"))?;
        String::from_utf8(plaintext).map_err(|_| vault_error("credential_decrypt_failed"))
    }
}

fn associated_data(workspace_id: Uuid, connection_id: Uuid, purpose: &str) -> Vec<u8> {
    format!("billing-credential:v1:{workspace_id}:{connection_id}:{purpose}").into_bytes()
}

fn invalid_key() -> ApiError {
    ApiError::service_unavailable(
        "billing_credential_key_invalid",
        "BILLING_CREDENTIAL_ENCRYPTION_KEY must be base64 encoding exactly 32 bytes",
    )
}

fn vault_error(code: &'static str) -> ApiError {
    ApiError::service_unavailable(code, "billing credential could not be securely processed")
}

#[cfg(test)]
mod tests {
    use aes_gcm::{Aes256Gcm, KeyInit};
    use uuid::Uuid;

    use super::CredentialVault;

    fn vault() -> CredentialVault {
        CredentialVault {
            cipher: Aes256Gcm::new_from_slice(&[7_u8; 32]).expect("valid key"),
        }
    }

    #[test]
    fn encrypted_credentials_round_trip_only_for_their_workspace_and_purpose() {
        let vault = vault();
        let workspace_id = Uuid::new_v4();
        let connection_id = Uuid::new_v4();
        let sealed = vault
            .seal(workspace_id, connection_id, "stripe_api", "sk_test_secret")
            .expect("encrypt secret");

        assert!(!sealed.contains("sk_test_secret"));
        assert_eq!(
            vault
                .open(workspace_id, connection_id, "stripe_api", &sealed)
                .expect("decrypt secret"),
            "sk_test_secret"
        );
        assert!(vault
            .open(Uuid::new_v4(), connection_id, "stripe_api", &sealed)
            .is_err());
        assert!(vault
            .open(workspace_id, connection_id, "stripe_webhook", &sealed)
            .is_err());
    }
}
