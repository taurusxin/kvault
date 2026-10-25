pub mod crypto;
pub mod import;
pub mod model;
pub mod storage;
#[cfg(test)]
mod tests;

use crate::{
    error::{AppError, Result},
    sync::{Conflict, Entity, SyncConfig},
};
use crypto::Keys;
use model::{Document, Entry, EntryInput, LocalState, Snapshot, Table, id, now, validate_document};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use storage::Storage;
use tokio_util::sync::CancellationToken;

pub struct Session {
    pub keys: Keys,
    pub state: LocalState,
    pub cancel: CancellationToken,
}
pub struct Vault {
    pub storage: Storage,
    pub session: Option<Session>,
    pub generation: u64,
    pub activity: Instant,
    pub activity_wall: u64,
}
pub struct AppState {
    pub vault: Mutex<Vault>,
    pub sync_gate: tokio::sync::Mutex<()>,
    pub due: AtomicU64,
    pub failures: AtomicU64,
}

impl AppState {
    pub fn new(storage: Storage) -> Self {
        Self {
            vault: Mutex::new(Vault {
                storage,
                session: None,
                generation: 0,
                activity: Instant::now(),
                activity_wall: now(),
            }),
            sync_gate: tokio::sync::Mutex::new(()),
            due: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        }
    }
    pub fn lock(&self) -> Result<()> {
        let mut vault = self.vault.lock().map_err(AppError::storage)?;
        if let Some(session) = vault.session.take() {
            session.cancel.cancel();
        }
        vault.generation += 1;
        self.due.store(0, Ordering::SeqCst);
        Ok(())
    }
    pub fn schedule(&self, delay: u64) {
        self.due.store(now() + delay, Ordering::SeqCst);
    }
    pub fn snapshot(&self, session_id: u64) -> Result<Snapshot> {
        let vault = self.vault.lock().map_err(AppError::storage)?;
        vault.check(session_id)?;
        vault.snapshot()
    }
}

