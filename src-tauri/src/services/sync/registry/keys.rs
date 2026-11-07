//! Unique-key rules, the foreign-key graph and migration classes of the sync registry.

use std::collections::BTreeMap;

use rusqlite::Connection;

use super::{schema, spec, TableClass};
use crate::error::{AppError, Result};

/// How two devices' rows that collide on a unique index are reconciled (§7.3, rulings U1–U3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UniqueRule {
    /// The creating services derive the id from the key, so the colliding rows share one id.
    /// Another id on the same key (unreachable by construction after D22) is kept-newest by the
    /// key clock, with a warning (`merge::unique`).
    SameId,
    /// Keep both rows; every one but the lowest id shows a numbered name (derived, `sync_typed`).
    Rename,
    /// Keep both rows; every one but the lowest id shows no link (derived, `sync_typed`).
    Unlink,
}

#[derive(Debug, Clone, Copy)]
pub struct UniqueSpec {
    pub table: &'static str,
    /// The index's column or expression list.
    pub exprs: &'static [&'static str],
    pub partial: Option<&'static str>,
    /// The plain columns `exprs` read (the key clock of the keep-newest fallback).
    pub key_columns: &'static [&'static str],
    pub rule: UniqueRule,
}

const fn unique(
    table: &'static str,
    cols: &'static [&'static str],
    rule: UniqueRule,
) -> UniqueSpec {
    UniqueSpec {
        table,
        exprs: cols,
        partial: None,
        key_columns: cols,
        rule,
    }
}

static UNIQUE_RULES: &[UniqueSpec] = &[
    unique("stock_investments", &["ticker"], UniqueRule::SameId),
    unique("crypto_investments", &["ticker"], UniqueRule::SameId),
    unique("watched_stocks", &["ticker"], UniqueRule::SameId),
    unique("stock_price_overrides", &["ticker"], UniqueRule::SameId),
    unique("dividend_overrides", &["ticker"], UniqueRule::SameId),
    unique("crypto_price_overrides", &["symbol"], UniqueRule::SameId),
    unique(
        "stock_splits",
        &["ticker", "effective_day"],
        UniqueRule::SameId,
    ),
    unique("projection_settings", &["asset_type"], UniqueRule::SameId),
    unique(
        "bank_transactions",
        &["bank_account_id", "transaction_id"],
        UniqueRule::SameId,
    ),
    UniqueSpec {
        table: "learned_payees",
        exprs: &[
            "COALESCE(normalized_payee,'')",
            "COALESCE(counterparty_iban,'')",
            "bank_account_id",
            "is_credit",
        ],
        partial: Some("bank_account_id IS NOT NULL"),
        key_columns: &[
            "normalized_payee",
            "counterparty_iban",
            "bank_account_id",
            "is_credit",
        ],
        rule: UniqueRule::SameId,
    },
    unique(
        "bank_balance_records",
        &["bank_account_id", "as_of"],
        UniqueRule::SameId,
    ),
    unique(
        "budget_goal_versions",
        &["category_id", "timeframe", "effective_month"],
        UniqueRule::SameId,
    ),
    unique("stock_tags", &["name"], UniqueRule::Rename),
    unique("stock_tag_groups", &["name"], UniqueRule::Rename),
    UniqueSpec {
        table: "cashflow_items",
        exprs: &["replacement_source_id"],
        partial: Some("replacement_source_id IS NOT NULL"),
        key_columns: &["replacement_source_id"],
        rule: UniqueRule::Unlink,
    },
];

pub fn unique_rules() -> &'static [UniqueSpec] {
    UNIQUE_RULES
}

/// What happens to a child row whose parent was deleted on another device (NO ACTION keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanRule {
    SetNull,
    DropChild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnDelete {
    Cascade,
    SetNull,
    NoAction(OrphanRule),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub child: String,
    pub column: String,
    pub parent: String,
    pub parent_column: String,
    pub on_delete: OnDelete,
}

