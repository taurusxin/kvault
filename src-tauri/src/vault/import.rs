use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::model::{Document, Entry, id, next_sort_order, now};
use crate::error::{AppError, Result};

pub const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMPORT_ROWS: usize = 10000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    pub line: usize,
    pub key: String,
    pub value: String,
    pub notes: String,
}

#[derive(Serialize)]
pub struct ImportIssue {
    pub line: usize,
    pub message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
    pub errors: Vec<ImportIssue>,
    pub has_header: bool,
    pub delimiter: Delimiter,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Delimiter {
    Auto,
    Comma,
    Space,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HeaderMode {
    Auto,
    Present,
    None,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicatePolicy {
    Skip,
    Overwrite,
}

fn whitespace_fields(line: &str) -> std::result::Result<Vec<String>, &'static str> {
    let mut fields = vec![];
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    let mut started = false;
    let mut closed = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(ch);
            }
        } else if ch.is_whitespace() {
            if started {
                fields.push(std::mem::take(&mut field));
                started = false;
                closed = false;
            }
        } else if ch == '"' && !started {
            quoted = true;
            started = true;
        } else {
            if closed || ch == '"' {
                return Err("引号必须包围整个字段，字段之间请使用空格分隔");
            }
            started = true;
            field.push(ch);
        }
    }
    if quoted {
        return Err("双引号未闭合");
    }
    if started {
        fields.push(field);
    }
    Ok(fields)
}

fn is_header(fields: &[String]) -> bool {
    let names: Vec<_> = fields.iter().map(|s| s.trim().to_lowercase()).collect();
    (names.len() == 2 || names.len() == 3)
        && matches!(names[0].as_str(), "key" | "键" | "键名")
        && matches!(names[1].as_str(), "value" | "值")
        && (names.len() == 2 || matches!(names[2].as_str(), "notes" | "note" | "备注"))
}

fn detect_delimiter(text: &str) -> Delimiter {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim_start();
    let mut quoted = false;
    for (index, ch) in line.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if !quoted {
            if ch == ',' {
                return Delimiter::Comma;
            }
            if ch.is_whitespace() {
                return if line[index..].trim_start().starts_with(',') {
                    Delimiter::Comma
                } else {
                    Delimiter::Space
                };
            }
        }
    }
    Delimiter::Space
}

pub fn parse(text: &str, delimiter: Delimiter, header_mode: HeaderMode) -> Result<ImportPreview> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err(AppError::invalid("导入内容不能超过 10 MiB"));
    }
    let text = text.trim_start_matches('\u{feff}');
    let delimiter = match delimiter {
        Delimiter::Auto => detect_delimiter(text),
        value => value,
    };
    let mut result = ImportPreview {
        rows: vec![],
        errors: vec![],
        has_header: false,
        delimiter,
    };
    let mut first = true;
    let mut count = 0;
    let mut consume =
        |line: usize, fields: std::result::Result<Vec<String>, String>| -> Result<()> {
            count += 1;
            if count > MAX_IMPORT_ROWS + 1 {
                return Err(AppError::invalid("一次最多导入 10000 条记录"));
            }
            let fields = match fields {
                Ok(f) => f,
                Err(message) => {
                    first = false;
                    result.errors.push(ImportIssue { line, message });
                    return Ok(());
                }
            };
            if first {
                first = false;
                result.has_header = match header_mode {
                    HeaderMode::Auto => is_header(&fields),
                    HeaderMode::Present => true,
                    HeaderMode::None => false,
                };
                if result.has_header {
                    return Ok(());
                }
            }
            if count - usize::from(result.has_header) > MAX_IMPORT_ROWS {
                return Err(AppError::invalid("一次最多导入 10000 条记录"));
            }
            if !(2..=3).contains(&fields.len()) {
                result.errors.push(ImportIssue {
                    line,
                    message: "需要 2 或 3 列：Key、Value、可选备注；含空格或逗号的字段请使用双引号"
                        .into(),
                });
            } else if fields[0].trim().is_empty() {
                result.errors.push(ImportIssue {
                    line,
                    message: "Key 不能为空".into(),
                });
            } else {
                result.rows.push(ImportRow {
                    line,
                    key: fields[0].trim().into(),
                    value: fields[1].clone(),
                    notes: fields.get(2).cloned().unwrap_or_default(),
                });
            }
            Ok(())
        };
    match delimiter {
        Delimiter::Comma => {
            let bytes = text.as_bytes();
            let mut scanned = 0;
            let mut source_line = 1;
            // csv positions may point at the LF left over from a CRLF terminator.
            // Map byte positions back to physical lines without changing field contents.
            let mut line_at = |position: Option<&csv::Position>| {
                let mut offset = position.map_or(0, |p| p.byte() as usize).min(bytes.len());
                while offset < bytes.len() && matches!(bytes[offset], b'\r' | b'\n') {
                    offset += 1;
                }
                for index in scanned..offset {
                    if bytes[index] == b'\r'
                        || (bytes[index] == b'\n' && (index == 0 || bytes[index - 1] != b'\r'))
                    {
                        source_line += 1;
                    }
                }
                scanned = offset;
                source_line
            };
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .from_reader(text.as_bytes());
            for record in reader.records() {
                match record {
                    Ok(record) => {
                        if record.iter().all(|f| f.trim().is_empty()) && record.len() == 1 {
                            continue;
                        }
                        let line = line_at(record.position());
                        consume(line, Ok(record.iter().map(String::from).collect()))?;
                    }
                    Err(error) => consume(
                        line_at(error.position()),
                        Err("CSV 无法解析，请检查 UTF-8 编码、引号和分隔符".into()),
                    )?,
                }
            }
        }
        Delimiter::Space => {
            for (index, line) in text.lines().enumerate() {
                if !line.trim().is_empty() {
                    consume(index + 1, whitespace_fields(line).map_err(String::from))?;
                }
            }
        }
        Delimiter::Auto => unreachable!(),
    }
    Ok(result)
}

