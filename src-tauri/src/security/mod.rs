//! Device-only credentials. No plaintext fallback is allowed.
#[cfg(test)]
mod tests;
use crate::error::{AppError, Result};
use serde::Serialize;
use zeroize::Zeroizing;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub const SERVICE: &str = "com.taurusxin.kvault.v2";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemUnlock {
    pub available: bool,
    pub enabled: bool,
    pub label: String,
    pub reason: Option<String>,
}

pub trait SecretStore: Send + Sync {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    fn write(&self, account: &str, secret: &[u8]) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
    fn availability(&self) -> SystemUnlock;
    fn protect(&self, account: &str, secret: &[u8], window: isize) -> Result<()>;
    fn unprotect(&self, account: &str, window: isize) -> Result<Zeroizing<Vec<u8>>>;
    fn revoke(&self, account: &str) -> Result<()>;
}

pub fn unavailable() -> AppError {
    AppError::new(
        "system_auth_unavailable",
        "系统验证不可用，请使用主密码解锁",
    )
}

pub fn credential_error() -> AppError {
    AppError::new(
        "credential_store",
        "无法访问系统凭据库，未使用磁盘文件替代；请检查钥匙串或凭据服务",
    )
}

pub struct SystemSecrets;

#[cfg(any(target_os = "windows", target_os = "linux"))]
impl SystemSecrets {
    fn entry(account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, account).map_err(|_| credential_error())
    }
}

impl SecretStore for SystemSecrets {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        #[cfg(target_os = "macos")]
        return macos::read_profile(account);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        match Self::entry(account)?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }
    fn write(&self, account: &str, secret: &[u8]) -> Result<()> {
        // Windows Credential Manager limits each blob to 2560 bytes.
        if secret.len() > 2500 {
            return Err(AppError::invalid("同步配置过长，不能超过 2500 字节"));
        }
        #[cfg(target_os = "macos")]
        return macos::write_profile(account, secret);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        Self::entry(account)?
            .set_secret(secret)
            .map_err(|_| credential_error())
    }
    fn delete(&self, account: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        return macos::delete_profile(account);
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_error()),
        }
    }
    fn availability(&self) -> SystemUnlock {
        #[cfg(target_os = "macos")]
        return macos::availability();
        #[cfg(target_os = "windows")]
        return windows::availability();
        #[cfg(target_os = "linux")]
        SystemUnlock {
            available: false,
            enabled: false,
            label: "系统验证".into(),
            reason: Some("Linux 当前使用主密码解锁；云凭据使用 Secret Service".into()),
        }
    }
    fn protect(&self, account: &str, secret: &[u8], _window: isize) -> Result<()> {
        #[cfg(target_os = "macos")]
        return macos::protect(account, secret);
        #[cfg(target_os = "windows")]
        return windows::protect(self, account, secret, _window);
        #[cfg(target_os = "linux")]
        {
            let _ = (account, secret);
            Err(unavailable())
        }
    }
    fn unprotect(&self, account: &str, _window: isize) -> Result<Zeroizing<Vec<u8>>> {
        #[cfg(target_os = "macos")]
        return macos::unprotect(account);
        #[cfg(target_os = "windows")]
        return windows::unprotect(self, account, _window);
        #[cfg(target_os = "linux")]
        {
            let _ = account;
            Err(unavailable())
        }
    }
    fn revoke(&self, account: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        return macos::revoke(account);
        #[cfg(target_os = "windows")]
        return windows::revoke(self, account);
        #[cfg(target_os = "linux")]
        {
            let _ = account;
            Ok(())
        }
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

#[cfg(test)]
impl SecretStore for MemorySecrets {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(account)
            .cloned()
            .map(Zeroizing::new))
    }
    fn write(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
    fn availability(&self) -> SystemUnlock {
        SystemUnlock {
            available: true,
            enabled: false,
            label: "测试系统验证".into(),
            reason: None,
        }
    }
    fn protect(&self, account: &str, secret: &[u8], _: isize) -> Result<()> {
        self.write(&format!("protected:{account}"), secret)
    }
    fn unprotect(&self, account: &str, _: isize) -> Result<Zeroizing<Vec<u8>>> {
        self.read(&format!("protected:{account}"))?
            .ok_or_else(unavailable)
    }
    fn revoke(&self, account: &str) -> Result<()> {
        self.delete(&format!("protected:{account}"))
    }
}
