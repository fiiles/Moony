# Milestones as an agenda — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn "Co vás čeká" into a two-column agenda with date chips, fold data upkeep into one quiet line, let every item be hidden / reminded later / muted by group, and show in the top bar only deadlines at most 14 days away.

**Architecture:** Backend: remove `canDismiss`, put the data date into upkeep keys, snooze upkeep for 30 days, store muted kinds in `app_config` (`milestones.mutedKinds`) and filter them in `list_milestones`, two thin commands. Frontend: pure utils decide agenda vs. upkeep, the agenda day, urgency and the top-bar deadlines; components render the agenda, the upkeep line, a "···" menu per row, the top-bar status and a settings card.

**Tech Stack:** Rust (rusqlite, serde_json), React 19 + TypeScript, TanStack Query, Radix (shadcn) DropdownMenu/Popover/Switch, react-i18next, Vitest, lucide-react.

**Spec:** `docs/specs/2026-10-05-milestones-agenda-design.md` (revises `docs/specs/2026-10-05-milestones-design.md`).

## Global Constraints

- Never call `invoke()` outside `src/lib/tauri-api.ts`.
- `shared/schema.ts` mirrors Rust; regenerate `shared/generated-types.ts` with `cd src-tauri && cargo test generate_bindings -- --ignored` and commit it.
- Commands are thin; logic in `src-tauri/src/services/`; errors are `AppError`; no `.unwrap()` outside tests.
- No new migration (muted kinds live in the existing `app_config` key-value table).
- Every user-facing string in BOTH `src/i18n/locales/en/` and `src/i18n/locales/cs/`.
- Design system (`docs/design-system/README.md`): tokens only (`bg-well`, `text-ink-*`, `border-line-soft`, `bg-dark-grad`, `text-ink-inverse`, `text-ink-inverse-2`, `shadow-dark`); dark material only for a deadline at most 7 days away; buttons and menu items are verbs; toasts past tense with "Vrátit".
- Never nest an interactive element inside the row's `<Link>`.
- Logging: no names, tickers or amounts above `debug`.
- Rust service tests use `Connection::open_in_memory()` via `services/milestones/test_support.rs`.
- Before every commit `git branch --show-current` must print `feature/milestones-agenda`; run `cd src-tauri && cargo fmt` before committing Rust.
- Commit messages: Conventional Commits, ending with a `Co-Authored-By:` line naming the model you run on.

---

### Task 1: Backend — hide anything, upkeep keys with data dates, monthly upkeep snooze, muted kinds

**Files:**
- Modify: `src-tauri/src/models/milestones.rs`
- Modify: `src-tauri/src/services/milestones/{mod,upkeep,accounts,loans}.rs` (+ tests)
- Modify: `src-tauri/src/commands/milestones.rs`, `src-tauri/src/lib.rs`
- Modify: `shared/generated-types.ts` (regenerated), `shared/schema.ts`, `src/lib/tauri-api.ts`
- Modify: `src/i18n/locales/{en,cs}/common.json` (validation key)
- Modify (compile fixes only): `src/components/milestones/MilestoneList.tsx`, `src/utils/milestones.test.ts`

**Interfaces:**
- Produces (Rust): `models::MILESTONE_KINDS: [&str; 15]`, `models::validate_milestone_kinds(&[String]) -> Result<()>`; `services::milestones::{muted_kinds(conn) -> Result<Vec<String>>, set_muted_kinds(conn, &[String]) -> Result<Vec<String>>}`; `Milestone` without `can_dismiss`; commands `get_milestone_muted_kinds() -> Vec<String>`, `set_milestone_muted_kinds(kinds: Vec<String>) -> Vec<String>`.
- Produces (TS): `Milestone` without `canDismiss`; `milestonesApi.mutedKinds(): Promise<MilestoneKind[]>`, `milestonesApi.setMutedKinds(kinds: MilestoneKind[]): Promise<MilestoneKind[]>`.

- [ ] **Step 1: Write the failing tests.** In `src-tauri/src/services/milestones/mod.rs` `mod tests` (it has `setup`, `day`, `policy`, `DAY`) add:

```rust
    #[test]
    fn hiding_an_upkeep_item_lasts_until_the_data_changes() {
        let conn = setup();
        conn.execute("INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)", [day(2026, 1, 1)])
            .unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', ?1)",
            [day(2026, 8, 1).to_string()],
        )
        .unwrap();
        let today = day(2026, 10, 5);
        let first = list_milestones(&conn, today).unwrap();
        assert_eq!(first[0].key, format!("backup_stale:{}", day(2026, 8, 1)));
        set_milestone_state(&conn, &first[0].key, MILESTONE_STATE_DONE, today).unwrap();
        assert!(list_milestones(&conn, today).unwrap().is_empty());
        // A newer backup that grows old again is a new occurrence.
        conn.execute(
            "UPDATE app_config SET value = ?1 WHERE key = 'backup.lastCreatedAt'",
            [day(2026, 8, 20).to_string()],
        )
        .unwrap();
        let again = list_milestones(&conn, today).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].key, format!("backup_stale:{}", day(2026, 8, 20)));
    }

    #[test]
    fn upkeep_snoozes_for_a_month_and_dates_for_a_week() {
        let conn = setup();
        let today = day(2026, 10, 5);
        let dated = format!("insurance_end:p1:{}", day(2026, 11, 1));
        set_milestone_state(&conn, "backup_stale:never", MILESTONE_STATE_SNOOZED, today).unwrap();
        set_milestone_state(&conn, &dated, MILESTONE_STATE_SNOOZED, today).unwrap();
        let until = |key: &str| -> i64 {
            conn.query_row("SELECT until_day FROM milestone_states WHERE key = ?1", [key], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(until("backup_stale:never"), today + 30 * DAY);
        assert_eq!(until(&dated), today + 7 * DAY);
    }

    #[test]
    fn muted_kinds_are_left_out_and_round_trip() {
        let conn = setup();
        policy(&conn, "y", day(2025, 10, 20), None, "annually", "4200");
        let today = day(2026, 10, 6);
        assert!(list_milestones(&conn, today).unwrap().iter().any(|m| m.kind == "insurance_payment"));
        let stored = set_muted_kinds(
            &conn,
            &["insurance_payment".to_string(), " insurance_payment ".to_string()],
        )
        .unwrap();
        assert_eq!(stored, vec!["insurance_payment".to_string()]);
        assert_eq!(muted_kinds(&conn).unwrap(), stored);
        assert!(list_milestones(&conn, today).unwrap().iter().all(|m| m.kind != "insurance_payment"));
        assert!(set_muted_kinds(&conn, &[]).unwrap().is_empty());
        assert!(muted_kinds(&conn).unwrap().is_empty());
    }

    #[test]
    fn unreadable_muted_kinds_mute_nothing() {
        let conn = setup();
        conn.execute("INSERT INTO app_config (key, value) VALUES ('milestones.mutedKinds', 'not json')", [])
            .unwrap();
        assert!(muted_kinds(&conn).unwrap().is_empty());
    }
```

In `src-tauri/src/models/milestones.rs` tests add:

```rust
    #[test]
    fn muted_kinds_must_be_known() {
        assert!(validate_milestone_kinds(&["watch_target".to_string(), "backup_stale".to_string()]).is_ok());
        assert!(validate_milestone_kinds(&[]).is_ok());
        assert!(validate_milestone_kinds(&["nope".to_string()]).is_err());
    }
```

