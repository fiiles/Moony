//! Rule statistics for the categorization rules page (redesign phase 2,
//! backend D): how often each rule — custom, pack or learned — would have
//! matched the transactions of the last N days, what a draft rule would catch
//! today, and the automation rate. DB-only: rules are read from the table and
//! the packs, learned payees from `learned_payees`, so the numbers do not
//! depend on what the in-memory engine currently holds.
//!
//! Every rule is replayed on its own (not through the waterfall), so a rule
//! shadowed by a higher-priority one still shows what it covers; the counts
//! answer "does this rule do anything for me", not "which rule won".

use std::collections::HashMap;

use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::learn;
use super::packs;
use super::rules::RuleEngine;
use super::tokenizer::{normalize_iban, normalize_payee_key, strip_punctuation};
use super::types::{CategorizationRule, RuleType, TransactionInput};
use crate::error::{AppError, Result};

const SECONDS_PER_DAY: i64 = 86_400;
/// Sample transactions returned by a draft-rule preview.
const PREVIEW_SAMPLES: usize = 5;

/// How often one rule matched in the period (TS `RuleHitCount`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleHitCount {
    /// Custom rule id, pack rule id, or the learned payee's id.
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    pub hits: usize,
    /// Booking day of the newest matching transaction.
    #[serde(rename = "lastHit")]
    pub last_hit: Option<i64>,
}

/// One transaction a draft rule matches (TS `RuleMatchSample`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleMatchSample {
    #[serde(rename = "bookingDate")]
    pub booking_date: i64,
    pub description: Option<String>,
    pub counterparty: Option<String>,
    /// Signed decimal string (debits negative).
    pub amount: String,
    pub currency: String,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
}

/// What a draft rule would match today (TS `RuleMatchPreview`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RuleMatchPreview {
    pub matches: usize,
    /// Matching transactions that have no category today.
    pub uncategorized: usize,
    /// The newest matches, at most five.
    pub samples: Vec<RuleMatchSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceCount {
    /// `rule`, `exact_match`, `own_account`, `mcp`, `manual` (a category without a recorded source).
    pub source: String,
    pub count: usize,
}

/// The automation rate (TS `CategorizationOverview`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CategorizationOverview {
    pub total: usize,
    pub categorized: usize,
    #[serde(rename = "bySource")]
    pub by_source: Vec<SourceCount>,
}

/// A transaction of the period, as the engine sees it plus what the row holds.
struct TxRow {
    input: TransactionInput,
    booking_date: i64,
    category_id: Option<String>,
    source: Option<String>,
    /// Stored (positive) amount and direction, for the samples.
    amount: String,
    tx_type: String,
    currency: String,
}

/// First second of the day `days` days before `now` (UTC).
fn since_day(now: i64, days: i64) -> i64 {
    let day = now - now.rem_euclid(SECONDS_PER_DAY);
    day - days.max(0) * SECONDS_PER_DAY
}

