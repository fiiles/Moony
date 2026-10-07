//! CSV import service for bank transactions.
//!
//! Layers, bottom to top:
//! - [`decode`] — text encoding, delimiter, header row;
//! - [`columns`] — header normalization and role patterns (EN/CZ/DE/FR/ES/IT/NL/PL);
//! - [`amounts`] — decimal separator consensus and the amount cell parser;
//! - [`rows`] — phase 1: typed rows, no database (`parse_rows`);
//! - [`inspect`] — what `parse_csv_file` reports about a file;
//! - this module — duplicate classification, [`preview_import`] (no writes) and
//!   [`import_transactions`] (one SQL transaction through the shared write path,
//!   ADR 0007).
//!
//! Bank-specific formats live as data in `services/csv_presets.rs`.

pub mod amounts;
pub mod columns;
pub mod decode;
pub mod inspect;
pub mod rows;
pub mod types;

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::InsertBankTransaction;
use crate::services::categorization::{packs, CategorizationEngine};
use crate::services::dedup;

pub use amounts::{clean_and_parse_amount, detect_decimal_separator, AmountError};
pub use columns::{normalize_header, suggest_column_mappings};
pub use decode::{decode_csv_content, detect_csv_delimiter, detect_header_row};
pub use inspect::{decode_for_import, inspect_csv, InspectOptions};
pub use rows::{parse_rows, ParsedCsv, ParsedRow, RowState};
pub use types::{
    CsvCategoryOverride, CsvDateRange, CsvImportConfig, CsvImportPreview, CsvImportResult,
    CsvPreviewResult, CsvPreviewRow, CsvRowMessage, CsvRowStatus,
};

/// Rows listed by a preview; counts always cover the whole file.
pub const PREVIEW_ROWS: usize = 200;

/// Why a row counts as a duplicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DuplicateKind {
    /// Same bank-side transaction id as an existing/earlier transaction.
    BankId,
    /// Same account, date, amount, type and description.
    Composite,
}

impl DuplicateKind {
    fn key(self) -> &'static str {
        match self {
            DuplicateKind::BankId => "csvImport.duplicateById",
            DuplicateKind::Composite => "csvImport.duplicate",
        }
    }
}

fn duplicate_message(line: usize, kind: DuplicateKind) -> CsvRowMessage {
    CsvRowMessage {
        line,
        key: kind.key().to_string(),
        detail: None,
    }
}

fn ensure_account_exists(conn: &Connection, account_id: &str) -> Result<()> {
    // Check up front — otherwise the currency fallback inside the write path
    // would surface a confusing per-row error.
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM bank_accounts WHERE id = ?1",
            [account_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Bank account not found: {}",
            account_id
        )));
    }
    Ok(())
}

fn extend_range(range: &mut Option<CsvDateRange>, date: i64) {
    match range {
        Some(r) => {
            r.from = r.from.min(date);
            r.to = r.to.max(date);
        }
        None => {
            *range = Some(CsvDateRange {
                from: date,
                to: date,
            })
        }
    }
}