Also extend the existing builder tests so they pin the new upkeep keys: in `accounts.rs` the stale-balances test asserts `list[0].key == format!("balances_stale:{}", day(2026, 7, 1))`; in `upkeep.rs` the valuation test asserts `list[0].key == format!("valuation_stale:old:{}", day(2025, 3, 12))` and the never-backed-up test asserts `list[0].key == "backup_stale:never"`; in `loans.rs` the balance-check test asserts `checks[0].key == format!("loan_balance_check:old:{}", day(2020, 1, 1))`. Remove every `can_dismiss` assertion.

Run: `cd src-tauri && cargo test milestones` — Expected: FAIL to compile (`set_muted_kinds`, `validate_milestone_kinds` missing).

- [ ] **Step 2: Model.** In `models/milestones.rs`: delete the `can_dismiss` field (and its doc comment) from `Milestone`; add after the state constants:

```rust
/// Every kind the service emits; muted kinds must be one of these.
pub const MILESTONE_KINDS: [&str; 15] = [
    "insurance_anniversary",
    "insurance_end",
    "insurance_payment",
    "loan_fixation_end",
    "loan_fixation_expired",
    "loan_payoff",
    "loan_balance_check",
    "bond_maturity",
    "bond_coupon",
    "account_termination",
    "savings_rate_end",
    "balances_stale",
    "backup_stale",
    "valuation_stale",
    "watch_target",
];

/// Muted kinds ("Nepřipomínat …") must be kinds the service knows.
pub fn validate_milestone_kinds(kinds: &[String]) -> Result<()> {
    if kinds.iter().all(|k| MILESTONE_KINDS.contains(&k.trim())) {
        Ok(())
    } else {
        Err(AppError::Validation("validation.milestoneKindUnknown".to_string()))
    }
}
```

- [ ] **Step 3: Service.** In `services/milestones/mod.rs`:
  - Remove `can_dismiss: true,` from `milestone()`; update the module doc ("hidden or snoozed", "muted kinds").
  - Add constants next to `SNOOZE_DAYS`:

```rust
/// Upkeep is snoozed for a month: a weekly reminder to revalue a flat is noise.
const UPKEEP_SNOOZE_DAYS: i64 = 30;
/// Undated data-upkeep kinds (their keys carry the date of the data they are about).
pub(crate) const UPKEEP_KINDS: [&str; 5] = [
    "backup_stale",
    "balances_stale",
    "valuation_stale",
    "loan_balance_check",
    "loan_fixation_expired",
];
/// `app_config` key holding the muted kinds as a JSON array.
const MUTED_KINDS_KEY: &str = "milestones.mutedKinds";
```

  - `set_milestone_state`: the snooze length depends on the key's kind (the part before the first `:`):

```rust
    let kind = key.trim().split(':').next().unwrap_or_default();
    let days = if UPKEEP_KINDS.contains(&kind) { UPKEEP_SNOOZE_DAYS } else { SNOOZE_DAYS };
    let until = (state == MILESTONE_STATE_SNOOZED).then_some(today + days * DAY);
```

  - Add:

```rust
/// Kinds the user turned off ("Nepřipomínat …"), sorted; empty when none or unreadable.
pub fn muted_kinds(conn: &Connection) -> Result<Vec<String>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM app_config WHERE key = ?1", [MUTED_KINDS_KEY], |r| r.get(0))
        .optional()?;
    Ok(match raw {
        None => Vec::new(),
        Some(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("[MILESTONES] muted kinds unreadable: {e}");
            Vec::new()
        }),
    })
}

/// Replace the muted kinds (callers validate them first). Stored trimmed, sorted and without
/// duplicates; an empty list removes the setting. Returns what was stored.
pub fn set_muted_kinds(conn: &Connection, kinds: &[String]) -> Result<Vec<String>> {
    let mut kinds: Vec<String> = kinds.iter().map(|k| k.trim().to_string()).collect();
    kinds.sort();
    kinds.dedup();
    if kinds.is_empty() {
        conn.execute("DELETE FROM app_config WHERE key = ?1", [MUTED_KINDS_KEY])?;
    } else {
        let json = serde_json::to_string(&kinds)
            .map_err(|e| AppError::Internal(format!("muted kinds: {e}")))?;
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![MUTED_KINDS_KEY, json],
        )?;
    }
    Ok(kinds)
}
```

  - `list_milestones`: after collecting the builders' items and before the state filter: `let muted = muted_kinds(conn)?; all.retain(|m| !muted.contains(&m.kind));`. Import `OptionalExtension` and `AppError`.
- [ ] **Step 4: Upkeep keys carry the data date; drop `can_dismiss` everywhere.**
  - `upkeep.rs` backup: key `format!("backup_stale:{}", since_day.map_or_else(|| "never".to_string(), |d| d.to_string()))`. Valuations: key `format!("valuation_stale:{id}:{last}")`.
  - `accounts.rs`: build the balances item from the oldest day without a fallback: `if let Some(oldest_day) = oldest { let mut m = milestone("balances_stale", format!("balances_stale:{oldest_day}"), None, ""); m.count = Some(stale); m.since_day = Some(oldest_day); out.push(m); }` (`oldest` is set exactly when `stale > 0`).
  - `loans.rs` balance check: key `format!("loan_balance_check:{}:{checked}", loan.id)`.
  - Delete every `m.can_dismiss = false;` line.
- [ ] **Step 5: Commands.** In `commands/milestones.rs` (import `validate_milestone_kinds`):

```rust
/// Kinds turned off in Settings → Obecné → Připomínky na přehledu.
#[tauri::command]
pub async fn get_milestone_muted_kinds(db: State<'_, Database>) -> Result<Vec<String>> {
    db.with_conn(milestones::muted_kinds)
}

/// Replace the muted kinds; returns what was stored (sorted, without duplicates).
#[tauri::command]
pub async fn set_milestone_muted_kinds(
    db: State<'_, Database>,
    kinds: Vec<String>,
) -> Result<Vec<String>> {
    validate_milestone_kinds(&kinds)?;
    db.with_conn(|conn| milestones::set_muted_kinds(conn, &kinds))
}
```

Register both in `lib.rs` under the `// Milestones` group.
- [ ] **Step 6: Run Rust gates and regenerate bindings.**
Run: `cd src-tauri && cargo test milestones && cargo test generate_bindings -- --ignored && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS; `generated-types.ts` loses `canDismiss`.
- [ ] **Step 7: TS contract.** `shared/schema.ts`: remove `canDismiss` (and its doc) from `Milestone`. `src/lib/tauri-api.ts` `milestonesApi` (import `MilestoneKind`):

```ts
  mutedKinds: () => tauriInvoke<MilestoneKind[]>('get_milestone_muted_kinds'),
  setMutedKinds: (kinds: MilestoneKind[]) =>
    tauriInvoke<MilestoneKind[]>('set_milestone_muted_kinds', { kinds }),
