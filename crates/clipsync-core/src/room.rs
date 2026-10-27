use base64::{prelude::BASE64_STANDARD, Engine};
use zeroize::Zeroizing;

use crate::crypto::{
    aes_gcm_decrypt, aes_gcm_encrypt, argon2id, random_iv, sha256, CryptoError, IV_SIZE, KEY_SIZE,
    TAG_SIZE,
};
use crate::protocol::safety_number;

#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    #[error("room code must not be empty")]
    EmptyRoomCode,
    #[error("room key must be {KEY_SIZE} bytes")]
    BadKeyLength,
    #[error("argon2 failed: {0}")]
    Kdf(String),
    #[error("base64 rejected")]
    Base64,
    #[error("ciphertext too short")]
    TooShort,
    #[error("aes-gcm rejected the frame")]
    Aead(#[from] CryptoError),
}

pub struct RoomCrypto {
    key: Zeroizing<[u8; KEY_SIZE]>,
}

impl RoomCrypto {
    pub fn open(room_id: &str, password: &str) -> Result<Self, RoomError> {
        let room = room_id.trim();
        if room.is_empty() {
            return Err(RoomError::EmptyRoomCode);
        }
        let digest = sha256(room.as_bytes());
        let salt = &digest[..16];
        let input = if password.is_empty() {
            room.as_bytes().to_vec()
        } else {
            let mut bytes = room.as_bytes().to_vec();
            bytes.push(0);
            bytes.extend_from_slice(password.as_bytes());
            bytes
        };
        Self::from_key(argon2id(&input, salt).map_err(|e| RoomError::Kdf(e.to_string()))?)
    }

    pub fn restore(key: &[u8]) -> Result<Self, RoomError> {
        if key.len() != KEY_SIZE {
            return Err(RoomError::BadKeyLength);
        }
        Self::from_key(Zeroizing::new(key.to_vec()))
    }

    fn from_key(key: Zeroizing<Vec<u8>>) -> Result<Self, RoomError> {
        let mut buf = Zeroizing::new([0u8; KEY_SIZE]);
        buf.copy_from_slice(&key);
        Ok(Self { key: buf })
    }

    pub fn key_material(&self) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.key[..].to_vec())
    }

    pub fn safety_number(&self) -> String {
        safety_number(&self.key[..])
    }

    pub fn encrypt(&self, plaintext: &[u8], aad: &[u8]) -> Result<String, RoomError> {
        Ok(BASE64_STANDARD.encode(self.encrypt_bytes(plaintext, aad)?))
    }

    pub fn encrypt_bytes(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, RoomError> {
        self.encrypt_bytes_with_iv(&random_iv(), plaintext, aad)
    }

    pub fn encrypt_bytes_with_iv(
        &self,
        iv: &[u8],
        plaintext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, RoomError> {
        if iv.len() != IV_SIZE {
            return Err(RoomError::TooShort);
        }
        let body = aes_gcm_encrypt(&self.key[..], iv, aad, plaintext)?;
        let mut out = iv.to_vec();
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn decrypt(&self, wire: &str, aad: &[u8]) -> Result<Vec<u8>, RoomError> {
        let raw = BASE64_STANDARD
            .decode(wire)
            .map_err(|_| RoomError::Base64)?;
        self.decrypt_bytes(&raw, aad)
    }

    pub fn decrypt_bytes(&self, raw: &[u8], aad: &[u8]) -> Result<Vec<u8>, RoomError> {
        if raw.len() < IV_SIZE + TAG_SIZE {
            return Err(RoomError::TooShort);
        }
        Ok(aes_gcm_decrypt(
            &self.key[..],
            &raw[..IV_SIZE],
            aad,
            &raw[IV_SIZE..],
        )?)
    }
}
