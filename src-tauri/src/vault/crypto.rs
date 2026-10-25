use crate::error::{AppError, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

pub const MAX_FILE_SIZE: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Header {
    pub format: String,
    pub version: u32,
    pub vault_id: String,
    pub epoch: u64,
    pub kdf: String,
    pub memory: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    header: Header,
    purpose: String,
    nonce: String,
    ciphertext: String,
}

pub struct Keys {
    pub header: Header,
    pub key: Zeroizing<[u8; 32]>,
}

impl Clone for Keys {
    fn clone(&self) -> Self {
        Self {
            header: self.header.clone(),
            key: Zeroizing::new(*self.key),
        }
    }
}

impl Keys {
    pub fn new(password: &str, vault_id: String, epoch: u64) -> Result<Self> {
        if password.chars().count() < 8 {
            return Err(AppError::invalid("主密码至少需要 8 个字符"));
        }
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        Self::derive(
            password,
            Header {
                format: "kvault".into(),
                version: 1,
                vault_id,
                epoch,
                kdf: "argon2id-v19".into(),
                memory: 65536,
                iterations: 3,
                parallelism: 1,
                salt: STANDARD.encode(salt),
            },
        )
    }

    pub fn derive(password: &str, header: Header) -> Result<Self> {
        validate_header(&header)?;
        let salt = STANDARD.decode(&header.salt).map_err(|_| invalid())?;
        let params = Params::new(
            header.memory,
            header.iterations,
            header.parallelism,
            Some(32),
        )
        .map_err(|_| invalid())?;
        let mut key = Zeroizing::new([0; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &salt, key.as_mut())
            .map_err(|_| invalid())?;
        Ok(Self { header, key })
    }
}

fn invalid() -> AppError {
    AppError::new("invalid_vault", "保险库格式无效、版本不受支持或文件已损坏")
}
fn validate_header(h: &Header) -> Result<()> {
    if h.format != "kvault"
        || h.version != 1
        || h.kdf != "argon2id-v19"
        || h.memory != 65536
        || h.iterations != 3
        || h.parallelism != 1
        || h.epoch == 0
        || uuid::Uuid::parse_str(&h.vault_id).is_err()
        || STANDARD.decode(&h.salt).map_or(true, |s| s.len() != 16)
    {
        return Err(invalid());
    }
    Ok(())
}

fn parse(bytes: &[u8], purpose: &str) -> Result<Envelope> {
    if bytes.len() > MAX_FILE_SIZE {
        return Err(AppError::invalid("保险库超过 64 MiB 文件上限"));
    }
    let env: Envelope = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    validate_header(&env.header)?;
    if env.purpose != purpose {
        return Err(invalid());
    }
    Ok(env)
}

pub fn header(bytes: &[u8], purpose: &str) -> Result<Header> {
    Ok(parse(bytes, purpose)?.header)
}

pub fn seal<T: Serialize>(value: &T, keys: &Keys, purpose: &str) -> Result<Vec<u8>> {
    let plain = Zeroizing::new(serde_json::to_vec(value).map_err(AppError::storage)?);
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let aad = serde_json::to_vec(&(&keys.header, purpose)).map_err(AppError::storage)?;
    let cipher = XChaCha20Poly1305::new_from_slice(keys.key.as_ref()).map_err(|_| invalid())?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: &aad,
            },
        )
        .map_err(|_| invalid())?;
    let bytes = serde_json::to_vec(&Envelope {
        header: keys.header.clone(),
        purpose: purpose.into(),
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(ciphertext),
    })
    .map_err(AppError::storage)?;
    if bytes.len() > MAX_FILE_SIZE {
        return Err(AppError::invalid("保险库超过 64 MiB 文件上限"));
    }
    Ok(bytes)
}

pub fn open<T: DeserializeOwned>(bytes: &[u8], keys: &Keys, purpose: &str) -> Result<T> {
    let env = parse(bytes, purpose)?;
    if env.header != keys.header {
        return Err(AppError::new(
            "key_changed",
            "保险库密钥已变更，请输入新主密码",
        ));
    }
    let nonce = STANDARD.decode(env.nonce).map_err(|_| invalid())?;
    if nonce.len() != 24 {
        return Err(invalid());
    }
    let ciphertext = STANDARD.decode(env.ciphertext).map_err(|_| invalid())?;
    let aad = serde_json::to_vec(&(&env.header, purpose)).map_err(AppError::storage)?;
    let cipher = XChaCha20Poly1305::new_from_slice(keys.key.as_ref()).map_err(|_| invalid())?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| AppError::new("decrypt", "密码不正确，或保险库内容已损坏"))?,
    );
    serde_json::from_slice(&plain).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_roundtrip_and_random_nonce() {
        let keys = Keys::new("test-password", uuid::Uuid::new_v4().to_string(), 1).unwrap();
        let a = seal(&"secret value", &keys, "snapshot").unwrap();
        let b = seal(&"secret value", &keys, "snapshot").unwrap();
        assert_ne!(a, b);
        assert!(!String::from_utf8_lossy(&a).contains("secret value"));
        assert_eq!(
            open::<String>(&a, &keys, "snapshot").unwrap(),
            "secret value"
        );
        let wrong = Keys::derive("wrong-password", keys.header.clone()).unwrap();
        assert!(open::<String>(&a, &wrong, "snapshot").is_err());
        let mut envelope: serde_json::Value = serde_json::from_slice(&a).unwrap();
        envelope["header"]["epoch"] = 2.into();
        assert!(
            open::<String>(&serde_json::to_vec(&envelope).unwrap(), &keys, "snapshot").is_err()
        );
        envelope = serde_json::from_slice(&a).unwrap();
        let mut ciphertext = STANDARD
            .decode(envelope["ciphertext"].as_str().unwrap())
            .unwrap();
        ciphertext[0] ^= 1;
        envelope["ciphertext"] = STANDARD.encode(ciphertext).into();
        assert!(
            open::<String>(&serde_json::to_vec(&envelope).unwrap(), &keys, "snapshot").is_err()
        );
        assert!(open::<String>(&a, &keys, "local").is_err());
    }
    #[test]
    fn reject_unbounded_kdf_before_derivation() {
        let mut h = Header {
            format: "kvault".into(),
            version: 1,
            vault_id: uuid::Uuid::new_v4().to_string(),
            epoch: 1,
            kdf: "argon2id-v19".into(),
            memory: u32::MAX,
            iterations: 3,
            parallelism: 1,
            salt: STANDARD.encode([0; 16]),
        };
        assert!(Keys::derive("password", h.clone()).is_err());
        h.memory = 65536;
        h.version = 2;
        assert!(Keys::derive("password", h).is_err());
    }
}
