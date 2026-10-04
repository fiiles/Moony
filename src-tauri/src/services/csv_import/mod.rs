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

use std::collections::{HashMap, HashSet};

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

type CompositeKey = (i64, Option<i64>, String, String, Option<String>);

fn composite_key(t: &InsertBankTransaction) -> CompositeKey {
    (
        t.booking_date,
        t.value_date,
        t.amount.clone(),
        t.tx_type.to_lowercase(),
        t.description.clone(),
    )
}

/// The duplicate rule of `services::dedup` plus the rows already accepted from
/// this file. The same checker backs the preview and the import, so what the
/// preview promises is what the import does.
#[derive(Default)]
struct DuplicateChecker {
    seen_ids: HashSet<String>,
    /// Composite keys of every accepted row.
    seen_keys: HashSet<CompositeKey>,
    /// Composite keys of accepted rows that carry no bank-side id. A row with
    /// an id only matches these: a different id is a different payment
    ///, exactly as `dedup::check_bank_transaction` treats stored rows.
    seen_idless_keys: HashSet<CompositeKey>,
}

impl DuplicateChecker {
    fn check(
        &self,
        conn: &Connection,
        account_id: &str,
        t: &InsertBankTransaction,
    ) -> Result<Option<DuplicateKind>> {
        let id = t
            .transaction_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if id.is_some_and(|id| self.seen_ids.contains(id)) {
            return Ok(Some(DuplicateKind::BankId));
        }
        if let Some(reason) = dedup::find_duplicate_bank_transaction(
            conn,
            account_id,
            t.transaction_id.as_deref(),
            t.booking_date,
            t.value_date,
            &t.amount,
            &t.tx_type,
            t.description.as_deref(),
        )? {
            // dedup reports the id rule first; its reason names the bank-side id.
            return Ok(Some(if reason.contains("bank-side id") {
                DuplicateKind::BankId
            } else {
                DuplicateKind::Composite
            }));
        }
        let key = composite_key(t);
        let hit = if id.is_some() {
            self.seen_idless_keys.contains(&key)
        } else {
            self.seen_keys.contains(&key)
        };
        Ok(hit.then_some(DuplicateKind::Composite))
    }

    /// Remember a row that is going to be written.
    fn accept(&mut self, t: &InsertBankTransaction) {
        let key = composite_key(t);
        if let Some(id) = t
            .transaction_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.seen_ids.insert(id.to_string());
        } else {
            self.seen_idless_keys.insert(key.clone());
        }
        self.seen_keys.insert(key);
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
/// line is in `importAnywayLines`, or `skipDuplicates` is off), `skipped` (zero
/// amount or row filter). Lists the first [`PREVIEW_ROWS`] rows; the counts
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

    let mut checker = DuplicateChecker::default();
    let mut rows: Vec<CsvPreviewRow> = Vec::new();
    let (mut ok_count, mut error_count, mut duplicate_count, mut skipped_count) = (0, 0, 0, 0);
    let mut categorized_count = 0usize;
    let mut date_range: Option<CsvDateRange> = None;

