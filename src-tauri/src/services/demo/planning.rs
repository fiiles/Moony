//! Budgets with one past version, spending plans with tagged payments, planned cashflow items
//! and the profile flags that belong to a finished first run.

use super::clock;
use super::text::{t, T};
use super::{DemoOptions, DemoSummary};
use crate::error::Result;
use crate::models::budgeting::InsertBudgetGoal;
use crate::models::cashflow::InsertCashflowItem;
use crate::models::spending_plans::{InsertSpendingPlan, SpendingPlanAssignment};
use crate::services::{
    budgeting, cashflow_planning, categorization::packs, onboarding, spending_plans,
};
use chrono::{Datelike, NaiveDate};
use rusqlite::{params, Connection};

/// Monthly limits (category, EUR) from the month of M36 and, raised, from the month of M12.
const BUDGETS_OLD: [(&str, &str); 5] = [
    ("cat_groceries", "360"),
    ("cat_dining", "180"),
    ("cat_shopping", "220"),
    ("cat_entertainment", "100"),
    ("cat_transport", "160"),
];
const BUDGETS_NEW: [(&str, &str); 5] = [
    ("cat_groceries", "420"),
    ("cat_dining", "200"),
    ("cat_shopping", "250"),
    ("cat_entertainment", "120"),
    ("cat_transport", "180"),
];

/// Months back the old and the raised budgets start.
const BUDGET_OLD_M: i32 = 36;
const BUDGET_NEW_M: i32 = 12;

/// Plan colors, all from the color picker's palette.
const COLOR_HOLIDAY: &str = "#06B6D4";
const COLOR_CHRISTMAS: &str = "#EF4444";
const COLOR_CAR: &str = "#64748B";

/// Share of a plan its tagged payments use: this year's holiday, the car, last year's holiday.
const HOLIDAY_USED: f64 = 0.87;
const CAR_USED: f64 = 0.80;
const LAST_HOLIDAY_USED: f64 = 0.97;

fn plan(name: &str, year: i32, amount: &str, icon: &str, color: &str) -> InsertSpendingPlan {
    InsertSpendingPlan {
        name: name.to_string(),
        year,
        amount: amount.to_string(),
        currency: "EUR".to_string(),
        icon: icon.to_string(),
        color: color.to_string(),
        note: None,
    }
}

