use std::collections::HashSet;

use super::*;
use crate::services::sync::test_support::migrated;

fn schema_tables(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn columns_of(conn: &rusqlite::Connection, table: &str) -> Vec<schema::ColumnInfo> {
    schema::columns(conn, table).unwrap()
}

#[test]
fn every_table_of_the_migrated_schema_has_a_class() {
    let conn = migrated();
    for name in schema_tables(&conn) {
        assert!(
            spec(&name).is_some(),
            "table `{name}` has no class in the sync registry"
        );
    }
}

#[test]
fn every_registered_table_exists() {
    let conn = migrated();
    let existing: HashSet<String> = schema_tables(&conn).into_iter().collect();
    for t in tables() {
        assert!(
            existing.contains(t.name),
            "registry names missing table `{}`",
            t.name
        );
    }
    let names: HashSet<_> = tables().iter().map(|t| t.name).collect();
    assert_eq!(names.len(), tables().len(), "a table is registered twice");
}

#[test]
fn every_named_column_exists() {
    let conn = migrated();
    for t in tables() {
        let cols: HashSet<String> = columns_of(&conn, t.name)
            .into_iter()
            .map(|c| c.name)
            .collect();
        let mut named: Vec<&str> = Vec::new();
        named.extend(t.local);
        named.extend(t.recomputed);
        named.extend(t.write_once);
        named.extend(t.monotonic);
        for (list, _) in t.recomputed_or_typed {
            named.extend(*list);
        }
        for g in t.groups {
            named.extend(*g);
        }
        for c in named {
            assert!(cols.contains(c), "{}.{c} does not exist", t.name);
        }
        for (_, src) in t.recomputed_or_typed {
            let src_cols: HashSet<String> = columns_of(&conn, src.table)
                .into_iter()
                .map(|c| c.name)
                .collect();
            assert!(
                src_cols.contains(src.fk),
                "{}.{} does not exist",
                src.table,
                src.fk
            );
        }
    }
}

#[test]
fn the_attachment_tables_are_local_in_v1() {
    for t in [
        "real_estate_photo_batches",
        "real_estate_photos",
        "real_estate_documents",
        "insurance_documents",
    ] {
        assert_eq!(spec(t).unwrap().class, TableClass::Local, "{t}");
    }
}

#[test]
fn no_synced_column_is_real() {
    let conn = migrated();
    for t in tables().iter().filter(|t| t.class != TableClass::Seed) {
        for c in columns_of(&conn, t.name) {
            assert!(
                !c.decl_type.to_uppercase().contains("REAL"),
                "{}.{} is REAL outside a Seed table",
                t.name,
                c.name
            );
        }
    }
}

#[test]
fn every_local_not_null_column_of_a_synced_table_has_a_default() {
    let conn = migrated();
    for t in synced_tables() {
        for c in columns_of(&conn, t.name) {
            if column_class(t.name, &c.name) == ColumnClass::Local && c.not_null {
                assert!(
                    c.has_default,
                    "{}.{} is Local NOT NULL without a DEFAULT",
                    t.name, c.name
                );
            }
        }
    }
}

#[test]
fn every_currency_column_is_grouped_or_listed_standalone() {
    let standalone = [
        ("bank_accounts", "currency"),
        ("stock_investments", "currency"),
        ("other_assets", "currency"),
        ("user_profile", "currency"),
    ];
    let conn = migrated();
    for t in synced_tables() {
        for c in columns_of(&conn, t.name) {
            if c.name.ends_with("currency") {
                let ok = group_of(t.name, &c.name).is_some()
                    || standalone.contains(&(t.name, c.name.as_str()))
                    || column_class(t.name, &c.name) == ColumnClass::Local;
                assert!(
                    ok,
                    "{}.{} is a currency column outside any group",
                    t.name, c.name
                );
            }
        }
    }
}

#[test]
fn a_column_belongs_to_at_most_one_group() {
    for t in tables() {
        let mut seen = HashSet::new();
        for g in t.groups {
            for c in *g {
                assert!(seen.insert(*c), "{}.{c} is in two groups", t.name);
            }
        }
    }
}

#[test]
fn user_profile_is_the_profile_singleton() {
    assert!(matches!(
        spec("user_profile").unwrap().key,
        KeyShape::Singleton("profile")
    ));
}

#[test]
fn the_app_config_allowlist_is_exactly_the_five_keys() {
    assert_eq!(
        APP_CONFIG_SYNCED_KEYS,
        [
            "stock_import_formats",
            "learned_csv_formats",
            "milestones.mutedKinds",
            "rule_packs_enabled",
            "rule_packs_disabled_rules"
        ]
    );
}

#[test]
fn the_four_recalculated_updated_at_columns_are_local() {
    for t in ["bank_accounts", "loans", "real_estate", "other_assets"] {
        assert_eq!(column_class(t, "updated_at"), ColumnClass::Local, "{t}");
    }
    assert_eq!(
        column_class("spending_plans", "updated_at"),
        ColumnClass::Synced
    );
}

#[test]
fn loan_terms_and_previous_terms_are_synced() {
    // Spec corrections 3 and 4: only the balance anchor is recomputed on loans.
    let conn = migrated();
    let mut checked = 0;
    for c in columns_of(&conn, "loans") {
        if c.name.starts_with("previous_") || c.name.contains("payment") || c.name.contains("rate")
        {
            assert_eq!(
                column_class("loans", &c.name),
                ColumnClass::Synced,
                "{}",
                c.name
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "expected loan terms columns, found {checked}");
    assert_eq!(
        column_class("loans", "balance_anchor_amount"),
        ColumnClass::Recomputed
    );
}
