# Device Sync Phase 1 (Engine Without Network) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the local half of device sync (spec §17 phase 1): registry, migration 029, natural IDs,
trigger capture, journal, merge, repairs and snapshot records, proven by three in-memory devices that
converge under any relay order — with no network, no crypto and nothing visible to users.

**Architecture:** A new `services::sync` module that only takes `&Connection` or plain data. TEMP
triggers generated from a registry plus `PRAGMA table_info` capture changes into `sync_clock` /
`sync_outbox` (installed only when `sync_state.enabled = 1`). The journal turns the outbox into
plaintext `Changeset` values (§9.1 JSON) that always carry full rows; the merge applies them by
delete epoch, generation and per-column clocks, then runs deterministic repairs through the services'
own recalculation code. Phase 2 wraps the journal output and the snapshot record stream in envelopes
and talks to the relay.

**Tech Stack:** Rust 2021, rusqlite 0.40 (bundled SQLCipher; `functions` feature, added in Task 1),
uuid 1 (`v5` added), serde/serde_json, flate2 (deflate size estimate), base64 0.23 (base64url on the
wire), sha2, rand 0.10 (seeded `StdRng` in tests).

**Spec:** `docs/specs/2026-11-06-device-sync-client-design.md` (read all of it, §16a included).
Pre-flight review and rulings: `.superpowers/sdd/2026-11-06-device-sync-phase-1-engine/preflight.md`,
`preflight-rulings.md` (R1–R19); this revision implements them.

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
   it came from an insert (locally: no explicit `status` column clock; on the wire: its clock equals
   the row's `g`); weak loses to any other status; otherwise the newer clock wins. A weak value never
   gets an explicit column clock.
10. **Generations need a delete epoch (§7.1, §7.3), R2 adjusted.** A key's *epoch* is its newest
    tombstone clock `X`. A full row whose generation is older than `X` is dropped (an edit of a
    deleted row). Full rows of the current epoch merge per column whichever generation is newer —
    two independent creations of the same natural-ID row are the same row (§7.3's "ordinary
    per-column LWW"), and the stored generation becomes the larger one. A row that was deleted and
    re-created carries both `"del"` and `"ins"`, so receivers end the old epoch (cascading its
    children) before the new insert. Two additive wire fields follow: `"p"` (the generation of each
    referenced parent, per FK column carried) and `"barrier"` (omitted when 0). Worked examples in
    Task 13.
11. **Recalculations that write synced or recomputed columns break ties by `rowid` or not at all**,
    and rowids differ per device once rows arrive by merge: `valuations::latest_valuation` and
    `OTHER_ASSET_PRICES`, `loan_events::refresh_anchor_mirror` and `list_events`,
    `loan_amortization::ordered_events`, `crypto_investments::recalculate_crypto_metrics`. Task 8
    makes `id` the last tie-breaker there. Out of scope: the read paths of `cost_basis.rs`,
    `dated_facts.rs` and the other `valuations.rs` queries (`value_at_day`, `list_valuations`), which
    feed history and metrics tables that are Seed/local, never a synced column.
12. **Recalculations can fail** (`validation.sellExceedsHoldings` when two devices sold the same
    shares; `validation.exchangeRateUnavailable` for an other asset without FX history). §7.1
    assumes they always run; the repair falls back deterministically instead of aborting the merge.
13. **Build facts:** rusqlite lacked the `functions` feature and `db::migrations` is private (both
    handled in Task 1); uuid lacks `v5` (Task 6).
14. `sync_pending_blobs` is not created (attachments wait, §16a). Migration 029 instead adds the
    Local `sync_renamed` table the rename repair needs (correction 17).
15. **Keep-newest compares the clock that put a row on its key (R10)**, not "the newer clock" of the
    whole row: `max(generation, clocks of the key columns)`, ties by device then primary key. An edit
    of another column does not make a row win its key.
16. **The packed HLC fits SQLite's signed INTEGER only while the ms part is below 2^47**
    (6429-10-17T02:45:55Z). §6.3's 48-bit field stays, but `HlcState::next` saturates the ms part at
    `2^47 − 1` and the changeset decoder rejects a clock above `i64::MAX` (R5, adjusted: the ruled
    year-9999 assertion is false, see Task 5).
17. **Renames need the typed name kept (§7.3 "Rename").** A repair-assigned suffix is not clocked,
    so recomputing it from stored names diverges once a later rename frees a name. The plan keeps the
    typed name of every repair-renamed row in the Local table `sync_renamed`, sends the typed name on
    the wire, and recomputes a whole scope's names from typed names with one pure function (R9).
18. **Divergences the plan chooses (R19):** an oversized row is refused by the journal, not "at
    capture time" (§16a; capture cannot measure deflated size); `"prev"` is omitted at `seq == 1`;
    capture uses three connection functions (`sync_hlc_tick`, `sync_hlc_now`, `sync_hlc_observe`)
    instead of one `sync_hlc()`, and four triggers per table (insert, update, primary-key change,
    delete) instead of three; §18's capture overhead target is measured by an `#[ignore]` test in Task
    19, the 50,000-row iPhone merge target waits for the phone phase.
19. **The journal always sends full rows (R1).** §6.4's "five edits send one value" still holds
    (one outbox entry, current values); a row never travels as a partial update, so the merge has no
    "partial update to a missing row" case.

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-sync` on branch
  `feat/device-sync`; run `git branch --show-current` before every commit. Never touch
  `/Users/filipkral/Programování/FinanceApp/Moony-tauri`.
- No network, no crypto, no Tauri command, no UI, no new user-facing string in phase 1.
- G5: a profile without `sync_state.enabled = 1` has zero TEMP triggers, no `sync_clock` /
  `sync_outbox` / `sync_renamed` writes, no extra work at unlock beyond one `SELECT`.
- Errors are `AppError` (rule 4); no `.unwrap()` outside `#[cfg(test)]` (rule 5) — including the
  `Mutex<HlcState>` inside connection functions (a poisoned lock becomes a SQLite function error);
  migration 029 is append-only (rule 6); money TEXT, timestamps INTEGER unix seconds, PKs TEXT
  (rule 10).
- HLC: 48-bit field of wall-clock milliseconds + 16-bit counter, `hlc = (ms << 16) | counter`,
  stored as `i64` (ms saturated at `2^47 − 1`, correction 16), compared as `(hlc, device)`; device
  = 16 bytes compared as bytes. A row that never had a clock reads as clock 0 with
  `DeviceId::ZERO` (16 zero bytes), which takes an index in `devs` like any device (plan's call).
- Future-clock warning threshold: a remote clock more than `86_400_000` ms ahead of local time.
- Envelope budget: deflated changeset JSON, zero-padded to a multiple of 1,024 bytes, at most
  `61_440` bytes (60 KiB); a single row alone may take up to `184_320` bytes (180 KiB); a row larger
  than that is refused and stays in the outbox.
- Natural-ID namespace UUID: `b7a0c3e2-5d1f-4c8e-9a6b-2f3e1d0c9b8a`; name =
  `table` + `\u{1f}` + key parts joined by `\u{1f}`.
- Rename suffix: `" (2)"`, `" (3)"`, …; the lowest primary key of a name group keeps the typed name.
- `app_config` synced keys: exactly `stock_import_formats`, `learned_csv_formats`,
  `milestones.mutedKinds`, `rule_packs_enabled`, `rule_packs_disabled_rules`.
- Changeset `"v": 1`; schema number = `known_migration_names().len()` = 29 after this plan.
- Rust tests use `Connection::open_in_memory()` plus the real migrations through
  `services::sync::test_support::migrated()` (never the `Database` struct, except the one unlock test
  in Task 9 that follows the existing `db::tests` pattern with `tempfile`).
- Dependencies: `uuid` feature `v5`, `flate2 = "1"`, `base64 = "0.23"` (already in `Cargo.lock`);
  rusqlite `functions` is already in (Task 1). No other crate or feature — in particular no rusqlite
  `backup`/`serialize` (Task 19 copies databases with `VACUUM INTO`, R6). **No `proptest`**
  (plan's call): seeded `rand::rngs::StdRng` from `rand` 0.10 (`rand::SeedableRng`,
  `rand::RngExt::random_range`, `rand::seq::SliceRandom::shuffle`) with a fixed seed list, so a
  failure names its seed.
- Files stay focused: no file in `services/sync/` planned over ~800 lines (the file map below is
  already split, R16). `db/migrations.rs` is existing and exempt.
- Every task ends with, from `src-tauri/`:
  `cargo test --lib <task filter>`, `cargo test --lib` (whole crate),
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
  Hooks: `.husky/pre-commit` runs `npx lint-staged`, `npm run typecheck` and `cargo fmt --check`;
  `.husky/pre-push` runs `npm test`, `cargo clippy -- -D warnings` and `cargo test`. Never bypass
  them with `--no-verify`.
- New service functions other tasks call are `pub` (a `pub(crate)` function unused until a later
  task fails `clippy -D warnings` on dead code).
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (R18).

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
   repair — Task 11 `full_rows_carry_typed_values_and_derived_values_alike` and Task 12
   `typed_values_without_source_rows_survive_repair`.
5. **A recalculation that fails on one device only** (no FX history for a foreign-currency other
   asset purchase): the merge must still commit, keep the stored value and warn — Task 12
   `failing_recalculation_keeps_the_value_and_warns`.

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/mod.rs` | Module root, `pub mod` list, `on_open()` (exists since Task 1) |
| `services/sync/probe.rs` (`#[cfg(test)]`) | Task 1 SQLite facts + the REPLACE lint (lint fixed in Task 2) |
| `services/sync/test_support.rs` (`#[cfg(test)]`) | `migrated()` connection, small seeding helpers |
| `services/sync/registry/mod.rs` | Types (`TableSpec`, classes) and lookups |
| `services/sync/registry/tables.rs` | The static table/column data |
| `services/sync/registry/schema.rs` | `PRAGMA table_info` / primary-key readers |
| `services/sync/registry/keys.rs` | Unique-key rules, FK graph and depth, migration classes |
| `services/sync/hlc.rs` | `DeviceId`, `Clock`, packing, `HlcState` |
| `services/sync/natural_id.rs` | UUIDv5 IDs (§7.3) |
| `services/sync/state.rs` | `sync_state` row access, `enable` |
| `services/sync/capture/mod.rs` | Install/uninstall, connection functions, mode |
| `services/sync/capture/triggers.rs` | Trigger SQL generation |
| `services/sync/changeset.rs` | Plaintext changeset types, row and changeset JSON (§9.1) |
| `services/sync/journal/mod.rs` | `build_next`, ordering, `record_sealed` |
| `services/sync/journal/read.rs` | `read_row` (full row + clocks), shared with snapshots |
| `services/sync/journal/cut.rs` | Size measurement and envelope cutting |
| `services/sync/repair/{mod,mirrors,invariants,renames}.rs` | §7.1 recalculations, §7.2/§7.4 repairs, the rename function, seed revival |
| `services/sync/merge/mod.rs` | `apply_changeset`, transaction, heads, reports |
| `services/sync/merge/row.rs` | Epoch / generation steps of one row |
| `services/sync/merge/columns.rs` | Column LWW, groups, weak `pending`, write-once |
| `services/sync/merge/parents.rs` | FK parent checks and orphan rules |
| `services/sync/merge/unique.rs` | Unique-key outcomes |
| `services/sync/chain.rs` | §9.3 verdicts, re-join and barrier re-queue |
| `services/sync/snapshot.rs` | §8 record stream, dump, join, restore by replace / merge |
| `services/sync/convergence/mod.rs` (`#[cfg(test)]`) | `Vault` harness, property tests |
| `services/sync/convergence/ops/{mod,bank,stocks,assets,planning}.rs` (`#[cfg(test)]`) | Random operations through the services |
| `services/sync/convergence/scenarios.rs` (`#[cfg(test)]`) | Named scenarios, perf measurement |
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

### Task 2: Migration 029 — sync bookkeeping, `sync_devices`, `split_basis`; REPLACE lint fix

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (`MIGRATION_029`, Vec entry `("029_device_sync", MIGRATION_029)`, `mod migration_029` tests)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `src-tauri/src/services/sync/probe.rs` (`no_source_file_uses_insert_or_replace`, R14)
- Modify: `docs/architecture/database.md` (029 bullet under "How Migrations Work"; catalog rows: new "### Device sync" section with the eight tables; amend the `investment_transactions`, `stock_price_overrides`, `watched_stocks`, `dividend_overrides` rows with `split_basis`)

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
CREATE TABLE sync_renamed (tbl TEXT NOT NULL, pk TEXT NOT NULL, typed TEXT NOT NULL,
    assigned TEXT NOT NULL, PRIMARY KEY (tbl, pk)) WITHOUT ROWID;
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
or uploaded; `sync_renamed` keeps the typed name of a repair-renamed row and the name the repair
assigned (correction 17). **`split_basis` canonical form:** a JSON array of split IDs sorted ascending
by ID, NULL when empty. Backfill (the D27 meaning on today's data): every `investment_transactions` row
gets the IDs of the `applied` splits of its ticker with `effective_day > transaction_date`; every row
of the three side tables gets the IDs of the `applied` splits of its ticker with
`effective_day > updated_at` (exactly the rows `rewrite_pre_split_rows` scaled). Use
`(SELECT json_group_array(id) FROM (SELECT s.id … ORDER BY s.id))` and leave NULL where no split
matches.

**REPLACE lint (R14, plan's call on the cut):** Task 1's lint cuts each file at its first
`#[cfg(test)]`, which in `db/migrations.rs` is `#[cfg(test)] mod dry_run;` at line 28, so no migration
SQL is scanned. Cutting at the first `#[cfg(test)] mod tests` instead would still miss
`MIGRATION_007`–`029`, which sit after other test modules (`mod stock_import_batch_tests`,
`mod migration_005`, …). The lint therefore removes every brace-matched
`#[cfg(test)]` + `mod <name> { … }` block (a body-less `#[cfg(test)] mod x;` removes only that line)
and scans the rest of every file, `db/migrations.rs` included. `src/bin/` stays exempt (it holds
`seed_demo.rs`, which does use `INSERT OR REPLACE`; it never runs on a sync-captured connection),
and so does `probe.rs` itself.

- [ ] **Step 1: Write the failing tests** in `mod migration_029` (pattern of `mod migration_028`) and the lint helper test in `probe.rs`

```rust
#[test] fn the_sync_tables_exist_and_start_empty()
// run_chain(all_migrations()); each of the eight tables exists; COUNT(*) = 0 in each.
#[test] fn sync_state_is_a_single_row()
// INSERT id=1 ok; INSERT id=2 → Err (CHECK).
#[test] fn split_basis_backfill_lists_applied_splits_after_the_row()
// database before 029 (filter names < "029_device_sync", as stock_splits_tests does): positions AAPL and MSFT;
// AAPL transactions on day 10 and day 30; AAPL splits 'b' applied day 20, 'a' applied day 25, 'c' pending day 26,
// 'd' dismissed day 27; MSFT split 'e' applied day 25 (another ticker, same day is fine under UNIQUE(ticker, effective_day));
// AAPL stock_price_overrides updated_at 5, AAPL watched_stocks updated_at 40 → after run_migrations:
assert_eq!(basis("investment_transactions","t-day10"), Some(r#"["a","b"]"#.into()));
assert_eq!(basis("investment_transactions","t-day30"), None);
assert_eq!(basis("stock_price_overrides","o1"), Some(r#"["a","b"]"#.into()));
assert_eq!(basis("watched_stocks","w1"), None);
#[test] fn the_chain_now_has_29_migrations()
assert_eq!(known_migration_names().len(), 29);
assert_eq!(known_migration_names().last(), Some(&"029_device_sync"));
// probe.rs
#[test] fn the_lint_strips_test_modules_but_keeps_later_production_code()
// production_text("const A: &str = \"x\";\n#[cfg(test)]\nmod dry_run;\n#[cfg(test)]\nmod t { fn f() { \"REPLACE INTO a\"; } }\nconst B: &str = \"INSERT OR REPLACE INTO b\";")
// → contains "const A", contains "INSERT OR REPLACE INTO b", does not contain "REPLACE INTO a".
```

- [ ] **Step 2: Run** — `cargo test --lib db::migrations::migration_029 services::sync::probe` → FAIL (no 029; no `production_text`).
- [ ] **Step 3: Implement `MIGRATION_029`** with the DDL above plus the backfill `UPDATE`s, doc comment in the style of 026–028; implement `fn production_text(text: &str) -> String` in `probe.rs` and use it in `no_source_file_uses_insert_or_replace` (its comment no longer claims "none today": `src/bin` has one and is exempt).
- [ ] **Step 4: Regenerate the snapshot** — `cargo test regenerate_schema_snapshot -- --ignored`; read the diff: only the eight tables, two indexes and four `split_basis` columns.
- [ ] **Step 5: Run** — `cargo test --lib db:: services::sync::probe` → PASS (including `baseline_matches_schema_snapshot`, `fresh_database_runs_the_chain_and_is_idempotent` and the lint over all migration SQL).
- [ ] **Step 6: Docs** — `database.md` as listed under Files.
- [ ] **Step 7: Gates + commit** — `feat(db): migration 029 adds sync bookkeeping and split_basis`.

---

### Task 3: Registry — table classes and columns

**Files:**
- Create: `src-tauri/src/services/sync/registry/mod.rs` (types and lookups), `registry/tables.rs` (the static data), `registry/schema.rs` (PRAGMA readers)
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
pub fn synced_tables() -> impl Iterator<Item = &'static TableSpec>;
// schema.rs
pub struct ColumnInfo { pub name: String, pub decl_type: String, pub not_null: bool, pub has_default: bool, pub pk_position: u32 }
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>>;   // PRAGMA table_info
pub fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>>;
```

Classes (§5.1 with §16a): **Synced** = the spec's list minus `real_estate_photo_batches`,
`real_estate_photos`, `real_estate_documents`, `insurance_documents`, plus `sync_devices`;
**Seed** = the ten of §5.1; **Derived** = `budget_goals`; **Local** = `_migrations`,
`attachment_leftovers`, `loan_legacy_anchors`, `mcp_pending_refresh`, the four attachment tables,
`sync_state`, `sync_clock`, `sync_outbox`, `sync_heads`, `sync_own_log`, `sync_unsent`,
`sync_renamed`. Names starting with `sqlite_` are ignored.

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

Recomputed and recomputed-or-typed columns travel in every full row (R1) and are overwritten by the
receiver's repair when it has source rows; only Local columns never travel, so only **Local**
`NOT NULL` columns need a `DEFAULT` for a merged insert.

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
#[test] fn every_local_not_null_column_of_a_synced_table_has_a_default() // a merged insert omits Local columns
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
- [ ] **Step 3: Implement** the three files (`tables.rs` holds only the `static` slice of `TableSpec` and the allowlist).
- [ ] **Step 4: Run** → PASS. **Step 5: Gates + commit** — `feat(sync): registry of table and column classes`.

---

### Task 4: Registry — unique keys, foreign keys, migration classes

**Files:**
- Create: `src-tauri/src/services/sync/registry/keys.rs` (`pub mod keys;` in `registry/mod.rs`)
- Modify: `src-tauri/src/db/migrations.rs` (`pub fn migration_sql(name: &str) -> Option<&'static str>`)
- Modify: `src-tauri/src/db/mod.rs:8` (add `migration_sql` to the `pub use migrations::{…}` list, R7)

**Interfaces:**
- Consumes: Task 3 `tables()`, `spec()`.
- Produces:

```rust
// db
pub fn migration_sql(name: &str) -> Option<&'static str>;          // re-exported as crate::db::migration_sql
// keys.rs
pub enum UniqueRule { SameId, KeepNewest, Rename, Unlink }
pub struct UniqueSpec { pub table: &'static str, pub exprs: &'static [&'static str],
    pub partial: Option<&'static str>, pub key_columns: &'static [&'static str], pub rule: UniqueRule }
pub fn unique_rules() -> &'static [UniqueSpec];
pub enum OrphanRule { SetNull, DropChild }
pub enum OnDelete { Cascade, SetNull, NoAction(OrphanRule) }
pub struct ForeignKey { pub child: String, pub column: String, pub parent: String, pub parent_column: String, pub on_delete: OnDelete }
pub const ORPHAN_RULES: &[(&str, &str, OrphanRule)];                  // (child table, column, rule)
pub fn foreign_keys(conn: &Connection) -> Result<Vec<ForeignKey>>;    // = foreign_keys_with(conn, ORPHAN_RULES)
pub fn foreign_keys_with(conn: &Connection, orphan_rules: &[(&str, &str, OrphanRule)]) -> Result<Vec<ForeignKey>>;
// PRAGMA foreign_key_list, Synced child AND Synced parent only; a NO ACTION key missing from `orphan_rules` → Err naming it
pub fn fk_depth(conn: &Connection) -> Result<BTreeMap<String, u32>>; // 0 = no Synced parent; self-references ignored
pub enum MigrationClass { Additive, Barrier }
pub const MIGRATION_CLASSES: &[(&str, MigrationClass)];
pub fn schema_number() -> u32;     // known_migration_names().len()
pub fn barrier_number() -> u32;    // 1-based position of the newest Barrier migration
pub fn uses_random_ids(sql: &str) -> bool;  // case-insensitive "randomblob(" or "uuid(" outside SQL comments
```

Unique rules (`exprs` is the index's column/expression list; `key_columns` the plain columns those
expressions read, used by keep-newest's key clock): **SameId** — `stock_investments(ticker)`,
`crypto_investments(ticker)`, `watched_stocks(ticker)`, `stock_price_overrides(ticker)`,
`dividend_overrides(ticker)`, `crypto_price_overrides(symbol)`, `stock_splits(ticker, effective_day)`,
`projection_settings(asset_type)`, `bank_transactions(bank_account_id, transaction_id)`,
`idx_learned_payees_scoped` (`COALESCE(normalized_payee,'')`, `COALESCE(counterparty_iban,'')`,
`bank_account_id`, `is_credit`, partial `bank_account_id IS NOT NULL`); **KeepNewest** —
`bank_balance_records(bank_account_id, as_of)`,
`budget_goal_versions(category_id, timeframe, effective_month)`; **Rename** — `stock_tags(name)`,
`stock_tag_groups(name)`; **Unlink** — `cashflow_replacement_source` (`replacement_source_id`,
partial `replacement_source_id IS NOT NULL`). The rename scopes and case folding live in
`repair/renames.rs` (Task 12), not here; service-enforced uniqueness (no index) lives there too.

`ORPHAN_RULES` (Spec correction 1): `bank_transactions.category_id`,
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
// PRAGMA index_list(t) for Synced t, unique=1, origin != 'pk'; match by index_info column names (or, for the
// expression index, by name idx_learned_payees_scoped) → message names table and columns without a rule.
#[test] fn every_rule_names_an_existing_unique_index()
#[test] fn the_fk_graph_comes_from_pragma_with_its_on_delete_rules()
assert!(fks.iter().any(|f| f.child=="bank_transactions" && f.column=="spending_plan_id" && matches!(f.on_delete, OnDelete::SetNull)));
assert!(fks.iter().any(|f| f.child=="investment_transactions" && f.column=="investment_id" && matches!(f.on_delete, OnDelete::Cascade)));
#[test] fn a_no_action_key_without_an_orphan_rule_is_an_error()
let err = foreign_keys_with(&conn, &ORPHAN_RULES[1..]).unwrap_err().to_string();
assert!(err.contains(ORPHAN_RULES[0].0) && err.contains(ORPHAN_RULES[0].1));
assert!(foreign_keys(&conn).is_ok());
#[test] fn a_required_column_cannot_be_set_null()      // DropChild exactly where the child column is NOT NULL
#[test] fn depth_orders_every_parent_before_its_children()
let d = fk_depth(&conn)?; assert_eq!(d["bank_transactions"], 3); assert_eq!(d["csv_import_batches"], 2);
for fk in foreign_keys(&conn)? { if fk.child != fk.parent { assert!(d[&fk.parent] < d[&fk.child]) } }
#[test] fn every_migration_has_a_class_and_no_class_is_stale()
assert_eq!(MIGRATION_CLASSES.iter().map(|m| m.0).collect::<Vec<_>>(), crate::db::known_migration_names());
#[test] fn the_random_id_scanner_flags_random_ids_and_ignores_comments()
assert!(uses_random_ids("INSERT INTO t (id) SELECT lower(hex(randomblob(16)));"));
assert!(!uses_random_ids("-- randomblob( is forbidden\nINSERT INTO t (id) VALUES ('seed-1');"));
assert!(uses_random_ids(crate::db::migration_sql("002_real_estate_purchase_date").unwrap())); // 002 did use random IDs (§14)
#[test] fn migrations_after_029_insert_synced_rows_only_with_deterministic_ids()
for name in crate::db::known_migration_names().into_iter().filter(|n| *n > "029_device_sync") {
    assert!(!uses_random_ids(crate::db::migration_sql(name).unwrap()), "{name}") }
assert!(crate::db::migration_sql("029_device_sync").is_some());
#[test] fn schema_number_is_29_and_the_newest_barrier_is_025()
assert_eq!(schema_number(), 29); assert_eq!(barrier_number(), 25);
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::registry::keys` → FAIL. **Step 3: Implement.** **Step 4:** → PASS (`cargo test --lib db::` stays green).
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
impl Clock { pub const ZERO: Self; }                  // hlc 0, DeviceId::ZERO: the clock of a row that never had one
pub const FUTURE_WARNING_MS: u64 = 86_400_000;
pub const MAX_MS: u64 = (1 << 47) - 1;                // correction 16
pub fn pack(ms: u64, counter: u16) -> u64;  pub fn unpack(hlc: u64) -> (u64, u16);
pub struct HlcState { last: u64 }
pub enum Observation { Ok, FarFuture { ahead_ms: u64 } }
impl HlcState { pub fn new(last: u64) -> Self; pub fn next(&mut self, now_ms: u64) -> u64; pub fn observe(&mut self, remote: u64, now_ms: u64) -> Observation; pub fn last(&self) -> u64; }
pub fn now_ms() -> u64;
```

`next`: `ms = min(MAX_MS, max(now_ms, unpack(last).0))`; counter 0 when `ms` moved, else
`counter + 1`; at `u16::MAX` the ms part advances by one (still capped) and the counter restarts at 0
(plan's call; at the cap the counter saturates instead and a warning is logged). `observe` sets
`last = max(last, remote)` and reports `FarFuture` when `unpack(remote).0 > now_ms + FUTURE_WARNING_MS`.

R5 pushback: the ruled assertion "`pack(ms of 9999-12-31T23:59:59.999Z, 65535)` is positive as i64"
is false — that ms value is 253,402,300,799,999 (> 2^47), so the packed value exceeds 2^63 and is
negative as `i64`. The closest safe test pins the real limit instead.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn packing_is_48_bits_of_ms_and_16_of_counter()
assert_eq!(pack(1_777_000_000_000, 123), 116_457_472_000_000_123);
assert_eq!(unpack(116_457_472_000_000_123), (1_777_000_000_000, 123));
#[test] fn every_clock_the_device_issues_is_a_positive_sqlite_integer()
assert!((pack(MAX_MS, u16::MAX) as i64) > 0);                         // 6429-10-17T02:45:55.327Z
assert!((pack(MAX_MS, u16::MAX) as i64) > (pack(1_777_000_000_000, 0) as i64));
assert!((pack(MAX_MS + 1, 0) as i64) < 0);                            // why the cap exists
#[test] fn next_never_passes_the_ms_cap()
let mut s = HlcState::new(0); assert_eq!(unpack(s.next(u64::MAX >> 16)).0, MAX_MS);
#[test] fn next_is_strictly_increasing_even_when_the_wall_clock_goes_back()
#[test] fn the_counter_overflow_moves_the_ms_part()
let mut s = HlcState::new(pack(5, u16::MAX)); assert_eq!(s.next(5), pack(6, 0));
#[test] fn observe_moves_the_clock_to_the_highest_seen()
#[test] fn a_clock_more_than_24_hours_ahead_is_reported()
let now = 1_000_000_000; let mut s = HlcState::new(0);
assert!(matches!(s.observe(pack(now + 86_400_000, 0), now), Observation::Ok));
assert!(matches!(s.observe(pack(now + 86_400_001, 0), now), Observation::FarFuture { ahead_ms: 86_400_001 }));
#[test] fn equal_hlcs_are_ordered_by_device_bytes()
#[test] fn the_zero_clock_is_below_every_issued_clock()
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
else `Uuid::new_v4()`; `learned_payees` [the stored `normalized_payee` or "", the stored
`counterparty_iban` or "", `bank_account_id`, `"1"`/`"0"` for `is_credit`] — the same tuple the
scoped unique index reads. `learn_scoped_and_apply` keeps its DELETE + INSERT, now of the same natural
id in one transaction: exactly the delete-and-re-create case Task 11 sends as `"del"` + `"ins"`.

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
  `INSERT OR REPLACE` anywhere outside `src/bin`)

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
`dividend_overrides`, ordered by id: (1) **remove** — every split in the row's basis whose status is
not `applied`, newest `effective_day` first (ties by id descending): scale back (the existing
`scale_amount` with `(d, n)` and the existing roundings) and drop it from the basis; (2) **add** —
every `applied` split not in the basis that *affects* the row, oldest first (ties by id ascending):
scale by `(n, d)` and add it. A transaction is affected when `transaction_date < effective_day`; a
side row when `updated_at < effective_day`. The basis is the source of truth: an applied split already
in the basis is never removed because the row no longer looks affected. Then drop the ticker's
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
- Modify: `services/loan_amortization.rs:323, 387-391` (`LedgerEvent` gains `pub id: String` — it has none today — filled by every constructor; `ordered_events` sorts by `(day, created_at, id)`)
- Modify: `services/crypto_investments.rs:29-32` (`ORDER BY transaction_date, created_at, id`)
- Modify: `services/budgeting.rs` (`pub fn merge_equal_neighbours`; extract `pub fn rebuild_goal_row(conn, category_id, timeframe, now) -> Result<()>` from the `displayed_version` block of `delete_goal_intervals` and the mirror step of `upsert_goal_at`, both now call it)

The tie-breaker change is limited to these recalculations (correction 11); `cost_basis.rs`,
`dated_facts.rs` and the history-only `valuations.rs` readers stay as they are.

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
- Create: `src-tauri/src/services/sync/state.rs`, `services/sync/capture/mod.rs`, `services/sync/capture/triggers.rs`
- Modify: `services/sync/mod.rs` (`pub fn on_open(conn: &Connection)`)
- Modify: `src-tauri/src/db/mod.rs:91` (in `open_with_key`, call `crate::services::sync::on_open(&conn)` right after `attach_attachments_store(&conn, &path)`, before `load_rates_from_db` and the two self-heals; `create_with_key` is untouched — a new profile never has sync)
- Modify: `src-tauri/src/services/categorization/packs.rs` (new `pub fn persist_effective(conn) -> AppResult<()>`: writes `rule_packs_enabled` and `rule_packs_disabled_rules` from `read_config`, so the language default stops deciding once sync is on)

**Interfaces:**
- Consumes: registry (Tasks 3–4), `hlc` (Task 5).
- Produces:

```rust
// state.rs
pub struct SyncState { pub enabled: bool, pub device: DeviceId, pub next_seq: u64, pub last_hash: Option<[u8; 32]>,
                       pub cursor: u64, pub snapshot_hlc: u64, pub barrier: u32, pub schema_seen: u32 }
pub fn load(conn: &Connection) -> Result<Option<SyncState>>;
pub fn ensure_row(conn: &Connection, device: DeviceId) -> Result<()>;
// INSERT the row (enabled=1, device_id, schema_seen=schema_number(), barrier=barrier_number()) when missing;
// when present: enabled=1, device_id=device; never touches cursor, snapshot_*, next_seq, last_hash (E3)
pub fn enable(conn: &Connection, device: DeviceId) -> Result<()>;
// first device of a vault (R4 order): ensure_row → capture::install(conn, device) → packs::persist_effective
// (captured: the two keys are clocked and queued)
pub fn disable(conn: &Connection) -> Result<()>;                       // §11.6 local part: uninstall, delete sync_state/clock/outbox/heads/own_log/unsent/renamed
pub fn record_error(conn: &Connection, message: &str) -> Result<()>;  // last_error, last_error_at = unixepoch()
// capture/mod.rs
pub enum Mode { Local, Merge, Repair }
pub fn install(conn: &Connection, device: DeviceId) -> Result<()>;
pub fn uninstall(conn: &Connection) -> Result<()>;
pub fn is_installed(conn: &Connection) -> Result<bool>;
pub fn mode(conn: &Connection) -> Result<Mode>;
pub fn with_mode<T>(conn: &Connection, mode: Mode, f: impl FnOnce() -> Result<T>) -> Result<T>; // restores the previous mode on Ok and Err; no-op wrapper when not installed
pub fn set_cascade_clock(conn: &Connection, clock: Clock) -> Result<()>;
pub fn observe(conn: &Connection, hlc: u64) -> Result<Observation>;   // SELECT sync_hlc_observe(?1)
pub fn tick(conn: &Connection) -> Result<u64>;                         // SELECT sync_hlc_tick() (tests)
// capture/triggers.rs
pub fn trigger_sql(conn: &Connection, device: DeviceId) -> Result<Vec<String>>;
pub fn pk_expr(spec: &TableSpec, pk: &[String], row: &str) -> String; // "NEW.id" | "json_array(NEW.a, NEW.b)" | "'profile'"
// mod.rs
pub fn on_open(conn: &Connection);    // never fails: logs and record_error
```

Mechanics (plan's calls):
- `install` registers three functions with `FunctionFlags::SQLITE_UTF8` sharing one
  `Arc<Mutex<HlcState>>` initialised from `SELECT COALESCE(MAX(hlc), 0) FROM sync_clock`:
  `sync_hlc_tick()` (advance and return), `sync_hlc_now()` (last issued), `sync_hlc_observe(x)`
  (returns 1 when the observation was `FarFuture`). A poisoned mutex returns
  `rusqlite::Error::UserFunctionError`, never panics. It creates
  `CREATE TEMP TABLE sync_mode (id INTEGER PRIMARY KEY CHECK (id = 1), mode TEXT NOT NULL, cascade_hlc INTEGER, cascade_device BLOB)`
  with `('local')`, then executes `trigger_sql`. The device ID is baked into the SQL as `x'…'`.
  Because the HLC state starts at the stored maximum, `install` runs **after** any snapshot restore.
- Per Synced table (from `registry` + `schema::columns()`), triggers `sync_<t>_ins`, `sync_<t>_upd`,
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
    writes the clock of **every** member; outbox as above. A `stock_splits.status` update always gets
    its own column clock (that is what makes a revert strong, correction 9).
  - **PK change** (`AFTER UPDATE OF <pk cols> WHEN OLD.pk IS NOT NEW.pk`, mode `local`): tombstone
    the old key and record `'*'` for the new key, both queued.
  - **DELETE**: mode `local` → tick, upsert `'-'` with `(sync_hlc_now(), device)`, delete the key's
    column clocks (keep `'*'`), queue. Mode `merge` → upsert `'-'` with the `cascade_hlc` /
    `cascade_device` from `sync_mode`, delete column clocks, no queue. Mode `repair` → nothing.
  - Every clock upsert keeps the maximum: `ON CONFLICT(tbl, pk, col) DO UPDATE SET hlc = excluded.hlc,
    device = excluded.device WHERE (excluded.hlc, excluded.device) > (sync_clock.hlc, sync_clock.device)`.
    The same SQL text is exported as `pub const CLOCK_UPSERT: &str` and reused by
    `repair::tombstone_local` (one copy of the rule).
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
#[test] fn the_composite_key_text_is_the_json_array_sqlite_writes()
// stock_investment_tags insert → sync_clock.pk == r#"["inv-1","tag-1"]"# (Task 10 pins RowKey::to_pk_text to the same bytes)
#[test] fn merge_mode_records_cascaded_tombstones_with_the_cascade_clock_and_queues_nothing()
#[test] fn repair_mode_records_nothing()
#[test] fn with_mode_restores_the_previous_mode_after_an_error()
#[test] fn the_hlc_starts_above_every_stored_clock()
#[test] fn disable_removes_triggers_and_every_sync_row()
#[test] fn enabling_sync_writes_the_effective_rule_packs_and_queues_them()
// profile language 'cs', no rule_packs_enabled key → enable → app_config has rule_packs_enabled == the Czech default;
// sync_outbox holds ('app_config','rule_packs_enabled') and sync_clock its '*'.
#[test] fn ensure_row_never_resets_the_cursor_or_snapshot_columns()
// db::tests
#[test] fn opening_a_sync_enabled_profile_installs_capture_before_the_self_heals()
// create_with_key; a position with one transaction and a stale quantity '99' written by SQL; enable(device); close; open_with_key →
// capture::is_installed == true; the self-heal corrected the quantity; sync_clock has no 'quantity' clock for that position
// (recomputed while it has source rows) and the outbox holds no new entry for it.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::capture services::sync::state db::tests` PASS → gates → commit `feat(sync): TEMP-trigger capture installed only for sync-enabled profiles`.

---

### Task 10: Changeset wire format (§9.1)

**Files:**
- Modify: `src-tauri/Cargo.toml` (`base64 = "0.23"`, `flate2 = "1"`)
- Create: `src-tauri/src/services/sync/changeset.rs`

**Interfaces:**
- Consumes: `DeviceId`, `Clock` (Task 5), `TableSpec` (Task 3).
- Produces (R8: row-level encoding is public so snapshots reuse it):

```rust
pub enum SqlValue { Null, Integer(i64), Real(f64), Text(String), Blob(Vec<u8>) }   // + From/To rusqlite::types::Value
pub enum RowKey { Single(String), Composite(Vec<String>), Singleton }
impl RowKey { pub fn to_pk_text(&self) -> String; pub fn from_pk_text(spec: &TableSpec, pk: &str) -> Result<Self>; }
// to_pk_text: Single → the text; Composite → serde_json compact array, byte-identical to SQLite json_array(); Singleton → "profile"
pub struct RowChange { pub table: String, pub key: RowKey, pub generation: Clock, pub insert: bool,
    pub deleted: Option<Clock>, pub columns: BTreeMap<String, (SqlValue, Clock)>, pub parents: BTreeMap<String, Clock> }
pub struct Changeset { pub schema: u32, pub barrier: u32, pub device: DeviceId, pub seq: u64,
    pub prev: Option<[u8; 32]>, pub rows: Vec<RowChange> }
pub struct DevTable { /* first-use ordered list of DeviceId with a reverse index */ }
impl DevTable { pub fn new() -> Self; pub fn index(&mut self, d: DeviceId) -> usize; pub fn into_vec(self) -> Vec<DeviceId>; }
pub fn encode_row(row: &RowChange, devs: &mut DevTable) -> serde_json::Value;
pub fn decode_row(value: &serde_json::Value, devs: &[DeviceId]) -> Result<RowChange>;
pub const CHANGESET_VERSION: u32 = 1;
pub fn to_json(cs: &Changeset) -> Vec<u8>;                 // built on encode_row
pub fn from_json(bytes: &[u8]) -> Result<Changeset>;      // built on decode_row; AppError::Validation("sync.changesetInvalid") on any shape error
pub fn padded_deflated_len(json: &[u8]) -> usize;          // deflate (flate2 default level) then round up to 1024
pub fn max_hlc(cs: &Changeset) -> u64;
```

Wire (exactly §9.1, plus corrections 10 and 18): top level `v`, `schema`, `barrier` (omitted when
0), `device` (base64url), `seq`, `prev` (base64url; omitted when `seq == 1`), `devs` (base64url list,
first-use order, every clock's device including the author's and `DeviceId::ZERO` when a clock is
zero), `rows`. A row: `t`, `k` (string | array | `"profile"`), `g` `[hlc, i]`, `ins: true` (omitted
when false), `del` `[hlc, i]` (optional), `c` `{col: [value, hlc, i]}` (omitted when empty), `p`
`{fk_col: [hlc, i]}` (omitted when empty). Values: JSON null, string, integer, float (seed tables
only), `{"b": "<base64>"}` for BLOB. HLCs are JSON integers up to `i64::MAX`; the decoder rejects a
larger value (correction 16). Rust is the only reader (HLCs exceed 2^53, so no JavaScript consumer
may parse this JSON).

- [ ] **Step 1: Failing tests**

```rust
#[test] fn the_spec_example_round_trips()
// the two rows of §9.1 (amount/currency insert; stock_investment_tags composite delete) → from_json(to_json(x)) == x
#[test] fn the_json_uses_the_spec_field_names()
// serde_json::Value of to_json: v == 1, rows[0].ins == true, rows[1].k is an array, rows[0].c.amount[0] == "-1250.00", rows[0].g is [hlc, 0]
#[test] fn a_row_encodes_and_decodes_on_its_own_with_a_shared_dev_table()
#[test] fn ties_keep_the_author_of_each_clock()      // two clocks with equal hlc and different devices → two devs entries, distinct indexes
#[test] fn the_zero_clock_travels_with_the_zero_device()
#[test] fn blob_values_travel_as_base64_objects()
#[test] fn a_composite_key_text_matches_sqlite_json_array()
assert_eq!(RowKey::Composite(vec!["inv-1".into(),"tag-1".into()]).to_pk_text(),
           conn.query_row("SELECT json_array('inv-1','tag-1')", [], |r| r.get::<_, String>(0))?);
#[test] fn an_unknown_device_index_is_rejected()
assert!(from_json(br#"{"v":1,"schema":29,"device":"…","seq":1,"devs":[],"rows":[{"t":"bonds","k":"x","g":[1,0]}]}"#).is_err());
#[test] fn a_clock_above_i64_max_is_rejected()
#[test] fn a_version_other_than_1_is_rejected()
#[test] fn padded_length_is_a_multiple_of_1024()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::changeset` PASS → gates → commit `feat(sync): plaintext changeset format`.

---

### Task 11: Journal — outbox to changesets

**Files:**
- Create: `src-tauri/src/services/sync/journal/mod.rs` (`build_next`, ordering, `record_sealed`), `journal/read.rs` (`read_row`), `journal/cut.rs` (measuring and cutting)

**Interfaces:**
- Consumes: registry, `fk_depth`, `foreign_keys`, `state::load`, `changeset::*`, `sync_renamed` (Task 2; filled by Task 12).
- Produces:

```rust
pub const ENVELOPE_BUDGET: usize = 61_440;
pub const SINGLE_ROW_LIMIT: usize = 184_320;
pub struct Built { pub changeset: Changeset, pub max_hlc: u64 }
pub struct Refused { pub table: String, pub pk: String, pub bytes: usize }
pub fn build_next(tx: &rusqlite::Transaction<'_>) -> Result<(Option<Built>, Vec<Refused>)>;
// the next envelope; deletes the outbox rows it consumed in `tx` (R11); None when nothing (except refused rows) is left
pub fn record_sealed(tx: &rusqlite::Transaction<'_>, built: &Built, hash: [u8; 32]) -> Result<()>;
// same transaction: sync_own_log(seq, hash, max_hlc); next_seq = seq + 1; last_hash = hash
// read.rs — shared with snapshot.rs
pub fn read_row(conn: &Connection, table: &str, pk: &str) -> Result<Option<RowChange>>;   // full live row, or a tombstone-only change, or None
```

Rules (R1; plan's calls where marked):
- **Always the full row.** A live row → `insert: true`, every column except Local ones (Recomputed
  and recomputed-or-typed included), each with its column clock, else the row's `'*'`, else
  `Clock::ZERO`; `generation` = `'*'` or `Clock::ZERO`. A live row that has a `'-'` clock also
  carries `deleted` = that clock (re-created since, correction 10). A missing row → `insert: false`,
  `deleted` = its `'-'`, `generation` = its `'*'` or ZERO, no columns. A key with neither → `None`.
- **Typed names:** for a row with a `sync_renamed` entry whose `assigned` equals the stored name, the
  name column carries `typed` (correction 17); a stale entry (stored ≠ assigned) is ignored.
- `parents`: for each FK column carried with a non-NULL value, the parent row's `'*'` clock (ZERO
  when it has none or is missing locally).
- **Order:** live rows sorted by `(fk_depth, queued_at, tbl, pk)`, `transaction_categories` rows
  parent-first within equal depth; then tombstone-only rows sorted by
  `(fk_depth DESC, queued_at, tbl, pk)` (children's changes travel before a parent's tombstone).
- **Cutting** (`cut.rs`): fill greedily; measure `padded_deflated_len(to_json(candidate))`
  whenever the raw JSON grew past the last measured size by 8 KiB, and always before closing; close
  the envelope before the row that pushes it over `ENVELOPE_BUDGET`. A row that alone exceeds the
  budget travels alone if ≤ `SINGLE_ROW_LIMIT`, otherwise it is `Refused` (logged with
  `log::error!`, left in the outbox, skipped by later calls; other rows continue).
- One envelope per call: `seq` = `next_seq`, `prev` = `last_hash`; `schema` = `schema_number()`,
  `barrier` = `sync_state.barrier`. The caller (phase 2: the engine; phase 1: tests and the harness)
  runs, in one transaction: `build_next` → hash (phase 1: `sha2::Sha256` of `to_json`; phase 2:
  of the sealed envelope, stored in `sync_unsent`) → `record_sealed` → commit. A write after the
  commit re-queues its row through the trigger.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn five_edits_of_one_row_send_one_value()
#[test] fn a_row_changed_then_deleted_sends_only_its_tombstone()
#[test] fn every_change_is_a_full_row_with_per_column_clocks()
// insert a spending plan, edit its note → the row carries every synced column; 'note' with its column clock, the rest with '*'.
#[test] fn a_row_that_never_had_a_clock_travels_with_the_zero_clock()
// a pre-sync row edited after enable → g == Clock::ZERO, unchanged columns carry ZERO, devs contains DeviceId::ZERO.
#[test] fn full_rows_carry_typed_values_and_derived_values_alike()         // Review Focus 4
// position without transactions (typed quantity "7") and with transactions (derived quantity) → both rows carry 'quantity'; Local columns absent.
#[test] fn a_deleted_and_recreated_row_carries_del_and_ins()
// learned_payees via learn_scoped twice (same natural id) → one row with insert == true and deleted == Some(tombstone).
#[test] fn every_fk_value_carries_its_parent_generation()
#[test] fn a_repair_renamed_row_travels_with_its_typed_name()
#[test] fn parents_travel_before_children_and_children_tombstones_before_parents()
#[test] fn a_5000_row_import_is_cut_into_envelopes_under_60_kib_with_batch_first()
// csv_import_batches row + 5,000 bank_transactions via bank_accounts::create_transaction_with_origin, then a newer edit of the batch row;
// loop (tx; build_next; record_sealed; commit) → every padded_deflated_len ≤ 61_440, > 1 envelope, the batch row in envelope 0,
// union of rows == the 5,001 keys, outbox empty at the end, every transaction row a full row.
#[test] fn the_largest_allowed_row_travels_alone()
// watched_stocks.notes = 100 KiB of base64 text of seeded random bytes via stock_monitor::update_notes → one envelope, 61_440 < len ≤ 184_320.
#[test] fn a_row_over_180_kib_is_refused_and_stays_queued()
// notes forced by SQL to 300 KiB of base64 text of seeded random bytes (deflates to ~225 KiB) → Refused{table:"watched_stocks",…};
// sync_outbox still has it; other rows built; a later build_next skips it again.
#[test] fn build_next_consumes_the_outbox_in_its_transaction()
// rollback instead of commit → the outbox rows are back.
#[test] fn record_sealed_advances_the_chain()
// seq 1 → own_log (1, hash, max_hlc), next_seq 2, last_hash == hash; the next build_next has seq 2 and prev == hash.
#[test] fn an_edit_after_the_commit_requeues_the_row()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::journal` PASS → gates → commit `feat(sync): journal builds full-row changesets from the outbox`.

---

### Task 12: Repairs after a merge (§7.1 steps 1–3, §7.2, §7.4, §14 seeds)

**Files:**
- Create: `src-tauri/src/services/sync/repair/mod.rs` (`Touched`, `after_merge`, `full`, seeds, domains), `repair/mirrors.rs`, `repair/invariants.rs`, `repair/renames.rs`
- Modify: `services/sync/mod.rs` (`on_open` calls `delete_revived_seeds` before `install`)
- Modify: `services/stock_tags.rs` (`create_group`, `update_group`, `delete_group`, `create_tag`, `update_tag`, `delete_tag`), `services/spending_plans.rs` (`create_plan`, `create_plans`, `update_plan`, `delete_plan`), `services/categories.rs` (`create_category`, `update_category`, `delete_category`): each calls `repair::renames::after_local_write` for its scope after its write (correction 17)

**Interfaces:**
- Consumes: Tasks 7–9; `stock_splits::reconcile_ticker`; `mcp_refresh::enqueue`;
  `attachment_store::delete_unreferenced`; `capture::CLOCK_UPSERT`.
- Produces (R12 — Task 13 fills `Touched` exactly as documented on each field):

```rust
#[derive(Default)] pub struct Touched {
    pub rows: BTreeMap<String, BTreeSet<String>>,     // table → pk text of every row the merge inserted, changed or deleted
    pub parents: BTreeMap<String, BTreeSet<String>>,  // parent table → ids referenced, old and new, by every touched child row
    pub deleted: BTreeSet<String>,                     // tables that lost a row (tombstone, cascade rule, orphan rule, unique loser)
    pub days: BTreeMap<&'static str, Option<i64>>,    // refresh domain → earliest affected UTC day; None = from the first fact
    pub tickers: BTreeSet<String>,                     // old and new `ticker` of touched rows of tables that have one
    pub budget_keys: BTreeSet<(String, String)>,       // old and new (category_id, timeframe) of touched budget_goal_versions
    pub rename_scopes: BTreeSet<(String, String)>,     // (table, scope value or "") of touched rows of the four rename tables
}
impl Touched { pub fn row(&mut self, table: &str, pk: &str); pub fn parent(&mut self, table: &str, id: &str);
               pub fn mark_deleted(&mut self, table: &str); pub fn day(&mut self, domain: &'static str, day: Option<i64>); /* keeps the minimum; None absorbs */ }
#[derive(Default)] pub struct RepairReport { pub warnings: Vec<String>, pub domains: BTreeMap<&'static str, Option<i64>> }
pub fn after_merge(conn: &Connection, touched: &Touched, now: i64) -> Result<RepairReport>;  // runs under with_mode(Repair)
pub fn full(conn: &Connection, now: i64) -> Result<RepairReport>;                           // every parent and scope; after snapshot restore
pub fn delete_revived_seeds(conn: &Connection) -> Result<usize>;
pub fn tombstone_local(conn: &Connection, table: &str, pk: &str, clock: Clock) -> Result<()>; // '-' via CLOCK_UPSERT, no outbox
pub fn domain_of(table: &str) -> Option<(&'static str, Option<&'static str>)>;            // (mcp_refresh domain, date column)
// renames.rs — the ONE rename function (R9); merge::unique calls it too
pub struct RenameScope { pub table: &'static str, pub name_column: &'static str, pub scope_column: Option<&'static str>, pub fold: fn(&str) -> String }
pub const RENAME_SCOPES: [RenameScope; 4];
pub fn assign_names(rows: &[(String, String)], fold: fn(&str) -> String) -> Vec<(String, String)>; // (pk, typed) → (pk, name); pure
pub fn recompute_scope(conn: &Connection, scope: &RenameScope, scope_value: &str) -> Result<usize>;
pub fn after_local_write(conn: &Connection, table: &str, scope_value: &str) -> Result<usize>;    // Ok(0) when capture is not installed (G5)
```

`RENAME_SCOPES` (plan's call): `stock_tags.name` and `stock_tag_groups.name` (whole table, fold =
identity: the UNIQUE index is binary), `spending_plans.name` per `year` (fold =
`spending_plans::name_key`), `transaction_categories.name` (whole table, fold =
`trim().to_lowercase()`, as `categories::name_exists`).

**`assign_names` (R9, plan's call — no suffix stripping):** input is the *typed* name of every row in
the scope. `taken` = the folds of all typed names. Group rows by `fold(typed)`; walk the groups in
ascending fold order and each group's rows in ascending pk; the first row of a group keeps its typed
name; each further row gets `typed + " (k)"` with the smallest `k ≥ 2` whose fold is not in `taken`,
then adds that fold to `taken`. It is a function of the set of `(pk, typed)` only, so it is
idempotent and order-free; a user-typed "Growth (2)" is just another typed name.
**`recompute_scope`:** typed = `sync_renamed.typed` when an entry exists and its `assigned` equals
the stored name, else the stored name (a stale entry means a user or merge wrote the column since);
run `assign_names`; write changed names in two passes (first every changing row to the placeholder
`"\u{1f}" + pk`, then the final names, so the binary UNIQUE index of tags never trips) under
`with_mode(Repair)`; upsert `sync_renamed(typed, assigned)` where assigned ≠ typed, delete it where
equal. `after_local_write` runs it on the author's device after a local write, so a rename that frees
a name restores the typed names there too.

`after_merge` runs, using only `Touched` (every selection ordered by primary key):
1. **Mirrors** (`mirrors.rs`): `refresh_balance_columns` for `parents["bank_accounts"] ∪ rows["bank_accounts"]`
   that have records; `refresh_anchor_mirror` and `rebuild_terms_chain` for touched loans;
   `sync_parent_price` for touched real estate / other assets that have valuations;
   `stock_splits::reconcile_ticker` for every `tickers` entry; `recalculate_investment_metrics` /
   `recalculate_crypto_metrics` / `recalculate_asset_totals` for touched parents that have source
   rows; `merge_equal_neighbours` + `rebuild_goal_row` for every `budget_keys` entry. **Failure
   policy** (correction 12, plan's call): `validation.sellExceedsHoldings` /
   `validation.stockSplitOversold` → write the unchecked net quantity `max(0, Σbuy − Σsell)` as
   canonical decimal text and add a warning; any other recalculation error → keep the stored value
   and add a warning. Never return `Err` for a recalculation; SQL errors still propagate.
2. **Invariants** (`invariants.rs`): one tag per group per investment → keep the assignment with the
   newest `'*'` clock (ties by `tag_id`), delete the others with `tombstone_local` at the winner's
   clock; one `balance_check` per loan per day → same rule on `loan_events`; `recompute_scope` for
   every `rename_scopes` entry; attachment blobs: when `deleted` contains `real_estate` or
   `insurance_policies` and `files_schema::is_attached`, `attachment_store::delete_unreferenced`.
3. **Domains** (`domain_of`): `stocks` ← stock_investments, investment_transactions,
   investment_dividends, stock_splits, stock_price_overrides, dividend_overrides; `crypto` ←
   crypto_investments, crypto_transactions, crypto_price_overrides; `other-assets` ← other_assets,
   other_asset_transactions, other_asset_valuations; `bank-accounts` ← bank_accounts,
   bank_balance_records, bank_transactions, csv_import_batches, bank_account_zones; `real-estate` ←
   real_estate, real_estate_valuations, real_estate_one_time_costs, real_estate_loans; `loans` ← loans,
   loan_events; `bonds` ← bonds. `after_merge` calls `mcp_refresh::enqueue(conn, domain, day)` for
   every `touched.days` entry and returns them as `report.domains`.

`delete_revived_seeds`: in every Synced table, delete (no triggers involved: it runs before
`install`) the rows whose key has a `'-'` clock and either no `'*'` or a `'*'` older than the `'-'`.

- [ ] **Step 1: Failing tests** (`repair::tests`, `repair::renames::tests`; real services on `migrated()` with sync enabled; `Touched` built by hand here, Task 13 tests that the merge fills it the same way)

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
#[test] fn assign_names_numbers_duplicates_by_id_and_skips_taken_names()
assert_eq!(assign_names(&[("b","Holiday"),("a","holiday"),("c","Holiday (2)")].map(to_owned), name_key),
           vec![("a","holiday"),("b","Holiday (3)"),("c","Holiday (2)")].map(to_owned));
#[test] fn assign_names_gives_the_same_names_in_any_input_order()
// every permutation of a 4-row input → identical output.
#[test] fn renames_applied_in_two_orders_end_with_the_same_names()
// two connections; tags a "G", b "G", c "G" arrive in orders [a,b,c] and [c,b,a] with recompute_scope after each → identical stored names and sync_renamed.
#[test] fn a_rename_that_frees_a_name_restores_the_typed_name()
// a "G", b typed "G" stored "G (2)"; rename a to "Value" through stock_tags::update_tag (after_local_write) → b "G", sync_renamed empty.
#[test] fn duplicate_category_names_fold_non_ascii_case()                   // "Děti" / "děti"
#[test] fn renames_without_sync_do_nothing()                                // after_local_write on a profile without capture → Ok(0), no sync_renamed row
#[test] fn budget_versions_are_normalised_and_the_goal_row_rebuilt()
#[test] fn split_units_follow_the_merged_status()
#[test] fn touched_domains_are_queued_with_the_earliest_day()
assert_eq!(mcp_refresh::pending(&conn)?.iter().find(|j| j.domain=="bank-accounts").unwrap().earliest_date, Some(day));
#[test] fn repairs_record_no_clock_and_queue_nothing()
#[test] fn a_seeded_row_with_a_tombstone_is_deleted_again()
// delete a seeded category with sync on (tombstone), re-insert it by SQL as a migration would → delete_revived_seeds == 1.
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::repair services::stock_tags services::spending_plans services::categories` PASS → gates → commit `feat(sync): deterministic repairs after a merge`.

---

### Task 13: Merge core — epochs, generations, column clocks, parents (D12–D14)

**Files:**
- Create: `src-tauri/src/services/sync/merge/mod.rs` (`apply_changeset`, `merge_rows`, reports), `merge/row.rs` (epoch and generation steps), `merge/columns.rs` (column LWW, groups, weak `pending`, write-once), `merge/parents.rs` (FK checks, orphan rules)

**Interfaces:**
- Consumes: Tasks 3–5, 9, 10, 12.
- Produces:

```rust
pub const DATA_CHANGED_EVENT: &str = "sync-data-changed";   // the engine (phase 2) emits it with ApplyReport.domains
pub enum StopReason { MissingParent { table: String, key: String, parent: String }, NewerSchema(u32) }
#[derive(Default)] pub struct ApplyReport { pub applied: usize, pub dropped: usize, pub domains: BTreeMap<&'static str, Option<i64>>, pub warnings: Vec<String> }
pub enum Outcome { Applied(ApplyReport), Stopped(StopReason) }
pub fn apply_changeset(conn: &Connection, cs: &Changeset, hash: [u8; 32], now: i64) -> Result<Outcome>;
pub fn merge_rows(conn: &Connection, rows: &[RowChange], now: i64) -> Result<Outcome>;  // inside the CALLER's transaction; also used by snapshot::restore_merge
// row.rs
#[derive(Default)] pub struct MergeCtx { pub touched: Touched, pub report: ApplyReport }
pub enum RowResult { Applied, Dropped, Stop(StopReason) }
pub fn apply_row(conn: &Connection, change: &RowChange, ctx: &mut MergeCtx) -> Result<RowResult>;
```

`merge_rows`: `PRAGMA defer_foreign_keys = ON`, `with_mode(Merge)`; every clock of every row goes
through `capture::observe` (a `FarFuture` adds a warning naming the author device and is logged with
`log::warn!`); rows **in the given order** through `apply_row`; the first `Stop` returns `Stopped`
(the caller rolls back); then `repair::after_merge(&ctx.touched)` (it switches to Repair itself) and
its warnings and domains go into the report. `apply_changeset`: `cs.schema > schema_number()` →
`Stopped(NewerSchema)` without a transaction; else `unchecked_transaction`, `merge_rows`, on
`Applied` upsert `sync_heads (cs.device, cs.seq, hash)` and commit, on `Stopped` roll back.

Row algorithm (`row.rs`, correction 10; the R2 pushback is explained below):

```
L = local '*' (ZERO if the row exists without one); X = local '-' (None if absent); live = row exists
0. parents.rs on the FK columns the change carries: Ok | set a column NULL | turn the change into a
   cascade tombstone at the parent's '-' | Stop(MissingParent)
1. if change.deleted = Δ:
       if live && Δ > L: NO ACTION orphan rules on local children (Repair mode), set_cascade_clock(Δ),
                         DELETE (SQLite cascades; merge-mode triggers tombstone the children at Δ), live = false
       '-' = max(X, Δ); X = that
2. if !change.insert: done
3. if X present && g < X: drop (an edit or creation from an epoch that has ended)
4. if !live: INSERT every carried column; '*' = g; explicit column clocks for every column whose clock != g,
             except a weak pending status (never explicit)
5. if live: columns.rs merges every carried column (local clock = column clock, else L; incoming = its clock);
            '*' = max(L, g); then every column whose resulting clock != the new '*' gets an explicit
            column clock, except a weak pending status
```

`columns.rs`: a group's clock on each side is the max of its members; the winning side writes every
member. `stock_splits.status` (correction 9): local is weak when `'pending'` and there is no explicit
`status` column clock; incoming is weak when `'pending'` and its clock equals `change.generation`;
weak loses to non-weak whatever the clocks; two weak values are equal; otherwise the newer clock wins.
`sync_devices.public_key` (write-once, plan's call made commutative): non-NULL beats NULL; between two
different non-NULL values the **older** clock wins and a warning names the device.

`parents.rs` for each FK `(col → parent)` whose new value `v` is non-NULL, with `pg =
change.parents[col]` (ZERO if absent), parent `PL` (`'*'` or ZERO) and `PD` (`'-'` or none):
`PD` present and `pg < PD` → the child belongs to a deleted parent epoch: apply the FK's rule —
`Cascade` / `NoAction(DropChild)` → the change becomes a tombstone-only change at `PD` (delete the
child if live, `'-'` = `PD`); `SetNull` / `NoAction(SetNull)` → write NULL to `col` with the change's
clock. Else parent live and `pg ≤ PL` → OK; else `Stop(MissingParent)` (a parent generation this
device never saw, or a parent missing without a tombstone). Before deleting a parent row (step 1, or a
cascade tombstone), apply the NO ACTION orphan rules to its live local children under
`with_mode(Repair)`: set NULL (nullable) or delete with `repair::tombstone_local` at the parent's
tombstone clock — otherwise the deferred FK check fails the whole changeset at commit.

`Touched` (R12) — `apply_row` reads the local row before writing and records: `row(table, pk)` for
every row it inserts, changes, deletes or cascades (including orphan-rule writes and the rows unique
outcomes touch in Task 14); `mark_deleted(table)` for every deletion; `parent(p, id)` for the old and
new non-NULL value of every FK column; `tickers` for the old and new `ticker` of touched rows of
tables that have that column; `budget_keys` for old and new `(category_id, timeframe)` of
`budget_goal_versions`; `rename_scopes` for old and new scope values of the four rename tables;
`day(domain, …)` via `repair::domain_of`: a written child row with a date column → `Some(min(old
date, new date))`, any deletion or a write to a parent table without a date column → `None`, and a
deletion also gives `None` to the domains of the deleted table's `Cascade` child tables.

T13 inserts and updates directly; Task 14 wraps steps 4 and 5 with `unique::resolve` (until then the
tests avoid unique collisions).

**Worked example 1 — concurrent creation, no delete (R2 branch "merge per column").** A detects split
S (natural id, `g = gA`) and applies it (status `applied`, column clock `tA`). B, offline, detects S
too (`g = gB > gA`, weak `pending`). On A, B's row is live-epoch (`X` absent) → per-column merge:
status `applied` (non-weak) beats B's weak `pending`; other columns by clock; `'*' = gB`. On B, A's
row has `g = gA < gB` → still merged per column: A's non-weak `applied` beats B's weak `pending`. Both
end `applied`, `'*' = gB`, same columns.

**Worked example 2 — delete and re-create with a concurrent child.** A deletes AAPL (`Δ`) and
re-creates it (`g5 > Δ`); A's row carries `"del": Δ` and `"ins"` with `g5`. B, offline, adds
transaction `c` under the old generation `gP < Δ` (`"p": {"investment_id": gP}`). Any device: A's row
first → old row deleted (cascading its old children), new row inserted; then `c` → `PD = Δ > gP` →
cascade, `c` dropped. `c` first → inserted under `gP`; then A's row → `Δ > L = gP` → delete (cascades
`c` at `Δ`), insert `g5`. B's own later edit of the old row (`g = gP < X = Δ`) is dropped everywhere.

**R2 pushback (implemented as the closest safe rule above).** The ruling keeps "changes addressed to
an OLDER generation than the local row stay dropped". In example 1 that breaks: on B, A's row (`gA <
gB`) would be dropped together with its `applied` status, while A merged B's row per column and kept
`applied` — A ends `applied`, B `pending`, for ever. Dropping must be keyed to the delete epoch (`g <
X`), not to the generation; whole-row replacement happens exactly when the ruling wanted it (a carried
or local delete newer than the local generation), via step 1.

- [ ] **Step 1: Failing tests** (`merge::tests`; two or three `migrated()` connections, changesets from Task 11's `build_next`, or hand-made `RowChange`s for the stop cases)

```rust
#[test] fn an_insert_is_created_with_its_clocks_and_triggers_queue_nothing()
#[test] fn a_newer_column_clock_wins_and_an_older_one_is_dropped()
#[test] fn equal_hlcs_tie_break_by_device()
#[test] fn a_group_is_taken_whole_from_the_newer_side()                  // amount from A, currency from B never mix
#[test] fn d13_a_tombstone_beats_a_later_edit_of_the_same_generation()
#[test] fn d13_an_insert_newer_than_the_tombstone_recreates_the_row()
#[test] fn concurrent_creation_of_same_natural_id_merges_per_column()    // worked example 1 shape on a watched_stocks row, both orders
#[test] fn the_stored_generation_is_the_larger_and_unchanged_columns_keep_their_clocks()
#[test] fn delete_and_recreate_with_a_concurrent_child_converges_in_both_orders() // worked example 2
#[test] fn an_edit_of_an_ended_epoch_is_dropped()
#[test] fn cascade_child_added_offline_follows_the_parent_tombstone()
#[test] fn set_null_child_added_offline_loses_the_reference()             // spending_plan_id
#[test] fn no_action_orphan_nullable_category_is_cleared()
#[test] fn no_action_orphan_required_rule_is_dropped()
#[test] fn a_tombstone_with_local_no_action_children_still_commits()
#[test] fn missing_parent_without_tombstone_stops_and_rolls_back()
assert!(matches!(apply_changeset(&b, &cs, h, now)?, Outcome::Stopped(StopReason::MissingParent{..}))); // b unchanged, no head
#[test] fn a_detection_insert_never_undoes_an_applied_split()             // weak pending, both orders
#[test] fn a_weak_pending_never_gets_a_column_clock()
#[test] fn a_revert_reaches_the_other_devices()                           // update to pending is strong and wins by clock
#[test] fn the_older_public_key_of_a_device_wins_in_both_orders()
#[test] fn a_clock_more_than_24_hours_ahead_is_applied_with_a_warning()
#[test] fn a_newer_schema_stops_before_touching_anything()
#[test] fn applying_a_changeset_twice_changes_nothing()                   // Review Focus 3: dump of synced tables + sync_clock identical
#[test] fn merge_moves_the_local_clock_past_every_clock_it_saw()
#[test] fn the_head_of_the_author_is_recorded_in_the_same_transaction()
#[test] fn the_merge_fills_touched_as_documented()
// one changeset with a bank_transactions insert (booking day 100, account acc-1), an update moving another payment from day 300 to day 200,
// and a spending plan tombstone → touched.rows has the three keys; parents["bank_accounts"] ∋ "acc-1"; days["bank-accounts"] == Some(100);
// deleted ∋ "spending_plans"; rename_scopes ∋ ("spending_plans", "<year>").
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::merge` PASS → gates → commit `feat(sync): merge by delete epoch, generation and per-column clocks`.

---

### Task 14: Merge — unique-key outcomes (D15)

**Files:**
- Create: `src-tauri/src/services/sync/merge/unique.rs`; Modify: `merge/row.rs` (call `resolve` before the INSERT of step 4 and before writing an indexed column in step 5)

**Interfaces:**
- Consumes: `registry::keys::unique_rules`, `repair::tombstone_local`, `repair::renames::recompute_scope`.
- Produces: `pub fn resolve(conn: &Connection, table: &str, incoming_pk: &str, values: &BTreeMap<String, (SqlValue, Clock)>, generation: Clock, ctx: &mut MergeCtx) -> Result<Resolution>`
  with `pub enum Resolution { Write(BTreeMap<String, (SqlValue, Clock)>) /* possibly unlinked or with a placeholder name */, Drop }`.

Conflicts are found by evaluating the index expressions over the incoming values
(`SELECT <exprs> FROM (SELECT ?1 AS c1, …)`) and comparing with local rows (`partial` clause honoured,
the incoming key excluded). Outcomes, written in Repair mode before the incoming write; every row they
touch goes into `ctx.touched`:
- **SameId**, different key (impossible after D22) → handled as KeepNewest plus a warning.
- **KeepNewest** (correction 15, R10): each side's key clock is `max(generation, clocks of the
  rule's key_columns)` (for the incoming row: `max(g, carried clocks of those columns)`); compare
  `(key clock, then pk)`; the loser is deleted with `tombstone_local` at the winner's key clock (so
  the loser's epoch ends), or, if it is the incoming row, `Drop` with its key tombstoned the same way.
- **Rename** (tags, groups): `Write` the incoming row with the placeholder name `"\u{1f}" + pk`,
  record `sync_renamed(typed = incoming name, assigned = placeholder)`, then `recompute_scope`
  assigns every name of the scope (the one rename function, correction 17).
- **Unlink**: among rows linked to the same `replacement_source_id`, every one but the lowest id is
  set NULL (local rows first, or the incoming value).

- [ ] **Step 1: Failing tests**

```rust
#[test] fn same_id_two_devices_buying_aapl_offline_share_one_position()   // through investments services on A and B, exchange both ways
#[test] fn keep_newest_balance_record_of_the_same_day()
#[test] fn keep_newest_uses_the_clock_that_put_the_row_on_its_key()
// budget version V1 (gen 10) moved onto (cat, month) by an update at clock 50; V2 created there at gen 30 → V1 wins on both devices.
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

### Task 16: Snapshot records — dump, join, restore by replace and by merge (§8)

**Files:**
- Create: `src-tauri/src/services/sync/snapshot.rs`

**Interfaces:**
- Consumes: `journal::read_row`, `changeset::{encode_row, decode_row, DevTable}` (R8, no second JSON
  codec), `merge::merge_rows`, `repair::full`, `state::ensure_row`, `capture::install`, registry.
- Produces:

```rust
pub enum Record { Header { schema: u32, barrier: u32, seq: u64, heads: Vec<(DeviceId, u64, [u8; 32])> },
                  Devices(Vec<DeviceId>), Row(RowChange) /* live rows and tombstone-only changes alike */,
                  Seed { table: String, row: BTreeMap<String, SqlValue> }, End { rows: u64, seeds: u64 } }
pub struct DumpStats { pub rows: u64, pub seeds: u64, pub max_hlc: u64 }
pub fn dump(conn: &Connection, seq: u64, out: &mut impl std::io::Write) -> Result<DumpStats>;
pub fn read_record(input: &mut impl std::io::Read, devs: &[DeviceId]) -> Result<Option<Record>>;
pub fn join(conn: &Connection, device: DeviceId, input: &mut impl std::io::Read, now: i64) -> Result<DumpStats>;
pub fn restore_replace(conn: &Connection, input: &mut impl std::io::Read, now: i64) -> Result<DumpStats>;
pub fn restore_merge(conn: &Connection, input: &mut impl std::io::Read, now: i64) -> Result<DumpStats>;
```

Stream (plan's call; phase 2 deflates and encrypts the same bytes): each record is a 4-byte
big-endian length followed by one JSON object with `"r"` ∈ `devs | header | row | seed | end`; a row
record is `{"r":"row", …encode_row(…)}` and its clocks index the stream's one `devs` record, which is
written first. Order: `devs`, `header`, rows of Synced tables by FK depth (categories parent-first;
every key from `read_row`: full live rows and tombstone-only changes), `seed` rows of the ten Seed
tables, `end`. Rows include the `user_profile` singleton's synced columns and the allowlisted
`app_config` keys only; never `sync_state`, `sync_renamed` or Local columns. A stream without `end`,
or whose counts differ, is rejected before anything is written (read fully first; plan's call: phase
1 holds records in memory). A header whose `barrier` is below `sync_state.barrier` is refused with
`AppError::Validation("sync.snapshotBelowBarrier")` (§8). **Heads (R3):** the header's `heads` are
every `sync_heads` row plus the dumper's own `(device, next_seq − 1, last_hash)` when
`next_seq > 1`.

`join` (R4 order — a joining device): `state::ensure_row(device)` → `restore_replace` (capture not
installed yet, so nothing is clocked or queued) → `capture::install(device)` (the HLC starts above
the restored clocks). Rule packs are **not** rewritten: they come from the vault.
`restore_replace`: one transaction, `with_mode(Repair)` if capture is installed, `defer_foreign_keys`;
delete the contents of every Synced and Seed table (deepest first; for `user_profile` keep row 1 and
overwrite its synced columns; for `app_config` delete only the allowlisted keys); clear `sync_clock`,
`sync_outbox`, `sync_heads`, `sync_renamed`; insert rows (with their clocks) and tombstones, seeds,
heads; `sync_state.cursor = seq`, `snapshot_seq = seq`, `snapshot_hlc = max_hlc` (`UPDATE` of the row
`ensure_row` created); then `repair::full`. `restore_merge` (after `410`, re-join): one transaction,
`merge::merge_rows` over the row records (it sets Merge mode, deferred FKs and observes every clock),
seeds inserted only where the primary key is absent, heads take the higher `seq` per device, outbox
untouched; then `repair::full`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn a_dump_restored_by_replace_reproduces_every_synced_table_and_clock()
#[test] fn a_profile_with_rows_from_before_sync_dumps_every_row()          // rows without any clock travel with Clock::ZERO
#[test] fn replace_discards_the_joining_profiles_own_rows_and_seeds()       // a seed category deleted in the vault does not come back
#[test] fn joining_queues_nothing_and_installs_capture_after_the_restore()
// join → sync_outbox empty, is_installed, capture::tick() > max clock of the snapshot, rule_packs keys equal the dumper's.
#[test] fn a_device_joined_from_a_snapshot_applies_the_dumpers_next_envelope()   // R3
// A enabled, pushes seq 1; A dumps; B joins; A pushes seq 2 → chain::judge on B == Verdict::Apply (not Gap).
#[test] fn local_data_never_enters_a_snapshot()
// mcp_server_token, language, fx_reference, backup.lastCreatedAt, sync_state, attachment rows → absent from every record.
#[test] fn the_seed_tables_travel_as_plain_rows()
#[test] fn a_truncated_stream_is_rejected_without_writing()
#[test] fn restore_by_merge_keeps_newer_local_edits_and_the_outbox()
#[test] fn heads_and_cursor_come_from_the_header()
#[test] fn derived_columns_are_recomputed_after_restore()                   // budget_goals rebuilt, balances mirrored
#[test] fn a_snapshot_from_below_a_crossed_barrier_is_refused()
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::snapshot` PASS → gates → commit `feat(sync): snapshot record stream, join, replace and merge restore`.

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
- Create: `src-tauri/src/services/sync/convergence/mod.rs` (`#[cfg(test)]`; `Vault`, `assert_converged`, property tests)
- Create: `convergence/ops/mod.rs` (`random_op` dispatch), `ops/bank.rs` (accounts, transactions, balances, categories, learned payees), `ops/stocks.rs` (positions, overrides, watchlist, tags, splits, crypto), `ops/assets.rs` (loans, events, real estate, valuations, other assets, bonds), `ops/planning.rs` (spending plans, budgets, cashflow, milestones, projection, app_config)

**Interfaces:**
- Consumes: everything above.
- Produces (test-only):

```rust
pub struct Device { pub conn: Connection, pub id: DeviceId, pub processed: BTreeSet<usize>, pub held: Vec<usize> }
pub struct Vault { pub devices: Vec<Device>, pub relay: Vec<(Changeset, [u8; 32])> }
pub enum PullEnd { Done, Stopped(Verdict), MergeStopped(StopReason) }
impl Vault {
    pub fn new(n: usize, seed_ops: impl FnOnce(&Connection)) -> Self;
    // device 0: seed_ops, then state::enable (installs, rule packs queued); devices 1..n: snapshot::join(dump(device 0)) — R4 order
    pub fn push(&mut self, d: usize);                     // loop { tx; build_next; sha256(to_json); record_sealed; commit; relay.push } until None
    pub fn pull(&mut self, d: usize, order: &[usize]) -> PullEnd;
    pub fn pull_all_random(&mut self, rng: &mut StdRng); // each device gets its own interleaving that keeps every author's seq order
    pub fn assert_converged(&self);                       // every Synced table's synced + recomputed-or-typed columns, ordered by pk
}
pub fn random_op(dev: &Device, rng: &mut StdRng);         // ops/mod.rs
```

`Vault::pull` handles each relay index of `order` not yet in `processed` (R13):
- `Apply` → `merge::apply_changeset`; `Applied` → processed; `Stopped(r)` → return `MergeStopped(r)`.
- `SkipReplay` → processed. `OwnAcknowledged` → `chain::acknowledge_own(seq)`, processed.
- `Clone` → `chain::rejoin(conn, DeviceId::random())`, update `Device.id`, then judge the same entry
  again (it is now another author's envelope).
- `Quarantine` → `chain::quarantine(author)`, push to `held`, processed. `Held` → push to `held`,
  processed. `BelowBarrier` → `chain::skip_below_barrier`, processed.
- `Gap`, `NewerSchema` → return `Stopped(verdict)`; the entry stays unprocessed for a later pull.

`assert_converged` compares all Synced columns exactly, `other_assets.average_purchase_price` within
`1e-9` relative (§5.2; every device gets the same seeded `exchange_rate_history`), ignores Local
columns, and also compares the set of tombstoned keys and the `sync_renamed` typed names. The ops call
the real services only (each op picks existing entities of that device with the rng):
`bank_accounts::{create_account, update_account, create_transaction, set_transaction_category, record_balance, delete_balance_record, delete_account}`,
`categories::{create_category, update_category, delete_category(reassign)}`,
`categorization::learn::learn_scoped`,
`spending_plans::{create_plan, update_plan, delete_plan, set_transaction_plans}`,
`budgeting::{upsert_goal_at, delete_goal_intervals, delete_budget}`,
`investments::{create_investment_with_transaction, add_transaction_to_investment, update_transaction, delete_transactions}`,
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
An op that the service rejects (validation) is skipped; that is
part of the test.

- [ ] **Step 1: Failing tests**

```rust
const SEEDS: [u64; 24] = [1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946, 17711, 28657, 46368, 75025];
#[test] fn random_operations_converge_under_any_relay_order()
// per seed: Vault::new(3, seed data); 6 rounds of { 1–8 random ops on random devices; random devices push; random devices pull in their own random order };
// final: everyone pushes, everyone pulls everything → assert_converged; failure message includes the seed.
#[test] fn adversarial_orders_converge()
// per seed: each device receives the other authors' envelopes in a worst-case interleaving (all of one author first, then the other; and reversed),
// with duplicated entries (replays) and one withheld entry that is offered in a later pull (Gap, then resume) → assert_converged.
#[test] fn the_harness_detects_a_divergence()
// sanity: write a synced column by SQL on one device with capture in Repair mode → assert_converged panics (#[should_panic]).
```

- [ ] **Steps 2–5:** run (FAIL) → implement → `cargo test --lib services::sync::convergence` PASS (time budget: under 60 s in debug; reduce rounds, not seeds, if slower) → gates → commit `test(sync): three devices converge under random relay orders`.

---

### Task 19: Named convergence scenarios and the capture measurement (§18)

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
| `concurrent_creation_of_same_natural_id_merges_per_column` | A and B both follow MSFT offline, A sets a target, B sets notes → one row with A's target and B's notes |
| `delete_and_recreate_aapl_with_a_concurrent_child` | Task 13 worked example 2 → AAPL present, B's transaction gone |
| `cascade_rule_child_added_offline` | A deletes a loan, B adds an event → no event anywhere |
| `set_null_rule_child_added_offline` | A deletes a spending plan, B tags a payment with it → payment untagged |
| `orphan_rule_category_deleted_while_categorising` | A deletes category K with reassign, B categorises a payment into K and learns a payee for K → payment uncategorised, learned payee dropped, A's reassigned rows on the target |
| `same_id_aapl_on_two_devices` | → one `stock_investments` row with the natural id, transactions of both |
| `keep_newest_balance_same_day` | → one record, the one whose key clock is newer |
| `rename_tags_spending_plans_and_categories` | → suffixes by id identical everywhere; a later rename on one device frees the name everywhere |
| `unlink_replacement_source` | → one link, lowest id keeps it |
| `split_applied_while_a_pre_split_buy_is_added_offline` | → the buy scaled once, basis lists the split |
| `split_applied_and_dismissed_concurrently` | → status by newer clock; units match the status on every device |
| `split_detected_on_two_devices_after_apply` | → `applied` survives on every device (Task 13 worked example 1), shares scaled once |
| `split_revert_reaches_every_device` | → `pending` and unscaled everywhere |
| `import_5000_rows_over_many_envelopes` | A imports 5,000 bank transactions; relay interleaves B's edits between A's envelopes → no stop, 5,000 rows on C |
| `barrier_requeue_resends_rows_pushed_below_the_barrier` | B pushes after the barrier marker landed; B migrates (simulated `schema_seen` < barrier) → B's rows re-sent and applied on A |
| `rejoin_after_simulated_rollback` | (R6) `VACUUM INTO '<tempdir>/b.db'` on B before two edits; push the edits; replace B's connection with `Connection::open` of the copy (`PRAGMA foreign_keys = ON`, no migrations — it is migrated — then `sync::on_open`); one more edit; pull → `Clone` verdict, `rejoin`, every edit present everywhere |
| `clone_detection_from_an_own_id_envelope` | → `judge` returns `Clone` before any push |
| `forked_author_is_quarantined_only` | relay serves two different seq 3 of B → B quarantined on A, C's envelopes still applied |
| `migration_seed_revival_is_deleted_again` | A deletes a seeded category, B re-runs the seed SQL (simulated migration) then `on_open` → category gone on B |
| `local_keys_and_columns_never_travel` | A changes `language`, `theme`, `fx_reference`, MCP token → B unchanged |
| `rule_packs_written_explicitly_travel` | A writes `rule_packs_enabled` → B has it |

Plus, `#[ignore]` (R19): `capture_overhead_on_a_5000_row_statement_import` — import the same 5,000
rows through `bank_accounts::create_transaction_with_origin` in one transaction on a `migrated()`
profile without sync and on one with `state::enable`, best of three runs each, and
`println!("capture overhead: {:.1}%", …)`; §18's target is under 20 % (run with
`cargo test --release --lib capture_overhead -- --ignored --nocapture`). The 50,000-row iPhone merge
target waits for the phone phase.

- [ ] **Step 1: Write the scenarios** (any failure is a merge/repair bug — fix it in the owning module with a unit test there first).
- [ ] **Steps 2–4:** `cargo test --lib services::sync::convergence::scenarios` PASS; run the ignored measurement once in release and note the printed ratio in the commit message → gates.
- [ ] **Step 5: Commit** — `test(sync): named convergence scenarios and the capture measurement`.

---

### Task 20: ADR and standards

**Files:**
- Create: `docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md`
- Modify: `docs/architecture/decisions/README.md` (index row 0024, date 2026-11-06, Accepted)
- Modify: `docs/standards/rust-backend.md` (new "## 11. Rewrites relative to stored values name their sync behaviour")
- Modify: `docs/architecture/overview.md` only if it lists the services modules (add `services/sync/` one line)

ADR 0024 (Context / Decision / Consequences): D4 trigger capture (TEMP triggers from the registry,
installed only when enabled, modes local/merge/repair); D9 HLC 48/16 with device tie-break, the 24 h
warning and the 2^47 ms cap; D12–D15 as implemented: full rows, delete epochs and generations, weak
`pending`, `"p"` and `"del"`+`"ins"`, keep-newest by key clock, renames from typed names (corrections
9, 10, 15, 17, 19); D17 envelope choice (XChaCha20-Poly1305 + HKDF, no signatures — decided now,
built in phase 2). Known limits: concurrent rate changes leave the current terms of the later
wall-clock write; duplicate insurance policy numbers are kept; a device without FX history keeps its
other-asset average until it has one; legacy plain files of a property deleted on another device
stay on disk until phase 4; a keep-newest loser whose key its author moved before seeing the
collision stays deleted on devices that resolved the collision first (only reachable through
category reassignment moving a budget version).

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
`sync_renamed` and the typed-name rename function (no suffix stripping); migration classes for
001–029; group list; `updated_at` of four tables Local; `real_estate.photos` Local; `loans` current
terms plain Synced; `previous_*` Synced + chain repair; full rows always, with `Clock::ZERO` and
`DeviceId::ZERO` for rows that never had a clock; delete epochs (R2 adjusted); weak-`pending` and
write-once rules made commutative; `"p"`, `"del"`+`"ins"`, `"barrier"` wire fields; live rows by FK
depth then tombstones deepest first; one envelope per `build_next`, outbox consumed in the caller's
transaction; journal refuses >180 KiB rows; recalculation fallback; repair deletes record local
tombstones; keep-newest by key clock; ID tie-breaks in the mirrors that write synced columns; HLC ms
capped at 2^47 − 1; export denies `sync_*` by prefix; REPLACE lint strips test modules and scans
migration SQL; snapshot records held in memory in phase 1; snapshot heads include the dumper's own;
`enable` and `join` orders; override upserts moved to a service; projection settings ignore client ids
for new rows; trigger clocks via `sync_hlc_tick()` + `sync_hlc_now()`; mode in a TEMP table.
