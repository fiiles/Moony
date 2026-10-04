//! Cashflow report commands

use crate::db::Database;
use crate::error::Result;
use crate::models::frequency::annualize;
use crate::models::{CashflowItem, InsertCashflowItem};
use crate::services::currency::convert_to_czk;
use crate::services::interest_tiers;
use serde::Serialize;
use std::collections::HashMap;
use tauri::State;
use uuid::Uuid;

/// Individual item in a cashflow category
#[derive(Debug, Clone, Serialize)]
pub struct CashflowReportItem {
    pub id: String,
    pub name: String,
    pub amount: f64, // Amount in user's selected period (monthly or yearly)
    #[serde(rename = "originalAmount")]
    pub original_amount: f64, // Original amount before period conversion
    #[serde(rename = "originalCurrency")]
    pub original_currency: String,
    #[serde(rename = "originalFrequency")]
    pub original_frequency: String, // "monthly" or "yearly"
    #[serde(rename = "isUserDefined")]
    pub is_user_defined: bool,
}

/// A category with its items
#[derive(Debug, Clone, Serialize)]
pub struct CashflowCategory {
    pub key: String,
    pub name: String,
    pub total: f64,
    pub items: Vec<CashflowReportItem>,
    #[serde(rename = "isUserEditable")]
    pub is_user_editable: bool,
}

/// A cashflow section (Personal or Investments)
#[derive(Debug, Clone, Serialize)]
pub struct CashflowSection {
    pub income: Vec<CashflowCategory>,
    pub expenses: Vec<CashflowCategory>,
    #[serde(rename = "totalIncome")]
    pub total_income: f64,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: f64,
    #[serde(rename = "netCashflow")]
    pub net_cashflow: f64,
}

/// Complete cashflow report with Personal and Investments sections
#[derive(Debug, Clone, Serialize)]
pub struct CashflowReport {
    #[serde(rename = "viewType")]
    pub view_type: String, // "monthly" or "yearly"
    pub personal: CashflowSection,
    pub investments: CashflowSection,
    #[serde(rename = "totalIncome")]
    pub total_income: f64,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: f64,
    #[serde(rename = "netCashflow")]
    pub net_cashflow: f64,
}

/// Normalize amount to target period
fn normalize_to_period(amount: f64, original_frequency: &str, target_period: &str) -> f64 {
    match (original_frequency, target_period) {
        ("yearly", "monthly") => amount / 12.0,
        ("monthly", "yearly") => amount * 12.0,
        _ => amount,
    }
}

/// Sort items alphabetically by name
fn sort_items_alphabetically(items: &mut [CashflowReportItem]) {
    items.sort_by_key(|item| item.name.to_lowercase());
}

/// Helper to get user-defined items by category
fn get_user_items_by_category(
    conn: &rusqlite::Connection,
    target_period: &str,
) -> rusqlite::Result<HashMap<String, Vec<CashflowReportItem>>> {
    let mut map: HashMap<String, Vec<CashflowReportItem>> = HashMap::new();

    let mut stmt =
        conn.prepare("SELECT id, name, amount, currency, frequency, category FROM cashflow_items")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        let original_amount: f64 = row.2.parse().unwrap_or(0.0);
        let amount_czk = convert_to_czk(original_amount, &row.3);
        let normalized = normalize_to_period(amount_czk, &row.4, target_period);

        let item = CashflowReportItem {
            id: row.0,
            name: row.1,
            amount: normalized,
            original_amount,
            original_currency: row.3,
            original_frequency: row.4,
            is_user_defined: true,
        };

        map.entry(row.5).or_default().push(item);
    }

    Ok(map)
}

/// Get real estate IDs by type (personal vs investment)
fn get_real_estate_ids_by_type(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
    let mut personal_ids = Vec::new();
    let mut investment_ids = Vec::new();

    let mut stmt = conn.prepare("SELECT id, type FROM real_estate")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        if row.1 == "investment" {
            investment_ids.push(row.0);
        } else {
            personal_ids.push(row.0);
        }
    }

    Ok((personal_ids, investment_ids))
}

