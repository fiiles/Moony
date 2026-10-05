//! Stock Monitor (watchlist) business logic — spec 2026-08-17-stock-monitor-design
//!
//! All functions take &Connection (rust-backend rule 1) and are keyed by
//! ticker (UNIQUE), so the future MCP tools wrap them 1:1 (spec §7).

use crate::error::{AppError, Result};
use crate::models::stock_monitor::{
    validate_target_direction, validate_target_price, InsertWatchedStock, StockMonitorDetail,
    WatchedStock, WatchedStockRow, TARGET_ABOVE, TARGET_BELOW,
};
use rusqlite::{Connection, OptionalExtension};

/// Notes size cap (spec D7)
const MAX_NOTES_BYTES: usize = 100 * 1024;

/// Follow a ticker. Idempotent: returns the existing row when already followed.
/// Caller must run InsertWatchedStock::validate() first (commands do).
pub fn follow_stock(conn: &Connection, data: &InsertWatchedStock) -> Result<WatchedStock> {
    let ticker = data.ticker.trim().to_uppercase();
    // INSERT OR IGNORE + fetch makes this idempotent even without the
    // Database::with_conn mutex serializing callers (UNIQUE on ticker).
    conn.execute(
        "INSERT OR IGNORE INTO watched_stocks (id, ticker) VALUES (?1, ?2)",
        rusqlite::params![uuid::Uuid::new_v4().to_string(), ticker],
    )?;
    get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::Internal("watched stock missing after insert".to_string()))
}

/// Fetch a single watchlist entry by ticker (case-insensitive)
pub fn get_watched(conn: &Connection, ticker: &str) -> Result<Option<WatchedStock>> {
    let ticker = ticker.trim().to_uppercase();
    let row = conn
        .query_row(
            "SELECT id, ticker, target_price, target_direction, notes, created_at, updated_at
             FROM watched_stocks WHERE ticker = ?1",
            [&ticker],
            |r| {
                Ok(WatchedStock {
                    id: r.get(0)?,
                    ticker: r.get(1)?,
                    target_price: r.get(2)?,
                    target_direction: r.get(3)?,
                    notes: r.get(4)?,
                    created_at: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn unfollow_stock(conn: &Connection, ticker: &str) -> Result<()> {
    let ticker = ticker.trim().to_uppercase();
    let n = conn.execute("DELETE FROM watched_stocks WHERE ticker = ?1", [&ticker])?;
    if n == 0 {
        return Err(AppError::NotFound(format!("watched stock {ticker}")));
    }
    Ok(())
}

/// `below` when the target sits under the current price (waiting for a dip to buy),
/// otherwise `above` (waiting for a rise; also when no price is known yet).
pub fn infer_target_direction(target: f64, current_price: Option<f64>) -> &'static str {
    match current_price {
        Some(price) if price > 0.0 && target < price => TARGET_BELOW,
        _ => TARGET_ABOVE,
    }
}

pub fn set_target_price(
    conn: &Connection,
    ticker: &str,
    target_price: Option<String>,
    target_direction: Option<String>,
) -> Result<WatchedStock> {
    validate_target_price(&target_price)?;
    validate_target_direction(&target_direction)?;
    let ticker = ticker.trim().to_uppercase();
    let normalized = target_price.map(|t| t.trim().to_string());
    let direction = match &normalized {
        None => None,
        Some(target) => Some(match target_direction {
            Some(explicit) => explicit,
            None => {
                let price: Option<f64> = conn
                    .query_row(
                        "SELECT original_price FROM stock_data WHERE ticker = ?1",
                        [&ticker],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .optional()?
                    .flatten()
                    .and_then(|p| p.trim().parse().ok());
                // validate_target_price guarantees a positive decimal.
                let target: f64 = target.parse().unwrap_or(0.0);
                infer_target_direction(target, price).to_string()
            }
        }),
    };
    let n = conn.execute(
        "UPDATE watched_stocks SET target_price = ?1, target_direction = ?2, updated_at = unixepoch()
         WHERE ticker = ?3",
        rusqlite::params![normalized, direction, ticker],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("watched stock {ticker}")));
    }
    get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::Internal("watched stock missing after update".to_string()))
}

pub fn update_notes(conn: &Connection, ticker: &str, notes: String) -> Result<WatchedStock> {
    if notes.len() > MAX_NOTES_BYTES {
        return Err(AppError::Validation("validation.notesTooLong".to_string()));
    }
    let ticker = ticker.trim().to_uppercase();
    let n = conn.execute(
        "UPDATE watched_stocks SET notes = ?1, updated_at = unixepoch() WHERE ticker = ?2",
        rusqlite::params![notes, ticker],
    )?;
    if n == 0 {
        return Err(AppError::NotFound(format!("watched stock {ticker}")));
    }
    get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::Internal("watched stock missing after update".to_string()))
}

/// Append to the notes instead of replacing them, so a caller adding a new
/// research entry does not have to resend (or regenerate) the whole document.
/// Sections are joined by one blank line, which keeps the result valid Markdown.
/// A blank addition is a no-op rather than an error, so a caller cannot append
/// a dangling separator. The cap applies to the combined result.
pub fn append_notes(conn: &Connection, ticker: &str, addition: &str) -> Result<WatchedStock> {
    let ticker = ticker.trim().to_uppercase();
    let existing = get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::NotFound(format!("watched stock {ticker}")))?;

    let addition = addition.trim();
    if addition.is_empty() {
        return Ok(existing);
    }

    let head = existing.notes.trim_end();
    let combined = if head.is_empty() {
        addition.to_string()
    } else {
        format!("{head}\n\n{addition}")
    };
    if combined.len() > MAX_NOTES_BYTES {
        return Err(AppError::Validation("validation.notesTooLong".to_string()));
    }

    conn.execute(
        "UPDATE watched_stocks SET notes = ?1, updated_at = unixepoch() WHERE ticker = ?2",
        rusqlite::params![combined, ticker],
    )?;
    get_watched(conn, &ticker)?
        .ok_or_else(|| AppError::Internal("watched stock missing after append".to_string()))
}

