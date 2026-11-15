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
    for (t, c) in [
        ("loans", "interest_rate"),
        ("loans", "monthly_payment"),
        ("loan_events", "previous_rate"),
        ("loan_events", "previous_monthly_payment"),
    ] {
        assert_eq!(column_class(t, c), ColumnClass::Synced, "{t}.{c}");
    }
    assert_eq!(
        column_class("loans", "balance_anchor_amount"),
        ColumnClass::Recomputed
    );
}

fn want<'a>(m: &[(&str, &[&'a str])], table: &str) -> Vec<&'a str> {
    m.iter()
        .find(|(n, _)| *n == table)
        .map(|(_, c)| sorted(c))
        .unwrap_or_default()
}

fn names_of(class: TableClass) -> Vec<&'static str> {
    let mut v: Vec<_> = tables()
        .iter()
        .filter(|t| t.class == class)
        .map(|t| t.name)
        .collect();
    v.sort_unstable();
    v
}

fn sorted<'a>(list: &[&'a str]) -> Vec<&'a str> {
    let mut v = list.to_vec();
    v.sort_unstable();
    v
}

#[test]
fn the_exact_tables_of_each_class_are_pinned() {
    let synced = [
        "bank_accounts",
        "bank_account_zones",
        "bank_balance_records",
        "bank_transactions",
        "csv_import_batches",
        "institutions",
        "transaction_categories",
        "categorization_rules",
        "learned_payees",
        "budget_goal_versions",
        "spending_plans",
        "stock_investments",
        "investment_transactions",
        "investment_dividends",
        "stock_import_batches",
        "stock_splits",
        "watched_stocks",
        "stock_price_overrides",
        "dividend_overrides",
        "stock_tags",
        "stock_tag_groups",
        "stock_investment_tags",
        "crypto_investments",
        "crypto_transactions",
        "crypto_price_overrides",
        "bonds",
        "loans",
        "loan_events",
        "real_estate",
        "real_estate_one_time_costs",
        "real_estate_valuations",
        "real_estate_loans",
        "real_estate_insurances",
        "insurance_policies",
        "other_assets",
        "other_asset_transactions",
        "other_asset_valuations",
        "cashflow_items",
        "cashflow_plan_exclusions",
        "projection_settings",
        "milestone_states",
        "user_profile",
        "app_config",
        "sync_devices",
    ];
    let seed = [
        "portfolio_metrics_history",
        "stock_value_history",
        "crypto_value_history",
        "exchange_rates",
        "exchange_rate_history",
        "stock_data",
        "crypto_prices",
        "dividend_data",
        "stock_dividend_events",
        "stock_dividend_coverage",
    ];
    let derived = ["budget_goals"];
    let local = [
        "_migrations",
        "asset_logos",
        "attachment_leftovers",
        "loan_legacy_anchors",
        "mcp_pending_refresh",
        "real_estate_photo_batches",
        "real_estate_photos",
        "real_estate_documents",
        "insurance_documents",
        "sync_blobs",
        "sync_state",
        "sync_clock",
        "sync_outbox",
        "sync_heads",
        "sync_own_log",
        "sync_unsent",
        "sync_typed",
    ];
    assert_eq!(names_of(TableClass::Synced), sorted(&synced));
    assert_eq!(names_of(TableClass::Seed), sorted(&seed));
    assert_eq!(names_of(TableClass::Derived), sorted(&derived));
    assert_eq!(names_of(TableClass::Local), sorted(&local));
}

#[test]
fn user_profile_local_columns_are_pinned() {
    assert_eq!(
        sorted(spec("user_profile").unwrap().local),
        sorted(&[
            "id",
            "language",
            "theme",
            "menu_preferences",
            "mcp_server_enabled",
            "mcp_server_token",
            "mcp_server_port",
            "created_at",
        ])
    );
}

