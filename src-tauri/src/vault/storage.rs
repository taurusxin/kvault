use super::{
    crypto::{self, Keys},
    model::LocalState,
    preferences::Preferences,
};
use crate::error::{AppError, Result};
use crate::{
    security::{SecretStore, SystemUnlock},
    sync::SyncConfig,
};
use atomic_write_file::AtomicWriteFile;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Storage {
    pub dir: PathBuf,
    pub secrets: Arc<dyn SecretStore>,
    scope: String,
    preferences: Arc<Mutex<Preferences>>,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Result<Self> {
        #[cfg(test)]
        let secrets = Arc::new(crate::security::MemorySecrets::default());
        #[cfg(not(test))]
        let secrets = Arc::new(crate::security::SystemSecrets);
        Self::with_secrets(dir, secrets)
    }
    pub fn with_secrets(dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        fs::create_dir_all(&dir).map_err(AppError::storage)?;
        let dir = fs::canonicalize(dir).map_err(AppError::storage)?;
        let scope = format!("{:x}", Sha256::digest(dir.to_string_lossy().as_bytes()));
        let config_path = dir.join("config.json");
        let preferences = if config_path.exists() {
            if fs::metadata(&config_path).map_err(AppError::storage)?.len() > 16384 {
                return Err(AppError::invalid("本机配置文件过大"));
            }
            serde_json::from_slice::<Preferences>(
                &fs::read(config_path).map_err(AppError::storage)?,
            )
            .map_err(|_| AppError::invalid("本机 config.json 无效，请修正或移除后重启"))?
        } else {
            Preferences::default()
        };
        preferences.validate()?;
        Ok(Self {
            dir,
            secrets,
            scope,
            preferences: Arc::new(Mutex::new(preferences)),
        })
    }
    pub fn preferences(&self) -> Result<Preferences> {
        Ok(self.preferences.lock().map_err(AppError::storage)?.clone())
    }
    pub fn update_preferences(&self, change: impl FnOnce(&mut Preferences)) -> Result<Preferences> {
        let mut guard = self.preferences.lock().map_err(AppError::storage)?;
        let mut next = guard.clone();
        change(&mut next);
        next.validate()?;
        self.write_raw(
            &self.dir.join("config.json"),
            &serde_json::to_vec_pretty(&next).map_err(AppError::storage)?,
        )?;
        *guard = next.clone();
        Ok(next)
    }
    pub fn unlock_account(&self, fingerprint: &str) -> String {
        format!("unlock:{}:{fingerprint}", self.scope)
    }
    fn sync_account(&self, vault_id: &str, profile: &str) -> String {
        format!("sync:{}:{vault_id}:{profile}", self.scope)
    }
    pub fn system_unlock(&self, header: Option<&crypto::Header>) -> Result<SystemUnlock> {
        let mut status = self.secrets.availability();
        status.enabled = match header {
            Some(h) => {
                self.preferences()?.system_unlock.as_deref() == Some(&crypto::fingerprint(h)?)
            }
            None => false,
        };
        Ok(status)
    }
    pub fn disable_system_unlock(&self) -> Result<()> {
        if let Some(fingerprint) = self.preferences()?.system_unlock {
            self.secrets.revoke(&self.unlock_account(&fingerprint))?;
            self.update_preferences(|p| p.system_unlock = None)?;
        }
        Ok(())
    }
    pub fn save_sync(&self, vault_id: &str, config: Option<&SyncConfig>) -> Result<Option<String>> {
        if let Some(config) = config {
            config.validate()?;
            let bytes = Zeroizing::new(serde_json::to_vec(config).map_err(AppError::storage)?);
            let profile = super::model::id();
            self.secrets
                .write(&self.sync_account(vault_id, &profile), &bytes)?;
            return Ok(Some(profile));
        }
        Ok(None)
    }
    pub fn delete_sync(&self, vault_id: &str, profile: Option<&str>) -> Result<()> {
        if let Some(profile) = profile {
            self.secrets.delete(&self.sync_account(vault_id, profile))?;
        }
        Ok(())
    }
    pub fn cleanup_profiles(&self, state: &mut LocalState, vault_id: &str) {
        // The retired IDs were committed inside the encrypted state before
        // deletion. A crash or unavailable keychain leaves a retryable queue.
        state.retired_profiles.retain(|profile| {
            state.sync_profile.as_ref() == Some(profile)
                || self.delete_sync(vault_id, Some(profile)).is_err()
        });
    }
    pub fn restore_device_state(&self, state: &mut LocalState, vault_id: &str) -> Result<()> {
        let prefs = self.preferences()?;
        state.auto_lock = prefs.auto_lock;
        state.config = None;
        state.credential_error = None;
        self.cleanup_profiles(state, vault_id);
        if let Some(profile) = &state.sync_profile {
            let result: Result<SyncConfig> = (|| {
                uuid::Uuid::parse_str(profile).map_err(|_| crate::security::credential_error())?;
                let bytes = self
                    .secrets
                    .read(&self.sync_account(vault_id, profile))?
                    .ok_or_else(crate::security::credential_error)?;
                let config: SyncConfig = serde_json::from_slice(&bytes)
                    .map_err(|_| crate::security::credential_error())?;
                config.validate()?;
                Ok(config)
            })();
            match result {
                Ok(config) => state.config = Some(config),
                Err(error) => state.credential_error = Some(error.message),
            }
        }
        Ok(())
    }
    pub fn path(&self) -> PathBuf {
        self.dir.join("vault.kvault")
    }
    pub fn pending(&self) -> PathBuf {
        self.dir.join("password-change.pending")
    }
    pub fn exists(&self) -> bool {
        self.path().exists()
    }
    pub fn read(&self, path: &Path) -> Result<Vec<u8>> {
        if fs::metadata(path).map_err(AppError::storage)?.len() > crypto::MAX_FILE_SIZE as u64 {
            return Err(AppError::invalid("保险库文件过大"));
        }
        fs::read(path).map_err(AppError::storage)
    }
    pub fn write_raw(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let mut options = AtomicWriteFile::options();
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path).map_err(AppError::storage)?;
        file.write_all(bytes).map_err(AppError::storage)?;
        file.commit().map_err(AppError::storage)
    }
    pub fn save(&self, state: &LocalState, keys: &Keys) -> Result<()> {
        let bytes = crypto::seal(state, keys, "local")?;
        let fingerprint = crypto::fingerprint(&keys.header)?;
        if self
            .preferences()?
            .system_unlock
            .as_ref()
            .is_some_and(|f| f != &fingerprint)
        {
            self.disable_system_unlock()?;
        }
        if self.exists() {
            let old = self.read(&self.path())?;
            self.write_raw(&self.dir.join("vault.previous.kvault"), &old)?;
        }
        self.write_raw(&self.path(), &bytes)
    }
    pub fn stage(&self, state: &LocalState, keys: &Keys) -> Result<()> {
        self.write_raw(&self.pending(), &crypto::seal(state, keys, "local")?)
    }
    pub fn discard_pending(&self) {
        let _ = fs::remove_file(self.pending());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_ciphertext_and_previous_version() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path().into()).unwrap();
        let keys = Keys::new("test-password", uuid::Uuid::new_v4().to_string(), 1).unwrap();
        storage.save(&LocalState::default(), &keys).unwrap();
        let old = storage.read(&storage.path()).unwrap();
        let state = LocalState {
            revision: 7,
            ..Default::default()
        };
        storage.save(&state, &keys).unwrap();
        assert_eq!(
            storage
                .read(&dir.path().join("vault.previous.kvault"))
                .unwrap(),
            old
        );
        assert_eq!(
            crypto::open::<LocalState>(&storage.read(&storage.path()).unwrap(), &keys, "local")
                .unwrap()
                .revision,
            7
        );
        assert!(
            storage
                .write_raw(&dir.path().join("missing/file"), b"failure")
                .is_err()
        );
        assert_eq!(
            crypto::open::<LocalState>(&storage.read(&storage.path()).unwrap(), &keys, "local")
                .unwrap()
                .revision,
            7
        );
    }
}
