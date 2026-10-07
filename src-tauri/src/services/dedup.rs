//! Duplicate detection for bulk imports — MCP write tools and the UI CSV
//! import (spec 2026-08-12, D4).
//!
//! Conservative matching on purpose: a false "duplicate" that silently skips a real
//! payment is worse than an occasional visible double. Amount/quantity/price
//! remain TEXT; bank keys canonicalize decimal spelling without rounding.

use crate::error::Result;
use rusqlite::{Connection, OptionalExtension};

/// Snapshot of existing occurrences: newly accepted ID-less rows are genuine
/// occurrences, while reimports consume only stored matches one at a time.
#[derive(Debug)]
pub struct BankOccurrenceChecker {
    stored: Vec<StoredOccurrence>,
    seen_ids: std::collections::HashSet<String>,
    currency: String,
}
#[derive(Debug)]
struct StoredOccurrence {
    id: String,
    bank_id: Option<String>,
    status: String,
    key: String,
    consumed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankOccurrence {
    Certain,
    Possible,
    Promote(String),
}
/// Existing imports may contain padded or whitespace-only bank IDs. Compare
/// those with the same normalization as new inputs without rewriting stored IDs.
pub(crate) fn normalized_bank_id(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|id| !id.is_empty())
}

pub(crate) fn canonical_amount(text: &str) -> String {
    let text = text.trim();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, 0), |(m, e)| (m, e.parse::<i32>().unwrap_or(0)));
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches(['-', '+']);
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if !integer
        .bytes()
        .chain(fraction.bytes())
        .all(|b| b.is_ascii_digit())
        || exponent.unsigned_abs() > 1000
    {
        return text.into();
    }
    let digits = format!("{integer}{fraction}");
    let position = integer.len() as i32 + exponent;
    let expanded = if position <= 0 {
        format!("0.{}{digits}", "0".repeat((-position) as usize))
    } else if position as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(position as usize - digits.len()))
    } else {
        format!(
            "{}.{}",
            &digits[..position as usize],
            &digits[position as usize..]
        )
    };
    let (i, f) = expanded.split_once('.').unwrap_or((&expanded, ""));
    let i = i.trim_start_matches('0');
    let i = if i.is_empty() { "0" } else { i };
    let f = f.trim_end_matches('0');
    let sign = if negative && (i != "0" || !f.is_empty()) {
        "-"
    } else {
        ""
    };
    if f.is_empty() {
        format!("{sign}{i}")
    } else {
        format!("{sign}{i}.{f}")
    }
}
fn composite_key(t: &crate::models::InsertBankTransaction, currency: &str) -> String {
    let payee = t
        .counterparty_name
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let identifier = t.counterparty_iban.as_deref().map(|s| {
        let canonical = super::financial_semantics::canonical_structured_identifier(
            s,
            t.counterparty_bank_code.as_deref(),
            t.counterparty_country.as_deref(),
        );
        if super::financial_semantics::valid_iban(&canonical) {
            serde_json::json!([canonical])
        } else {
            serde_json::json!([canonical, t.counterparty_bank_code, t.counterparty_country])
        }
    });
    // JSON tuples avoid delimiter collisions in descriptions and identifiers.
    serde_json::json!([
        t.booking_date,
        t.value_date,
        canonical_amount(&t.amount),
        t.currency
            .as_deref()
            .unwrap_or(currency)
            .to_ascii_uppercase(),
        t.tx_type.to_lowercase(),
        t.description,
        payee,
        identifier,
        t.status.as_deref().unwrap_or("booked").to_lowercase()
    ])
    .to_string()
}
impl BankOccurrenceChecker {
    pub fn load(conn: &Connection, account: &str) -> Result<Self> {
        let currency: String = conn.query_row(
            "SELECT currency FROM bank_accounts WHERE id=?1",
            [account],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare("SELECT id,transaction_id,booking_date,value_date,amount,currency,tx_type,description,counterparty_name,counterparty_iban,counterparty_bank_code,counterparty_country,status FROM bank_transactions WHERE bank_account_id=?1 ORDER BY id")?;
        let rows = stmt.query_map([account], |r| {
            let data = crate::models::InsertBankTransaction {
                bank_account_id: account.into(),
                transaction_id: r.get(1)?,
                booking_date: r.get(2)?,
                value_date: r.get(3)?,
                amount: r.get(4)?,
                currency: r.get(5)?,
                tx_type: r.get(6)?,
                description: r.get(7)?,
                counterparty_name: r.get(8)?,
                counterparty_iban: r.get(9)?,
                counterparty_bank_code: r.get(10)?,
                counterparty_country: r.get(11)?,
                status: r.get(12)?,
                category_id: None,
                flow_kind: None,
            };
            Ok(StoredOccurrence {
                id: r.get(0)?,
                bank_id: normalized_bank_id(data.transaction_id.as_deref()).map(str::to_string),
                status: data.status.as_deref().unwrap_or("booked").to_lowercase(),
                key: composite_key(&data, &currency),
                consumed: false,
            })
        })?;
        Ok(Self {
            stored: rows.collect::<std::result::Result<Vec<_>, _>>()?,
            seen_ids: Default::default(),
            currency,
        })
    }
    pub fn record_written(&mut self, t: &crate::models::InsertBankTransaction, id: &str) {
        if normalized_bank_id(t.transaction_id.as_deref()).is_some()
            && t.status
                .as_deref()
                .is_some_and(|status| status.eq_ignore_ascii_case("pending"))
        {
            self.stored.push(StoredOccurrence {
                id: id.into(),
                bank_id: normalized_bank_id(t.transaction_id.as_deref()).map(str::to_string),
                status: "pending".into(),
                key: composite_key(t, &self.currency),
                consumed: true,
            });
        }
    }
    pub fn currency(&self) -> &str {
        &self.currency
    }
    pub fn check(&mut self, t: &crate::models::InsertBankTransaction) -> Option<BankOccurrence> {
        let bank_id = normalized_bank_id(t.transaction_id.as_deref());
        if let Some(id) = bank_id {
            let seen = !self.seen_ids.insert(id.to_string());
            if let Some(stored) = self
                .stored
                .iter_mut()
                .find(|s| s.bank_id.as_deref() == Some(id))
            {
                stored.consumed = true;
                if stored.status == "pending"
                    && t.status
                        .as_deref()
                        .unwrap_or("booked")
                        .eq_ignore_ascii_case("booked")
                {
                    stored.status = "booked".into();
                    return Some(BankOccurrence::Promote(stored.id.clone()));
                }
                return Some(BankOccurrence::Certain);
            }
            if seen {
                return Some(BankOccurrence::Certain);
            }
        }
        let key = composite_key(t, &self.currency);
        if let Some(stored) = self
            .stored
            .iter_mut()
            .find(|s| !s.consumed && s.key == key && (bank_id.is_none() || s.bank_id.is_none()))
        {
            stored.consumed = true;
            return Some(BankOccurrence::Possible);
        }
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn find_duplicate_in_tx_table(
    conn: &Connection,
    table: &str,
    scope_col: &str,
    scope_val: &str,
    transaction_date: i64,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
) -> Result<Option<String>> {
    let sql = format!(
        "SELECT id FROM {table}
         WHERE {scope_col} = ?1 AND transaction_date = ?2 AND type = ?3
           AND quantity = ?4 AND price_per_unit = ?5"
    );
    let hit: Option<String> = conn
        .query_row(
            &sql,
            rusqlite::params![
                scope_val,
                transaction_date,
                tx_type,
                quantity,
                price_per_unit
            ],
            |row| row.get(0),
        )
        .optional()?;
    Ok(hit.map(|_| {
        "identical transaction (same date, type, quantity, price) already exists".to_string()
    }))
}

/// Exact stock duplicate lookup.
pub fn find_duplicate_stock_transaction(
    conn: &Connection,
    ticker_upper: &str,
    transaction_date: i64,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
) -> Result<Option<String>> {
    find_duplicate_in_tx_table(
        conn,
        "investment_transactions",
        "ticker",
        ticker_upper,
        transaction_date,
        tx_type,
        quantity,
        price_per_unit,
    )
}

/// Crypto transactions: exact (ticker, date, type, quantity, price) match.
pub fn find_duplicate_crypto_transaction(
    conn: &Connection,
    ticker_upper: &str,
    transaction_date: i64,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
) -> Result<Option<String>> {
    find_duplicate_in_tx_table(
        conn,
        "crypto_transactions",
        "ticker",
        ticker_upper,
        transaction_date,
        tx_type,
        quantity,
        price_per_unit,
    )
}

/// Other-asset transactions: exact (asset, date, type, quantity, price) match.
pub fn find_duplicate_other_asset_transaction(
    conn: &Connection,
    asset_id: &str,
    transaction_date: i64,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
) -> Result<Option<String>> {
    find_duplicate_in_tx_table(
        conn,
        "other_asset_transactions",
        "asset_id",
        asset_id,
        transaction_date,
        tx_type,
        quantity,
        price_per_unit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                transaction_id TEXT,
                tx_type TEXT NOT NULL,
                amount TEXT NOT NULL,
                description TEXT,
                booking_date INTEGER NOT NULL,
                value_date INTEGER
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
        "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn review_round1_checker_normalizes_legacy_ids_and_whitespace_only_is_absent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE bank_accounts(id TEXT,currency TEXT);
            INSERT INTO bank_accounts VALUES ('acc-1','CZK');
            CREATE TABLE bank_transactions(id TEXT,bank_account_id TEXT,transaction_id TEXT,
                booking_date INTEGER,value_date INTEGER,amount TEXT,currency TEXT,tx_type TEXT,
                description TEXT,counterparty_name TEXT,counterparty_iban TEXT,
                counterparty_bank_code TEXT,counterparty_country TEXT,status TEXT);
            INSERT INTO bank_transactions(id,bank_account_id,transaction_id,booking_date,amount,currency,tx_type,description,status) VALUES
                ('padded','acc-1',' FIO-123 ',1767225600,'99','CZK','debit','coffee','booked'),
                ('blank','acc-1',' \t ',1767225600,'1','CZK','debit','blank','booked');").unwrap();
        let mut checker = BankOccurrenceChecker::load(&conn, "acc-1").unwrap();
        let mut row: crate::models::InsertBankTransaction =
            serde_json::from_value(serde_json::json!({
                "bankAccountId":"acc-1","transactionId":"\tFIO-123\n", "bookingDate":1767225600,
                "amount":"99","currency":"CZK","type":"debit","description":"coffee"
            }))
            .unwrap();
        assert_eq!(checker.check(&row), Some(BankOccurrence::Certain));
        row.transaction_id = Some("new-id".into());
        row.amount = "1".into();
        row.description = Some("blank".into());
        assert_eq!(checker.check(&row), Some(BankOccurrence::Possible));
        assert_eq!(checker.check(&row), Some(BankOccurrence::Certain));
    }

    #[test]
    fn stock_exact_match() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO investment_transactions (id, ticker, type, quantity, price_per_unit, transaction_date)
             VALUES ('s1', 'AAPL', 'buy', '10', '150.5', 1700000000)",
            [],
        )
        .unwrap();
        assert!(
            find_duplicate_stock_transaction(&conn, "AAPL", 1700000000, "buy", "10", "150.5")
                .unwrap()
                .is_some()
        );
        assert!(
            find_duplicate_stock_transaction(&conn, "AAPL", 1700000000, "sell", "10", "150.5")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn crypto_exact_match() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO crypto_transactions (id, ticker, type, quantity, price_per_unit, transaction_date)
             VALUES ('c1', 'BTC', 'buy', '0.5', '900000', 1700000000)",
            [],
        )
        .unwrap();
        assert!(find_duplicate_crypto_transaction(
            &conn, "BTC", 1700000000, "buy", "0.5", "900000"
        )
        .unwrap()
        .is_some());
        assert!(find_duplicate_crypto_transaction(
            &conn, "BTC", 1700000001, "buy", "0.5", "900000"
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn other_asset_exact_match_scoped_to_asset() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO other_asset_transactions (id, asset_id, type, quantity, price_per_unit, transaction_date)
             VALUES ('o1', 'gold-1', 'buy', '2', '5000', 1700000000)",
            [],
        )
        .unwrap();
        assert!(find_duplicate_other_asset_transaction(
            &conn, "gold-1", 1700000000, "buy", "2", "5000"
        )
        .unwrap()
        .is_some());
        assert!(find_duplicate_other_asset_transaction(
            &conn, "silver-9", 1700000000, "buy", "2", "5000"
        )
        .unwrap()
        .is_none());
    }
}
