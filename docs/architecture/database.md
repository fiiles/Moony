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
- `006_financial_semantics` adds nullable transaction `flow_kind` overrides and category
  `kind` roles with six constrained values. Legacy custom categories default to expense;
  system Income, Internal Transfers and Investments get income, transfer and investment.
- `010_income_categories` renames the seeded Income category to Salary (unless renamed), seeds
  Investment income and Other income, moves Savings to transfer (unless re-roled) and collapses
  the investment, financing and adjustment roles into transfer.
- `011_fx_eur_reference` rewrites `exchange_rates` and `exchange_rate_history` in place from
  CZK per unit to units per 1 EUR (CZK becomes an ordinary quote, the EUR row is dropped, the
  derived pegged codes AED/SAR/QAR/BHD/OMR/BGN are removed) and records
  `app_config.fx_reference = 'EUR'`. Rows of a day without an EUR quote cannot be rebased and are
  dropped.
- `012_retire_czk_caches` makes native breakdowns the only stored record of a snapshot day: adds
  `portfolio_metrics_history.real_estate_personal_by_currency`, gives a class whose breakdown is
  empty while its CZK total was not `{"CZK": total}` once, then drops the eight `total_*`
  columns and `value_czk` of `stock_value_history` and `crypto_value_history` (ADR 0012). Both
  migrations rewrite data, so the pre-migration copy of the database (see "Data folder artefacts")
  is the rollback.
- `016_mcp_pending_refresh` adds a durable follow-up queue for MCP writes. Each domain coalesces
  its earliest affected day; a generation token prevents concurrent imports from losing work.
  It stores no financial facts and never assigns ownership to historical unbatched imports.
- `017_profile_country` adds `user_profile.country`. A profile that predates it gets `CZ` (an enabled
  `cz` rule pack, the Czech UI or CZK as main currency) or `SK` (an enabled `sk` pack); every other
  profile stays NULL.
- `018_insurance_notice_days` adds `user_profile.insurance_notice_days`, the notice the insurance
  anniversary milestone counts back from. NULL follows the profile country (43 days for CZ and SK,
  no reminder elsewhere), 0 switches the reminder off.
- `019_loan_manual_records` makes manual loan balances records that hold. It adds
  `loan_events.previous_rate` / `previous_monthly_payment` (the terms in force before an event that
  changes them; legacy rows keep NULL). A form-entered anchor with no event on its day becomes a
  `balance_check` record (the manual balance record); an anchor that an extra payment or a rate
  change re-anchored does not become one directly (the ledger replays those events), but it may
  hide a balance the user had entered by hand before the event overwrote it, so it is copied to
  `loan_legacy_anchors` first. `loans.balance_anchor_*` then mirror the newest record, NULL without
  one. At every database open `services::loan_events::reconcile_legacy_anchors` compares each
  copied anchor with the ledger's estimate at its day: when they differ by more than one currency
  unit the user had overridden the estimate and a manual record of the legacy amount is added (a
  manual value holds); when they agree nothing is added. The processed row is deleted, so the
  reconciliation runs once per row and is idempotent; a failure is logged and never blocks unlock.
  A loan with a manual balance stops amortizing after it, so its outstanding balance, today's
  included, can be higher than before (ADR 0016).
- `020_bond_purchase_date` adds `bonds.purchase_date`, the UTC day a bond was bought (nullable;
  every existing bond stays NULL). History counts a dated bond from that day on and a bond without
  a date as an undated item (carried, with the unknown-remainder rule); today's value is unchanged.
