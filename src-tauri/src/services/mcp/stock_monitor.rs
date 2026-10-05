//! MCP tools: Stock Monitor watchlist (spec 2026-08-17-stock-monitor-design §7).
//!
//! Thin wrappers over `services::stock_monitor`, which is already keyed by
//! ticker, so the tools map 1:1 onto the same functions the UI uses (one write
//! path, ADR 0007).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::Result;
use crate::models::stock_monitor::InsertWatchedStock;
use crate::services::stock_monitor as watchlist;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchlistFollowArgs {
    #[schemars(description = "Ticker symbol to follow (e.g. AAPL, BMW.DE)")]
    pub ticker: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchlistNotesArgs {
    #[schemars(description = "Ticker symbol (followed automatically if it is not yet)")]
    pub ticker: String,
    #[schemars(description = "Research notes in Markdown; replaces the existing notes entirely")]
    pub notes: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchlistAppendNotesArgs {
    #[schemars(description = "Ticker symbol (followed automatically if it is not yet)")]
    pub ticker: String,
    #[schemars(
        description = "Markdown section to add at the end of the existing notes, separated by a blank line. Send only the new text — never resend what is already stored."
    )]
    pub notes: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchlistTickerArgs {
    #[schemars(description = "Ticker symbol whose notes to read (e.g. AAPL, BMW.DE)")]
    pub ticker: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WatchlistTargetPriceArgs {
    #[schemars(description = "Ticker symbol (followed automatically if it is not yet)")]
    pub ticker: String,
    #[serde(rename = "targetPrice")]
    #[schemars(
        description = "Target price in the stock's own currency as a decimal string (e.g. \"250.50\"); omit or null to clear it"
    )]
    pub target_price: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "\"below\" when waiting for the price to fall to the target (buy or buy more), \"above\" when waiting for it to rise (sell). Omit to infer it from the current price."
    )]
    pub direction: Option<String>,
}

/// Every followed stock with its cached price, day change inputs, target and
/// whether it is also held in the portfolio.
pub fn watchlist_list(conn: &Connection) -> Result<Value> {
    let rows = watchlist::list_watched_stocks(conn)?;
    Ok(json!({
        "count": rows.len(),
        "watchlist": rows
            .iter()
            .map(|r| json!({
                "ticker": r.ticker,
                "name": r.short_name.as_ref().or(r.long_name.as_ref()),
                "currency": r.currency,
                "currentPrice": r.current_price,
                "previousClose": r.previous_close,
                "fiftyTwoWeekLow": r.fifty_two_week_low,
                "fiftyTwoWeekHigh": r.fifty_two_week_high,
                "targetPrice": r.target_price,
                "targetDirection": r.target_direction,
                "notesBytes": r.notes.len(),
                "exchange": r.exchange,
                "priceFetchedAt": r.price_fetched_at,
                "isHeld": r.is_held,
            }))
            .collect::<Vec<_>>(),
    }))
}

/// Follow a ticker. Idempotent: following an already-followed ticker keeps its
/// existing target price and notes.
pub fn watchlist_follow(conn: &Connection, ticker: &str) -> Result<Value> {
    let data = InsertWatchedStock {
        ticker: ticker.to_string(),
    };
    data.validate()?;
    let watched = watchlist::follow_stock(conn, &data)?;
    Ok(json!({
        "ticker": watched.ticker,
        "targetPrice": watched.target_price,
        "notesBytes": watched.notes.len(),
        "followed": true,
    }))
}

/// The full notes of one ticker — the only tool that returns note bodies, so
/// reading one document never drags in every other ticker's. Un-followed
/// tickers answer with empty notes rather than an error, so a caller can probe
/// before writing.
pub fn watchlist_notes_get(conn: &Connection, ticker: &str) -> Result<Value> {
    let ticker = ticker.trim().to_uppercase();
    match watchlist::get_watched(conn, &ticker)? {
        Some(w) => Ok(json!({
            "ticker": w.ticker,
            "followed": true,
            "notes": w.notes,
            "notesBytes": w.notes.len(),
            "targetPrice": w.target_price,
        })),
        None => Ok(json!({
            "ticker": ticker,
            "followed": false,
            "notes": "",
            "notesBytes": 0,
            "targetPrice": Value::Null,
        })),
    }
}

/// Append a section to the notes so a caller adds one entry without resending
/// the whole document.
pub fn watchlist_append_notes(conn: &Connection, ticker: &str, notes: String) -> Result<Value> {
    let followed_now = ensure_followed(conn, ticker)?;
    let appended_chars = notes.trim().len();
    let watched = watchlist::append_notes(conn, ticker, &notes)?;
    Ok(json!({
        "ticker": watched.ticker,
        "notesBytes": watched.notes.len(),
        "appendedBytes": appended_chars,
        "targetPrice": watched.target_price,
        "followedByThisCall": followed_now,
    }))
}

