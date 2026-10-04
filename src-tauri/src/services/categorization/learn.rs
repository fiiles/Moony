//! Learned-payee persistence, shared by the manual UI flow
//! (`learn_categorization`) and suggestion confirmation
//! (`accept_category_suggestion`).
//!
//! Only human decisions reach this module (spec 2026-08-19 / 2026-08-27 D5):
//! an unconfirmed LLM suggestion never trains the exact-match layer.
//!
//! Payees are keyed by [`normalize_payee_key`] (RUL-02) and IBANs by
//! [`normalize_iban`], when learning and when matching. Rows written
//! before that keep their raw text (`original_payee`, and an IBAN as typed);
//! they are normalized *here*, on read, so nothing needs a data migration:
//! engine load, the rules list, delete and category edits all treat rows that
//! share a key as one learned rule, and the newest one wins.

use std::collections::HashMap;

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::tokenizer::{normalize_iban, normalize_payee_key};
use super::CategorizationEngine;
use crate::error::{AppError, Result};
use crate::services::bank_accounts::category_exists;

/// Payee key for matching, or `None` when too short (or only a reference
/// number) to identify anyone — the same cut-off the exact-match engine uses.
fn payee_key(payee: &str) -> Option<String> {
    let key = normalize_payee_key(payee);
    (key.len() > 2).then_some(key)
}

fn iban_key(iban: &str) -> Option<String> {
    let key = normalize_iban(iban);
    (!key.is_empty()).then_some(key)
}

/// `(payee key, IBAN key, own id)` — see `StoredRow::group`.
type Group = (Option<String>, Option<String>, Option<String>);

/// One `learned_payees` row as stored.
#[derive(Debug, Clone)]
struct StoredRow {
    id: String,
    normalized_payee: Option<String>,
    original_payee: Option<String>,
    counterparty_iban: Option<String>,
    category_id: String,
    created_at: i64,
    updated_at: i64,
}

impl StoredRow {
    /// The row's payee key, derived from the raw text when it was kept (it
    /// still has the punctuation `simple_normalize` used to drop), else from
    /// the stored key.
    fn payee_key(&self) -> Option<String> {
        self.original_payee
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .and_then(payee_key)
            .or_else(|| self.normalized_payee.as_deref().and_then(payee_key))
    }

    fn iban_key(&self) -> Option<String> {
        self.counterparty_iban.as_deref().and_then(iban_key)
    }

    /// Rows of the same group are one learned rule. A row with neither a
    /// payee nor an IBAN key identifies nobody and is its own group.
    fn group(&self) -> Group {
        self.group_with(self.payee_key(), self.iban_key())
    }

    /// `group()` for keys the caller has already derived.
    fn group_with(&self, payee: Option<String>, iban: Option<String>) -> Group {
        let own_id = (payee.is_none() && iban.is_none()).then(|| self.id.clone());
        (payee, iban, own_id)
    }
}

