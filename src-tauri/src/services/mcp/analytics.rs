//! MCP tools: analytics. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use super::money::MoneyContext;
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

/// One recurring cashflow line, normalised to a monthly amount in CZK.
struct CashflowLine {
    name: String,
    category: String,
    monthly_czk: f64,
}

pub fn cashflow_report(conn: &Connection, view_type: Option<String>) -> Result<Value> {
    let view_type = view_type.unwrap_or_else(|| "monthly".to_string());
    let multiplier: f64 = if view_type == "yearly" { 12.0 } else { 1.0 };

    // One MoneyContext per call: all arithmetic stays in CZK, converted to the
    // main currency only when the response is built.
    let ctx = MoneyContext::load(conn)?;
    let to_monthly = |amount: f64, currency: &str, frequency: &str| -> f64 {
        let v = ctx.to_czk(amount, currency);
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

    let mut income: Vec<CashflowLine> = Vec::new();
    let mut expenses: Vec<CashflowLine> = Vec::new();

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
            let line = CashflowLine {
                name,
                category,
                monthly_czk: monthly,
            };
            if item_type == "income" {
                income.push(line);
            } else {
                expenses.push(line);
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
            let monthly = ctx.to_czk(payment, &currency);
            if monthly > 0.0 {
                expenses.push(CashflowLine {
                    name: format!("Loan: {}", name),
                    category: "loan_payment".to_string(),
                    monthly_czk: monthly,
                });
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
                expenses.push(CashflowLine {
                    name: format!("Insurance: {}", name),
                    category: "insurance".to_string(),
                    monthly_czk: monthly,
                });
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
            let balance_czk = ctx.to_czk(balance, &currency);
            let monthly = balance_czk * (rate / 100.0) / 12.0;
            if monthly > 0.0 {
                income.push(CashflowLine {
                    name: format!("Interest: {}", name),
                    category: "interest".to_string(),
                    monthly_czk: monthly,
                });
            }
        }
    }

    let total_income: f64 = income.iter().map(|l| l.monthly_czk).sum();
    let total_expenses: f64 = expenses.iter().map(|l| l.monthly_czk).sum();

    // Amounts are converted to the user's main currency here, once, at
    // serialisation time.
    let money = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));
    let fmt_lines = |lines: &[CashflowLine]| -> Vec<Value> {
        lines
            .iter()
            .map(|l| {
                serde_json::json!({
                    "name": l.name, "category": l.category,
                    "amount": money(l.monthly_czk * multiplier)
                })
            })
            .collect()
    };

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "viewType": view_type,
        "summary": {
            "totalIncome": money(total_income * multiplier),
            "totalExpenses": money(total_expenses * multiplier),
            "netCashflow": money((total_income - total_expenses) * multiplier),
        },
        "income": fmt_lines(&income),
        "expenses": fmt_lines(&expenses),
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

    // One MoneyContext per call: all arithmetic stays in CZK, converted to the
    // main currency only when the response is built.
    let ctx = MoneyContext::load(conn)?;

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
        *spending_map.entry(cat_id).or_insert(0.0) += ctx.to_czk(amount, &currency);
    }

    // Amounts are converted to the user's main currency here, once, at
    // serialisation time; the usage ratio is currency-invariant.
    let money = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));
    let report: Vec<Value> = goals
        .iter()
        .map(|(cat_id, cat_name, goal_amount, goal_currency)| {
            let budget_czk = ctx.to_czk(*goal_amount, goal_currency);
            let actual_czk = spending_map.get(cat_id).copied().unwrap_or(0.0);
            serde_json::json!({
                "categoryId": cat_id,
                "categoryName": cat_name,
                "budget": money(budget_czk),
                "actual": money(actual_czk),
                "remaining": money(budget_czk - actual_czk),
                "usagePercent": if budget_czk > 0.0 { format!("{:.1}", (actual_czk / budget_czk) * 100.0) } else { "N/A".to_string() },
                "overBudget": actual_czk > budget_czk,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "timeframe": timeframe,
        "budgetCategories": report,
    }))
}

/// One stock holding as read from the DB (prices and dividends in their own currencies).
struct StockRow {
    id: String,
    ticker: String,
    company: String,
    qty: f64,
    curr_price: f64,
    price_currency: String,
    yearly_div: f64,
    div_currency: String,
}

