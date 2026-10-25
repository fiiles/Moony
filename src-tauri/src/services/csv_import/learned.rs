//! Layouts a person confirmed once, recognised next month (mobile spec §8.6 rung 4).
//!
//! The formats live in `app_config` under [`CONFIG_KEY`] as a JSON array of
//! [`LearnedCsvFormat`] (the wire type), at most [`MAX_FORMATS`], newest used first. A file is
//! matched by its [`layout_signature`]: the header cells, the delimiter and the column count.
//! The text encoding and the position of the header row are stored but are not in the key: each
//! month's file is decoded and its header row found by the file's own detection, and a bank
//! that adds a preamble line or whose month has no accented letter in its rows must still be
//! recognised. One layout has one format; saving it again replaces the earlier entry and keeps
//! its id and creation date.
//!
//! A header row that looks like a transaction (a date, an amount, a long number in a cell) is
//! never learned, so no row value is ever stored. Nothing here writes a header, a column name
//! or any other file content to a log.

use rusqlite::{Connection, OptionalExtension};

use super::amounts::clean_and_parse_amount;
use super::columns::find_column;
use super::types::{
    parse_decimal_separator, InsertLearnedCsvFormat, LearnedCsvFormat, LearnedCsvMapping,
};
use crate::error::{AppError, Result};
use crate::services::{csv_presets, date_parser};

/// `app_config` key of the learned formats.
pub const CONFIG_KEY: &str = "learned_csv_formats";
/// Formats kept; saving one more drops the one used least recently.
pub const MAX_FORMATS: usize = 20;
const MAX_NAME_CHARS: usize = 60;
/// A cell holding this many digits in a row is a number of a transaction or an account, not a
/// column name.
const DATA_DIGIT_RUN: usize = 6;

const SIGNATURE_VERSION: &str = "v1";
const PART_SEPARATOR: char = '\u{1e}';
const CELL_SEPARATOR: char = '\u{1f}';

/// A header cell as the signature compares it: no BOM, trimmed, whitespace runs collapsed to
/// one space, lowercase. An empty cell stays empty.
fn normalise_cell(cell: &str) -> String {
    cell.trim()
        .trim_start_matches('\u{feff}')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The key of a layout: `v1`, the delimiter, the column count and the normalised header cells,
/// the parts joined with U+001E and the cells with U+001F. A trailing delimiter (an empty last
/// cell) is part of the layout. The encoding and the header row's position are not.
pub fn layout_signature(headers: &[String], delimiter: &str) -> String {
    let cells: Vec<String> = headers.iter().map(|cell| normalise_cell(cell)).collect();
    format!(
        "{SIGNATURE_VERSION}{PART_SEPARATOR}{delimiter}{PART_SEPARATOR}{}{PART_SEPARATOR}{}",
        cells.len(),
        cells.join(&CELL_SEPARATOR.to_string())
    )
}

fn has_long_digit_run(cell: &str) -> bool {
    let mut run = 0;
    for c in cell.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= DATA_DIGIT_RUN {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// An amount the import would read: a number with a digit in it, in either decimal notation.
fn reads_as_amount(cell: &str) -> bool {
    cell.chars().any(|c| c.is_ascii_digit())
        && [',', '.']
            .into_iter()
            .any(|decimal| clean_and_parse_amount(cell, decimal).is_ok())
}

/// Whether one of the cells of what was taken for a header row is a value: a date, an amount or
/// a run of at least six digits (an account or document number). A file without a header line
/// has its first transaction there, and a transaction is never learned.
pub fn header_looks_like_data(headers: &[String]) -> bool {
    headers
        .iter()
        .map(|cell| cell.trim().trim_start_matches('\u{feff}').trim())
        .any(|cell| {
            !cell.is_empty()
                && (has_long_digit_run(cell)
                    || date_parser::parse_date(cell, None).is_some()
                    || reads_as_amount(cell))
        })
}

fn is_blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|s| s.trim().is_empty())
}

fn validated_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Validation("validation.nameRequired".into()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(AppError::Validation("validation.nameTooLong".into()));
    }
    Ok(name.to_string())
}

/// The mapping in the one form that is stored: no blank column names, `auto` for an empty date
/// format, the decimal separator as `auto`, `,` or `.`.
fn tidy_mapping(mapping: &LearnedCsvMapping) -> Result<LearnedCsvMapping> {
    let optional = |value: &Option<String>| value.clone().filter(|s| !s.trim().is_empty());
    let date_format = mapping.date_format.trim();
    Ok(LearnedCsvMapping {
        date_column: mapping.date_column.clone(),
        date_format: if date_format.is_empty() || date_format.eq_ignore_ascii_case("auto") {
            "auto".to_string()
        } else {
            mapping.date_format.clone()
        },
        amount_column: optional(&mapping.amount_column),
        debit_column: optional(&mapping.debit_column),
        credit_column: optional(&mapping.credit_column),
        balance_column: optional(&mapping.balance_column),
        transaction_id_column: optional(&mapping.transaction_id_column),
        description_columns: mapping
            .description_columns
            .iter()
            .filter(|s| !s.trim().is_empty())
            .cloned()
            .collect(),
        counterparty_column: optional(&mapping.counterparty_column),
        counterparty_iban_column: optional(&mapping.counterparty_iban_column),
        counterparty_bank_code_column: optional(&mapping.counterparty_bank_code_column),
        counterparty_country_column: optional(&mapping.counterparty_country_column),
        currency_column: optional(&mapping.currency_column),
        decimal_separator: parse_decimal_separator(Some(&mapping.decimal_separator))?
            .map_or_else(|| "auto".to_string(), |c| c.to_string()),
    })
}

