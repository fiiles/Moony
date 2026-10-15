//! Bank statement import (CAMT.053, MT940, OFX/QFX, QIF): every format parses to
//! the neutral [`Statement`] model, which [`to_rows::to_rows`] turns into the
//! [`ParsedImport`](crate::services::csv_import::ParsedImport) the shared CSV
//! import core already consumes (preview, duplicate check, write path).
//!
//! This module is the service the commands call: [`inspect`] summarizes a file,
//! [`preview`] and [`import`] run one of its statements through the shared core.

pub mod account_match;
pub mod camt;
pub mod decimal;
pub(crate) mod decode;
pub(crate) mod detect;
pub mod merge;
pub mod model;
pub mod mt940;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
pub(crate) mod test_support;
pub mod to_rows;
pub mod types;
pub mod xml_tree;

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::json;

use crate::error::{AppError, Result};
use crate::services::categorization::CategorizationEngine;
use crate::services::csv_import::{self, CsvDateRange, CsvImportPreview, CsvImportResult};

pub use crate::services::csv_import::{DeclaredBalance, SourceCategoryMapping};
pub use account_match::{match_account, AccountMatch};
pub use detect::{check_size, detect, reject_doctype, DETECT_BYTES, MAX_FILE_BYTES};
pub use merge::{merge, BalanceCheck, MergedStatement};
pub use model::{join_description, utc_day, AccountRef, Entry, Statement, StatementFormat};
pub use types::{
    DateOrder, SourceCategoryCount, StatementImportOptions, StatementInspection, StatementSummary,
    StatementWarning,
};

/// A statement file read and merged: what [`inspect`], [`preview`] and [`import`] start from.
#[derive(Debug)]
pub struct Loaded {
    pub format: StatementFormat,
    pub encoding: &'static str,
    pub statements: Vec<MergedStatement>,
    pub warnings: Vec<StatementWarning>,
    pub date_order_ambiguous: bool,
    pub detected_date_order: Option<DateOrder>,
}

/// What one format's parser reports about a file.
#[derive(Default)]
struct Parsed {
    statements: Vec<Statement>,
    warnings: Vec<StatementWarning>,
    date_order_ambiguous: bool,
    detected_date_order: Option<DateOrder>,
}

/// Detects, decodes, parses and merges a statement file. `Ok(None)` = the bytes are not a
/// statement (the CSV path takes them). `country` (the profile's) picks the legacy 8-bit
/// encoding of the text formats. Only QIF reads `date_order`.
pub fn load(
    bytes: &[u8],
    country: Option<&str>,
    _date_order: Option<DateOrder>,
) -> Result<Option<Loaded>> {
    let Some(format) = detect(&bytes[..bytes.len().min(DETECT_BYTES)])? else {
        return Ok(None);
    };
    check_size(bytes.len() as u64)?;
    let decoded = decode::decode(bytes, format, country);
    let parsed = match format {
        StatementFormat::Camt053 => Parsed {
            statements: camt::parse(&decoded.text)?,
            ..Parsed::default()
        },
        StatementFormat::Mt940 => Parsed {
            statements: mt940::parse(&decoded.text)?,
            ..Parsed::default()
        },
        StatementFormat::Ofx | StatementFormat::Qif => {
            return Err(AppError::validation_with(
                "validation.statementFormatNotYetSupported",
                json!({ "format": format.label() }),
            ));
        }
    };
    if parsed.statements.is_empty() {
        return Err(AppError::Validation(
            "validation.statementNoStatements".into(),
        ));
    }
    Ok(Some(Loaded {
        format,
        encoding: decoded.encoding,
        statements: merge(parsed.statements),
        warnings: parsed.warnings,
        date_order_ambiguous: parsed.date_order_ambiguous,
        detected_date_order: parsed.detected_date_order,
    }))
}

