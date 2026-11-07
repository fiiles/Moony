//! Planning operations: spending plans, budgets, cash-flow items and plan inclusion, milestones,
//! projection settings, and the allowlisted `app_config` keys written through their services.

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{
    by_content, currency, ids, money, month, no_target, now, one_of, pick, pick2, run, OpFn, Res,
};
use crate::error::Result;
use crate::models::budgeting::{BudgetGoalInterval, InsertBudgetGoal};
use crate::models::cashflow::{InsertCashflowItem, SetCashflowPlanInclusion, CASHFLOW_CATEGORIES};
use crate::models::milestones::{
    validate_milestone_key, validate_milestone_kinds, validate_milestone_state, MILESTONE_KINDS,
};
use crate::models::projection::ProjectionSettings;
use crate::models::spending_plans::{InsertSpendingPlan, SpendingPlanAssignment};
use crate::services::categorization::packs;
use crate::services::stock_import::formats;
use crate::services::stock_import::simulate::test_db::config;
use crate::services::{budgeting, cashflow_planning, milestones, projection, spending_plans};

pub(super) const OPS: &[(&str, OpFn)] = &[
    ("spending_plans::create_plan", create_plan),
    ("spending_plans::update_plan", update_plan),
    ("spending_plans::delete_plan", delete_plan),
    ("spending_plans::set_transaction_plans", assign_plans),
    ("budgeting::upsert_goal_at", upsert_goal),
    ("budgeting::delete_goal_intervals", delete_intervals),
    ("budgeting::delete_budget", delete_budget),
    ("cashflow_planning::create_cashflow_item", create_item),
    ("cashflow_planning::update_cashflow_item", update_item),
    ("cashflow_planning::delete_cashflow_item", delete_item),
    ("cashflow_planning::set_plan_inclusion", plan_inclusion),
    ("milestones::set_milestone_state", milestone_state),
    ("milestones::clear_milestone_state", clear_milestone),
    ("milestones::set_muted_kinds", muted_kinds),
    ("projection::save_settings", projection_settings),
    ("stock_import::formats::save_format", save_format),
    ("categorization::packs::write_enabled", packs_enabled),
    ("categorization::packs::write_rule_disabled", pack_rule),
];

fn plans() -> String {
    by_content(
        "spending_plans",
        "t.id",
        "",
        "t.year, t.name, t.amount, t.currency",
    )
}

fn items() -> String {
    by_content(
        "cashflow_items",
        "t.id",
        "",
        "t.name, t.category, t.amount, t.frequency",
    )
}

fn categories() -> String {
    by_content(
        "transaction_categories",
        "t.id",
        "",
        "t.name, t.icon, t.color",
    )
}
const TIMEFRAMES: [&str; 3] = ["monthly", "quarterly", "yearly"];
const INCOME: [&str; 6] = [
    "personalIncome",
    "interestIncome",
    "stockDividends",
    "bondsInterest",
    "rentalIncome",
    "otherInvestmentIncome",
];

fn plan_data(rng: &mut StdRng) -> InsertSpendingPlan {
    InsertSpendingPlan {
        name: one_of(rng, &["Holiday", "Car", "Gifts"]).to_string(),
        year: one_of(rng, &[2025, 2026]),
        amount: money(rng),
        currency: currency(rng).to_string(),
        icon: "plane".into(),
        color: "#336699".into(),
        note: rng.random_bool(0.3).then(|| "note".to_string()),
    }
}

fn create_plan(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = plan_data(rng);
    run(format!("{:?} {}", data.name, data.year), || {
        data.validate()?;
        spending_plans::create_plan(conn, &data).map(drop)
    })
}

fn update_plan(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &plans())? else {
        return no_target();
    };
    let data = plan_data(rng);
    run(format!("{id} -> {:?} {}", data.name, data.year), || {
        data.validate()?;
        spending_plans::update_plan(conn, &id, &data).map(drop)
    })
}

fn delete_plan(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &plans())? else {
        return no_target();
    };
    run(id.clone(), || spending_plans::delete_plan(conn, &id))
}

fn assign_plans(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let transactions = by_content(
        "bank_transactions",
        "t.id",
        "",
        "t.booking_date, t.amount, t.tx_type, t.description",
    );
    let Some(transaction) = pick(conn, rng, &transactions)? else {
        return no_target();
    };
    let plan = if rng.random_bool(0.8) {
        pick(conn, rng, &plans())?
    } else {
        None
    };
    let assignment = SpendingPlanAssignment {
        transaction_id: transaction,
        plan_id: plan,
    };
    run(format!("{assignment:?}"), || {
        spending_plans::set_transaction_plans(conn, std::slice::from_ref(&assignment), false)
    })
}