/// Dry run: parse, validate and duplicate-check the file exactly as the import
/// would, without writing anything.
///
/// Statuses: `ok` (will be imported), `error`, `duplicate` (skipped unless its
/// line is in `importAnywayLines`, or `skipDuplicates` is off, for possible
/// composites only), `promoted` (same-ID pending updated), `skipped` (zero
/// amount or row filter). Lists the requested bounded page; the counts
/// cover the whole file.
pub fn preview_import(
    conn: &Connection,
    account_id: &str,
    content: &str,
    config: &CsvImportConfig,
    engine: Option<&CategorizationEngine>,
) -> Result<CsvImportPreview> {
    ensure_account_exists(conn, account_id)?;
    config.validate()?;
    let parsed = parse_rows(content, account_id, config)?;

    let mut checker = dedup::BankOccurrenceChecker::load(conn, account_id)?;
    let mut rows: Vec<CsvPreviewRow> = Vec::new();
    let mut pending_categories: HashMap<String, Option<(String, Option<String>)>> = HashMap::new();
    let (mut ok_count, mut error_count, mut duplicate_count, mut skipped_count) = (0, 0, 0, 0);
    let mut categorized_count = 0usize;
    let mut forced_duplicate_count = 0usize;
    let mut promoted_count = 0usize;
    let mut date_range: Option<CsvDateRange> = None;

    for (row_index, row) in parsed.rows.iter().enumerate() {
        let mut category: Option<(String, Option<String>)> = None;
        let (status, message) = match &row.state {
            RowState::Candidate(insert) => {
                category = resolve_row_category(conn, config, row.line, insert, engine)?
                    .map(|(id, source)| (id, Some(source.to_string())));
                match checker.check(insert) {
                    None => {
                        if insert.status.as_deref() == Some("pending") {
                            if let Some(id) = insert.transaction_id.as_ref() {
                                pending_categories.insert(id.clone(), category.clone());
                            }
                        }
                        checker.record_written(insert, "preview");
                        ok_count += 1;
                        if category.is_some() {
                            categorized_count += 1;
                        }
                        extend_range(&mut date_range, insert.booking_date);
                        (CsvRowStatus::Ok, None)
                    }
                    Some(dedup::BankOccurrence::Promote(id)) => {
                        category = if id == "preview" {
                            insert
                                .transaction_id
                                .as_ref()
                                .and_then(|id| pending_categories.get(id))
                                .cloned()
                                .flatten()
                        } else {
                            let (category_id, source): (Option<String>,Option<String>)=conn.query_row("SELECT category_id,categorization_source FROM bank_transactions WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
                            category_id.map(|id| (id, source))
                        };
                        promoted_count += 1;
                        extend_range(&mut date_range, insert.booking_date);
                        (
                            CsvRowStatus::Promoted,
                            Some(CsvRowMessage {
                                line: row.line,
                                key: "csvImport.pendingPromoted".into(),
                                detail: None,
                            }),
                        )
                    }
                    Some(hit) => {
                        let kind = if hit == dedup::BankOccurrence::Certain {
                            DuplicateKind::BankId
                        } else {
                            DuplicateKind::Composite
                        };
                        duplicate_count += 1;
                        if kind == DuplicateKind::Composite
                            && (!config.skip_duplicates
                                || config.import_anyway_lines.contains(&row.line))
                        {
                            if insert.status.as_deref() == Some("pending") {
                                if let Some(id) = insert.transaction_id.as_ref() {
                                    pending_categories.insert(id.clone(), category.clone());
                                }
                            }
                            checker.record_written(insert, "preview");
                            forced_duplicate_count += 1;
                            if category.is_some() {
                                categorized_count += 1;
                            }
                            extend_range(&mut date_range, insert.booking_date);
                        }
                        (
                            CsvRowStatus::Duplicate,
                            Some(duplicate_message(row.line, kind)),
                        )
                    }
                }
            }
            RowState::Error(m) => {
                error_count += 1;
                (CsvRowStatus::Error, Some(m.clone()))
            }
            RowState::SkippedZero(m) | RowState::SkippedFiltered(m) => {
                skipped_count += 1;
                (CsvRowStatus::Skipped, Some(m.clone()))
            }
        };
        if rows.len() < config.preview_limit && row_index >= config.preview_offset {
            let candidate = match &row.state {
                RowState::Candidate(t) => Some(t.as_ref()),
                _ => None,
            };
            let can_import_anyway = status == CsvRowStatus::Duplicate
                && message
                    .as_ref()
                    .is_some_and(|m| m.key == "csvImport.duplicate");
            rows.push(CsvPreviewRow {
                value_date: candidate.and_then(|t| t.value_date),
                counterparty_iban: candidate.and_then(|t| t.counterparty_iban.clone()),
                counterparty_bank_code: candidate.and_then(|t| t.counterparty_bank_code.clone()),
                counterparty_country: candidate.and_then(|t| t.counterparty_country.clone()),
                transaction_id: candidate.and_then(|t| t.transaction_id.clone()),
                transaction_status: row.transaction_status.clone(),
                row_balance: row.balance.map(|b| b.to_string()),
                can_import_anyway,

                line: row.line,
                booking_date: row.booking_date,
                amount: row.signed_amount.clone(),
                currency: row.currency.clone(),
                description: row.description.clone(),
                counterparty: row.counterparty.clone(),
                status,
                message,
                category_id: category.as_ref().map(|(id, _)| id.clone()),
                category_source: category.and_then(|(_, source)| source),
            });
        }
    }

    Ok(CsvImportPreview {
        rows,
        total_rows: parsed.total_rows,
        ok_count,
        error_count,
        duplicate_count,
        skipped_count,
        date_range,
        categorized_count,
        forced_duplicate_count,
        promoted_count,
        offset: config.preview_offset,
        limit: config.preview_limit,
    })
}

/// `categorization_source` of a category the user picked in the preview.
pub const SOURCE_MANUAL: &str = "manual";

/// The category one row would be written with: the preview override for its
/// line when that category still exists, else the deterministic rules (the
/// same resolver the import applies). `None` keeps the row uncategorized.
fn resolve_row_category(
    conn: &Connection,
    config: &CsvImportConfig,
    line: usize,
    insert: &InsertBankTransaction,
    engine: Option<&CategorizationEngine>,
) -> Result<Option<(String, &'static str)>> {
    if let Some(over) = config.category_overrides.iter().find(|o| o.line == line) {
        if super::bank_accounts::category_exists(conn, &over.category_id)? {
            return Ok(Some((over.category_id.clone(), SOURCE_MANUAL)));
        }
    }
    let Some(engine) = engine else {
        return Ok(None);
    };
    let input = super::categorization::apply::transaction_input_from_insert(insert);
    let Some((category_id, source)) =
        super::categorization::apply::resolve_definitive(engine, &input)
    else {
        return Ok(None);
    };
    if !super::bank_accounts::category_exists(conn, &category_id)? {
        return Ok(None);
    }
    Ok(Some((category_id, source)))
}

/// ISO country code of an IBAN-looking value (`DE89 3704 …` → `DE`).
fn iban_country(value: &str) -> Option<String> {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let chars: Vec<char> = compact.chars().collect();
    let plausible = (15..=34).contains(&chars.len())
        && chars[..2].iter().all(|c| c.is_ascii_uppercase())
        && chars[2..4].iter().all(|c| c.is_ascii_digit())
        && chars[4..].iter().all(|c| c.is_ascii_alphanumeric());
    plausible.then(|| chars[..2].iter().collect())
}

/// The rule pack the imported counterparties point to: the most
/// frequent IBAN country among `ibans` (ties → alphabetical), as the pack id
/// (lowercase ISO code), when such a pack exists and is not enabled yet.
fn suggest_rule_pack(conn: &Connection, ibans: &[String]) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for iban in ibans {
        if let Some(country) = iban_country(iban) {
            *counts.entry(country).or_default() += 1;
        }
    }
    let (country, _) = counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?;
    let pack_id = country.to_ascii_lowercase();
    if !packs::available_packs()
        .iter()
        .any(|p| p.pack_id == pack_id)
    {
        return None;
    }
    // A hint must never fail the import: unreadable config → no hint.
    let (enabled, _) = packs::read_config(conn).ok()?;
    (!enabled.contains(&pack_id)).then_some(pack_id)
}

