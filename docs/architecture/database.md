# Database Schema Reference

The single SQLCipher-encrypted SQLite database behind Moony. The schema is
defined by the append-only migrations in `src-tauri/src/db/migrations.rs`,
which start from one hand-written baseline, `001_initial_schema`. Domain
grouping below follows the domain map in `docs/architecture/overview.md`.

## How Migrations Work

- Migrations are **append-only inline SQL string consts** in
  `src-tauri/src/db/migrations.rs`. There are no `.sql` files.
- The chain starts with `001_initial_schema` (`MIGRATION_001`): the whole data
  model, grouped by domain and commented, plus the seed rows of
  `transaction_categories` and `institutions` (ADR 0010).
- `002_real_estate_purchase_date` (`MIGRATION_002`) adds the nullable
  `real_estate.purchase_date` and seeds the empty valuation logs once: every
  real estate and every other asset with a price above zero and no valuation row
  gets its current price as the first estimate, dated at the UTC day of its
  `created_at` (ids are UUID v4 built in SQL). Existing logs are untouched.
- To add one: define `const MIGRATION_003: &str = r#"..."#;` (next free
  number) and append a `("003_short_name", MIGRATION_003)` entry to the `Vec`
  returned by `all_migrations()`. Numbers are zero-padded, sequential, never
  reused.
- Applied migrations are tracked by name in the `_migrations` table. On every
  DB open, `run_migrations()` looks at that table and is in exactly one of
  three states:
  - **no rows** (a new database): the whole chain runs;
  - **only names this build knows**: every entry not yet recorded runs, each
    in its own transaction;
  - **any unknown name**: the open fails with *"This database was created by a
    pre-release build of Moony and cannot be opened by this version."* and
    nothing is executed. There is no stamping or repair path for such
    databases.
- A failing migration rolls back completely and leaves `_migrations`
  untouched, so the next start retries from a consistent state. Before the
  first pending migration on an existing database, the opener copies the
  database file (see "Data folder artefacts").
- **Never edit a migration that has shipped**, including the baseline.
  Existing databases will not re-run it; the fix is always a new migration.
- **Never rename a migration that has shipped either.** Because tracking is by
  name, a rename makes the runner treat it as new and re-execute its DDL — and
  SQLite has no `ADD COLUMN IF NOT EXISTS`, so it fails on existing databases.
- The schema is guarded by `schema.snapshot` (same directory as
  `migrations.rs`): a test dumps the schema and the seed rows of a freshly
  migrated in-memory database and asserts they match the file byte-for-byte.
  When a new migration intentionally changes the schema, regenerate the file
  with `cargo test regenerate_schema_snapshot -- --ignored` (run in
  `src-tauri/`) and commit it alongside the migration.
- SQLite has no `ALTER COLUMN` (can't change nullability, drop a column with
  constraints, etc.). The established workaround is
  **create-new → copy → drop → rename**: create `table_new` with the desired
  shape, `INSERT INTO ... SELECT` the data across, drop the old table (and its
  indexes), rename `table_new` back, recreate indexes.

## Storage Conventions

- **Primary keys are TEXT UUIDs**, generated in Rust via
  `Uuid::new_v4().to_string()`. System-seeded rows use readable prefixed IDs
  instead (`cat_groceries`, `inst_fio`). The only exceptions:
  `user_profile.id` (`INTEGER PRIMARY KEY AUTOINCREMENT` — single-row table),
  `app_config.key` and `exchange_rates.currency` (natural TEXT keys), and
  composite-key join/history tables.
- **Timestamps are INTEGER unix epochs**, typically
  `INTEGER NOT NULL DEFAULT (unixepoch())` for `created_at` / `updated_at`.
  Day-granular dates (`booking_date`, `transaction_date`, history
  `recorded_at` days) are the **UTC midnight** of the calendar day, and period
  filters are bounded on the UTC calendar (ADR 0008, `src/utils/period.ts`).