/// Replace the notes of a ticker, following it first when needed so an agent
/// can capture research in one call.
pub fn watchlist_update_notes(conn: &Connection, ticker: &str, notes: String) -> Result<Value> {
    let followed_now = ensure_followed(conn, ticker)?;
    let watched = watchlist::update_notes(conn, ticker, notes)?;
    Ok(json!({
        "ticker": watched.ticker,
        "notesBytes": watched.notes.len(),
        "targetPrice": watched.target_price,
        "followedByThisCall": followed_now,
    }))
}

/// Set (or clear, with a null target) the target price, following the ticker
/// first when needed.
pub fn watchlist_set_target_price(
    conn: &Connection,
    ticker: &str,
    target_price: Option<String>,
    direction: Option<String>,
) -> Result<Value> {
    let followed_now = ensure_followed(conn, ticker)?;
    let watched = watchlist::set_target_price(conn, ticker, target_price, direction)?;
    Ok(json!({
        "ticker": watched.ticker,
        "targetPrice": watched.target_price,
        "targetDirection": watched.target_direction,
        "notesBytes": watched.notes.len(),
        "followedByThisCall": followed_now,
    }))
}

/// Follow `ticker` unless it already is; returns true when this call added it.
fn ensure_followed(conn: &Connection, ticker: &str) -> Result<bool> {
    let data = InsertWatchedStock {
        ticker: ticker.to_string(),
    };
    data.validate()?;
    let already = watchlist::get_watched(conn, ticker)?.is_some();
    if !already {
        watchlist::follow_stock(conn, &data)?;
    }
    Ok(!already)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

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

    #[test]
    fn follow_is_idempotent_and_keeps_notes() {
        let conn = setup_test_db();
        watchlist_follow(&conn, "aapl").expect("follow");
        watchlist_update_notes(&conn, "AAPL", "# Thesis".to_string()).expect("notes");
        let again = watchlist_follow(&conn, "AAPL").expect("follow again");
        assert_eq!(again["ticker"], "AAPL");
        // The notes survived, but the response reports only their size
        assert_eq!(again["notesBytes"], 8);
        assert!(
            again.get("notes").is_none(),
            "write tools must not echo notes"
        );
        assert_eq!(
            watchlist_notes_get(&conn, "AAPL").expect("read")["notes"],
            "# Thesis"
        );
    }

    #[test]
    fn notes_follow_an_unfollowed_ticker_first() {
        let conn = setup_test_db();
        let result = watchlist_update_notes(&conn, "msft", "watching".to_string()).expect("notes");
        assert_eq!(result["ticker"], "MSFT");
        assert_eq!(result["notesBytes"], 8);
        assert_eq!(result["followedByThisCall"], true);
        // Second call edits in place rather than re-following
        let again = watchlist_update_notes(&conn, "MSFT", "updated".to_string()).expect("notes");
        assert_eq!(again["notesBytes"], 7);
        assert_eq!(again["followedByThisCall"], false);
        assert_eq!(
            watchlist_notes_get(&conn, "MSFT").expect("read")["notes"],
            "updated"
        );
    }

    #[test]
    fn target_price_follows_first_then_sets_and_clears() {
        let conn = setup_test_db();
        let set = watchlist_set_target_price(&conn, "nvda", Some("180.00".to_string()), None)
            .expect("set target");
        assert_eq!(set["targetPrice"], "180.00");
        assert_eq!(set["followedByThisCall"], true);
        let cleared = watchlist_set_target_price(&conn, "NVDA", None, None).expect("clear target");
        assert!(cleared["targetPrice"].is_null());
        assert_eq!(cleared["followedByThisCall"], false);
    }

    #[test]
    fn target_direction_is_stored_and_listed() {
        let conn = setup_test_db();
        let set =
            watchlist_set_target_price(&conn, "CAT", Some("600".into()), Some("below".into()))
                .expect("set target");
        assert_eq!(set["targetDirection"], "below");
        let list = watchlist_list(&conn).expect("list");
        assert_eq!(list["watchlist"][0]["targetDirection"], "below");
    }

    #[test]
    fn invalid_input_is_rejected_without_following() {
        let conn = setup_test_db();
        assert!(matches!(
            watchlist_follow(&conn, "AA PL").unwrap_err(),
            AppError::Validation(_)
        ));
        assert!(matches!(
            watchlist_set_target_price(&conn, "AAPL", Some("-5".to_string()), None).unwrap_err(),
            AppError::Validation(_)
        ));
        // The rejected target must not have left a half-followed row behind
        let listed = watchlist_list(&conn).expect("list");
        assert_eq!(listed["count"], 1, "only the valid ensure_followed row");
        assert_eq!(listed["watchlist"][0]["ticker"], "AAPL");
    }

    #[test]
    fn list_reports_note_size_not_note_bodies() {
        let conn = setup_test_db();
        watchlist_update_notes(&conn, "AAPL", "x".repeat(20_000)).expect("notes");
        watchlist_follow(&conn, "MSFT").expect("follow");
        let listed = watchlist_list(&conn).expect("list");
        let rows = listed["watchlist"].as_array().unwrap();
        let aapl = rows.iter().find(|r| r["ticker"] == "AAPL").unwrap();
        let msft = rows.iter().find(|r| r["ticker"] == "MSFT").unwrap();
        assert_eq!(aapl["notesBytes"], 20_000);
        assert_eq!(msft["notesBytes"], 0);
        // The whole point: the 20 kB document is not in the listing
        assert!(aapl.get("notes").is_none());
        assert!(
            serde_json::to_string(&listed).unwrap().len() < 2_000,
            "listing must stay small regardless of note size"
        );
    }

    #[test]
    fn notes_sizes_are_bytes_not_chars() {
        // Czech diacritics are multi-byte in UTF-8, so the two counts differ.
        // The field is named after bytes because MAX_NOTES_BYTES caps bytes.
        let conn = setup_test_db();
        let note = "Hloubková fundamentální analýza";
        assert_ne!(
            note.len(),
            note.chars().count(),
            "test note must be non-ASCII"
        );
        let written = watchlist_update_notes(&conn, "CAT", note.to_string()).expect("notes");
        assert_eq!(written["notesBytes"], note.len());
        let got = watchlist_notes_get(&conn, "CAT").expect("read");
        assert_eq!(got["notesBytes"], note.len());
        assert_eq!(got["notes"], note);
    }

    #[test]
    fn notes_get_returns_the_full_document() {
        let conn = setup_test_db();
        watchlist_update_notes(&conn, "AAPL", "# Thesis\n\n- cheap".to_string()).expect("notes");
        let got = watchlist_notes_get(&conn, "aapl").expect("read");
        assert_eq!(got["ticker"], "AAPL");
        assert_eq!(got["notes"], "# Thesis\n\n- cheap");
        assert_eq!(got["notesBytes"], 17);
        assert_eq!(got["followed"], true);
    }

    #[test]
    fn notes_get_on_an_unfollowed_ticker_is_empty_not_an_error() {
        let conn = setup_test_db();
        let got = watchlist_notes_get(&conn, "NOPE").expect("read");
        assert_eq!(got["followed"], false);
        assert_eq!(got["notes"], "");
        assert_eq!(got["notesBytes"], 0);
    }

    #[test]
    fn append_adds_a_section_and_reports_both_sizes() {
        let conn = setup_test_db();
        watchlist_update_notes(&conn, "AAPL", "# Thesis".to_string()).expect("notes");
        let appended =
            watchlist_append_notes(&conn, "aapl", "## Q2\n\nbeat".to_string()).expect("append");
        assert_eq!(appended["ticker"], "AAPL");
        assert_eq!(appended["appendedBytes"], 11);
        assert_eq!(appended["notesBytes"], 21); // 8 + 2 separator + 11
        assert!(appended.get("notes").is_none());
        assert_eq!(
            watchlist_notes_get(&conn, "AAPL").expect("read")["notes"],
            "# Thesis\n\n## Q2\n\nbeat"
        );
    }

    #[test]
    fn append_follows_an_unfollowed_ticker_first() {
        let conn = setup_test_db();
        let appended =
            watchlist_append_notes(&conn, "nvda", "first entry".to_string()).expect("append");
        assert_eq!(appended["followedByThisCall"], true);
        assert_eq!(appended["notesBytes"], 11);
    }

    #[test]
    fn list_reports_prices_and_held_flag() {
        let conn = setup_test_db();
        watchlist_follow(&conn, "AAPL").expect("follow");
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at,
                                     short_name, previous_close, fifty_two_week_low, exchange)
             VALUES ('sd1','AAPL','230.10','USD',100,100,'Apple Inc.','228.50','164.08','NMS')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity, currency)
             VALUES ('si1','AAPL','10','USD')",
            [],
        )
        .unwrap();
        let listed = watchlist_list(&conn).expect("list");
        let row = &listed["watchlist"][0];
        assert_eq!(row["name"], "Apple Inc.");
        assert_eq!(row["currentPrice"], "230.10");
        assert_eq!(row["previousClose"], "228.50");
        assert_eq!(row["isHeld"], true);
    }
}
