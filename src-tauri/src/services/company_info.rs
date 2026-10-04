//! Company data of a stock for the position detail.
//!
//! The data lives in `stock_data`, filled by `price_api::refresh_stock_metadata_yahoo`; this
//! module reads it and decides when it is old enough to ask Yahoo again. Functions take a plain
//! `&Connection` (rust-backend rule 1) so the command can keep the network call outside the
//! database lock.

use crate::error::Result;
use crate::models::company_info::StockCompanyInfo;
use rusqlite::{Connection, OptionalExtension};

/// How long stored company data counts as current. Company data moves slowly, so one day is
/// enough. `price_api::refresh_stock_metadata_yahoo` applies the same 24 h TTL itself
/// (`METADATA_CACHE_HOURS`); keep the two in step.
const METADATA_MAX_AGE_SECONDS: i64 = 24 * 60 * 60;

/// Whether stored metadata is due for a refresh: it was never fetched, or at least a day ago.
/// A timestamp in the future (the clock moved back) counts as fresh, so the card does not ask
/// Yahoo on every open.
pub fn metadata_is_stale(fetched_at: Option<i64>, now: i64) -> bool {
    match fetched_at {
        None => true,
        Some(at) => now.saturating_sub(at) >= METADATA_MAX_AGE_SECONDS,
    }
}

/// Record that Yahoo was asked for a ticker's company data and answered without any (it does not
/// know the ticker, or keeps no profile for it). The attempt counts as a fetch, so the ticker is
/// not sent again before `METADATA_MAX_AGE_SECONDS` have passed (PRIVACY.md promises at most once
/// a day); stored figures stay as they are. A ticker without a `stock_data` row is left alone.
pub fn mark_metadata_checked(conn: &Connection, ticker: &str, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE stock_data SET metadata_fetched_at = ?2 WHERE ticker = ?1",
        rusqlite::params![ticker.trim().to_uppercase(), now],
    )?;
    Ok(())
}

/// A figure that is positive by nature (ratios, prices, market cap, yield), or `None`. Yahoo
/// reports 0 where a figure does not apply (no market cap for most funds, a dividend rate of 0
/// for a company that pays none), and a stored string that is not a number is no figure either;
/// neither may reach the card as a value.
fn positive_figure(value: Option<String>) -> Option<String> {
    value.filter(|v| {
        v.trim()
            .parse::<f64>()
            .is_ok_and(|n| n.is_finite() && n > 0.0)
    })
}

