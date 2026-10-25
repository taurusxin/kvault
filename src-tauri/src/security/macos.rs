use super::{SERVICE, SystemUnlock, credential_error, unavailable};
use crate::error::{AppError, Result};
use security_framework::passwords::{self, PasswordOptions};
use security_framework_sys::base::errSecItemNotFound;
use std::{ffi::c_void, ptr};
use zeroize::{Zeroize, Zeroizing};

const UNLOCK_SERVICE: &str = "com.taurusxin.kvault.v2.system-unlock";

unsafe extern "C" {
    fn kvault_system_available() -> i32;
    fn kvault_system_enroll(
        secret: *const u8,
        secret_len: usize,
        account: *const u8,
        account_len: usize,
        output: *mut *mut c_void,
        output_len: *mut usize,
    ) -> i32;
    fn kvault_system_open(
        envelope: *const u8,
        envelope_len: usize,
        account: *const u8,
        account_len: usize,
        output: *mut *mut c_void,
        output_len: *mut usize,
    ) -> i32;
    fn kvault_system_free(memory: *mut c_void, size: usize);
}

pub fn availability() -> SystemUnlock {
    // Capability detection never prompts and never opens a vault key.
    let available = unsafe { kvault_system_available() == 1 };
    SystemUnlock {
        available,
        enabled: false,
        label: "Touch ID / 系统验证".into(),
        reason: (!available).then(|| "需要 Secure Enclave 与可用的系统验证；仍可使用主密码".into()),
    }
}

fn options(service: &str, account: &str) -> PasswordOptions {
    let mut options = PasswordOptions::new_generic_password(service, account);
    options.set_access_synchronized(Some(false));
    options
}
pub fn read_profile(account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
    match passwords::generic_password(options(SERVICE, account)) {
        Ok(secret) => Ok(Some(Zeroizing::new(secret))),
        Err(e) if e.code() == errSecItemNotFound => Ok(None),
        Err(_) => Err(credential_error()),
    }
}
pub fn write_profile(account: &str, secret: &[u8]) -> Result<()> {
    let mut options = options(SERVICE, account);
    options.set_label("KVault 云同步配置");
    passwords::set_generic_password_options(secret, options).map_err(|_| credential_error())
}
pub fn delete_profile(account: &str) -> Result<()> {
    match passwords::delete_generic_password_options(options(SERVICE, account)) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == errSecItemNotFound => Ok(()),
        Err(_) => Err(credential_error()),
    }
}
fn auth_error() -> AppError {
    AppError::new(
        "system_auth",
        "系统验证未完成或本机授权已失效，请使用主密码并重新启用",
    )
}
struct NativeBuffer {
    memory: *mut c_void,
    size: usize,
}
impl Drop for NativeBuffer {
    fn drop(&mut self) {
        if !self.memory.is_null() && self.size > 0 {
            unsafe { std::slice::from_raw_parts_mut(self.memory.cast::<u8>(), self.size) }
                .zeroize();
        }
        unsafe {
            kvault_system_free(self.memory, self.size);
        }
    }
}
fn native(
    operation: impl FnOnce(*mut *mut c_void, *mut usize) -> i32,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut buffer = NativeBuffer {
        memory: ptr::null_mut(),
        size: 0,
    };
    match operation(&mut buffer.memory, &mut buffer.size) {
        0 => {}
        1 => {
            return Err(AppError::new(
                "auth_cancelled",
                "已取消系统验证，可使用主密码解锁",
            ));
        }
        2 => return Err(unavailable()),
        _ => return Err(auth_error()),
    }
    if buffer.memory.is_null() || buffer.size == 0 || buffer.size > 2500 {
        return Err(auth_error());
    }
    // Swift allocation is wiped by NativeBuffer::drop after copying to a Rust
    // zeroizing buffer. No key material crosses the Tauri/frontend boundary.
    Ok(Zeroizing::new(
        unsafe { std::slice::from_raw_parts(buffer.memory.cast::<u8>(), buffer.size) }.to_vec(),
    ))
}
pub fn protect(account: &str, secret: &[u8]) -> Result<()> {
    let envelope = native(|output, size| unsafe {
        kvault_system_enroll(
            secret.as_ptr(),
            secret.len(),
            account.as_ptr(),
            account.len(),
            output,
            size,
        )
    })?;
    let mut options = options(UNLOCK_SERVICE, account);
    options.set_label("KVault 系统解锁（Secure Enclave 密文）");
    // Login keychain stores only an opaque Secure Enclave key plus the wrapped
    // DEK. Reading this blob does not bypass the OS-protected private-key use.
    passwords::set_generic_password_options(&envelope, options).map_err(|_| credential_error())
}
pub fn unprotect(account: &str) -> Result<Zeroizing<Vec<u8>>> {
    let envelope = Zeroizing::new(
        passwords::generic_password(options(UNLOCK_SERVICE, account)).map_err(|_| auth_error())?,
    );
    native(|output, size| unsafe {
        kvault_system_open(
            envelope.as_ptr(),
            envelope.len(),
            account.as_ptr(),
            account.len(),
            output,
            size,
        )
    })
}
pub fn revoke(account: &str) -> Result<()> {
    match passwords::delete_generic_password_options(options(UNLOCK_SERVICE, account)) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == errSecItemNotFound => Ok(()),
        Err(_) => Err(credential_error()),
    }
}
