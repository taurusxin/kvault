use crate::{
    error::{AppError, Result},
    sync::{
        REMOTE_NAME, SyncConfig,
        merge::merge,
        remote::RemoteStore,
        service::{self, cancellable, changed, status},
    },
    vault::{
        Action, Bootstrap, Shared,
        crypto::{self, Keys},
        import::{self, Delimiter, HeaderMode, ImportPreview, MAX_IMPORT_BYTES},
        model::{Document, LocalState, Rotation, Snapshot, Table, id, now, validate_document},
    },
};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use zeroize::Zeroizing;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| AppError::new("internal", "后台任务执行失败"))?
}

#[tauri::command]
pub fn bootstrap(state: State<'_, Shared>) -> Result<Bootstrap> {
    let vault = state.vault.lock().map_err(AppError::storage)?;
    Ok(Bootstrap {
        exists: vault.storage.exists(),
        recovery_pending: vault.storage.pending().exists(),
        platform: std::env::consts::OS.into(),
    })
}

#[tauri::command]
pub async fn create_vault(
    state: State<'_, Shared>,
    app: AppHandle,
    password: String,
) -> Result<Snapshot> {
    let password = Zeroizing::new(password);
    let shared = state.inner().clone();
    let generation = shared.vault.lock().map_err(AppError::storage)?.generation;
    let snapshot = blocking(move || {
        let keys = Keys::new(&password, id(), 1)?;
        let mut vault = shared.vault.lock().map_err(AppError::storage)?;
        if vault.generation != generation {
            return Err(AppError::locked());
        }
        if vault.storage.exists() {
            return Err(AppError::invalid("已有保险库，请解锁或导入备份"));
        }
        let mut local = LocalState::default();
        local.document.tables.push(Table {
            id: id(),
            name: "默认表".into(),
            created_at: now(),
            updated_at: now(),
            deleted: false,
        });
        vault.storage.save(&local, &keys)?;
        vault.install(keys, local)
    })
    .await?;
    status(&app, "local", "已保存在本地", None);
    changed(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn unlock_vault(
    state: State<'_, Shared>,
    app: AppHandle,
    password: String,
) -> Result<Snapshot> {
    unlock_impl(state.inner(), &app, password).await
}

pub(crate) async fn unlock_impl(
    shared: &Shared,
    app: &impl service::Events,
    password: String,
) -> Result<Snapshot> {
    let password = Zeroizing::new(password);
    let (keys, mut local, from_pending, generation) = blocking({
        let shared = shared.clone();
        move || {
            let vault = shared.vault.lock().map_err(AppError::storage)?;
            if vault.session.is_some() {
                return Err(AppError::invalid("保险库已解锁，请先锁定再尝试恢复"));
            }
            for (path, pending) in [
                (vault.storage.path(), false),
                (vault.storage.pending(), true),
            ] {
                if !path.exists() {
                    continue;
                }
                let bytes = vault.storage.read(&path)?;
                let keys = Keys::derive(&password, crypto::header(&bytes, "local")?)?;
                if let Ok(local) = crypto::open::<LocalState>(&bytes, &keys, "local") {
                    return Ok((keys, local, pending, vault.generation));
                }
            }
            Err(AppError::new("decrypt", "密码不正确，或保险库内容已损坏"))
        }
    })
    .await?;
    if local.conflicts.is_empty() {
        validate_document(&local.document)?;
    }
    if local.rotation.is_some() {
        let config = local
            .config
            .as_ref()
            .ok_or_else(|| AppError::new("recovery_pending", "缺少换密恢复所需的同步配置"))?;
        let remote = RemoteStore::new(config)?;
        match remote.fetch(REMOTE_NAME).await {
            Ok(Some(file)) => {
                let h = crypto::header(&file.bytes, "snapshot")?;
                if h.vault_id != keys.header.vault_id {
                    return Err(AppError::new(
                        "different_vault",
                        "云端保险库标识不匹配，已停止恢复",
                    ));
                }
                if h == keys.header {
                    let document: Document = crypto::open(&file.bytes, &keys, "snapshot")?;
                    validate_document(&document)?;
                    if from_pending {
                        let (merged, conflicts) =
                            merge(&local.baseline, &local.document, &document);
                        local.document = merged;
                        local.conflicts = conflicts;
                        local.baseline = document;
                        local.etag = Some(file.etag);
                        local.dirty = local.document != local.baseline;
                        local.last_synced_at = Some(now());
                    }
                    local.rotation = None;
                } else if from_pending {
                    return Err(AppError::new(
                        "recovery_pending",
                        "云端换密尚未提交，请使用旧密码解锁以回滚",
                    ));
                }
                // Old password can unlock a read-only local copy until the new password is supplied.
            }
            Ok(None) => {
                return Err(AppError::new(
                    "remote_missing",
                    "恢复期间云端保险库不存在，已保留本地文件",
                ));
            }
            Err(e) if from_pending => return Err(e),
            Err(_) => {}
        }
    } else if from_pending {
        return Err(AppError::new(
            "recovery_pending",
            "换密暂存文件缺少恢复信息",
        ));
    }
    let snapshot = {
        let mut vault = shared.vault.lock().map_err(AppError::storage)?;
        if vault.generation != generation {
            return Err(AppError::locked());
        }
        if local.rotation.is_none() {
            vault.storage.save(&local, &keys)?;
            vault.storage.discard_pending();
        }
        vault.install(keys, local)?
    };
    shared.schedule(0);
    changed(app, &snapshot);
    status(
        app,
        if snapshot.recovery_pending {
            "recovery"
        } else if snapshot.config.is_some() {
            "pending"
        } else {
            "local"
        },
        if snapshot.recovery_pending {
            "密码变更待核验，请输入新密码恢复或联网重试"
        } else {
            "保险库已解锁"
        },
        snapshot.last_synced_at,
    );
    Ok(snapshot)
}

#[tauri::command]
pub fn lock_vault(state: State<'_, Shared>, app: AppHandle) -> Result<()> {
    state.lock()?;
    let _ = app.emit("vault:locked", ());
    status(&app, "locked", "保险库已锁定", None);
    Ok(())
}

#[tauri::command]
pub fn touch_vault(state: State<'_, Shared>, session_id: u64) -> Result<()> {
    let mut vault = state.vault.lock().map_err(AppError::storage)?;
    vault.check(session_id)?;
    vault.touch();
    Ok(())
}

#[tauri::command]
pub async fn mutate_vault(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
    mut action: Action,
) -> Result<Snapshot> {
    let local_only = matches!(&action, Action::SetAutoLock { .. });
    if let Action::SetSync {
        config: Some(config),
    } = &mut action
    {
        *config = {
            let vault = state.vault.lock().map_err(AppError::storage)?;
            vault.check(session_id)?;
            vault.hydrate_config(config.clone())
        };
        let token = {
            let vault = state.vault.lock().map_err(AppError::storage)?;
            vault.check(session_id)?.cancel.clone()
        };
        cancellable(&token, RemoteStore::new(config)?.probe()).await?;
        let remote = cancellable(&token, RemoteStore::new(config)?.fetch(REMOTE_NAME)).await?;
        if let Some(file) = remote {
            let h = crypto::header(&file.bytes, "snapshot")?;
            let vault = state.vault.lock().map_err(AppError::storage)?;
            let session = vault.check(session_id)?;
            if h.vault_id != session.keys.header.vault_id {
                return Err(AppError::new(
                    "different_vault",
                    "该目标已有不同的保险库，已拒绝连接；可在首次设置中连接已有云端保险库",
                ));
            }
            if h != session.keys.header {
                return Err(AppError::new(
                    "key_changed",
                    "目标保险库密码版本不同，请先迁移主密码",
                ));
            }
        }
    }
    let shared = state.inner().clone();
    let snapshot = blocking(move || {
        shared
            .vault
            .lock()
            .map_err(AppError::storage)?
            .mutate(session_id, action)
    })
    .await?;
    if local_only {
        changed(&app, &snapshot);
        return Ok(snapshot);
    }
    state.schedule(2000);
    changed(&app, &snapshot);
    status(
        &app,
        if !snapshot.conflicts.is_empty() {
            "conflict"
        } else if snapshot.config.is_some() {
            "pending"
        } else {
            "local"
        },
        if snapshot.config.is_some() {
            "修改已保存，等待同步"
        } else {
            "已保存在本地"
        },
        snapshot.last_synced_at,
    );
    Ok(snapshot)
}

#[tauri::command]
pub async fn test_connection(
    state: State<'_, Shared>,
    config: SyncConfig,
    session_id: Option<u64>,
) -> Result<()> {
    let (config, token) = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        if let Some(id) = session_id {
            let s = vault.check(id)?;
            (vault.hydrate_config(config), Some(s.cancel.clone()))
        } else {
            (config, None)
        }
    };
    let remote = RemoteStore::new(&config)?;
    if let Some(token) = token {
        cancellable(&token, remote.probe()).await
    } else {
        remote.probe().await
    }
}

#[tauri::command]
pub async fn connect_remote(
    state: State<'_, Shared>,
    app: AppHandle,
    config: SyncConfig,
    password: String,
) -> Result<Snapshot> {
    let password = Zeroizing::new(password);
    let generation = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        if vault.storage.exists() {
            return Err(AppError::invalid("本机已有保险库，不能覆盖连接"));
        }
        vault.generation
    };
    let remote = RemoteStore::new(&config)?;
    remote.probe().await?;
    let file = remote.fetch(REMOTE_NAME).await?.ok_or_else(|| {
        AppError::new(
            "remote_missing",
            "目标中没有保险库，请先在其他设备创建并同步",
        )
    })?;
    let shared = state.inner().clone();
    let snapshot = blocking(move || {
        let keys = Keys::derive(&password, crypto::header(&file.bytes, "snapshot")?)?;
        let doc: Document = crypto::open(&file.bytes, &keys, "snapshot")?;
        validate_document(&doc)?;
        let local = LocalState {
            document: doc.clone(),
            baseline: doc,
            config: Some(config),
            etag: Some(file.etag),
            dirty: false,
            last_synced_at: Some(now()),
            ..Default::default()
        };
        let mut vault = shared.vault.lock().map_err(AppError::storage)?;
        if vault.generation != generation {
            return Err(AppError::locked());
        }
        if vault.storage.exists() {
            return Err(AppError::invalid("已有本地保险库，已取消连接"));
        }
        vault.storage.save(&local, &keys)?;
        vault.install(keys, local)
    })
    .await?;
    state.schedule(60000);
    changed(&app, &snapshot);
    status(&app, "synced", "已连接云端保险库", snapshot.last_synced_at);
    Ok(snapshot)
}