fn load_rows(conn: &Connection, since: i64) -> Result<Vec<TxRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, description, counterparty_name, counterparty_iban, amount, tx_type, booking_date,
                category_id, categorization_source, bank_account_id, currency
         FROM bank_transactions WHERE booking_date >= ?1
         ORDER BY booking_date DESC, created_at DESC",
    )?;
    let rows = stmt.query_map([since], |row| {
        let amount: String = row.get(4)?;
        let tx_type: String = row.get(5)?;
        let value: f64 = amount.trim().parse().unwrap_or(0.0);
        let is_credit = tx_type.eq_ignore_ascii_case("credit");
        Ok(TxRow {
            input: TransactionInput {
                id: row.get(0)?,
                description: row.get(1)?,
                counterparty: row.get(2)?,
                counterparty_iban: row.get(3)?,
                amount: if is_credit { value.abs() } else { -value.abs() },
                is_credit,
                bank_account_id: row.get(9)?,
            },
            booking_date: row.get(6)?,
            category_id: row.get(7)?,
            source: row.get(8)?,
            amount,
            tx_type,
            currency: row.get(10)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The stored custom rules, every one treated as active so an inactive rule
/// still shows what it would cover.
fn custom_rules(conn: &Connection) -> Result<Vec<CategorizationRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, rule_type, pattern, category_id, priority, stop_processing, iban_pattern
         FROM categorization_rules",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i32>(5)?,
            row.get::<_, i32>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, name, rule_type, pattern, category_id, priority, stop, iban) = row?;
        let Ok(rule_type) = rule_type.parse::<RuleType>() else {
            continue;
        };
        out.push(CategorizationRule {
            id,
            name,
            rule_type,
            pattern,
            category_id,
            priority,
            is_active: true,
            stop_processing: stop != 0,
            iban_pattern: iban,
        });
    }
    Ok(out)
}

/// A single-rule engine; the rule is forced active.
fn engine_for(rule: &CategorizationRule) -> RuleEngine {
    let mut rule = rule.clone();
    rule.is_active = true;
    RuleEngine::new(vec![rule])
}

fn count_rule(rule: &CategorizationRule, rows: &[TxRow]) -> RuleHitCount {
    let engine = engine_for(rule);
    let mut hits = 0;
    let mut last_hit = None;
    for row in rows {
        if engine.apply(&row.input).is_some() {
            hits += 1;
            // Rows come newest first.
            if last_hit.is_none() {
                last_hit = Some(row.booking_date);
            }
        }
    }
    RuleHitCount {
        rule_id: rule.id.clone(),
        hits,
        last_hit,
    }
}

/// The learned rule's match, mirroring `ExactMatchEngine::apply`: an IBAN rule
/// matches on the IBAN alone, a payee rule on the payee key (or its legacy
/// punctuation-stripped form).
fn learned_matches(row: &learn::LearnedPayeeRow, tx: &TransactionInput) -> bool {
    if let Some(ref iban) = row.counterparty_iban {
        return tx
            .counterparty_iban
            .as_deref()
            .map(|i| normalize_iban(i) == *iban)
            .unwrap_or(false);
    }
    let Some(ref stored) = row.normalized_payee else {
        return false;
    };
    let Some(counterparty) = tx.counterparty.as_deref() else {
        return false;
    };
    let key = normalize_payee_key(counterparty);
    if key.len() <= 2 {
        return false;
    }
    key == *stored || strip_punctuation(&key) == *stored
}

/// Hits of every custom rule, every rule of the enabled packs and every
/// learned rule over the transactions of the last `days` days.
pub fn rule_hit_counts(conn: &Connection, days: i64) -> Result<Vec<RuleHitCount>> {
    let now = chrono::Utc::now().timestamp();
    let rows = load_rows(conn, since_day(now, days))?;
    let mut out = Vec::new();
    for rule in custom_rules(conn)? {
        out.push(count_rule(&rule, &rows));
    }
    let (enabled, _disabled) = packs::read_config(conn)?;
    for pack in packs::available_packs()
        .iter()
        .filter(|p| enabled.iter().any(|id| id == &p.pack_id))
    {
        for rule in &pack.rules {
            out.push(count_rule(rule, &rows));
        }
    }
    for learned in learn::list_learned_payees(conn)? {
        let mut hits = 0;
        let mut last_hit = None;
        for row in &rows {
            if learned_matches(&learned, &row.input) {
                hits += 1;
                if last_hit.is_none() {
                    last_hit = Some(row.booking_date);
                }
            }
        }
        out.push(RuleHitCount {
            rule_id: learned.id,
            hits,
            last_hit,
        });
    }
    Ok(out)
}

/// A regex rule with a pattern that does not compile is a validation error,
/// never a rule that silently matches nothing.
fn validate_rule(rule: &CategorizationRule) -> Result<()> {
    if rule.rule_type == RuleType::Regex
        && rule.iban_pattern.as_deref().is_none_or(str::is_empty)
        && Regex::new(&rule.pattern).is_err()
    {
        return Err(AppError::Validation("validation.invalidRegex".into()));
    }
    Ok(())
}

/// What `rule` would match among the transactions of the last `days` days.
pub fn preview_rule_matches(
    conn: &Connection,
    rule: &CategorizationRule,
    days: i64,
) -> Result<RuleMatchPreview> {
    validate_rule(rule)?;
    let now = chrono::Utc::now().timestamp();
    let rows = load_rows(conn, since_day(now, days))?;
    let engine = engine_for(rule);
    let mut preview = RuleMatchPreview {
        matches: 0,
        uncategorized: 0,
        samples: Vec::new(),
    };
    for row in &rows {
        if engine.apply(&row.input).is_none() {
            continue;
        }
        preview.matches += 1;
        if row.category_id.is_none() {
            preview.uncategorized += 1;
        }
        if preview.samples.len() < PREVIEW_SAMPLES {
            let value = row.amount.trim().parse::<f64>().unwrap_or(0.0).abs();
            let signed = if row.tx_type.eq_ignore_ascii_case("credit") {
                value
            } else {
                -value
            };
            preview.samples.push(RuleMatchSample {
                booking_date: row.booking_date,
                description: row.input.description.clone(),
                counterparty: row.input.counterparty.clone(),
                amount: format!("{signed}"),
                currency: row.currency.clone(),
                category_id: row.category_id.clone(),
            });
        }
    }
    Ok(preview)
}

/// Give `rule`'s category to the matching transactions of the period that have
/// none yet ("Zařadit nezařazené hned"). Categorized rows are never touched.
pub fn apply_rule_to_uncategorized(
    conn: &Connection,
    rule: &CategorizationRule,
    days: i64,
) -> Result<usize> {
    validate_rule(rule)?;
    if !crate::services::bank_accounts::category_exists(conn, &rule.category_id)? {
        return Err(AppError::NotFound("Category not found".into()));
    }
    let now = chrono::Utc::now().timestamp();
    let rows = load_rows(conn, since_day(now, days))?;
    let engine = engine_for(rule);
    let mut changed = 0;
    for row in rows.iter().filter(|r| r.category_id.is_none()) {
        if engine.apply(&row.input).is_none() {
            continue;
        }
        changed += conn.execute(
            "UPDATE bank_transactions SET category_id = ?1, categorization_source = 'rule' WHERE id = ?2",
            rusqlite::params![rule.category_id, row.input.id],
        )?;
    }
    Ok(changed)
}

/// Transactions of the period: how many, how many categorized, and by which layer.
pub fn categorization_overview(conn: &Connection, days: i64) -> Result<CategorizationOverview> {
    let now = chrono::Utc::now().timestamp();
    let rows = load_rows(conn, since_day(now, days))?;
    let mut by_source: HashMap<String, usize> = HashMap::new();
    let mut categorized = 0;
    for row in &rows {
        if row.category_id.is_none() {
            continue;
        }
        categorized += 1;
        let source = row.source.clone().unwrap_or_else(|| "manual".to_string());
        *by_source.entry(source).or_default() += 1;
    }
    let mut by_source: Vec<SourceCount> = by_source
        .into_iter()
        .map(|(source, count)| SourceCount { source, count })
        .collect();
    by_source.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.source.cmp(&b.source)));
    Ok(CategorizationOverview {
        total: rows.len(),
        categorized,
        by_source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE categorization_rules (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, rule_type TEXT NOT NULL, pattern TEXT NOT NULL,
                category_id TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 50,
                is_active INTEGER NOT NULL DEFAULT 1, stop_processing INTEGER NOT NULL DEFAULT 0,
                is_system INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL DEFAULT 0, iban_pattern TEXT
            );
            CREATE TABLE learned_payees (
                id TEXT PRIMARY KEY, normalized_payee TEXT, original_payee TEXT, counterparty_iban TEXT,
                category_id TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL, transaction_id TEXT, tx_type TEXT NOT NULL,
                amount TEXT NOT NULL, currency TEXT NOT NULL, description TEXT, counterparty_name TEXT,
                counterparty_iban TEXT, booking_date INTEGER NOT NULL, value_date INTEGER, category_id TEXT,
                merchant_category_code TEXT, remittance_info TEXT, status TEXT NOT NULL DEFAULT 'booked',
                data_source TEXT NOT NULL DEFAULT 'manual', created_at INTEGER NOT NULL DEFAULT 0,
                import_batch_id TEXT, categorization_source TEXT, suggested_category_id TEXT
            );
            INSERT INTO transaction_categories (id, name) VALUES ('cat-food', 'Food'), ('cat-fun', 'Fun');
            INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[]');
            INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, is_active)
                VALUES ('r-albert', 'Albert', 'Contains', 'albert', 'cat-food', 1),
                       ('r-old', 'Old employer', 'Contains', 'beta corp', 'cat-fun', 0);
            "#,
        )
        .expect("schema");
        // Stored the way `learn_payee` stores it: the counterparty text it was learned
        // from plus its normalized key (the listing re-derives the key from the text).
        conn.execute(
            "INSERT INTO learned_payees (id, normalized_payee, original_payee, category_id)
             VALUES ('l-kalich', ?1, 'U Kalicha', 'cat-fun')",
            [normalize_payee_key("U Kalicha")],
        )
        .unwrap();
        let now = chrono::Utc::now().timestamp();
        let day = now - now.rem_euclid(SECONDS_PER_DAY);
        let rows = [
            (
                "t1",
                "debit",
                "1240",
                "Platba kartou Albert",
                "Albert Praha",
                Some("cat-food"),
                Some("rule"),
                day - SECONDS_PER_DAY,
            ),
            (
                "t2",
                "debit",
                "450",
                "Restaurace U Kalicha",
                "U Kalicha",
                None,
                None,
                day - 3 * SECONDS_PER_DAY,
            ),
            (
                "t3",
                "debit",
                "99",
                "Albert hypermarket",
                "Albert",
                None,
                None,
                day - 5 * SECONDS_PER_DAY,
            ),
            (
                "t4",
                "credit",
                "68500",
                "Vyplata",
                "ACME",
                Some("cat-fun"),
                None,
                day - 7 * SECONDS_PER_DAY,
            ),
            (
                "t5",
                "debit",
                "300",
                "Albert",
                "Albert",
                None,
                None,
                day - 400 * SECONDS_PER_DAY,
            ),
        ];
        for (id, tx_type, amount, desc, cp, cat, source, booked) in rows {
            conn.execute(
                "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, description, counterparty_name, booking_date, category_id, categorization_source)
                 VALUES (?1, 'acc', ?2, ?3, 'CZK', ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![id, tx_type, amount, desc, cp, booked, cat, source],
            )
            .unwrap();
        }
        conn
    }

    fn hits_of<'a>(counts: &'a [RuleHitCount], id: &str) -> &'a RuleHitCount {
        counts
            .iter()
            .find(|c| c.rule_id == id)
            .expect("rule counted")
    }

    #[test]
    fn counts_custom_pack_and_learned_rules_independently() {
        let conn = setup();
        let counts = rule_hit_counts(&conn, 90).unwrap();
        // Two Albert rows inside 90 days (t5 is a year old); inactive rules are counted too.
        let albert = hits_of(&counts, "r-albert");
        assert_eq!(albert.hits, 2);
        assert!(albert.last_hit.is_some());
        assert_eq!(hits_of(&counts, "r-old").hits, 0);
        assert_eq!(hits_of(&counts, "l-kalich").hits, 1);
        // No pack is enabled, so only the three rules are listed.
        assert_eq!(counts.len(), 3);
    }

    #[test]
    fn previews_a_draft_rule_and_rejects_a_broken_regex() {
        let conn = setup();
        let rule = CategorizationRule::new(
            "draft".into(),
            "Albert".into(),
            RuleType::Contains,
            "albert".into(),
            "cat-food".into(),
        );
        let preview = preview_rule_matches(&conn, &rule, 90).unwrap();
        assert_eq!(preview.matches, 2);
        assert_eq!(preview.uncategorized, 1);
        assert_eq!(preview.samples.len(), 2);
        assert_eq!(preview.samples[0].amount, "-1240");
        assert_eq!(
            preview.samples[0].counterparty.as_deref(),
            Some("Albert Praha")
        );

        let broken = CategorizationRule::new(
            "draft".into(),
            "Broken".into(),
            RuleType::Regex,
            "(".into(),
            "cat-food".into(),
        );
        assert!(matches!(
            preview_rule_matches(&conn, &broken, 90),
            Err(AppError::Validation(key)) if key == "validation.invalidRegex"
        ));
    }

    #[test]
    fn applies_a_rule_to_uncategorized_rows_only() {
        let conn = setup();
        let rule = CategorizationRule::new(
            "r-albert".into(),
            "Albert".into(),
            RuleType::Contains,
            "albert".into(),
            "cat-fun".into(),
        );
        assert_eq!(apply_rule_to_uncategorized(&conn, &rule, 90).unwrap(), 1);
        let (t1, t3): (String, String) = conn
            .query_row(
                "SELECT (SELECT category_id FROM bank_transactions WHERE id = 't1'),
                        (SELECT category_id FROM bank_transactions WHERE id = 't3')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(t1, "cat-food", "a categorized row is left alone");
        assert_eq!(t3, "cat-fun");
    }

    #[test]
    fn overview_counts_the_period_by_source() {
        let conn = setup();
        let overview = categorization_overview(&conn, 90).unwrap();
        assert_eq!(overview.total, 4);
        assert_eq!(overview.categorized, 2);
        assert_eq!(overview.by_source.len(), 2);
        assert!(overview
            .by_source
            .iter()
            .any(|s| s.source == "manual" && s.count == 1));
    }
}
