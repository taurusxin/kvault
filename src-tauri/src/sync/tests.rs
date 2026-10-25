use crate::{
    commands::{change_password_impl, migrate_impl, unlock_impl},
    sync::{REMOTE_NAME, SyncConfig, remote::RemoteStore, service::sync, test_support::Server},
    vault::{
        Action,
        crypto::{self, Keys},
        model::{Document, EntryInput, LocalState},
    },
};
use std::sync::atomic::Ordering;

fn clone_device(state: &crate::vault::Shared) -> (crate::vault::Shared, tempfile::TempDir, u64) {
    let dir = tempfile::tempdir().unwrap();
    let cloned = std::sync::Arc::new(crate::vault::AppState::new(
        crate::vault::storage::Storage::new(dir.path().into()).unwrap(),
    ));
    let vault = state.vault.lock().unwrap();
    let s = vault.session.as_ref().unwrap();
    let mut target = cloned.vault.lock().unwrap();
    let mut local = s.state.clone();
    local.sync_profile = target
        .storage
        .save_sync(&s.keys.header.vault_id, s.state.config.as_ref())
        .unwrap();
    target.storage.save(&local, &s.keys).unwrap();
    let snapshot = target.install(s.keys.clone(), local).unwrap();
    drop(target);
    (cloned, dir, snapshot.session_id)
}
fn write_entry(state: &crate::vault::Shared, sid: u64, entry_id: Option<String>, value: &str) {
    let mut vault = state.vault.lock().unwrap();
    let table_id = vault.snapshot().unwrap().tables[0].id.clone();
    vault
        .mutate(
            sid,
            Action::SaveEntry {
                input: EntryInput {
                    id: entry_id,
                    table_id,
                    key: "token".into(),
                    value: value.into(),
                    notes: "".into(),
                },
            },
        )
        .unwrap();
}

#[tokio::test]
async fn two_devices_preserve_conflicts_and_keep_both_with_unique_keys() {
    let server = Server::start().await;
    let (a, _da, aid) = server.fixture();
    write_entry(&a, aid, None, "original");
    sync(&a, &(), aid).await.unwrap();
    let (b, _db, bid) = clone_device(&a);
    let entry_id = a.snapshot(aid).unwrap().entries[0].id.clone();
    write_entry(&a, aid, Some(entry_id.clone()), "device A");
    write_entry(&b, bid, Some(entry_id), "device B");
    sync(&a, &(), aid).await.unwrap();
    let conflicted = sync(&b, &(), bid).await.unwrap();
    assert_eq!(conflicted.conflicts.len(), 1);
    b.vault
        .lock()
        .unwrap()
        .mutate(
            bid,
            Action::ResolveConflict {
                id: conflicted.conflicts[0].id.clone(),
                choice: "both".into(),
                rename: "token.from-a".into(),
            },
        )
        .unwrap();
    sync(&b, &(), bid).await.unwrap();
    let result = sync(&a, &(), aid).await.unwrap();
    assert!(result.conflicts.is_empty());
    assert_eq!(result.entries.len(), 2);
    assert!(result.entries.iter().any(|e| e.value == "device A"));
    assert!(result.entries.iter().any(|e| e.value == "device B"));
}

#[tokio::test]
async fn another_devices_rotation_preserves_old_offline_edits_during_migration() {
    let server = Server::start().await;
    let (a, _da, aid) = server.fixture();
    sync(&a, &(), aid).await.unwrap();
    let (b, _db, bid) = clone_device(&a);
    write_entry(&b, bid, None, "offline edit");
    change_password_impl(&a, &(), aid, "old-password".into(), "new-password".into())
        .await
        .unwrap();
    assert_eq!(sync(&b, &(), bid).await.err().unwrap().code, "key_changed");
    let migrated = migrate_impl(&b, &(), bid, "new-password".into())
        .await
        .unwrap();
    assert_eq!(migrated.epoch, 2);
    assert_eq!(migrated.entries[0].value, "offline edit");
    sync(&b, &(), migrated.session_id).await.unwrap();
    b.lock().unwrap();
    assert_eq!(
        unlock_impl(&b, &(), "new-password".into())
            .await
            .unwrap()
            .entries[0]
            .value,
        "offline edit"
    );
}

