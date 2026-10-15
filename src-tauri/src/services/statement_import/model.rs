//! The neutral statement model every format parser fills in, and the helpers the
//! parsers share.

use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;

use crate::error::AppError;
use crate::services::csv_import::DeclaredBalance;

/// The statement formats Moony reads (wire value in the lowercase rename).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StatementFormat {
    #[serde(rename = "camt053")]
    Camt053,
    #[serde(rename = "mt940")]
    Mt940,
    #[serde(rename = "ofx")]
    Ofx,
    #[serde(rename = "qif")]
    Qif,
}

/// How a statement names the account it belongs to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountRef {
    /// IBAN or a local account number, as written in the file.
    pub identifier: Option<String>,
    pub bank_code: Option<String>,
    pub name: Option<String>,
}

/// One statement of a file (a file can hold several).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Statement {
    pub account: AccountRef,
    pub currency: Option<String>,
    /// Declared period, midnight-UTC seconds, inclusive.
    pub period: Option<(i64, i64)>,
    pub opening: Option<DeclaredBalance>,
    pub closing: Option<DeclaredBalance>,
    /// Statement or page number, used to order the statements of one file.
    pub sequence: Option<String>,
    pub entries: Vec<Entry>,
}

/// One booked (or pending) movement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub booking_date: i64,
    pub value_date: Option<i64>,
    /// Unsigned canonical decimal text; the direction is `is_credit`.
    pub amount: String,
    pub is_credit: bool,
    /// `None` = the statement's currency.
    pub currency: Option<String>,
    /// The bank's own id for the entry, when the format has one.
    pub bank_id: Option<String>,
    pub description: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_account: Option<String>,
    pub counterparty_bank_code: Option<String>,
    pub counterparty_country: Option<String>,
    pub pending: bool,
    /// The category the file itself names (QIF `L` line).
    pub source_category: Option<String>,
    /// R2: row message key of an entry that is shown but never imported.
    pub skip_reason: Option<&'static str>,
}

/// Midnight UTC of a calendar day, in unix seconds (ADR 0008: the day as written
/// in the file, never shifted by a time zone).
pub fn utc_day(date: chrono::NaiveDate) -> i64 {
    date.and_time(chrono::NaiveTime::MIN).and_utc().timestamp()
}

/// Longest `detail` of a malformed-statement error, in characters. Element names, entity
/// names and the like come from the file and can be megabytes long.
const MAX_DETAIL_CHARS: usize = 64;

/// `validation.statementMalformed` with `detail` (the element, field or entry that cannot be
/// read), cut to [`MAX_DETAIL_CHARS`] characters.
pub(crate) fn malformed(detail: &str) -> AppError {
    let detail: String = detail.chars().take(MAX_DETAIL_CHARS).collect();
    AppError::validation_with("validation.statementMalformed", json!({ "detail": detail }))
}

/// Joins the non-empty parts with " | " (the CSV multi-column separator); None when all are empty.
pub fn join_description<'a>(parts: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let parts: Vec<&str> = parts
        .into_iter()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_wire_values_are_lowercase() {
        assert_eq!(
            serde_json::to_string(&StatementFormat::Camt053).unwrap(),
            "\"camt053\""
        );
        assert_eq!(
            serde_json::from_str::<StatementFormat>("\"qif\"").unwrap(),
            StatementFormat::Qif
        );
    }

    #[test]
    fn utc_day_is_midnight_utc() {
        let day = chrono::NaiveDate::from_ymd_opt(2024, 3, 5).unwrap();
        assert_eq!(utc_day(day), 19_787 * 86_400);
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        assert_eq!(utc_day(epoch), 0);
    }

    #[test]
    fn malformed_names_its_detail_and_cuts_a_long_one() {
        let (key, params) = malformed("Ntry 3").keyed().unwrap();
        assert_eq!(key, "validation.statementMalformed");
        assert_eq!(params, serde_json::json!({ "detail": "Ntry 3" }));
        // names and details come from the file: cut to 64 characters, on a character boundary
        let (_, params) = malformed(&"é".repeat(10_000)).keyed().unwrap();
        assert_eq!(params["detail"], "é".repeat(64));
    }

    #[test]
    fn join_description_trims_and_drops_empty_parts() {
        assert_eq!(
            join_description(["  Rent ", "", "  ", "May"]),
            Some("Rent | May".to_string())
        );
        assert_eq!(join_description(["", "   "]), None);
        assert_eq!(join_description(std::iter::empty::<&str>()), None);
    }
}
