//! MCP tools: portfolio, on the app's net worth service (ADR 0006, ADR 0011).

use rmcp::schemars;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;

use super::money::{amount_json, load_main_currency};
use crate::error::Result;
use crate::services::net_worth;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PortfolioHistoryArgs {
    #[serde(rename = "startDate")]
    #[schemars(description = "Start date as Unix timestamp (seconds)")]
    pub start_date: Option<i64>,
    #[serde(rename = "endDate")]
    #[schemars(description = "End date as Unix timestamp (seconds)")]
    pub end_date: Option<i64>,
    #[schemars(description = "Max records to return (default 365, max 3650)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PortfolioMetricsArgs {
    #[serde(rename = "excludePersonalRealEstate")]
    #[schemars(description = "Exclude personal-use real estate from net worth calculation")]
    pub exclude_personal_real_estate: Option<bool>,
}

/// Today's net worth in the user's main currency: the app's net worth service with the app's
/// current quote snapshot (C-A), so the assistant and the app agree. A class that could not
/// be converted at all is `null`; the totals leave unconverted amounts out and
/// `missingCurrencies` names them. When the asset classes (or the loans) hold amounts but none
/// converted, their total and the net worth are `null` too (never a misleading "0").
pub fn portfolio_metrics(conn: &Connection, exclude_personal_real_estate: bool) -> Result<Value> {
    let main = load_main_currency(conn)?;
    let worth = net_worth::current_net_worth(conn, &main, exclude_personal_real_estate)?;
    let currency = worth.currency.as_str();
    let classes = &worth.classes;
    let personal = if exclude_personal_real_estate {
        Some(0.0)
    } else {
        classes.real_estate_personal
    };

    Ok(serde_json::json!({
        "mainCurrency": currency,
        "net_worth": amount_json(worth.net_worth, currency),
        // No totals when classes hold amounts but none converted (a 0 would read as real)
        "total_assets": amount_json(worth.total_assets, currency),
        "total_liabilities": amount_json(worth.total_liabilities, currency),
        "breakdown": {
            "savings": amount_json(classes.savings, currency),
            "investments": amount_json(classes.investments, currency),
            "crypto": amount_json(classes.crypto, currency),
            "bonds": amount_json(classes.bonds, currency),
            "real_estate_personal": amount_json(personal, currency),
            "real_estate_investment": amount_json(classes.real_estate_investment, currency),
            "other_assets": amount_json(classes.other_assets, currency),
            "loans": amount_json(classes.loans, currency),
        },
        "missingCurrencies": worth.quality.missing_currencies(),
    }))
}

