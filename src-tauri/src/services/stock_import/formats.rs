//! Custom mappings remembered per header layout.
//!
//! The formats live in `app_config` under [`CONFIG_KEY`] as a JSON array of
//! [`SavedStockImportFormat`] (the wire type), newest first, at most
//! [`MAX_FORMATS`] (the oldest are dropped). A format is matched next time by
//! the `header_signature` of the file it was saved from, so one layout has one
//! format: saving it again replaces the earlier entry and keeps its id, which
//! the source of earlier imports (`format:<id>`) and their broker ids refer to.

use rusqlite::OptionalExtension;

use super::types::{header_signature, SavedStockImportFormat, StockImportConfig};
use crate::error::{AppError, Result};

/// `app_config` key of the saved formats.
pub const CONFIG_KEY: &str = "stock_import_formats";
/// Formats kept; saving one more drops the oldest.
pub const MAX_FORMATS: usize = 20;
const MAX_NAME_CHARS: usize = 60;

/// The saved formats, newest first. Unreadable stored data (a format written
/// by another version, a damaged value) is skipped, never an error: the wizard
/// must open whatever is in there.
pub fn list_formats(conn: &rusqlite::Connection) -> Result<Vec<SavedStockImportFormat>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [CONFIG_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let items = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Array(items)) => items,
        _ => {
            log::warn!("[STOCK IMPORT] Saved formats are unreadable and were ignored");
            return Ok(Vec::new());
        }
    };
    Ok(items
        .into_iter()
        .filter_map(
            |item| match serde_json::from_value::<SavedStockImportFormat>(item) {
                Ok(format) => Some(format),
                Err(e) => {
                    log::warn!("[STOCK IMPORT] A saved format is unreadable and was ignored");
                    log::debug!("[STOCK IMPORT] Unreadable saved format: {e}");
                    None
                }
            },
        )
        .collect())
}

fn write_formats(conn: &rusqlite::Connection, formats: &[SavedStockImportFormat]) -> Result<()> {
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![CONFIG_KEY, serde_json::to_string(formats)?],
    )?;
    Ok(())
}

/// Remember `config` for files with these `headers`. What belongs to one file
/// (instrument overrides, "import anyway" lines) is not kept; the stored
/// config's `source` is the format's own id.
pub fn save_format(
    conn: &rusqlite::Connection,
    name: &str,
    headers: &[String],
    config: &StockImportConfig,
) -> Result<SavedStockImportFormat> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("validation.nameRequired".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::Validation("validation.nameTooLong".into()));
    }
    if headers.iter().all(|h| h.trim().is_empty()) {
        return Err(AppError::Validation("validation.csvEmptyFile".into()));
    }
    config.validate()?;

    let signature = header_signature(headers);
    let mut formats = list_formats(conn)?;
    let id = match formats.iter().position(|f| f.header_signature == signature) {
        Some(index) => formats.remove(index).id,
        None => format!("format:{}", uuid::Uuid::new_v4()),
    };

    let mut config = config.clone();
    config.source = id.clone();
    config.instrument_overrides.clear();
    config.import_anyway_lines.clear();
    let saved = SavedStockImportFormat {
        id,
        name: name.to_string(),
        header_signature: signature,
        config,
        created_at: chrono::Utc::now().timestamp(),
    };

    formats.insert(0, saved.clone());
    formats.truncate(MAX_FORMATS);
    write_formats(conn, &formats)?;
    Ok(saved)
}

