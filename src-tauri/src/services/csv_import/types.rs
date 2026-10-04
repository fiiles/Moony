//! Wire types of the CSV import (TS: `CsvImportConfigInput`, `CsvImportResult`,
//! `CsvImportPreview`, `CsvPreviewRow`, `CsvPreviewResult`, `CsvRowMessage`,
//! `CsvDateRange`). Per-field `#[serde(rename = "camelCase")]`, as everywhere in
//! this codebase; mirror every change in `shared/schema.ts`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::services::csv_presets::CsvRowFilter;

/// A per-row problem or note. Never carries English prose: `key` is an i18n
/// key and `detail` the raw value it is about (the unparseable date, the
/// filtered state, …).
///
/// Keys (namespace in front of the dot):
/// - `csvImport.*` — `bank_accounts` namespace: `rowCannotParse`,
///   `dateUnparseable`, `amountUnparseable`, `zeroAmount`, `filteredOut`,
///   `duplicate`, `duplicateById`;
/// - `validation.*` — `common` namespace: whatever `InsertBankTransaction::
///   validate()` rejects with (`validation.currencyInvalid`, …),
///   `validation.currencyUnknown` for a currency cell that is not an ISO 4217
///   code (the cell goes in `detail`), plus `validation.dateRequired` /
///   `validation.amountRequired` for empty cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CsvRowMessage {
    /// 1-based line of the file where the row starts (the header line counts).
    pub line: usize,
    pub key: String,
    pub detail: Option<String>,
}

/// Inclusive range of booking dates (unix seconds, midnight UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CsvDateRange {
    pub from: i64,
    pub to: i64,
}

fn default_true() -> bool {
    true
}

/// Configuration of one CSV import or preview (TS `CsvImportConfigInput`).
///
/// Column names are the file's header cells; a name that differs only in case,
/// diacritics or punctuation still resolves (see `columns::find_column`), so a
/// preset's spelling can be passed through unchanged.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CsvImportConfig {
    /// One character; empty = auto-detect.
    pub delimiter: String,
    /// Data rows to skip *after* the header row.
    #[serde(rename = "skipRows", default)]
    pub skip_rows: usize,
    /// 0-based line of the header row; `None` = auto-detect.
    #[serde(rename = "headerRow", default)]
    pub header_row: Option<usize>,
    #[serde(rename = "dateColumn")]
    pub date_column: String,
    /// chrono format (`%d.%m.%Y`). Parsed strictly: a row that does not match
    /// is an error, never re-read as another order. Empty or `auto` detects the
    /// format from the date column.
    #[serde(rename = "dateFormat")]
    pub date_format: String,
    /// Single signed amount column. Optional when a debit/credit pair is given.
    #[serde(rename = "amountColumn", default)]
    pub amount_column: Option<String>,
    /// Debit/credit pair (`amount = credit − debit`). Used when `amountColumn`
    /// is not set; both must be set.
    #[serde(rename = "debitColumn", default)]
    pub debit_column: Option<String>,
    #[serde(rename = "creditColumn", default)]
    pub credit_column: Option<String>,
    /// Running balance after each row; only feeds `lastBalance`.
    #[serde(rename = "balanceColumn", default)]
    pub balance_column: Option<String>,
    /// Bank-side transaction id; feeds the id rule of the duplicate check.
    #[serde(rename = "transactionIdColumn", default)]
    pub transaction_id_column: Option<String>,
    #[serde(rename = "descriptionColumns", default)]
    pub description_columns: Option<Vec<String>>,
    #[serde(rename = "counterpartyColumn", default)]
    pub counterparty_column: Option<String>,
    #[serde(rename = "counterpartyIbanColumn", default)]
    pub counterparty_iban_column: Option<String>,
    #[serde(rename = "currencyColumn", default)]
    pub currency_column: Option<String>,
    /// `","` or `"."`; `None`, empty or `auto` detects it from the amount
    /// column(s).
    #[serde(rename = "decimalSeparator", default)]
    pub decimal_separator: Option<String>,
    #[serde(rename = "rowFilter", default)]
    pub row_filter: Option<CsvRowFilter>,
    /// Skip rows that the duplicate check matches (default). `false` imports
    /// them and reports them in `duplicates`.
    #[serde(rename = "skipDuplicates", default = "default_true")]
    pub skip_duplicates: bool,
    /// File lines of duplicate rows to import anyway.
    #[serde(rename = "importAnywayLines", default)]
    pub import_anyway_lines: Vec<usize>,
    /// Force a text encoding (`Windows-1250`); `None` = detect.
    #[serde(default)]
    pub encoding: Option<String>,
    /// Categories the user picked in the preview for rows the rules left
    /// uncategorized (or wanted differently), by file line. Applied on import
    /// before the rules run, so they win; a missing category is ignored.
    #[serde(rename = "categoryOverrides", default)]
    pub category_overrides: Vec<CsvCategoryOverride>,
}