/// Balance after the chronologically latest statement row, as a signed decimal
/// string. Statements come oldest-first (Revolut) or newest-first (Chase, most
/// Czech banks), so the direction is read from the file: newest-first files
/// take the first row of the latest day, oldest-first files the last one.
/// Rows that are errors or were filtered out do not count; skipped duplicates
/// and zero rows do (they are real statement lines).
fn last_balance(rows: &[ParsedRow]) -> Option<String> {
    statement_balance_row(rows)
        .and_then(|r| r.balance)
        .map(|b| b.to_string())
}

/// Newest booking day; descending statements select the first row on that day,
/// ascending statements select the last. Pending rows (including zeros) never
/// provide a booked statement balance.
fn statement_balance_row(rows: &[ParsedRow]) -> Option<&ParsedRow> {
    let dated: Vec<_> = rows
        .iter()
        .filter(|r| {
            r.booking_date.is_some()
                && r.transaction_status.as_deref() != Some("pending")
                && matches!(r.state, RowState::Candidate(_) | RowState::SkippedZero(_))
        })
        .collect();
    let latest = dated.iter().filter_map(|r| r.booking_date).max()?;
    let newest_first =
        dated.first().and_then(|r| r.booking_date) > dated.last().and_then(|r| r.booking_date);
    let mut latest_rows = dated.into_iter().filter(|r| r.booking_date == Some(latest));
    if newest_first {
        latest_rows.next()
    } else {
        latest_rows.next_back()
    }
}