```

Compile fixes only: in `MilestoneList.tsx` render the done button unconditionally (Task 3 rewrites the file); in `src/utils/milestones.test.ts` remove `canDismiss` from the fixture. Validation copy in `common.json` under `validation`: en `"milestoneKindUnknown": "This kind of reminder is unknown."`, cs `"milestoneKindUnknown": "Tento druh připomínky aplikace nezná."`.
Run: `npm run typecheck && npm run lint && npm test` — Expected: PASS.
- [ ] **Step 8: Commit** (`feat(milestones): hide any item, monthly upkeep snooze and muted kinds`).

---

### Task 2: Frontend logic, hooks and copy

**Files:**
- Modify: `src/utils/milestones.ts`, `src/utils/milestones.test.ts`
- Modify: `src/hooks/use-milestones.ts`, `src/hooks/use-milestone-mutations.ts`
- Replace: `src/i18n/locales/{cs,en}/milestones.json`
- Modify (compile only, until Task 3): `src/components/dashboard/UpcomingCard.tsx`, `src/components/shell/MilestonesStatus.tsx`, `src/components/milestones/MilestoneList.tsx`

**Interfaces:**
- Consumes: `milestonesApi.mutedKinds/setMutedKinds` (Task 1).
- Produces: in `src/utils/milestones.ts`: `UPKEEP_KINDS`, `DEADLINE_KINDS`, `type MilestoneGroupId`, `MILESTONE_GROUP_IDS`, `groupOf(kind): MilestoneGroupId`, `kindsOf(group): MilestoneKind[]`, `isUpkeep(m)`, `isDeadline(m)`, `agendaDay(m, today)`, `splitMilestones(list, today): { agenda; upkeep }`, `isUrgent(m, today)`, `upcomingDeadlines(list, today, withinDays = 14)`, `columns<T>(items): [T[], T[]]`; kept: `milestoneHref`, `daysBetween`, `relativeLabel`. Removed: `isAttention`, `groupMilestones`, `attentionCount`, `referenceDay`. Hooks: `useMilestoneMutedKinds()` (key `['milestone-muted-kinds']`), `useMilestoneActions(): { hide(m), remindLater(m), muteGroup(group), setGroupMuted(group, muted) }`.

- [ ] **Step 1: Failing tests.** Replace `src/utils/milestones.test.ts` with tests for the new API (keep the existing `milestoneHref`, `daysBetween` and `relativeLabel` cases; the fixture has no `canDismiss`):

```ts
import { describe, expect, it } from 'vitest';
import type { Milestone, MilestoneKind } from '@shared/schema';
import {
  MILESTONE_GROUP_IDS,
  agendaDay,
  columns,
  daysBetween,
  groupOf,
  isUrgent,
  kindsOf,
  milestoneHref,
  relativeLabel,
  splitMilestones,
  upcomingDeadlines,
} from './milestones';

const DAY = 86_400;
const TODAY = 20_000 * DAY;

function m(overrides: Partial<Milestone>): Milestone {
  return {
    key: 'k',
    kind: 'insurance_end',
    stage: 'now',
    tone: 'action',
    sourceId: 'p1',
    title: 'Pojistka',
    dueDay: null,
    actionDay: null,
    sinceDay: null,
    amount: null,
    currency: null,
    referenceAmount: null,
    direction: null,
    count: null,
    ...overrides,
  };
}

const ALL_KINDS: MilestoneKind[] = [
  'insurance_anniversary', 'insurance_end', 'insurance_payment', 'loan_fixation_end',
  'loan_fixation_expired', 'loan_payoff', 'loan_balance_check', 'bond_maturity', 'bond_coupon',
  'account_termination', 'savings_rate_end', 'balances_stale', 'backup_stale', 'valuation_stale',
  'watch_target',
];

describe('agendaDay', () => {
  it('is the notice deadline while it can be met, else the event, and today for a target', () => {
    const anniversary = m({ kind: 'insurance_anniversary', dueDay: TODAY + 66 * DAY, actionDay: TODAY + 23 * DAY });
    expect(agendaDay(anniversary, TODAY)).toBe(TODAY + 23 * DAY);
    expect(agendaDay({ ...anniversary, tone: 'info' }, TODAY)).toBe(TODAY + 66 * DAY);
    expect(agendaDay(m({ kind: 'watch_target' }), TODAY)).toBe(TODAY);
  });
});

describe('splitMilestones', () => {
  it('sorts the agenda by its day and folds upkeep apart, keeping the backend order of upkeep', () => {
    const payment = m({ key: 'pay', kind: 'insurance_payment', tone: 'action', dueDay: TODAY + 60 * DAY });
    const deadline = m({ key: 'end', kind: 'insurance_end', dueDay: TODAY + 10 * DAY });
    const target = m({ key: 'aapl', kind: 'watch_target' });
    const backup = m({ key: 'b', kind: 'backup_stale', sinceDay: TODAY - 40 * DAY });
    const valuation = m({ key: 'v', kind: 'valuation_stale', sinceDay: TODAY - 400 * DAY });
    const { agenda, upkeep } = splitMilestones([payment, backup, deadline, valuation, target], TODAY);
    expect(agenda.map((x) => x.key)).toEqual(['aapl', 'end', 'pay']);
    expect(upkeep.map((x) => x.key)).toEqual(['b', 'v']);
  });
});

describe('upcomingDeadlines and isUrgent', () => {
  it('keeps deadlines up to 14 days ahead, never information, upkeep or targets', () => {
    const soon = m({ key: 'rate', kind: 'savings_rate_end', dueDay: TODAY + 10 * DAY });
    const later = m({ key: 'fix', kind: 'loan_fixation_end', dueDay: TODAY + 15 * DAY });
    const info = m({ key: 'coupon', kind: 'bond_coupon', tone: 'info', dueDay: TODAY + 3 * DAY });
    const passed = m({ key: 'ann', kind: 'insurance_anniversary', tone: 'info', dueDay: TODAY + 5 * DAY, actionDay: TODAY - 1 * DAY });
    const target = m({ key: 't', kind: 'watch_target' });
    const backup = m({ key: 'b', kind: 'backup_stale' });
    const first = m({ key: 'end', kind: 'insurance_end', dueDay: TODAY + 2 * DAY });
    expect(upcomingDeadlines([soon, later, info, passed, target, backup, first], TODAY).map((x) => x.key)).toEqual(['end', 'rate']);
    expect(isUrgent(first, TODAY)).toBe(true);
    expect(isUrgent(soon, TODAY)).toBe(false);
    expect(isUrgent(info, TODAY)).toBe(false);
  });
});

describe('groups', () => {
  it('every kind belongs to exactly one group', () => {
    const grouped = MILESTONE_GROUP_IDS.flatMap((g) => kindsOf(g));
    expect([...grouped].sort()).toEqual([...ALL_KINDS].sort());
    expect(groupOf('loan_fixation_expired')).toBe('loanChecks');
    expect(groupOf('insurance_payment')).toBe('insurancePayments');
  });
});

describe('columns', () => {
  it('fills the first column first', () => {
    expect(columns([1, 2, 3, 4, 5])).toEqual([[1, 2, 3], [4, 5]]);
    expect(columns([])).toEqual([[], []]);
  });
});
```

(Keep the existing `milestoneHref`, `daysBetween`/`relativeLabel` describe blocks below these, adjusted to the new fixture.)
Run: `npx vitest run src/utils/milestones.test.ts` — Expected: FAIL (missing exports).

- [ ] **Step 2: Implement `src/utils/milestones.ts`.** Keep `milestoneHref`, `daysBetween`, `relativeLabel` unchanged; delete `isAttention`, `groupMilestones`, `attentionCount`, `referenceDay`; add:

```ts
/** Undated data upkeep: folded into one line under the agenda, never in the top bar. */
export const UPKEEP_KINDS: readonly MilestoneKind[] = [
  'backup_stale',
  'balances_stale',
  'valuation_stale',
  'loan_balance_check',
  'loan_fixation_expired',
];

