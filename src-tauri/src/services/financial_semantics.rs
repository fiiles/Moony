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

/// A domestic account number (sort code + account, routing + account, BSB + account) needs at
/// least this many digits to count as an identity: a bare 8-digit UK or 9-digit US account exists
/// at many banks, the composite does not collide in practice.
pub const MIN_NUMBER_DIGITS: usize = 11;
const MAX_NUMBER_DIGITS: usize = 34;

#[derive(Debug, Clone, Default)]
pub struct AccountIdentities {
    /// Canonical valid IBANs: authoritative.
    ibans: HashSet<String>,
    /// Digits-only keys of stored domestic numbers, see [`account_number_key`].
    numbers: HashSet<String>,
}
impl AccountIdentities {
    pub fn from_identifiers(identifiers: impl IntoIterator<Item = String>) -> Self {
        let mut own = Self::default();
        for raw in identifiers.into_iter().filter(|s| !s.trim().is_empty()) {
            let canonical = canonical_account_identifier(&raw, None);
            if valid_iban(&canonical) {
                own.ibans.insert(canonical);
            } else if let Some(key) = account_number_key(&raw) {
                // Short, Czech-shaped and lettered identifiers are preserved by the
                // canonicalizer but still cannot prove own identity.
                own.numbers.insert(key);
            }
        }
        own
    }
    pub fn load(conn: &Connection) -> Result<Self> {
        Ok(Self::from_identifiers(load_account_identifiers(conn)?))
    }
    pub fn contains(&self, identifier: Option<&str>, bank_code: Option<&str>) -> bool {
        identifier.is_some_and(|id| {
            self.ibans
                .contains(&canonical_account_identifier(id, bank_code))
        })
    }
    /// The counterparty written as a domestic number (whole, or account plus a bank/sort-code/routing
    /// column) is one of the stored numbers. An IBAN, or anything that converts to one, is judged by
    /// the IBAN set alone.
    pub fn contains_number(&self, identifier: Option<&str>, bank_code: Option<&str>) -> bool {
        if self.numbers.is_empty() {
            return false;
        }
        let Some(identifier) = identifier else {
            return false;
        };
        if valid_iban(&canonical_account_identifier(identifier, bank_code)) {
            return false;
        }
        let account = identifier.rsplit(':').next().unwrap_or(identifier);
        if account.chars().any(char::is_alphabetic) {
            return false;
        }
        let digits: String = account.chars().filter(char::is_ascii_digit).collect();
        let bank: String = bank_code
            .unwrap_or_default()
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        self.numbers.contains(&digits) || self.numbers.contains(&format!("{bank}{digits}"))
    }
}

/// A Czech `[prefix-]account[/bank]`: without its bank code (or with one that failed the checksum
/// and so never became an IBAN) it is not an identity.
fn is_czech_domestic_shape(text: &str) -> bool {
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let text = text.trim();
    let text = match text.rsplit_once('/') {
        Some((account, code)) if code.len() == 4 && all_digits(code) => account,
        _ => text,
    };
    match text.split_once('-') {
        Some((prefix, account)) => {
            prefix.len() <= 6 && all_digits(prefix) && all_digits(account) && account.len() <= 10
        }
        None => all_digits(text) && text.len() <= 10,
    }
}

/// Digits-only key of a domestic account number written in any layout (`12-34-56 12345678`,
/// `021000021/123456789`): `None` for letters (an IBAN, a name), a Czech `prefix-account`, fewer
/// than [`MIN_NUMBER_DIGITS`] or more than 34 digits. The `foreign:CC:bank:` marker that
/// [`canonical_structured_identifier`] adds for an institution country is ignored.
pub fn account_number_key(identifier: &str) -> Option<String> {
    let body = identifier.rsplit(':').next().unwrap_or(identifier);
    if body.chars().any(char::is_alphabetic) || is_czech_domestic_shape(body) {
        return None;
    }
    let digits: String = body.chars().filter(char::is_ascii_digit).collect();
    (MIN_NUMBER_DIGITS..=MAX_NUMBER_DIGITS)
        .contains(&digits.len())
        .then_some(digits)
}

/// Preserve each account row's pairing. A reliable IBAN is authoritative; otherwise only a
/// domestic number of at least 11 digits (sort code, routing or BSB plus account) can prove own.
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

/// The accounts a statement's account identifier points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentifierMatch {
    /// The identifier is a valid IBAN; ids of the accounts whose stored iban or bban is that IBAN.
    Iban(Vec<String>),
    /// A domestic number of at least [`MIN_NUMBER_DIGITS`] digits; ids of the accounts storing it.
    Number(Vec<String>),
    /// Neither: a short number, a Czech `prefix-account`, a name or free text.
    Unrecognized,
}

