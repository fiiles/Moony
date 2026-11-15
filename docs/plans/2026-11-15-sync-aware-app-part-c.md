# Sync-Aware App (Part C: Backups and Wording) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** With healthy sync the app stops asking for manual backups and says that sync protects the data; when sync stops protecting it, the backup reminder comes back by itself. Every text that says where data lives is true with sync on, and a person can tell the device password, the device recovery key and the Sync Recovery Kit apart.

**Architecture:** One rule, in Rust only. The engine records one more local value, `sync_state.last_complete_at` (a round with no problem that left no change and no document or photo to upload; migration 039). The milestone service takes the later of the last manual backup and that time as the newest safe copy (`backup_stale` after 30 days), and the sync status puts the time and one verdict on the wire (`SyncStatus.lastCompleteAt`, `SyncStatus.protectsData`). The frontend never re-derives the rule: it reads the verdict, and uses one trivial predicate `syncIsOn` (flag on, phase neither `off` nor `settingUp`) to pick wording. Everything else is wording in ten locales and one new Settings card.

**Tech Stack:** Rust (rusqlite, Tauri 2), React 19 + TanStack Query, react-i18next (ten locales), Vitest (node env).

**Spec:** `docs/specs/2026-11-14-sync-aware-app-design.md`, part C (§7.1–§7.5) and what §8–§12 say about it; `docs/specs/2026-11-14-sync-attachments-design.md` §5.6 (when a device counts as synchronised) and §5.7 (backups without every document and photo). Read §7, §11 and §12 before your task; the spec wins over this plan on any conflict, and its "(my call)" decisions are binding. This plan's own calls are listed at the end ("Decisions made by this plan").

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-syncux`, on a branch `feat/sync-aware-app-part-c` cut from the merged `main` at build time (after the attachments plan has merged), never in the sibling `Moony-tauri`. Check `git branch --show-current` before each commit. Never push. Commit with explicit paths (`git add <path> …`, never `-A` or `.`).
- **The tree this plan was written against is not the tree it runs on.** Part C is built after the attachments plan (`docs/plans/2026-11-14-sync-attachments-plan-2-app.md`) has finished. Its Tasks 15–18 add, among others: `src/utils/sync-attachments.ts` (`attachmentsLine`, `uploadsKeepUnsynced`, `offerDownloadEverything`, `notInBackup`), the phone backup screen's `mobile.backup.notHere_*` line in `src/mobile/pages/settings/BackupSection.tsx`, the "Documents and photos" rows (`deviceSync.attachments.*`) in `SyncStatusCard.tsx`, `SyncStatusIndicator.tsx` and `SyncStatusBlock.tsx`, the uploading state in `src/utils/sync-state.ts`, and a rewritten `deviceSync.whatSyncs.body`. Before Task 1, check: `grep -n "export function notInBackup" src/utils/sync-attachments.ts` prints one line; if it prints nothing, stop and report. Where a task here touches a file those tasks also changed, find the spot by role with the grep the task names, never by line number, and never re-create, move or re-word what they added.
- Read `AGENTS.md` (ten rules), `docs/standards/` (rust-backend, typescript-frontend, type-contract, testing, i18n, i18n-glossary) and `docs/DEPRECATED.md` before your first change.
- **Everything about sync stays behind the flag:** frontend `useDeviceSyncFeature()` (`src/hooks/use-device-sync.ts`), backend `services::sync::feature::enabled()`. With the flag off, every screen, reminder and notification reads and behaves exactly as today. With the flag on and sync never turned on (phase `off` or `settingUp`), backups behave and read as today too; the only things that change with the flag alone are the ones the spec changes regardless of the sync state: Getting started's "Ochránit data" (§7.3), the first-run privacy point and the lock screen (§7.4 row 1, Task 6) and the "Co si uschovat" card (§7.5, Task 8).
- "Sync is on" in the frontend is exactly `syncIsOn(featureOn, status)` (Task 3, `src/utils/safe-copy.ts`); no component writes its own phase list.
- The newest-safe-copy rule exists once, in `src-tauri/src/services/milestones/upkeep.rs` (Task 2). TS reads `SyncStatus.protectsData` and the `backup_stale` milestone; it never compares safe-copy times itself.
- Thresholds, in whole UTC days (`day_floor`, `DAY` as in `upkeep.rs`): the backup reminder at 30 (`BACKUP_STALE_DAYS`, existing), the new profile's grace at 14 (`FIRST_BACKUP_GRACE_DAYS`, existing), "sync protects" while the last complete sync is less than 7 days old (`SYNC_STALE_DAYS`, existing, from parts A/B). A time in the future is fresh (age 0), never a negative age.
- Migration: append-only in `src-tauri/src/db/migrations.rs` per `docs/playbooks/add-db-migration.md`. This plan writes **039** because the attachments branch ends at 038; at build time take the next free number after the last entry of `all_migrations()` and use it everywhere this plan says 039.
- Wire changes follow the seven steps of `docs/standards/type-contract.md` (`SyncStatus.lastCompleteAt`, `SyncStatus.protectsData`; Task 2). Per-field `#[serde(rename = "…")]`; regenerate with `cd src-tauri && cargo test generate_bindings -- --ignored`.
- Every new or changed user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`) in the task that introduces it, with each language's plural forms (`docs/standards/i18n.md` §2; cs `_one` `_few` `_other`). The English and Czech wording below is binding; translate the other eight per `docs/standards/i18n-glossary.md` (Task 9 adds the secrets' terms; before Task 9 use the renderings the locale files already use). A key removed from English is removed from every locale. A changed value is changed in every locale.
- User copy says "sync server", never "relay" (cs "synchronizační server").
- Tests only for pure logic (`src/utils/`, `src/mobile/lib/`, Rust services); no component, hook, command-handler or `tauri-api.ts` tests (`docs/standards/testing.md`).
- Look and feel: `docs/design-system/README.md`; desktop settings use `SettingsCard` / `SettingsRow`, phone settings use `SectionHead` / `EntityRows` / `SettingsRow`.
- Gates after every task: `npm run lint && npm run typecheck && npm run format:check && npm test`; for tasks that touch Rust also `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Commits: Conventional Commits; the message ends with the `Co-Authored-By` attribution line your own session gives you.
- Never run the app before Task 10. Task 10 runs it only as stated there (own identifier, a port from 1424 up, never the owner's real profile, never ports 1420–1423).
- Not in this plan (spec §7.4 last row): `README.md`, `PRIVACY.md`, `SECURITY.md` and AGENTS.md's "No cloud sync" change when sync is released, not before. Do not touch them.

## Review Focus

1. **Flag off, or sync never turned on: backups exactly as today.** Rust: `backup(conn, today, None)` and a status in phase `off`/`settingUp` carrying a `last_complete_at` give the same milestone as before (Task 2 `without_sync_the_backup_reminder_is_unchanged`). TS: `syncIsOn(false, …)` is false for every phase, `gettingStartedItems` without the flag returns today's `makeBackup` item, `protectionLine(false, …)` is `null`, `keepSafeItems(false, …)` is empty, the upkeep rows and the notification plan without `syncOn` equal today's (Task 3 `safe-copy.test.ts`, `getting-started.test.ts`; Task 4 `upkeep.test.ts`, `notification-plan.test.ts`; Task 5; Task 8).
2. **What makes a sync "complete".** Only a round with no problem at all, with `sync_outbox` and `sync_unsent` empty and no `up` entry in `sync_blobs` that a row still names, stuck ones (refused, conflict, invalid ID) included. A round with only a gap, a refused row, a waiting or a refused upload records its success but not a complete sync; a failed round records neither (Task 1 tests in `engine/tests/complete.rs` and `engine/tests/blobs.rs`).
3. **The 30-day boundary on the newer of the two copies.** Fresh on the 30th whole UTC day, stale on the 31st, whichever of backup and complete sync is newer; a new profile with sync on and no complete round keeps its 14 days of grace; a stalled sync without a backup brings the reminder back at 30 days (Task 2 tests).
4. **Clock skew.** A complete sync or a backup dated in the future keeps the reminder quiet and never gives a negative age; `protectionLine` shows "just now" for it (Task 2 `a_safe_copy_in_the_future_counts_as_fresh`, Task 5 `protectionLine` test).
5. **"Sync protects your data" never shows beside a reminder.** `protectsData` is false while `sync_stalled` is raised, while sync is off, and once the last complete sync is 7 whole UTC days old; whenever it is true, `backup_stale` is absent (Task 2 `protection_and_the_backup_reminder_never_overlap`).
6. **The Kit-code rename is complete in all ten locales** and no key went missing: the three greps of Task 9 print nothing and `src/i18n/locales.test.ts` passes.

## File map

| Path | Responsibility | Task |
|---|---|---|
| `src-tauri/src/db/migrations.rs`, `db/schema.snapshot`, `services/sync/registry/keys.rs` | migration 039 `sync_state.last_complete_at` | 1 |
| `src-tauri/src/services/sync/state.rs`, `blobs/queue.rs`, `engine/mod.rs`, `engine/tests/complete.rs`, `engine/tests/blobs.rs` | record the last complete sync | 1 |
| `src-tauri/src/services/milestones/upkeep.rs`, `mod.rs` | newest safe copy, `sync_protects`, `backup_stale` with sync | 2 |
| `src-tauri/src/models/sync.rs`, `services/sync/engine/status.rs`, `shared/schema.ts`, `shared/generated-types.ts` | `lastCompleteAt`, `protectsData` on the wire | 2 |
| `src/utils/safe-copy.ts` (+ test) | `syncIsOn`, `protectionLine` | 3, 5 |
| `src/utils/getting-started.ts`, `src/mobile/lib/getting-started.ts` (+ tests), both `GettingStartedCard.tsx` | "Ochránit data" | 3 |
| `src/hooks/use-milestone-sub-line.ts`, `src/mobile/lib/upkeep.ts`, `src/mobile/lib/notification-plan.ts` (+ tests), `UpkeepCard.tsx`, `use-upkeep.ts`, `use-notification-schedule.ts`, `MilestoneSettingsCard.tsx`, `RemindersSection.tsx`, `NotificationsSection.tsx` | the reminder names both ways out | 4 |
| `src/components/settings/DataSection.tsx`, `src/mobile/pages/settings/BackupSection.tsx` | "Vaše data chrání synchronizace", backup screen wording | 5 |
| `src/components/auth/OnboardingWizard.tsx`, `src/mobile/pages/first-run/PrivacyStep.tsx`, `src/pages/auth-page.tsx` | where data goes, before unlock | 6 |
| `src/components/settings/DangerZoneCard.tsx`, `src/mobile/sheets/DeleteProfileSheet.tsx`, the two empty overviews | deleting and empty states with sync on | 7 |
| `src/utils/keep-safe.ts` (+ test), `src/components/settings/KeepSafeCard.tsx`, `SettingsSectionPages.tsx`, `src/mobile/pages/settings/SecuritySection.tsx` | "Co si uschovat" | 8 |
| `src/i18n/locales/*/common.json`, `*/deviceSync.json`, `docs/standards/i18n-glossary.md` | Kit-code rename, `keyRotated`, glossary | 9 |
| `docs/architecture/overview.md`, `docs/specs/2026-11-14-sync-aware-app-design.md` | as built, verification | 10 |

Every surface that asks for a backup today, and what part C turns it into:

| Surface | Today | Part C |
|---|---|---|
| `backup_stale` milestone (`upkeep.rs`): desktop agenda rows and folded summary, phone Údržba row, phone notification | last manual backup older than 30 days (14 days' grace) | newest safe copy older than 30 days (Task 2); with sync on the sub-line, the Údržba row, the notification and the reminder settings hints name both ways out (Task 4) |
| Getting started, desktop and phone (`makeBackup`) | "Udělat zálohu", done by a backup | flag on: "Ochránit data", done by a backup or by sync on, hint names both (Task 3) |
| Phone first run, last step | §5.2, built in parts A/B | unchanged; its lead stays, since the step only shows while sync is off (parts A/B skip it after a join) |
| Settings → Data a zálohy, phone Záloha a přenos | backup row, restore | sync on: the §7.3 line above the backup row while `protectsData`; phone lead, restore lead and OS-backup note name sync; desktop says what a backup made now leaves out (Task 5) |
| Delete data (desktop Danger zone `backupFirst`, phone delete sheet lead "Make a backup first") | single-device wording | sync on: says what happens to the synced data and the other devices (Task 7) |
| Milestone settings hints (`groups.backup.hint`, phone `notifications.backup.hint`) | "When the last backup is older than 30 days." | sync on: names the complete sync too (Task 4) |

---

### Task 1: Record the last complete sync (§7.2)

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const `MIGRATION_039`, the `all_migrations()` entry, `mod migration_039` test, the `known_migration_names().last()` assertion), `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `src-tauri/src/services/sync/registry/keys.rs` (`MIGRATION_CLASSES` gets `("039_sync_last_complete", A)`; `schema_number_is_38_…` test becomes `schema_number_is_39_and_the_newest_barrier_is_025`), `src-tauri/src/services/sync/chain/tests.rs` (the two `schema_seen, 38` assertions → 39)
- Modify: `src-tauri/src/services/sync/state.rs` (`EngineMarks.last_complete_at`, `marks()`, `record_complete`, test)
- Modify: `src-tauri/src/services/sync/blobs/queue.rs` (`holds_content_to_upload`), `src-tauri/src/services/sync/blobs/tests/queue.rs` (test)
- Modify: `src-tauri/src/services/sync/engine/mod.rs` (`run_round_into`, its doc step 6)
- Create: `src-tauri/src/services/sync/engine/tests/complete.rs`; Modify: `engine/tests/mod.rs` (`mod complete;`), `engine/tests/blobs.rs` (two tests)
- Modify: `docs/architecture/database.md` (migration list entry, `sync_state` catalogue row)

**Interfaces:**
- Produces (SQL): `MIGRATION_039 = "ALTER TABLE sync_state ADD COLUMN last_complete_at INTEGER;"` — Local table, nullable, no backfill (existing rows fill at their next clean round). Doc comment: `/// 039: sync_state.last_complete_at (Local): when a round last succeeded with nothing left to send here (spec 2026-11-14 §7.2); NULL before the first.`
- Produces (Rust, `state.rs`):
  ```rust
  pub struct EngineMarks { …, /// When a round last succeeded with nothing left to send (migration 039). pub last_complete_at: Option<i64> }
  /// A round succeeded and left nothing to send: stamp the time. Overwrites (never `MAX`), so a clock that was ahead is healed by the next complete round.
  pub fn record_complete(conn: &Connection, at: i64) -> Result<()>
  ```
- Produces (Rust, `blobs/queue.rs`):
  ```rust
  /// Whether any content held here still has to reach the sync server: an `up` entry that a row
  /// names (the same row join as `summary`, `references()`), whatever its `last_error`, the stuck
  /// ones (`STUCK`) included. Without the attachments store attached: `false` (nothing held here).
  pub fn holds_content_to_upload(conn: &Connection) -> Result<bool>
  ```
- Behaviour (`engine/mod.rs`, `run_round_into`, the closure that records the outcome): `None => { state::record_success(conn, now)?; if status::unsent_changes(conn)? == 0 && !queue::holds_content_to_upload(conn)? { state::record_complete(conn, now)?; } Ok(()) }`; the `Gap`/`ClockAhead` success arm and every error arm leave `last_complete_at` alone. Add to the doc's step 6: "A round with no problem that leaves no change to send (`sync_outbox`, `sync_unsent`) and no document or photo to upload, stuck ones included (`queue::holds_content_to_upload`), also records a complete sync (`state::record_complete`, spec 2026-11-14 §7.2)." `state::disable` already deletes the row, so turning sync off forgets the time.

- [ ] **Step 1: Write the failing tests**

`migrations.rs`, `mod migration_039` (copy `migration_038`'s shape): `sync_state_takes_an_empty_last_complete_at` — chain to the migration before 039, insert a `sync_state` row as `migration_033`'s test does, run the full chain, assert `last_complete_at` is `NULL` and can be updated.

`state.rs` tests:
```rust
#[test]
fn record_complete_stamps_the_time_and_a_later_one_overwrites_it() {
    // proves: marks() reads the column; a corrected clock heals a time from the future (SET, not MAX)
    let conn = /* the enabled-row helper the neighbouring record_success test uses */;
    record_complete(&conn, 77).unwrap();
    assert_eq!(marks(&conn).unwrap().unwrap().last_complete_at, Some(77));
    record_complete(&conn, 50).unwrap();
    assert_eq!(marks(&conn).unwrap().unwrap().last_complete_at, Some(50));
}
```

`blobs/tests/queue.rs` (insert entries with the helpers the file's other tests use):
- `content_to_upload_counts_stuck_entries_and_ignores_the_rest` — // proves: only `up` entries a row names count, stuck ones too. No entries → false; a `down` and a `done` entry named by a row → false; an `up` entry no row names → false; an `up` entry named by a row → true; the same entry with `last_error = 'refused'` → true.

`engine/tests/complete.rs` (`use super::*;` and the round helper pattern of `echo.rs`):
- `a_clean_round_with_nothing_left_records_a_complete_sync` — // proves the happy path. `vault()`, `institution(&vault.a.lock(), "a1")`, one `Normal` round at `NOW` → `marks.last_complete_at == Some(NOW)` and equals `last_success_at`.
- `a_round_that_leaves_a_refused_row_is_not_complete` — // proves a row too large to send (it stays in the outbox) keeps the sync incomplete. Set up as `push.rs`'s `an_oversized_row_is_refused_and_the_loop_still_ends`, run one round → `last_success_at == Some(NOW)`, `last_complete_at == None`.
- `a_round_that_holds_a_gap_records_success_but_not_complete` — // proves a waiting gap (`sync_stalled` raises it at once) is not a complete sync. Set up as `gaps.rs`'s `a_withheld_envelope_holds_the_cursor_while_others_sync_and_pushes_go_on` → the round's problem is `Gap`, `last_success_at` set, `last_complete_at == None`.
- `a_failed_round_records_neither` — // proves an unreachable server leaves both times alone. A clean round at `NOW`, then the fake relay failing every route (`set_failing` as in `relay_faults.rs`) and a round at `NOW + 600` → both times still `NOW`.

`engine/tests/blobs.rs` (its `owner`, `document`, `with_store`, `round_on`, `round_at` helpers):
- `a_document_waiting_for_the_lane_keeps_the_sync_incomplete_until_it_is_up` — // proves a waiting upload blocks "complete" and the next round after the upload records it. As `a_large_blob_waits_for_the_lane` → `last_complete_at == None`; then run the upload lane once (`blobs::transfer::run_uploads` as `lanes.rs` calls it) and `round_at(…, NOW + 300)` → `Some(NOW + 300)`.
- `a_refused_upload_keeps_the_sync_incomplete` — // proves a stuck upload (content on this device only) is not complete although the wire counts it as `failed`. As `a_refused_upload_is_a_failed_item_and_the_round_succeeds` → `last_success_at == Some(NOW)`, `last_complete_at == None`.
- `a_small_document_sent_before_the_push_leaves_the_round_complete` — // proves the pre-push upload counts. As `small_blobs_go_up_before_the_push` → `last_complete_at == Some(NOW)`.

- [ ] **Step 2: Run them and see them fail** — `cd src-tauri && cargo test --lib -- last_complete record_complete content_to_upload engine::tests::complete a_document_waiting_for_the_lane a_refused_upload_keeps a_small_document_sent`; Expected: FAIL (column, `record_complete`, `holds_content_to_upload` not found).

- [ ] **Step 3: Implement** the migration (playbook steps 1–8: const, `Vec` entry, schema snapshot via `cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored` — the diff must show only the new column —, `database.md`, class `A`, the schema-number tests found with `grep -rn "schema_number(), 38\|schema_seen, 38\|\"038_sync_blobs_size\"" src-tauri/src`), `record_complete`, `marks()`, `holds_content_to_upload`, the round.

- [ ] **Step 4: Gates** — Rust and TS gates; Expected: all pass.

- [ ] **Step 5: Commit** — `feat(sync): record the last complete sync`

---

### Task 2: The newest safe copy and the verdict on the wire (§7.1, §7.2, §7.3 verdict)

**Files:**
- Modify: `src-tauri/src/services/milestones/upkeep.rs` (module doc, `newest_safe_copy`, `sync_protects`, `backup`, `build`, tests)
- Modify: `src-tauri/src/services/milestones/mod.rs` (builder type, the upkeep builder gets `sync`, `pub(crate) use upkeep::sync_protects;`, one list test, the `SyncStatus` literal in its tests)
- Modify: `src-tauri/src/models/sync.rs` (`SyncStatus`), `src-tauri/src/services/sync/engine/status.rs` (`status`, `quiet`, test), `src-tauri/src/services/sync/engine/tests/runtime.rs` (its `SyncStatus` literal)
- Regenerate: `shared/generated-types.ts`; Modify: `shared/schema.ts` (`SyncStatus`)
- Modify: every TS test fixture that builds a `SyncStatus` (find them: `grep -rln "enabledAt" src --include="*.test.ts"`; today `src/utils/sync-state.test.ts`, after Task 18 also `src/utils/sync-attachments.test.ts`)

**Interfaces:**
- Produces (Rust model, after `attachments`):
  ```rust
  /// When a round last succeeded with nothing left to send here: no change, no document or photo
  /// waiting to upload or unable to (unix seconds; spec 2026-11-14 §7.2). `None` while off or before the first.
  #[serde(rename = "lastCompleteAt")]
  pub last_complete_at: Option<i64>,
  /// Sync keeps this device's data safe now (spec §7.3, `milestones::sync_protects`): on, nothing
  /// needs the person, and a complete sync less than 7 whole UTC days old.
  #[serde(rename = "protectsData")]
  pub protects_data: bool,
  ```
- Produces (TS, `shared/schema.ts`, in `SyncStatus` after `attachments`):
  ```ts
  /** When a round last succeeded with nothing left to send: no change, no document or photo waiting to upload or unable to (unix seconds); `null` while off or before the first. */
  lastCompleteAt: number | null;
  /** Sync keeps this device's data safe now: on, nothing needs the person, a complete sync less than 7 whole UTC days old (Rust `sync_protects`). */
  protectsData: boolean;
  ```
- Produces (Rust, `upkeep.rs`):
  ```rust
  /// Sync is on for the profile: any phase but `Off` and `SettingUp`.
  fn sync_on(sync: &SyncStatus) -> bool
  /// The newest safe copy (spec §7.1): the later of the last manual backup and, while sync is on,
  /// the last complete sync. A time in the future is returned as it is; callers compare whole UTC
  /// days, so it counts as fresh, never as a negative age.
  pub(super) fn newest_safe_copy(last_backup: Option<i64>, sync: Option<&SyncStatus>) -> Option<i64>
  /// Sync protects this device's data now (spec §7.3): on, `sync_stalled` quiet, and a complete
  /// sync on one of the last `SYNC_STALE_DAYS` UTC days (`day_floor(at) > today - SYNC_STALE_DAYS * DAY`).
  pub(crate) fn sync_protects(today: i64, sync: &SyncStatus) -> bool
  /// `backup_stale` when the newest safe copy is older than 30 whole UTC days; with neither copy,
  /// after the profile's 14 days of grace. Key `backup_stale:<day of the newest safe copy>` or
  /// `backup_stale:never`; `since_day` = that day. `sync: None` (flag off, or the status could not
  /// be read) is today's rule exactly.
  pub(super) fn backup(conn: &Connection, today: i64, sync: Option<&SyncStatus>) -> Result<Option<Milestone>>
  pub(super) fn build(conn: &Connection, today: i64, sync: Option<&SyncStatus>) -> Result<Vec<Milestone>>
  ```
  `backup` keeps today's body; only `last` becomes `newest_safe_copy(last_backup, sync)`.
- `mod.rs`: `type Builder<'a> = &'a dyn Fn(&Connection, i64) -> Result<Vec<Milestone>>;` the `"upkeep"` entry is `&|c, t| upkeep::build(c, t, sync)`, in the same position (the list order of existing tests must not change). `get_milestones` is unchanged: it already passes the status only with the flag on.
- `status.rs`: `quiet` sets `last_complete_at: None, protects_data: false`; an enabled profile sets `last_complete_at: marks.last_complete_at`, builds the status with `protects_data: false`, then `shown.protects_data = crate::services::milestones::sync_protects(today_utc_day(), &shown)` (`crate::services::loan_amortization::today_utc_day`).

- [ ] **Step 1: Write the failing Rust tests** in `upkeep.rs` (extend the `sync(…)` helper with a `complete(phase, problem, last_complete) -> SyncStatus` that sets `last_success_at` and `last_complete_at` to `last_complete`; existing `build(&conn, today)` calls become `build(&conn, today, None)`; `today = day(2026, 10, 5)` unless stated):
  - `without_sync_the_backup_reminder_is_unchanged` — // Review Focus 1. The two existing backup tests' cases give the same result through `backup(&conn, today, None)` and through `Some(&complete(Off | SettingUp, None, Some(today)))`.
  - `a_complete_sync_counts_as_a_safe_copy` — backup at `day(2026, 8, 24)`; `complete(Idle, None, Some(day(2026, 10, 1)))` → `None`; with the complete sync at `day(2026, 9, 4)` → `Some`, `since_day == Some(day(2026, 9, 4))`, key `backup_stale:{day(2026, 9, 4)}`.
  - `the_newer_of_backup_and_sync_wins` — backup `day(2026, 9, 20)`, complete `day(2026, 8, 1)` → `None`; backup `day(2026, 8, 1)`, complete `day(2026, 8, 2) + 60` → `Some` with `since_day == Some(day(2026, 8, 2))`.
  - `the_thirtieth_utc_day_is_fresh_the_thirty_first_is_not` — // Review Focus 3. No backup, profile created 2026-01-01; complete at `today - 30 * DAY + 82_800` → `None`; at `today - 31 * DAY + 82_800` → `Some`.
  - `a_new_profile_with_sync_but_no_complete_round_keeps_its_grace` — profile created `day(2026, 9, 25)`, `complete(Idle, None, None)` with `last_success_at = Some(day(2026, 10, 4))` → `None` on `day(2026, 10, 5)`, `Some` with key `backup_stale:never` on `day(2026, 10, 9)`.
  - `a_stalled_sync_brings_the_backup_reminder_back_after_thirty_days` — // spec §7.1 consequences. `complete(Paused, Some(Offline), Some(today - 35 * DAY))`, no backup, old profile → `backup` is `Some` and `sync_stalled(today, …)` is `Some`.
  - `a_safe_copy_in_the_future_counts_as_fresh` — // Review Focus 4. Complete at `today + 40 * DAY` → `None`; backup at `today + 40 * DAY`, no sync → `None`.
  - `sync_protects_only_while_complete_within_a_week_and_nothing_needs_the_person` — table (`today = day(2026, 11, 14)`): `Idle`/none/complete `today` → true; `Idle`/none/`today - 6 * DAY` → true; `Idle`/none/`today - 7 * DAY` → false; `Paused`/`Offline`/`today - 2 * DAY` → true; `Error`/`Unauthorized`/`today` → false; `Waiting`/`Gap`/`today` → false; `Idle`/none/`None` → false; `Off` and `SettingUp`/`today` → false; `Idle`/none/`today + 5 * DAY` → true.
  - `protection_and_the_backup_reminder_never_overlap` — // Review Focus 5. For every `back` in 0..=40 days and phases `Idle`, `Paused`+`Offline`, `Error`+`Unauthorized`, no backup, old profile: `!(sync_protects(today, &s) && backup(&conn, today, Some(&s)).unwrap().is_some())`.
  In `mod.rs` tests: `the_backup_reminder_reads_the_sync_status` — old profile, no backup: `list_milestones(&conn, today, Some(&healthy))` (complete `today`) has no `backup_stale`; `Some(&stalled)` (complete 40 days ago, `Paused`/`Offline`) has one; muting `backup_stale` hides it. In `status.rs` tests: `the_status_carries_the_last_complete_sync_and_its_verdict` — `enabled_at(…)` profile: no complete sync → `last_complete_at == None`, `!protects_data`; after `state::record_complete(&conn, now)` with `now` = the current unix time → `Some(now)`, `protects_data`; `a_profile_with_sync_off_and_no_account_names_no_server` also asserts `last_complete_at == None` and `!protects_data`.

- [ ] **Step 2: Run them and see them fail** — `cd src-tauri && cargo test --lib -- services::milestones services::sync::engine::status`; Expected: FAIL (fields and functions not found).

- [ ] **Step 3: Implement** the functions, the model fields, `status()`, `mod.rs`; update every `SyncStatus` literal (`grep -rn "SyncStatus {" src-tauri/src`).

- [ ] **Step 4: Regenerate and mirror** — `cd src-tauri && cargo test generate_bindings -- --ignored`; add the two fields to `shared/schema.ts`; add `lastCompleteAt: null, protectsData: false` to every TS fixture found by the grep above.

- [ ] **Step 5: Gates** — Rust and TS gates; Expected: all pass; `git diff --stat shared/generated-types.ts` shows only the two fields.

- [ ] **Step 6: Commit** — `feat(milestones): the backup reminder follows the newest safe copy`

---

### Task 3: "Sync is on" in one place, and Getting started asks to protect the data (§7.3)

**Files:**
- Create: `src/utils/safe-copy.ts`, `src/utils/safe-copy.test.ts`
- Modify: `src/utils/getting-started.ts` (+ `getting-started.test.ts`), `src/mobile/lib/getting-started.ts` (+ `getting-started.test.ts`)
- Modify: `src/components/dashboard/GettingStartedCard.tsx`, `src/mobile/components/GettingStartedCard.tsx`
- Modify: `src/components/settings/DangerZoneCard.tsx`, `src/hooks/use-device-sync.ts` (`useSyncDevices`): their own phase checks become `syncIsOn`
- Modify: `src/i18n/locales/*/dashboard.json`, `*/mobile.json`

**Interfaces:**
- Produces (`src/utils/safe-copy.ts`):
  ```ts
  /** Sync is on for this profile in a build that runs it: the flag is on and the phase is neither `off` nor `settingUp`. Everything part C changes for "sync on" reads as today without it. */
  export function syncIsOn(featureOn: boolean, status: SyncStatus | undefined): boolean;
  ```
  (`useSyncDevices` passes `true` for `featureOn`, since its query is already gated by the flag.)
- Produces (`src/utils/getting-started.ts`):
  ```ts
  export type GettingStartedKey = 'importStatement' | 'addInvestment' | 'setBudget' | 'makeBackup' | 'protectData';
  /** With the sync flag on, the last core item is "Ochránit data", done by a backup or by sync on (spec §7.3); without it today's "Udělat zálohu". */
  export interface GettingStartedSync { featureOn: boolean; syncOn: boolean }
  export function gettingStartedItems(progress: Progress, prefs: MenuPreferences | undefined, sync?: GettingStartedSync): GettingStartedItem[];
  ```
  `sync` defaults to `{ featureOn: false, syncOn: false }`. With `featureOn`: `{ key: 'protectData', done: progress.lastBackupAt !== null || sync.syncOn, href: '/settings/data', optional: false }` in place of `makeBackup`.
- Produces (`src/mobile/lib/getting-started.ts`): `phoneGettingStartedItems(progress, prefs, canBackup = true, sync?: GettingStartedSync)` — passes `sync` on; the `canBackup` filter drops `protectData` as it drops `makeBackup`; `phoneHref('protectData')` → `settingsPath('backup')`.
- Components: both cards compute `sync = { featureOn: useDeviceSyncFeature(), syncOn: syncIsOn(featureOn, useDeviceSyncStatus().data) }`; desktop `ICONS.protectData = ShieldCheck`; labels `gettingStarted.protectData` / hint `gettingStarted.protectDataHint` (desktop `dashboard`, phone hint `mobile`, as the card reads `makeBackupHint` today).

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `dashboard` `gettingStarted.protectData` | Ochránit data | Protect your data |
| `dashboard` `gettingStarted.protectDataHint` | Udělejte zálohu v Nastavení → Data a zálohy, nebo zapněte synchronizaci. (plan's wording) | Make a backup in Settings → Data and backups, or turn on sync. (plan's wording) |
| `mobile` `gettingStarted.protectDataHint` | Udělejte zálohu v Nastavení → Záloha a přenos, nebo zapněte synchronizaci. (plan's wording) | Make a backup in Settings → Backup and transfer, or turn on sync. (plan's wording) |

- [ ] **Step 1: Write the failing tests**
  - `safe-copy.test.ts`: `it('is on only with the flag and a phase past setup')` — table over `featureOn` × phases: `(false, every phase)` → false; `(true, undefined)` → false; `(true, 'off' | 'settingUp')` → false; `(true, 'idle' | 'syncing' | 'waiting' | 'paused' | 'error')` → true. // Review Focus 1
  - `getting-started.test.ts`: `it('keeps "Make a backup" without the sync flag')` — no `sync` argument, and `{ featureOn: false, syncOn: true }` → last item key `makeBackup`, `done` false with `lastBackupAt: null` (// Review Focus 1); `it('asks to protect the data with the flag on')` — `{ featureOn: true, syncOn: false }` → key `protectData`, href `/settings/data`, `done` false; `syncOn: true` → `done` true; `lastBackupAt: 1` → `done` true.
  - `src/mobile/lib/getting-started.test.ts`: `it('leads "Protect your data" to Záloha a přenos and drops it where backups cannot be made')` — href `settingsPath('backup')`; with `canBackup: false` no `protectData` item.
  Run `npx vitest run src/utils/safe-copy.test.ts src/utils/getting-started.test.ts src/mobile/lib/getting-started.test.ts`; Expected: FAIL.
- [ ] **Step 2: Implement** the module, the two item builders, the two cards, the two predicate swaps (behaviour unchanged there), the copy in ten locales.
- [ ] **Step 3: Gates** — TS gates; Expected: all pass; `grep -n "settingUp\|'idle', 'syncing'" src/components/settings/DangerZoneCard.tsx src/hooks/use-device-sync.ts` prints nothing (the status mapping in `src/utils/sync-state.ts` keeps its own checks).
- [ ] **Step 4: Commit** — `feat(onboarding): Getting started asks to protect the data`

---

### Task 4: With sync on, the backup reminder names both ways out (§7.1)

**Files:**
- Modify: `src/hooks/use-milestone-sub-line.ts`
- Modify: `src/mobile/lib/upkeep.ts` (+ `upkeep.test.ts`), `src/mobile/hooks/use-upkeep.ts`, `src/mobile/components/UpkeepCard.tsx`
- Modify: `src/mobile/lib/notification-plan.ts` (+ `notification-plan.test.ts`), `src/mobile/hooks/use-notification-schedule.ts`
- Modify: `src/components/settings/MilestoneSettingsCard.tsx`, `src/mobile/pages/settings/RemindersSection.tsx`, `src/mobile/pages/settings/NotificationsSection.tsx`
- Modify: `src/i18n/locales/*/milestones.json`, `*/mobile.json`

**Interfaces:**
- Consumes: `syncIsOn` (Task 3); the `backup_stale` milestone of Task 2 (its `sinceDay` is the newest safe copy's day).
- `useMilestoneSubLine`: reads `syncIsOn(useDeviceSyncFeature(), useDeviceSyncStatus().data)`; `backup_stale` → with sync on `sub.backup_never_sync` / `sub.backup_stale_sync` (`date: day(m.sinceDay)`), otherwise today's keys. Title, link (`/settings/data`) and `short.backup_stale` stay.
- Produces (`src/mobile/lib/upkeep.ts`):
  ```ts
  export interface UpkeepRow { …; /** A second line the row brings itself (the backup row with sync on). */ sub?: UpkeepTitle }
  export interface UpkeepInput { …; /** Sync is on (`syncIsOn`): the backup row names the newest safe copy and both ways out. Default false. */ syncOn?: boolean }
  ```
  Backup row with `syncOn`: title `upkeep.safeCopyNever` (no `sinceDay`) or `upkeep.safeCopyStale` with `{ count }` as today's; `sub: { key: 'upkeep.bothWaysOut', params: {} }`. Without it: unchanged, no `sub`. `UpkeepCard`'s `default` branch renders `row.sub` when present (`t(row.sub.key, row.sub.params)`). `use-upkeep.ts` passes `syncOn`.
- Produces (`src/mobile/lib/notification-plan.ts`): `NotificationPlanInput.syncOn?: boolean` (default false). With it the backup item is `id: notificationId('backup', 'backupSync')` (a new id, so `planSignature` changes and a pending reminder is re-planned with the new text), `title: notifications.backup.titleSync`, body `notifications.backup.staleSync` / `neverSync`; `at`, `path` and the conditions unchanged. `use-notification-schedule.ts` passes `syncOn`.
- Hints: `MilestoneSettingsCard` and `RemindersSection` show `groups.backup.hintSync` for the `backup` group while sync is on; `NotificationsSection` shows `notifications.backup.hintSync` for its backup row (unless the permission line replaces it, as today).

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `milestones` `sub.backup_stale_sync` | Poslední bezpečná kopie {{date}}: obnovte synchronizaci, nebo zálohujte (plan's wording) | Last safe copy {{date}}: restore sync or make a backup (plan's wording) |
| `milestones` `sub.backup_never_sync` | Zatím žádná bezpečná kopie: obnovte synchronizaci, nebo zálohujte (plan's wording) | No safe copy yet: restore sync or make a backup (plan's wording) |
| `milestones` `groups.backup.hintSync` | Když není záloha ani úplná synchronizace novější než 30 dní. (plan's wording) | When neither a backup nor a complete sync is newer than 30 days. (plan's wording) |
| `mobile` `upkeep.safeCopyStale_one` / `_few` / `_other` | Poslední bezpečná kopie před {{count}} dnem / Poslední bezpečná kopie před {{count}} dny / Poslední bezpečná kopie před {{count}} dny | `_one` Last safe copy {{count}} day ago / `_other` Last safe copy {{count}} days ago |
| `mobile` `upkeep.safeCopyNever` | Zatím žádná bezpečná kopie | No safe copy yet |
| `mobile` `upkeep.bothWaysOut` | Obnovte synchronizaci, nebo zálohujte | Restore sync or make a backup |
| `mobile` `notifications.backup.titleSync` | Ochraňte data v Moony | Protect your Moony data |
| `mobile` `notifications.backup.staleSync` | Poslední bezpečná kopie je starší než měsíc. Obnovte synchronizaci, nebo zálohujte. | Your last safe copy is over a month old. Restore sync or make a backup. |
| `mobile` `notifications.backup.neverSync` | Zatím nemáte žádnou bezpečnou kopii. Obnovte synchronizaci, nebo zálohujte. | You have no safe copy yet. Restore sync or make a backup. |
| `mobile` `notifications.backup.hintSync` | Když není záloha ani úplná synchronizace novější než 30 dní. | When neither a backup nor a complete sync is newer than 30 days. |

All `mobile` rows are the plan's wording (the spec gives none).

- [ ] **Step 1: Write the failing tests**
  - `upkeep.test.ts`, `describe('backup_stale with sync on')`: `it('names the newest safe copy and both ways out')` — `syncOn: true`, `milestone('backup_stale', { sinceDay: ago(41) })` → title `{ key: 'upkeep.safeCopyStale', params: { count: 41 } }`, `sub` `{ key: 'upkeep.bothWaysOut', params: {} }`; without `sinceDay` → `upkeep.safeCopyNever`; `it('reads as today without sync')` — `syncOn` omitted and `false` → `upkeep.backupStale`, `sub` undefined (// Review Focus 1); `it('still has no backup row where backups cannot be made')` — `syncOn: true, canBackup: false` → no row.
  - `notification-plan.test.ts`: `it('names sync in the backup reminder under its own id while sync is on')` — `syncOn: true` → one backup item with `id === notificationId('backup', 'backupSync')`, title `notifications.backup.titleSync`, body `notifications.backup.staleSync` (and `neverSync` for `sinceDay: null`); the existing backup test stays as it is (// Review Focus 1).
  Run `npx vitest run src/mobile/lib/upkeep.test.ts src/mobile/lib/notification-plan.test.ts`; Expected: FAIL.
- [ ] **Step 2: Implement** the hook branch, the row, the card, the plan, the two hooks' inputs, the three hints, the copy in ten locales.
- [ ] **Step 3: Gates** — TS gates; Expected: all pass.
- [ ] **Step 4: Commit** — `feat(milestones): with sync on the backup reminder names both ways out`

---

### Task 5: The backup screens say when sync protects the data (§7.3, §7.4 rows 2–3, attachments §5.7)

**Files:**
- Modify: `src/utils/safe-copy.ts` (+ test)
- Modify: `src/components/settings/DataSection.tsx`
- Modify: `src/mobile/pages/settings/BackupSection.tsx` (Task 18 put a `notInBackup` line here: `grep -n "notInBackup\|backup.notHere" src/mobile/pages/settings/BackupSection.tsx`; leave it where it is)
- Modify: `src/i18n/locales/*/deviceSync.json`, `*/mobile.json`, `*/settings.json`

**Interfaces:**
- Consumes: `SyncStatus.protectsData`, `lastCompleteAt` (Task 2), `syncIsOn` (Task 3), `syncAge` and the `deviceSync` `age.*` keys (`src/utils/sync-state.ts`, parts A/B), `useSyncDevices`, `notInBackup` (`src/utils/sync-attachments.ts`, attachments Task 18).
- Produces (`src/utils/safe-copy.ts`):
  ```ts
  export interface ProtectionLine { devices: number; age: { key: SyncAgeKey; count: number } }
  /** The §7.3 line: only with the flag on, `protectsData` and a `lastCompleteAt`, once the device list is in; `devices` = its length (at least 1), `age` = `syncAge(lastCompleteAt, now)`. `null` otherwise. */
  export function protectionLine(featureOn: boolean, status: SyncStatus | undefined, devices: readonly SyncDevice[] | undefined, now: number): ProtectionLine | null;
  ```
- Desktop `DataSection`: when `protectionLine(…)` is non-null, a paragraph (`text-caption text-ink-3`, inside the card, directly above the `data.backupRow` row) `tSync('protection.line', { count: line.devices, age: tSync(\`age.${line.age.key}\`, { count: line.age.count }) })`. While sync is on and `notInBackup(status.attachments) > 0`, the backup row's hint gets a second line `data.notInBackup` (`count`). `now` is read at render (`Date.now() / 1000`), no timer.
- Phone `BackupSection`: the same line (`text-m-sub text-ink-3`) above the "Poslední záloha" card; while `syncIsOn`: the page lead `backup.leadSync`, the restore lead `backup.restoreLeadSync`, the closing note `backup.osBackupNoteSync`; otherwise today's keys.

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `deviceSync` `protection.line_one` / `_few` / `_other` | (all three) Vaše data chrání synchronizace (naposledy {{age}}, {{count}} zařízení). Zálohu si udělejte, když chcete uchovat stav k určitému dni. | `_one` Sync protects your data (last complete sync {{age}}, {{count}} device). Make a backup when you want to keep how things stood on a given day. / `_other` … ({{count}} devices) … |
| `settings` `data.notInBackup_one` / `_few` / `_other` | {{count}} dokument nebo fotka zatím není v tomto počítači, záloha vytvořená teď ji proto nebude obsahovat. / {{count}} dokumenty a fotky zatím nejsou v tomto počítači, záloha vytvořená teď je proto nebude obsahovat. / {{count}} dokumentů a fotek zatím není v tomto počítači, záloha vytvořená teď je proto nebude obsahovat. | `_one` {{count}} document or photo is not on this computer yet, so a backup made now leaves it out. / `_other` {{count}} documents and photos are not on this computer yet, so a backup made now leaves them out. |
| `mobile` `backup.leadSync` | Synchronizace drží vaše zařízení ve shodě. Záloha je jeden šifrovaný soubor pro jednorázový přenos nebo kopii k uschování: databáze, klíče a dokumenty a fotky v tomto telefonu. Otevře ji jen vaše heslo. | Sync keeps your devices in step. A backup is one encrypted file for a one-off move or a copy to keep: the database, the keys and the documents and photos on this phone. Only your password opens it. |
| `mobile` `backup.restoreLeadSync` | Obnovte zálohu z tohoto telefonu, nebo jednorázově přeneste profil z Moony na počítači: vyberte soubor zálohy a po obnovení odemkněte heslem, které platilo při jejím vytvoření. Zařízení ve shodě drží synchronizace. | Restore a backup from this phone, or move a profile once from Moony on the computer: choose the backup file and, after the restore, unlock with the password that was in use when it was created. Sync keeps your devices in step. |
| `mobile` `backup.osBackupNoteSync` | Profil se neukládá do zálohy telefonu (iCloud ani počítač). Mimo telefon jsou vaše data na synchronizačním serveru a na dalších zařízeních a v zálohách Moony, které si sami uložíte. | The profile is not saved in the phone's own backup (iCloud or a computer). Outside this phone, your data is on the sync server and your other devices, and in any Moony backup you save yourself. |

The `protection.line` cs text is the spec's with "dnes 14:02" rendered as the status age (`naposledy před 4 min`; plan's wording); the en text, `data.notInBackup` and the three `mobile` texts are the plan's wording.

- [ ] **Step 1: Write the failing test** in `safe-copy.test.ts`, `describe('protectionLine')` (`now = 1_800_000_000`; a status helper with `protectsData`, `lastCompleteAt`): `it('is null without the flag, without the verdict or before the devices are in')` — `featureOn: false` with `protectsData: true` → null (// Review Focus 1); `protectsData: false` → null; `lastCompleteAt: null` → null; `devices: undefined` → null. `it('counts the devices and gives the age')` — three devices, `lastCompleteAt: now - 240` → `{ devices: 3, age: { key: 'minutes', count: 4 } }`; empty list → `devices: 1`. `it('shows a complete sync in the future as just now')` — `lastCompleteAt: now + 3_600` → `age: { key: 'justNow', count: 0 }` (// Review Focus 4).
  Run `npx vitest run src/utils/safe-copy.test.ts`; Expected: FAIL.
- [ ] **Step 2: Implement** `protectionLine`, the two screens, the copy in ten locales.
- [ ] **Step 3: Gates** — TS gates; Expected: all pass.
- [ ] **Step 4: Commit** — `feat(sync): backup screens say when sync protects the data`

---

### Task 6: Where data goes, before unlock and in the first run (§7.4 rows 1 and 4)

**Files:**
- Modify: `src/components/auth/OnboardingWizard.tsx` (the welcome step's `localOnly` point), `src/mobile/pages/first-run/PrivacyStep.tsx` (its `localOnly` point), `src/pages/auth-page.tsx` (`lock.meta*`, `login.lead`)
- Modify: `src/i18n/locales/*/auth.json`, `*/mobile.json`, `*/deviceSync.json`

**Interfaces:**
- Consumes: `useDeviceSyncFeature()` (answers before unlock; the sync state of a locked profile is unknown, so these follow the flag).
- Flag on: the desktop welcome point reads `onboarding.welcome.points.localOnly.textSync`, the phone's `firstRun.points.localOnly.textSync`, the lock screen `lock.metaSync` / `lock.metaNoVersionSync`, the login lead `login.leadSync`; flag off: today's keys. The point's title "Bez cloudu" / "No cloud" stays (the sync server is the person's own).
- `deviceSync` `intro.description` changes its value in all ten locales (it shows only in Settings → Sync, which needs the flag). If Task 18 already reworded it (`git log -p -S "intro.description" -- src/i18n/locales/en/deviceSync.json`), keep its wording when it already names documents and photos and skip this row.
- No change: `firstRun.backupLead` (spec §7.4 row 3) — the Záloha step shows only while sync is off (parts A/B leave it out after a join), so its text stays true.

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `auth` `onboarding.welcome.points.localOnly.textSync` | – data opustí tento počítač, jen když zapnete synchronizaci, a pak jen koncově šifrovaná na synchronizační server, který si zvolíte. Zálohu si uděláte sami jako šifrovaný ZIP v Nastavení. | – your data leaves this computer only if you turn on sync, and then end-to-end encrypted to a sync server you choose. You make backups yourself, as an encrypted ZIP in Settings. |
| `mobile` `firstRun.points.localOnly.textSync` | – data opustí tento telefon, jen když zapnete synchronizaci, a pak jen koncově šifrovaná na synchronizační server, který si zvolíte; zálohu si uložíte do Souborů nebo iCloud Drive sami. | – your data leaves this phone only if you turn on sync, and then end-to-end encrypted to a sync server you choose; you save the backup to Files or iCloud Drive yourself. |
| `deviceSync` `intro.description` (changed) | Udržujte data včetně dokumentů a fotek synchronizovaná na všech svých zařízeních. Jsou koncově šifrovaná: synchronizační server ukládá jen data, která nepřečte. | Keep your data, documents and photos included, in step on all your devices. It is end-to-end encrypted: the sync server only stores data it cannot read. |
| `auth` `lock.metaSync` | Moony {{version}} · AES-256 (SQLCipher) · data jsou šifrovaná na každém zařízení | Moony {{version}} · AES-256 (SQLCipher) · your data is encrypted on every device |
| `auth` `lock.metaNoVersionSync` | Moony · AES-256 (SQLCipher) · data jsou šifrovaná na každém zařízení | Moony · AES-256 (SQLCipher) · your data is encrypted on every device |
| `auth` `login.leadSync` | Zadejte heslo a databáze se dešifruje v tomto počítači. Něco odesílá jen synchronizace, pokud ji máte zapnutou, a to jen koncově šifrované. | Enter your password and the database is decrypted on this computer. Only sync, if you turned it on, sends anything, and only end-to-end encrypted. |

All rows are the plan's wording (the spec states the meaning, not the text).

- [ ] **Step 1: Implement** the four switches and the copy in ten locales (no new pure logic).
- [ ] **Step 2: Gates** — TS gates; Expected: all pass.
- [ ] **Step 3: Commit** — `feat(sync): the first run, the lock screen and the sync intro say where data goes`

---

### Task 7: Deleting and empty overviews with sync on (§7.4 row 5)

**Files:**
- Modify: `src/components/settings/DangerZoneCard.tsx`, `src/mobile/sheets/DeleteProfileSheet.tsx`
- Modify: the desktop empty overview that renders `empty.description` and the phone one that renders `overview.emptyDescription` (find them: `grep -rn "empty.description'\|overview.emptyDescription" src/components src/pages src/mobile`)
- Modify: `src/i18n/locales/*/settings.json`, `*/mobile.json`, `*/dashboard.json`

**Interfaces:**
- Consumes: `syncIsOn` (Task 3).
- Desktop Danger zone (the confirm dialog already says what happens with sync on: `confirmDescriptionSynced`, `syncDeleted`): the row hint reads `dangerZone.deleteAllHintSynced` while sync is on; `dangerZone.syncDeleted` changes its value in all ten locales.
- Phone delete sheet: description `settings.delete.leadSynced` while sync is on.
- Empty overviews: `empty.descriptionSync` / `overview.emptyDescriptionSync` while sync is on.

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `settings` `dangerZone.deleteAllHintSynced` | Odstraní databázi, soubory s klíči a přílohy na tomto počítači i synchronizovaná data na synchronizačním serveru: ostatní zařízení se přestanou synchronizovat a data, která mají, si ponechají. Kopie vedle databáze (moony.db.bak-…, pre-restore-…) zůstanou v datové složce (Data a zálohy → Otevřít složku), stejně jako zálohy uložené mimo aplikaci. | Removes the database, the key files and the attachments on this computer, and the synced data on the sync server: your other devices stop syncing and keep the data they hold. Copies next to the database (moony.db.bak-…, pre-restore-…) stay in the data folder (Data and backups → Open folder), and so do backups saved outside the app. |
| `settings` `dangerZone.syncDeleted` (changed) | Smažou se i synchronizovaná data na synchronizačním serveru: vaše ostatní zařízení se přestanou synchronizovat a data, která mají, si ponechají. | Synced data on the sync server is deleted too: your other devices stop syncing and keep the data they hold. |
| `mobile` `settings.delete.leadSynced` | Trvale smaže databázi, klíče a přílohy z tohoto telefonu i synchronizovaná data na synchronizačním serveru: ostatní zařízení se přestanou synchronizovat a data, která mají, si ponechají. Kopie zálohy mimo telefon zůstanou. Nejdřív si udělejte zálohu — pro tento telefon je to jediná cesta zpět. | Permanently deletes the database, the keys and the attachments from this phone, and the synced data on the sync server: your other devices stop syncing and keep the data they hold. Backup copies outside the phone stay. Make a backup first — it is the only way back for this phone. |
| `dashboard` `empty.descriptionSync` | Přidejte účet nebo investici a Moony z nich sestaví přehled, grafy a vývoj. | Add an account or an investment and Moony builds your overview, charts and trends from it. |
| `mobile` `overview.emptyDescriptionSync` | Přidejte účet nebo investici a Moony z nich sestaví přehled, grafy a vývoj. | Add an account or an investment and Moony builds your overview, charts and trends from it. |

All rows are the plan's wording. Before writing "keep the data they hold", confirm it in code: `delete_account` (`src-tauri/src/commands/auth.rs`) deletes the vault, and a device whose vault is gone pauses with `VaultGone` and keeps its local data (`engine/mod.rs`, `state::set_vault_gone`); if that is not what the code does, stop and report.

- [ ] **Step 1: Implement** the switches and the copy in ten locales (no new pure logic).
- [ ] **Step 2: Gates** — TS gates; Expected: all pass.
- [ ] **Step 3: Commit** — `feat(sync): deleting and empty overviews say what sync changes`

---

### Task 8: "Co si uschovat" — telling the secrets apart (§7.5 first point)

**Files:**
- Create: `src/utils/keep-safe.ts`, `src/utils/keep-safe.test.ts`, `src/components/settings/KeepSafeCard.tsx`
- Modify: `src/components/settings/SettingsSectionPages.tsx` (the security page: the card after `BiometricUnlockCard`), `src/mobile/pages/settings/SecuritySection.tsx`
- Modify: `src/i18n/locales/*/settings.json`

**Interfaces:**
- Consumes: `syncIsOn` (Task 3); desktop `ShowRecoveryKitFlow({ onClose })` (`src/components/settings/sync/ShowRecoveryKitFlow.tsx`), phone `SyncKitSheet({ open, onOpenChange })` (`src/mobile/components/sync/SyncKitSheet.tsx`); both ask the device password first, as today.
- Produces (`src/utils/keep-safe.ts`):
  ```ts
  export type KeepSafeItem = 'password' | 'recoveryKey' | 'kit';
  /** What "Co si uschovat" lists: nothing without the sync flag (the card is not shown); the device password and recovery key; the Sync Recovery Kit while sync is on. The connection code is never listed (spec §7.5: the server's admin shows it again). */
  export function keepSafeItems(featureOn: boolean, syncOn: boolean): KeepSafeItem[];
  ```
- Desktop `export function KeepSafeCard()`: renders nothing for an empty list; otherwise a `SettingsCard` `keepSafe.title` / `keepSafe.description` with one `SettingsRow` per item (`label` = `keepSafe.<item>.title`, `hint` = `keepSafe.<item>.text`); the `kit` row has an outline button `keepSafe.kit.show` that mounts `ShowRecoveryKitFlow`.
- Phone `SecuritySection`: after the existing rows, `SectionHead` `keepSafe.title`, a caption `keepSafe.description`, then `EntityRows` of `SettingsRow` (icons `KeyRound` password, `FileKey` recovery key, `LifeBuoy` kit; sub = the item's text); the `kit` row opens `SyncKitSheet`.

Copy (`settings`):

| Key | cs | en |
|---|---|---|
| `keepSafe.title` | Co si uschovat | What to keep safe |
| `keepSafe.description` | Každá z těchto věcí slouží k něčemu jinému. (plan's wording) | Each of these does a different job. |
| `keepSafe.password.title` | Heslo zařízení | Device password |
| `keepSafe.password.text` | Odemyká Moony na tomto zařízení. | Unlocks Moony on this device. |
| `keepSafe.recoveryKey.title` | Obnovovací klíč | Recovery key |
| `keepSafe.recoveryKey.text` | Obnoví heslo na tomto zařízení. Každé zařízení má vlastní. | Resets the password on this device. Each device has its own. |
| `keepSafe.kit.title` | Záchranná sada synchronizace | Sync Recovery Kit |
| `keepSafe.kit.text` | Vrátí synchronizovaná data, když přijdete o všechna zařízení. Jedna pro všechna zařízení. | Brings your synced data back when every device is lost. One for all your devices. |
| `keepSafe.kit.show` | Zobrazit | Show |

The cs texts follow the spec's parentheses; the en texts are the plan's wording.

- [ ] **Step 1: Write the failing test** — `keep-safe.test.ts`: `it('lists nothing without the sync flag')` → `keepSafeItems(false, true)` is `[]` (// Review Focus 1); `it('lists the device secrets, and the Kit while sync is on')` → `(true, false)` is `['password', 'recoveryKey']`, `(true, true)` is `['password', 'recoveryKey', 'kit']`.
  Run `npx vitest run src/utils/keep-safe.test.ts`; Expected: FAIL.
- [ ] **Step 2: Implement** the module, the card, the phone rows, the copy in ten locales.
- [ ] **Step 3: Gates** — TS gates; Expected: all pass.
- [ ] **Step 4: Commit** — `feat(settings): a card that tells the device password, recovery key and Recovery Kit apart`

---

### Task 9: The Kit's code is the "Recovery Kit code" everywhere; `keyRotated` names the sync key (§7.5 points 2–3)

**Files:**
- Modify: `src/i18n/locales/*/common.json`, `*/deviceSync.json` (all ten)
- Modify: `docs/standards/i18n-glossary.md`

**Interfaces:**
- Values change; no key is added, renamed or removed (Rust and the components keep using the same keys).
- Keys and their new values:

| Namespace / key | cs | en |
|---|---|---|
| `common` `validation.syncRecoveryCodeInvalid` | Tento kód záchranné sady není platný. Zkontrolujte, že jste zadali všech 80 znaků – na velikosti písmen, mezerách a pomlčkách nezáleží. | This Recovery Kit code is not valid. Check that you entered all 80 characters - capitalization, spaces and dashes don't matter. |
| `common` `validation.syncPasswordRequired` | Pro zkopírování nebo uložení kódu záchranné sady znovu zadejte heslo. | Enter your password again to copy or save the Recovery Kit code. |
| `common` `validation.syncRecoveryVaultNotFound` | Synchronizační server nemá k tomuto kódu záchranné sady žádná synchronizovaná data. Zkontrolujte kód a adresu synchronizačního serveru v Záchranné sadě synchronizace. | The sync server has no synced data for this Recovery Kit code. Check the code and the sync server address on your Sync Recovery Kit. |
| `deviceSync` `setup.kit.file.codeLabel` | Kód záchranné sady | Recovery Kit code |
| `deviceSync` `join.codeLabel` | Kód záchranné sady | Recovery Kit code |
| `deviceSync` `join.kitHint` | Zadejte kód záchranné sady nebo text jejího QR kódu. | Enter the Recovery Kit code, or the text of the kit's QR code. |
| `deviceSync` `join.choice.kit.lead` | Zadáte kód záchranné sady a adresu serveru. Tak se připojuje i druhý počítač. | You enter the Recovery Kit code and the server address. This is also how a second computer joins. |
| `deviceSync` `codes.kitNotForSetup` | Toto je kód záchranné sady, ne připojovací kód. Použijte ho při připojení k již existujícím synchronizovaným datům. | (unchanged) This is a Recovery Kit code, not a connection code. Use it when you join synced data that already exists. |
| `deviceSync` `status.problem.keyRotated` | Klíč synchronizace byl změněn na jiném zařízení. Nastavte synchronizaci na tomto zařízení znovu. | The sync key was changed on another device. Set up sync on this device again. |

The cs name "Kód záchranné sady" and the `keyRotated` meaning are the spec's; the en name "Recovery Kit code" (already used by `phone.typeCode`, `phone.notAKitCode`) is the plan's.
- Other eight locales: the same keys, with the Recovery Kit code term of the glossary rows below; any other string the greps of Step 2 find gets the same term.
- Glossary (`docs/standards/i18n-glossary.md`, "Core terms", after "Recovery key"; amend "Recovery key" to "Recovery key (cs obnovovací klíč; one per device, resets that device's password)"):

| English | de | es | it | nb | pl | sv | ja | zh |
|---|---|---|---|---|---|---|---|---|
| Device password (cs heslo zařízení; unlocks one device) | Gerätepasswort | Contraseña del dispositivo | Password del dispositivo | Enhetspassord | Hasło urządzenia | Enhetens lösenord | デバイスのパスワード | 设备密码 |
| Sync Recovery Kit (cs Záchranná sada synchronizace; one for all devices) | Sync-Wiederherstellungs-Kit | Kit de recuperación de sincronización | Kit di ripristino della sincronizzazione | Gjenopprettingssett for synkronisering | Zestaw odzyskiwania synchronizacji | Återställningskit för synkronisering | 同期リカバリーキット | 同步恢复包 |
| Recovery Kit code (cs kód záchranné sady; never "recovery code" / "obnovovací kód") | Code des Wiederherstellungs-Kits | Código del kit de recuperación | Codice del kit di ripristino | Kode fra gjenopprettingssettet | Kod zestawu odzyskiwania | Kod från återställningskitet | リカバリーキットのコード | 恢复包代码 |
| Connection code (cs připojovací kód; from the sync server's admin) | Verbindungscode | Código de conexión | Codice di connessione | Tilkoblingskode | Kod połączenia | Anslutningskod | 接続コード | 连接码 |

These are the renderings the locale files already use for the Kit and the connection code.

- [ ] **Step 1: See what is left before the change** — run the three greps of Step 2 and keep their output as the list to change.
- [ ] **Step 2: Implement** the values in ten locales and the glossary. Then each of these must print nothing:
  ```bash
  grep -rnE "[Oo]bnovovací(ho|m)? kód|kód ze (záchranné )?sady|Obnovovací klíč byl změněn" src/i18n/locales/cs
  grep -rniE "recovery code|recovery key was changed" src/i18n/locales/en
  grep -rnE "Wiederherstellungscode|[Cc]ódigo de recuperación|[Cc]odice di ripristino|[Gg]jenopprettingskode|[Kk]od odzyskiwania|[Åå]terställningskod|リカバリーコード|恢复代码" src/i18n/locales
  ```
  Each locale's `status.problem.keyRotated` must name the sync key (not the device recovery key of the glossary); check the eight by reading them. Rust test fixtures in `services/sync/kit_file.rs` say "Recovery code:" as sample text; they test no label and stay.
- [ ] **Step 3: Gates** — TS gates; Expected: all pass (`src/i18n/locales.test.ts` included; // Review Focus 6).
- [ ] **Step 4: Commit** — `fix(sync): the Kit's code is the Recovery Kit code everywhere, and keyRotated names the sync key`

---

### Task 10: Docs, the spec's "As built" and verification (§10)

**Files:**
- Modify: `docs/architecture/overview.md` (device-sync section)
- Modify: `docs/specs/2026-11-14-sync-aware-app-design.md` (a new "## 13. As built (part C, <date>)" after §12; §1–§12 are not rewritten)
- No change: `docs/design-system/README.md` (the line and the card use existing `SettingsCard` / `SettingsRow` / `SectionHead` patterns), `README.md`, `PRIVACY.md`, `SECURITY.md`, `AGENTS.md` (spec §7.4: at release)

- [ ] **Step 1: Docs** — `overview.md`, device-sync section, one bullet each, naming files by role, no line numbers: the complete sync (`sync_state.last_complete_at`, what blocks it, recorded only by rounds); the newest safe copy and `sync_protects` in `services/milestones/upkeep.rs`, with `SyncStatus.lastCompleteAt` / `protectsData` on the wire and `syncIsOn` as the frontend's only "sync is on"; "Co si uschovat"; the Kit-code term. Spec §13: one bullet per item of "Decisions made by this plan" below that the build kept, plus anything the build changed.

- [ ] **Step 2: Full gates** — `npm run lint && npm run typecheck && npm run format:check && npm test` and `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; Expected: all pass.

- [ ] **Step 3: Start a local sync server** (background shell): in the sync server checkout the attachments plan's Task 19 used (`/Users/filipkral/Programování/FinanceApp/Moony-relay-blobs` while its blob branch is unmerged, else `/Users/filipkral/Programování/FinanceApp/Moony-relay`), `npm run build`, then `PORT=8797 SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start`. In its admin (`http://127.0.0.1:8797`) set up the admin, save the address, add a person (recipe: `docs/architecture/overview.md`, "Run it against a local sync server").

- [ ] **Step 4: Sync off first** — start the app as a background shell command with sync turned off:
  `MOONY_DEV_SYNC=0 npm run tauri dev -- --config '{"identifier":"com.moonyfinance.desktop.partc","build":{"devUrl":"http://localhost:1424","beforeDevCommand":"npm run dev -- --port 1424 --strictPort"}}'`
  Finish the first run with throwaway values (record nothing outside the scratchpad). Expected: Getting started shows "Udělat zálohu"; Settings → Zabezpečení has no "Co si uschovat"; Data a zálohy has no sync line; the lock screen and the login lead show today's text. Quit.

- [ ] **Step 5: Sync on** — start again without `MOONY_DEV_SYNC=0` (same identifier, same port). Expected before setup: Getting started shows "Ochránit data" (not done); "Co si uschovat" lists the password and the recovery key only; the lock screen and the first-run text are the new ones. Set up sync with the person's connection code, save the Recovery Kit text to the scratchpad. Expected after the first round: Getting started's item is done; Data a zálohy shows the protection line (`deviceSync` `protection.line_*`; the exact text is in the locale files, with the age "just now" and one device); "Co si uschovat" lists the Kit and "Zobrazit" asks the device password then shows it; the Kit's code field reads "Kód záchranné sady"; Danger zone's hint names the synced data. Add a document larger than 8 MiB to an insurance policy: while it waits to upload, the line is absent (content held only here withdraws the verdict); it returns after the upload and the next round (a periodic round, ≤ 5 min, or "Sync now"), with a fresh age.

- [ ] **Step 6: A stopped server** — stop the sync server. Expected, while the last complete sync is less than 7 whole UTC days old (as it is here): the line stays ("Server mimo dosah" in the top bar is neutral); no `backup_stale` and no `sync_stalled` in the agenda. From the seventh day the line goes. The 7- and 30-day switches are covered by the Task 2 tests with injected days; do not change the system clock.

- [ ] **Step 7: Phone (simulator), if available** — per `docs/architecture/overview.md` ("Pair two devices locally"), with `MOONY_RELAY_URL` pointing at `http://127.0.0.1:8797` and a phone dev-bridge port of 1430 or above, never 1420–1423: join the desktop's account; Záloha a přenos shows the protection line and the sync texts; Zabezpečení shows "Co si uschovat" with the Kit. If the simulator cannot be used in this session, say so in the report.

- [ ] **Step 8: Clean up** — stop the app and the server by PID; remove the temp data dir, the `com.moonyfinance.desktop.partc` app data, and any `.cargo/config.toml` created for the simulator.

- [ ] **Step 9: Commit** — `docs(sync): part C in the architecture overview and the spec's as-built notes`

**Left to the owner (cannot be checked here):** the phone notification text with sync on, on the owner's iPhone (needs a 30-day-old safe copy); the protection line, "Co si uschovat" and the renamed Kit code on the owner's iPhone; the other eight locales' wording; the repository docs at release (spec §7.4).

---

## Spec coverage

| Spec | Task |
|---|---|
| §7.1 newest safe copy, 30 days, 14 days' grace | 2 |
| §7.1 complete sync = round succeeded and nothing waits to upload, documents and photos included | 1 |
| §7.1 consequences: healthy sync silent, stall → `sync_stalled` then backup at 30 days, phone notification follows | 2 (rule, tests), 4 (notification) |
| §7.1 with sync on the text names both ways out | 4 |
| §7.2 explicit local value in `sync_state`, next migration | 1 |
| §7.3 backup and restore stay; the "Vaše data chrání synchronizace" line, desktop and phone | 2 (verdict), 5 |
| §7.3 Getting started "Ochránit data", done by a backup or sync on, hint names both | 3 |
| §7.3 phone first run, last step | built in parts A/B (§5.2); no change (Task 6 note) |
| §7.4 row 1 first-run privacy points | 6 |
| §7.4 row 2 phone Záloha a přenos lead and `backup.restoreLead` | 5 |
| §7.4 row 3 `backup.osBackupNote`, `firstRun.backupLead` | 5 (`osBackupNote`); `firstRun.backupLead` unchanged (Task 6 note) |
| §7.4 row 4 sync intro `intro.description` | 6 |
| §7.4 row 5 delete account / Danger zone | 7 |
| §7.4 row 6 repository docs | not before release (Global Constraints) |
| §7.5 "Co si uschovat" | 8 |
| §7.5 "Kód záchranné sady" everywhere | 9 |
| §7.5 `keyRotated` | 9 |
| §8 wire procedure, services, ten locales, append-only migration | 1, 2, every task |
| §9 tests for pure logic only | 1–5, 8 |
| §10 milestones in `upkeep.rs` tests; locale parity | 2, every task, 10 |
| §11 calls 6, 7, 8 | 2, 3, 9 |
| §12 `SYNC_STALE_DAYS`, `TRANSIENT_PROBLEMS`, `sync_stalled` only with the flag | 2 (`sync_protects` reuses `sync_stalled`) |
| Attachments §5.6 "synchronised only when nothing waits to upload" | 1 |
| Attachments §5.7 what a backup leaves out | Task 18 (phone, attachments plan); 5 (desktop) |

## Decisions made by this plan

1. A complete sync needs a round with no problem at all. A round whose only problem is a waiting gap or a change dated ahead still records its success, as today, but not a complete sync, because `sync_stalled` already treats that state as needing the person.
2. Uploads that cannot go by themselves (refused, conflict, invalid ID) keep the sync incomplete, although the wire counts them as `failed` and not as `waitingUpload`: their content exists on this device only. Only entries a row still names count.
3. Content not on this device (not fetched yet, not uploaded by the other device yet, or a failed download) does not weaken this device's safe copy: it is on the server or on the device that added it, whose own rule covers it.
4. "Sync protects the data" (the §7.3 line, `protectsData`) means: sync on, `sync_stalled` quiet, and a complete sync less than 7 whole UTC days old. The line and the backup reminder can therefore never show together.
5. A safe-copy time in the future counts as fresh (age 0, never negative), as backups and `sync_stalled` already do; the complete time is overwritten at each complete round, so a corrected clock heals it.
6. Only rounds record the complete time (at most one periodic round, 5 minutes, after a lane finishes an upload); existing profiles start without one after the migration (no backfill).
7. With sync on, the backup reminder keeps its kind, key format, title and link. Its sub-line, the phone's Údržba row, the phone notification (under its own id, so a pending one is re-planned) and the reminder settings hints say "last safe copy" and name both ways out. The folded agenda summary stays "back up your data".
8. With the flag on and sync off, the phone's Záloha a přenos texts, the delete texts and the empty overviews read as today; they change only while sync is on. Getting started's "Ochránit data" follows the flag, as the spec asks, not the sync state.
9. "Co si uschovat" shows only with the sync flag on, so Security reads as today without it; the Kit row shows only while sync is on.
10. Where a phone cannot make backups (anything but iOS for now), Getting started has no "Ochránit data" item, as it has no backup item there today.
11. The spec's "naposledy dnes 14:02" is shown with the status age of parts A/B ("naposledy před 4 min").
12. Wording outside the spec's §7.4 table that says data never leaves the device is changed the same way: the desktop lock screen and login lead (flag on), and the empty overviews on both platforms (sync on).
13. The desktop Data a zálohy also says when documents and photos are not on this computer yet and so not in a backup made now (sync on), using the attachments plan's `notInBackup`.
14. The English name of the Kit's code is "Recovery Kit code" (the Czech "Kód záchranné sady" is the spec's); the other eight locales' terms are fixed in the glossary.
