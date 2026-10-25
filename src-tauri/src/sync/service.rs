use super::{REMOTE_NAME, SyncStatus, merge::merge, remote::RemoteStore};
use crate::{
    error::{AppError, Result},
    vault::{
        Shared, crypto,
        model::{Document, Snapshot, now, validate_document},
    },
};
use std::{future::Future, sync::atomic::Ordering};
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

pub trait Events: Send + Sync {
    fn sync_status(&self, status: SyncStatus);
    fn vault_changed(&self, snapshot: &Snapshot);
}
impl Events for AppHandle {
    fn sync_status(&self, status: SyncStatus) {
        let _ = self.emit("sync:status", status);
    }
    fn vault_changed(&self, snapshot: &Snapshot) {
        let _ = self.emit("vault:changed", snapshot);
    }
}
#[cfg(test)]
impl Events for () {
    fn sync_status(&self, _: SyncStatus) {}
    fn vault_changed(&self, _: &Snapshot) {}
}
pub fn status(
    app: &impl Events,
    state: &str,
    message: impl Into<String>,
    last_synced_at: Option<u64>,
) {
    app.sync_status(SyncStatus {
        state: state.into(),
        message: message.into(),
        last_synced_at,
    });
}
pub fn changed(app: &impl Events, snapshot: &Snapshot) {
    app.vault_changed(snapshot);
}

pub async fn cancellable<T>(
    token: &CancellationToken,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! { biased; _ = token.cancelled() => Err(AppError::locked()), result = future => result }
}

pub async fn sync(state: &Shared, app: &impl Events, session_id: u64) -> Result<Snapshot> {
    let Ok(_guard) = state.sync_gate.try_lock() else {
        return state.snapshot(session_id);
    };
    state.due.store(now() + 60000, Ordering::SeqCst);
    let result = sync_inner(state, app, session_id).await;
    if let Err(error) = &result {
        if error.code != "locked" {
            let failures = state.failures.fetch_add(1, Ordering::SeqCst).min(4);
            state.schedule((15000u64 << failures).min(300000));
            status(
                app,
                if error.code == "network" {
                    "offline"
                } else if error.code == "key_changed" {
                    "keyChanged"
                } else {
                    "error"
                },
                &error.message,
                None,
            );
        }
    } else {
        state.failures.store(0, Ordering::SeqCst);
    }
    result
}

