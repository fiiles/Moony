//! Looking at a file before anything is imported: encoding, delimiter, header
//! row, bank preset, suggested column mapping, date format and decimal
//! separator. Backs the `parse_csv_file` command.

use std::collections::HashMap;

use crate::error::{AppError, Result};
use crate::services::csv_presets::{self, CsvPreset, ResolvedPreset};
use crate::services::date_parser;

use super::amounts::detect_decimal_separator;
use super::columns::{find_column, suggest_column_mappings};
use super::decode::{
    decode_csv_content, detect_csv_delimiter, detect_header_row, read_headers, DecodedCsv,
};
use super::rows::{data_records, has_duplicates, SAMPLE_ROWS};
use super::types::CsvPreviewResult;

/// What the caller already decided; `None` fields are detected.
#[derive(Debug, Clone, Default)]
pub struct InspectOptions<'a> {
    pub delimiter: Option<char>,
    /// 0-based line of the header row.
    pub header_row: Option<usize>,
    /// Data rows to skip after the header.
    pub skip_rows: usize,
    /// Apply this preset instead of auto-detecting one (the user picked a bank).
    pub preset_id: Option<&'a str>,
    /// Force a text encoding.
    pub encoding: Option<&'a str>,
}

/// Decode a file for import or inspection.
///
/// Valid UTF-8 (or a BOM) decides on its own. For legacy encodings the header
/// row of a first decode is matched against the presets, and when the matching
/// preset names a different encoding the file is decoded again with it — a
/// Windows-1252 export of a German bank must not be read as Windows-1250 just
/// because that comes first in the fallback order. Deterministic for a given
/// file, so the inspection and the later import agree.
pub fn decode_for_import(
    bytes: &[u8],
    encoding: Option<&str>,
    forced_preset: Option<&CsvPreset>,
) -> DecodedCsv {
    let hint = encoding.or(forced_preset.map(|p| p.encoding.as_str()));
    let first = decode_csv_content(bytes, hint);
    if encoding.is_some() || first.encoding == "UTF-8" || first.encoding.starts_with("UTF-16") {
        return first;
    }

    let delimiter = detect_csv_delimiter(&first.text);
    let header_row = detect_header_row(&first.text, delimiter);
    let headers = read_headers(&first.text, delimiter, header_row);
    let preset = forced_preset.or_else(|| csv_presets::detect_preset(&headers));
    let preferred = preset
        .and_then(|p| encoding_rs::Encoding::for_label(p.encoding.as_bytes()))
        .filter(|enc| *enc != encoding_rs::UTF_8 && enc.name() != first.encoding);
    match preferred {
        Some(enc) => decode_csv_content(bytes, Some(enc.name())),
        None => first,
    }
}

/// The preset to use: the explicitly chosen one, else detected from headers.
fn pick_preset(headers: &[String], preset_id: Option<&str>) -> Option<&'static CsvPreset> {
    preset_id
        .and_then(csv_presets::get_preset)
        .or_else(|| csv_presets::detect_preset(headers))
}

/// Overlay a resolved preset on the generic suggestions: the preset's columns
/// at confidence 1.0, generic suggestions for the roles it does not define, and
/// no generic suggestion that would reuse a column the preset claimed or
/// contradict its amount style.
fn overlay_preset(mappings: &mut HashMap<String, (String, f32)>, resolved: &ResolvedPreset) {
    let mut preset_roles: Vec<(&str, String)> = vec![("date", resolved.date.clone())];
    let optional = [
        ("amount", &resolved.amount),
        ("debit", &resolved.debit),
        ("credit", &resolved.credit),
        ("balance", &resolved.balance),
        ("transactionId", &resolved.transaction_id),
        ("currency", &resolved.currency),
        ("counterparty", &resolved.counterparty),
        ("counterparty_iban", &resolved.counterparty_iban),
    ];
    for (role, value) in optional {
        if let Some(header) = value {
            preset_roles.push((role, header.clone()));
        }
    }
    if let Some(first) = resolved.description.first() {
        preset_roles.push(("description", first.clone()));
    }

    let claimed: Vec<&String> = preset_roles.iter().map(|(_, h)| h).collect();
    let defined: Vec<&str> = preset_roles.iter().map(|(r, _)| *r).collect();
    mappings.retain(|role, (header, _)| {
        !defined.contains(&role.as_str()) && !claimed.contains(&&*header)
    });
    // A preset with a single amount column rules out a generic debit/credit
    // guess and vice versa.
    if resolved.amount.is_some() {
        mappings.remove("debit");
        mappings.remove("credit");
    } else {
        mappings.remove("amount");
    }
    for (role, header) in preset_roles {
        mappings.insert(role.to_string(), (header, 1.0));
    }
}