fn upsert_goal(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(category) = pick(conn, rng, &categories())? else {
        return no_target();
    };
    let from = month(rng);
    let data = InsertBudgetGoal {
        category_id: category,
        timeframe: one_of(rng, &TIMEFRAMES).to_string(),
        amount: money(rng),
        currency: Some(currency(rng).to_string()),
        effective_month: Some(from),
        effective_until: rng
            .random_bool(0.3)
            .then(|| month(rng))
            .filter(|until| *until >= from),
    };
    run(
        format!(
            "{} {} {} from {from} until {:?}",
            data.category_id, data.timeframe, data.amount, data.effective_until
        ),
        || {
            data.validate()?;
            budgeting::upsert_goal_at(conn, data.clone(), now()).map(drop)
        },
    )
}

fn delete_intervals(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let versions = ids(
        conn,
        "SELECT category_id || '|' || timeframe || '|' || effective_month \
         FROM budget_goal_versions ORDER BY category_id, timeframe, effective_month",
    )?;
    if versions.is_empty() {
        return no_target();
    }
    let chosen = &versions[rng.random_range(0..versions.len())];
    let parts: Vec<&str> = chosen.split('|').collect();
    let interval = BudgetGoalInterval {
        category_id: parts[0].to_string(),
        timeframe: parts[1].to_string(),
        effective_month: parts[2].parse().unwrap_or_default(),
    };
    run(chosen.clone(), || {
        interval.validate()?;
        budgeting::delete_goal_intervals(conn, std::slice::from_ref(&interval))
    })
}

fn delete_budget(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((category, timeframe)) = pick2(
        conn,
        rng,
        "SELECT DISTINCT category_id, timeframe FROM budget_goal_versions ORDER BY 1, 2",
    )?
    else {
        return no_target();
    };
    run(format!("{category} {timeframe}"), || {
        budgeting::delete_budget(conn, &category, &timeframe)
    })
}

/// `(source, item type, category)` of an automatic source, or its content order key.
type Source = (String, String, String);

/// A replacement link to one of the report's automatic sources (a loan's payment, an account's
/// interest), as the planning screen offers them: `(source, item type, category)`, in content
/// order (category, name, amount).
fn source(conn: &Connection, rng: &mut StdRng) -> Result<Option<Source>> {
    let report = cashflow_planning::get_report_in_currency(conn, "monthly", "CZK")?;
    let mut sources: Vec<(Source, Source)> = report
        .automatic_sources
        .into_iter()
        .filter_map(|i| {
            let content = (i.category.clone(), i.name, i.original_amount);
            i.source_id
                .map(|id| (content, (id, i.item_type, i.category)))
        })
        .collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((!sources.is_empty()).then(|| sources[rng.random_range(0..sources.len())].1.clone()))
}

fn item_data(conn: &Connection, rng: &mut StdRng) -> Result<InsertCashflowItem> {
    let category = one_of(rng, &CASHFLOW_CATEGORIES).to_string();
    let item_type = if INCOME.contains(&category.as_str()) {
        "income"
    } else {
        "expense"
    };
    let mut data = InsertCashflowItem {
        name: one_of(rng, &["Salary", "Netflix", "Rent"]).to_string(),
        amount: money(rng),
        currency: Some(currency(rng).to_string()),
        frequency: one_of(rng, &TIMEFRAMES).to_string(),
        item_type: item_type.to_string(),
        replacement_source_id: None,
        category,
    };
    if rng.random_bool(0.6) {
        if let Some((source, item_type, category)) = source(conn, rng)? {
            data.replacement_source_id = Some(source);
            data.item_type = item_type;
            data.category = category;
        }
    }
    Ok(data)
}

fn create_item(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = item_data(conn, rng)?;
    run(
        format!(
            "{:?} {} {:?}",
            data.name, data.category, data.replacement_source_id
        ),
        || {
            data.validate()?;
            cashflow_planning::create_cashflow_item(conn, data.clone()).map(drop)
        },
    )
}

