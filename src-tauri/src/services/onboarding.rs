//! First-run onboarding progress and flags
//!
//! Flags live in `app_config` under a small whitelist so the generic setter
//! can never touch API keys or rule-pack configuration.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::models::OnboardingProgress;

pub const FLAG_COMPLETED_AT: &str = "onboarding.completedAt";
pub const FLAG_CHECKLIST_DISMISSED: &str = "onboarding.checklistDismissed";
pub const FLAG_LAST_BACKUP_AT: &str = "backup.lastCreatedAt";

const ALLOWED_FLAGS: [&str; 3] = [
    FLAG_COMPLETED_AT,
    FLAG_CHECKLIST_DISMISSED,
    FLAG_LAST_BACKUP_AT,
];

fn read_flag(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [key],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

fn exists(conn: &Connection, sql: &str) -> Result<bool> {
    Ok(conn
        .query_row(sql, [], |_| Ok(true))
        .optional()?
        .unwrap_or(false))
}

/// Set (or clear with `None`) one whitelisted flag.
pub fn set_flag(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    if !ALLOWED_FLAGS.contains(&key) {
        return Err(AppError::Validation(
            "validation.onboardingFlagUnknown".into(),
        ));
    }
    match value {
        Some(v) => {
            conn.execute(
                "INSERT INTO app_config (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, v],
            )?;
        }
        None => {
            conn.execute("DELETE FROM app_config WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

/// Record "a backup was just created" (called by the backup command).
pub fn record_backup_created(conn: &Connection, at: i64) -> Result<()> {
    set_flag(conn, FLAG_LAST_BACKUP_AT, Some(&at.to_string()))
}

/// Everything the dashboard checklist needs, in one round trip.
pub fn get_progress(conn: &Connection) -> Result<OnboardingProgress> {
    let has_account = exists(conn, "SELECT 1 FROM bank_accounts LIMIT 1")?;
    let has_transactions = exists(conn, "SELECT 1 FROM bank_transactions LIMIT 1")?;
    let has_investment = exists(
        conn,
        "SELECT 1 FROM stock_investments WHERE CAST(quantity AS REAL) > 0 LIMIT 1",
    )? || exists(
        conn,
        "SELECT 1 FROM crypto_investments WHERE CAST(quantity AS REAL) > 0 LIMIT 1",
    )?;
    let has_budget = exists(conn, "SELECT 1 FROM budget_goals LIMIT 1")?;

    let parse_ts = |v: Option<String>| v.and_then(|s| s.parse::<i64>().ok());
    Ok(OnboardingProgress {
        has_account,
        has_transactions,
        has_investment,
        has_budget,
        last_backup_at: parse_ts(read_flag(conn, FLAG_LAST_BACKUP_AT)?),
        checklist_dismissed: read_flag(conn, FLAG_CHECKLIST_DISMISSED)?
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false),
        completed_at: parse_ts(read_flag(conn, FLAG_COMPLETED_AT)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE bank_transactions (id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL);
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, quantity TEXT NOT NULL DEFAULT '0');
            CREATE TABLE crypto_investments (id TEXT PRIMARY KEY, quantity TEXT NOT NULL DEFAULT '0');
            CREATE TABLE budget_goals (id TEXT PRIMARY KEY, amount TEXT NOT NULL);
            "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn progress_is_empty_for_a_fresh_account() {
        let conn = setup_test_db();
        let p = get_progress(&conn).expect("progress");
        assert!(!p.has_account && !p.has_transactions && !p.has_investment && !p.has_budget);
        assert_eq!(p.last_backup_at, None);
        assert!(!p.checklist_dismissed);
        assert_eq!(p.completed_at, None);
    }

    #[test]
    fn progress_reflects_data_and_flags() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts VALUES ('a', 'Main');
             INSERT INTO bank_transactions VALUES ('t', 'a');
             INSERT INTO crypto_investments VALUES ('c', '0.5');
             INSERT INTO stock_investments VALUES ('s', '0');
             INSERT INTO budget_goals VALUES ('g', '100');",
        )
        .expect("seed");
        set_flag(&conn, FLAG_COMPLETED_AT, Some("1790000000")).expect("flag");
        set_flag(&conn, FLAG_CHECKLIST_DISMISSED, Some("1")).expect("flag");
        record_backup_created(&conn, 1790000500).expect("backup flag");

        let p = get_progress(&conn).expect("progress");
        assert!(p.has_account && p.has_transactions && p.has_investment && p.has_budget);
        assert_eq!(p.completed_at, Some(1790000000));
        assert!(p.checklist_dismissed);
        assert_eq!(p.last_backup_at, Some(1790000500));

        // Clearing a flag removes the row.
        set_flag(&conn, FLAG_CHECKLIST_DISMISSED, None).expect("clear");
        assert!(!get_progress(&conn).expect("progress").checklist_dismissed);
    }

    #[test]
    fn a_sold_out_position_does_not_count_as_an_investment() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO stock_investments VALUES ('s', '0')", [])
            .expect("seed");
        assert!(!get_progress(&conn).expect("progress").has_investment);
    }

    #[test]
    fn only_whitelisted_flags_can_be_set() {
        let conn = setup_test_db();
        let err = set_flag(&conn, "api_key_coingecko", Some("x")).expect_err("must refuse");
        assert!(matches!(err, AppError::Validation(_)));
        assert_eq!(read_flag(&conn, "api_key_coingecko").expect("read"), None);
    }
}
