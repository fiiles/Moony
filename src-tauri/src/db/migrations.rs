//! Database migrations for Moony
//!
//! The schema is an append-only list of `(name, sql)` migrations in
//! [`all_migrations`]. `001_initial_schema` is the hand-written baseline of the
//! whole data model: every table and index plus the seed rows of
//! `transaction_categories` and `institutions`. Later changes are appended as
//! new entries.
//!
//! Rules (docs/playbooks/add-db-migration.md):
//! - Migrations are append-only from the baseline: next number + Vec entry.
//! - Never edit or rename an applied migration. Databases track migrations by
//!   name and never re-run one that is recorded.
//! - `schema.snapshot` (same directory) records the schema and seed rows the
//!   chain produces. A test fails on any difference; after an intentional
//!   change regenerate it with
//!   `cargo test regenerate_schema_snapshot -- --ignored`.
//!
//! The runner distinguishes three states of the `_migrations` table:
//! - no rows: the whole chain runs;
//! - only names this build knows: the pending migrations run, each in its own
//!   transaction;
//! - any other name: the database comes from a pre-release build, so the
//!   runner returns an error and executes nothing.

use crate::error::{AppError, Result};
use rusqlite::Connection;

/// Run all database migrations
pub fn run_migrations(conn: &Connection) -> Result<()> {
    run_chain(conn, &all_migrations())
}

fn run_chain(conn: &Connection, migrations: &[(&str, &str)]) -> Result<()> {
    // Create migrations table to track applied migrations
    conn.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at INTEGER NOT NULL DEFAULT (unixepoch())
        )",
        [],
    )?;

    let applied = applied_migration_names(conn)?;
    if has_unknown_name(&applied, migrations) {
        return Err(AppError::Database(
            "This database was created by a pre-release build of Moony and cannot be opened by this version."
                .into(),
        ));
    }

    apply_pending(conn, migrations, &applied)
}

/// True when `applied` holds a name that is not part of `migrations`.
fn has_unknown_name(applied: &[String], migrations: &[(&str, &str)]) -> bool {
    applied
        .iter()
        .any(|name| !migrations.iter().any(|(known, _)| known == name))
}

/// The append-only migration chain, in application order.
fn all_migrations() -> Vec<(&'static str, &'static str)> {
    vec![("001_initial_schema", MIGRATION_001)]
}

/// Names of every migration this build knows, in application order.
pub fn known_migration_names() -> Vec<&'static str> {
    all_migrations().into_iter().map(|(name, _)| name).collect()
}

