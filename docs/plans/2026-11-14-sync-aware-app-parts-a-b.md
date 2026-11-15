# Sync-Aware App (Parts A and B) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new device can join sync from the first run (desktop and phone), Settings → Sync offers two clear choices, and the sync state is visible where people look (top-bar popover, phone Settings row, Údržba card, a `sync_stalled` reminder), with one status mapping for both platforms.

**Architecture:** Two small backend additions (a join into a brand-new profile leaves a plain backup; `SyncStatus.enabledAt` plus a pure `sync_stalled` milestone fed by the engine's current status). On the frontend, one pure mapping `src/utils/sync-state.ts` drives every status surface; the join flows become context-aware components (`JoinContext`) that Settings and the first runs share instead of copying; the first-run step orders live in pure modules with tests.

**Tech Stack:** Rust (rusqlite, Tauri 2), React 19 + TanStack Query + react-hook-form/zod, react-i18next (ten locales), Vitest (node env).

**Spec:** `docs/specs/2026-11-14-sync-aware-app-design.md`, parts A and B only (§5 incl. §5.4, §6). Read §5–§6 and §11 before your task; the spec wins over this plan on any conflict, and its "(my call)" decisions are binding.

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-syncux` (branch `feat/sync-aware-app`), never in the sibling `Moony-tauri`. Check `git branch --show-current` before each commit. Never push. Commit with explicit paths.
- Read `AGENTS.md` (ten rules), `docs/standards/` (rust-backend, typescript-frontend, type-contract, testing, i18n) and `docs/DEPRECATED.md` before your first change.
- **Part C (§7) is out of scope:** no change to `backup_stale`, the backup rows, Getting started, the §7.4 wording table, "Co si uschovat" or the Kit-code rename. Where text mentions documents and photos it says they stay on the device that added them for now.
- **Everything about sync stays behind the flag:** frontend `useDeviceSyncFeature()` (`src/hooks/use-device-sync.ts`), backend `services::sync::feature::enabled()`. With the flag off, the first runs and Settings look exactly as today.
- "Jak si server spustit" has no address yet: leave the link out entirely (§5.3).
- No new Tauri command and no migration in this plan. "When sync was turned on on this device" is already stored: `sync_devices.joined_at` of this device's own row (written once by `devices::record_this_device` at setup and at a join, never overwritten).
- Wire changes follow `docs/standards/type-contract.md`: `SyncStatus.enabledAt` (Task 2) and `MilestoneKind` `'sync_stalled'` (Task 3). Per-field `#[serde(rename = "…")]`; regenerate `shared/generated-types.ts` with `cd src-tauri && cargo test generate_bindings -- --ignored`.
- Every new or changed user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`) in the task that introduces it, with each language's plural forms (`docs/standards/i18n.md` §2). The Czech and English wording below is binding; translate the other eight per `docs/standards/i18n-glossary.md`. A key removed from English is removed from every locale.
- Tests only for pure logic (`src/utils/`, `src/mobile/lib/`, Rust services); no component, hook, command-handler or `tauri-api.ts` tests (`docs/standards/testing.md`).
- The stall threshold is **7 whole UTC days** on both sides: TS `SYNC_STALE_DAYS = 7` (`src/utils/sync-state.ts`), Rust `SYNC_STALE_DAYS: i64 = 7` (`services/milestones/upkeep.rs`); "days" means `floor(now / 86400) - floor(from / 86400)` (TS `daysBetween` in `src/utils/milestones.ts`, Rust `day_floor`).
- The server counts as "out of reach" (neutral, retried by the engine) for the problems `offline`, `relayError` and `rateLimited` and no other (TS `TRANSIENT_PROBLEMS`, Rust `TRANSIENT_PROBLEMS`).
- Look and feel: `docs/design-system/README.md`; the top-bar popover copies `DataStatus`, phone rows copy the Údržba card and `SettingsRow`.
- Gates after every task: `npm run lint && npm run typecheck && npm run format:check && npm test`; for tasks that touch Rust also `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`.
- Commits: Conventional Commits; the message ends with the line `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- Never run the app before Task 12. Task 12 runs it only as stated there (own identifier, port 1423, never the owner's real profile, never ports 1420–1422).

## Review Focus

1. A first-run person who chose the join path, then continued without sync (or gave up after a failure), must not end with a nameless profile: the Co sledovat step asks for name and surname on that path (Task 10 `profileNameSchema` test; Task 11 reuses it).
2. A home server out of reach for exactly 6 vs 7 UTC days, and a device that never synced (counted from `enabledAt`): the indicator and the reminder must switch on the same day (Task 3 Rust tests, Task 4 TS table).
3. A transient `relayError` or `rateLimited` (a proxy 502, a rate limit) must stay neutral and raise no reminder before 7 days (Task 3 and Task 4 tests).
4. Clock skew: a `lastSuccessAt` or `enabledAt` in the future must never give a negative age or day count, nor stall (Task 3 `a_last_success_in_the_future_does_not_stall`, Task 4 `syncAge` and table rows).
5. After a successful phone first-run join there is no Záloha step and no way back from Připojení; after declining, the normal steps including Záloha follow (Task 11 `first-run.test.ts`).

## File map

| Path | Responsibility | Task |
|---|---|---|
| `src-tauri/src/services/sync/impact.rs`, `join.rs` | `holds_nothing`; a brand-new profile's join backup settles Plain | 1 |
| `src-tauri/src/services/sync/devices.rs`, `engine/status.rs`, `models/sync.rs` | `SyncStatus.enabled_at` | 2 |
| `src-tauri/src/services/milestones/upkeep.rs`, `mod.rs`, `models/milestones.rs`, `commands/milestones.rs` | `sync_stalled` | 3 |
| `src/utils/sync-state.ts` (replaces `device-sync-indicator.ts` and `mobile/lib/sync-summary.ts`) | the one status mapping, age | 4 |
| `src/components/shell/SyncStatusIndicator.tsx` | top-bar popover | 5 |
| `src/utils/join-context.ts` | how a join flow presents itself in Settings vs the first run | 8 |
| `src/components/settings/sync/join/` (`JoinChoice`, `PairingJoinFlow`, `RecoveryJoinFlow`, `JoinDeviceDialog`) | the desktop join flows, shared by Settings and the wizard | 8 |
| `src/mobile/components/sync/JoinChoiceSheet.tsx` | the phone's join choice in Settings | 9 |
| `src/utils/onboarding-steps.ts`, `src/utils/form-schemas.ts` | desktop wizard order; password-only and name-only form schemas | 10 |
| `src/components/auth/onboarding/ConnectStep.tsx`, `DevicePasswordForm.tsx` | desktop Připojení and password step | 10 |
| `src/mobile/lib/first-run.ts`, `src/mobile/pages/first-run/ConnectStep.tsx` | phone order with paths; phone Připojení | 11 |

---

### Task 1: A join into a brand-new profile leaves a plain backup (§5.4)

**Files:**
- Modify: `src-tauri/src/services/sync/impact.rs` (method + test)
- Modify: `src-tauri/src/services/sync/join.rs` (`replace_hold`, `Replaced::always_kept` doc, module doc's "a join's … backup stay kept")
- Modify: `src-tauri/src/services/sync/engine/tests/mod.rs` (helper), `engine/tests/replace_safety.rs`, `engine/tests/join_new.rs`
- Modify: `docs/architecture/overview.md` (the "Replaces and their backups" bullet)

**Interfaces:**
- Produces (Rust): `impl ReplaceImpact { pub fn holds_nothing(&self) -> bool }` — true when `accounts`, `transactions`, `documents`, `photos`, `pending_items` and `pending_batches` are all 0; `changes_through` never decides.
- Produces (test helper): `pub fn bank_account(conn: &Connection, id: &str)` in `engine/tests/mod.rs`, inserting one minimal `bank_accounts` row (read the migrated schema for the required columns).
- Behaviour: in `replace_hold`, for `ReplaceKind::Join` only, read `impact::replace_impact(conn)?.holds_nothing()` inside the hold after the consent check and before the backup is written; `always_kept` becomes `match input.kind { Join => !brand_new, Repair => true, Resync => resync_keeps }`. The backup is still written and still marked Kept before the replace commits (crash safety); `settled` then marks it Plain. Log `log::info!("[SYNC] The joining profile held nothing: its backup is not kept")`.

- [ ] **Step 1: Write the failing tests**

In `impact.rs` tests:

```rust
#[test]
fn holds_nothing_counts_everything_but_the_watermark() {
    let empty = ReplaceImpact { changes_through: 42, ..Default::default() };
    assert!(empty.holds_nothing());
    for impact in [
        ReplaceImpact { accounts: 1, ..Default::default() },
        ReplaceImpact { transactions: 1, ..Default::default() },
        ReplaceImpact { documents: 1, ..Default::default() },
        ReplaceImpact { photos: 1, ..Default::default() },
        ReplaceImpact { pending_items: 1, ..Default::default() },
        ReplaceImpact { pending_batches: 1, ..Default::default() },
    ] {
        assert!(!impact.holds_nothing(), "{impact:?}");
    }
}
```

In `replace_safety.rs`, replace `every_join_that_replaced_keeps_its_backup` with two tests:
- `a_join_into_a_profile_with_data_keeps_its_backup`: `bank_account(&joiner.lock(), "b1")` before `agreed`; assert `outcome.kept`, one listed backup with `BackupMark::Kept`.
- `a_join_into_a_brand_new_profile_leaves_a_plain_backup`: `TestConn::new(migrated())` with only `institution(&…, "local-only")` (institutions are not counted); assert `!outcome.kept`, `outcome.backup == dir.path().join(format!("sync-repair-{NOW}.zip"))`, `outcome.backup.is_file()`, one listed backup with `BackupMark::Plain`.

- [ ] **Step 2: Run them and see them fail**

Run: `cd src-tauri && cargo test --lib holds_nothing_counts_everything_but_the_watermark a_join_into_a_brand_new_profile_leaves_a_plain_backup`
Expected: FAIL (`holds_nothing` not found).

- [ ] **Step 3: Implement `holds_nothing` and the `replace_hold` change**

- [ ] **Step 4: Keep the existing join tests honest**

`join_new.rs`: `newcomer()` also calls `bank_account(&conn.lock(), "local-account")`; the test that re-joins a device that turned sync off and `the_join_backs_up_and_replaces_in_one_connection_hold` (`replace_safety.rs`) give their joiner a bank account the same way. Run `cd src-tauri && cargo test --lib services::sync`; any other join test that asserts `-kept` for a profile holding nothing counted gets a bank account — never weaken the assertion.

- [ ] **Step 5: Docs** — in `docs/architecture/overview.md` ("Replaces and their backups") add: a join into a profile whose replace impact holds nothing (no account, transaction, document, photo or unsent change) writes its backup as usual and leaves it plain (prunable), spec 2026-11-14 §5.4.

- [ ] **Step 6: Gates** — Rust gates; Expected: all pass.

- [ ] **Step 7: Commit** — `fix(sync): a join into a brand-new profile leaves a plain backup`

---

### Task 2: `SyncStatus.enabledAt` — when sync was turned on on this device

**Files:**
- Modify: `src-tauri/src/services/sync/devices.rs` (function + test)
- Modify: `src-tauri/src/services/sync/engine/status.rs` (`status`, `quiet`; test)
- Modify: `src-tauri/src/models/sync.rs` (`SyncStatus`)
- Regenerate: `shared/generated-types.ts`
- Modify: `shared/schema.ts` (`SyncStatus`)
- Modify: `src/utils/device-sync-indicator.test.ts`, `src/mobile/lib/sync-summary.test.ts` (fixtures get `enabledAt: null`)

**Interfaces:**
- Produces (Rust): `pub fn this_device_joined_at(conn: &Connection) -> Result<Option<i64>>` in `devices.rs` — `joined_at` of the `sync_devices` row whose `id` is `state::load(conn)?.device.to_hex()`; `None` without a `sync_state` row or before the row is recorded.
- Produces (Rust model): `SyncStatus { …, #[serde(rename = "enabledAt")] pub enabled_at: Option<i64> }` — set only for an enabled profile (`None` in `quiet`).
- Produces (TS, `shared/schema.ts`):
  ```ts
  /** When sync was turned on on this device: `joinedAt` of its own device row (unix seconds); `null` while off or before the row exists. */
  enabledAt: number | null;
  ```

- [ ] **Step 1: Write the failing tests**
- `devices.rs`: `this_device_joined_at_is_its_first_record` — no `sync_state` row → `None`; after `record_this_device(…, 1_800_000_000)` → `Some(1_800_000_000)`; after a second `record_this_device(…, 1_900_000_000)` → still `Some(1_800_000_000)`.
- `status.rs`: `an_enabled_profile_reports_when_this_device_joined` — an enabled profile with its device row joined at 1_700_000_000 → `status(...).enabled_at == Some(1_700_000_000)`; `a_profile_with_sync_off_and_no_account_names_no_server` (existing) additionally asserts `enabled_at == None`.

- [ ] **Step 2: Run them and see them fail** — `cd src-tauri && cargo test --lib this_device_joined_at an_enabled_profile_reports_when_this_device_joined`; Expected: FAIL (not found).

- [ ] **Step 3: Implement** the function, the field, and `status()` reading it for an enabled profile.

- [ ] **Step 4: Regenerate and mirror** — `cd src-tauri && cargo test generate_bindings -- --ignored`; add the field to `shared/schema.ts`; add `enabledAt: null` to the two TS fixtures.

- [ ] **Step 5: Gates** — Rust and TS gates; Expected: all pass, `git diff --stat shared/generated-types.ts` shows only `enabledAt`.

- [ ] **Step 6: Commit** — `feat(sync): the status says when sync was turned on on this device`

---

### Task 3: The `sync_stalled` upkeep milestone (§6.4)

**Files:**
- Modify: `src-tauri/src/services/milestones/upkeep.rs` (consts, pure function, tests)
- Modify: `src-tauri/src/services/milestones/mod.rs` (`UPKEEP_KINDS` gets `"sync_stalled"`; `list_milestones` signature; existing tests pass `None`; one list test)
- Modify: `src-tauri/src/models/milestones.rs` (`MILESTONE_KINDS` gets `"sync_stalled"`, length 17)
- Modify: `src-tauri/src/commands/milestones.rs` (`get_milestones`), `src-tauri/src/commands/sync.rs` (`current_status` → `pub(crate)`)
- Modify: `shared/schema.ts` (`MilestoneKind` gets `'sync_stalled'`)
- Modify: `src/utils/milestones.ts` (+ `milestones.test.ts`), `src/hooks/use-milestone-sub-line.ts`, `src/components/milestones/MilestoneRows.tsx` (`UPKEEP_ICONS.sync_stalled = RefreshCw`), `src/components/settings/MilestoneSettingsCard.tsx`, `src/mobile/pages/settings/RemindersSection.tsx`, `src/mobile/pages/Settings.tsx` (reminders summary count), `src/mobile/lib/milestone-path.ts` (+ test), `src/hooks/use-device-sync-events.ts`
- Modify: `src/i18n/locales/*/milestones.json` (ten locales)

**Interfaces:**
- Produces (Rust, `upkeep.rs`):
  ```rust
  /// A sync without a successful round for this many whole UTC days asks for a look (spec 2026-11-14 §6.4).
  pub(crate) const SYNC_STALE_DAYS: i64 = 7;
  /// Problems the engine retries by itself: the server is out of reach for now.
  pub(crate) const TRANSIENT_PROBLEMS: [SyncProblem; 3] =
      [SyncProblem::Offline, SyncProblem::RelayError, SyncProblem::RateLimited];
  pub(super) fn sync_stalled(today: i64, sync: &SyncStatus) -> Option<Milestone>
  ```
  Rules (in order): phase `Off`/`SettingUp` → `None`. **At once:** phase `Waiting`, `Paused` or `Error` with a problem not in `TRANSIENT_PROBLEMS` (a `Paused`/`Error` with `problem: None` counts as needing the person). **By time:** anchor = `last_success_at.or(enabled_at)`; when the anchor exists and `day_floor(anchor) <= today - SYNC_STALE_DAYS * DAY` (any phase but off/settingUp, a running round included). Otherwise `None`. The milestone: `milestone("sync_stalled", key, None, "")` with `since_day = last_success_at.map(day_floor)`; key `sync_stalled:<day_floor(anchor)>`, or `sync_stalled:never` without an anchor. A `Syncing` phase never stalls "at once".
- Produces (Rust, `mod.rs`): `pub fn list_milestones(conn: &Connection, today: i64, sync: Option<&SyncStatus>) -> Result<Vec<Milestone>>` — after the builders, `all.extend(sync.and_then(|s| upkeep::sync_stalled(today, s)))`, before muting and states.
- Produces (command): `get_milestones(app: AppHandle, db: State<'_, Database>)` — `sync = feature::enabled().then(|| commands::sync::current_status(&app, &db))`; an `Err` is logged (`log::warn!("[MILESTONES] sync status skipped: …")`) and becomes `None`. Decision (§9): produced only with the sync flag on — without it no engine runs and the Settings page offers only "turn off".
- Produces (TS, `src/utils/milestones.ts`): `'sync_stalled'` in `UPKEEP_KINDS`; `MilestoneGroupId` gains `'sync'`; `GROUP_OF.sync_stalled = 'sync'`; `'sync'` last in `MILESTONE_GROUP_IDS`; `milestoneHref` → `'/settings/sync'`; new
  ```ts
  /** The reminder groups Settings lists: `sync` only while the sync feature is on. */
  export function visibleMilestoneGroups(syncFeature: boolean): MilestoneGroupId[]
  ```
  used by `MilestoneSettingsCard`, `RemindersSection` and the phone Settings summary (`useDeviceSyncFeature()`).
- Not in the top bar: `MilestonesStatus` shows deadlines only; the stall shows there through `SyncStatusIndicator` (Task 5). Not a phone notification (spec non-goal): `notification-plan.ts` is unchanged.
- Refresh: `use-device-sync-events.ts` also invalidates `['milestones']` on `sync-status-changed`.

Copy (`milestones` namespace):

| Key | cs | en |
|---|---|---|
| `titles.sync_stalled` | Synchronizace | Sync |
| `sub.sync_stalled` | Naposledy synchronizováno {{date}} | Last synced {{date}} |
| `sub.sync_stalled_never` | Zatím nesynchronizováno | Not synced yet |
| `short.sync_stalled` | zkontrolovat synchronizaci | check sync |
| `groups.sync.label` | Synchronizace | Sync |
| `groups.sync.hint` | Když se synchronizace zastaví nebo potřebuje váš zásah. | When sync stops or needs you. |
| `groups.sync.mute` | Nepřipomínat synchronizaci | Don't remind me about sync |

`use-milestone-sub-line.ts`: `sync_stalled` like `backup_stale` (`sub.sync_stalled` with `day(m.sinceDay)`, `sub.sync_stalled_never` when `sinceDay` is null).

- [ ] **Step 1: Write the failing Rust tests** in `upkeep.rs` with a helper `fn sync(phase: SyncPhase, problem: Option<SyncProblem>, last_success: Option<i64>, enabled_at: Option<i64>) -> SyncStatus` (other fields zero/false/None) and `today = day(2026, 11, 14)`:
  - `sync_off_or_being_set_up_raises_nothing` — `Off` and `SettingUp` with `last_success = day(2026, 1, 1)` → `None`.
  - `a_healthy_sync_raises_nothing` — `Idle`, last success `today + 3_600` → `None`.
  - `an_unreachable_server_is_quiet_for_six_days_and_stalls_on_the_seventh` — `Paused`/`Offline`, last success `day(2026, 11, 8) + 82_800` → `None`; `day(2026, 11, 7) + 60` → `Some` with `since_day == Some(day(2026, 11, 7))`, `key == format!("sync_stalled:{}", day(2026, 11, 7))`.
  - `relay_errors_and_rate_limits_count_as_out_of_reach` — `Error`/`RelayError` and `Paused`/`RateLimited`, last success `day(2026, 11, 12)` → `None`.
  - `a_problem_that_needs_the_person_stalls_at_once` — last success `today`: `Error`/`Unauthorized`, `Error`/`CaptureBroken`, `Waiting`/`Gap`, `Paused`/`Lapsed`, `Paused`/`None` → each `Some`.
  - `a_running_round_does_not_stall_at_once` — `Syncing`/`Lapsed`, last success `today` → `None`; last success `day(2026, 11, 6)` → `Some`.
  - `a_device_that_never_synced_counts_from_when_it_joined` — `Paused`/`Offline`, no success, `enabled_at = day(2026, 11, 11)` → `None`; `enabled_at = day(2026, 11, 7)` → `Some` with `since_day == None`, key `sync_stalled:{day(2026,11,7)}`.
  - `without_a_success_or_a_join_record_nothing_stalls_by_time` — `Paused`/`Offline`, both `None` → `None`; `Error`/`Internal`, both `None` → `Some` with key `"sync_stalled:never"`.
  - `a_last_success_in_the_future_does_not_stall` — `Paused`/`Offline`, last success `day(2026, 12, 1)` → `None`.
  - `a_new_stall_after_a_success_has_a_new_key` — two stalls with last success 10 days apart give different keys.
  In `mod.rs` tests: `sync_stalled_is_listed_with_a_status_and_can_be_muted` — `list_milestones(&conn, today, Some(&stalled))` contains one `sync_stalled`; with `set_muted_kinds(&conn, &["sync_stalled".into()])` it is gone; `list_milestones(&conn, today, None)` has none.

- [ ] **Step 2: Run them and see them fail** — `cd src-tauri && cargo test --lib services::milestones`; Expected: FAIL (`sync_stalled` not found / wrong arity).

- [ ] **Step 3: Implement** the Rust consts, `sync_stalled`, the `list_milestones` parameter (existing callers and tests pass `None`), `MILESTONE_KINDS`, `UPKEEP_KINDS`, the command.

- [ ] **Step 4: Write the failing TS tests**
  - `src/utils/milestones.test.ts`: `groupOf('sync_stalled') === 'sync'`; `milestoneHref({ …, kind: 'sync_stalled' }) === '/settings/sync'`; `isUpkeep` true for it; `visibleMilestoneGroups(false)` equals `MILESTONE_GROUP_IDS` without `'sync'`, `visibleMilestoneGroups(true)` equals `MILESTONE_GROUP_IDS`.
  - `src/mobile/lib/milestone-path.test.ts`: `EXPECTED.sync_stalled = settingsPath('sync')`.
  Run `npx vitest run src/utils/milestones.test.ts src/mobile/lib/milestone-path.test.ts`; Expected: FAIL.

- [ ] **Step 5: Implement the TS side** (schema kind, `milestones.ts`, sub-line, icon, the three group lists, `milestonePath` case, events invalidation) and the copy in ten locales.

- [ ] **Step 6: Gates** — Rust and TS gates; Expected: all pass.

- [ ] **Step 7: Commit** — `feat(milestones): a reminder when sync has stalled`

---

### Task 4: One sync-state mapping for desktop and phone (§6.1)

**Files:**
- Create: `src/utils/sync-state.ts`, `src/utils/sync-state.test.ts`, `src/utils/sync-state-backend-pin.test.ts`
- Delete: `src/utils/device-sync-indicator.ts` (+ test), `src/mobile/lib/sync-summary.ts` (+ test)
- Modify: `src/components/shell/SyncStatusIndicator.tsx` (still a link here; uses `syncState`), `src/mobile/pages/Settings.tsx` (Sync row sub-line)
- Modify: `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Produces (`src/utils/sync-state.ts`):
  ```ts
  export const SYNC_STALE_DAYS = 7;
  /** The engine retries these by itself: the server is out of reach for now. */
  export const TRANSIENT_PROBLEMS: readonly SyncProblem[] = ['offline', 'relayError', 'rateLimited'];
  export type SyncStateKind =
    'synced' | 'syncing' | 'unreachable' | 'unreachableLong' | 'waiting' | 'paused' | 'error';
  export type SyncTone = 'fresh' | 'neutral' | 'stale';
  export interface SyncStateView {
    kind: SyncStateKind;
    tone: SyncTone;
    /** `deviceSync` key of the §6.1 label: `indicator.<kind>` (plural for `unreachableLong`, `count: days`). */
    labelKey: string;
    /** `deviceSync` key of the phone's Settings row: `phone.row.<kind>`. */
    rowKey: string;
    /** Whole UTC days without a success (`unreachableLong` only), else 0. */
    days: number;
    /** What the age after the label counts from: `lastSuccessAt`; `null` for `unreachableLong` or without a success. */
    ageFrom: number | null;
  }
  /** `now` in unix seconds. `null` while sync is off or being set up (no indicator). */
  export function syncState(status: SyncStatus | undefined, now: number): SyncStateView | null;
  export type SyncAgeKey = 'justNow' | 'minutes' | 'hours' | 'days';
  /** "před 4 min": whole minutes/hours/days since `from`; under a minute or in the future is `justNow` (count 0). */
  export function syncAge(from: number, now: number): { key: SyncAgeKey; count: number };
  ```
  Order of decisions: `undefined`/`off`/`settingUp` → `null`; phase `syncing` → `syncing`; problem in `TRANSIENT_PROBLEMS` → anchor `lastSuccessAt ?? enabledAt`, `days = max(0, daysBetween(anchor, now))`, `unreachableLong` (stale) when the anchor exists and `days >= SYNC_STALE_DAYS`, else `unreachable` (neutral, `days` 0); `idle` → `synced`; `waiting` → `waiting`; `paused` → `paused`; `error` → `error`. Tones: synced fresh; syncing, unreachable neutral; the rest stale. `tone === 'stale'` is exactly "needs the person" (§6.3, §6.4).
- Consumers: Task 5 (popover), Task 6 (Settings pages), the phone Settings row here: sub-line = `t(rowKey, { count: days })`, followed by `t('phone.rowWithAge', { state, age: t(\`age.${a.key}\`, { count: a.count }) })` when `ageFrom !== null`; `phone.row.off` when `syncState` is `null`. Age is computed at render (no timer; §9 leaves refresh to the builder).

Copy (`deviceSync`): replace `indicator.offline` and `phone.row.offline` (remove from all locales) with:

| Key | cs | en |
|---|---|---|
| `indicator.unreachable` | Server mimo dosah | Server out of reach |
| `indicator.unreachableLong_*` | Nesynchronizováno {{count}} den / dny / dní | Not synced for {{count}} day / days |
| `phone.row.unreachable` | Server mimo dosah | Server out of reach |
| `phone.row.unreachableLong_*` | Nesynchronizováno {{count}} den / dny / dní | Not synced for {{count}} day / days |
| `phone.rowWithAge` | {{state}} · {{age}} | {{state}} · {{age}} |
| `age.justNow` | právě teď | just now |
| `age.minutes_*` | před {{count}} min | {{count}} min ago |
| `age.hours_*` | před {{count}} h | {{count}} h ago |
| `age.days_*` | před {{count}} dnem / dny | {{count}} day / days ago |

Existing labels stay: `indicator.synced` Synchronizováno, `indicator.syncing` Synchronizuji…, `indicator.waiting` Změna je zadržena, `indicator.paused` Synchronizace pozastavena, `indicator.error` Chyba synchronizace; `phone.row.synced` Aktuální etc.

- [ ] **Step 1: Write the failing tests**

`sync-state.test.ts` (helper `status(phase, problem, lastSuccessAt, enabledAt)`; `D = 86_400`; `now = 20_000 * D + 43_200`):

```ts
it('maps the spec rows', () => {
  const rows: [SyncStatus | undefined, SyncStateKind | null, SyncTone | null][] = [
    [undefined, null, null],
    [status('off'), null, null],
    [status('settingUp'), null, null],
    [status('idle', null, now - 240), 'synced', 'fresh'],
    [status('syncing'), 'syncing', 'neutral'],
    [status('syncing', 'offline', now - 30 * D), 'syncing', 'neutral'],
    [status('paused', 'offline', now - 6 * D), 'unreachable', 'neutral'],
    [status('paused', 'offline', now - 7 * D), 'unreachableLong', 'stale'],
    [status('error', 'relayError', now - D), 'unreachable', 'neutral'],
    [status('paused', 'rateLimited', now - D), 'unreachable', 'neutral'],
    [status('waiting', 'gap', now), 'waiting', 'stale'],
    [status('waiting', 'clockAhead', now), 'waiting', 'stale'],
    [status('paused', 'lapsed', now), 'paused', 'stale'],
    [status('paused', null, now), 'paused', 'stale'],
    [status('error', 'internal', now), 'error', 'stale'],
    [status('error', 'captureBroken', now), 'error', 'stale'],
  ];
  for (const [input, kind, tone] of rows) {
    const view = syncState(input, now);
    expect(view?.kind ?? null, JSON.stringify(input)).toBe(kind);
    expect(view?.tone ?? null).toBe(tone);
  }
});
it('switches to stale on the seventh UTC day', () => {
  const lateOnDay = 20_000 * D - 60;                  // 23:59 of the day before `now`'s day
  expect(syncState(status('paused', 'offline', lateOnDay - 5 * D), 20_000 * D + 60)?.kind).toBe('unreachable');
  expect(syncState(status('paused', 'offline', lateOnDay - 6 * D), 20_000 * D + 60)).toMatchObject({ kind: 'unreachableLong', days: 7, labelKey: 'indicator.unreachableLong', ageFrom: null });
});
it('counts a device that never synced from when sync was turned on', () => {
  expect(syncState(status('paused', 'offline', null, now - 8 * D), now)).toMatchObject({ kind: 'unreachableLong', days: 8 });
  expect(syncState(status('paused', 'offline', null, null), now)).toMatchObject({ kind: 'unreachable', days: 0, ageFrom: null });
});
it('never counts negative days for a success in the future', () => {
  expect(syncState(status('paused', 'offline', now + 3 * D), now)).toMatchObject({ kind: 'unreachable', days: 0 });
});
it('keys the labels and the age by kind', () => {
  expect(syncState(status('idle', null, now - 240), now)).toMatchObject({ labelKey: 'indicator.synced', rowKey: 'phone.row.synced', ageFrom: now - 240 });
});
it('gives the age in whole units', () => {
  const cases: [number, SyncAgeKey, number][] = [
    [0, 'justNow', 0], [59, 'justNow', 0], [60, 'minutes', 1], [3_599, 'minutes', 59],
    [3_600, 'hours', 1], [86_399, 'hours', 23], [86_400, 'days', 1], [-500, 'justNow', 0],
  ];
  for (const [elapsed, key, count] of cases) expect(syncAge(now - elapsed, now)).toEqual({ key, count });
});
```

`sync-state-backend-pin.test.ts` (the `upkeep-backend-pin.test.ts` pattern, reading `src-tauri/src/services/milestones/upkeep.rs`): `SYNC_STALE_DAYS` equals the Rust `const SYNC_STALE_DAYS: i64 = (\d+);`; the Rust `TRANSIENT_PROBLEMS` names (`SyncProblem::Offline`, `RelayError`, `RateLimited`, lower-camel-cased) equal the TS list in order.

- [ ] **Step 2: Run them and see them fail** — `npx vitest run src/utils/sync-state`; Expected: FAIL (module not found).

- [ ] **Step 3: Implement `sync-state.ts`**, switch `SyncStatusIndicator` (label with `count: days`) and the phone Settings row to it, delete the two old modules and their tests, update the copy in ten locales.

- [ ] **Step 4: Gates** — TS gates; Expected: all pass.

- [ ] **Step 5: Commit** — `feat(sync): one status mapping for desktop and phone`

---

### Task 5: Desktop top bar — the sync popover (§6.2)

**Files:**
- Modify: `src/components/shell/SyncStatusIndicator.tsx`
- Modify: `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Consumes: `syncState`, `syncAge` (Task 4); `useDeviceSyncStatus`, `useDeviceSyncFeature`; `useDeviceSyncMutations().syncNow`; `hostOf` (`src/utils/sync-setup.ts`); `StatusText` (`DataStatus.tsx`); `Popover*` (`src/components/ui/popover`).
- Produces: `export function SyncStatusIndicator()` — same export, now a `Popover` whose trigger is the `StatusText` (tone and label of `syncState`, `aria-label` = `popover.title`), shown only while the flag is on and `syncState` is non-null. Content, top to bottom, styled as `DataStatus`'s:
  1. `popover.title` heading.
  2. First line: label, plus ` · ` + age when `ageFrom !== null`.
  3. Tone `stale` and `status.problem` set: the sentence `status.problem.<problem>` under it (`text-caption text-ink-3`).
  4. Rows: `popover.pending` → `status.pendingChanges`; when `status.serverAddress`: `popover.server` → `hostOf(address)` plus ` · ${t('server.insecure')}` when `serverInsecure`. (No "Dokumenty a fotky" row: that is the attachments design.)
  5. Actions: tone `stale` → one button `popover.openSettings`, a wouter `Link` to `/settings/sync` that closes the popover. Otherwise a button `status.syncNow` (`syncNow.mutate()`; `loading` while `syncNow.isPending || status.phase === 'syncing'`; a failure toasts `common:status.error` with `translateApiError`) and a link `popover.settings` + ` →` to `/settings/sync`. The popover stays open; the status event re-renders it with the round's result.

Copy (`deviceSync`):

| Key | cs | en |
|---|---|---|
| `popover.title` | Synchronizace | Sync |
| `popover.pending` | Čeká na odeslání | Waiting to be sent |
| `popover.server` | Server | Server |
| `popover.openSettings` | Otevřít nastavení | Open settings |
| `popover.settings` | Nastavení synchronizace | Sync settings |

- [ ] **Step 1: Implement** the popover (no new pure logic; the mapping is tested in Task 4).
- [ ] **Step 2: Gates** — TS gates; Expected: all pass.
- [ ] **Step 3: Commit** — `feat(sync): the top-bar sync status opens a popover`

---

### Task 6: Settings pages and the phone's Údržba follow the shared states (§6.1 note, §6.3)

**Files:**
- Modify: `src/components/settings/sync/SyncStatusCard.tsx`, `src/components/settings/sync/SyncProblemAlert.tsx`
- Modify: `src/mobile/components/sync/SyncStatusBlock.tsx`
- Modify: `src/mobile/lib/upkeep.ts` (+ `upkeep.test.ts`), `src/mobile/components/UpkeepCard.tsx`
- Modify: `src/i18n/locales/*/deviceSync.json`, `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: `syncState` (Task 4); the `sync_stalled` milestone (Task 3).
- Settings, both platforms: when `syncState(status, now)?.kind === 'unreachable'`, no alert box — `SyncProblemAlert` (desktop) and `ProblemAlert` (phone) render nothing — and a plain line `status.unreachableNote` shows above the status rows; the waiting count stays where it is (desktop row hint, phone `phone.pending` row). `unreachableLong` keeps today's alert.
- Phone Údržba (`buildUpkeepRows`): `UpkeepKind` gains `'sync_stalled'`; when the milestones contain `sync_stalled`, one row `{ id: 'sync_stalled', kind: 'sync_stalled', title: { key: 'upkeep.syncStalled', params: {} }, milestone }`, placed after `backup_stale` and before `valuation_stale`. `UpkeepCard`: `ICONS.sync_stalled = RefreshCw`, `VERBS.sync_stalled = 'upkeep.actions.open'`, `targetOf` → `settingsPath('sync')`, sub-line via `useMilestoneSubLine` (as the loan rows). Nothing is shown while sync is healthy or merely out of reach, because the milestone is absent then.

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `deviceSync` `status.unreachableNote` | Synchronizace se obnoví, až bude server v dosahu. | Sync resumes once the server is in reach. |
| `mobile` `upkeep.syncStalled` | Synchronizace se zastavila | Sync has stopped |
| `mobile` `upkeep.actions.open` | Otevřít | Open |

- [ ] **Step 1: Write the failing test** in `upkeep.test.ts`, `describe('sync_stalled')`: `it('is one row when the milestone is there, between backup and valuations')` — milestones `[backup_stale, valuation_stale, sync_stalled]` → row ids in order `['backup_stale', 'sync_stalled', 'valuation_stale:…']`, the sync row's `title.key === 'upkeep.syncStalled'` and `milestone.kind === 'sync_stalled'`; `it('is absent without the milestone')`.
- [ ] **Step 2: Run it and see it fail** — `npx vitest run src/mobile/lib/upkeep.test.ts`; Expected: FAIL.
- [ ] **Step 3: Implement** the row, the card mapping, the two Settings notes, the copy in ten locales.
- [ ] **Step 4: Gates** — TS gates; Expected: all pass.
- [ ] **Step 5: Commit** — `feat(sync): quiet out-of-reach note in Settings and a stalled-sync row in Údržba`

---

### Task 7: Smaller status items (§6.5)

**Files:**
- Modify: `src/utils/sync-devices.ts` (+ `sync-devices.test.ts`)
- Modify: `src/components/settings/sync/SyncDevicesCard.tsx`, `ChangeServerDialog.tsx`, `SyncSection.tsx`
- Create: `src/components/settings/sync/WhatSyncsLink.tsx`
- Modify: `src/mobile/pages/settings/SyncSection.tsx`, `src/mobile/components/sync/ChangeServerSheet.tsx`
- Modify: `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Produces (`sync-devices.ts`):
  ```ts
  /** The names of the account's other devices, in list order, trimmed, empty names left out. */
  export function otherDeviceNames(devices: readonly SyncDevice[] | undefined): string[];
  ```
- Lost device: desktop `SyncDevicesCard` puts the `limits.lostDevice` paragraph inside `Collapsible` (`src/components/ui/collapsible.tsx`) whose trigger reads `limits.lostTitle`; phone `SyncSection` replaces the standing paragraph by a text button `limits.lostTitle` that toggles the same paragraph (local `useState`).
- Change address: after success, `ChangeServerDialog` and `ChangeServerSheet` switch to a done screen (instead of closing with a toast): title `server.success`, body `server.repeatOn` with `devices: names.join(', ')` when `otherDeviceNames(useSyncDevices().data)` is non-empty, else `server.repeatOnOthers`; one button `common:buttons.close`.
- "Co se synchronizuje", shown in both sync states: desktop `export function WhatSyncsLink()` — a ghost button `whatSyncs.title` opening a `Dialog` with `whatSyncs.title`, `whatSyncs.body`, `common:buttons.close`; `SyncSection` renders it after the cards in both branches. Phone: a `SettingsRow` (icon `Info`) `whatSyncs.title` in both branches of `SyncSection`, opening a `Sheet` (`@/mobile/components/Sheet`) with the body.

Copy (`deviceSync`):

| Key | cs | en |
|---|---|---|
| `limits.lostTitle` | Ztratili jste zařízení nebo Záchrannou sadu? | Lost a device or the Recovery Kit? |
| `server.repeatOn` | Totéž udělejte na: {{devices}} | Do the same on: {{devices}} |
| `server.repeatOnOthers` | Totéž udělejte na ostatních zařízeních. | Do the same on your other devices. |
| `whatSyncs.title` | Co se synchronizuje | What syncs |
| `whatSyncs.body` | Synchronizuje se všechno, co do Moony zadáte. Jazyk, vzhled, menu, oznámení, heslo zařízení, obnovovací klíč a zálohy patří každému zařízení zvlášť. Ceny a historie se dopočítávají na každém zařízení. Dokumenty a fotky zatím zůstávají na zařízení, na kterém jste je přidali. | Everything you enter into Moony syncs. The language, look, menu, notifications, device password, recovery key and backups belong to each device. Prices and history are worked out on each device. Documents and photos stay on the device that added them for now. |

- [ ] **Step 1: Write the failing test** — `sync-devices.test.ts`: `it('lists the other devices by name')` — `[{ name: ' iPhone ', thisDevice: false }, { name: 'Mac', thisDevice: true }, { name: '', thisDevice: false }, { name: 'Windows PC', thisDevice: false }]` (other fields filled) → `['iPhone', 'Windows PC']`; `undefined` → `[]`.
- [ ] **Step 2: Run it and see it fail** — `npx vitest run src/utils/sync-devices.test.ts`; Expected: FAIL.
- [ ] **Step 3: Implement** the function, the two disclosures, the two done screens, `WhatSyncsLink` and the phone row/sheet, the copy in ten locales.
- [ ] **Step 4: Gates** — TS gates; Expected: all pass.
- [ ] **Step 5: Commit** — `feat(sync): lost-device disclosure, other devices after an address change, what syncs`

---

### Task 8: Desktop Settings → Sync — two choices and one shared join dialog (§5.3 desktop; sharing for §5.1)

**Files:**
- Create: `src/utils/join-context.ts`, `src/utils/join-context.test.ts`
- Create: `src/components/settings/sync/join/JoinChoice.tsx`, `PairingJoinFlow.tsx`, `RecoveryJoinFlow.tsx`, `JoinDeviceDialog.tsx`
- Modify: `src/components/settings/sync/JoinFields.tsx`, `SyncIntroCard.tsx`, `SyncSection.tsx`
- Delete: `src/components/settings/sync/JoinSyncDialog.tsx`, `RecoveryJoinDialog.tsx` (their bodies move into the two flows)
- Modify: `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Produces (`src/utils/join-context.ts`):
  ```ts
  /** Where a join runs: Settings (the profile may hold data) or the first run (a brand-new profile; the password typed a step earlier). */
  export type JoinContext = { kind: 'settings' } | { kind: 'firstRun'; password: string };
  export interface JoinPresentation {
    askPassword: boolean;        // settings: true; firstRun: false
    showReplaceWarning: boolean; // settings: true; firstRun: false (§5.1: no warning)
    showImpact: boolean;         // settings: true; firstRun: false (the impact is still read for the consent)
    cancelKey: 'buttons.cancel' | 'buttons.back'; // `common` key of the flow's way out
  }
  export function joinPresentation(context: JoinContext): JoinPresentation;
  /** The device password a join sends: the typed one in Settings, the first run's otherwise. */
  export function joinPassword(context: JoinContext, typed: string): string;
  ```
- Produces (components, named exports):
  ```ts
  export type JoinMethod = 'pair' | 'kit';
  // JoinChoice.tsx — the device name field and the two ChoiceCards in a RadioGroup (as ServerChoice uses ChoiceCard)
  export function JoinChoice(props: { name: string; onName: (v: string) => void; method: JoinMethod; onMethod: (m: JoinMethod) => void }): JSX.Element;
  export interface JoinFlowProps {
    context: JoinContext;
    /** Chosen on the JoinChoice screen; trimmed and non-empty. */
    deviceName: string;
    onJoined: (outcome: ReplaceOutcomeView) => void;
    /** Leaves the flow (Settings: closes the dialog; first run: back to the choice). Cancels a pairing first. */
    onCancel: () => void;
    /** Places the flow's buttons: Settings wraps them in `DialogFooter`, the wizard in its nav row. */
    footer: (actions: ReactNode) => ReactNode;
  }
  export function PairingJoinFlow(props: JoinFlowProps): JSX.Element;   // today's JoinSyncDialog body
  export function RecoveryJoinFlow(props: JoinFlowProps): JSX.Element;  // today's RecoveryJoinDialog body
  export function JoinDeviceDialog({ onClose }: { onClose: () => void }): JSX.Element;
  ```
  `JoinFields` loses `name`/`onName` (the name lives on `JoinChoice`) and gains `askPassword: boolean`; without the password field its `error` shows as a `role="alert"` line under the server field. The flows keep every existing behaviour (six digits, http warning and acknowledgement, server check, `IMPACT_CHANGED_KEY` re-read, "Zkusit znovu" / "Začít znovu", cancel-setup-and-retry), only shaped by `joinPresentation`: `JoinWarning` and `ReplaceImpactNotice` only when shown; `useReplaceImpact` always read and its consent always sent (`consentOf('join', impact.data)`, the zero impact in the first run); the final button (pairing at `received`: `join.confirm`; kit submit: `join.kitTitle`) reads `join.connect` in the first run.
- `JoinDeviceDialog`: screen `choice` (`DialogHeader` `intro.joinTitle` / `intro.joinLead`, `JoinChoice` with name default `t(\`setup.defaultName.${defaultDeviceKey()}\`)` and method `pair`, footer [`common:buttons.cancel`] [`common:buttons.continue`], continue disabled while the trimmed name is empty) → screen `flow` (`DialogHeader` `join.pairTitle`+`join.pairHint` or `join.kitTitle`+`join.kitHint`, the flow with `context: { kind: 'settings' }`, `footer = (a) => <DialogFooter>{a}</DialogFooter>`, `onJoined` → `replaceOutcomeToast(t, t('pairing.joined', { path: outcome.backupPath }), outcome)` then `onClose()`). The dialog cannot be dismissed while a join runs.
- `SyncIntroCard` props become `{ resuming: boolean; onStart: () => void; onJoin: () => void }`. Under the intro paragraphs, two blocks (title, lead, button): `intro.startTitle` / `intro.startLead` / button `intro.start` (when `resuming`: buttons `intro.resume` and `intro.cancelSetup` in its place, the existing confirmation kept), and `intro.joinTitle` / `intro.joinLead` / button `intro.joinButton`. No "Jak si server spustit" link. `SyncSection`: `type Open = 'setup' | 'join' | null`, mounts `JoinDeviceDialog`.

Copy (`deviceSync`; remove `join.introPair` and `join.introKit` from every locale after grepping that nothing else uses them):

| Key | cs | en |
|---|---|---|
| `intro.startTitle` | Začít synchronizovat | Start syncing |
| `intro.startLead` | Toto zařízení bude první. Potřebujete připojovací kód svého synchronizačního serveru. | This device will be the first. You need your sync server's connection code. |
| `intro.joinTitle` | Připojit toto zařízení | Connect this device |
| `intro.joinLead` | Synchronizaci už máte na jiném zařízení. | You already sync on another device. |
| `intro.joinButton` | Připojit toto zařízení | Connect this device |
| `join.choice.pair.title` | Mám u sebe telefon se synchronizací | I have a phone that syncs |
| `join.choice.pair.lead` | Zobrazí se kód. V telefonu: Nastavení → Synchronizace → Přidat zařízení. | A code appears. On the phone: Settings → Sync → Add a device. |
| `join.choice.kit.title` | Mám Záchrannou sadu synchronizace | I have the Sync Recovery Kit |
| `join.choice.kit.lead` | Zadáte kód ze sady a adresu serveru. Tak se připojuje i druhý počítač. | You enter the code from the kit and the server address. This is also how a second computer joins. |
| `join.connect` | Připojit | Connect |
| `phone.addDeviceHint` (changed) | Na novém zařízení zvolte v Nastavení → Synchronizace „Připojit toto zařízení“ a pak namiřte fotoaparát na kód, který ukáže. | On the new device, choose "Connect this device" in Settings → Sync, then point the camera at the code it shows. |

- [ ] **Step 1: Write the failing test** — `join-context.test.ts`:
  ```ts
  expect(joinPresentation({ kind: 'settings' })).toEqual({ askPassword: true, showReplaceWarning: true, showImpact: true, cancelKey: 'buttons.cancel' });
  expect(joinPresentation({ kind: 'firstRun', password: 'pw' })).toEqual({ askPassword: false, showReplaceWarning: false, showImpact: false, cancelKey: 'buttons.back' });
  expect(joinPassword({ kind: 'settings' }, 'typed')).toBe('typed');
  expect(joinPassword({ kind: 'firstRun', password: 'pw' }, '')).toBe('pw');
  ```
- [ ] **Step 2: Run it and see it fail** — `npx vitest run src/utils/join-context.test.ts`; Expected: FAIL.
- [ ] **Step 3: Implement** `join-context.ts`, the four components (moving, not copying, the two dialogs' bodies), `JoinFields`, `SyncIntroCard`, `SyncSection`; delete the two old dialogs; the copy in ten locales.
- [ ] **Step 4: Gates** — TS gates; Expected: all pass; `grep -rn "JoinSyncDialog\|RecoveryJoinDialog\|introPair\|introKit" src` finds nothing.
- [ ] **Step 5: Commit** — `feat(sync): Settings offers start or connect, with one shared join dialog`

---

### Task 9: Phone Settings → Synchronizace — two choices and the join sheet (§5.3 phone)

**Files:**
- Create: `src/mobile/components/sync/JoinChoiceSheet.tsx`
- Modify: `src/mobile/pages/settings/SyncSection.tsx`
- Modify: `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Produces:
  ```ts
  export type PhoneJoinMethod = 'scan' | 'kit' | 'paste';
  /** "Připojit toto zařízení": scan a code from the computer, or type a Recovery Kit code; with the developer tools, paste a pairing code. */
  export function JoinChoiceSheet(props: { open: boolean; onOpenChange: (open: boolean) => void; devTools: boolean; onPick: (method: PhoneJoinMethod) => void }): JSX.Element;
  ```
  Rows (`SettingsRow` inside `EntityRows`, as the page's): `QrCode` `phone.choice.scan.title` with sub `phone.choice.scan.lead`; `Type` `phone.typeCode`; with `devTools` `ClipboardPaste` `phone.devPasteTitle`. Picking closes the sheet and calls `onPick`.
- `SyncSection` while sync is off: `SettingsRow` `Smartphone` `intro.startTitle` with sub `intro.startLead` (when `settingUp`: `intro.resume` and the `intro.cancelSetup` row as today), and `SettingsRow` `Link2` (or `QrCode`) `intro.joinTitle` with sub `intro.joinLead` → `JoinChoiceSheet`; `onPick('scan')` → `setFlow({ mode: 'join', paste: false })`, `'paste'` → `paste: true`, `'kit'` → `setOpen('kitJoin')`. The old `phone.scan`, `phone.typeCode` and dev-paste rows leave the off state (the dev-paste row stays in the on state). The replace warning and impact stay in `ScanFlow` / `RecoveryJoinSheet` here.

Copy (`deviceSync`; remove `phone.scan` from every locale once unused):

| Key | cs | en |
|---|---|---|
| `phone.choice.scan.title` | Naskenovat kód z počítače | Scan a code from the computer |
| `phone.choice.scan.lead` | Na počítači: Nastavení → Synchronizace → Přidat zařízení. Naskenovat jde i QR kód Záchranné sady. | On the computer: Settings → Sync → Add a device. The scanner also takes a Recovery Kit QR. |

- [ ] **Step 1: Implement** the sheet and the page change (no new pure logic), the copy in ten locales.
- [ ] **Step 2: Gates** — TS gates; Expected: all pass.
- [ ] **Step 3: Commit** — `feat(sync): the phone's Settings offers start or connect`

---

### Task 10: Desktop first run — the join path (§5.1)

**Files:**
- Create: `src/utils/onboarding-steps.ts`, `src/utils/onboarding-steps.test.ts`
- Modify: `src/utils/form-schemas.ts`, `src/utils/form-schemas.test.ts`
- Create: `src/components/auth/onboarding/DevicePasswordForm.tsx`, `src/components/auth/onboarding/ConnectStep.tsx`
- Modify: `src/components/auth/OnboardingWizard.tsx`
- Modify: `src/i18n/locales/*/auth.json`, `src/i18n/locales/*/deviceSync.json`

**Interfaces:**
- Consumes: `JoinChoice`, `PairingJoinFlow`, `RecoveryJoinFlow`, `JoinContext` (Task 8); `REPLACE_DEVICE_MUTATION_KEY` (`src/hooks/use-device-sync-mutations.ts`); `useDeviceSyncFeature`.
- Produces (`src/utils/onboarding-steps.ts`, also used by Task 11):
  ```ts
  export type OnboardingStep = 'welcome' | 'account' | 'recovery' | 'connect' | 'areas' | 'firstAccount';
  /** `new`: today's run; `join`: Připojení after the key; `declined`: Připojení was left without sync, the normal steps follow. */
  export type OnboardingPath = 'new' | 'join' | 'declined';
  export function onboardingSteps(path: OnboardingPath): OnboardingStep[];
  /** Where the run goes once the database exists: `connect` on the join path, else `areas`. */
  export function stepAfterRecovery(path: OnboardingPath): 'connect' | 'areas';
  ```
  `new` → `[welcome, account, recovery, areas, firstAccount]`; `join` → `[welcome, account, recovery, connect]`; `declined` → `[welcome, account, recovery, connect, areas, firstAccount]`.
- Produces (`src/utils/form-schemas.ts`):
  ```ts
  /** The join path's account step: the device password twice (same rules and keys as `setupSchema`). */
  export const devicePasswordSchema: z.ZodType<{ password: string; confirmPassword: string }>;
  /** The declined path asks the name on Co sledovat: name and surname required (trimmed), e-mail optional. */
  export const profileNameSchema: z.ZodType<{ name: string; surname: string; email?: string }>;
  ```
- Produces (components):
  ```ts
  // DevicePasswordForm.tsx — the join path's step 2 (react-hook-form + zodResolver(devicePasswordSchema), PasswordInput ×2, the strength hint as the wizard's)
  export function DevicePasswordForm(props: { pending: boolean; onSubmit: (password: string) => void; onBack: () => void }): JSX.Element;
  // ConnectStep.tsx — step 4 of the join path
  export function ConnectStep(props: { password: string; onJoined: () => void; onDecline: () => void }): JSX.Element;
  ```
- Wizard behaviour (`OnboardingWizard.tsx`):
  - `const [path, setPath] = useState<OnboardingPath>('new')`; `WizardSteps` gets `onboardingSteps(path)` with `stepLabels.connect = t('onboarding.steps.connect')`.
  - Welcome footer, flag on: `onboarding.welcome.otherDeviceLead` then two text buttons, `onboarding.welcome.joinSync` (`data-testid="onboarding-join-sync"`: `setPath('join'); setStep('account')`) and the existing `RestoreBackup` link. Flag off: today's footer unchanged.
  - Account step, `path === 'join'`: title `onboarding.joinAccount.title`, lead `onboarding.joinAccount.subtitle`, `DevicePasswordForm`; submit stores the password in `const joinPassword = useRef<string | null>(null)` and calls `setupMutation.mutate({ name: '', surname: '', email: '', password, language, currency })` (the backend validates only the password and currency; the synced profile brings name, surname, e-mail and currency). Back → `setPath('new'); setStep('welcome')`.
  - `createAccountNow` success → `setStep(stepAfterRecovery(path))`.
  - Step `connect` (no back button): `<ConnectStep password={joinPassword.current} … />` (with no password in memory, which only an unexpected remount causes, the step calls `onDecline`); `onJoined` → `joinPassword.current = null`, `toast(tSync('join.firstRunDone'))`, `await finish()` (the wizard ends on the Overview; areas and first account are skipped, the menu stays the default, everything shown); `onDecline` → `joinPassword.current = null`, `setPath('declined')`, `setStep('areas')`.
  - Areas step, `path === 'declined'`: above the cards a lead `onboarding.areas.nameLead` and name / surname / e-mail fields (labels `onboarding.account.name|surname|email`) in a `useForm` with `profileNameSchema`; `saveAreas` validates them first and sends `name`, `surname`, `email` with `menuPreferences` in `authApi.updateProfile`.
- `ConnectStep`: `StepHead`-like title `onboarding.connect.title` and lead `onboarding.connect.subtitle`; screen `choice` = `JoinChoice` (name default as in Task 8, method `pair`) with the nav row [`onboarding.connect.withoutSync` (ghost) → `onDecline`] [`common:buttons.continue`]; screen `flow` = the chosen flow with `context = { kind: 'firstRun', password }`, `onCancel` → back to `choice`, `footer = (a) => <nav row: [withoutSync ghost, disabled while a join runs] [a]>`. While `useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0`: an inline block with a spinner, `onboarding.connect.fetching` and `onboarding.connect.keepOpen` above the flow, and both "withoutSync" and back disabled. The flow's own "Zkusit znovu" stays for a join that received its credentials and failed for the moment. Nothing new is stored to resume the step (§5.1).

Copy:

| Namespace / key | cs | en |
|---|---|---|
| `auth` `onboarding.steps.connect` | Připojení | Connect |
| `auth` `onboarding.welcome.otherDeviceLead` | Už Moony používáte na jiném zařízení? | Already using Moony on another device? |
| `auth` `onboarding.welcome.joinSync` | Připojit k synchronizaci | Join sync |
| `auth` `onboarding.joinAccount.title` | Heslo pro toto zařízení | Password for this device |
| `auth` `onboarding.joinAccount.subtitle` | Toto heslo odemyká Moony jen na tomto zařízení a může se lišit od hesla na vašich ostatních zařízeních. Jméno a e-mail přijdou ze synchronizovaného profilu. | This password unlocks Moony on this device only and may differ from the password on your other devices. Your name and e-mail come from the synced profile. |
| `auth` `onboarding.connect.title` | Připojit toto zařízení | Connect this device |
| `auth` `onboarding.connect.subtitle` | Data, jméno a měna přijdou z vašeho synchronizačního účtu. | Your data, name and currency come from your sync account. |
| `auth` `onboarding.connect.withoutSync` | Pokračovat bez synchronizace | Continue without sync |
| `auth` `onboarding.connect.fetching` | Stahuji synchronizovaná data… | Downloading the synced data… |
| `auth` `onboarding.connect.keepOpen` | Nezavírejte Moony. | Don't close Moony. |
| `auth` `onboarding.areas.nameLead` | Profil zatím nemá jméno. Doplňte ho, ať vás Moony může oslovit. | Your profile has no name yet. Add it so Moony can greet you. |
| `deviceSync` `join.firstRunDone` | Toto zařízení se teď synchronizuje | This device is syncing now |

- [ ] **Step 1: Write the failing tests**
  - `onboarding-steps.test.ts`: the three lists above exactly; `stepAfterRecovery('join') === 'connect'`, `stepAfterRecovery('new') === 'areas'`, `stepAfterRecovery('declined') === 'areas'`.
  - `form-schemas.test.ts`: `devicePasswordSchema` accepts `{ password: 'a'.repeat(MIN_PASSWORD_LENGTH), confirmPassword: same }`; a shorter password fails with message `'validation.passwordMinLength'`; a mismatch fails on path `['confirmPassword']` with `'validation.passwordMismatch'`. `profileNameSchema` accepts `{ name: 'Alex', surname: 'Novák', email: '' }`; `{ name: '  ', surname: 'N' }` fails with `'validation.nameRequired'` (Review Focus 1); a bad e-mail fails with `'validation.invalidEmail'`.
- [ ] **Step 2: Run them and see them fail** — `npx vitest run src/utils/onboarding-steps.test.ts src/utils/form-schemas.test.ts`; Expected: FAIL.
- [ ] **Step 3: Implement** the two pure modules, the two components and the wizard changes; the copy in ten locales.
- [ ] **Step 4: Gates** — TS gates; Expected: all pass.
- [ ] **Step 5: Commit** — `feat(onboarding): join sync from the desktop first run`

---

### Task 11: Phone first run — the join path and the protect-your-data step (§5.2)

**Files:**
- Modify: `src/mobile/lib/first-run.ts` (+ `first-run.test.ts`)
- Modify: `src/mobile/hooks/use-first-run.ts`, `src/mobile/pages/FirstRun.tsx`, `src/mobile/pages/Lock.tsx`
- Modify: `src/mobile/pages/first-run/PrivacyStep.tsx`, `AccountStep.tsx`, `AreasStep.tsx`, `BackupStep.tsx`
- Create: `src/mobile/pages/first-run/ConnectStep.tsx`
- Modify: `src/mobile/components/sync/ScanFlow.tsx`, `RecoveryJoinSheet.tsx`
- Modify: `src/i18n/locales/*/mobile.json`

**Interfaces:**
- Consumes: `OnboardingPath` (Task 10, `src/utils/onboarding-steps.ts`); `devicePasswordSchema`, `profileNameSchema` (Task 10); `joinPresentation`, `JoinContext` (Task 8); `JoinChoiceSheet`'s copy keys (Task 9); `REPLACE_DEVICE_MUTATION_KEY`.
- Produces (`first-run.ts`):
  ```ts
  export const FIRST_RUN_STEPS = ['privacy', 'account', 'recovery', 'connect', 'areas', 'firstAccount', 'biometrics', 'backup'] as const;
  /** Where the run ends: the overview, or Settings → Synchronizace ("Nastavte synchronizaci"). */
  export type FirstRunExit = 'overview' | 'sync';
  export function firstRunSteps(input: { biometryAvailable: boolean; canBackup?: boolean; path?: OnboardingPath }): FirstRunStep[];
  ```
  `path` defaults to `'new'`: `new` leaves out `connect`; `join` is `privacy, account, recovery, connect` then `biometrics` (if available) and never `backup`; `declined` is every step (`biometrics`/`backup` by the existing rules). `canLeaveWithoutAccount(step)` becomes "at or before `recovery`" (the database exists from `connect` on). `nextStep`, `previousStep`, `stepIndex` keep their signatures.
- Produces (`use-first-run.ts`, added to the returned flow):
  ```ts
  path: OnboardingPath;
  startJoin(): void;                         // privacy → account on the join path
  submitJoinPassword(password: string): void; // setupMutation with name/surname/email '' (as Task 10); keeps the password
  joinPassword: string | null;               // set from submitJoinPassword until Připojení ends
  joined(): void;                            // drops the password, toast deviceSync `join.firstRunDone`, moveOn('connect')
  declineJoin(): void;                       // drops the password, path 'declined', step 'areas'
  profileName: { name: string; surname: string; email: string };
  setProfileName(next: { name: string; surname: string; email: string }): void;
  finish(to?: FirstRunExit): Promise<void>;  // default 'overview'
  ```
  `goBack` from `account` on the join path sets the path back to `new`. `createDatabase` success on the join path keeps `joinPassword` (on `new` it clears the draft as today) and moves with `nextStep` (→ `connect`). `saveAreas` on `declined` validates `profileName` with `profileNameSchema` (a failure toasts the translated first message and stays) and sends `name`, `surname`, `email`. `FirstRun`'s `onFinished` becomes `(to: FirstRunExit) => void`; `Lock.tsx` navigates to `settingsPath('sync')` for `'sync'`, else `routes.overview`. `stepName.connect = t('onboarding.steps.connect')`.
- Steps:
  - `PrivacyStep`, flag on: `auth` `onboarding.welcome.otherDeviceLead`, then a text button `onboarding.welcome.joinSync` (`flow.startJoin`) and the existing restore link; flag off: unchanged.
  - `AccountStep`, `flow.path === 'join'`: `onboarding.joinAccount.title` / `subtitle`, two password fields (`field48`, `PasswordInput`) with `devicePasswordSchema`; submit → `flow.submitJoinPassword(password)`.
  - `ConnectStep` (new): `StepHead` `onboarding.connect.title` / `subtitle`; device name field (`field48`, default `t(\`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}\`)`); two option buttons — `phone.choice.scan.title` with lead `phone.choice.scan.lead`, and `phone.typeCode`; with `useDeviceSyncStatus().data?.devTools` a third `phone.devPasteTitle`; a text link `onboarding.connect.withoutSync` → `flow.declineJoin()`. Picking mounts `ScanFlow` (mode `join`, `paste` for the dev option) or opens `RecoveryJoinSheet`, both with `firstRun={{ password: flow.joinPassword, deviceName }}` and `onReplaced={() => flow.joined()}` (with `flow.joinPassword === null` only the "without sync" link is offered). While `useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY }) > 0`: the choices are replaced by an inline block with a spinner, `onboarding.connect.fetching` and `onboarding.connect.keepOpen`.
  - `AreasStep`, `flow.path === 'declined'`: lead `onboarding.areas.nameLead` and three `field48` inputs bound to `flow.profileName`.
  - `BackupStep`: title stays `firstRun.backupTitle` with the new value below; with the flag on, under the "Teď ne" link a caption `firstRun.syncLead` and a text link `firstRun.syncLink` → `flow.finish('sync')` (finishes without a backup).
  - `ScanFlow` gains `firstRun?: { password: string; deviceName: string }`: with it, the flow starts as if `JoinDetailsSheet` had been submitted with those details (`details.current = firstRun`, the scan starts in a mount effect; `paste` opens the paste sheet instead), `backToDetails(error)` toasts the error and calls `onDone()` (Připojení is its details screen), the impact block, the replace warning and `kitConfirmBody` are hidden (`joinPresentation({ kind: 'firstRun', password })`), and the impact is still read for the consent.
  - `RecoveryJoinSheet` gains the same `firstRun?` prop: the name and password fields and the impact block are hidden, `ready` does not wait for them, the join sends the given values.

Copy (`mobile`):

| Key | cs | en |
|---|---|---|
| `firstRun.backupTitle` (changed) | Ochraňte svá data | Protect your data |
| `firstRun.syncLead` | Máte synchronizační server? | Have a sync server? |
| `firstRun.syncLink` | Nastavte synchronizaci | Set up sync |

- [ ] **Step 1: Write the failing tests** in `first-run.test.ts` (update the existing ones for the eight-step order):
  - `it('the join path goes from the key to Připojení and ends with biometrics, never Záloha')` — `firstRunSteps({ biometryAvailable: true, canBackup: true, path: 'join' })` equals `['privacy', 'account', 'recovery', 'connect', 'biometrics']`; without biometry `['privacy', 'account', 'recovery', 'connect']`.
  - `it('a declined join continues with every normal step')` — `path: 'declined'`, biometry and backup → all eight steps.
  - `it('the new path has no Připojení')` — default path → `['privacy', 'account', 'recovery', 'areas', 'firstAccount', 'biometrics', 'backup']`.
  - `it('has no way back from Připojení')` — `canLeaveWithoutAccount('connect') === false`, `previousStep(joinSteps, 'connect') === null`, `canLeaveWithoutAccount('recovery') === true`.
  - `it('ends the join path after Připojení without biometry')` — `nextStep(['privacy','account','recovery','connect'], 'connect') === null`.
- [ ] **Step 2: Run them and see them fail** — `npx vitest run src/mobile/lib/first-run.test.ts`; Expected: FAIL.
- [ ] **Step 3: Implement** `first-run.ts`, the hook, the steps, `ScanFlow`/`RecoveryJoinSheet` first-run mode, `FirstRun`/`Lock` exit, the copy in ten locales.
- [ ] **Step 4: Gates** — TS gates; Expected: all pass.
- [ ] **Step 5: Commit** — `feat(mobile): join sync from the phone's first run`

---

### Task 12: Docs and manual verification (§10)

**Files:**
- Modify: `docs/architecture/overview.md` (device-sync section: the first-run join, `sync_stalled`, the one mapping `src/utils/sync-state.ts` with its 7-day switch and the transient problems)

- [ ] **Step 1: Docs** — add one bullet per item above to the device-sync section of `docs/architecture/overview.md`, naming files by role, no line numbers.

- [ ] **Step 2: Full gates** — `npm run lint && npm run typecheck && npm run format:check && npm test` and `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; Expected: all pass.

- [ ] **Step 3: Start a local sync server** (background shell): build `/Users/filipkral/Programování/FinanceApp/Moony-relay` (`npm run build`), then `PORT=8797 SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start`. In its admin (`http://127.0.0.1:8797`) set up the admin, save the address and add a person (recipe: `docs/architecture/overview.md`, "Run it against a local sync server").

- [ ] **Step 4: A first device with data** — start the app as a background shell command:
  `npm run tauri dev -- --config '{"identifier":"com.moonyfinance.desktop.syncux","build":{"devUrl":"http://localhost:1423","beforeDevCommand":"npm run dev -- --port 1423 --strictPort"}}'`
  Finish the normal first run with throwaway values (record the password in no file outside the scratchpad), add an account, Settings → Synchronizace → "Začít synchronizovat" with the person's connection code; save the Recovery Kit text to the scratchpad. Expected: the top bar shows "Synchronizováno"; its popover shows the age, "Čeká na odeslání 0", the server `127.0.0.1:8797`, "Synchronizovat" and "Nastavení synchronizace →". Quit the app.

- [ ] **Step 5: A fresh device joins by Recovery Kit from the first run** — start again with identifier `com.moonyfinance.desktop.syncux2` (same command, other identifier). Step 1 shows "Už Moony používáte na jiném zařízení?" with "Připojit k synchronizaci"; take it, set a password, confirm the key, choose "Mám Záchrannou sadu synchronizace", enter the kit code and server. Expected: no replace warning or impact list, inline "Stahuji synchronizovaná data… Nezavírejte Moony.", then the Overview with the first device's data and the toast "Toto zařízení se teď synchronizuje"; the UI language stays the one picked; Settings → Data/Restore lists the join backup without `-kept` in its name.

- [ ] **Step 6: Decline and retry** — with a third fresh identifier (`…syncux3`): join path to Připojení, enter a wrong kit code (error shown, retry possible), then "Pokračovat bez synchronizace". Expected: Co sledovat asks for name and surname, then První účet; the run ends on the Overview.

- [ ] **Step 7: Status rows** — on the second device, stop the sync server. Expected: "Server mimo dosah" (neutral), Settings shows "Synchronizace se obnoví, až bude server v dosahu." without an alert box, no `sync_stalled` in the agenda. The 7-day switch and the other rows are covered by the Task 3/4 tests (injected clock); do not change the system clock.

- [ ] **Step 8: Settings two choices** — on the third device, Settings → Synchronizace shows "Začít synchronizovat" and "Připojit toto zařízení" (no "Jak si server spustit"); "Připojit toto zařízení" opens the choice, then the flow with the replace warning and impact list; "Co se synchronizuje" opens its text; cancel.

- [ ] **Step 9: Phone (simulator), if available** — per `docs/architecture/overview.md` ("Pair two devices locally"), with `MOONY_RELAY_URL` pointing at `http://127.0.0.1:8797` and a phone dev-bridge port other than 1420–1422: the first run's join path reaches Připojení, the dev paste option joins from the desktop's "Přidat zařízení" code, and the run ends after Biometrie without Záloha. If the simulator cannot be used in this session, say so in the report; the owner's iPhone check stays the owner's.

- [ ] **Step 10: Clean up** — stop the app and the server; remove the temp data dir and any `.cargo/config.toml` created for the simulator.

- [ ] **Step 11: Commit** — `docs(sync): the sync-aware app in the architecture overview`