fn update_item(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &items())? else {
        return no_target();
    };
    let data = item_data(conn, rng)?;
    run(
        format!("{id} -> {:?} {:?}", data.name, data.replacement_source_id),
        || {
            data.validate()?;
            cashflow_planning::update_cashflow_item(conn, id.clone(), data.clone()).map(drop)
        },
    )
}

fn delete_item(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &items())? else {
        return no_target();
    };
    run(id.clone(), || {
        cashflow_planning::delete_cashflow_item(conn, id.clone())
    })
}

fn plan_inclusion(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let key = if rng.random_bool(0.5) {
        format!("category:{}", one_of(rng, &CASHFLOW_CATEGORIES))
    } else {
        let item = pick(conn, rng, &items())?.map(|id| format!("item:{id}"));
        let key = match item {
            Some(key) => Some(key),
            None => source(conn, rng)?.map(|(source, _, _)| source),
        };
        match key {
            Some(key) => key,
            None => return no_target(),
        }
    };
    let data = SetCashflowPlanInclusion {
        key,
        included: rng.random_bool(0.5),
    };
    run(format!("{} {}", data.key, data.included), || {
        data.validate()?;
        cashflow_planning::set_plan_inclusion(conn, data.clone())
    })
}

const MILESTONE_KEYS: [&str; 4] = [
    "backup_stale",
    "balances_stale",
    "loan_payoff:a",
    "bond_maturity:b",
];

fn milestone_state(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let key = one_of(rng, &MILESTONE_KEYS);
    let state = one_of(rng, &["done", "snoozed"]);
    run(format!("{key} {state}"), || {
        validate_milestone_state(key, state)?;
        milestones::set_milestone_state(conn, key, state, now() - now() % super::DAY)
    })
}

fn clear_milestone(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let key = one_of(rng, &MILESTONE_KEYS);
    run(key.to_string(), || {
        validate_milestone_key(key)?;
        milestones::clear_milestone_state(conn, key)
    })
}

fn muted_kinds(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let kinds: Vec<String> = MILESTONE_KINDS
        .iter()
        .take(4)
        .filter(|_| rng.random_bool(0.4))
        .map(|k| k.to_string())
        .collect();
    run(format!("{kinds:?}"), || {
        validate_milestone_kinds(&kinds)?;
        milestones::set_muted_kinds(conn, &kinds).map(drop)
    })
}

fn projection_settings(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let settings = vec![ProjectionSettings {
        id: String::new(),
        asset_type: one_of(rng, &["stocks", "bonds", "savings"]).to_string(),
        yearly_growth_rate: format!("{}.0", rng.random_range(1..9)),
        monthly_contribution: money(rng),
        contribution_currency: currency(rng).to_string(),
        enabled: rng.random_bool(0.7),
        created_at: 0,
        updated_at: 0,
    }];
    run(
        format!(
            "{} {}",
            settings[0].asset_type, settings[0].yearly_growth_rate
        ),
        || {
            for setting in &settings {
                setting.validate()?;
            }
            projection::save_settings(conn, settings.clone())
        },
    )
}

fn save_format(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let name = one_of(rng, &["Broker A", "Broker B"]);
    let headers: Vec<String> = match rng.random_range(0..2) {
        0 => ["date", "symbol", "qty", "price", "currency"],
        _ => ["Date", "Ticker", "Quantity", "Price", "Currency"],
    }
    .iter()
    .map(|h| h.to_string())
    .collect();
    let mut cfg = config("custom");
    cfg.decimal_separator = one_of(rng, &[".", ","]).to_string();
    run(format!("{name:?} {headers:?}"), || {
        formats::save_format(conn, name, &headers, &cfg).map(drop)
    })
}

fn packs_enabled(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let ids: Vec<String> = packs::available_packs()
        .iter()
        .filter(|_| rng.random_bool(0.6))
        .map(|p| p.pack_id.clone())
        .collect();
    run(format!("{ids:?}"), || packs::write_enabled(conn, &ids))
}

fn pack_rule(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let rules: Vec<String> = packs::available_packs()
        .iter()
        .flat_map(|p| p.rules.iter().take(3).map(|r| r.id.clone()))
        .collect();
    if rules.is_empty() {
        return no_target();
    }
    let rule = rules[rng.random_range(0..rules.len())].clone();
    let disabled = rng.random_bool(0.6);
    run(format!("{rule} disabled {disabled}"), || {
        packs::write_rule_disabled(conn, &rule, disabled)
    })
}
