//! Unified price resolution service
//!
//! SINGLE SOURCE OF TRUTH for "what is the current price of this stock or
//! crypto asset". The dashboard metrics, the per-asset enrichment and the
//! portfolio projection all go through here, so they cannot disagree.
//!
//! # The rule
//!
//! A manual override wins **until a newer API price arrives**. Compared by
//! timestamp (override `updated_at` vs API `fetched_at`):
//!
//! | override | API | result |
//! |---|---|---|
//! | none | none | no price |
//! | set | none | override (`is_manual`) |
//! | none | set | API price |
//! | set | set, API strictly newer | API price (the override has expired) |
//! | set | set, override newer or same age | override (`is_manual`) |
//!
//! Stocks and crypto behave identically. The price and the currency always
//! come from the same row (an override quoted in EUR is never combined with an
//! API row quoted in USD).

use crate::services::currency::convert_to_czk;
use crate::services::parsing::parse_money;

/// Resolved price for a stock or crypto asset
#[derive(Debug, Clone)]
pub struct ResolvedPrice {
    /// Price in original currency (as string for precision)
    pub original_price: String,
    /// Original currency code
    pub currency: String,
    /// Price converted to CZK
    pub price_czk: f64,
    /// When the price was last fetched/updated (Unix timestamp)
    pub fetched_at: Option<i64>,
    /// Whether this is a manual override
    pub is_manual: bool,
}

/// Which asset class a price belongs to (selects the tables, not the rule).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Stock,
    Crypto,
}

/// One stored price row: (price text, currency, timestamp).
type PriceRow = (String, String, i64);

/// Tables and columns holding the two price sources of an asset class.
struct Sources {
    override_sql: &'static str,
    api_sql: &'static str,
}

impl AssetKind {
    fn sources(self) -> Sources {
        match self {
            AssetKind::Stock => Sources {
                override_sql:
                    "SELECT price, currency, updated_at FROM stock_price_overrides WHERE ticker = ?1",
                api_sql: "SELECT original_price, currency, fetched_at FROM stock_data WHERE ticker = ?1",
            },
            AssetKind::Crypto => Sources {
                override_sql:
                    "SELECT price, currency, updated_at FROM crypto_price_overrides WHERE symbol = ?1",
                api_sql: "SELECT price, currency, fetched_at FROM crypto_prices WHERE symbol = ?1",
            },
        }
    }
}

/// The rule from the module docs, on plain rows: returns the winning row and
/// whether it is the manual override.
fn pick_price(
    override_row: Option<PriceRow>,
    api_row: Option<PriceRow>,
) -> Option<(PriceRow, bool)> {
    match (override_row, api_row) {
        (Some(o), Some(a)) if a.2 > o.2 => Some((a, false)),
        (Some(o), _) => Some((o, true)),
        (None, Some(a)) => Some((a, false)),
        (None, None) => None,
    }
}

fn query_row(conn: &rusqlite::Connection, sql: &str, key: &str) -> Option<PriceRow> {
    conn.query_row(sql, [key], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })
    .ok()
}

/// Resolve the current price of a stock ticker or crypto symbol.
pub fn resolve_price(
    conn: &rusqlite::Connection,
    kind: AssetKind,
    key: &str,
) -> Option<ResolvedPrice> {
    let sources = kind.sources();
    let override_row = query_row(conn, sources.override_sql, key);
    let api_row = query_row(conn, sources.api_sql, key);

    pick_price(override_row, api_row).map(|((price, currency, at), is_manual)| {
        let amount = parse_money(&price, 0.0, "resolved price");
        ResolvedPrice {
            price_czk: convert_to_czk(amount, &currency),
            original_price: price,
            currency,
            fetched_at: Some(at),
            is_manual,
        }
    })
}

/// Resolve the current price for a stock ticker (see the module docs for the rule).
pub fn resolve_stock_price(conn: &rusqlite::Connection, ticker: &str) -> Option<ResolvedPrice> {
    resolve_price(conn, AssetKind::Stock, ticker)
}

