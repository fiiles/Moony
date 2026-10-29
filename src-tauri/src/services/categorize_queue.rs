//! The categorize queue of the phone app (spec 2026-10-17 §8.5): the booked payments that still
//! have no category, newest first, each with the category the app would suggest, and the 30-day
//! snooze behind "Bez kategorie, nechat".
//!
//! The suggestion is read-only here: an AI client's pending suggestion wins, then the
//! categorization engine (own account, custom rule, learned payee, locale pack). Nothing is
//! written until the user accepts one through the usual categorize commands.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::bank_transactions::BankTransaction;
use crate::models::categorization::{
    CategorizeQueue, CategorizeQueueRow, CategorySuggestion, SuggestionSource,
};
use crate::services::bank_history::{transaction_from_row, TRANSACTION_COLUMNS};
use crate::services::categorization::recategorize::input_from_transaction;
use crate::services::categorization::{CategorizationEngine, Layer};

/// The most payments one read returns; `total` still counts all of them.
pub const QUEUE_LIMIT: usize = 500;

const SECONDS_PER_DAY: i64 = 86_400;

/// Booked payments without a category, for one account (`?1`) or all of them (`?1` NULL).
const UNCATEGORIZED: &str =
    "status = 'booked' AND category_id IS NULL AND (?1 IS NULL OR bank_account_id = ?1)";

/// The queue as of `now` (unix seconds), for one account or all of them.
///
/// A payment whose snooze (`?2` is `now`) has not run out is held back and only counted in
/// `snoozed`. `total` counts every waiting payment, `rows` the newest [`QUEUE_LIMIT`] of them.
pub fn list(
    conn: &Connection,
    engine: &CategorizationEngine,
    account_id: Option<&str>,
    now: i64,
) -> Result<CategorizeQueue> {
    let waiting = format!(
        "{UNCATEGORIZED} AND (categorize_snoozed_until IS NULL OR categorize_snoozed_until <= ?2)"
    );
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM bank_transactions WHERE {waiting}"),
        params![account_id, now],
        |row| row.get(0),
    )?;
    let snoozed: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM bank_transactions
             WHERE {UNCATEGORIZED} AND categorize_snoozed_until > ?2"
        ),
        params![account_id, now],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare(&format!(
        "SELECT {TRANSACTION_COLUMNS} FROM bank_transactions WHERE {waiting}
         ORDER BY booking_date DESC, created_at DESC, id DESC LIMIT ?3"
    ))?;
    let transactions = stmt
        .query_map(
            params![account_id, now, QUEUE_LIMIT as i64],
            transaction_from_row,
        )?
        .collect::<rusqlite::Result<Vec<BankTransaction>>>()?;

    let categories = category_ids(conn)?;
    let rows = transactions
        .into_iter()
        .map(|transaction| {
            let suggestion = suggest(engine, &categories, &transaction);
            CategorizeQueueRow {
                transaction,
                suggestion,
            }
        })
        .collect();
    Ok(CategorizeQueue {
        rows,
        total,
        snoozed,
    })
}

/// When a snooze of `days` days that starts at `now` (unix seconds) ends.
pub fn snooze_until(now: i64, days: i64) -> i64 {
    now.saturating_add(days.saturating_mul(SECONDS_PER_DAY))
}

/// Hold the uncategorized payments among `ids` back from the queue until `until` (unix seconds).
/// Returns how many payments were held back; a payment that has a category is left alone, and an
/// id named twice counts once.
pub fn snooze(conn: &Connection, ids: &[String], until: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut held = 0;
    {
        let mut stmt = tx.prepare(
            "UPDATE bank_transactions SET categorize_snoozed_until = ?1
             WHERE id = ?2 AND category_id IS NULL",
        )?;
        let mut seen = HashSet::new();
        for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
            held += stmt.execute(params![until, id])?;
        }
    }
    tx.commit()?;
    Ok(held)
}

/// Every category that exists, to drop a suggestion whose category was deleted.
fn category_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM transaction_categories")?;
    let ids = stmt
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<HashSet<String>>>()?;
    Ok(ids)
}

/// The category to offer for a payment: the one an AI client left on it, else the engine's
/// answer. A category that no longer exists is no suggestion.
fn suggest(
    engine: &CategorizationEngine,
    categories: &HashSet<String>,
    transaction: &BankTransaction,
) -> Option<CategorySuggestion> {
    let (category_id, source) = match &transaction.suggested_category_id {
        Some(category_id) => (category_id.clone(), SuggestionSource::Mcp),
        None => engine_suggestion(engine, transaction)?,
    };
    categories
        .contains(&category_id)
        .then_some(CategorySuggestion {
            category_id,
            source,
        })
}