- **All money and decimal values are TEXT strings** (`'0'`, `'1234.56'`) —
  balances, prices, quantities, rates. Parsed in Rust/TS at use time to avoid
  float drift. The **only exception**: `exchange_rates.rate` and
  `exchange_rate_history.rate` are `REAL`.
- **CZK is the base currency.** Aggregated values (e.g.
  `stock_value_history.value_czk`, `portfolio_metrics_history` totals) are
  stored in CZK; entity tables carry a `currency` column for native amounts.
- **Per-currency breakdowns are JSON strings in TEXT columns**, e.g. the seven
  `portfolio_metrics_history.*_by_currency` columns
  (`TEXT NOT NULL DEFAULT '{}'`). Other JSON-in-TEXT columns:
  `user_profile.menu_preferences`, `real_estate.recurring_costs` / `.photos`,
  `insurance_policies.limits`.
- **Paired override tables**: fetched market data lives in one table, manual
  user overrides in a sibling that wins at read time —
  `stock_data` + `stock_price_overrides`, `crypto_prices` +
  `crypto_price_overrides`, `dividend_data` + `dividend_overrides`.
- **History tables use `(date, key)` UNIQUE constraints** so backfills upsert
  instead of duplicating: `stock_value_history` / `crypto_value_history`
  `UNIQUE(ticker, recorded_at)`, `exchange_rate_history`
  `PRIMARY KEY (date, currency)`, `bank_transactions`
  `UNIQUE(bank_account_id, transaction_id)`.

## Table Catalog

Grouped by owning domain (see the domain map in `overview.md`).

### Infrastructure

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `_migrations` | Tracks applied migrations by name | `name TEXT UNIQUE`, `applied_at`; created directly in `run_migrations()`, not in a migration const |

### Auth / profile

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `app_config` | Key-value app settings incl. recovery-key hash | `key TEXT PRIMARY KEY`, `value TEXT`; the CoinGecko API key (`api_key_coingecko`) is the only API key still read; `stock_import_formats` holds the custom stock CSV mappings (JSON array, newest first, at most 20; `services/stock_import/formats.rs`) |
| `user_profile` | Single-row user profile and preferences | `INTEGER AUTOINCREMENT` PK (convention exception); `currency DEFAULT 'CZK'`, `language`, `menu_preferences` JSON, `coingecko_modal_dismissed`, `mcp_server_enabled` gates the local API server, `mcp_server_token` TEXT (NULL until the server is first enabled), `mcp_server_port` INTEGER (NULL = default 41414) |

### Bank accounts

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `institutions` | Banks/financial institutions, pre-seeded with Czech banks | Readable seeded IDs (`inst_fio`, `inst_csob`, …); `bic`, `logo_url` |
| `bank_accounts` | All bank/savings accounts | `account_type DEFAULT 'checking'`, `institution_id` FK, `iban`/`bban`, `data_source DEFAULT 'manual'`, `has_zone_designation`, `exclude_from_balance`, `interest_rate` TEXT |
| `bank_account_zones` | Tiered interest-rate zones per account | FK `bank_account_id` ON DELETE CASCADE; `from_amount`/`to_amount`/`interest_rate` TEXT. Tiers are bands of the balance (not contiguous, not sorted); the one interest model is `services/interest_tiers.rs`, mirrored for display by `src/utils/bank-account-zones.ts` and cross-checked through `shared/fixtures/interest-tiers.json`; an empty / `0` `to_amount` is unlimited |
| `bank_transactions` | Imported/manual bank transactions | `UNIQUE(bank_account_id, transaction_id)` dedupes imports (`services/dedup.rs`: a bank-side id is the only key when present; id-less rows fall back to an exact date/amount/type/description match, which the MCP import reports as `possibleDuplicate` and `importAnyway` overrides); `category_id` FK, `categorization_source`, `import_batch_id` FK ON DELETE CASCADE, `suggested_category_id` FK (MCP-suggested category awaiting confirmation); indexes on account, date, category, batch |
| `csv_import_batches` | One row per CSV upload with import stats | FK `bank_account_id` ON DELETE CASCADE; `imported_count`/`duplicate_count`/`error_count` |

