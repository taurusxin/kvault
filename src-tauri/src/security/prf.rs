//! Windows device-unlock protocol, independent of Win32 for deterministic tests.
//! Credential Manager holds only this bounded envelope; PRF results stay in RAM.
use super::SecretStore;
use crate::error::{AppError, Result};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const RP_ID: &str = "kvault.taurusxin.com";
pub const MIN_API: u32 = 8;
pub const MAX_CREDENTIAL: usize = 2048;
const MAGIC: &[u8; 8] = b"KVPRF\x01\0\0";
const SUFFIX_SIZE: usize = 32 + 12 + 48;

pub fn unsupported(message: impl Into<String>) -> AppError {
    AppError::new("system_auth_unavailable", message)
}
fn invalid() -> AppError {
    AppError::new(
        "system_auth",
        "Windows 系统解锁凭据无效或已失效，请使用主密码并重新启用",
    )
}
pub fn check_capabilities(api: u32, platform: bool) -> Result<()> {
    if api < MIN_API {
        return Err(unsupported(format!(
            "系统解锁需要 WebAuthn API ≥ {MIN_API}，当前为 {api}；请更新 Windows 或使用主密码"
        )));
    }
    if !platform {
        return Err(unsupported(
            "未检测到可进行用户验证的本机认证器，请先在 Windows 设置中启用 Hello",
        ));
    }
    Ok(())
}

pub struct AuthResult {
    pub credential: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub prf: Option<Zeroizing<[u8; 32]>>,
}
pub struct Registration {
    pub result: AuthResult,
    pub prf_enabled: bool,
    pub resident: bool,
    pub internal: bool,
}
pub trait Authenticator {
    fn capabilities(&self) -> Result<(u32, bool)>;
    fn register(&self, account: &str, salt: &[u8; 32]) -> Result<Registration>;
    fn evaluate(&self, credential: &[u8], salt: &[u8; 32]) -> Result<AuthResult>;
    fn delete(&self, credential: &[u8]) -> Result<()>;
}
fn validate_auth(result: &AuthResult, credential: &[u8]) -> Result<()> {
    if result.credential != credential || result.authenticator_data.len() < 37 {
        return Err(invalid());
    }
    let data = &result.authenticator_data;
    // Require user presence + verification, forbid backup-eligible/synced keys.
    if data[..32] != Sha256::digest(RP_ID.as_bytes())[..]
        || data[32] & 0x05 != 0x05
        || data[32] & 0x18 != 0
    {
        return Err(unsupported(
            "认证器未满足本机绑定及用户验证要求，请使用主密码",
        ));
    }
    if result.prf.is_none() {
        return Err(unsupported(
            "该 Windows Hello 认证器未返回 PRF，不能用于加密解锁；请更新系统或使用主密码",
        ));
    }
    Ok(())
}

struct Envelope {
    credential: Vec<u8>,
    salt: [u8; 32],
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}
impl Envelope {
    fn header(&self) -> Result<Vec<u8>> {
        if self.credential.is_empty() || self.credential.len() > MAX_CREDENTIAL {
            return Err(invalid());
        }
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(self.credential.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&self.credential);
        bytes.extend_from_slice(&self.salt);
        bytes.extend_from_slice(&self.nonce);
        Ok(bytes)
    }
    fn aad(&self, account: &str) -> Result<Vec<u8>> {
        if account.is_empty() || account.len() > 1024 {
            return Err(invalid());
        }
        let mut aad = b"kvault/windows/prf/aes-256-gcm/v1\0".to_vec();
        aad.extend_from_slice(RP_ID.as_bytes());
        aad.push(0);
        aad.extend_from_slice(&(account.len() as u32).to_le_bytes());
        aad.extend_from_slice(account.as_bytes());
        aad.extend_from_slice(&self.header()?);
        Ok(aad)
    }
    fn encode(&self) -> Result<Vec<u8>> {
        if self.ciphertext.len() != 48 {
            return Err(invalid());
        }
        let mut bytes = self.header()?;
        bytes.extend_from_slice(&self.ciphertext);
        Ok(bytes)
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 10 + 1 + SUFFIX_SIZE
            || bytes.len() > 10 + MAX_CREDENTIAL + SUFFIX_SIZE
            || &bytes[..8] != MAGIC
        {
            return Err(invalid());
        }
        let size = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        if size == 0 || size > MAX_CREDENTIAL || bytes.len() != 10 + size + SUFFIX_SIZE {
            return Err(invalid());
        }
        let offset = 10 + size;
        Ok(Self {
            credential: bytes[10..offset].to_vec(),
            salt: bytes[offset..offset + 32]
                .try_into()
                .map_err(|_| invalid())?,
            nonce: bytes[offset + 32..offset + 44]
                .try_into()
                .map_err(|_| invalid())?,
            ciphertext: bytes[offset + 44..].to_vec(),
        })
    }
    fn open(&self, account: &str, prf: &[u8; 32]) -> Result<Zeroizing<Vec<u8>>> {
        let cipher = Aes256Gcm::new_from_slice(prf).map_err(|_| invalid())?;
        let plain = cipher
            .decrypt(
                Nonce::from_slice(&self.nonce),
                Payload {
                    msg: &self.ciphertext,
                    aad: &self.aad(account)?,
                },
            )
            .map_err(|_| invalid())?;
        Ok(Zeroizing::new(plain))
    }
}
fn entry(account: &str) -> String {
    format!("webauthn-prf:{account}")
}

