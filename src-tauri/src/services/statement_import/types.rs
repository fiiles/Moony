//! Wire types of the statement import (TS: `StatementInspection`, `StatementSummary`,
//! `StatementImportOptionsInput`, …). Per-field `#[serde(rename = "camelCase")]`; mirror
//! every change in `shared/schema.ts`.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::account_match::AccountMatch;
use super::merge::BalanceCheck;
use super::model::StatementFormat;
use crate::error::{AppError, Result};
use crate::services::csv_import::types::{default_preview_limit, default_true};
use crate::services::csv_import::{
    CsvCategoryOverride, CsvDateRange, DeclaredBalance, ImportOptions, SourceCategoryMapping,
};

/// The order of the day, month and year parts of a QIF date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum DateOrder {
    #[serde(rename = "dmy")]
    Dmy,
    #[serde(rename = "mdy")]
    Mdy,
    #[serde(rename = "ymd")]
    Ymd,
}

/// How many entries of a statement carry one source category (QIF `L` line).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SourceCategoryCount {
    pub name: String,
    pub count: usize,
}

/// A note about a file that does not stop the import. `key` is a
/// `statementImport.*` i18n key (`bank_accounts` namespace), `detail` the raw value
/// it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StatementWarning {
    pub key: String,
    pub detail: Option<String>,
}

/// What the dialog shows about one statement of a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StatementSummary {
    /// Position among the file's merged statements; the `statementIndex` of the options.
    pub index: usize,
    #[serde(rename = "accountIdentifier")]
    pub account_identifier: Option<String>,
    #[serde(rename = "accountName")]
    pub account_name: Option<String>,
    pub currency: Option<String>,
    pub period: Option<CsvDateRange>,
    #[serde(rename = "openingBalance")]
    pub opening_balance: Option<DeclaredBalance>,
    #[serde(rename = "closingBalance")]
    pub closing_balance: Option<DeclaredBalance>,
    /// Entries that would be imported (those without a skip reason).
    #[serde(rename = "entryCount")]
    pub entry_count: usize,
    /// Entries shown but never imported (not booked, corrections, …).
    #[serde(rename = "skippedCount")]
    pub skipped_count: usize,
    #[serde(rename = "balanceCheck")]
    pub balance_check: BalanceCheck,
    /// Declared minus computed, in the statement's currency; set on a mismatch.
    #[serde(rename = "balanceDifference")]
    pub balance_difference: Option<String>,
    #[serde(rename = "accountMatch")]
    pub account_match: AccountMatch,
    #[serde(rename = "matchedAccountId")]
    pub matched_account_id: Option<String>,
    /// Source categories of the importable entries, most frequent first.
    #[serde(rename = "sourceCategories")]
    pub source_categories: Vec<SourceCategoryCount>,
}

/// What `inspect_statement_file` learned about a statement file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StatementInspection {
    pub format: StatementFormat,
    /// Encoding the file was decoded with.
    pub encoding: String,
    pub statements: Vec<StatementSummary>,
    /// The first statement of the selected account, else the first.
    #[serde(rename = "defaultStatementIndex")]
    pub default_statement_index: usize,
    /// `true` when a QIF file's date order cannot be told from its dates.
    #[serde(rename = "dateOrderAmbiguous")]
    pub date_order_ambiguous: bool,
    #[serde(rename = "detectedDateOrder")]
    pub detected_date_order: Option<DateOrder>,
    pub warnings: Vec<StatementWarning>,
}

/// Options of one statement preview or import (TS `StatementImportOptionsInput`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StatementImportOptions {
    #[serde(rename = "statementIndex")]
    pub statement_index: usize,
    /// QIF only: how to read the dates; `None` = the detected order.
    #[serde(rename = "dateOrder", default)]
    pub date_order: Option<DateOrder>,
    /// Skip possible composite duplicates (default); certain bank IDs always skip.
    #[serde(rename = "skipDuplicates", default = "default_true")]
    pub skip_duplicates: bool,
    /// Entry ordinals of possible composite duplicates to import anyway.
    #[serde(rename = "importAnywayLines", default)]
    pub import_anyway_lines: Vec<usize>,
    /// Categories picked in the preview, by entry ordinal.
    #[serde(rename = "categoryOverrides", default)]
    pub category_overrides: Vec<CsvCategoryOverride>,
    /// Categories chosen for the source categories of the file.
    #[serde(rename = "sourceCategoryMap", default)]
    pub source_category_map: Vec<SourceCategoryMapping>,
    #[serde(rename = "previewOffset", default)]
    pub preview_offset: usize,
    #[serde(rename = "previewLimit", default = "default_preview_limit")]
    pub preview_limit: usize,
}