pub fn stocks_analysis(conn: &Connection) -> Result<Value> {
    // One MoneyContext per call: all arithmetic stays in CZK, converted to the
    // main currency only when the response is built.
    let ctx = MoneyContext::load(conn)?;

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

    let rows: Vec<StockRow> = stmt
        .query_map([], |row| {
            Ok(StockRow {
                id: row.get(0)?,
                ticker: row.get(1)?,
                company: row.get(2)?,
                qty: row.get::<_, String>(3)?.parse().unwrap_or(0.0),
                curr_price: row.get::<_, String>(4)?.parse().unwrap_or(0.0),
                price_currency: row.get(5)?,
                yearly_div: row.get::<_, String>(6)?.parse().unwrap_or(0.0),
                div_currency: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Amounts are converted to the user's main currency here, once, at
    // serialisation time; ratios are currency-invariant.
    let money = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));

    let mut stocks: Vec<Value> = Vec::new();
    let mut total_val_czk = 0.0_f64;
    let mut total_gain_czk = 0.0_f64;
    let mut total_div_czk = 0.0_f64;
    for StockRow {
        id,
        ticker,
        company,
        qty,
        curr_price,
        price_currency,
        yearly_div,
        div_currency,
    } in rows
    {
        let curr_val_czk = ctx.to_czk(qty * curr_price, &price_currency);
        // Historical cost: every transaction at its own day's rate
        // (ADR 0001) — never the stored scalar at today's rate.
        let cost_czk = cost_map.get(&id).map(|p| p.cost_basis_czk).unwrap_or(0.0);
        let gain_czk = curr_val_czk - cost_czk;
        let gain_pct = if cost_czk > 0.0 {
            (gain_czk / cost_czk) * 100.0
        } else {
            0.0
        };
        let div_czk = ctx.to_czk(qty * yearly_div, &div_currency);
        let div_yield = if curr_val_czk > 0.0 {
            (div_czk / curr_val_czk) * 100.0
        } else {
            0.0
        };
        total_val_czk += curr_val_czk;
        total_gain_czk += gain_czk;
        total_div_czk += div_czk;
        stocks.push(serde_json::json!({
            "id": id,
            "ticker": ticker,
            "companyName": company,
            "quantity": qty,
            "currentValue": money(curr_val_czk),
            "costBasis": money(cost_czk),
            "gainLoss": money(gain_czk),
            "gainLossPct": format!("{:.2}", gain_pct),
            "annualDividend": money(div_czk),
            "dividendYieldPct": format!("{:.2}", div_yield),
        }));
    }

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "summary": {
            "totalValue": money(total_val_czk),
            "totalGainLoss": money(total_gain_czk),
            "annualDividends": money(total_div_czk),
        },
        "stocks": stocks,
    }))
}