/// One row's category chosen in the preview (TS `CsvCategoryOverride`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CsvCategoryOverride {
    /// 1-based file line of the row.
    pub line: usize,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

impl Default for CsvImportConfig {
    fn default() -> Self {
        CsvImportConfig {
            delimiter: String::new(),
            skip_rows: 0,
            header_row: None,
            date_column: String::new(),
            date_format: String::new(),
            amount_column: None,
            debit_column: None,
            credit_column: None,
            balance_column: None,
            transaction_id_column: None,
            description_columns: None,
            counterparty_column: None,
            counterparty_iban_column: None,
            currency_column: None,
            decimal_separator: None,
            row_filter: None,
            skip_duplicates: true,
            import_anyway_lines: Vec::new(),
            encoding: None,
            category_overrides: Vec::new(),
        }
    }
}

fn is_blank(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|s| s.trim().is_empty())
}

impl CsvImportConfig {
    /// Validate at the trust boundary. Keys are `validation.*` i18n keys.
    pub fn validate(&self) -> Result<()> {
        if self.date_column.trim().is_empty() {
            return Err(AppError::Validation("validation.dateColumnRequired".into()));
        }
        let has_amount = !is_blank(&self.amount_column);
        let has_pair = !is_blank(&self.debit_column) && !is_blank(&self.credit_column);
        if !has_amount && !has_pair {
            return Err(AppError::Validation(
                "validation.amountColumnRequired".into(),
            ));
        }
        parse_decimal_separator(self.decimal_separator.as_deref())?;
        Ok(())
    }

    /// The single-character delimiter, if one was configured.
    pub fn delimiter_char(&self) -> Option<char> {
        self.delimiter.chars().next()
    }
}

/// `Ok(None)` = detect from the data.
pub fn parse_decimal_separator(value: Option<&str>) -> Result<Option<char>> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(s) if s.eq_ignore_ascii_case("auto") => Ok(None),
        Some(",") => Ok(Some(',')),
        Some(".") => Ok(Some('.')),
        Some(_) => Err(AppError::Validation(
            "validation.csvDecimalSeparatorInvalid".into(),
        )),
    }
}

