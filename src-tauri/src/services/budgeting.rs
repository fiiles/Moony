//! Budgeting service - aggregates transactions by category for spending analysis
//!
//! This service provides:
//! - Income/expense aggregation by category across all bank accounts
//! - Budget goal CRUD operations
//! - Transaction retrieval for category drill-down

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::budgeting::{
    BudgetGoal, BudgetingReport, BudgetingTransaction, CategorySpendingSummary, InsertBudgetGoal,
};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use uuid::Uuid;

// ========================== Budget Goals CRUD ==========================

/// Get all budget goals
pub fn get_all_goals(conn: &Connection) -> Result<Vec<BudgetGoal>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, timeframe, amount, currency, created_at, updated_at
         FROM budget_goals
         ORDER BY category_id, timeframe",
    )?;

    let goals = stmt
        .query_map([], |row| {
            Ok(BudgetGoal {
                id: row.get(0)?,
                category_id: row.get(1)?,
                timeframe: row.get(2)?,
                amount: row.get(3)?,
                currency: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(goals)
}

/// Get budget goals for a specific timeframe
pub fn get_goals_by_timeframe(conn: &Connection, timeframe: &str) -> Result<Vec<BudgetGoal>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, timeframe, amount, currency, created_at, updated_at
         FROM budget_goals
         WHERE timeframe = ?1
         ORDER BY category_id",
    )?;

    let goals = stmt
        .query_map([timeframe], |row| {
            Ok(BudgetGoal {
                id: row.get(0)?,
                category_id: row.get(1)?,
                timeframe: row.get(2)?,
                amount: row.get(3)?,
                currency: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(goals)
}

/// Upsert (create or update) a budget goal
pub fn upsert_goal(conn: &Connection, data: InsertBudgetGoal) -> Result<BudgetGoal> {
    data.validate()?;

    let currency = data.currency.unwrap_or_else(|| "CZK".to_string());
    let now = chrono::Utc::now().timestamp();

    // Check if goal already exists for this category + timeframe
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM budget_goals WHERE category_id = ?1 AND timeframe = ?2",
            params![data.category_id, data.timeframe],
            |row| row.get(0),
        )
        .ok();

    let id = if let Some(existing_id) = existing {
        // Update existing goal
        conn.execute(
            "UPDATE budget_goals SET amount = ?1, currency = ?2, updated_at = ?3 WHERE id = ?4",
            params![data.amount, currency, now, existing_id],
        )?;
        existing_id
    } else {
        // Create new goal
        let new_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO budget_goals (id, category_id, timeframe, amount, currency, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![new_id, data.category_id, data.timeframe, data.amount, currency, now],
        )?;
        new_id
    };

    // Retrieve and return the goal
    conn.query_row(
        "SELECT id, category_id, timeframe, amount, currency, created_at, updated_at
         FROM budget_goals WHERE id = ?1",
        [&id],
        |row| {
            Ok(BudgetGoal {
                id: row.get(0)?,
                category_id: row.get(1)?,
                timeframe: row.get(2)?,
                amount: row.get(3)?,
                currency: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .map_err(|e| AppError::Database(e.to_string()))
}

/// Delete a budget goal
pub fn delete_goal(conn: &Connection, id: &str) -> Result<()> {
    let rows = conn.execute("DELETE FROM budget_goals WHERE id = ?1", [id])?;
    if rows == 0 {
        return Err(AppError::NotFound(format!("Budget goal {} not found", id)));
    }
    Ok(())
}

// ========================== Budgeting Report ==========================

/// Get the full budgeting report for a time period
pub fn get_report(
    conn: &Connection,
    start_date: i64,
    end_date: i64,
    timeframe: &str,
) -> Result<BudgetingReport> {
    // Always get monthly budget goals (budgets are always stored as monthly)
    let goals = get_goals_by_timeframe(conn, "monthly")?;

    // Calculate multiplier for scaling monthly budgets to the requested timeframe
    let budget_multiplier: f64 = match timeframe {
        "quarterly" => 3.0,
        "yearly" => 12.0,
        _ => 1.0, // monthly
    };

    let goals_map: HashMap<String, BudgetGoal> = goals
        .into_iter()
        .map(|g| (g.category_id.clone(), g))
        .collect();

    // Aggregate transactions by category, tx_type, and currency (and, for foreign
    // currencies, by booking day: each day converts at its own rate).
    // Exclude internal transfers at the SQL level for efficiency
    let mut stmt = conn.prepare(
        "SELECT
            COALESCE(bt.category_id, 'uncategorized') as cat_id,
            COALESCE(tc.name, 'Uncategorized') as cat_name,
            tc.icon as cat_icon,
            tc.color as cat_color,
            bt.tx_type,
            bt.currency,
            SUM(CAST(bt.amount AS REAL)) as total_amount,
            COUNT(*) as tx_count,
            CASE WHEN UPPER(bt.currency) = 'CZK' THEN 0
                 ELSE (bt.booking_date / 86400) * 86400 END as fx_day
         FROM bank_transactions bt
         LEFT JOIN transaction_categories tc ON bt.category_id = tc.id
         WHERE bt.booking_date >= ?1 AND bt.booking_date <= ?2
           AND COALESCE(bt.category_id, '') != 'cat_internal_transfers'
         GROUP BY cat_id, bt.tx_type, bt.currency, fx_day
         ORDER BY cat_name",
    )?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        f64,
        i32,
        i64,
    )> = stmt
        .query_map(params![start_date, end_date], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Separate income (cat_income credits, plus uncategorized credits below) and
    // expenses (all other categories, net amount, plus uncategorized debits)
    let mut income_map: HashMap<String, CategorySpendingSummary> = HashMap::new();
    let mut expense_map: HashMap<String, CategorySpendingSummary> = HashMap::new();
    let mut total_income = 0.0_f64;
    let mut total_expenses = 0.0_f64;
    let mut uncategorized_income = 0.0_f64;
    let mut uncategorized_expenses = 0.0_f64;
    let mut uncategorized_count = 0_i32;

    // Rates of each booking day, looked up once per day (falls back to the
    // nearest earlier day, then to the latest rate when a day has no history).
    let mut rates_by_day: HashMap<i64, HashMap<String, f64>> = HashMap::new();

    for (cat_id, cat_name, cat_icon, cat_color, tx_type, currency, amount, count, fx_day) in rows {
        let is_credit = tx_type.to_lowercase() == "credit";
        // Convert amount to CZK for consistent totals, at the booking day's
        // rate (ADR 0001: converting old amounts at today's rate is wrong)
        let abs_amount_czk = if currency.eq_ignore_ascii_case("CZK") {
            amount.abs()
        } else {
            let rates = rates_by_day
                .entry(fx_day)
                .or_insert_with(|| crate::services::currency::get_rates_for_date(conn, fx_day));
            crate::services::currency::convert_to_czk_with_rates(amount.abs(), &currency, rates)
        };

        // Handle uncategorized transactions
        if cat_id == "uncategorized" {
            if is_credit {
                uncategorized_income += abs_amount_czk;
            } else {
                uncategorized_expenses += abs_amount_czk;
            }
            uncategorized_count += count;
            continue;
        }

        // Income category (cat_income) - credits go to income totals
        if cat_id == "cat_income" {
            if is_credit {
                total_income += abs_amount_czk;
                let entry = income_map.entry(cat_id.clone()).or_insert_with(|| {
                    let goal = goals_map.get(&cat_id).cloned();
                    CategorySpendingSummary {
                        category_id: cat_id.clone(),
                        category_name: cat_name.clone(),
                        category_icon: cat_icon.clone(),
                        category_color: cat_color.clone(),
                        total_amount: "0".to_string(),
                        transaction_count: 0,
                        budget_goal: goal,
                        budget_percentage: None,
                    }
                });
                let current: f64 = entry.total_amount.parse().unwrap_or(0.0);
                entry.total_amount = format!("{:.2}", current + abs_amount_czk);
                entry.transaction_count += count;
            }
            // Ignore debits in income category (unusual case)
            continue;
        }

        // All other expense categories: calculate NET amount
        // Debits add to expense, credits (refunds) reduce expense
        let entry = expense_map.entry(cat_id.clone()).or_insert_with(|| {
            let goal = goals_map.get(&cat_id).cloned();
            CategorySpendingSummary {
                category_id: cat_id.clone(),
                category_name: cat_name.clone(),
                category_icon: cat_icon.clone(),
                category_color: cat_color.clone(),
                total_amount: "0".to_string(),
                transaction_count: 0,
                budget_goal: goal,
                budget_percentage: None,
            }
        });

        let current: f64 = entry.total_amount.parse().unwrap_or(0.0);
        // Credits (refunds) reduce the net expense amount
        let delta = if is_credit {
            -abs_amount_czk // Refund reduces expense
        } else {
            abs_amount_czk // Debit adds to expense
        };
        entry.total_amount = format!("{:.2}", current + delta);
        entry.transaction_count += count;
    }

    // Calculate total expenses from expense_map (only positive net amounts)
    for summary in expense_map.values() {
        let amount: f64 = summary.total_amount.parse().unwrap_or(0.0);
        if amount > 0.0 {
            total_expenses += amount;
        }
    }
    // Add uncategorized expenses to total
    total_expenses += uncategorized_expenses;
    // ...and, symmetrically, uncategorized credits to income:
    // a credit nobody has categorized yet is still money that came in. It stays
    // reported separately in `uncategorized_income` so the UI can label it.
    total_income += uncategorized_income;

    // Calculate budget percentages (scale monthly budget by timeframe multiplier)
    for summary in expense_map.values_mut() {
        if let Some(ref goal) = summary.budget_goal {
            let spent: f64 = summary.total_amount.parse::<f64>().unwrap_or(0.0).max(0.0);
            // Goals are stored in the currency the user typed them in; spent
            // totals are CZK, so convert the goal before comparing.
            let monthly_budget_native: f64 = goal.amount.parse().unwrap_or(0.0);
            let monthly_budget =
                crate::services::currency::convert_to_czk(monthly_budget_native, &goal.currency);
            let scaled_budget = monthly_budget * budget_multiplier;
            if scaled_budget > 0.0 {
                summary.budget_percentage = Some((spent / scaled_budget) * 100.0);
            }
        }
    }

    // Convert maps to sorted vectors, filtering out categories with zero or negative net
    let mut income_categories: Vec<CategorySpendingSummary> = income_map.into_values().collect();
    let mut expense_categories: Vec<CategorySpendingSummary> = expense_map
        .into_values()
        .filter(|c| {
            let amount: f64 = c.total_amount.parse().unwrap_or(0.0);
            amount > 0.0 // Only show categories with positive net expense
        })
        .collect();

    // Sort by total amount descending
    income_categories.sort_by(|a, b| {
        let a_val: f64 = a.total_amount.parse().unwrap_or(0.0);
        let b_val: f64 = b.total_amount.parse().unwrap_or(0.0);
        b_val
            .partial_cmp(&a_val)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    expense_categories.sort_by(|a, b| {
        let a_val: f64 = a.total_amount.parse().unwrap_or(0.0);
        let b_val: f64 = b.total_amount.parse().unwrap_or(0.0);
        b_val
            .partial_cmp(&a_val)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(BudgetingReport {
        period_start: start_date,
        period_end: end_date,
        timeframe: timeframe.to_string(),
        total_income: format!("{:.2}", total_income),
        total_expenses: format!("{:.2}", total_expenses),
        net_balance: format!("{:.2}", total_income - total_expenses),
        income_categories,
        expense_categories,
        uncategorized_income: format!("{:.2}", uncategorized_income),
        uncategorized_expenses: format!("{:.2}", uncategorized_expenses),
        uncategorized_transaction_count: uncategorized_count,
    })
}

/// Get transactions for a specific category in a time period
pub fn get_category_transactions(
    conn: &Connection,
    category_id: &str,
    start_date: i64,
    end_date: i64,
) -> Result<Vec<BudgetingTransaction>> {
    let category_filter = if category_id == "uncategorized" {
        "bt.category_id IS NULL"
    } else {
        "bt.category_id = ?3"
    };

    let query = format!(
        "SELECT
            bt.id,
            bt.booking_date,
            bt.amount,
            bt.currency,
            bt.description,
            bt.counterparty_name,
            bt.counterparty_iban,
            bt.category_id,
            bt.bank_account_id,
            ba.name as bank_account_name,
            bt.tx_type
         FROM bank_transactions bt
         JOIN bank_accounts ba ON bt.bank_account_id = ba.id
         WHERE bt.booking_date >= ?1 AND bt.booking_date <= ?2 AND {}
         ORDER BY bt.booking_date DESC",
        category_filter
    );

    let mut stmt = conn.prepare(&query)?;

    let transactions = if category_id == "uncategorized" {
        stmt.query_map(params![start_date, end_date], |row| {
            Ok(BudgetingTransaction {
                id: row.get(0)?,
                booking_date: row.get(1)?,
                amount: row.get(2)?,
                currency: row.get(3)?,
                description: row.get(4)?,
                counterparty_name: row.get(5)?,
                counterparty_iban: row.get(6)?,
                category_id: row.get(7)?,
                bank_account_id: row.get(8)?,
                bank_account_name: row.get(9)?,
                tx_type: row.get(10)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(params![start_date, end_date, category_id], |row| {
            Ok(BudgetingTransaction {
                id: row.get(0)?,
                booking_date: row.get(1)?,
                amount: row.get(2)?,
                currency: row.get(3)?,
                description: row.get(4)?,
                counterparty_name: row.get(5)?,
                counterparty_iban: row.get(6)?,
                category_id: row.get(7)?,
                bank_account_id: row.get(8)?,
                bank_account_name: row.get(9)?,
                tx_type: row.get(10)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(transactions)
}

// ========================== Async wrappers for Database ==========================

/// Get budgeting report (async wrapper)
pub async fn get_budgeting_report(
    db: &Database,
    start_date: i64,
    end_date: i64,
    timeframe: String,
) -> Result<BudgetingReport> {
    db.with_conn(|conn| get_report(conn, start_date, end_date, &timeframe))
}

/// Get category transactions (async wrapper)
pub async fn get_transactions_for_category(
    db: &Database,
    category_id: String,
    start_date: i64,
    end_date: i64,
) -> Result<Vec<BudgetingTransaction>> {
    db.with_conn(|conn| get_category_transactions(conn, &category_id, start_date, end_date))
}

/// Get all budget goals (async wrapper)
pub async fn get_budget_goals(db: &Database) -> Result<Vec<BudgetGoal>> {
    db.with_conn(get_all_goals)
}

/// Upsert budget goal (async wrapper)
pub async fn upsert_budget_goal(db: &Database, data: InsertBudgetGoal) -> Result<BudgetGoal> {
    db.with_conn(|conn| upsert_goal(conn, data))
}

/// Delete budget goal (async wrapper)
pub async fn delete_budget_goal(db: &Database, id: String) -> Result<()> {
    db.with_conn(|conn| delete_goal(conn, &id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory db");

        // Create minimal schema for testing
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                icon TEXT,
                color TEXT
            );

            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );

            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                booking_date INTEGER NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                description TEXT,
                counterparty_name TEXT,
                category_id TEXT,
                tx_type TEXT NOT NULL
            );

            CREATE TABLE budget_goals (
                id TEXT PRIMARY KEY,
                category_id TEXT NOT NULL,
                timeframe TEXT NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(category_id, timeframe)
            );

            INSERT INTO transaction_categories (id, name, icon, color) VALUES
                ('cat_groceries', 'Groceries', 'shopping-cart', '#4CAF50'),
                ('cat_dining', 'Dining', 'utensils', '#FF9800'),
                ('cat_income', 'Income', 'trending-up', '#8BC34A');

            INSERT INTO bank_accounts (id, name) VALUES
                ('acc1', 'Main Account');
            "#,
        )
        .expect("Failed to create test schema");

        conn
    }

    #[test]
    fn test_budget_goal_crud() {
        let conn = setup_test_db();

        // Create a goal
        let goal = upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "5000".to_string(),
                currency: Some("CZK".to_string()),
            },
        )
        .expect("Failed to create goal");

        assert_eq!(goal.category_id, "cat_groceries");
        assert_eq!(goal.amount, "5000");
        assert_eq!(goal.timeframe, "monthly");

        // Update the goal
        let updated = upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "6000".to_string(),
                currency: Some("CZK".to_string()),
            },
        )
        .expect("Failed to update goal");

        assert_eq!(updated.id, goal.id); // Same ID
        assert_eq!(updated.amount, "6000");

        // Delete the goal
        delete_goal(&conn, &goal.id).expect("Failed to delete goal");

        let goals = get_all_goals(&conn).expect("Failed to get goals");
        assert!(goals.is_empty());
    }

    #[test]
    fn test_get_report_empty() {
        let conn = setup_test_db();

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("Failed to get report");

        assert_eq!(report.total_income, "0.00");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.net_balance, "0.00");
        assert!(report.income_categories.is_empty());
        assert!(report.expense_categories.is_empty());
    }

    #[test]
    fn test_get_report_with_transactions() {
        let conn = setup_test_db();

        // Add some test transactions
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '100.50', 'CZK', 'cat_groceries', 'debit'),
                ('tx2', 'acc1', 1704067200, '50.00', 'CZK', 'cat_groceries', 'debit'),
                ('tx3', 'acc1', 1704067200, '1000.00', 'CZK', 'cat_income', 'credit');
            "#,
        )
        .expect("Failed to insert transactions");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("Failed to get report");

        assert_eq!(report.total_income, "1000.00");
        assert_eq!(report.total_expenses, "150.50");
        assert_eq!(report.net_balance, "849.50");
        assert_eq!(report.income_categories.len(), 1);
        assert_eq!(report.expense_categories.len(), 1);
        assert_eq!(report.expense_categories[0].transaction_count, 2);
    }

    #[test]
    fn test_validate_budget_goal() {
        // Valid goal
        let valid = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "5000".to_string(),
            currency: None,
        };
        assert!(valid.validate().is_ok());

        // Invalid timeframe
        let invalid_timeframe = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "weekly".to_string(),
            amount: "5000".to_string(),
            currency: None,
        };
        assert!(invalid_timeframe.validate().is_err());

        // Invalid amount
        let invalid_amount = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "-100".to_string(),
            currency: None,
        };
        assert!(invalid_amount.validate().is_err());
    }

    #[test]
    fn test_get_all_goals_multiple_categories() {
        let conn = setup_test_db();

        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "3000".to_string(),
                currency: Some("CZK".to_string()),
            },
        )
        .unwrap();

        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_dining".to_string(),
                timeframe: "monthly".to_string(),
                amount: "1500".to_string(),
                currency: Some("CZK".to_string()),
            },
        )
        .unwrap();

        let goals = get_all_goals(&conn).expect("get goals");
        assert_eq!(goals.len(), 2);
    }

    #[test]
    fn test_validate_budget_goal_zero_amount_invalid() {
        let invalid = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "0".to_string(),
            currency: None,
        };
        assert!(invalid.validate().is_err());
    }

    /// Booking dates are stored as UTC midnight of the calendar day (ADR 0008).
    /// The report's inclusive `end` bound must therefore be the last second of
    /// the period's last day in UTC — the local-time boundaries the frontend
    /// used to send dropped the last day for UTC+ users.
    #[test]
    fn last_day_of_month_at_utc_midnight_is_inside_the_month() {
        let conn = setup_test_db();
        const SEPT_30_UTC: i64 = 1_790_726_400; // 2026-09-30T00:00:00Z
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES ('tx1', 'acc1', ?1, '100.00', 'CZK', 'cat_groceries', 'debit')",
            [SEPT_30_UTC],
        )
        .expect("insert");

        // UTC period bounds: 2026-09-01T00:00:00Z ..= 2026-09-30T23:59:59Z
        let report = get_report(&conn, 1_788_220_800, 1_790_812_799, "monthly").expect("report");
        assert_eq!(report.total_expenses, "100.00");

        // Local-time bounds from a UTC+2 client (the old frontend behaviour):
        // 2026-08-31T22:00:00Z ..= 2026-09-29T22:00:00Z — the transaction is lost.
        let local = get_report(&conn, 1_788_213_600, 1_790_719_200, "monthly").expect("report");
        assert_eq!(local.total_expenses, "0.00");

        // ...and the following month (starting 2026-09-30T22:00:00Z) misses it too.
        let next = get_report(&conn, 1_790_805_600, 1_793_397_600, "monthly").expect("report");
        assert_eq!(next.total_expenses, "0.00");
    }

    #[test]
    fn year_end_at_utc_midnight_is_inside_the_year() {
        let conn = setup_test_db();
        const DEC_31_UTC: i64 = 1_798_675_200; // 2026-12-31T00:00:00Z
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES ('tx1', 'acc1', ?1, '250.00', 'CZK', 'cat_income', 'credit')",
            [DEC_31_UTC],
        )
        .expect("insert");
        // 2026-01-01T00:00:00Z ..= 2026-12-31T23:59:59Z
        let report = get_report(&conn, 1_767_225_600, 1_798_761_599, "yearly").expect("report");
        assert_eq!(report.total_income, "250.00");
    }

    #[test]
    fn budget_percentage_converts_goal_currency_to_czk() {
        // XTS is an ISO test code, so it cannot collide with real rates used
        // by other tests sharing the global rate cache.
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTS".to_string(),
            10.0,
        )]));
        let conn = setup_test_db();
        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "100".to_string(), // 100 XTS = 1 000 CZK
                currency: Some("XTS".to_string()),
            },
        )
        .expect("goal");
        conn.execute_batch(
            r#"INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
               VALUES ('tx1', 'acc1', 1704067200, '250', 'CZK', 'cat_groceries', 'debit');"#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        let groceries = &report.expense_categories[0];
        // 250 CZK spent of a 1 000 CZK budget = 25 % (not 250 % as with a CZK assumption)
        assert!((groceries.budget_percentage.expect("pct") - 25.0).abs() < 1e-9);
    }

    fn add_fx_history(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );",
        )
        .expect("fx history schema");
    }

    fn add_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            params![day, currency, rate],
        )
        .expect("rate");
    }

    fn add_expense(conn: &Connection, id: &str, booked: i64, amount: &str, currency: &str) {
        conn.execute(
            "INSERT INTO bank_transactions
             (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES (?1, 'acc1', ?2, ?3, ?4, 'cat_groceries', 'debit')",
            params![id, booked, amount, currency],
        )
        .expect("tx");
    }

    /// ADR 0001: a foreign-currency transaction is converted at the
    /// rate of its booking day, not at today's.
    #[test]
    fn foreign_currency_transactions_convert_at_the_booking_day_rate() {
        // XTB: an ISO test code that no other test touches in the shared cache.
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2, // today
        )]));
        let conn = setup_test_db();
        add_fx_history(&conn);
        let january: i64 = 1_705_276_800; // 2024-01-15
        let june: i64 = 1_718_409_600; // 2024-06-15
        add_rate(&conn, january, "XTB", 24.5);
        add_rate(&conn, june, "XTB", 24.0);
        add_expense(&conn, "tx1", january, "100", "XTB");
        add_expense(&conn, "tx2", june, "100", "XTB");

        let report = get_report(&conn, 0, i64::MAX, "yearly").expect("report");
        // 100 * 24.5 + 100 * 24.0 — not 2 * 100 * 25.2
        assert_eq!(report.total_expenses, "4850.00");
        assert_eq!(report.expense_categories[0].total_amount, "4850.00");
        assert_eq!(report.expense_categories[0].transaction_count, 2);
    }

    #[test]
    fn same_day_transactions_share_one_rate_and_czk_is_untouched() {
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2,
        )]));
        let conn = setup_test_db();
        add_fx_history(&conn);
        let day: i64 = 1_705_276_800;
        add_rate(&conn, day, "XTB", 24.5);
        add_expense(&conn, "tx1", day + 3600, "10", "XTB");
        add_expense(&conn, "tx2", day + 7200, "30", "XTB");
        add_expense(&conn, "tx3", day, "500", "CZK");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        // 40 XTB at 24.5 = 980, plus 500 CZK
        assert_eq!(report.total_expenses, "1480.00");
    }

    #[test]
    fn a_day_without_history_falls_back_to_the_latest_rate() {
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2,
        )]));
        // No exchange_rate_history table rows at all.
        let conn = setup_test_db();
        add_fx_history(&conn);
        add_expense(&conn, "tx1", 1_705_276_800, "100", "XTB");
        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_expenses, "2520.00");

        // History that only starts after the booking day: also the latest rate.
        let conn = setup_test_db();
        add_fx_history(&conn);
        add_rate(&conn, 1_705_276_800 + 30 * 86_400, "XTB", 20.0);
        add_expense(&conn, "tx1", 1_705_276_800, "100", "XTB");
        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_expenses, "2520.00");
    }

    /// a credit nobody categorized yet is still money that
    /// came in. Uncategorized debits already counted toward `total_expenses`;
    /// the credit side must be symmetric.
    #[test]
    fn uncategorized_credits_count_as_income() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '30000.00', 'CZK', NULL, 'credit'),
                ('tx2', 'acc1', 1704067200, '50000.00', 'CZK', 'cat_income', 'credit'),
                ('tx3', 'acc1', 1704067200, '20000.00', 'CZK', 'cat_groceries', 'debit'),
                ('tx4', 'acc1', 1704067200, '5000.00', 'CZK', NULL, 'debit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");

        assert_eq!(report.total_income, "80000.00");
        assert_eq!(report.total_expenses, "25000.00");
        assert_eq!(report.net_balance, "55000.00");
        // Still reported separately so the UI can show an "Uncategorized income" row.
        assert_eq!(report.uncategorized_income, "30000.00");
        assert_eq!(report.uncategorized_expenses, "5000.00");
        assert_eq!(report.uncategorized_transaction_count, 2);
        // The per-category income list is unchanged: uncategorized is not a category.
        assert_eq!(report.income_categories.len(), 1);
        assert_eq!(report.income_categories[0].total_amount, "50000.00");
    }

    #[test]
    fn only_uncategorized_credits_still_produce_income_and_a_surplus() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '3200.00', 'CZK', NULL, 'credit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_income, "3200.00");
        assert_eq!(report.net_balance, "3200.00");
    }

    #[test]
    fn test_get_report_only_income() {
        let conn = setup_test_db();

        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '500.00', 'CZK', 'cat_income', 'credit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_income, "500.00");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.net_balance, "500.00");
    }
}