/// Portfolio stocks that are not on the watchlist yet.
///
/// Sold-out positions (quantity 0) are excluded: the Investments page hides
/// them by default, so offering to follow them would be surprising. This is
/// the single source of truth for both the "follow my portfolio" action and
/// the button's visibility, so the two can never disagree.
pub fn portfolio_follow_candidates(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT si.ticker
         FROM stock_investments si
         WHERE CAST(si.quantity AS REAL) > 0
           AND NOT EXISTS (SELECT 1 FROM watched_stocks w WHERE w.ticker = si.ticker)
         ORDER BY si.ticker",
    )?;
    let tickers = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tickers)
}

/// Follow every portfolio stock that is not followed yet; returns the tickers
/// actually added (empty when the watchlist already covers the portfolio).
/// Existing entries keep their target price and notes — follow_stock is an
/// INSERT OR IGNORE, so this never overwrites what the user already curated.
pub fn follow_portfolio_stocks(conn: &Connection) -> Result<Vec<String>> {
    let candidates = portfolio_follow_candidates(conn)?;
    for ticker in &candidates {
        let data = InsertWatchedStock {
            ticker: ticker.clone(),
        };
        data.validate()?;
        follow_stock(conn, &data)?;
    }
    Ok(candidates)
}

/// Overview table: every watchlist entry, enriched from stock_data when present
pub fn list_watched_stocks(conn: &Connection) -> Result<Vec<WatchedStockRow>> {
    let mut stmt = conn.prepare(
        "SELECT w.id, w.ticker, w.target_price, w.target_direction, w.notes,
                sd.short_name, sd.long_name, sd.currency, sd.original_price,
                sd.previous_close, sd.fifty_two_week_low, sd.fifty_two_week_high,
                sd.exchange, sd.fetched_at,
                (SELECT si.id FROM stock_investments si WHERE si.ticker = w.ticker) AS held_investment_id,
                w.created_at
         FROM watched_stocks w
         LEFT JOIN stock_data sd ON sd.ticker = w.ticker
         ORDER BY w.ticker",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let held_investment_id: Option<String> = r.get(14)?;
            Ok(WatchedStockRow {
                id: r.get(0)?,
                ticker: r.get(1)?,
                target_price: r.get(2)?,
                target_direction: r.get(3)?,
                notes: r.get(4)?,
                short_name: r.get(5)?,
                long_name: r.get(6)?,
                currency: r.get(7)?,
                current_price: r.get(8)?,
                previous_close: r.get(9)?,
                fifty_two_week_low: r.get(10)?,
                fifty_two_week_high: r.get(11)?,
                exchange: r.get(12)?,
                price_fetched_at: r.get(13)?,
                is_held: held_investment_id.is_some(),
                held_investment_id,
                followed_at: r.get(15)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Detail for any ticker (followed or not; cache row optional)
pub fn get_stock_monitor_detail(conn: &Connection, ticker: &str) -> Result<StockMonitorDetail> {
    let ticker = ticker.trim().to_uppercase();
    let watched = get_watched(conn, &ticker)?;
    let held_investment_id: Option<String> = conn
        .query_row(
            "SELECT id FROM stock_investments WHERE ticker = ?1",
            [&ticker],
            |r| r.get(0),
        )
        .optional()?;
    let is_held = held_investment_id.is_some();

    type DataTuple = (
        Option<String>, // short_name
        Option<String>, // long_name
        Option<String>, // currency
        Option<String>, // original_price
        Option<String>, // previous_close
        Option<String>, // fifty_two_week_low
        Option<String>, // fifty_two_week_high
        Option<String>, // market_cap
        Option<String>, // pe_ratio
        Option<String>, // trailing_dividend_yield
        Option<String>, // exchange
        Option<i64>,    // fetched_at
    );
    let data: Option<DataTuple> = conn
        .query_row(
            "SELECT short_name, long_name, currency, original_price, previous_close,
                    fifty_two_week_low, fifty_two_week_high, market_cap, pe_ratio,
                    trailing_dividend_yield, exchange, fetched_at
             FROM stock_data WHERE ticker = ?1",
            [&ticker],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional()?;

    let d = data.unwrap_or((
        None, None, None, None, None, None, None, None, None, None, None, None,
    ));
    Ok(StockMonitorDetail {
        ticker,
        followed: watched.is_some(),
        target_price: watched.as_ref().and_then(|w| w.target_price.clone()),
        target_direction: watched.as_ref().and_then(|w| w.target_direction.clone()),
        followed_at: watched.as_ref().map(|w| w.created_at),
        notes: watched.map(|w| w.notes).unwrap_or_default(),
        short_name: d.0,
        long_name: d.1,
        currency: d.2,
        current_price: d.3,
        previous_close: d.4,
        fifty_two_week_low: d.5,
        fifty_two_week_high: d.6,
        market_cap: d.7,
        pe_ratio: d.8,
        trailing_dividend_yield: d.9,
        exchange: d.10,
        price_fetched_at: d.11,
        held_investment_id,
        is_held,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE watched_stocks (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                target_price TEXT,
                target_direction TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE stock_data (
                id TEXT PRIMARY KEY,
                ticker TEXT UNIQUE,
                original_price TEXT,
                currency TEXT,
                price_date INTEGER,
                fetched_at INTEGER,
                short_name TEXT,
                long_name TEXT,
                market_cap TEXT,
                pe_ratio TEXT,
                trailing_dividend_yield TEXT,
                fifty_two_week_high TEXT,
                fifty_two_week_low TEXT,
                exchange TEXT,
                previous_close TEXT
            );
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT,
                quantity TEXT,

                currency TEXT
            );
        "#,
        )
        .expect("schema");
        conn
    }

    fn follow(conn: &Connection, ticker: &str) -> WatchedStock {
        follow_stock(
            conn,
            &InsertWatchedStock {
                ticker: ticker.to_string(),
            },
        )
        .expect("follow")
    }

    #[test]
    fn follow_creates_row_and_normalizes_case() {
        let conn = setup_test_db();
        let ws = follow(&conn, "bmw.de");
        assert_eq!(ws.ticker, "BMW.DE");
        assert_eq!(ws.notes, "");
        assert!(ws.target_price.is_none());
    }

    #[test]
    fn follow_is_idempotent() {
        let conn = setup_test_db();
        let first = follow(&conn, "AAPL");
        let second = follow(&conn, "aapl");
        assert_eq!(first.id, second.id);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM watched_stocks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn unfollow_removes_row() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        unfollow_stock(&conn, "aapl").expect("unfollow");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM watched_stocks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn unfollow_unknown_is_not_found() {
        let conn = setup_test_db();
        let err = unfollow_stock(&conn, "NOPE").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn set_target_price_updates_and_clears() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        let ws = set_target_price(&conn, "AAPL", Some("250.50".to_string()), None).expect("set");
        assert_eq!(ws.target_price.as_deref(), Some("250.50"));
        let ws = set_target_price(&conn, "AAPL", None, None).expect("clear");
        assert!(ws.target_price.is_none());
    }

    #[test]
    fn portfolio_candidates_exclude_followed_and_sold_out_positions() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO stock_investments (id, ticker, quantity, currency) VALUES
                ('si1','AAPL','10','USD'),
                ('si2','MSFT','5','USD'),
                ('si3','INTC','0','USD');",
        )
        .unwrap();
        follow(&conn, "AAPL");

        let candidates = portfolio_follow_candidates(&conn).expect("candidates");
        // AAPL already followed; INTC is a sold-out position (quantity 0)
        assert_eq!(candidates, vec!["MSFT".to_string()]);
    }

    #[test]
    fn following_the_portfolio_adds_every_candidate_once() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO stock_investments (id, ticker, quantity, currency) VALUES
                ('si1','AAPL','10','USD'),
                ('si2','MSFT','5','USD');",
        )
        .unwrap();

        let added = follow_portfolio_stocks(&conn).expect("follow portfolio");
        assert_eq!(added, vec!["AAPL".to_string(), "MSFT".to_string()]);
        assert_eq!(list_watched_stocks(&conn).unwrap().len(), 2);

        // Nothing left to do the second time — this is what hides the button
        let again = follow_portfolio_stocks(&conn).expect("follow again");
        assert!(again.is_empty());
        assert!(portfolio_follow_candidates(&conn).unwrap().is_empty());
        assert_eq!(list_watched_stocks(&conn).unwrap().len(), 2);
    }

    #[test]
    fn following_the_portfolio_preserves_existing_notes_and_targets() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity, currency)
             VALUES ('si1','AAPL','10','USD')",
            [],
        )
        .unwrap();
        follow(&conn, "AAPL");
        update_notes(&conn, "AAPL", "# Thesis".to_string()).unwrap();
        set_target_price(&conn, "AAPL", Some("250".to_string()), None).unwrap();

        assert!(follow_portfolio_stocks(&conn).unwrap().is_empty());
        let kept = get_watched(&conn, "AAPL").unwrap().expect("still followed");
        assert_eq!(kept.notes, "# Thesis");
        assert_eq!(kept.target_price.as_deref(), Some("250"));
    }

    #[test]
    fn set_target_price_rejects_invalid() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        for bad in ["0", "-5", "abc"] {
            let err = set_target_price(&conn, "AAPL", Some(bad.to_string()), None).unwrap_err();
            assert!(matches!(err, AppError::Validation(_)), "input {bad}");
        }
    }

    #[test]
    fn set_target_price_unknown_ticker_is_not_found() {
        let conn = setup_test_db();
        let err = set_target_price(&conn, "NOPE", Some("10".to_string()), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn update_notes_roundtrips_and_caps_size() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        let ws = update_notes(&conn, "AAPL", "# Thesis\n\n- cheap".to_string()).expect("notes");
        assert_eq!(ws.notes, "# Thesis\n\n- cheap");
        let too_long = "x".repeat(MAX_NOTES_BYTES + 1);
        let err = update_notes(&conn, "AAPL", too_long).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn append_notes_on_empty_replaces_without_leading_blank_lines() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        let ws = append_notes(&conn, "AAPL", "# Thesis").expect("append");
        assert_eq!(ws.notes, "# Thesis");
    }

    #[test]
    fn append_notes_joins_with_one_blank_line() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        append_notes(&conn, "AAPL", "# Thesis").expect("first");
        let ws = append_notes(&conn, "AAPL", "## 2026-08-26\n\nQ2 beat").expect("second");
        assert_eq!(ws.notes, "# Thesis\n\n## 2026-08-26\n\nQ2 beat");
    }

    #[test]
    fn append_notes_trims_the_addition_and_ignores_empty() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        append_notes(&conn, "AAPL", "  # Thesis  \n").expect("first");
        let ws = append_notes(&conn, "AAPL", "   \n  ").expect("whitespace-only is a no-op");
        assert_eq!(
            ws.notes, "# Thesis",
            "nothing appended, no trailing separator"
        );
    }

    #[test]
    fn append_notes_caps_on_the_combined_size() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        // Just under the cap, so the addition is what pushes it over.
        let big = "x".repeat(MAX_NOTES_BYTES - 4);
        update_notes(&conn, "AAPL", big.clone()).expect("seed");
        let err = append_notes(&conn, "AAPL", "yyyyyy").unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
        // The rejected append must not have modified the stored notes
        assert_eq!(get_watched(&conn, "AAPL").unwrap().unwrap().notes, big);
    }

    #[test]
    fn append_notes_unknown_ticker_is_not_found() {
        let conn = setup_test_db();
        let err = append_notes(&conn, "NOPE", "note").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn update_notes_unknown_ticker_is_not_found() {
        let conn = setup_test_db();
        let err = update_notes(&conn, "NOPE", "note".to_string()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn list_joins_stock_data_and_flags_held() {
        let conn = setup_test_db();
        follow(&conn, "AAPL");
        follow(&conn, "BMW.DE");
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at,
                                     short_name, fifty_two_week_low, previous_close, exchange)
             VALUES ('sd1', 'AAPL', '230.10', 'USD', 100, 100, 'Apple Inc.', '164.08', '228.50', 'NMS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity, currency)
             VALUES ('si1', 'AAPL', '10', 'USD')",
            [],
        )
        .unwrap();

        let rows = list_watched_stocks(&conn).expect("list");
        assert_eq!(rows.len(), 2);
        let aapl = rows.iter().find(|r| r.ticker == "AAPL").unwrap();
        assert_eq!(aapl.current_price.as_deref(), Some("230.10"));
        assert_eq!(aapl.previous_close.as_deref(), Some("228.50"));
        assert_eq!(aapl.short_name.as_deref(), Some("Apple Inc."));
        assert_eq!(aapl.currency.as_deref(), Some("USD"));
        assert!(aapl.is_held);
        assert_eq!(aapl.held_investment_id.as_deref(), Some("si1"));
        // No stock_data row yet for BMW.DE — prices are None, row still listed
        let bmw = rows.iter().find(|r| r.ticker == "BMW.DE").unwrap();
        assert!(bmw.current_price.is_none());
        assert!(!bmw.is_held);
        assert!(bmw.held_investment_id.is_none());
    }

    #[test]
    fn detail_works_for_unfollowed_ticker() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at,
                                     long_name, market_cap, pe_ratio)
             VALUES ('sd1', 'MSFT', '420.00', 'USD', 100, 100, 'Microsoft Corporation', '3120000000000', '36.55')",
            [],
        )
        .unwrap();
        let d = get_stock_monitor_detail(&conn, "msft").expect("detail");
        assert!(!d.followed);
        assert_eq!(d.ticker, "MSFT");
        assert_eq!(d.notes, "");
        assert_eq!(d.current_price.as_deref(), Some("420.00"));
        assert_eq!(d.market_cap.as_deref(), Some("3120000000000"));
        assert!(d.held_investment_id.is_none());
    }

    #[test]
    fn detail_includes_watchlist_fields_when_followed() {
        let conn = setup_test_db();
        follow(&conn, "MSFT");
        set_target_price(&conn, "MSFT", Some("500".to_string()), None).unwrap();
        update_notes(&conn, "MSFT", "note".to_string()).unwrap();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity, currency)
             VALUES ('si9', 'MSFT', '5', 'USD')",
            [],
        )
        .unwrap();
        let d = get_stock_monitor_detail(&conn, "MSFT").expect("detail");
        assert!(d.followed);
        assert_eq!(d.target_price.as_deref(), Some("500"));
        assert_eq!(d.notes, "note");
        // no stock_data row — data fields None but detail still returned
        assert!(d.current_price.is_none());
        assert_eq!(d.held_investment_id.as_deref(), Some("si9"));
        assert!(d.is_held);
    }

    fn with_price(conn: &Connection, ticker: &str, price: &str) {
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency) VALUES (?1, ?2, ?3, 'USD')",
            rusqlite::params![format!("sd-{ticker}"), ticker, price],
        )
        .expect("price row");
    }

    #[test]
    fn a_target_below_the_price_waits_for_a_dip() {
        let conn = setup_test_db();
        follow_stock(
            &conn,
            &InsertWatchedStock {
                ticker: "CAT".into(),
            },
        )
        .unwrap();
        with_price(&conn, "CAT", "845");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("below"));
    }

    #[test]
    fn a_target_above_the_price_or_without_a_price_waits_for_a_rise() {
        let conn = setup_test_db();
        follow_stock(
            &conn,
            &InsertWatchedStock {
                ticker: "CAT".into(),
            },
        )
        .unwrap();
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(
            ws.target_direction.as_deref(),
            Some("above"),
            "no price known"
        );
        with_price(&conn, "CAT", "500");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), None).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("above"));
    }

    #[test]
    fn an_explicit_direction_wins_and_clearing_the_target_clears_it() {
        let conn = setup_test_db();
        follow_stock(
            &conn,
            &InsertWatchedStock {
                ticker: "CAT".into(),
            },
        )
        .unwrap();
        with_price(&conn, "CAT", "845");
        let ws = set_target_price(&conn, "CAT", Some("600".into()), Some("above".into())).unwrap();
        assert_eq!(ws.target_direction.as_deref(), Some("above"));
        let rows = list_watched_stocks(&conn).unwrap();
        assert_eq!(rows[0].target_direction.as_deref(), Some("above"));
        let detail = get_stock_monitor_detail(&conn, "CAT").unwrap();
        assert_eq!(detail.target_direction.as_deref(), Some("above"));
        let ws = set_target_price(&conn, "CAT", None, Some("below".into())).unwrap();
        assert_eq!(ws.target_price, None);
        assert_eq!(ws.target_direction, None);
    }

    #[test]
    fn an_unknown_direction_is_rejected() {
        let conn = setup_test_db();
        follow_stock(
            &conn,
            &InsertWatchedStock {
                ticker: "CAT".into(),
            },
        )
        .unwrap();
        let err = set_target_price(&conn, "CAT", Some("600".into()), Some("sideways".into()))
            .unwrap_err();
        assert!(
            matches!(err, AppError::Validation(ref k) if k == "validation.targetDirectionInvalid")
        );
    }
}