/// Every column the mapping names.
fn named_columns(mapping: &LearnedCsvMapping) -> Vec<&str> {
    let optional = [
        &mapping.amount_column,
        &mapping.debit_column,
        &mapping.credit_column,
        &mapping.balance_column,
        &mapping.transaction_id_column,
        &mapping.counterparty_column,
        &mapping.counterparty_iban_column,
        &mapping.counterparty_bank_code_column,
        &mapping.counterparty_country_column,
        &mapping.currency_column,
    ];
    std::iter::once(mapping.date_column.as_str())
        .chain(optional.into_iter().flatten().map(String::as_str))
        .chain(mapping.description_columns.iter().map(String::as_str))
        .filter(|name| !name.trim().is_empty())
        .collect()
}

impl InsertLearnedCsvFormat {
    /// Validate at the trust boundary. Keys are `validation.*` i18n keys (`common` namespace).
    pub fn validate(&self) -> Result<()> {
        validated_name(&self.name)?;
        if self.headers.iter().filter(|h| !h.trim().is_empty()).count() < 2 {
            return Err(AppError::Validation("validation.csvEmptyFile".into()));
        }
        if header_looks_like_data(&self.headers) {
            return Err(AppError::Validation(
                "validation.learnedFormatHeaderless".into(),
            ));
        }
        // The csv reader takes one byte as the delimiter.
        let mut delimiter = self.delimiter.chars();
        if !matches!((delimiter.next(), delimiter.next()), (Some(c), None) if c.is_ascii()) {
            return Err(AppError::Validation(
                "validation.csvDelimiterInvalid".into(),
            ));
        }

        let mapping = &self.mapping;
        if mapping.date_column.trim().is_empty() {
            return Err(AppError::Validation("validation.dateColumnRequired".into()));
        }
        let has_amount = !is_blank(&mapping.amount_column);
        let has_pair = !is_blank(&mapping.debit_column) && !is_blank(&mapping.credit_column);
        if !has_amount && !has_pair {
            return Err(AppError::Validation(
                "validation.amountColumnRequired".into(),
            ));
        }
        parse_decimal_separator(Some(&mapping.decimal_separator))?;
        if named_columns(mapping)
            .into_iter()
            .any(|name| find_column(&self.headers, name).is_none())
        {
            return Err(AppError::Validation(
                "validation.learnedFormatColumns".into(),
            ));
        }
        Ok(())
    }
}

/// The stored formats as written: unreadable data (a format written by another version, a
/// damaged value) is skipped, never an error, so the import and the settings page open
/// whatever is in there. The log says that something was skipped, not what it held.
fn read_stored(conn: &Connection) -> Result<Vec<LearnedCsvFormat>> {
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
            log::warn!("[CSV IMPORT] Learned formats are unreadable and were ignored");
            return Ok(Vec::new());
        }
    };
    Ok(items
        .into_iter()
        .filter_map(
            |item| match serde_json::from_value::<LearnedCsvFormat>(item) {
                Ok(format) => Some(format),
                Err(_) => {
                    log::warn!("[CSV IMPORT] A learned format is unreadable and was ignored");
                    None
                }
            },
        )
        .collect())
}

/// The stored formats, the one used last first. The sort is stable, so formats used in the same
/// second keep the order they were stored in (the one saved or used last is stored first).
fn read_ordered(conn: &Connection) -> Result<Vec<LearnedCsvFormat>> {
    let mut formats = read_stored(conn)?;
    formats.sort_by_key(|f| std::cmp::Reverse(f.last_used_at));
    Ok(formats)
}

fn write_formats(conn: &Connection, formats: &[LearnedCsvFormat]) -> Result<()> {
    let stored: Vec<LearnedCsvFormat> = formats
        .iter()
        .map(|f| LearnedCsvFormat {
            shadowed_by_preset_id: None,
            ..f.clone()
        })
        .collect();
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![CONFIG_KEY, serde_json::to_string(&stored)?],
    )?;
    Ok(())
}

/// `format` with the shipped preset that now matches its headers, if any (a preset always wins
/// over a learned format, so the settings list says when one is shadowed).
fn with_shadow(format: LearnedCsvFormat) -> LearnedCsvFormat {
    let shadowed_by_preset_id = csv_presets::detect_preset(&format.headers).map(|p| p.id.clone());
    LearnedCsvFormat {
        shadowed_by_preset_id,
        ..format
    }
}