impl StatementImportOptions {
    /// The part of these options the shared import core takes.
    pub fn import_options(&self) -> ImportOptions {
        ImportOptions {
            skip_duplicates: self.skip_duplicates,
            import_anyway_lines: self.import_anyway_lines.clone(),
            category_overrides: self.category_overrides.clone(),
            source_category_map: self.source_category_map.clone(),
            preview_offset: self.preview_offset,
            preview_limit: self.preview_limit,
        }
    }

    /// Validate at the trust boundary against the file the options are for. Keys are
    /// `validation.*` i18n keys.
    pub fn validate(&self, format: StatementFormat, statement_count: usize) -> Result<()> {
        if self.statement_index >= statement_count {
            return Err(AppError::Validation(
                "validation.statementIndexInvalid".into(),
            ));
        }
        if self.date_order.is_some() && format != StatementFormat::Qif {
            return Err(AppError::Validation(
                "validation.statementDateOrderInvalid".into(),
            ));
        }
        self.import_options().validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> StatementImportOptions {
        serde_json::from_str(r#"{"statementIndex": 0}"#).expect("minimal payload")
    }

    #[test]
    fn options_deserialize_with_defaults() {
        let options = options();
        assert!(options.skip_duplicates, "duplicates are skipped by default");
        assert_eq!(options.preview_limit, 200);
        assert!(options.date_order.is_none() && options.import_anyway_lines.is_empty());
        assert!(options.category_overrides.is_empty() && options.source_category_map.is_empty());
    }

    #[test]
    fn date_order_wire_values_are_lowercase() {
        assert_eq!(serde_json::to_string(&DateOrder::Mdy).unwrap(), "\"mdy\"");
        let parsed: StatementImportOptions =
            serde_json::from_str(r#"{"statementIndex": 1, "dateOrder": "ymd"}"#).unwrap();
        assert_eq!(parsed.date_order, Some(DateOrder::Ymd));
    }

    #[test]
    fn validation_uses_i18n_keys() {
        let key = |error: AppError| match error {
            AppError::Validation(key) => key,
            other => panic!("not a validation error: {other}"),
        };
        assert!(options().validate(StatementFormat::Camt053, 1).is_ok());
        assert_eq!(
            key(options().validate(StatementFormat::Camt053, 0).unwrap_err()),
            "validation.statementIndexInvalid"
        );
        let order = StatementImportOptions {
            date_order: Some(DateOrder::Dmy),
            ..options()
        };
        assert_eq!(
            key(order.validate(StatementFormat::Ofx, 1).unwrap_err()),
            "validation.statementDateOrderInvalid"
        );
        assert!(order.validate(StatementFormat::Qif, 1).is_ok());
        let paged = StatementImportOptions {
            preview_limit: 0,
            ..options()
        };
        assert_eq!(
            key(paged.validate(StatementFormat::Camt053, 1).unwrap_err()),
            "validation.csvPreviewRangeInvalid"
        );
    }

    #[test]
    fn import_options_carry_every_choice() {
        let options = StatementImportOptions {
            skip_duplicates: false,
            import_anyway_lines: vec![2],
            category_overrides: vec![CsvCategoryOverride {
                line: 1,
                category_id: "c1".into(),
            }],
            source_category_map: vec![SourceCategoryMapping {
                source: "Food".into(),
                category_id: "c2".into(),
            }],
            preview_offset: 5,
            preview_limit: 50,
            ..options()
        };
        let core = options.import_options();
        assert!(!core.skip_duplicates);
        assert_eq!(core.import_anyway_lines, vec![2]);
        assert_eq!(core.category_overrides, options.category_overrides);
        assert_eq!(core.source_category_map, options.source_category_map);
        assert_eq!((core.preview_offset, core.preview_limit), (5, 50));
    }
}
