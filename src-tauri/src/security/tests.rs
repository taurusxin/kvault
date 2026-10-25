use super::*;
use crate::{
    commands::{change_password_impl, configure_system_unlock, system_unlock_impl, unlock_impl},
    sync::SyncConfig,
    vault::{
        self, Action,
        crypto::{self, Keys},
        model::{Document, Entry, EntryInput, LocalState, Table, id},
        storage::Storage,
    },
};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct ControlledSecrets {
    memory: MemorySecrets,
    fail_read: AtomicBool,
    cancel: AtomicBool,
    pause_protect: AtomicBool,
    pause_read: AtomicBool,
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}
impl ControlledSecrets {
    fn pause(&self) {
        *self.entered.0.lock().unwrap() = true;
        self.entered.1.notify_all();
        let mut released = self.released.0.lock().unwrap();
        while !*released {
            released = self.released.1.wait(released).unwrap();
        }
    }
    fn wait_for_prompt(&self) {
        let entered = self.entered.0.lock().unwrap();
        let (_guard, timeout) = self
            .entered
            .1
            .wait_timeout_while(entered, std::time::Duration::from_secs(10), |v| !*v)
            .unwrap();
        assert!(!timeout.timed_out(), "native prompt was not reached");
    }
    fn release(&self) {
        *self.released.0.lock().unwrap() = true;
        self.released.1.notify_all();
    }
}
impl SecretStore for ControlledSecrets {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        if self.fail_read.load(Ordering::SeqCst) {
            return Err(credential_error());
        }
        self.memory.read(account)
    }
    fn write(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.memory.write(account, secret)
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.memory.delete(account)
    }
    fn availability(&self) -> SystemUnlock {
        self.memory.availability()
    }
    fn protect(&self, account: &str, secret: &[u8], _: isize) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(AppError::new("auth_cancelled", "已取消"));
        }
        if self.pause_protect.load(Ordering::SeqCst) {
            self.pause();
        }
        self.memory.protect(account, secret, 0)
    }
    fn unprotect(&self, account: &str, _: isize) -> Result<Zeroizing<Vec<u8>>> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(AppError::new("auth_cancelled", "已取消"));
        }
        if self.pause_read.load(Ordering::SeqCst) {
            self.pause();
        }
        self.memory.unprotect(account, 0)
    }
    fn revoke(&self, account: &str) -> Result<()> {
        self.memory.revoke(account)
    }
}
fn fixture(secrets: Arc<ControlledSecrets>) -> (vault::Shared, tempfile::TempDir, u64) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::with_secrets(dir.path().into(), secrets).unwrap();
    let keys = Keys::new("master-password", id(), 1).unwrap();
    let table_id = id();
    let local = LocalState {
        document: Document {
            tables: vec![Table {
                id: table_id.clone(),
                name: "私密分类".into(),
                created_at: 1,
                updated_at: 1,
                deleted: false,
            }],
            entries: vec![Entry {
                id: id(),
                table_id,
                key: "private-key".into(),
                value: "private-value".into(),
                notes: "private-notes".into(),
                sort_order: 1,
                created_at: 1,
                updated_at: 1,
                deleted: false,
            }],
        },
        ..Default::default()
    };
    storage.save(&local, &keys).unwrap();
    let shared = Arc::new(vault::AppState::new(storage));
    let sid = shared
        .vault
        .lock()
        .unwrap()
        .install(keys, local)
        .unwrap()
        .session_id;
    (shared, dir, sid)
}

