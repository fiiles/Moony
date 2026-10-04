//! Transaction category management: list, create, edit, usage, safe delete.
//!
//! Commands in `commands/categories.rs` stay thin and delegate here. A delete
//! never leaves a dangling reference: while anything points at the category
//! the caller must name a replacement (`reassign_to`), and every reference is
//! moved in one database transaction.

use crate::error::{AppError, Result};
use crate::models::{
    CategoryUsage, InsertTransactionCategory, TransactionCategory, UpdateTransactionCategory,
};
use crate::services::bank_accounts::category_exists;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

/// Where a new custom category sorts: after the seeded ones (1..17), before "Other" (99).
const DEFAULT_SORT_ORDER: i32 = 50;

const SELECT_COLUMNS: &str = "id, name, icon, color, parent_id, sort_order, is_system, created_at";

fn row_to_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionCategory> {
    Ok(TransactionCategory {
        id: row.get(0)?,
        name: row.get(1)?,
        icon: row.get(2)?,
        color: row.get(3)?,
        parent_id: row.get(4)?,
        sort_order: row.get(5)?,
        is_system: row.get::<_, i32>(6)? != 0,
        created_at: row.get(7)?,
    })
}

/// All categories in display order.
pub fn list_categories(conn: &Connection) -> Result<Vec<TransactionCategory>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} FROM transaction_categories ORDER BY sort_order ASC, name ASC"
    ))?;
    let rows = stmt.query_map([], row_to_category)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn get_category(conn: &Connection, id: &str) -> Result<TransactionCategory> {
    conn.query_row(
        &format!("SELECT {SELECT_COLUMNS} FROM transaction_categories WHERE id = ?1"),
        [id],
        row_to_category,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("Category not found: {id}")))
}

/// Is another category (not `except_id`) already called `name`? Compared
/// case-insensitively in Rust, because SQLite's `lower()` only folds ASCII.
fn name_taken(conn: &Connection, name: &str, except_id: Option<&str>) -> Result<bool> {
    let wanted = name.trim().to_lowercase();
    let mut stmt = conn.prepare("SELECT id, name FROM transaction_categories")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, existing) = row?;
        if Some(id.as_str()) != except_id && existing.trim().to_lowercase() == wanted {
            return Ok(true);
        }
    }
    Ok(false)
}

fn name_exists_error() -> AppError {
    AppError::Validation("validation.categoryNameExists".into())
}

/// Create a custom category.
pub fn create_category(
    conn: &Connection,
    data: &InsertTransactionCategory,
) -> Result<TransactionCategory> {
    data.validate()?;
    let name = data.name.trim();
    if name_taken(conn, name, None)? {
        return Err(name_exists_error());
    }
    if let Some(ref parent) = data.parent_id {
        if !category_exists(conn, parent)? {
            return Err(AppError::NotFound(format!("Category not found: {parent}")));
        }
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO transaction_categories (id, name, icon, color, parent_id, sort_order, is_system, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
        params![
            id,
            name,
            data.icon,
            data.color,
            data.parent_id,
            data.sort_order.unwrap_or(DEFAULT_SORT_ORDER),
            now
        ],
    )?;
    get_category(conn, &id)
}

/// Edit a category's name, icon or color (system categories included).
/// Absent fields are left as they are.
pub fn update_category(
    conn: &Connection,
    id: &str,
    data: &UpdateTransactionCategory,
) -> Result<TransactionCategory> {
    data.validate()?;
    let existing = get_category(conn, id)?;

    let name = match data.name.as_deref().map(str::trim) {
        Some(new_name) if new_name != existing.name => {
            if name_taken(conn, new_name, Some(id))? {
                return Err(name_exists_error());
            }
            new_name.to_string()
        }
        _ => existing.name.clone(),
    };
    let icon = data.icon.clone().or(existing.icon);
    let color = data.color.clone().or(existing.color);

    conn.execute(
        "UPDATE transaction_categories SET name = ?1, icon = ?2, color = ?3 WHERE id = ?4",
        params![name, icon, color, id],
    )?;
    get_category(conn, id)
}

fn usage_of(conn: &Connection, id: &str) -> Result<CategoryUsage> {
    let usage = conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM bank_transactions WHERE category_id = ?1),
            (SELECT COUNT(*) FROM categorization_rules WHERE category_id = ?1),
            (SELECT COUNT(*) FROM learned_payees WHERE category_id = ?1),
            (SELECT COUNT(*) FROM budget_goals WHERE category_id = ?1)",
        [id],
        |row| {
            Ok(CategoryUsage {
                category_id: id.to_string(),
                transactions: row.get(0)?,
                rules: row.get(1)?,
                learned_payees: row.get(2)?,
                budget_goals: row.get(3)?,
            })
        },
    )?;
    Ok(usage)
}

