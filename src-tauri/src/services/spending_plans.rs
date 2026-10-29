//! Spending plans: CRUD, closing and tagging payments to a plan.
//!
//! A plan is unique per (year, name) compared case-insensitively over Unicode, so the check is
//! done in Rust (SQLite `lower()` only folds ASCII). Deleting a plan clears the tag on its
//! payments but never touches the payments themselves.

use crate::error::{AppError, Result};
use crate::models::{InsertSpendingPlan, SpendingPlan, SpendingPlanAssignment};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

const COLUMNS: &str = "id, name, year, amount, currency, icon, color, note, closed_at, created_at";

fn plan_row(row: &rusqlite::Row) -> rusqlite::Result<SpendingPlan> {
    Ok(SpendingPlan {
        id: row.get(0)?,
        name: row.get(1)?,
        year: row.get(2)?,
        amount: row.get(3)?,
        currency: row.get(4)?,
        icon: row.get(5)?,
        color: row.get(6)?,
        note: row.get(7)?,
        closed_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}

/// Comparison key for plan names: trimmed, Unicode-lowercased.
pub(crate) fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Spending plan {id}"))
}

fn ensure_name_free(conn: &Connection, year: i32, name: &str, except: Option<&str>) -> Result<()> {
    let key = name_key(name);
    let mut stmt = conn.prepare("SELECT id, name FROM spending_plans WHERE year = ?1")?;
    let rows = stmt.query_map([year], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, existing) = row?;
        if Some(id.as_str()) != except && name_key(&existing) == key {
            return Err(AppError::Validation(
                "validation.spendingPlanNameTaken".into(),
            ));
        }
    }
    Ok(())
}

fn insert_plan(conn: &Connection, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    let data = data.validate()?;
    ensure_name_free(conn, data.year, &data.name, None)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            data.name,
            data.year,
            data.amount,
            data.currency,
            data.icon,
            data.color,
            data.note
        ],
    )?;
    get_plan(conn, &id)
}

pub fn create_plan(conn: &Connection, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    insert_plan(conn, data)
}

/// Create several plans in one transaction; any failure leaves nothing behind.
pub fn create_plans(conn: &Connection, items: &[InsertSpendingPlan]) -> Result<Vec<SpendingPlan>> {
    let tx = conn.unchecked_transaction()?;
    let mut created = Vec::with_capacity(items.len());
    for item in items {
        created.push(insert_plan(&tx, item)?);
    }
    tx.commit()?;
    Ok(created)
}

pub fn get_plan(conn: &Connection, id: &str) -> Result<SpendingPlan> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM spending_plans WHERE id = ?1"),
        [id],
        plan_row,
    )
    .optional()?
    .ok_or_else(|| not_found(id))
}

pub fn update_plan(conn: &Connection, id: &str, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    let data = data.validate()?;
    get_plan(conn, id)?;
    ensure_name_free(conn, data.year, &data.name, Some(id))?;
    conn.execute(
        "UPDATE spending_plans SET name = ?2, year = ?3, amount = ?4, currency = ?5, icon = ?6,
             color = ?7, note = ?8, updated_at = unixepoch() WHERE id = ?1",
        params![
            id,
            data.name,
            data.year,
            data.amount,
            data.currency,
            data.icon,
            data.color,
            data.note
        ],
    )?;
    get_plan(conn, id)
}

pub fn set_plan_closed(conn: &Connection, id: &str, closed: bool) -> Result<SpendingPlan> {
    get_plan(conn, id)?;
    conn.execute(
        "UPDATE spending_plans
         SET closed_at = CASE WHEN ?2 THEN unixepoch() ELSE NULL END, updated_at = unixepoch()
         WHERE id = ?1",
        params![id, closed],
    )?;
    get_plan(conn, id)
}

