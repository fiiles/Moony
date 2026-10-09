//! Automatic FX timeseries backfill.
//!
//! Cost basis and historical valuations derive from `exchange_rate_history`
//! (services/cost_basis.rs, convert_to_czk_at). The daily writer only records
//! rates for days the app is running, so two kinds of gaps appear:
//!
//! - a **prefix gap**: a transaction predates the whole timeseries (a user
//!   enters a buy from 2018 — or 2000 — today), and
//! - a **tail gap**: the app was closed for a stretch of days.
//!
//! This module heals both automatically, mirroring how historical *prices*
//! are already backfilled per ticker after a historical transaction is
//! entered. Rates come from frankfurter.dev (ECB daily series, EUR base,
//! inverted to CZK base). Backfill is best-effort: a fetch failure must never
//! break transaction entry or rate refresh — days that stay uncovered fall
//! back to current rates at read time (documented fallback semantics).

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::db::Database;
use crate::error::Result;
use crate::services::fx;

const DAY: i64 = 86_400;
/// The currency every stored rate is quoted against; it never has a row of its own.
const REFERENCE: &str = "CZK";
/// Walk-back window used by rate lookups (`get_rates_for_date`): the fetch
/// starts this much before the earliest transaction so weekend/holiday
/// transactions can resolve to the closest earlier day.
const LOOKBACK: i64 = 10 * DAY;
/// ECB reference rates begin 1999-01-04; frankfurter serves nothing earlier.
const ECB_EPOCH: i64 = 915_408_000; // 1999-01-04 UTC midnight
/// Tail gaps up to the lookup walk-back resolve on their own; only fetch when
/// the timeseries has fallen further behind than that.
const TAIL_TOLERANCE: i64 = 3 * DAY;

/// Every table column that names a currency the user holds, is quoted in or reports in.
const CURRENCY_COLUMNS: &[(&str, &str)] = &[
    ("investment_transactions", "currency"),
    ("crypto_transactions", "currency"),
    ("bank_transactions", "currency"),
    ("bank_accounts", "currency"),
    ("stock_data", "currency"),
    ("crypto_prices", "currency"),
    ("bonds", "currency"),
    ("loans", "currency"),
    ("real_estate", "purchase_price_currency"),
    ("real_estate", "market_price_currency"),
    ("real_estate", "monthly_rent_currency"),
    ("real_estate_valuations", "currency"),
    ("other_assets", "currency"),
    ("other_asset_transactions", "currency"),
    ("other_asset_valuations", "currency"),
    ("insurance_policies", "one_time_payment_currency"),
    ("insurance_policies", "regular_payment_currency"),
    ("cashflow_items", "currency"),
    ("budget_goal_versions", "currency"),
    ("user_profile", "currency"),
];

/// `(table, currency column, date column)` for every fact that carries both a currency and a
/// day. Only these decide how far back the rate history has to reach.
const DATED_CURRENCY_FACTS: &[(&str, &str, &str)] = &[
    ("investment_transactions", "currency", "transaction_date"),
    ("crypto_transactions", "currency", "transaction_date"),
    ("bank_transactions", "currency", "booking_date"),
    ("other_asset_transactions", "currency", "transaction_date"),
    ("real_estate_valuations", "currency", "valued_at"),
    ("other_asset_valuations", "currency", "valued_at"),
    ("real_estate", "purchase_price_currency", "purchase_date"),
];

/// Days after which a held currency's newest stored rate counts as stale.
pub const STALE_AFTER_DAYS: i64 = 14;

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        [table, column],
        |r| r.get::<_, bool>(0),
    )?)
}

