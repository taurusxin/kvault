use crate::error::{AppError, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX_FILE_SIZE: usize = 64 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"KVAULT\x02\0";
const HEADER_SIZE: usize = 132;
const WRAP_PREFIX_SIZE: usize = 84;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub vault_id: String,
    pub epoch: u64,
    pub memory: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: String,
    pub wrap_nonce: String,
    pub wrapped_key: String,
}

#[derive(Clone)]
pub struct Keys {
    pub header: Header,
    /// Random DEK. The password-derived KEK is never kept in the session.
    pub key: Zeroizing<[u8; 32]>,
}

fn invalid() -> AppError {
    AppError::new(
        "invalid_vault",
        "保险库格式无效或文件损坏；仅支持新的 v2 二进制格式",
    )
}

fn decode<const N: usize>(text: &str) -> Result<[u8; N]> {
    STANDARD
        .decode(text)
        .map_err(|_| invalid())?
        .try_into()
        .map_err(|_| invalid())
}

fn encode_header(h: &Header) -> Result<Vec<u8>> {
    if h.memory != 65536 || h.iterations != 3 || h.parallelism != 1 || h.epoch == 0 {
        return Err(invalid());
    }
    let uuid = uuid::Uuid::parse_str(&h.vault_id).map_err(|_| invalid())?;
    let mut bytes = Vec::with_capacity(HEADER_SIZE);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(uuid.as_bytes());
    bytes.extend_from_slice(&h.epoch.to_le_bytes());
    bytes.extend_from_slice(&h.memory.to_le_bytes());
    bytes.extend_from_slice(&h.iterations.to_le_bytes());
    bytes.extend_from_slice(&h.parallelism.to_le_bytes());
    bytes.extend_from_slice(&decode::<16>(&h.salt)?);
    bytes.extend_from_slice(&decode::<24>(&h.wrap_nonce)?);
    bytes.extend_from_slice(&decode::<48>(&h.wrapped_key)?);
    Ok(bytes)
}

fn wrapping_aad(h: &Header) -> Result<Vec<u8>> {
    let mut aad = b"kvault/v2/password-wrap\0".to_vec();
    aad.extend_from_slice(&encode_header(h)?[..WRAP_PREFIX_SIZE]);
    Ok(aad)
}

fn derive_kek(password: &str, header: &Header) -> Result<Zeroizing<[u8; 32]>> {
    encode_header(header)?; // Bound KDF parameters before allocating memory.
    let params = Params::new(
        header.memory,
        header.iterations,
        header.parallelism,
        Some(32),
    )
    .map_err(|_| invalid())?;
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(
            password.as_bytes(),
            &decode::<16>(&header.salt)?,
            key.as_mut(),
        )
        .map_err(|_| invalid())?;
    Ok(key)
}

impl Keys {
    pub fn new(password: &str, vault_id: String, epoch: u64) -> Result<Self> {
        if password.chars().count() < 8 {
            return Err(AppError::invalid("主密码至少需要 8 个字符"));
        }
        let mut salt = [0; 16];
        let mut wrap_nonce = [0; 24];
        let mut key = Zeroizing::new([0; 32]);
        OsRng.fill_bytes(&mut salt);
        OsRng.fill_bytes(&mut wrap_nonce);
        OsRng.fill_bytes(key.as_mut());
        let mut header = Header {
            vault_id,
            epoch,
            memory: 65536,
            iterations: 3,
            parallelism: 1,
            salt: STANDARD.encode(salt),
            wrap_nonce: STANDARD.encode(wrap_nonce),
            wrapped_key: STANDARD.encode([0; 48]),
        };
        let kek = derive_kek(password, &header)?;
        let cipher = XChaCha20Poly1305::new_from_slice(kek.as_ref()).map_err(|_| invalid())?;
        header.wrapped_key = STANDARD.encode(
            cipher
                .encrypt(
                    XNonce::from_slice(&wrap_nonce),
                    Payload {
                        msg: key.as_ref(),
                        aad: &wrapping_aad(&header)?,
                    },
                )
                .map_err(|_| invalid())?,
        );
        Ok(Self { header, key })
    }

    pub fn derive(password: &str, header: Header) -> Result<Self> {
        let kek = derive_kek(password, &header)?;
        let cipher = XChaCha20Poly1305::new_from_slice(kek.as_ref()).map_err(|_| invalid())?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    XNonce::from_slice(&decode::<24>(&header.wrap_nonce)?),
                    Payload {
                        msg: &decode::<48>(&header.wrapped_key)?,
                        aad: &wrapping_aad(&header)?,
                    },
                )
                .map_err(|_| AppError::new("decrypt", "密码不正确，或保险库内容已损坏"))?,
        );
        Self::from_device(header, &plain)
    }

    pub fn from_device(header: Header, secret: &[u8]) -> Result<Self> {
        encode_header(&header)?;
        let mut key = Zeroizing::new([0; 32]);
        if secret.len() != 32 {
            return Err(invalid());
        }
        key.copy_from_slice(secret);
        Ok(Self { header, key })
    }
}