/// Summarizes a statement file for the import dialog; `Ok(None)` = not a statement. Reads
/// the database only to match each statement's account against the accounts.
pub fn inspect(
    conn: &Connection,
    account_id: &str,
    bytes: &[u8],
    country: Option<&str>,
) -> Result<Option<StatementInspection>> {
    let Some(loaded) = load(bytes, country, None)? else {
        return Ok(None);
    };
    let statements = loaded
        .statements
        .iter()
        .enumerate()
        .map(|(index, merged)| summarize(conn, account_id, index, merged))
        .collect::<Result<Vec<_>>>()?;
    let default_statement_index = statements
        .iter()
        .position(|summary| summary.account_match == AccountMatch::Selected)
        .unwrap_or(0);
    Ok(Some(StatementInspection {
        format: loaded.format,
        encoding: loaded.encoding.to_string(),
        statements,
        default_statement_index,
        date_order_ambiguous: loaded.date_order_ambiguous,
        detected_date_order: loaded.detected_date_order,
        warnings: loaded.warnings,
    }))
}

fn summarize(
    conn: &Connection,
    account_id: &str,
    index: usize,
    merged: &MergedStatement,
) -> Result<StatementSummary> {
    let statement = &merged.statement;
    let (account_match, matched_account_id) = match_account(conn, account_id, &statement.account)?;
    let skipped_count = statement
        .entries
        .iter()
        .filter(|entry| entry.skip_reason.is_some())
        .count();
    Ok(StatementSummary {
        index,
        account_identifier: statement.account.identifier.clone(),
        account_name: statement.account.name.clone(),
        currency: statement.currency.clone(),
        period: statement.period.map(|(from, to)| CsvDateRange { from, to }),
        opening_balance: statement.opening.clone(),
        closing_balance: statement.closing.clone(),
        entry_count: statement.entries.len() - skipped_count,
        skipped_count,
        balance_check: merged.balance_check,
        balance_difference: merged.balance_difference.clone(),
        account_match,
        matched_account_id,
        source_categories: source_categories(statement),
    })
}

/// The source categories of the entries that would be imported, most frequent first, then
/// by name.
fn source_categories(statement: &Statement) -> Vec<SourceCategoryCount> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for entry in statement
        .entries
        .iter()
        .filter(|entry| entry.skip_reason.is_none())
    {
        if let Some(name) = entry
            .source_category
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut categories: Vec<SourceCategoryCount> = counts
        .into_iter()
        .map(|(name, count)| SourceCategoryCount {
            name: name.to_string(),
            count,
        })
        .collect();
    categories.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    categories
}

/// The rows of the statement the options select, after the options are validated against
/// the file.
fn selected_rows(
    account_id: &str,
    bytes: &[u8],
    country: Option<&str>,
    options: &StatementImportOptions,
) -> Result<csv_import::ParsedImport> {
    let loaded = load(bytes, country, options.date_order)?
        .ok_or_else(|| AppError::Validation("validation.statementNotRecognized".into()))?;
    options.validate(loaded.format, loaded.statements.len())?;
    Ok(to_rows::to_rows(
        &loaded.statements[options.statement_index].statement,
        account_id,
    ))
}

/// Dry run of importing one statement of the file (see [`csv_import::preview_rows`]).
pub fn preview(
    conn: &Connection,
    account_id: &str,
    bytes: &[u8],
    country: Option<&str>,
    options: &StatementImportOptions,
    engine: Option<&CategorizationEngine>,
) -> Result<CsvImportPreview> {
    let parsed = selected_rows(account_id, bytes, country, options)?;
    csv_import::preview_rows(conn, account_id, &parsed, &options.import_options(), engine)
}

/// Imports one statement of the file in one SQL transaction (see [`csv_import::import_rows`]).
pub fn import(
    conn: &mut Connection,
    account_id: &str,
    file_name: &str,
    bytes: &[u8],
    country: Option<&str>,
    options: &StatementImportOptions,
    engine: Option<&CategorizationEngine>,
) -> Result<CsvImportResult> {
    let parsed = selected_rows(account_id, bytes, country, options)?;
    csv_import::import_rows(
        conn,
        account_id,
        file_name,
        &parsed,
        &options.import_options(),
        engine,
    )
}
