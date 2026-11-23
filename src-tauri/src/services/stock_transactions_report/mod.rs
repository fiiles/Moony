//! Stock transactions report: realized sales valued by cost method and holding-period test,
//! with the lots behind them (`lots`) and their valuation (`sales`).

pub mod lots;
pub mod sales;

use rusqlite::Connection;

use crate::error::Result;
use crate::models::fx::ConversionQuality;
use crate::models::{StockReportStats, StockTransactionsReport, StockTransactionsReportRequest};
use crate::services::fx;
use crate::services::reporting::money_in_currency;

const DAY: i64 = 86_400;

/// Builds the report. `today` is the UTC day the service uses for current values. The request is
/// validated by the command, not here.
pub fn report(
    conn: &Connection,
    request: &StockTransactionsReportRequest,
    today: i64,
) -> Result<StockTransactionsReport> {
    let currency = fx::normalize(&request.report_currency)
        .unwrap_or_else(|| request.report_currency.trim().to_uppercase());
    let replay = lots::replay(lots::load_trades(conn)?);
    let first_day = replay.trades.first().map(|t| t.day);
    let last_day = replay.trades.last().map(|t| t.day);
    let span_start = first_day.unwrap_or(today).min(today);
    let span_end = last_day.unwrap_or(today).max(today);
    let history = fx::History::load(conn, span_start, span_end)?;
    let current = fx::current();
    let mut conv = sales::Converter::new(&history, &current, &currency);
    let outcome = sales::value_sales(&replay, request, &currency, &mut conv);
    let zero = money_in_currency(0.0, &currency);
    Ok(StockTransactionsReport {
        fx_quality: ConversionQuality::from(&conv.quality),
        today: today.div_euclid(DAY) * DAY,
        first_transaction_day: first_day,
        summary: outcome.summary,
        // Task 3 fills the stats, buckets, ledger and open lots.
        stats: StockReportStats {
            proceeds: zero.clone(),
            sales_count: 0,
            sold_tickers: 0,
            bought: zero.clone(),
            buys_count: 0,
            bought_tickers: 0,
            fees_paid: zero.clone(),
            traded_volume: zero.clone(),
            dividends: zero,
            dividends_count: 0,
        },
        buckets: Vec::new(),
        tickers: outcome.tickers,
        sales: outcome.sales,
        ledger: Vec::new(),
        open_lots: Vec::new(),
        uncovered_tickers: outcome.uncovered_tickers,
        currency,
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    pub const D1: i64 = 1_673_308_800; // 2023-01-10
    pub const D2: i64 = 1_704_844_800; // 2024-01-10
    pub const D3: i64 = 1_770_681_600; // 2026-02-10

    pub fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                fee TEXT
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL
            );
            "#,
        )
        .expect("schema");
        conn
    }

    pub fn add_position(conn: &Connection, id: &str, ticker: &str) {
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity) VALUES (?1, ?2, ?3, '0')",
            rusqlite::params![id, ticker, format!("{ticker} Inc.")],
        )
        .expect("position");
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_trade(
        conn: &Connection,
        id: &str,
        investment: &str,
        kind: &str,
        quantity: &str,
        price: &str,
        currency: &str,
        day: i64,
        fee: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO investment_transactions
               (id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                currency, transaction_date, fee)
             VALUES (?1, ?2, ?3, ?2, ?2, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![id, investment, kind, quantity, price, currency, day, fee],
        )
        .expect("trade");
    }

    pub fn add_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )
        .expect("rate");
    }

    /// Position AAA (USD) of the shared fixture, with CZK 25 and the USD rate on each trade day.
    pub fn fixture() -> Connection {
        let conn = setup_test_db();
        add_position(&conn, "AAA", "AAA");
        add_trade(&conn, "b1", "AAA", "buy", "10", "100", "USD", D1, Some("2"));
        add_trade(&conn, "b2", "AAA", "buy", "10", "200", "USD", D2, Some("4"));
        add_trade(
            &conn,
            "s1",
            "AAA",
            "sell",
            "15",
            "300",
            "USD",
            D3,
            Some("3"),
        );
        for (day, usd) in [(D1, 1.25), (D2, 1.0), (D3, 1.0)] {
            add_rate(&conn, day, "CZK", 25.0);
            add_rate(&conn, day, "USD", usd);
        }
        conn
    }
}