/** Dates the user can miss: the only kinds the top bar shows. */
export const DEADLINE_KINDS: readonly MilestoneKind[] = [
  'insurance_anniversary',
  'insurance_end',
  'loan_fixation_end',
  'bond_maturity',
  'account_termination',
  'savings_rate_end',
];

export type MilestoneGroupId =
  | 'insuranceDates'
  | 'insurancePayments'
  | 'loans'
  | 'bonds'
  | 'accounts'
  | 'targets'
  | 'backup'
  | 'balances'
  | 'valuations'
  | 'loanChecks';

/** "Nepřipomínat …" and the settings switches mute whole groups of kinds. */
const GROUP_OF: Record<MilestoneKind, MilestoneGroupId> = {
  insurance_anniversary: 'insuranceDates',
  insurance_end: 'insuranceDates',
  insurance_payment: 'insurancePayments',
  loan_fixation_end: 'loans',
  loan_payoff: 'loans',
  bond_maturity: 'bonds',
  bond_coupon: 'bonds',
  account_termination: 'accounts',
  savings_rate_end: 'accounts',
  watch_target: 'targets',
  backup_stale: 'backup',
  balances_stale: 'balances',
  valuation_stale: 'valuations',
  loan_balance_check: 'loanChecks',
  loan_fixation_expired: 'loanChecks',
};

/** Groups in settings order. */
export const MILESTONE_GROUP_IDS: readonly MilestoneGroupId[] = [
  'insuranceDates',
  'insurancePayments',
  'loans',
  'bonds',
  'accounts',
  'targets',
  'backup',
  'balances',
  'valuations',
  'loanChecks',
];

export function groupOf(kind: MilestoneKind): MilestoneGroupId {
  return GROUP_OF[kind];
}

export function kindsOf(group: MilestoneGroupId): MilestoneKind[] {
  return (Object.keys(GROUP_OF) as MilestoneKind[]).filter((kind) => GROUP_OF[kind] === group);
}

export function isUpkeep(m: Milestone): boolean {
  return UPKEEP_KINDS.includes(m.kind);
}

/** A deadline that can still be met (an anniversary past its notice deadline is information). */
export function isDeadline(m: Milestone): boolean {
  return DEADLINE_KINDS.includes(m.kind) && m.tone === 'action';
}

/**
 * The day an agenda row shows: the notice deadline while it can be met, otherwise the
 * event; a crossed target is today.
 */
export function agendaDay(m: Milestone, today: number): number {
  if (m.kind === 'watch_target') return today;
  if (m.actionDay !== null && m.tone === 'action') return m.actionDay;
  return m.dueDay ?? today;
}

/** Agenda (sorted by its day; ties keep the backend order) and data upkeep. */
export function splitMilestones(
  list: readonly Milestone[],
  today: number
): { agenda: Milestone[]; upkeep: Milestone[] } {
  const agenda = list.filter((m) => !isUpkeep(m));
  const upkeep = list.filter(isUpkeep);
  agenda.sort((a, b) => agendaDay(a, today) - agendaDay(b, today));
  return { agenda, upkeep };
}

/** A deadline at most 7 days away: the only rows with a dark date chip. */
export function isUrgent(m: Milestone, today: number): boolean {
  return isDeadline(m) && daysBetween(today, agendaDay(m, today)) <= 7;
}

/** Deadlines from today up to `withinDays` ahead, nearest first (the top bar). */
export function upcomingDeadlines(
  list: readonly Milestone[],
  today: number,
  withinDays = 14
): Milestone[] {
  return list
    .filter((m) => {
      if (!isDeadline(m)) return false;
      const days = daysBetween(today, agendaDay(m, today));
      return days >= 0 && days <= withinDays;
    })
    .sort((a, b) => agendaDay(a, today) - agendaDay(b, today));
}

