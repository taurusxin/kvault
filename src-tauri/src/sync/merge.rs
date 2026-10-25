use crate::vault::model::{Document, Entry, Table, id};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Entity {
    Table(Table),
    Entry(Entry),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub id: String,
    pub reason: String,
    pub local: Option<Entity>,
    pub remote: Option<Entity>,
    #[serde(default)]
    pub local_entries: Vec<Entry>,
    #[serde(default)]
    pub remote_entries: Vec<Entry>,
}

impl Conflict {
    fn new(reason: &str, local: Option<Entity>, remote: Option<Entity>) -> Self {
        Self {
            id: id(),
            reason: reason.into(),
            local,
            remote,
            local_entries: vec![],
            remote_entries: vec![],
        }
    }
}

pub fn merge(base: &Document, local: &Document, remote: &Document) -> (Document, Vec<Conflict>) {
    let mut result = Document::default();
    let mut conflicts = Vec::new();
    let table_ids: BTreeSet<_> = local
        .tables
        .iter()
        .chain(&remote.tables)
        .map(|t| t.id.clone())
        .collect();
    for table_id in table_ids {
        let b = base.tables.iter().find(|t| t.id == table_id);
        let l = local.tables.iter().find(|t| t.id == table_id);
        let r = remote.tables.iter().find(|t| t.id == table_id);
        let chosen = if l == r || r == b {
            l
        } else if l == b {
            r
        } else {
            let mut c = Conflict::new(
                "table",
                l.cloned().map(Entity::Table),
                r.cloned().map(Entity::Table),
            );
            c.local_entries = local
                .entries
                .iter()
                .filter(|e| e.table_id == table_id)
                .cloned()
                .collect();
            c.remote_entries = remote
                .entries
                .iter()
                .filter(|e| e.table_id == table_id)
                .cloned()
                .collect();
            conflicts.push(c);
            l.or(r)
        };
        if let Some(t) = chosen {
            result.tables.push(t.clone());
        }
    }
    let entry_ids: BTreeSet<_> = local
        .entries
        .iter()
        .chain(&remote.entries)
        .map(|e| e.id.clone())
        .collect();
    for entry_id in entry_ids {
        let b = base.entries.iter().find(|e| e.id == entry_id);
        let l = local.entries.iter().find(|e| e.id == entry_id);
        let r = remote.entries.iter().find(|e| e.id == entry_id);
        let chosen = if l == r || r == b {
            l
        } else if l == b {
            r
        } else {
            conflicts.push(Conflict::new(
                "entry",
                l.cloned().map(Entity::Entry),
                r.cloned().map(Entity::Entry),
            ));
            l.or(r)
        };
        if let Some(e) = chosen {
            result.entries.push(e.clone());
        }
    }
    // Cascading table deletion competes with any child edit, including newly created records.
    for table in &result.tables {
        let l = local.tables.iter().find(|t| t.id == table.id);
        let r = remote.tables.iter().find(|t| t.id == table.id);
        let children = |d: &Document| {
            d.entries
                .iter()
                .filter(|e| e.table_id == table.id)
                .cloned()
                .collect::<Vec<_>>()
        };
        let lc = children(local);
        let rc = children(remote);
        let bc = children(base);
        let deletion_race =
            l.is_some_and(|t| t.deleted) && r.is_some_and(|t| !t.deleted) && rc != bc
                || r.is_some_and(|t| t.deleted) && l.is_some_and(|t| !t.deleted) && lc != bc;
        if deletion_race {
            conflicts.retain(|c| {
                !matches!(&c.local, Some(Entity::Table(t)) if t.id == table.id)
                    && !matches!(&c.remote, Some(Entity::Table(t)) if t.id == table.id)
                    && !matches!(&c.local, Some(Entity::Entry(e)) if e.table_id == table.id)
                    && !matches!(&c.remote, Some(Entity::Entry(e)) if e.table_id == table.id)
            });
            let mut conflict = Conflict::new(
                "tableDeletion",
                l.cloned().map(Entity::Table),
                r.cloned().map(Entity::Table),
            );
            conflict.local_entries = lc;
            conflict.remote_entries = rc;
            conflicts.push(conflict);
        }
    }
    conflicts.extend(duplicates(&result));
    (result, conflicts)
}