#[tauri::command]
pub async fn sync_now(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
) -> Result<Snapshot> {
    service::sync(state.inner(), &app, session_id).await
}

#[tauri::command]
pub async fn copy_value(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
    entry_id: String,
) -> Result<()> {
    copy_entry_field(state.inner(), &app, session_id, &entry_id, false)
}

#[tauri::command]
pub fn copy_key(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
    entry_id: String,
) -> Result<()> {
    copy_entry_field(state.inner(), &app, session_id, &entry_id, true)
}

fn copy_entry_field(
    state: &Shared,
    app: &AppHandle,
    session_id: u64,
    entry_id: &str,
    key: bool,
) -> Result<()> {
    let vault = state.vault.lock().map_err(AppError::storage)?;
    let session = vault.check(session_id)?;
    let entry = session
        .state
        .document
        .entries
        .iter()
        .find(|e| e.id == entry_id && !e.deleted)
        .ok_or_else(|| AppError::invalid("记录不存在"))?;
    app.clipboard()
        .write_text(if key { &entry.key } else { &entry.value })
        .map_err(|_| AppError::new("clipboard", "无法写入剪贴板"))
}

#[derive(serde::Serialize)]
pub struct ImportFile {
    name: String,
    text: String,
}

#[tauri::command]
pub async fn select_import_csv(
    state: State<'_, Shared>,
    session_id: u64,
) -> Result<Option<ImportFile>> {
    state.snapshot(session_id)?;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("选择 CSV 文件")
        .add_filter("UTF-8 CSV 文件", &["csv"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let shared = state.inner().clone();
    blocking(move || {
        shared.snapshot(session_id)?;
        let meta = std::fs::metadata(file.path()).map_err(AppError::storage)?;
        if meta.len() > MAX_IMPORT_BYTES as u64 {
            return Err(AppError::invalid("CSV 文件不能超过 10 MiB"));
        }
        let bytes = std::fs::read(file.path()).map_err(AppError::storage)?;
        if bytes.len() > MAX_IMPORT_BYTES {
            return Err(AppError::invalid("CSV 文件不能超过 10 MiB"));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| AppError::invalid("CSV 文件需使用 UTF-8 编码，请转换编码后重试"))?;
        shared.snapshot(session_id)?;
        Ok(Some(ImportFile {
            name: file.file_name(),
            text,
        }))
    })
    .await
}

#[tauri::command]
pub async fn preview_import(
    state: State<'_, Shared>,
    session_id: u64,
    text: String,
    delimiter: Delimiter,
    header: HeaderMode,
) -> Result<ImportPreview> {
    state.snapshot(session_id)?;
    let shared = state.inner().clone();
    let text = Zeroizing::new(text);
    blocking(move || {
        let preview = import::parse(&text, delimiter, header)?;
        shared.snapshot(session_id)?;
        Ok(preview)
    })
    .await
}

#[tauri::command]
pub async fn change_password(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
    old_password: String,
    new_password: String,
) -> Result<Snapshot> {
    change_password_impl(state.inner(), &app, session_id, old_password, new_password).await
}

pub(crate) async fn change_password_impl(
    state: &Shared,
    app: &impl service::Events,
    session_id: u64,
    old_password: String,
    new_password: String,
) -> Result<Snapshot> {
    let old_password = Zeroizing::new(old_password);
    let new_password = Zeroizing::new(new_password);
    let _guard = state.sync_gate.lock().await;
    service::sync_inner(state, app, session_id).await?;
    let (old_keys, local, token) = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        let s = vault.check(session_id)?;
        (s.keys.clone(), s.state.clone(), s.cancel.clone())
    };
    if !local.conflicts.is_empty() || local.rotation.is_some() {
        return Err(AppError::new(
            "conflict",
            "请先处理同步冲突或未完成的密码变更",
        ));
    }
    let (new_keys, verified_revision) = blocking(move || {
        let verified = Keys::derive(&old_password, old_keys.header.clone())?;
        let check = crypto::seal(&true, &old_keys, "password-check")?;
        crypto::open::<bool>(&check, &verified, "password-check")?;
        Ok((
            Keys::new(
                &new_password,
                old_keys.header.vault_id,
                old_keys.header.epoch + 1,
            )?,
            local.revision,
        ))
    })
    .await?;
    let config = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        vault.check(session_id)?.state.config.clone()
    };
    let Some(config) = config else {
        let mut vault = state.vault.lock().map_err(AppError::storage)?;
        let mut next = vault.check(session_id)?.state.clone();
        next.revision += 1;
        vault.storage.save(&next, &new_keys)?;
        let snapshot = vault.install(new_keys, next)?;
        changed(app, &snapshot);
        return Ok(snapshot);
    };
    let remote = RemoteStore::new(&config)?;
    let current = cancellable(&token, remote.fetch(REMOTE_NAME))
        .await?
        .ok_or_else(|| AppError::new("remote_missing", "云端保险库不存在，请先完成同步"))?;
    let new_remote;
    {
        let mut vault = state.vault.lock().map_err(AppError::storage)?;
        let session = vault.check(session_id)?;
        if session.state.revision != verified_revision {
            return Err(AppError::new(
                "precondition",
                "保险库在换密期间发生修改，请重试",
            ));
        }
        let remote_doc: Document = crypto::open(&current.bytes, &session.keys, "snapshot")?;
        if remote_doc != session.state.document {
            return Err(AppError::new(
                "precondition",
                "云端出现新修改，请同步后重试换密",
            ));
        }
        new_remote = crypto::seal(&session.state.document, &new_keys, "snapshot")?;
        let rotation = Rotation {
            remote_hash: format!("{:x}", Sha256::digest(&new_remote)),
            previous_etag: Some(current.etag.clone()),
            epoch: new_keys.header.epoch,
        };
        let mut next = session.state.clone();
        next.rotation = Some(rotation.clone());
        next.baseline = next.document.clone();
        next.dirty = false;
        next.revision += 1;
        vault.storage.stage(&next, &new_keys)?;
        let mut journal = session.state.clone();
        journal.rotation = Some(rotation);
        vault.persist(journal)?;
    }
    let upload = cancellable(
        &token,
        remote.put(REMOTE_NAME, new_remote, Some(&current.etag)),
    )
    .await;
    let verify = cancellable(&token, remote.fetch(REMOTE_NAME)).await;
    match verify {
        Ok(Some(file)) if crypto::header(&file.bytes, "snapshot")? == new_keys.header => {
            let remote_doc: Document = crypto::open(&file.bytes, &new_keys, "snapshot")?;
            validate_document(&remote_doc)?;
            let mut vault = state.vault.lock().map_err(AppError::storage)?;
            vault.check(session_id)?;
            let bytes = vault.storage.read(&vault.storage.pending())?;
            let mut next: LocalState = crypto::open(&bytes, &new_keys, "local")?;
            let (doc, conflicts) = merge(&next.baseline, &next.document, &remote_doc);
            next.document = doc;
            next.conflicts = conflicts;
            next.baseline = remote_doc;
            next.etag = Some(file.etag);
            next.rotation = None;
            next.last_synced_at = Some(now());
            vault.storage.save(&next, &new_keys)?;
            vault.storage.discard_pending();
            let snapshot = vault.install(new_keys, next)?;
            changed(app, &snapshot);
            status(
                app,
                "synced",
                "主密码已更新，请在其他设备使用新密码",
                snapshot.last_synced_at,
            );
            Ok(snapshot)
        }
        Ok(Some(file)) => {
            let mut vault = state.vault.lock().map_err(AppError::storage)?;
            let session = vault.check(session_id)?;
            // Authenticate the old generation before rolling back a failed conditional update.
            let _: Document = crypto::open(&file.bytes, &session.keys, "snapshot")?;
            let mut next = session.state.clone();
            next.rotation = None;
            vault.persist(next)?;
            vault.storage.discard_pending();
            Err(upload.err().unwrap_or_else(|| {
                AppError::new("precondition", "云端换密未提交，请重新同步后重试")
            }))
        }
        _ => {
            let snapshot = state.snapshot(session_id)?;
            changed(app, &snapshot);
            status(
                app,
                "recovery",
                "换密结果待确认，本地暂时只读，请联网后输入新密码恢复",
                snapshot.last_synced_at,
            );
            Err(AppError::new(
                "recovery_pending",
                "无法确认云端换密结果，已保留新旧加密文件；请联网后重新解锁完成恢复",
            ))
        }
    }
}