### Categorization

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `transaction_categories` | Spending categories, system-seeded + user-defined | Readable seeded IDs (`cat_groceries`, …); self-referencing `parent_id` FK, `is_system`, `sort_order` |
| `categorization_rules` | Priority-ordered pattern rules | `rule_type`/`pattern` (`regex`, `contains`, `word`, `starts_with`, `ends_with`, `is_credit`, `is_debit`), `priority DEFAULT 50` (index `priority DESC, is_active`), `stop_processing`, `iban_pattern` |
| `learned_payees` | Learned payee → category mappings for auto-categorization | `normalized_payee` is nullable (IBAN-only rules); UNIQUE index `(normalized_payee, counterparty_iban)`; partial indexes for the 3-level matching hierarchy |

### Budgeting

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `budget_goals` | Spending limit per category per timeframe | `UNIQUE(category_id, timeframe)`; `timeframe` ∈ monthly/quarterly/yearly; `amount` TEXT; FK ON DELETE CASCADE |

### Stocks

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `stock_investments` | One row per held ticker (position) | `ticker TEXT UNIQUE`; `quantity` TEXT; `currency`. No stored average price: cost basis and average price derive from `investment_transactions` at each transaction day's rate (`services/cost_basis.rs`) |
| `investment_transactions` | Buy/sell transactions per stock position | FK `investment_id` ON DELETE CASCADE; `type`, `price_per_unit`, `currency`, `transaction_date`; index on `ticker` (for MCP import dedup); `import_batch_id` FK → `stock_import_batches` ON DELETE SET NULL (NULL for hand-made and MCP-created rows; editing a transaction keeps it in its batch) and `external_id` TEXT (`<source>:<broker's transaction id>`, NULL when the file had none; duplicate rule 1 of the CSV import), each indexed (migration 003) |
| `stock_import_batches` | One row per stock CSV import, so it can be undone | `file_name`, `source` (`xtb`, `trading212`, `degiro`, `ibkr`, `moony`, `custom` or `format:<uuid>`), `trade_count` (rows written), `created_at`. Written in the same SQL transaction as the imported rows and only when something was written; undo deletes the batch's transactions, the positions left without any, then the row (`services/stock_import/batches.rs`) |
| `stock_data` | Fetched Yahoo Finance quote + company metadata cache | `ticker UNIQUE`; `currency` is the currency `original_price` is stored in, the one Yahoo reports for the quote (its minor units are converted: `GBp` pence are stored as GBP, a hundredth); the ticker suffix only stands in for a response that names none; `quote_currency` TEXT is the code Yahoo reported with the last quote, as it came (NULL until the first refresh after migration 004), kept to find tickers whose stored history was written in another unit (`services/quote_unit.rs`); `previous_close`, `sector`, `industry`, `pe_ratio`, `market_cap`, `beta`, 52-week range, dividend fields, `metadata_fetched_at` |
| `watched_stocks` | Stock Monitor watchlist: followed tickers with optional target price and markdown notes; prices come from `stock_data` via JOIN | `ticker UNIQUE`; `target_price` money-as-TEXT in native currency, a plain informative reference (no direction, no "reached" signal); never enters net worth |
| `stock_price_overrides` | Manual price overrides (win over `stock_data`) | `ticker UNIQUE`; `currency DEFAULT 'CZK'` |
| `dividend_data` | Fetched yearly dividend sums per ticker | `ticker UNIQUE`; `yearly_dividend_sum` TEXT in `currency` (converted from the quote unit like a price) |
| `dividend_overrides` | Manual dividend overrides (win over `dividend_data`) | `ticker UNIQUE` |
| `stock_tags` | User tags for grouping investments | `name UNIQUE`; `group_id` FK ON DELETE SET NULL |
| `stock_investment_tags` | Investment ↔ tag many-to-many join | Composite PK `(investment_id, tag_id)`; both FKs ON DELETE CASCADE |
| `stock_tag_groups` | Optional grouping of tags (e.g. "Strategy") | `name UNIQUE` |
| `stock_value_history` | Daily CZK value per ticker (also feeds portfolio trend) | `UNIQUE(ticker, recorded_at)`; `value_czk`, native `price`+`currency`, `is_stale` |