/// Ids of the debits in a calendar year that match a category or description.
fn payment_ids(
    conn: &Connection,
    year: i32,
    category: Option<&str>,
    descriptions: &[&str],
) -> Result<Vec<String>> {
    let from = clock::day_of(NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or_default());
    let to = clock::day_of(NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or_default());
    let mut stmt = conn.prepare(
        "SELECT id FROM bank_transactions
         WHERE tx_type = 'debit' AND booking_date BETWEEN ?1 AND ?2
           AND (?3 IS NULL OR category_id = ?3)
         ORDER BY booking_date, id",
    )?;
    let ids = stmt
        .query_map(params![from, to, category], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if descriptions.is_empty() {
        return Ok(ids);
    }
    let mut out = Vec::new();
    let mut stmt = conn.prepare("SELECT description FROM bank_transactions WHERE id = ?1")?;
    for id in ids {
        let desc: Option<String> = stmt.query_row([&id], |r| r.get(0))?;
        if desc.is_some_and(|d| descriptions.contains(&d.as_str())) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Write budgets, plans, cashflow items and the first-run flags.
pub fn seed(conn: &Connection, opts: &DemoOptions, summary: &mut DemoSummary) -> Result<()> {
    let lang = opts.language.as_str();
    let now = opts.now;

    // Budgets, chronologically so the later version closes the earlier one
    for (month, limits) in [(BUDGET_OLD_M, BUDGETS_OLD), (BUDGET_NEW_M, BUDGETS_NEW)] {
        for (category, amount) in limits {
            budgeting::upsert_goal_at(
                conn,
                InsertBudgetGoal {
                    category_id: category.to_string(),
                    timeframe: "monthly".to_string(),
                    amount: amount.to_string(),
                    currency: Some("EUR".to_string()),
                    effective_month: Some(clock::months_ago(now, month, 1)),
                    effective_until: None,
                },
                now,
            )?;
        }
    }
    summary.counts.insert(
        "budget_goal_versions",
        BUDGETS_OLD.len() + BUDGETS_NEW.len(),
    );

    // Spending plans: this year's three, last year's closed holiday
    let year = clock::date_of(clock::today(now)).year();
    let holiday = t(lang, T::PlanHoliday);
    // Placeholder amounts: each plan is sized from what was tagged to it (below)
    let this_holiday =
        spending_plans::create_plan(conn, &plan(holiday, year, "100", "plane", COLOR_HOLIDAY))?;
    let christmas = spending_plans::create_plan(
        conn,
        &plan(
            t(lang, T::PlanChristmas),
            year,
            "600",
            "gift",
            COLOR_CHRISTMAS,
        ),
    )?;
    let car = spending_plans::create_plan(
        conn,
        &plan(t(lang, T::PlanCar), year, "100", "car", COLOR_CAR),
    )?;
    let last_holiday = spending_plans::create_plan(
        conn,
        &plan(holiday, year - 1, "100", "plane", COLOR_HOLIDAY),
    )?;

    let tag = |ids: Vec<String>, plan_id: &str| -> Vec<SpendingPlanAssignment> {
        ids.into_iter()
            .map(|transaction_id| SpendingPlanAssignment {
                transaction_id,
                plan_id: Some(plan_id.to_string()),
            })
            .collect()
    };
    let mut assignments = Vec::new();
    assignments.extend(tag(
        payment_ids(conn, year, Some("cat_travel"), &[])?,
        &this_holiday.id,
    ));
    assignments.extend(tag(
        payment_ids(conn, year - 1, Some("cat_travel"), &[])?,
        &last_holiday.id,
    ));
    // gifts of a December that has not happened yet do not exist; last December stays untagged
    assignments.extend(tag(
        payment_ids(conn, year, Some("cat_shopping"), &[t(lang, T::Gifts)])?,
        &christmas.id,
    ));
    assignments.extend(tag(
        payment_ids(
            conn,
            year,
            None,
            &[
                t(lang, T::Fuel),
                t(lang, T::CarService),
                t(lang, T::PremCar),
            ],
        )?,
        &car.id,
    ));
    summary
        .counts
        .insert("spending_plan_tags", assignments.len());
    spending_plans::set_transaction_plans(conn, &assignments, false)?;

    // Size the plans from their tagged spend: this year's holiday about 87 % used, the car
    // about 80 %, last year's holiday about 97 % (Christmas keeps its 600)
    let spent: std::collections::HashMap<String, f64> = spending_plans::plan_index(conn)?
        .into_iter()
        .map(|s| {
            let value = s.spent.and_then(|v| v.parse().ok()).unwrap_or(0.0);
            (s.plan.id, value)
        })
        .collect();
    for (planned, used) in [
        (&this_holiday, HOLIDAY_USED),
        (&car, CAR_USED),
        (&last_holiday, LAST_HOLIDAY_USED),
    ] {
        let spend = spent.get(&planned.id).copied().unwrap_or(0.0);
        let amount = ((spend / used / 100.0).round() * 100.0).max(100.0);
        spending_plans::update_plan(
            conn,
            &planned.id,
            &InsertSpendingPlan {
                name: planned.name.clone(),
                year: planned.year,
                amount: format!("{amount:.0}"),
                currency: planned.currency.clone(),
                icon: planned.icon.clone(),
                color: planned.color.clone(),
                note: planned.note.clone(),
            },
        )?;
    }

    spending_plans::set_plan_closed(conn, &last_holiday.id, true)?;
    let closed_at = clock::day_of(NaiveDate::from_ymd_opt(year - 1, 12, 31).unwrap_or_default());
    conn.execute(
        "UPDATE spending_plans SET closed_at = ?2 WHERE id = ?1",
        params![last_holiday.id, closed_at],
    )?;
    summary.counts.insert("spending_plans", 4);

    // Planned cashflow items (loans, insurance, rent and so on come from their sources)
    for (name, amount, category, item_type) in [
        (t(lang, T::Salary), "4100", "personalIncome", "income"),
        (t(lang, T::Groceries), "420", "consumptionCosts", "expense"),
        (
            t(lang, T::CfSubscriptions),
            "27.97",
            "subscriptions",
            "expense",
        ),
    ] {
        cashflow_planning::create_cashflow_item(
            conn,
            InsertCashflowItem {
                name: name.to_string(),
                amount: amount.to_string(),
                currency: Some("EUR".to_string()),
                frequency: "monthly".to_string(),
                item_type: item_type.to_string(),
                replacement_source_id: None,
                category: category.to_string(),
            },
        )?;
    }
    summary.counts.insert("cashflow_items", 3);

    // A finished first run: global rule pack only, checklist done and dismissed
    packs::write_enabled(conn, &["global".to_string()])?;
    onboarding::set_flag(conn, onboarding::FLAG_COMPLETED_AT, Some(&now.to_string()))?;
    onboarding::set_flag(conn, onboarding::FLAG_CHECKLIST_DISMISSED, Some("1"))?;
    Ok(())
}