pub async fn sync_inner(state: &Shared, app: &impl Events, session_id: u64) -> Result<Snapshot> {
    let (keys, config, cancel) = {
        let vault = state.vault.lock().map_err(AppError::storage)?;
        let session = vault.check(session_id)?;
        if let Some(message) = &session.state.credential_error {
            return Err(AppError::new("credential_store", message));
        }
        if session.state.rotation.is_some() {
            return Err(AppError::new(
                "recovery_pending",
                "密码变更待确认，请输入密码完成恢复",
            ));
        }
        let Some(config) = session.state.config.clone() else {
            return vault.snapshot();
        };
        if !session.state.conflicts.is_empty() {
            status(
                app,
                "conflict",
                "存在待处理的同步冲突",
                session.state.last_synced_at,
            );
            return vault.snapshot();
        }
        (session.keys.clone(), config, session.cancel.clone())
    };
    let remote = RemoteStore::new(&config)?;
    status(app, "syncing", "正在同步加密保险库", None);
    for _ in 0..3 {
        let fetched = cancellable(&cancel, remote.fetch(REMOTE_NAME)).await?;
        let remote_doc: Option<Document> = if let Some(file) = &fetched {
            let h = crypto::header(&file.bytes, "snapshot")?;
            if h.vault_id != keys.header.vault_id {
                return Err(AppError::new(
                    "different_vault",
                    "目标中已有另一个保险库，已拒绝覆盖",
                ));
            }
            if h != keys.header {
                return Err(AppError::new(
                    "key_changed",
                    "云端主密码已修改，请输入新密码迁移本地离线内容",
                ));
            }
            let document: Document = crypto::open(&file.bytes, &keys, "snapshot")?;
            validate_document(&document)?;
            Some(document)
        } else {
            None
        };
        let (document, upload) = {
            let mut vault = state.vault.lock().map_err(AppError::storage)?;
            let session = vault.check(session_id)?;
            if cancel.is_cancelled() || session.state.config.as_ref() != Some(&config) {
                return Err(AppError::locked());
            }
            if !session.state.conflicts.is_empty() {
                return vault.snapshot();
            }
            let mut next = session.state.clone();
            if remote_doc.is_none() && next.last_synced_at.is_some() {
                return Err(AppError::new(
                    "remote_missing",
                    "云端保险库已被删除。请重新配置同步目标后再初始化，避免意外覆盖",
                ));
            }
            let target = remote_doc.clone().unwrap_or_default();
            let (merged, conflicts) = merge(&next.baseline, &next.document, &target);
            next.document = merged.clone();
            next.baseline = target.clone();
            next.etag = fetched.as_ref().map(|f| f.etag.clone());
            next.conflicts = conflicts;
            next.revision += 1;
            next.dirty = merged != target || fetched.is_none();
            let upload = next.dirty;
            if !upload {
                next.last_synced_at = Some(now());
            }
            let has_conflicts = !next.conflicts.is_empty();
            vault.persist(next)?;
            let snapshot = vault.snapshot()?;
            changed(app, &snapshot);
            if has_conflicts {
                status(
                    app,
                    "conflict",
                    "双方修改已保留，请处理冲突后继续同步",
                    snapshot.last_synced_at,
                );
                return Ok(snapshot);
            }
            if !upload {
                status(app, "synced", "已同步", snapshot.last_synced_at);
                return Ok(snapshot);
            }
            (merged, upload)
        };
        if upload {
            let bytes = crypto::seal(&document, &keys, "snapshot")?;
            match cancellable(
                &cancel,
                remote.put(
                    REMOTE_NAME,
                    bytes,
                    fetched.as_ref().map(|f| f.etag.as_str()),
                ),
            )
            .await
            {
                Err(e) if e.code == "precondition" => continue,
                Err(e) => return Err(e),
                Ok(()) => {}
            }
            let mut vault = state.vault.lock().map_err(AppError::storage)?;
            let session = vault.check(session_id)?;
            if cancel.is_cancelled() {
                return Err(AppError::locked());
            }
            // Only acknowledge the uploaded revision. Edits made during the request stay dirty.
            let mut next = session.state.clone();
            next.baseline = document;
            next.etag = None;
            next.last_synced_at = Some(now());
            next.dirty = next.document != next.baseline;
            next.revision += 1;
            let dirty = next.dirty;
            vault.persist(next)?;
            let snapshot = vault.snapshot()?;
            changed(app, &snapshot);
            if dirty {
                state.schedule(2000);
                status(
                    app,
                    "pending",
                    "本地新修改等待同步",
                    snapshot.last_synced_at,
                );
            } else {
                status(app, "synced", "已同步", snapshot.last_synced_at);
            }
            return Ok(snapshot);
        }
    }
    Err(AppError::new(
        "busy_remote",
        "云端持续发生修改，本地数据已保存，请稍后重试",
    ))
}

pub fn start_worker(state: Shared, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let (idle, session_id) = if let Ok(vault) = state.vault.lock() {
                (
                    vault.idle_due(),
                    vault
                        .session
                        .as_ref()
                        .filter(|s| {
                            s.state.config.is_some()
                                && s.state.rotation.is_none()
                                && s.state.conflicts.is_empty()
                        })
                        .map(|_| vault.generation),
                )
            } else {
                (false, None)
            };
            if idle {
                if state.lock_if_idle().unwrap_or(false) {
                    let _ = app.emit("vault:locked", ());
                    status(&app, "locked", "保险库已自动锁定", None);
                }
                continue;
            }
            let due = state.due.load(Ordering::SeqCst);
            if due > 0
                && due <= now()
                && let Some(session_id) = session_id
            {
                state.due.store(now() + 60000, Ordering::SeqCst);
                let state = state.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = sync(&state, &app, session_id).await;
                });
            }
        }
    });
}
