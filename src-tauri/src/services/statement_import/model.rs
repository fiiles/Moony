//! The neutral statement model every format parser fills in, and two helpers the
//! parsers share.

use serde::{Deserialize, Serialize};
use specta::Type;

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

impl StatementFormat {
    /// The name a person knows the format by.
    pub fn label(self) -> &'static str {
        match self {
            StatementFormat::Camt053 => "CAMT.053",
            StatementFormat::Mt940 => "MT940",
            StatementFormat::Ofx => "OFX",
            StatementFormat::Qif => "QIF",
        }
    }
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
    fn labels_are_the_names_people_know() {
        assert_eq!(StatementFormat::Camt053.label(), "CAMT.053");
        assert_eq!(StatementFormat::Mt940.label(), "MT940");
        assert_eq!(StatementFormat::Ofx.label(), "OFX");
        assert_eq!(StatementFormat::Qif.label(), "QIF");
    }

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
    fn join_description_trims_and_drops_empty_parts() {
        assert_eq!(
            join_description(["  Rent ", "", "  ", "May"]),
            Some("Rent | May".to_string())
        );
        assert_eq!(join_description(["", "   "]), None);
        assert_eq!(join_description(std::iter::empty::<&str>()), None);
    }
}