fn migrations_table_exists(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_migrations'",
        [],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

/// Names recorded in `_migrations` (empty when the table does not exist yet).
pub fn applied_migration_names(conn: &Connection) -> Result<Vec<String>> {
    if !migrations_table_exists(conn) {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT name FROM _migrations ORDER BY id")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// True when opening this database would run at least one migration. Used
/// by the opener to take a file copy *before* any DDL runs.
pub fn has_pending_migrations(conn: &Connection) -> Result<bool> {
    has_pending(conn, &all_migrations())
}

fn has_pending(conn: &Connection, migrations: &[(&str, &str)]) -> Result<bool> {
    if !migrations_table_exists(conn) {
        // Fresh database: the baseline will run, but there is nothing to back up.
        return Ok(false);
    }
    let applied = applied_migration_names(conn)?;
    if has_unknown_name(&applied, migrations) {
        // The runner refuses such a database without running anything, so
        // there is nothing to back up either.
        return Ok(false);
    }
    Ok(migrations
        .iter()
        .any(|(name, _)| !applied.iter().any(|a| a == name)))
}

/// Apply every migration not yet in `applied`, each in its own transaction:
/// a failing statement rolls back the whole migration and leaves `_migrations`
/// untouched, so the next start retries from a consistent state instead of
/// dying on half-applied DDL.
fn apply_pending(conn: &Connection, migrations: &[(&str, &str)], applied: &[String]) -> Result<()> {
    for (name, sql) in migrations {
        if applied.iter().any(|a| a == name) {
            continue;
        }
        log::info!("[MIGRATION] Applying: {}", name);
        let tx = conn.unchecked_transaction()?;
        let result = tx
            .execute_batch(sql)
            .and_then(|_| tx.execute("INSERT INTO _migrations (name) VALUES (?1)", [name]))
            .map(|_| ());
        if let Err(e) = result {
            drop(tx); // rolls back
            log::error!("[MIGRATION] FAILED: {} ({})", name, e);
            return Err(AppError::Database(format!(
                "Migration {name} failed and was rolled back: {e}"
            )));
        }
        tx.commit()?;
        log::info!("[MIGRATION] Applied: {}", name);
    }
    Ok(())
}

/// `001_initial_schema`: the complete data model. Append a new migration for
/// any change; do not edit this one.
const MIGRATION_001: &str = r#"
-- Moony initial schema: the complete data model, grouped by domain.
--
-- Conventions (docs/architecture/database.md): TEXT UUID primary keys,
-- INTEGER unix-epoch timestamps, money and decimals as TEXT, CZK as the base
-- currency. A table may reference a table defined further down: SQLite
-- resolves foreign keys when a row is written, not when the table is created.

-- ------------------------------------------------------------------------
-- Auth / profile
-- ------------------------------------------------------------------------

-- Key-value app settings (recovery-key hash, CoinGecko API key, ...).
CREATE TABLE IF NOT EXISTS app_config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Single-row user profile and preferences; also holds the local API server settings.
CREATE TABLE IF NOT EXISTS user_profile (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    surname TEXT NOT NULL,
    email TEXT NOT NULL,
    menu_preferences TEXT DEFAULT '{"loans":true,"insurance":true,"investments":true,"bonds":true,"realEstate":true}',
    currency TEXT NOT NULL DEFAULT 'CZK',
    exclude_personal_real_estate INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    language TEXT NOT NULL DEFAULT 'en',
    coingecko_modal_dismissed INTEGER NOT NULL DEFAULT 0,
    mcp_server_enabled INTEGER NOT NULL DEFAULT 0,
    mcp_server_token TEXT,
    mcp_server_port INTEGER
);

-- ------------------------------------------------------------------------
-- Bank accounts
-- ------------------------------------------------------------------------

-- Banks and other institutions; seeded below.
CREATE TABLE IF NOT EXISTS institutions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    bic TEXT,
    country TEXT,
    logo_url TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Seed: the institutions offered in the account form.
INSERT OR IGNORE INTO institutions (id, name, bic, country, logo_url) VALUES
    ('inst_air_bank', 'Air Bank', 'AIRACZPP', 'CZ', '/bank-logos/air-bank.svg'),
    ('inst_ceska_sporitelna', 'Česká spořitelna', 'GIBACZPX', 'CZ', '/bank-logos/ceska-sporitelna.svg'),
    ('inst_creditas', 'Banka CREDITAS', 'CTASCZ22', 'CZ', '/bank-logos/creditas.svg'),
    ('inst_csob', 'ČSOB', 'CEKOCZPP', 'CZ', '/bank-logos/csob.svg'),
    ('inst_fio', 'Fio banka', 'FIOBCZPP', 'CZ', '/bank-logos/fio.svg'),
    ('inst_ing', 'ING Bank', 'INGBCZPP', 'CZ', '/bank-logos/ing.svg'),
    ('inst_jt_banka', 'J&T Banka', 'JTBPCZPP', 'CZ', '/bank-logos/jt-banka.svg'),
    ('inst_komercni_banka', 'Komerční banka', 'KOMBCZPP', 'CZ', '/bank-logos/komercni-banka.svg'),
    ('inst_max_banka', 'MAX banka', 'EXPNCZPP', 'CZ', '/bank-logos/max-banka.svg'),
    ('inst_moneta', 'MONETA Money Bank', 'AGBACZPP', 'CZ', '/bank-logos/moneta.svg'),
    ('inst_nrb', 'Národní rozvojová banka', 'NROZCZPP', 'CZ', '/bank-logos/nrb.svg'),
    ('inst_other', 'Other', NULL, NULL, NULL),
    ('inst_ppf', 'PPF banka', 'PMBPCZPP', 'CZ', '/bank-logos/ppf.svg'),
    ('inst_raiffeisenbank', 'Raiffeisenbank', 'RZBCCZPP', 'CZ', '/bank-logos/raiffeisenbank.svg'),
    ('inst_revolut', 'Revolut', 'REVOGB21', 'GB', '/bank-logos/revolut.svg'),
    ('inst_trinity', 'Trinity Bank', 'MCEKCZPP', 'CZ', '/bank-logos/trinity.svg'),
    ('inst_unicredit', 'UniCredit Bank', 'BACXCZPP', 'CZ', '/bank-logos/unicredit.svg'),
    ('inst_wise', 'Wise', 'TRWIBEB1XXX', 'BE', '/bank-logos/wise.svg');

CREATE TABLE IF NOT EXISTS bank_accounts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    account_type TEXT NOT NULL DEFAULT 'checking',
    iban TEXT,
    bban TEXT,
    currency TEXT NOT NULL DEFAULT 'CZK',
    balance TEXT NOT NULL DEFAULT '0',
    institution_id TEXT REFERENCES institutions(id),
    external_account_id TEXT,
    data_source TEXT NOT NULL DEFAULT 'manual',
    last_synced_at INTEGER,
    interest_rate TEXT,
    has_zone_designation INTEGER NOT NULL DEFAULT 0,
    termination_date INTEGER,
    exclude_from_balance INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_bank_accounts_institution ON bank_accounts(institution_id);
CREATE INDEX IF NOT EXISTS idx_bank_accounts_type ON bank_accounts(account_type);

-- Tiered interest-rate bands of an account balance.
CREATE TABLE IF NOT EXISTS bank_account_zones (
    id TEXT PRIMARY KEY,
    bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
    from_amount TEXT NOT NULL,
    to_amount TEXT,
    interest_rate TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- One row per CSV upload; deleting a batch removes its transactions.
CREATE TABLE IF NOT EXISTS csv_import_batches (
    id TEXT PRIMARY KEY,
    bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
    file_name TEXT NOT NULL,
    imported_count INTEGER NOT NULL DEFAULT 0,
    duplicate_count INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    imported_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_csv_import_batches_account ON csv_import_batches(bank_account_id);

CREATE TABLE IF NOT EXISTS bank_transactions (
    id TEXT PRIMARY KEY,
    bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
    transaction_id TEXT,
    tx_type TEXT NOT NULL,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL,
    description TEXT,
    counterparty_name TEXT,
    counterparty_iban TEXT,
    booking_date INTEGER NOT NULL,
    value_date INTEGER,
    category_id TEXT REFERENCES transaction_categories(id),
    merchant_category_code TEXT,
    remittance_info TEXT,
    status TEXT NOT NULL DEFAULT 'booked',
    data_source TEXT NOT NULL DEFAULT 'manual',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    import_batch_id TEXT REFERENCES csv_import_batches(id) ON DELETE CASCADE,
    categorization_source TEXT,
    suggested_category_id TEXT REFERENCES transaction_categories(id),
    UNIQUE(bank_account_id, transaction_id)
);

CREATE INDEX IF NOT EXISTS idx_bank_transactions_account ON bank_transactions(bank_account_id);
CREATE INDEX IF NOT EXISTS idx_bank_transactions_batch ON bank_transactions(import_batch_id);
CREATE INDEX IF NOT EXISTS idx_bank_transactions_category ON bank_transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_bank_transactions_date ON bank_transactions(booking_date);

-- ------------------------------------------------------------------------
-- Categorization
-- ------------------------------------------------------------------------

-- Spending categories: system-seeded plus user-defined.
CREATE TABLE IF NOT EXISTS transaction_categories (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    icon TEXT,
    color TEXT,
    parent_id TEXT REFERENCES transaction_categories(id),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_system INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Seed: the system categories.
INSERT OR IGNORE INTO transaction_categories (id, name, icon, color, parent_id, sort_order, is_system) VALUES
    ('cat_dining', 'Dining & Restaurants', 'utensils', '#FF9800', NULL, 2, 1),
    ('cat_entertainment', 'Entertainment', 'film', '#E91E63', NULL, 5, 1),
    ('cat_groceries', 'Groceries', 'shopping-cart', '#4CAF50', NULL, 1, 1),
    ('cat_health', 'Health & Medical', 'heart', '#F44336', NULL, 7, 1),
    ('cat_housing', 'Housing', 'home', '#F59E0B', NULL, 14, 1),
    ('cat_income', 'Income', 'trending-up', '#8BC34A', NULL, 9, 1),
    ('cat_insurance', 'Insurance', 'shield', '#8B5CF6', NULL, 16, 1),
    ('cat_internal_transfers', 'Internal Transfers', 'arrows-right-left', '#94A3B8', NULL, 13, 1),
    ('cat_investments', 'Investments', 'line-chart', '#6366F1', NULL, 11, 1),
    ('cat_loan_payments', 'Loan Payments', 'credit-card', '#EF4444', NULL, 17, 1),
    ('cat_other', 'Other', 'more-horizontal', '#9E9E9E', NULL, 99, 1),
    ('cat_savings', 'Savings', 'piggy-bank', '#22C55E', NULL, 12, 1),
    ('cat_shopping', 'Shopping', 'shopping-bag', '#00BCD4', NULL, 6, 1),
    ('cat_taxes', 'Taxes', 'landmark', '#DC2626', NULL, 15, 1),
    ('cat_transport', 'Transportation', 'car', '#2196F3', NULL, 3, 1),
    ('cat_travel', 'Travel', 'plane', '#3F51B5', NULL, 8, 1),
    ('cat_utilities', 'Utilities', 'zap', '#9C27B0', NULL, 4, 1);

-- Priority-ordered pattern rules.
CREATE TABLE IF NOT EXISTS categorization_rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    rule_type TEXT NOT NULL,
    pattern TEXT NOT NULL,
    category_id TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 50,
    is_active INTEGER NOT NULL DEFAULT 1,
    stop_processing INTEGER NOT NULL DEFAULT 0,
    is_system INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    iban_pattern TEXT,
    FOREIGN KEY (category_id) REFERENCES transaction_categories(id)
);

CREATE INDEX IF NOT EXISTS idx_categorization_rules_iban ON categorization_rules(iban_pattern) WHERE iban_pattern IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_categorization_rules_priority ON categorization_rules(priority DESC, is_active);

-- Learned payee -> category mappings used by auto-categorization.
CREATE TABLE IF NOT EXISTS learned_payees (
    id TEXT PRIMARY KEY,
    normalized_payee TEXT,
    original_payee TEXT,
    counterparty_iban TEXT,
    category_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY (category_id) REFERENCES transaction_categories(id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_learned_payees_composite ON learned_payees(normalized_payee, counterparty_iban);
CREATE INDEX IF NOT EXISTS idx_learned_payees_iban_only ON learned_payees(counterparty_iban) WHERE normalized_payee IS NULL;
CREATE INDEX IF NOT EXISTS idx_learned_payees_iban_partial ON learned_payees(counterparty_iban);
CREATE INDEX IF NOT EXISTS idx_learned_payees_payee_default ON learned_payees(normalized_payee) WHERE counterparty_iban IS NULL;

-- ------------------------------------------------------------------------
-- Budgeting
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS budget_goals (
    id TEXT PRIMARY KEY,
    category_id TEXT NOT NULL REFERENCES transaction_categories(id) ON DELETE CASCADE,
    timeframe TEXT NOT NULL, -- 'monthly', 'quarterly', 'yearly'
    amount TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(category_id, timeframe)
);

CREATE INDEX IF NOT EXISTS idx_budget_goals_category ON budget_goals(category_id);
CREATE INDEX IF NOT EXISTS idx_budget_goals_timeframe ON budget_goals(timeframe);

-- ------------------------------------------------------------------------
-- Stocks
-- ------------------------------------------------------------------------

-- One row per held ticker; cost basis and average price derive from the transactions.
CREATE TABLE IF NOT EXISTS stock_investments (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    company_name TEXT NOT NULL,
    quantity TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK'
);

CREATE TABLE IF NOT EXISTS investment_transactions (
    id TEXT PRIMARY KEY,
    investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    ticker TEXT NOT NULL,
    company_name TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price_per_unit TEXT NOT NULL,
    currency TEXT NOT NULL,
    transaction_date INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_investment_transactions_ticker ON investment_transactions(ticker);

-- Fetched quote and company metadata cache.
CREATE TABLE IF NOT EXISTS stock_data (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    original_price TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    price_date INTEGER NOT NULL,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
    short_name TEXT,
    long_name TEXT,
    sector TEXT,
    industry TEXT,
    pe_ratio TEXT,
    forward_pe TEXT,
    market_cap TEXT,
    beta TEXT,
    fifty_two_week_high TEXT,
    fifty_two_week_low TEXT,
    trailing_dividend_rate TEXT,
    trailing_dividend_yield TEXT,
    ex_dividend_date INTEGER,
    description TEXT,
    exchange TEXT,
    quote_type TEXT,
    metadata_fetched_at INTEGER,
    previous_close TEXT
);

-- Stock Monitor watchlist; prices come from stock_data.
CREATE TABLE IF NOT EXISTS watched_stocks (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    target_price TEXT,
    notes TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Manual price overrides; they win over stock_data at read time.
CREATE TABLE IF NOT EXISTS stock_price_overrides (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    price TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS dividend_data (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    yearly_dividend_sum TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'USD',
    last_fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Manual dividend overrides; they win over dividend_data at read time.
CREATE TABLE IF NOT EXISTS dividend_overrides (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    yearly_dividend_sum TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS stock_tag_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS stock_tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    color TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    group_id TEXT REFERENCES stock_tag_groups(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_stock_tags_group ON stock_tags(group_id);

CREATE TABLE IF NOT EXISTS stock_investment_tags (
    investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES stock_tags(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (investment_id, tag_id)
);

CREATE INDEX IF NOT EXISTS idx_stock_investment_tags_investment ON stock_investment_tags(investment_id);
CREATE INDEX IF NOT EXISTS idx_stock_investment_tags_tag ON stock_investment_tags(tag_id);

-- Daily CZK value per ticker.
CREATE TABLE IF NOT EXISTS stock_value_history (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    value_czk TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price TEXT NOT NULL,
    currency TEXT NOT NULL,
    is_stale INTEGER NOT NULL DEFAULT 0,
    UNIQUE(ticker, recorded_at)
);

CREATE INDEX IF NOT EXISTS idx_stock_value_history_date ON stock_value_history(recorded_at);
CREATE INDEX IF NOT EXISTS idx_stock_value_history_ticker ON stock_value_history(ticker);
CREATE INDEX IF NOT EXISTS idx_stock_value_history_ticker_date ON stock_value_history(ticker, recorded_at);

-- ------------------------------------------------------------------------
-- Crypto
-- ------------------------------------------------------------------------

-- One row per held crypto asset; cost basis derives from the transactions.
CREATE TABLE IF NOT EXISTS crypto_investments (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,
    coingecko_id TEXT NOT NULL,
    name TEXT NOT NULL,
    quantity TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK'
);

CREATE TABLE IF NOT EXISTS crypto_transactions (
    id TEXT PRIMARY KEY,
    investment_id TEXT NOT NULL REFERENCES crypto_investments(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    ticker TEXT NOT NULL,
    name TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price_per_unit TEXT NOT NULL,
    currency TEXT NOT NULL,
    transaction_date INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_crypto_transactions_ticker ON crypto_transactions(ticker);

-- Fetched CoinGecko price cache.
CREATE TABLE IF NOT EXISTS crypto_prices (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL UNIQUE,
    coingecko_id TEXT,
    price TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Manual price overrides; they win over crypto_prices at read time.
CREATE TABLE IF NOT EXISTS crypto_price_overrides (
    id TEXT PRIMARY KEY,
    symbol TEXT NOT NULL UNIQUE,
    price TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Daily CZK value per ticker.
CREATE TABLE IF NOT EXISTS crypto_value_history (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    value_czk TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price TEXT NOT NULL,
    currency TEXT NOT NULL,
    is_stale INTEGER NOT NULL DEFAULT 0,
    UNIQUE(ticker, recorded_at)
);

CREATE INDEX IF NOT EXISTS idx_crypto_value_history_date ON crypto_value_history(recorded_at);
CREATE INDEX IF NOT EXISTS idx_crypto_value_history_ticker ON crypto_value_history(ticker);
CREATE INDEX IF NOT EXISTS idx_crypto_value_history_ticker_date ON crypto_value_history(ticker, recorded_at);

-- ------------------------------------------------------------------------
-- Bonds
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS bonds (
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

-- ------------------------------------------------------------------------
-- Loans
-- ------------------------------------------------------------------------

-- The outstanding balance is computed on read, never stored.
CREATE TABLE IF NOT EXISTS loans (
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

-- Extra payments, rate changes and balance checks.
CREATE TABLE IF NOT EXISTS loan_events (
    id TEXT PRIMARY KEY,
    loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
    event_day INTEGER NOT NULL,
    amount TEXT,
    rate TEXT,
    monthly_payment TEXT,
    note TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_loan_events_loan ON loan_events(loan_id, event_day);

-- ------------------------------------------------------------------------
-- Real estate
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS real_estate (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    type TEXT NOT NULL,
    purchase_price TEXT NOT NULL DEFAULT '0',
    purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
    market_price TEXT NOT NULL DEFAULT '0',
    market_price_currency TEXT NOT NULL DEFAULT 'CZK',
    monthly_rent TEXT,
    monthly_rent_currency TEXT DEFAULT 'CZK',
    recurring_costs TEXT DEFAULT '[]',
    photos TEXT DEFAULT '[]',
    notes TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS real_estate_one_time_costs (
    id TEXT PRIMARY KEY,
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    date INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Dated market-value estimates; the newest one is mirrored into real_estate.market_price.
CREATE TABLE IF NOT EXISTS real_estate_valuations (
    id TEXT PRIMARY KEY,
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    value TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    valued_at INTEGER NOT NULL,
    note TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_real_estate_valuations_asset ON real_estate_valuations(real_estate_id, valued_at);

CREATE TABLE IF NOT EXISTS real_estate_loans (
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
    PRIMARY KEY (real_estate_id, loan_id)
);

CREATE TABLE IF NOT EXISTS real_estate_insurances (
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    insurance_id TEXT NOT NULL REFERENCES insurance_policies(id) ON DELETE CASCADE,
    PRIMARY KEY (real_estate_id, insurance_id)
);

CREATE TABLE IF NOT EXISTS real_estate_photo_batches (
    id TEXT PRIMARY KEY,
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    photo_date INTEGER NOT NULL,
    description TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_batches_real_estate ON real_estate_photo_batches(real_estate_id);

CREATE TABLE IF NOT EXISTS real_estate_photos (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES real_estate_photo_batches(id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    thumbnail_path TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_photos_batch ON real_estate_photos(batch_id);

CREATE TABLE IF NOT EXISTS real_estate_documents (
    id TEXT PRIMARY KEY,
    real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    file_path TEXT NOT NULL,
    file_type TEXT NOT NULL DEFAULT 'other',
    file_size INTEGER,
    uploaded_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_real_estate_documents ON real_estate_documents(real_estate_id);

-- ------------------------------------------------------------------------
-- Insurance
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS insurance_policies (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    provider TEXT NOT NULL,
    policy_name TEXT NOT NULL,
    policy_number TEXT NOT NULL,
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

CREATE TABLE IF NOT EXISTS insurance_documents (
    id TEXT PRIMARY KEY,
    insurance_id TEXT NOT NULL REFERENCES insurance_policies(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    file_path TEXT NOT NULL,
    file_type TEXT NOT NULL DEFAULT 'other',
    file_size INTEGER,
    uploaded_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_insurance_documents ON insurance_documents(insurance_id);

-- ------------------------------------------------------------------------
-- Other assets
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS other_assets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    quantity TEXT NOT NULL DEFAULT '0',
    market_price TEXT NOT NULL DEFAULT '0',
    currency TEXT NOT NULL DEFAULT 'CZK',
    average_purchase_price TEXT NOT NULL DEFAULT '0',
    yield_type TEXT NOT NULL DEFAULT 'none',
    yield_value TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS other_asset_transactions (
    id TEXT PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price_per_unit TEXT NOT NULL,
    currency TEXT NOT NULL,
    transaction_date INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_other_asset_transactions_asset ON other_asset_transactions(asset_id);

-- Dated price-per-unit estimates; the newest one is mirrored into other_assets.market_price.
CREATE TABLE IF NOT EXISTS other_asset_valuations (
    id TEXT PRIMARY KEY,
    asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
    value TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    valued_at INTEGER NOT NULL,
    note TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_other_asset_valuations_asset ON other_asset_valuations(asset_id, valued_at);

-- ------------------------------------------------------------------------
-- Portfolio history
-- ------------------------------------------------------------------------

-- Daily net-worth snapshots per asset class, in CZK.
CREATE TABLE IF NOT EXISTS portfolio_metrics_history (
    id TEXT PRIMARY KEY,
    total_savings TEXT NOT NULL,
    total_loans_principal TEXT NOT NULL,
    total_investments TEXT NOT NULL,
    total_crypto TEXT NOT NULL DEFAULT '0',
    total_bonds TEXT NOT NULL,
    total_real_estate_personal TEXT NOT NULL,
    total_real_estate_investment TEXT NOT NULL,
    recorded_at INTEGER NOT NULL DEFAULT (unixepoch()),
    total_other_assets TEXT NOT NULL DEFAULT '0',
    is_stale INTEGER NOT NULL DEFAULT 0,
    investments_by_currency TEXT NOT NULL DEFAULT '{}',
    crypto_by_currency TEXT NOT NULL DEFAULT '{}',
    savings_by_currency TEXT NOT NULL DEFAULT '{}',
    bonds_by_currency TEXT NOT NULL DEFAULT '{}',
    real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
    loans_by_currency TEXT NOT NULL DEFAULT '{}',
    other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
    source TEXT NOT NULL DEFAULT 'live'
);

CREATE INDEX IF NOT EXISTS idx_portfolio_metrics_history_recorded_at ON portfolio_metrics_history(recorded_at);

-- ------------------------------------------------------------------------
-- Currency
-- ------------------------------------------------------------------------

-- Latest ECB rates, persisted for offline use.
CREATE TABLE IF NOT EXISTS exchange_rates (
    currency TEXT PRIMARY KEY,
    rate REAL NOT NULL,
    fetched_at INTEGER NOT NULL
);

-- Daily historical rates for accurate backfills.
CREATE TABLE IF NOT EXISTS exchange_rate_history (
    date INTEGER NOT NULL,
    currency TEXT NOT NULL,
    rate REAL NOT NULL,
    PRIMARY KEY (date, currency)
);

CREATE INDEX IF NOT EXISTS idx_erh_currency_date ON exchange_rate_history(currency, date);

-- ------------------------------------------------------------------------
-- Cashflow / projection
-- ------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cashflow_items (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    amount TEXT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CZK',
    frequency TEXT NOT NULL,
    item_type TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    category TEXT NOT NULL DEFAULT 'income'
);

CREATE TABLE IF NOT EXISTS projection_settings (
    id TEXT PRIMARY KEY,
    asset_type TEXT NOT NULL UNIQUE,
    yearly_growth_rate TEXT NOT NULL DEFAULT '0',
    monthly_contribution TEXT NOT NULL DEFAULT '0',
    contribution_currency TEXT NOT NULL DEFAULT 'CZK',
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the schema snapshot, relative to src-tauri/
    const SNAPSHOT_PATH: &str = "src/db/schema.snapshot";

    /// Dump the complete schema and seeded data of a migrated database
    /// in a stable, comparable text form.
    fn dump_schema_and_seed_data(conn: &Connection) -> String {
        let mut out = String::new();

        let mut stmt = conn
            .prepare(
                "SELECT type, name, COALESCE(sql, '') FROM sqlite_master
                 WHERE name NOT LIKE 'sqlite_%' AND name != '_migrations'
                 ORDER BY type, name",
            )
            .expect("prepare sqlite_master query");
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .expect("query sqlite_master");
        for row in rows {
            let (obj_type, name, sql) = row.expect("read sqlite_master row");
            out.push_str(&format!("== {} {} ==\n{}\n\n", obj_type, name, sql.trim()));
        }

        // Seeded data (stable columns only — created_at is insert-time volatile)
        for (table, cols) in [
            (
                "transaction_categories",
                "id, name, icon, color, parent_id, sort_order, is_system",
            ),
            ("institutions", "id, name, bic, country, logo_url"),
        ] {
            out.push_str(&format!("== data {} ==\n", table));
            let mut stmt = conn
                .prepare(&format!("SELECT {} FROM {} ORDER BY id", cols, table))
                .expect("prepare data query");
            let col_count = stmt.column_count();
            let rows = stmt
                .query_map([], move |row| {
                    let mut vals = Vec::with_capacity(col_count);
                    for i in 0..col_count {
                        let v: rusqlite::types::Value = row.get(i)?;
                        vals.push(match v {
                            rusqlite::types::Value::Null => "NULL".to_string(),
                            rusqlite::types::Value::Integer(n) => n.to_string(),
                            rusqlite::types::Value::Real(f) => f.to_string(),
                            rusqlite::types::Value::Text(s) => s,
                            rusqlite::types::Value::Blob(b) => format!("<blob {} bytes>", b.len()),
                        });
                    }
                    Ok(vals.join(" | "))
                })
                .expect("query data");
            for row in rows {
                out.push_str(&row.expect("read data row"));
                out.push('\n');
            }
            out.push('\n');
        }

        out
    }

    /// A second migration that only exists inside tests, appended to the real
    /// chain to exercise the "known names, run the pending ones" path.
    const FAKE_SECOND: (&str, &str) = (
        "002_fake_for_tests",
        "CREATE TABLE fake_for_tests (id TEXT PRIMARY KEY);",
    );

    fn chain_with_fake_second() -> Vec<(&'static str, &'static str)> {
        let mut chain = all_migrations();
        chain.push(FAKE_SECOND);
        chain
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(true),
        )
        .unwrap_or(false)
    }

    /// Regenerate the snapshot after an intentional schema change:
    /// cargo test regenerate_schema_snapshot -- --ignored
    #[test]
    #[ignore]
    fn regenerate_schema_snapshot() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("run migrations");
        let dump = dump_schema_and_seed_data(&conn);
        std::fs::write(SNAPSHOT_PATH, &dump).expect("write schema snapshot");
        println!("Schema snapshot written to {}", SNAPSHOT_PATH);
    }

    /// The chain must always produce exactly the schema and seed rows recorded
    /// in `schema.snapshot`.
    #[test]
    fn baseline_matches_schema_snapshot() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("run migrations");
        let dump = dump_schema_and_seed_data(&conn);
        let snapshot = std::fs::read_to_string(SNAPSHOT_PATH).expect("read schema snapshot");
        assert_eq!(
            dump, snapshot,
            "Migrated schema differs from schema.snapshot. If this change is intentional, regenerate with: cargo test regenerate_schema_snapshot -- --ignored"
        );
    }

    #[test]
    fn fresh_database_runs_the_chain_and_is_idempotent() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("first run");
        assert_eq!(
            applied_migration_names(&conn).expect("applied"),
            known_migration_names()
        );
        // Seed rows come from the baseline.
        let categories: i64 = conn
            .query_row("SELECT COUNT(*) FROM transaction_categories", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(categories, 17);

        run_migrations(&conn).expect("second run is a no-op");
        assert_eq!(
            applied_migration_names(&conn).expect("applied").len(),
            known_migration_names().len()
        );
    }

    #[test]
    fn known_names_run_only_the_pending_migrations() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("baseline");
        // Data written after the baseline must survive a later migration.
        conn.execute(
            "DELETE FROM transaction_categories WHERE id = 'cat_travel'",
            [],
        )
        .expect("delete a system category");

        let chain = chain_with_fake_second();
        let applied = applied_migration_names(&conn).expect("applied");
        apply_pending(&conn, &chain, &applied).expect("pending");

        assert!(table_exists(&conn, "fake_for_tests"));
        assert_eq!(
            applied_migration_names(&conn).expect("applied"),
            vec!["001_initial_schema", "002_fake_for_tests"]
        );
        let travel: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transaction_categories WHERE id = 'cat_travel'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(travel, 0, "the baseline must not run a second time");
    }

    #[test]
    fn unknown_migration_names_are_refused_without_running_anything() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE _migrations (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                applied_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            INSERT INTO _migrations (name) VALUES ('001_baseline'), ('002_from_another_build');",
        )
        .expect("simulate a database from a pre-release build");

        let err = run_migrations(&conn).expect_err("must refuse");
        assert!(
            err.to_string().contains("pre-release build of Moony"),
            "{err}"
        );
        assert!(
            !table_exists(&conn, "app_config"),
            "nothing may be executed"
        );
        assert_eq!(
            applied_migration_names(&conn).expect("applied"),
            vec!["001_baseline", "002_from_another_build"],
            "_migrations stays untouched"
        );

        // One unknown name is enough, even next to the baseline.
        conn.execute(
            "INSERT INTO _migrations (name) VALUES ('001_initial_schema')",
            [],
        )
        .expect("record baseline");
        assert!(run_migrations(&conn).is_err());
    }

    /// a migration that fails halfway must leave no trace — neither
    /// its first statements nor a `_migrations` row — so the next start can
    /// retry instead of failing on "table already exists".
    #[test]
    fn failing_migration_is_rolled_back_atomically() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE _migrations (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, applied_at INTEGER NOT NULL DEFAULT (unixepoch()));",
        )
        .expect("migrations table");

        let bad =
            "CREATE TABLE half_done (id TEXT PRIMARY KEY); INSERT INTO does_not_exist VALUES (1);";
        let err = apply_pending(&conn, &[("900_bad", bad)], &[]).expect_err("must fail");
        assert!(err.to_string().contains("900_bad"), "{err}");

        assert!(
            !table_exists(&conn, "half_done"),
            "first statement must be rolled back"
        );
        let recorded: i64 = conn
            .query_row("SELECT COUNT(*) FROM _migrations", [], |r| r.get(0))
            .expect("count");
        assert_eq!(recorded, 0);

        // A good migration afterwards still applies normally.
        apply_pending(
            &conn,
            &[("901_good", "CREATE TABLE fine (id TEXT PRIMARY KEY);")],
            &[],
        )
        .expect("good migration");
        let recorded: i64 = conn
            .query_row("SELECT COUNT(*) FROM _migrations", [], |r| r.get(0))
            .expect("count");
        assert_eq!(recorded, 1);
    }

    #[test]
    fn has_pending_migrations_reports_fresh_and_outdated_databases() {
        let conn = Connection::open_in_memory().expect("db");
        // No _migrations table yet: nothing to back up (baseline creates everything).
        assert!(!has_pending_migrations(&conn).expect("fresh"));

        run_migrations(&conn).expect("migrate");
        assert!(!has_pending_migrations(&conn).expect("current"));

        // A later migration appears in the chain: the opener must take a backup first.
        let chain = chain_with_fake_second();
        assert!(has_pending(&conn, &chain).expect("outdated"));

        // Once applied, nothing is pending any more.
        let applied = applied_migration_names(&conn).expect("applied");
        apply_pending(&conn, &chain, &applied).expect("apply");
        assert!(!has_pending(&conn, &chain).expect("up to date"));

        // A database from a pre-release build is refused, not backed up.
        conn.execute(
            "INSERT INTO _migrations (name) VALUES ('900_from_the_future')",
            [],
        )
        .expect("unknown name");
        assert!(!has_pending(&conn, &chain).expect("unknown names"));
    }
}
