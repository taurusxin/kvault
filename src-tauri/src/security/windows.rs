//! Windows Hello uses a non-exportable Passport CNG RSA key. Only NCryptDecrypt
//! can release the DEK; Credential Manager contains the RSA ciphertext only.
//! No UserConsentVerifier + DPAPI-only fallback is provided.
use super::{SecretStore, SystemUnlock, unavailable};
use crate::error::{AppError, Result};
use std::{ffi::c_void, ptr};
use zeroize::Zeroizing;

#[link(name = "ncrypt")]
unsafe extern "system" {
    fn NCryptOpenStorageProvider(out: *mut usize, name: *const u16, flags: u32) -> i32;
    fn NCryptCreatePersistedKey(
        provider: usize,
        out: *mut usize,
        algorithm: *const u16,
        name: *const u16,
        legacy: u32,
        flags: u32,
    ) -> i32;
    fn NCryptOpenKey(
        provider: usize,
        out: *mut usize,
        name: *const u16,
        legacy: u32,
        flags: u32,
    ) -> i32;
    fn NCryptSetProperty(
        key: usize,
        name: *const u16,
        value: *const u8,
        size: u32,
        flags: u32,
    ) -> i32;
    fn NCryptGetProperty(
        key: usize,
        name: *const u16,
        value: *mut u8,
        size: u32,
        written: *mut u32,
        flags: u32,
    ) -> i32;
    fn NCryptFinalizeKey(key: usize, flags: u32) -> i32;
    fn NCryptEncrypt(
        key: usize,
        input: *const u8,
        size: u32,
        padding: *const Oaep,
        output: *mut u8,
        capacity: u32,
        written: *mut u32,
        flags: u32,
    ) -> i32;
    fn NCryptDecrypt(
        key: usize,
        input: *const u8,
        size: u32,
        padding: *const Oaep,
        output: *mut u8,
        capacity: u32,
        written: *mut u32,
        flags: u32,
    ) -> i32;
    fn NCryptDeleteKey(key: usize, flags: u32) -> i32;
    fn NCryptFreeObject(handle: usize) -> i32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
    fn GetTokenInformation(
        token: *mut c_void,
        class: u32,
        data: *mut c_void,
        length: u32,
        required: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

#[repr(C)]
struct Oaep {
    algorithm: *const u16,
    label: *const u8,
    label_len: u32,
}
struct Handle(usize);
impl Drop for Handle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                NCryptFreeObject(self.0);
            }
        }
    }
}
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn check(status: i32, stage: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(status_error(status, stage))
    }
}
fn status_error(status: i32, stage: &str) -> AppError {
    if matches!(status as u32, 0x80090036 | 0x800704c7) {
        AppError::new("auth_cancelled", "已取消 Windows Hello，可使用主密码解锁")
    } else {
        AppError::new(
            "system_auth",
            format!(
                "Windows Hello 在{stage}时失败（0x{:08X}），请使用主密码。",
                status as u32
            ),
        )
    }
}
fn provider() -> Result<Handle> {
    let mut handle = 0;
    check(
        unsafe {
            NCryptOpenStorageProvider(
                &mut handle,
                wide("Microsoft Passport Key Storage Provider").as_ptr(),
                0,
            )
        },
        "打开 Passport 密钥提供程序",
    )?;
    Ok(Handle(handle))
}
fn current_sid() -> Result<String> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(-1isize as *mut c_void, 8, &mut token) == 0 {
            return Err(unavailable());
        }
        let result = (|| {
            let mut size = 0;
            GetTokenInformation(token, 1, ptr::null_mut(), 0, &mut size);
            if size == 0 || size > 65536 {
                return Err(unavailable());
            }
            // TOKEN_USER begins with a SID pointer; u64 storage supplies alignment.
            let mut data = vec![0u64; (size as usize).div_ceil(8)];
            if GetTokenInformation(token, 1, data.as_mut_ptr().cast(), size, &mut size) == 0 {
                return Err(unavailable());
            }
            let sid = *(data.as_ptr() as *const *const c_void);
            let mut text = ptr::null_mut();
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return Err(unavailable());
            }
            let mut length = 0;
            while *text.add(length) != 0 && length < 256 {
                length += 1;
            }
            let result = String::from_utf16(std::slice::from_raw_parts(text, length))
                .map_err(|_| unavailable());
            LocalFree(text.cast());
            result
        })();
        CloseHandle(token);
        result
    }
}
fn key_name(account: &str) -> Result<Vec<u16>> {
    Ok(wide(&format!("{}//KVault/v2/{account}", current_sid()?)))
}
fn property(key: &Handle, name: &str, value: &[u8]) -> Result<()> {
    check(set_property(key, name, value), &format!("设置 {name}"))
}
fn set_property(key: &Handle, name: &str, value: &[u8]) -> i32 {
    unsafe {
        NCryptSetProperty(
            key.0,
            wide(name).as_ptr(),
            value.as_ptr(),
            value.len() as u32,
            0,
        )
    }
}
fn integer(key: &Handle, name: &str) -> Result<u32> {
    integer_status(key, name).map_err(|status| status_error(status, &format!("读取 {name}")))
}
fn integer_status(key: &Handle, name: &str) -> std::result::Result<u32, i32> {
    let mut value = 0u32;
    let mut written = 0;
    let status = unsafe {
        NCryptGetProperty(
            key.0,
            wide(name).as_ptr(),
            (&mut value as *mut u32).cast(),
            4,
            &mut written,
            0,
        )
    };
    if status != 0 {
        return Err(status);
    }
    if written != 4 {
        return Err(0x80090004u32 as i32); // NTE_BAD_LEN
    }
    Ok(value)
}
fn unsupported_property(status: i32) -> bool {
    matches!(status as u32, 0x80090029 | 0x80090027 | 0x80070032)
}
fn set_cache_policy(key: &Handle) -> Result<()> {
    let status = set_property(key, "NgcCacheType", &1u32.to_le_bytes());
    if unsupported_property(status) {
        // Older Passport providers expose the same policy under its old name.
        // Never fall back to an authentication-optional key or software store.
        property(key, "NgcCacheTypeProperty", &1u32.to_le_bytes())
    } else {
        check(status, "设置 NgcCacheType")
    }
}
fn cache_policy(key: &Handle) -> Result<u32> {
    match integer_status(key, "NgcCacheType") {
        Err(status) if unsupported_property(status) => integer(key, "NgcCacheTypeProperty"),
        Err(status) => Err(status_error(status, "读取 NgcCacheType")),
        Ok(value) => Ok(value),
    }
}
fn verify_policy(key: &Handle) -> Result<()> {
    // Reject imported/exportable or authentication-optional replacement keys.
    for (property, actual, expected) in [
        ("NgcCacheType", cache_policy(key)?, 1),
        (
            "Key Usage（允许解密、禁止导入）",
            integer(key, "Key Usage")? & 9,
            1,
        ),
        ("Export Policy", integer(key, "Export Policy")?, 0),
        ("Length", integer(key, "Length")?, 2048),
    ] {
        if actual != expected {
            return Err(AppError::new(
                "system_auth",
                format!(
                    "Windows Hello 密钥策略校验失败：{property}（实际 {actual}，要求 {expected}），请使用主密码并重新启用。"
                ),
            ));
        }
    }
    Ok(())
}
fn ui_context(key: &Handle, window: isize) -> Result<()> {
    if window != 0 {
        property(key, "HWND Handle", &window.to_ne_bytes())?;
    }
    let message = wide("解锁 KVault 保险库");
    let bytes: Vec<u8> = message.iter().flat_map(|c| c.to_le_bytes()).collect();
    property(key, "Use Context", &bytes)
}
fn open_key(account: &str) -> Result<(Handle, Handle)> {
    let provider = provider()?;
    let mut key = 0;
    check(
        unsafe { NCryptOpenKey(provider.0, &mut key, key_name(account)?.as_ptr(), 0, 0) },
        "打开已创建的密钥",
    )?;
    let key = Handle(key);
    verify_policy(&key)?;
    Ok((provider, key))
}
pub fn availability() -> SystemUnlock {
    let available = provider().is_ok();
    SystemUnlock {
        available,
        enabled: false,
        label: "Windows Hello".into(),
        reason: Some(
            if available {
                "需先在 Windows 设置中配置 Hello；启用时验证设备支持情况"
            } else {
                "Windows Hello 不可用，可继续使用主密码"
            }
            .into(),
        ),
    }
}
pub fn protect(
    store: &impl SecretStore,
    account: &str,
    secret: &[u8],
    window: isize,
) -> Result<()> {
    if secret.len() != 32 {
        return Err(unavailable());
    }
    revoke(store, account)?;
    let provider = provider()?;
    let mut raw = 0;
    check(
        unsafe {
            NCryptCreatePersistedKey(
                provider.0,
                &mut raw,
                wide("RSA").as_ptr(),
                key_name(account)?.as_ptr(),
                0,
                0,
            )
        },
        "创建 RSA 密钥",
    )?;
    let mut key = Handle(raw);
    let result = (|| {
        property(&key, "Length", &2048u32.to_le_bytes())?;
        property(&key, "Key Usage", &1u32.to_le_bytes())?;
        property(&key, "Export Policy", &0u32.to_le_bytes())?;
        set_cache_policy(&key)?;
        ui_context(&key, window)?;
        check(
            unsafe { NCryptFinalizeKey(key.0, 0) },
            "完成密钥创建 / Hello 授权",
        )?;
        verify_policy(&key)?;
        let algorithm = wide("SHA256");
        let padding = Oaep {
            algorithm: algorithm.as_ptr(),
            label: account.as_ptr(),
            label_len: account.len() as u32,
        };
        let mut ciphertext = vec![0; 256];
        let mut length = 0;
        check(
            unsafe {
                NCryptEncrypt(
                    key.0,
                    secret.as_ptr(),
                    32,
                    &padding,
                    ciphertext.as_mut_ptr(),
                    256,
                    &mut length,
                    4,
                )
            },
            "RSA-OAEP-SHA256 加密数据密钥",
        )?;
        if length != 256 {
            return Err(AppError::new(
                "system_auth",
                "Windows Hello 加密结果长度异常（应为 256 字节）",
            ));
        }
        store.write(&format!("hello:{account}"), &ciphertext)?;
        let verified = unprotect(store, account, window)?;
        if verified.as_slice() != secret {
            return Err(AppError::new(
                "system_auth",
                "Windows Hello 密钥往返验证失败，请重新启用",
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        // Delete only the key created by this enrollment attempt.
        if unsafe { NCryptDeleteKey(key.0, 0) } == 0 {
            key.0 = 0;
        }
        // Preserve the failing API/status rather than replacing it with a
        // cleanup error after the user has already completed the Hello prompt.
        let _ = store.delete(&format!("hello:{account}"));
    }
    result
}
pub fn unprotect(
    store: &impl SecretStore,
    account: &str,
    window: isize,
) -> Result<Zeroizing<Vec<u8>>> {
    let ciphertext = store
        .read(&format!("hello:{account}"))?
        .ok_or_else(unavailable)?;
    if ciphertext.len() != 256 {
        return Err(unavailable());
    }
    let (_provider, key) = open_key(account)?;
    ui_context(&key, window)?;
    // Require a fresh Hello gesture on every private-key operation.
    property(&key, "PinCacheIsGestureRequired", &1u32.to_le_bytes())?;
    let algorithm = wide("SHA256");
    let padding = Oaep {
        algorithm: algorithm.as_ptr(),
        label: account.as_ptr(),
        label_len: account.len() as u32,
    };
    let mut plain = Zeroizing::new(vec![0; 256]);
    let mut length = 0;
    check(
        unsafe {
            NCryptDecrypt(
                key.0,
                ciphertext.as_ptr(),
                256,
                &padding,
                plain.as_mut_ptr(),
                256,
                &mut length,
                4,
            )
        },
        "RSA-OAEP-SHA256 解密 / Hello 授权",
    )?;
    if length != 32 {
        return Err(AppError::new(
            "system_auth",
            "Windows Hello 解密结果长度异常（应为 32 字节）",
        ));
    }
    plain.truncate(32);
    Ok(plain)
}
pub fn revoke(store: &impl SecretStore, account: &str) -> Result<()> {
    let provider = provider()?;
    let mut raw = 0;
    let status = unsafe { NCryptOpenKey(provider.0, &mut raw, key_name(account)?.as_ptr(), 0, 0) };
    if status == 0 {
        let mut key = Handle(raw);
        check(unsafe { NCryptDeleteKey(key.0, 0) }, "删除旧密钥")?;
        key.0 = 0; // NCryptDeleteKey consumes the handle.
    } else if !matches!(status as u32, 0x8009000d | 0x80090016 | 0x80090011) {
        check(status, "查找旧密钥")?;
    }
    store.delete(&format!("hello:{account}"))
}