### Crypto

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `crypto_investments` | One row per held crypto asset | `ticker UNIQUE`, `coingecko_id`; `currency`. No stored average price (same derivation as `stock_investments`; crypto positions may mix transaction currencies) |
| `crypto_transactions` | Buy/sell transactions per crypto position | FK `investment_id` ON DELETE CASCADE; index on `ticker` (for MCP import dedup) |
| `crypto_prices` | Fetched CoinGecko price cache | `symbol UNIQUE`; `coingecko_id` |
| `crypto_price_overrides` | Manual price overrides (win over `crypto_prices`) | `symbol UNIQUE` |
| `crypto_value_history` | Daily CZK value per crypto ticker (also feeds portfolio trend) | `UNIQUE(ticker, recorded_at)`; `value_czk`, `is_stale` |

### Bonds

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `bonds` | Bond holdings | `isin` nullable; `coupon_value`, `interest_rate`, `quantity`, `currency`, `maturity_date` |

### Loans

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `loans` | Loans/mortgages | `principal` (the original amount, never reduced), `monthly_payment`, `interest_rate` + `interest_rate_validity_date`, `start_date`/`end_date`; `balance_anchor_amount` TEXT + `balance_anchor_date` INTEGER (UTC day): both NULL = amortize from `principal` at `start_date`, both set = the user's real balance as of that day, amortization continues from it. The outstanding balance is computed on read (`services/loan_amortization.rs`), never stored; `portfolio_metrics_history.total_loans_principal` and `loans_by_currency` hold the outstanding totals |
| `loan_events` | Extra payments, rate changes and balance checks of a loan | FK `loan_id` ON DELETE CASCADE; `kind` CHECK (`extra_payment`/`rate_change`/`balance_check`), `event_day` INTEGER (UTC day), `amount`/`rate`/`monthly_payment` TEXT; index `(loan_id, event_day)`. Adding an event re-anchors the loan (`services/loan_events.rs`) |

### Real estate

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `real_estate` | Properties (personal or investment) | `type`; purchase/market price each with own currency column; `purchase_date` INTEGER (UTC day, nullable = unknown; migration 002) where the value trace on the detail page starts; `monthly_rent`; `recurring_costs` JSON `'[]'`, `photos` JSON `'[]'` |
| `real_estate_one_time_costs` | One-off costs per property | FK `real_estate_id` ON DELETE CASCADE; `amount` TEXT, `date` |
| `real_estate_valuations` | Dated market-value estimates per property | FK `real_estate_id` ON DELETE CASCADE; `value` TEXT, `currency`, `valued_at` INTEGER (UTC day), `note`; index `(real_estate_id, valued_at)`. The newest row is mirrored into `real_estate.market_price` (`services/valuations.rs`). A priced property has a log from day one: `create_property` writes the first row (`record_initial_valuation`) and migration 002 seeded the properties that were created before |
| `real_estate_loans` | Property ↔ loan join | Composite PK `(real_estate_id, loan_id)`; both FKs ON DELETE CASCADE |
| `real_estate_insurances` | Property ↔ insurance policy join | Composite PK `(real_estate_id, insurance_id)`; both FKs ON DELETE CASCADE |
| `real_estate_photo_batches` | Photo uploads grouped by date + description | FK `real_estate_id` ON DELETE CASCADE; indexed |
| `real_estate_photos` | Individual photos in a batch | FK `batch_id` ON DELETE CASCADE; `file_path` + `thumbnail_path` |
| `real_estate_documents` | Documents (deeds, contracts) per property | FK `real_estate_id` ON DELETE CASCADE; `file_path`, `file_type`, `file_size` |