/// Delete a plan; its payments stay, untagged.
pub fn delete_plan(conn: &Connection, id: &str) -> Result<()> {
    get_plan(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE bank_transactions SET spending_plan_id = NULL WHERE spending_plan_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM spending_plans WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

/// Tag, move or untag payments in one transaction. Tagging to a closed plan is refused unless
/// `restore` (an undo putting earlier tags back).
pub fn set_transaction_plans(
    conn: &Connection,
    assignments: &[SpendingPlanAssignment],
    restore: bool,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for a in assignments {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bank_transactions WHERE id = ?1)",
            [&a.transaction_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(AppError::NotFound(format!(
                "Transaction {}",
                a.transaction_id
            )));
        }
        if let Some(plan_id) = &a.plan_id {
            let plan = get_plan(&tx, plan_id)?;
            if plan.closed_at.is_some() && !restore {
                return Err(AppError::Validation("validation.spendingPlanClosed".into()));
            }
        }
        tx.execute(
            "UPDATE bank_transactions SET spending_plan_id = ?2 WHERE id = ?1",
            params![a.transaction_id, a.plan_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("fk");
        conn.execute_batch(
            r#"
            CREATE TABLE spending_plans (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, year INTEGER NOT NULL,
                amount TEXT NOT NULL, currency TEXT NOT NULL, icon TEXT NOT NULL,
                color TEXT NOT NULL, note TEXT, closed_at INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL DEFAULT 'a',
                amount TEXT NOT NULL DEFAULT '-10', currency TEXT NOT NULL DEFAULT 'EUR',
                booking_date INTEGER NOT NULL DEFAULT 0, status TEXT NOT NULL DEFAULT 'booked',
                tx_type TEXT NOT NULL DEFAULT 'expense', category_id TEXT,
                spending_plan_id TEXT REFERENCES spending_plans(id) ON DELETE SET NULL
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn plan(name: &str, year: i32) -> InsertSpendingPlan {
        InsertSpendingPlan {
            name: name.into(),
            year,
            amount: "1000".into(),
            currency: "eur".into(),
            icon: "plane".into(),
            color: "#3366cc".into(),
            note: None,
        }
    }

    fn add_tx(conn: &Connection, id: &str) {
        conn.execute("INSERT INTO bank_transactions (id) VALUES (?1)", [id])
            .unwrap();
    }

    fn tag(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT spending_plan_id FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn assign(tx: &str, plan: Option<&str>) -> SpendingPlanAssignment {
        SpendingPlanAssignment {
            transaction_id: tx.into(),
            plan_id: plan.map(String::from),
        }
    }

    fn validation_key(r: Result<impl std::fmt::Debug>) -> String {
        match r {
            Err(AppError::Validation(k)) => k,
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn create_and_get_plan_roundtrip() {
        let conn = setup_test_db();
        let mut data = plan("  Dovolená ", 2026);
        data.note = Some("Řecko".into());
        let created = create_plan(&conn, &data).unwrap();
        assert_eq!(created.name, "Dovolená");
        assert_eq!(created.currency, "EUR");
        assert_eq!(created.closed_at, None);
        assert_eq!(get_plan(&conn, &created.id).unwrap(), created);
        assert_eq!(created.note.as_deref(), Some("Řecko"));
        assert!(matches!(
            get_plan(&conn, "nope"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn validate_rejects_each_bad_field() {
        let bad = |f: fn(&mut InsertSpendingPlan), key: &str| {
            let mut p = plan("Ok", 2026);
            f(&mut p);
            assert_eq!(validation_key(p.validate()), format!("validation.{key}"));
        };
        bad(|p| p.name = "   ".into(), "spendingPlanNameRequired");
        bad(|p| p.name = "x".repeat(101), "spendingPlanNameTooLong");
        bad(|p| p.year = 1999, "spendingPlanYearInvalid");
        bad(|p| p.year = 2101, "spendingPlanYearInvalid");
        bad(|p| p.amount = "abc".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "NaN".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "0".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "-5".into(), "spendingPlanAmountInvalid");
        bad(|p| p.currency = "EURO".into(), "currencyInvalid");
        bad(|p| p.icon = "".into(), "spendingPlanIconInvalid");
        bad(|p| p.icon = "Plane".into(), "spendingPlanIconInvalid");
        bad(|p| p.icon = "a".repeat(41), "spendingPlanIconInvalid");
        bad(|p| p.color = "3366cc".into(), "spendingPlanColorInvalid");
        bad(|p| p.color = "#33g6cc".into(), "spendingPlanColorInvalid");
        bad(
            |p| p.note = Some("n".repeat(501)),
            "spendingPlanNoteTooLong",
        );
        assert!(plan("Ok", 2026).validate().is_ok());
    }

    #[test]
    fn name_unique_per_year_unicode_case() {
        let conn = setup_test_db();
        create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        assert_eq!(
            validation_key(create_plan(&conn, &plan("DOVOLENÁ", 2026))),
            "validation.spendingPlanNameTaken"
        );
        create_plan(&conn, &plan("Dovolená", 2027)).unwrap();
        create_plan(&conn, &plan("Dovolena", 2026)).unwrap();
        assert_eq!(name_key("  ŠKOLA "), "škola");
    }

    #[test]
    fn update_keeps_own_name() {
        let conn = setup_test_db();
        let a = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let b = create_plan(&conn, &plan("Kolo", 2026)).unwrap();
        let mut data = plan("DOVOLENÁ", 2026);
        data.amount = "2500".into();
        let updated = update_plan(&conn, &a.id, &data).unwrap();
        assert_eq!(updated.name, "DOVOLENÁ");
        assert_eq!(updated.amount, "2500");
        assert_eq!(
            validation_key(update_plan(&conn, &b.id, &plan("dovolená", 2026))),
            "validation.spendingPlanNameTaken"
        );
        assert!(matches!(
            update_plan(&conn, "nope", &plan("X", 2026)),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn create_plans_is_all_or_nothing() {
        let conn = setup_test_db();
        let r = create_plans(&conn, &[plan("Dovolená", 2026), plan("dovolená", 2026)]);
        assert_eq!(validation_key(r), "validation.spendingPlanNameTaken");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM spending_plans", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let ok = create_plans(&conn, &[plan("A", 2026), plan("B", 2026)]).unwrap();
        assert_eq!(ok.len(), 2);
    }

    #[test]
    fn close_and_reopen() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        assert!(set_plan_closed(&conn, &p.id, true)
            .unwrap()
            .closed_at
            .is_some());
        assert_eq!(
            set_plan_closed(&conn, &p.id, false).unwrap().closed_at,
            None
        );
        assert!(matches!(
            set_plan_closed(&conn, "nope", true),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn delete_clears_tags() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        add_tx(&conn, "t2");
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], false).unwrap();
        delete_plan(&conn, &p.id).unwrap();
        assert!(matches!(get_plan(&conn, &p.id), Err(AppError::NotFound(_))));
        assert_eq!(tag(&conn, "t1"), None);
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
        assert!(matches!(
            delete_plan(&conn, &p.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn delete_plan_leaves_payments_in_place() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], false).unwrap();
        // Even a bare DELETE (FK action only) keeps the payment and nulls the tag.
        conn.execute("DELETE FROM spending_plans WHERE id = ?1", [&p.id])
            .unwrap();
        let (count, tagged): (i64, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(spending_plan_id) FROM bank_transactions WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((count, tagged), (1, None));
    }

    #[test]
    fn assign_move_remove() {
        let conn = setup_test_db();
        let a = create_plan(&conn, &plan("A", 2026)).unwrap();
        let b = create_plan(&conn, &plan("B", 2026)).unwrap();
        add_tx(&conn, "t1");
        add_tx(&conn, "t2");
        set_transaction_plans(
            &conn,
            &[assign("t1", Some(&a.id)), assign("t2", Some(&a.id))],
            false,
        )
        .unwrap();
        assert_eq!(tag(&conn, "t2").as_deref(), Some(a.id.as_str()));
        set_transaction_plans(&conn, &[assign("t1", Some(&b.id))], false).unwrap();
        assert_eq!(tag(&conn, "t1").as_deref(), Some(b.id.as_str()));
        set_transaction_plans(&conn, &[assign("t1", None), assign("t2", None)], false).unwrap();
        assert_eq!((tag(&conn, "t1"), tag(&conn, "t2")), (None, None));
    }

    #[test]
    fn assign_to_closed_rejected_unless_restore() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        set_plan_closed(&conn, &p.id, true).unwrap();
        assert_eq!(
            validation_key(set_transaction_plans(
                &conn,
                &[assign("t1", Some(&p.id))],
                false
            )),
            "validation.spendingPlanClosed"
        );
        assert_eq!(tag(&conn, "t1"), None);
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], true).unwrap();
        assert_eq!(tag(&conn, "t1").as_deref(), Some(p.id.as_str()));
        // Untagging from a closed plan is always allowed.
        set_transaction_plans(&conn, &[assign("t1", None)], false).unwrap();
    }

    #[test]
    fn assign_unknown_ids_not_found() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        assert!(matches!(
            set_transaction_plans(&conn, &[assign("ghost", Some(&p.id))], false),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            set_transaction_plans(&conn, &[assign("t1", Some("ghost"))], false),
            Err(AppError::NotFound(_))
        ));
        // All-or-nothing: a good first assignment is rolled back by a bad second one.
        let r = set_transaction_plans(
            &conn,
            &[assign("t1", Some(&p.id)), assign("ghost", None)],
            false,
        );
        assert!(matches!(r, Err(AppError::NotFound(_))));
        assert_eq!(tag(&conn, "t1"), None);
    }
}