/// (child table, column, rule) for every NO ACTION key between synced tables.
pub const ORPHAN_RULES: &[(&str, &str, OrphanRule)] = &[
    ("bank_transactions", "category_id", OrphanRule::SetNull),
    (
        "bank_transactions",
        "suggested_category_id",
        OrphanRule::SetNull,
    ),
    ("transaction_categories", "parent_id", OrphanRule::SetNull),
    ("bank_accounts", "institution_id", OrphanRule::SetNull),
    ("categorization_rules", "category_id", OrphanRule::DropChild),
    ("learned_payees", "category_id", OrphanRule::DropChild),
    ("budget_goal_versions", "category_id", OrphanRule::DropChild),
];

pub fn foreign_keys(conn: &Connection) -> Result<Vec<ForeignKey>> {
    foreign_keys_with(conn, ORPHAN_RULES)
}

fn is_synced(table: &str) -> bool {
    spec(table).is_some_and(|s| s.class == TableClass::Synced)
}

/// Foreign keys between Synced tables, read from `PRAGMA foreign_key_list`.
pub fn foreign_keys_with(
    conn: &Connection,
    orphan_rules: &[(&str, &str, OrphanRule)],
) -> Result<Vec<ForeignKey>> {
    let mut out = Vec::new();
    for child in super::synced_tables() {
        let mut stmt = conn.prepare(&format!(
            "PRAGMA foreign_key_list(\"{}\")",
            child.name.replace('"', "\"\"")
        ))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(6)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (parent, column, to, action) in rows {
            if !is_synced(&parent) {
                continue;
            }
            let parent_column = match to {
                Some(c) => c,
                None => schema::primary_key(conn, &parent)?
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
            };
            let on_delete = match action.as_str() {
                "CASCADE" => OnDelete::Cascade,
                "SET NULL" => OnDelete::SetNull,
                _ => {
                    let rule = orphan_rules
                        .iter()
                        .find(|(t, c, _)| *t == child.name && *c == column)
                        .map(|(_, _, rule)| *rule)
                        .ok_or_else(|| {
                            AppError::Internal(format!(
                                "foreign key {}.{} -> {} has no orphan rule",
                                child.name, column, parent
                            ))
                        })?;
                    OnDelete::NoAction(rule)
                }
            };
            out.push(ForeignKey {
                child: child.name.to_string(),
                column,
                parent,
                parent_column,
                on_delete,
            });
        }
    }
    Ok(out)
}