fn not_found() -> AppError {
    AppError::NotFound("Learned format not found".into())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// The learned formats, the one used last first.
pub fn list_formats(conn: &Connection) -> Result<Vec<LearnedCsvFormat>> {
    Ok(read_ordered(conn)?.into_iter().map(with_shadow).collect())
}

/// The format learned for a file with these header cells and this delimiter.
pub fn find_format(
    conn: &Connection,
    headers: &[String],
    delimiter: &str,
) -> Result<Option<LearnedCsvFormat>> {
    let signature = layout_signature(headers, delimiter);
    Ok(read_ordered(conn)?
        .into_iter()
        .find(|f| f.signature == signature)
        .map(with_shadow))
}

/// Remember the layout of `input`. One layout has one format: an existing format of the same
/// signature keeps its id and creation date and takes the new name, headers, encoding, header
/// row, mapping and account. The saved format counts as used now; a 21st format drops the one
/// used least recently.
pub fn save_format(conn: &Connection, input: &InsertLearnedCsvFormat) -> Result<LearnedCsvFormat> {
    input.validate()?;
    let name = validated_name(&input.name)?;
    let mapping = tidy_mapping(&input.mapping)?;
    let signature = layout_signature(&input.headers, &input.delimiter);
    let now = now();

    let mut formats = read_ordered(conn)?;
    let (id, created_at) = match formats.iter().position(|f| f.signature == signature) {
        Some(index) => {
            let earlier = formats.remove(index);
            (earlier.id, earlier.created_at)
        }
        None => (format!("learned:{}", uuid::Uuid::new_v4()), now),
    };
    let saved = LearnedCsvFormat {
        id,
        name,
        signature,
        headers: input.headers.clone(),
        delimiter: input.delimiter.clone(),
        encoding: input.encoding.clone(),
        header_row: input.header_row,
        mapping,
        account_id: input.account_id.clone(),
        created_at,
        last_used_at: now,
        shadowed_by_preset_id: None,
    };

    formats.insert(0, saved.clone());
    formats.truncate(MAX_FORMATS);
    write_formats(conn, &formats)?;
    Ok(with_shadow(saved))
}

/// Give a format another name (trimmed, 1–60 characters). Its place in the list stays.
pub fn rename_format(conn: &Connection, id: &str, name: &str) -> Result<LearnedCsvFormat> {
    let name = validated_name(name)?;
    let mut formats = read_ordered(conn)?;
    let format = formats
        .iter_mut()
        .find(|f| f.id == id)
        .ok_or_else(not_found)?;
    format.name = name;
    let renamed = format.clone();
    write_formats(conn, &formats)?;
    Ok(with_shadow(renamed))
}

pub fn delete_format(conn: &Connection, id: &str) -> Result<()> {
    let mut formats = read_ordered(conn)?;
    let before = formats.len();
    formats.retain(|f| f.id != id);
    if formats.len() == before {
        return Err(AppError::NotFound("Learned format not found".into()));
    }
    write_formats(conn, &formats)
}

/// A successful import used this format: it is the most recently used one now.
pub fn mark_used(conn: &Connection, id: &str) -> Result<()> {
    let mut formats = read_ordered(conn)?;
    let index = formats
        .iter()
        .position(|f| f.id == id)
        .ok_or_else(not_found)?;
    let mut used = formats.remove(index);
    used.last_used_at = now();
    formats.insert(0, used);
    write_formats(conn, &formats)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::super::decode::{detect_csv_delimiter, detect_header_row, read_headers};
    use super::super::inspect::decode_for_import_in;
    use super::super::test_support::{fixture_bytes, FIXTURE_DIR};
    use super::super::types::{InsertLearnedCsvFormat, LearnedCsvFormat, LearnedCsvMapping};
    use super::*;
    use crate::error::AppError;

    const KEY: &str = "learned_csv_formats";

    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .expect("schema");
        conn
    }

    fn headers(names: &[&str]) -> Vec<String> {
        names.iter().map(|h| h.to_string()).collect()
    }

    fn stored_json(conn: &Connection) -> Option<String> {
        conn.query_row("SELECT value FROM app_config WHERE key = ?1", [KEY], |r| {
            r.get(0)
        })
        .ok()
    }

    fn store(conn: &Connection, formats: &[LearnedCsvFormat]) {
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![KEY, serde_json::to_string(formats).unwrap()],
        )
        .unwrap();
    }

    /// Every stored format looks `seconds` older, as if it had been saved and used earlier.
    fn age(conn: &Connection, seconds: i64) {
        let mut formats = list_formats(conn).unwrap();
        for format in &mut formats {
            format.created_at -= seconds;
            format.last_used_at -= seconds;
        }
        store(conn, &formats);
    }

    fn validation_key<T: std::fmt::Debug>(result: Result<T>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => format!("{other:?}"),
        }
    }

    fn mapping() -> LearnedCsvMapping {
        LearnedCsvMapping {
            date_column: "Datum".into(),
            date_format: "%d.%m.%Y".into(),
            amount_column: Some("Částka".into()),
            debit_column: None,
            credit_column: None,
            balance_column: None,
            transaction_id_column: None,
            description_columns: vec!["Poznámka".into()],
            counterparty_column: None,
            counterparty_iban_column: None,
            counterparty_bank_code_column: None,
            counterparty_country_column: None,
            currency_column: None,
            decimal_separator: ",".into(),
        }
    }

    fn input(name: &str, header_cells: &[&str]) -> InsertLearnedCsvFormat {
        InsertLearnedCsvFormat {
            name: name.into(),
            headers: headers(header_cells),
            delimiter: ";".into(),
            encoding: "UTF-8".into(),
            header_row: 0,
            mapping: mapping(),
            account_id: None,
        }
    }

    /// A layout of its own for every `n` (0..26), with headers no data check can mistake for a
    /// value.
    fn input_n(n: u8) -> InsertLearnedCsvFormat {
        let last = format!("Pole {}", (b'a' + n) as char);
        input(
            &format!("Banka {n}"),
            &["Datum", "Částka", "Poznámka", last.as_str()],
        )
    }

    const UNICREDIT_LIKE: [&str; 9] = [
        "Číslo účtu",
        "Datum zaúčtování",
        "Datum valuty",
        "Částka",
        "Měna",
        "Zůstatek po transakci",
        "Název protistrany",
        "Typ transakce",
        "Detail",
    ];

    fn unicredit_like_input() -> InsertLearnedCsvFormat {
        InsertLearnedCsvFormat {
            name: "UniCredit".into(),
            headers: headers(&UNICREDIT_LIKE),
            delimiter: ";".into(),
            encoding: "UTF-8".into(),
            header_row: 0,
            mapping: LearnedCsvMapping {
                date_column: "Datum zaúčtování".into(),
                amount_column: Some("Částka".into()),
                balance_column: Some("Zůstatek po transakci".into()),
                currency_column: Some("Měna".into()),
                counterparty_column: Some("Název protistrany".into()),
                description_columns: vec!["Detail".into()],
                ..mapping()
            },
            account_id: None,
        }
    }

    /// What the import reads of a file before it decides anything: the decoded text's delimiter,
    /// header row and header cells, and the encoding it was decoded with.
    fn read_layout(bytes: &[u8]) -> (Vec<String>, String, usize, &'static str) {
        let decoded = decode_for_import_in(bytes, None, None, Some("CZ"));
        let delimiter = detect_csv_delimiter(&decoded.text);
        let header_row = detect_header_row(&decoded.text, delimiter);
        let cells = read_headers(&decoded.text, delimiter, header_row);
        (cells, delimiter.to_string(), header_row, decoded.encoding)
    }

    #[test]
    fn nothing_is_learned_at_first() {
        let conn = db();
        assert!(list_formats(&conn).unwrap().is_empty());
        assert!(find_format(&conn, &headers(&["Datum", "Částka"]), ";")
            .unwrap()
            .is_none());
        assert!(stored_json(&conn).is_none(), "reading writes nothing");
    }

    #[test]
    fn the_signature_ignores_case_spaces_and_a_bom() {
        assert_eq!(
            layout_signature(&headers(&["\u{feff}Datum ", "ČÁSTKA"]), ";"),
            layout_signature(&headers(&["datum", "částka"]), ";")
        );
        assert_eq!(
            layout_signature(&headers(&["Datum   zaúčtování", "Částka"]), ";"),
            layout_signature(&headers(&["  datum zaúčtování ", "ČÁSTKA"]), ";")
        );
        assert_eq!(
            layout_signature(&headers(&["datum", "částka"]), ";"),
            "v1\u{1e};\u{1e}2\u{1e}datum\u{1f}částka"
        );
        assert_ne!(
            layout_signature(&headers(&["datum", "částka"]), ";"),
            layout_signature(&headers(&["datum", "částka"]), ","),
            "the delimiter is part of the layout"
        );
        assert_ne!(
            layout_signature(&headers(&["datum", "částka", "poznámka"]), ";"),
            layout_signature(&headers(&["datum", "částka", "poznámka", "měna"]), ";"),
            "the column count is part of the layout"
        );
        assert_ne!(
            layout_signature(&headers(&["datum", "částka", ""]), ";"),
            layout_signature(&headers(&["datum", "částka"]), ";"),
            "a trailing delimiter is part of the layout"
        );
        assert_ne!(
            layout_signature(&headers(&["datum", "částka"]), ";"),
            layout_signature(&headers(&["částka", "datum"]), ";"),
            "the order of the columns is part of the layout"
        );
    }

    #[test]
    fn the_same_layout_matches_across_months_encodings_and_preambles() {
        let conn = db();
        let september = fixture_bytes("unknown-unicredit-like.csv");
        let october = fixture_bytes("unknown-unicredit-like-october.csv");

        let (september_headers, delimiter, header_row, encoding) = read_layout(&september);
        assert_eq!(
            (delimiter.as_str(), header_row, encoding),
            (";", 0, "UTF-8"),
            "the September export as it is"
        );
        let saved = save_format(&conn, &unicredit_like_input()).unwrap();
        assert_eq!(saved.signature, layout_signature(&september_headers, ";"));

        // Another month of the same bank.
        let (october_headers, ..) = read_layout(&october);
        assert_eq!(
            layout_signature(&october_headers, ";"),
            saved.signature,
            "other rows, same layout"
        );

        // The same month in Windows-1250 (the bank's legacy export) behind one more line.
        let preamble = "Výpis z účtu;;;;;;;;\n";
        let with_preamble = format!("{preamble}{}", String::from_utf8(october.clone()).unwrap());
        let (legacy, _, had_errors) = encoding_rs::WINDOWS_1250.encode(&with_preamble);
        assert!(!had_errors, "every character is in Windows-1250");
        assert!(String::from_utf8(legacy.to_vec()).is_err(), "not UTF-8");
        let (legacy_headers, legacy_delimiter, legacy_header_row, legacy_encoding) =
            read_layout(&legacy);
        assert_eq!(
            (
                legacy_delimiter.as_str(),
                legacy_header_row,
                legacy_encoding
            ),
            (";", 1, "windows-1250"),
            "another encoding and another header row"
        );
        assert_eq!(
            layout_signature(&legacy_headers, &legacy_delimiter),
            saved.signature,
            "neither the encoding nor the position of the header row is in the key"
        );

        for cells in [&october_headers, &legacy_headers] {
            let found = find_format(&conn, cells, ";").unwrap().expect("recognised");
            assert_eq!(found.id, saved.id, "the format saved from September");
            assert_eq!(found.name, "UniCredit");
        }
        assert!(
            find_format(&conn, &october_headers, ",").unwrap().is_none(),
            "another delimiter is another layout"
        );
    }

    #[test]
    fn a_data_row_is_never_learned() {
        let conn = db();
        for header_cells in [
            vec!["29.09.2026", "Spotify", "-199,00"],
            vec!["Datum", "1 525,77"],
            vec!["1234567890/2700", "Datum", "Částka"],
        ] {
            let mut data_row = input("Data", &header_cells);
            data_row.mapping.date_column = header_cells[0].into();
            data_row.mapping.amount_column = Some(header_cells[1].into());
            data_row.mapping.description_columns = Vec::new();
            assert!(
                header_looks_like_data(&data_row.headers),
                "{header_cells:?}"
            );
            assert_eq!(
                validation_key(data_row.validate()),
                "validation.learnedFormatHeaderless",
                "{header_cells:?}"
            );
            assert_eq!(
                validation_key(save_format(&conn, &data_row)),
                "validation.learnedFormatHeaderless",
                "{header_cells:?}"
            );
        }
        assert!(list_formats(&conn).unwrap().is_empty());
        assert!(
            stored_json(&conn).is_none(),
            "no row value reached the store"
        );
    }

    #[test]
    fn a_value_in_the_header_row_is_recognised_as_data() {
        for cells in [
            vec!["Datum", "-199,00"],
            vec!["Datum", "199"],
            vec!["Datum", "1,234.50"],
            vec!["Datum", "CZK 1 200,00"],
            vec!["Datum", "(45.10)"],
            vec!["Date", "2026-09-29"],
            vec!["Datum", "29/09/2026"],
            vec!["Datum", "29. 9. 2026"],
            vec!["Konto", "1234567"],
            vec!["Konto", "CZ12 3456 7890"],
            vec!["Datum", "123456 pole"],
            vec!["\u{feff}29.09.2026", "Spotify"],
        ] {
            assert!(header_looks_like_data(&headers(&cells)), "{cells:?}");
        }
        for cells in [
            vec!["Datum", "Částka"],
            vec!["", "Datum"],
            vec!["Booking date", "Amount (EUR)", "Currency"],
            vec!["Číslo účtu", "Zůstatek po transakci", "Typ transakce"],
            vec!["IBAN", "BIC", "Reference"],
        ] {
            assert!(!header_looks_like_data(&headers(&cells)), "{cells:?}");
        }
    }

    #[test]
    fn the_headers_of_every_shipped_layout_are_no_data() {
        let mut seen = 0;
        for entry in std::fs::read_dir(FIXTURE_DIR).expect("fixture dir") {
            let name = entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .to_string();
            if !name.ends_with(".csv") || name.starts_with("stocks-") {
                continue;
            }
            let (cells, _, _, _) = read_layout(&fixture_bytes(&name));
            assert!(
                !header_looks_like_data(&cells),
                "the header of {name} would never be learned: {cells:?}"
            );
            seen += 1;
        }
        assert!(seen >= 13, "{seen} bank fixtures read");
        for preset in crate::services::csv_presets::all_presets() {
            assert!(
                !header_looks_like_data(&preset.match_headers),
                "the headers of preset {} would never be learned",
                preset.id
            );
        }
    }

    #[test]
    fn saving_a_layout_again_replaces_it_and_keeps_its_id_and_creation() {
        let conn = db();
        let mut first_input = input("Old name", &["Datum", "Částka", "Poznámka"]);
        first_input.account_id = Some("account-1".into());
        let first = save_format(&conn, &first_input).unwrap();
        save_format(&conn, &input_n(0)).unwrap();
        age(&conn, 1_000);
        let aged = list_formats(&conn).unwrap();
        let aged_first = aged.iter().find(|f| f.id == first.id).unwrap().clone();
        assert!(aged_first.created_at < first.created_at - 900);

        let mut again = input("  New name ", &["DATUM ", "částka", "Poznámka"]);
        again.encoding = "windows-1250".into();
        again.header_row = 3;
        again.account_id = Some("account-2".into());
        again.mapping.decimal_separator = ".".into();
        let replaced = save_format(&conn, &again).unwrap();

        assert_eq!(replaced.id, first.id, "earlier imports keep their format");
        assert_eq!(
            replaced.created_at, aged_first.created_at,
            "creation is kept"
        );
        assert!(replaced.last_used_at > aged_first.last_used_at + 900, "now");
        assert_eq!(replaced.name, "New name");
        assert_eq!(replaced.encoding, "windows-1250");
        assert_eq!(replaced.header_row, 3);
        assert_eq!(replaced.account_id.as_deref(), Some("account-2"));
        assert_eq!(replaced.mapping.decimal_separator, ".");
        assert_eq!(replaced.headers, again.headers, "the latest spelling");

        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 2, "replaced, not added");
        assert_eq!(listed[0], replaced, "and now the newest");
    }

    #[test]
    fn a_saved_format_carries_the_wire_values() {
        let conn = db();
        let mut wanted = input("  Moje banka  ", &["Datum", "Částka", "Poznámka"]);
        wanted.account_id = Some("account-1".into());
        let saved = save_format(&conn, &wanted).unwrap();

        assert!(saved.id.starts_with("learned:"), "{}", saved.id);
        assert!(uuid::Uuid::parse_str(&saved.id["learned:".len()..]).is_ok());
        assert_eq!(saved.name, "Moje banka");
        assert_eq!(saved.signature, layout_signature(&wanted.headers, ";"));
        assert_eq!(saved.headers, wanted.headers);
        assert_eq!(saved.delimiter, ";");
        assert_eq!(saved.encoding, "UTF-8");
        assert_eq!(saved.header_row, 0);
        assert_eq!(saved.mapping, wanted.mapping);
        assert_eq!(saved.account_id.as_deref(), Some("account-1"));
        assert!(saved.created_at > 1_700_000_000);
        assert_eq!(saved.last_used_at, saved.created_at);
        assert_eq!(saved.shadowed_by_preset_id, None);
        assert_eq!(list_formats(&conn).unwrap(), vec![saved]);
    }

    #[test]
    fn what_is_not_a_choice_is_kept_in_one_form() {
        let conn = db();
        let mut wanted = input("a", &["Datum", "Částka", "Poznámka"]);
        wanted.mapping.date_format = "  ".into();
        wanted.mapping.decimal_separator = " , ".into();
        wanted.mapping.balance_column = Some(" ".into());
        wanted.mapping.description_columns = vec!["Poznámka".into(), "".into()];
        let saved = save_format(&conn, &wanted).unwrap();
        assert_eq!(saved.mapping.date_format, "auto");
        assert_eq!(saved.mapping.decimal_separator, ",");
        assert_eq!(saved.mapping.balance_column, None);
        assert_eq!(saved.mapping.description_columns, vec!["Poznámka"]);

        wanted.mapping.date_format = "AUTO".into();
        wanted.mapping.decimal_separator = "Auto".into();
        let again = save_format(&conn, &wanted).unwrap();
        assert_eq!(again.mapping.date_format, "auto");
        assert_eq!(again.mapping.decimal_separator, "auto");

        wanted.mapping.decimal_separator = ";".into();
        assert_eq!(
            validation_key(save_format(&conn, &wanted)),
            "validation.csvDecimalSeparatorInvalid"
        );
    }

    #[test]
    fn formats_are_listed_newest_used_first() {
        let conn = db();
        for (i, name) in ["first", "second", "third"].iter().enumerate() {
            let named = InsertLearnedCsvFormat {
                name: name.to_string(),
                ..input_n(i as u8)
            };
            save_format(&conn, &named).unwrap();
        }
        let names: Vec<String> = list_formats(&conn)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(
            names,
            vec!["third", "second", "first"],
            "same second: saved last, first"
        );

        // Whatever order the array was written in, the time of the last use decides.
        let mut stored = list_formats(&conn).unwrap();
        stored[0].last_used_at = 100;
        stored[1].last_used_at = 300;
        stored[2].last_used_at = 200;
        store(&conn, &stored);
        let names: Vec<String> = list_formats(&conn)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, vec!["second", "first", "third"]);
    }

    #[test]
    fn at_most_twenty_formats_are_kept_and_the_least_used_goes() {
        let conn = db();
        let mut saved = Vec::new();
        for n in 0..20 {
            saved.push(save_format(&conn, &input_n(n)).unwrap());
        }
        assert_eq!(list_formats(&conn).unwrap().len(), 20);
        // The first one is the oldest; using it makes the second one the least recently used.
        mark_used(&conn, &saved[0].id).unwrap();
        save_format(&conn, &input_n(20)).unwrap();

        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 20);
        assert!(
            listed.iter().any(|f| f.id == saved[0].id),
            "the used one stays"
        );
        assert!(
            listed.iter().all(|f| f.id != saved[1].id),
            "the least recently used one goes"
        );
        assert_eq!(listed[0].name, "Banka 20");
        assert_eq!(listed[1].id, saved[0].id);

        // By the time of the last use when the clock says so, not by the age of the entry.
        let mut stored = list_formats(&conn).unwrap();
        stored[5].last_used_at = 1;
        let doomed = stored[5].id.clone();
        store(&conn, &stored);
        save_format(&conn, &input_n(21)).unwrap();
        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 20);
        assert!(listed.iter().all(|f| f.id != doomed));
    }

    #[test]
    fn mark_used_moves_a_format_to_the_top() {
        let conn = db();
        let a = save_format(&conn, &input_n(0)).unwrap();
        let b = save_format(&conn, &input_n(1)).unwrap();
        let c = save_format(&conn, &input_n(2)).unwrap();
        age(&conn, 1_000);

        mark_used(&conn, &a.id).unwrap();

        let listed = list_formats(&conn).unwrap();
        let order: Vec<&str> = listed.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(order, vec![a.id.as_str(), c.id.as_str(), b.id.as_str()]);
        assert!(listed[0].last_used_at > listed[1].last_used_at + 900);
        assert_eq!(
            listed[0].created_at,
            a.created_at - 1_000,
            "creation is kept"
        );
        assert_eq!(listed[0].name, a.name);

        let err = mark_used(&conn, "learned:nope").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert_eq!(list_formats(&conn).unwrap().len(), 3);
    }

    #[test]
    fn rename_trims_and_validates() {
        let conn = db();
        let a = save_format(&conn, &input_n(0)).unwrap();
        let b = save_format(&conn, &input_n(1)).unwrap();

        let renamed = rename_format(&conn, &a.id, "  Moje banka  ").unwrap();
        assert_eq!(renamed.name, "Moje banka");
        assert_eq!(renamed.id, a.id);
        assert_eq!(renamed.last_used_at, a.last_used_at, "renaming is no use");
        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed[0].id, b.id, "and does not move it");
        assert_eq!(listed[1].name, "Moje banka");

        assert_eq!(
            validation_key(rename_format(&conn, &a.id, "   ")),
            "validation.nameRequired"
        );
        assert_eq!(
            validation_key(rename_format(&conn, &a.id, &"x".repeat(61))),
            "validation.nameTooLong"
        );
        assert!(rename_format(&conn, &a.id, &"x".repeat(60)).is_ok());
        assert_eq!(list_formats(&conn).unwrap()[1].name, "x".repeat(60));

        let err = rename_format(&conn, "learned:nope", "Name").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn deleting_removes_only_that_one() {
        let conn = db();
        let a = save_format(&conn, &input_n(0)).unwrap();
        let b = save_format(&conn, &input_n(1)).unwrap();

        delete_format(&conn, &a.id).unwrap();
        assert_eq!(list_formats(&conn).unwrap(), vec![b.clone()]);

        let err = delete_format(&conn, &a.id).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert_eq!(list_formats(&conn).unwrap().len(), 1);

        delete_format(&conn, &b.id).unwrap();
        assert!(list_formats(&conn).unwrap().is_empty());
    }

    #[test]
    fn a_format_without_a_usable_name_layout_or_delimiter_is_refused() {
        let conn = db();
        const THREE: [&str; 3] = ["Datum", "Částka", "Poznámka"];
        assert_eq!(
            validation_key(save_format(&conn, &input("  ", &THREE))),
            "validation.nameRequired"
        );
        assert_eq!(
            validation_key(save_format(&conn, &input(&"x".repeat(61), &THREE))),
            "validation.nameTooLong"
        );
        assert!(save_format(&conn, &input(&"x".repeat(60), &THREE)).is_ok());
        for cells in [
            vec![],
            vec![""],
            vec!["Datum"],
            vec!["Datum", " ", ""],
            vec!["", "  "],
        ] {
            assert_eq!(
                validation_key(save_format(&conn, &input("ok", &cells))),
                "validation.csvEmptyFile",
                "{cells:?}"
            );
        }
        for delimiter in ["", ";;", "ab", "é"] {
            let mut odd = input("ok", &THREE);
            odd.delimiter = delimiter.into();
            assert_eq!(
                validation_key(save_format(&conn, &odd)),
                "validation.csvDelimiterInvalid",
                "{delimiter:?}"
            );
        }
        for delimiter in [",", ";", "\t", "|"] {
            let mut fine = input("ok", &THREE);
            fine.delimiter = delimiter.into();
            assert!(fine.validate().is_ok(), "{delimiter:?}");
        }
        assert_eq!(
            list_formats(&conn).unwrap().len(),
            1,
            "only the 60-character name"
        );
    }

    #[test]
    fn a_mapping_naming_a_missing_column_is_refused() {
        let conn = db();
        let cells = ["Datum", "Částka", "Poznámka", "Měna"];
        type Break = fn(&mut LearnedCsvMapping);
        let broken: [(&str, Break); 11] = [
            ("date", |m| m.date_column = "Valuta".into()),
            ("amount", |m| m.amount_column = Some("Objem".into())),
            ("debit", |m| m.debit_column = Some("Výdaj".into())),
            ("credit", |m| m.credit_column = Some("Příjem".into())),
            ("balance", |m| m.balance_column = Some("Zůstatek".into())),
            ("id", |m| m.transaction_id_column = Some("ID".into())),
            ("description", |m| {
                m.description_columns.push("Detail".into())
            }),
            ("counterparty", |m| {
                m.counterparty_column = Some("Protistrana".into())
            }),
            ("iban", |m| {
                m.counterparty_iban_column = Some("Protiúčet".into())
            }),
            ("bank code", |m| {
                m.counterparty_bank_code_column = Some("Kód".into())
            }),
            ("currency", |m| m.currency_column = Some("Mena2".into())),
        ];
        for (what, apply) in broken {
            let mut format = input("ok", &cells);
            apply(&mut format.mapping);
            assert_eq!(
                validation_key(save_format(&conn, &format)),
                "validation.learnedFormatColumns",
                "{what}"
            );
        }
        let mut country = input("ok", &cells);
        country.mapping.counterparty_country_column = Some("Země".into());
        assert_eq!(
            validation_key(save_format(&conn, &country)),
            "validation.learnedFormatColumns"
        );
        assert!(stored_json(&conn).is_none());

        // The file's own spelling is not required: case and diacritics resolve as in an import.
        let mut spelled = input("ok", &cells);
        spelled.mapping.date_column = "DATUM".into();
        spelled.mapping.amount_column = Some("Castka".into());
        spelled.mapping.currency_column = Some("měna".into());
        assert!(save_format(&conn, &spelled).is_ok());
    }

    #[test]
    fn a_mapping_without_date_or_amount_is_refused() {
        let cells = ["Datum", "Výdaj", "Příjem", "Poznámka"];
        let mut format = input("ok", &cells);
        format.mapping.date_column = "  ".into();
        assert_eq!(
            validation_key(format.validate()),
            "validation.dateColumnRequired"
        );

        let mut format = input("ok", &cells);
        format.mapping.amount_column = None;
        assert_eq!(
            validation_key(format.validate()),
            "validation.amountColumnRequired"
        );
        format.mapping.amount_column = Some("  ".into());
        assert_eq!(
            validation_key(format.validate()),
            "validation.amountColumnRequired"
        );
        // Half a pair is not enough; the whole pair is.
        format.mapping.amount_column = None;
        format.mapping.debit_column = Some("Výdaj".into());
        assert_eq!(
            validation_key(format.validate()),
            "validation.amountColumnRequired"
        );
        format.mapping.credit_column = Some("Příjem".into());
        assert!(format.validate().is_ok());
    }

    #[test]
    fn a_preset_layout_is_reported_as_shadowed() {
        let conn = db();
        let fio = save_format(
            &conn,
            &InsertLearnedCsvFormat {
                name: "Fio".into(),
                headers: headers(&["ID operace", "Datum", "Objem", "Měna"]),
                mapping: LearnedCsvMapping {
                    date_column: "Datum".into(),
                    amount_column: Some("Objem".into()),
                    description_columns: Vec::new(),
                    ..mapping()
                },
                ..input("", &[])
            },
        )
        .unwrap();
        assert_eq!(fio.shadowed_by_preset_id.as_deref(), Some("fio"));

        let own = save_format(&conn, &unicredit_like_input()).unwrap();
        assert_eq!(own.shadowed_by_preset_id, None);

        let listed = list_formats(&conn).unwrap();
        let shadowed: Vec<(&str, Option<&str>)> = listed
            .iter()
            .map(|f| (f.name.as_str(), f.shadowed_by_preset_id.as_deref()))
            .collect();
        assert_eq!(shadowed, vec![("UniCredit", None), ("Fio", Some("fio"))]);
        let found = find_format(&conn, &fio.headers, ";").unwrap().unwrap();
        assert_eq!(found.shadowed_by_preset_id.as_deref(), Some("fio"));
        assert_eq!(
            rename_format(&conn, &fio.id, "Moje Fio")
                .unwrap()
                .shadowed_by_preset_id
                .as_deref(),
            Some("fio")
        );
    }

    #[test]
    fn unreadable_stored_formats_never_break_the_list() {
        let conn = db();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, 'not json')",
            [KEY],
        )
        .unwrap();
        assert!(list_formats(&conn).unwrap().is_empty());
        assert!(find_format(&conn, &headers(&["Datum", "Částka"]), ";")
            .unwrap()
            .is_none());
        // A value that is JSON but not a list reads the same way.
        conn.execute(
            "UPDATE app_config SET value = '{\"id\":\"learned:x\"}' WHERE key = ?1",
            [KEY],
        )
        .unwrap();
        assert!(list_formats(&conn).unwrap().is_empty());
        // The next save recovers the key.
        save_format(&conn, &input_n(0)).unwrap();
        assert_eq!(list_formats(&conn).unwrap().len(), 1);

        // One entry that no longer reads (an older shape) is skipped, the rest stay.
        let good = serde_json::to_value(list_formats(&conn).unwrap()).unwrap();
        let mut entries = good.as_array().unwrap().clone();
        entries.insert(0, serde_json::json!({ "id": "learned:old", "name": 5 }));
        conn.execute(
            "UPDATE app_config SET value = ?1 WHERE key = ?2",
            rusqlite::params![serde_json::Value::Array(entries).to_string(), KEY],
        )
        .unwrap();
        let listed = list_formats(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Banka 0");
        // Saving repairs the key: the damaged entry is gone for good.
        save_format(&conn, &input_n(1)).unwrap();
        let json = stored_json(&conn).unwrap();
        assert!(!json.contains("learned:old"), "{json}");
        assert_eq!(list_formats(&conn).unwrap().len(), 2);
    }

    #[test]
    fn formats_are_stored_as_the_wire_json_in_app_config() {
        let conn = db();
        save_format(
            &conn,
            &InsertLearnedCsvFormat {
                headers: headers(&["ID operace", "Datum", "Objem", "Měna"]),
                mapping: LearnedCsvMapping {
                    amount_column: Some("Objem".into()),
                    description_columns: Vec::new(),
                    ..mapping()
                },
                ..input("Moje banka", &[])
            },
        )
        .unwrap();
        let json = stored_json(&conn).expect("stored under the key");
        assert!(json.starts_with('['), "{json}");
        for field in [
            "\"headers\"",
            "\"lastUsedAt\"",
            "\"createdAt\"",
            "\"dateColumn\"",
        ] {
            assert!(json.contains(field), "{field} in {json}");
        }
        assert!(
            json.contains("\"shadowedByPresetId\":null"),
            "the preset is looked up on every read, never stored: {json}"
        );
        assert!(!json.contains("\"fio\""), "{json}");
    }
}