/// Bank account ids whose stored iban/bban names the same account as `identifier`.
pub fn accounts_matching_identifier(
    conn: &Connection,
    identifier: &str,
    bank_code: Option<&str>,
) -> Result<IdentifierMatch> {
    let wanted = canonical_structured_identifier(identifier, bank_code, None);
    let by_iban = valid_iban(&wanted);
    // A routing number plus a 9-digit account is only a key in its joined form.
    let mut keys: Vec<String> = Vec::new();
    if !by_iban {
        let joined = bank_code.map(|code| format!("{}{identifier}", code.trim()));
        for key in [Some(identifier.to_string()), joined]
            .into_iter()
            .flatten()
            .filter_map(|text| account_number_key(&text))
        {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        if keys.is_empty() {
            return Ok(IdentifierMatch::Unrecognized);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT ba.id, ba.iban, ba.bban, i.country FROM bank_accounts ba \
         LEFT JOIN institutions i ON i.id = ba.institution_id ORDER BY ba.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut ids = Vec::new();
    for row in rows {
        let (id, iban, bban, country) = row?;
        let stored = [iban, bban].into_iter().flatten();
        let hit = if by_iban {
            stored.into_iter().any(|raw| {
                canonical_structured_identifier(&raw, None, country.as_deref()) == wanted
            })
        } else {
            stored
                .into_iter()
                .any(|raw| account_number_key(&raw).is_some_and(|key| keys.contains(&key)))
        };
        if hit {
            ids.push(id);
        }
    }
    Ok(if by_iban {
        IdentifierMatch::Iban(ids)
    } else {
        IdentifierMatch::Number(ids)
    })
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
    // A domestic number (sort code, routing, BSB + account) proves an own account like an IBAN
    // does, but an explicit override still wins.
    if explicit.is_none() && own.contains_number(identifier, bank_code) {
        return FlowResolution {
            kind: FlowKind::Transfer,
            source: FlowSource::OwnAccount,
        };
    }
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
    fn matching_identifier_finds_the_account_of_an_iban_through_a_czech_bban() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE institutions(id TEXT, country TEXT);
             CREATE TABLE bank_accounts(id TEXT, iban TEXT, bban TEXT, institution_id TEXT);
             INSERT INTO institutions VALUES ('de', 'DE');
             INSERT INTO bank_accounts VALUES
               ('a-iban', 'cz65 0800 0000 1920 0014 5399', NULL, NULL),
               ('b-bban', NULL, '19-2000145399/0800', NULL),
               ('c-foreign', NULL, '19-2000145399/0800', 'de'),
               ('d-other', 'CZ5508000000001234567899', NULL, NULL),
               ('e-bare', NULL, NULL, NULL);",
        )
        .unwrap();
        assert_eq!(
            accounts_matching_identifier(&conn, "CZ65 0800 0000 1920 0014 5399", None).unwrap(),
            IdentifierMatch::Iban(vec!["a-iban".into(), "b-bban".into()])
        );
        assert_eq!(
            accounts_matching_identifier(&conn, "DE89370400440532013000", None).unwrap(),
            IdentifierMatch::Iban(vec![])
        );
    }

    #[test]
    fn matching_identifier_joins_a_routing_number_with_the_account() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE institutions(id TEXT, country TEXT);
             CREATE TABLE bank_accounts(id TEXT, iban TEXT, bban TEXT, institution_id TEXT);
             INSERT INTO bank_accounts VALUES
               ('us', NULL, '021000021123456789', NULL),
               ('other', NULL, '021000021999999999', NULL);",
        )
        .unwrap();
        assert_eq!(
            accounts_matching_identifier(&conn, "123456789", Some("021000021")).unwrap(),
            IdentifierMatch::Number(vec!["us".into()])
        );
        assert_eq!(
            accounts_matching_identifier(&conn, "021000021123456789", None).unwrap(),
            IdentifierMatch::Number(vec!["us".into()])
        );
        // nine digits alone are no identity
        assert_eq!(
            accounts_matching_identifier(&conn, "123456789", None).unwrap(),
            IdentifierMatch::Unrecognized
        );
        assert_eq!(
            accounts_matching_identifier(&conn, "ACC-77", None).unwrap(),
            IdentifierMatch::Unrecognized
        );
    }
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
        for kind in [FlowKind::Income, FlowKind::Expense, FlowKind::Transfer] {
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

    #[test]
    fn a_uk_sort_code_and_account_number_prove_an_own_transfer() {
        let own = AccountIdentities::from_identifiers(vec!["12-34-56 12345678".into()]);
        let resolve = |id: &str, bank: Option<&str>| {
            resolve_structured_flow(None, Some(id), bank, Some("GB"), None, false, &own)
        };
        // Written whole, or split into the account column and the sort-code column.
        for (id, bank) in [
            ("12-34-56 12345678", None),
            ("12345612345678", None),
            ("12345678", Some("123456")),
            ("12345678", Some("12-34-56")),
        ] {
            let flow = resolve(id, bank);
            assert_eq!(flow.kind, FlowKind::Transfer, "{id} {bank:?}");
            assert_eq!(flow.source, FlowSource::OwnAccount);
        }
        // Another account at the same bank, the same number at another bank, the bare number.
        assert_eq!(resolve("87654321", Some("123456")).kind, FlowKind::Expense);
        assert_eq!(resolve("12345678", Some("654321")).kind, FlowKind::Expense);
        assert_eq!(resolve("12345678", None).kind, FlowKind::Expense);
    }

    #[test]
    fn a_us_routing_and_account_number_prove_an_own_transfer() {
        let own = AccountIdentities::from_identifiers(vec!["021000021 123456789012".into()]);
        let flow = resolve_structured_flow(
            None,
            Some("123456789012"),
            Some("021000021"),
            Some("US"),
            None,
            true,
            &own,
        );
        assert_eq!(
            (flow.kind, flow.source),
            (FlowKind::Transfer, FlowSource::OwnAccount)
        );
    }

    #[test]
    fn short_czech_shaped_and_alphabetic_numbers_never_prove_an_own_transfer() {
        let own = AccountIdentities::from_identifiers(vec![
            "12345678".into(),               // a bare UK account exists at many banks
            "19-2000145399".into(),          // Czech prefix-account without its bank code
            "GB-ACCOUNT 12345678901".into(), // letters: not a number at all
            "19-2000145398/0800".into(),     // Czech with a bank code but a bad checksum: no IBAN
        ]);
        for (id, bank) in [
            ("12345678", None),
            ("12345678", Some("123456")),
            ("19-2000145399", None),
            ("19-2000145399", Some("0800")),
            ("12345678901", None),
            ("19-2000145398/0800", None),
        ] {
            let flow = resolve_structured_flow(None, Some(id), bank, None, None, false, &own);
            assert_eq!(flow.kind, FlowKind::Expense, "{id} {bank:?}");
        }
    }

    #[test]
    fn an_explicit_flow_and_the_iban_path_are_not_overruled_by_a_number_match() {
        let own = AccountIdentities::from_identifiers(vec![
            "12-34-56 12345678".into(),
            "DE89370400440532013000".into(),
        ]);
        let explicit = resolve_structured_flow(
            Some(FlowKind::Income),
            Some("12-34-56 12345678"),
            None,
            None,
            None,
            true,
            &own,
        );
        assert_eq!(
            (explicit.kind, explicit.source),
            (FlowKind::Income, FlowSource::Explicit)
        );
        // An IBAN is judged by the IBAN set alone.
        let stranger = resolve_structured_flow(
            None,
            Some("DE89370400440532013001"),
            None,
            None,
            None,
            false,
            &own,
        );
        assert_eq!(stranger.kind, FlowKind::Expense);
        let mine = resolve_structured_flow(
            None,
            Some("DE89 3704 0044 0532 0130 00"),
            None,
            None,
            None,
            false,
            &own,
        );
        assert_eq!(mine.source, FlowSource::OwnAccount);
    }

    #[test]
    fn stored_domestic_numbers_load_with_and_without_an_institution_country() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE institutions(id TEXT,country TEXT);
             CREATE TABLE bank_accounts(iban TEXT,bban TEXT,institution_id TEXT);
             INSERT INTO institutions VALUES ('gb','GB');
             INSERT INTO bank_accounts VALUES (NULL,'12-34-56 12345678',NULL);
             INSERT INTO bank_accounts VALUES (NULL,'040004 87654321','gb');",
        )
        .unwrap();
        let own = AccountIdentities::load(&conn).unwrap();
        assert!(own.contains_number(Some("12345678"), Some("123456")));
        assert!(own.contains_number(Some("87654321"), Some("040004")));
        assert!(!own.contains_number(Some("87654321"), Some("123456")));
    }
}
