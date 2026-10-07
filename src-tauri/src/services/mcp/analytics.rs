//! MCP tools: analytics. Stock and tag analytics were moved from the retired
//! REST API (services/local_api.rs) (ADR 0006); the cashflow and budgeting
//! reports reuse the app's own report services so the assistant and the app
//! never disagree on money semantics.

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
    #[schemars(
        description = "Period start as Unix timestamp (seconds), the first second of a calendar month (UTC)"
    )]
    pub start_date: i64,
    #[serde(rename = "endDate")]
    #[schemars(
        description = "Period end as Unix timestamp (seconds), the last second of the period (UTC)"
    )]
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

/// The app's planning cashflow report (`services::cashflow_planning`) in the
/// user's main currency: same items, repaid-loan and replacement handling,
/// totals and FX quality as the Cashflow page.
pub fn cashflow_report(conn: &Connection, view_type: Option<String>) -> Result<Value> {
    use crate::services::reporting::parse_money;
    let view_type = view_type.unwrap_or_else(|| "monthly".to_string());
    let report = crate::services::cashflow_planning::get_report(conn, &view_type)?;
    // The report is in CZK; convert once, when the response is built.
    let ctx = MoneyContext::load(conn)?;
    let money =
        |czk: &str| -> Result<String> { Ok(format!("{:.2}", ctx.czk_to_main(parse_money(czk)?))) };

    let mut income = Vec::new();
    let mut expenses = Vec::new();
    for (section_key, section) in [
        ("personal", &report.personal),
        ("investments", &report.investments),
    ] {
        for (out, categories) in [
            (&mut income, &section.income),
            (&mut expenses, &section.expenses),
        ] {
            for category in categories {
                for item in &category.items {
                    out.push(serde_json::json!({
                        "name": item.name,
                        "category": category.key,
                        "section": section_key,
                        "amount": money(&item.amount)?,
                        "originalAmount": item.original_amount,
                        "originalCurrency": item.original_currency,
                        "originalFrequency": item.original_frequency,
                        "userDefined": item.is_user_defined,
                        "fxQuality": item.fx_quality,
                    }));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "viewType": report.view_type,
        "estimate": report.estimate,
        "ratesAsOf": report.rates_as_of,
        "summary": {
            "totalIncome": money(&report.total_income)?,
            "totalExpenses": money(&report.total_expenses)?,
            "netCashflow": money(&report.net_cashflow)?,
        },
        "quality": {
            "missingFxCurrencies": report.quality.missing_fx_currencies,
            "estimatedFxCount": report.quality.estimated_fx_count,
        },
        "income": income,
        "expenses": expenses,
    }))
}

/// The app's budgeting report (`services::budgeting`) in the user's main
/// currency: booked transactions only, signed refunds, the shared flow
/// resolver (transfers are not spending), limit versions by month and
/// historical booking-day rates with missing FX reported, not guessed.
pub fn budgeting_report(
    conn: &Connection,
    start_date: i64,
    end_date: i64,
    timeframe: Option<String>,
) -> Result<Value> {
    use crate::services::reporting::parse_money;
    let timeframe = timeframe.unwrap_or_else(|| "monthly".to_string());
    let report = crate::services::budgeting::get_report(conn, start_date, end_date, &timeframe)?;
    // The report is in CZK; convert once, when the response is built. The
    // usage ratio is currency-invariant.
    let ctx = MoneyContext::load(conn)?;
    let main = |czk: f64| format!("{:.2}", ctx.czk_to_main(czk));
    let money = |czk: &str| -> Result<String> { Ok(main(parse_money(czk)?)) };

    let mut categories = Vec::new();
    for cat in report
        .expense_categories
        .iter()
        .filter(|c| c.budget_goal.is_some())
    {
        let actual = parse_money(&cat.total_amount)?;
        let limit = cat
            .budget_limit_czk
            .as_deref()
            .map(parse_money)
            .transpose()?;
        categories.push(serde_json::json!({
            "categoryId": cat.category_id,
            "categoryName": cat.category_name,
            // null when the limit's currency has no historical rate.
            "budget": limit.map(main),
            "actual": main(actual),
            "remaining": limit.map(|l| main(l - actual)),
            "usagePercent": match limit {
                Some(l) if l > 0.0 => format!("{:.1}", actual / l * 100.0),
                _ => "N/A".to_string(),
            },
            "overBudget": limit.is_some_and(|l| actual > l),
            "transactionCount": cat.transaction_count,
        }));
    }

    let mut nonconsumption = Vec::new();
    for flow in &report.nonconsumption_flows {
        nonconsumption.push(serde_json::json!({
            "kind": flow.key,
            "amount": money(&flow.amount)?,
            "count": flow.count,
        }));
    }

    Ok(serde_json::json!({
        "mainCurrency": ctx.main_currency(),
        "timeframe": timeframe,
        "nonconsumptionFlows": nonconsumption,
        "summary": {
            "totalIncome": money(&report.total_income)?,
            "totalExpenses": money(&report.total_expenses)?,
            "netBalance": money(&report.net_balance)?,
            "uncategorizedExpenses": money(&report.uncategorized_expenses)?,
            "bankCashMovement": money(&report.bank_cash_movement)?,
        },
        "quality": {
            "missingFxCount": report.quality.missing_fx_count,
            "missingFxCurrencies": report.quality.missing_fx_currencies,
            "observedCompletedMonths": report.quality.observed_completed_months.len(),
            "partialMonthCount": report.quality.partial_month_count,
        },
        "budgetCategories": categories,
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
    /// rates), plus the rates and profile the money context reads. Money
    /// columns are TEXT exactly as migrations.rs declares them.
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
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                frequency TEXT NOT NULL,
                item_type TEXT NOT NULL,
                category TEXT NOT NULL
            );
            CREATE TABLE loans (
                name TEXT NOT NULL,
                monthly_payment TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL
            );
            CREATE TABLE insurance_policies (
                policy_name TEXT NOT NULL,
                regular_payment TEXT NOT NULL DEFAULT '0',
                regular_payment_currency TEXT NOT NULL,
                payment_frequency TEXT NOT NULL,
                status TEXT NOT NULL
            );
            CREATE TABLE bank_accounts (
                name TEXT NOT NULL,
                balance TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL,
                interest_rate TEXT,
                exclude_from_balance INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            CREATE TABLE budget_goals (
                category_id TEXT NOT NULL,
                timeframe TEXT NOT NULL,
                amount TEXT NOT NULL,
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

    // ── Reports shared with the app (final review I-5) ──────────────────

    const SEP: i64 = 1788220800; // 2026-09-01 UTC
    const OCT: i64 = 1790812800; // 2026-10-01 UTC

    /// Full app schema: the MCP reports reuse the app's report services.
    fn app_db(main_currency: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        crate::db::migrate_for_tests(&conn).expect("schema");
        conn.execute(
            "INSERT INTO user_profile (name, surname, email, currency) VALUES ('T', 'U', 't@example.test', ?1)",
            [main_currency],
        )
        .expect("profile");
        conn.execute_batch(
            "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('EUR', 25.0, 0);
             INSERT INTO bank_accounts (id, name, currency) VALUES ('acc', 'Main', 'CZK');",
        )
        .expect("seed");
        conn
    }

    fn seed_budget(conn: &Connection) {
        use crate::models::InsertBudgetGoal;
        use crate::services::budgeting::upsert_goal_at;
        let goal = |amount: &str, month: i64| InsertBudgetGoal {
            category_id: "cat_groceries".into(),
            timeframe: "monthly".into(),
            amount: amount.into(),
            currency: Some("CZK".into()),
            effective_month: Some(month),
        };
        upsert_goal_at(conn, goal("1000", SEP), SEP).expect("september limit");
        upsert_goal_at(conn, goal("2000", OCT), SEP).expect("october limit");
        conn.execute_batch(&format!(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, category_id, status) VALUES
                ('buy', 'acc', 'debit', '600', 'CZK', {d}, 'cat_groceries', 'booked'),
                ('refund', 'acc', 'credit', '100', 'CZK', {d}, 'cat_groceries', 'booked'),
                ('pending', 'acc', 'debit', '9999', 'CZK', {d}, 'cat_groceries', 'pending');",
            d = SEP + 86400
        ))
        .expect("transactions");
    }

    #[test]
    fn budgeting_report_matches_the_app_report_semantics() {
        let conn = app_db("CZK");
        seed_budget(&conn);

        let result = budgeting_report(&conn, SEP, OCT - 1, None).expect("budgeting");

        assert_eq!(result["mainCurrency"], "CZK");
        let cats = result["budgetCategories"].as_array().expect("categories");
        assert_eq!(cats.len(), 1);
        let cat = &cats[0];
        // September's limit version, not the later October one; the refund
        // nets out and the pending payment is not spending yet.
        assert_eq!(cat["categoryId"], "cat_groceries");
        assert_eq!(cat["budget"], "1000.00");
        assert_eq!(cat["actual"], "500.00");
        assert_eq!(cat["remaining"], "500.00");
        assert_eq!(cat["usagePercent"], "50.0");
        assert_eq!(cat["overBudget"], false);
        let app = crate::services::budgeting::get_report(&conn, SEP, OCT - 1, "monthly").unwrap();
        assert_eq!(result["summary"]["totalExpenses"], app.total_expenses);
        assert_eq!(result["quality"]["missingFxCount"], 0);
    }

    #[test]
    fn budgeting_report_converts_to_the_main_currency_and_flags_missing_fx() {
        let conn = app_db("EUR");
        seed_budget(&conn);
        // A EUR expense without a historical rate is excluded and reported.
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, category_id) VALUES ('eur', 'acc', 'debit', '10', 'EUR', ?1, 'cat_groceries')",
            [SEP + 86400],
        )
        .unwrap();

        let result = budgeting_report(&conn, SEP, OCT - 1, None).expect("budgeting");

        assert_eq!(result["mainCurrency"], "EUR");
        let cat = &result["budgetCategories"][0];
        // 1 000 / 500 CZK at the current 25 CZK per EUR.
        assert_eq!(cat["budget"], "40.00");
        assert_eq!(cat["actual"], "20.00");
        assert_eq!(cat["usagePercent"], "50.0");
        assert_eq!(result["quality"]["missingFxCount"], 1);
        assert_eq!(result["quality"]["missingFxCurrencies"][0], "EUR");
    }

    fn seed_cashflow(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO cashflow_items (id, name, amount, currency, frequency, item_type, category) VALUES
                ('salary', 'Salary', '50000', 'CZK', 'monthly', 'income', 'personalIncome');
             INSERT INTO loans (id, name, principal, currency, interest_rate, monthly_payment, start_date) VALUES
                ('active', 'Mortgage', '1000000', 'CZK', '0', '10000', 1704067200),
                ('paid', 'Old loan', '1000', 'CZK', '0', '100', 1704067200);",
        )
        .expect("seed cashflow");
    }

    #[test]
    fn cashflow_report_matches_the_app_planning_report() {
        let conn = app_db("CZK");
        seed_cashflow(&conn);
        // A manual item replacing the mortgage must not be counted twice.
        crate::services::cashflow_planning::create_cashflow_item(
            &conn,
            crate::models::InsertCashflowItem {
                name: "Adjusted mortgage".into(),
                amount: "9000".into(),
                currency: Some("CZK".into()),
                frequency: "monthly".into(),
                item_type: "expense".into(),
                category: "personalLoans".into(),
                replacement_source_id: Some("loan:active".into()),
            },
        )
        .expect("replacement");

        let result = cashflow_report(&conn, None).expect("cashflow");
        let app = crate::services::cashflow_planning::get_report(&conn, "monthly").unwrap();

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["viewType"], "monthly");
        assert_eq!(result["summary"]["totalIncome"], app.total_income);
        assert_eq!(result["summary"]["totalExpenses"], app.total_expenses);
        assert_eq!(result["summary"]["netCashflow"], app.net_cashflow);
        assert_eq!(result["summary"]["totalIncome"], "50000.00");
        assert_eq!(result["summary"]["totalExpenses"], "9000.00");
        let names: Vec<&str> = result["expenses"]
            .as_array()
            .expect("expenses")
            .iter()
            .map(|l| l["name"].as_str().expect("name"))
            .collect();
        assert_eq!(
            names,
            vec!["Adjusted mortgage"],
            "no repaid loan, no double count"
        );
    }

    #[test]
    fn cashflow_report_reports_amounts_in_the_main_currency() {
        let conn = app_db("EUR");
        seed_cashflow(&conn);

        let monthly = cashflow_report(&conn, None).expect("cashflow");
        let yearly = cashflow_report(&conn, Some("yearly".to_string())).expect("cashflow");

        assert_eq!(monthly["mainCurrency"], "EUR");
        // 50 000 CZK salary, 10 000 CZK mortgage at 25 CZK per EUR.
        assert_eq!(monthly["summary"]["totalIncome"], "2000.00");
        assert_eq!(monthly["summary"]["totalExpenses"], "400.00");
        assert_eq!(monthly["summary"]["netCashflow"], "1600.00");
        assert_eq!(yearly["viewType"], "yearly");
        assert_eq!(yearly["summary"]["totalIncome"], "24000.00");
    }
}