/// Result of an import (TS `CsvImportResult`).
///
/// Every row of the file ends up in exactly one bucket:
/// `importedCount + skippedDuplicates.len() + errorCount + skippedZero +
/// skippedFiltered` = data rows (blank lines are not rows).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CsvImportResult {
    /// Rows written (includes duplicates that were imported anyway).
    #[serde(rename = "importedCount")]
    pub imported_count: usize,
    /// Duplicate hits that were imported anyway — `duplicates.len()`.
    #[serde(rename = "duplicateCount")]
    pub duplicate_count: usize,
    #[serde(rename = "errorCount")]
    pub error_count: usize,
    pub errors: Vec<CsvRowMessage>,
    /// Duplicates that were imported anyway (`importAnywayLines` or
    /// `skipDuplicates: false`).
    pub duplicates: Vec<CsvRowMessage>,
    /// Duplicates that were skipped.
    #[serde(rename = "skippedDuplicates")]
    pub skipped_duplicates: Vec<CsvRowMessage>,
    #[serde(rename = "skippedZero")]
    pub skipped_zero: usize,
    /// Rows dropped by the row filter (Revolut pending rows).
    #[serde(rename = "skippedFiltered")]
    pub skipped_filtered: usize,
    /// Booking dates of the imported rows; `None` when nothing was imported.
    #[serde(rename = "dateRange")]
    pub date_range: Option<CsvDateRange>,
    /// Imported rows the rules could not categorize.
    #[serde(rename = "uncategorizedCount")]
    pub uncategorized_count: usize,
    /// Balance after the chronologically last statement row, as a signed
    /// decimal string; `None` without a balance column.
    #[serde(rename = "lastBalance")]
    pub last_balance: Option<String>,
    /// Rule-pack id (lowercase ISO country) the counterparty IBANs point to
    /// when that pack is not enabled yet.
    #[serde(rename = "suggestedRulePack")]
    pub suggested_rule_pack: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum CsvRowStatus {
    /// Will be imported.
    #[serde(rename = "ok")]
    Ok,
    /// Cannot be imported (see `message`).
    #[serde(rename = "error")]
    Error,
    /// Matches an existing or earlier transaction; skipped unless imported anyway.
    #[serde(rename = "duplicate")]
    Duplicate,
    /// Dropped on purpose: zero amount or row filter.
    #[serde(rename = "skipped")]
    Skipped,
}

/// One row of the preview table.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CsvPreviewRow {
    pub line: usize,
    #[serde(rename = "bookingDate")]
    pub booking_date: Option<i64>,
    /// Signed decimal string (`-250.5`), `None` when the amount is unreadable.
    pub amount: Option<String>,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub counterparty: Option<String>,
    pub status: CsvRowStatus,
    pub message: Option<CsvRowMessage>,
    /// Category the import would write: the user's override for this line, else
    /// what the deterministic rules resolve; `None` = stays uncategorized.
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    /// `manual` for an override, else the rule layer (`rule`, `exact_match`,
    /// `own_account`).
    #[serde(rename = "categorySource")]
    pub category_source: Option<String>,
}

/// Dry run of an import (TS `CsvImportPreview`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CsvImportPreview {
    /// The first 200 rows in file order.
    pub rows: Vec<CsvPreviewRow>,
    #[serde(rename = "totalRows")]
    pub total_rows: usize,
    #[serde(rename = "okCount")]
    pub ok_count: usize,
    #[serde(rename = "errorCount")]
    pub error_count: usize,
    #[serde(rename = "duplicateCount")]
    pub duplicate_count: usize,
    /// Zero-amount and filtered rows.
    #[serde(rename = "skippedCount")]
    pub skipped_count: usize,
    /// Booking dates of the rows an import with this config would write.
    #[serde(rename = "dateRange")]
    pub date_range: Option<CsvDateRange>,
    /// Rows that would be written with a category (rules or overrides).
    #[serde(rename = "categorizedCount")]
    pub categorized_count: usize,
}

