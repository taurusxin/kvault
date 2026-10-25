use crate::sync::{Conflict, SyncConfig};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub id: String,
    pub name: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub table_id: String,
    pub key: String,
    pub value: String,
    pub notes: String,
    #[serde(default)]
    pub sort_order: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub deleted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub tables: Vec<Table>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub document: Document,
    pub config: Option<SyncConfig>,
    pub baseline: Document,
    pub etag: Option<String>,
    pub conflicts: Vec<Conflict>,
    pub revision: u64,
    pub last_synced_at: Option<u64>,
    pub dirty: bool,
    #[serde(default)]
    pub rotation: Option<Rotation>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rotation {
    pub remote_hash: String,
    pub previous_etag: Option<String>,
    pub epoch: u64,
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            document: Document::default(),
            config: None,
            baseline: Document::default(),
            etag: None,
            conflicts: vec![],
            revision: 0,
            last_synced_at: None,
            dirty: true,
            rotation: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session_id: u64,
    pub vault_id: String,
    pub epoch: u64,
    pub tables: Vec<Table>,
    pub entries: Vec<Entry>,
    pub conflicts: Vec<Conflict>,
    pub config: Option<SyncConfig>,
    pub revision: u64,
    pub last_synced_at: Option<u64>,
    pub dirty: bool,
    pub recovery_pending: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    pub id: Option<String>,
    pub table_id: String,
    pub key: String,
    pub value: String,
    pub notes: String,
}

pub fn compare_entry_order(a: &Entry, b: &Entry) -> std::cmp::Ordering {
    a.sort_order
        .cmp(&b.sort_order)
        .then_with(|| b.updated_at.cmp(&a.updated_at))
        .then_with(|| a.id.cmp(&b.id))
}

pub fn next_sort_order(doc: &Document) -> u64 {
    doc.entries
        .iter()
        .map(|entry| entry.sort_order)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn validate_document(doc: &Document) -> crate::error::Result<()> {
    use std::collections::HashSet;
    let mut table_ids = HashSet::new();
    let mut names = HashSet::new();
    for table in &doc.tables {
        if !table_ids.insert(&table.id) || Uuid::parse_str(&table.id).is_err() {
            return Err(crate::error::AppError::invalid("保险库包含无效的表标识"));
        }
        if !table.deleted && (table.name.trim().is_empty() || !names.insert(&table.name)) {
            return Err(crate::error::AppError::invalid("表名称不能为空或重复"));
        }
    }
    let active: HashSet<_> = doc
        .tables
        .iter()
        .filter(|t| !t.deleted)
        .map(|t| &t.id)
        .collect();
    let mut ids = HashSet::new();
    let mut keys = HashSet::new();
    for entry in &doc.entries {
        if !ids.insert(&entry.id) || Uuid::parse_str(&entry.id).is_err() {
            return Err(crate::error::AppError::invalid("保险库包含无效的记录标识"));
        }
        if !entry.deleted
            && (!active.contains(&entry.table_id)
                || entry.key.trim().is_empty()
                || !keys.insert((&entry.table_id, &entry.key)))
        {
            return Err(crate::error::AppError::invalid(
                "记录所属表无效或存在重复、空白 Key",
            ));
        }
    }
    Ok(())
}
