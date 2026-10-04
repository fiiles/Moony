//! MCP tools: portfolio. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use super::money::MoneyContext;
use crate::error::Result;

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

pub fn portfolio_metrics(conn: &Connection, exclude_personal_real_estate: bool) -> Result<Value> {
    let exclude = exclude_personal_real_estate;
    // One MoneyContext per call: all arithmetic stays in CZK, converted to the
    // main currency only when the response is built.
    let ctx = MoneyContext::load(conn)?;

    // Bank accounts (savings)
    let savings: f64 = {
        let mut stmt = conn.prepare(
            "SELECT balance, currency FROM bank_accounts WHERE exclude_from_balance = 0",
        )?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .filter_map(|r| r.ok())
            .map(|(bal, cur)| ctx.to_czk(bal.parse::<f64>().unwrap_or(0.0), &cur))
            .sum();
        result
    };

    // Loans, valued at the amortized balance as of today
    let loans: f64 = crate::services::loans::outstanding_by_currency(
        conn,
        crate::services::loan_amortization::today_utc_day(),
    )?
    .into_iter()
    .map(|(currency, balance)| ctx.to_czk(balance, &currency))
    .sum();

    // Stocks
    let investments: f64 = {
        let mut stmt = conn.prepare(
            // Same rule as services::pricing: the override wins unless the API price is newer.
            "SELECT si.quantity, COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.price ELSE sd.original_price END, '0') AS cp,
                COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.currency ELSE sd.currency END, 'USD') AS cc
         FROM stock_investments si
         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
         LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker",
        )?;
        let result = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(q, cp, cc)| {
                let val = q.parse::<f64>().unwrap_or(0.0) * cp.parse::<f64>().unwrap_or(0.0);
                ctx.to_czk(val, &cc)
            })
            .sum();
        result
    };

    // Crypto
    let crypto: f64 = {
        let mut stmt = conn.prepare(
            "SELECT ci.quantity, COALESCE(CASE WHEN cpo.price IS NOT NULL AND (cp.fetched_at IS NULL OR cp.fetched_at <= cpo.updated_at) THEN cpo.price ELSE cp.price END, '0') AS cp,
                COALESCE(CASE WHEN cpo.price IS NOT NULL AND (cp.fetched_at IS NULL OR cp.fetched_at <= cpo.updated_at) THEN cpo.currency ELSE cp.currency END, 'USD') AS cc
         FROM crypto_investments ci
         LEFT JOIN crypto_prices cp ON ci.ticker = cp.symbol
         LEFT JOIN crypto_price_overrides cpo ON ci.ticker = cpo.symbol",
        )?;
        let result = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(q, cp, cc)| {
                let val = q.parse::<f64>().unwrap_or(0.0) * cp.parse::<f64>().unwrap_or(0.0);
                ctx.to_czk(val, &cc)
            })
            .sum();
        result
    };

    // Bonds
    let bonds: f64 = {
        let mut stmt = conn.prepare("SELECT coupon_value, quantity, currency FROM bonds")?;
        let result = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(cv, q, c)| {
                let val = cv.parse::<f64>().unwrap_or(0.0) * q.parse::<f64>().unwrap_or(0.0);
                ctx.to_czk(val, &c)
            })
            .sum();
        result
    };

    // Real estate
    let (re_personal, re_investment): (f64, f64) = {
        let mut stmt =
            conn.prepare("SELECT market_price, market_price_currency, type FROM real_estate")?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut personal = 0.0_f64;
        let mut investment = 0.0_f64;
        for (price, cur, typ) in rows {
            let val = ctx.to_czk(price.parse::<f64>().unwrap_or(0.0), &cur);
            if typ == "personal" {
                personal += val;
            } else {
                investment += val;
            }
        }
        (personal, investment)
    };

    let re_personal_final = if exclude { 0.0 } else { re_personal };

    // Other assets
    let other: f64 = {
        let mut stmt = conn.prepare("SELECT quantity, market_price, currency FROM other_assets")?;
        let result = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(q, mp, c)| {
                let val = q.parse::<f64>().unwrap_or(0.0) * mp.parse::<f64>().unwrap_or(0.0);
                ctx.to_czk(val, &c)
            })
            .sum();
        result
    };

    let total_assets =
        savings + investments + crypto + bonds + re_personal_final + re_investment + other;
    let net_worth = total_assets - loans;

    // Amounts are converted to the user's main currency here, once, at
    // serialisation time.
    let money = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "net_worth": money(net_worth),
        "total_assets": money(total_assets),
        "total_liabilities": money(loans),
        "breakdown": {
            "savings": money(savings),
            "investments": money(investments),
            "crypto": money(crypto),
            "bonds": money(bonds),
            "real_estate_personal": money(re_personal_final),
            "real_estate_investment": money(re_investment),
            "other_assets": money(other),
            "loans": money(loans),
        },
    }))
}