#[test]
fn local_recomputed_and_recomputed_or_typed_sets_are_pinned() {
    let local: &[(&str, &[&str])] = &[
        (
            "user_profile",
            &[
                "id",
                "language",
                "theme",
                "menu_preferences",
                "mcp_server_enabled",
                "mcp_server_token",
                "mcp_server_port",
                "created_at",
            ],
        ),
        ("bank_accounts", &["updated_at"]),
        ("loans", &["updated_at"]),
        ("real_estate", &["updated_at", "photos"]),
        ("other_assets", &["updated_at"]),
    ];
    let recomputed: &[(&str, &[&str])] = &[
        ("loans", &["balance_anchor_amount", "balance_anchor_date"]),
        // Ruling T18c: current split units are derived from the entry values.
        (
            "investment_transactions",
            &["price_per_unit", "quantity", "split_basis"],
        ),
        ("stock_price_overrides", &["price", "split_basis"]),
        ("watched_stocks", &["split_basis", "target_price"]),
        (
            "dividend_overrides",
            &["split_basis", "yearly_dividend_sum"],
        ),
    ];
    type Source<'a> = (&'a [&'a str], &'a str, &'a str);
    let rot: &[(&str, &[Source])] = &[
        (
            "bank_accounts",
            &[(
                &["balance", "balance_as_of"],
                "bank_balance_records",
                "bank_account_id",
            )],
        ),
        (
            "real_estate",
            &[(
                &["market_price", "market_price_currency"],
                "real_estate_valuations",
                "real_estate_id",
            )],
        ),
        (
            "other_assets",
            &[
                (&["market_price"], "other_asset_valuations", "asset_id"),
                (
                    &["quantity", "average_purchase_price"],
                    "other_asset_transactions",
                    "asset_id",
                ),
            ],
        ),
        (
            "stock_investments",
            &[(&["quantity"], "investment_transactions", "investment_id")],
        ),
        (
            "crypto_investments",
            &[(
                &["quantity", "currency"],
                "crypto_transactions",
                "investment_id",
            )],
        ),
    ];
    for t in tables() {
        assert_eq!(sorted(t.local), want(local, t.name), "{} local", t.name);
        assert_eq!(
            sorted(t.recomputed),
            want(recomputed, t.name),
            "{} recomputed",
            t.name
        );
        let got: Vec<(Vec<&str>, &str, &str)> = t
            .recomputed_or_typed
            .iter()
            .map(|(c, s)| (c.to_vec(), s.table, s.fk))
            .collect();
        let exp: Vec<(Vec<&str>, &str, &str)> = rot
            .iter()
            .find(|(n, _)| *n == t.name)
            .map(|(_, l)| l.iter().map(|(c, tb, fk)| (c.to_vec(), *tb, *fk)).collect())
            .unwrap_or_default();
        assert_eq!(got, exp, "{} recomputed_or_typed", t.name);
        assert!(t.write_once.is_empty() || t.name == "sync_devices");
        assert!(t.monotonic.is_empty() || t.name == "stock_splits");
    }
    assert_eq!(spec("sync_devices").unwrap().write_once, &["public_key"]);
    assert_eq!(spec("stock_splits").unwrap().monotonic, &["status"]);
}

#[test]
fn the_exact_column_groups_are_pinned() {
    let amount = &[&["amount", "currency"][..]][..];
    let expected: &[(&str, &[&[&str]])] = &[
        ("bank_balance_records", amount),
        ("bank_transactions", amount),
        ("budget_goal_versions", amount),
        ("spending_plans", amount),
        ("cashflow_items", amount),
        ("real_estate_one_time_costs", amount),
        ("crypto_price_overrides", &[&["price", "currency"]]),
        (
            "crypto_transactions",
            &[&["quantity", "price_per_unit", "currency"]],
        ),
        (
            "other_asset_transactions",
            &[&["quantity", "price_per_unit", "currency"]],
        ),
        (
            "investment_transactions",
            &[&[
                "entry_quantity",
                "entry_price_per_unit",
                "fee",
                "currency",
                "entry_basis",
            ]],
        ),
        (
            "stock_price_overrides",
            &[&["entry_price", "currency", "entry_basis", "entry_at"]],
        ),
        (
            "dividend_overrides",
            &[&[
                "entry_yearly_dividend_sum",
                "currency",
                "entry_basis",
                "entry_at",
            ]],
        ),
        (
            "watched_stocks",
            &[&[
                "entry_target_price",
                "target_direction",
                "entry_basis",
                "entry_at",
            ]],
        ),
        ("investment_dividends", &[&["gross_amount", "currency"]]),
        ("real_estate_valuations", &[&["value", "currency"]]),
        ("other_asset_valuations", &[&["value", "currency"]]),
        (
            "real_estate",
            &[
                &["purchase_price", "purchase_price_currency"],
                &["monthly_rent", "monthly_rent_currency"],
                &["market_price", "market_price_currency"],
            ],
        ),
        (
            "insurance_policies",
            &[
                &["one_time_payment", "one_time_payment_currency"],
                &["regular_payment", "regular_payment_currency"],
            ],
        ),
        ("loans", &[&["principal", "currency"]]),
        ("bonds", &[&["coupon_value", "currency"]]),
        (
            "projection_settings",
            &[&["monthly_contribution", "contribution_currency"]],
        ),
        // recomputed-or-typed lists are groups too
        ("bank_accounts", &[&["balance", "balance_as_of"]]),
        (
            "other_assets",
            &[&["market_price"], &["quantity", "average_purchase_price"]],
        ),
        ("stock_investments", &[&["quantity"]]),
        ("crypto_investments", &[&["quantity", "currency"]]),
    ];
    for t in tables() {
        let got: Vec<Vec<&str>> = t.groups.iter().map(|g| g.to_vec()).collect();
        let exp: Vec<Vec<&str>> = expected
            .iter()
            .find(|(n, _)| *n == t.name)
            .map(|(_, g)| g.iter().map(|x| x.to_vec()).collect())
            .unwrap_or_default();
        assert_eq!(got, exp, "{} groups", t.name);
    }
}

#[test]
fn key_arity_matches_the_primary_key_of_every_synced_table() {
    let conn = migrated();
    for t in synced_tables() {
        assert_eq!(
            key_arity(t.name),
            schema::primary_key(&conn, t.name).unwrap().len(),
            "{}",
            t.name
        );
    }
}

#[test]
fn key_only_tables_match_the_schema() {
    let conn = migrated();
    for t in synced_tables() {
        let all_key = schema::columns(&conn, t.name)
            .unwrap()
            .iter()
            .all(|c| c.pk_position > 0);
        assert_eq!(is_key_only(t.name), all_key, "{}", t.name);
    }
}