/// Import bank transactions from decoded CSV content (one write path, ADR 0007).
///
/// Phase 1 (no writes): `parse_rows` — unparseable or invalid rows become
/// per-row errors, unlike the all-or-nothing MCP bulk tool a UI import proceeds
/// with the good rows. Phase 2: one SQL transaction — batch record, then every
/// row goes through `services::bank_accounts::create_transaction_with_origin`.
///
/// Stored occurrences are consumed once; new ID-less repeats stay distinct.
/// Certain bank IDs cannot be forced; same-ID pending rows promote in place
/// while retaining their original batch ownership and explicit user meaning.
///
/// `engine` enables rules-first categorization (spec D2): every row that is
/// written is run through the app's deterministic layers just before its
/// INSERT. `None` disables that (unit tests) and every row lands uncategorized.
pub fn import_transactions(
    conn: &mut Connection,
    account_id: &str,
    file_name: &str,
    content: &str,
    config: &CsvImportConfig,
    engine: Option<&CategorizationEngine>,
) -> Result<CsvImportResult> {
    ensure_account_exists(conn, account_id)?;
    config.validate()?;
    let parsed = parse_rows(content, account_id, config)?;

    let batch_id = Uuid::new_v4().to_string();
    let mut checker = dedup::BankOccurrenceChecker::load(conn, account_id)?;
    let mut imported_count = 0usize;
    let mut promoted_count = 0usize;
    let mut uncategorized_count = 0usize;
    let mut errors: Vec<CsvRowMessage> = Vec::new();
    let mut duplicates: Vec<CsvRowMessage> = Vec::new();
    let mut skipped_duplicates: Vec<CsvRowMessage> = Vec::new();
    let (mut skipped_zero, mut skipped_filtered) = (0usize, 0usize);
    let mut date_range: Option<CsvDateRange> = None;
    let mut imported_ibans: Vec<String> = Vec::new();

    let mut statement_range = None;
    for row in &parsed.rows {
        if matches!(row.state, RowState::Candidate(_) | RowState::SkippedZero(_)) {
            if let Some(day) = row.booking_date {
                extend_range(&mut statement_range, day);
            }
        }
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO csv_import_batches (id, bank_account_id, file_name, imported_count, duplicate_count, error_count,date_from,date_to) VALUES (?1, ?2, ?3, 0, 0, 0,?4,?5)",
        rusqlite::params![batch_id, account_id, file_name,statement_range.map(|r|r.from),statement_range.map(|r|r.to)],
    )?;

    for row in &parsed.rows {
        let mut insert = match &row.state {
            RowState::Candidate(insert) => (**insert).clone(),
            RowState::Error(m) => {
                errors.push(m.clone());
                continue;
            }
            RowState::SkippedZero(_) => {
                skipped_zero += 1;
                continue;
            }
            RowState::SkippedFiltered(_) => {
                skipped_filtered += 1;
                continue;
            }
        };

        if let Some(hit) = checker.check(&insert) {
            if let dedup::BankOccurrence::Promote(id) = hit {
                super::bank_accounts::promote_pending_transaction(&tx, &id, &insert)?;
                promoted_count += 1;
                extend_range(&mut date_range, insert.booking_date);
                continue;
            }
            let kind = if hit == dedup::BankOccurrence::Certain {
                DuplicateKind::BankId
            } else {
                DuplicateKind::Composite
            };
            let forced = kind == DuplicateKind::Composite
                && (!config.skip_duplicates || config.import_anyway_lines.contains(&row.line));
            if !forced {
                skipped_duplicates.push(duplicate_message(row.line, kind));
                continue;
            }
            duplicates.push(duplicate_message(row.line, kind));
        }

        // A category picked in the preview wins over the rules (the user was
        // explicit); a since-deleted category falls through to the rules.
        let mut categorization_source: Option<&'static str> = None;
        if let Some(over) = config
            .category_overrides
            .iter()
            .find(|o| o.line == row.line)
        {
            if super::bank_accounts::category_exists(&tx, &over.category_id)? {
                insert.category_id = Some(over.category_id.clone());
                categorization_source = Some(SOURCE_MANUAL);
            }
        }
        // Rules-first: pre-write enrichment, so the write path itself stays a
        // plain `&Connection` service call (spec D2).
        if categorization_source.is_none() {
            categorization_source =
                super::bank_accounts::enrich_with_rules(&tx, &mut insert, engine)?;
        }
        let created = super::bank_accounts::create_transaction_with_origin(
            &tx,
            &insert,
            "csv_import",
            Some(&batch_id),
            categorization_source,
        )?;
        checker.record_written(&insert, &created.id);
        tx.execute(
            "UPDATE bank_transactions SET import_line=?1,row_balance=?2 WHERE id=?3",
            rusqlite::params![
                row.line as i64,
                row.balance.map(|b| b.to_string()),
                created.id
            ],
        )?;
        imported_count += 1;
        if insert.category_id.is_none() {
            uncategorized_count += 1;
        }
        extend_range(&mut date_range, insert.booking_date);
        if let Some(iban) = insert.counterparty_iban {
            imported_ibans.push(iban);
        }
    }

    // `duplicate_count` of the batch = rows NOT imported because they were
    // duplicates; newly inserted and promoted rows have separate counters.
    tx.execute(
        "UPDATE csv_import_batches SET imported_count = ?1, duplicate_count = ?2, error_count = ?3, promoted_count = ?5 WHERE id = ?4",
        rusqlite::params![
            imported_count as i64,
            skipped_duplicates.len() as i64,
            errors.len() as i64,
            batch_id,
            promoted_count as i64
        ],
    )?;
    tx.commit()?;

    Ok(CsvImportResult {
        imported_count,
        promoted_count,
        statement_balance: statement_balance(&parsed.rows, checker.currency(), &batch_id),
        duplicate_count: duplicates.len(),
        error_count: errors.len(),
        errors,
        duplicates,
        skipped_duplicates,
        skipped_zero,
        skipped_filtered,
        date_range,
        uncategorized_count,
        last_balance: last_balance(&parsed.rows),
        suggested_rule_pack: suggest_rule_pack(conn, &imported_ibans),
    })
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

fn statement_balance(
    rows: &[ParsedRow],
    account_currency: &str,
    batch_id: &str,
) -> Option<types::StatementBalance> {
    let row = statement_balance_row(rows)?;
    Some(types::StatementBalance {
        amount: row.balance?.to_string(),
        currency: row
            .currency
            .as_deref()
            .unwrap_or(account_currency)
            .to_ascii_uppercase(),
        as_of: row.booking_date?,
        line: row.line,
        batch_id: batch_id.into(),
    })
}

pub fn import_batches(conn: &Connection, account_id: &str) -> Result<Vec<types::CsvImportBatch>> {
    let mut stmt=conn.prepare("SELECT b.id,b.bank_account_id,b.file_name,b.imported_count,b.duplicate_count,b.error_count,b.imported_at,b.promoted_count,COUNT(t.id),b.date_from,b.date_to FROM csv_import_batches b LEFT JOIN bank_transactions t ON t.import_batch_id=b.id WHERE b.bank_account_id=?1 GROUP BY b.id ORDER BY b.imported_at DESC,b.id")?;
    let rows = stmt.query_map([account_id], |r| {
        let from: Option<i64> = r.get(9)?;
        let to: Option<i64> = r.get(10)?;
        Ok(types::CsvImportBatch {
            id: r.get(0)?,
            bank_account_id: r.get(1)?,
            file_name: r.get(2)?,
            imported_count: r.get(3)?,
            duplicate_count: r.get(4)?,
            error_count: r.get(5)?,
            imported_at: r.get(6)?,
            promoted_count: r.get(7)?,
            linked_count: r.get(8)?,
            date_range: from.zip(to).map(|(from, to)| CsvDateRange { from, to }),
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
pub fn delete_import_batch(conn: &Connection, batch_id: &str) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM csv_import_batches WHERE id=?1)",
        [batch_id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(AppError::NotFound(format!(
            "Import batch not found: {batch_id}"
        )));
    }
    let deleted = tx.execute(
        "DELETE FROM bank_transactions WHERE import_batch_id=?1",
        [batch_id],
    )?;
    tx.execute("DELETE FROM csv_import_batches WHERE id=?1", [batch_id])?;
    tx.commit()?;
    Ok(deleted)
}
