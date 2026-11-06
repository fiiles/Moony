//! Manual price and dividend overrides. Each ticker (or crypto symbol) has one override row,
//! whose ID is derived from the ticker so two devices that set the same override offline
//! create the same row.

use crate::error::Result;
use crate::models::{DividendOverride, StockPriceOverride};
use crate::services::sync::natural_id::natural_id;
use rusqlite::Connection;

/// Sets (or replaces) the manual price of a stock ticker. The ticker is stored uppercase.
pub fn upsert_stock_price(
    conn: &Connection,
    ticker: &str,
    price: &str,
    currency: &str,
    now: i64,
) -> Result<StockPriceOverride> {
    let ticker = ticker.to_uppercase();
    let id = natural_id("stock_price_overrides", &[&ticker]);
    conn.execute(
        "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(ticker) DO UPDATE SET price = ?3, currency = ?4, updated_at = ?5",
        rusqlite::params![id, ticker, price, currency, now],
    )?;
    Ok(StockPriceOverride {
        id,
        ticker,
        price: price.to_string(),
        currency: currency.to_string(),
        updated_at: now,
    })
}

/// Sets (or replaces) the manual yearly dividend of a stock ticker.
pub fn upsert_dividend(
    conn: &Connection,
    ticker: &str,
    amount: &str,
    currency: &str,
    now: i64,
) -> Result<DividendOverride> {
    let ticker = ticker.to_uppercase();
    let id = natural_id("dividend_overrides", &[&ticker]);
    conn.execute(
        "INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(ticker) DO UPDATE SET yearly_dividend_sum = ?3, currency = ?4, updated_at = ?5",
        rusqlite::params![id, ticker, amount, currency, now],
    )?;
    Ok(DividendOverride {
        id,
        ticker,
        yearly_dividend_sum: amount.to_string(),
        currency: currency.to_string(),
        updated_at: now,
    })
}

/// Sets (or replaces) the manual price of a crypto symbol. The symbol is stored uppercase.
pub fn upsert_crypto_price(
    conn: &Connection,
    symbol: &str,
    price: &str,
    currency: &str,
    now: i64,
) -> Result<()> {
    let symbol = symbol.to_uppercase();
    let id = natural_id("crypto_price_overrides", &[&symbol]);
    conn.execute(
        "INSERT INTO crypto_price_overrides (id, symbol, price, currency, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(symbol) DO UPDATE SET price = ?3, currency = ?4, updated_at = ?5",
        rusqlite::params![id, symbol, price, currency, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::test_support::migrated;

    fn one(conn: &Connection, sql: &str) -> (String, String) {
        conn.query_row(sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("row")
    }

    #[test]
    fn override_upserts_use_natural_ids_and_keep_the_row_on_conflict() {
        let conn = migrated();

        let first = upsert_stock_price(&conn, "aapl", "100", "USD", 10).expect("first");
        assert_eq!(first.id, natural_id("stock_price_overrides", &["AAPL"]));
        assert_eq!(first.ticker, "AAPL");
        let second = upsert_stock_price(&conn, "AAPL", "120", "EUR", 20).expect("second");
        assert_eq!(second.id, first.id);
        assert_eq!(
            one(&conn, "SELECT id, price FROM stock_price_overrides"),
            (first.id.clone(), "120".to_string())
        );

        let dividend = upsert_dividend(&conn, "msft", "3", "USD", 10).expect("dividend");
        assert_eq!(dividend.id, natural_id("dividend_overrides", &["MSFT"]));
        let again = upsert_dividend(&conn, "MSFT", "4", "USD", 20).expect("again");
        assert_eq!(again.id, dividend.id);
        assert_eq!(
            one(
                &conn,
                "SELECT id, yearly_dividend_sum FROM dividend_overrides"
            ),
            (dividend.id, "4".to_string())
        );

        upsert_crypto_price(&conn, "btc", "50000", "USD", 10).expect("crypto");
        upsert_crypto_price(&conn, "BTC", "60000", "USD", 20).expect("crypto again");
        assert_eq!(
            one(&conn, "SELECT id, price FROM crypto_price_overrides"),
            (
                natural_id("crypto_price_overrides", &["BTC"]),
                "60000".to_string()
            )
        );
    }
}