impl Vault {
    pub fn touch(&mut self) {
        self.activity = Instant::now();
        self.activity_wall = now();
    }
    pub fn idle_due(&self) -> bool {
        self.session.is_some()
            && (self.activity.elapsed().as_secs() >= 300
                || now().saturating_sub(self.activity_wall) >= 300000)
    }
    pub fn check(&self, session_id: u64) -> Result<&Session> {
        if self.generation != session_id {
            return Err(AppError::locked());
        }
        self.session.as_ref().ok_or_else(AppError::locked)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let session = self.session.as_ref().ok_or_else(AppError::locked)?;
        Ok(Snapshot {
            session_id: self.generation,
            vault_id: session.keys.header.vault_id.clone(),
            epoch: session.keys.header.epoch,
            tables: session
                .state
                .document
                .tables
                .iter()
                .filter(|t| !t.deleted)
                .cloned()
                .collect(),
            entries: session
                .state
                .document
                .entries
                .iter()
                .filter(|e| !e.deleted)
                .cloned()
                .collect(),
            conflicts: session.state.conflicts.clone(),
            config: session.state.config.as_ref().map(SyncConfig::redacted),
            revision: session.state.revision,
            last_synced_at: session.state.last_synced_at,
            dirty: session.state.dirty,
            recovery_pending: session.state.rotation.is_some(),
        })
    }
    pub fn install(&mut self, keys: Keys, state: LocalState) -> Result<Snapshot> {
        if let Some(old) = self.session.take() {
            old.cancel.cancel();
        }
        self.generation += 1;
        self.touch();
        self.session = Some(Session {
            keys,
            state,
            cancel: CancellationToken::new(),
        });
        self.snapshot()
    }
    pub fn persist(&mut self, state: LocalState) -> Result<()> {
        let session = self.session.as_ref().ok_or_else(AppError::locked)?;
        self.storage.save(&state, &session.keys)?;
        self.session.as_mut().ok_or_else(AppError::locked)?.state = state;
        Ok(())
    }
    pub fn mutate(&mut self, session_id: u64, action: Action) -> Result<Snapshot> {
        let session = self.check(session_id)?;
        if session.state.rotation.is_some() {
            return Err(AppError::new(
                "recovery_pending",
                "密码变更尚待核验，请先完成恢复",
            ));
        }
        if !session.state.conflicts.is_empty() && !matches!(action, Action::ResolveConflict { .. })
        {
            return Err(AppError::new("conflict", "请先解决同步冲突，再修改保险库"));
        }
        let mut state = session.state.clone();
        match action {
            Action::CreateTable { name } => {
                let name = table_name(&state.document, &name, None)?;
                state.document.tables.push(Table {
                    id: id(),
                    name,
                    created_at: now(),
                    updated_at: now(),
                    deleted: false,
                });
            }
            Action::RenameTable { id, name } => {
                let name = table_name(&state.document, &name, Some(&id))?;
                let table = state
                    .document
                    .tables
                    .iter_mut()
                    .find(|t| t.id == id && !t.deleted)
                    .ok_or_else(|| AppError::invalid("表不存在"))?;
                table.name = name;
                table.updated_at = now();
            }
            Action::DeleteTable { id } => {
                let table = state
                    .document
                    .tables
                    .iter_mut()
                    .find(|t| t.id == id && !t.deleted)
                    .ok_or_else(|| AppError::invalid("表不存在"))?;
                table.deleted = true;
                table.updated_at = now();
                for entry in state
                    .document
                    .entries
                    .iter_mut()
                    .filter(|e| e.table_id == id && !e.deleted)
                {
                    entry.deleted = true;
                    entry.updated_at = now();
                }
            }
            Action::SaveEntry { input } => {
                let key = input.key.trim().to_string();
                if key.is_empty() {
                    return Err(AppError::invalid("Key 不能为空"));
                }
                if !state
                    .document
                    .tables
                    .iter()
                    .any(|t| t.id == input.table_id && !t.deleted)
                {
                    return Err(AppError::invalid("请选择有效的表"));
                }
                if state.document.entries.iter().any(|e| {
                    !e.deleted
                        && e.table_id == input.table_id
                        && e.key == key
                        && Some(&e.id) != input.id.as_ref()
                }) {
                    return Err(AppError::invalid("此表中已存在相同 Key"));
                }
                if let Some(entry_id) = input.id {
                    let entry = state
                        .document
                        .entries
                        .iter_mut()
                        .find(|e| e.id == entry_id && !e.deleted)
                        .ok_or_else(|| AppError::invalid("记录不存在"))?;
                    entry.table_id = input.table_id;
                    entry.key = key;
                    entry.value = input.value;
                    entry.notes = input.notes;
                    entry.updated_at = now();
                } else {
                    state.document.entries.push(Entry {
                        id: id(),
                        table_id: input.table_id,
                        key,
                        value: input.value,
                        notes: input.notes,
                        sort_order: model::next_sort_order(&state.document),
                        created_at: now(),
                        updated_at: now(),
                        deleted: false,
                    });
                }
            }
            Action::DeleteEntry { id } => {
                let entry = state
                    .document
                    .entries
                    .iter_mut()
                    .find(|e| e.id == id && !e.deleted)
                    .ok_or_else(|| AppError::invalid("记录不存在"))?;
                entry.deleted = true;
                entry.updated_at = now();
            }
            Action::DeleteEntries { ids } => {
                let indices = entry_indices(&state.document, ids)?;
                let timestamp = now();
                for index in indices {
                    let entry = &mut state.document.entries[index];
                    entry.deleted = true;
                    entry.updated_at = timestamp;
                }
            }
            Action::MoveEntries { ids, table_id } => {
                let indices = entry_indices(&state.document, ids)?;
                if !state
                    .document
                    .tables
                    .iter()
                    .any(|t| t.id == table_id && !t.deleted)
                {
                    return Err(AppError::invalid("目标数据表不存在"));
                }
                let selected: HashSet<_> = indices.iter().copied().collect();
                let mut keys = HashSet::new();
                for (index, entry) in state.document.entries.iter().enumerate() {
                    if !entry.deleted
                        && (entry.table_id == table_id || selected.contains(&index))
                        && !keys.insert(entry.key.as_str())
                    {
                        return Err(AppError::invalid(format!(
                            "目标表存在重复 Key「{}」，未移动任何记录",
                            entry.key
                        )));
                    }
                }
                let timestamp = now();
                for index in indices {
                    let entry = &mut state.document.entries[index];
                    if entry.table_id != table_id {
                        entry.table_id = table_id.clone();
                        entry.updated_at = timestamp;
                    }
                }
            }
            Action::ImportEntries {
                table_id,
                rows,
                duplicate_policy,
            } => {
                import::apply(&mut state.document, &table_id, rows, duplicate_policy)?;
            }
            Action::ReorderEntries { ids, table_id } => {
                if let Some(table) = &table_id
                    && !state
                        .document
                        .tables
                        .iter()
                        .any(|t| t.id == *table && !t.deleted)
                {
                    return Err(AppError::invalid("排序的数据表已不存在"));
                }
                let scope: HashSet<_> = state
                    .document
                    .entries
                    .iter()
                    .filter(|e| {
                        !e.deleted && table_id.as_ref().is_none_or(|table| e.table_id == *table)
                    })
                    .map(|e| e.id.clone())
                    .collect();
                if ids.is_empty()
                    || ids.len() != scope.len()
                    || ids.iter().collect::<HashSet<_>>().len() != ids.len()
                    || ids.iter().any(|id| !scope.contains(id))
                {
                    return Err(AppError::invalid(
                        "排序列表已变化，请取消后重新编辑，未保存任何排序",
                    ));
                }
                let by_id: std::collections::HashMap<_, _> = state
                    .document
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| (entry.id.clone(), index))
                    .collect();
                let mut indices: Vec<_> = state
                    .document
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.deleted)
                    .map(|(index, _)| index)
                    .collect();
                indices.sort_by(|a, b| {
                    model::compare_entry_order(
                        &state.document.entries[*a],
                        &state.document.entries[*b],
                    )
                });
                let mut ordered = ids.into_iter();
                // Reorder this table in its existing global slots, preserving other tables' order.
                for (position, index) in indices.into_iter().enumerate() {
                    let index = if scope.contains(&state.document.entries[index].id) {
                        by_id[&ordered.next().expect("validated complete scope")]
                    } else {
                        index
                    };
                    state.document.entries[index].sort_order = position as u64 + 1;
                }
            }
            Action::SetSync { config } => {
                state.config = config;
                state.baseline = Document::default();
                state.etag = None;
                state.last_synced_at = None;
                self.session
                    .as_ref()
                    .ok_or_else(AppError::locked)?
                    .cancel
                    .cancel();
                self.session.as_mut().ok_or_else(AppError::locked)?.cancel =
                    CancellationToken::new();
            }
            Action::ResolveConflict { id, choice, rename } => {
                let conflict = state
                    .conflicts
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                    .ok_or_else(|| AppError::invalid("冲突不存在"))?;
                resolve(&mut state.document, &conflict, &choice, &rename)?;
                state
                    .conflicts
                    .retain(|c| c.id != id && !c.reason.starts_with("duplicate"));
                state
                    .conflicts
                    .extend(crate::sync::merge::duplicates(&state.document));
            }
        }
        if state.conflicts.is_empty() {
            validate_document(&state.document)?;
        }
        state.revision += 1;
        state.dirty = true;
        self.persist(state)?;
        self.touch();
        self.snapshot()
    }
    pub fn hydrate_config(&self, mut config: SyncConfig) -> SyncConfig {
        match (
            &mut config,
            self.session.as_ref().and_then(|s| s.state.config.as_ref()),
        ) {
            (
                SyncConfig::S3 {
                    endpoint,
                    access_key,
                    secret_key,
                    session_token,
                    ..
                },
                Some(SyncConfig::S3 {
                    endpoint: old_endpoint,
                    access_key: old_access,
                    secret_key: old_secret,
                    session_token: old_token,
                    ..
                }),
            ) if endpoint == old_endpoint && access_key == old_access => {
                if secret_key.is_empty() {
                    *secret_key = old_secret.clone();
                }
                if session_token.as_ref().is_none_or(|s| s.is_empty()) {
                    *session_token = old_token.clone();
                }
            }
            (
                SyncConfig::Webdav {
                    endpoint,
                    username,
                    password,
                    ..
                },
                Some(SyncConfig::Webdav {
                    endpoint: old_endpoint,
                    username: old_username,
                    password: old_password,
                    ..
                }),
            ) if endpoint == old_endpoint && username == old_username && password.is_empty() => {
                *password = old_password.clone();
            }
            _ => {}
        }
        config
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    CreateTable {
        name: String,
    },
    RenameTable {
        id: String,
        name: String,
    },
    DeleteTable {
        id: String,
    },
    SaveEntry {
        input: EntryInput,
    },
    DeleteEntry {
        id: String,
    },
    DeleteEntries {
        ids: Vec<String>,
    },
    MoveEntries {
        ids: Vec<String>,
        table_id: String,
    },
    ReorderEntries {
        ids: Vec<String>,
        table_id: Option<String>,
    },
    ImportEntries {
        table_id: String,
        rows: Vec<import::ImportRow>,
        duplicate_policy: import::DuplicatePolicy,
    },
    SetSync {
        config: Option<SyncConfig>,
    },
    ResolveConflict {
        id: String,
        choice: String,
        rename: String,
    },
}

