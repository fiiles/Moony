//! Recategorizing other payments when a rule is rewritten (spec
//! 2026-10-10-rule-override-recategorize-history): which payments a learned
//! rule or a custom rule covers today, how their current categories break
//! down, giving them the rule's category, and taking that back.
//!
//! Coverage is replayed per rule, not through the waterfall: the question is
//! "which payments does THIS rule reach", the same reading `hits.rs` uses.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use super::apply::{SOURCE_EXACT_MATCH, SOURCE_RULE};
use super::engine::Layer;
use super::hits::{engine_for, learned_matches};
use super::learn::{self, LearnedPayeeRow};
use super::tokenizer::normalize_payee_key;
use super::types::{
    CategorizationResult, CategorizationRule, CategorizationSource, RuleType, TransactionInput,
};
use super::CategorizationEngine;
use crate::error::{AppError, Result};
use crate::models::bank_transactions::BankTransaction;
use crate::models::categorization::{
    CategoryCount, ChangedRow, GoverningRule, RecategorizationPreview,
};
use crate::services::bank_accounts::category_exists;
use crate::services::financial_semantics::{canonical_structured_identifier, valid_iban};

/// Which rule's coverage is asked about.
pub enum Matcher {
    /// A learned rule, stored or about to be stored, matched like the learned layer does.
    Learned(LearnedPayeeRow),
    /// A custom rule, matched like the custom layer does (every account, both directions).
    Custom(CategorizationRule),
}

/// One payment a matcher covers, with what the row holds today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedRow {
    pub id: String,
    pub category_id: Option<String>,
    pub source: Option<String>,
}

struct Candidate {
    input: TransactionInput,
    category_id: Option<String>,
    source: Option<String>,
}

const CANDIDATE_COLUMNS: &str = "id, description, counterparty_name, counterparty_iban, counterparty_bank_code, counterparty_country, amount, tx_type, bank_account_id, category_id, categorization_source";

/// The engine's signed amount and direction of a stored payment: the amount is kept positive as
/// TEXT and the direction is `tx_type`. No rule matches on the amount, so a text that does not
/// parse reads as 0 here rather than failing the whole read.
fn engine_amount(amount: &str, tx_type: &str) -> (f64, bool) {
    let value: f64 = amount.trim().parse().unwrap_or(0.0);
    let is_credit = tx_type.eq_ignore_ascii_case("credit");
    (
        if is_credit { value.abs() } else { -value.abs() },
        is_credit,
    )
}

/// A ledger row as the engine sees it (the categorize queue reads its suggestions through this).
pub(crate) fn input_from_transaction(tx: &BankTransaction) -> TransactionInput {
    let (amount, is_credit) = engine_amount(&tx.amount, &tx.tx_type);
    TransactionInput {
        id: tx.id.clone(),
        description: tx.description.clone(),
        counterparty: tx.counterparty_name.clone(),
        counterparty_iban: tx.counterparty_iban.clone(),
        counterparty_bank_code: tx.counterparty_bank_code.clone(),
        counterparty_country: tx.counterparty_country.clone(),
        amount,
        is_credit,
        bank_account_id: Some(tx.bank_account_id.clone()),
    }
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Candidate> {
    let amount: String = row.get(6)?;
    let tx_type: String = row.get(7)?;
    let (amount, is_credit) = engine_amount(&amount, &tx_type);
    Ok(Candidate {
        input: TransactionInput {
            id: row.get(0)?,
            description: row.get(1)?,
            counterparty: row.get(2)?,
            counterparty_iban: row.get(3)?,
            counterparty_bank_code: row.get(4)?,
            counterparty_country: row.get(5)?,
            amount,
            is_credit,
            bank_account_id: row.get(8)?,
        },
        category_id: row.get(9)?,
        source: row.get(10)?,
    })
}

/// Every transaction, or only one account's (a scoped learned rule never reaches another).
fn load_candidates(conn: &Connection, account: Option<&str>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    match account {
        Some(account) => {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CANDIDATE_COLUMNS} FROM bank_transactions WHERE bank_account_id = ?1"
            ))?;
            for row in stmt.query_map([account], candidate_from_row)? {
                out.push(row?);
            }
        }
        None => {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CANDIDATE_COLUMNS} FROM bank_transactions"
            ))?;
            for row in stmt.query_map([], candidate_from_row)? {
                out.push(row?);
            }
        }
    }
    Ok(out)
}

/// The transaction as the engine sees it, or `None` when the id is unknown.
pub fn load_transaction_input(conn: &Connection, id: &str) -> Result<Option<TransactionInput>> {
    let candidate = conn
        .query_row(
            &format!("SELECT {CANDIDATE_COLUMNS} FROM bank_transactions WHERE id = ?1"),
            [id],
            candidate_from_row,
        )
        .optional()?;
    Ok(candidate.map(|c| c.input))
}