/// Recorded snapshot days, newest first, at most `limit`, in the user's main currency: each
/// day's native breakdowns converted at that day's rates (the app's history service).
/// `missingCurrencies` covers the returned days only.
pub fn portfolio_history(
    conn: &Connection,
    start_date: Option<i64>,
    end_date: Option<i64>,
    limit: i64,
) -> Result<Value> {
    let main = load_main_currency(conn)?;
    // The `limit` newest days start at the limit-th newest snapshot of the range; converting
    // only from there keeps days outside the answer out of `missingCurrencies`.
    let limit_start: Option<i64> = conn
        .query_row(
            "SELECT recorded_at FROM portfolio_metrics_history
             WHERE (?1 IS NULL OR recorded_at >= ?1) AND (?2 IS NULL OR recorded_at <= ?2)
             ORDER BY recorded_at DESC LIMIT 1 OFFSET ?3",
            rusqlite::params![start_date, end_date, (limit - 1).max(0)],
            |row| row.get(0),
        )
        .optional()?;
    let from = match (start_date, limit_start) {
        (Some(start), Some(limit_start)) => Some(start.max(limit_start)),
        (start, limit_start) => start.or(limit_start),
    };
    let (points, quality) = net_worth::history_in(conn, &main, from, end_date)?;
    let currency = main.as_str();

    let rows: Vec<Value> = points
        .iter()
        .rev()
        .take(usize::try_from(limit).unwrap_or(0))
        .map(|point| {
            let c = &point.classes;
            serde_json::json!({
                "recordedAt": point.recorded_at,
                "source": point.source,
                "totalSavings": amount_json(c.savings, currency),
                "totalLoansPrincipal": amount_json(c.loans, currency),
                "totalInvestments": amount_json(c.investments, currency),
                "totalCrypto": amount_json(c.crypto, currency),
                "totalBonds": amount_json(c.bonds, currency),
                "totalRealEstatePersonal": amount_json(c.real_estate_personal, currency),
                "totalRealEstateInvestment": amount_json(c.real_estate_investment, currency),
                "totalOtherAssets": amount_json(c.other_assets, currency),
                "netWorth": amount_json(point.net_worth, currency),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "mainCurrency": currency,
        "history": rows,
        "missingCurrencies": quality.missing_currencies(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal schema for `portfolio_metrics`: every table it reads, plus the
    /// rates and the profile that the money context needs.
    fn setup_db(main_currency: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE exchange_rates (
                currency TEXT PRIMARY KEY,
                rate REAL NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY,
                total_savings TEXT NOT NULL,
                total_loans_principal TEXT NOT NULL,
                total_investments TEXT NOT NULL,
                total_crypto TEXT NOT NULL DEFAULT '0',
                total_bonds TEXT NOT NULL,
                total_real_estate_personal TEXT NOT NULL,
                total_real_estate_investment TEXT NOT NULL,
                recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
                total_other_assets TEXT NOT NULL DEFAULT '0',
                is_stale INTEGER NOT NULL DEFAULT 0,
                investments_by_currency TEXT NOT NULL DEFAULT '{}',
                crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}',
                bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
                loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
                source TEXT NOT NULL DEFAULT 'live'
            );
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                balance TEXT NOT NULL,
                currency TEXT NOT NULL,
                exclude_from_balance INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE loans (
                id TEXT PRIMARY KEY,
                principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                interest_rate TEXT NOT NULL DEFAULT '0',
                monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL DEFAULT (unixepoch()),
                end_date INTEGER,
                balance_anchor_amount TEXT,
                balance_anchor_date INTEGER
            );
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, ticker TEXT NOT NULL, quantity TEXT NOT NULL, currency TEXT NOT NULL);
            CREATE TABLE investment_transactions (investment_id TEXT, type TEXT, quantity TEXT, price_per_unit TEXT, currency TEXT, transaction_date INTEGER, created_at INTEGER);
            CREATE TABLE stock_data (
                ticker TEXT PRIMARY KEY,
                original_price TEXT,
                currency TEXT,
                fetched_at INTEGER
            );
            CREATE TABLE stock_price_overrides (
                ticker TEXT PRIMARY KEY,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE crypto_investments (id TEXT PRIMARY KEY, ticker TEXT NOT NULL, quantity TEXT NOT NULL, currency TEXT NOT NULL);
            CREATE TABLE crypto_transactions (investment_id TEXT, type TEXT, quantity TEXT, price_per_unit TEXT, currency TEXT, transaction_date INTEGER, created_at INTEGER);
            CREATE TABLE crypto_prices (
                symbol TEXT PRIMARY KEY,
                price TEXT,
                currency TEXT,
                fetched_at INTEGER
            );
            CREATE TABLE crypto_price_overrides (
                symbol TEXT PRIMARY KEY,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE bonds (
                id TEXT PRIMARY KEY,
                coupon_value TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL
            );
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY,
                market_price TEXT NOT NULL,
                market_price_currency TEXT NOT NULL,
                type TEXT NOT NULL
            );
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY,
                quantity TEXT NOT NULL,
                market_price TEXT NOT NULL,
                currency TEXT NOT NULL
            );

            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES
                ('EUR', 25.0, 0), ('USD', 20.0, 0);
            -- 2000 EUR (50 000 CZK) + 25 000 CZK = 75 000 CZK of savings
            INSERT INTO bank_accounts (id, balance, currency) VALUES
                ('b1', '2000', 'EUR'), ('b2', '25000', 'CZK');
            -- 5 x 10 USD = 1 000 CZK
            INSERT INTO other_assets (id, quantity, market_price, currency)
                VALUES ('o1', '5', '10', 'USD');
            -- 100 USD = 2 000 CZK of personal real estate
            INSERT INTO real_estate (id, market_price, market_price_currency, type)
                VALUES ('r1', '100', 'USD', 'personal');
            "#,
        )
        .expect("schema + seed");
        conn.execute(
            "INSERT INTO user_profile (currency) VALUES (?1)",
            [main_currency],
        )
        .expect("profile");
        // The MCP converts with the app's current snapshot (C-A), mirroring the table above.
        crate::services::fx::set_current_for_test(&[("EUR", 25.0), ("USD", 20.0)]);
        conn
    }

    #[test]
    fn portfolio_metrics_reports_net_worth_in_the_main_currency() {
        let conn = setup_db("EUR");

        let result = portfolio_metrics(&conn, false).expect("metrics");

        assert_eq!(result["mainCurrency"], "EUR");
        // 78 000 CZK / 25
        assert_eq!(result["net_worth"], "3120.00");
        assert_eq!(result["total_assets"], "3120.00");
        assert_eq!(result["breakdown"]["savings"], "3000.00");
        assert_eq!(result["breakdown"]["other_assets"], "40.00");
        assert_eq!(result["breakdown"]["real_estate_personal"], "80.00");
        assert!(result.get("net_worth_czk").is_none());
        assert!(result.get("note").is_none());
    }

    #[test]
    fn portfolio_metrics_honours_exclude_personal_real_estate() {
        let conn = setup_db("EUR");

        let result = portfolio_metrics(&conn, true).expect("metrics");

        assert_eq!(result["net_worth"], "3040.00");
        assert_eq!(result["breakdown"]["real_estate_personal"], "0.00");
    }

    #[test]
    fn portfolio_metrics_stays_in_czk_for_a_czk_main_currency() {
        let conn = setup_db("CZK");

        let result = portfolio_metrics(&conn, false).expect("metrics");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["net_worth"], "78000.00");
        assert_eq!(result["breakdown"]["savings"], "75000.00");
    }

    #[test]
    fn portfolio_metrics_excludes_unconvertible_amounts_and_lists_their_currency() {
        let conn = setup_db("EUR");
        conn.execute(
            "INSERT INTO bank_accounts (id, balance, currency) VALUES ('b3', '999', 'KWD')",
            [],
        )
        .expect("kwd account");

        let result = portfolio_metrics(&conn, false).expect("metrics");

        // The KWD balance is left out (never counted 1:1) and declared.
        assert_eq!(result["breakdown"]["savings"], "3000.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }

    #[test]
    fn portfolio_metrics_classes_are_null_when_the_main_currency_has_no_rate() {
        let conn = setup_db("JPY");

        let result = portfolio_metrics(&conn, false).expect("metrics");

        assert_eq!(result["mainCurrency"], "JPY");
        // Nothing converts into JPY: no class value, no asset total (null, never "0"), JPY
        // reported. Without loans the liabilities are a real 0, the net worth still unknown.
        assert!(result["breakdown"]["savings"].is_null());
        assert!(result["breakdown"]["other_assets"].is_null());
        assert_eq!(result["breakdown"]["loans"], "0");
        assert!(result["net_worth"].is_null());
        assert!(result["total_assets"].is_null());
        assert_eq!(result["total_liabilities"], "0");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["JPY"]));
    }

    #[test]
    fn portfolio_metrics_amounts_use_the_iso_minor_units_of_the_main_currency() {
        for (main, balance, expected) in [("JPY", "123.6", "124"), ("KWD", "12.3456", "12.346")] {
            let conn = setup_db(main);
            conn.execute(
                "INSERT INTO bank_accounts (id, balance, currency) VALUES ('own', ?1, ?2)",
                [balance, main],
            )
            .expect("account in the main currency");

            let result = portfolio_metrics(&conn, false).expect("metrics");

            // Identity needs no quote, so only the main-currency account converts.
            assert_eq!(result["breakdown"]["savings"], expected, "{main}");
            assert_eq!(result["net_worth"], expected, "{main}");
            assert_eq!(result["total_assets"], expected, "{main}");
        }
    }

    #[test]
    fn complete_portfolio_metrics_declare_no_missing_currencies() {
        let result = portfolio_metrics(&setup_db("EUR"), false).expect("metrics");
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
    }

    const DAY: i64 = 86_400;

    fn add_snapshot(conn: &Connection, id: &str, day: i64, savings_czk: &str) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history
                (id, total_savings, total_loans_principal, total_investments, total_crypto,
                 total_bonds, total_real_estate_personal, total_real_estate_investment,
                 recorded_at, total_other_assets)
             VALUES (?1, ?2, '1200', '0', '0', '0', '0', '0', ?3, '0')",
            rusqlite::params![id, savings_czk, day],
        )
        .expect("snapshot");
    }

    fn add_day_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )
        .expect("day rate");
    }

    #[test]
    fn portfolio_history_converts_each_row_at_its_own_day_rate() {
        let conn = setup_db("EUR");
        add_day_rate(&conn, 100 * DAY, "EUR", 24.0);
        add_day_rate(&conn, 101 * DAY, "EUR", 26.0);
        // Legacy rows (CZK totals, no breakdowns): the same CZK total on both days, only the
        // day's rate differs.
        add_snapshot(&conn, "s1", 100 * DAY, "2400");
        add_snapshot(&conn, "s2", 101 * DAY, "2400");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        let rows = result["history"].as_array().expect("history array");
        assert_eq!(rows.len(), 2);
        // Newest first, as before.
        assert_eq!(rows[0]["recordedAt"], 101 * DAY);
        assert_eq!(rows[0]["totalSavings"], "92.31");
        assert_eq!(rows[0]["totalLoansPrincipal"], "46.15");
        assert_eq!(rows[0]["netWorth"], "46.15");
        assert_eq!(rows[1]["recordedAt"], 100 * DAY);
        assert_eq!(rows[1]["totalSavings"], "100.00");
        assert_eq!(rows[1]["totalLoansPrincipal"], "50.00");
        assert_eq!(rows[1]["source"], "live");
    }

    #[test]
    fn portfolio_history_converts_the_native_breakdowns() {
        let conn = setup_db("EUR");
        add_day_rate(&conn, 100 * DAY, "EUR", 25.0);
        add_day_rate(&conn, 100 * DAY, "USD", 20.0);
        conn.execute(
            "INSERT INTO portfolio_metrics_history
                (id, total_savings, total_loans_principal, total_investments, total_crypto,
                 total_bonds, total_real_estate_personal, total_real_estate_investment,
                 recorded_at, savings_by_currency, investments_by_currency)
             VALUES ('s1', '0', '0', '0', '0', '0', '0', '0', ?1, '{\"EUR\":100}',
                     '{\"USD\":50,\"KWD\":7}')",
            [100 * DAY],
        )
        .expect("snapshot");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        let row = &result["history"][0];
        assert_eq!(row["totalSavings"], "100.00");
        // 50 USD = 1000 CZK = 40 EUR; the KWD amount is left out and reported
        assert_eq!(row["totalInvestments"], "40.00");
        assert_eq!(row["netWorth"], "140.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }

    #[test]
    fn portfolio_history_row_without_a_snapshot_uses_the_closest_earlier_day() {
        let conn = setup_db("EUR");
        add_day_rate(&conn, 100 * DAY, "EUR", 24.0);
        // Three days after the only snapshot (a weekend, say).
        add_snapshot(&conn, "s1", 103 * DAY, "2400");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["history"][0]["totalSavings"], "100.00");
    }

    #[test]
    fn portfolio_history_in_czk_rounds_the_stored_czk_total() {
        let conn = setup_db("CZK");
        add_snapshot(&conn, "s1", 100 * DAY, "2400.126");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["history"][0]["totalSavings"], "2400.13");
    }

    #[test]
    fn portfolio_history_applies_the_date_filters_and_limit() {
        let conn = setup_db("EUR");
        add_snapshot(&conn, "s1", 100 * DAY, "2500");
        add_snapshot(&conn, "s2", 101 * DAY, "2500");
        add_snapshot(&conn, "s3", 102 * DAY, "2500");

        let windowed =
            portfolio_history(&conn, Some(101 * DAY), Some(102 * DAY), 365).expect("history");
        assert_eq!(windowed["history"].as_array().expect("array").len(), 2);

        let limited = portfolio_history(&conn, None, None, 1).expect("history");
        assert_eq!(limited["history"][0]["recordedAt"], 102 * DAY);
        assert_eq!(limited["history"].as_array().expect("array").len(), 1);

        let limited_window =
            portfolio_history(&conn, Some(100 * DAY), Some(101 * DAY), 1).expect("history");
        assert_eq!(limited_window["history"][0]["recordedAt"], 101 * DAY);
        assert_eq!(
            limited_window["history"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn portfolio_history_reports_only_the_returned_days() {
        let conn = setup_db("EUR");
        add_day_rate(&conn, 101 * DAY, "EUR", 25.0);
        // The older day cannot convert (no rate); it is outside a limit of one.
        add_snapshot(&conn, "s1", 90 * DAY, "2500");
        add_snapshot(&conn, "s2", 101 * DAY, "2500");

        let limited = portfolio_history(&conn, None, None, 1).expect("history");
        assert_eq!(limited["missingCurrencies"], serde_json::json!([]));

        let all = portfolio_history(&conn, None, None, 365).expect("history");
        assert!(all["history"][1]["totalSavings"].is_null());
        assert_eq!(all["missingCurrencies"], serde_json::json!(["EUR"]));
    }

    #[test]
    fn portfolio_history_without_rows_still_names_the_main_currency() {
        let conn = setup_db("EUR");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["history"], serde_json::json!([]));
    }
}