/** Two columns filled top to bottom, the first one first. */
export function columns<T>(items: readonly T[]): [T[], T[]] {
  const half = Math.ceil(items.length / 2);
  return [items.slice(0, half), items.slice(half)];
}
```

Import `MilestoneKind` with `Milestone`. Run the test file — Expected: PASS.

- [ ] **Step 3: Hooks.** `src/hooks/use-milestones.ts` add:

```ts
/** Kinds muted in settings or through "Nepřipomínat …". */
export function useMilestoneMutedKinds() {
  return useQuery({
    queryKey: ['milestone-muted-kinds'],
    queryFn: () => milestonesApi.mutedKinds(),
  });
}
```

Replace `src/hooks/use-milestone-mutations.ts`:

```ts
import { useMutation } from '@tanstack/react-query';
import { toast } from 'sonner';
import { useTranslation } from 'react-i18next';
import type { Milestone, MilestoneKind, MilestoneState } from '@shared/schema';
import { queryClient } from '@/lib/queryClient';
import { milestonesApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { isUpkeep, kindsOf, type MilestoneGroupId } from '@/utils/milestones';

/**
 * "Skrýt", "Připomenout za týden / měsíc" and "Nepřipomínat …", each with a "Vrátit" toast.
 * Every successful mutation refreshes ["milestones"] through the query client.
 */
export function useMilestoneActions() {
  const { t } = useTranslation('milestones');
  const { t: tc } = useTranslation('common');
  const { data: muted = [] } = useMilestoneMutedKinds();

  const onError = (error: Error) => {
    toast.error(tc('status.error'), { description: translateApiError(error, tc) });
  };

  const undo = useMutation({
    mutationFn: (key: string) => milestonesApi.clearState(key),
    onError,
  });

  const setState = useMutation({
    mutationFn: ({ m, state }: { m: Milestone; state: MilestoneState }) =>
      milestonesApi.setState(m.key, state),
    onSuccess: (_data, { m, state }) => {
      const message =
        state === 'done'
          ? t('toast.hidden')
          : t(isUpkeep(m) ? 'toast.snoozedMonth' : 'toast.snoozedWeek');
      toast(message, { action: { label: t('actions.undo'), onClick: () => undo.mutate(m.key) } });
    },
    onError,
  });

  const setMuted = useMutation({
    mutationFn: (kinds: MilestoneKind[]) => milestonesApi.setMutedKinds(kinds),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['milestone-muted-kinds'] }),
    onError,
  });

  const withGroup = (group: MilestoneGroupId, on: boolean): MilestoneKind[] => {
    const kinds = kindsOf(group);
    return on
      ? Array.from(new Set([...muted, ...kinds]))
      : muted.filter((kind) => !kinds.includes(kind));
  };

  return {
    hide: (m: Milestone) => setState.mutate({ m, state: 'done' }),
    remindLater: (m: Milestone) => setState.mutate({ m, state: 'snoozed' }),
    muteGroup: (group: MilestoneGroupId) => {
      const previous = muted;
      setMuted.mutate(withGroup(group, true), {
        onSuccess: () =>
          toast(t('toast.muted', { group: t(`groups.${group}.label`) }), {
            action: { label: t('actions.undo'), onClick: () => setMuted.mutate(previous) },
          }),
      });
    },
    setGroupMuted: (group: MilestoneGroupId, mutedOn: boolean) =>
      setMuted.mutate(withGroup(group, mutedOn)),
  };
}
```

- [ ] **Step 4: Copy.** Replace `src/i18n/locales/cs/milestones.json` with:

```json
{
  "card": {
    "title": "Co vás čeká",
    "settingsLink": "Upravit připomínky",
    "showAll": "Zobrazit vše ({{count}})",
    "showLess": "Zobrazit méně",
    "nothingDue": "V příštích 90 dnech nic nevyprší.",
    "upkeep": "Údržba dat",
    "upkeepShow": "Zobrazit {{count}}",
    "upkeepHide": "Sbalit",
    "today": "Dnes",
    "target": "cíl",
    "rowMenu": "Možnosti připomínky"
  },
  "status": {
    "deadline": "{{title}} · {{when}}",
    "more": "+{{count}}",
    "title": "Termíny do 14 dní",
    "openOverview": "Otevřít přehled"
  },
  "actions": {
    "hide": "Skrýt",
    "remindWeek": "Připomenout za týden",
    "remindMonth": "Připomenout za měsíc",
    "undo": "Vrátit"
  },
  "toast": {
    "hidden": "Skryto",
    "snoozedWeek": "Odloženo o týden",
    "snoozedMonth": "Odloženo o měsíc",
    "muted": "Připomínky vypnuty · {{group}}"
  },
  "relative": {
    "today": "dnes",
    "tomorrow": "zítra",
    "inDays_one": "za {{count}} den",
    "inDays_few": "za {{count}} dny",
    "inDays_other": "za {{count}} dní",
    "inMonths_one": "za {{count}} měsíc",
    "inMonths_few": "za {{count}} měsíce",
    "inMonths_other": "za {{count}} měsíců",
    "daysAgo_one": "před {{count}} dnem",
    "daysAgo_few": "před {{count}} dny",
    "daysAgo_other": "před {{count}} dny",
    "monthsAgo_one": "před {{count}} měsícem",
    "monthsAgo_few": "před {{count}} měsíci",
    "monthsAgo_other": "před {{count}} měsíci"
  },
  "titles": {
    "backup_stale": "Záloha dat",
    "balances_stale": "Zůstatky účtů"
  },
  "sub": {
    "insurance_anniversary": "Poslední den výpovědi · výročí {{date}}",
    "insurance_anniversary_passed": "Výročí smlouvy · lhůta pro výpověď uplynula",
    "insurance_end": "Smlouva končí",
    "insurance_payment": "Platba {{amount}}",
    "loan_fixation_end": "Konec fixace · čas porovnat nabídky",
    "loan_payoff": "Poslední splátka",
    "bond_maturity": "Splatnost · vrátí se {{amount}}",
    "bond_coupon": "Kupón {{amount}}",
    "account_termination": "Účet končí",
    "savings_rate_end": "Zvýhodněná sazba končí",
    "watch_target_below": "Cena {{price}} klesla na cíl {{target}}",
    "watch_target_above": "Cena {{price}} dosáhla cíle {{target}}",
    "loan_fixation_expired": "Fixace skončila {{date}} · doplňte novou sazbu",
    "loan_balance_check": "Zůstatek neověřen od {{date}} · porovnejte s výpisem",
    "valuation_stale": "Bez nového odhadu od {{date}} · přeceňte",
    "backup_stale": "Poslední záloha {{date}}",
    "backup_never": "Zatím žádná záloha",
    "balances_stale_one": "{{count}} účet bez aktualizace přes 30 dní",
    "balances_stale_few": "{{count}} účty bez aktualizace přes 30 dní",
    "balances_stale_other": "{{count}} účtů bez aktualizace přes 30 dní"
  },
  "short": {
    "backup_stale": "zálohovat data",
    "balances_stale_one": "aktualizovat {{count}} zůstatek",
    "balances_stale_few": "aktualizovat {{count}} zůstatky",
    "balances_stale_other": "aktualizovat {{count}} zůstatků",
    "valuation_stale": "{{title}} přecenit",
    "loan_balance_check": "{{title}} ověřit zůstatek",
    "loan_fixation_expired": "{{title}} doplnit sazbu"
  },
  "groups": {
    "insuranceDates": {
      "label": "Výročí a konce pojistek",
      "hint": "Poslední den výpovědi asi 10 týdnů předem, konec smlouvy 2 měsíce předem.",
      "mute": "Nepřipomínat výročí a konce pojistek"
    },
    "insurancePayments": {
      "label": "Platby pojistného",
      "hint": "Čtvrtletní, pololetní a roční platby 14 dní předem.",
      "mute": "Nepřipomínat platby pojistného"
    },
    "loans": {
      "label": "Konce fixace a doplacení úvěrů",
      "hint": "Konec fixace 6 měsíců předem, poslední splátka měsíc předem.",
      "mute": "Nepřipomínat konce fixace a doplacení"
    },
    "bonds": {
      "label": "Splatnosti a kupóny dluhopisů",
      "hint": "Splatnost měsíc předem, kupón týden předem.",
      "mute": "Nepřipomínat splatnosti a kupóny"
    },
    "accounts": {
      "label": "Konce účtů a zvýhodněných sazeb",
      "hint": "Konec termínovaného účtu 4 týdny předem, konec zvýhodněné sazby 14 dní předem.",
      "mute": "Nepřipomínat konce účtů a sazeb"
    },
    "targets": {
      "label": "Cílové ceny akcií",
      "hint": "Když cena dosáhne cílové ceny ve zvoleném směru.",
      "mute": "Nepřipomínat cílové ceny"
    },
    "backup": {
      "label": "Zálohy dat",
      "hint": "Když je poslední záloha starší než 30 dní.",
      "mute": "Nepřipomínat zálohy"
    },
    "balances": {
      "label": "Aktualizace zůstatků",
      "hint": "Když zůstatek účtu nikdo neaktualizoval přes 30 dní.",
      "mute": "Nepřipomínat zůstatky účtů"
    },
    "valuations": {
      "label": "Ocenění nemovitostí",
      "hint": "Když je odhad hodnoty nemovitosti starší než rok.",
      "mute": "Nepřipomínat ocenění nemovitostí"
    },
    "loanChecks": {
      "label": "Kontroly úvěrů",
      "hint": "Ověření zůstatku jednou ročně a nová sazba po konci fixace.",
      "mute": "Nepřipomínat kontroly úvěrů"
    }
  },
  "settings": {
    "title": "Připomínky na přehledu",
    "description": "Co připomíná karta Co vás čeká a horní lišta. Vypnuté druhy se nezobrazí nikde."
  }
}
```

and `src/i18n/locales/en/milestones.json` with the same keys (English plurals `_one`/`_other` only):

```json
{
  "card": {
    "title": "What's coming up",
    "settingsLink": "Edit reminders",
    "showAll": "Show all ({{count}})",
    "showLess": "Show less",
    "nothingDue": "Nothing comes due in the next 90 days.",
    "upkeep": "Data upkeep",
    "upkeepShow": "Show {{count}}",
    "upkeepHide": "Collapse",
    "today": "Today",
    "target": "target",
    "rowMenu": "Reminder options"
  },
  "status": {
    "deadline": "{{title}} · {{when}}",
    "more": "+{{count}}",
    "title": "Deadlines in the next 14 days",
    "openOverview": "Open overview"
  },
  "actions": {
    "hide": "Hide",
    "remindWeek": "Remind me in a week",
    "remindMonth": "Remind me in a month",
    "undo": "Undo"
  },
  "toast": {
    "hidden": "Hidden",
    "snoozedWeek": "Snoozed for a week",
    "snoozedMonth": "Snoozed for a month",
    "muted": "Reminders off · {{group}}"
  },
  "relative": {
    "today": "today",
    "tomorrow": "tomorrow",
    "inDays_one": "in {{count}} day",
    "inDays_other": "in {{count}} days",
    "inMonths_one": "in {{count}} month",
    "inMonths_other": "in {{count}} months",
    "daysAgo_one": "{{count}} day ago",
    "daysAgo_other": "{{count}} days ago",
    "monthsAgo_one": "{{count}} month ago",
    "monthsAgo_other": "{{count}} months ago"
  },
  "titles": {
    "backup_stale": "Data backup",
    "balances_stale": "Account balances"
  },
  "sub": {
    "insurance_anniversary": "Last day to cancel · anniversary {{date}}",
    "insurance_anniversary_passed": "Contract anniversary · the notice period has passed",
    "insurance_end": "Contract ends",
    "insurance_payment": "Payment {{amount}}",
    "loan_fixation_end": "Fixed rate ends · time to compare offers",
    "loan_payoff": "Last payment",
    "bond_maturity": "Matures · {{amount}} comes back",
    "bond_coupon": "Coupon {{amount}}",
    "account_termination": "Account ends",
    "savings_rate_end": "Promotional rate ends",
    "watch_target_below": "Price {{price}} fell to the target {{target}}",
    "watch_target_above": "Price {{price}} reached the target {{target}}",
    "loan_fixation_expired": "Fixed rate ended {{date}} · enter the new rate",
    "loan_balance_check": "Balance not checked since {{date}} · compare with a statement",
    "valuation_stale": "No new valuation since {{date}} · revalue it",
    "backup_stale": "Last backup {{date}}",
    "backup_never": "No backup yet",
    "balances_stale_one": "{{count}} account not updated for over 30 days",
    "balances_stale_other": "{{count}} accounts not updated for over 30 days"
  },
  "short": {
    "backup_stale": "back up your data",
    "balances_stale_one": "update {{count}} balance",
    "balances_stale_other": "update {{count}} balances",
    "valuation_stale": "revalue {{title}}",
    "loan_balance_check": "check the balance of {{title}}",
    "loan_fixation_expired": "enter the new rate of {{title}}"
  },
  "groups": {
    "insuranceDates": {
      "label": "Insurance anniversaries and ends",
      "hint": "The last day to cancel about 10 weeks ahead, a contract end 2 months ahead.",
      "mute": "Don't remind me about insurance anniversaries and ends"
    },
    "insurancePayments": {
      "label": "Insurance payments",
      "hint": "Quarterly, half-yearly and yearly payments 14 days ahead.",
      "mute": "Don't remind me about insurance payments"
    },
    "loans": {
      "label": "Fixed-rate ends and loan payoffs",
      "hint": "A fixed-rate end 6 months ahead, the last payment a month ahead.",
      "mute": "Don't remind me about fixed rates and payoffs"
    },
    "bonds": {
      "label": "Bond maturities and coupons",
      "hint": "A maturity a month ahead, a coupon a week ahead.",
      "mute": "Don't remind me about maturities and coupons"
    },
    "accounts": {
      "label": "Account and promotional rate ends",
      "hint": "A term account end 4 weeks ahead, a promotional rate end 14 days ahead.",
      "mute": "Don't remind me about account and rate ends"
    },
    "targets": {
      "label": "Stock target prices",
      "hint": "When the price reaches the target in its direction.",
      "mute": "Don't remind me about target prices"
    },
    "backup": {
      "label": "Data backups",
      "hint": "When the last backup is older than 30 days.",
      "mute": "Don't remind me about backups"
    },
    "balances": {
      "label": "Balance updates",
      "hint": "When an account balance has not been updated for over 30 days.",
      "mute": "Don't remind me about account balances"
    },
    "valuations": {
      "label": "Property valuations",
      "hint": "When a property valuation is older than a year.",
      "mute": "Don't remind me about property valuations"
    },
    "loanChecks": {
      "label": "Loan checks",
      "hint": "A yearly balance check and the new rate after a fixed rate ends.",
      "mute": "Don't remind me about loan checks"
    }
  },
  "settings": {
    "title": "Overview reminders",
    "description": "What the What's coming up card and the top bar remind you of. Turned-off kinds show nowhere."
  }
}
```

- [ ] **Step 5: Keep the app compiling.** The old components still import removed utils and keys. Make the minimal edits so `npm run typecheck` passes (Task 3 replaces these files): `UpcomingCard` may render `splitMilestones(...).agenda` with the existing list; `MilestonesStatus` may use `upcomingDeadlines`; `MilestoneList` uses `useMilestoneActions().hide / remindLater` and drops `referenceDay`.
Run: `npm run typecheck && npm run lint && npm test` + en/cs key parity of `milestones.json` (node script, plural suffixes stripped). Expected: PASS.
- [ ] **Step 6: Commit** (`feat(milestones): agenda logic, muting hooks and copy`).

---

### Task 3: UI — agenda card, upkeep line, row menu, top bar, settings card

**Files:**
- Replace: `src/components/milestones/MilestoneList.tsx` (exports `AgendaRow`, `UpkeepRow`, `MilestoneMenu`, `upkeepShort`)
- Replace: `src/components/dashboard/UpcomingCard.tsx`, `src/components/shell/MilestonesStatus.tsx`
- Create: `src/components/settings/MilestoneSettingsCard.tsx`
- Modify: `src/components/settings/SettingsSectionPages.tsx` (General section: after `DashboardPreferencesCard`)

**Interfaces:**
- Consumes: Task 2 utils and hooks; `SectionHead` (`link={{ href, label }}`), `Card variant="flat"`, `CardContent`, `Button variant="ghost" size="icon-sm"`, `DropdownMenu*`, `Popover*`, `StatusText`, `SettingsCard`, `SettingsRow`, `Switch`, `useFormat()` (`day`, `money`, `percent`, `locale`), `formatNativePrice`, `utcDayFloor`.

- [ ] **Step 1: `MilestoneList.tsx`.** Replace the file with:

```tsx
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import {
  BellOff,
  Clock,
  DatabaseBackup,
  Ellipsis,
  EyeOff,
  FileCheck,
  Home,
  Landmark,
  Percent,
  type LucideIcon,
} from 'lucide-react';
import type { Milestone, MilestoneKind } from '@shared/schema';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { utcDayFloor } from '@/utils/chart-axis';
import { formatNativePrice } from '@/utils/stock-monitor';
import {
  agendaDay,
  daysBetween,
  groupOf,
  isUpkeep,
  isUrgent,
  milestoneHref,
  relativeLabel,
} from '@/utils/milestones';