/// The payments `matcher` covers today, `exclude` (the row being edited) left out.
/// All time, every status. Transfers between the user's own accounts are never
/// covered: that layer wins over every rule in the waterfall, so a rule never
/// reaches them either.
pub fn matching_rows(
    conn: &Connection,
    engine: &CategorizationEngine,
    matcher: &Matcher,
    exclude: Option<&str>,
) -> Result<Vec<MatchedRow>> {
    let candidates = match matcher {
        Matcher::Learned(row) => load_candidates(conn, row.bank_account_id.as_deref())?,
        Matcher::Custom(_) => load_candidates(conn, None)?,
    };
    let rule_engine = match matcher {
        Matcher::Custom(rule) => Some(engine_for(rule)),
        Matcher::Learned(_) => None,
    };
    Ok(candidates
        .into_iter()
        .filter(|c| exclude != Some(c.input.id.as_str()))
        .filter(|c| !engine.is_own_account(&c.input))
        .filter(|c| match matcher {
            Matcher::Learned(row) => learned_matches(row, &c.input),
            Matcher::Custom(_) => rule_engine
                .as_ref()
                .is_some_and(|e| e.apply(&c.input).is_some()),
        })
        .map(|c| MatchedRow {
            id: c.input.id,
            category_id: c.category_id,
            source: c.source,
        })
        .collect())
}

