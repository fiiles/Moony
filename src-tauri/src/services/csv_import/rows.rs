//! Phase 1 of an import — shared by the preview and the real import: turn the
//! decoded file into typed rows without touching the database.
//!
//! `parse_rows` resolves the settings the file needs (delimiter, header row,
//! column indexes, decimal separator, date format), then classifies every data
//! row as a ready candidate, an error, or deliberately skipped (zero amount,
//! row filter). Duplicate detection needs the database and happens afterwards
//! in the caller, in file order.

use std::collections::HashSet;

use crate::error::{AppError, Result};
use crate::models::currency::is_iso_4217;
use crate::models::InsertBankTransaction;
use crate::services::{csv_presets::CsvRowFilter, date_parser};

use super::amounts::{
    amount_to_text, clean_and_parse_amount, detect_decimal_separator, AmountError,
};
use super::columns::find_column;
use super::decode::{
    csv_reader, detect_csv_delimiter, detect_header_row, read_headers, slice_from_line, LineIndex,
};
use super::types::{parse_decimal_separator, CsvImportConfig, CsvRowMessage};

/// How many data rows are looked at to settle the decimal separator and the
/// date format of "auto" settings.
pub const SAMPLE_ROWS: usize = 200;

/// Outcome of parsing one data row.
#[derive(Debug, Clone)]
pub enum RowState {
    /// Valid; still subject to the duplicate check.
    Candidate(Box<InsertBankTransaction>),
    /// Cannot be imported.
    Error(CsvRowMessage),
    /// Amount is zero.
    SkippedZero(CsvRowMessage),
    /// Dropped by the row filter.
    SkippedFiltered(CsvRowMessage),
}

/// One data row with what the preview table shows about it.
#[derive(Debug, Clone)]
pub struct ParsedRow {
    /// 1-based file line the row starts on.
    pub line: usize,
    pub booking_date: Option<i64>,
    /// Signed decimal text (`-250.5`).
    pub signed_amount: Option<String>,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub counterparty: Option<String>,
    /// Parsed balance cell (rows that are not errors/filtered only).
    pub balance: Option<f64>,
    pub state: RowState,
}

/// All data rows of a file in file order.
#[derive(Debug, Clone)]
pub struct ParsedCsv {
    pub rows: Vec<ParsedRow>,
    /// Non-blank data rows (what the user would call "rows in the file").
    pub total_rows: usize,
}

/// Column indexes resolved against the header row.
struct Columns {
    date: usize,
    amount: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    balance: Option<usize>,
    transaction_id: Option<usize>,
    description: Vec<usize>,
    counterparty: Option<usize>,
    counterparty_iban: Option<usize>,
    currency: Option<usize>,
    filter: Option<(usize, String)>,
}

fn lookup(headers: &[String], name: &Option<String>) -> Option<usize> {
    name.as_deref().and_then(|n| find_column(headers, n))
}

impl Columns {
    /// Required columns must exist, optional ones that are configured but
    /// missing are ignored (a preset may name columns an export variant lacks).
    fn resolve(headers: &[String], config: &CsvImportConfig) -> Result<Columns> {
        let not_found = || AppError::Validation("validation.csvColumnsNotFound".into());

        let date = find_column(headers, &config.date_column).ok_or_else(not_found)?;

        let amount = lookup(headers, &config.amount_column);
        let (debit, credit) = (
            lookup(headers, &config.debit_column),
            lookup(headers, &config.credit_column),
        );
        // A single amount column wins; the pair is the fallback.
        let (amount, debit, credit) = match (amount, debit, credit) {
            (Some(a), _, _) => (Some(a), None, None),
            (None, Some(d), Some(c)) => (None, Some(d), Some(c)),
            _ => return Err(not_found()),
        };

        let filter = match &config.row_filter {
            Some(CsvRowFilter { column, equals }) => {
                let idx = find_column(headers, column).ok_or_else(not_found)?;
                Some((idx, equals.trim().to_lowercase()))
            }
            None => None,
        };

        let description = config
            .description_columns
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(|c| find_column(headers, c))
            .collect();

        Ok(Columns {
            date,
            amount,
            debit,
            credit,
            balance: lookup(headers, &config.balance_column),
            transaction_id: lookup(headers, &config.transaction_id_column),
            description,
            counterparty: lookup(headers, &config.counterparty_column),
            counterparty_iban: lookup(headers, &config.counterparty_iban_column),
            currency: lookup(headers, &config.currency_column),
            filter,
        })
    }
}

/// Data records of `content` after the header row and `skip_rows`, with the
/// 1-based file line each starts on.
pub(super) fn data_records(
    content: &str,
    delimiter: char,
    header_row: usize,
    skip_rows: usize,
) -> impl Iterator<Item = (usize, std::result::Result<csv::StringRecord, csv::Error>)> + '_ {
    let tail = slice_from_line(content, header_row);
    let index = LineIndex::new(tail);
    csv_reader(tail, delimiter)
        .into_records()
        .skip(1 + skip_rows)
        .map(move |result| {
            // Physical line from the byte offset (the csv crate's line counter
            // skips blank lines); the tail starts on 0-based line `header_row`.
            let byte = match &result {
                Ok(record) => record.position().map(|p| p.byte() as usize),
                Err(e) => e.position().map(|p| p.byte() as usize),
            };
            let line = byte.map_or(0, |b| header_row + index.line_of_record(b) + 1);
            (line, result)
        })
}

