# Device Sync Phase 1 (Engine Without Network) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the local half of device sync (spec §17 phase 1): registry, migration 029, natural IDs,
trigger capture, journal, merge, repairs and snapshot records, proven by three in-memory devices that
converge under any relay order — with no network, no crypto and nothing visible to users.

**Architecture:** A new `services::sync` module that only takes `&Connection` or plain data. TEMP
triggers generated from a registry plus `PRAGMA table_info` capture changes into `sync_clock` /
`sync_outbox` (installed only when `sync_state.enabled = 1`). The journal turns the outbox into
plaintext `Changeset` values (§9.1 JSON); the merge applies them by generation + per-column clocks
and then runs deterministic repairs through the services' own recalculation code. Phase 2 wraps the
journal output and snapshot record stream in envelopes and talks to the relay.

**Tech Stack:** Rust 2021, rusqlite 0.40 (bundled SQLCipher; `functions` feature added), uuid 1 (`v5`
added), serde/serde_json, flate2 (deflate size estimate), base64 0.23 (base64url on the wire), sha2,
rand 0.10 (seeded `StdRng` in tests).

**Spec:** `docs/specs/2026-11-06-device-sync-client-design.md` (read all of it, §16a included).

## Spec corrections (the code wins; the spec is not edited)

1. **More NO ACTION foreign keys than §7.1 D14 lists.** Besides the references to
   `transaction_categories` from transactions, rules, learned payees and budget versions, the schema
   has `bank_accounts.institution_id → institutions`, `bank_transactions.suggested_category_id →
   transaction_categories` and the self-reference `transaction_categories.parent_id`. All three are
   nullable: their orphan rule is *set NULL*.
2. **Three mirrors are recomputed-or-typed, not recomputed (§5.2).** `bank_accounts.balance` /
   `balance_as_of` keep a typed balance when the account has no `bank_balance_records`
   (`create_account` without `balance_as_of`), and `refresh_balance_columns` would reset it to `'0'`.
   `real_estate.market_price` / `market_price_currency` and `other_assets.market_price` keep a typed
   price when there is no valuation (`sync_parent_price` returns early). The plan classes all three
   recomputed-or-typed, with their valuation / record table as the source rows.
3. **`loans.interest_rate` / `monthly_payment` are typed values, not derived (§5.2).** The loan
   form edits them after events exist and the ledger applies them to the last segment
   (`loan_amortization::apply_event`, `terms_event`); `sync_current_terms` moves them only when an
   event is added or removed. They are plain **Synced** (LWW).
4. **`loan_events.previous_rate` / `previous_monthly_payment` must sync (§5.2 says Recomputed).**
   The earliest event carrying them is the only record of the loan's initial terms
   (`loan_amortization::initial_terms`). They are **Synced**, and a repair rewrites the values of
   every *later* carrier from the merged events (no such function exists today; Task 8 adds
   `loan_events::rebuild_terms_chain`).
5. **`updated_at` columns stamped by recalculations** are exactly: `bank_accounts`
   (`refresh_balance_columns`), `loans` (`refresh_anchor_mirror`, `sync_current_terms`),
   `real_estate` and `other_assets` (`sync_parent_price`; `other_assets` also
   `recalculate_asset_totals`). `stock_investments` and `crypto_investments` have no `updated_at`.
   Those four `updated_at` columns are **Local**.
6. **`real_estate.photos`** (legacy JSON list of photo paths) is missing from §5.2; it is **Local**
   like the other file paths.
7. **Two service-enforced uniqueness rules §7.4 does not list:** category names
   (case-insensitive, `categories.rs` `name_exists`) → the *rename* rule; insurance
   `(provider, policy_number)` (`insurance.rs`) → no repair (known limit, recorded in the ADR).
8. **The three override upserts live in commands**, not services (`commands/investments.rs`
   `set_manual_price` / `set_manual_dividend`, `commands/crypto.rs` `update_crypto_price`); Task 6
   moves them into `services/price_overrides.rs` (thin-command amendment). The crypto override key
   column is `symbol`, not `ticker`.
9. **§7.1's monotonic `stock_splits.status` rule diverges on revert.** "`applied`/`dismissed` beat
   `pending` whatever the clocks" means a revert on one device (an update to `pending`) never
   reaches the others. The plan keeps the intent with a commutative rule: a `pending` is *weak* when
   its clock is its row's generation (a detection insert); weak loses to any other status; otherwise
   the newer clock wins.
10. **§7.3 "ordinary per-column LWW" for same-ID rows diverges when a tombstone interleaves.** The
    plan makes the newer *generation* (insert clock) win the whole row and drops changes addressed
    to an older generation (worked example in Task 13). Two additive wire fields follow from it
    (§9.1): `"p"` (the generation of each referenced parent, per FK column carried) and a row that
    carries both `"del"` and `"ins"` when it was deleted and re-created since the last push. A
    `"barrier"` field (omitted when 0) carries §14's barrier.
11. **Recalculations break ties by `rowid` or not at all**, and rowids differ per device once rows
    arrive by merge: `valuations::latest_valuation` and `OTHER_ASSET_PRICES`,
    `loan_events::refresh_anchor_mirror` and `list_events`, `loan_amortization::ordered_events`,
    `crypto_investments::recalculate_crypto_metrics`. Task 8 makes `id` the last tie-breaker.
12. **Recalculations can fail** (`validation.sellExceedsHoldings` when two devices sold the same
    shares; `validation.exchangeRateUnavailable` for an other asset without FX history). §7.1
    assumes they always run; the repair falls back deterministically instead of aborting the merge.
13. **Build facts:** rusqlite lacks the `functions` feature (no `create_scalar_function`), uuid lacks
    `v5`, and `db::migrations` is a private module (`run_migrations` must be re-exported).
14. `sync_pending_blobs` is not created (attachments wait, §16a).

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-sync` on branch
  `feat/device-sync`; run `git branch --show-current` before every commit.
- No network, no crypto, no Tauri command, no UI, no new user-facing string in phase 1.
- G5: a profile without `sync_state.enabled = 1` has zero TEMP triggers, no `sync_clock` /
  `sync_outbox` writes, no extra work at unlock beyond one `SELECT`.
- Errors are `AppError` (rule 4); no `.unwrap()` outside `#[cfg(test)]` (rule 5); migration 029
  is append-only (rule 6); money TEXT, timestamps INTEGER unix seconds, PKs TEXT (rule 10).
- HLC: 48 bits of wall-clock milliseconds + 16-bit counter, `hlc = (ms << 16) | counter`, compared
  as `(hlc, device)`; device = 16 bytes compared as bytes.
- Future-clock warning threshold: a remote clock more than `86_400_000` ms ahead of local time.
- Envelope budget: deflated changeset JSON, zero-padded to a multiple of 1,024 bytes, at most
  `61_440` bytes (60 KiB); a single row alone may take up to `184_320` bytes (180 KiB); a row larger
  than that is refused and stays in the outbox.
- Natural-ID namespace UUID: `b7a0c3e2-5d1f-4c8e-9a6b-2f3e1d0c9b8a`; name =
  `table` + `\u{1f}` + key parts joined by `\u{1f}`.
- Rename suffix: `" (2)"`, `" (3)"`, … in ascending primary-key order; the lowest ID keeps the name.
- `app_config` synced keys: exactly `stock_import_formats`, `learned_csv_formats`,
  `milestones.mutedKinds`, `rule_packs_enabled`, `rule_packs_disabled_rules`.
- Changeset `"v": 1`; schema number = `known_migration_names().len()` = 29 after this plan.
- Rust tests use `Connection::open_in_memory()` plus the real migrations through
  `crate::db::run_migrations` (never the `Database` struct, except the one unlock test in Task 9
  that follows the existing `db::tests` pattern with `tempfile`).
