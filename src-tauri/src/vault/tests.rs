use super::*;

fn fixture() -> (Vault, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::new(dir.path().into()).unwrap();
    let mut vault = Vault {
        storage,
        session: None,
        generation: 0,
        activity: Instant::now(),
        activity_wall: now(),
    };
    vault
        .install(
            Keys::new("test-password", id(), 1).unwrap(),
            LocalState::default(),
        )
        .unwrap();
    (vault, dir)
}

#[test]
fn duplicate_keys_are_scoped_to_tables_and_deletion_cascades() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    vault
        .mutate(sid, Action::CreateTable { name: "A".into() })
        .unwrap();
    vault
        .mutate(sid, Action::CreateTable { name: "B".into() })
        .unwrap();
    let tables = vault.snapshot().unwrap().tables;
    for t in &tables {
        vault
            .mutate(
                sid,
                Action::SaveEntry {
                    input: EntryInput {
                        id: None,
                        table_id: t.id.clone(),
                        key: "key".into(),
                        value: "secret".into(),
                        notes: "".into(),
                    },
                },
            )
            .unwrap();
    }
    assert!(
        vault
            .mutate(
                sid,
                Action::SaveEntry {
                    input: EntryInput {
                        id: None,
                        table_id: tables[0].id.clone(),
                        key: "key".into(),
                        value: "duplicate".into(),
                        notes: "".into()
                    }
                }
            )
            .is_err()
    );
    vault
        .mutate(
            sid,
            Action::DeleteTable {
                id: tables[0].id.clone(),
            },
        )
        .unwrap();
    assert_eq!(vault.snapshot().unwrap().entries.len(), 1);
    assert!(
        vault
            .session
            .as_ref()
            .unwrap()
            .state
            .document
            .entries
            .iter()
            .any(|e| e.deleted)
    );
}

#[test]
fn idle_timeout_also_counts_wall_time_across_system_sleep() {
    let (mut vault, _dir) = fixture();
    assert!(!vault.idle_due());
    vault.activity_wall = now().saturating_sub(301000);
    assert!(vault.idle_due());
    vault.touch();
    assert!(!vault.idle_due());
}

#[test]
fn auto_lock_settings_are_plaintext_device_preferences_and_do_not_rewrite_business_data() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    let mut state = vault.session.as_ref().unwrap().state.clone();
    state.dirty = false;
    vault.persist(state).unwrap();
    let original_bytes = vault.storage.read(&vault.storage.path()).unwrap();
    let before = vault.snapshot().unwrap();
    let settings = AutoLockSettings {
        enabled: false,
        timeout_minutes: 30,
    };
    let snapshot = vault
        .mutate(
            sid,
            Action::SetAutoLock {
                settings: settings.clone(),
            },
        )
        .unwrap();
    assert_eq!(snapshot.auto_lock, settings);
    assert!(!snapshot.dirty);
    assert_eq!(snapshot.revision, before.revision + 1);
    let session = vault.session.as_ref().unwrap();
    let bytes = vault.storage.read(&vault.storage.path()).unwrap();
    assert_eq!(bytes, original_bytes);
    assert!(!String::from_utf8_lossy(&bytes).contains("autoLock"));
    let mut restored: LocalState = crypto::open(&bytes, &session.keys, "local").unwrap();
    assert_eq!(restored.auto_lock, AutoLockSettings::default());
    vault
        .storage
        .restore_device_state(&mut restored, &session.keys.header.vault_id)
        .unwrap();
    assert_eq!(restored.auto_lock, settings);
    let plaintext = std::fs::read_to_string(vault.storage.dir.join("config.json")).unwrap();
    assert!(plaintext.contains("timeoutMinutes"));
    assert!(!plaintext.contains("document"));
    assert_eq!(restored.document, session.state.document);
    assert!(
        serde_json::to_value(&restored.document)
            .unwrap()
            .get("autoLock")
            .is_none()
    );
    let keys = session.keys.clone();
    assert_eq!(vault.install(keys, restored).unwrap().auto_lock, settings);
    let ciphertext = vault.storage.read(&vault.storage.path()).unwrap();
    let revision = vault.snapshot().unwrap().revision;
    for timeout_minutes in [0, 1441, u32::MAX] {
        assert!(
            vault
                .mutate(
                    vault.generation,
                    Action::SetAutoLock {
                        settings: AutoLockSettings {
                            enabled: true,
                            timeout_minutes
                        }
                    }
                )
                .is_err()
        );
        assert_eq!(vault.snapshot().unwrap().revision, revision);
        assert_eq!(
            vault.storage.read(&vault.storage.path()).unwrap(),
            ciphertext
        );
    }
}