/// Get loan IDs linked to investment real estate
fn get_investment_loan_ids(
    conn: &rusqlite::Connection,
    investment_re_ids: &[String],
) -> rusqlite::Result<Vec<String>> {
    if investment_re_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: String = investment_re_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT DISTINCT loan_id FROM real_estate_loans WHERE real_estate_id IN ({})",
        placeholders
    );

    let mut stmt = conn.prepare(&query)?;
    let params: Vec<&dyn rusqlite::ToSql> = investment_re_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Get insurance IDs linked to investment real estate
fn get_investment_insurance_ids(
    conn: &rusqlite::Connection,
    investment_re_ids: &[String],
) -> rusqlite::Result<Vec<String>> {
    if investment_re_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: String = investment_re_ids
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let query = format!(
        "SELECT DISTINCT insurance_id FROM real_estate_insurances WHERE real_estate_id IN ({})",
        placeholders
    );

    let mut stmt = conn.prepare(&query)?;
    let params: Vec<&dyn rusqlite::ToSql> = investment_re_ids
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;

    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Actual cashflow from bank transactions for `6m`, `12m` or `ytd` (design
/// system README §11): monthly sums, expenses by category, income by source,
/// plus the preceding range for comparison.
#[tauri::command]
pub async fn get_cashflow_actuals(
    db: State<'_, Database>,
    range: String,
) -> Result<crate::models::CashflowActuals> {
    crate::services::cashflow_actuals::validate_range(&range)?;
    crate::services::cashflow_actuals::get_cashflow_actuals(&db, range).await
}

/// Get cashflow report
#[tauri::command]
pub async fn get_cashflow_report(
    db: State<'_, Database>,
    view_type: String,
) -> Result<CashflowReport> {
    let target_period = if view_type == "yearly" {
        "yearly"
    } else {
        "monthly"
    };

    db.with_conn(|conn| {
        // Get all user-defined items grouped by category
        let user_items = get_user_items_by_category(conn, target_period)?;

        // Get real estate IDs by type
        let (personal_re_ids, investment_re_ids) = get_real_estate_ids_by_type(conn)?;

        // Get loan/insurance IDs linked to investment real estate
        let investment_loan_ids = get_investment_loan_ids(conn, &investment_re_ids)?;
        let investment_insurance_ids = get_investment_insurance_ids(conn, &investment_re_ids)?;

        // ==================== PERSONAL SECTION ====================
        let mut personal_income_categories: Vec<CashflowCategory> = Vec::new();
        let mut personal_expense_categories: Vec<CashflowCategory> = Vec::new();

        // Personal Income (user-defined only)
        let mut personal_income_items: Vec<CashflowReportItem> = user_items.get("personalIncome").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut personal_income_items);
        let personal_income_total: f64 = personal_income_items.iter().map(|i| i.amount).sum();
        personal_income_categories.push(CashflowCategory {
            key: "personalIncome".to_string(), name: "Personal Income".to_string(),
            total: personal_income_total, items: personal_income_items, is_user_editable: true,
        });

        // Personal Loan Payments (loans NOT linked to investment real estate)
        let mut personal_loan_items: Vec<CashflowReportItem> = Vec::new();
        let mut loan_stmt = conn.prepare("SELECT id, name, monthly_payment, currency FROM loans")?;
        let loans = loan_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        })?;

        for loan in loans.filter_map(|r| r.ok()) {
            if !investment_loan_ids.contains(&loan.0) {
                let monthly_payment: f64 = loan.2.parse().unwrap_or(0.0);
                let monthly_payment_czk = convert_to_czk(monthly_payment, &loan.3);
                let normalized = normalize_to_period(monthly_payment_czk, "monthly", target_period);

                if normalized > 0.0 {
                    personal_loan_items.push(CashflowReportItem {
                        id: loan.0, name: loan.1, amount: normalized,
                        original_amount: monthly_payment, original_currency: loan.3,
                        original_frequency: "monthly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalLoans").cloned() {
            personal_loan_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_loan_items);
        let personal_loan_total: f64 = personal_loan_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "personalLoans".to_string(), name: "Loan Payments".to_string(),
            total: personal_loan_total, items: personal_loan_items, is_user_editable: true,
        });

        // Personal Insurance Payments (insurance NOT linked to investment real estate)
        let mut personal_insurance_items: Vec<CashflowReportItem> = Vec::new();
        let mut ins_stmt = conn.prepare(
            "SELECT id, policy_name, regular_payment, regular_payment_currency, payment_frequency FROM insurance_policies WHERE status = 'active'"
        )?;
        let insurances = ins_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?))
        })?;

        for ins in insurances.filter_map(|r| r.ok()) {
            if !investment_insurance_ids.contains(&ins.0) {
                let payment: f64 = ins.2.parse().unwrap_or(0.0);
                let yearly_payment = annualize(payment, &ins.4);
                let yearly_payment_czk = convert_to_czk(yearly_payment, &ins.3);
                let normalized = normalize_to_period(yearly_payment_czk, "yearly", target_period);

                if normalized > 0.0 {
                    personal_insurance_items.push(CashflowReportItem {
                        id: ins.0, name: ins.1, amount: normalized,
                        original_amount: yearly_payment, original_currency: ins.3,
                        original_frequency: "yearly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalInsurance").cloned() {
            personal_insurance_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_insurance_items);
        let personal_insurance_total: f64 = personal_insurance_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "personalInsurance".to_string(), name: "Insurance Payments".to_string(),
            total: personal_insurance_total, items: personal_insurance_items, is_user_editable: true,
        });

        // Personal Real Estate Costs
        let mut personal_re_cost_items: Vec<CashflowReportItem> = Vec::new();
        let mut re_cost_stmt = conn.prepare("SELECT id, name, recurring_costs FROM real_estate")?;
        let re_costs = re_cost_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        for re in re_costs.filter_map(|r| r.ok()) {
            if personal_re_ids.contains(&re.0) {
                let costs: Vec<serde_json::Value> = serde_json::from_str(&re.2).unwrap_or_default();
                let mut property_yearly_total = 0.0;
                for cost in costs {
                    let amount = cost.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let frequency = cost.get("frequency").and_then(|v| v.as_str()).unwrap_or("monthly");
                    let currency = cost.get("currency").and_then(|v| v.as_str()).unwrap_or("CZK");
                    let yearly_amount = annualize(amount, frequency);
                    property_yearly_total += convert_to_czk(yearly_amount, currency);
                }
                let normalized = normalize_to_period(property_yearly_total, "yearly", target_period);
                if normalized > 0.0 {
                    personal_re_cost_items.push(CashflowReportItem {
                        id: re.0, name: re.1, amount: normalized,
                        original_amount: property_yearly_total, original_currency: "CZK".to_string(),
                        original_frequency: "yearly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalRealEstateCosts").cloned() {
            personal_re_cost_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_re_cost_items);
        let personal_re_cost_total: f64 = personal_re_cost_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "personalRealEstateCosts".to_string(), name: "Personal Real Estate Costs".to_string(),
            total: personal_re_cost_total, items: personal_re_cost_items, is_user_editable: true,
        });

        // Subscriptions (user-defined only)
        let mut subscription_items: Vec<CashflowReportItem> = user_items.get("subscriptions").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut subscription_items);
        let subscription_total: f64 = subscription_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "subscriptions".to_string(), name: "Subscriptions".to_string(),
            total: subscription_total, items: subscription_items, is_user_editable: true,
        });

        // Consumption Costs (user-defined only)
        let mut consumption_items: Vec<CashflowReportItem> = user_items.get("consumptionCosts").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut consumption_items);
        let consumption_total: f64 = consumption_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "consumptionCosts".to_string(), name: "Consumption Costs".to_string(),
            total: consumption_total, items: consumption_items, is_user_editable: true,
        });

        // Other Personal Costs (user-defined only)
        let mut other_personal_cost_items: Vec<CashflowReportItem> = user_items.get("otherPersonalCosts").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut other_personal_cost_items);
        let other_personal_cost_total: f64 = other_personal_cost_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(CashflowCategory {
            key: "otherPersonalCosts".to_string(), name: "Other Personal Costs".to_string(),
            total: other_personal_cost_total, items: other_personal_cost_items, is_user_editable: true,
        });

        // Calculate personal totals
        let personal_total_income: f64 = personal_income_categories.iter().map(|c| c.total).sum();
        let personal_total_expenses: f64 = personal_expense_categories.iter().map(|c| c.total).sum();
        let personal_net_cashflow = personal_total_income - personal_total_expenses;

        let personal_section = CashflowSection {
            income: personal_income_categories,
            expenses: personal_expense_categories,
            total_income: personal_total_income,
            total_expenses: personal_total_expenses,
            net_cashflow: personal_net_cashflow,
        };

        // ==================== INVESTMENTS SECTION ====================
        let mut investment_income_categories: Vec<CashflowCategory> = Vec::new();
        let mut investment_expense_categories: Vec<CashflowCategory> = Vec::new();

        // 1. Interest Income (Savings)
        let mut savings_items: Vec<CashflowReportItem> = Vec::new();
        let mut savings_stmt = conn.prepare(
            "SELECT id, name, balance, currency, interest_rate, has_zone_designation FROM bank_accounts WHERE account_type = 'savings' AND exclude_from_balance = 0"
        )?;
        let savings_rows = savings_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i32>(5)?,
            ))
        })?;

        for row in savings_rows.filter_map(|r| r.ok()) {
            let balance: f64 = row.2.parse().unwrap_or(0.0);
            let base_interest_rate: f64 = row.4.parse().unwrap_or(0.0);
            let has_zones = row.5 != 0;

            // One shared tier model: progressive bands, not "fill"
            let yearly_interest = interest_tiers::account_yearly_interest(
                conn,
                &row.0,
                balance,
                base_interest_rate,
                has_zones,
            )?;
            let yearly_interest_czk = convert_to_czk(yearly_interest, &row.3);
            let normalized = normalize_to_period(yearly_interest_czk, "yearly", target_period);

            if normalized > 0.0 {
                savings_items.push(CashflowReportItem {
                    id: row.0, name: row.1, amount: normalized,
                    original_amount: yearly_interest, original_currency: row.3,
                    original_frequency: "yearly".to_string(), is_user_defined: false,
                });
            }
        }
        if let Some(mut items) = user_items.get("interestIncome").cloned() {
            savings_items.append(&mut items);
        }
        sort_items_alphabetically(&mut savings_items);
        let savings_total: f64 = savings_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(CashflowCategory {
            key: "interestIncome".to_string(), name: "Interest Income".to_string(),
            total: savings_total, items: savings_items, is_user_editable: true,
        });

        // 2. Stock Dividends
        let mut dividend_items: Vec<CashflowReportItem> = Vec::new();
        let mut inv_stmt = conn.prepare("SELECT ticker, company_name, quantity FROM stock_investments")?;
        let investments = inv_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        for inv in investments.filter_map(|r| r.ok()) {
            let qty: f64 = inv.2.parse().unwrap_or(0.0);
            let dividend_data: rusqlite::Result<(Option<String>, Option<String>)> = conn.query_row(
                "SELECT COALESCE(
                    (SELECT yearly_dividend_sum FROM dividend_overrides WHERE ticker = ?1),
                    (SELECT yearly_dividend_sum FROM dividend_data WHERE ticker = ?1)
                ),
                COALESCE(
                    (SELECT currency FROM dividend_overrides WHERE ticker = ?1),
                    (SELECT currency FROM dividend_data WHERE ticker = ?1)
                )", [&inv.0], |row| Ok((row.get(0)?, row.get(1)?)),
            );

            if let Ok((Some(div_sum_str), Some(currency))) = dividend_data {
                let div_per_share: f64 = div_sum_str.parse().unwrap_or(0.0);
                let yearly_dividend = div_per_share * qty;
                let yearly_dividend_czk = convert_to_czk(yearly_dividend, &currency);
                let normalized = normalize_to_period(yearly_dividend_czk, "yearly", target_period);

                if normalized > 0.0 {
                    dividend_items.push(CashflowReportItem {
                        id: inv.0.clone(), name: inv.1, amount: normalized,
                        original_amount: yearly_dividend, original_currency: currency,
                        original_frequency: "yearly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("stockDividends").cloned() {
            dividend_items.append(&mut items);
        }
        sort_items_alphabetically(&mut dividend_items);
        let dividend_total: f64 = dividend_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(CashflowCategory {
            key: "stockDividends".to_string(), name: "Stock Dividends".to_string(),
            total: dividend_total, items: dividend_items, is_user_editable: true,
        });

        // 3. Bonds Interest
        let mut bond_items: Vec<CashflowReportItem> = Vec::new();
        let mut bond_stmt = conn.prepare("SELECT id, name, coupon_value, quantity, interest_rate, currency FROM bonds")?;
        let bonds = bond_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?))
        })?;

        for bond in bonds.filter_map(|r| r.ok()) {
            let coupon_value: f64 = bond.2.parse().unwrap_or(0.0);
            let quantity: f64 = bond.3.parse().unwrap_or(1.0);
            let interest_rate: f64 = bond.4.parse().unwrap_or(0.0);
            let yearly_interest = coupon_value * quantity * interest_rate / 100.0;
            let yearly_interest_czk = convert_to_czk(yearly_interest, &bond.5);
            let normalized = normalize_to_period(yearly_interest_czk, "yearly", target_period);

            if normalized > 0.0 {
                bond_items.push(CashflowReportItem {
                    id: bond.0, name: bond.1, amount: normalized,
                    original_amount: yearly_interest, original_currency: bond.5,
                    original_frequency: "yearly".to_string(), is_user_defined: false,
                });
            }
        }
        if let Some(mut items) = user_items.get("bondsInterest").cloned() {
            bond_items.append(&mut items);
        }
        sort_items_alphabetically(&mut bond_items);
        let bond_total: f64 = bond_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(CashflowCategory {
            key: "bondsInterest".to_string(), name: "Bonds Interest".to_string(),
            total: bond_total, items: bond_items, is_user_editable: true,
        });

        // 4. Rental Income (from investment real estate only)
        let mut rental_items: Vec<CashflowReportItem> = Vec::new();
        let mut re_stmt = conn.prepare(
            "SELECT id, name, monthly_rent, monthly_rent_currency FROM real_estate WHERE monthly_rent IS NOT NULL AND monthly_rent != ''"
        )?;
        let real_estates = re_stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))
        })?;

        for re in real_estates.filter_map(|r| r.ok()) {
            if investment_re_ids.contains(&re.0) {
                if let Some(rent_str) = re.2 {
                    let monthly_rent: f64 = rent_str.parse().unwrap_or(0.0);
                    let currency = re.3.unwrap_or_else(|| "CZK".to_string());
                    let monthly_rent_czk = convert_to_czk(monthly_rent, &currency);
                    let normalized = normalize_to_period(monthly_rent_czk, "monthly", target_period);

                    if normalized > 0.0 {
                        rental_items.push(CashflowReportItem {
                            id: re.0, name: re.1, amount: normalized,
                            original_amount: monthly_rent, original_currency: currency,
                            original_frequency: "monthly".to_string(), is_user_defined: false,
                        });
                    }
                }
            }
        }
        if let Some(mut items) = user_items.get("rentalIncome").cloned() {
            rental_items.append(&mut items);
        }
        sort_items_alphabetically(&mut rental_items);
        let rental_total: f64 = rental_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(CashflowCategory {
            key: "rentalIncome".to_string(), name: "Rental Income".to_string(),
            total: rental_total, items: rental_items, is_user_editable: true,
        });

        // 5. Other Investment Income (user-defined only)
        let mut other_inv_income_items: Vec<CashflowReportItem> = user_items.get("otherInvestmentIncome").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut other_inv_income_items);
        let other_inv_income_total: f64 = other_inv_income_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(CashflowCategory {
            key: "otherInvestmentIncome".to_string(), name: "Other Investment Income".to_string(),
            total: other_inv_income_total, items: other_inv_income_items, is_user_editable: true,
        });

        // Investment Expenses

        // 1. Investment Real Estate Loans
        let mut investment_loan_items: Vec<CashflowReportItem> = Vec::new();
        let mut loan_stmt2 = conn.prepare("SELECT id, name, monthly_payment, currency FROM loans")?;
        let loans2 = loan_stmt2.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
                row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        })?;

        for loan in loans2.filter_map(|r| r.ok()) {
            if investment_loan_ids.contains(&loan.0) {
                let monthly_payment: f64 = loan.2.parse().unwrap_or(0.0);
                let monthly_payment_czk = convert_to_czk(monthly_payment, &loan.3);
                let normalized = normalize_to_period(monthly_payment_czk, "monthly", target_period);

                if normalized > 0.0 {
                    investment_loan_items.push(CashflowReportItem {
                        id: loan.0, name: loan.1, amount: normalized,
                        original_amount: monthly_payment, original_currency: loan.3,
                        original_frequency: "monthly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentLoans").cloned() {
            investment_loan_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_loan_items);
        let investment_loan_total: f64 = investment_loan_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(CashflowCategory {
            key: "investmentLoans".to_string(), name: "Investment Real Estate Loans".to_string(),
            total: investment_loan_total, items: investment_loan_items, is_user_editable: true,
        });

        // 2. Investment Real Estate Insurance
        let mut investment_insurance_items: Vec<CashflowReportItem> = Vec::new();
        let mut ins_stmt2 = conn.prepare(
            "SELECT id, policy_name, regular_payment, regular_payment_currency, payment_frequency FROM insurance_policies WHERE status = 'active'"
        )?;
        let insurances2 = ins_stmt2.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?))
        })?;

        for ins in insurances2.filter_map(|r| r.ok()) {
            if investment_insurance_ids.contains(&ins.0) {
                let payment: f64 = ins.2.parse().unwrap_or(0.0);
                let yearly_payment = annualize(payment, &ins.4);
                let yearly_payment_czk = convert_to_czk(yearly_payment, &ins.3);
                let normalized = normalize_to_period(yearly_payment_czk, "yearly", target_period);

                if normalized > 0.0 {
                    investment_insurance_items.push(CashflowReportItem {
                        id: ins.0, name: ins.1, amount: normalized,
                        original_amount: yearly_payment, original_currency: ins.3,
                        original_frequency: "yearly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentInsurance").cloned() {
            investment_insurance_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_insurance_items);
        let investment_insurance_total: f64 = investment_insurance_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(CashflowCategory {
            key: "investmentInsurance".to_string(), name: "Investment Real Estate Insurance".to_string(),
            total: investment_insurance_total, items: investment_insurance_items, is_user_editable: true,
        });

        // 3. Investment Real Estate Costs
        let mut investment_re_cost_items: Vec<CashflowReportItem> = Vec::new();
        let mut re_cost_stmt2 = conn.prepare("SELECT id, name, recurring_costs FROM real_estate")?;
        let re_costs2 = re_cost_stmt2.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;

        for re in re_costs2.filter_map(|r| r.ok()) {
            if investment_re_ids.contains(&re.0) {
                let costs: Vec<serde_json::Value> = serde_json::from_str(&re.2).unwrap_or_default();
                let mut property_yearly_total = 0.0;
                for cost in costs {
                    let amount = cost.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let frequency = cost.get("frequency").and_then(|v| v.as_str()).unwrap_or("monthly");
                    let currency = cost.get("currency").and_then(|v| v.as_str()).unwrap_or("CZK");
                    let yearly_amount = annualize(amount, frequency);
                    property_yearly_total += convert_to_czk(yearly_amount, currency);
                }
                let normalized = normalize_to_period(property_yearly_total, "yearly", target_period);
                if normalized > 0.0 {
                    investment_re_cost_items.push(CashflowReportItem {
                        id: re.0, name: re.1, amount: normalized,
                        original_amount: property_yearly_total, original_currency: "CZK".to_string(),
                        original_frequency: "yearly".to_string(), is_user_defined: false,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentRealEstateCosts").cloned() {
            investment_re_cost_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_re_cost_items);
        let investment_re_cost_total: f64 = investment_re_cost_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(CashflowCategory {
            key: "investmentRealEstateCosts".to_string(), name: "Investment Real Estate Costs".to_string(),
            total: investment_re_cost_total, items: investment_re_cost_items, is_user_editable: true,
        });

        // 4. Other Investment Costs (user-defined only)
        let mut other_inv_cost_items: Vec<CashflowReportItem> = user_items.get("otherInvestmentCosts").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut other_inv_cost_items);
        let other_inv_cost_total: f64 = other_inv_cost_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(CashflowCategory {
            key: "otherInvestmentCosts".to_string(), name: "Other Investment Costs".to_string(),
            total: other_inv_cost_total, items: other_inv_cost_items, is_user_editable: true,
        });

        // Calculate investment totals
        let investment_total_income: f64 = investment_income_categories.iter().map(|c| c.total).sum();
        let investment_total_expenses: f64 = investment_expense_categories.iter().map(|c| c.total).sum();
        let investment_net_cashflow = investment_total_income - investment_total_expenses;

        let investments_section = CashflowSection {
            income: investment_income_categories,
            expenses: investment_expense_categories,
            total_income: investment_total_income,
            total_expenses: investment_total_expenses,
            net_cashflow: investment_net_cashflow,
        };

        // Calculate overall totals
        let total_income = personal_total_income + investment_total_income;
        let total_expenses = personal_total_expenses + investment_total_expenses;
        let net_cashflow = total_income - total_expenses;

        Ok(CashflowReport {
            view_type: target_period.to_string(),
            personal: personal_section,
            investments: investments_section,
            total_income,
            total_expenses,
            net_cashflow,
        })
    })
}

/// Get all user-defined cashflow items
#[tauri::command]
pub async fn get_all_cashflow_items(db: State<'_, Database>) -> Result<Vec<CashflowItem>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, amount, currency, frequency, item_type, category, created_at, updated_at
             FROM cashflow_items ORDER BY created_at DESC"
        )?;

        let items = stmt.query_map([], |row| {
            Ok(CashflowItem {
                id: row.get(0)?,
                name: row.get(1)?,
                amount: row.get(2)?,
                currency: row.get(3)?,
                frequency: row.get(4)?,
                item_type: row.get(5)?,
                category: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(items)
    })
}

/// Create a new cashflow item
#[tauri::command]
pub async fn create_cashflow_item(
    db: State<'_, Database>,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let currency = data.currency.unwrap_or_else(|| "CZK".to_string());

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO cashflow_items (id, name, amount, currency, frequency, item_type, category, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                data.name,
                data.amount,
                currency,
                data.frequency,
                data.item_type,
                data.category,
                now,
                now,
            ],
        )?;

        Ok(CashflowItem {
            id,
            name: data.name,
            amount: data.amount,
            currency,
            frequency: data.frequency,
            item_type: data.item_type,
            category: data.category,
            created_at: now,
            updated_at: now,
        })
    })
}

/// Update a cashflow item
#[tauri::command]
pub async fn update_cashflow_item(
    db: State<'_, Database>,
    id: String,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    let now = chrono::Utc::now().timestamp();
    let currency = data.currency.unwrap_or_else(|| "CZK".to_string());

    db.with_conn(move |conn| {
        conn.execute(
            "UPDATE cashflow_items
             SET name = ?2, amount = ?3, currency = ?4, frequency = ?5, item_type = ?6, category = ?7, updated_at = ?8
             WHERE id = ?1",
            rusqlite::params![
                id,
                data.name,
                data.amount,
                currency,
                data.frequency,
                data.item_type,
                data.category,
                now,
            ],
        )?;

        let item = conn.query_row(
            "SELECT id, name, amount, currency, frequency, item_type, category, created_at, updated_at
             FROM cashflow_items WHERE id = ?1",
            [&id],
            |row| {
                Ok(CashflowItem {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    amount: row.get(2)?,
                    currency: row.get(3)?,
                    frequency: row.get(4)?,
                    item_type: row.get(5)?,
                    category: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            },
        )?;

        Ok(item)
    })
}

/// Delete a cashflow item
#[tauri::command]
pub async fn delete_cashflow_item(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(move |conn| {
        conn.execute("DELETE FROM cashflow_items WHERE id = ?1", [&id])?;
        Ok(())
    })
}