pub fn protect(
    store: &impl SecretStore,
    auth: &impl Authenticator,
    account: &str,
    secret: &[u8],
) -> Result<()> {
    let (api, platform) = auth.capabilities()?;
    check_capabilities(api, platform)?; // Before any prompt or credential change.
    if secret.len() != 32 {
        return Err(invalid());
    }
    // Read before enrollment so a credential-store error doesn't create a key.
    let previous = store.read(&entry(account))?;
    let previous = previous
        .as_deref()
        .map(|bytes| Envelope::decode(bytes))
        .transpose()?;
    let mut salt = [0; 32];
    let mut nonce = [0; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    // Reuse an existing verified device credential rather than replacing it
    // before the new envelope commits. Cancellation/disk errors keep it usable.
    let (authentication, registration_ok, newly_created) = if let Some(old) = &previous {
        (auth.evaluate(&old.credential, &salt)?, true, false)
    } else {
        let registration = auth.register(account, &salt)?;
        let valid = registration.prf_enabled && registration.resident && registration.internal;
        (registration.result, valid, true)
    };
    let credential = previous
        .as_ref()
        .map(|old| old.credential.clone())
        .unwrap_or_else(|| authentication.credential.clone());
    let result = (|| {
        if !registration_ok {
            return Err(unsupported(
                "认证器不支持本机 PRF 凭据，未启用系统解锁；请使用主密码",
            ));
        }
        validate_auth(&authentication, &credential)?;
        let prf = authentication.prf.as_deref().ok_or_else(invalid)?;
        let mut envelope = Envelope {
            credential: credential.clone(),
            salt,
            nonce,
            ciphertext: Vec::new(),
        };
        let cipher = Aes256Gcm::new_from_slice(prf).map_err(|_| invalid())?;
        envelope.ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: secret,
                    aad: &envelope.aad(account)?,
                },
            )
            .map_err(|_| invalid())?;
        // A fresh assertion must reproduce the create-time PRF and unwrap DEK.
        let verified = auth.evaluate(&credential, &salt)?;
        validate_auth(&verified, &credential)?;
        let recovered = envelope.open(account, verified.prf.as_deref().ok_or_else(invalid)?)?;
        if recovered.as_slice() != secret {
            return Err(invalid());
        }
        store.write(&entry(account), &envelope.encode()?)?;
        Ok(())
    })();
    if result.is_err() && newly_created {
        let _ = auth.delete(&credential); // Keep the original failure/cancellation.
    }
    result
}
pub fn unprotect(
    store: &impl SecretStore,
    auth: &impl Authenticator,
    account: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    let (api, platform) = auth.capabilities()?;
    check_capabilities(api, platform)?;
    let bytes = store.read(&entry(account))?.ok_or_else(invalid)?;
    let envelope = Envelope::decode(&bytes)?;
    let verified = auth.evaluate(&envelope.credential, &envelope.salt)?;
    validate_auth(&verified, &envelope.credential)?;
    envelope.open(account, verified.prf.as_deref().ok_or_else(invalid)?)
}
pub fn revoke(store: &impl SecretStore, auth: &impl Authenticator, account: &str) -> Result<()> {
    if let Some(bytes) = store.read(&entry(account))? {
        let envelope = Envelope::decode(&bytes)?;
        auth.delete(&envelope.credential)?;
        store.delete(&entry(account))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::MemorySecrets;
    use std::cell::{Cell, RefCell};

    struct Fake {
        api: u32,
        platform: bool,
        flags: u8,
        supports_prf: bool,
        inconsistent_prf: bool,
        missing_output: bool,
        cancelled: Cell<bool>,
        registrations: Cell<u8>,
        evaluations: Cell<u32>,
        deleted: RefCell<Vec<Vec<u8>>>,
    }
    impl Default for Fake {
        fn default() -> Self {
            Self {
                api: MIN_API,
                platform: true,
                flags: 5,
                supports_prf: true,
                inconsistent_prf: false,
                missing_output: false,
                cancelled: Cell::new(false),
                registrations: Cell::new(0),
                evaluations: Cell::new(0),
                deleted: RefCell::default(),
            }
        }
    }
    impl Fake {
        fn result(&self, credential: Vec<u8>, salt: &[u8; 32], evaluating: bool) -> AuthResult {
            let mut data = Sha256::digest(RP_ID.as_bytes()).to_vec();
            data.extend_from_slice(&[self.flags, 0, 0, 0, 0]);
            let mut hash = Sha256::new();
            hash.update(b"public-test-PRF-secret");
            hash.update(&credential);
            hash.update(salt);
            if evaluating && self.inconsistent_prf {
                hash.update(b"different");
            }
            AuthResult {
                credential,
                authenticator_data: data,
                prf: (self.supports_prf && !self.missing_output)
                    .then(|| Zeroizing::new(hash.finalize().into())),
            }
        }
    }
    impl Authenticator for Fake {
        fn capabilities(&self) -> Result<(u32, bool)> {
            Ok((self.api, self.platform))
        }
        fn register(&self, _: &str, salt: &[u8; 32]) -> Result<Registration> {
            let id = self.registrations.get() + 1;
            self.registrations.set(id);
            Ok(Registration {
                result: self.result(vec![id; 32], salt, false),
                prf_enabled: self.supports_prf,
                resident: true,
                internal: true,
            })
        }
        fn evaluate(&self, credential: &[u8], salt: &[u8; 32]) -> Result<AuthResult> {
            self.evaluations.set(self.evaluations.get() + 1);
            if self.cancelled.get() {
                return Err(AppError::new("auth_cancelled", "测试取消"));
            }
            Ok(self.result(credential.to_vec(), salt, true))
        }
        fn delete(&self, credential: &[u8]) -> Result<()> {
            self.deleted.borrow_mut().push(credential.to_vec());
            Ok(())
        }
    }
    fn bytes(store: &MemorySecrets, account: &str) -> Vec<u8> {
        store.read(&entry(account)).unwrap().unwrap().to_vec()
    }

    #[test]
    fn preflight_rejects_unsupported_systems_without_enrollment() {
        for auth in [
            Fake {
                api: 7,
                ..Default::default()
            },
            Fake {
                platform: false,
                ..Default::default()
            },
        ] {
            let store = MemorySecrets::default();
            assert_eq!(
                protect(&store, &auth, "test", &[42; 32]).unwrap_err().code,
                "system_auth_unavailable"
            );
            assert_eq!(auth.registrations.get(), 0);
            assert_eq!(auth.evaluations.get(), 0);
            assert!(store.read(&entry("test")).unwrap().is_none());
        }
    }
    #[test]
    fn enrollment_requires_actual_prf_and_verified_device_bound_output() {
        for auth in [
            Fake {
                supports_prf: false,
                ..Default::default()
            },
            Fake {
                inconsistent_prf: true,
                ..Default::default()
            },
            Fake {
                missing_output: true,
                ..Default::default()
            },
            Fake {
                flags: 1,
                ..Default::default()
            },
            Fake {
                flags: 4,
                ..Default::default()
            },
            Fake {
                flags: 5 | 8,
                ..Default::default()
            },
            Fake {
                flags: 5 | 16,
                ..Default::default()
            },
        ] {
            let store = MemorySecrets::default();
            assert!(protect(&store, &auth, "test", &[42; 32]).is_err());
            assert!(store.read(&entry("test")).unwrap().is_none());
            assert_eq!(auth.deleted.borrow().len(), 1);
        }
    }
    #[test]
    fn roundtrip_uses_fresh_assertions_and_never_stores_dek_or_prf() {
        let auth = Fake::default();
        let store = MemorySecrets::default();
        let dek = [42; 32];
        protect(&store, &auth, "test", &dek).unwrap();
        let stored = bytes(&store, "test");
        let envelope = Envelope::decode(&stored).unwrap();
        let prf = auth
            .result(envelope.credential.clone(), &envelope.salt, false)
            .prf
            .unwrap();
        assert!(
            !stored
                .windows(32)
                .any(|bytes| bytes == dek || bytes == prf.as_ref())
        );
        assert!(stored.len() <= 2500);
        for _ in 0..2 {
            assert_eq!(unprotect(&store, &auth, "test").unwrap().as_slice(), &dek);
        }
        assert_eq!(auth.evaluations.get(), 3);
        protect(&store, &auth, "test", &dek).unwrap();
        let next = Envelope::decode(&bytes(&store, "test")).unwrap();
        assert_ne!(envelope.salt, next.salt);
        assert_ne!(envelope.nonce, next.nonce);
        assert_eq!(envelope.credential, next.credential);
        assert!(auth.deleted.borrow().is_empty());
    }
    #[test]
    fn rejects_tampering_wrong_account_and_unbounded_envelopes() {
        let auth = Fake::default();
        let store = MemorySecrets::default();
        protect(&store, &auth, "test", &[42; 32]).unwrap();
        let original = bytes(&store, "test");
        for index in 0..original.len() {
            let mut tampered = original.clone();
            tampered[index] ^= 1;
            store.write(&entry("test"), &tampered).unwrap();
            assert!(unprotect(&store, &auth, "test").is_err(), "byte {index}");
        }
        store.write(&entry("other-vault"), &original).unwrap();
        assert!(unprotect(&store, &auth, "other-vault").is_err());
        for invalid in [
            vec![],
            vec![0; 256],
            vec![0; 2501],
            original[..original.len() - 1].to_vec(),
        ] {
            assert!(Envelope::decode(&invalid).is_err());
        }
    }
    #[test]
    fn cancellation_preserves_existing_authorization_and_revocation_removes_it() {
        let auth = Fake::default();
        let store = MemorySecrets::default();
        protect(&store, &auth, "test", &[42; 32]).unwrap();
        let before = bytes(&store, "test");
        auth.cancelled.set(true);
        assert_eq!(
            protect(&store, &auth, "test", &[43; 32]).unwrap_err().code,
            "auth_cancelled"
        );
        assert_eq!(bytes(&store, "test"), before);
        assert_eq!(
            unprotect(&store, &auth, "test").unwrap_err().code,
            "auth_cancelled"
        );
        auth.cancelled.set(false);
        assert_eq!(
            unprotect(&store, &auth, "test").unwrap().as_slice(),
            &[42; 32]
        );
        revoke(&store, &auth, "test").unwrap();
        assert!(unprotect(&store, &auth, "test").is_err());
        revoke(&store, &auth, "test").unwrap();
    }

    #[test]
    fn failed_credential_commit_revokes_only_the_new_registration() {
        struct ReadOnly<'a>(&'a MemorySecrets);
        impl SecretStore for ReadOnly<'_> {
            fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
                self.0.read(account)
            }
            fn write(&self, _: &str, _: &[u8]) -> Result<()> {
                Err(AppError::new("credential_store", "测试保存失败"))
            }
            fn delete(&self, account: &str) -> Result<()> {
                self.0.delete(account)
            }
            fn availability(&self) -> crate::security::SystemUnlock {
                self.0.availability()
            }
            fn protect(&self, account: &str, secret: &[u8], window: isize) -> Result<()> {
                self.0.protect(account, secret, window)
            }
            fn unprotect(&self, account: &str, window: isize) -> Result<Zeroizing<Vec<u8>>> {
                self.0.unprotect(account, window)
            }
            fn revoke(&self, account: &str) -> Result<()> {
                self.0.revoke(account)
            }
        }
        let auth = Fake::default();
        let store = MemorySecrets::default();
        protect(&store, &auth, "test", &[42; 32]).unwrap();
        let before = bytes(&store, "test");
        assert_eq!(
            protect(&ReadOnly(&store), &auth, "test", &[43; 32])
                .unwrap_err()
                .code,
            "credential_store"
        );
        assert_eq!(bytes(&store, "test"), before);
        assert!(auth.deleted.borrow().is_empty());
        assert_eq!(
            unprotect(&store, &auth, "test").unwrap().as_slice(),
            &[42; 32]
        );
        let empty = MemorySecrets::default();
        assert!(protect(&ReadOnly(&empty), &auth, "new", &[42; 32]).is_err());
        assert_eq!(auth.deleted.borrow().as_slice(), &[vec![2; 32]]);
    }
}