#[tokio::test]
async fn both_protocols_check_real_conditions_and_cleanup() {
    let server = Server::start().await;
    let dav = RemoteStore::new(&server.config()).unwrap();
    dav.probe().await.unwrap();
    assert!(server.data.files.lock().unwrap().is_empty());
    let config = SyncConfig::S3 {
        endpoint: server.endpoint.clone(),
        region: "us-east-1".into(),
        bucket: "bucket".into(),
        access_key: "test".into(),
        secret_key: "test-secret".into(),
        session_token: None,
        prefix: "nested/vault".into(),
        path_style: true,
    };
    let s3 = RemoteStore::new(&config).unwrap();
    s3.probe().await.unwrap();
    assert!(server.data.signed_requests.load(Ordering::SeqCst) >= 6);
    assert!(server.data.files.lock().unwrap().is_empty());
    server.data.ignore_conditions.store(true, Ordering::SeqCst);
    assert_eq!(dav.probe().await.unwrap_err().code, "unsupported");
    assert!(server.data.files.lock().unwrap().is_empty());
}

#[tokio::test]
async fn editing_during_upload_is_not_acknowledged_or_lost() {
    let server = Server::start().await;
    let (state, _dir, sid) = server.fixture();
    server.data.pause_upload.store(true, Ordering::SeqCst);
    let task = tokio::spawn({
        let state = state.clone();
        async move { sync(&state, &(), sid).await }
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        server.data.entered.notified(),
    )
    .await
    .unwrap();
    {
        let mut vault = state.vault.lock().unwrap();
        let table_id = vault.snapshot().unwrap().tables[0].id.clone();
        vault
            .mutate(
                sid,
                Action::SaveEntry {
                    input: EntryInput {
                        id: None,
                        table_id,
                        key: "created-during-upload".into(),
                        value: "local secret".into(),
                        notes: "".into(),
                    },
                },
            )
            .unwrap();
    }
    server.data.release.notify_one();
    assert!(task.await.unwrap().unwrap().dirty);
    assert_eq!(state.snapshot(sid).unwrap().entries.len(), 1);
    sync(&state, &(), sid).await.unwrap();
    assert!(!state.snapshot(sid).unwrap().dirty);
    let file = RemoteStore::new(&server.config())
        .unwrap()
        .fetch(REMOTE_NAME)
        .await
        .unwrap()
        .unwrap();
    let vault = state.vault.lock().unwrap();
    let keys = &vault.check(sid).unwrap().keys;
    let doc: Document = crypto::open(&file.bytes, keys, "snapshot").unwrap();
    assert_eq!(doc.entries[0].value, "local secret");
    assert!(!String::from_utf8_lossy(&file.bytes).contains("private-service-password"));
}

#[tokio::test]
async fn password_rotation_reencrypts_disk_and_remote() {
    let server = Server::start().await;
    let (state, _dir, sid) = server.fixture();
    let snapshot = change_password_impl(
        &state,
        &(),
        sid,
        "old-password".into(),
        "new-password".into(),
    )
    .await
    .unwrap();
    assert_eq!(snapshot.epoch, 2);
    assert!(!snapshot.recovery_pending);
    state.lock().unwrap();
    assert!(
        unlock_impl(&state, &(), "old-password".into())
            .await
            .is_err()
    );
    let snapshot = unlock_impl(&state, &(), "new-password".into())
        .await
        .unwrap();
    let file = RemoteStore::new(&server.config())
        .unwrap()
        .fetch(REMOTE_NAME)
        .await
        .unwrap()
        .unwrap();
    let vault = state.vault.lock().unwrap();
    let keys = &vault.check(snapshot.session_id).unwrap().keys;
    assert!(crypto::open::<Document>(&file.bytes, keys, "snapshot").is_ok());
}

#[tokio::test]
async fn rejected_rotation_keeps_old_password_and_discards_stage() {
    let server = Server::start().await;
    let (state, _dir, sid) = server.fixture();
    sync(&state, &(), sid).await.unwrap();
    server.data.reject_put.store(true, Ordering::SeqCst);
    assert!(
        change_password_impl(
            &state,
            &(),
            sid,
            "old-password".into(),
            "new-password".into()
        )
        .await
        .is_err()
    );
    assert!(!state.snapshot(sid).unwrap().recovery_pending);
    assert!(!state.vault.lock().unwrap().storage.pending().exists());
    state.lock().unwrap();
    assert!(
        unlock_impl(&state, &(), "old-password".into())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn ambiguous_rotation_recovers_after_restart_with_new_password() {
    let server = Server::start().await;
    let (state, dir, sid) = server.fixture();
    sync(&state, &(), sid).await.unwrap();
    server.data.ambiguous_put.store(true, Ordering::SeqCst);
    assert_eq!(
        change_password_impl(
            &state,
            &(),
            sid,
            "old-password".into(),
            "new-password".into()
        )
        .await
        .err()
        .unwrap()
        .code,
        "recovery_pending"
    );
    assert!(state.snapshot(sid).unwrap().recovery_pending);
    state.lock().unwrap();
    let restarted = std::sync::Arc::new(crate::vault::AppState::new(
        crate::vault::storage::Storage::with_secrets(
            dir.path().into(),
            state.vault.lock().unwrap().storage.secrets.clone(),
        )
        .unwrap(),
    ));
    let snapshot = unlock_impl(&restarted, &(), "new-password".into())
        .await
        .unwrap();
    assert_eq!(snapshot.epoch, 2);
    assert!(!snapshot.recovery_pending);
    assert!(!restarted.vault.lock().unwrap().storage.pending().exists());
}

#[tokio::test]
async fn staged_but_uncommitted_rotation_rolls_back_with_old_password() {
    let server = Server::start().await;
    let (state, _dir, sid) = server.fixture();
    sync(&state, &(), sid).await.unwrap();
    {
        let mut vault = state.vault.lock().unwrap();
        let s = vault.check(sid).unwrap();
        let keys = Keys::new("new-password", s.keys.header.vault_id.clone(), 2).unwrap();
        let mut local = s.state.clone();
        local.rotation = Some(crate::vault::model::Rotation {
            remote_hash: "pending".into(),
            previous_etag: None,
            epoch: 2,
        });
        vault.storage.stage(&local, &keys).unwrap();
        vault.persist(local).unwrap();
    }
    state.lock().unwrap();
    let snapshot = unlock_impl(&state, &(), "old-password".into())
        .await
        .unwrap();
    assert_eq!(snapshot.epoch, 1);
    assert!(!snapshot.recovery_pending);
}

#[tokio::test]
async fn locked_session_rejects_stale_commands_and_local_save_is_independent() {
    let server = Server::start().await;
    let (state, _dir, sid) = server.fixture();
    state
        .vault
        .lock()
        .unwrap()
        .mutate(
            sid,
            Action::CreateTable {
                name: "离线可写".into(),
            },
        )
        .unwrap();
    let file = {
        let vault = state.vault.lock().unwrap();
        vault.storage.read(&vault.storage.path()).unwrap()
    };
    assert!(!String::from_utf8_lossy(&file).contains("离线可写"));
    state.lock().unwrap();
    assert!(state.snapshot(sid).is_err());
    assert!(
        state
            .vault
            .lock()
            .unwrap()
            .mutate(
                sid,
                Action::CreateTable {
                    name: "stale".into()
                }
            )
            .is_err()
    );
    let snapshot = unlock_impl(&state, &(), "old-password".into())
        .await
        .unwrap();
    assert_eq!(snapshot.tables.len(), 2);
    let vault = state.vault.lock().unwrap();
    let bytes = vault.storage.read(&vault.storage.path()).unwrap();
    let _: LocalState = crypto::open(
        &bytes,
        &vault.check(snapshot.session_id).unwrap().keys,
        "local",
    )
    .unwrap();
}