- `021_encrypted_attachments` prepares the move of attachment content into the encrypted store
  `attachments.db` (ADR 0017, see "The attachments store"). It rebuilds `insurance_documents`,
  `real_estate_documents` and `real_estate_photos` so a row can reference a blob (`blob_id`, a
  photo's `thumb_blob_id`, a document's `mime_type` and `original_name`) instead of a plain file:
  `file_path` (and a photo's `thumbnail_path`) become nullable and stay set only on rows whose
  content has not been converted yet, and a CHECK requires a file or a blob. Existing rows are
  copied across unchanged; the content itself moves in the background after the first unlock, not
  in the migration. It also creates `attachment_leftovers`, which remembers plain originals the
  conversion found outside the data folder and does not delete.
- `022_profile_theme` adds `user_profile.theme TEXT NOT NULL DEFAULT 'system'`, the colour theme:
  `system` follows the operating system, `light` and `dark` are explicit. Every existing profile
  gets `system`; the three values are checked when the profile is updated, not by the column.
- `023_stock_splits` creates `stock_splits` (splits per ticker, `pending` until applied or dismissed)
  and adds `stock_data.splits_checked_at`, when the splits of a ticker were last looked up.
- To add one: define `const MIGRATION_0NN: &str = r#"..."#;` (next free
  number) and append a `("0NN_short_name", MIGRATION_0NN)` entry to the `Vec`
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
- **There is no base currency (ADR 0012).** Entity tables carry a `currency`
  column next to every native amount, and native amount, currency and date are
  the only stored facts. No converted total is persisted: snapshots store
  per-currency breakdowns, ticker histories store native quantity, price and
  currency, and every reader converts into the response's declared currency at
  read time. Stored exchange rates are units of a currency per 1 EUR
  (`app_config.fx_reference`), which is the rate store's reference only. The
  `DEFAULT 'CZK'` on currency columns is a storage default for a record created
  without a currency, not an aggregation unit.
- **Per-currency breakdowns are JSON strings in TEXT columns**, e.g. the
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
| `attachment_leftovers` | Plain originals the attachment conversion found outside the data folder and does not delete | `path` UNIQUE; rows whose file is gone are dropped when the status is read (migration 021, `services/attachment_conversion.rs`) |

### Auth / profile

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `app_config` | Key-value app settings incl. recovery-key hash | `key TEXT PRIMARY KEY`, `value TEXT`; the CoinGecko API key (`api_key_coingecko`) is the only API key still read; `stock_import_formats` holds the custom stock CSV mappings (JSON array, newest first, at most 20; `services/stock_import/formats.rs`); `milestones.mutedKinds` holds the milestone kinds the user turned off (JSON array of kind names; `services/milestones/`); `fx_reference` records the currency stored rates are quoted against (`EUR` since migration 011); `history_rebuild_version` is the version of the one-time history rebuild the profile has completed (absent means never; the current version is 3): the first start of a version rebuilds every history day from dated facts and stores it once the days rebuild succeeded (`services/history_plan.rs`, ADR 0016) |
| `user_profile` | Single-row user profile and preferences | `INTEGER AUTOINCREMENT` PK (convention exception); `currency DEFAULT 'CZK'` (the main currency; creation and update accept only the 36 codes with an exchange-rate source, `validation.currencyUnsupported`; an older unsupported value is kept as is), `language`, `menu_preferences` JSON, `coingecko_modal_dismissed`, `mcp_server_enabled` gates the local API server, `mcp_server_token` TEXT (NULL until the server is first enabled), `mcp_server_port` INTEGER (NULL = default 41414), `country` TEXT (ISO 3166-1 alpha-2 of the profile, NULL until set in the wizard or Settings; migration 017), `insurance_notice_days` INTEGER (NULL = the 50-day default; 0 = no insurance-anniversary reminder; else days; migration 018), `theme` TEXT NOT NULL DEFAULT `'system'` (`system`/`light`/`dark`, validated on update with `validation.themeInvalid`; migration 022) |

### Bank accounts

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `institutions` | Banks/financial institutions, pre-seeded with Czech banks | Readable seeded IDs (`inst_fio`, `inst_csob`, …); `bic`, `logo_url` |
| `bank_accounts` | All bank/savings accounts | `account_type DEFAULT 'checking'`, `institution_id` FK, `iban`/`bban`, `data_source DEFAULT 'manual'`, `has_zone_designation`, `exclude_from_balance`, `interest_rate` TEXT, `interest_rate_valid_until` INTEGER (UTC day the promotional rate ends, nullable; migration 005), `balance_as_of` INTEGER nullable UTC booking day (migration 008; legacy remains unknown) |
| `bank_account_zones` | Tiered interest-rate zones per account | FK `bank_account_id` ON DELETE CASCADE; `from_amount`/`to_amount`/`interest_rate` TEXT. Tiers are bands of the balance (not contiguous, not sorted); the one interest model is `services/interest_tiers.rs`, mirrored for display by `src/utils/bank-account-zones.ts` and cross-checked through `shared/fixtures/interest-tiers.json`; an empty / `0` `to_amount` is unlimited |
| `bank_balance_records` | Dated balance records of an account (migration 014): one row per explicit balance update, by hand or confirmed from a statement | FK `bank_account_id` ON DELETE CASCADE; `amount`/`currency` TEXT (account currency); `as_of` UTC day start; `source` `manual` \| `statement`; `source_batch_id` → `csv_import_batches` ON DELETE SET NULL; `UNIQUE (bank_account_id, as_of)` — recording the same day replaces the row. `bank_accounts.balance`/`balance_as_of` mirror the newest record (`services/bank_accounts.rs::refresh_balance_columns`); an undated legacy balance has no record until the first dated one is recorded. Written only through `record_balance`, `create_account` (opening balance with a date) and `delete_balance_record`. |
| `bank_transactions` | Imported/manual bank transactions | `UNIQUE(bank_account_id, transaction_id)` dedupes imports (`services/dedup.rs`: certain same-bank-ID matches cannot be forced, different IDs remain distinct; fallback keys include exact canonical decimal TEXT, currency, direction, dates, description, normalized payee and safely canonicalized identifier. Stored occurrences are consumed once and within-file ID-less multiplicity is retained; possible duplicates allow explicit overrides); `category_id` FK, nullable `flow_kind` explicit meaning income/expense/transfer (NULL derives dynamically; migration 006; migration 010 rewrote the removed roles to transfer, the CHECK constraint still lists them), `categorization_source`, `import_batch_id` FK ON DELETE CASCADE, `import_line` INTEGER and `row_balance` TEXT preserve source details, `counterparty_bank_code`/`counterparty_country` TEXT preserve optional routing metadata (migration 008), `suggested_category_id` FK (MCP-suggested category awaiting confirmation); indexes on account, date, category, batch |
| `csv_import_batches` | One row per CSV upload, validated nonempty MCP bank import request, or explicitly assigned earlier MCP import call, with import stats | FK `bank_account_id` ON DELETE CASCADE; `imported_count`/`duplicate_count`/`error_count`, `promoted_count` (migration 008); reads calculate actual `linkedCount`; `date_from`/`date_to` retain the statement period even for duplicate-only records (legacy period stays unknown). MCP accepts an optional `fileName` (defaults to `MCP`) and links new `api_sync` transactions to the batch in the same SQL transaction. Atomic deletion removes only owned rows and returns the actual count; promotion preserves the preexisting row's batch ownership. Rows are created and finished through `csv_import::insert_batch`/`finish_batch` only. Legacy unbatched rows are never assigned automatically; MCP `bank_transactions_assign_import` links explicitly listed unowned `manual` rows of one account written within one minute to a new `MCP …` record dated at their original write (only `import_batch_id` changes). |

### Categorization

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `transaction_categories` | Spending categories, system-seeded + user-defined | Readable seeded IDs (`cat_groceries`, …); self-referencing `parent_id` FK, `is_system`, `sort_order`; `kind` ∈ income/expense/transfer (default expense; migration 006). Migration 010 renamed `cat_income` to Salary (when still seeded), seeded `cat_investment_income` and `cat_other_income`, moved `cat_savings` to transfer and collapsed investment/financing/adjustment into transfer; the CHECK constraint still lists the old values, which the app never writes |
| `categorization_rules` | Priority-ordered pattern rules | `rule_type`/`pattern` (`regex`, `contains`, `word`, `starts_with`, `ends_with`, `is_credit`, `is_debit`), `priority DEFAULT 50` (index `priority DESC, is_active`), `stop_processing`, `iban_pattern` |
| `learned_payees` | Learned payee → category mappings for auto-categorization | `normalized_payee` is nullable (account-identity rules). New opt-in rows carry `bank_account_id` FK ON DELETE CASCADE and `is_credit` (migration 009); a partial unique scope index prevents duplicates within one account/direction. NULL scope preserves legacy broad rules. Reliable structured identifiers canonicalize to the same IBAN; incomplete identifiers fall back to the normalized payee. Writes commit transactionally before engine learning. |

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
| `stock_data` | Fetched Yahoo Finance quote + company metadata cache | `ticker UNIQUE`; `currency` is the currency `original_price` is stored in, the one Yahoo reports for the quote (its minor units are converted: `GBp` pence are stored as GBP, a hundredth); the ticker suffix only stands in for a response that names none; `quote_currency` TEXT is the code Yahoo reported with the last quote, as it came (NULL until the first refresh after migration 004), kept to find tickers whose stored history was written in another unit (`services/quote_unit.rs`); `previous_close`, `sector`, `industry`, `pe_ratio`, `market_cap`, `beta`, 52-week range, dividend fields, `metadata_fetched_at`; `splits_checked_at` INTEGER is when the splits of the ticker were last looked up (NULL = never; migration 023) |
| `watched_stocks` | Stock Monitor watchlist: followed tickers with optional target price and markdown notes; prices come from `stock_data` via JOIN | `ticker UNIQUE`; `target_price` money-as-TEXT in native currency; `target_direction` TEXT CHECK (`below` = waiting for the price to fall to the target, `above` = waiting for it to rise; NULL without a target; migration 005 sets it for existing targets from the stored price, `above` when none is known); never enters net worth |
| `stock_price_overrides` | Manual price overrides (win over `stock_data`) | `ticker UNIQUE`; `currency DEFAULT 'CZK'` |
| `stock_splits` | Stock splits per ticker, found by the Yahoo lookup, entered by hand or carried in an import; a split waits as `pending` until the user applies or dismisses it | `UNIQUE(ticker, effective_day)`; `effective_day` is the UTC midnight of the ex-date (unix epoch INTEGER); `numerator` / `denominator` integers stored as TEXT (a 4-for-1 split is `'4'` / `'1'`); `source` CHECK (`yahoo`, `manual`, `import`); `status` CHECK (`pending`, `applied`, `dismissed`); `detected_at` DEFAULT `unixepoch()`; `resolved_at` and `applied_count` (transactions rewritten by the last apply) are NULL until resolved; index on `ticker` (migration 023) |
| `dividend_data` | Fetched yearly dividend sums per ticker | `ticker UNIQUE`; `yearly_dividend_sum` TEXT in `currency` (converted from the quote unit like a price) |
| `dividend_overrides` | Manual dividend overrides (win over `dividend_data`) | `ticker UNIQUE` |
| `stock_tags` | User tags for grouping investments | `name UNIQUE`; `group_id` FK ON DELETE SET NULL |
| `stock_investment_tags` | Investment ↔ tag many-to-many join | Composite PK `(investment_id, tag_id)`; both FKs ON DELETE CASCADE; at most one tag per tag group per investment, enforced by the tag service (`services/stock_tags.rs`; migration 015 removed older duplicates: the earliest assignment of each group stays, then by tag name); tags without a group may overlap |
| `stock_tag_groups` | Optional grouping of tags (e.g. "Strategy") | `name UNIQUE` |
| `stock_value_history` | Daily native value per ticker (also feeds portfolio trend) | `UNIQUE(ticker, recorded_at)`; native `quantity`, `price`+`currency`, `is_stale`; values convert per response at the row's day (the `value_czk` cache was dropped in migration 012) |

### Crypto

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `crypto_investments` | One row per held crypto asset | `ticker UNIQUE`, `coingecko_id`; `currency`. No stored average price (same derivation as `stock_investments`; crypto positions may mix transaction currencies) |
| `crypto_transactions` | Buy/sell transactions per crypto position | FK `investment_id` ON DELETE CASCADE; index on `ticker` (for MCP import dedup) |
| `crypto_prices` | Fetched CoinGecko price cache | `symbol UNIQUE`; `coingecko_id` |
| `crypto_price_overrides` | Manual price overrides (win over `crypto_prices`) | `symbol UNIQUE` |
| `crypto_value_history` | Daily native value per crypto ticker (also feeds portfolio trend) | `UNIQUE(ticker, recorded_at)`; native `quantity`, `price`+`currency`, `is_stale` (`value_czk` dropped in migration 012) |

### Bonds

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `bonds` | Bond holdings | `isin` nullable; `coupon_value`, `interest_rate`, `quantity`, `currency`, `maturity_date`; `purchase_date` INTEGER (UTC day, nullable = unknown; migration 020, validated like a property's: a day up to today) from which history counts the bond (0 before it); without one the bond is carried on every day |

### Loans

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `loans` | Loans/mortgages | `principal` (the original amount, never reduced), `monthly_payment`, `interest_rate` + `interest_rate_validity_date`, `start_date`/`end_date`; `balance_anchor_amount` TEXT + `balance_anchor_date` INTEGER (UTC day) are a **mirror of the newest manual balance record** (a `balance_check` row of `loan_events`; both NULL without one), written only by `services/loan_events.rs` (`refresh_anchor_mirror`). The outstanding balance is computed on read from the loan's events (`loan_balance_at` in `services/loan_amortization.rs`), never stored; a loan with a manual balance stops amortizing after it. `portfolio_metrics_history.loans_by_currency` holds the outstanding balances per currency, derived per day from the same ledger |
| `loan_events` | Extra payments, rate changes and manual balance records of a loan; the facts of the loan ledger | FK `loan_id` ON DELETE CASCADE; `kind` CHECK (`extra_payment`/`rate_change`/`balance_check`; `balance_check` is the manual balance record: it holds exactly from its day, one per day, the same day replaces), `event_day` INTEGER (UTC day), `amount`/`rate`/`monthly_payment` TEXT, `previous_rate`/`previous_monthly_payment` TEXT (terms in force before the event, migration 019; NULL on legacy rows); index `(loan_id, event_day)` |
| `loan_legacy_anchors` | Pre-019 anchors that sat on an extra-payment or rate-change day, kept for the one-time reconciliation after migration 019 | `loan_id` TEXT PK (no foreign key: a row of a deleted loan is just dropped), `amount` TEXT, `day` INTEGER (UTC day). Written only by migration 019 and emptied by `reconcile_legacy_anchors` at database open; empty on every database after its first open of this version |

### Real estate

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `real_estate` | Properties (personal or investment) | `type`; purchase/market price each with own currency column; `purchase_date` INTEGER (UTC day, nullable = unknown; migration 002) where the value trace on the detail page starts; `monthly_rent`; `recurring_costs` JSON `'[]'`, `photos` JSON `'[]'` |
| `real_estate_one_time_costs` | One-off costs per property | FK `real_estate_id` ON DELETE CASCADE; `amount` TEXT, `date` |
| `real_estate_valuations` | Dated market-value estimates per property | FK `real_estate_id` ON DELETE CASCADE; `value` TEXT, `currency`, `valued_at` INTEGER (UTC day), `note`; index `(real_estate_id, valued_at)`. The newest row is mirrored into `real_estate.market_price` (`services/valuations.rs`). A priced property has a log from day one: `create_property` writes the first row (`record_initial_valuation`) and migration 002 seeded the properties that were created before |
| `real_estate_loans` | Property ↔ loan join | Composite PK `(real_estate_id, loan_id)`; both FKs ON DELETE CASCADE |
| `real_estate_insurances` | Property ↔ insurance policy join | Composite PK `(real_estate_id, insurance_id)`; both FKs ON DELETE CASCADE |
| `real_estate_photo_batches` | Photo uploads grouped by date + description | FK `real_estate_id` ON DELETE CASCADE; indexed |
| `real_estate_photos` | Individual photos in a batch | FK `batch_id` ON DELETE CASCADE; `blob_id` + `thumb_blob_id` reference the encrypted store (see "The attachments store"); `file_path` + `thumbnail_path` (data-dir-relative) only on rows from earlier versions not converted yet; CHECK: both blobs or both paths (migration 021) |
| `real_estate_documents` | Documents (deeds, contracts) per property | FK `real_estate_id` ON DELETE CASCADE; `blob_id`, `mime_type`, `original_name`; `file_path` only on unconverted legacy rows; CHECK: a blob or a path (migration 021); `file_type` (the category), `file_size` |

### Insurance

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `insurance_policies` | Insurance policies | `type`, `provider`, `payment_frequency` (canonical `monthly`/`quarterly`/`semi_annually`/`annually`/`one_time`, see `models/frequency.rs`), one-time + regular payment each with currency column, `limits` JSON `'[]'`, `status DEFAULT 'active'` |
| `insurance_documents` | Documents per policy | FK `insurance_id` ON DELETE CASCADE; same columns and CHECK as `real_estate_documents` (migration 021) |

### Other assets

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `other_assets` | Miscellaneous assets (gold, art, …) | `quantity`, `market_price`, `average_purchase_price` TEXT; `yield_type DEFAULT 'none'` + `yield_value` |
| `other_asset_transactions` | Buy/sell transactions per other asset | FK `asset_id` ON DELETE CASCADE; index on `asset_id` (for MCP import dedup) |
| `other_asset_valuations` | Dated price-per-unit estimates per other asset | FK `asset_id` ON DELETE CASCADE; `value` TEXT, `currency`, `valued_at` INTEGER (UTC day), `note`; index `(asset_id, valued_at)`. The newest row's value is mirrored into `other_assets.market_price` (not its currency: `other_assets.currency` also prices the purchase average), so readers of today's price join `valuations::OTHER_ASSET_PRICES` (newest valuation's value and currency, else `market_price` in the asset's currency). A priced asset has a log from day one: `create_asset` writes the first row and migration 002 seeded the older ones |

### Portfolio / net worth

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `portfolio_metrics_history` | Daily net-worth snapshots per asset class | Native breakdowns only: seven `*_by_currency` JSON TEXT `DEFAULT '{}'` (`real_estate_by_currency` holds every property) plus `real_estate_personal_by_currency` (the personal subset; investment real estate is the difference per currency). The CZK `total_*` columns were dropped in migration 012 (legacy rows without breakdowns got `{"CZK": total}` once); readers convert each row at its day (`services/net_worth.rs`). `is_stale`; `source` TEXT `'live'`/`'backfill'` provenance — a live row is a snapshot taken on its day (an authentic record); every other past day is derived from dated facts (`services/history_days.rs`, ADR 0016): savings from the newest balance record, loans from the loan ledger (every day before today, live rows included), real estate from the purchase date and valuations, other assets from transactions and valuations, bonds from their purchase date, stocks and crypto from the ticker histories; what no fact dates (an account without records, an other asset with a quantity and no transactions, a bond without a purchase date) is the remainder over the facts of the nearest live row that counts the same items, today's value only when none does. One row per day from the earliest dated fact. A rebuild rewrites backfill rows whole and a live row only for the classes the facts cover completely, and deletes backfill rows before the earliest fact (live rows never); today's row belongs to the live snapshot. Index on `recorded_at`. `get_portfolio_history` returns `source`; the charts draw every day alike |

(`stock_value_history` and `crypto_value_history` above also feed this domain's trend charts.)

### Currency

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `exchange_rates` | Latest ECB rates persisted for offline use | `currency TEXT PRIMARY KEY`; **`rate REAL`** (money-as-TEXT exception) = units of the currency per 1 EUR, as the ECB publishes them (migration 011; before it, CZK per unit). CZK is stored as a quote like any other currency; EUR (the reference, 1.0) and derived pegged codes (AED, SAR, QAR, BHD, OMR, BGN) never have a row. The reference is recorded in `app_config.fx_reference` |
| `exchange_rate_history` | Daily historical rates for accurate backfills | `PRIMARY KEY (date, currency)`; **`rate REAL`** = units of the currency per 1 EUR (as `exchange_rates`: CZK stored as a quote, no EUR or derived rows; reference in `app_config.fx_reference`); index `(currency, date)`. Coverage self-heals (`services/fx_backfill.rs`): per currency (investment, crypto and bank-account transactions), the prefix before that currency's earliest transaction and any multi-day tail gap are fetched from frankfurter.dev on rate refresh and before historical-transaction recalcs; live-recorded rows win over backfill (`INSERT OR IGNORE`). ECB rows are stored under the feed's reference date (`time=` attribute, `parse_ecb_xml`), not the fetch day, so a weekend fetch lands on Friday; rates are stored as published (a feed without any usable rate is an error, a missing CZK quote is not). Backfill requests the held currencies, CZK included, and keeps days without a CZK quote. Budget reports convert foreign-currency transactions with the rates of their booking day; a CSV import that inserted rows and any manual bank transaction trigger the backfill (a no-op when covered), and a currency the ECB never quotes stays reported as missing FX (never 1:1) |

### Cashflow / projection

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `cashflow_items` | User-defined recurring income/expense items | `amount` TEXT, `frequency`, `item_type`, `category` |
| `cashflow_plan_exclusions` | Planning rows the user leaves out of the plan without deleting them | `key TEXT PRIMARY KEY`: `category:<categoryKey>` (whole category, including items added later), an automatic source id (`loan:<id>`, `insurance:<id>`, `propertyCosts:<id>`, `account:<id>`, `stock:<ticker>`, `bond:<id>`, `rental:<id>`) or `item:<manual item id>`; `created_at` (migration 013). Deleting a manual item deletes its key; excluding an unknown `item:` is rejected; keys of vanished automatic sources are kept, so a source that comes back (a stock bought again) comes back in the same state |
| `projection_settings` | Growth/contribution assumptions per asset class | `asset_type UNIQUE`; `yearly_growth_rate`, `monthly_contribution` TEXT in its own `contribution_currency` (stored as typed, converted to the report currency by the projection; legacy rows are CZK amounts) |

### Milestones

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `milestone_states` | Hidden (`done`) or snoozed state of milestone occurrences, keyed by occurrence key | `key TEXT PRIMARY KEY` (e.g. `insurance_anniversary:<policy id>:<day>`, so the next year's occurrence is a new key; upkeep keys carry the date of their data, e.g. `backup_stale:<last backup day or never>`, so a hidden upkeep item returns once the data changes); `state` CHECK (`done`/`snoozed`); `until_day` INTEGER (UTC day a snooze ends, NULL for `done`; 7 days, upkeep snoozes 30 days); `updated_at` (migration 005) |

## The attachments store (`attachments.db`)

Attachment content lives in a second SQLCipher file next to `moony.db`, attached to the main
connection as the schema `files` without a `KEY` clause, so it is encrypted with the main key
(ADR 0017, `db/files_schema.rs`). It holds content only:

| Table | Purpose | Notable columns / constraints |
|---|---|---|
| `files.blobs` | One photo, thumbnail or document | `id` TEXT UUID, `data` BLOB, `size`, `sha256` (hex), `created_at`; `auto_vacuum = INCREMENTAL`, `secure_delete = ON` |

- Owners, names and MIME types stay in the main schema; a row references its blob by `blob_id`.
  Foreign keys cannot cross schemas, so a blob is written and deleted in the same transaction as
  its row, and the unlock run sweeps unreferenced blobs (`attachment_store::delete_unreferenced`;
  its reference list must name every column that holds a blob id).
- `files` has its own append-only version list keyed by `PRAGMA files.user_version`
  (`files_schema.rs`), the one exception to "every schema change is a numbered migration in
  `migrations.rs`". A pending version first copies `attachments.db` like the main migrations
  copy `moony.db`. Never edit an applied version; append one.
- A store that cannot be attached (damaged, from another profile, newer than the build) is left
  untouched and the profile opens without it; attachment calls then report
  `attachments.storeUnavailable`.
- The webview reads attachments only through the `moony-file` scheme
  (`services/attachment_protocol.rs`).

## Data folder artefacts

Next to `moony.db` the app may create: `moony.db.bak-<unix ts>` and
`attachments.db.bak-<unix ts>` — copies taken before the first pending migration
of that database runs (newest three kept, see `db/mod.rs`; a brand-new database
has nothing to copy); `attachments-converted/` — plain originals of earlier
versions that the conversion has encrypted, deleted at the next unlock once the
store checks out; the legacy folders `real_estate_photos/`,
`real_estate_documents/`, `insurance_documents/` while plain files of earlier
versions are not converted yet; `pre-restore-<unix ts>/` — the previous account
after a restore from backup (never deleted automatically). Decrypted copies of
documents opened in another app live in the app *cache* folder under `open/`
and are wiped on lock, exit and start. Backups themselves are zip archives
written wherever the user chooses (`services/backup.rs`, format 2: both database
snapshots and the key files).

### Report history (007_report_history)

`budget_goal_versions` stores UUID versions by category, timeframe and effective UTC calendar month. Amounts are TEXT; a null amount ends a goal from that month. The upgrade backfills the existing value from its creation month and makes no claim about earlier limits. Reports integrate monthly equivalents across the selected months using each month's version and recorded FX. Goal edits and currency changes preserve earlier versions; deletion writes a current-month tombstone and cancels scheduled future versions. `budget_goals.amount`/`currency` always mirror the version in effect now (a scheduled or back-dated save never replaces it); the API exposes that version's `effectiveMonth` plus the full `versions` list, and the edit dialog prefills from the version in effect for the month being edited. Category reassignment moves historical references atomically, with the target retaining versions where effective month and timeframe collide.

`cashflow_items.replacement_source_id` optionally links a manual estimate to an automatic source. A partial unique index allows only one replacement per source. Source identifiers use a type prefix and the native entity ID (`loan:UUID`, `insurance:UUID`, `account:UUID`, `bond:UUID`, `stock:ticker`, `rental:UUID`, `propertyCosts:UUID`). The planning response retains the automatic source inventory even when a replacement excludes it from totals.
