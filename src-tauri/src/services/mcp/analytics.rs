//! MCP tools: analytics. Stock and tag analytics were moved from the retired
//! REST API (services/local_api.rs) (ADR 0006); the cashflow and budgeting
//! reports reuse the app's own report services so the assistant and the app
//! never disagree on money semantics.

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use std::collections::HashMap;

use crate::error::Result;
use crate::services::positions::{self, AssetKind};

use super::money::{amount_json, main_currency_positions, percent_json};

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
    let view_type = view_type.unwrap_or_else(|| "monthly".to_string());
    let target = super::money::load_main_currency(conn)?;
    let report =
        crate::services::cashflow_planning::get_report_in_currency(conn, &view_type, &target)?;
    let money = |value: &str| -> Result<String> { Ok(value.to_string()) };

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
                        "originalComponents": item.original_components,
                        "convertedComponentCount": item.converted_component_count,
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
        "mainCurrency": report.report_currency,
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
    let target = super::money::load_main_currency(conn)?;
    let report = crate::services::budgeting::get_report_in_currency(
        conn, start_date, end_date, &timeframe, None, &target,
    )?;
    let main = |amount: f64| {
        crate::services::reporting::money_in_currency(amount, &report.report_currency)
    };
    let money = |amount: &str| -> Result<String> { Ok(amount.to_string()) };

    let mut categories = Vec::new();
    for cat in report
        .expense_categories
        .iter()
        .filter(|c| c.budget_goal.is_some())
    {
        let actual = parse_money(&cat.total_amount)?;
        let limit = cat.budget_limit.as_deref().map(parse_money).transpose()?;
        categories.push(serde_json::json!({
            "categoryId": cat.category_id,
            "categoryName": cat.category_name,
            // null when the limit's currency has no historical rate.
            "budget": limit.map(main),
            "actual": main(actual),
            "remaining": limit.map(|l| main(l - actual)),
            "budgetCurrency": cat.budget_currency,
            "budgetSpent": cat.budget_spent,
            "budgetLimitNative": cat.budget_limit_native,
            "usagePercent": cat.budget_percentage.map(|p| format!("{p:.1}")).unwrap_or_else(|| "N/A".into()),
            "overBudget": cat.budget_percentage.is_some_and(|p| p > 100.0),
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
        "mainCurrency": report.report_currency,
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

/// Dividend in percent of the value; `None` unless both are known.
fn dividend_yield(dividend: Option<f64>, value: Option<f64>) -> Option<f64> {
    let (dividend, value) = dividend.zip(value)?;
    Some(if value > 0.0 {
        dividend / value * 100.0
    } else {
        0.0
    })
}

pub fn stocks_analysis(conn: &Connection) -> Result<Value> {
    let positions = main_currency_positions(conn, AssetKind::Stock)?;
    let currency = positions.currency.as_str();
    let names: HashMap<String, String> = {
        let mut stmt = conn.prepare("SELECT id, company_name FROM stock_investments")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };

    // A row shows what is known; an amount that cannot be converted is null. The summary
    // (`positions::totals`) sums the known values and costs, the unrealized gain only over
    // positions where both are known, and the dividend over positions with a known value.
    let stocks: Vec<Value> = positions
        .items
        .iter()
        .map(|item| {
            serde_json::json!({
                "id": item.investment_id,
                "ticker": item.ticker,
                "companyName": names.get(&item.investment_id),
                "quantity": item.quantity,
                "currentValue": amount_json(item.value, currency),
                "costBasis": amount_json(item.cost_basis, currency),
                "gainLoss": amount_json(item.unrealized_gain, currency),
                "gainLossPct": percent_json(item.unrealized_gain_percent),
                "realizedGain": amount_json(item.realized_gain, currency),
                "annualDividend": amount_json(item.yearly_dividend, currency),
                "dividendYieldPct": percent_json(dividend_yield(item.yearly_dividend, item.value)),
            })
        })
        .collect();

    let totals = positions::totals(&positions.items);
    Ok(serde_json::json!({
        "mainCurrency": currency,
        "summary": {
            "totalValue": amount_json(totals.value, currency),
            "totalCostBasis": amount_json(totals.cost_basis, currency),
            "totalGainLoss": amount_json(totals.unrealized_gain, currency),
            "realizedGain": amount_json(totals.realized_gain, currency),
            "annualDividends": amount_json(totals.yearly_dividend, currency),
        },
        "stocks": stocks,
        "missingCurrencies": positions.quality.missing_currencies(),
        "fxEstimated": positions.quality.is_estimated(),
    }))
}

pub fn tag_metrics(conn: &Connection, tag_ids: Option<Vec<String>>) -> Result<Value> {
    let positions = main_currency_positions(conn, AssetKind::Stock)?;
    let currency = positions.currency.as_str();

    let tags: Vec<(String, String)> = if let Some(ref ids) = tag_ids {
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, name FROM stock_tags WHERE id IN ({}) ORDER BY name",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let result = stmt
            .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        result
    } else {
        let mut stmt = conn.prepare("SELECT id, name FROM stock_tags ORDER BY name")?;
        let result = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        result
    };
    let tag_map = positions::investment_tag_ids(conn)?;
    let by_tag = positions::tag_totals(&positions, &tag_map);

    let result: Vec<Value> = tags
        .iter()
        .map(|(tag_id, tag_name)| {
            let totals = by_tag.get(tag_id).cloned().unwrap_or_default();
            serde_json::json!({
                "tagId": tag_id,
                "tagName": tag_name,
                "stockCount": totals.count,
                "totalValue": amount_json(totals.value, currency),
                "costBasis": amount_json(totals.cost_basis, currency),
                "gainLoss": amount_json(totals.unrealized_gain, currency),
                "gainLossPct": format!("{:.2}", totals.unrealized_gain_percent()),
                "annualDividend": amount_json(totals.yearly_dividend, currency),
                "dividendYieldPct": format!(
                    "{:.2}",
                    dividend_yield(totals.yearly_dividend, totals.value)
                        .unwrap_or(0.0)
                ),
                "missingCurrencies": totals.quality.missing_currencies(),
                "fxEstimated": totals.quality.is_estimated(),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "mainCurrency": currency,
        "tags": result,
        // Every position counted once, whichever tags it carries
        "missingCurrencies": positions.quality.missing_currencies(),
        "fxEstimated": positions.quality.is_estimated(),
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
                currency TEXT,
                fetched_at INTEGER NOT NULL DEFAULT 100
            );
            CREATE TABLE stock_price_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT 0
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
                ('CZK', 25.0, 0), ('USD', 1.0869565217391304, 0);
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                ({d1}, 'CZK', 25.0), ({d1}, 'USD', 1.0), ({d2}, 'CZK', 25.0), ({d2}, 'USD', 1.25);
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
        // The current snapshot (C-A), mirroring `exchange_rates` above.
        crate::services::fx::set_current_for_test(&[("CZK", 25.0), ("USD", 25.0 / 23.0)]);
        conn
    }

    /// C1 (ADR 0011): bought 10 x 100 USD when USDCZK = EURCZK = 25; today USDCZK 25 and
    /// EURCZK 22. Only the exchange_rate_history and the current snapshot carry rates.
    fn c1_db() -> Connection {
        let conn = setup_db("EUR");
        conn.execute_batch(&format!(
            r#"
            DELETE FROM exchange_rates;
            DELETE FROM exchange_rate_history;
            DELETE FROM investment_transactions;
            DELETE FROM stock_investments;
            DELETE FROM stock_data;
            DELETE FROM dividend_data;
            DELETE FROM stock_investment_tags;
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                ({d1}, 'CZK', 25.0), ({d1}, 'USD', 1.0);
            INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                VALUES ('inv-aapl', 'AAPL', 'Apple', '10', 'USD');
            INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
            VALUES ('t1', 'inv-aapl', 'buy', 'AAPL', 'Apple', '10', '100', 'USD', {d1});
            INSERT INTO stock_data (ticker, original_price, currency) VALUES ('AAPL', '100', 'USD');
            INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('inv-aapl', 'tag-1');
            "#,
            d1 = D1,
        ))
        .expect("c1 seed");
        crate::services::fx::set_current_for_test(&[("CZK", 22.0), ("USD", 22.0 / 25.0)]);
        conn
    }

    #[test]
    fn stocks_analysis_matches_the_positions_service_for_an_eur_investor() {
        let conn = c1_db();

        let result = stocks_analysis(&conn).expect("analysis");

        assert_eq!(result["mainCurrency"], "EUR");
        let aapl = &result["stocks"][0];
        assert_eq!(aapl["ticker"], "AAPL");
        assert_eq!(aapl["costBasis"], "1000.00");
        assert_eq!(aapl["currentValue"], "1136.36");
        assert_eq!(aapl["gainLoss"], "136.36");
        assert_eq!(result["summary"]["totalGainLoss"], "136.36");
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
        assert_eq!(result["fxEstimated"], false);
    }

    #[test]
    fn tag_metrics_match_the_positions_service_for_an_eur_investor() {
        let conn = c1_db();

        let result = tag_metrics(&conn, None).expect("tag metrics");

        assert_eq!(result["mainCurrency"], "EUR");
        let tag = &result["tags"][0];
        assert_eq!(tag["costBasis"], "1000.00");
        assert_eq!(tag["totalValue"], "1136.36");
        assert_eq!(tag["gainLoss"], "136.36");
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
    fn stocks_analysis_nulls_a_row_with_an_unquoted_price_currency() {
        let conn = setup_db("CZK");
        conn.execute("UPDATE stock_data SET currency = 'KWD'", [])
            .expect("kwd price");

        let result = stocks_analysis(&conn).expect("analysis");

        let stock = &result["stocks"][0];
        for field in [
            "currentValue",
            "gainLoss",
            "gainLossPct",
            "dividendYieldPct",
        ] {
            assert!(stock[field].is_null(), "{field} must be null");
        }
        // The cost basis does not depend on the price currency.
        assert_eq!(stock["costBasis"], "45000.00");
        // The row is left out of the totals (its dividend too) and its currency is declared;
        // with nothing else held the totals are unknown, not 0.
        assert!(result["summary"]["annualDividends"].is_null());
        assert!(result["summary"]["totalValue"].is_null());
        assert!(result["summary"]["totalGainLoss"].is_null());
        assert_eq!(result["summary"]["totalCostBasis"], "45000.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }

    #[test]
    fn stocks_analysis_nulls_a_row_with_a_partial_cost_basis() {
        let conn = setup_db("CZK");
        conn.execute(
            "INSERT INTO investment_transactions
                (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
             VALUES ('t3', 'inv-a', 'buy', 'AAA', 'A Corp', '1', '10', 'KWD', ?1)",
            [D2],
        )
        .expect("kwd buy");

        let result = stocks_analysis(&conn).expect("analysis");

        let stock = &result["stocks"][0];
        assert!(stock["costBasis"].is_null());
        assert!(stock["gainLoss"].is_null());
        // The value is known: 21 × 110 USD × 23.
        assert_eq!(stock["currentValue"], "53130.00");
        // Every known amount counts: the value and its dividend (21 × 2 USD × 23), no gain.
        assert_eq!(result["summary"]["totalValue"], "53130.00");
        assert!(result["summary"]["totalCostBasis"].is_null());
        assert!(result["summary"]["totalGainLoss"].is_null());
        assert_eq!(result["summary"]["annualDividends"], "966.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }

    #[test]
    fn tag_metrics_leaves_an_unconvertible_stock_out_of_the_tag_totals() {
        let conn = setup_db("CZK");
        conn.execute("UPDATE stock_data SET currency = 'KWD'", [])
            .expect("kwd price");

        let result = tag_metrics(&conn, None).expect("tag metrics");

        let tag = &result["tags"][0];
        assert_eq!(tag["stockCount"], 1);
        // The unknown value and gain stay out (null: nothing else to sum); the known cost counts.
        assert!(tag["totalValue"].is_null());
        assert_eq!(tag["costBasis"], "45000.00");
        assert!(tag["gainLoss"].is_null());
        assert_eq!(tag["missingCurrencies"], serde_json::json!(["KWD"]));
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
        assert_eq!(tag["fxEstimated"], false);
        assert_eq!(result["fxEstimated"], false);
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
        conn.execute_batch("CREATE TABLE user_profile(name TEXT,surname TEXT,email TEXT,currency TEXT);
CREATE TABLE exchange_rates(currency TEXT,rate REAL,fetched_at INTEGER);
CREATE TABLE institutions(id TEXT,country TEXT);
CREATE TABLE bank_accounts(id TEXT,name TEXT,currency TEXT,iban TEXT,bban TEXT,institution_id TEXT,balance TEXT DEFAULT '0',interest_rate TEXT DEFAULT '0',has_zone_designation INTEGER DEFAULT 0,account_type TEXT DEFAULT 'checking',exclude_from_balance INTEGER DEFAULT 0);
CREATE TABLE transaction_categories(id TEXT,name TEXT,kind TEXT,icon TEXT,color TEXT);
INSERT INTO transaction_categories VALUES ('cat_groceries','Groceries','expense',NULL,NULL);
CREATE TABLE bank_transactions(id TEXT,bank_account_id TEXT,tx_type TEXT,amount TEXT,currency TEXT,booking_date INTEGER,category_id TEXT,status TEXT DEFAULT 'booked',description TEXT,counterparty_name TEXT,counterparty_iban TEXT,counterparty_bank_code TEXT,counterparty_country TEXT,flow_kind TEXT,remittance_info TEXT);
CREATE TABLE exchange_rate_history(date INTEGER,currency TEXT,rate REAL,PRIMARY KEY(date,currency));
CREATE TABLE budget_goals(id TEXT PRIMARY KEY,category_id TEXT,timeframe TEXT,amount TEXT,currency TEXT,created_at INTEGER,updated_at INTEGER,UNIQUE(category_id,timeframe));
CREATE TABLE budget_goal_versions(id TEXT PRIMARY KEY,category_id TEXT,timeframe TEXT,effective_month INTEGER,amount TEXT,currency TEXT,created_at INTEGER,UNIQUE(category_id,timeframe,effective_month));
CREATE TABLE cashflow_items(id TEXT PRIMARY KEY,name TEXT,amount TEXT,currency TEXT,frequency TEXT,item_type TEXT,category TEXT,created_at INTEGER DEFAULT 0,updated_at INTEGER DEFAULT 0,replacement_source_id TEXT UNIQUE);
CREATE TABLE loans(id TEXT,name TEXT,principal TEXT,currency TEXT,interest_rate TEXT,monthly_payment TEXT,start_date INTEGER,end_date INTEGER,balance_anchor_amount TEXT,balance_anchor_date INTEGER);
CREATE TABLE real_estate(id TEXT,name TEXT,type TEXT,recurring_costs TEXT,monthly_rent TEXT,monthly_rent_currency TEXT);
CREATE TABLE real_estate_loans(real_estate_id TEXT,loan_id TEXT);
CREATE TABLE real_estate_insurances(real_estate_id TEXT,insurance_id TEXT);
CREATE TABLE insurance_policies(id TEXT,policy_name TEXT,regular_payment TEXT,regular_payment_currency TEXT,payment_frequency TEXT,status TEXT);
CREATE TABLE stock_investments(ticker TEXT,company_name TEXT,quantity TEXT);
CREATE TABLE bonds(id TEXT,name TEXT,coupon_value TEXT,quantity TEXT,interest_rate TEXT,currency TEXT);").expect("minimal report schema");

        conn.execute(
            "INSERT INTO user_profile (name, surname, email, currency) VALUES ('T', 'U', 't@example.test', ?1)",
            [main_currency],
        )
        .expect("profile");
        conn.execute_batch(
            "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('CZK', 25.0, 0);
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
    fn target_mcp_native_reports_match_services_without_any_fx() {
        for target in ["EUR", "AED"] {
            let conn = app_db(target);
            conn.execute("DELETE FROM exchange_rates", []).unwrap();
            crate::services::budgeting::upsert_goal_at(
                &conn,
                crate::models::InsertBudgetGoal {
                    category_id: "cat_groceries".into(),
                    timeframe: "monthly".into(),
                    amount: "100.00".into(),
                    currency: Some(target.into()),
                    effective_month: Some(SEP),
                },
                SEP,
            )
            .unwrap();
            conn.execute("INSERT INTO bank_transactions(id,bank_account_id,tx_type,amount,currency,booking_date,category_id) VALUES ('native','acc','debit','25',?1,?2,'cat_groceries')",rusqlite::params![target,SEP]).unwrap();
            let mcp = budgeting_report(&conn, SEP, OCT - 1, None).unwrap();
            let app = crate::services::budgeting::get_report_in_currency(
                &conn,
                SEP,
                OCT - 1,
                "monthly",
                None,
                target,
            )
            .unwrap();
            assert_eq!(mcp["summary"]["totalExpenses"], app.total_expenses);
            assert_eq!(mcp["budgetCategories"][0]["usagePercent"], "25.0");
            assert_eq!(mcp["quality"]["missingFxCount"], 0);
            conn.execute("INSERT INTO cashflow_items(id,name,amount,currency,frequency,item_type,category) VALUES ('native','Annual','1200',?1,'yearly','expense','subscriptions')",[target]).unwrap();
            let mcp = cashflow_report(&conn, None).unwrap();
            let app = crate::services::cashflow_planning::get_report_in_currency(
                &conn, "monthly", target,
            )
            .unwrap();
            assert_eq!(mcp["summary"]["totalExpenses"], app.total_expenses);
            assert_eq!(mcp["summary"]["totalExpenses"], "100.00");
        }
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
        let app = crate::services::budgeting::get_report_in_currency(
            &conn,
            SEP,
            OCT - 1,
            "monthly",
            None,
            "CZK",
        )
        .unwrap();
        assert_eq!(result["summary"]["totalExpenses"], app.total_expenses);
        assert_eq!(result["quality"]["missingFxCount"], 0);
    }

    #[test]
    fn budgeting_report_converts_to_the_main_currency_and_flags_missing_fx() {
        let conn = app_db("EUR");
        seed_budget(&conn);
        // Native EUR requires no quote; cross-currency CZK uses historical EUR quotes.
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, category_id) VALUES ('eur', 'acc', 'debit', '10', 'EUR', ?1, 'cat_groceries')",
            [SEP + 86400],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'CZK',25)",
            [SEP],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'CZK',25)",
            [SEP + 86400],
        )
        .unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,tx_type,amount,currency,booking_date,category_id) VALUES ('missing','acc','debit','99','XZZ',?1,'cat_groceries')",[SEP+86400]).unwrap();
        let result = budgeting_report(&conn, SEP, OCT - 1, None).expect("budgeting");

        assert_eq!(result["mainCurrency"], "EUR");
        let cat = &result["budgetCategories"][0];
        // Historical CZK500 becomes EUR20, plus native EUR10.
        assert_eq!(cat["budget"], "40.00");
        assert_eq!(cat["actual"], "30.00");
        assert_eq!(cat["usagePercent"], "N/A");
        assert_eq!(result["quality"]["missingFxCount"], 1);
        assert_eq!(result["quality"]["missingFxCurrencies"][0], "XZZ");
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
        let app =
            crate::services::cashflow_planning::get_report_in_currency(&conn, "monthly", "CZK")
                .unwrap();

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
        // A private code isolates this current-rate fixture from other tests.
        crate::services::fx::set_current_for_test(&[("CZK", 25.0), ("XMC", 1.0)]);
        let conn = app_db("XMC");
        seed_cashflow(&conn);

        let monthly = cashflow_report(&conn, None).expect("cashflow");
        let yearly = cashflow_report(&conn, Some("yearly".to_string())).expect("cashflow");

        assert_eq!(monthly["mainCurrency"], "XMC");
        // 50 000 CZK salary, 10 000 CZK mortgage at 25 CZK per EUR.
        assert_eq!(monthly["summary"]["totalIncome"], "2000.00");
        assert_eq!(monthly["summary"]["totalExpenses"], "400.00");
        assert_eq!(monthly["summary"]["netCashflow"], "1600.00");
        assert_eq!(yearly["viewType"], "yearly");
        assert_eq!(yearly["summary"]["totalIncome"], "24000.00");
    }
}