/// Current categories of `rows`, largest bucket first; `None` is the uncategorized bucket.
pub fn breakdown(rows: &[MatchedRow]) -> Vec<CategoryCount> {
    let mut counts: HashMap<Option<String>, usize> = HashMap::new();
    for row in rows {
        *counts.entry(row.category_id.clone()).or_default() += 1;
    }
    let mut out: Vec<CategoryCount> = counts
        .into_iter()
        .map(|(category_id, count)| CategoryCount { category_id, count })
        .collect();
    // Largest first; on a tie named categories come before "no category", then by id.
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| match (&a.category_id, &b.category_id) {
                (Some(x), Some(y)) => x.cmp(y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    out
}

/// Give `category_id` to every row whose category differs, stamping `source`
/// and clearing a pending suggestion. Returns what each changed row held before.
pub fn apply(
    conn: &Connection,
    rows: &[MatchedRow],
    category_id: &str,
    source: &str,
) -> Result<Vec<ChangedRow>> {
    if !category_exists(conn, category_id)? {
        return Err(AppError::NotFound(format!(
            "Category not found: {category_id}"
        )));
    }
    let mut changed = Vec::new();
    for row in rows
        .iter()
        .filter(|r| r.category_id.as_deref() != Some(category_id))
    {
        let updated = conn.execute(
            "UPDATE bank_transactions
             SET category_id = ?1, categorization_source = ?2, suggested_category_id = NULL
             WHERE id = ?3",
            params![category_id, source, row.id],
        )?;
        if updated > 0 {
            changed.push(ChangedRow {
                id: row.id.clone(),
                previous_category_id: row.category_id.clone(),
                previous_source: row.source.clone(),
            });
        }
    }
    Ok(changed)
}

/// Put back what `apply` changed. Rows deleted meanwhile are skipped; a
/// previous category deleted meanwhile becomes "no category", and the previous
/// source goes with it (provenance never outlives the category it described).
///
/// One transaction: a failure part-way leaves every row as it was, and each
/// distinct previous category is looked up once.
pub fn restore(conn: &Connection, rows: &[ChangedRow]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut known: HashMap<String, bool> = HashMap::new();
    let mut restored = 0;
    for row in rows {
        let category = match row.previous_category_id.as_deref() {
            Some(c) => {
                let exists = match known.get(c) {
                    Some(&exists) => exists,
                    None => {
                        let exists = category_exists(&tx, c)?;
                        known.insert(c.to_string(), exists);
                        exists
                    }
                };
                exists.then_some(c)
            }
            None => None,
        };
        let source = category.and(row.previous_source.as_deref());
        restored += tx.execute(
            "UPDATE bank_transactions SET category_id = ?1, categorization_source = ?2 WHERE id = ?3",
            params![category, source, row.id],
        )?;
    }
    tx.commit()?;
    Ok(restored)
}

/// The learned rule a remembered correction on `tx` would write, keyed the way
/// `learn::learn_scoped` keys it: this account and direction, the canonical
/// account identity when it is a valid IBAN, else the payee key. `None` when
/// nothing identifies the payee (or the row has no account).
pub fn learned_scope_for(tx: &TransactionInput) -> Option<LearnedPayeeRow> {
    let account = tx.bank_account_id.clone()?;
    let identity = tx
        .counterparty_iban
        .as_deref()
        .map(|id| {
            canonical_structured_identifier(
                id,
                tx.counterparty_bank_code.as_deref(),
                tx.counterparty_country.as_deref(),
            )
        })
        .filter(|id| valid_iban(id));
    let payee = if identity.is_some() {
        None
    } else {
        tx.counterparty
            .as_deref()
            .map(normalize_payee_key)
            .filter(|key| key.len() > 2)
    };
    if identity.is_none() && payee.is_none() {
        return None;
    }
    Some(LearnedPayeeRow {
        id: String::new(),
        normalized_payee: payee,
        original_payee: tx.counterparty.clone(),
        counterparty_iban: identity,
        category_id: String::new(),
        bank_account_id: Some(account),
        is_credit: Some(tx.is_credit),
        created_at: 0,
        updated_at: 0,
    })
}

/// A stored custom rule and whether it is a system rule.
pub fn load_custom_rule(conn: &Connection, id: &str) -> Result<Option<(CategorizationRule, bool)>> {
    let row = conn
        .query_row(
            "SELECT id, name, rule_type, pattern, category_id, priority, is_active, stop_processing, iban_pattern, is_system
             FROM categorization_rules WHERE id = ?1",
            [id],
            |row| {
                let rule_type: String = row.get(2)?;
                Ok((
                    CategorizationRule {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        rule_type: rule_type.parse().unwrap_or(RuleType::Contains),
                        pattern: row.get(3)?,
                        category_id: row.get(4)?,
                        priority: row.get(5)?,
                        is_active: row.get::<_, i32>(6)? != 0,
                        stop_processing: row.get::<_, i32>(7)? != 0,
                        iban_pattern: row.get(8)?,
                    },
                    row.get::<_, i32>(9)? != 0,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

/// Point a custom rule at another category and, when asked, give that category
/// to every payment the rule covers. One transaction; the caller reloads the engine.
pub fn retarget_custom_rule(
    conn: &Connection,
    engine: &CategorizationEngine,
    rule_id: &str,
    category_id: &str,
    apply_history: bool,
) -> Result<Vec<ChangedRow>> {
    if !category_exists(conn, category_id)? {
        return Err(AppError::NotFound(format!(
            "Category not found: {category_id}"
        )));
    }
    let (mut rule, is_system) = load_custom_rule(conn, rule_id)?
        .ok_or_else(|| AppError::NotFound(format!("Rule not found: {rule_id}")))?;
    if is_system {
        return Err(AppError::Validation("validation.systemRule".into()));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE categorization_rules SET category_id = ?1 WHERE id = ?2 AND is_system = 0",
        params![category_id, rule_id],
    )?;
    rule.category_id = category_id.to_string();
    // The live engine loads only active rules, so an inactive one never reached
    // any payment: retargeting it must not rewrite history either.
    let changed = if apply_history && rule.is_active {
        let rows = matching_rows(&tx, engine, &Matcher::Custom(rule), None)?;
        apply(&tx, &rows, category_id, SOURCE_RULE)?
    } else {
        Vec::new()
    };
    tx.commit()?;
    Ok(changed)
}

fn empty_preview(governing: &str, learnable: bool) -> RecategorizationPreview {
    RecategorizationPreview {
        governing: governing.to_string(),
        rule: None,
        learnable,
        matches: 0,
        by_category: Vec::new(),
    }
}

fn preview_of(
    governing: &str,
    rule: Option<GoverningRule>,
    rows: &[MatchedRow],
) -> RecategorizationPreview {
    RecategorizationPreview {
        governing: governing.to_string(),
        rule,
        learnable: true,
        matches: rows.len(),
        by_category: breakdown(rows),
    }
}

/// What remembering a correction on this transaction would rewrite and cover.
pub fn preview_for_transaction(
    conn: &Connection,
    engine: &CategorizationEngine,
    transaction_id: &str,
) -> Result<RecategorizationPreview> {
    let tx = load_transaction_input(conn, transaction_id)?
        .ok_or_else(|| AppError::NotFound(format!("Transaction not found: {transaction_id}")))?;
    let (layer, result) = engine.explain(&tx);
    if layer == Layer::OwnAccount {
        return Ok(empty_preview("ownAccount", false));
    }
    if layer == Layer::Custom {
        let rule_id = match &result {
            CategorizationResult::Match {
                source: CategorizationSource::Rule { rule_id, .. },
                ..
            } => Some(rule_id.clone()),
            _ => None,
        };
        let loaded = match rule_id {
            Some(id) => load_custom_rule(conn, &id)?,
            None => None,
        };
        if let Some((rule, _)) = loaded {
            let rows = matching_rows(
                conn,
                engine,
                &Matcher::Custom(rule.clone()),
                Some(transaction_id),
            )?;
            let governing = GoverningRule {
                id: rule.id,
                name: rule.name,
                category_id: rule.category_id,
            };
            return Ok(preview_of("custom", Some(governing), &rows));
        }
        // The engine knows a rule the table no longer has: fall through to the learned reading.
    }
    let governing = match layer {
        Layer::Learned => "learned",
        Layer::Pack => "pack",
        _ => "none",
    };
    match learned_scope_for(&tx) {
        None => Ok(empty_preview(governing, false)),
        Some(scope) => {
            let rows = matching_rows(conn, engine, &Matcher::Learned(scope), Some(transaction_id))?;
            Ok(preview_of(governing, None, &rows))
        }
    }
}

fn stored_learned_rule(conn: &Connection, id: &str) -> Result<LearnedPayeeRow> {
    learn::list_learned_payees(conn)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or_else(|| AppError::NotFound(format!("Learned rule not found: {id}")))
}

/// What a stored learned rule covers today (the rules page).
pub fn preview_for_learned_rule(
    conn: &Connection,
    engine: &CategorizationEngine,
    id: &str,
) -> Result<RecategorizationPreview> {
    let row = stored_learned_rule(conn, id)?;
    let rows = matching_rows(conn, engine, &Matcher::Learned(row), None)?;
    Ok(preview_of("learned", None, &rows))
}

/// Give a stored learned rule's category to every payment it covers.
pub fn apply_learned_rule(
    conn: &Connection,
    engine: &CategorizationEngine,
    id: &str,
) -> Result<Vec<ChangedRow>> {
    let row = stored_learned_rule(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    let rows = matching_rows(&tx, engine, &Matcher::Learned(row.clone()), None)?;
    let changed = apply(&tx, &rows, &row.category_id, SOURCE_EXACT_MATCH)?;
    tx.commit()?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            INSERT INTO transaction_categories VALUES ('cat_dining','Dining'),('cat_groceries','Groceries'),('cat_gifts','Gifts');
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY);
            INSERT INTO bank_accounts VALUES ('acc-1'),('acc-2');
            CREATE TABLE categorization_rules (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, rule_type TEXT NOT NULL, pattern TEXT NOT NULL,
                category_id TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 50,
                is_active INTEGER NOT NULL DEFAULT 1, stop_processing INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 0, iban_pattern TEXT
            );
            CREATE TABLE learned_payees (
                id TEXT PRIMARY KEY, normalized_payee TEXT, original_payee TEXT, counterparty_iban TEXT,
                category_id TEXT NOT NULL, bank_account_id TEXT, is_credit INTEGER,
                created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL, tx_type TEXT NOT NULL, amount TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', description TEXT, counterparty_name TEXT, counterparty_iban TEXT,
                counterparty_bank_code TEXT, counterparty_country TEXT, booking_date INTEGER NOT NULL DEFAULT 0,
                category_id TEXT, categorization_source TEXT, suggested_category_id TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn insert_tx(
        conn: &Connection,
        id: &str,
        account: &str,
        tx_type: &str,
        counterparty: &str,
        category: Option<&str>,
        source: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, description, counterparty_name, category_id, categorization_source)
             VALUES (?1, ?2, ?3, '100', 'Platba kartou', ?4, ?5, ?6)",
            params![id, account, tx_type, counterparty, category, source],
        )
        .expect("insert");
    }

    fn albert_scope() -> LearnedPayeeRow {
        LearnedPayeeRow {
            id: String::new(),
            normalized_payee: Some("albert".into()),
            original_payee: Some("Albert".into()),
            counterparty_iban: None,
            category_id: "cat_groceries".into(),
            bank_account_id: Some("acc-1".into()),
            is_credit: Some(false),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn category_of(conn: &Connection, id: &str) -> (Option<String>, Option<String>) {
        conn.query_row(
            "SELECT category_id, categorization_source FROM bank_transactions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row")
    }

    #[test]
    fn learned_scope_matches_only_its_account_direction_and_payee() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(&conn, "t2", "acc-1", "debit", "ALBERT 123456", None, None);
        insert_tx(
            &conn,
            "t3",
            "acc-1",
            "credit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(
            &conn,
            "t4",
            "acc-2",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(
            &conn,
            "t5",
            "acc-1",
            "debit",
            "Lidl",
            Some("cat_dining"),
            None,
        );
        let rows = matching_rows(
            &conn,
            &CategorizationEngine::new_empty(),
            &Matcher::Learned(albert_scope()),
            Some("t1"),
        )
        .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["t2"],
            "same account, outgoing, payee key albert; t1 excluded"
        );
    }

    #[test]
    fn custom_rule_spans_every_account() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(
            &conn,
            "t2",
            "acc-2",
            "credit",
            "Albert Hypermarket",
            None,
            None,
        );
        insert_tx(
            &conn,
            "t3",
            "acc-1",
            "debit",
            "Lidl",
            Some("cat_dining"),
            None,
        );
        let rule = CategorizationRule::new(
            "r1".into(),
            "Supermarkety".into(),
            RuleType::Contains,
            "albert".into(),
            "cat_dining".into(),
        );
        let rows = matching_rows(
            &conn,
            &CategorizationEngine::new_empty(),
            &Matcher::Custom(rule),
            None,
        )
        .unwrap();
        let mut ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2"]);
    }

    #[test]
    fn breakdown_orders_by_count_and_keeps_the_uncategorized_bucket() {
        let rows = vec![
            MatchedRow {
                id: "a".into(),
                category_id: None,
                source: None,
            },
            MatchedRow {
                id: "b".into(),
                category_id: Some("cat_dining".into()),
                source: None,
            },
            MatchedRow {
                id: "c".into(),
                category_id: Some("cat_dining".into()),
                source: None,
            },
            MatchedRow {
                id: "d".into(),
                category_id: Some("cat_gifts".into()),
                source: None,
            },
        ];
        assert_eq!(
            breakdown(&rows),
            vec![
                CategoryCount {
                    category_id: Some("cat_dining".into()),
                    count: 2
                },
                CategoryCount {
                    category_id: Some("cat_gifts".into()),
                    count: 1
                },
                CategoryCount {
                    category_id: None,
                    count: 1
                },
            ],
            "ties: named categories before the uncategorized bucket"
        );
    }

    #[test]
    fn apply_skips_rows_already_in_the_category_and_restore_round_trips() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            Some("rule"),
        );
        insert_tx(
            &conn,
            "t2",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_groceries"),
            None,
        );
        insert_tx(&conn, "t3", "acc-1", "debit", "Albert", None, None);
        conn.execute(
            "UPDATE bank_transactions SET suggested_category_id = 'cat_gifts' WHERE id = 't3'",
            [],
        )
        .unwrap();
        let rows = matching_rows(
            &conn,
            &CategorizationEngine::new_empty(),
            &Matcher::Learned(albert_scope()),
            None,
        )
        .unwrap();
        let changed = apply(&conn, &rows, "cat_groceries", SOURCE_EXACT_MATCH).unwrap();
        let mut changed_ids: Vec<&str> = changed.iter().map(|c| c.id.as_str()).collect();
        changed_ids.sort();
        assert_eq!(changed_ids, vec!["t1", "t3"], "t2 already had the category");
        assert_eq!(
            category_of(&conn, "t1"),
            (Some("cat_groceries".into()), Some("exact_match".into()))
        );
        assert_eq!(
            conn.query_row(
                "SELECT suggested_category_id FROM bank_transactions WHERE id = 't3'",
                [],
                |r| r.get::<_, Option<String>>(0)
            )
            .unwrap(),
            None,
            "a pending suggestion is cleared"
        );
        let t1 = changed.iter().find(|c| c.id == "t1").unwrap();
        assert_eq!(t1.previous_category_id.as_deref(), Some("cat_dining"));
        assert_eq!(t1.previous_source.as_deref(), Some("rule"));

        conn.execute("DELETE FROM bank_transactions WHERE id = 't3'", [])
            .unwrap();
        let restored = restore(&conn, &changed).unwrap();
        assert_eq!(restored, 1, "a row deleted meanwhile is skipped");
        assert_eq!(
            category_of(&conn, "t1"),
            (Some("cat_dining".into()), Some("rule".into()))
        );
    }

    #[test]
    fn apply_rejects_an_unknown_category() {
        let conn = setup();
        let err = apply(&conn, &[], "cat_missing", SOURCE_RULE).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn retarget_changes_only_the_rule_category_and_applies_when_asked() {
        let conn = setup();
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, priority, stop_processing, iban_pattern)
             VALUES ('r1', 'Supermarkety', 'contains', 'albert', 'cat_dining', 70, 1, NULL)",
            [],
        ).unwrap();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            Some("rule"),
        );
        insert_tx(
            &conn,
            "t2",
            "acc-2",
            "debit",
            "Albert",
            Some("cat_groceries"),
            None,
        );

        let changed = retarget_custom_rule(
            &conn,
            &CategorizationEngine::new_empty(),
            "r1",
            "cat_groceries",
            false,
        )
        .unwrap();
        assert!(changed.is_empty());
        let (category, priority, stop): (String, i32, i32) = conn
            .query_row("SELECT category_id, priority, stop_processing FROM categorization_rules WHERE id = 'r1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!(
            (category.as_str(), priority, stop),
            ("cat_groceries", 70, 1)
        );
        assert_eq!(
            category_of(&conn, "t1").0.as_deref(),
            Some("cat_dining"),
            "history untouched without apply"
        );

        let changed = retarget_custom_rule(
            &conn,
            &CategorizationEngine::new_empty(),
            "r1",
            "cat_gifts",
            true,
        )
        .unwrap();
        let mut ids: Vec<&str> = changed.iter().map(|c| c.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["t1", "t2"]);
        assert_eq!(
            category_of(&conn, "t2"),
            (Some("cat_gifts".into()), Some("rule".into()))
        );
    }

    #[test]
    fn restore_drops_the_source_when_the_previous_category_is_gone() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            Some("rule"),
        );
        let rows = matching_rows(
            &conn,
            &CategorizationEngine::new_empty(),
            &Matcher::Learned(albert_scope()),
            None,
        )
        .unwrap();
        let changed = apply(&conn, &rows, "cat_groceries", SOURCE_EXACT_MATCH).unwrap();
        assert_eq!(changed.len(), 1);
        conn.execute(
            "DELETE FROM transaction_categories WHERE id = 'cat_dining'",
            [],
        )
        .unwrap();
        assert_eq!(restore(&conn, &changed).unwrap(), 1);
        assert_eq!(
            category_of(&conn, "t1"),
            (None, None),
            "no category, so no provenance either"
        );
    }

    #[test]
    fn retarget_with_history_leaves_an_inactive_rule_alone() {
        let conn = setup();
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, is_active)
             VALUES ('r_off', 'Vypnuto', 'contains', 'albert', 'cat_dining', 0)",
            [],
        ).unwrap();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            Some("rule"),
        );
        let changed = retarget_custom_rule(
            &conn,
            &CategorizationEngine::new_empty(),
            "r_off",
            "cat_gifts",
            true,
        )
        .unwrap();
        assert!(
            changed.is_empty(),
            "an inactive rule never reached any payment"
        );
        let category: String = conn
            .query_row(
                "SELECT category_id FROM categorization_rules WHERE id = 'r_off'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(category, "cat_gifts", "the rule itself is still retargeted");
        assert_eq!(
            category_of(&conn, "t1"),
            (Some("cat_dining".into()), Some("rule".into()))
        );
    }

    #[test]
    fn retarget_refuses_system_rules_and_unknown_rules() {
        let conn = setup();
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, is_system) VALUES ('sys', 'System', 'contains', 'x', 'cat_dining', 1)",
            [],
        ).unwrap();
        assert!(matches!(
            retarget_custom_rule(
                &conn,
                &CategorizationEngine::new_empty(),
                "sys",
                "cat_gifts",
                false
            )
            .unwrap_err(),
            AppError::Validation(_)
        ));
        assert!(matches!(
            retarget_custom_rule(
                &conn,
                &CategorizationEngine::new_empty(),
                "nope",
                "cat_gifts",
                false
            )
            .unwrap_err(),
            AppError::NotFound(_)
        ));
    }

    #[test]
    fn learned_scope_for_prefers_a_valid_iban_identity_over_the_payee() {
        let mut tx = TransactionInput::new("t".into(), None, Some("Albert".into()), -10.0);
        tx.bank_account_id = Some("acc-1".into());
        let scope = learned_scope_for(&tx).unwrap();
        assert_eq!(scope.normalized_payee.as_deref(), Some("albert"));
        assert_eq!(scope.counterparty_iban, None);
        assert_eq!(scope.bank_account_id.as_deref(), Some("acc-1"));
        assert_eq!(scope.is_credit, Some(false));

        tx.counterparty_iban = Some("CZ65 0800 0000 1920 0014 5399".into());
        let scope = learned_scope_for(&tx).unwrap();
        assert_eq!(
            scope.counterparty_iban.as_deref(),
            Some("CZ6508000000192000145399")
        );
        assert_eq!(
            scope.normalized_payee, None,
            "an identity rule is keyed by the account alone"
        );

        let mut nothing = TransactionInput::new("t".into(), None, Some("AB".into()), -10.0);
        nothing.bank_account_id = Some("acc-1".into());
        assert!(
            learned_scope_for(&nothing).is_none(),
            "two characters identify nobody"
        );
        let mut no_account = TransactionInput::new("t".into(), None, Some("Albert".into()), -10.0);
        no_account.bank_account_id = None;
        assert!(learned_scope_for(&no_account).is_none());
    }

    #[test]
    fn preview_for_transaction_reports_the_layer_and_the_coverage() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(
            &conn,
            "t2",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(&conn, "t3", "acc-1", "debit", "Albert", None, None);
        let engine = CategorizationEngine::new_empty();

        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "none");
        assert!(p.learnable);
        assert_eq!(p.matches, 2);
        assert_eq!(
            p.by_category,
            vec![
                CategoryCount {
                    category_id: Some("cat_dining".into()),
                    count: 1
                },
                CategoryCount {
                    category_id: None,
                    count: 1
                },
            ]
        );

        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) VALUES ('r1', 'Supermarkety', 'contains', 'albert', 'cat_dining')",
            [],
        ).unwrap();
        engine.update_custom_rules(vec![CategorizationRule::new(
            "r1".into(),
            "Supermarkety".into(),
            RuleType::Contains,
            "albert".into(),
            "cat_dining".into(),
        )]);
        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "custom");
        assert_eq!(
            p.rule.as_ref().map(|r| r.name.as_str()),
            Some("Supermarkety")
        );
        assert_eq!(p.matches, 2, "the custom rule's own coverage, t1 excluded");

        engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);
        conn.execute("UPDATE bank_transactions SET counterparty_iban = 'CZ6508000000192000145399' WHERE id = 't1'", []).unwrap();
        let p = preview_for_transaction(&conn, &engine, "t1").unwrap();
        assert_eq!(p.governing, "ownAccount");
        assert!(!p.learnable);
        assert_eq!(p.matches, 0);

        assert!(matches!(
            preview_for_transaction(&conn, &engine, "missing").unwrap_err(),
            AppError::NotFound(_)
        ));
    }

    #[test]
    fn learned_rule_preview_and_apply_use_the_stored_rule() {
        let conn = setup();
        conn.execute(
            "INSERT INTO learned_payees (id, normalized_payee, original_payee, category_id, bank_account_id, is_credit) VALUES ('l1', 'albert', 'Albert', 'cat_groceries', 'acc-1', 0)",
            [],
        ).unwrap();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_dining"),
            None,
        );
        insert_tx(
            &conn,
            "t2",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_groceries"),
            None,
        );
        let p = preview_for_learned_rule(&conn, &CategorizationEngine::new_empty(), "l1").unwrap();
        assert_eq!(p.governing, "learned");
        assert_eq!(p.matches, 2);
        let changed = apply_learned_rule(&conn, &CategorizationEngine::new_empty(), "l1").unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(
            category_of(&conn, "t1"),
            (Some("cat_groceries".into()), Some("exact_match".into()))
        );
        assert!(matches!(
            preview_for_learned_rule(&conn, &CategorizationEngine::new_empty(), "missing")
                .unwrap_err(),
            AppError::NotFound(_)
        ));
    }

    const OWN_IBAN: &str = "CZ6508000000192000145399";

    fn set_counterparty_iban(conn: &Connection, id: &str, iban: &str) {
        conn.execute(
            "UPDATE bank_transactions SET counterparty_iban = ?1 WHERE id = ?2",
            params![iban, id],
        )
        .expect("set iban");
    }

    #[test]
    fn rules_never_cover_transfers_between_the_users_own_accounts() {
        let conn = setup();
        insert_tx(&conn, "normal", "acc-1", "debit", "Albert", None, None);
        insert_tx(&conn, "own", "acc-1", "debit", "Albert", None, None);
        set_counterparty_iban(&conn, "own", OWN_IBAN);
        let engine = CategorizationEngine::new_empty();
        engine.set_own_ibans(vec![OWN_IBAN.into()]);

        let ids =
            |rows: Vec<MatchedRow>| -> Vec<String> { rows.into_iter().map(|r| r.id).collect() };
        assert_eq!(
            ids(matching_rows(&conn, &engine, &Matcher::Learned(albert_scope()), None).unwrap()),
            vec!["normal".to_string()],
            "the payee scope skips the own-account transfer the waterfall files elsewhere"
        );
        let rule = CategorizationRule::new(
            "r1".into(),
            "Supermarkety".into(),
            RuleType::Contains,
            "albert".into(),
            "cat_dining".into(),
        );
        assert_eq!(
            ids(matching_rows(&conn, &engine, &Matcher::Custom(rule), None).unwrap()),
            vec!["normal".to_string()],
            "a custom rule skips it too"
        );
        assert_eq!(
            matching_rows(
                &conn,
                &CategorizationEngine::new_empty(),
                &Matcher::Learned(albert_scope()),
                None
            )
            .unwrap()
            .len(),
            2,
            "without the own-account IBAN both rows are covered"
        );
    }

    #[test]
    fn identity_scope_covers_the_account_whatever_the_name() {
        let conn = setup();
        insert_tx(
            &conn,
            "same",
            "acc-1",
            "debit",
            "Different Name",
            None,
            None,
        );
        set_counterparty_iban(&conn, "same", OWN_IBAN);
        insert_tx(&conn, "other_iban", "acc-1", "debit", "Albert", None, None);
        set_counterparty_iban(&conn, "other_iban", "CZ5508000000001234567899");
        insert_tx(
            &conn,
            "other_acc",
            "acc-2",
            "debit",
            "Different Name",
            None,
            None,
        );
        set_counterparty_iban(&conn, "other_acc", OWN_IBAN);
        let scope = LearnedPayeeRow {
            id: String::new(),
            normalized_payee: None,
            original_payee: None,
            counterparty_iban: Some(OWN_IBAN.into()),
            category_id: "cat_groceries".into(),
            bank_account_id: Some("acc-1".into()),
            is_credit: Some(false),
            created_at: 0,
            updated_at: 0,
        };
        let rows = matching_rows(
            &conn,
            &CategorizationEngine::new_empty(),
            &Matcher::Learned(scope),
            None,
        )
        .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["same"],
            "same IBAN on this account and direction, under any name; another IBAN or another account stays out"
        );
    }

    #[test]
    fn learned_scope_keys_match_what_learning_stores() {
        use crate::models::categorization::InsertScopedLearning;
        // (payee, iban, bank code, country)
        type Case<'a> = (
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
        );
        let cases: [Case; 3] = [
            (Some("Albert"), None, None, None),
            (
                None,
                Some("CZ65 0800 0000 1920 0014 5399"),
                Some("0800"),
                Some("CZ"),
            ),
            (Some("AB"), None, None, None),
        ];
        for (payee, iban, bank_code, country) in cases {
            let conn = setup();
            let mut tx = TransactionInput::new("t".into(), None, payee.map(String::from), -10.0);
            tx.bank_account_id = Some("acc-1".into());
            tx.counterparty_iban = iban.map(String::from);
            tx.counterparty_bank_code = bank_code.map(String::from);
            tx.counterparty_country = country.map(String::from);
            let scope = learned_scope_for(&tx);

            let data = InsertScopedLearning {
                bank_account_id: "acc-1".into(),
                is_credit: false,
                payee: payee.map(String::from),
                counterparty_iban: iban.map(String::from),
                counterparty_bank_code: bank_code.map(String::from),
                counterparty_country: country.map(String::from),
                category_id: "cat_dining".into(),
            };
            let learned = learn::learn_scoped_and_apply(
                &conn,
                &CategorizationEngine::new_empty(),
                &data,
                false,
            );
            let stored: Option<(Option<String>, Option<String>)> = conn
                .query_row(
                    "SELECT normalized_payee, counterparty_iban FROM learned_payees",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .unwrap();
            match scope {
                Some(scope) => {
                    assert!(learned.is_ok(), "learning accepts {payee:?} / {iban:?}");
                    assert_eq!(
                        stored,
                        Some((scope.normalized_payee, scope.counterparty_iban)),
                        "the preview keys {payee:?} / {iban:?} the way learning stores it"
                    );
                }
                None => {
                    assert!(
                        matches!(learned, Err(AppError::Validation(_))),
                        "{payee:?} / {iban:?} identifies nobody: learning refuses it"
                    );
                    assert_eq!(stored, None, "and stores nothing");
                }
            }
        }
    }

    #[test]
    fn restore_is_all_or_nothing() {
        let conn = setup();
        insert_tx(
            &conn,
            "t1",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_groceries"),
            Some("exact_match"),
        );
        insert_tx(
            &conn,
            "t2",
            "acc-1",
            "debit",
            "Albert",
            Some("cat_groceries"),
            Some("exact_match"),
        );
        let undo = vec![
            ChangedRow {
                id: "t1".into(),
                previous_category_id: Some("cat_dining".into()),
                previous_source: None,
            },
            ChangedRow {
                id: "t2".into(),
                previous_category_id: Some("cat_dining".into()),
                previous_source: None,
            },
        ];
        conn.execute_batch(
            "CREATE TRIGGER block_t2 BEFORE UPDATE ON bank_transactions WHEN NEW.id = 't2'
             BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
        )
        .unwrap();
        assert!(restore(&conn, &undo).is_err());
        assert_eq!(
            category_of(&conn, "t1"),
            (Some("cat_groceries".into()), Some("exact_match".into())),
            "the first row was rolled back with the failed second one"
        );
    }
}