#[tauri::command]
pub async fn migrate_password(
    state: State<'_, Shared>,
    app: AppHandle,
    session_id: u64,
    password: String,
) -> Result<Snapshot> {
    migrate_impl(state.inner(), &app, session_id, password).await
}

pub(crate) async fn migrate_impl(
    state: &Shared,
    app: &impl service::Events,
    session_id: u64,
    password: String,
) -> Result<Snapshot> {
    let password = Zeroizing::new(password);
    let _guard = state.sync_gate.lock().await;
    let (old, config, token) = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        let s = vault.check(session_id)?;
        if s.state.rotation.is_some() {
            return Err(AppError::new(
                "recovery_pending",
                "请从锁定界面重新输入密码完成换密恢复",
            ));
        }
        (
            s.keys.header.clone(),
            s.state
                .config
                .clone()
                .ok_or_else(|| AppError::invalid("尚未配置同步"))?,
            s.cancel.clone(),
        )
    };
    let remote = RemoteStore::new(&config)?;
    let file = cancellable(&token, remote.fetch(REMOTE_NAME))
        .await?
        .ok_or_else(|| AppError::new("remote_missing", "云端保险库不存在"))?;
    let header = crypto::header(&file.bytes, "snapshot")?;
    if header.vault_id != old.vault_id || header.epoch <= old.epoch {
        return Err(AppError::invalid("远端没有更新的主密码版本"));
    }
    let (keys, remote_doc) = blocking(move || {
        let keys = Keys::derive(&password, header)?;
        let doc: Document = crypto::open(&file.bytes, &keys, "snapshot")?;
        validate_document(&doc)?;
        Ok((keys, doc))
    })
    .await?;
    let mut vault = state.vault.lock().map_err(AppError::storage)?;
    let s = vault.check(session_id)?;
    let mut next = s.state.clone();
    let (document, conflicts) = merge(&next.baseline, &next.document, &remote_doc);
    next.document = document;
    next.conflicts = conflicts;
    next.baseline = remote_doc;
    next.etag = Some(file.etag);
    next.revision += 1;
    next.dirty = true;
    vault.storage.save(&next, &keys)?;
    let snapshot = vault.install(keys, next)?;
    state.schedule(2000);
    changed(app, &snapshot);
    status(
        app,
        if snapshot.conflicts.is_empty() {
            "pending"
        } else {
            "conflict"
        },
        "已使用新密码迁移本地修改",
        snapshot.last_synced_at,
    );
    Ok(snapshot)
}

