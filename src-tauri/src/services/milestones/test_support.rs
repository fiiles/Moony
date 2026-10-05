//! In-memory schema with the tables the milestone builders read.

use rusqlite::Connection;

/// UTC midnight of a calendar date, in unix seconds.
pub(crate) fn day(y: i32, m: u32, d: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(y, m, d)
        .expect("valid date")
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
        .timestamp()
}

pub(crate) fn setup() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE user_profile (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE insurance_policies (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL DEFAULT 'other',
            provider TEXT NOT NULL DEFAULT '',
            policy_name TEXT NOT NULL,
            policy_number TEXT NOT NULL DEFAULT '',
            start_date INTEGER NOT NULL,
            end_date INTEGER,
            payment_frequency TEXT NOT NULL,
            one_time_payment TEXT,
            one_time_payment_currency TEXT DEFAULT 'CZK',
            regular_payment TEXT NOT NULL DEFAULT '0',
            regular_payment_currency TEXT NOT NULL DEFAULT 'CZK',
            limits TEXT DEFAULT '[]',
            notes TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE loans (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            principal TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'CZK',
            interest_rate TEXT NOT NULL DEFAULT '0',
            interest_rate_validity_date INTEGER,
            monthly_payment TEXT NOT NULL DEFAULT '0',
            start_date INTEGER NOT NULL DEFAULT (unixepoch()),
            end_date INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
            balance_anchor_amount TEXT,
            balance_anchor_date INTEGER
        );
        CREATE TABLE bonds (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            isin TEXT,
            coupon_value TEXT NOT NULL,
            interest_rate TEXT NOT NULL DEFAULT '0',
            maturity_date INTEGER,
            currency TEXT NOT NULL DEFAULT 'CZK',
            quantity TEXT NOT NULL DEFAULT '1',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE bank_accounts (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            termination_date INTEGER,
            interest_rate_valid_until INTEGER,
            exclude_from_balance INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE real_estate (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            purchase_date INTEGER,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE real_estate_valuations (
            id TEXT PRIMARY KEY,
            real_estate_id TEXT NOT NULL,
            value TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'CZK',
            valued_at INTEGER NOT NULL,
            note TEXT,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE watched_stocks (
            id TEXT PRIMARY KEY,
            ticker TEXT NOT NULL UNIQUE,
            target_price TEXT,
            target_direction TEXT,
            notes TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL DEFAULT (unixepoch()),
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        CREATE TABLE stock_data (
            id TEXT PRIMARY KEY,
            ticker TEXT UNIQUE,
            original_price TEXT,
            currency TEXT
        );
        CREATE TABLE milestone_states (
            key TEXT PRIMARY KEY,
            state TEXT NOT NULL CHECK (state IN ('done', 'snoozed')),
            until_day INTEGER,
            updated_at INTEGER NOT NULL DEFAULT (unixepoch())
        );
        "#,
    )
    .expect("schema");
    conn
}