#[tokio::test]
async fn system_unlock_is_opt_in_and_password_remains_available() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets);
    assert!(!state.snapshot(sid).unwrap().system_unlock.enabled);
    assert!(
        configure_system_unlock(&state, &(), sid, true, "wrong-password".into(), 0)
            .await
            .is_err()
    );
    assert!(!state.snapshot(sid).unwrap().system_unlock.enabled);
    let enabled = configure_system_unlock(&state, &(), sid, true, "master-password".into(), 0)
        .await
        .unwrap();
    assert!(enabled.system_unlock.enabled);
    state.lock().unwrap();
    let opened = system_unlock_impl(&state, &(), 0).await.unwrap();
    assert_eq!(opened.entries[0].value, "private-value");
    state.lock().unwrap();
    let password = unlock_impl(&state, &(), "master-password".into())
        .await
        .unwrap();
    assert!(password.system_unlock.enabled);
    configure_system_unlock(&state, &(), password.session_id, false, String::new(), 0)
        .await
        .unwrap();
    state.lock().unwrap();
    assert!(system_unlock_impl(&state, &(), 0).await.is_err());
    assert!(
        unlock_impl(&state, &(), "master-password".into())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn cancelled_system_auth_does_not_unlock_or_remove_password_access() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets.clone());
    secrets.cancel.store(true, Ordering::SeqCst);
    assert_eq!(
        configure_system_unlock(&state, &(), sid, true, "master-password".into(), 0)
            .await
            .err()
            .unwrap()
            .code,
        "auth_cancelled"
    );
    assert!(!state.snapshot(sid).unwrap().system_unlock.enabled);
    secrets.cancel.store(false, Ordering::SeqCst);
    configure_system_unlock(&state, &(), sid, true, "master-password".into(), 0)
        .await
        .unwrap();
    state.lock().unwrap();
    secrets.cancel.store(true, Ordering::SeqCst);
    assert_eq!(
        system_unlock_impl(&state, &(), 0).await.err().unwrap().code,
        "auth_cancelled"
    );
    assert!(state.vault.lock().unwrap().session.is_none());
    assert!(
        unlock_impl(&state, &(), "master-password".into())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn password_rotation_revokes_system_key_and_reencrypts_all_data() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets.clone());
    configure_system_unlock(&state, &(), sid, true, "master-password".into(), 0)
        .await
        .unwrap();
    let (account, old_key) = {
        let vault = state.vault.lock().unwrap();
        let keys = &vault.session.as_ref().unwrap().keys;
        (
            vault
                .storage
                .unlock_account(&crypto::fingerprint(&keys.header).unwrap()),
            keys.key.clone(),
        )
    };
    let next = change_password_impl(
        &state,
        &(),
        sid,
        "master-password".into(),
        "changed-password".into(),
    )
    .await
    .unwrap();
    assert!(!next.system_unlock.enabled);
    assert!(secrets.memory.unprotect(&account, 0).is_err());
    assert_ne!(
        state
            .vault
            .lock()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .keys
            .key
            .as_ref(),
        old_key.as_ref()
    );
    state.lock().unwrap();
    assert!(system_unlock_impl(&state, &(), 0).await.is_err());
    assert!(
        unlock_impl(&state, &(), "master-password".into())
            .await
            .is_err()
    );
    assert_eq!(
        unlock_impl(&state, &(), "changed-password".into())
            .await
            .unwrap()
            .entries[0]
            .notes,
        "private-notes"
    );
}

