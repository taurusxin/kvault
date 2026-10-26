//! Windows Hello unlock via native WebAuthn PRF and AES-256-GCM.
//! Requires API v8; there is no Passport/CNG, DPAPI or verification-only fallback.
use super::{
    SecretStore, SystemUnlock,
    prf::{self, AuthResult, Authenticator, Registration},
};
use crate::error::{AppError, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use std::{mem::size_of, ptr};
use windows_sys::{
    Win32::{Foundation::HWND, Networking::WindowsWebServices::*},
    core::{BOOL, PCWSTR, w},
};
use zeroize::{Zeroize, Zeroizing};

// windows-sys 0.61 models the v7 make-options and v6 attestation prefixes.
// Append only the fields defined by Microsoft's webauthn.h for API v8.
#[repr(C)]
#[derive(Default)]
struct MakeOptionsV8 {
    base: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS,
    prf_eval: *mut WEBAUTHN_HMAC_SECRET_SALT,
    hint_count: u32,
    hints: *mut PCWSTR,
    third_party_payment: BOOL,
}
#[repr(C)]
struct AttestationV7 {
    base: WEBAUTHN_CREDENTIAL_ATTESTATION,
    prf: *mut WEBAUTHN_HMAC_SECRET_SALT,
    third_party_payment: BOOL,
}

// Guard against accidentally appending to a changed generated SDK prefix.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(MakeOptionsV8, prf_eval) == 128);
    assert!(size_of::<MakeOptionsV8>() == 160);
    assert!(std::mem::offset_of!(AttestationV7, prf) == 144);
    assert!(size_of::<AttestationV7>() == 160);
};