const UPKEEP_ICONS: Partial<Record<MilestoneKind, LucideIcon>> = {
  backup_stale: DatabaseBackup,
  balances_stale: Landmark,
  valuation_stale: Home,
  loan_balance_check: FileCheck,
  loan_fixation_expired: Percent,
};

/** "Byt 3+kk Vinohrady přecenit": one item of the folded upkeep line. */
export function upkeepShort(m: Milestone, t: TFunction<'milestones'>): string {
  if (m.kind === 'balances_stale') return t('short.balances_stale', { count: m.count ?? 0 });
  return t(`short.${m.kind}`, { title: m.title });
}

/** The sub-line of a row: what happens; the date chip already says when. */
function useSubLine() {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const day = (d: number | null) =>
    d === null ? '' : fmt.day(d, { day: 'numeric', month: 'numeric' });
  const money = (m: Milestone) =>
    m.amount === null ? '' : fmt.money(Number(m.amount), m.currency ?? 'CZK');
  const price = (m: Milestone, value: string | null) =>
    value === null ? '' : formatNativePrice(Number(value), m.currency, fmt.locale);

  return (m: Milestone): string => {
    switch (m.kind) {
      case 'insurance_anniversary':
        return m.tone === 'info'
          ? t('sub.insurance_anniversary_passed')
          : t('sub.insurance_anniversary', { date: day(m.dueDay) });
      case 'insurance_payment':
      case 'bond_coupon':
      case 'bond_maturity':
        return t(`sub.${m.kind}`, { amount: money(m) });
      case 'watch_target':
        return t(m.direction === 'below' ? 'sub.watch_target_below' : 'sub.watch_target_above', {
          price: price(m, m.referenceAmount),
          target: price(m, m.amount),
        });
      case 'backup_stale':
        return m.sinceDay === null
          ? t('sub.backup_never')
          : t('sub.backup_stale', { date: day(m.sinceDay) });
      case 'balances_stale':
        return t('sub.balances_stale', { count: m.count ?? 0 });
      case 'valuation_stale':
      case 'loan_balance_check':
        return t(`sub.${m.kind}`, { date: day(m.sinceDay) });
      case 'loan_fixation_expired':
        return t('sub.loan_fixation_expired', { date: day(m.dueDay) });
      default:
        return t(`sub.${m.kind}`);
    }
  };
}