fn is_blank_record(record: &csv::StringRecord) -> bool {
    record.iter().all(|f| f.trim().is_empty())
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn message(line: usize, key: &str, detail: Option<&str>) -> CsvRowMessage {
    CsvRowMessage {
        line,
        key: key.to_string(),
        detail: detail.map(str::to_string),
    }
}

/// Parse the amount of a row: single column, or `credit − debit`.
fn parse_amount(
    record: &csv::StringRecord,
    cols: &Columns,
    decimal: char,
) -> std::result::Result<f64, (&'static str, Option<String>)> {
    let cell = |idx: usize| record.get(idx).unwrap_or("").trim();
    let cell_error = |raw: &str, e: AmountError| match e {
        AmountError::Empty => ("validation.amountRequired", None),
        AmountError::Invalid => ("csvImport.amountUnparseable", Some(raw.to_string())),
    };

    if let Some(idx) = cols.amount {
        let raw = cell(idx);
        return clean_and_parse_amount(raw, decimal).map_err(|e| cell_error(raw, e));
    }

    // Debit/credit pair: an empty side is zero, both empty is "no amount".
    // Both columns hold magnitudes (some banks print debits as negatives), so
    // the sign comes from which column carries the value.
    let (Some(d), Some(c)) = (cols.debit, cols.credit) else {
        return Err(("validation.amountRequired", None));
    };
    let side = |idx: usize| -> std::result::Result<Option<f64>, (&'static str, Option<String>)> {
        let raw = cell(idx);
        match clean_and_parse_amount(raw, decimal) {
            Ok(v) => Ok(Some(v.abs())),
            Err(AmountError::Empty) => Ok(None),
            Err(e) => Err(cell_error(raw, e)),
        }
    };
    match (side(c)?, side(d)?) {
        (None, None) => Err(("validation.amountRequired", None)),
        (Some(credit), None) => Ok(credit),
        (None, Some(debit)) => Ok(-debit),
        // Both filled (rare): subtract, rounded to kill float noise.
        (Some(credit), Some(debit)) => Ok(((credit - debit) * 1e8).round() / 1e8),
    }
}

/// Settings that depend on the data and are settled before rows are parsed.
struct Settled {
    decimal: char,
    date_format: String,
}

fn settle(
    content: &str,
    delimiter: char,
    header_row: usize,
    config: &CsvImportConfig,
    cols: &Columns,
) -> Result<Settled> {
    let configured_decimal = parse_decimal_separator(config.decimal_separator.as_deref())?;
    let auto_date = {
        let f = config.date_format.trim();
        f.is_empty() || f.eq_ignore_ascii_case("auto")
    };
    if configured_decimal.is_some() && !auto_date {
        return Ok(Settled {
            decimal: configured_decimal.unwrap_or('.'),
            date_format: config.date_format.clone(),
        });
    }

    let sample: Vec<csv::StringRecord> =
        data_records(content, delimiter, header_row, config.skip_rows)
            .filter_map(|(_, r)| r.ok())
            .filter(|r| !is_blank_record(r))
            .take(SAMPLE_ROWS)
            .collect();

    let decimal = match configured_decimal {
        Some(c) => c,
        None => {
            let amount_columns = [cols.amount, cols.debit, cols.credit, cols.balance];
            let cells: Vec<&str> = sample
                .iter()
                .flat_map(|r| amount_columns.iter().flatten().filter_map(|&i| r.get(i)))
                .collect();
            detect_decimal_separator(&cells)
        }
    };
    let date_format = if auto_date {
        let dates: Vec<&str> = sample
            .iter()
            .filter_map(|r| r.get(cols.date))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        date_parser::detect_date_format(&dates).format
    } else {
        config.date_format.clone()
    };
    Ok(Settled {
        decimal,
        date_format,
    })
}

/// Parse a decoded CSV into typed rows. No database access.
pub fn parse_rows(content: &str, account_id: &str, config: &CsvImportConfig) -> Result<ParsedCsv> {
    let delimiter = config
        .delimiter_char()
        .unwrap_or_else(|| detect_csv_delimiter(content));
    let header_row = config
        .header_row
        .unwrap_or_else(|| detect_header_row(content, delimiter));
    let headers = read_headers(content, delimiter, header_row);
    if headers.iter().all(|h| h.is_empty()) {
        return Err(AppError::Validation("validation.csvEmptyFile".into()));
    }
    let cols = Columns::resolve(&headers, config)?;
    let Settled {
        decimal,
        date_format,
    } = settle(content, delimiter, header_row, config, &cols)?;

    let mut rows = Vec::new();
    let mut total_rows = 0usize;

    for (line, result) in data_records(content, delimiter, header_row, config.skip_rows) {
        let record = match result {
            Ok(record) => record,
            Err(e) => {
                total_rows += 1;
                rows.push(bare_row(
                    line,
                    RowState::Error(message(
                        line,
                        "csvImport.rowCannotParse",
                        Some(&e.to_string()),
                    )),
                ));
                continue;
            }
        };
        if is_blank_record(&record) {
            continue;
        }
        total_rows += 1;
        rows.push(parse_row(
            line,
            &record,
            account_id,
            &cols,
            decimal,
            &date_format,
        ));
    }

    Ok(ParsedCsv { rows, total_rows })
}

/// A row about which nothing beyond its fate is known.
fn bare_row(line: usize, state: RowState) -> ParsedRow {
    ParsedRow {
        line,
        booking_date: None,
        signed_amount: None,
        currency: None,
        description: None,
        counterparty: None,
        balance: None,
        state,
    }
}

fn parse_row(
    line: usize,
    record: &csv::StringRecord,
    account_id: &str,
    cols: &Columns,
    decimal: char,
    date_format: &str,
) -> ParsedRow {
    let cell = |idx: usize| record.get(idx).unwrap_or("").trim();
    let cell_opt = |idx: Option<usize>| idx.and_then(|i| non_empty(cell(i)));

    // Row filter first: dropped rows (Revolut PENDING) often lack a date.
    if let Some((idx, equals)) = &cols.filter {
        let value = cell(*idx);
        if value.to_lowercase() != *equals {
            return bare_row(
                line,
                RowState::SkippedFiltered(message(line, "csvImport.filteredOut", Some(value))),
            );
        }
    }

    // The preview shows whatever parsed, even for rows that end up as errors.
    let description = {
        let mut parts: Vec<String> = Vec::new();
        for &i in &cols.description {
            if let Some(text) = non_empty(cell(i)) {
                if !parts.contains(&text) {
                    parts.push(text);
                }
            }
        }
        (!parts.is_empty()).then(|| parts.join(" | "))
    };
    let counterparty = cell_opt(cols.counterparty);
    let currency = cell_opt(cols.currency).map(|c| c.to_ascii_uppercase());

    let finish = |booking_date: Option<i64>,
                  signed_amount: Option<String>,
                  balance: Option<f64>,
                  state: RowState| ParsedRow {
        line,
        booking_date,
        signed_amount,
        currency: currency.clone(),
        description: description.clone(),
        counterparty: counterparty.clone(),
        balance,
        state,
    };
    let error = |key: &str, detail: Option<&str>| RowState::Error(message(line, key, detail));

    let raw_date = cell(cols.date);
    if raw_date.is_empty() {
        return finish(None, None, None, error("validation.dateRequired", None));
    }
    let Some(booking_date) = date_parser::parse_date_to_timestamp_strict(raw_date, date_format)
    else {
        return finish(
            None,
            None,
            None,
            error("csvImport.dateUnparseable", Some(raw_date)),
        );
    };

    let balance = cols
        .balance
        .and_then(|i| clean_and_parse_amount(cell(i), decimal).ok());

    let amount = match parse_amount(record, cols, decimal) {
        Ok(a) => a,
        Err((key, detail)) => {
            return finish(
                Some(booking_date),
                None,
                balance,
                error(key, detail.as_deref()),
            );
        }
    };
    if amount == 0.0 {
        return finish(
            Some(booking_date),
            Some("0".to_string()),
            balance,
            RowState::SkippedZero(message(line, "csvImport.zeroAmount", None)),
        );
    }
    let signed_amount = format!(
        "{}{}",
        if amount < 0.0 { "-" } else { "" },
        amount_to_text(amount)
    );

    let insert = InsertBankTransaction {
        bank_account_id: account_id.to_string(),
        transaction_id: cell_opt(cols.transaction_id),
        tx_type: if amount >= 0.0 { "credit" } else { "debit" }.to_string(),
        amount: amount_to_text(amount),
        // No currency cell → account currency (create_transaction fallback).
        currency: currency.clone(),
        description: description.clone(),
        counterparty_name: counterparty.clone(),
        counterparty_iban: cell_opt(cols.counterparty_iban),
        booking_date,
        value_date: None,
        category_id: None,
        status: None,
    };
    let state = match insert.validate() {
        // Well-formed but not a currency ("XYZ", a metal or fund code): reject
        // here, the wire-level check in validate() only knows "3 letters".
        Ok(()) if currency.as_deref().is_some_and(|c| !is_iso_4217(c)) => {
            error("validation.currencyUnknown", currency.as_deref())
        }
        Ok(()) => RowState::Candidate(Box::new(insert)),
        Err(AppError::Validation(key)) => error(&key, None),
        Err(other) => error("csvImport.rowCannotParse", Some(&other.to_string())),
    };
    finish(Some(booking_date), Some(signed_amount), balance, state)
}

/// Distinct values of a column among rows, for the transaction-id uniqueness
/// check done by the inspection step.
pub fn has_duplicates<'a>(values: impl IntoIterator<Item = &'a str>) -> bool {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|v| !v.trim().is_empty())
        .any(|v| !seen.insert(v.trim()))
}