pub fn duplicates(document: &Document) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    for (index, entry) in document
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.deleted)
    {
        if let Some(other) = document.entries[..index]
            .iter()
            .find(|e| !e.deleted && e.table_id == entry.table_id && e.key == entry.key)
        {
            conflicts.push(Conflict::new(
                "duplicateKey",
                Some(Entity::Entry(other.clone())),
                Some(Entity::Entry(entry.clone())),
            ));
        }
    }
    for (index, table) in document
        .tables
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.deleted)
    {
        if let Some(other) = document.tables[..index]
            .iter()
            .find(|t| !t.deleted && t.name == table.name)
        {
            let mut c = Conflict::new(
                "duplicateTable",
                Some(Entity::Table(other.clone())),
                Some(Entity::Table(table.clone())),
            );
            c.local_entries = document
                .entries
                .iter()
                .filter(|e| e.table_id == other.id)
                .cloned()
                .collect();
            c.remote_entries = document
                .entries
                .iter()
                .filter(|e| e.table_id == table.id)
                .cloned()
                .collect();
            conflicts.push(c);
        }
    }
    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Document {
        Document {
            tables: vec![Table {
                id: id(),
                name: "工作".into(),
                created_at: 1,
                updated_at: 1,
                deleted: false,
            }],
            entries: vec![],
        }
    }
    fn entry(table: &str, key: &str) -> Entry {
        Entry {
            id: id(),
            table_id: table.into(),
            key: key.into(),
            value: "v".into(),
            notes: "".into(),
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
            deleted: false,
        }
    }
    #[test]
    fn independent_edits_merge_and_duplicates_conflict() {
        let base = fixture();
        let mut local = base.clone();
        let mut remote = base.clone();
        local.entries.push(entry(&base.tables[0].id, "a"));
        remote.entries.push(entry(&base.tables[0].id, "b"));
        let (doc, conflicts) = merge(&base, &local, &remote);
        assert_eq!(doc.entries.len(), 2);
        assert!(conflicts.is_empty());
        remote.entries[0].key = "a".into();
        assert_eq!(merge(&base, &local, &remote).1[0].reason, "duplicateKey");
    }
    #[test]
    fn custom_order_syncs_and_competing_edits_preserve_both_versions() {
        let mut base = fixture();
        let table = base.tables[0].id.clone();
        base.entries.push(entry(&table, "first"));
        base.entries.push(entry(&table, "second"));
        base.entries[0].sort_order = 1;
        base.entries[1].sort_order = 2;
        let mut local = base.clone();
        local.entries[0].sort_order = 2;
        local.entries[1].sort_order = 1;
        let (mut synced, conflicts) = merge(&base, &local, &base);
        assert!(conflicts.is_empty());
        synced
            .entries
            .sort_by(crate::vault::model::compare_entry_order);
        assert_eq!(synced.entries[0].key, "second");
        let mut remote = base.clone();
        remote.entries[0].notes = "offline note".into();
        let (_, conflicts) = merge(&base, &local, &remote);
        assert_eq!(conflicts.len(), 1);
        assert!(matches!(conflicts[0].local, Some(Entity::Entry(_))));
        assert!(matches!(conflicts[0].remote, Some(Entity::Entry(_))));
    }
    #[test]
    fn edit_delete_and_table_cascade_preserve_versions() {
        let mut base = fixture();
        base.entries.push(entry(&base.tables[0].id, "token"));
        let mut l = base.clone();
        let mut r = base.clone();
        l.entries[0].deleted = true;
        r.entries[0].value = "new secret".into();
        assert_eq!(merge(&base, &l, &r).1[0].reason, "entry");
        l.tables[0].deleted = true;
        let (_, conflicts) = merge(&base, &l, &r);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].reason, "tableDeletion");
        assert_eq!(conflicts[0].remote_entries[0].value, "new secret");
    }
    #[test]
    fn tombstone_is_not_resurrected() {
        let mut base = fixture();
        base.entries.push(entry(&base.tables[0].id, "a"));
        let mut local = base.clone();
        local.entries[0].deleted = true;
        let (merged, c) = merge(&base, &local, &base);
        assert!(c.is_empty());
        assert!(merged.entries[0].deleted);
    }
}
