//! Shared account identity and financial meaning for categorization and reports.

use crate::error::Result;
use crate::models::FlowKind;
use rusqlite::Connection;
use std::collections::HashSet;

pub use crate::models::reporting::FlowSource;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowResolution {
    pub kind: FlowKind,
    pub source: FlowSource,
}

#[derive(Debug, Clone, Default)]
pub struct AccountIdentities(HashSet<String>);
impl AccountIdentities {
    pub fn from_identifiers(identifiers: impl IntoIterator<Item = String>) -> Self {
        Self(
            identifiers
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| canonical_account_identifier(&s, None))
                // Bare domestic numbers and opaque foreign routing values are
                // preserved by the canonicalizer, but cannot prove own identity.
                .filter(|s| valid_iban(s))
                .collect(),
        )
    }
    pub fn load(conn: &Connection) -> Result<Self> {
        Ok(Self::from_identifiers(load_account_identifiers(conn)?))
    }
    pub fn contains(&self, identifier: Option<&str>, bank_code: Option<&str>) -> bool {
        identifier.is_some_and(|id| {
            self.0
                .contains(&canonical_account_identifier(id, bank_code))
        })
    }
}

/// Preserve each account row's pairing. A reliable IBAN is authoritative;
/// otherwise only safely interpretable domestic/routing metadata can prove own.
pub(crate) fn load_account_identifiers(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt=conn.prepare("SELECT ba.iban,ba.bban,i.country FROM bank_accounts ba LEFT JOIN institutions i ON i.id=ba.institution_id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = vec![];
    for row in rows {
        let (iban, bban, country) = row?;
        let canonical = iban
            .as_deref()
            .map(|id| canonical_structured_identifier(id, None, country.as_deref()));
        if let Some(id) = canonical.as_ref().filter(|id| valid_iban(id)) {
            out.push(id.clone());
            continue;
        }
        for identifier in [iban, bban]
            .into_iter()
            .flatten()
            .filter(|s| !s.trim().is_empty())
        {
            out.push(canonical_structured_identifier(
                &identifier,
                None,
                country.as_deref(),
            ));
        }
    }
    Ok(out)
}

/// A valid IBAN is authoritative; known foreign domestic/routing metadata
/// cannot be interpreted as Czech, even when its digits happen to match.
pub fn canonical_structured_identifier(
    identifier: &str,
    bank_code: Option<&str>,
    country: Option<&str>,
) -> String {
    let compact: String = identifier
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    if valid_iban(&compact) {
        return compact;
    }
    if country.is_some_and(|c| !c.trim().is_empty() && !c.eq_ignore_ascii_case("CZ")) {
        return format!(
            "foreign:{}:{}:{}",
            country.unwrap_or_default().to_ascii_uppercase(),
            bank_code.unwrap_or_default(),
            identifier
        );
    }
    canonical_account_identifier(identifier, bank_code)
}

pub fn resolve_structured_flow(
    explicit: Option<FlowKind>,
    identifier: Option<&str>,
    bank_code: Option<&str>,
    country: Option<&str>,
    category: Option<FlowKind>,
    is_credit: bool,
    own: &AccountIdentities,
) -> FlowResolution {
    let canonical = identifier.map(|s| canonical_structured_identifier(s, bank_code, country));
    resolve_flow(
        explicit,
        canonical.as_deref(),
        None,
        category,
        is_credit,
        own,
    )
}

/// Unknown/non-IBAN routing identifiers are retained verbatim.
pub fn canonical_account_identifier(identifier: &str, bank_code: Option<&str>) -> String {
    let compact: String = identifier
        .chars()
        .filter(|c| {
            !c.is_whitespace() && !matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
        })
        .flat_map(char::to_uppercase)
        .collect();
    // Only normalize actual IBANs; foreign domestic/routing formats stay verbatim.
    if valid_iban(&compact) {
        return compact;
    }
    let (domestic, bank) = match compact.split_once('/') {
        Some((account, code)) => (account, code),
        None => match bank_code {
            Some(code) => (compact.as_str(), code.trim()),
            None => return identifier.to_string(),
        },
    };
    if bank.len() != 4 || !bank.bytes().all(|b| b.is_ascii_digit()) {
        return identifier.to_string();
    }
    let (prefix, account) = domestic.split_once('-').unwrap_or(("0", domestic));
    if prefix.is_empty()
        || prefix.len() > 6
        || account.is_empty()
        || account.len() > 10
        || !prefix
            .bytes()
            .chain(account.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return identifier.to_string();
    }
    let prefix = format!("{prefix:0>6}");
    let account = format!("{account:0>10}");
    let checksum = |value: &str, weights: &[u32]| {
        value
            .bytes()
            .zip(weights)
            .map(|(b, w)| u32::from(b - b'0') * w)
            .sum::<u32>()
            % 11
            == 0
    };
    if !checksum(&prefix, &[10, 5, 8, 4, 2, 1])
        || !checksum(&account, &[6, 3, 7, 9, 10, 5, 8, 4, 2, 1])
        || account == "0000000000"
    {
        return identifier.to_string();
    }
    let bban = format!("{bank}{prefix}{account}");
    let check = 98 - mod97(&format!("{bban}123500"));
    format!("CZ{check:02}{bban}")
}

fn mod97(digits: &str) -> u32 {
    digits
        .bytes()
        .fold(0, |r, b| (r * 10 + u32::from(b - b'0')) % 97)
}
pub fn valid_iban(iban: &str) -> bool {
    if !(15..=34).contains(&iban.len()) || !iban.is_ascii() {
        return false;
    }
    // SWIFT IBAN Registry release 103, September 2026.
    // https://www.swift.com/swift-resource/9606/download (reviewed 2026-10-07).
    let expected_length = match &iban[..2] {
        "AD" => 24,
        "AE" => 23,
        "AL" => 28,
        "AT" => 20,
        "AZ" => 28,
        "BA" => 20,
        "BE" => 16,
        "BG" => 22,
        "BH" => 22,
        "BI" => 27,
        "BR" => 29,
        "BY" => 28,
        "CH" => 21,
        "CR" => 22,
        "CY" => 28,
        "CZ" => 24,
        "DE" => 22,
        "DJ" => 27,
        "DK" => 18,
        "DO" => 28,
        "EE" => 20,
        "EG" => 29,
        "ES" => 24,
        "FI" => 18,
        "FK" => 18,
        "FO" => 18,
        "FR" => 27,
        "GB" => 22,
        "GE" => 22,
        "GI" => 23,
        "GL" => 18,
        "GR" => 27,
        "GT" => 28,
        "HN" => 28,
        "HR" => 21,
        "HU" => 28,
        "IE" => 22,
        "IL" => 23,
        "IQ" => 23,
        "IS" => 26,
        "IT" => 27,
        "JO" => 30,
        "KW" => 30,
        "KZ" => 20,
        "LB" => 28,
        "LC" => 32,
        "LI" => 21,
        "LT" => 20,
        "LU" => 20,
        "LV" => 21,
        "LY" => 25,
        "MC" => 27,
        "MD" => 24,
        "ME" => 22,
        "MK" => 19,
        "MN" => 20,
        "MR" => 27,
        "MT" => 31,
        "MU" => 30,
        "NI" => 28,
        "NL" => 18,
        "NO" => 15,
        "OM" => 23,
        "PK" => 24,
        "PL" => 28,
        "PS" => 29,
        "PT" => 25,
        "QA" => 29,
        "RO" => 24,
        "RS" => 22,
        "RU" => 33,
        "SA" => 24,
        "SC" => 31,
        "SD" => 18,
        "SE" => 24,
        "SI" => 19,
        "SK" => 24,
        "SM" => 27,
        "SO" => 23,
        "ST" => 25,
        "SV" => 28,
        "TL" => 23,
        "TN" => 24,
        "TR" => 26,
        "UA" => 29,
        "VA" => 22,
        "VG" => 24,
        "XK" => 20,
        "YE" => 30,
        _ => return false,
    };
    if iban.len() != expected_length {
        return false;
    }
    let bytes = iban.as_bytes();
    if !bytes[..2].iter().all(u8::is_ascii_uppercase)
        || !bytes[2..4].iter().all(u8::is_ascii_digit)
        || !bytes[4..].iter().all(u8::is_ascii_alphanumeric)
    {
        return false;
    }
    let mut digits = String::new();
    for b in bytes[4..].iter().chain(bytes[..4].iter()) {
        if b.is_ascii_digit() {
            digits.push(char::from(*b));
        } else {
            digits.push_str(&(u32::from(*b - b'A') + 10).to_string());
        }
    }
    mod97(&digits) == 1
}

/// Explicit override > known own account > category role > credit/debit fallback.
pub fn resolve_flow(
    explicit: Option<FlowKind>,
    identifier: Option<&str>,
    bank_code: Option<&str>,
    category: Option<FlowKind>,
    is_credit: bool,
    own: &AccountIdentities,
) -> FlowResolution {
    if let Some(kind) = explicit {
        return FlowResolution {
            kind,
            source: FlowSource::Explicit,
        };
    }
    if own.contains(identifier, bank_code) {
        return FlowResolution {
            kind: FlowKind::Transfer,
            source: FlowSource::OwnAccount,
        };
    }
    if let Some(kind) = category {
        return FlowResolution {
            kind,
            source: FlowSource::Category,
        };
    }
    FlowResolution {
        kind: if is_credit {
            FlowKind::Income
        } else {
            FlowKind::Expense
        },
        source: FlowSource::Direction,
    }
}
/// Expenses use debit-positive amounts; other roles use credit-positive amounts.
pub fn signed_flow_amount(kind: FlowKind, is_credit: bool, amount: f64) -> f64 {
    if is_credit == (kind != FlowKind::Expense) {
        amount
    } else {
        -amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integrity_own_foreign_institution_country_blocks_czech_domestic_shape() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE institutions(id TEXT,country TEXT); CREATE TABLE bank_accounts(iban TEXT,bban TEXT,institution_id TEXT); INSERT INTO institutions VALUES ('foreign','DE'); INSERT INTO bank_accounts VALUES (NULL,'19-2000145399/0800','foreign');").unwrap();
        let own = AccountIdentities::load(&conn).unwrap();
        assert!(!own.contains(Some("CZ6508000000192000145399"), None));
    }

    #[test]
    fn integrity_foreign_country_routing_and_registered_extreme_lengths() {
        let own = AccountIdentities::from_identifiers(vec!["19-2000145399/0800".into()]);
        assert_eq!(
            resolve_structured_flow(
                None,
                Some("19-2000145399"),
                Some("0800"),
                Some("CZ"),
                None,
                false,
                &own
            )
            .kind,
            FlowKind::Transfer
        );
        assert_eq!(
            resolve_structured_flow(
                None,
                Some("19-2000145399"),
                Some("0800"),
                Some("DE"),
                None,
                false,
                &own
            )
            .kind,
            FlowKind::Expense
        );
        assert_eq!(
            resolve_structured_flow(
                Some(FlowKind::Income),
                Some("19-2000145399"),
                Some("0800"),
                Some("CZ"),
                None,
                false,
                &own
            )
            .kind,
            FlowKind::Income
        );
        assert_eq!(
            resolve_structured_flow(
                None,
                Some("CZ6508000000192000145399"),
                None,
                Some("DE"),
                None,
                false,
                &own
            )
            .kind,
            FlowKind::Transfer
        );
        for iban in ["NO9386011117947", "RU0304452522540817810538091310419"] {
            assert!(valid_iban(iban));
            assert!(
                AccountIdentities::from_identifiers(vec![iban.into()]).contains(Some(iban), None)
            );
        }
    }

    #[test]
    fn integrity_rejects_checksum_valid_wrong_national_length() {
        let own = AccountIdentities::from_identifiers(vec!["CZ7900000000000".into()]);
        assert!(!own.contains(Some("CZ7900000000000"), None));
    }

    #[test]
    fn canonical_identity_equates_czech_domestic_and_iban_with_bank_code() {
        let expected = "CZ6508000000192000145399";
        assert_eq!(
            canonical_account_identifier("CZ65 0800 0000 1920 0014 5399", None),
            expected
        );
        assert_eq!(
            canonical_account_identifier("19-2000145399/0800", None),
            expected
        );
        assert_eq!(
            canonical_account_identifier("19-2000145399", Some("0800")),
            expected
        );
        assert_ne!(
            canonical_account_identifier("19-2000145399/0100", None),
            expected
        );
        assert_ne!(
            canonical_account_identifier("19-2000145399", None),
            expected
        );
    }
    #[test]
    fn incomplete_own_bban_never_proves_an_own_transfer() {
        let own =
            AccountIdentities::from_identifiers(vec!["123456789".into(), "2000145399".into()]);
        assert!(!own.contains(Some("123456789"), None));
        assert!(!own.contains(Some("2000145399"), Some("0800")));
        assert_eq!(
            resolve_flow(
                None,
                Some("123456789"),
                None,
                Some(FlowKind::Income),
                true,
                &own
            )
            .kind,
            FlowKind::Income
        );
        let engine = crate::services::categorization::CategorizationEngine::new_empty();
        engine.set_own_ibans(vec!["123456789".into()]);
        let tx = crate::services::categorization::types::TransactionInput {
            id: "t".into(),
            description: None,
            counterparty: None,
            counterparty_iban: Some("123456789".into()),
            counterparty_bank_code: None,
            counterparty_country: None,
            amount: 100.0,
            is_credit: true,
            bank_account_id: None,
        };
        assert!(!engine.categorize(&tx).has_category());
    }

    #[test]
    fn foreign_routing_identifiers_are_preserved_verbatim() {
        for id in [
            "sort 12-34-56 account 1234",
            "US account: abc",
            "invalid IBAN 123",
        ] {
            assert_eq!(canonical_account_identifier(id, None), id);
        }
    }
    #[test]
    fn explicit_override_beats_dynamic_own_account_detection() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE bank_accounts (iban TEXT, bban TEXT, institution_id TEXT); CREATE TABLE institutions(id TEXT,country TEXT);")
            .unwrap();
        let derived = |explicit, own: &AccountIdentities| {
            resolve_flow(
                explicit,
                Some("19-2000145399/0800"),
                None,
                Some(FlowKind::Income),
                true,
                own,
            )
        };
        assert_eq!(
            derived(None, &AccountIdentities::load(&conn).unwrap()).kind,
            FlowKind::Income
        );
        conn.execute(
            "INSERT INTO bank_accounts(iban,bban) VALUES ('CZ6508000000192000145399',NULL)",
            [],
        )
        .unwrap();
        let own = AccountIdentities::load(&conn).unwrap();
        assert_eq!(
            derived(None, &own),
            FlowResolution {
                kind: FlowKind::Transfer,
                source: FlowSource::OwnAccount
            }
        );
        assert_eq!(
            derived(Some(FlowKind::Income), &own),
            FlowResolution {
                kind: FlowKind::Income,
                source: FlowSource::Explicit
            }
        );
        conn.execute("DELETE FROM bank_accounts", []).unwrap();
        assert_eq!(
            derived(None, &AccountIdentities::load(&conn).unwrap()).kind,
            FlowKind::Income
        );
    }
    #[test]
    fn custom_roles_refunds_and_income_reversals_preserve_sign() {
        let own = AccountIdentities::default();
        assert_eq!(
            resolve_flow(None, None, None, Some(FlowKind::Income), false, &own).kind,
            FlowKind::Income
        );
        assert_eq!(signed_flow_amount(FlowKind::Income, false, 100.0), -100.0);
        assert_eq!(signed_flow_amount(FlowKind::Expense, true, 100.0), -100.0);
        for kind in [
            FlowKind::Investment,
            FlowKind::Financing,
            FlowKind::Adjustment,
            FlowKind::Transfer,
        ] {
            assert_eq!(
                resolve_flow(None, None, None, Some(kind), false, &own).kind,
                kind
            );
        }
    }
    #[test]
    fn own_bban_is_loaded_and_no_personal_name_or_equal_amount_is_used() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE bank_accounts (iban TEXT,bban TEXT,institution_id TEXT); CREATE TABLE institutions(id TEXT,country TEXT); INSERT INTO bank_accounts(iban,bban) VALUES (NULL,'19-2000145399/0800');").unwrap();
        let own = AccountIdentities::load(&conn).unwrap();
        assert!(own.contains(Some("CZ6508000000192000145399"), None));
        assert_eq!(
            resolve_flow(None, None, None, None, true, &own).kind,
            FlowKind::Income
        );
    }
}