fn check(status: i32, stage: &str) -> Result<()> {
    if status >= 0 {
        return Ok(());
    }
    if matches!(status as u32, 0x80090029 | 0x80070032 | 0x80004001) {
        return Err(prf::unsupported(format!(
            "本机认证器不支持所需 PRF 能力：{stage}（0x{:08X}），请更新 Windows 或使用主密码。",
            status as u32
        )));
    }
    Err(if matches!(status as u32, 0x80090036 | 0x800704c7) {
        AppError::new("auth_cancelled", "已取消 Windows Hello，可继续使用主密码")
    } else {
        AppError::new(
            "system_auth",
            format!(
                "Windows Hello 在{stage}时失败（0x{:08X}），请使用主密码。",
                status as u32
            ),
        )
    })
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn copy_bytes(data: *const u8, size: u32, max: usize) -> Result<Vec<u8>> {
    if data.is_null() || size == 0 || size as usize > max {
        return Err(prf::unsupported("WebAuthn 返回的数据无效，未启用系统解锁"));
    }
    // The pointer belongs to a successful WebAuthn result and is alive until
    // its guard is dropped; every copy is bounded before making a slice.
    Ok(unsafe { std::slice::from_raw_parts(data, size as usize) }.to_vec())
}
unsafe fn copy_prf(data: *mut WEBAUTHN_HMAC_SECRET_SALT) -> Option<Zeroizing<[u8; 32]>> {
    if data.is_null() {
        return None;
    }
    let data = unsafe { &*data };
    if data.cbFirst != 32 || data.pbFirst.is_null() {
        return None;
    }
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(unsafe { std::slice::from_raw_parts(data.pbFirst, 32) });
    Some(key)
}
unsafe fn wipe_prf(data: *mut WEBAUTHN_HMAC_SECRET_SALT) {
    if !data.is_null() {
        let data = unsafe { &*data };
        // We request a single 32-byte PRF output. Wipe native copies before
        // WebAuthNFree*; don't trust unexpected lengths for writes.
        if data.cbFirst == 32 && !data.pbFirst.is_null() {
            unsafe { std::slice::from_raw_parts_mut(data.pbFirst, 32) }.zeroize();
        }
    }
}
struct Attestation(*mut WEBAUTHN_CREDENTIAL_ATTESTATION);
impl Drop for Attestation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                if (*self.0).dwVersion >= 7 {
                    wipe_prf((*self.0.cast::<AttestationV7>()).prf);
                }
                WebAuthNFreeCredentialAttestation(self.0);
            }
        }
    }
}
struct Assertion(*mut WEBAUTHN_ASSERTION);
impl Drop for Assertion {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                if (*self.0).dwVersion >= 3 {
                    wipe_prf((*self.0).pHmacSecret);
                }
                WebAuthNFreeAssertion(self.0);
            }
        }
    }
}
fn client_data(kind: &str) -> Vec<u8> {
    let mut challenge = [0; 32];
    OsRng.fill_bytes(&mut challenge);
    format!(r#"{{"type":"webauthn.{kind}","challenge":"{}","origin":"https://{}","crossOrigin":false}}"#,
        URL_SAFE_NO_PAD.encode(challenge), prf::RP_ID).into_bytes()
}
fn client(bytes: &mut [u8]) -> WEBAUTHN_CLIENT_DATA {
    WEBAUTHN_CLIENT_DATA {
        dwVersion: 1,
        cbClientDataJSON: bytes.len() as u32,
        pbClientDataJSON: bytes.as_mut_ptr(),
        pwszHashAlgId: w!("SHA-256"),
    }
}
struct Native {
    window: isize,
}
impl Native {
    fn hwnd(&self) -> Result<HWND> {
        if self.window == 0 {
            return Err(AppError::invalid("Windows Hello 需要有效的应用窗口"));
        }
        Ok(self.window as HWND)
    }
}
impl Authenticator for Native {
    fn capabilities(&self) -> Result<(u32, bool)> {
        let api = unsafe { WebAuthNGetApiVersionNumber() };
        if api < prf::MIN_API {
            return Ok((api, false));
        }
        let mut available = 0;
        check(
            unsafe { WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable(&mut available) },
            "检查本机认证器能力",
        )?;
        Ok((api, available != 0))
    }
    fn register(&self, _account: &str, salt: &[u8; 32]) -> Result<Registration> {
        let rp_id = wide(prf::RP_ID);
        let rp = WEBAUTHN_RP_ENTITY_INFORMATION {
            dwVersion: 1,
            pwszId: rp_id.as_ptr(),
            pwszName: w!("KVault"),
            pwszIcon: ptr::null(),
        };
        // A fresh handle avoids replacing a resident credential left by an
        // interrupted enrollment. Account/vault binding is authenticated by AEAD.
        let mut user_id = [0; 32];
        OsRng.fill_bytes(&mut user_id);
        let user = WEBAUTHN_USER_ENTITY_INFORMATION {
            dwVersion: 1,
            cbId: 32,
            pbId: user_id.as_mut_ptr(),
            pwszName: w!("KVault 本机保险库"),
            pwszIcon: ptr::null(),
            pwszDisplayName: w!("KVault 系统解锁"),
        };
        let mut parameters = [
            WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                dwVersion: 1,
                pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                lAlg: -7,
            },
            WEBAUTHN_COSE_CREDENTIAL_PARAMETER {
                dwVersion: 1,
                pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
                lAlg: -257,
            },
        ];
        let parameters = WEBAUTHN_COSE_CREDENTIAL_PARAMETERS {
            cCredentialParameters: parameters.len() as u32,
            pCredentialParameters: parameters.as_mut_ptr(),
        };
        let mut raw_salt = *salt;
        let mut eval = WEBAUTHN_HMAC_SECRET_SALT {
            cbFirst: 32,
            pbFirst: raw_salt.as_mut_ptr(),
            cbSecond: 0,
            pbSecond: ptr::null_mut(),
        };
        let mut protection = WEBAUTHN_CRED_PROTECT_EXTENSION_IN {
            dwCredProtect: 3,
            bRequireCredProtect: 1,
        };
        let mut extension = WEBAUTHN_EXTENSION {
            pwszExtensionIdentifier: w!("credProtect"),
            cbExtension: size_of::<WEBAUTHN_CRED_PROTECT_EXTENSION_IN>() as u32,
            pvExtension: (&mut protection as *mut WEBAUTHN_CRED_PROTECT_EXTENSION_IN).cast(),
        };
        let options = MakeOptionsV8 {
            base: WEBAUTHN_AUTHENTICATOR_MAKE_CREDENTIAL_OPTIONS {
                dwVersion: 8,
                dwTimeoutMilliseconds: 60_000,
                Extensions: WEBAUTHN_EXTENSIONS {
                    cExtensions: 1,
                    pExtensions: &mut extension,
                },
                dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
                bRequireResidentKey: 1,
                bPreferResidentKey: 1,
                dwUserVerificationRequirement: WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED,
                dwAttestationConveyancePreference: WEBAUTHN_ATTESTATION_CONVEYANCE_PREFERENCE_NONE,
                bEnablePrf: 1,
                ..Default::default()
            },
            prf_eval: &mut eval,
            ..Default::default()
        };
        let mut bytes = client_data("create");
        let data = client(&mut bytes);
        let mut output = Attestation(ptr::null_mut());
        check(
            unsafe {
                WebAuthNAuthenticatorMakeCredential(
                    self.hwnd()?,
                    &rp,
                    &user,
                    &parameters,
                    &data,
                    (&options as *const MakeOptionsV8).cast(),
                    &mut output.0,
                )
            },
            "注册本机 PRF 凭据",
        )?;
        if output.0.is_null() {
            return Err(prf::unsupported("WebAuthn 未返回注册结果"));
        }
        let base = unsafe { &*output.0 };
        let credential = copy_bytes(
            base.pbCredentialId,
            base.cbCredentialId,
            prf::MAX_CREDENTIAL,
        )?;
        let result = (|| {
            if base.dwVersion < 7 {
                return Err(prf::unsupported("系统未返回注册时 PRF，未启用系统解锁"));
            }
            Ok(Registration {
                result: AuthResult {
                    credential: credential.clone(),
                    authenticator_data: copy_bytes(
                        base.pbAuthenticatorData,
                        base.cbAuthenticatorData,
                        65536,
                    )?,
                    prf: unsafe { copy_prf((*output.0.cast::<AttestationV7>()).prf) },
                },
                prf_enabled: base.bPrfEnabled != 0,
                resident: base.bResidentKey != 0,
                internal: base.dwUsedTransport == WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
            })
        })();
        if result.is_err() {
            let _ = self.delete(&credential);
        }
        result
    }
    fn evaluate(&self, credential: &[u8], salt: &[u8; 32]) -> Result<AuthResult> {
        let mut id = credential.to_vec();
        let mut allowed = WEBAUTHN_CREDENTIAL_EX {
            dwVersion: 1,
            cbId: id.len() as u32,
            pbId: id.as_mut_ptr(),
            pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            dwTransports: WEBAUTHN_CTAP_TRANSPORT_INTERNAL,
        };
        let mut allowed_ptr = &mut allowed as *mut WEBAUTHN_CREDENTIAL_EX;
        let mut list = WEBAUTHN_CREDENTIAL_LIST {
            cCredentials: 1,
            ppCredentials: &mut allowed_ptr,
        };
        let mut raw_salt = *salt;
        let mut eval = WEBAUTHN_HMAC_SECRET_SALT {
            cbFirst: 32,
            pbFirst: raw_salt.as_mut_ptr(),
            cbSecond: 0,
            pbSecond: ptr::null_mut(),
        };
        let mut salts = WEBAUTHN_HMAC_SECRET_SALT_VALUES {
            pGlobalHmacSalt: &mut eval,
            cCredWithHmacSecretSaltList: 0,
            pCredWithHmacSecretSaltList: ptr::null_mut(),
        };
        let options = WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS {
            dwVersion: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6,
            dwTimeoutMilliseconds: 60_000,
            dwAuthenticatorAttachment: WEBAUTHN_AUTHENTICATOR_ATTACHMENT_PLATFORM,
            dwUserVerificationRequirement: WEBAUTHN_USER_VERIFICATION_REQUIREMENT_REQUIRED,
            pAllowCredentialList: &mut list,
            pHmacSecretSaltValues: &mut salts,
            ..Default::default()
        };
        let mut bytes = client_data("get");
        let data = client(&mut bytes);
        let rp_id = wide(prf::RP_ID);
        let mut output = Assertion(ptr::null_mut());
        check(
            unsafe {
                WebAuthNAuthenticatorGetAssertion(
                    self.hwnd()?,
                    rp_id.as_ptr(),
                    &data,
                    &options,
                    &mut output.0,
                )
            },
            "读取 PRF / Hello 验证",
        )?;
        if output.0.is_null() || unsafe { (*output.0).dwVersion } < 3 {
            return Err(prf::unsupported("系统未返回 PRF 验证结果，请使用主密码"));
        }
        let value = unsafe { &*output.0 };
        Ok(AuthResult {
            credential: copy_bytes(
                value.Credential.pbId,
                value.Credential.cbId,
                prf::MAX_CREDENTIAL,
            )?,
            authenticator_data: copy_bytes(
                value.pbAuthenticatorData,
                value.cbAuthenticatorData,
                65536,
            )?,
            prf: unsafe { copy_prf(value.pHmacSecret) },
        })
    }
    fn delete(&self, credential: &[u8]) -> Result<()> {
        if credential.is_empty() || credential.len() > prf::MAX_CREDENTIAL {
            return Err(AppError::invalid("无效的 PRF 凭据 ID"));
        }
        let status = unsafe {
            WebAuthNDeletePlatformCredential(credential.len() as u32, credential.as_ptr())
        };
        if matches!(status as u32, 0x80090011 | 0x80070490) {
            return Ok(());
        }
        check(status, "撤销本机 PRF 凭据")
    }
}

pub fn availability() -> SystemUnlock {
    let result = Native { window: 0 }
        .capabilities()
        .and_then(|(api, platform)| prf::check_capabilities(api, platform));
    SystemUnlock {
        available: result.is_ok(),
        enabled: false,
        label: "Windows Hello".into(),
        reason: Some(match result {
            Ok(()) => "已通过 WebAuthn 基础能力检查；启用时需验证 PRF 和加密解锁".into(),
            Err(error) => error.message,
        }),
    }
}
pub fn protect(
    store: &impl SecretStore,
    account: &str,
    secret: &[u8],
    window: isize,
) -> Result<()> {
    prf::protect(store, &Native { window }, account, secret)
}
pub fn unprotect(
    store: &impl SecretStore,
    account: &str,
    window: isize,
) -> Result<Zeroizing<Vec<u8>>> {
    prf::unprotect(store, &Native { window }, account)
}
pub fn revoke(store: &impl SecretStore, account: &str) -> Result<()> {
    prf::revoke(store, &Native { window: 0 }, account)
}