/// Apply the entire validated batch to a cloned document; the caller persists it once.
pub fn apply(
    document: &mut Document,
    table_id: &str,
    rows: Vec<ImportRow>,
    policy: DuplicatePolicy,
) -> Result<()> {
    if rows.is_empty() || rows.len() > MAX_IMPORT_ROWS {
        return Err(AppError::invalid("请选择 1 至 10000 条记录导入"));
    }
    if rows
        .iter()
        .map(|r| {
            r.key
                .len()
                .saturating_add(r.value.len())
                .saturating_add(r.notes.len())
        })
        .sum::<usize>()
        > MAX_IMPORT_BYTES
    {
        return Err(AppError::invalid("导入内容不能超过 10 MiB"));
    }
    if !document
        .tables
        .iter()
        .any(|t| t.id == table_id && !t.deleted)
    {
        return Err(AppError::invalid("请选择有效的目标数据表"));
    }
    let mut existing: HashMap<String, usize> = document
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.deleted && e.table_id == table_id)
        .map(|(i, e)| (e.key.clone(), i))
        .collect();
    let mut sort_order = next_sort_order(document);
    for row in rows {
        let key = row.key.trim();
        if key.is_empty() {
            return Err(AppError::invalid(format!(
                "第 {} 行 Key 不能为空，未导入任何记录",
                row.line
            )));
        }
        if let Some(&index) = existing.get(key) {
            if matches!(policy, DuplicatePolicy::Overwrite) {
                let entry = &mut document.entries[index];
                entry.value = row.value;
                entry.notes = row.notes;
                entry.updated_at = now();
            }
        } else {
            existing.insert(key.into(), document.entries.len());
            document.entries.push(Entry {
                id: id(),
                table_id: table_id.into(),
                key: key.into(),
                value: row.value,
                notes: row.notes,
                sort_order,
                created_at: now(),
                updated_at: now(),
                deleted: false,
            });
            sort_order = sort_order.saturating_add(1);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_supports_bom_headers_quotes_and_multiline_values() {
        let preview = parse("\u{feff}key,value,notes\r\ntoken,\"a,b\",\"a \"\"quote\"\"\"\r\nmultiline,\"one\ntwo\",\n", Delimiter::Comma, HeaderMode::Auto).unwrap();
        assert!(preview.has_header);
        assert!(preview.errors.is_empty());
        assert_eq!(preview.rows.len(), 2);
        assert_eq!(preview.rows[0].value, "a,b");
        assert_eq!(preview.rows[0].notes, "a \"quote\"");
        assert_eq!(preview.rows[1].value, "one\ntwo");
        assert_eq!(preview.rows[0].line, 2);
        assert_eq!(preview.rows[1].line, 3);
    }
    #[test]
    fn csv_reports_physical_lines_for_all_line_endings() {
        for newline in ["\n", "\r\n", "\r"] {
            let text = format!(
                "{newline}key,value{newline}first,\"one{newline}two\"{newline}second,value{newline}invalid{newline}"
            );
            let preview = parse(&text, Delimiter::Comma, HeaderMode::Auto).unwrap();
            assert!(preview.has_header);
            assert_eq!(preview.rows[0].line, 3);
            assert_eq!(preview.rows[0].value, format!("one{newline}two"));
            assert_eq!(preview.rows[1].line, 5);
            assert_eq!(preview.errors[0].line, 6);
        }
    }
    #[test]
    fn space_text_supports_quoted_values_empty_fields_and_reports_lines() {
        let preview = parse("\nalpha  \"hello world\"  备注\r\nbeta\t\"\"\ngamma only one extra column\nlast \"unclosed", Delimiter::Space, HeaderMode::None).unwrap();
        assert_eq!(preview.rows.len(), 2);
        assert_eq!(preview.rows[0].line, 2);
        assert_eq!(preview.rows[0].value, "hello world");
        assert_eq!(preview.rows[1].value, "");
        assert_eq!(preview.errors.len(), 2);
        assert_eq!(preview.errors[0].line, 4);
    }
    #[test]
    fn auto_detects_comma_and_does_not_drop_headerless_data() {
        let spaced = parse(
            "token \"value, with commas\" 备注",
            Delimiter::Auto,
            HeaderMode::Auto,
        )
        .unwrap();
        assert!(spaced.errors.is_empty());
        assert_eq!(spaced.rows[0].value, "value, with commas");
        let preview = parse("name,alice\nkey,value", Delimiter::Auto, HeaderMode::Auto).unwrap();
        assert_eq!(preview.rows.len(), 2);
        assert!(!preview.has_header);
        let preview = parse("key,value\nname,alice", Delimiter::Comma, HeaderMode::None).unwrap();
        assert_eq!(preview.rows.len(), 2);
        assert_eq!(
            parse(
                ",value\ninvalid\nok,value",
                Delimiter::Comma,
                HeaderMode::Auto
            )
            .unwrap()
            .errors
            .len(),
            2
        );
    }
    #[test]
    fn import_limits_reject_oversized_batches() {
        assert!(
            parse(
                &"a,v\n".repeat(MAX_IMPORT_ROWS + 1),
                Delimiter::Comma,
                HeaderMode::None
            )
            .is_err()
        );
        assert!(
            parse(
                &"a".repeat(MAX_IMPORT_BYTES + 1),
                Delimiter::Space,
                HeaderMode::None
            )
            .is_err()
        );
    }
}