pub fn fingerprint(h: &Header) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(encode_header(h)?)))
}

fn purpose_id(purpose: &str) -> Result<u8> {
    match purpose {
        "local" => Ok(1),
        "snapshot" => Ok(2),
        "password-check" => Ok(3),
        _ => Err(invalid()),
    }
}

pub fn header(bytes: &[u8], purpose: &str) -> Result<Header> {
    if bytes.len() > MAX_FILE_SIZE
        || bytes.len() < HEADER_SIZE + 1 + 24 + 16
        || &bytes[..8] != MAGIC
        || bytes[HEADER_SIZE] != purpose_id(purpose)?
    {
        return Err(invalid());
    }
    let h = Header {
        vault_id: uuid::Uuid::from_slice(&bytes[8..24])
            .map_err(|_| invalid())?
            .to_string(),
        epoch: u64::from_le_bytes(bytes[24..32].try_into().map_err(|_| invalid())?),
        memory: u32::from_le_bytes(bytes[32..36].try_into().map_err(|_| invalid())?),
        iterations: u32::from_le_bytes(bytes[36..40].try_into().map_err(|_| invalid())?),
        parallelism: u32::from_le_bytes(bytes[40..44].try_into().map_err(|_| invalid())?),
        salt: STANDARD.encode(&bytes[44..60]),
        wrap_nonce: STANDARD.encode(&bytes[60..84]),
        wrapped_key: STANDARD.encode(&bytes[84..132]),
    };
    encode_header(&h)?;
    Ok(h)
}

pub fn seal<T: Serialize>(value: &T, keys: &Keys, purpose: &str) -> Result<Vec<u8>> {
    let mut plain = Zeroizing::new(Vec::new());
    ciborium::into_writer(value, &mut *plain).map_err(AppError::storage)?;
    if plain.len() > MAX_FILE_SIZE - HEADER_SIZE - 41 {
        return Err(invalid());
    }
    let mut bytes = encode_header(&keys.header)?;
    bytes.push(purpose_id(purpose)?);
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let cipher = XChaCha20Poly1305::new_from_slice(keys.key.as_ref()).map_err(|_| invalid())?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: &bytes,
            },
        )
        .map_err(|_| invalid())?;
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&ciphertext);
    Ok(bytes)
}

pub fn open<T: DeserializeOwned>(bytes: &[u8], keys: &Keys, purpose: &str) -> Result<T> {
    if header(bytes, purpose)? != keys.header {
        return Err(AppError::new(
            "key_changed",
            "保险库密钥已变更，请输入新主密码",
        ));
    }
    let nonce_start = HEADER_SIZE + 1;
    let cipher = XChaCha20Poly1305::new_from_slice(keys.key.as_ref()).map_err(|_| invalid())?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&bytes[nonce_start..nonce_start + 24]),
                Payload {
                    msg: &bytes[nonce_start + 24..],
                    aad: &bytes[..nonce_start],
                },
            )
            .map_err(|_| AppError::new("decrypt", "无法认证保险库内容，文件可能已损坏"))?,
    );
    let mut reader = std::io::Cursor::new(plain.as_slice());
    let value = ciborium::from_reader(&mut reader).map_err(|_| invalid())?;
    if reader.position() as usize != plain.len() {
        return Err(invalid());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_binary_roundtrip_and_wrapped_random_dek() {
        let keys = Keys::new("test-password", uuid::Uuid::new_v4().to_string(), 1).unwrap();
        let a = seal(&"secret value", &keys, "snapshot").unwrap();
        let b = seal(&"secret value", &keys, "snapshot").unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with(MAGIC));
        assert!(!String::from_utf8_lossy(&a).contains("secret value"));
        let unlocked = Keys::derive("test-password", header(&a, "snapshot").unwrap()).unwrap();
        assert_eq!(
            open::<String>(&a, &unlocked, "snapshot").unwrap(),
            "secret value"
        );
        assert!(Keys::derive("wrong-password", keys.header.clone()).is_err());
        let device = Keys::from_device(keys.header.clone(), keys.key.as_ref()).unwrap();
        assert_eq!(
            open::<String>(&a, &device, "snapshot").unwrap(),
            "secret value"
        );
        for offset in [8, 24, 44, 60, 84, HEADER_SIZE, HEADER_SIZE + 1, a.len() - 1] {
            let mut tampered = a.clone();
            tampered[offset] ^= 1;
            assert!(open::<String>(&tampered, &keys, "snapshot").is_err());
            if offset < HEADER_SIZE
                && let Ok(h) = header(&tampered, "snapshot")
            {
                assert!(Keys::derive("test-password", h).is_err());
            }
        }
        assert!(open::<String>(&a, &keys, "local").is_err());
        assert!(header(b"{\"header\":{\"version\":1}}", "local").is_err());
    }
    #[test]
    fn reject_unbounded_kdf_before_derivation() {
        let keys = Keys::new("test-password", uuid::Uuid::new_v4().to_string(), 1).unwrap();
        let mut h = keys.header;
        h.memory = u32::MAX;
        assert!(Keys::derive("test-password", h).is_err());
    }
}