/** "···": hide, remind later, mute the group. */
export function MilestoneMenu({ m }: { m: Milestone }) {
  const { t } = useTranslation('milestones');
  const { hide, remindLater, muteGroup } = useMilestoneActions();
  const group = groupOf(m.kind);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('card.rowMenu')}
          className="shrink-0 text-ink-4 hover:text-ink data-[state=open]:text-ink"
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => hide(m)}>
          <EyeOff />
          {t('actions.hide')}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => remindLater(m)}>
          <Clock />
          {t(isUpkeep(m) ? 'actions.remindMonth' : 'actions.remindWeek')}
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => muteGroup(group)}>
          <BellOff />
          {t(`groups.${group}.mute`)}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Agenda row (spec 2026-10-05-milestones-agenda §2): a date chip (day and month, the
 * distance under it; dark material for a deadline at most 7 days away), title and one
 * sub-line, the "···" menu next to the link, never inside it.
 */
export function AgendaRow({ m, onNavigate }: { m: Milestone; onNavigate?: () => void }) {
  const { t } = useTranslation('milestones');
  const fmt = useFormat();
  const subLine = useSubLine();
  const today = utcDayFloor(Date.now() / 1000);
  const day = agendaDay(m, today);
  const rel = relativeLabel(daysBetween(today, day));
  const urgent = isUrgent(m, today);
  const target = m.kind === 'watch_target';
  const sub = subLine(m);

  return (
    <li className="flex items-center gap-1 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        onClick={onNavigate}
        className="grid min-w-0 flex-1 grid-cols-[50px_1fr] items-center gap-3 py-2.5 text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <span
          className={cn(
            'grid h-[42px] place-content-center rounded-r2 text-center leading-tight',
            urgent ? 'bg-dark-grad text-ink-inverse shadow-dark' : 'bg-well text-ink'
          )}
        >
          <b className="block text-table font-650 num">
            {target ? t('card.today') : fmt.day(day, { day: 'numeric', month: 'numeric' })}
          </b>
          <small
            className={cn(
              'block text-micro font-500',
              urgent ? 'text-ink-inverse-2' : 'text-ink-4'
            )}
          >
            {target ? t('card.target') : t(`relative.${rel.key}`, { count: rel.count })}
          </small>
        </span>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{m.title}</b>
          <small title={sub} className="mt-[3px] block truncate text-micro font-500 text-ink-4">
            {sub}
          </small>
        </span>
      </Link>
      <MilestoneMenu m={m} />
    </li>
  );
}

/** Upkeep row: an icon in a `well` square instead of a date chip. */
export function UpkeepRow({ m }: { m: Milestone }) {
  const { t } = useTranslation('milestones');
  const subLine = useSubLine();
  const Icon = UPKEEP_ICONS[m.kind] ?? Landmark;
  const title = m.title || t(`titles.${m.kind}`);
  const sub = subLine(m);
  return (
    <li className="flex items-center gap-1 border-b border-line-soft last:border-0">
      <Link
        href={milestoneHref(m)}
        className="grid min-w-0 flex-1 grid-cols-[30px_1fr] items-center gap-[11px] py-2.5 text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <i className="grid size-[29px] place-items-center rounded-r2 bg-well text-ink-2">
          <Icon className="size-[15px]" strokeWidth={1.75} aria-hidden />
        </i>
        <span className="min-w-0">
          <b className="block truncate text-table font-650">{title}</b>
          <small title={sub} className="mt-[3px] block truncate text-micro font-500 text-ink-4">
            {sub}
          </small>
        </span>
      </Link>
      <MilestoneMenu m={m} />
    </li>
  );
}
```

Check how existing dropdown menus render icons (`src/components/stocks/InvestmentsTable.tsx`) and match it (icon size classes). If `TFunction<'milestones'>` does not typecheck with the repo's i18next setup, type the parameter as the return type of `useTranslation('milestones').t`.

- [ ] **Step 2: `UpcomingCard.tsx`.** Replace with:

```tsx
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Wrench } from 'lucide-react';
import { SectionHead } from '@/components/shell/PageHead';
import { Card, CardContent } from '@/components/ui/card';
import { AgendaRow, UpkeepRow, upkeepShort } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { columns, splitMilestones } from '@/utils/milestones';

/** Agenda rows shown before "Zobrazit vše" (three per column). */
const COLLAPSED_ROWS = 6;

/**
 * "Co vás čeká" (spec 2026-10-05-milestones-agenda): a two-column agenda with date chips,
 * filled top to bottom, and data upkeep folded into one line under it. Hidden when empty.
 */