/// What the categorization engine would give a payment, and which layer of it decided.
fn engine_suggestion(
    engine: &CategorizationEngine,
    transaction: &BankTransaction,
) -> Option<(String, SuggestionSource)> {
    let (layer, result) = engine.explain(&input_from_transaction(transaction));
    let source = match layer {
        Layer::OwnAccount | Layer::Custom => SuggestionSource::Rule,
        Layer::Learned => SuggestionSource::Learned,
        Layer::Pack => SuggestionSource::Pack,
        Layer::None => return None,
    };
    Some((result.category_id()?.to_string(), source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::categorization::{CategorySuggestion, SuggestionSource};
    use crate::services::categorization::{CategorizationRule, RuleType};

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE csv_import_batches (id TEXT PRIMARY KEY, file_name TEXT);
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                transaction_id TEXT,
                tx_type TEXT NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                description TEXT,
                counterparty_name TEXT,
                counterparty_iban TEXT,
                booking_date INTEGER NOT NULL,
                value_date INTEGER,
                category_id TEXT,
                merchant_category_code TEXT,
                remittance_info TEXT,
                status TEXT NOT NULL DEFAULT 'booked',
                data_source TEXT NOT NULL DEFAULT 'manual',
                created_at INTEGER NOT NULL DEFAULT 0,
                import_batch_id TEXT,
                categorization_source TEXT,
                suggested_category_id TEXT,
                flow_kind TEXT,
                counterparty_bank_code TEXT,
                counterparty_country TEXT,
                import_line INTEGER,
                row_balance TEXT,
                categorize_snoozed_until INTEGER,
                spending_plan_id TEXT
            );
            INSERT INTO transaction_categories (id, name) VALUES
                ('cat_groceries', 'Groceries'), ('cat_dining', 'Dining'),
                ('cat_transport', 'Transport'), ('cat_health', 'Health'),
                ('cat_internal_transfers', 'Internal Transfers');
            "#,
        )
        .expect("schema");
        conn
    }

    /// A booked debit without a category; the builder methods below change one thing at a time.
    struct Tx {
        id: &'static str,
        account: &'static str,
        counterparty: Option<&'static str>,
        booking_date: i64,
        created_at: i64,
        status: &'static str,
        category: Option<&'static str>,
        suggested: Option<&'static str>,
        snoozed_until: Option<i64>,
        iban: Option<&'static str>,
        tx_type: &'static str,
    }

    fn tx(id: &'static str) -> Tx {
        Tx {
            id,
            account: "acc-1",
            counterparty: None,
            booking_date: NOW - 10 * DAY,
            created_at: 0,
            status: "booked",
            category: None,
            suggested: None,
            snoozed_until: None,
            iban: None,
            tx_type: "debit",
        }
    }

    impl Tx {
        fn account(mut self, account: &'static str) -> Self {
            self.account = account;
            self
        }
        fn payee(mut self, payee: &'static str) -> Self {
            self.counterparty = Some(payee);
            self
        }
        fn booked_on(mut self, booking_date: i64, created_at: i64) -> Self {
            self.booking_date = booking_date;
            self.created_at = created_at;
            self
        }
        fn pending(mut self) -> Self {
            self.status = "pending";
            self
        }
        fn categorized(mut self, category: &'static str) -> Self {
            self.category = Some(category);
            self
        }
        fn suggested(mut self, category: &'static str) -> Self {
            self.suggested = Some(category);
            self
        }
        fn snoozed_until(mut self, until: i64) -> Self {
            self.snoozed_until = Some(until);
            self
        }
        fn credit(mut self) -> Self {
            self.tx_type = "credit";
            self
        }
        fn iban(mut self, iban: &'static str) -> Self {
            self.iban = Some(iban);
            self
        }
        fn insert(self, conn: &Connection) {
            conn.execute(
                "INSERT INTO bank_transactions
                    (id, bank_account_id, tx_type, amount, currency, description,
                     counterparty_name, counterparty_iban, booking_date, status, created_at,
                     category_id, suggested_category_id, categorize_snoozed_until)
                 VALUES (?1, ?2, ?11, '120.50', 'CZK', 'Card payment', ?3, ?4, ?5, ?6, ?7,
                         ?8, ?9, ?10)",
                rusqlite::params![
                    self.id,
                    self.account,
                    self.counterparty,
                    self.iban,
                    self.booking_date,
                    self.status,
                    self.created_at,
                    self.category,
                    self.suggested,
                    self.snoozed_until,
                    self.tx_type,
                ],
            )
            .expect("insert transaction");
        }
    }

    fn engine() -> CategorizationEngine {
        CategorizationEngine::new_empty()
    }

    fn rule(pattern: &str, category: &str) -> CategorizationRule {
        CategorizationRule::new(
            format!("rule-{pattern}"),
            pattern.to_string(),
            RuleType::Contains,
            pattern.to_string(),
            category.to_string(),
        )
    }

    fn ids(queue: &CategorizeQueue) -> Vec<&str> {
        queue
            .rows
            .iter()
            .map(|r| r.transaction.id.as_str())
            .collect()
    }

    fn suggestion(queue: &CategorizeQueue, id: &str) -> Option<CategorySuggestion> {
        queue
            .rows
            .iter()
            .find(|r| r.transaction.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the queue"))
            .suggestion
            .clone()
    }

    fn pending_snooze(conn: &Connection, id: &str) -> Option<i64> {
        conn.query_row(
            "SELECT categorize_snoozed_until FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("row")
    }

    #[test]
    fn lists_booked_uncategorized_newest_first() {
        let conn = setup_test_db();
        tx("old").booked_on(NOW - 30 * DAY, 5).insert(&conn);
        tx("new-late").booked_on(NOW - DAY, 9).insert(&conn);
        tx("new-early").booked_on(NOW - DAY, 3).insert(&conn);
        tx("middle").booked_on(NOW - 10 * DAY, 1).insert(&conn);

        let queue = list(&conn, &engine(), None, NOW).expect("queue");

        assert_eq!(ids(&queue), ["new-late", "new-early", "middle", "old"]);
        assert_eq!(queue.total, 4);
        assert_eq!(queue.snoozed, 0);
    }

    #[test]
    fn excludes_pending_categorized_and_snoozed_until_it_expires() {
        let conn = setup_test_db();
        tx("waiting").insert(&conn);
        tx("pending").pending().insert(&conn);
        tx("done").categorized("cat_groceries").insert(&conn);
        tx("held").snoozed_until(NOW + DAY).insert(&conn);
        tx("expired").snoozed_until(NOW - DAY).insert(&conn);
        tx("expires-now").snoozed_until(NOW).insert(&conn);

        let queue = list(&conn, &engine(), None, NOW).expect("queue");
        let mut listed = ids(&queue);
        listed.sort_unstable();
        assert_eq!(listed, ["expired", "expires-now", "waiting"]);
        assert_eq!(queue.total, 3);
        assert_eq!(queue.snoozed, 1);

        // A day later the hold on `held` has run out and it is back in the queue.
        let later = list(&conn, &engine(), None, NOW + DAY + 1).expect("queue");
        assert_eq!(later.total, 4);
        assert_eq!(later.snoozed, 0);
        assert!(ids(&later).contains(&"held"));
    }

    #[test]
    fn snoozed_counts_only_uncategorized_payments() {
        let conn = setup_test_db();
        tx("held").snoozed_until(NOW + DAY).insert(&conn);
        // A payment categorized while its snooze was running no longer counts as held back.
        tx("done")
            .categorized("cat_groceries")
            .snoozed_until(NOW + DAY)
            .insert(&conn);

        let queue = list(&conn, &engine(), None, NOW).expect("queue");
        assert_eq!(queue.snoozed, 1);
        assert_eq!(queue.total, 0);
    }

    #[test]
    fn filters_by_account() {
        let conn = setup_test_db();
        tx("a1").account("acc-1").insert(&conn);
        tx("a2").account("acc-2").insert(&conn);
        tx("a2-held")
            .account("acc-2")
            .snoozed_until(NOW + DAY)
            .insert(&conn);

        let one = list(&conn, &engine(), Some("acc-2"), NOW).expect("queue");
        assert_eq!(ids(&one), ["a2"]);
        assert_eq!(one.total, 1);
        assert_eq!(one.snoozed, 1);

        let none = list(&conn, &engine(), Some("acc-3"), NOW).expect("queue");
        assert!(none.rows.is_empty());
        assert_eq!((none.total, none.snoozed), (0, 0));

        let all = list(&conn, &engine(), None, NOW).expect("queue");
        assert_eq!(all.total, 2);
        assert_eq!(all.snoozed, 1);
    }

    #[test]
    fn mcp_suggestion_wins_over_the_engine() {
        let conn = setup_test_db();
        tx("t1")
            .payee("Albert")
            .suggested("cat_dining")
            .insert(&conn);
        let engine = engine();
        engine.update_custom_rules(vec![rule("Albert", "cat_groceries")]);

        let queue = list(&conn, &engine, None, NOW).expect("queue");

        assert_eq!(
            suggestion(&queue, "t1"),
            Some(CategorySuggestion {
                category_id: "cat_dining".into(),
                source: SuggestionSource::Mcp,
            })
        );
    }

    #[test]
    fn engine_layers_map_to_sources() {
        let conn = setup_test_db();
        tx("learned").payee("Uber Eats").insert(&conn);
        tx("custom").payee("Albert Hypermarket").insert(&conn);
        tx("pack").payee("Dr. Max Pharmacy").insert(&conn);
        tx("own")
            .payee("My savings")
            .iban("CZ6508000000192000145399")
            .insert(&conn);
        tx("unknown").payee("Nobody knows").insert(&conn);

        let engine = engine();
        engine.learn_from_user_simple("Uber Eats", "cat_dining");
        engine.update_custom_rules(vec![rule("Albert", "cat_groceries")]);
        engine.update_rules(vec![rule("Pharmacy", "cat_health")]);
        engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);

        let queue = list(&conn, &engine, None, NOW).expect("queue");

        let expect = |category: &str, source| {
            Some(CategorySuggestion {
                category_id: category.into(),
                source,
            })
        };
        assert_eq!(
            suggestion(&queue, "learned"),
            expect("cat_dining", SuggestionSource::Learned)
        );
        assert_eq!(
            suggestion(&queue, "custom"),
            expect("cat_groceries", SuggestionSource::Rule)
        );
        assert_eq!(
            suggestion(&queue, "pack"),
            expect("cat_health", SuggestionSource::Pack)
        );
        assert_eq!(
            suggestion(&queue, "own"),
            expect("cat_internal_transfers", SuggestionSource::Rule)
        );
        assert_eq!(suggestion(&queue, "unknown"), None);
    }

    #[test]
    fn a_custom_rule_outranks_a_learned_payee_like_the_engine_does() {
        let conn = setup_test_db();
        tx("t1").payee("Albert").insert(&conn);
        let engine = engine();
        engine.learn_from_user_simple("Albert", "cat_dining");
        engine.update_custom_rules(vec![rule("Albert", "cat_groceries")]);

        let queue = list(&conn, &engine, None, NOW).expect("queue");

        assert_eq!(
            suggestion(&queue, "t1"),
            Some(CategorySuggestion {
                category_id: "cat_groceries".into(),
                source: SuggestionSource::Rule,
            })
        );
    }

    #[test]
    fn the_engine_sees_the_direction_of_a_payment() {
        let conn = setup_test_db();
        tx("in").credit().insert(&conn);
        tx("out").insert(&conn);
        let engine = engine();
        let mut incoming = rule("incoming", "cat_groceries");
        incoming.rule_type = RuleType::IsCredit;
        engine.update_custom_rules(vec![incoming]);

        let queue = list(&conn, &engine, None, NOW).expect("queue");

        assert_eq!(
            suggestion(&queue, "in"),
            Some(CategorySuggestion {
                category_id: "cat_groceries".into(),
                source: SuggestionSource::Rule,
            })
        );
        assert_eq!(suggestion(&queue, "out"), None);
    }

    #[test]
    fn drops_a_suggestion_for_a_deleted_category() {
        let conn = setup_test_db();
        tx("engine").payee("Albert").insert(&conn);
        tx("mcp").suggested("cat_deleted").insert(&conn);
        let engine = engine();
        engine.update_custom_rules(vec![rule("Albert", "cat_gone")]);

        let queue = list(&conn, &engine, None, NOW).expect("queue");

        assert_eq!(queue.rows.len(), 2);
        assert_eq!(suggestion(&queue, "engine"), None);
        assert_eq!(suggestion(&queue, "mcp"), None);
    }

    #[test]
    fn caps_rows_but_counts_total() {
        let conn = setup_test_db();
        let tx_rows = QUEUE_LIMIT + 7;
        conn.execute_batch(&format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {tx_rows})
             INSERT INTO bank_transactions
                (id, bank_account_id, tx_type, amount, currency, booking_date, created_at)
             SELECT 'bulk-' || i, 'acc-1', 'debit', '1', 'CZK', {NOW} - i * 60, 0 FROM n;
             INSERT INTO bank_transactions
                (id, bank_account_id, tx_type, amount, currency, booking_date, categorize_snoozed_until)
             VALUES ('held', 'acc-1', 'debit', '1', 'CZK', {NOW}, {NOW} + 1000);"
        ))
        .expect("bulk rows");

        let queue = list(&conn, &engine(), None, NOW).expect("queue");

        assert_eq!(queue.rows.len(), QUEUE_LIMIT);
        assert_eq!(queue.total, tx_rows as i64);
        assert_eq!(queue.snoozed, 1);
        // The newest are the ones kept.
        assert_eq!(queue.rows[0].transaction.id, "bulk-1");
        assert_eq!(
            queue.rows[QUEUE_LIMIT - 1].transaction.id,
            format!("bulk-{QUEUE_LIMIT}")
        );
    }

    #[test]
    fn the_wire_row_carries_the_ledger_fields() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO csv_import_batches (id, file_name) VALUES ('b1', 'march.csv')",
            [],
        )
        .expect("batch");
        tx("t1").payee("Albert").insert(&conn);
        conn.execute(
            "UPDATE bank_transactions SET import_batch_id = 'b1', flow_kind = 'expense' WHERE id = 't1'",
            [],
        )
        .expect("batch link");

        let queue = list(&conn, &engine(), None, NOW).expect("queue");
        let t = &queue.rows[0].transaction;

        assert_eq!(t.amount, "120.50");
        assert_eq!(t.currency, "CZK");
        assert_eq!(t.counterparty_name.as_deref(), Some("Albert"));
        assert_eq!(t.import_file_name.as_deref(), Some("march.csv"));
        assert_eq!(t.status, "booked");
        assert_eq!(t.category_id, None);
    }

    #[test]
    fn a_snooze_ends_whole_days_after_it_starts() {
        assert_eq!(snooze_until(NOW, 1), NOW + DAY);
        assert_eq!(snooze_until(NOW, 30), NOW + 30 * DAY);
        assert_eq!(snooze_until(i64::MAX, 365), i64::MAX);
    }

    #[test]
    fn snooze_holds_uncategorized_rows_back() {
        let conn = setup_test_db();
        tx("a").insert(&conn);
        tx("b").insert(&conn);
        tx("untouched").insert(&conn);

        let until = NOW + 30 * DAY;
        let ids_to_snooze = vec!["a".to_string(), "b".to_string()];
        assert_eq!(snooze(&conn, &ids_to_snooze, until).expect("snooze"), 2);

        assert_eq!(pending_snooze(&conn, "a"), Some(until));
        assert_eq!(pending_snooze(&conn, "b"), Some(until));
        assert_eq!(pending_snooze(&conn, "untouched"), None);
        let queue = list(&conn, &engine(), None, NOW).expect("queue");
        assert_eq!(ids(&queue), ["untouched"]);
        assert_eq!(queue.snoozed, 2);
    }

    #[test]
    fn snooze_skips_categorized_rows() {
        let conn = setup_test_db();
        tx("open").insert(&conn);
        tx("done").categorized("cat_groceries").insert(&conn);

        let ids_to_snooze = vec![
            "open".to_string(),
            "done".to_string(),
            "missing".to_string(),
        ];
        let touched = snooze(&conn, &ids_to_snooze, NOW + DAY).expect("snooze");

        assert_eq!(touched, 1);
        assert_eq!(pending_snooze(&conn, "open"), Some(NOW + DAY));
        assert_eq!(pending_snooze(&conn, "done"), None);
    }

    #[test]
    fn snooze_counts_a_repeated_id_once() {
        let conn = setup_test_db();
        tx("a").insert(&conn);

        let ids_to_snooze = vec!["a".to_string(), "a".to_string()];
        assert_eq!(snooze(&conn, &ids_to_snooze, NOW + DAY).expect("snooze"), 1);
    }

    #[test]
    fn snoozing_again_replaces_the_deadline() {
        let conn = setup_test_db();
        tx("a").snoozed_until(NOW + DAY).insert(&conn);

        snooze(&conn, &["a".to_string()], NOW + 30 * DAY).expect("snooze");

        assert_eq!(pending_snooze(&conn, "a"), Some(NOW + 30 * DAY));
    }
}