/// Reference counts for every category, in display order.
pub fn category_usage(conn: &Connection) -> Result<Vec<CategoryUsage>> {
    list_categories(conn)?
        .iter()
        .map(|category| usage_of(conn, &category.id))
        .collect()
}

/// Delete a custom category.
///
/// While anything references it (transactions, rules, learned payees, budget
/// goals) a replacement must be named in `reassign_to`; every reference then
/// moves to it inside one database transaction. System categories can be
/// renamed but never deleted.
///
/// Moved transactions get their `categorization_source` reset, the same
/// invariant `set_transaction_category` keeps: provenance describes how a
/// category was decided, and the replacement is a human decision. A budget
/// goal moves too, unless the replacement already has a goal for that
/// timeframe — then the replacement's own goal wins. Pending MCP suggestions
/// for the category are dropped (an unconfirmed suggestion never trained
/// anything).
pub fn delete_category(conn: &Connection, id: &str, reassign_to: Option<&str>) -> Result<()> {
    let category = get_category(conn, id)?;
    if category.is_system {
        return Err(AppError::Validation(
            "validation.categorySystemNotDeletable".into(),
        ));
    }

    match reassign_to {
        Some(target) => {
            if target == id {
                return Err(AppError::Validation(
                    "validation.categoryReassignInvalid".into(),
                ));
            }
            if !category_exists(conn, target)? {
                return Err(AppError::NotFound(format!("Category not found: {target}")));
            }
        }
        None => {
            if usage_of(conn, id)?.total() > 0 {
                return Err(AppError::Validation("validation.categoryInUse".into()));
            }
        }
    }

    let tx = conn.unchecked_transaction()?;
    if let Some(target) = reassign_to {
        tx.execute(
            "UPDATE bank_transactions SET category_id = ?2, categorization_source = NULL
             WHERE category_id = ?1",
            params![id, target],
        )?;
        tx.execute(
            "UPDATE categorization_rules SET category_id = ?2 WHERE category_id = ?1",
            params![id, target],
        )?;
        tx.execute(
            "UPDATE learned_payees SET category_id = ?2 WHERE category_id = ?1",
            params![id, target],
        )?;
        tx.execute(
            "DELETE FROM budget_goals
             WHERE category_id = ?1
               AND timeframe IN (SELECT timeframe FROM budget_goals WHERE category_id = ?2)",
            params![id, target],
        )?;
        tx.execute(
            "UPDATE budget_goals SET category_id = ?2 WHERE category_id = ?1",
            params![id, target],
        )?;
    }
    tx.execute(
        "UPDATE bank_transactions SET suggested_category_id = NULL WHERE suggested_category_id = ?1",
        [id],
    )?;
    tx.execute(
        "UPDATE transaction_categories SET parent_id = NULL WHERE parent_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM transaction_categories WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        // Mirrors production: foreign keys are enforced.
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("pragma");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                icon TEXT,
                color TEXT,
                parent_id TEXT REFERENCES transaction_categories(id),
                sort_order INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                category_id TEXT REFERENCES transaction_categories(id),
                suggested_category_id TEXT REFERENCES transaction_categories(id),
                categorization_source TEXT
            );
            CREATE TABLE categorization_rules (
                id TEXT PRIMARY KEY,
                category_id TEXT NOT NULL REFERENCES transaction_categories(id)
            );
            CREATE TABLE learned_payees (
                id TEXT PRIMARY KEY,
                normalized_payee TEXT,
                category_id TEXT NOT NULL REFERENCES transaction_categories(id)
            );
            CREATE TABLE budget_goals (
                id TEXT PRIMARY KEY,
                category_id TEXT NOT NULL REFERENCES transaction_categories(id) ON DELETE CASCADE,
                timeframe TEXT NOT NULL,
                amount TEXT NOT NULL,
                UNIQUE(category_id, timeframe)
            );

            INSERT INTO transaction_categories (id, name, icon, color, sort_order, is_system) VALUES
                ('cat_groceries', 'Groceries', 'shopping-cart', '#4CAF50', 1, 1),
                ('cat_other', 'Other', 'more-horizontal', '#9E9E9E', 99, 1),
                ('custom-pets', 'Pets', 'paw-print', '#FF9800', 50, 0),
                ('custom-kids', 'Kids', 'baby', '#2196F3', 50, 0);
            "#,
        )
        .expect("schema");
        conn
    }

    fn insert(name: &str) -> InsertTransactionCategory {
        InsertTransactionCategory {
            name: name.to_string(),
            icon: None,
            color: None,
            parent_id: None,
            sort_order: None,
        }
    }

    fn validation_key(err: AppError) -> String {
        match err {
            AppError::Validation(key) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    // ---- create ----

    #[test]
    fn create_stores_a_trimmed_custom_category_after_the_seeded_ones() {
        let conn = setup_test_db();
        let created = create_category(
            &conn,
            &InsertTransactionCategory {
                icon: Some("gift".into()),
                color: Some("#E91E63".into()),
                ..insert("  Charity  ")
            },
        )
        .expect("create");

        assert_eq!(created.name, "Charity");
        assert!(!created.is_system, "created categories are never system");
        assert_eq!(created.sort_order, 50);
        assert_eq!(created.icon.as_deref(), Some("gift"));
        let listed = list_categories(&conn).expect("list");
        assert!(listed.iter().any(|c| c.id == created.id));
        // seeded first, custom in the middle, "Other" last
        assert_eq!(listed.first().map(|c| c.id.as_str()), Some("cat_groceries"));
        assert_eq!(listed.last().map(|c| c.id.as_str()), Some("cat_other"));
    }

    #[test]
    fn create_rejects_bad_input_with_i18n_keys() {
        let conn = setup_test_db();
        assert_eq!(
            validation_key(create_category(&conn, &insert("   ")).unwrap_err()),
            "validation.categoryNameRequired"
        );
        assert_eq!(
            validation_key(create_category(&conn, &insert(&"x".repeat(51))).unwrap_err()),
            "validation.categoryNameTooLong"
        );
        assert_eq!(
            validation_key(
                create_category(
                    &conn,
                    &InsertTransactionCategory {
                        color: Some("orange".into()),
                        ..insert("Hobby")
                    }
                )
                .unwrap_err()
            ),
            "validation.categoryColorInvalid"
        );
        assert_eq!(
            validation_key(
                create_category(
                    &conn,
                    &InsertTransactionCategory {
                        icon: Some("Paw Print".into()),
                        ..insert("Hobby")
                    }
                )
                .unwrap_err()
            ),
            "validation.categoryIconInvalid"
        );
    }

    #[test]
    fn create_rejects_a_duplicate_name_ignoring_case() {
        let conn = setup_test_db();
        assert_eq!(
            validation_key(create_category(&conn, &insert("pets")).unwrap_err()),
            "validation.categoryNameExists"
        );
        // Non-ASCII names compare case-insensitively too (SQLite's lower() would not).
        create_category(&conn, &insert("Děti")).expect("create");
        assert_eq!(
            validation_key(create_category(&conn, &insert("děti")).unwrap_err()),
            "validation.categoryNameExists"
        );
    }

    #[test]
    fn create_rejects_an_unknown_parent() {
        let conn = setup_test_db();
        let err = create_category(
            &conn,
            &InsertTransactionCategory {
                parent_id: Some("ghost".into()),
                ..insert("Hobby")
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    // ---- update ----

    #[test]
    fn update_changes_only_the_fields_that_are_present() {
        let conn = setup_test_db();
        let updated = update_category(
            &conn,
            "custom-pets",
            &UpdateTransactionCategory {
                name: Some("Pets & vets".into()),
                icon: None,
                color: Some("#00BCD4".into()),
            },
        )
        .expect("update");
        assert_eq!(updated.name, "Pets & vets");
        assert_eq!(updated.color.as_deref(), Some("#00BCD4"));
        assert_eq!(updated.icon.as_deref(), Some("paw-print"), "icon untouched");
    }

    #[test]
    fn update_can_rename_a_system_category() {
        let conn = setup_test_db();
        let updated = update_category(
            &conn,
            "cat_groceries",
            &UpdateTransactionCategory {
                name: Some("Food".into()),
                ..Default::default()
            },
        )
        .expect("update");
        assert_eq!(updated.name, "Food");
        assert!(
            updated.is_system,
            "renaming does not change the system flag"
        );
    }

    #[test]
    fn update_rejects_a_name_taken_by_another_category_but_allows_keeping_its_own() {
        let conn = setup_test_db();
        assert_eq!(
            validation_key(
                update_category(
                    &conn,
                    "custom-pets",
                    &UpdateTransactionCategory {
                        name: Some("kids".into()),
                        ..Default::default()
                    }
                )
                .unwrap_err()
            ),
            "validation.categoryNameExists"
        );
        // Re-saving its own name (even with different case) is fine.
        update_category(
            &conn,
            "custom-pets",
            &UpdateTransactionCategory {
                name: Some("PETS".into()),
                ..Default::default()
            },
        )
        .expect("own name");
    }

    #[test]
    fn update_unknown_category_is_not_found() {
        let conn = setup_test_db();
        let err =
            update_category(&conn, "ghost", &UpdateTransactionCategory::default()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    // ---- usage ----

    fn seed_references_to_pets(conn: &Connection) {
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, category_id, categorization_source) VALUES
                ('t1', 'custom-pets', 'rule'), ('t2', 'custom-pets', NULL), ('t3', 'cat_groceries', NULL);
            INSERT INTO categorization_rules (id, category_id) VALUES ('r1', 'custom-pets');
            INSERT INTO learned_payees (id, normalized_payee, category_id) VALUES
                ('l1', 'zoo shop', 'custom-pets'), ('l2', 'vet', 'custom-pets'), ('l3', 'tesco', 'cat_groceries');
            INSERT INTO budget_goals (id, category_id, timeframe, amount) VALUES
                ('g1', 'custom-pets', 'monthly', '1000');
            "#,
        )
        .expect("seed");
    }

    #[test]
    fn usage_counts_every_kind_of_reference() {
        let conn = setup_test_db();
        seed_references_to_pets(&conn);

        let usage = category_usage(&conn).expect("usage");
        let pets = usage
            .iter()
            .find(|u| u.category_id == "custom-pets")
            .unwrap();
        assert_eq!(
            (
                pets.transactions,
                pets.rules,
                pets.learned_payees,
                pets.budget_goals
            ),
            (2, 1, 2, 1)
        );
        assert_eq!(pets.total(), 6);
        let kids = usage
            .iter()
            .find(|u| u.category_id == "custom-kids")
            .unwrap();
        assert_eq!(kids.total(), 0);
        assert_eq!(usage.len(), 4, "one entry per category");
    }

    // ---- delete ----

    #[test]
    fn delete_an_unused_category() {
        let conn = setup_test_db();
        delete_category(&conn, "custom-kids", None).expect("delete");
        assert!(!list_categories(&conn)
            .unwrap()
            .iter()
            .any(|c| c.id == "custom-kids"));
    }

    #[test]
    fn delete_refuses_system_categories_even_with_a_replacement() {
        let conn = setup_test_db();
        assert_eq!(
            validation_key(delete_category(&conn, "cat_groceries", Some("cat_other")).unwrap_err()),
            "validation.categorySystemNotDeletable"
        );
    }

    #[test]
    fn delete_unknown_category_is_not_found() {
        let conn = setup_test_db();
        let err = delete_category(&conn, "ghost", None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn delete_refuses_a_category_that_is_still_used_and_changes_nothing() {
        let conn = setup_test_db();
        seed_references_to_pets(&conn);

        assert_eq!(
            validation_key(delete_category(&conn, "custom-pets", None).unwrap_err()),
            "validation.categoryInUse"
        );
        // Each kind of reference alone is enough to refuse.
        let usage = category_usage(&conn).unwrap();
        assert_eq!(
            usage
                .iter()
                .find(|u| u.category_id == "custom-pets")
                .unwrap()
                .total(),
            6
        );
        let still_there: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transaction_categories WHERE id = 'custom-pets'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(still_there, 1);
    }

    #[test]
    fn delete_refuses_when_only_a_rule_or_only_a_goal_references_it() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO categorization_rules (id, category_id) VALUES ('r1', 'custom-kids')",
            [],
        )
        .unwrap();
        assert_eq!(
            validation_key(delete_category(&conn, "custom-kids", None).unwrap_err()),
            "validation.categoryInUse"
        );
        conn.execute("DELETE FROM categorization_rules", [])
            .unwrap();
        conn.execute(
            "INSERT INTO budget_goals (id, category_id, timeframe, amount) VALUES ('g1', 'custom-kids', 'monthly', '5')",
            [],
        )
        .unwrap();
        assert_eq!(
            validation_key(delete_category(&conn, "custom-kids", None).unwrap_err()),
            "validation.categoryInUse"
        );
    }

    #[test]
    fn delete_with_a_replacement_moves_every_reference() {
        let conn = setup_test_db();
        seed_references_to_pets(&conn);
        // A pending MCP suggestion for the doomed category is dropped, not moved.
        conn.execute(
            "INSERT INTO bank_transactions (id, category_id, suggested_category_id) VALUES ('t4', NULL, 'custom-pets')",
            [],
        )
        .unwrap();

        delete_category(&conn, "custom-pets", Some("custom-kids")).expect("delete");

        let moved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_transactions WHERE category_id = 'custom-kids'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(moved, 2);
        // The provenance of the moved rows is reset: the new category is a human decision.
        let sources: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_transactions WHERE category_id = 'custom-kids' AND categorization_source IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sources, 0);
        let rules: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categorization_rules WHERE category_id = 'custom-kids'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rules, 1);
        let learned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM learned_payees WHERE category_id = 'custom-kids'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(learned, 2);
        let goals: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM budget_goals WHERE category_id = 'custom-kids'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(goals, 1);
        let suggestion: Option<String> = conn
            .query_row(
                "SELECT suggested_category_id FROM bank_transactions WHERE id = 't4'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(suggestion, None);
        // Unrelated rows are untouched, and the category is gone.
        let groceries: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_transactions WHERE category_id = 'cat_groceries'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(groceries, 1);
        assert!(!list_categories(&conn)
            .unwrap()
            .iter()
            .any(|c| c.id == "custom-pets"));
    }

    #[test]
    fn delete_keeps_the_targets_budget_goal_when_both_have_one() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO budget_goals (id, category_id, timeframe, amount) VALUES
                ('g-pets-m', 'custom-pets', 'monthly', '1000'),
                ('g-pets-y', 'custom-pets', 'yearly', '12000'),
                ('g-kids-m', 'custom-kids', 'monthly', '3000');
            "#,
        )
        .unwrap();

        delete_category(&conn, "custom-pets", Some("custom-kids")).expect("delete");

        let mut stmt = conn
            .prepare("SELECT timeframe, amount FROM budget_goals WHERE category_id = 'custom-kids' ORDER BY timeframe")
            .unwrap();
        let goals: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            goals,
            vec![
                ("monthly".to_string(), "3000".to_string()), // the target's own goal wins
                ("yearly".to_string(), "12000".to_string()), // the free timeframe is taken over
            ]
        );
    }

    #[test]
    fn delete_rejects_a_bad_replacement() {
        let conn = setup_test_db();
        seed_references_to_pets(&conn);
        assert_eq!(
            validation_key(delete_category(&conn, "custom-pets", Some("custom-pets")).unwrap_err()),
            "validation.categoryReassignInvalid"
        );
        assert!(matches!(
            delete_category(&conn, "custom-pets", Some("ghost")).unwrap_err(),
            AppError::NotFound(_)
        ));
    }

    #[test]
    fn delete_is_atomic_when_a_step_fails() {
        let conn = setup_test_db();
        seed_references_to_pets(&conn);
        // Make the final DELETE impossible: a child category the code cannot detach
        // would not fail, so break the table instead — a trigger aborts the delete.
        conn.execute_batch(
            "CREATE TRIGGER no_delete BEFORE DELETE ON transaction_categories
             BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
        )
        .unwrap();

        assert!(delete_category(&conn, "custom-pets", Some("custom-kids")).is_err());

        // The reassignments before the failing step were rolled back.
        let pets_tx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bank_transactions WHERE category_id = 'custom-pets'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pets_tx, 2);
    }

    #[test]
    fn delete_detaches_children() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO transaction_categories (id, name, parent_id, sort_order) VALUES
                ('child', 'Vet', 'custom-pets', 50);
            "#,
        )
        .unwrap();

        delete_category(&conn, "custom-pets", None).expect("delete");

        let parent: Option<String> = conn
            .query_row(
                "SELECT parent_id FROM transaction_categories WHERE id = 'child'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent, None);
    }
}