#[test]
fn configured_idle_timeout_checks_both_clocks_and_can_be_disabled() {
    let (mut vault, _dir) = fixture();
    vault
        .mutate(
            vault.generation,
            Action::SetAutoLock {
                settings: AutoLockSettings {
                    enabled: true,
                    timeout_minutes: 2,
                },
            },
        )
        .unwrap();
    vault.activity_wall = now().saturating_sub(61000);
    assert!(!vault.idle_due());
    vault.activity_wall = now().saturating_sub(121000);
    assert!(vault.idle_due());
    vault.touch();
    vault.activity = Instant::now() - std::time::Duration::from_secs(121);
    assert!(vault.idle_due());
    vault
        .mutate(
            vault.generation,
            Action::SetAutoLock {
                settings: AutoLockSettings {
                    enabled: false,
                    timeout_minutes: 2,
                },
            },
        )
        .unwrap();
    vault.activity = Instant::now() - std::time::Duration::from_secs(86400);
    vault.activity_wall = now().saturating_sub(86400000);
    assert!(!vault.idle_due());
    vault.lock();
    assert!(vault.session.is_none());
}

#[test]
fn idle_lock_rechecks_latest_policy_and_cancels_the_expired_session() {
    let (vault, dir) = fixture();
    let shared = AppState::new(Storage::new(dir.path().into()).unwrap());
    *shared.vault.lock().unwrap() = vault;
    let sid;
    {
        let mut vault = shared.vault.lock().unwrap();
        sid = vault.generation;
        vault.activity_wall = now().saturating_sub(301000);
        assert!(vault.idle_due());
        vault
            .mutate(
                sid,
                Action::SetAutoLock {
                    settings: AutoLockSettings {
                        enabled: false,
                        timeout_minutes: 1,
                    },
                },
            )
            .unwrap();
    }
    assert!(!shared.lock_if_idle().unwrap());
    let cancel = {
        let mut vault = shared.vault.lock().unwrap();
        vault
            .mutate(
                sid,
                Action::SetAutoLock {
                    settings: AutoLockSettings {
                        enabled: true,
                        timeout_minutes: 1,
                    },
                },
            )
            .unwrap();
        vault.activity_wall = now().saturating_sub(61000);
        vault.session.as_ref().unwrap().cancel.clone()
    };
    shared.schedule(2000);
    assert!(shared.lock_if_idle().unwrap());
    assert!(cancel.is_cancelled());
    assert_eq!(shared.due.load(Ordering::SeqCst), 0);
    assert!(shared.snapshot(sid).is_err());
    assert!(!shared.lock_if_idle().unwrap());
}

#[test]
fn batch_import_duplicate_policies_and_atomic_validation() {
    use super::import::{DuplicatePolicy, ImportRow};
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    vault
        .mutate(
            sid,
            Action::CreateTable {
                name: "导入".into(),
            },
        )
        .unwrap();
    let table_id = vault.snapshot().unwrap().tables[0].id.clone();
    let row = |line, key: &str, value: &str| ImportRow {
        line,
        key: key.into(),
        value: value.into(),
        notes: "导入备注".into(),
    };
    vault
        .mutate(
            sid,
            Action::ImportEntries {
                table_id: table_id.clone(),
                rows: vec![row(1, " token ", "first"), row(2, "token", "skipped")],
                duplicate_policy: DuplicatePolicy::Skip,
            },
        )
        .unwrap();
    let entry = vault.snapshot().unwrap().entries[0].clone();
    assert_eq!(entry.value, "first");
    vault
        .mutate(
            sid,
            Action::ImportEntries {
                table_id: table_id.clone(),
                rows: vec![
                    row(1, "token", "second"),
                    row(2, "token", "last"),
                    row(3, "new", "new secret"),
                ],
                duplicate_policy: DuplicatePolicy::Overwrite,
            },
        )
        .unwrap();
    let snapshot = vault.snapshot().unwrap();
    assert_eq!(snapshot.entries.len(), 2);
    let updated = snapshot.entries.iter().find(|e| e.id == entry.id).unwrap();
    assert_eq!(updated.value, "last");
    assert_eq!(updated.created_at, entry.created_at);
    assert_eq!(updated.notes, "导入备注");
    let before = vault.storage.read(&vault.storage.path()).unwrap();
    assert!(
        vault
            .mutate(
                sid,
                Action::ImportEntries {
                    table_id,
                    rows: vec![row(1, "would-be-partial", "v"), row(2, "  ", "invalid")],
                    duplicate_policy: DuplicatePolicy::Skip
                }
            )
            .is_err()
    );
    assert_eq!(vault.snapshot().unwrap().revision, snapshot.revision);
    assert_eq!(vault.snapshot().unwrap().entries.len(), 2);
    assert_eq!(vault.storage.read(&vault.storage.path()).unwrap(), before);
}