/// The CZK total columns of `portfolio_metrics_history` and their response
/// keys, in the order the query selects them (columns 1 through 8).
const HISTORY_TOTALS: [&str; 8] = [
    "totalSavings",
    "totalLoansPrincipal",
    "totalInvestments",
    "totalCrypto",
    "totalBonds",
    "totalRealEstatePersonal",
    "totalRealEstateInvestment",
    "totalOtherAssets",
];

pub fn portfolio_history(
    conn: &Connection,
    start_date: Option<i64>,
    end_date: Option<i64>,
    limit: i64,
) -> Result<Value> {
    let mut conditions = Vec::new();
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_date {
        conditions.push("recorded_at >= ?");
        p.push(Box::new(s));
    }
    if let Some(e) = end_date {
        conditions.push("recorded_at <= ?");
        p.push(Box::new(e));
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };
    let sql = format!(
        "SELECT id, total_savings, total_loans_principal, total_investments,
            total_crypto, total_bonds, total_real_estate_personal,
            total_real_estate_investment, total_other_assets, recorded_at
     FROM portfolio_metrics_history {} ORDER BY recorded_at DESC LIMIT ?",
        where_clause
    );
    p.push(Box::new(limit));
    let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let snapshots: Vec<(String, Vec<rusqlite::types::Value>, i64)> = stmt
        .query_map(refs.as_slice(), |row| {
            let totals = (1..=HISTORY_TOTALS.len())
                .map(|i| row.get::<_, rusqlite::types::Value>(i))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((
                row.get::<_, String>(0)?,
                totals,
                row.get::<_, i64>(HISTORY_TOTALS.len() + 1)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // The snapshot totals are CZK. Each row converts at the rates of its own
    // day (ADR 0001), one rates query for the whole span.
    let ctx = MoneyContext::load(conn)?;
    let days: Vec<i64> = snapshots.iter().map(|(_, _, day)| *day).collect();
    let day_rates = ctx.day_rates_for(conn, &days);

    let rows: Vec<Value> = snapshots
        .into_iter()
        .map(|(id, totals, recorded_at)| {
            let mut row = serde_json::Map::new();
            row.insert("id".to_string(), Value::String(id));
            for (key, cell) in HISTORY_TOTALS.iter().zip(totals) {
                row.insert(
                    (*key).to_string(),
                    ctx.stored_czk_to_main_on(cell, recorded_at, &day_rates),
                );
            }
            row.insert("recordedAt".to_string(), serde_json::json!(recorded_at));
            Value::Object(row)
        })
        .collect();
    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "history": rows,
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
                total_other_assets TEXT NOT NULL DEFAULT '0'
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
            CREATE TABLE stock_investments (ticker TEXT PRIMARY KEY, quantity TEXT NOT NULL);
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
            CREATE TABLE crypto_investments (ticker TEXT PRIMARY KEY, quantity TEXT NOT NULL);
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
        // The same CZK total on both days: only the day's rate differs.
        add_snapshot(&conn, "s1", 100 * DAY, "2400");
        add_snapshot(&conn, "s2", 101 * DAY, "2400");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        let rows = result["history"].as_array().expect("history array");
        assert_eq!(rows.len(), 2);
        // Newest first, as before.
        assert_eq!(rows[0]["id"], "s2");
        assert_eq!(rows[0]["totalSavings"], "92.31");
        assert_eq!(rows[0]["totalLoansPrincipal"], "46.15");
        assert_eq!(rows[1]["id"], "s1");
        assert_eq!(rows[1]["totalSavings"], "100.00");
        assert_eq!(rows[1]["totalLoansPrincipal"], "50.00");
        assert_eq!(rows[1]["recordedAt"], 100 * DAY);
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
    fn portfolio_history_keeps_the_stored_czk_text_for_a_czk_main_currency() {
        let conn = setup_db("CZK");
        add_day_rate(&conn, 100 * DAY, "EUR", 24.0);
        add_snapshot(&conn, "s1", 100 * DAY, "2400.126");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["history"][0]["totalSavings"], "2400.126");
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
        assert_eq!(limited["history"][0]["id"], "s3");
        assert_eq!(limited["history"].as_array().expect("array").len(), 1);
    }

    #[test]
    fn portfolio_history_without_rows_still_names_the_main_currency() {
        let conn = setup_db("EUR");

        let result = portfolio_history(&conn, None, None, 365).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["history"], serde_json::json!([]));
    }
}
