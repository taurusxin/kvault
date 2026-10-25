use super::{
    crypto::{self, Keys},
    model::LocalState,
};
use crate::error::{AppError, Result};
use atomic_write_file::AtomicWriteFile;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Storage {
    pub dir: PathBuf,
}

impl Storage {
    pub fn new(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir).map_err(AppError::storage)?;
        Ok(Self { dir })
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