pub fn delete_format(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let mut formats = list_formats(conn)?;
    let before = formats.len();
    formats.retain(|f| f.id != id);
    if formats.len() == before {
        return Err(AppError::NotFound("Saved format not found".into()));
    }
    write_formats(conn, &formats)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::simulate::test_db::{config, db, over};
    use super::super::types::{
        header_signature, StockImportTransforms, StockTypeValueMapping, TypeValueAction,
    };
    use super::*;

    const KEY: &str = "stock_import_formats";

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|h| h.to_string()).collect()
    }

    fn stored_json(conn: &Connection) -> Option<String> {
        conn.query_row("SELECT value FROM app_config WHERE key = ?1", [KEY], |r| {
            r.get(0)
        })
        .ok()
    }

    fn validation_key(result: Result<SavedStockImportFormat>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn nothing_is_saved_at_first() {
        let conn = db();
        assert!(list_formats(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_saved_format_is_remembered_by_its_header_layout() {
        let conn = db();
        let h = headers(&["Datum", "Akce", "Ticker", "Ks", "Cena", "Měna"]);
        let mut cfg = config("custom");
        cfg.transforms = StockImportTransforms {
            asset_class_column: Some(7),
            asset_class_allowed: vec!["STK".into()],
            ..StockImportTransforms::default()
        };
        cfg.type_values = vec![StockTypeValueMapping {
            value: "Koupě".into(),
            action: TypeValueAction::Buy,
        }];

        let saved = save_format(&conn, "  Můj broker  ", &h, &cfg).unwrap();

        assert!(saved.id.starts_with("format:"), "{}", saved.id);
        assert!(uuid::Uuid::parse_str(&saved.id["format:".len()..]).is_ok());
        assert_eq!(saved.name, "Můj broker");
        assert_eq!(saved.header_signature, header_signature(&h));
        assert!(saved.created_at > 1_700_000_000);
        assert_eq!(
            saved.config.source, saved.id,
            "the format is the source of the files read with it"
        );
        assert_eq!(saved.config.transforms, cfg.transforms);
        assert_eq!(saved.config.type_values, cfg.type_values);

        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed, vec![saved]);
    }

    #[test]
    fn what_belongs_to_one_file_is_not_remembered() {
        let conn = db();
        let mut cfg = config("custom");
        cfg.instrument_overrides = vec![over("symbol:AAPL")];
        cfg.import_anyway_lines = vec![4, 9];

        let saved = save_format(&conn, "Mine", &headers(&["a", "b"]), &cfg).unwrap();

        assert!(saved.config.instrument_overrides.is_empty());
        assert!(saved.config.import_anyway_lines.is_empty());
    }

    #[test]
    fn formats_are_listed_newest_first() {
        let conn = db();
        for (i, name) in ["first", "second", "third"].iter().enumerate() {
            save_format(
                &conn,
                name,
                &headers(&["col", &i.to_string()]),
                &config("custom"),
            )
            .unwrap();
        }
        let names: Vec<String> = list_formats(&conn)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, vec!["third", "second", "first"]);
    }

    #[test]
    fn at_most_twenty_formats_are_kept_and_the_oldest_go() {
        let conn = db();
        for i in 0..22 {
            save_format(
                &conn,
                &format!("format {i}"),
                &headers(&["col", &i.to_string()]),
                &config("custom"),
            )
            .unwrap();
        }
        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 20);
        assert_eq!(listed[0].name, "format 21");
        assert_eq!(listed[19].name, "format 2");
    }

    #[test]
    fn saving_a_layout_again_replaces_its_format_and_keeps_its_id() {
        let conn = db();
        let h = headers(&["Date", "Ticker", "Qty"]);
        let first = save_format(&conn, "Old name", &h, &config("custom")).unwrap();
        save_format(&conn, "Other", &headers(&["x"]), &config("custom")).unwrap();

        let mut cfg = config("custom");
        cfg.decimal_separator = ",".into();
        // Same layout, written differently: case and spaces do not matter.
        let again = save_format(
            &conn,
            "New name",
            &headers(&["DATE ", "ticker", "qty"]),
            &cfg,
        )
        .unwrap();

        assert_eq!(
            again.id, first.id,
            "files imported earlier keep their source"
        );
        let listed = list_formats(&conn).unwrap();
        let names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["New name", "Other"],
            "replaced, and now the newest"
        );
        assert_eq!(listed[0].config.decimal_separator, ",");
    }

    #[test]
    fn a_format_without_a_name_layout_or_valid_config_is_refused() {
        let conn = db();
        let h = headers(&["a"]);
        assert_eq!(
            validation_key(save_format(&conn, "   ", &h, &config("custom"))),
            "validation.nameRequired"
        );
        assert_eq!(
            validation_key(save_format(&conn, &"x".repeat(61), &h, &config("custom"))),
            "validation.nameTooLong"
        );
        assert_eq!(
            validation_key(save_format(
                &conn,
                "ok",
                &headers(&["", "  "]),
                &config("custom")
            )),
            "validation.csvEmptyFile"
        );
        let mut broken = config("custom");
        broken.symbol_column = None;
        assert_eq!(
            validation_key(save_format(&conn, "ok", &h, &broken)),
            "validation.stockImportSymbolRequired"
        );
        assert!(list_formats(&conn).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_format_removes_only_that_one() {
        let conn = db();
        let a = save_format(&conn, "a", &headers(&["a"]), &config("custom")).unwrap();
        let b = save_format(&conn, "b", &headers(&["b"]), &config("custom")).unwrap();

        delete_format(&conn, &a.id).unwrap();
        assert_eq!(list_formats(&conn).unwrap(), vec![b.clone()]);

        delete_format(&conn, &b.id).unwrap();
        assert!(list_formats(&conn).unwrap().is_empty());
    }

    #[test]
    fn deleting_an_unknown_format_is_not_found() {
        let conn = db();
        save_format(&conn, "a", &headers(&["a"]), &config("custom")).unwrap();
        let err = delete_format(&conn, "format:nope").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert_eq!(list_formats(&conn).unwrap().len(), 1);
    }

    #[test]
    fn formats_are_stored_as_the_wire_json_in_app_config() {
        let conn = db();
        save_format(&conn, "a", &headers(&["a"]), &config("custom")).unwrap();
        let json = stored_json(&conn).expect("stored under the key");
        assert!(json.starts_with('['), "{json}");
        assert!(json.contains("\"headerSignature\""), "{json}");
        assert!(json.contains("\"createdAt\""), "{json}");
        assert!(json.contains("\"dateColumn\""), "{json}");
    }

    #[test]
    fn unreadable_stored_data_never_breaks_the_list() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, 'not json')",
            [KEY],
        )
        .unwrap();
        assert!(list_formats(&conn).unwrap().is_empty());
        // A new save recovers the key.
        save_format(&conn, "a", &headers(&["a"]), &config("custom")).unwrap();
        assert_eq!(list_formats(&conn).unwrap().len(), 1);

        // One entry that no longer reads (an old shape) is skipped, the rest stay.
        let good = serde_json::to_value(list_formats(&conn).unwrap()).unwrap();
        let mut entries = good.as_array().unwrap().clone();
        entries.insert(0, serde_json::json!({ "id": "format:old", "name": 5 }));
        conn.execute(
            "UPDATE app_config SET value = ?1 WHERE key = ?2",
            rusqlite::params![serde_json::Value::Array(entries).to_string(), KEY],
        )
        .unwrap();
        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "a");
    }
}