/// Every row, oldest first, so that applying them in order lets the newest win.
fn all_rows(conn: &Connection) -> Result<Vec<StoredRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, normalized_payee, original_payee, counterparty_iban, category_id, created_at, updated_at
         FROM learned_payees
         ORDER BY updated_at ASC, created_at ASC, rowid ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(StoredRow {
                id: row.get(0)?,
                normalized_payee: row.get(1)?,
                original_payee: row.get(2)?,
                counterparty_iban: row.get(3)?,
                category_id: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// What the engine needs to forget or relearn a rule after a database change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedKey {
    pub payee: Option<String>,
    pub iban: Option<String>,
}

/// Persist a payee/IBAN → category mapping and teach the in-memory engine.
///
/// Earlier rows for the same payee key and IBAN — including old ones keyed by
/// the raw text — are replaced, so a payee is never stored twice. (This is a
/// DELETE + INSERT rather than an upsert because SQLite treats NULLs as
/// distinct in unique indexes.) A no-op when neither a usable payee nor an
/// IBAN is given.
pub fn learn_payee(
    conn: &Connection,
    engine: &CategorizationEngine,
    payee: Option<&str>,
    counterparty_iban: Option<&str>,
    category_id: &str,
) -> Result<()> {
    let key = payee.and_then(payee_key);
    let iban = counterparty_iban.and_then(iban_key);
    if key.is_none() && iban.is_none() {
        return Ok(());
    }

    engine.learn_from_user(payee, counterparty_iban, category_id);

    for row in all_rows(conn)? {
        if row.payee_key() == key && row.iban_key() == iban {
            conn.execute("DELETE FROM learned_payees WHERE id = ?1", [&row.id])?;
        }
    }
    conn.execute(
        "INSERT INTO learned_payees (id, normalized_payee, original_payee, counterparty_iban, category_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())",
        params![
            Uuid::new_v4().to_string(),
            key,
            payee,
            iban,
            category_id
        ],
    )?;
    Ok(())
}

/// Teach the engine every stored row (call after unlock). Rows are applied
/// oldest first, so when several rows share a normalized key — the audit had
/// sixteen for one airline — the newest decides. Returns the rows read.
pub fn load_learned_payees(conn: &Connection, engine: &CategorizationEngine) -> Result<usize> {
    let rows = all_rows(conn)?;
    let mut loaded = 0;
    for row in &rows {
        let (payee, iban) = (row.payee_key(), row.iban_key());
        if payee.is_none() && iban.is_none() {
            continue;
        }
        engine.learn_from_user(payee.as_deref(), iban.as_deref(), &row.category_id);
        loaded += 1;
    }
    Ok(loaded)
}

/// A learned rule as listed in the UI: one entry per normalized payee + IBAN.
#[derive(Debug, Clone)]
pub struct LearnedPayeeRow {
    pub id: String,
    /// The normalized key that is actually matched ("austrian air"), not the raw text.
    pub normalized_payee: Option<String>,
    pub original_payee: Option<String>,
    pub counterparty_iban: Option<String>,
    pub category_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// The learned rules for the management UI: rows sharing a normalized key are
/// shown once (the newest), newest rule first.
pub fn list_learned_payees(conn: &Connection) -> Result<Vec<LearnedPayeeRow>> {
    // Each row's group is derived once and looked up in a map: re-deriving the group of every
    // earlier row for every new row (normalising its text again each time) made this quadratic,
    // 0.28 s for 519 rows and 2.8 s for 5,000 (RUL-01).
    let mut newest: Vec<(StoredRow, Option<String>)> = Vec::new();
    let mut slot_of: HashMap<Group, usize> = HashMap::new();
    for row in all_rows(conn)? {
        let key = row.payee_key();
        let group = row.group_with(key.clone(), row.iban_key());
        match slot_of.get(&group) {
            // Rows arrive oldest first: a later row replaces the earlier one.
            Some(&slot) => newest[slot] = (row, key),
            None => {
                slot_of.insert(group, newest.len());
                newest.push((row, key));
            }
        }
    }
    newest.sort_by_key(|(row, _)| std::cmp::Reverse((row.updated_at, row.created_at)));
    Ok(newest
        .into_iter()
        .map(|(row, key)| LearnedPayeeRow {
            id: row.id,
            normalized_payee: key,
            original_payee: row.original_payee,
            counterparty_iban: row.counterparty_iban,
            category_id: row.category_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
        .collect())
}

/// The ids of every row in the same group as `id` (including `id`), plus the
/// key the engine knows the group by. `None` when `id` does not exist.
fn group_of(conn: &Connection, id: &str) -> Result<Option<(Vec<String>, LearnedKey)>> {
    let rows = all_rows(conn)?;
    let Some(target) = rows.iter().find(|r| r.id == id) else {
        return Ok(None);
    };
    let group = target.group();
    let ids = rows
        .iter()
        .filter(|r| r.group() == group)
        .map(|r| r.id.clone())
        .collect();
    Ok(Some((
        ids,
        LearnedKey {
            payee: target.payee_key(),
            iban: target.iban_key(),
        },
    )))
}

/// Delete a listed rule — every row that shares its normalized key, or the
/// stale ones would bring it back at the next start. Returns the key to
/// forget in the engine, or `None` when `id` is unknown.
pub fn delete_learned_payee(conn: &Connection, id: &str) -> Result<Option<LearnedKey>> {
    let Some((ids, key)) = group_of(conn, id)? else {
        return Ok(None);
    };
    for row_id in ids {
        conn.execute("DELETE FROM learned_payees WHERE id = ?1", [&row_id])?;
    }
    Ok(Some(key))
}

/// Change the category of a listed rule (all rows that share its key).
/// Returns the key to relearn in the engine, or `None` when `id` is unknown.
pub fn set_learned_payee_category(
    conn: &Connection,
    id: &str,
    category_id: &str,
) -> Result<Option<LearnedKey>> {
    if !category_exists(conn, category_id)? {
        return Err(AppError::NotFound(format!(
            "Category not found: {category_id}"
        )));
    }
    let Some((ids, key)) = group_of(conn, id)? else {
        return Ok(None);
    };
    for row_id in ids {
        conn.execute(
            "UPDATE learned_payees SET category_id = ?1, updated_at = unixepoch() WHERE id = ?2",
            params![category_id, row_id],
        )?;
    }
    Ok(Some(key))
}

/// Remove the rows for a payee text and/or IBAN (the `forget_payee` command):
/// everything that normalizes to the same key goes, whatever reference number
/// it was learned with.
pub fn forget_learned_payee(
    conn: &Connection,
    payee: Option<&str>,
    counterparty_iban: Option<&str>,
) -> Result<()> {
    let key = payee.and_then(payee_key);
    let iban = counterparty_iban.and_then(iban_key);
    for row in all_rows(conn)? {
        if row.payee_key() == key && row.iban_key() == iban {
            conn.execute("DELETE FROM learned_payees WHERE id = ?1", [&row.id])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::categorization::tokenizer::simple_normalize;
    use crate::services::categorization::types::TransactionInput;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE learned_payees (
                id TEXT PRIMARY KEY,
                normalized_payee TEXT,
                original_payee TEXT,
                counterparty_iban TEXT,
                category_id TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE UNIQUE INDEX idx_learned_payees_composite
                ON learned_payees(normalized_payee, counterparty_iban);
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY);
            "#,
        )
        .expect("schema");
        conn
    }

    /// RUL-01: how long the rules list takes for a given number of learned rows.
    /// `cargo test --lib learn::tests::benchmark_rules_list -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark: prints timings"]
    fn benchmark_rules_list() {
        for rows in [519usize, 2_000, 5_000] {
            let conn = setup_test_db();
            conn.execute_batch("BEGIN").unwrap();
            for i in 0..rows {
                conn.execute(
                    "INSERT INTO learned_payees (id, normalized_payee, original_payee, counterparty_iban, category_id, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'cat_food', ?5)",
                    params![
                        format!("id{i}"),
                        format!("payee number {i}"),
                        format!("PAYEE NUMBER {i} S.R.O."),
                        (i % 5 == 0).then(|| format!("CZ65080000001920001453{:02}", i % 100)),
                        i as i64
                    ],
                )
                .unwrap();
            }
            conn.execute_batch("COMMIT").unwrap();
            let t = std::time::Instant::now();
            let listed = list_learned_payees(&conn).unwrap();
            println!(
                "{rows} learned rows -> {} listed in {:?}",
                listed.len(),
                t.elapsed()
            );
        }
    }

    #[test]
    fn learn_payee_persists_and_teaches_the_engine() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();

        learn_payee(&conn, &engine, Some("Uber Eats"), None, "cat_dining").unwrap();

        // Engine answers immediately...
        let tx = TransactionInput::new("t1".into(), None, Some("Uber Eats".into()), -100.0);
        assert_eq!(engine.categorize(&tx).category_id(), Some("cat_dining"));

        // ...and the mapping is persisted.
        let (normalized, category): (String, String) = conn
            .query_row(
                "SELECT normalized_payee, category_id FROM learned_payees",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("row");
        assert_eq!(normalized, "uber eats");
        assert_eq!(category, "cat_dining");
    }

    #[test]
    fn learn_payee_replaces_the_existing_mapping() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();

        learn_payee(&conn, &engine, Some("Albert"), None, "cat_groceries").unwrap();
        learn_payee(&conn, &engine, Some("Albert"), None, "cat_dining").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM learned_payees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let category: String = conn
            .query_row("SELECT category_id FROM learned_payees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(category, "cat_dining");
    }

    #[test]
    fn learn_payee_without_identity_is_a_noop() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();

        learn_payee(&conn, &engine, None, None, "cat_dining").unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM learned_payees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ==================== Reference-number normalization (RUL-02) ====================

    fn insert_raw_row(
        conn: &Connection,
        id: &str,
        raw_payee: &str,
        category: &str,
        updated_at: i64,
    ) {
        // How rows looked before RUL-02: the key is simple_normalize(raw), the raw text is kept.
        conn.execute(
            "INSERT INTO learned_payees (id, normalized_payee, original_payee, counterparty_iban, category_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5)",
            rusqlite::params![id, simple_normalize(raw_payee), raw_payee, category, updated_at],
        )
        .unwrap();
    }

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM learned_payees", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn learning_every_flight_reference_keeps_a_single_row() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();

        for i in 0..16 {
            let payee = format!("AUSTRIAN AIR25721704836{:02}", 55 + i);
            learn_payee(&conn, &engine, Some(&payee), None, "cat_travel").unwrap();
        }

        assert_eq!(row_count(&conn), 1, "one learned rule for one airline");
        let key: String = conn
            .query_row("SELECT normalized_payee FROM learned_payees", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(key, "austrian air");
        // ...and a transaction with yet another reference hits it.
        let tx = TransactionInput::new("t".into(), None, Some("AUSTRIAN AIR9999".into()), -300.0);
        assert_eq!(engine.categorize(&tx).category_id(), Some("cat_travel"));
    }

    #[test]
    fn learning_replaces_older_rows_keyed_by_the_raw_text() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();
        insert_raw_row(&conn, "old1", "AUSTRIAN AIR2572170483655", "cat_other", 100);
        insert_raw_row(&conn, "old2", "AUSTRIAN AIR2572170483656", "cat_other", 200);
        insert_raw_row(&conn, "keep", "Albert", "cat_groceries", 300);

        learn_payee(&conn, &engine, Some("AUSTRIAN AIR9999"), None, "cat_travel").unwrap();

        assert_eq!(row_count(&conn), 2, "both stale airline rows are gone");
        let airline: String = conn
            .query_row(
                "SELECT category_id FROM learned_payees WHERE normalized_payee = 'austrian air'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(airline, "cat_travel");
    }

    #[test]
    fn engine_load_collapses_old_rows_and_the_newest_wins() {
        let conn = setup_test_db();
        // The audit's situation: many rows for one airline, learned at different times.
        for i in 0..16 {
            insert_raw_row(
                &conn,
                &format!("r{i}"),
                &format!("AUSTRIAN AIR25721704836{:02}", 55 + i),
                if i == 15 { "cat_travel" } else { "cat_other" },
                1000 + i,
            );
        }
        insert_raw_row(&conn, "dr", "DR. MAX 03", "cat_health", 2000);

        let engine = CategorizationEngine::new_empty();
        let loaded = load_learned_payees(&conn, &engine).unwrap();
        assert_eq!(loaded, 17, "every stored row is read");

        assert_eq!(
            engine.stats().learned_payees,
            2,
            "sixteen rows, one key — plus Dr. Max"
        );
        let airline =
            TransactionInput::new("t".into(), None, Some("AUSTRIAN AIR9999".into()), -300.0);
        assert_eq!(
            engine.categorize(&airline).category_id(),
            Some("cat_travel"),
            "the newest row decides"
        );
        // The punctuation of the raw text is kept in the key, so the next branch matches.
        let pharmacy = TransactionInput::new("t".into(), None, Some("DR. MAX 07".into()), -90.0);
        assert_eq!(
            engine.categorize(&pharmacy).category_id(),
            Some("cat_health")
        );
    }

    #[test]
    fn list_shows_the_normalized_key_once_per_payee_newest_first() {
        let conn = setup_test_db();
        insert_raw_row(&conn, "a1", "AUSTRIAN AIR2572170483655", "cat_other", 100);
        insert_raw_row(&conn, "a2", "AUSTRIAN AIR2572170483656", "cat_travel", 300);
        insert_raw_row(&conn, "d1", "DR. MAX 03", "cat_health", 200);

        let list = list_learned_payees(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0].id, "a2",
            "newest row of the group, newest group first"
        );
        assert_eq!(list[0].normalized_payee.as_deref(), Some("austrian air"));
        assert_eq!(list[0].category_id, "cat_travel");
        assert_eq!(
            list[0].original_payee.as_deref(),
            Some("AUSTRIAN AIR2572170483656")
        );
        assert_eq!(list[1].normalized_payee.as_deref(), Some("dr. max"));
    }

    #[test]
    fn list_keeps_rows_without_a_usable_key_apart() {
        // A payee that is only a reference number identifies nobody: each such row is its own
        // group, while rows that do share a key still collapse.
        let conn = setup_test_db();
        insert_raw_row(&conn, "n1", "12", "cat_other", 100);
        insert_raw_row(&conn, "n2", "34", "cat_other", 200);
        insert_raw_row(&conn, "k1", "Albert 1", "cat_groceries", 300);
        insert_raw_row(&conn, "k2", "Albert 2", "cat_groceries", 400);

        let ids: Vec<String> = list_learned_payees(&conn)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["k2", "n2", "n1"], "newest first, one Albert row");
    }

    #[test]
    fn deleting_one_listed_rule_removes_the_whole_group_from_the_database_and_engine() {
        let conn = setup_test_db();
        insert_raw_row(&conn, "a1", "AUSTRIAN AIR2572170483655", "cat_travel", 100);
        insert_raw_row(&conn, "a2", "AUSTRIAN AIR2572170483656", "cat_travel", 300);
        insert_raw_row(&conn, "keep", "Albert", "cat_groceries", 200);
        let engine = CategorizationEngine::new_empty();
        load_learned_payees(&conn, &engine).unwrap();

        let forgotten = delete_learned_payee(&conn, "a2")
            .unwrap()
            .expect("known row");
        engine.forget_payee(forgotten.payee.as_deref(), forgotten.iban.as_deref());

        assert_eq!(
            row_count(&conn),
            1,
            "stale rows must not resurrect the rule at next start"
        );
        let tx = TransactionInput::new("t".into(), None, Some("AUSTRIAN AIR9999".into()), -300.0);
        assert!(!engine.categorize(&tx).has_category());
        assert!(delete_learned_payee(&conn, "ghost").unwrap().is_none());
    }

    #[test]
    fn changing_a_listed_rules_category_changes_the_whole_group() {
        let conn = setup_test_db();
        insert_raw_row(&conn, "a1", "AUSTRIAN AIR2572170483655", "cat_other", 100);
        insert_raw_row(&conn, "a2", "AUSTRIAN AIR2572170483656", "cat_other", 300);
        conn.execute(
            "INSERT INTO transaction_categories (id) VALUES ('cat_travel'), ('cat_other')",
            [],
        )
        .unwrap();

        let key = set_learned_payee_category(&conn, "a2", "cat_travel")
            .unwrap()
            .expect("known row");
        assert_eq!(key.payee.as_deref(), Some("austrian air"));

        let travel: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM learned_payees WHERE category_id = 'cat_travel'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(travel, 2);
        assert!(matches!(
            set_learned_payee_category(&conn, "a2", "ghost").unwrap_err(),
            crate::error::AppError::NotFound(_)
        ));
    }

    #[test]
    fn forgetting_by_payee_text_removes_rows_whatever_reference_they_carry() {
        let conn = setup_test_db();
        insert_raw_row(&conn, "a1", "AUSTRIAN AIR2572170483655", "cat_travel", 100);
        insert_raw_row(&conn, "keep", "Albert", "cat_groceries", 200);

        forget_learned_payee(&conn, Some("Austrian Air 5555"), None).unwrap();

        assert_eq!(row_count(&conn), 1);
    }

    // ==================== IBAN normalization ====================

    #[test]
    fn learned_ibans_are_stored_normalized_and_replace_their_spaced_twin() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();
        conn.execute(
            "INSERT INTO learned_payees (id, normalized_payee, original_payee, counterparty_iban, category_id)
             VALUES ('old', NULL, NULL, 'DE89 3704 0044 0532 0130 00', 'cat_other')",
            [],
        )
        .unwrap();

        learn_payee(
            &conn,
            &engine,
            None,
            Some("de89370400440532013000"),
            "cat_savings",
        )
        .unwrap();

        assert_eq!(row_count(&conn), 1);
        let (iban, category): (String, String) = conn
            .query_row(
                "SELECT counterparty_iban, category_id FROM learned_payees",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(iban, "DE89370400440532013000");
        assert_eq!(category, "cat_savings");
    }
}
