//! MCP tools: analytics. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CashflowArgs {
    #[serde(rename = "viewType")]
    #[schemars(
        description = "Whether to show monthly or yearly amounts: monthly (default) or yearly"
    )]
    pub view_type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BudgetingArgs {
    #[serde(rename = "startDate")]
    #[schemars(description = "Start date as Unix timestamp (seconds)")]
    pub start_date: i64,
    #[serde(rename = "endDate")]
    #[schemars(description = "End date as Unix timestamp (seconds)")]
    pub end_date: i64,
    #[schemars(
        description = "Budget timeframe to compare against: monthly (default), quarterly, or yearly"
    )]
    pub timeframe: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TagMetricsArgs {
    #[serde(rename = "tagIds")]
    #[schemars(description = "Filter to specific tag IDs. Omit to get metrics for all tags.")]
    pub tag_ids: Option<Vec<String>>,
}

pub fn cashflow_report(conn: &Connection, view_type: Option<String>) -> Result<Value> {
    let view_type = view_type.unwrap_or_else(|| "monthly".to_string());
    let multiplier: f64 = if view_type == "yearly" { 12.0 } else { 1.0 };

    let rates: std::collections::HashMap<String, f64> = {
        let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    let czk = |currency: &str, amount: f64| -> f64 {
        if currency == "CZK" {
            return amount;
        }
        amount * rates.get(currency).copied().unwrap_or(1.0)
    };
    let to_monthly = |amount: f64, currency: &str, frequency: &str| -> f64 {
        let v = czk(currency, amount);
        match frequency {
            "monthly" => v,
            "quarterly" => v / 3.0,
            "semi_annual" => v / 6.0,
            "annual" => v / 12.0,
            "weekly" => v * 4.33,
            "daily" => v * 30.0,
            _ => v,
        }
    };

    let mut income: Vec<Value> = Vec::new();
    let mut expenses: Vec<Value> = Vec::new();

    // Cashflow items
    {
        let mut stmt = conn.prepare(
            "SELECT name, amount, currency, frequency, item_type, category FROM cashflow_items",
        )?;
        let items: Vec<(String, f64, String, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, f64>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (name, amount, currency, frequency, item_type, category) in items {
            if frequency == "one_time" {
                continue;
            }
            let monthly = to_monthly(amount, &currency, &frequency);
            let entry =
                serde_json::json!({ "name": name, "monthly_czk": monthly, "category": category });
            if item_type == "income" {
                income.push(entry);
            } else {
                expenses.push(entry);
            }
        }
    }

    // Loan payments
    {
        let mut stmt = conn.prepare(
            "SELECT name, monthly_payment, currency FROM loans WHERE monthly_payment > 0",
        )?;
        let items: Vec<(String, f64, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get::<_, f64>(1)?, row.get(2)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (name, payment, currency) in items {
            let monthly = czk(&currency, payment);
            if monthly > 0.0 {
                expenses.push(serde_json::json!({ "name": format!("Loan: {}", name), "monthly_czk": monthly, "category": "loan_payment" }));
            }
        }
    }

    // Insurance premiums
    {
        let mut stmt = conn.prepare("SELECT policy_name, regular_payment, regular_payment_currency, payment_frequency FROM insurance_policies WHERE status = 'active' AND regular_payment > 0")?;
        let items: Vec<(String, f64, String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get::<_, f64>(1)?, row.get(2)?, row.get(3)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (name, payment, currency, frequency) in items {
            let monthly = to_monthly(payment, &currency, &frequency);
            if monthly > 0.0 {
                expenses.push(serde_json::json!({ "name": format!("Insurance: {}", name), "monthly_czk": monthly, "category": "insurance" }));
            }
        }
    }

    // Savings interest
    {
        let mut stmt = conn.prepare("SELECT name, balance, currency, interest_rate FROM bank_accounts WHERE interest_rate IS NOT NULL AND CAST(interest_rate AS REAL) > 0 AND exclude_from_balance = 0")?;
        let items: Vec<(String, f64, String, f64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, f64>(1)?,
                    row.get(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        for (name, balance, currency, rate) in items {
            let balance_czk = czk(&currency, balance);
            let monthly = balance_czk * (rate / 100.0) / 12.0;
            if monthly > 0.0 {
                income.push(serde_json::json!({ "name": format!("Interest: {}", name), "monthly_czk": monthly, "category": "interest" }));
            }
        }
    }

    let total_income: f64 = income
        .iter()
        .filter_map(|v| v["monthly_czk"].as_f64())
        .sum();
    let total_expenses: f64 = expenses
        .iter()
        .filter_map(|v| v["monthly_czk"].as_f64())
        .sum();

    let fmt_income: Vec<Value> = income
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v["name"], "category": v["category"],
                "amount_czk": format!("{:.2}", v["monthly_czk"].as_f64().unwrap_or(0.0) * multiplier)
            })
        })
        .collect();
    let fmt_expenses: Vec<Value> = expenses
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v["name"], "category": v["category"],
                "amount_czk": format!("{:.2}", v["monthly_czk"].as_f64().unwrap_or(0.0) * multiplier)
            })
        })
        .collect();

    Ok(serde_json::json!({
        "viewType": view_type,
        "summary": {
            "totalIncomeCzk": format!("{:.2}", total_income * multiplier),
            "totalExpensesCzk": format!("{:.2}", total_expenses * multiplier),
            "netCashflowCzk": format!("{:.2}", (total_income - total_expenses) * multiplier),
        },
        "income": fmt_income,
        "expenses": fmt_expenses,
    }))
}