/// Longest chain of Synced parents above each Synced table (0 = none); self-references ignored.
pub fn fk_depth(conn: &Connection) -> Result<BTreeMap<String, u32>> {
    let fks = foreign_keys(conn)?;
    let mut depth: BTreeMap<String, u32> = super::synced_tables()
        .map(|t| (t.name.to_string(), 0))
        .collect();
    let limit = depth.len() as u32 + 1;
    loop {
        let mut changed = false;
        for fk in fks.iter().filter(|f| f.child != f.parent) {
            let want = depth[&fk.parent] + 1;
            if depth[&fk.child] < want {
                if want > limit {
                    return Err(AppError::Internal(format!(
                        "foreign key cycle through {}",
                        fk.child
                    )));
                }
                depth.insert(fk.child.clone(), want);
                changed = true;
            }
        }
        if !changed {
            return Ok(depth);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationClass {
    Additive,
    Barrier,
}

use MigrationClass::{Additive as A, Barrier as B};

/// Class of every migration, in application order. Those up to 029 predate sync (history only).
pub const MIGRATION_CLASSES: &[(&str, MigrationClass)] = &[
    ("001_initial_schema", A),
    ("002_real_estate_purchase_date", A),
    ("003_stock_import_batches", A),
    ("004_stock_quote_currency", A),
    ("005_milestones", A),
    ("006_financial_semantics", A),
    ("007_report_history", A),
    ("008_import_integrity", A),
    ("009_scoped_learning", A),
    ("010_income_categories", B),
    ("011_fx_eur_reference", B),
    ("012_retire_czk_caches", B),
    ("013_cashflow_plan_exclusions", A),
    ("014_balance_records", B),
    ("015_one_tag_per_group", B),
    ("016_mcp_pending_refresh", A),
    ("017_profile_country", A),
    ("018_insurance_notice_days", A),
    ("019_loan_manual_records", B),
    ("020_bond_purchase_date", A),
    ("021_encrypted_attachments", B),
    ("022_profile_theme", A),
    ("023_stock_splits", A),
    ("024_categorize_snooze", A),
    ("025_crypto_yahoo_symbol", B),
    ("026_spending_plans", A),
    ("027_stock_dividends", A),
    ("028_transaction_fee", A),
    ("029_device_sync", A),
    ("030_sync_engine", A),
    ("031_sync_missed_below_barrier", A),
    ("032_sync_snapshot_too_large", A),
];

pub fn schema_number() -> u32 {
    crate::db::known_migration_names().len() as u32
}

/// 1-based position of the newest Barrier migration.
pub fn barrier_number() -> u32 {
    MIGRATION_CLASSES
        .iter()
        .rposition(|m| m.1 == MigrationClass::Barrier)
        .map_or(0, |i| i as u32 + 1)
}

/// `sql` with its `--` and `/* */` comments removed.
fn strip_sql_comments(sql: &str) -> String {
    let mut code = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('-', Some('-')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        code.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                code.push(' ');
            }
            _ => code.push(c),
        }
    }
    code
}

/// True when `sql` (outside SQL comments) calls `randomblob(` or `uuid(`.
pub fn uses_random_ids(sql: &str) -> bool {
    let lower = strip_sql_comments(sql).to_lowercase();
    lower.contains("randomblob(") || lower.contains("uuid(")
}

/// SQL functions and keywords whose value differs from one device to the next.
const VOLATILE: [&str; 8] = [
    "unixepoch(",
    "datetime(",
    "strftime(",
    "julianday(",
    "random(",
    "randomblob(",
    "current_timestamp",
    "current_date",
];

/// True when a statement of `sql` (outside comments) that inserts into or updates one of the
/// `synced` tables uses a value that differs per device (a clock, a random number). Every device
/// runs a migration on its own and nothing is captured, so such a seed would diverge.
pub fn seeds_with_volatile_values(sql: &str, synced: &[&str]) -> bool {
    let code = strip_sql_comments(sql).to_lowercase();
    code.split(';').any(|statement| {
        let words: Vec<&str> = statement
            .split(|c: char| c.is_whitespace() || c == '(')
            .filter(|w| !w.is_empty())
            .collect();
        let target = match words.as_slice() {
            ["insert", rest @ ..] => rest
                .iter()
                .position(|w| *w == "into")
                .and_then(|i| rest.get(i + 1)),
            ["update", rest @ ..] => rest.iter().find(|w| **w != "or" && !is_conflict_word(w)),
            _ => None,
        };
        target.is_some_and(|t| synced.contains(t)) && VOLATILE.iter().any(|v| statement.contains(v))
    })
}

fn is_conflict_word(word: &str) -> bool {
    matches!(word, "rollback" | "abort" | "replace" | "fail" | "ignore")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::registry::{spec, synced_tables, TableClass};
    use crate::services::sync::test_support::migrated;
    use std::collections::BTreeSet;

    fn synced_unique_indexes(conn: &rusqlite::Connection) -> Vec<(String, String, Vec<String>)> {
        let mut out = Vec::new();
        for t in synced_tables() {
            let mut stmt = conn
                .prepare(&format!("PRAGMA index_list(\"{}\")", t.name))
                .unwrap();
            let idx: Vec<(String, i64, String)> = stmt
                .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            for (name, unique, origin) in idx {
                if unique == 0 || origin == "pk" {
                    continue;
                }
                let mut info = conn
                    .prepare(&format!("PRAGMA index_info(\"{name}\")"))
                    .unwrap();
                let cols: Vec<String> = info
                    .query_map([], |r| r.get::<_, Option<String>>(2))
                    .unwrap()
                    .map(|r| r.unwrap().unwrap_or_default())
                    .collect();
                out.push((t.name.to_string(), name, cols));
            }
        }
        out
    }

    #[test]
    fn every_unique_index_of_a_synced_table_has_a_rule() {
        let conn = migrated();
        for (table, index, cols) in synced_unique_indexes(&conn) {
            let found = unique_rules().iter().any(|u| {
                u.table == table
                    && if cols.iter().any(|c| c.is_empty()) {
                        index == "idx_learned_payees_scoped"
                    } else {
                        u.exprs.iter().map(|e| e.to_string()).collect::<Vec<_>>() == cols
                    }
            });
            assert!(
                found,
                "unique index `{index}` on `{table}` {cols:?} has no rule"
            );
        }
    }

    #[test]
    fn every_rule_names_an_existing_unique_index() {
        let conn = migrated();
        let indexes = synced_unique_indexes(&conn);
        for u in unique_rules() {
            let found = indexes.iter().any(|(table, index, cols)| {
                *table == u.table
                    && if cols.iter().any(|c| c.is_empty()) {
                        index == "idx_learned_payees_scoped"
                    } else {
                        u.exprs.iter().map(|e| e.to_string()).collect::<Vec<_>>() == *cols
                    }
            });
            assert!(
                found,
                "rule on `{}` {:?} names no unique index",
                u.table, u.exprs
            );
            assert!(spec(u.table).is_some_and(|s| s.class == TableClass::Synced));
        }
    }

    #[test]
    fn every_unique_index_has_its_ruled_outcome() {
        let rules: Vec<(&str, UniqueRule)> =
            unique_rules().iter().map(|u| (u.table, u.rule)).collect();
        assert_eq!(
            rules,
            vec![
                ("stock_investments", UniqueRule::SameId),
                ("crypto_investments", UniqueRule::SameId),
                ("watched_stocks", UniqueRule::SameId),
                ("stock_price_overrides", UniqueRule::SameId),
                ("dividend_overrides", UniqueRule::SameId),
                ("crypto_price_overrides", UniqueRule::SameId),
                ("stock_splits", UniqueRule::SameId),
                ("projection_settings", UniqueRule::SameId),
                ("bank_transactions", UniqueRule::SameId),
                ("learned_payees", UniqueRule::SameId),
                ("bank_balance_records", UniqueRule::SameId),
                ("budget_goal_versions", UniqueRule::SameId),
                ("stock_tags", UniqueRule::Rename),
                ("stock_tag_groups", UniqueRule::Rename),
                ("cashflow_items", UniqueRule::Unlink),
            ]
        );
    }

    #[test]
    fn the_fk_graph_comes_from_pragma_with_its_on_delete_rules() {
        let conn = migrated();
        let fks = foreign_keys(&conn).unwrap();
        assert!(fks.iter().any(|f| f.child == "bank_transactions"
            && f.column == "spending_plan_id"
            && matches!(f.on_delete, OnDelete::SetNull)));
        assert!(fks.iter().any(|f| f.child == "investment_transactions"
            && f.column == "investment_id"
            && matches!(f.on_delete, OnDelete::Cascade)));
    }

    #[test]
    fn a_no_action_key_without_an_orphan_rule_is_an_error() {
        let conn = migrated();
        let err = foreign_keys_with(&conn, &ORPHAN_RULES[1..])
            .unwrap_err()
            .to_string();
        assert!(err.contains(ORPHAN_RULES[0].0) && err.contains(ORPHAN_RULES[0].1));
        assert!(foreign_keys(&conn).is_ok());
    }

    #[test]
    fn a_required_column_cannot_be_set_null() {
        let conn = migrated();
        for fk in foreign_keys(&conn).unwrap() {
            let OnDelete::NoAction(rule) = fk.on_delete else {
                continue;
            };
            let col = crate::services::sync::registry::schema::columns(&conn, &fk.child)
                .unwrap()
                .into_iter()
                .find(|c| c.name == fk.column)
                .unwrap();
            let expected = if col.not_null {
                OrphanRule::DropChild
            } else {
                OrphanRule::SetNull
            };
            assert_eq!(rule, expected, "{}.{}", fk.child, fk.column);
        }
    }

    #[test]
    fn depth_orders_every_parent_before_its_children() {
        let conn = migrated();
        let d = fk_depth(&conn).unwrap();
        assert_eq!(d["bank_transactions"], 3);
        assert_eq!(d["csv_import_batches"], 2);
        for fk in foreign_keys(&conn).unwrap() {
            if fk.child != fk.parent {
                assert!(
                    d[&fk.parent] < d[&fk.child],
                    "{} -> {}",
                    fk.parent,
                    fk.child
                );
            }
        }
    }

    #[test]
    fn every_migration_has_a_class_and_no_class_is_stale() {
        assert_eq!(
            MIGRATION_CLASSES.iter().map(|m| m.0).collect::<Vec<_>>(),
            crate::db::known_migration_names()
        );
        let barriers: BTreeSet<&str> = MIGRATION_CLASSES
            .iter()
            .filter(|m| m.1 == MigrationClass::Barrier)
            .map(|m| m.0)
            .collect();
        assert_eq!(
            barriers.into_iter().collect::<Vec<_>>(),
            vec![
                "010_income_categories",
                "011_fx_eur_reference",
                "012_retire_czk_caches",
                "014_balance_records",
                "015_one_tag_per_group",
                "019_loan_manual_records",
                "021_encrypted_attachments",
                "025_crypto_yahoo_symbol",
            ]
        );
    }

    #[test]
    fn the_random_id_scanner_flags_random_ids_and_ignores_comments() {
        assert!(uses_random_ids(
            "INSERT INTO t (id) SELECT lower(hex(randomblob(16)));"
        ));
        assert!(!uses_random_ids(
            "-- randomblob( is forbidden\nINSERT INTO t (id) VALUES ('seed-1');"
        ));
        assert!(uses_random_ids(
            crate::db::migration_sql("002_real_estate_purchase_date").unwrap()
        ));
    }

    #[test]
    fn migrations_after_029_insert_synced_rows_only_with_deterministic_ids() {
        for name in crate::db::known_migration_names()
            .into_iter()
            .filter(|n| *n > "029_device_sync")
        {
            assert!(
                !uses_random_ids(crate::db::migration_sql(name).unwrap()),
                "{name}"
            );
        }
        assert!(crate::db::migration_sql("029_device_sync").is_some());
    }

    #[test]
    fn migrations_after_029_seed_synced_rows_with_literal_values() {
        let synced: Vec<&str> = synced_tables().map(|t| t.name).collect();
        assert!(synced.contains(&"institutions"));
        assert!(seeds_with_volatile_values(
            "INSERT INTO institutions (id, created_at) VALUES ('x', unixepoch())",
            &synced
        ));
        assert!(!seeds_with_volatile_values(
            "INSERT INTO institutions (id, created_at) VALUES ('x', 1767225600)",
            &synced
        ));
        assert!(seeds_with_volatile_values(
            "UPDATE institutions SET updated_at = CURRENT_TIMESTAMP",
            &synced
        ));
        assert!(!seeds_with_volatile_values(
            "-- unixepoch( is volatile\nINSERT INTO institutions (id) VALUES ('x');",
            &synced
        ));
        assert!(
            !seeds_with_volatile_values(
                "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('a', 'b', unixepoch())",
                &synced
            ),
            "a Local table may use the clock"
        );
        for name in crate::db::known_migration_names()
            .into_iter()
            .filter(|n| *n > "029_device_sync")
        {
            assert!(
                !seeds_with_volatile_values(crate::db::migration_sql(name).unwrap(), &synced),
                "{name}"
            );
        }
    }

    #[test]
    fn schema_number_is_32_and_the_newest_barrier_is_025() {
        assert_eq!(schema_number(), 32);
        assert_eq!(barrier_number(), 25);
    }
}