/// Resolve the current price for a crypto symbol (see the module docs for the rule).
pub fn resolve_crypto_price(conn: &rusqlite::Connection, symbol: &str) -> Option<ResolvedPrice> {
    resolve_price(conn, AssetKind::Crypto, symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_data (
                ticker TEXT PRIMARY KEY,
                original_price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE stock_price_overrides (
                ticker TEXT PRIMARY KEY,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                updated_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE crypto_prices (
                symbol TEXT PRIMARY KEY,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                fetched_at INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE crypto_price_overrides (
                symbol TEXT PRIMARY KEY,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD',
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn insert_stock(conn: &Connection, ticker: &str, price: &str, currency: &str, at: i64) {
        conn.execute(
            "INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ticker, price, currency, at],
        )
        .unwrap();
    }

    fn insert_stock_override(
        conn: &Connection,
        ticker: &str,
        price: &str,
        currency: &str,
        at: i64,
    ) {
        conn.execute(
            "INSERT INTO stock_price_overrides (ticker, price, currency, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ticker, price, currency, at],
        )
        .unwrap();
    }

    fn insert_crypto(conn: &Connection, symbol: &str, price: &str, currency: &str, at: i64) {
        conn.execute(
            "INSERT INTO crypto_prices (symbol, price, currency, fetched_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![symbol, price, currency, at],
        )
        .unwrap();
    }

    fn insert_crypto_override(
        conn: &Connection,
        symbol: &str,
        price: &str,
        currency: &str,
        at: i64,
    ) {
        conn.execute(
            "INSERT INTO crypto_price_overrides (symbol, price, currency, updated_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![symbol, price, currency, at],
        )
        .unwrap();
    }

    #[test]
    fn test_resolve_stock_price_returns_none_when_no_data() {
        let conn = setup_test_db();
        let result = resolve_stock_price(&conn, "AAPL");
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_stock_price_from_global_data() {
        let conn = setup_test_db();
        insert_stock(&conn, "AAPL", "150.00", "USD", 100);

        let result = resolve_stock_price(&conn, "AAPL").expect("price");
        assert_eq!(result.original_price, "150.00");
        assert_eq!(result.currency, "USD");
        assert!(!result.is_manual);
    }

    #[test]
    fn test_resolve_stock_price_prefers_newer_override() {
        let conn = setup_test_db();
        insert_stock(&conn, "MSFT", "300.00", "USD", 50);
        insert_stock_override(&conn, "MSFT", "350.00", "USD", 100);

        let result = resolve_stock_price(&conn, "MSFT").expect("price");
        assert_eq!(result.original_price, "350.00");
        assert!(result.is_manual);
    }

    #[test]
    fn test_stock_api_price_newer_than_override_wins() {
        // Override 150 on Monday, Yahoo refresh 180 on Tuesday.
        let conn = setup_test_db();
        insert_stock_override(&conn, "AAPL", "150.00", "USD", 100);
        insert_stock(&conn, "AAPL", "180.00", "USD", 200);

        let result = resolve_stock_price(&conn, "AAPL").expect("price");
        assert_eq!(result.original_price, "180.00");
        assert_eq!(result.fetched_at, Some(200));
        assert!(!result.is_manual, "the override has expired");
    }

    #[test]
    fn test_stock_override_without_api_price_is_used() {
        let conn = setup_test_db();
        insert_stock_override(&conn, "FUND", "12.50", "EUR", 100);

        let result = resolve_stock_price(&conn, "FUND").expect("price");
        assert_eq!(result.original_price, "12.50");
        assert_eq!(result.currency, "EUR");
        assert!(result.is_manual);
    }

    #[test]
    fn test_stock_override_wins_a_tie() {
        // "Until a newer API price arrives": an equally old API price is not newer.
        let conn = setup_test_db();
        insert_stock(&conn, "AAPL", "180.00", "USD", 100);
        insert_stock_override(&conn, "AAPL", "150.00", "USD", 100);

        let result = resolve_stock_price(&conn, "AAPL").expect("price");
        assert_eq!(result.original_price, "150.00");
        assert!(result.is_manual);
    }

    #[test]
    fn test_resolve_crypto_price_returns_none_when_no_data() {
        let conn = setup_test_db();
        let result = resolve_crypto_price(&conn, "BTC");
        assert!(result.is_none());
    }

    #[test]
    fn test_crypto_follows_the_same_rule_as_stocks() {
        let conn = setup_test_db();
        // Override newer than the API price -> override
        insert_crypto(&conn, "BTC", "60000", "USD", 100);
        insert_crypto_override(&conn, "BTC", "50000", "USD", 200);
        let r = resolve_crypto_price(&conn, "BTC").expect("price");
        assert_eq!(r.original_price, "50000");
        assert!(r.is_manual);

        // API newer than the override -> API
        insert_crypto(&conn, "ETH", "3000", "USD", 300);
        insert_crypto_override(&conn, "ETH", "2500", "USD", 200);
        let r = resolve_crypto_price(&conn, "ETH").expect("price");
        assert_eq!(r.original_price, "3000");
        assert!(!r.is_manual);

        // Tie -> override
        insert_crypto(&conn, "SOL", "150", "USD", 100);
        insert_crypto_override(&conn, "SOL", "120", "USD", 100);
        let r = resolve_crypto_price(&conn, "SOL").expect("price");
        assert_eq!(r.original_price, "120");
        assert!(r.is_manual);

        // Override only -> override
        insert_crypto_override(&conn, "ADA", "0.5", "USD", 100);
        let r = resolve_crypto_price(&conn, "ADA").expect("price");
        assert!(r.is_manual);
    }

    #[test]
    fn test_price_and_currency_always_come_from_the_same_row() {
        // The projection used to COALESCE price and currency independently.
        let conn = setup_test_db();
        insert_stock(&conn, "SAP", "100.00", "USD", 100);
        insert_stock_override(&conn, "SAP", "90.00", "EUR", 200);

        let result = resolve_stock_price(&conn, "SAP").expect("price");
        assert_eq!(result.original_price, "90.00");
        assert_eq!(result.currency, "EUR");
    }

    #[test]
    fn test_price_czk_converts_the_native_price() {
        let conn = setup_test_db();
        insert_stock(&conn, "CEZ", "950.00", "CZK", 100);
        let result = resolve_stock_price(&conn, "CEZ").expect("price");
        assert!((result.price_czk - 950.0).abs() < 1e-9);
    }

    #[test]
    fn test_pick_price_rule_table() {
        let row = |p: &str, at: i64| Some((p.to_string(), "USD".to_string(), at));
        assert!(pick_price(None, None).is_none());
        assert_eq!(
            pick_price(row("o", 1), None).map(|(r, m)| (r.0, m)),
            Some(("o".into(), true))
        );
        assert_eq!(
            pick_price(None, row("a", 1)).map(|(r, m)| (r.0, m)),
            Some(("a".into(), false))
        );
        assert_eq!(
            pick_price(row("o", 2), row("a", 1)).map(|(r, m)| (r.0, m)),
            Some(("o".into(), true))
        );
        assert_eq!(
            pick_price(row("o", 1), row("a", 2)).map(|(r, m)| (r.0, m)),
            Some(("a".into(), false))
        );
        assert_eq!(
            pick_price(row("o", 1), row("a", 1)).map(|(r, m)| (r.0, m)),
            Some(("o".into(), true))
        );
    }
}