#[tauri::command]
pub async fn export_backup(state: State<'_, Shared>, session_id: u64) -> Result<bool> {
    let bytes = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        let s = vault.check(session_id)?;
        if !s.state.conflicts.is_empty() || s.state.rotation.is_some() {
            return Err(AppError::invalid("请先处理冲突或密码恢复，再导出备份"));
        }
        crypto::seal(&s.state.document, &s.keys, "snapshot")?
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("导出加密备份")
        .add_filter("KVault 保险库", &["kvault"])
        .set_file_name("kvault-backup.kvault")
        .save_file()
        .await
    else {
        return Ok(false);
    };
    {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        vault.check(session_id)?;
        vault.storage.write_raw(file.path(), &bytes)?;
    }
    Ok(true)
}

#[tauri::command]
pub async fn import_backup(
    state: State<'_, Shared>,
    app: AppHandle,
    password: String,
    replace: bool,
    session_id: Option<u64>,
) -> Result<Option<Snapshot>> {
    let password = Zeroizing::new(password);
    let generation = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        if vault.storage.exists() {
            if !replace {
                return Err(AppError::invalid(
                    "导入将替换当前保险库，请先确认并导出备份",
                ));
            }
            vault.check(session_id.ok_or_else(AppError::locked)?)?;
        }
        vault.generation
    };
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("导入加密保险库")
        .add_filter("KVault 保险库", &["kvault"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let shared = state.inner().clone();
    let snapshot = blocking(move || {
        let mut vault = shared.vault.lock().map_err(AppError::storage)?;
        if vault.generation != generation {
            return Err(AppError::locked());
        }
        if vault.storage.exists() {
            vault.check(session_id.ok_or_else(AppError::locked)?)?;
        }
        let bytes = vault.storage.read(file.path())?;
        let keys = Keys::derive(&password, crypto::header(&bytes, "snapshot")?)?;
        let document: Document = crypto::open(&bytes, &keys, "snapshot")?;
        validate_document(&document)?;
        let next = LocalState {
            document,
            ..Default::default()
        };
        vault.storage.save(&next, &keys)?;
        vault.storage.discard_pending();
        vault.install(keys, next)
    })
    .await?;
    changed(&app, &snapshot);
    status(&app, "local", "已导入加密备份，请重新配置同步目标", None);
    Ok(Some(snapshot))
}