    for row in &parsed.rows {
        let mut category: Option<(String, &'static str)> = None;
        let (status, message) = match &row.state {
            RowState::Candidate(insert) => {
                category = resolve_row_category(conn, config, row.line, insert, engine)?;
                match checker.check(conn, account_id, insert)? {
                    None => {
                        ok_count += 1;
                        if category.is_some() {
                            categorized_count += 1;
                        }
                        checker.accept(insert);
                        extend_range(&mut date_range, insert.booking_date);
                        (CsvRowStatus::Ok, None)
                    }
                    Some(kind) => {
                        duplicate_count += 1;
                        if !config.skip_duplicates || config.import_anyway_lines.contains(&row.line)
                        {
                            if category.is_some() {
                                categorized_count += 1;
                            }
                            checker.accept(insert);
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
        if rows.len() < PREVIEW_ROWS {
            rows.push(CsvPreviewRow {
                line: row.line,
                booking_date: row.booking_date,
                amount: row.signed_amount.clone(),
                currency: row.currency.clone(),
                description: row.description.clone(),
                counterparty: row.counterparty.clone(),
                status,
                message,
                category_id: category.as_ref().map(|(id, _)| id.clone()),
                category_source: category.map(|(_, source)| source.to_string()),
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
    let dated: Vec<&ParsedRow> = rows
        .iter()
        .filter(|r| {
            r.booking_date.is_some()
                && matches!(r.state, RowState::Candidate(_) | RowState::SkippedZero(_))
        })
        .collect();
    let latest = dated.iter().filter_map(|r| r.booking_date).max()?;
    let newest_first =
        dated.first().and_then(|r| r.booking_date) > dated.last().and_then(|r| r.booking_date);
    let mut on_latest_day = dated.iter().filter(|r| r.booking_date == Some(latest));
    let row = if newest_first {
        on_latest_day.next()
    } else {
        on_latest_day.next_back()
    }?;
    row.balance.map(|b| {
        if b == 0.0 {
            "0".to_string()
        } else {
            b.to_string()
        }
    })
}

/// Import bank transactions from decoded CSV content (one write path, ADR 0007).
///
/// Phase 1 (no writes): `parse_rows` — unparseable or invalid rows become
/// per-row errors, unlike the all-or-nothing MCP bulk tool a UI import proceeds
/// with the good rows. Phase 2: one SQL transaction — batch record, then every
/// row goes through `services::bank_accounts::create_transaction_with_origin`.
///
/// Duplicates: a `services::dedup::find_duplicate_bank_transaction`
/// hit is **skipped** unless its line is in `config.import_anyway_lines` (or
/// `config.skip_duplicates` is off). The check runs inside the import
/// transaction, so an identical row later in the same file matches the earlier
/// one. When a row is imported anyway after a *bank-side id* hit, its id is not
/// stored (the (account, id) pair is unique). The MCP bulk tools keep their own
/// skip-and-report behaviour.
///
/// Two genuine purchases that look identical (two 99 CZK coffees on the same
/// day, same description) are indistinguishable from a re-imported line under
/// the composite rule — that is what the preview's "import anyway" is for.
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
    let mut checker = DuplicateChecker::default();
    let mut imported_count = 0usize;
    let mut uncategorized_count = 0usize;
    let mut errors: Vec<CsvRowMessage> = Vec::new();
    let mut duplicates: Vec<CsvRowMessage> = Vec::new();
    let mut skipped_duplicates: Vec<CsvRowMessage> = Vec::new();
    let (mut skipped_zero, mut skipped_filtered) = (0usize, 0usize);
    let mut date_range: Option<CsvDateRange> = None;
    let mut imported_ibans: Vec<String> = Vec::new();

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO csv_import_batches (id, bank_account_id, file_name, imported_count, duplicate_count, error_count) VALUES (?1, ?2, ?3, 0, 0, 0)",
        rusqlite::params![batch_id, account_id, file_name],
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

        if let Some(kind) = checker.check(&tx, account_id, &insert)? {
            let forced = !config.skip_duplicates || config.import_anyway_lines.contains(&row.line);
            if !forced {
                skipped_duplicates.push(duplicate_message(row.line, kind));
                continue;
            }
            duplicates.push(duplicate_message(row.line, kind));
            if kind == DuplicateKind::BankId {
                // (account, bank id) is unique: the second copy goes in without one.
                insert.transaction_id = None;
            }
        }
        checker.accept(&insert);

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
        super::bank_accounts::create_transaction_with_origin(
            &tx,
            &insert,
            "csv_import",
            Some(&batch_id),
            categorization_source,
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
    // duplicates, so imported + duplicates + errors adds up in the history.
    tx.execute(
        "UPDATE csv_import_batches SET imported_count = ?1, duplicate_count = ?2, error_count = ?3 WHERE id = ?4",
        rusqlite::params![
            imported_count as i64,
            skipped_duplicates.len() as i64,
            errors.len() as i64,
            batch_id
        ],
    )?;
    tx.commit()?;

    Ok(CsvImportResult {
        imported_count,
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
