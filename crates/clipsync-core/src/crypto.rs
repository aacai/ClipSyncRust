use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, Payload},
    Aes256Gcm,
};
use argon2::{Algorithm, Argon2, Params, Version};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{ARGON2_M_KIB, ARGON2_P, ARGON2_T};

pub const KEY_SIZE: usize = 32;
pub const IV_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

/// 只为算指纹就把整段文件读进 RAM 太浪费，按块喂哈希器。
pub fn sha256_hex_file(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn argon2id(password: &[u8], salt: &[u8]) -> Result<Zeroizing<Vec<u8>>, argon2::Error> {
    argon2id_with(password, salt, ARGON2_T, ARGON2_M_KIB, ARGON2_P)
}

pub fn argon2id_with(
    password: &[u8],
    salt: &[u8],
    t: u32,
    m_kib: u32,
    p: u32,
) -> Result<Zeroizing<Vec<u8>>, argon2::Error> {
    let params = Params::new(m_kib, t, p, Some(KEY_SIZE))?;
    let hasher = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new(vec![0u8; KEY_SIZE]);
    hasher.hash_password_into(password, salt, &mut out)?;
    Ok(out)
}

pub fn random_iv() -> [u8; IV_SIZE] {
    let mut iv = [0u8; IV_SIZE];
    getrandom::fill(&mut iv).expect("os random unavailable");
    iv
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("aes-256 key must be {KEY_SIZE} bytes")]
    BadKeyLength,
    #[error("nonce must be {IV_SIZE} bytes")]
    BadNonceLength,
    #[error("aes-gcm rejected the frame")]
    Aead,
}

pub fn aes_gcm_encrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::BadKeyLength)?;
    let nonce = aes_gcm::Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::try_from(iv)
        .map_err(|_| CryptoError::BadNonceLength)?;
    cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Aead)
}

pub fn aes_gcm_decrypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::BadKeyLength)?;
    let nonce = aes_gcm::Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::try_from(iv)
        .map_err(|_| CryptoError::BadNonceLength)?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Aead)
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn a_streamed_file_hash_matches_the_byte_hash() {
        // 跨过 64KB 分块边界：少喂或多喂一个字节都会露馅。
        let payload: Vec<u8> = (0..200_000).map(|index| (index % 251) as u8).collect();
        let dir = std::env::temp_dir().join(format!("clipsync-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("payload");
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(super::sha256_hex_file(&path).unwrap(), sha256_hex(&payload));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_reports_an_error_instead_of_a_digest() {
        assert!(super::sha256_hex_file(std::path::Path::new("/definitely/not/here")).is_err());
    }
}
