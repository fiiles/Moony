//! The static registry data: every table of the main schema and its column classes.

use super::{KeyShape, SourceRows, TableClass, TableSpec};

/// `app_config` keys that sync; every other key stays on the device.
pub const APP_CONFIG_SYNCED_KEYS: [&str; 5] = [
    "stock_import_formats",
    "learned_csv_formats",
    "milestones.mutedKinds",
    "rule_packs_enabled",
    "rule_packs_disabled_rules",
];

const BASE: TableSpec = TableSpec {
    name: "",
    class: TableClass::Synced,
    key: KeyShape::Primary,
    local: &[],
    recomputed: &[],
    recomputed_or_typed: &[],
    groups: &[],
    write_once: &[],
    monotonic: &[],
};

pub(super) static TABLES: &[TableSpec] = &[
    TableSpec {
        name: "bank_accounts",
        class: TableClass::Synced,
        local: &["updated_at"],
        recomputed_or_typed: &[(
            &["balance", "balance_as_of"],
            SourceRows {
                table: "bank_balance_records",
                fk: "bank_account_id",
            },
        )],
        groups: &[&["balance", "balance_as_of"]],
        ..BASE
    },
    TableSpec {
        name: "bank_account_zones",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "bank_balance_records",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "bank_transactions",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "csv_import_batches",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "institutions",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "transaction_categories",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "categorization_rules",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "learned_payees",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "budget_goal_versions",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "spending_plans",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "stock_investments",
        class: TableClass::Synced,
        recomputed_or_typed: &[(
            &["quantity"],
            SourceRows {
                table: "investment_transactions",
                fk: "investment_id",
            },
        )],
        groups: &[&["quantity"]],
        ..BASE
    },
    TableSpec {
        name: "investment_transactions",
        class: TableClass::Synced,
        groups: &[&[
            "quantity",
            "price_per_unit",
            "fee",
            "currency",
            "split_basis",
        ]],
        ..BASE
    },
    TableSpec {
        name: "investment_dividends",
        class: TableClass::Synced,
        groups: &[&["gross_amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "stock_import_batches",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "stock_splits",
        class: TableClass::Synced,
        monotonic: &["status"],
        ..BASE
    },
    TableSpec {
        name: "watched_stocks",
        class: TableClass::Synced,
        groups: &[&["target_price", "target_direction", "split_basis"]],
        ..BASE
    },
    TableSpec {
        name: "stock_price_overrides",
        class: TableClass::Synced,
        groups: &[&["price", "currency", "split_basis"]],
        ..BASE
    },
    TableSpec {
        name: "dividend_overrides",
        class: TableClass::Synced,
        groups: &[&["yearly_dividend_sum", "currency", "split_basis"]],
        ..BASE
    },
    TableSpec {
        name: "stock_tags",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "stock_tag_groups",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "stock_investment_tags",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "crypto_investments",
        class: TableClass::Synced,
        recomputed_or_typed: &[(
            &["quantity", "currency"],
            SourceRows {
                table: "crypto_transactions",
                fk: "investment_id",
            },
        )],
        groups: &[&["quantity", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "crypto_transactions",
        class: TableClass::Synced,
        groups: &[&["quantity", "price_per_unit", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "crypto_price_overrides",
        class: TableClass::Synced,
        groups: &[&["price", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "bonds",
        class: TableClass::Synced,
        groups: &[&["coupon_value", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "loans",
        class: TableClass::Synced,
        local: &["updated_at"],
        recomputed: &["balance_anchor_amount", "balance_anchor_date"],
        groups: &[&["principal", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "loan_events",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "real_estate",
        class: TableClass::Synced,
        local: &["updated_at", "photos"],
        recomputed_or_typed: &[(
            &["market_price", "market_price_currency"],
            SourceRows {
                table: "real_estate_valuations",
                fk: "real_estate_id",
            },
        )],
        groups: &[
            &["purchase_price", "purchase_price_currency"],
            &["monthly_rent", "monthly_rent_currency"],
            &["market_price", "market_price_currency"],
        ],
        ..BASE
    },
    TableSpec {
        name: "real_estate_one_time_costs",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "real_estate_valuations",
        class: TableClass::Synced,
        groups: &[&["value", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "real_estate_loans",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "real_estate_insurances",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "insurance_policies",
        class: TableClass::Synced,
        groups: &[
            &["one_time_payment", "one_time_payment_currency"],
            &["regular_payment", "regular_payment_currency"],
        ],
        ..BASE
    },
    TableSpec {
        name: "other_assets",
        class: TableClass::Synced,
        local: &["updated_at"],
        recomputed_or_typed: &[
            (
                &["market_price"],
                SourceRows {
                    table: "other_asset_valuations",
                    fk: "asset_id",
                },
            ),
            (
                &["quantity", "average_purchase_price"],
                SourceRows {
                    table: "other_asset_transactions",
                    fk: "asset_id",
                },
            ),
        ],
        groups: &[&["market_price"], &["quantity", "average_purchase_price"]],
        ..BASE
    },
    TableSpec {
        name: "other_asset_transactions",
        class: TableClass::Synced,
        groups: &[&["quantity", "price_per_unit", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "other_asset_valuations",
        class: TableClass::Synced,
        groups: &[&["value", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "cashflow_items",
        class: TableClass::Synced,
        groups: &[&["amount", "currency"]],
        ..BASE
    },
    TableSpec {
        name: "cashflow_plan_exclusions",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "projection_settings",
        class: TableClass::Synced,
        groups: &[&["monthly_contribution", "contribution_currency"]],
        ..BASE
    },
    TableSpec {
        name: "milestone_states",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "user_profile",
        class: TableClass::Synced,
        key: KeyShape::Singleton("profile"),
        local: &[
            "id",
            "language",
            "theme",
            "menu_preferences",
            "mcp_server_enabled",
            "mcp_server_token",
            "mcp_server_port",
            "created_at",
        ],
        ..BASE
    },
    TableSpec {
        name: "app_config",
        class: TableClass::Synced,
        ..BASE
    },
    TableSpec {
        name: "sync_devices",
        class: TableClass::Synced,
        write_once: &["public_key"],
        ..BASE
    },
    // Seed: copied into a new device's snapshot, never merged.
    TableSpec {
        name: "portfolio_metrics_history",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "stock_value_history",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "crypto_value_history",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "exchange_rates",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "exchange_rate_history",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "stock_data",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "crypto_prices",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "dividend_data",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "stock_dividend_events",
        class: TableClass::Seed,
        ..BASE
    },
    TableSpec {
        name: "stock_dividend_coverage",
        class: TableClass::Seed,
        ..BASE
    },
    // Derived: rebuilt locally after every merge.
    TableSpec {
        name: "budget_goals",
        class: TableClass::Derived,
        ..BASE
    },
    // Local: never leaves the device (the attachment tables moved here by spec 16a).
    TableSpec {
        name: "_migrations",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "attachment_leftovers",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "loan_legacy_anchors",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "mcp_pending_refresh",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "real_estate_photo_batches",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "real_estate_photos",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "real_estate_documents",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "insurance_documents",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_state",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_clock",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_outbox",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_heads",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_own_log",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_unsent",
        class: TableClass::Local,
        ..BASE
    },
    TableSpec {
        name: "sync_renamed",
        class: TableClass::Local,
        ..BASE
    },
];