- New dependencies, nothing else: `uuid` feature `v5`, `rusqlite` feature `functions`,
  `flate2 = "1"`, `base64 = "0.23"` (already in `Cargo.lock`). **No `proptest`** (plan's call):
  seeded `rand::rngs::StdRng` from `rand` 0.10 (`rand::SeedableRng`, `rand::RngExt::random_range`,
  `rand::seq::SliceRandom::shuffle`) with a fixed seed list, so a failure names its seed.
- Files stay focused: no file in `services/sync/` over ~800 lines; submodules where noted.
- Every task ends with, from `src-tauri/`:
  `cargo test --lib <task filter>`, `cargo test --lib` (whole crate),
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
  Hooks: `.husky/pre-commit` runs `npx lint-staged`, `npm run typecheck` and `cargo fmt --check`;
  `.husky/pre-push` runs `npm test`, `cargo clippy -- -D warnings` and `cargo test`. Never bypass
  them with `--no-verify`.
- New service functions other tasks call are `pub` (a `pub(crate)` function unused until a later
  task fails `clippy -D warnings` on dead code).
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A merged history that sells more than it holds** (two devices sell the same shares offline):
   the merge must commit, every device must store the same quantity, and sync must not stop —
   Task 12 `oversold_merged_history_commits_with_the_unchecked_quantity`.
2. **Mirror rows that tie on (day, created_at)** arriving in different orders on different devices:
   every device must mirror the same row — Task 8 `mirrors_break_ties_by_id_not_insertion_order`.
3. **The same changeset applied twice** (a crash between commit and cursor, a replay after a head
   reset): the second run must change nothing — Task 13 `applying_a_changeset_twice_changes_nothing`.
4. **Typed values without source rows** (an account balance never dated, a property price without
   valuations, a position without transactions): they must survive capture, journal, merge and
   repair — Task 11 `typed_value_travels_only_without_source_rows` and Task 12
   `typed_values_without_source_rows_survive_repair`.
5. **A recalculation that fails on one device only** (no FX history for a foreign-currency other
   asset purchase): the merge must still commit, keep the stored value and warn — Task 12
   `failing_recalculation_keeps_the_value_and_warns`.

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/mod.rs` | Module root, `pub mod` list, `on_open()` |
| `services/sync/probe.rs` (`#[cfg(test)]`) | Task 1 SQLite facts + the REPLACE lint |
| `services/sync/test_support.rs` (`#[cfg(test)]`) | `migrated()` connection, small seeding helpers |
| `services/sync/registry/mod.rs` | Table / column classes, groups, app_config allowlist, schema reading |
| `services/sync/registry/keys.rs` | Unique-key rules, FK graph and depth, migration classes |
| `services/sync/hlc.rs` | `DeviceId`, `Clock`, packing, `HlcState` |
| `services/sync/natural_id.rs` | UUIDv5 IDs (§7.3) |
| `services/sync/state.rs` | `sync_state` row access |
| `services/sync/capture.rs` | Trigger generation, connection functions, mode |
| `services/sync/changeset.rs` | Plaintext changeset types and §9.1 JSON |
| `services/sync/journal.rs` | Outbox → changesets, cutting, mark-sent |
| `services/sync/repair/{mod,mirrors,invariants}.rs` | §7.1 recalculations, §7.2/§7.4 repairs, seed revival |
| `services/sync/merge/{mod,row,parents,unique}.rs` | §7.1 D12–D15 merge |
| `services/sync/chain.rs` | §9.3 heads / own log verdicts, re-join and barrier re-queue |
| `services/sync/snapshot.rs` | §8 record stream, dump, restore by replace / merge |
| `services/sync/convergence/{mod,ops,scenarios}.rs` (`#[cfg(test)]`) | §18 convergence tests |
| `services/price_overrides.rs` | Override upserts moved out of commands |
| `docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md` | ADR |

---

### Task 1: Probe the SQLite facts the design rests on

**Files:**
- Modify: `src-tauri/Cargo.toml` (rusqlite features `["bundled-sqlcipher", "blob", "functions"]`)
- Modify: `src-tauri/src/db/mod.rs:8` (`pub use migrations::{applied_migration_names, known_migration_names, run_migrations};`)
- Modify: `src-tauri/src/services/mod.rs` (add `pub mod sync;`)
- Create: `src-tauri/src/services/sync/mod.rs`, `services/sync/test_support.rs`, `services/sync/probe.rs`

**Interfaces:**
- Produces: `crate::db::run_migrations(conn: &Connection) -> Result<()>` (re-export);
  `services::sync::test_support::migrated() -> Connection` (in-memory, `PRAGMA foreign_keys = ON`,
  real migrations).

The probe tests use hand-written TEMP triggers and a scalar function registered with
`conn.create_scalar_function("probe_tick", 0, FunctionFlags::SQLITE_UTF8, …)` (never
`SQLITE_DIRECTONLY`, which forbids use in triggers). They write into a probe table
`CREATE TABLE probe_log (tbl TEXT, op TEXT, pk TEXT)` created by the test in `main`.

- [ ] **Step 1: Write the probe tests** (each must pass on the real schema; a failure means the design changes, so stop and report)

```rust
#[test] fn temp_trigger_on_main_table_writes_an_unqualified_main_table()
// CREATE TEMP TRIGGER t AFTER INSERT ON main.institutions BEGIN INSERT INTO probe_log VALUES ('institutions','ins',NEW.id); END
// insert an institution → probe_log (in main, not temp) has 1 row; sqlite_temp_master lists the trigger, sqlite_master does not.
#[test] fn temp_trigger_fires_inside_a_transaction_that_also_writes_the_attached_files_schema()
// files_schema::attach(&conn, tempdir/attachments.db); BEGIN; INSERT INTO files.blobs …; INSERT INTO main.institutions …; COMMIT → 1 probe row.
#[test] fn temp_triggers_survive_attaching_files()
// create triggers first, then files_schema::attach → a later insert still logs.
#[test] fn cascade_and_set_null_fire_the_child_triggers()
// triggers on bank_transactions (AFTER DELETE, AFTER UPDATE): delete a csv_import_batches row → child 'del' logged (CASCADE);
// delete a spending_plans row → child 'upd' logged with NEW.spending_plan_id IS NULL (SET NULL).
#[test] fn on_conflict_do_update_fires_the_update_trigger_and_not_the_insert_trigger()
// app_config: INSERT … ON CONFLICT(key) DO UPDATE twice → 1 'ins', 1 'upd'.
#[test] fn insert_or_replace_deletes_without_the_delete_trigger()
// documents the reason for the lint: REPLACE into app_config → no 'del' row with recursive_triggers off.
#[test] fn a_trigger_body_can_select_a_connection_function_and_later_statements_see_its_effect()
// BEGIN SELECT probe_tick(); INSERT INTO probe_log VALUES ('x','tick', probe_now()); END → probe_now() equals the ticked value.
#[test] fn a_trigger_when_clause_reads_a_temp_table()
// CREATE TEMP TABLE sync_mode(mode TEXT); trigger WHEN (SELECT mode FROM sync_mode) = 'local'; flip the mode → fires only in 'local'.
#[test] fn defer_foreign_keys_allows_a_child_before_its_parent_inside_one_transaction()
// PRAGMA defer_foreign_keys = ON; insert bank_transactions before its bank_accounts row; COMMIT ok.
#[test] fn no_source_file_uses_insert_or_replace()
// walk src-tauri/src/**/*.rs, cut each file at its first "#[cfg(test)]", regex (?is)\b(INSERT\s+OR\s+REPLACE|REPLACE\s+INTO)\b → zero hits (plan's call: forbid it everywhere, there are none today).
```

- [ ] **Step 2: Run them** — `cd src-tauri && cargo test --lib services::sync::probe` → all PASS (they are facts, not features; a FAIL stops the plan).
- [ ] **Step 3: Whole-crate gates** — `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` → clean.
- [ ] **Step 4: Commit** — `git add -A src-tauri && git commit -m "test(sync): probe the SQLite facts trigger capture relies on"` (+ Co-Authored-By line).

---

### Task 2: Migration 029 — sync bookkeeping, `sync_devices`, `split_basis`

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (`MIGRATION_029`, Vec entry `("029_device_sync", MIGRATION_029)`, `mod migration_029` tests)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (029 bullet under "How Migrations Work"; catalog rows: new "### Device sync" section with the seven tables; amend the `investment_transactions`, `stock_price_overrides`, `watched_stocks`, `dividend_overrides` rows with `split_basis`)

**Interfaces:**
- Produces the tables every later task uses (exact DDL below; Local except `sync_devices`).

```sql
CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    relay_url TEXT, account_number TEXT, vault_id TEXT, key_epoch INTEGER, vault_keys TEXT,
    device_id BLOB NOT NULL, device_secret_key BLOB,
    next_seq INTEGER NOT NULL DEFAULT 1, last_hash BLOB,
    cursor INTEGER NOT NULL DEFAULT 0, snapshot_seq INTEGER,
    snapshot_hlc INTEGER NOT NULL DEFAULT 0,
    barrier INTEGER NOT NULL DEFAULT 0, schema_seen INTEGER NOT NULL DEFAULT 0,
    last_error TEXT, last_error_at INTEGER, last_success_at INTEGER
);
CREATE TABLE sync_clock (tbl TEXT NOT NULL, pk TEXT NOT NULL, col TEXT NOT NULL,
    hlc INTEGER NOT NULL, device BLOB NOT NULL, PRIMARY KEY (tbl, pk, col)) WITHOUT ROWID;
CREATE INDEX idx_sync_clock_device_hlc ON sync_clock(device, hlc);
CREATE TABLE sync_outbox (tbl TEXT NOT NULL, pk TEXT NOT NULL, queued_at INTEGER NOT NULL,
    PRIMARY KEY (tbl, pk)) WITHOUT ROWID;
CREATE INDEX idx_sync_outbox_queued ON sync_outbox(queued_at);
CREATE TABLE sync_heads (device BLOB PRIMARY KEY, seq INTEGER NOT NULL, hash BLOB NOT NULL,
    quarantined INTEGER NOT NULL DEFAULT 0) WITHOUT ROWID;
CREATE TABLE sync_own_log (seq INTEGER PRIMARY KEY, hash BLOB NOT NULL, max_hlc INTEGER NOT NULL);
CREATE TABLE sync_unsent (seq INTEGER PRIMARY KEY, envelope BLOB NOT NULL);
CREATE TABLE sync_devices (id TEXT PRIMARY KEY, public_key TEXT, name TEXT NOT NULL,
    platform TEXT NOT NULL, app_version TEXT NOT NULL, joined_at INTEGER NOT NULL,
    removed INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0, 1)));
ALTER TABLE investment_transactions ADD COLUMN split_basis TEXT;
ALTER TABLE stock_price_overrides ADD COLUMN split_basis TEXT;
ALTER TABLE watched_stocks ADD COLUMN split_basis TEXT;
ALTER TABLE dividend_overrides ADD COLUMN split_basis TEXT;
```

Plan's calls: `sync_devices.id` is the device ID as 32 lowercase hex characters; `sync_heads.quarantined`
holds §9.3's per-author quarantine; `schema_seen` lets §14's barrier re-queue notice "this device just
migrated past a barrier"; `snapshot_hlc` is the newest clock covered by the newest snapshot restored
or uploaded. **`split_basis` canonical form:** a JSON array of split IDs sorted ascending by ID, NULL
when empty. Backfill (the D27 meaning on today's data): every `investment_transactions` row gets the
IDs of the `applied` splits of its ticker with `effective_day > transaction_date`; every row of the
three side tables gets the IDs of the `applied` splits of its ticker with
`effective_day > updated_at` (exactly the rows `rewrite_pre_split_rows` scaled). Use
`(SELECT json_group_array(id) FROM (SELECT s.id … ORDER BY s.id))` and leave NULL where no split
matches.

- [ ] **Step 1: Write the failing tests** in `mod migration_029` (pattern of `mod migration_028`)

```rust
#[test] fn the_sync_tables_exist_and_start_empty()
// run_chain(all_migrations()); each of the seven tables exists; COUNT(*) = 0 in each.
#[test] fn sync_state_is_a_single_row()
// INSERT id=1 ok; INSERT id=2 → Err (CHECK).
#[test] fn split_basis_backfill_lists_applied_splits_after_the_row()
// database before 029 (filter names < "029_device_sync", as stock_splits_tests does): position AAPL,
// transactions on day 10 and day 30, splits 'b' applied day 20, 'a' applied day 25, 'c' pending day 25 (other day), 'd' dismissed;
// stock_price_overrides updated_at 5, watched_stocks updated_at 40 → after run_migrations:
assert_eq!(basis("investment_transactions","t-day10"), Some(r#"["a","b"]"#.into()));
assert_eq!(basis("investment_transactions","t-day30"), None);
assert_eq!(basis("stock_price_overrides","o1"), Some(r#"["a","b"]"#.into()));
assert_eq!(basis("watched_stocks","w1"), None);
#[test] fn the_chain_now_has_29_migrations()
assert_eq!(known_migration_names().len(), 29);
assert_eq!(known_migration_names().last(), Some(&"029_device_sync"));
```

- [ ] **Step 2: Run** — `cargo test --lib db::migrations::migration_029` → FAIL (no 029).
- [ ] **Step 3: Implement `MIGRATION_029`** with the DDL above plus the backfill `UPDATE`s, doc comment in the style of 026–028.
- [ ] **Step 4: Regenerate the snapshot** — `cargo test regenerate_schema_snapshot -- --ignored`; read the diff: only the seven tables, two indexes and four `split_basis` columns.
- [ ] **Step 5: Run** — `cargo test --lib db::` → PASS (including `baseline_matches_schema_snapshot`, `fresh_database_runs_the_chain_and_is_idempotent`).
- [ ] **Step 6: Docs** — `database.md` as listed under Files.
- [ ] **Step 7: Gates + commit** — `feat(db): migration 029 adds sync bookkeeping and split_basis`.

---

### Task 3: Registry — table classes and columns

**Files:**
- Create: `src-tauri/src/services/sync/registry/mod.rs`
- Modify: `services/sync/mod.rs` (`pub mod registry;`)

**Interfaces:**
- Produces:

```rust
pub enum TableClass { Synced, Seed, Derived, Local }
pub enum ColumnClass { Synced, Local, Recomputed, RecomputedOrTyped, WriteOnce, Monotonic }
pub struct SourceRows { pub table: &'static str, pub fk: &'static str }
pub enum KeyShape { Primary, Singleton(&'static str) }          // user_profile → Singleton("profile")
pub struct TableSpec {
    pub name: &'static str, pub class: TableClass, pub key: KeyShape,
    pub local: &'static [&'static str], pub recomputed: &'static [&'static str],
    pub recomputed_or_typed: &'static [(&'static [&'static str], SourceRows)],
    pub groups: &'static [&'static [&'static str]],
    pub write_once: &'static [&'static str], pub monotonic: &'static [&'static str],
}
pub const APP_CONFIG_SYNCED_KEYS: [&str; 5];
pub fn tables() -> &'static [TableSpec];
pub fn spec(table: &str) -> Option<&'static TableSpec>;
pub fn column_class(table: &str, column: &str) -> ColumnClass;    // default Synced for Synced tables
pub fn group_of(table: &str, column: &str) -> Option<&'static [&'static str]>;
pub struct ColumnInfo { pub name: String, pub decl_type: String, pub not_null: bool, pub has_default: bool, pub pk_position: u32 }
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>>;   // PRAGMA table_info
pub fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>>;
pub fn synced_tables() -> impl Iterator<Item = &'static TableSpec>;
```

Classes (§5.1 with §16a): **Synced** = the spec's list minus `real_estate_photo_batches`,
`real_estate_photos`, `real_estate_documents`, `insurance_documents`, plus `sync_devices`;
**Seed** = the ten of §5.1; **Derived** = `budget_goals`; **Local** = `_migrations`,
`attachment_leftovers`, `loan_legacy_anchors`, `mcp_pending_refresh`, the four attachment tables,
`sync_state`, `sync_clock`, `sync_outbox`, `sync_heads`, `sync_own_log`, `sync_unsent`. Names starting
with `sqlite_` are ignored.

Columns (Spec corrections 2–6):

| Table | Local | Recomputed | Recomputed-or-typed (source) |
|---|---|---|---|
| `user_profile` | `id`, `language`, `theme`, `menu_preferences`, `mcp_server_enabled`, `mcp_server_token`, `mcp_server_port`, `created_at` | | |
| `bank_accounts` | `updated_at` | | `[balance, balance_as_of]` ← `bank_balance_records.bank_account_id` |
| `loans` | `updated_at` | `balance_anchor_amount`, `balance_anchor_date` | |
| `real_estate` | `updated_at`, `photos` | | `[market_price, market_price_currency]` ← `real_estate_valuations.real_estate_id` |
| `other_assets` | `updated_at` | | `[market_price]` ← `other_asset_valuations.asset_id`; `[quantity, average_purchase_price]` ← `other_asset_transactions.asset_id` |
| `stock_investments` | | | `[quantity]` ← `investment_transactions.investment_id` |
| `crypto_investments` | | | `[quantity, currency]` ← `crypto_transactions.investment_id` |

Groups (plan's call, the §7.1 list made exact): `bank_balance_records`, `bank_transactions`,
`budget_goal_versions`, `spending_plans`, `cashflow_items`, `real_estate_one_time_costs`:
`[amount, currency]`; `crypto_price_overrides`: `[price, currency]`; `crypto_transactions`,
`other_asset_transactions`: `[quantity, price_per_unit, currency]`; `investment_transactions`:
`[quantity, price_per_unit, fee, currency, split_basis]`; `stock_price_overrides`:
`[price, currency, split_basis]`; `dividend_overrides`: `[yearly_dividend_sum, currency, split_basis]`;
`watched_stocks`: `[target_price, target_direction, split_basis]`; `investment_dividends`:
`[gross_amount, currency]`; `real_estate_valuations`, `other_asset_valuations`: `[value, currency]`;
`real_estate`: `[purchase_price, purchase_price_currency]`, `[monthly_rent, monthly_rent_currency]`;
`insurance_policies`: `[one_time_payment, one_time_payment_currency]`,
`[regular_payment, regular_payment_currency]`; `loans`: `[principal, currency]`; `bonds`:
`[coupon_value, currency]`; `projection_settings`: `[monthly_contribution, contribution_currency]`.
Standalone currency columns: `bank_accounts.currency`, `stock_investments.currency`,
`other_assets.currency`, `user_profile.currency`. A recomputed-or-typed column list is itself a group.
`monotonic`: `stock_splits.status`. `write_once`: `sync_devices.public_key`.

- [ ] **Step 1: Write the failing tests** (`registry::tests`, on `test_support::migrated()`)

```rust
#[test] fn every_table_of_the_migrated_schema_has_a_class()      // names from sqlite_master minus sqlite_*; message names the missing table
#[test] fn every_registered_table_exists()                        // no stale names
#[test] fn every_named_column_exists()                            // local, recomputed, r-or-t, groups, write_once, monotonic, source fk
#[test] fn the_attachment_tables_are_local_in_v1()                // §16a: the four tables → TableClass::Local
#[test] fn no_synced_column_is_real()                              // decl_type REAL only in Seed tables (§9.1)
#[test] fn every_non_synced_not_null_column_has_a_default()        // Local/Recomputed NOT NULL columns need DEFAULT so a merged insert can omit them
#[test] fn every_currency_column_is_grouped_or_listed_standalone() // columns ending in "currency" of Synced tables
#[test] fn a_column_belongs_to_at_most_one_group()
#[test] fn user_profile_is_the_profile_singleton()
assert!(matches!(spec("user_profile").unwrap().key, KeyShape::Singleton("profile")));
#[test] fn the_app_config_allowlist_is_exactly_the_five_keys()
assert_eq!(APP_CONFIG_SYNCED_KEYS, ["stock_import_formats","learned_csv_formats","milestones.mutedKinds","rule_packs_enabled","rule_packs_disabled_rules"]);
#[test] fn the_four_recalculated_updated_at_columns_are_local()
for t in ["bank_accounts","loans","real_estate","other_assets"] { assert_eq!(column_class(t,"updated_at"), ColumnClass::Local) }
assert_eq!(column_class("spending_plans","updated_at"), ColumnClass::Synced);
#[test] fn loan_terms_and_previous_terms_are_synced()               // Spec corrections 3 and 4
```

- [ ] **Step 2: Run** — `cargo test --lib services::sync::registry` → FAIL (module missing).
- [ ] **Step 3: Implement** `registry/mod.rs` as a `static` slice of `TableSpec` plus the PRAGMA readers.
- [ ] **Step 4: Run** → PASS. **Step 5: Gates + commit** — `feat(sync): registry of table and column classes`.

---

### Task 4: Registry — unique keys, foreign keys, migration classes

**Files:**
- Create: `src-tauri/src/services/sync/registry/keys.rs` (`pub mod keys;` in `registry/mod.rs`)

**Interfaces:**
- Consumes: Task 3 `tables()`, `spec()`.
- Produces:

```rust
pub enum UniqueRule { SameId, KeepNewest, Rename { fold_case: bool }, Unlink }
pub struct UniqueSpec { pub table: &'static str, pub index: &'static str,
    pub exprs: &'static [&'static str], pub partial: Option<&'static str>,
    pub name_column: Option<&'static str>, pub rule: UniqueRule }
pub fn unique_rules() -> &'static [UniqueSpec];
pub enum OrphanRule { SetNull, DropChild }
pub enum OnDelete { Cascade, SetNull, NoAction(OrphanRule) }
pub struct ForeignKey { pub child: String, pub column: String, pub parent: String, pub parent_column: String, pub on_delete: OnDelete }
pub fn foreign_keys(conn: &Connection) -> Result<Vec<ForeignKey>>;   // PRAGMA foreign_key_list, Synced child AND Synced parent only
pub fn fk_depth(conn: &Connection) -> Result<BTreeMap<String, u32>>; // 0 = no Synced parent; self-references ignored
pub enum MigrationClass { Additive, Barrier }
pub const MIGRATION_CLASSES: &[(&str, MigrationClass)];
pub fn schema_number() -> u32;     // known_migration_names().len()
pub fn barrier_number() -> u32;    // 1-based position of the newest Barrier migration
```

Unique rules (the index names are SQLite's; read them in the test from `PRAGMA index_list`, the
`index` field is the stable expression list, not the autoindex name): **SameId** —
`stock_investments(ticker)`, `crypto_investments(ticker)`, `watched_stocks(ticker)`,
`stock_price_overrides(ticker)`, `dividend_overrides(ticker)`, `crypto_price_overrides(symbol)`,
`stock_splits(ticker, effective_day)`, `projection_settings(asset_type)`,
`bank_transactions(bank_account_id, transaction_id)`, `idx_learned_payees_scoped`
(`COALESCE(normalized_payee,'')`, `COALESCE(counterparty_iban,'')`, `bank_account_id`, `is_credit`,
partial `bank_account_id IS NOT NULL`); **KeepNewest** — `bank_balance_records(bank_account_id, as_of)`,
`budget_goal_versions(category_id, timeframe, effective_month)`; **Rename** `{fold_case: false}` —
`stock_tags(name)`, `stock_tag_groups(name)`; **Unlink** — `cashflow_replacement_source`
(`replacement_source_id`, partial `replacement_source_id IS NOT NULL`). Service-enforced uniqueness
(no index) lives in the repair module (Task 12/14), not here.

Orphan rules for NO ACTION (Spec correction 1): `bank_transactions.category_id`,
`bank_transactions.suggested_category_id`, `transaction_categories.parent_id`,
`bank_accounts.institution_id` → `SetNull`; `categorization_rules.category_id`,
`learned_payees.category_id`, `budget_goal_versions.category_id` → `DropChild`.

Migration classes (plan's call; ≤ 029 predate sync, so this records history for D26 and its test):
Barrier = `010_income_categories`, `011_fx_eur_reference`, `012_retire_czk_caches`,
`014_balance_records`, `015_one_tag_per_group`, `019_loan_manual_records`,
`021_encrypted_attachments`, `025_crypto_yahoo_symbol`; every other name through `029_device_sync`
Additive.

- [ ] **Step 1: Write the failing tests** (`registry::keys::tests`)

```rust
#[test] fn every_unique_index_of_a_synced_table_has_a_rule()
// PRAGMA index_list(t) for Synced t, unique=1, origin != 'pk'; match by index_info column names (or the
// expression index by name idx_learned_payees_scoped) → message names table and columns without a rule.
#[test] fn every_rule_names_an_existing_unique_index()
#[test] fn the_fk_graph_comes_from_pragma_with_its_on_delete_rules()
assert!(fks.iter().any(|f| f.child=="bank_transactions" && f.column=="spending_plan_id" && matches!(f.on_delete, OnDelete::SetNull)));
assert!(fks.iter().any(|f| f.child=="investment_transactions" && f.column=="investment_id" && matches!(f.on_delete, OnDelete::Cascade)));
#[test] fn every_no_action_key_has_an_orphan_rule()   // a NO ACTION FK with no rule → foreign_keys() returns Err naming it
#[test] fn a_required_column_cannot_be_set_null()      // DropChild exactly where the child column is NOT NULL
#[test] fn depth_orders_every_parent_before_its_children()
let d = fk_depth(&conn)?; assert_eq!(d["bank_transactions"], 3); assert_eq!(d["csv_import_batches"], 2);
for fk in foreign_keys(&conn)? { if fk.child != fk.parent { assert!(d[&fk.parent] < d[&fk.child]) } }
#[test] fn every_migration_has_a_class_and_no_class_is_stale()
assert_eq!(MIGRATION_CLASSES.iter().map(|m| m.0).collect::<Vec<_>>(), crate::db::known_migration_names());
#[test] fn migrations_after_029_insert_synced_rows_only_with_deterministic_ids()
// for every migration name > "029_device_sync" (none yet): its SQL must not contain "randomblob(" (case-insensitive);
// reads the SQL through a new `#[cfg(test)] pub(crate) fn migration_sql(name) -> Option<&'static str>` in migrations.rs.
#[test] fn schema_number_is_29_and_the_newest_barrier_is_025()
assert_eq!(schema_number(), 29); assert_eq!(barrier_number(), 25);
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::registry::keys` → FAIL. **Step 3: Implement.** **Step 4:** → PASS.
- [ ] **Step 5: Gates + commit** — `feat(sync): unique-key rules, FK graph and migration classes`.

---

### Task 5: Hybrid logical clock

**Files:**
- Create: `src-tauri/src/services/sync/hlc.rs`

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)] pub struct DeviceId(pub [u8; 16]);
impl DeviceId { pub const ZERO: Self; pub fn random() -> Self; pub fn to_hex(&self) -> String; pub fn from_hex(s: &str) -> Result<Self>;
                pub fn to_b64(&self) -> String; pub fn from_b64(s: &str) -> Result<Self>; }   // base64url, no padding
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)] pub struct Clock { pub hlc: u64, pub device: DeviceId } // Ord = (hlc, device)
impl Clock { pub const ZERO: Self; }
pub const FUTURE_WARNING_MS: u64 = 86_400_000;
pub fn pack(ms: u64, counter: u16) -> u64;  pub fn unpack(hlc: u64) -> (u64, u16);
pub struct HlcState { last: u64 }
pub enum Observation { Ok, FarFuture { ahead_ms: u64 } }
impl HlcState { pub fn new(last: u64) -> Self; pub fn next(&mut self, now_ms: u64) -> u64; pub fn observe(&mut self, remote: u64, now_ms: u64) -> Observation; pub fn last(&self) -> u64; }
pub fn now_ms() -> u64;
```

`next`: `ms = max(now_ms, unpack(last).0)`; counter 0 when `ms` moved, else `counter + 1`; at
`u16::MAX` the ms part advances by one and the counter restarts at 0 (plan's call). `observe` sets
`last = max(last, remote)` and reports `FarFuture` when `unpack(remote).0 > now_ms + FUTURE_WARNING_MS`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn packing_is_48_bits_of_ms_and_16_of_counter()
assert_eq!(pack(1_777_000_000_000, 123), 116_457_472_000_000_123);
assert_eq!(unpack(116_457_472_000_000_123), (1_777_000_000_000, 123));
assert_eq!(pack((1 << 48) - 1, u16::MAX) as i64 > 0, true);          // fits a positive SQLite INTEGER
#[test] fn next_is_strictly_increasing_even_when_the_wall_clock_goes_back()
#[test] fn the_counter_overflow_moves_the_ms_part()
let mut s = HlcState::new(pack(5, u16::MAX)); assert_eq!(s.next(5), pack(6, 0));
#[test] fn observe_moves_the_clock_to_the_highest_seen()
#[test] fn a_clock_more_than_24_hours_ahead_is_reported()
let now = 1_000_000_000; let mut s = HlcState::new(0);
assert!(matches!(s.observe(pack(now + 86_400_000, 0), now), Observation::Ok));
assert!(matches!(s.observe(pack(now + 86_400_001, 0), now), Observation::FarFuture { ahead_ms: 86_400_001 }));
#[test] fn equal_hlcs_are_ordered_by_device_bytes()
#[test] fn device_ids_round_trip_hex_and_base64url()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::hlc` PASS → gates → commit `feat(sync): hybrid logical clock`.

---

### Task 6: Natural IDs in the creating services (§7.3 "same ID")

**Files:**
- Modify: `src-tauri/Cargo.toml` (`uuid = { version = "1", features = ["v4", "v5", "serde"] }`)
- Create: `src-tauri/src/services/sync/natural_id.rs`
- Create: `src-tauri/src/services/price_overrides.rs` (+ `pub mod price_overrides;` in `services/mod.rs`)
- Modify: `services/investments.rs:290-299, 498-504` (new positions), `services/crypto_investments.rs:186`,
  `services/stock_monitor.rs:23`, `services/projection.rs:58-63`, `services/stock_splits/mod.rs:94,133,231`,
  `services/bank_accounts.rs:372` (`create_transaction_with_origin`), `services/categorization/learn.rs:226`
  (`learn_scoped_and_apply`), `commands/investments.rs:470-545`, `commands/crypto.rs:340-365`

**Interfaces:**
- Produces: `pub const NAMESPACE: Uuid = Uuid::from_u128(0xb7a0c3e2_5d1f_4c8e_9a6b_2f3e1d0c9b8a);`
  `pub fn natural_id(table: &str, key: &[&str]) -> String` (`Uuid::new_v5(&NAMESPACE, name)`, name =
  `table` + `"\u{1f}"` + parts joined by `"\u{1f}"`, lowercase hyphenated string).
- Produces in `services/price_overrides.rs`:
  `pub fn upsert_stock_price(conn, ticker: &str, price: &str, currency: &str, now: i64) -> Result<StockPriceOverride>`,
  `pub fn upsert_dividend(conn, ticker: &str, amount: &str, currency: &str, now: i64) -> Result<DividendOverride>`,
  `pub fn upsert_crypto_price(conn, symbol: &str, price: &str, currency: &str, now: i64) -> Result<()>` —
  the commands' SQL moved verbatim except the ID; the commands keep their snapshot calls.

Key parts per table (normalised exactly as stored): `stock_investments` / `crypto_investments` /
`watched_stocks` / `stock_price_overrides` / `dividend_overrides` [uppercase ticker];
`crypto_price_overrides` [uppercase symbol]; `stock_splits` [ticker, effective_day as decimal];
`projection_settings` [asset_type] (a client-supplied `id` is ignored for new rows: plan's call);
`bank_transactions` [bank_account_id, trimmed transaction_id] only when `transaction_id` is set,
else `Uuid::new_v4()`; `learned_payees` [`payee` or "", `identity` or "", bank_account_id,
`"1"`/`"0"` for is_credit].

- [ ] **Step 1: Failing tests**

```rust
// natural_id.rs
#[test] fn known_answers_pin_the_namespace_and_the_separator()
assert_eq!(natural_id("stock_investments", &["AAPL"]), "0dee614b-ed60-5962-ab0a-cc1d356a50ad");
assert_eq!(natural_id("stock_splits", &["NVDA", "1718064000"]), "89519a96-305b-52e9-9d93-4d828c336723");
assert_eq!(natural_id("learned_payees", &["ALBERT", "", "acc-1", "0"]), "325164e5-8176-51d7-86d9-f6dd04f3b363");
#[test] fn the_table_name_is_part_of_the_id()
assert_ne!(natural_id("watched_stocks", &["AAPL"]), natural_id("stock_investments", &["AAPL"]));
// services (each in the service's own tests module, on its existing setup_test_db or test_support::migrated()):
#[test] fn two_profiles_buying_the_same_ticker_create_the_same_position_id()          // investments.rs
#[test] fn a_crypto_holding_id_follows_its_ticker()                                    // crypto_investments.rs
#[test] fn following_a_ticker_uses_its_natural_id()                                    // stock_monitor.rs
#[test] fn a_detected_split_id_follows_ticker_and_day()                                // stock_splits (record_detected, insert_from_import, record_manual)
#[test] fn a_new_projection_setting_id_follows_its_asset_type()                        // projection.rs
#[test] fn a_bank_transaction_with_a_bank_id_gets_the_natural_id_and_one_without_a_random_one() // bank_accounts.rs
#[test] fn re_learning_a_scope_reuses_the_natural_id()                                 // learn.rs
#[test] fn override_upserts_use_natural_ids_and_keep_the_row_on_conflict()             // price_overrides.rs: second upsert keeps id, changes price
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::natural_id services::price_overrides services::investments services::crypto_investments services::stock_monitor services::stock_splits services::projection services::bank_accounts services::categorization` PASS → gates → commit `feat(sync): natural IDs for rows with a natural key`.

---

### Task 7: `split_basis` maintained by splits, transactions and overrides (D27)

**Files:**
- Modify: `src-tauri/src/services/stock_splits/mod.rs` (apply / revert / record_manual through one reconcile)
- Modify: `services/investments.rs` (`create_transaction_internal`, `update_transaction`), `services/price_overrides.rs`, `services/stock_monitor.rs:103` (`set_target_price`)
- Modify: `docs/DEPRECATED.md` (new section "## Retired with device sync (do not re-add)": the
  `updated_at < effective_day` selection in `rewrite_pre_split_rows` — `split_basis` decides; and
  `INSERT OR REPLACE` anywhere)

**Interfaces:**
- Produces in `stock_splits`:

```rust
pub fn basis_for_day(conn: &Connection, ticker: &str, day: i64) -> Result<Option<String>>;
// canonical JSON of the applied splits of `ticker` with effective_day > day, sorted by id; None when empty
pub fn reconcile_ticker(conn: &Connection, ticker: &str) -> Result<ReconcileReport>;
pub struct ReconcileReport { pub per_split: BTreeMap<String, i64> /* split id → investment_transactions rewritten */, pub rows: i64 }
```

`reconcile_ticker` (the one rewrite both local actions and the sync repair use; plan's call): for
each row of the ticker in `investment_transactions`, `stock_price_overrides`, `watched_stocks`,
`dividend_overrides`: (1) **remove** — every split in the row's basis whose status is not `applied`,
newest `effective_day` first (ties by id descending): scale back (the existing `scale_amount` with
`(d, n)` and the existing roundings) and drop it from the basis; (2) **add** — every `applied` split
not in the basis that *affects* the row, oldest first (ties by id ascending): scale by `(n, d)` and add
it. A transaction is affected when `transaction_date < effective_day`; a side row when
`updated_at < effective_day`. The basis is the source of truth: an applied split already in the
basis is never removed because the row no longer looks affected. Then drop the ticker's
`dividend_data` row (as today) and run the existing `refresh_positions`. `apply_in` = set
`applied`/`resolved_at`, `reconcile_ticker`, `applied_count = per_split[id]`; `revert` = set
`pending`, `reconcile_ticker`. Writes: `create_transaction_internal` and `update_transaction` store
`basis_for_day(ticker, transaction_date)`; the three side-table writes store
`basis_for_day(ticker, now)` (normally NULL).

- [ ] **Step 1: Failing tests** (`stock_splits` tests, on `test_support::migrated()`)

```rust
#[test] fn apply_records_the_split_in_the_basis_of_every_rewritten_row()
#[test] fn revert_scales_back_exactly_the_rows_that_carry_the_split()
// a transaction added after apply with a pre-split date (basis already lists S) is scaled back too; a side row edited after apply (basis NULL) is not
#[test] fn reconcile_scales_a_pre_split_row_that_missed_the_apply()
// insert a pre-split transaction with split_basis NULL while S is applied (what a merge produces) → reconcile → quantity × n/d, basis [S]
#[test] fn reconcile_removes_a_split_that_is_no_longer_applied()
// row basis [S], set S dismissed by SQL → reconcile → scaled back, basis NULL
#[test] fn reconcile_is_idempotent()
#[test] fn a_new_transaction_dated_before_an_applied_split_carries_it()
assert_eq!(basis("investment_transactions", &id), Some(format!(r#"["{s}"]"#)));
#[test] fn an_override_set_after_the_split_carries_no_basis()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::stock_splits services::investments services::stock_monitor services::price_overrides` PASS (existing split tests must stay green) → gates → commit `feat(stocks): split_basis makes split apply and revert exact`.

---

### Task 8: Service entry points the repairs reuse

**Files:**
- Modify: `services/bank_accounts.rs:568` (`pub fn refresh_balance_columns`)
- Modify: `services/valuations.rs:163-176, 205-225, 230` (`pub fn sync_parent_price`; `latest_valuation` and `OTHER_ASSET_PRICES` order `…, id DESC`)
- Modify: `services/loan_events.rs` (`refresh_anchor_mirror`/`list_events`: `rowid` → `id`; new `rebuild_terms_chain`)
- Modify: `services/loan_amortization.rs:387-391` (`ordered_events` sorts by `(day, created_at, id)`; `LedgerEvent` gains `id` if it lacks it)
- Modify: `services/crypto_investments.rs:29-32` (`ORDER BY transaction_date, created_at, id`)
- Modify: `services/budgeting.rs` (`pub fn merge_equal_neighbours`; extract `pub fn rebuild_goal_row(conn, category_id, timeframe, now) -> Result<()>` from the `displayed_version` block of `delete_goal_intervals` and the mirror step of `upsert_goal_at`, both now call it)

**Interfaces:**
- Produces: `bank_accounts::refresh_balance_columns(conn, account_id: &str) -> Result<()>`;
  `valuations::sync_parent_price(conn, kind: ValuationKind, asset_id: &str) -> Result<()>`;
  `budgeting::merge_equal_neighbours(conn, category_id: &str, timeframe: &str) -> Result<()>`;
  `budgeting::rebuild_goal_row(conn, category_id: &str, timeframe: &str, now: i64) -> Result<()>`
  (row exists iff `displayed_version(versions, month_start(now))` is `Some`; insert with a new v4 id,
  update amount/currency, or delete); `loan_events::rebuild_terms_chain(conn, loan_id: &str) -> Result<usize>`
  — for `previous_rate` and separately `previous_monthly_payment`: the carriers (events whose field is
  non-NULL) ordered by `(event_day, created_at, id)`; the first keeps its value; each later carrier
  gets the term in force after the carriers before it (a `rate_change` sets the rate to its `rate`;
  an event with `monthly_payment` sets the payment to it); returns the rows changed.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn mirrors_break_ties_by_id_not_insertion_order()          // Review Focus 2 (valuations.rs tests)
// two connections; same two valuations (same valued_at, same created_at, ids "a" and "b", different values) inserted in opposite orders
// → sync_parent_price gives the same market_price on both (the value of "b").
#[test] fn the_crypto_currency_tie_is_broken_by_id()
#[test] fn rebuild_terms_chain_fixes_a_later_carrier_and_keeps_the_first()
// rate changes added as two devices would: day 50 (prev 3 → 5) and day 100 (prev 3 → 4) → rebuild → day-100 previous_rate "5", day-50 previous_rate "3"; second run returns 0.
#[test] fn rebuild_goal_row_follows_the_displayed_version()
// versions: Jan 100, Mar NULL; now = Feb → row 100; now = Apr → no row; budget with a scheduled future version only → row exists.
#[test] fn delete_goal_intervals_and_upsert_still_mirror_through_rebuild_goal_row() // existing budgeting tests stay green
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::valuations services::loan_events services::loan_amortization services::crypto_investments services::budgeting services::bank_accounts` PASS → gates → commit `refactor(services): deterministic mirrors and callable recalculations for sync repairs`.

---

### Task 9: Capture — `sync_state`, connection functions, TEMP triggers, unlock hook

**Files:**
- Create: `src-tauri/src/services/sync/state.rs`, `services/sync/capture.rs`
- Modify: `services/sync/mod.rs` (`pub fn on_open(conn: &Connection)`)
- Modify: `src-tauri/src/services/categorization/packs.rs` (new `pub fn persist_effective(conn) -> AppResult<()>`: writes `rule_packs_enabled` and `rule_packs_disabled_rules` from `read_config`, so the language default stops deciding once sync is on)
- Modify: `src-tauri/src/db/mod.rs:86` (call `crate::services::sync::on_open(&conn)` right after `attach_attachments_store`, before `load_rates_from_db` and the two self-heals)

**Interfaces:**
- Consumes: registry (Tasks 3–4), `hlc` (Task 5).
- Produces:

```rust
// state.rs
pub struct SyncState { pub enabled: bool, pub device: DeviceId, pub next_seq: u64, pub last_hash: Option<[u8; 32]>,
                       pub cursor: u64, pub snapshot_hlc: u64, pub barrier: u32, pub schema_seen: u32 }
pub fn load(conn: &Connection) -> Result<Option<SyncState>>;
pub fn enable(conn: &Connection, device: DeviceId) -> Result<()>;      // upsert row id=1, enabled=1, schema_seen=schema_number(), barrier=barrier_number(); then packs::persist_effective (§5.2)
pub fn disable(conn: &Connection) -> Result<()>;                       // §11.6 local part: uninstall triggers, delete sync_state/clock/outbox/heads/own_log/unsent
pub fn record_error(conn: &Connection, message: &str) -> Result<()>;  // last_error, last_error_at = unixepoch()
// capture.rs
pub enum Mode { Local, Merge, Repair }
pub fn install(conn: &Connection, device: DeviceId) -> Result<()>;
pub fn uninstall(conn: &Connection) -> Result<()>;
pub fn is_installed(conn: &Connection) -> Result<bool>;
pub fn mode(conn: &Connection) -> Result<Mode>;
pub fn with_mode<T>(conn: &Connection, mode: Mode, f: impl FnOnce() -> Result<T>) -> Result<T>; // restores the previous mode on Ok and Err
pub fn set_cascade_clock(conn: &Connection, clock: Clock) -> Result<()>;
pub fn observe(conn: &Connection, hlc: u64) -> Result<Observation>;   // SELECT sync_hlc_observe(?1)
pub fn tick(conn: &Connection) -> Result<u64>;                         // SELECT sync_hlc_tick() (journal/tests)
pub fn trigger_sql(conn: &Connection, device: DeviceId) -> Result<Vec<String>>;
pub fn pk_expr(spec: &TableSpec, pk: &[String], row: &str) -> String; // "NEW.id" | "json_array(NEW.a, NEW.b)" | "'profile'"
// mod.rs
pub fn on_open(conn: &Connection);    // never fails: logs and record_error
```

Mechanics (plan's calls):
- `install` registers three functions with `FunctionFlags::SQLITE_UTF8` sharing one
  `Arc<Mutex<HlcState>>` initialised from `SELECT COALESCE(MAX(hlc), 0) FROM sync_clock`:
  `sync_hlc_tick()` (advance and return), `sync_hlc_now()` (last issued), `sync_hlc_observe(x)`
  (returns 1 when the observation was `FarFuture`). It creates
  `CREATE TEMP TABLE sync_mode (id INTEGER PRIMARY KEY CHECK (id = 1), mode TEXT NOT NULL, cascade_hlc INTEGER, cascade_device BLOB)`
  with `('local')`, then executes `trigger_sql`. The device ID is baked into the SQL as `x'…'`.
- Per Synced table (from `registry` + `columns()`), triggers `sync_<t>_ins`, `sync_<t>_upd`,
  `sync_<t>_pk`, `sync_<t>_del`; `app_config` adds `WHEN NEW.key IN (<allowlist>)` /
  `OLD.key IN (…)`; the key is `pk_expr` (single PK → `NEW.id`, composite → `json_array(…)`,
  singleton → `'profile'`).
  - **INSERT**, mode `local`: `SELECT sync_hlc_tick()`; upsert `(t, pk, '*', sync_hlc_now(), device)`;
    delete the key's column clocks (`col NOT IN ('*','-')`); `INSERT INTO sync_outbox … ON CONFLICT DO NOTHING`
    with `queued_at = sync_hlc_now()`. Other modes: nothing.
  - **UPDATE**, mode `local`, `WHEN` any captured column changed (`OLD.c IS NOT NEW.c` over Synced,
    WriteOnce and Monotonic columns, and recomputed-or-typed columns gated by
    `NOT EXISTS (SELECT 1 FROM <source> WHERE <fk> = NEW.<pk>)`): tick once; for each captured column
    `c` an `INSERT … SELECT … WHERE OLD.c IS NOT NEW.c`; for a group, one condition over all members
    writes the clock of **every** member; outbox as above.
  - **PK change** (`AFTER UPDATE OF <pk cols> WHEN OLD.pk IS NOT NEW.pk`, mode `local`): tombstone
    the old key and record `'*'` for the new key, both queued.
  - **DELETE**: mode `local` → tick, upsert `'-'` with `(sync_hlc_now(), device)`, delete the key's
    column clocks (keep `'*'`), queue. Mode `merge` → upsert `'-'` with the `cascade_hlc` /
    `cascade_device` from `sync_mode`, delete column clocks, no queue. Mode `repair` → nothing.
  - Every clock upsert keeps the maximum: `ON CONFLICT(tbl, pk, col) DO UPDATE SET hlc = excluded.hlc,
    device = excluded.device WHERE (excluded.hlc, excluded.device) > (sync_clock.hlc, sync_clock.device)`.
- `on_open`: if `state::load` says enabled → `schema_seen = schema_number()` → `install`. Task 12
  inserts `repair::delete_revived_seeds` and Task 15 the barrier re-queue before `install` (no stubs
  here). Any error → `log::warn!` + `record_error`; the profile always opens.

- [ ] **Step 1: Failing tests** (`capture::tests`, `state::tests`, plus one in `db::tests`)

```rust
#[test] fn a_profile_without_sync_has_no_triggers_and_no_journal()          // G5
// migrated(); on_open(&conn); write through bank_accounts::create_account → sqlite_temp_master has 0 triggers; sync_clock and sync_outbox empty.
#[test] fn installed_triggers_cover_every_synced_table_and_nothing_else()
#[test] fn an_insert_records_a_row_clock_and_queues_the_row()
#[test] fn an_update_records_clocks_only_for_changed_synced_columns()
// update spending_plans.note → clock for 'note' only; update loans.updated_at only → no clock, no outbox row.
#[test] fn a_group_member_change_stamps_every_member_with_one_clock()
// bank_transactions amount change → 'amount' and 'currency' clocks equal.
#[test] fn a_recomputed_or_typed_column_is_captured_only_without_source_rows()
// stock_investments.quantity UPDATE with no transactions → clock; after inserting a transaction → no clock for quantity.
#[test] fn a_delete_records_a_tombstone_and_keeps_the_generation()
#[test] fn only_allowlisted_app_config_keys_are_captured()
// 'rule_packs_enabled' → queued; 'fx_reference' and 'backup.lastCreatedAt' → not.
#[test] fn user_profile_is_captured_as_the_profile_singleton_and_local_columns_are_not()
#[test] fn merge_mode_records_cascaded_tombstones_with_the_cascade_clock_and_queues_nothing()
#[test] fn repair_mode_records_nothing()
#[test] fn with_mode_restores_the_previous_mode_after_an_error()
#[test] fn the_hlc_starts_above_every_stored_clock()
#[test] fn disable_removes_triggers_and_every_sync_row()
#[test] fn enabling_sync_writes_the_effective_rule_packs()
// profile language 'cs', no rule_packs_enabled key → enable → app_config has rule_packs_enabled == the Czech default, and it is queued.
// db::tests
#[test] fn opening_a_sync_enabled_profile_installs_capture_before_the_self_heals()
// create_with_key; a position with one transaction and a stale quantity '99' written by SQL; enable(device); close; open_with_key →
// capture::is_installed == true; the self-heal corrected the quantity; sync_clock has no 'quantity' clock for that position
// (recomputed while it has source rows) and the outbox holds no entry for it.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::capture services::sync::state db::tests` PASS → gates → commit `feat(sync): TEMP-trigger capture installed only for sync-enabled profiles`.

---

### Task 10: Changeset wire format (§9.1)

**Files:**
- Modify: `src-tauri/Cargo.toml` (`base64 = "0.23"`, `flate2 = "1"`)
- Create: `src-tauri/src/services/sync/changeset.rs`

**Interfaces:**
- Consumes: `DeviceId`, `Clock` (Task 5).
- Produces:

```rust
pub enum SqlValue { Null, Integer(i64), Real(f64), Text(String), Blob(Vec<u8>) }   // + From/To rusqlite::types::Value
pub enum RowKey { Single(String), Composite(Vec<String>), Singleton }
impl RowKey { pub fn to_pk_text(&self, spec: &TableSpec) -> String; pub fn from_pk_text(spec: &TableSpec, pk: &str) -> Result<Self>; }
pub struct RowChange { pub table: String, pub key: RowKey, pub generation: Clock, pub insert: bool,
    pub deleted: Option<Clock>, pub columns: BTreeMap<String, (SqlValue, Clock)>, pub parents: BTreeMap<String, Clock> }
pub struct Changeset { pub schema: u32, pub barrier: u32, pub device: DeviceId, pub seq: u64,
    pub prev: Option<[u8; 32]>, pub rows: Vec<RowChange> }
pub const CHANGESET_VERSION: u32 = 1;
pub fn to_json(cs: &Changeset) -> Vec<u8>;
pub fn from_json(bytes: &[u8]) -> Result<Changeset>;      // AppError::Validation("sync.changesetInvalid") on any shape error
pub fn padded_deflated_len(json: &[u8]) -> usize;          // deflate (flate2 default level) then round up to 1024
pub fn max_hlc(cs: &Changeset) -> u64;
```

Wire (exactly §9.1, plus Spec correction 10): top level `v`, `schema`, `barrier` (omitted when 0),
`device` (base64url), `seq`, `prev` (base64url; omitted when `seq == 1`), `devs` (base64url list,
first-use order, every clock's device including the author's), `rows`. A row: `t`, `k` (string |
array | `"profile"`), `g` `[hlc, i]`, `ins: true` (omitted when false), `del` `[hlc, i]` (optional),
`c` `{col: [value, hlc, i]}` (omitted when empty), `p` `{fk_col: [hlc, i]}` (omitted when empty).
Values: JSON null, string, integer, float (seed tables only), `{"b": "<base64>"}` for BLOB. Clock
zero is `[0, i]` with `DeviceId::ZERO` in `devs`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn the_spec_example_round_trips()
// the two rows of §9.1 (amount/currency insert; stock_investment_tags composite delete) → from_json(to_json(x)) == x
#[test] fn the_json_uses_the_spec_field_names()
// serde_json::Value of to_json: v == 1, rows[0].ins == true, rows[1].k is an array, rows[0].c.amount[0] == "-1250.00", rows[0].g is [hlc, 0]
#[test] fn ties_keep_the_author_of_each_clock()      // two clocks with equal hlc and different devices → two devs entries, distinct indexes
#[test] fn blob_values_travel_as_base64_objects()
#[test] fn an_unknown_device_index_is_rejected()
assert!(from_json(br#"{"v":1,"schema":29,"device":"…","seq":1,"devs":[],"rows":[{"t":"bonds","k":"x","g":[1,0]}]}"#).is_err());
#[test] fn a_version_other_than_1_is_rejected()
#[test] fn padded_length_is_a_multiple_of_1024()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::changeset` PASS → gates → commit `feat(sync): plaintext changeset format`.

---

### Task 11: Journal — outbox to changesets

**Files:**
- Create: `src-tauri/src/services/sync/journal.rs`

**Interfaces:**
- Consumes: registry, `fk_depth`, `foreign_keys`, `state::load`, `changeset::*`.
- Produces:

```rust
pub const ENVELOPE_BUDGET: usize = 61_440;
pub const SINGLE_ROW_LIMIT: usize = 184_320;
pub struct Built { pub changeset: Changeset, pub outbox_keys: Vec<(String, String)>, pub max_hlc: u64 }
pub struct Refused { pub table: String, pub pk: String, pub bytes: usize }
pub fn build_next(conn: &Connection) -> Result<(Option<Built>, Vec<Refused>)>;   // reads only; the next envelope, None when nothing is left
pub fn mark_sent(conn: &Connection, built: &Built, hash: [u8; 32]) -> Result<()>;
pub fn watermark(conn: &Connection) -> Result<u64>;                       // MAX(max_hlc) of sync_own_log, 0 if none
pub fn read_row(conn: &Connection, table: &str, pk: &str, watermark: u64) -> Result<Option<RowChange>>;  // also used by snapshot (watermark 0 = full row)
```

Rules (plan's calls where marked):
- A live row whose `'*'` hlc > watermark → full row (`insert: true`): every column except Local
  ones, Recomputed and recomputed-or-typed included (NOT NULL columns must reach the insert; the
  receiver's repair overwrites derived ones). Otherwise a partial row: columns whose clock (column
  clock, else `'*'`) hlc > watermark, whole groups together, recomputed-or-typed columns only while
  the row has no source rows, never Recomputed or Local. A partial row with no column left is
  dropped from the outbox silently.
- A live row that has a `'-'` clock and is sent as `insert` carries `deleted` = that clock
  (re-created since; Spec correction 10). A missing row → `deleted` = its `'-'` clock, `generation`
  = its `'*'` (or ZERO), no columns.
- `parents`: for each FK column carried with a non-NULL value, the parent row's `'*'` clock (ZERO
  when it has none or is missing locally).
- Clocks: a column without its own clock carries `'*'`; a row without `'*'` carries ZERO.
- **Order:** live rows sorted by `(fk_depth, queued_at, tbl, pk)`, `transaction_categories` rows
  parent-first within equal depth; then tombstones sorted by `(fk_depth DESC, queued_at, tbl, pk)`
  (children's updates and tombstones travel before a parent's tombstone; plan's call, see Task 13).
- **Cutting:** fill greedily; after each row compute `padded_deflated_len(to_json(candidate))`; when
  it exceeds `ENVELOPE_BUDGET`, close the envelope without that row. A row that alone exceeds the
  budget travels alone if ≤ `SINGLE_ROW_LIMIT`, otherwise it is `Refused` (logged with
  `log::error!`, left in the outbox; other rows continue). Plan's call: to avoid quadratic cost,
  re-measure only when the raw JSON grew past the last measured size by 8 KiB, and always before
  closing.
- One envelope per call (plan's call): `seq` = `next_seq`, `prev` = `last_hash` (the hash of the
  previous envelope exists only after sealing), so callers loop `build_next` → hash/seal →
  `mark_sent` until it returns `None`; rows already taken stay in the outbox until `mark_sent`;
  `schema` = `schema_number()`, `barrier` = `sync_state.barrier`.
- `mark_sent`: delete exactly `outbox_keys`; insert `sync_own_log(seq, hash, max_hlc)`;
  `next_seq = seq + 1`, `last_hash = hash`. Phase 1 callers pass `sha2::Sha256` of `to_json`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn five_edits_of_one_row_send_one_value()
#[test] fn a_row_changed_then_deleted_sends_only_its_tombstone()
#[test] fn an_insert_since_the_last_push_is_a_full_row_and_a_later_edit_is_partial()
#[test] fn local_and_recomputed_columns_never_travel_in_a_partial_row()
#[test] fn typed_value_travels_only_without_source_rows()                   // Review Focus 4
#[test] fn a_deleted_and_recreated_row_carries_del_and_ins()
#[test] fn every_fk_value_carries_its_parent_generation()
#[test] fn parents_travel_before_children_and_children_tombstones_before_parents()
#[test] fn a_5000_row_import_is_cut_into_envelopes_under_60_kib_with_batch_first()
// csv_import_batches row + 5,000 bank_transactions via bank_accounts::create_transaction_with_origin → every padded_deflated_len ≤ 61_440,
// > 1 envelope, the batch row in envelope 0, every transaction after it, union of rows == outbox.
#[test] fn the_largest_allowed_row_travels_alone()
// watched_stocks.notes = 100 KiB of non-repeating text (seeded StdRng, base64 of random bytes) via stock_monitor::update_notes → one envelope, 61_440 < len ≤ 184_320.
#[test] fn a_row_over_180_kib_is_refused_and_stays_queued()
// same row with notes forced to 200 KiB by SQL → Refused{table:"watched_stocks",…}; sync_outbox still has it; other rows built.
#[test] fn mark_sent_clears_the_keys_and_advances_the_chain()
// seq 1 → own_log (1, hash, max_hlc), next_seq 2, last_hash == hash, the next build_next has seq 2 and prev == hash.
#[test] fn rows_queued_after_build_next_stay_in_the_outbox()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::journal` PASS → gates → commit `feat(sync): journal builds changesets from the outbox`.

---

### Task 12: Repairs after a merge (§7.1 steps 1–3, §7.2, §7.4, §14 seeds)

**Files:**
- Create: `src-tauri/src/services/sync/repair/mod.rs`, `repair/mirrors.rs`, `repair/invariants.rs`
- Modify: `services/sync/mod.rs` (`on_open` calls `delete_revived_seeds` before `install`)

**Interfaces:**
- Consumes: Tasks 7–9; `stock_splits::reconcile_ticker`; `mcp_refresh::enqueue`;
  `attachment_store::delete_unreferenced`.
- Produces:

```rust
#[derive(Default)] pub struct Touched { pub rows: BTreeMap<String, BTreeSet<String>>, pub parents: BTreeMap<String, BTreeSet<String>>, pub deleted: BTreeSet<String> /* tables */, pub days: BTreeMap<&'static str, Option<i64>> }
impl Touched { pub fn row(&mut self, table: &str, pk: &str); pub fn parent(&mut self, table: &str, id: &str); pub fn day(&mut self, domain: &'static str, day: Option<i64>); }
#[derive(Default)] pub struct RepairReport { pub warnings: Vec<String>, pub domains: BTreeMap<&'static str, Option<i64>> }
pub fn after_merge(conn: &Connection, touched: &Touched, now: i64) -> Result<RepairReport>;  // runs under with_mode(Repair)
pub fn full(conn: &Connection, now: i64) -> Result<RepairReport>;                           // every parent; used after snapshot restore
pub fn delete_revived_seeds(conn: &Connection) -> Result<usize>;
pub fn tombstone_local(conn: &Connection, table: &str, pk: &str, clock: Clock) -> Result<()>; // '-' upsert-max without outbox (repair deletes)
pub fn domain_of(table: &str) -> Option<(&'static str, Option<&'static str>)>;            // (mcp_refresh domain, date column)
```

`after_merge` runs, for touched parents only, in this order (each step a pure function of the merged
rows; every row selection ordered by primary key):
1. **Mirrors** (`mirrors.rs`): `refresh_balance_columns` for accounts that have records;
   `refresh_anchor_mirror` and `rebuild_terms_chain` for loans; `sync_parent_price` for real estate /
   other assets that have valuations; `stock_splits::reconcile_ticker` for every touched ticker
   (from `investment_transactions`, `stock_splits`, the three side tables, `stock_investments`);
   `recalculate_investment_metrics` / `recalculate_crypto_metrics` / `recalculate_asset_totals` for
   parents that have source rows; `merge_equal_neighbours` + `rebuild_goal_row` for every touched
   `(category_id, timeframe)` of `budget_goal_versions`. **Failure policy** (Spec correction 12,
   plan's call): `validation.sellExceedsHoldings` / `validation.stockSplitOversold` → write the
   unchecked net quantity `max(0, Σbuy − Σsell)` as canonical decimal text and add a warning;
   any other recalculation error → keep the stored value and add a warning. Never return `Err` for a
   recalculation; SQL errors still propagate.
2. **Invariants** (`invariants.rs`): one tag per group per investment → keep the assignment with the
   newest `'*'` clock (ties by `tag_id`), delete the others with `tombstone_local` at the winner's
   clock; one `balance_check` per loan per day → same rule on `loan_events`; spending-plan names per
   `year` with `spending_plans::name_key` folding and category names with `trim().to_lowercase()` →
   rename rule (lowest id keeps the name, others `" (n)"`, n from 2, skipping names already taken);
   attachment blobs: when `deleted` contains `real_estate` or `insurance_policies` and
   `files_schema::is_attached`, `attachment_store::delete_unreferenced`.
3. **Domains** (`domain_of`): `stocks` ← stock_investments, investment_transactions,
   investment_dividends, stock_splits, stock_price_overrides, dividend_overrides; `crypto` ←
   crypto_investments, crypto_transactions, crypto_price_overrides; `other-assets` ← other_assets,
   other_asset_transactions, other_asset_valuations; `bank-accounts` ← bank_accounts,
   bank_balance_records, bank_transactions, csv_import_batches, bank_account_zones; `real-estate` ←
   real_estate, real_estate_valuations, real_estate_one_time_costs, real_estate_loans; `loans` ← loans,
   loan_events; `bonds` ← bonds. The earliest day is the min of the date column's old and new values
   of touched child rows; a parent row change or a tombstone makes it `None` (from the first fact).
   `after_merge` calls `mcp_refresh::enqueue(conn, domain, day)` for each and returns them.

`delete_revived_seeds`: in every Synced table, delete (no triggers involved: it runs before
`install`) the rows whose key has a `'-'` clock and either no `'*'` or a `'*'` older than the `'-'`.

- [ ] **Step 1: Failing tests** (`repair::tests`, real services on `migrated()` with sync enabled)

```rust
#[test] fn oversold_merged_history_commits_with_the_unchecked_quantity()   // Review Focus 1
// buy 15, then two sells of 10 inserted by SQL as a merge would → after_merge Ok; quantity "0"; one warning; second run identical.
#[test] fn failing_recalculation_keeps_the_value_and_warns()                // Review Focus 5
// other asset in EUR, a USD buy, no exchange_rate_history → after_merge Ok; average_purchase_price unchanged; warning names the asset.
#[test] fn typed_values_without_source_rows_survive_repair()                // Review Focus 4
// account balance "1234" without records, property market_price "5000000" without valuations, position quantity "7" without transactions → full() leaves all three.
#[test] fn mirrors_follow_the_merged_source_rows()
#[test] fn one_tag_per_group_keeps_the_newest_assignment()
#[test] fn one_balance_check_per_day_keeps_the_newest()
#[test] fn duplicate_spending_plan_names_get_numbered_suffixes_by_id()
// "Holiday" (id b), "holiday" (id a), "Holiday (2)" (id c) same year → a "holiday", b "Holiday (3)", c "Holiday (2)".
#[test] fn duplicate_category_names_fold_non_ascii_case()                   // "Děti" / "děti"
#[test] fn budget_versions_are_normalised_and_the_goal_row_rebuilt()
#[test] fn split_units_follow_the_merged_status()
#[test] fn touched_domains_are_queued_with_the_earliest_day()
assert_eq!(mcp_refresh::pending(&conn)?.iter().find(|j| j.domain=="bank-accounts").unwrap().earliest_date, Some(day));
#[test] fn repairs_record_no_clock_and_queue_nothing()
#[test] fn a_seeded_row_with_a_tombstone_is_deleted_again()
// delete category 'cat_groceries'-style seed with sync on (tombstone), re-insert it by SQL as a migration would → delete_revived_seeds == 1.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::repair` PASS → gates → commit `feat(sync): deterministic repairs after a merge`.

---

### Task 13: Merge core — generations, tombstones, column clocks, parents (D12–D14)

**Files:**
- Create: `src-tauri/src/services/sync/merge/mod.rs`, `merge/row.rs`, `merge/parents.rs`

**Interfaces:**
- Consumes: Tasks 3–5, 9, 10, 12.
- Produces:

```rust
pub const DATA_CHANGED_EVENT: &str = "sync-data-changed";   // the engine (phase 2) emits it with ApplyReport.domains
pub enum StopReason { MissingParent { table: String, key: String, parent: String }, MissingRow { table: String, key: String }, NewerSchema(u32) }
pub struct ApplyReport { pub applied: usize, pub dropped: usize, pub domains: BTreeMap<&'static str, Option<i64>>, pub warnings: Vec<String> }
pub enum Outcome { Applied(ApplyReport), Stopped(StopReason) }
pub fn apply_changeset(conn: &Connection, cs: &Changeset, hash: [u8; 32], now: i64) -> Result<Outcome>;
// merge/row.rs — also used by snapshot::restore_merge
#[derive(Default)] pub struct MergeCtx { pub touched: Touched, pub report: ApplyReport }
pub enum RowResult { Applied, Dropped, Stop(StopReason) }
pub fn apply_row(conn: &Connection, change: &RowChange, ctx: &mut MergeCtx) -> Result<RowResult>;
```

`apply_changeset`: if `cs.schema > schema_number()` → `Stopped(NewerSchema)` without a transaction.
Else one `unchecked_transaction`, `PRAGMA defer_foreign_keys = ON`, `with_mode(Merge)`; every clock
goes through `capture::observe` (a `FarFuture` adds a warning naming the author device and is logged
with `log::warn!`); rows **in changeset order**; any `StopReason` rolls back and returns `Stopped`;
then `repair::after_merge` (Repair mode), `sync_heads` upsert `(cs.device, cs.seq, hash)`, commit.

Row algorithm (`row.rs`; the spec's rules made order-independent — plan's call):

```
L = local '*' clock (ZERO if the row exists without one), D = local '-' clock (None if absent), live = row exists
0. parents.rs on the FK columns the change carries (may rewrite the change, drop it as cascaded, or Stop)
1. if change.deleted = T:
       if live && T > L: delete it (set_cascade_clock(T); NO ACTION orphan rules first, see parents.rs); live = false
       record '-' = max(D, T)
       if !change.insert: done
2. if change.insert (generation g):
       live && g >  L → replace the row in place (UPDATE every carried column; no DELETE, children stay),
                         drop the key's column clocks, '*' = g, then column clocks for columns whose clock != g
       live && g == L → column LWW (step 4)
       live && g <  L → drop (an older generation)
       !live && D >= g → drop (deleted after this insert)
       !live otherwise → unique.rs outcome (Task 14) then INSERT, '*' = g, column clocks != g
3. else (partial update):
       !live && D present → drop;  !live → Stop(MissingRow)
       g < L → drop;  g > L → Stop(MissingRow) (a generation this device never saw)
       g == L → column LWW (step 4)
4. column LWW: local clock of c = column clock or L; for a group, both sides' group clock = max over
   members; the incoming side wins when its clock is greater; the winner's members are all written.
   stock_splits.status: weak = ('pending' && clock == generation); weak loses to non-weak, else clock.
   sync_devices.public_key: a non-NULL local value never changes; a different incoming value is ignored
   with a warning.
```

`parents.rs` for each FK `(col → parent)` whose new value `v` is non-NULL, with `pg = change.parents[col]`
(ZERO if absent), parent `PL` / `PD`: OK when the parent is live and `PL == pg`, or live with
`PL > pg` and no `PD > pg` (a concurrent same-ID insert, children kept). Otherwise, when the parent is
missing with `PD` present, or live with `PL > pg` and `PD > pg` (the child's parent generation was
deleted): apply the FK's rule — `Cascade` / `NoAction(DropChild)` → the change becomes a tombstone
at `PD` (delete the child if live, record `'-'` = `PD`, `Touched`); `SetNull` /
`NoAction(SetNull)` → write NULL to `col` (with the change's clock). A parent live with `PL < pg`, or
missing without `PD` → `Stop(MissingParent)`. Before deleting a parent row (step 1 or a tombstone
change), apply NO ACTION orphan rules to its live local children under `with_mode(Repair)`: set NULL
(nullable) or delete with `tombstone_local` at the parent's tombstone clock — otherwise the deferred
FK check fails the whole changeset at commit.

Worked example the tests pin (why the newer generation wins whole): A deletes AAPL (`T`) and
re-creates it (`g5`); B concurrently adds transaction `c` under the old generation `gP`. With
`"del"` + `"ins"` and `"p"`, every device ends with AAPL at `g5` and without `c`, whichever order the
relay serves A's and B's changesets in.

- [ ] **Step 1: Failing tests** (`merge::tests`; two or three `migrated()` connections, changesets from Task 11's `build_next`, or hand-made `RowChange`s for the stop cases)

```rust
#[test] fn an_insert_is_created_with_its_clocks_and_triggers_queue_nothing()
#[test] fn a_newer_column_clock_wins_and_an_older_one_is_dropped()
#[test] fn equal_hlcs_tie_break_by_device()
#[test] fn a_group_is_taken_whole_from_the_newer_side()                  // amount from A, currency from B never mix
#[test] fn d13_a_tombstone_beats_a_later_edit_of_the_same_generation()
#[test] fn d13_an_insert_newer_than_the_tombstone_recreates_the_row()
#[test] fn a_newer_generation_replaces_the_row_and_old_generation_edits_are_dropped()
#[test] fn the_newer_generation_wins_whatever_the_order_with_an_interleaved_tombstone()   // the worked example, both orders
#[test] fn cascade_child_added_offline_follows_the_parent_tombstone()
#[test] fn set_null_child_added_offline_loses_the_reference()             // spending_plan_id
#[test] fn no_action_orphan_nullable_category_is_cleared()
#[test] fn no_action_orphan_required_rule_is_dropped()
#[test] fn a_tombstone_with_local_no_action_children_still_commits()
#[test] fn missing_parent_without_tombstone_stops_and_rolls_back()
assert!(matches!(apply_changeset(&b, &cs, h, now)?, Outcome::Stopped(StopReason::MissingParent{..}))); // b unchanged, no head
#[test] fn a_partial_update_to_an_unknown_row_stops()
#[test] fn a_detection_insert_never_undoes_an_applied_split()             // weak pending
#[test] fn a_revert_reaches_the_other_devices()                           // update to pending wins by clock
#[test] fn the_public_key_of_a_device_is_write_once()
#[test] fn a_clock_more_than_24_hours_ahead_is_applied_with_a_warning()
#[test] fn a_newer_schema_stops_before_touching_anything()
#[test] fn applying_a_changeset_twice_changes_nothing()                   // Review Focus 3: dump of synced tables + sync_clock identical
#[test] fn merge_moves_the_local_clock_past_every_clock_it_saw()
#[test] fn the_head_of_the_author_is_recorded_in_the_same_transaction()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::merge` PASS → gates → commit `feat(sync): merge by generation and per-column clocks`.

---

### Task 14: Merge — unique-key outcomes (D15)

**Files:**
- Create: `src-tauri/src/services/sync/merge/unique.rs`; Modify: `merge/row.rs` (call it before INSERT and before an UPDATE that writes an indexed column)

**Interfaces:**
- Consumes: `registry::keys::unique_rules`, `repair::tombstone_local`.
- Produces: `pub fn resolve(conn: &Connection, table: &str, incoming_pk: &str, values: &BTreeMap<String, SqlValue>, generation: Clock) -> Result<Resolution>`
  with `pub enum Resolution { Write(BTreeMap<String, SqlValue>) /* possibly renamed or unlinked */, Drop }`.

Conflicts are found by evaluating the index expressions over the incoming values
(`SELECT <exprs> FROM (SELECT ?1 AS c1, …)`) and comparing with local rows (`partial` clause
honoured, the incoming key excluded). Outcomes, all written in Repair mode before the incoming write:
- **SameId**, different key (impossible after D22) → handled as KeepNewest plus a warning.
- **KeepNewest**: compare generations; the loser is deleted (`tombstone_local` at the winner's
  generation) or, if it is the incoming row, `Drop` with its key tombstoned the same way.
- **Rename**: compute final names for the conflict group including the incoming row (lowest primary
  key keeps the name, others `" (n)"` from 2, skipping taken names); rename local rows first, then
  `Write` the incoming row with its final name. Applies to updates of the name too.
- **Unlink**: among rows linked to the same `replacement_source_id`, every one but the lowest id is
  set NULL (local rows first, or the incoming value).

- [ ] **Step 1: Failing tests**

```rust
#[test] fn same_id_two_devices_buying_aapl_offline_share_one_position()   // through investments services on A and B, exchange both ways
#[test] fn keep_newest_balance_record_of_the_same_day()
#[test] fn keep_newest_budget_version_of_the_same_month()
#[test] fn rename_tags_with_the_same_name_by_id()
#[test] fn rename_tag_groups_also_on_a_rename_update()
#[test] fn unlink_two_items_replacing_the_same_source()
#[test] fn learned_payee_scope_rows_with_random_and_natural_ids_keep_the_newer()   // the SameId fallback
#[test] fn every_outcome_is_the_same_in_both_orders()                    // A→B and B→A give identical tables
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::merge` PASS → gates → commit `feat(sync): unique-key outcomes avoid constraint conflicts`.

---

### Task 15: Chain bookkeeping — heads, own log, re-join and barrier re-queue (§9.3, §11.4, §14)

**Files:**
- Create: `src-tauri/src/services/sync/chain.rs`; Modify: `services/sync/mod.rs` (`on_open` runs the barrier re-queue after the seed revival, before `schema_seen` is updated and before `install`)

**Interfaces:**
- Produces:

```rust
pub struct EnvelopeMeta { pub device: DeviceId, pub seq: u64, pub prev: Option<[u8; 32]>, pub hash: [u8; 32], pub schema: u32, pub barrier: u32 }
impl EnvelopeMeta { pub fn of(cs: &Changeset, hash: [u8; 32]) -> Self; }
pub enum Verdict { Apply, SkipReplay, OwnAcknowledged, Clone, Quarantine, Held, Gap, NewerSchema, BelowBarrier }
pub fn judge(conn: &Connection, meta: &EnvelopeMeta) -> Result<Verdict>;     // pure over sync_state, sync_heads, sync_own_log
pub fn acknowledge_own(conn: &Connection, seq: u64) -> Result<()>;           // deletes sync_unsent(seq)
pub fn quarantine(conn: &Connection, device: DeviceId) -> Result<()>;
pub fn skip_below_barrier(conn: &Connection, meta: &EnvelopeMeta) -> Result<()>; // advances that author's head without applying
pub fn requeue_own_since(conn: &Connection, device: DeviceId, since_hlc: u64) -> Result<usize>;
pub fn rejoin(conn: &Connection, new_device: DeviceId) -> Result<usize>;
```

`judge` in this order: own device → `OwnAcknowledged` when `sync_own_log` has `(seq, hash)`, else
`Clone`; `schema > schema_number()` → `NewerSchema`; author quarantined → `Held`; no head and
`seq == 1`, or `seq == head + 1` with `prev == head.hash` → `Apply` (or `BelowBarrier` when
`meta.barrier < sync_state.barrier`); `seq < head`, or `seq == head` with the same hash →
`SkipReplay`; `seq == head` with another hash, or `seq == head + 1` with a different `prev` →
`Quarantine`; `seq > head + 1` (or no head and `seq > 1`) → `Gap`. `requeue_own_since` inserts into
the outbox (`ON CONFLICT DO NOTHING`, `queued_at` = the clock) every live or tombstoned key with a
clock `device = ?1 AND hlc > ?2` (`idx_sync_clock_device_hlc`). `rejoin`: confirmed = max `max_hlc`
of `sync_own_log` (the newest own envelope the device can confirm), requeue with the old device and
that watermark, then set `device_id = new`, `next_seq = 1`, `last_hash = NULL`, clear
`sync_own_log` and `sync_unsent`, and reinstall capture with the new device when installed. Barrier
(`on_open`): when `schema_seen < barrier_number()`: `requeue_own_since(device, snapshot_hlc)` and
`barrier = barrier_number()`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn the_next_sequence_with_the_right_prev_is_applied()
#[test] fn a_replay_is_skipped()
#[test] fn a_forked_author_is_quarantined_and_held_while_others_apply()
#[test] fn a_gap_stops_until_the_missing_envelope_arrives()
#[test] fn an_own_envelope_in_the_log_is_acknowledged()
#[test] fn an_own_id_envelope_never_sealed_here_means_a_clone()
#[test] fn an_envelope_from_a_newer_schema_is_not_applied()
#[test] fn an_envelope_below_the_barrier_is_skipped_and_the_head_moves()
#[test] fn rejoin_requeues_own_rows_newer_than_the_last_confirmed_envelope()
#[test] fn rejoin_takes_a_new_device_id_and_restarts_the_chain()
#[test] fn crossing_a_barrier_requeues_own_rows_newer_than_the_snapshot()
// enable; snapshot_hlc = X; own clocks above and below X; schema_seen = 24 → on_open → only the rows above X are queued; barrier == 25, schema_seen == 29.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::chain` PASS → gates → commit `feat(sync): envelope chain verdicts, re-join and barrier re-queue`.

---

### Task 16: Snapshot records — dump, restore by replace, restore by merge (§8)

**Files:**
- Create: `src-tauri/src/services/sync/snapshot.rs`

**Interfaces:**
- Consumes: `journal::read_row`, merge row application (`merge::row::apply_row`), `repair::full`, registry.
- Produces:

```rust
pub enum Record { Header { schema: u32, barrier: u32, seq: u64, heads: Vec<(DeviceId, u64, [u8; 32])> },
                  Devices(Vec<DeviceId>), Row(RowChange), Tomb { table: String, key: RowKey, generation: Clock, deleted: Clock },
                  Seed { table: String, row: BTreeMap<String, SqlValue> }, End { rows: u64, tombs: u64, seeds: u64 } }
pub struct DumpStats { pub rows: u64, pub tombs: u64, pub seeds: u64, pub max_hlc: u64 }
pub fn dump(conn: &Connection, seq: u64, out: &mut impl std::io::Write) -> Result<DumpStats>;
pub fn read_record(input: &mut impl std::io::Read) -> Result<Option<Record>>;
pub fn restore_replace(conn: &Connection, input: &mut impl std::io::Read, now: i64) -> Result<DumpStats>;
pub fn restore_merge(conn: &Connection, input: &mut impl std::io::Read, now: i64) -> Result<DumpStats>;
```

Stream (plan's call; phase 2 deflates and encrypts the same bytes): each record is a 4-byte
big-endian length followed by one JSON object with `"r"` ∈ `header | devs | row | tomb | seed | end`;
clocks are `[hlc, i]` into the preceding `devs` record (written first; one per stream). Order:
`devs`, `header`, live rows of Synced tables by FK depth (categories parent-first), `tomb` records,
`seed` rows of the ten Seed tables, `end`. Row records are full rows (`read_row` with watermark 0)
including the `user_profile` singleton's synced columns and the allowlisted `app_config` keys only;
never `sync_state` or Local columns. A stream without `end`, or whose counts differ, is rejected
before anything is written (read fully first; plan's call: phase 1 holds records in memory). A
header whose `barrier` is below `sync_state.barrier` is refused with
`AppError::Validation("sync.snapshotBelowBarrier")` (§8: a device never restores from below a
barrier it has crossed).

`restore_replace` (join): one transaction, Repair mode if capture is installed,
`defer_foreign_keys`; delete the contents of every Synced and Seed table (deepest first; for
`user_profile` keep row 1 and overwrite its synced columns; for `app_config` delete only the
allowlisted keys); clear `sync_clock`, `sync_outbox`, `sync_heads`; insert rows and clocks, tombstones,
seeds, heads; `sync_state.cursor = seq`, `snapshot_seq = seq`, `snapshot_hlc = max_hlc`; then
`repair::full`. `restore_merge` (after `410`, re-join): rows and tombstones go through the merge's
row application (generations and clocks decide), seeds are inserted only where the primary key is
absent, heads take the higher `seq` per device, outbox untouched; then `repair::full`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn a_dump_restored_by_replace_reproduces_every_synced_table_and_clock()
#[test] fn replace_discards_the_joining_profiles_own_rows_and_seeds()       // a seed category deleted in the vault does not come back
#[test] fn local_data_never_enters_a_snapshot()
// mcp_server_token, language, fx_reference, backup.lastCreatedAt, sync_state, attachment rows → absent from every record.
#[test] fn the_seed_tables_travel_as_plain_rows()
#[test] fn a_truncated_stream_is_rejected_without_writing()
#[test] fn restore_by_merge_keeps_newer_local_edits_and_the_outbox()
#[test] fn heads_and_cursor_come_from_the_header()
#[test] fn derived_columns_are_recomputed_after_restore()                   // budget_goals rebuilt, balances mirrored
#[test] fn a_snapshot_from_below_a_crossed_barrier_is_refused()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::snapshot` PASS → gates → commit `feat(sync): snapshot record stream with replace and merge restore`.

---

### Task 17: The JSON export skips every sync table (§11.4)

**Files:**
- Modify: `src-tauri/src/services/backup.rs:39, 640-648` (`export_all_data` filters `name.starts_with("sync_")` next to `EXPORT_DENYLIST`; doc comment says why: vault keys, device private key, account number)

**Interfaces:** none new.

- [ ] **Step 1: Failing test** in `backup.rs` tests

```rust
#[test] fn export_skips_every_sync_table()
// migrated() + state::enable + one captured write + a sync_devices row → export_all_data → the JSON "tables" object has no key starting with "sync_";
// the user tables are still there.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::backup` PASS → gates → commit `fix(backup): the full export never contains sync bookkeeping`.

---

### Task 18: Convergence harness and the random-order property test (§18)

**Files:**
- Create: `src-tauri/src/services/sync/convergence/mod.rs` (`#[cfg(test)]`), `convergence/ops.rs`

**Interfaces:**
- Consumes: everything above.
- Produces (test-only):

```rust
pub struct Device { pub conn: Connection, pub id: DeviceId, pub log: Vec<(Changeset, [u8; 32])> /* what it pushed */ }
pub struct Vault { pub devices: Vec<Device>, pub relay: Vec<(Changeset, [u8; 32])> }
impl Vault {
    pub fn new(n: usize, seed_ops: impl FnOnce(&Connection)) -> Self; // device 0 seeds + enables; 1..n restore_replace(dump(device 0)) and enable
    pub fn push(&mut self, d: usize);                    // loop journal::build_next → hash → mark_sent → relay.push until None
    pub fn pull(&mut self, d: usize, order: &[usize]);   // apply relay entries by index through chain::judge + merge::apply_changeset
    pub fn pull_all_random(&mut self, rng: &mut StdRng); // each device gets its own interleaving that keeps every author's seq order
    pub fn assert_converged(&self);                       // every Synced table, synced + recomputed-or-typed columns, ordered by pk
}
pub fn random_op(dev: &Device, rng: &mut StdRng);        // ops.rs
```

`assert_converged` compares all Synced columns exactly, `other_assets.average_purchase_price` within
`1e-9` relative (§5.2; every device gets the same seeded `exchange_rate_history`), ignores Local
columns, and also compares the set of tombstoned keys. `ops.rs` calls the real services only
(plan's call on the catalogue, each op picks existing entities of that device with the rng):
`bank_accounts::{create_account, update_account, create_transaction, set_transaction_category, record_balance, delete_balance_record, delete_account}`,
`categories::{create_category, update_category, delete_category(reassign)}`,
`spending_plans::{create_plan, update_plan, delete_plan, set_transaction_plans}`,
`budgeting::{upsert_goal_at, delete_goal_intervals, delete_budget}`,
`investments::create_transaction_internal` (via the position-creating entry), `investments::{update_transaction, delete_transactions}`,
`price_overrides::*`, `stock_monitor::{follow_stock, set_target_price, update_notes, unfollow_stock}`,
`stock_tags::{create_group, create_tag, update_tag, set_investment_tags, delete_tag}`,
`stock_splits::{record_detected, apply, revert, dismiss}`,
`crypto_investments::{create_crypto_with_transaction, add_transaction_to_crypto, delete_transaction}`,
`loans::{create_loan, update_loan}`, `loan_events::{add_event, delete_event}`,
`real_estate::{create_property, update_property}`, `valuations::{add_valuation, delete_valuation}`,
`other_assets::{create_asset, create_asset_transaction}`,
`cashflow_planning::{create_cashflow_item, update_cashflow_item, delete_cashflow_item, set_plan_inclusion}`,
`milestones::{set_milestone_state, clear_milestone_state, set_muted_kinds}`, `projection::save_settings`,
plus an `app_config` allowlisted write through its service (`stock_import::formats`, `categorization::packs`).
An op that the service rejects (validation) is skipped; that is part of the test.

- [ ] **Step 1: Failing tests**

```rust
const SEEDS: [u64; 24] = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946, 17711, 28657, 46368, 75025];
#[test] fn random_operations_converge_under_any_relay_order()
// per seed: Vault::new(3, seed data); 6 rounds of { 1–8 random ops on random devices; random devices push; random devices pull in their own random order };
// final: everyone pushes, everyone pulls everything → assert_converged; failure message includes the seed.
#[test] fn adversarial_orders_converge()
// per seed: each device receives the other authors' envelopes in a worst-case interleaving (all of one author first, then the other; and reversed) and with
// duplicated entries (replays) → assert_converged.
#[test] fn the_harness_detects_a_divergence()
// sanity: write a synced column by SQL on one device with capture in Repair mode → assert_converged panics (#[should_panic]).
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::convergence` PASS (time budget: under 60 s in debug; reduce rounds, not seeds, if slower) → gates → commit `test(sync): three devices converge under random relay orders`.

---

### Task 19: Named convergence scenarios (§18, review findings)

**Files:**
- Create: `src-tauri/src/services/sync/convergence/scenarios.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 18 `Vault`.

Each test drives real services on specific devices, exchanges in **both** relevant orders, and ends
with `assert_converged()` plus the assertion named:

| Test | Scenario → extra assertion |
|---|---|
| `d13_tombstone_beats_offline_edit` | A deletes a bond, B edits its name offline → bond gone everywhere |
| `generation_readd_drops_edits_of_the_old_row` | A removes and re-adds a tag assignment, B edits the old one → one assignment, old edit gone |
| `delete_and_recreate_aapl_with_a_concurrent_child` | Task 13 worked example → AAPL present, B's transaction gone |
| `cascade_rule_child_added_offline` | A deletes a loan, B adds an event → no event anywhere |
| `set_null_rule_child_added_offline` | A deletes a spending plan, B tags a payment with it → payment untagged |
| `orphan_rule_category_deleted_while_categorising` | A deletes category K with reassign, B categorises a payment into K and learns a payee for K → payment uncategorised, learned payee dropped, A's reassigned rows on the target |
| `same_id_aapl_on_two_devices` | → one `stock_investments` row with the natural id, transactions of both |
| `keep_newest_balance_same_day` | → one record, the newer generation's amount |
| `rename_tags_spending_plans_and_categories` | → suffixes by id identical everywhere |
| `unlink_replacement_source` | → one link, lowest id keeps it |
| `split_applied_while_a_pre_split_buy_is_added_offline` | → the buy scaled once, basis lists the split |
| `split_applied_and_dismissed_concurrently` | → status by newer clock; units match the status on every device |
| `split_detected_on_two_devices_after_apply` | → stays `applied`, shares scaled once |
| `split_revert_reaches_every_device` | → `pending` and unscaled everywhere |
| `import_5000_rows_over_many_envelopes` | A imports 5,000 bank transactions; relay interleaves B's edits between A's envelopes → no stop, 5,000 rows on C |
| `barrier_requeue_resends_rows_pushed_below_the_barrier` | B pushes after the barrier marker landed; B migrates (simulated `schema_seen` < barrier) → B's rows re-sent and applied on A |
| `rejoin_after_simulated_rollback` | copy B's database (serialize/deserialize `Connection::backup` to memory) before two edits, push the edits, restore the copy, make one more edit, pull → `Clone` verdict, `rejoin`, every edit present everywhere |
| `clone_detection_from_an_own_id_envelope` | → `judge` returns `Clone` before any push |
| `forked_author_is_quarantined_only` | relay serves two different seq 3 of B → B quarantined on A, C's envelopes still applied |
| `migration_seed_revival_is_deleted_again` | A deletes a seeded category, B re-runs the seed SQL (simulated migration) then `on_open` → category gone on B |
| `local_keys_and_columns_never_travel` | A changes `language`, `theme`, `fx_reference`, MCP token → B unchanged |
| `rule_packs_written_explicitly_travel` | A writes `rule_packs_enabled` → B has it |

- [ ] **Step 1: Write the scenarios** (they fail until each is wired to the harness; any real failure is a merge/repair bug — fix it in the owning module with a unit test there first).
- [ ] **Steps 2–4:** `cargo test --lib services::sync::convergence::scenarios` PASS → gates.
- [ ] **Step 5: Commit** — `test(sync): named convergence scenarios from the spec review`.

---

### Task 20: ADR and standards

**Files:**
- Create: `docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md`
- Modify: `docs/architecture/decisions/README.md` (index row 0024, date 2026-11-06, Accepted)
- Modify: `docs/standards/rust-backend.md` (new "## 11. Rewrites relative to stored values name their sync behaviour")
- Modify: `docs/architecture/overview.md` only if it lists the services modules (add `services/sync/` one line)

ADR 0024 (Context / Decision / Consequences): D4 trigger capture (TEMP triggers from the registry,
installed only when enabled, modes local/merge/repair); D9 HLC 48/16 with device tie-break and the
24 h warning; D12–D15 as implemented, including the generation rule, weak `pending`, `"p"` and
`"del"`+`"ins"` (Spec corrections 9–10); D17 envelope choice (XChaCha20-Poly1305 + HKDF, no
signatures — decided now, built in phase 2). Known limits: concurrent rate changes leave the current
terms of the later wall-clock write; duplicate insurance policy numbers are kept; a device without FX
history keeps its other-asset average until it has one; legacy plain files of a property deleted on
another device stay on disk until phase 4.

rust-backend.md §11 text (exact): "A feature that rewrites rows relative to their stored values
(scaling, re-numbering, moving amounts between rows) must state its sync behaviour in its spec: either
plain last-writer-wins per column is correct, or it records what it applied in the row (like
`split_basis`) and adds a deterministic repair in `services/sync/repair/`. Repairs are pure functions
of the merged rows: never of arrival order, the device or the time; order every selection by primary
key."

- [ ] **Step 1:** write the ADR, the index row and the standards section.
- [ ] **Step 2:** `cd src-tauri && cargo test --lib` → PASS (Prettier's `format:check` covers only `src/`, so the Markdown needs no formatter run).
- [ ] **Step 3: Commit** — `docs(sync): ADR 0024 and the rewrite rule for future features`.

---

## Plan's calls (summary)

Natural-ID namespace and separator; no proptest (24 fixed seeds); `split_basis` canonical form,
basis-as-truth reconcile and `id`-ordered scaling; `reconcile_ticker` shared by apply/revert and the
repair; `sync_devices.id` as hex; `sync_heads.quarantined`, `sync_state.schema_seen`/`snapshot_hlc`;
migration classes for 001–029; group list; `updated_at` of four tables Local; `real_estate.photos`
Local; `loans` current terms plain Synced; `previous_*` Synced + chain repair; newer generation wins
the whole row; `"p"`, `"del"`+`"ins"`, `"barrier"` wire fields; weak-`pending` status rule; live rows
by FK depth then tombstones deepest first; full row when `'*'` is newer than the own-log watermark;
journal refuses >180 KiB rows (not capture); recalculation fallback; repair deletes record local
tombstones; ID tie-breaks in mirrors; export denies `sync_*` by prefix; REPLACE lint covers every
table; snapshot records held in memory in phase 1; override upserts moved to a service; projection
settings ignore client ids for new rows; trigger clocks via `sync_hlc_tick()` + `sync_hlc_now()`;
mode in a TEMP table.