### Insurance

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `insurance_policies` | Insurance policies | `type`, `provider`, `payment_frequency` (canonical `monthly`/`quarterly`/`semi_annually`/`annually`/`one_time`, see `models/frequency.rs`), one-time + regular payment each with currency column, `limits` JSON `'[]'`, `status DEFAULT 'active'` |
| `insurance_documents` | Documents per policy | FK `insurance_id` ON DELETE CASCADE; `file_path`, `file_type`, `file_size` |

### Other assets

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `other_assets` | Miscellaneous assets (gold, art, …) | `quantity`, `market_price`, `average_purchase_price` TEXT; `yield_type DEFAULT 'none'` + `yield_value` |
| `other_asset_transactions` | Buy/sell transactions per other asset | FK `asset_id` ON DELETE CASCADE; index on `asset_id` (for MCP import dedup) |
| `other_asset_valuations` | Dated price-per-unit estimates per other asset | FK `asset_id` ON DELETE CASCADE; `value` TEXT, `currency`, `valued_at` INTEGER (UTC day), `note`; index `(asset_id, valued_at)`. The newest row is mirrored into `other_assets.market_price`. A priced asset has a log from day one: `create_asset` writes the first row and migration 002 seeded the older ones |

### Portfolio / net worth

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `portfolio_metrics_history` | Daily net-worth snapshots per asset class | Totals as CZK TEXT (`total_savings`, `total_loans_principal`, …, `total_other_assets`); `is_stale`; seven `*_by_currency` JSON TEXT `DEFAULT '{}'`; `source` TEXT `'live'`/`'backfill'` provenance — live rows are authentic records, backfill rows may be rebuilt. Index on `recorded_at`. `get_portfolio_history` returns `source`; the dashboard draws backfilled days dashed because their static classes (accounts, bonds, real estate, loans) are carried from the nearest live row |

(`stock_value_history` and `crypto_value_history` above also feed this domain's trend charts.)

### Currency

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `exchange_rates` | Latest ECB rates persisted for offline use | `currency TEXT PRIMARY KEY`; **`rate REAL`** (money-as-TEXT exception) |
| `exchange_rate_history` | Daily historical rates for accurate backfills | `PRIMARY KEY (date, currency)`; **`rate REAL`**; index `(currency, date)`. Coverage self-heals (`services/fx_backfill.rs`): the prefix before the earliest transaction and any multi-day tail gap are fetched from frankfurter.dev on rate refresh and before historical-transaction recalcs; live-recorded rows win over backfill (`INSERT OR IGNORE`). ECB rows are stored under the feed's reference date (`time=` attribute, `parse_ecb_xml`), not the fetch day, so a weekend fetch lands on Friday; a feed without a CZK quote is an error. Budget reports convert foreign-currency transactions with the rates of their booking day |

### Cashflow / projection

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `cashflow_items` | User-defined recurring income/expense items | `amount` TEXT, `frequency`, `item_type`, `category` |
| `projection_settings` | Growth/contribution assumptions per asset class | `asset_type UNIQUE`; `yearly_growth_rate`, `monthly_contribution` TEXT |

## Data folder artefacts

Next to `moony.db` the app may create: `moony.db.bak-<unix ts>` — a copy
taken before the first pending migration runs on an existing database (newest
three kept, see `db/mod.rs`; a brand-new database has nothing to copy);
`pre-restore-<unix ts>/` — the previous account after a restore from backup
(never deleted automatically). Backups themselves are zip archives written
wherever the user chooses (`services/backup.rs`).