fn entry_indices(doc: &Document, ids: Vec<String>) -> Result<Vec<usize>> {
    let ids: HashSet<_> = ids.into_iter().collect();
    if ids.is_empty() {
        return Err(AppError::invalid("请先选择记录"));
    }
    let indices: Vec<_> = doc
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.deleted && ids.contains(&entry.id))
        .map(|(index, _)| index)
        .collect();
    if indices.len() != ids.len() {
        return Err(AppError::invalid(
            "部分记录已不存在，请重新选择，未修改任何记录",
        ));
    }
    Ok(indices)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub exists: bool,
    pub recovery_pending: bool,
    pub platform: String,
}

fn table_name(doc: &Document, name: &str, exclude: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::invalid("表名称不能为空"));
    }
    if doc
        .tables
        .iter()
        .any(|t| !t.deleted && t.name == name && Some(t.id.as_str()) != exclude)
    {
        return Err(AppError::invalid("已存在同名表"));
    }
    Ok(name.into())
}

fn replace_entity(doc: &mut Document, entity: Entity) {
    match entity {
        Entity::Table(t) => {
            doc.tables.retain(|e| e.id != t.id);
            doc.tables.push(t);
        }
        Entity::Entry(e) => {
            doc.entries.retain(|entry| entry.id != e.id);
            doc.entries.push(e);
        }
    }
}
fn tombstone(doc: &mut Document, entity: &Entity) {
    match entity.clone() {
        Entity::Table(mut t) => {
            t.deleted = true;
            t.updated_at = now();
            for e in doc.entries.iter_mut().filter(|e| e.table_id == t.id) {
                e.deleted = true;
                e.updated_at = now();
            }
            replace_entity(doc, Entity::Table(t));
        }
        Entity::Entry(mut e) => {
            e.deleted = true;
            e.updated_at = now();
            replace_entity(doc, Entity::Entry(e));
        }
    }
}
fn resolve(doc: &mut Document, c: &Conflict, choice: &str, rename: &str) -> Result<()> {
    if !matches!(choice, "local" | "remote" | "both") {
        return Err(AppError::invalid("无效的冲突处理方式"));
    }
    let chosen = if choice == "remote" {
        &c.remote
    } else {
        &c.local
    };
    let other = if choice == "remote" {
        &c.local
    } else {
        &c.remote
    };
    if c.reason.starts_with("duplicate")
        && let Some(e) = other
    {
        tombstone(doc, e);
    }
    if let Some(entity) = chosen {
        replace_entity(doc, entity.clone());
        if let Entity::Table(table) = entity {
            if c.reason == "tableDeletion" {
                doc.entries.retain(|e| e.table_id != table.id);
                doc.entries.extend(if choice == "remote" {
                    c.remote_entries.clone()
                } else {
                    c.local_entries.clone()
                });
            }
            if table.deleted {
                for e in doc.entries.iter_mut().filter(|e| e.table_id == table.id) {
                    e.deleted = true;
                }
            }
        }
    } else if let Some(entity) = other {
        tombstone(doc, entity);
    }
    if choice == "both" {
        let entity = c
            .remote
            .as_ref()
            .ok_or_else(|| AppError::invalid("删除版本无法保留为副本"))?;
        let rename = rename.trim();
        if rename.is_empty() {
            return Err(AppError::invalid("保留双方时请填写新的 Key 或表名"));
        }
        match entity.clone() {
            Entity::Entry(mut e) => {
                if e.deleted {
                    return Err(AppError::invalid("删除版本无法保留为副本"));
                }
                if doc.entries.iter().any(|other| {
                    !other.deleted && other.table_id == e.table_id && other.key == rename
                }) {
                    return Err(AppError::invalid("新的 Key 已被使用"));
                }
                e.id = id();
                e.key = rename.into();
                e.updated_at = now();
                replace_entity(doc, Entity::Entry(e));
            }
            Entity::Table(mut t) => {
                if t.deleted {
                    return Err(AppError::invalid("删除版本无法保留为副本"));
                }
                t.name = table_name(doc, rename, None)?;
                t.id = id();
                t.updated_at = now();
                for mut e in c.remote_entries.clone() {
                    e.id = id();
                    e.table_id = t.id.clone();
                    doc.entries.push(e);
                }
                replace_entity(doc, Entity::Table(t));
            }
        }
    }
    Ok(())
}

pub type Shared = Arc<AppState>;