export function UpcomingCard() {
  const { t } = useTranslation('milestones');
  const { data: milestones = [] } = useMilestones();
  const [expanded, setExpanded] = useState(false);
  const [upkeepOpen, setUpkeepOpen] = useState(false);

  if (milestones.length === 0) return null;

  const today = utcDayFloor(Date.now() / 1000);
  const { agenda, upkeep } = splitMilestones(milestones, today);
  const [left, right] = columns(expanded ? agenda : agenda.slice(0, COLLAPSED_ROWS));

  return (
    <section className="mt-9">
      <SectionHead
        title={t('card.title')}
        link={{ href: '/settings', label: t('card.settingsLink') }}
      />
      <Card variant="flat">
        <CardContent className="pb-3 pt-2">
          {agenda.length === 0 ? (
            <p className="m-0 py-3 text-table text-ink-3">{t('card.nothingDue')}</p>
          ) : (
            <div className="grid grid-cols-2 gap-x-7">
              {[left, right].map((column, i) => (
                <ul key={i} className="m-0 list-none p-0">
                  {column.map((m) => (
                    <AgendaRow key={m.key} m={m} />
                  ))}
                </ul>
              ))}
            </div>
          )}
          {agenda.length > COLLAPSED_ROWS && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mt-2 text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
            >
              {expanded ? t('card.showLess') : t('card.showAll', { count: agenda.length })}
            </button>
          )}
          {upkeep.length > 0 && (
            <div className="mt-2 border-t border-line-soft pt-2.5">
              <button
                type="button"
                aria-expanded={upkeepOpen}
                onClick={() => setUpkeepOpen((v) => !v)}
                className="flex w-full min-w-0 items-center gap-2.5 text-left text-caption font-500 text-ink-3 hover:text-ink-2 focus-visible:outline-none focus-visible:shadow-focus"
              >
                <Wrench className="size-[14px] shrink-0 text-ink-4" strokeWidth={1.75} aria-hidden />
                <span className="min-w-0 flex-1 truncate">
                  <b className="font-650 text-ink-2">{t('card.upkeep')}</b>
                  {' · '}
                  {upkeep.map((m) => upkeepShort(m, t)).join(' · ')}
                </span>
                <span className="shrink-0 font-650 text-ink-2">
                  {upkeepOpen ? t('card.upkeepHide') : t('card.upkeepShow', { count: upkeep.length })}
                </span>
              </button>
              {upkeepOpen && (
                <ul className="m-0 mt-1 grid list-none grid-cols-2 gap-x-7 p-0">
                  {upkeep.map((m) => (
                    <UpkeepRow key={m.key} m={m} />
                  ))}
                </ul>
              )}
            </div>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
```

- [ ] **Step 3: `MilestonesStatus.tsx`.** Replace with:

```tsx
import { useState } from 'react';
import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { StatusText } from '@/components/shell/DataStatus';
import { AgendaRow } from '@/components/milestones/MilestoneList';
import { useMilestones } from '@/hooks/use-milestones';
import { utcDayFloor } from '@/utils/chart-axis';
import { agendaDay, daysBetween, relativeLabel, upcomingDeadlines } from '@/utils/milestones';

/**
 * Top-bar status (spec 2026-10-05-milestones-agenda §2.5): only a deadline at most 14 days
 * away, the nearest one by name ("Spořicí účet · za 10 dní", +N for more). Information,
 * upkeep and targets never show here; most weeks it is absent.
 */
export function MilestonesStatus() {
  const { t } = useTranslation('milestones');
  const [open, setOpen] = useState(false);
  const { data: milestones = [] } = useMilestones();
  const today = utcDayFloor(Date.now() / 1000);
  const deadlines = upcomingDeadlines(milestones, today);

  // A deadline that goes away (hidden, muted) must not leave the popover open.
  if (deadlines.length === 0 && open) setOpen(false);
  if (deadlines.length === 0) return null;

  const first = deadlines[0];
  const rel = relativeLabel(daysBetween(today, agendaDay(first, today)));

  return (
    <Popover open={open && deadlines.length > 0} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="min-w-0 max-w-[320px] rounded-r1 focus-visible:outline-none focus-visible:shadow-focus"
        >
          <StatusText className="min-w-0 text-ink-2">
            <span className="truncate">
              {t('status.deadline', {
                title: first.title,
                when: t(`relative.${rel.key}`, { count: rel.count }),
              })}
            </span>
            {deadlines.length > 1 && (
              <span className="shrink-0 text-ink-4">
                {t('status.more', { count: deadlines.length - 1 })}
              </span>
            )}
          </StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[400px] p-0">
        <div className="px-4 pt-3 text-eyebrow uppercase text-ink-4">{t('status.title')}</div>
        <ul className="m-0 max-h-[420px] list-none overflow-y-auto px-4 py-0">
          {deadlines.map((m) => (
            <AgendaRow key={m.key} m={m} onNavigate={() => setOpen(false)} />
          ))}
        </ul>
        <div className="border-t border-line-soft px-4 py-2.5">
          <Link
            href="/"
            onClick={() => setOpen(false)}
            className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
          >
            {t('status.openOverview')} →
          </Link>
        </div>
      </PopoverContent>
    </Popover>
  );
}
```

- [ ] **Step 4: Settings card.** Create `src/components/settings/MilestoneSettingsCard.tsx`:

```tsx
import { useTranslation } from 'react-i18next';
import { Switch } from '@/components/ui/switch';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { useMilestoneMutedKinds } from '@/hooks/use-milestones';
import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { MILESTONE_GROUP_IDS, kindsOf } from '@/utils/milestones';

/** Settings → Obecné → Připomínky na přehledu: one switch per reminder group. */
export function MilestoneSettingsCard() {
  const { t } = useTranslation('milestones');
  const { data: muted = [] } = useMilestoneMutedKinds();
  const { setGroupMuted } = useMilestoneActions();

  return (
    <SettingsCard title={t('settings.title')} description={t('settings.description')}>
      {MILESTONE_GROUP_IDS.map((group) => {
        const on = !kindsOf(group).every((kind) => muted.includes(kind));
        return (
          <SettingsRow
            key={group}
            htmlFor={`milestone-group-${group}`}
            label={t(`groups.${group}.label`)}
            hint={t(`groups.${group}.hint`)}
          >
            <Switch
              id={`milestone-group-${group}`}
              checked={on}
              onCheckedChange={(checked) => setGroupMuted(group, !checked)}
            />
          </SettingsRow>
        );
      })}
    </SettingsCard>
  );
}
```

Render `<MilestoneSettingsCard />` in `GeneralSection` (`SettingsSectionPages.tsx`) right after `<DashboardPreferencesCard />`.
- [ ] **Step 5: Verify** `npm run typecheck && npm run lint && npm test`. Expected: PASS. No references to removed utils remain (`grep -rn "groupMilestones\|attentionCount\|isAttention\|referenceDay\|canDismiss" src shared` is empty).
- [ ] **Step 6: Commit** (`feat(milestones): agenda card, upkeep line, row menu and settings card`).

---

### Task 4: Docs and gates

**Files:** `docs/design-system/README.md`, `CHANGELOG.md`, `docs/architecture/database.md`

- [ ] **Step 1:** Design-system README:
  - §5 indicator bullet: "Milestones indicator: left of the update status, a neutral `.status` with `ink-2` text, shown only when a deadline (notice deadline, contract or fixed-rate end, maturity, account or promotional-rate end) is at most 14 days away; it names the nearest one ("Spořicí účet · za 10 dní", "+N" for more) and opens a popover with those deadlines. Information, data upkeep and target prices never appear there."
  - §6 "Milestone row" entry: "Agenda row: a 50 × 42 px date chip on `well` (day and month 12/650, distance under it 10/500 `ink-4`; dark material only for a deadline at most 7 days away), title 12/650, one sub-line 10/500 `ink-4`, a ghost `icon-sm` '···' (always visible, `ink-4`) next to the link, never inside it, opening Skrýt / Připomenout za týden (upkeep: za měsíc) / Nepřipomínat …. Upkeep rows use an icon in a `well` square instead of the chip."
  - §7 Overview row: "'Co vás čeká' (a two-column agenda with date chips filled top to bottom, data upkeep folded into one line under it, hidden when empty)".
- [ ] **Step 2:** CHANGELOG `[Unreleased]`: reword the milestones entries for the agenda (date chips, upkeep line, Skrýt / Připomenout / Nepřipomínat, settings card "Připomínky na přehledu", top bar only for deadlines at most 14 days away).
- [ ] **Step 3:** `database.md`: in the `app_config` row mention the `milestones.mutedKinds` key (JSON array of muted milestone kinds); in the `milestone_states` row say upkeep keys carry the date of their data and upkeep snoozes last 30 days.
- [ ] **Step 4: Full gates.**

```bash
npm run lint && npm run typecheck && npm test
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri && cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts
```

- [ ] **Step 5: Commit** (`docs: milestones agenda`).