pub fn budgeting_report(
    conn: &Connection,
    start_date: Option<i64>,
    end_date: Option<i64>,
    timeframe: Option<String>,
) -> Result<Value> {
    let timeframe = timeframe.unwrap_or_else(|| "monthly".to_string());
    let start_date = start_date.unwrap_or(0);
    let end_date = end_date.unwrap_or(i64::MAX);

    let rates: std::collections::HashMap<String, f64> = {
        let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    let czk = |currency: &str, amount: f64| -> f64 {
        if currency == "CZK" {
            return amount;
        }
        amount * rates.get(currency).copied().unwrap_or(1.0)
    };

    // Goals
    let goals: Vec<(String, String, f64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT bg.category_id, tc.name, bg.amount, bg.currency
             FROM budget_goals bg JOIN transaction_categories tc ON bg.category_id = tc.id
             WHERE bg.timeframe = ?",
        )?;
        let result = stmt
            .query_map([&timeframe], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get::<_, f64>(2)?, row.get(3)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    // Actual spending
    let spending: Vec<(String, f64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT bt.category_id, SUM(CAST(bt.amount AS REAL)), bt.currency
             FROM bank_transactions bt
             WHERE bt.booking_date >= ? AND bt.booking_date <= ? AND bt.tx_type = 'debit' AND bt.category_id IS NOT NULL
             GROUP BY bt.category_id, bt.currency",
        )?;
        let result = stmt
            .query_map(rusqlite::params![start_date, end_date], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    let mut spending_map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for (cat_id, amount, currency) in spending {
        *spending_map.entry(cat_id).or_insert(0.0) += czk(&currency, amount);
    }

    let report: Vec<Value> = goals
        .iter()
        .map(|(cat_id, cat_name, goal_amount, goal_currency)| {
            let budget_czk = czk(goal_currency, *goal_amount);
            let actual_czk = spending_map.get(cat_id).copied().unwrap_or(0.0);
            serde_json::json!({
                "categoryId": cat_id,
                "categoryName": cat_name,
                "budgetCzk": format!("{:.2}", budget_czk),
                "actualCzk": format!("{:.2}", actual_czk),
                "remainingCzk": format!("{:.2}", budget_czk - actual_czk),
                "usagePercent": if budget_czk > 0.0 { format!("{:.1}", (actual_czk / budget_czk) * 100.0) } else { "N/A".to_string() },
                "overBudget": actual_czk > budget_czk,
            })
        })
        .collect();

    Ok(serde_json::json!({ "timeframe": timeframe, "budgetCategories": report }))
}

pub fn stocks_analysis(conn: &Connection) -> Result<Value> {
    let rates: std::collections::HashMap<String, f64> = {
        let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    let czk = |currency: &str, amount: f64| -> f64 {
        if currency == "CZK" {
            return amount;
        }
        amount * rates.get(currency).copied().unwrap_or(1.0)
    };

    let cost_map = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?;

    let mut stmt = conn.prepare(
        "SELECT si.id, si.ticker, si.company_name, si.quantity,
                COALESCE(spo.price, sd.original_price, '0') AS current_price,
                COALESCE(spo.currency, sd.currency, 'USD') AS price_currency,
                COALESCE(do2.yearly_dividend_sum, dd.yearly_dividend_sum, '0') AS yearly_dividend,
                COALESCE(do2.currency, dd.currency, 'USD') AS dividend_currency
         FROM stock_investments si
         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
         LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker
         LEFT JOIN dividend_data dd ON si.ticker = dd.ticker
         LEFT JOIN dividend_overrides do2 ON si.ticker = do2.ticker",
    )?;

    let stocks: Vec<Value> = stmt
        .query_map([], |row| {
            let id: String = row.get(0)?;
            let ticker: String = row.get(1)?;
            let company: String = row.get(2)?;
            let qty: f64 = row.get::<_, String>(3)?.parse().unwrap_or(0.0);
            let curr_price: f64 = row.get::<_, String>(4)?.parse().unwrap_or(0.0);
            let price_currency: String = row.get(5)?;
            let yearly_div: f64 = row.get::<_, String>(6)?.parse().unwrap_or(0.0);
            let div_currency: String = row.get(7)?;
            Ok((
                id,
                ticker,
                company,
                qty,
                curr_price,
                price_currency,
                yearly_div,
                div_currency,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(id, ticker, company, qty, curr_price, price_currency, yearly_div, div_currency)| {
                let curr_val_czk = czk(&price_currency, qty * curr_price);
                // Historical cost: every transaction at its own day's rate
                // (ADR 0001) — never the stored scalar at today's rate.
                let cost_czk = cost_map.get(&id).map(|p| p.cost_basis_czk).unwrap_or(0.0);
                let gain_czk = curr_val_czk - cost_czk;
                let gain_pct = if cost_czk > 0.0 {
                    (gain_czk / cost_czk) * 100.0
                } else {
                    0.0
                };
                let div_czk = czk(&div_currency, qty * yearly_div);
                let div_yield = if curr_val_czk > 0.0 {
                    (div_czk / curr_val_czk) * 100.0
                } else {
                    0.0
                };
                serde_json::json!({
                    "id": id,
                    "ticker": ticker,
                    "companyName": company,
                    "quantity": qty,
                    "currentValueCzk": format!("{:.2}", curr_val_czk),
                    "costBasisCzk": format!("{:.2}", cost_czk),
                    "gainLossCzk": format!("{:.2}", gain_czk),
                    "gainLossPct": format!("{:.2}", gain_pct),
                    "annualDividendCzk": format!("{:.2}", div_czk),
                    "dividendYieldPct": format!("{:.2}", div_yield),
                })
            },
        )
        .collect();

    let total_val: f64 = stocks
        .iter()
        .filter_map(|v| v["currentValueCzk"].as_str()?.parse::<f64>().ok())
        .sum();
    let total_gain: f64 = stocks
        .iter()
        .filter_map(|v| v["gainLossCzk"].as_str()?.parse::<f64>().ok())
        .sum();
    let total_div: f64 = stocks
        .iter()
        .filter_map(|v| v["annualDividendCzk"].as_str()?.parse::<f64>().ok())
        .sum();

    Ok(serde_json::json!({
        "summary": {
            "totalValueCzk": format!("{:.2}", total_val),
            "totalGainLossCzk": format!("{:.2}", total_gain),
            "annualDividendsCzk": format!("{:.2}", total_div),
        },
        "stocks": stocks,
    }))
}

pub fn tag_metrics(conn: &Connection, tag_ids: Option<Vec<String>>) -> Result<Value> {
    let cost_map = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?;
    let rates: std::collections::HashMap<String, f64> = {
        let mut stmt = conn.prepare("SELECT currency, rate FROM exchange_rates")?;
        let result = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    let czk = |currency: &str, amount: f64| -> f64 {
        if currency == "CZK" {
            return amount;
        }
        amount * rates.get(currency).copied().unwrap_or(1.0)
    };

    let tags: Vec<(String, String)> = if let Some(ref ids) = tag_ids {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, name FROM stock_tags WHERE id IN ({}) ORDER BY name",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let p: Vec<Box<dyn rusqlite::ToSql>> = ids
            .iter()
            .map(|id| Box::new(id.to_string()) as Box<dyn rusqlite::ToSql>)
            .collect();
        let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
        let result = stmt
            .query_map(refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        result
    } else {
        let mut stmt = conn.prepare("SELECT id, name FROM stock_tags ORDER BY name")?;
        let result = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    let result: Vec<Value> = tags
        .iter()
        .filter_map(|(tag_id, tag_name)| {
            let stocks: Vec<(String, f64, f64, String, f64, String)> = {
                let mut s = conn
                    .prepare(
                        "SELECT si.id, si.quantity, COALESCE(spo.price, sd.original_price, '0') AS cp,
                                COALESCE(spo.currency, sd.currency, 'USD') AS cc,
                                COALESCE(do2.yearly_dividend_sum, dd.yearly_dividend_sum, '0') AS yd,
                                COALESCE(do2.currency, dd.currency, 'USD') AS dc
                         FROM stock_investments si
                         JOIN stock_investment_tags sit ON si.id = sit.investment_id
                         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
                         LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker
                         LEFT JOIN dividend_data dd ON si.ticker = dd.ticker
                         LEFT JOIN dividend_overrides do2 ON si.ticker = do2.ticker
                         WHERE sit.tag_id = ?",
                    )
                    .ok()?;
                let result = s
                    .query_map([tag_id], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?.parse::<f64>().unwrap_or(0.0),
                            row.get::<_, String>(2)?.parse::<f64>().unwrap_or(0.0),
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?.parse::<f64>().unwrap_or(0.0),
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .ok()?
                    .filter_map(|r| r.ok())
                    .collect();
                result
            };

            let mut val = 0.0_f64;
            let mut cost = 0.0_f64;
            let mut div = 0.0_f64;
            for (id, qty, cp, cc, yd, dc) in &stocks {
                val += czk(cc, qty * cp);
                cost += cost_map.get(id).map(|p| p.cost_basis_czk).unwrap_or(0.0);
                div += czk(dc, qty * yd);
            }
            let gain = val - cost;
            let gain_pct = if cost > 0.0 { (gain / cost) * 100.0 } else { 0.0 };
            let div_yield = if val > 0.0 { (div / val) * 100.0 } else { 0.0 };

            Some(serde_json::json!({
                "tagId": tag_id,
                "tagName": tag_name,
                "stockCount": stocks.len(),
                "totalValueCzk": format!("{:.2}", val),
                "costBasisCzk": format!("{:.2}", cost),
                "gainLossCzk": format!("{:.2}", gain),
                "gainLossPct": format!("{:.2}", gain_pct),
                "annualDividendCzk": format!("{:.2}", div),
                "dividendYieldPct": format!("{:.2}", div_yield),
            }))
        })
        .collect();

    Ok(Value::Array(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;
    const D1: i64 = 1_700_006_400;
    const D2: i64 = D1 + DAY;

    /// Minimal schema for the stocks analytics paths (post-migration-008:
    /// no stored `average_price`). Cost basis must come from transactions at
    /// their day's rates.
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(&format!(
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
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            CREATE TABLE exchange_rates (
                currency TEXT PRIMARY KEY,
                rate REAL NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE stock_data (
                ticker TEXT PRIMARY KEY,
                original_price TEXT,
                currency TEXT
            );
            CREATE TABLE stock_price_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE dividend_data (
                ticker TEXT PRIMARY KEY,
                yearly_dividend_sum TEXT,
                currency TEXT
            );
            CREATE TABLE dividend_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT,
                currency TEXT
            );
            CREATE TABLE stock_tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT,
                group_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE stock_investment_tags (
                investment_id TEXT NOT NULL,
                tag_id TEXT NOT NULL
            );

            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('USD', 23.0, 0);
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                ({d1}, 'USD', 25.0), ({d2}, 'USD', 20.0);
            INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                VALUES ('inv-a', 'AAA', 'A Corp', '20', 'USD');
            INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
            VALUES
                ('t1', 'inv-a', 'buy', 'AAA', 'A Corp', '10', '100', 'USD', {d1}),
                ('t2', 'inv-a', 'buy', 'AAA', 'A Corp', '10', '100', 'USD', {d2});
            INSERT INTO stock_data (ticker, original_price, currency) VALUES ('AAA', '110', 'USD');
            INSERT INTO stock_tags (id, name) VALUES ('tag-1', 'Tech');
            INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('inv-a', 'tag-1');
            "#,
            d1 = D1,
            d2 = D2,
        ))
        .expect("schema + seed");
        conn
    }

    #[test]
    fn stocks_analysis_cost_basis_uses_transaction_day_rates() {
        let conn = setup_db();

        let result = stocks_analysis(&conn).expect("analysis");

        let stock = &result["stocks"][0];
        // 10×100×25 + 10×100×20 — NOT 20×100×23 (avg × today's rate).
        assert_eq!(stock["costBasisCzk"], "45000.00");
        // Current value stays at today's rate: 20 × 110 × 23.
        assert_eq!(stock["currentValueCzk"], "50600.00");
        assert_eq!(stock["gainLossCzk"], "5600.00");
        assert_eq!(result["summary"]["totalGainLossCzk"], "5600.00");
    }

    #[test]
    fn tag_metrics_cost_basis_uses_transaction_day_rates() {
        let conn = setup_db();

        let result = tag_metrics(&conn, None).expect("tag metrics");

        let tag = &result[0];
        assert_eq!(tag["costBasisCzk"], "45000.00");
        assert_eq!(tag["totalValueCzk"], "50600.00");
        assert_eq!(tag["gainLossCzk"], "5600.00");
    }
}