/// Currencies the rate history must cover: every currency found in the user's data (balances,
/// transactions, price quotes, properties, policies, budgets, the profile), expressed as a
/// stored source quote. The reference currency needs no rate, a pegged currency is represented
/// by its anchor, and codes without a published source quote cannot be fetched at all. Sorted.
pub fn held_currencies(conn: &Connection) -> Result<Vec<String>> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for (table, column) in CURRENCY_COLUMNS {
        if !has_column(conn, table, column)? {
            continue;
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT DISTINCT {column} FROM {table} WHERE {column} IS NOT NULL"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for code in rows.filter_map(|r| r.ok()) {
            let Some(code) = fx::normalize(&code) else {
                continue;
            };
            match fx::derived(&code) {
                Some((anchor, _)) => set.insert(anchor.to_string()),
                None => set.insert(code),
            };
        }
    }
    set.retain(|code| code != REFERENCE && fx::source_codes().contains(code));
    Ok(set.into_iter().collect())
}

/// `currency` together with the pegged currencies stored through it.
fn codes_behind(currency: &str) -> Vec<String> {
    let mut codes = vec![currency.to_string()];
    codes.extend(
        fx::supported_codes()
            .into_iter()
            .filter(|code| fx::derived(code).is_some_and(|(anchor, _)| anchor == currency)),
    );
    codes
}

/// Earliest day of any dated fact in `currency` (or in a currency pegged to it).
fn earliest_day(conn: &Connection, currency: &str) -> Result<Option<i64>> {
    let codes = codes_behind(currency);
    let mut earliest: Option<i64> = None;
    for (table, currency_column, date_column) in DATED_CURRENCY_FACTS {
        if !has_column(conn, table, currency_column)? || !has_column(conn, table, date_column)? {
            continue;
        }
        let sql = format!(
            "SELECT MIN({date_column}) FROM {table} WHERE UPPER(TRIM({currency_column})) = ?1"
        );
        for code in &codes {
            let day: Option<i64> = conn.query_row(&sql, [code.as_str()], |r| r.get(0))?;
            earliest = earliest.into_iter().chain(day).min();
        }
    }
    Ok(earliest)
}

/// Held currencies whose newest stored rate is more than [`STALE_AFTER_DAYS`] days older than
/// `today`; values in them use that old rate. A currency without any stored rate is not listed
/// here (it is reported as missing when a conversion needs it). Sorted.
pub fn stale_currencies(conn: &Connection, today: i64) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for currency in held_currencies(conn)? {
        let latest: Option<i64> = conn.query_row(
            "SELECT MAX(date) FROM exchange_rate_history WHERE UPPER(currency) = ?1",
            [currency.as_str()],
            |r| r.get(0),
        )?;
        if latest.is_some_and(|day| day < today - STALE_AFTER_DAYS * DAY) {
            stale.push(currency);
        }
    }
    Ok(stale)
}

/// First fetch day for history starting at `earliest_tx`.
fn needed_start(earliest_tx: i64) -> i64 {
    (((earliest_tx / DAY) * DAY) - LOOKBACK).max(ECB_EPOCH)
}

/// Day ranges (inclusive, midnight-UTC keys) missing from
/// `exchange_rate_history` for every held currency: the prefix before the
/// earliest covered day, and the tail since the latest one. Empty when
/// nothing needs fetching.
///
/// The prefix is checked per currency: a currency first recorded by the
/// daily writer long after its earliest dated fact still needs its history
/// even when another currency was backfilled further back. A currency
/// without any recorded rate is not fetched again and again; it stays
/// reported as missing. A currency without any dated fact (a bare balance
/// or quote) needs no prefix, only the tail.
pub fn missing_fx_ranges(conn: &Connection, today: i64) -> Result<Vec<(i64, i64)>> {
    let symbols = held_currencies(conn)?;
    if symbols.is_empty() {
        return Ok(Vec::new());
    }
    let mut earliest = Vec::new();
    for symbol in &symbols {
        if let Some(day) = earliest_day(conn, symbol)? {
            earliest.push((symbol, day));
        }
    }

    let coverage: Option<(i64, i64)> = conn
        .query_row(
            "SELECT MIN(date), MAX(date) FROM exchange_rate_history",
            [],
            |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<i64>>(1)?)),
        )
        .ok()
        .and_then(|(min, max)| min.zip(max));

    let mut ranges = Vec::new();
    match coverage {
        None => {
            let from = earliest.iter().map(|(_, day)| *day).min().unwrap_or(today);
            ranges.push((needed_start(from), today));
        }
        Some((_, max_date)) => {
            let mut prefix: Option<(i64, i64)> = None;
            for (symbol, first_tx) in &earliest {
                let covered_from: Option<i64> = conn.query_row(
                    "SELECT MIN(date) FROM exchange_rate_history WHERE UPPER(currency) = ?1",
                    [symbol.as_str()],
                    |r| r.get(0),
                )?;
                let Some(covered_from) = covered_from else {
                    continue;
                };
                let start = needed_start(*first_tx);
                if covered_from > start {
                    prefix = Some(match prefix {
                        None => (start, covered_from),
                        Some((a, b)) => (a.min(start), b.max(covered_from)),
                    });
                }
            }
            ranges.extend(prefix);
            if max_date < today - TAIL_TOLERANCE {
                ranges.push((max_date, today));
            }
        }
    }
    Ok(ranges)
}

