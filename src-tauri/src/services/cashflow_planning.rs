//! Monthly-equivalent planning estimates, separate from booked actuals.
use crate::error::{AppError, Result};
use crate::models::frequency::annualize;
use crate::models::CashflowReport;
use crate::models::{CashflowItem, InsertCashflowItem};
use crate::services::interest_tiers;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;
/// Individual item in a cashflow category
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RawCashflowReportItem {
    pub id: String,
    pub name: String,
    pub amount: f64,          // Amount in user's selected period (monthly or yearly)
    pub original_amount: f64, // Original amount before period conversion
    pub original_currency: String,
    pub original_frequency: String, // "monthly" or "yearly"
    pub is_user_defined: bool,
    pub source_fx_quality: Option<crate::models::FxQuality>,
}

/// A category with its items
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RawCashflowCategory {
    pub key: String,
    pub name: String,
    pub total: f64,
    pub items: Vec<RawCashflowReportItem>,
    pub is_user_editable: bool,
}

/// A cashflow section (Personal or Investments)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RawCashflowSection {
    pub income: Vec<RawCashflowCategory>,
    pub expenses: Vec<RawCashflowCategory>,
    pub total_income: f64,
    pub total_expenses: f64,
    pub net_cashflow: f64,
}

/// Complete cashflow report with Personal and Investments sections
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RawCashflowReport {
    pub view_type: String, // "monthly" or "yearly"
    pub personal: RawCashflowSection,
    pub investments: RawCashflowSection,
    pub total_income: f64,
    pub total_expenses: f64,
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

/// Converted property subtotals retain estimated/missing component quality even
/// though the aggregate's amount is denominated in CZK.
fn property_costs_annualized(
    costs_json: &str,
    fx: &mut PlanningFx,
) -> (f64, crate::models::FxQuality) {
    use crate::models::FxQuality;
    let costs: Vec<serde_json::Value> = serde_json::from_str(costs_json).unwrap_or_default();
    let mut total = 0.0;
    let mut quality = FxQuality::Historical;
    for cost in costs {
        let amount = cost.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let frequency = cost
            .get("frequency")
            .and_then(|v| v.as_str())
            .unwrap_or("monthly");
        let currency = cost
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("CZK");
        total += fx.convert(annualize(amount, frequency), currency);
        quality = match (quality, fx.quality_for(currency)) {
            (FxQuality::Missing, _) | (_, FxQuality::Missing) => FxQuality::Missing,
            (FxQuality::Estimated, _) | (_, FxQuality::Estimated) => FxQuality::Estimated,
            _ => FxQuality::Historical,
        };
    }
    (total, quality)
}

/// Sort items alphabetically by name
fn sort_items_alphabetically(items: &mut [RawCashflowReportItem]) {
    items.sort_by_key(|item| item.name.to_lowercase());
}

/// Helper to get user-defined items by category
fn get_user_items_by_category(
    conn: &rusqlite::Connection,
    target_period: &str,
    fx: &mut PlanningFx,
) -> Result<HashMap<String, Vec<RawCashflowReportItem>>> {
    let mut map: HashMap<String, Vec<RawCashflowReportItem>> = HashMap::new();

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

    for row in rows.collect::<std::result::Result<Vec<_>, _>>()? {
        let original_amount: f64 = crate::services::reporting::parse_money(&row.2)?;
        let amount_czk = fx.convert(original_amount, &row.3);
        let normalized = normalize_to_period(amount_czk, &row.4, target_period);

        let item = RawCashflowReportItem {
            id: row.0,
            name: row.1,
            amount: normalized,
            original_amount,
            original_currency: row.3,
            original_frequency: row.4,
            is_user_defined: true,
            source_fx_quality: None,
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

    for row in rows.collect::<std::result::Result<Vec<_>, _>>()? {
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

    rows.collect::<std::result::Result<Vec<_>, _>>()
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

    rows.collect::<std::result::Result<Vec<_>, _>>()
}

fn get_raw_report(
    conn: &rusqlite::Connection,
    view_type: &str,
    fx: &mut PlanningFx,
) -> Result<RawCashflowReport> {
    let target_period = if view_type == "yearly" {
        "yearly"
    } else {
        "monthly"
    };

    {
        let active_loans = active_loan_ids(conn, chrono::Utc::now().timestamp())?;
        // Get all user-defined items grouped by category
        let user_items = get_user_items_by_category(conn, target_period, fx)?;

        // Get real estate IDs by type
        let (personal_re_ids, investment_re_ids) = get_real_estate_ids_by_type(conn)?;

        // Get loan/insurance IDs linked to investment real estate
        let investment_loan_ids = get_investment_loan_ids(conn, &investment_re_ids)?;
        let investment_insurance_ids = get_investment_insurance_ids(conn, &investment_re_ids)?;

        // ==================== PERSONAL SECTION ====================
        let mut personal_income_categories: Vec<RawCashflowCategory> = Vec::new();
        let mut personal_expense_categories: Vec<RawCashflowCategory> = Vec::new();

        // Personal Income (user-defined only)
        let mut personal_income_items: Vec<RawCashflowReportItem> = user_items
            .get("personalIncome")
            .cloned()
            .unwrap_or_default();
        sort_items_alphabetically(&mut personal_income_items);
        let personal_income_total: f64 = personal_income_items.iter().map(|i| i.amount).sum();
        personal_income_categories.push(RawCashflowCategory {
            key: "personalIncome".to_string(),
            name: "Personal Income".to_string(),
            total: personal_income_total,
            items: personal_income_items,
            is_user_editable: true,
        });

        // Personal Loan Payments (loans NOT linked to investment real estate)
        let mut personal_loan_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut loan_stmt =
            conn.prepare("SELECT id, name, monthly_payment, currency FROM loans")?;
        let loans = loan_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        for loan in loans.collect::<std::result::Result<Vec<_>, _>>()? {
            if !active_loans.contains(&loan.0) {
                continue;
            }
            if !investment_loan_ids.contains(&loan.0) {
                let monthly_payment: f64 = crate::services::reporting::parse_money(&loan.2)?;
                let monthly_payment_czk = fx.convert(monthly_payment, &loan.3);
                let normalized = normalize_to_period(monthly_payment_czk, "monthly", target_period);

                if normalized > 0.0 || fx.last_missing {
                    personal_loan_items.push(RawCashflowReportItem {
                        id: loan.0,
                        name: loan.1,
                        amount: normalized,
                        original_amount: monthly_payment,
                        original_currency: loan.3,
                        original_frequency: "monthly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: None,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalLoans").cloned() {
            personal_loan_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_loan_items);
        let personal_loan_total: f64 = personal_loan_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "personalLoans".to_string(),
            name: "Loan Payments".to_string(),
            total: personal_loan_total,
            items: personal_loan_items,
            is_user_editable: true,
        });

        // Personal Insurance Payments (insurance NOT linked to investment real estate)
        let mut personal_insurance_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut ins_stmt = conn.prepare(
            "SELECT id, policy_name, regular_payment, regular_payment_currency, payment_frequency FROM insurance_policies WHERE status = 'active'"
        )?;
        let insurances = ins_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        for ins in insurances.collect::<std::result::Result<Vec<_>, _>>()? {
            if !investment_insurance_ids.contains(&ins.0) {
                let payment: f64 = crate::services::reporting::parse_money(&ins.2)?;
                let yearly_payment = annualize(payment, &ins.4);
                let yearly_payment_czk = fx.convert(yearly_payment, &ins.3);
                let normalized = normalize_to_period(yearly_payment_czk, "yearly", target_period);

                if normalized > 0.0 || fx.last_missing {
                    personal_insurance_items.push(RawCashflowReportItem {
                        id: ins.0,
                        name: ins.1,
                        amount: normalized,
                        original_amount: yearly_payment,
                        original_currency: ins.3,
                        original_frequency: "yearly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: None,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalInsurance").cloned() {
            personal_insurance_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_insurance_items);
        let personal_insurance_total: f64 = personal_insurance_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "personalInsurance".to_string(),
            name: "Insurance Payments".to_string(),
            total: personal_insurance_total,
            items: personal_insurance_items,
            is_user_editable: true,
        });

        // Personal Real Estate Costs
        let mut personal_re_cost_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut re_cost_stmt = conn.prepare("SELECT id, name, recurring_costs FROM real_estate")?;
        let re_costs = re_cost_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        for re in re_costs.collect::<std::result::Result<Vec<_>, _>>()? {
            if personal_re_ids.contains(&re.0) {
                let (property_yearly_total, source_fx_quality) =
                    property_costs_annualized(&re.2, fx);
                let normalized =
                    normalize_to_period(property_yearly_total, "yearly", target_period);
                if normalized > 0.0 || source_fx_quality == crate::models::FxQuality::Missing {
                    personal_re_cost_items.push(RawCashflowReportItem {
                        id: re.0,
                        name: re.1,
                        amount: normalized,
                        original_amount: property_yearly_total,
                        original_currency: "MULTI".to_string(),
                        original_frequency: "yearly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: Some(source_fx_quality),
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("personalRealEstateCosts").cloned() {
            personal_re_cost_items.append(&mut items);
        }
        sort_items_alphabetically(&mut personal_re_cost_items);
        let personal_re_cost_total: f64 = personal_re_cost_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "personalRealEstateCosts".to_string(),
            name: "Personal Real Estate Costs".to_string(),
            total: personal_re_cost_total,
            items: personal_re_cost_items,
            is_user_editable: true,
        });

        // Subscriptions (user-defined only)
        let mut subscription_items: Vec<RawCashflowReportItem> =
            user_items.get("subscriptions").cloned().unwrap_or_default();
        sort_items_alphabetically(&mut subscription_items);
        let subscription_total: f64 = subscription_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "subscriptions".to_string(),
            name: "Subscriptions".to_string(),
            total: subscription_total,
            items: subscription_items,
            is_user_editable: true,
        });

        // Consumption Costs (user-defined only)
        let mut consumption_items: Vec<RawCashflowReportItem> = user_items
            .get("consumptionCosts")
            .cloned()
            .unwrap_or_default();
        sort_items_alphabetically(&mut consumption_items);
        let consumption_total: f64 = consumption_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "consumptionCosts".to_string(),
            name: "Consumption Costs".to_string(),
            total: consumption_total,
            items: consumption_items,
            is_user_editable: true,
        });

        // Other Personal Costs (user-defined only)
        let mut other_personal_cost_items: Vec<RawCashflowReportItem> = user_items
            .get("otherPersonalCosts")
            .cloned()
            .unwrap_or_default();
        sort_items_alphabetically(&mut other_personal_cost_items);
        let other_personal_cost_total: f64 =
            other_personal_cost_items.iter().map(|i| i.amount).sum();
        personal_expense_categories.push(RawCashflowCategory {
            key: "otherPersonalCosts".to_string(),
            name: "Other Personal Costs".to_string(),
            total: other_personal_cost_total,
            items: other_personal_cost_items,
            is_user_editable: true,
        });

        // Calculate personal totals
        let personal_total_income: f64 = personal_income_categories.iter().map(|c| c.total).sum();
        let personal_total_expenses: f64 =
            personal_expense_categories.iter().map(|c| c.total).sum();
        let personal_net_cashflow = personal_total_income - personal_total_expenses;

        let personal_section = RawCashflowSection {
            income: personal_income_categories,
            expenses: personal_expense_categories,
            total_income: personal_total_income,
            total_expenses: personal_total_expenses,
            net_cashflow: personal_net_cashflow,
        };

        // ==================== INVESTMENTS SECTION ====================
        let mut investment_income_categories: Vec<RawCashflowCategory> = Vec::new();
        let mut investment_expense_categories: Vec<RawCashflowCategory> = Vec::new();

        // 1. Interest Income (Savings)
        let mut savings_items: Vec<RawCashflowReportItem> = Vec::new();
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

        for row in savings_rows.collect::<std::result::Result<Vec<_>, _>>()? {
            let balance: f64 = crate::services::reporting::parse_money(&row.2)?;
            let base_interest_rate: f64 = crate::services::reporting::parse_money(&row.4)?;
            let has_zones = row.5 != 0;

            // One shared tier model: progressive bands, not "fill"
            let yearly_interest = interest_tiers::account_yearly_interest(
                conn,
                &row.0,
                balance,
                base_interest_rate,
                has_zones,
            )?;
            let yearly_interest_czk = fx.convert(yearly_interest, &row.3);
            let normalized = normalize_to_period(yearly_interest_czk, "yearly", target_period);

            if normalized > 0.0 || fx.last_missing {
                savings_items.push(RawCashflowReportItem {
                    id: row.0,
                    name: row.1,
                    amount: normalized,
                    original_amount: yearly_interest,
                    original_currency: row.3,
                    original_frequency: "yearly".to_string(),
                    is_user_defined: false,
                    source_fx_quality: None,
                });
            }
        }
        if let Some(mut items) = user_items.get("interestIncome").cloned() {
            savings_items.append(&mut items);
        }
        sort_items_alphabetically(&mut savings_items);
        let savings_total: f64 = savings_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(RawCashflowCategory {
            key: "interestIncome".to_string(),
            name: "Interest Income".to_string(),
            total: savings_total,
            items: savings_items,
            is_user_editable: true,
        });

        // 2. Stock Dividends
        let mut dividend_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut inv_stmt =
            conn.prepare("SELECT ticker, company_name, quantity FROM stock_investments")?;
        let investments = inv_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        for inv in investments.collect::<std::result::Result<Vec<_>, _>>()? {
            let qty: f64 = crate::services::reporting::parse_money(&inv.2)?;
            let dividend_data: (Option<String>, Option<String>) = conn.query_row(
                "SELECT COALESCE(
                    (SELECT yearly_dividend_sum FROM dividend_overrides WHERE ticker = ?1),
                    (SELECT yearly_dividend_sum FROM dividend_data WHERE ticker = ?1)
                ),
                COALESCE(
                    (SELECT currency FROM dividend_overrides WHERE ticker = ?1),
                    (SELECT currency FROM dividend_data WHERE ticker = ?1)
                )",
                [&inv.0],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            if let (Some(div_sum_str), Some(currency)) = dividend_data {
                let div_per_share: f64 = crate::services::reporting::parse_money(&div_sum_str)?;
                let yearly_dividend = div_per_share * qty;
                let yearly_dividend_czk = fx.convert(yearly_dividend, &currency);
                let normalized = normalize_to_period(yearly_dividend_czk, "yearly", target_period);

                if normalized > 0.0 || fx.last_missing {
                    dividend_items.push(RawCashflowReportItem {
                        id: inv.0.clone(),
                        name: inv.1,
                        amount: normalized,
                        original_amount: yearly_dividend,
                        original_currency: currency,
                        original_frequency: "yearly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: None,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("stockDividends").cloned() {
            dividend_items.append(&mut items);
        }
        sort_items_alphabetically(&mut dividend_items);
        let dividend_total: f64 = dividend_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(RawCashflowCategory {
            key: "stockDividends".to_string(),
            name: "Stock Dividends".to_string(),
            total: dividend_total,
            items: dividend_items,
            is_user_editable: true,
        });

        // 3. Bonds Interest
        let mut bond_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut bond_stmt = conn.prepare(
            "SELECT id, name, coupon_value, quantity, interest_rate, currency FROM bonds",
        )?;
        let bonds = bond_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        for bond in bonds.collect::<std::result::Result<Vec<_>, _>>()? {
            let coupon_value: f64 = crate::services::reporting::parse_money(&bond.2)?;
            let quantity: f64 = crate::services::reporting::parse_money(&bond.3)?;
            let interest_rate: f64 = crate::services::reporting::parse_money(&bond.4)?;
            let yearly_interest = coupon_value * quantity * interest_rate / 100.0;
            let yearly_interest_czk = fx.convert(yearly_interest, &bond.5);
            let normalized = normalize_to_period(yearly_interest_czk, "yearly", target_period);

            if normalized > 0.0 || fx.last_missing {
                bond_items.push(RawCashflowReportItem {
                    id: bond.0,
                    name: bond.1,
                    amount: normalized,
                    original_amount: yearly_interest,
                    original_currency: bond.5,
                    original_frequency: "yearly".to_string(),
                    is_user_defined: false,
                    source_fx_quality: None,
                });
            }
        }
        if let Some(mut items) = user_items.get("bondsInterest").cloned() {
            bond_items.append(&mut items);
        }
        sort_items_alphabetically(&mut bond_items);
        let bond_total: f64 = bond_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(RawCashflowCategory {
            key: "bondsInterest".to_string(),
            name: "Bonds Interest".to_string(),
            total: bond_total,
            items: bond_items,
            is_user_editable: true,
        });

        // 4. Rental Income (from investment real estate only)
        let mut rental_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut re_stmt = conn.prepare(
            "SELECT id, name, monthly_rent, monthly_rent_currency FROM real_estate WHERE monthly_rent IS NOT NULL AND monthly_rent != ''"
        )?;
        let real_estates = re_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;

        for re in real_estates.collect::<std::result::Result<Vec<_>, _>>()? {
            if investment_re_ids.contains(&re.0) {
                if let Some(rent_str) = re.2 {
                    let monthly_rent: f64 = crate::services::reporting::parse_money(&rent_str)?;
                    let currency = re.3.unwrap_or_else(|| "CZK".to_string());
                    let monthly_rent_czk = fx.convert(monthly_rent, &currency);
                    let normalized =
                        normalize_to_period(monthly_rent_czk, "monthly", target_period);

                    if normalized > 0.0 || fx.last_missing {
                        rental_items.push(RawCashflowReportItem {
                            id: re.0,
                            name: re.1,
                            amount: normalized,
                            original_amount: monthly_rent,
                            original_currency: currency,
                            original_frequency: "monthly".to_string(),
                            is_user_defined: false,
                            source_fx_quality: None,
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
        investment_income_categories.push(RawCashflowCategory {
            key: "rentalIncome".to_string(),
            name: "Rental Income".to_string(),
            total: rental_total,
            items: rental_items,
            is_user_editable: true,
        });

        // 5. Other Investment Income (user-defined only)
        let mut other_inv_income_items: Vec<RawCashflowReportItem> = user_items
            .get("otherInvestmentIncome")
            .cloned()
            .unwrap_or_default();
        sort_items_alphabetically(&mut other_inv_income_items);
        let other_inv_income_total: f64 = other_inv_income_items.iter().map(|i| i.amount).sum();
        investment_income_categories.push(RawCashflowCategory {
            key: "otherInvestmentIncome".to_string(),
            name: "Other Investment Income".to_string(),
            total: other_inv_income_total,
            items: other_inv_income_items,
            is_user_editable: true,
        });

        // Investment Expenses

        // 1. Investment Real Estate Loans
        let mut investment_loan_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut loan_stmt2 =
            conn.prepare("SELECT id, name, monthly_payment, currency FROM loans")?;
        let loans2 = loan_stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        for loan in loans2.collect::<std::result::Result<Vec<_>, _>>()? {
            if !active_loans.contains(&loan.0) {
                continue;
            }
            if investment_loan_ids.contains(&loan.0) {
                let monthly_payment: f64 = crate::services::reporting::parse_money(&loan.2)?;
                let monthly_payment_czk = fx.convert(monthly_payment, &loan.3);
                let normalized = normalize_to_period(monthly_payment_czk, "monthly", target_period);

                if normalized > 0.0 || fx.last_missing {
                    investment_loan_items.push(RawCashflowReportItem {
                        id: loan.0,
                        name: loan.1,
                        amount: normalized,
                        original_amount: monthly_payment,
                        original_currency: loan.3,
                        original_frequency: "monthly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: None,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentLoans").cloned() {
            investment_loan_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_loan_items);
        let investment_loan_total: f64 = investment_loan_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(RawCashflowCategory {
            key: "investmentLoans".to_string(),
            name: "Investment Real Estate Loans".to_string(),
            total: investment_loan_total,
            items: investment_loan_items,
            is_user_editable: true,
        });

        // 2. Investment Real Estate Insurance
        let mut investment_insurance_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut ins_stmt2 = conn.prepare(
            "SELECT id, policy_name, regular_payment, regular_payment_currency, payment_frequency FROM insurance_policies WHERE status = 'active'"
        )?;
        let insurances2 = ins_stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        for ins in insurances2.collect::<std::result::Result<Vec<_>, _>>()? {
            if investment_insurance_ids.contains(&ins.0) {
                let payment: f64 = crate::services::reporting::parse_money(&ins.2)?;
                let yearly_payment = annualize(payment, &ins.4);
                let yearly_payment_czk = fx.convert(yearly_payment, &ins.3);
                let normalized = normalize_to_period(yearly_payment_czk, "yearly", target_period);

                if normalized > 0.0 || fx.last_missing {
                    investment_insurance_items.push(RawCashflowReportItem {
                        id: ins.0,
                        name: ins.1,
                        amount: normalized,
                        original_amount: yearly_payment,
                        original_currency: ins.3,
                        original_frequency: "yearly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: None,
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentInsurance").cloned() {
            investment_insurance_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_insurance_items);
        let investment_insurance_total: f64 =
            investment_insurance_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(RawCashflowCategory {
            key: "investmentInsurance".to_string(),
            name: "Investment Real Estate Insurance".to_string(),
            total: investment_insurance_total,
            items: investment_insurance_items,
            is_user_editable: true,
        });

        // 3. Investment Real Estate Costs
        let mut investment_re_cost_items: Vec<RawCashflowReportItem> = Vec::new();
        let mut re_cost_stmt2 =
            conn.prepare("SELECT id, name, recurring_costs FROM real_estate")?;
        let re_costs2 = re_cost_stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        for re in re_costs2.collect::<std::result::Result<Vec<_>, _>>()? {
            if investment_re_ids.contains(&re.0) {
                let (property_yearly_total, source_fx_quality) =
                    property_costs_annualized(&re.2, fx);
                let normalized =
                    normalize_to_period(property_yearly_total, "yearly", target_period);
                if normalized > 0.0 || source_fx_quality == crate::models::FxQuality::Missing {
                    investment_re_cost_items.push(RawCashflowReportItem {
                        id: re.0,
                        name: re.1,
                        amount: normalized,
                        original_amount: property_yearly_total,
                        original_currency: "MULTI".to_string(),
                        original_frequency: "yearly".to_string(),
                        is_user_defined: false,
                        source_fx_quality: Some(source_fx_quality),
                    });
                }
            }
        }
        if let Some(mut items) = user_items.get("investmentRealEstateCosts").cloned() {
            investment_re_cost_items.append(&mut items);
        }
        sort_items_alphabetically(&mut investment_re_cost_items);
        let investment_re_cost_total: f64 = investment_re_cost_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(RawCashflowCategory {
            key: "investmentRealEstateCosts".to_string(),
            name: "Investment Real Estate Costs".to_string(),
            total: investment_re_cost_total,
            items: investment_re_cost_items,
            is_user_editable: true,
        });

        // 4. Other Investment Costs (user-defined only)
        let mut other_inv_cost_items: Vec<RawCashflowReportItem> = user_items
            .get("otherInvestmentCosts")
            .cloned()
            .unwrap_or_default();
        sort_items_alphabetically(&mut other_inv_cost_items);
        let other_inv_cost_total: f64 = other_inv_cost_items.iter().map(|i| i.amount).sum();
        investment_expense_categories.push(RawCashflowCategory {
            key: "otherInvestmentCosts".to_string(),
            name: "Other Investment Costs".to_string(),
            total: other_inv_cost_total,
            items: other_inv_cost_items,
            is_user_editable: true,
        });

        // Calculate investment totals
        let investment_total_income: f64 =
            investment_income_categories.iter().map(|c| c.total).sum();
        let investment_total_expenses: f64 =
            investment_expense_categories.iter().map(|c| c.total).sum();
        let investment_net_cashflow = investment_total_income - investment_total_expenses;

        let investments_section = RawCashflowSection {
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

        Ok(RawCashflowReport {
            view_type: target_period.to_string(),
            personal: personal_section,
            investments: investments_section,
            total_income,
            total_expenses,
            net_cashflow,
        })
    }
}
/// Get all user-defined cashflow items
pub fn get_all_cashflow_items(conn: &rusqlite::Connection) -> Result<Vec<CashflowItem>> {
    {
        let mut stmt = conn.prepare(
            "SELECT id, name, amount, currency, frequency, item_type, category, created_at, updated_at,replacement_source_id
             FROM cashflow_items ORDER BY created_at DESC"
        )?;

        let items = stmt
            .query_map([], |row| {
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
                    replacement_source_id: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(items)
    }
}

/// Create a new cashflow item
pub fn create_cashflow_item(
    conn: &rusqlite::Connection,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    data.validate()?;
    validate_replacement(conn, &data, None)?;
    let id = Uuid::new_v4().to_string();
    data.validate()?;
    let now = chrono::Utc::now().timestamp();
    let currency = data.currency.unwrap_or_else(|| "CZK".to_string());

    {
        conn.execute(
            "INSERT INTO cashflow_items (id, name, amount, currency, frequency, item_type, category, created_at, updated_at,replacement_source_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,?10)",
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
                data.replacement_source_id,
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
            replacement_source_id: data.replacement_source_id,
        })
    }
}

/// Update a cashflow item
pub fn update_cashflow_item(
    conn: &rusqlite::Connection,
    id: String,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    data.validate()?;
    validate_replacement(conn, &data, Some(&id))?;
    let now = chrono::Utc::now().timestamp();
    let currency = data.currency.unwrap_or_else(|| "CZK".to_string());

    {
        let updated=conn.execute(
            "UPDATE cashflow_items
             SET name = ?2, amount = ?3, currency = ?4, frequency = ?5, item_type = ?6, category = ?7, updated_at = ?8,replacement_source_id=?9
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
                data.replacement_source_id,
            ],
        )?;

        if updated == 0 {
            return Err(AppError::NotFound(format!("Cashflow item {id} not found")));
        }
        let item = conn.query_row(
            "SELECT id, name, amount, currency, frequency, item_type, category, created_at, updated_at,replacement_source_id
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
                    replacement_source_id:row.get(9)?,
                })
            },
        )?;

        Ok(item)
    }
}

/// Delete a cashflow item
pub fn delete_cashflow_item(conn: &rusqlite::Connection, id: String) -> Result<()> {
    {
        if conn.execute("DELETE FROM cashflow_items WHERE id = ?1", [&id])? == 0 {
            return Err(AppError::NotFound(format!("Cashflow item {id} not found")));
        }
        Ok(())
    }
}

fn active_loan_ids(conn: &rusqlite::Connection, day: i64) -> Result<HashSet<String>> {
    use crate::services::loan_amortization::{outstanding_balance_at, LoanTerms};
    use crate::services::reporting::parse_money;
    let mut stmt=conn.prepare("SELECT id,principal,interest_rate,monthly_payment,start_date,end_date,balance_anchor_amount,balance_anchor_date FROM loans")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<i64>>(7)?,
        ))
    })?;
    let mut active = HashSet::new();
    for row in rows {
        let (id, principal, rate, payment, start, end, anchor, anchor_day) = row?;
        let terms = LoanTerms {
            principal: parse_money(&principal)?,
            annual_rate_pct: if rate.is_empty() {
                0.0
            } else {
                parse_money(&rate)?
            },
            monthly_payment: parse_money(&payment)?,
            start_day: start,
            end_day: end,
            anchor_amount: anchor.as_deref().map(parse_money).transpose()?,
            anchor_day,
        };
        if start <= day
            && end.is_none_or(|end| end >= day)
            && outstanding_balance_at(&terms, day) > 0.005
        {
            active.insert(id);
        }
    }
    Ok(active)
}
pub fn get_report(conn: &rusqlite::Connection, view_type: &str) -> Result<CashflowReport> {
    get_report_in_currency(conn, view_type, "CZK")
}
pub fn get_report_in_currency(
    conn: &rusqlite::Connection,
    view_type: &str,
    report_currency: &str,
) -> Result<CashflowReport> {
    let report_currency = crate::services::reporting::normalize_currency(report_currency)?;
    let money = |v| crate::services::reporting::money_in_currency(v, &report_currency);
    let mut fx = PlanningFx {
        target: report_currency.clone(),
        ..PlanningFx::default()
    };
    let raw = get_raw_report(conn, view_type, &mut fx)?;
    for section in [&raw.personal, &raw.investments] {
        for item in section
            .income
            .iter()
            .chain(&section.expenses)
            .flat_map(|c| &c.items)
        {
            if !item.amount.is_finite() || !item.original_amount.is_finite() {
                return Err(AppError::Validation("validation.invalidAmount".into()));
            }
        }
    }
    let manual = get_all_cashflow_items(conn)?;
    let replacements: HashMap<_, _> = manual
        .iter()
        .filter_map(|i| {
            i.replacement_source_id
                .as_ref()
                .map(|source| (source.clone(), i.id.clone()))
        })
        .collect();
    let source_for = |key: &str, id: &str| {
        format!(
            "{}:{}",
            match key {
                "personalLoans" | "investmentLoans" => "loan",
                "personalInsurance" | "investmentInsurance" => "insurance",
                "personalRealEstateCosts" | "investmentRealEstateCosts" => "propertyCosts",
                "interestIncome" => "account",
                "stockDividends" => "stock",
                "bondsInterest" => "bond",
                "rentalIncome" => "rental",
                _ => key,
            },
            id
        )
    };
    let mut property_components = HashMap::new();
    let mut stmt = conn.prepare("SELECT id,recurring_costs FROM real_estate")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, json) = row?;
        let costs: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
        let components = costs
            .into_iter()
            .map(|c| crate::models::CashflowOriginalComponent {
                amount: c
                    .get("amount")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_else(|| "0".into()),
                currency: c
                    .get("currency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("CZK")
                    .to_string(),
                frequency: c
                    .get("frequency")
                    .and_then(|v| v.as_str())
                    .unwrap_or("monthly")
                    .to_string(),
            })
            .collect::<Vec<_>>();
        property_components.insert(id, components);
    }
    let mut automatic_sources = vec![];
    // Quality describes active contributions; replaced sources retain row-level
    // quality in inventory without making the effective plan incomplete.
    fx.quality = crate::models::ReportQuality::default();
    let mut section = |raw: RawCashflowSection| {
        let mut categories = |raw: Vec<RawCashflowCategory>, direction: &str| {
            raw.into_iter()
                .map(|c| {
                    let items = c
                        .items
                        .into_iter()
                        .filter_map(|i| {
                            let source = (!i.is_user_defined).then(|| source_for(&c.key, &i.id));
                            let original_components =
                                if !i.is_user_defined && c.key.ends_with("RealEstateCosts") {
                                    property_components.get(&i.id).cloned().unwrap_or_default()
                                } else {
                                    Vec::new()
                                };
                            let item = crate::models::CashflowReportItem {
                                original_components,
                                fx_quality: i
                                    .source_fx_quality
                                    .unwrap_or_else(|| fx.quality_for(&i.original_currency)),
                                category: c.key.clone(),
                                item_type: direction.into(),
                                id: i.id.clone(),
                                name: i.name,
                                amount: money(i.amount),
                                original_amount: manual
                                    .iter()
                                    .find(|m| m.id == i.id)
                                    .map(|m| m.amount.clone())
                                    .unwrap_or_else(|| i.original_amount.to_string()),
                                original_currency: i.original_currency,
                                original_frequency: i.original_frequency,
                                is_user_defined: i.is_user_defined,
                                source_id: source.clone(),
                                replacement_source_id: manual
                                    .iter()
                                    .find(|m| m.id == i.id)
                                    .and_then(|m| m.replacement_source_id.clone()),
                            };
                            if let Some(source) = source {
                                automatic_sources.push(item.clone());
                                if replacements.contains_key(&source) {
                                    return None;
                                }
                            }
                            if item.original_components.is_empty() {
                                fx.record_quality(&item.original_currency);
                            } else {
                                for component in &item.original_components {
                                    fx.record_quality(&component.currency);
                                }
                            }
                            Some(item)
                        })
                        .collect::<Vec<_>>();
                    let total: f64 = items
                        .iter()
                        .map(|i| i.amount.parse::<f64>().expect("money"))
                        .sum();
                    crate::models::CashflowCategory {
                        key: c.key,
                        name: c.name,
                        total: money(total),
                        items,
                        is_user_editable: c.is_user_editable,
                    }
                })
                .collect::<Vec<_>>()
        };
        let income = categories(raw.income, "income");
        let expenses = categories(raw.expenses, "expense");
        let total_income: f64 = income
            .iter()
            .map(|c| c.total.parse::<f64>().expect("money"))
            .sum();
        let total_expenses: f64 = expenses
            .iter()
            .map(|c| c.total.parse::<f64>().expect("money"))
            .sum();
        crate::models::CashflowSection {
            income,
            expenses,
            total_income: money(total_income),
            total_expenses: money(total_expenses),
            net_cashflow: money(total_income - total_expenses),
        }
    };
    let personal = section(raw.personal);
    let investments = section(raw.investments);
    let income = personal.total_income.parse::<f64>().expect("money")
        + investments.total_income.parse::<f64>().expect("money");
    let expense = personal.total_expenses.parse::<f64>().expect("money")
        + investments.total_expenses.parse::<f64>().expect("money");
    Ok(CashflowReport {
        report_currency: report_currency.clone(),
        view_type: raw.view_type,
        personal,
        investments,
        total_income: money(income),
        total_expenses: money(expense),
        net_cashflow: money(income - expense),
        automatic_sources,
        as_of: chrono::Utc::now().timestamp(),
        estimate: true,
        quality: fx.quality,
        rates_as_of: crate::services::currency::get_exchange_rates_fetched_at(),
    })
}
fn validate_replacement(
    conn: &rusqlite::Connection,
    data: &InsertCashflowItem,
    id: Option<&str>,
) -> Result<()> {
    if let Some(source) = &data.replacement_source_id {
        use rusqlite::OptionalExtension;
        let previous: Option<String> = if let Some(id) = id {
            conn.query_row(
                "SELECT replacement_source_id FROM cashflow_items WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .flatten()
        } else {
            None
        };
        let report = get_report(conn, "monthly")?;
        if previous.as_ref() != Some(source)
            && !report
                .automatic_sources
                .iter()
                .any(|i| i.source_id.as_ref() == Some(source) && i.item_type == data.item_type)
        {
            return Err(AppError::Validation(
                "validation.cashflowReplacementInvalid".into(),
            ));
        }
        let count:i64=conn.query_row("SELECT COUNT(*) FROM cashflow_items WHERE replacement_source_id=?1 AND id!=COALESCE(?2,'')",rusqlite::params![source,id],|r|r.get(0))?;
        if count > 0 {
            return Err(AppError::Validation(
                "validation.cashflowReplacementInvalid".into(),
            ));
        }
    }
    Ok(())
}

struct PlanningFx {
    target: String,
    rates: HashMap<String, f64>,
    quality: crate::models::ReportQuality,
    last_missing: bool,
}
impl Default for PlanningFx {
    fn default() -> Self {
        Self {
            target: "CZK".into(),
            rates: crate::services::currency::get_all_rates(),
            quality: crate::models::ReportQuality::default(),
            last_missing: false,
        }
    }
}
impl PlanningFx {
    fn rate(&self, currency: &str) -> Option<f64> {
        if currency.eq_ignore_ascii_case("CZK") {
            Some(1.0)
        } else {
            self.rates
                .get(&currency.trim().to_uppercase())
                .copied()
                .filter(|r| r.is_finite() && *r > 0.0)
        }
    }
    fn quality_for(&self, currency: &str) -> crate::models::FxQuality {
        use crate::models::FxQuality;
        if currency.trim().eq_ignore_ascii_case(&self.target) {
            FxQuality::Historical
        } else if self.rate(currency).is_some() && self.rate(&self.target).is_some() {
            FxQuality::Estimated
        } else {
            FxQuality::Missing
        }
    }
    fn record_quality(&mut self, currency: &str) {
        use crate::models::FxQuality;
        match self.quality_for(currency) {
            FxQuality::Historical => {}
            FxQuality::Estimated => self.quality.estimated_fx_count += 1,
            FxQuality::Missing => {
                self.quality.missing_fx_count += 1;
                for code in [currency.trim().to_uppercase(), self.target.clone()] {
                    if self.rate(&code).is_none()
                        && !self.quality.missing_fx_currencies.contains(&code)
                    {
                        self.quality.missing_fx_currencies.push(code);
                    }
                }
                self.quality.missing_fx_currencies.sort();
            }
        }
    }
    fn convert(&mut self, amount: f64, currency: &str) -> f64 {
        self.last_missing = false;
        self.record_quality(currency);
        if currency.trim().eq_ignore_ascii_case(&self.target) {
            return amount;
        }
        if let (Some(source), Some(target)) = (self.rate(currency), self.rate(&self.target)) {
            return amount * source / target;
        }
        self.last_missing = true;
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn planning_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE loans(id TEXT,name TEXT,principal TEXT,currency TEXT,interest_rate TEXT,monthly_payment TEXT,start_date INTEGER,end_date INTEGER,balance_anchor_amount TEXT,balance_anchor_date INTEGER);
 CREATE TABLE cashflow_items(id TEXT PRIMARY KEY,name TEXT,amount TEXT,currency TEXT,frequency TEXT,item_type TEXT,category TEXT,created_at INTEGER,updated_at INTEGER,replacement_source_id TEXT UNIQUE);
 CREATE TABLE real_estate(id TEXT,name TEXT,type TEXT,recurring_costs TEXT,monthly_rent TEXT,monthly_rent_currency TEXT);
 CREATE TABLE real_estate_loans(real_estate_id TEXT,loan_id TEXT);
 CREATE TABLE real_estate_insurances(real_estate_id TEXT,insurance_id TEXT);
 CREATE TABLE insurance_policies(id TEXT,policy_name TEXT,regular_payment TEXT,regular_payment_currency TEXT,payment_frequency TEXT,status TEXT);
 CREATE TABLE bank_accounts(id TEXT,name TEXT,balance TEXT,currency TEXT,interest_rate TEXT,has_zone_designation INTEGER,account_type TEXT,exclude_from_balance INTEGER);
 CREATE TABLE stock_investments(ticker TEXT,company_name TEXT,quantity TEXT);
 CREATE TABLE bonds(id TEXT,name TEXT,coupon_value TEXT,quantity TEXT,interest_rate TEXT,currency TEXT);
 INSERT INTO loans VALUES ('active','Loan','1000000','CZK','0','100',1704067200,NULL,NULL,NULL);").unwrap();
        conn
    }
    #[test]
    fn target_replaced_missing_source_does_not_make_active_plan_incomplete() {
        let conn = planning_db();
        conn.execute("UPDATE loans SET currency='XRS'", []).unwrap();
        create_cashflow_item(
            &conn,
            InsertCashflowItem {
                name: "Native replacement".into(),
                amount: "100".into(),
                currency: Some("EUR".into()),
                frequency: "monthly".into(),
                item_type: "expense".into(),
                category: "personalLoans".into(),
                replacement_source_id: Some("loan:active".into()),
            },
        )
        .unwrap();
        let report = get_report_in_currency(&conn, "monthly", "EUR").unwrap();
        assert_eq!(report.total_expenses, "100.00");
        assert_eq!(
            report.automatic_sources[0].fx_quality,
            crate::models::FxQuality::Missing
        );
        assert_eq!(report.quality.missing_fx_count, 0);
    }
    #[test]
    fn target_planning_mixed_property_components_and_replacement_conserve() {
        crate::services::currency::update_exchange_rates(HashMap::from([
            ("XTP".into(), 27.5),
            ("XSP".into(), 25.0),
        ]));
        let conn = planning_db();
        conn.execute("DELETE FROM loans", []).unwrap();
        insert_property(
            &conn,
            "personal",
            serde_json::json!([
                {"amount":100,"currency":"XTP","frequency":"monthly"},
                {"amount":110,"currency":"XSP","frequency":"monthly"}
            ]),
        );
        let cross_report = get_report_in_currency(&conn, "monthly", "XTP").unwrap();
        assert_eq!(cross_report.total_expenses, "200.00");
        assert_eq!(cross_report.automatic_sources[0].amount, "200.00");
        // Also assert the unrounded annual composite before period normalization.
        let mut fx = PlanningFx {
            target: "EUR".into(),
            rates: HashMap::from([("EUR".into(), 27.5), ("USD".into(), 25.0)]),
            ..PlanningFx::default()
        };
        let (annual, q) = property_costs_annualized(
            r#"[{"amount":100,"currency":"EUR","frequency":"monthly"},{"amount":110,"currency":"USD","frequency":"monthly"}]"#,
            &mut fx,
        );
        assert_eq!(annual, 2400.0);
        assert_eq!(q, crate::models::FxQuality::Estimated);
        // Same-currency component remains usable even if the other component is missing.
        conn.execute("UPDATE real_estate SET recurring_costs=?1", [r#"[{"amount":100,"currency":"AED","frequency":"monthly"},{"amount":110,"currency":"XZZ","frequency":"monthly"}]"#]).unwrap();
        let report = get_report_in_currency(&conn, "monthly", "AED").unwrap();
        let source = &report.automatic_sources[0];
        assert_eq!(source.amount, "100.00");
        assert_eq!(source.original_currency, "MULTI");
        assert_eq!(source.original_components.len(), 2);
        assert_eq!(source.original_components[0].currency, "AED");
        assert_eq!(source.fx_quality, crate::models::FxQuality::Missing);
        assert_eq!(report.total_expenses, "100.00");
        create_cashflow_item(
            &conn,
            InsertCashflowItem {
                name: "Replacement".into(),
                amount: "1200".into(),
                currency: Some("AED".into()),
                frequency: "yearly".into(),
                item_type: "expense".into(),
                category: "personalRealEstateCosts".into(),
                replacement_source_id: source.source_id.clone(),
            },
        )
        .unwrap();
        let replacement = get_report_in_currency(&conn, "monthly", "AED").unwrap();
        assert_eq!(replacement.total_expenses, "100.00");
        assert_eq!(replacement.automatic_sources.len(), 1);
        assert_eq!(
            replacement
                .personal
                .expenses
                .iter()
                .flat_map(|c| &c.items)
                .count(),
            1
        );
    }
    #[test]
    fn target_planning_minor_units_and_missing_target_are_explicit() {
        for (currency, amount, expected) in [
            ("KWD", "0.123", "0.123"),
            ("JPY", "1.6", "2"),
            ("AED", "100", "100.00"),
        ] {
            let conn = planning_db();
            conn.execute("DELETE FROM loans", []).unwrap();
            create_cashflow_item(
                &conn,
                InsertCashflowItem {
                    name: "Native".into(),
                    amount: amount.into(),
                    currency: Some(currency.into()),
                    frequency: "monthly".into(),
                    item_type: "expense".into(),
                    category: "subscriptions".into(),
                    replacement_source_id: None,
                },
            )
            .unwrap();
            let report = get_report_in_currency(&conn, "monthly", currency).unwrap();
            assert_eq!(report.total_expenses, expected);
            let missing = get_report_in_currency(&conn, "monthly", "XZA").unwrap();
            assert_eq!(missing.total_expenses, "0.00");
            assert_eq!(
                missing
                    .personal
                    .expenses
                    .iter()
                    .flat_map(|c| &c.items)
                    .count(),
                1
            );
            assert!(missing
                .quality
                .missing_fx_currencies
                .contains(&"XZA".into()));
        }
    }
    #[test]
    fn target_planning_native_annual_item_normalizes_without_fx() {
        let conn = planning_db();
        conn.execute("DELETE FROM loans", []).unwrap();
        create_cashflow_item(
            &conn,
            InsertCashflowItem {
                name: "Annual".into(),
                amount: "1200".into(),
                currency: Some("EUR".into()),
                frequency: "yearly".into(),
                item_type: "expense".into(),
                category: "subscriptions".into(),
                replacement_source_id: None,
            },
        )
        .unwrap();
        let report = get_report_in_currency(&conn, "monthly", "EUR").unwrap();
        assert_eq!(report.total_expenses, "100.00");
        assert_eq!(report.quality.estimated_fx_count, 0);
    }
    #[test]
    fn replacement_avoids_double_count_retains_inventory_and_restores_on_delete() {
        let conn = planning_db();
        let before = get_report(&conn, "monthly").unwrap();
        assert_eq!(before.total_expenses, "100.00");
        let item = InsertCashflowItem {
            name: "Adjusted loan".into(),
            amount: "1200".into(),
            currency: Some("CZK".into()),
            frequency: "yearly".into(),
            item_type: "expense".into(),
            category: "personalLoans".into(),
            replacement_source_id: Some("loan:active".into()),
        };
        let created = create_cashflow_item(&conn, item.clone()).unwrap();
        assert!(create_cashflow_item(&conn, item.clone()).is_err());
        let mut wrong = item;
        wrong.replacement_source_id = Some("loan:missing".into());
        assert!(create_cashflow_item(&conn, wrong).is_err());
        let report = get_report(&conn, "monthly").unwrap();
        assert_eq!(report.total_expenses, "100.00");
        assert_eq!(report.automatic_sources.len(), 1);
        assert!(report
            .personal
            .expenses
            .iter()
            .flat_map(|c| &c.items)
            .all(|i| i.is_user_defined));
        delete_cashflow_item(&conn, created.id).unwrap();
        assert_eq!(
            get_report(&conn, "monthly").unwrap().total_expenses,
            "100.00"
        );
    }
    #[test]
    fn planned_unknown_fx_is_excluded_and_marked_without_one_to_one_fallback() {
        let conn = planning_db();
        conn.execute("UPDATE loans SET currency='XPM'", []).unwrap();
        let report = get_report(&conn, "monthly").unwrap();
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.quality.missing_fx_count, 1);
        assert_eq!(report.automatic_sources.len(), 1);
        assert_eq!(
            report.automatic_sources[0].fx_quality,
            crate::models::FxQuality::Missing
        );
    }
    fn insert_property(conn: &rusqlite::Connection, kind: &str, costs: serde_json::Value) {
        conn.execute("INSERT INTO real_estate(id,name,type,recurring_costs) VALUES ('property','Property',?1,?2)",rusqlite::params![kind,costs.to_string()]).unwrap();
    }
    #[test]
    fn personal_and_investment_property_foreign_costs_are_estimated() {
        crate::services::currency::update_exchange_rates(HashMap::from([("XPK".into(), 10.0)]));
        for kind in ["personal", "investment"] {
            let conn = planning_db();
            insert_property(
                &conn,
                kind,
                serde_json::json!([{"amount":100,"frequency":"monthly","currency":"XPK"}]),
            );
            let report = get_report(&conn, "monthly").unwrap();
            let source = report
                .automatic_sources
                .iter()
                .find(|s| s.source_id.as_deref() == Some("propertyCosts:property"))
                .unwrap();
            assert_eq!(source.amount, "1000.00");
            assert_eq!(
                source.fx_quality,
                crate::models::FxQuality::Estimated,
                "{kind}"
            );
            assert_eq!(report.quality.estimated_fx_count, 1);
        }
    }
    #[test]
    fn property_mixed_fx_quality_and_inventory_are_independent_of_component_order() {
        for kind in ["personal", "investment"] {
            for czk_amount in [0, 5] {
                for missing_first in [false, true] {
                    let conn = planning_db();
                    let missing =
                        serde_json::json!({"amount":100,"frequency":"monthly","currency":"XPU"});
                    let known = serde_json::json!({"amount":czk_amount,"frequency":"monthly","currency":"CZK"});
                    let costs = if missing_first {
                        serde_json::json!([missing, known])
                    } else {
                        serde_json::json!([known, missing])
                    };
                    insert_property(&conn, kind, costs);
                    let report = get_report(&conn, "monthly").unwrap();
                    let source=report.automatic_sources.iter().find(|s|s.source_id.as_deref()==Some("propertyCosts:property")).unwrap_or_else(||panic!("missing inventory: {kind},CZK{czk_amount},missing_first{missing_first}"));
                    assert_eq!(source.amount, format!("{czk_amount}.00"));
                    assert_eq!(
                        source.fx_quality,
                        crate::models::FxQuality::Missing,
                        "{kind},missing_first{missing_first}"
                    );
                    assert_eq!(report.quality.missing_fx_count, 1);
                    assert_eq!(report.quality.missing_fx_currencies, vec!["XPU"]);
                }
            }
        }
    }
    #[test]
    fn absent_dividend_values_are_optional_but_query_and_type_failures_propagate() {
        let conn = planning_db();
        conn.execute_batch("CREATE TABLE dividend_overrides(ticker TEXT,yearly_dividend_sum TEXT,currency TEXT); CREATE TABLE dividend_data(ticker TEXT,yearly_dividend_sum TEXT,currency TEXT); INSERT INTO stock_investments VALUES ('TEST','Test stock','2');").unwrap();
        let report = get_report(&conn, "monthly").unwrap();
        assert_eq!(report.investments.total_income, "0.00");
        conn.execute("INSERT INTO dividend_data VALUES ('TEST','12','CZK')", [])
            .unwrap();
        assert_eq!(
            get_report(&conn, "monthly")
                .unwrap()
                .investments
                .total_income,
            "2.00"
        );
        conn.execute("UPDATE dividend_data SET yearly_dividend_sum=x'FF'", [])
            .unwrap();
        assert!(
            matches!(get_report(&conn, "monthly"), Err(AppError::Database(_))),
            "type failure must not become absent dividend"
        );
        conn.execute("DROP TABLE dividend_data", []).unwrap();
        assert!(
            matches!(get_report(&conn, "monthly"), Err(AppError::Database(_))),
            "schema failure must not become absent dividend"
        );
    }
    #[test]
    fn automatic_loan_sources_omit_repaid_and_expired_loans() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE loans(id TEXT,principal TEXT,interest_rate TEXT,monthly_payment TEXT,start_date INTEGER,end_date INTEGER,balance_anchor_amount TEXT,balance_anchor_date INTEGER);
   INSERT INTO loans VALUES ('paid','1000','0','100',1704067200,NULL,NULL,NULL),('expired','100000','0','100',1704067200,1706745600,NULL,NULL),('active','100000','0','100',1704067200,NULL,NULL,NULL);").unwrap();
        let active = active_loan_ids(&conn, 1735689600).unwrap();
        assert_eq!(active, HashSet::from(["active".into()]));
    }
}