#[test]
fn batch_move_and_delete_are_atomic_and_preserve_identity() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    for name in ["source", "occupied", "target"] {
        vault
            .mutate(sid, Action::CreateTable { name: name.into() })
            .unwrap();
    }
    let tables = vault.snapshot().unwrap().tables;
    let source = tables
        .iter()
        .find(|t| t.name == "source")
        .unwrap()
        .id
        .clone();
    let occupied = tables
        .iter()
        .find(|t| t.name == "occupied")
        .unwrap()
        .id
        .clone();
    let target = tables
        .iter()
        .find(|t| t.name == "target")
        .unwrap()
        .id
        .clone();
    for (table_id, key) in [(&source, "alpha"), (&source, "beta"), (&occupied, "alpha")] {
        vault
            .mutate(
                sid,
                Action::SaveEntry {
                    input: EntryInput {
                        id: None,
                        table_id: table_id.clone(),
                        key: key.into(),
                        value: "secret".into(),
                        notes: "note".into(),
                    },
                },
            )
            .unwrap();
    }
    let before = vault.snapshot().unwrap();
    let originals: Vec<_> = before
        .entries
        .iter()
        .filter(|e| e.table_id == source)
        .cloned()
        .collect();
    let ids: Vec<_> = originals.iter().map(|e| e.id.clone()).collect();
    let ciphertext = vault.storage.read(&vault.storage.path()).unwrap();
    for action in [
        Action::MoveEntries {
            ids: ids.clone(),
            table_id: occupied.clone(),
        },
        Action::MoveEntries {
            ids: vec![ids[0].clone(), "missing".into()],
            table_id: target.clone(),
        },
        Action::MoveEntries {
            ids: ids.clone(),
            table_id: "missing".into(),
        },
        Action::DeleteEntries {
            ids: vec![ids[0].clone(), "missing".into()],
        },
        Action::DeleteEntries { ids: vec![] },
        Action::MoveEntries {
            ids: vec![],
            table_id: target.clone(),
        },
    ] {
        assert!(vault.mutate(sid, action).is_err());
        assert_eq!(vault.snapshot().unwrap().revision, before.revision);
        assert_eq!(
            vault.storage.read(&vault.storage.path()).unwrap(),
            ciphertext
        );
    }
    let moved = vault
        .mutate(
            sid,
            Action::MoveEntries {
                ids: ids.clone(),
                table_id: target.clone(),
            },
        )
        .unwrap();
    assert_eq!(moved.revision, before.revision + 1);
    for original in &originals {
        let entry = moved.entries.iter().find(|e| e.id == original.id).unwrap();
        assert_eq!(entry.table_id, target);
        assert_eq!(entry.key, original.key);
        assert_eq!(entry.value, original.value);
        assert_eq!(entry.notes, original.notes);
        assert_eq!(entry.created_at, original.created_at);
    }
    // Identical Keys selected across two different tables also collide at the target.
    let same_key = moved
        .entries
        .iter()
        .filter(|e| e.key == "alpha")
        .map(|e| e.id.clone())
        .collect();
    let ciphertext = vault.storage.read(&vault.storage.path()).unwrap();
    assert!(
        vault
            .mutate(
                sid,
                Action::MoveEntries {
                    ids: same_key,
                    table_id: source
                }
            )
            .is_err()
    );
    assert_eq!(
        vault.storage.read(&vault.storage.path()).unwrap(),
        ciphertext
    );
    let deleted = vault
        .mutate(sid, Action::DeleteEntries { ids: ids.clone() })
        .unwrap();
    assert_eq!(deleted.revision, moved.revision + 1);
    assert_eq!(deleted.entries.len(), 1);
    let session = vault.session.as_ref().unwrap();
    assert!(
        session
            .state
            .document
            .entries
            .iter()
            .filter(|e| ids.contains(&e.id))
            .all(|e| e.deleted)
    );
    assert!(deleted.dirty);
    let on_disk: LocalState = crypto::open(
        &vault.storage.read(&vault.storage.path()).unwrap(),
        &session.keys,
        "local",
    )
    .unwrap();
    assert_eq!(
        on_disk
            .document
            .entries
            .iter()
            .filter(|e| e.deleted)
            .count(),
        2
    );
}

