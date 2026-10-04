//! MCP tools: portfolio. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

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
    // Exchange rates
    let rates: Vec<(String, f64)> = {
        let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    let czk_rate = |currency: &str, amount: f64| -> f64 {
        if currency == "CZK" {
            return amount;
        }
        let rate = rates
            .iter()
            .find(|(c, _)| c == currency)
            .map(|(_, r)| *r)
            .unwrap_or(1.0);
        amount * rate
    };

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
            .map(|(bal, cur)| czk_rate(&cur, bal.parse::<f64>().unwrap_or(0.0)))
            .sum();
        result
    };

    // Loans, valued at the amortized balance as of today
    let loans: f64 = crate::services::loans::outstanding_by_currency(
        conn,
        crate::services::loan_amortization::today_utc_day(),
    )?
    .into_iter()
    .map(|(currency, balance)| czk_rate(&currency, balance))
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
                czk_rate(&cc, val)
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
                czk_rate(&cc, val)
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
                czk_rate(&c, val)
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
            let val = czk_rate(&cur, price.parse::<f64>().unwrap_or(0.0));
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
                czk_rate(&c, val)
            })
            .sum();
        result
    };

    let total_assets =
        savings + investments + crypto + bonds + re_personal_final + re_investment + other;
    let net_worth = total_assets - loans;

    Ok(serde_json::json!({
        "net_worth_czk": format!("{:.2}", net_worth),
        "total_assets_czk": format!("{:.2}", total_assets),
        "total_liabilities_czk": format!("{:.2}", loans),
        "breakdown": {
            "savings_czk": format!("{:.2}", savings),
            "investments_czk": format!("{:.2}", investments),
            "crypto_czk": format!("{:.2}", crypto),
            "bonds_czk": format!("{:.2}", bonds),
            "real_estate_personal_czk": format!("{:.2}", re_personal_final),
            "real_estate_investment_czk": format!("{:.2}", re_investment),
            "other_assets_czk": format!("{:.2}", other),
            "loans_czk": format!("{:.2}", loans),
        },
        "note": "All values in CZK."
    }))
}

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
    let rows: Vec<Value> = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "totalSavings": row.get::<_, String>(1)?,
                "totalLoansPrincipal": row.get::<_, String>(2)?,
                "totalInvestments": row.get::<_, String>(3)?,
                "totalCrypto": row.get::<_, String>(4)?,
                "totalBonds": row.get::<_, String>(5)?,
                "totalRealEstatePersonal": row.get::<_, String>(6)?,
                "totalRealEstateInvestment": row.get::<_, String>(7)?,
                "totalOtherAssets": row.get::<_, String>(8)?,
                "recordedAt": row.get::<_, i64>(9)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Value::Array(rows))
}