/// Company data stored for a ticker (matched without case or surrounding whitespace). A ticker
/// without a `stock_data` row yields an empty record (every field `None` but the ticker), so
/// "nothing stored" is data for the caller, not an error.
pub fn read_company_info(conn: &Connection, ticker: &str) -> Result<StockCompanyInfo> {
    let ticker = ticker.trim().to_uppercase();
    let stored = conn
        .query_row(
            "SELECT sector, industry, pe_ratio, forward_pe, market_cap, beta,
                    fifty_two_week_high, fifty_two_week_low,
                    trailing_dividend_rate, trailing_dividend_yield,
                    quote_type, currency, metadata_fetched_at
             FROM stock_data WHERE ticker = ?1",
            [&ticker],
            |r| {
                Ok(StockCompanyInfo {
                    ticker: ticker.clone(),
                    sector: r.get(0)?,
                    industry: r.get(1)?,
                    pe_ratio: positive_figure(r.get(2)?),
                    forward_pe: positive_figure(r.get(3)?),
                    market_cap: positive_figure(r.get(4)?),
                    // Zero and negative betas are real, so no filter here
                    beta: r.get(5)?,
                    fifty_two_week_high: positive_figure(r.get(6)?),
                    fifty_two_week_low: positive_figure(r.get(7)?),
                    dividend_rate: positive_figure(r.get(8)?),
                    dividend_yield: positive_figure(r.get(9)?),
                    quote_type: r.get(10)?,
                    currency: r.get(11)?,
                    metadata_fetched_at: r.get(12)?,
                })
            },
        )
        .optional()?;
    Ok(stored.unwrap_or_else(|| StockCompanyInfo {
        ticker,
        ..Default::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the `stock_data` columns this service reads (the real table has more).
    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_data (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                currency TEXT NOT NULL DEFAULT 'USD',
                sector TEXT,
                industry TEXT,
                pe_ratio TEXT,
                forward_pe TEXT,
                market_cap TEXT,
                beta TEXT,
                fifty_two_week_high TEXT,
                fifty_two_week_low TEXT,
                trailing_dividend_rate TEXT,
                trailing_dividend_yield TEXT,
                quote_type TEXT,
                metadata_fetched_at INTEGER
            );
        "#,
        )
        .expect("schema");
        conn
    }

    /// A row as the Yahoo metadata refresh leaves it.
    fn insert_full_row(conn: &Connection, ticker: &str) {
        conn.execute(
            "INSERT INTO stock_data (
                 id, ticker, currency, sector, industry, pe_ratio, forward_pe, market_cap, beta,
                 fifty_two_week_high, fifty_two_week_low, trailing_dividend_rate,
                 trailing_dividend_yield, quote_type, metadata_fetched_at
             ) VALUES (?1, ?2, 'USD', 'Technology', 'Consumer Electronics', '31.52', '28.10',
                       '3120000000000', '1.245', '288.62', '169.21', '1.04', '0.003400',
                       'EQUITY', 1700000000)",
            rusqlite::params![format!("id-{ticker}"), ticker],
        )
        .expect("insert full row");
    }

    #[test]
    fn reads_the_stored_metadata_of_a_ticker() {
        let conn = setup_test_db();
        insert_full_row(&conn, "AAPL");

        let info = read_company_info(&conn, "AAPL").expect("read");

        assert_eq!(info.ticker, "AAPL");
        assert_eq!(info.sector.as_deref(), Some("Technology"));
        assert_eq!(info.industry.as_deref(), Some("Consumer Electronics"));
        assert_eq!(info.pe_ratio.as_deref(), Some("31.52"));
        assert_eq!(info.forward_pe.as_deref(), Some("28.10"));
        assert_eq!(info.market_cap.as_deref(), Some("3120000000000"));
        assert_eq!(info.beta.as_deref(), Some("1.245"));
        assert_eq!(info.fifty_two_week_high.as_deref(), Some("288.62"));
        assert_eq!(info.fifty_two_week_low.as_deref(), Some("169.21"));
        // Stored in the trailing_* columns, reported as plain dividend figures
        assert_eq!(info.dividend_rate.as_deref(), Some("1.04"));
        assert_eq!(info.dividend_yield.as_deref(), Some("0.003400"));
        assert_eq!(info.quote_type.as_deref(), Some("EQUITY"));
        assert_eq!(info.currency.as_deref(), Some("USD"));
        assert_eq!(info.metadata_fetched_at, Some(1_700_000_000));
    }

    #[test]
    fn a_row_without_metadata_reports_nothing_but_its_currency() {
        let conn = setup_test_db();
        // What the price refresh inserts: the quote, no company data yet
        conn.execute(
            "INSERT INTO stock_data (id, ticker, currency) VALUES ('id-1', 'MSFT', 'USD')",
            [],
        )
        .expect("insert");

        let info = read_company_info(&conn, "MSFT").expect("read");

        assert_eq!(info.ticker, "MSFT");
        assert_eq!(info.currency.as_deref(), Some("USD"));
        assert!(info.sector.is_none());
        assert!(info.industry.is_none());
        assert!(info.pe_ratio.is_none());
        assert!(info.forward_pe.is_none());
        assert!(info.market_cap.is_none());
        assert!(info.beta.is_none());
        assert!(info.fifty_two_week_high.is_none());
        assert!(info.fifty_two_week_low.is_none());
        assert!(info.dividend_rate.is_none());
        assert!(info.dividend_yield.is_none());
        assert!(info.quote_type.is_none());
        assert!(info.metadata_fetched_at.is_none());
    }

    #[test]
    fn a_missing_ticker_is_an_empty_record_not_an_error() {
        let conn = setup_test_db();
        insert_full_row(&conn, "AAPL");

        let info = read_company_info(&conn, "NOPE").expect("read");

        assert_eq!(info.ticker, "NOPE");
        assert!(info.currency.is_none());
        assert!(info.sector.is_none());
        assert!(info.pe_ratio.is_none());
        assert!(info.metadata_fetched_at.is_none());
    }

    #[test]
    fn the_ticker_is_matched_without_case_or_surrounding_whitespace() {
        let conn = setup_test_db();
        insert_full_row(&conn, "BMW.DE");

        let info = read_company_info(&conn, "  bmw.de ").expect("read");

        assert_eq!(info.ticker, "BMW.DE");
        assert_eq!(info.sector.as_deref(), Some("Technology"));
    }

    #[test]
    fn figures_that_do_not_apply_are_not_reported() {
        let conn = setup_test_db();
        // How Yahoo describes a fund (no market cap) and a company that pays no dividend
        conn.execute(
            "INSERT INTO stock_data (
                 id, ticker, currency, pe_ratio, forward_pe, market_cap, beta,
                 fifty_two_week_high, fifty_two_week_low, trailing_dividend_rate,
                 trailing_dividend_yield, quote_type, metadata_fetched_at
             ) VALUES ('id-1', 'VWCE.DE', 'EUR', '0.00', '0.00', '0', '0.000',
                       '135.40', '0.00', '0.00', '0.000000', 'ETF', 1700000000)",
            [],
        )
        .expect("insert");

        let info = read_company_info(&conn, "VWCE.DE").expect("read");

        assert!(info.pe_ratio.is_none());
        assert!(info.forward_pe.is_none());
        assert!(info.market_cap.is_none());
        assert!(info.fifty_two_week_low.is_none());
        assert!(info.dividend_rate.is_none());
        assert!(info.dividend_yield.is_none());
        // Real values and the beta (zero is a legitimate beta) pass through
        assert_eq!(info.fifty_two_week_high.as_deref(), Some("135.40"));
        assert_eq!(info.beta.as_deref(), Some("0.000"));
        assert_eq!(info.quote_type.as_deref(), Some("ETF"));
    }

    #[test]
    fn a_stored_figure_that_is_not_a_number_is_not_reported() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_data (id, ticker, currency, pe_ratio, market_cap)
             VALUES ('id-1', 'ODD', 'USD', 'n/a', 'NaN')",
            [],
        )
        .expect("insert");

        let info = read_company_info(&conn, "ODD").expect("read");

        assert!(info.pe_ratio.is_none());
        assert!(info.market_cap.is_none());
    }

    /// Yahoo answered and had nothing for the ticker: the attempt counts as a fetch, so the
    /// ticker is not sent again before the day is over (PRIVACY.md), and nothing stored is lost.
    #[test]
    fn an_answer_without_data_counts_as_a_fetch() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_data (id, ticker, currency, sector) VALUES ('id-1', 'GONE', 'USD', 'Energy')",
            [],
        )
        .expect("insert");
        let now = 1_700_000_000;

        mark_metadata_checked(&conn, " gone ", now).expect("mark");

        let info = read_company_info(&conn, "GONE").expect("read");
        assert_eq!(info.metadata_fetched_at, Some(now));
        assert!(!metadata_is_stale(info.metadata_fetched_at, now + 3_600));
        assert_eq!(info.sector.as_deref(), Some("Energy"));
    }

    #[test]
    fn marking_a_ticker_without_a_row_changes_nothing() {
        let conn = setup_test_db();
        mark_metadata_checked(&conn, "NOPE", 1_700_000_000).expect("mark");
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_data", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn metadata_that_was_never_fetched_is_stale() {
        assert!(metadata_is_stale(None, 1_700_000_000));
    }

    #[test]
    fn metadata_is_fresh_for_less_than_a_day() {
        let fetched = 1_700_000_000;
        assert!(!metadata_is_stale(Some(fetched), fetched));
        assert!(!metadata_is_stale(Some(fetched), fetched + 3_600));
        assert!(!metadata_is_stale(Some(fetched), fetched + 24 * 3_600 - 1));
    }

    #[test]
    fn metadata_goes_stale_after_a_day() {
        let fetched = 1_700_000_000;
        assert!(metadata_is_stale(Some(fetched), fetched + 24 * 3_600));
        assert!(metadata_is_stale(Some(fetched), fetched + 30 * 24 * 3_600));
    }

    #[test]
    fn metadata_stamped_in_the_future_is_not_stale() {
        // A clock that moved backwards must not trigger a refresh on every open
        let now = 1_700_000_000;
        assert!(!metadata_is_stale(Some(now + 3_600), now));
    }
}