#[test]
fn table_order_is_validated_encrypted_and_new_tables_append() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    for name in ["A", "B", "C"] {
        vault
            .mutate(sid, Action::CreateTable { name: name.into() })
            .unwrap();
    }
    let original = vault.snapshot().unwrap();
    let ids: Vec<_> = original
        .tables
        .iter()
        .map(|table| table.id.clone())
        .collect();
    let before = vault.storage.read(&vault.storage.path()).unwrap();
    for invalid in [
        vec![],
        vec![ids[0].clone()],
        vec![ids[0].clone(), ids[0].clone(), ids[2].clone()],
        vec![ids[0].clone(), ids[1].clone(), "missing".into()],
    ] {
        assert!(
            vault
                .mutate(sid, Action::ReorderTables { ids: invalid })
                .is_err()
        );
        assert_eq!(vault.storage.read(&vault.storage.path()).unwrap(), before);
        assert_eq!(vault.snapshot().unwrap().revision, original.revision);
    }
    let reversed: Vec<_> = ids.iter().rev().cloned().collect();
    let result = vault
        .mutate(
            sid,
            Action::ReorderTables {
                ids: reversed.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        result
            .tables
            .iter()
            .map(|table| table.id.clone())
            .collect::<Vec<_>>(),
        reversed
    );
    for table in &result.tables {
        let previous = original
            .tables
            .iter()
            .find(|old| old.id == table.id)
            .unwrap();
        assert_eq!(table.name, previous.name);
        assert_eq!(table.updated_at, previous.updated_at);
    }
    let stored: LocalState = crypto::open(
        &vault.storage.read(&vault.storage.path()).unwrap(),
        &vault.session.as_ref().unwrap().keys,
        "local",
    )
    .unwrap();
    let mut restored = stored.document.tables;
    restored.sort_by(model::compare_table_order);
    assert_eq!(restored, result.tables);
    let result = vault
        .mutate(sid, Action::CreateTable { name: "D".into() })
        .unwrap();
    assert_eq!(result.tables.last().unwrap().name, "D");
    let deleted = result.tables[1].id.clone();
    vault
        .mutate(sid, Action::DeleteTable { id: deleted })
        .unwrap();
    let mut live: Vec<_> = vault
        .snapshot()
        .unwrap()
        .tables
        .iter()
        .map(|table| table.id.clone())
        .collect();
    live.reverse();
    assert!(
        vault
            .mutate(sid, Action::ReorderTables { ids: live })
            .is_ok()
    );
    let mut legacy = serde_json::to_value(&original.tables[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("sortOrder");
    assert_eq!(
        serde_json::from_value::<Table>(legacy).unwrap().sort_order,
        0
    );
}

#[test]
fn custom_order_is_atomic_scoped_and_encrypted_with_legacy_compatibility() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    for name in ["A", "B"] {
        vault
            .mutate(sid, Action::CreateTable { name: name.into() })
            .unwrap();
    }
    let tables = vault.snapshot().unwrap().tables;
    for (index, table_id) in [
        tables[0].id.clone(),
        tables[1].id.clone(),
        tables[0].id.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        vault
            .mutate(
                sid,
                Action::SaveEntry {
                    input: EntryInput {
                        id: None,
                        table_id,
                        key: format!("key-{index}"),
                        value: "secret".into(),
                        notes: "note".into(),
                    },
                },
            )
            .unwrap();
    }
    let mut original = vault.snapshot().unwrap().entries;
    original.sort_by(model::compare_entry_order);
    let ids: Vec<_> = original.iter().map(|e| e.id.clone()).collect();
    let before = vault.storage.read(&vault.storage.path()).unwrap();
    let revision = vault.snapshot().unwrap().revision;
    for action in [
        Action::ReorderEntries {
            ids: vec![ids[0].clone()],
            table_id: None,
        },
        Action::ReorderEntries {
            ids: vec![ids[0].clone(), ids[0].clone(), ids[2].clone()],
            table_id: None,
        },
        Action::ReorderEntries {
            ids: vec!["missing".into(), ids[1].clone(), ids[2].clone()],
            table_id: None,
        },
        Action::ReorderEntries {
            ids: ids.clone(),
            table_id: Some(tables[0].id.clone()),
        },
        Action::ReorderEntries {
            ids: ids.clone(),
            table_id: Some("missing".into()),
        },
    ] {
        assert!(vault.mutate(sid, action).is_err());
        assert_eq!(vault.storage.read(&vault.storage.path()).unwrap(), before);
        assert_eq!(vault.snapshot().unwrap().revision, revision);
    }
    let snapshot = vault
        .mutate(
            sid,
            Action::ReorderEntries {
                ids: vec![ids[2].clone(), ids[0].clone()],
                table_id: Some(tables[0].id.clone()),
            },
        )
        .unwrap();
    let mut ordered = snapshot.entries;
    ordered.sort_by(model::compare_entry_order);
    assert_eq!(
        ordered.iter().map(|e| &e.id).collect::<Vec<_>>(),
        [&ids[2], &ids[1], &ids[0]]
    );
    assert_eq!(snapshot.revision, revision + 1);
    for entry in &ordered {
        let previous = original.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(entry.value, previous.value);
        assert_eq!(entry.notes, previous.notes);
        assert_eq!(entry.updated_at, previous.updated_at);
    }
    let keys = &vault.session.as_ref().unwrap().keys;
    let stored: LocalState = crypto::open(
        &vault.storage.read(&vault.storage.path()).unwrap(),
        keys,
        "local",
    )
    .unwrap();
    let mut restored = stored.document.entries;
    restored.sort_by(model::compare_entry_order);
    assert_eq!(restored, ordered);
    let mut legacy = serde_json::to_value(&original[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("sortOrder");
    assert_eq!(
        serde_json::from_value::<Entry>(legacy).unwrap().sort_order,
        0
    );
    let reversed: Vec<_> = ids.into_iter().rev().collect();
    let result = vault
        .mutate(
            sid,
            Action::ReorderEntries {
                ids: reversed.clone(),
                table_id: None,
            },
        )
        .unwrap();
    let mut all = result.entries;
    all.sort_by(model::compare_entry_order);
    assert_eq!(
        all.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        reversed
    );
}

#[test]
fn three_duplicate_records_can_be_resolved_without_stale_conflicts() {
    let (mut vault, _dir) = fixture();
    let sid = vault.generation;
    vault
        .mutate(sid, Action::CreateTable { name: "A".into() })
        .unwrap();
    let table_id = vault.snapshot().unwrap().tables[0].id.clone();
    let mut state = vault.session.as_ref().unwrap().state.clone();
    for _ in 0..3 {
        state.document.entries.push(Entry {
            id: id(),
            table_id: table_id.clone(),
            key: "same".into(),
            value: "v".into(),
            notes: "".into(),
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
            deleted: false,
        });
    }
    state.conflicts = crate::sync::merge::duplicates(&state.document);
    vault.persist(state).unwrap();
    while let Some(c) = vault.snapshot().unwrap().conflicts.first() {
        vault
            .mutate(
                sid,
                Action::ResolveConflict {
                    id: c.id.clone(),
                    choice: "remote".into(),
                    rename: "".into(),
                },
            )
            .unwrap();
    }
    assert_eq!(vault.snapshot().unwrap().entries.len(), 1);
}

/// Seed only an explicitly selected disposable directory for native UI acceptance.
#[test]
#[ignore]
fn seed_native_qa_fixture() {
    let dir = std::path::PathBuf::from(
        std::env::var("KVAULT_QA_DIR")
            .expect("set KVAULT_QA_DIR to a disposable .qa-vault directory"),
    );
    assert_eq!(dir.file_name().unwrap(), ".qa-vault");
    let storage = Storage::new(dir).unwrap();
    assert!(!storage.exists(), "never overwrite an existing vault");
    let keys = Keys::new("kvault-test-only", id(), 1).unwrap();
    let mut state = LocalState::default();
    for name in ["开发配置", "个人", "工作"] {
        let table_id = id();
        state.document.tables.push(Table {
            id: table_id.clone(),
            name: name.into(),
            sort_order: 0,
            created_at: now(),
            updated_at: now(),
            deleted: false,
        });
        if name == "开发配置" {
            for (key, notes) in [
                ("api.production.token", "生产环境接口凭据"),
                ("database.connection", "本地开发数据库连接信息"),
                ("ssh.server.host", "开发服务器地址"),
            ] {
                state.document.entries.push(Entry {
                    id: id(),
                    table_id: table_id.clone(),
                    key: key.into(),
                    value: "disposable-test-value".into(),
                    notes: notes.into(),
                    sort_order: 0,
                    created_at: now(),
                    updated_at: now(),
                    deleted: false,
                });
            }
        }
    }
    storage.save(&state, &keys).unwrap();
}
