use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
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

pub fn aes_gcm_encrypt(key: &[u8], iv: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, ()> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ())?;
    let nonce = aes_gcm::Nonce::<Aes256Gcm>::from_slice(iv);
    cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| ())
}

pub fn aes_gcm_decrypt(key: &[u8], iv: &[u8], aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, ()> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| ())?;
    let nonce = aes_gcm::Nonce::<Aes256Gcm>::from_slice(iv);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| ())
}