#[tokio::test]
async fn credential_store_failure_preserves_local_access_and_never_leaks_cloud_secrets_to_files() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, dir, sid) = fixture(secrets.clone());
    let config = SyncConfig::Webdav {
        endpoint: "https://cloud.example.test".into(),
        directory: "vault".into(),
        username: "private-user".into(),
        password: "private-cloud-password".into(),
    };
    state
        .vault
        .lock()
        .unwrap()
        .mutate(
            sid,
            Action::SetSync {
                config: Some(config),
            },
        )
        .unwrap();
    state.lock().unwrap();
    secrets.fail_read.store(true, Ordering::SeqCst);
    let local = unlock_impl(&state, &(), "master-password".into())
        .await
        .unwrap();
    assert!(local.credential_error.is_some());
    assert_eq!(local.entries[0].key, "private-key");
    assert!(
        change_password_impl(
            &state,
            &(),
            local.session_id,
            "master-password".into(),
            "changed-password".into()
        )
        .await
        .is_err()
    );
    state
        .vault
        .lock()
        .unwrap()
        .mutate(
            local.session_id,
            Action::SaveEntry {
                input: EntryInput {
                    id: Some(local.entries[0].id.clone()),
                    table_id: local.tables[0].id.clone(),
                    key: "private-key".into(),
                    value: "offline-change".into(),
                    notes: "".into(),
                },
            },
        )
        .unwrap();
    for file in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(file.unwrap().path()).unwrap();
        for secret in [
            "private-user",
            "private-cloud-password",
            "private-key",
            "offline-change",
            "私密分类",
        ] {
            assert!(!bytes.windows(secret.len()).any(|w| w == secret.as_bytes()));
        }
    }
    state.lock().unwrap();
    secrets.fail_read.store(false, Ordering::SeqCst);
    let restored = unlock_impl(&state, &(), "master-password".into())
        .await
        .unwrap();
    assert!(restored.credential_error.is_none() && restored.config.is_some());
    assert_eq!(restored.entries[0].value, "offline-change");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_during_enrollment_rejects_response_and_revokes_the_new_os_secret() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets.clone());
    secrets.pause_protect.store(true, Ordering::SeqCst);
    let task_state = state.clone();
    let task = tokio::spawn(async move {
        configure_system_unlock(&task_state, &(), sid, true, "master-password".into(), 0).await
    });
    secrets.wait_for_prompt();
    state.lock().unwrap();
    secrets.release();
    assert_eq!(task.await.unwrap().err().unwrap().code, "locked");
    assert!(
        state
            .vault
            .lock()
            .unwrap()
            .storage
            .preferences()
            .unwrap()
            .system_unlock
            .is_none()
    );
    assert!(
        secrets
            .memory
            .0
            .lock()
            .unwrap()
            .keys()
            .all(|k| !k.starts_with("protected:"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lock_during_native_unlock_rejects_the_late_secret() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets.clone());
    configure_system_unlock(&state, &(), sid, true, "master-password".into(), 0)
        .await
        .unwrap();
    state.lock().unwrap();
    secrets.pause_read.store(true, Ordering::SeqCst);
    let task_state = state.clone();
    let task = tokio::spawn(async move { system_unlock_impl(&task_state, &(), 0).await });
    secrets.wait_for_prompt();
    state.lock().unwrap();
    secrets.release();
    assert_eq!(task.await.unwrap().err().unwrap().code, "locked");
    assert!(state.vault.lock().unwrap().session.is_none());
}

#[test]
fn failed_local_commit_keeps_the_previous_authenticated_cloud_profile() {
    let secrets = Arc::new(ControlledSecrets::default());
    let (state, _dir, sid) = fixture(secrets);
    let config = |host: &str| SyncConfig::Webdav {
        endpoint: format!("https://{host}.example.test"),
        directory: "vault".into(),
        username: "user".into(),
        password: "secret".into(),
    };
    let mut vault = state.vault.lock().unwrap();
    vault
        .mutate(
            sid,
            Action::SetSync {
                config: Some(config("old")),
            },
        )
        .unwrap();
    let before = vault.storage.read(&vault.storage.path()).unwrap();
    let previous = vault.storage.dir.join("vault.previous.kvault");
    std::fs::remove_file(&previous).unwrap();
    std::fs::create_dir(&previous).unwrap(); // Deliberately reject atomic backup replacement.
    assert!(
        vault
            .mutate(
                sid,
                Action::SetSync {
                    config: Some(config("new"))
                }
            )
            .is_err()
    );
    assert_eq!(before, vault.storage.read(&vault.storage.path()).unwrap());
    let session = vault.check(sid).unwrap();
    let mut restored: LocalState = crypto::open(&before, &session.keys, "local").unwrap();
    vault
        .storage
        .restore_device_state(&mut restored, &session.keys.header.vault_id)
        .unwrap();
    assert!(restored.config == Some(config("old")));
    assert!(session.state.config == Some(config("old")));
}
