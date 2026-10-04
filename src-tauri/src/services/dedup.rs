//! Duplicate detection for bulk imports — MCP write tools and the UI CSV
//! import (spec 2026-08-12, D4).
//!
//! Strict matching on purpose: a false "duplicate" that silently skips a real
//! payment is worse than an occasional visible double. Amount/quantity/price
//! are TEXT columns compared as exact strings — the same export re-uploaded
//! produces the same strings.

use crate::error::Result;
use rusqlite::{Connection, OptionalExtension};

/// Why a bank transaction counts as already imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankDuplicate {
    /// The bank-side export id already exists in the account. Certain: the
    /// bank itself says it is the same transaction.
    BankId(String),
    /// Same account, date, value date, amount, type and description as an
    /// existing row. Only a *possible* duplicate — two genuine purchases (two
    /// 99 CZK coffees on one day) look exactly like a re-imported line.
    Identical,
}

impl BankDuplicate {
    /// Human-readable reason for reports and previews.
    pub fn reason(&self) -> String {
        match self {
            BankDuplicate::BankId(id) => format!(
                "transaction with bank-side id '{}' already exists in this account",
                id
            ),
            BankDuplicate::Identical => "identical transaction (same account, date, value date, amount, type, description) already exists"
                .to_string(),
        }
    }
}

/// Bank transactions. When the row carries a bank-side export id, that id is
/// the dedup key: a hit on the id is a certain duplicate, and a stored row with
/// a *different* id is a different payment however alike the other fields are
///. The strict composite match is the fallback for id-less rows — and
/// for an id-bearing row it only considers stored rows that have no id (an
/// earlier import without ids must still catch the re-import).
#[allow(clippy::too_many_arguments)]
pub fn check_bank_transaction(
    conn: &Connection,
    bank_account_id: &str,
    transaction_id: Option<&str>,
    booking_date: i64,
    value_date: Option<i64>,
    amount: &str,
    tx_type: &str,
    description: Option<&str>,
) -> Result<Option<BankDuplicate>> {
    // An empty/whitespace export id is "no id", not an id: '' == '' would make
    // the primary rule match every later id-less row and silently skip real
    // payments. Guarded here so every caller inherits it.
    let ext_id = transaction_id.filter(|s| !s.trim().is_empty());
    if let Some(ext_id) = ext_id {
        let hit: Option<String> = conn
            .query_row(
                "SELECT id FROM bank_transactions
                 WHERE bank_account_id = ?1 AND transaction_id = ?2",
                rusqlite::params![bank_account_id, ext_id],
                |row| row.get(0),
            )
            .optional()?;
        if hit.is_some() {
            return Ok(Some(BankDuplicate::BankId(ext_id.to_string())));
        }
    }
    let hit: Option<String> = conn
        .query_row(
            "SELECT id FROM bank_transactions
             WHERE bank_account_id = ?1 AND booking_date = ?2 AND amount = ?3
               AND tx_type = ?4 AND description IS ?5 AND value_date IS ?6
               AND (?7 = 0 OR transaction_id IS NULL OR TRIM(transaction_id) = '')",
            rusqlite::params![
                bank_account_id,
                booking_date,
                amount,
                tx_type,
                description,
                value_date,
                ext_id.is_some() as i64
            ],
            |row| row.get(0),
        )
        .optional()?;
    Ok(hit.map(|_| BankDuplicate::Identical))
}