/// Parse a frankfurter.dev range response (EUR base) into CZK-base rows
/// `(day, currency, rate_czk_per_unit)`. Includes EUR itself. Days without a
/// CZK quote are skipped (no inversion possible).
pub fn parse_frankfurter_rates(
    body: &serde_json::Value,
    symbols: &[String],
) -> Vec<(i64, String, f64)> {
    let Some(days) = body["rates"].as_object() else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for (date, day_rates) in days {
        let day = day_from_date(date);
        // Everything is quoted per EUR; without the CZK quote there is no
        // way to invert to CZK base.
        let Some(czk_per_eur) = day_rates["CZK"].as_f64() else {
            continue;
        };
        rows.push((day, "EUR".to_string(), czk_per_eur));
        for currency in symbols {
            if currency == "EUR" || currency == "CZK" {
                continue;
            }
            if let Some(per_eur) = day_rates[currency.as_str()].as_f64() {
                rows.push((day, currency.clone(), czk_per_eur / per_eur));
            }
        }
    }
    rows
}

/// Insert fetched rows, keeping any already-recorded day/currency pair
/// (live-recorded rates win over backfill).
pub fn store_fx_history(conn: &Connection, rows: &[(i64, String, f64)]) -> Result<usize> {
    let mut inserted = 0usize;
    for (day, currency, rate) in rows {
        inserted += conn.execute(
            "INSERT OR IGNORE INTO exchange_rate_history (date, currency, rate)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )?;
    }
    Ok(inserted)
}

/// Fetch and store every missing range. Returns the number of inserted rows.
pub async fn ensure_fx_coverage(db: &Database) -> Result<usize> {
    let today = (chrono::Utc::now().timestamp() / DAY) * DAY;
    let (symbols, ranges) =
        db.with_conn(|conn| Ok((held_currencies(conn)?, missing_fx_ranges(conn, today)?)))?;
    if symbols.is_empty() || ranges.is_empty() {
        return Ok(0);
    }

    let mut inserted = 0usize;
    for (start, end) in ranges {
        let url = format!(
            "https://api.frankfurter.dev/v1/{}..{}?base=EUR&symbols=CZK,{}",
            date_string(start),
            date_string(end),
            symbols.join(",")
        );
        log::debug!("[FX_BACKFILL] Fetching ECB rates {url}");
        let body: serde_json::Value = crate::services::http::client()
            .get(&url)
            .send()
            .await?
            .json()
            .await?;
        let rows = parse_frankfurter_rates(&body, &symbols);
        inserted += db.with_conn(|conn| store_fx_history(conn, &rows))?;
    }
    log::info!("[FX_BACKFILL] Inserted {inserted} rate rows");
    Ok(inserted)
}

/// `ensure_fx_coverage` that never fails the caller: transaction entry and
/// rate refresh must succeed even when the network is down. Uncovered days
/// keep falling back to current rates at read time until the next attempt.
pub async fn ensure_fx_coverage_best_effort(db: &Database) {
    if let Err(e) = ensure_fx_coverage(db).await {
        log::warn!("[FX_BACKFILL] WARNING: backfill failed (will retry on next refresh): {e}");
    }
}

fn date_string(day: i64) -> String {
    chrono::DateTime::from_timestamp(day, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn day_from_date(date: &str) -> i64 {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| d.and_hms_opt(0, 0, 0).map(|dt| dt.and_utc().timestamp()))
        .ok()
        .flatten()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: i64 = 86_400;
    const TODAY: i64 = 1_787_702_400; // 2026-08-26 UTC midnight

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
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
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                booking_date INTEGER NOT NULL,
                currency TEXT NOT NULL
            );
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY, currency TEXT NOT NULL);
            CREATE TABLE stock_data (ticker TEXT PRIMARY KEY, currency TEXT NOT NULL);
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY,
                purchase_price_currency TEXT NOT NULL,
                market_price_currency TEXT NOT NULL,
                monthly_rent_currency TEXT,
                purchase_date INTEGER
            );
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY, currency TEXT NOT NULL);
            "#,
        )
        .expect("schema");
        conn
    }

    fn add_stock_tx(conn: &Connection, id: &str, ccy: &str, date: i64) {
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
             VALUES (?1, 'inv', 'buy', 'AAA', 'A', '1', '100', ?2, ?3)",
            rusqlite::params![id, ccy, date],
        )
        .expect("tx");
    }

    fn add_rate(conn: &Connection, day: i64, ccy: &str) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, 20.0)",
            rusqlite::params![day, ccy],
        )
        .expect("rate");
    }

    fn add_bank_tx(conn: &Connection, id: &str, ccy: &str, date: i64) {
        conn.execute(
            "INSERT INTO bank_transactions (id, booking_date, currency) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, date, ccy],
        )
        .expect("bank tx");
    }

    /// Final review I-4: foreign bank-account history needs rates too.
    #[test]
    fn bank_transaction_currencies_are_covered() {
        let conn = setup_db();
        add_bank_tx(&conn, "b1", "gbp", TODAY - 30 * D);
        add_bank_tx(&conn, "b2", "CZK", TODAY - 900 * D);
        assert_eq!(held_currencies(&conn).expect("currencies"), vec!["GBP"]);
    }

    #[test]
    fn bank_only_history_fetches_from_its_earliest_booking() {
        let conn = setup_db();
        add_bank_tx(&conn, "b1", "GBP", TODAY - 400 * D);
        add_bank_tx(&conn, "b2", "CZK", TODAY - 900 * D);

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(TODAY - 410 * D, TODAY)]);
    }

    #[test]
    fn a_currency_covered_later_than_its_history_gets_its_prefix() {
        // USD is backfilled for the investments; GBP only has the rows the
        // daily writer recorded since the app's first run.
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 110 * D, "USD");
        add_rate(&conn, TODAY, "USD");
        add_bank_tx(&conn, "b1", "GBP", TODAY - 300 * D);
        add_rate(&conn, TODAY - 5 * D, "GBP");
        add_rate(&conn, TODAY, "GBP");

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(TODAY - 310 * D, TODAY - 5 * D)]);
    }

    #[test]
    fn a_currency_without_a_source_quote_is_never_fetched() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 110 * D, "USD");
        add_rate(&conn, TODAY, "USD");
        add_bank_tx(&conn, "b1", "KWD", TODAY - 500 * D);

        assert!(missing_fx_ranges(&conn, TODAY).expect("ranges").is_empty());
    }

    #[test]
    fn held_currencies_cover_balances_quotes_properties_and_pegs() {
        let conn = setup_db();
        conn.execute_batch(
            "
            INSERT INTO bank_accounts (id, currency) VALUES ('a', 'CHF');
            INSERT INTO stock_data (ticker, currency) VALUES ('CSPX.L', 'USD');
            INSERT INTO real_estate (id, purchase_price_currency, market_price_currency, monthly_rent_currency) VALUES ('r', 'PLN', 'PLN', NULL);
            INSERT INTO user_profile (id, currency) VALUES (1, 'AED');
            INSERT INTO bank_accounts (id, currency) VALUES ('b', 'KWD');
        ",
        )
        .expect("seed");
        assert_eq!(
            held_currencies(&conn).expect("held"),
            vec!["CHF", "PLN", "USD"]
        );
    }

    #[test]
    fn a_pegged_currency_is_covered_through_its_anchor_and_dates_count_for_it() {
        let conn = setup_db();
        add_bank_tx(&conn, "b1", "AED", TODAY - 300 * D);
        add_rate(&conn, TODAY - 5 * D, "USD");
        add_rate(&conn, TODAY, "USD");

        assert_eq!(held_currencies(&conn).expect("held"), vec!["USD"]);
        assert_eq!(
            missing_fx_ranges(&conn, TODAY).expect("ranges"),
            vec![(TODAY - 310 * D, TODAY - 5 * D)]
        );
    }

    #[test]
    fn a_property_purchase_date_extends_the_history_it_needs() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO real_estate (id, purchase_price_currency, market_price_currency, purchase_date)
             VALUES ('r', 'PLN', 'PLN', ?1)",
            [TODAY - 400 * D],
        )
        .expect("seed");
        add_rate(&conn, TODAY - 20 * D, "PLN");
        add_rate(&conn, TODAY, "PLN");

        assert_eq!(
            missing_fx_ranges(&conn, TODAY).expect("ranges"),
            vec![(TODAY - 410 * D, TODAY - 20 * D)]
        );
    }

    #[test]
    fn a_balance_without_dated_facts_needs_only_the_tail() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO bank_accounts (id, currency) VALUES ('a', 'CHF')",
            [],
        )
        .expect("seed");
        add_rate(&conn, TODAY - 200 * D, "CHF");
        add_rate(&conn, TODAY - 30 * D, "CHF");

        assert_eq!(
            missing_fx_ranges(&conn, TODAY).expect("ranges"),
            vec![(TODAY - 30 * D, TODAY)]
        );
    }

    #[test]
    fn a_balance_without_any_history_fetches_the_recent_window() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO bank_accounts (id, currency) VALUES ('a', 'CHF')",
            [],
        )
        .expect("seed");

        assert_eq!(
            missing_fx_ranges(&conn, TODAY).expect("ranges"),
            vec![(TODAY - 10 * D, TODAY)]
        );
    }

    #[test]
    fn stale_currencies_lists_held_currencies_quoted_over_two_weeks_ago() {
        let conn = setup_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, currency) VALUES ('a', 'CHF'), ('b', 'GBP'), ('c', 'PLN');",
        )
        .expect("seed");
        add_rate(&conn, TODAY - 20 * D, "CHF");
        add_rate(&conn, TODAY - 2 * D, "GBP");
        // PLN was never quoted: reported as missing elsewhere, not as stale.

        assert_eq!(stale_currencies(&conn, TODAY).expect("stale"), vec!["CHF"]);
    }

    #[test]
    fn held_currencies_collects_distinct_non_czk_from_both_tables() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_stock_tx(&conn, "t2", "USD", TODAY - 90 * D);
        add_stock_tx(&conn, "t3", "CZK", TODAY - 80 * D);
        conn.execute(
            "INSERT INTO crypto_transactions
             (id, investment_id, type, ticker, name, quantity, price_per_unit, currency, transaction_date)
             VALUES ('c1', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '1', 'EUR', ?1)",
            [TODAY - 50 * D],
        )
        .expect("tx");

        let mut symbols = held_currencies(&conn).expect("currencies");
        symbols.sort();
        assert_eq!(symbols, vec!["EUR".to_string(), "USD".to_string()]);
    }

    #[test]
    fn no_transactions_means_nothing_to_fetch() {
        let conn = setup_db();
        assert!(missing_fx_ranges(&conn, TODAY).expect("ranges").is_empty());
    }

    #[test]
    fn no_history_at_all_fetches_from_lookback_before_earliest_tx() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(TODAY - 110 * D, TODAY)]);
    }

    #[test]
    fn prefix_gap_fetches_up_to_coverage_start() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 30 * D, "USD");
        add_rate(&conn, TODAY, "USD");

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(TODAY - 110 * D, TODAY - 30 * D)]);
    }

    #[test]
    fn tail_gap_fetches_since_last_covered_day() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 110 * D, "USD");
        add_rate(&conn, TODAY - 7 * D, "USD");

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(TODAY - 7 * D, TODAY)]);
    }

    #[test]
    fn short_tail_gap_within_walkback_is_tolerated() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 110 * D, "USD");
        add_rate(&conn, TODAY - 2 * D, "USD");

        assert!(missing_fx_ranges(&conn, TODAY).expect("ranges").is_empty());
    }

    #[test]
    fn covered_timeseries_needs_nothing() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", TODAY - 100 * D);
        add_rate(&conn, TODAY - 110 * D, "USD");
        add_rate(&conn, TODAY, "USD");

        assert!(missing_fx_ranges(&conn, TODAY).expect("ranges").is_empty());
    }

    #[test]
    fn czk_only_transactions_need_no_rates() {
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "CZK", TODAY - 100 * D);

        assert!(missing_fx_ranges(&conn, TODAY).expect("ranges").is_empty());
    }

    #[test]
    fn range_never_starts_before_ecb_epoch() {
        // A transaction from 1995: frankfurter has no data before 1999-01-04.
        let conn = setup_db();
        add_stock_tx(&conn, "t1", "USD", 800_000_000); // 1995-05-09

        let ranges = missing_fx_ranges(&conn, TODAY).expect("ranges");

        assert_eq!(ranges, vec![(ECB_EPOCH, TODAY)]);
    }

    #[test]
    fn parse_frankfurter_inverts_eur_base_to_czk_base() {
        let body = serde_json::json!({
            "base": "EUR",
            "rates": {
                "2018-05-16": { "CZK": 25.5, "USD": 1.18 },
                "2018-05-17": { "CZK": 25.6, "USD": 1.20 },
                "2018-05-18": { "USD": 1.21 } // no CZK -> skipped
            }
        });

        let mut rows = parse_frankfurter_rates(&body, &["USD".to_string(), "EUR".to_string()]);
        rows.sort_by_key(|r| (r.0, r.1.clone()));

        let d16 = day_from_date("2018-05-16");
        let d17 = day_from_date("2018-05-17");
        assert_eq!(rows.len(), 4);
        // USD/CZK = CZK-per-EUR / USD-per-EUR
        assert_eq!(rows[0], (d16, "EUR".to_string(), 25.5));
        assert!((rows[1].2 - 25.5 / 1.18).abs() < 1e-9 && rows[1].1 == "USD" && rows[1].0 == d16);
        assert_eq!(rows[2], (d17, "EUR".to_string(), 25.6));
        assert!((rows[3].2 - 25.6 / 1.20).abs() < 1e-9);
    }

    #[test]
    fn store_keeps_existing_rows_and_inserts_new() {
        let conn = setup_db();
        add_rate(&conn, TODAY, "USD"); // live-recorded 20.0

        let inserted = store_fx_history(
            &conn,
            &[
                (TODAY, "USD".to_string(), 99.0),     // conflict: kept as 20.0
                (TODAY - D, "USD".to_string(), 21.0), // new
            ],
        )
        .expect("store");

        assert_eq!(inserted, 1);
        let kept: f64 = conn
            .query_row(
                "SELECT rate FROM exchange_rate_history WHERE date = ?1 AND currency = 'USD'",
                [TODAY],
                |r| r.get(0),
            )
            .expect("row");
        assert!((kept - 20.0).abs() < 1e-9);
    }
}