pub fn tag_metrics(conn: &Connection, tag_ids: Option<Vec<String>>) -> Result<Value> {
    let cost_map = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?;
    // One MoneyContext per call: all arithmetic stays in CZK, converted to the
    // main currency only when the response is built.
    let ctx = MoneyContext::load(conn)?;

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

    // Amounts are converted to the user's main currency here, once, at
    // serialisation time; ratios are currency-invariant.
    let money = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));

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
                val += ctx.to_czk(qty * cp, cc);
                cost += cost_map.get(id).map(|p| p.cost_basis_czk).unwrap_or(0.0);
                div += ctx.to_czk(qty * yd, dc);
            }
            let gain = val - cost;
            let gain_pct = if cost > 0.0 { (gain / cost) * 100.0 } else { 0.0 };
            let div_yield = if val > 0.0 { (div / val) * 100.0 } else { 0.0 };

            Some(serde_json::json!({
                "tagId": tag_id,
                "tagName": tag_name,
                "stockCount": stocks.len(),
                "totalValue": money(val),
                "costBasis": money(cost),
                "gainLoss": money(gain),
                "gainLossPct": format!("{:.2}", gain_pct),
                "annualDividend": money(div),
                "dividendYieldPct": format!("{:.2}", div_yield),
            }))
        })
        .collect();

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "tags": result,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;
    const D1: i64 = 1_700_006_400;
    const D2: i64 = D1 + DAY;

    /// Minimal schema for the analytics paths (post-migration-008: no stored
    /// `average_price`; cost basis must come from transactions at their day's
    /// rates), plus the rates and profile the money context reads.
    ///
    /// The cashflow/budgeting amounts are REAL here because those queries read
    /// them with `get::<f64>`.
    fn setup_db(main_currency: &str) -> Connection {
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
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
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
            CREATE TABLE cashflow_items (
                name TEXT NOT NULL,
                amount REAL NOT NULL,
                currency TEXT NOT NULL,
                frequency TEXT NOT NULL,
                item_type TEXT NOT NULL,
                category TEXT NOT NULL
            );
            CREATE TABLE loans (
                name TEXT NOT NULL,
                monthly_payment REAL NOT NULL,
                currency TEXT NOT NULL
            );
            CREATE TABLE insurance_policies (
                policy_name TEXT NOT NULL,
                regular_payment REAL NOT NULL,
                regular_payment_currency TEXT NOT NULL,
                payment_frequency TEXT NOT NULL,
                status TEXT NOT NULL
            );
            CREATE TABLE bank_accounts (
                name TEXT NOT NULL,
                balance REAL NOT NULL,
                currency TEXT NOT NULL,
                interest_rate REAL,
                exclude_from_balance INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            CREATE TABLE budget_goals (
                category_id TEXT NOT NULL,
                timeframe TEXT NOT NULL,
                amount REAL NOT NULL,
                currency TEXT NOT NULL
            );
            CREATE TABLE bank_transactions (
                category_id TEXT,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                booking_date INTEGER NOT NULL,
                tx_type TEXT NOT NULL
            );

            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES
                ('USD', 23.0, 0), ('EUR', 25.0, 0);
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
            INSERT INTO dividend_data (ticker, yearly_dividend_sum, currency) VALUES ('AAA', '2', 'USD');
            INSERT INTO stock_tags (id, name) VALUES ('tag-1', 'Tech');
            INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('inv-a', 'tag-1');
            "#,
            d1 = D1,
            d2 = D2,
        ))
        .expect("schema + seed");
        conn.execute(
            "INSERT INTO user_profile (currency) VALUES (?1)",
            [main_currency],
        )
        .expect("profile");
        conn
    }

    #[test]
    fn stocks_analysis_cost_basis_uses_transaction_day_rates() {
        let conn = setup_db("CZK");

        let result = stocks_analysis(&conn).expect("analysis");

        assert_eq!(result["mainCurrency"], "CZK");
        let stock = &result["stocks"][0];
        // 10×100×25 + 10×100×20 — NOT 20×100×23 (avg × today's rate).
        assert_eq!(stock["costBasis"], "45000.00");
        // Current value stays at today's rate: 20 × 110 × 23.
        assert_eq!(stock["currentValue"], "50600.00");
        assert_eq!(stock["gainLoss"], "5600.00");
        assert_eq!(result["summary"]["totalGainLoss"], "5600.00");
    }

    #[test]
    fn stocks_analysis_reports_amounts_in_the_main_currency() {
        let conn = setup_db("EUR");

        let result = stocks_analysis(&conn).expect("analysis");

        assert_eq!(result["mainCurrency"], "EUR");
        let stock = &result["stocks"][0];
        // CZK 45 000 / 50 600 / 5 600 / 920 at 25 CZK per EUR.
        assert_eq!(stock["costBasis"], "1800.00");
        assert_eq!(stock["currentValue"], "2024.00");
        assert_eq!(stock["gainLoss"], "224.00");
        assert_eq!(stock["annualDividend"], "36.80");
        // Ratios do not depend on the currency.
        assert_eq!(stock["gainLossPct"], "12.44");
        assert_eq!(stock["dividendYieldPct"], "1.82");
        assert_eq!(result["summary"]["totalValue"], "2024.00");
        assert_eq!(result["summary"]["totalGainLoss"], "224.00");
        assert_eq!(result["summary"]["annualDividends"], "36.80");
        assert!(stock.get("currentValueCzk").is_none());
    }

    #[test]
    fn tag_metrics_cost_basis_uses_transaction_day_rates() {
        let conn = setup_db("CZK");

        let result = tag_metrics(&conn, None).expect("tag metrics");

        assert_eq!(result["mainCurrency"], "CZK");
        let tag = &result["tags"][0];
        assert_eq!(tag["costBasis"], "45000.00");
        assert_eq!(tag["totalValue"], "50600.00");
        assert_eq!(tag["gainLoss"], "5600.00");
    }

    #[test]
    fn tag_metrics_reports_amounts_in_the_main_currency() {
        let conn = setup_db("EUR");

        let result = tag_metrics(&conn, None).expect("tag metrics");

        assert_eq!(result["mainCurrency"], "EUR");
        let tag = &result["tags"][0];
        assert_eq!(tag["costBasis"], "1800.00");
        assert_eq!(tag["totalValue"], "2024.00");
        assert_eq!(tag["gainLoss"], "224.00");
        assert_eq!(tag["annualDividend"], "36.80");
        assert_eq!(tag["gainLossPct"], "12.44");
        assert_eq!(tag["stockCount"], 1);
    }

    fn seed_cashflow(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO cashflow_items (name, amount, currency, frequency, item_type, category) VALUES
                ('Salary', 50000, 'CZK', 'monthly', 'income', 'salary'),
                ('Rent', 400, 'EUR', 'monthly', 'expense', 'housing'),
                ('Bonus', 9999, 'CZK', 'one_time', 'income', 'salary');
            INSERT INTO loans (name, monthly_payment, currency) VALUES ('Mortgage', 10000, 'CZK');
            INSERT INTO bank_accounts (name, balance, currency, interest_rate)
                VALUES ('Savings', 100000, 'CZK', 3);
            "#,
        )
        .expect("seed cashflow");
    }

    #[test]
    fn cashflow_report_reports_amounts_in_the_main_currency() {
        let conn = setup_db("EUR");
        seed_cashflow(&conn);

        let result = cashflow_report(&conn, None).expect("cashflow");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["viewType"], "monthly");
        // Income 50 000 + interest 250 = 50 250 CZK; expenses 10 000 (rent) + 10 000 (loan).
        assert_eq!(result["summary"]["totalIncome"], "2010.00");
        assert_eq!(result["summary"]["totalExpenses"], "800.00");
        assert_eq!(result["summary"]["netCashflow"], "1210.00");
        let income = result["income"].as_array().expect("income");
        assert_eq!(income[0]["name"], "Salary");
        assert_eq!(income[0]["amount"], "2000.00");
        assert_eq!(income[1]["name"], "Interest: Savings");
        assert_eq!(income[1]["amount"], "10.00");
        let expenses = result["expenses"].as_array().expect("expenses");
        assert_eq!(expenses[0]["name"], "Rent");
        assert_eq!(expenses[0]["amount"], "400.00");
        assert_eq!(expenses[1]["name"], "Loan: Mortgage");
        assert_eq!(expenses[1]["amount"], "400.00");
        assert!(result["summary"].get("totalIncomeCzk").is_none());
    }

    #[test]
    fn cashflow_report_yearly_view_scales_the_main_currency_amounts() {
        let conn = setup_db("EUR");
        seed_cashflow(&conn);

        let result = cashflow_report(&conn, Some("yearly".to_string())).expect("cashflow");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["viewType"], "yearly");
        assert_eq!(result["summary"]["totalIncome"], "24120.00");
        assert_eq!(result["income"][0]["amount"], "24000.00");
    }

    #[test]
    fn cashflow_report_stays_in_czk_for_a_czk_main_currency() {
        let conn = setup_db("CZK");
        seed_cashflow(&conn);

        let result = cashflow_report(&conn, None).expect("cashflow");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["summary"]["totalIncome"], "50250.00");
        assert_eq!(result["expenses"][0]["amount"], "10000.00");
    }

    #[test]
    fn budgeting_report_reports_amounts_in_the_main_currency() {
        let conn = setup_db("EUR");
        conn.execute_batch(
            r#"
            INSERT INTO transaction_categories (id, name) VALUES ('cat-food', 'Groceries');
            INSERT INTO budget_goals (category_id, timeframe, amount, currency)
                VALUES ('cat-food', 'monthly', 400, 'EUR');
            INSERT INTO bank_transactions (category_id, amount, currency, booking_date, tx_type) VALUES
                ('cat-food', '5000', 'CZK', 100, 'debit'),
                ('cat-food', '100', 'EUR', 200, 'debit'),
                ('cat-food', '9999', 'CZK', 100000, 'debit');
            "#,
        )
        .expect("seed budgeting");

        let result = budgeting_report(&conn, Some(0), Some(1000), None).expect("budgeting");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["timeframe"], "monthly");
        let cat = &result["budgetCategories"][0];
        assert_eq!(cat["categoryName"], "Groceries");
        // Budget 400 EUR = 10 000 CZK; spent 5 000 CZK + 100 EUR = 7 500 CZK.
        assert_eq!(cat["budget"], "400.00");
        assert_eq!(cat["actual"], "300.00");
        assert_eq!(cat["remaining"], "100.00");
        assert_eq!(cat["usagePercent"], "75.0");
        assert_eq!(cat["overBudget"], false);
        assert!(cat.get("budgetCzk").is_none());
    }
}