/// The sampled cells of the column suggested for `role`.
fn cells_of<'a>(
    mappings: &HashMap<String, (String, f32)>,
    headers: &[String],
    scanned: &'a [csv::StringRecord],
    role: &str,
) -> Vec<&'a str> {
    mappings
        .get(role)
        .and_then(|(header, _)| find_column(headers, header))
        .map(|idx| scanned.iter().filter_map(|r| r.get(idx)).collect())
        .unwrap_or_default()
}

/// Inspect a file. The only error cases are an unreadable structure
/// (`validation.csvEmptyFile`) — anything else yields a best-effort result the
/// user can correct.
pub fn inspect_csv(bytes: &[u8], options: &InspectOptions<'_>) -> Result<CsvPreviewResult> {
    let forced_preset = options.preset_id.and_then(csv_presets::get_preset);
    let decoded = decode_for_import(bytes, options.encoding, forced_preset);
    let content = &decoded.text;

    let delimiter = options
        .delimiter
        .unwrap_or_else(|| detect_csv_delimiter(content));
    let header_row = options
        .header_row
        .unwrap_or_else(|| detect_header_row(content, delimiter));
    let headers = read_headers(content, delimiter, header_row);
    if headers.iter().all(|h| h.is_empty()) {
        return Err(AppError::Validation("validation.csvEmptyFile".into()));
    }

    // Walk the data once: count rows, keep five for display and the first
    // SAMPLE_ROWS for detection.
    let mut total_rows = 0usize;
    let mut sample_rows: Vec<Vec<String>> = Vec::new();
    let mut scanned: Vec<csv::StringRecord> = Vec::new();
    for (_, result) in data_records(content, delimiter, header_row, options.skip_rows) {
        let Ok(record) = result else { continue };
        if record.iter().all(|f| f.trim().is_empty()) {
            continue;
        }
        total_rows += 1;
        if sample_rows.len() < 5 {
            sample_rows.push(record.iter().map(|s| s.to_string()).collect());
        }
        if scanned.len() < SAMPLE_ROWS {
            scanned.push(record);
        }
    }

    let mut mappings = suggest_column_mappings(&headers);
    let preset = pick_preset(&headers, options.preset_id);
    let resolved = preset.and_then(|p| csv_presets::resolve_preset(p, &headers));
    let applied_preset = resolved.as_ref().and(preset);
    if let Some(resolved) = &resolved {
        overlay_preset(&mut mappings, resolved);
    }

    // A transaction-id column that repeats values is not an id (a row counter
    // would make every re-import look like a duplicate). Only generic guesses
    // are dropped; a preset's explicit choice stays.
    let from_preset = resolved
        .as_ref()
        .is_some_and(|r| r.transaction_id.is_some());
    if !from_preset && has_duplicates(cells_of(&mappings, &headers, &scanned, "transactionId")) {
        mappings.remove("transactionId");
    }

    // Date format: the preset's when every sampled date reads with it, else the
    // consensus of the samples.
    let date_samples: Vec<&str> = cells_of(&mappings, &headers, &scanned, "date")
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let preset_format = applied_preset.map(|p| p.date_format.as_str()).filter(|f| {
        !date_samples.is_empty()
            && date_samples
                .iter()
                .all(|s| date_parser::parse_date_strict(s, f).is_some())
    });
    let (date_format, date_format_ambiguous) = match preset_format {
        Some(format) => (date_parser::date_only_format(format), false),
        None => {
            let detected = date_parser::detect_date_format(&date_samples);
            (detected.format, detected.ambiguous)
        }
    };

    // Decimal separator from every amount-like column.
    let amount_cells: Vec<&str> = ["amount", "debit", "credit", "balance"]
        .into_iter()
        .flat_map(|role| cells_of(&mappings, &headers, &scanned, role))
        .collect();
    let decimal_separator = detect_decimal_separator(&amount_cells);

    let suggested_description_columns = match resolved.as_ref() {
        Some(r) if !r.description.is_empty() => r.description.clone(),
        _ => mappings
            .get("description")
            .map(|(header, _)| vec![header.clone()])
            .unwrap_or_default(),
    };

    Ok(CsvPreviewResult {
        headers,
        sample_rows,
        total_rows,
        delimiter: delimiter.to_string(),
        suggested_mappings: mappings,
        header_row,
        detected_preset_id: applied_preset.map(|p| p.id.clone()),
        date_format,
        date_format_ambiguous,
        decimal_separator: decimal_separator.to_string(),
        encoding: decoded.encoding.to_string(),
        suggested_description_columns,
        suggested_row_filter: resolved.and_then(|r| r.row_filter),
    })
}