/// [`check_bank_transaction`] with the reason as text. Returns
/// Some(human-readable reason) when a duplicate exists.
#[allow(clippy::too_many_arguments)]
pub fn find_duplicate_bank_transaction(
    conn: &Connection,
    bank_account_id: &str,
    transaction_id: Option<&str>,
    booking_date: i64,
    value_date: Option<i64>,
    amount: &str,
    tx_type: &str,
    description: Option<&str>,
) -> Result<Option<String>> {
    Ok(check_bank_transaction(
        conn,
        bank_account_id,
        transaction_id,
        booking_date,
        value_date,
        amount,
        tx_type,
        description,
    )?
    .map(|d| d.reason()))
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

/// Stock transactions: exact (ticker, date, type, quantity, price) match.
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

    fn insert_bank_tx(
        conn: &Connection,
        id: &str,
        tx_id: Option<&str>,
        desc: Option<&str>,
        value_date: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, transaction_id, tx_type, amount, description, booking_date, value_date)
             VALUES (?1, 'acc-1', ?2, 'debit', '250.5', ?3, 1700000000, ?4)",
            rusqlite::params![id, tx_id, desc, value_date],
        )
        .unwrap();
    }

    #[test]
    fn bank_matches_on_export_transaction_id() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", Some("FIO-123"), Some("groceries"), None);
        let hit = find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            Some("FIO-123"),
            1699999999,
            None,
            "999",
            "credit",
            None,
        )
        .unwrap();
        assert!(hit.is_some(), "id match wins regardless of other fields");
        assert!(hit.unwrap().contains("FIO-123"));
    }

    #[test]
    fn bank_id_match_is_scoped_to_account() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", Some("FIO-123"), None, None);
        let hit = find_duplicate_bank_transaction(
            &conn,
            "acc-OTHER",
            Some("FIO-123"),
            1700000000,
            None,
            "250.5",
            "debit",
            None,
        )
        .unwrap();
        assert!(hit.is_none());
    }

    #[test]
    fn bank_empty_transaction_id_is_treated_as_no_id() {
        let conn = setup_test_db();
        // A stored row with transaction_id = '' (bad/legacy data)
        insert_bank_tx(&conn, "t1", Some(""), Some("groceries"), None);
        // An incoming empty id must NOT match the '' row via the id rule
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            Some(""),
            1800000000,
            None,
            "999",
            "credit",
            Some("rent"),
        )
        .unwrap()
        .is_none());
        // Whitespace-only counts as empty too — but the composite rule still
        // catches genuinely identical rows.
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            Some("   "),
            1700000000,
            None,
            "250.5",
            "debit",
            Some("groceries"),
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn bank_composite_match_requires_all_fields() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", None, Some("groceries"), Some(1700000000));
        // exact composite → duplicate
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            None,
            1700000000,
            Some(1700000000),
            "250.5",
            "debit",
            Some("groceries"),
        )
        .unwrap()
        .is_some());
        // one field differs → not a duplicate
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            None,
            1700000000,
            Some(1700000000),
            "250.5",
            "debit",
            Some("rent"),
        )
        .unwrap()
        .is_none());
        assert!(
            find_duplicate_bank_transaction(
                &conn,
                "acc-1",
                None,
                1700000000,
                Some(1700000000),
                "250.50",
                "debit",
                Some("groceries"),
            )
            .unwrap()
            .is_none(),
            "string compare is exact: '250.50' != '250.5'"
        );
    }

    #[test]
    fn bank_composite_matches_null_description() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", None, None, None);
        assert!(find_duplicate_bank_transaction(
            &conn, "acc-1", None, 1700000000, None, "250.5", "debit", None,
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn bank_composite_distinguishes_value_date() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", None, Some("groceries"), Some(1700000000));
        // different value_date → not the same transaction
        assert!(
            find_duplicate_bank_transaction(
                &conn,
                "acc-1",
                None,
                1700000000,
                Some(1700086400),
                "250.5",
                "debit",
                Some("groceries"),
            )
            .unwrap()
            .is_none(),
            "value date differs → not a duplicate"
        );
        // matching value_date → duplicate
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            None,
            1700000000,
            Some(1700000000),
            "250.5",
            "debit",
            Some("groceries"),
        )
        .unwrap()
        .is_some());
        // None vs None still matches
        insert_bank_tx(&conn, "t2", None, Some("rent"), None);
        assert!(find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            None,
            1700000000,
            None,
            "250.5",
            "debit",
            Some("rent"),
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn bank_distinct_export_ids_are_distinct_transactions() {
        // two coffees of 99 CZK on one day, each with its own bank
        // id, are two payments — the id is the only key when it is present.
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", Some("FIO-1"), Some("coffee"), None);
        let hit = find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            Some("FIO-2"),
            1700000000,
            None,
            "250.5",
            "debit",
            Some("coffee"),
        )
        .unwrap();
        assert!(hit.is_none(), "a different bank id is a different payment");
    }

    #[test]
    fn bank_id_row_still_matches_stored_row_without_id() {
        // An earlier import without ids must still catch a re-import that now
        // carries them: a stored row with no id cannot be told apart by id.
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", None, Some("coffee"), None);
        let hit = find_duplicate_bank_transaction(
            &conn,
            "acc-1",
            Some("FIO-9"),
            1700000000,
            None,
            "250.5",
            "debit",
            Some("coffee"),
        )
        .unwrap();
        assert!(hit.is_some());
    }

    #[test]
    fn bank_check_tells_certain_id_hits_from_possible_repeats() {
        let conn = setup_test_db();
        insert_bank_tx(&conn, "t1", Some("FIO-1"), Some("coffee"), None);
        insert_bank_tx(&conn, "t2", None, Some("tea"), None);
        let by_id =
            check_bank_transaction(&conn, "acc-1", Some("FIO-1"), 1, None, "1", "credit", None)
                .unwrap();
        assert_eq!(by_id, Some(BankDuplicate::BankId("FIO-1".into())));
        let by_composite = check_bank_transaction(
            &conn,
            "acc-1",
            None,
            1700000000,
            None,
            "250.5",
            "debit",
            Some("tea"),
        )
        .unwrap();
        assert_eq!(by_composite, Some(BankDuplicate::Identical));
        assert_eq!(
            check_bank_transaction(&conn, "acc-1", None, 1, None, "1", "credit", None).unwrap(),
            None
        );
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