/// What `parse_csv_file` learned about a file (TS `CsvPreviewResult`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CsvPreviewResult {
    pub headers: Vec<String>,
    /// First five data rows.
    #[serde(rename = "sampleRows")]
    pub sample_rows: Vec<Vec<String>>,
    #[serde(rename = "totalRows")]
    pub total_rows: usize,
    pub delimiter: String,
    /// Role → (header, confidence 0–1). Roles: `date`, `valueDate`, `amount`,
    /// `debit`, `credit`, `balance`, `currency`, `description`,
    /// `counterparty`, `counterparty_iban` (legacy snake_case key),
    /// `transactionId`. A detected preset's columns come first, at 1.0.
    #[serde(rename = "suggestedMappings")]
    pub suggested_mappings: HashMap<String, (String, f32)>,
    /// 0-based line of the header row.
    #[serde(rename = "headerRow")]
    pub header_row: usize,
    #[serde(rename = "detectedPresetId")]
    pub detected_preset_id: Option<String>,
    /// chrono format of the date column (date part only).
    #[serde(rename = "dateFormat")]
    pub date_format: String,
    /// `true` when day/month order cannot be told from the samples (all values
    /// ≤ 12 in both positions) and `dateFormat` is a guess the user should
    /// confirm.
    #[serde(rename = "dateFormatAmbiguous")]
    pub date_format_ambiguous: bool,
    /// `","` or `"."`.
    #[serde(rename = "decimalSeparator")]
    pub decimal_separator: String,
    /// Encoding the file was decoded with.
    pub encoding: String,
    /// All description columns to pre-select: the preset's, else the single
    /// suggested `description`.
    #[serde(rename = "suggestedDescriptionColumns")]
    pub suggested_description_columns: Vec<String>,
    /// The detected preset's row filter, resolved to the file's header name.
    #[serde(rename = "suggestedRowFilter")]
    pub suggested_row_filter: Option<CsvRowFilter>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserializes_the_pre_v2_payload_with_defaults() {
        let json = r#"{
            "delimiter": ";", "skipRows": 0, "dateColumn": "Datum",
            "dateFormat": "%d.%m.%Y", "amountColumn": "Částka",
            "descriptionColumns": ["Popis"], "counterpartyColumn": null,
            "counterpartyIbanColumn": null, "currencyColumn": null
        }"#;
        let config: CsvImportConfig = serde_json::from_str(json).expect("old payload");
        assert!(config.skip_duplicates, "duplicates are skipped by default");
        assert!(config.import_anyway_lines.is_empty());
        assert!(config.header_row.is_none() && config.row_filter.is_none());
        assert_eq!(config.amount_column.as_deref(), Some("Částka"));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validation_uses_i18n_keys() {
        let mut config = CsvImportConfig {
            date_column: "Date".into(),
            amount_column: Some("Amount".into()),
            ..Default::default()
        };
        assert!(config.validate().is_ok());

        config.date_column = " ".into();
        assert!(matches!(
            config.validate(),
            Err(AppError::Validation(k)) if k == "validation.dateColumnRequired"
        ));

        config.date_column = "Date".into();
        config.amount_column = None;
        assert!(matches!(
            config.validate(),
            Err(AppError::Validation(k)) if k == "validation.amountColumnRequired"
        ));
        // Half a pair is not enough; the full pair is.
        config.debit_column = Some("Debit".into());
        assert!(config.validate().is_err());
        config.credit_column = Some("Credit".into());
        assert!(config.validate().is_ok());

        config.decimal_separator = Some(";".into());
        assert!(matches!(
            config.validate(),
            Err(AppError::Validation(k)) if k == "validation.csvDecimalSeparatorInvalid"
        ));
    }

    #[test]
    fn decimal_separator_parsing() {
        assert_eq!(parse_decimal_separator(None).unwrap(), None);
        assert_eq!(parse_decimal_separator(Some("")).unwrap(), None);
        assert_eq!(parse_decimal_separator(Some("auto")).unwrap(), None);
        assert_eq!(parse_decimal_separator(Some(",")).unwrap(), Some(','));
        assert_eq!(parse_decimal_separator(Some(" . ")).unwrap(), Some('.'));
        assert!(parse_decimal_separator(Some("x")).is_err());
    }

    #[test]
    fn row_status_serializes_lowercase() {
        let json = serde_json::to_string(&CsvRowStatus::Duplicate).unwrap();
        assert_eq!(json, "\"duplicate\"");
    }
}
