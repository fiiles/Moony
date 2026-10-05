# Milestones ("Co vás čeká") — design

Date: 2026-10-05 · Status: approved by the owner on 2026-10-05 (scope, delivery channels, target
direction, dismissal and new fields chosen in the brainstorming session; implementation delegated).

## 1. Context

Every domain page already computes its own "next thing": the insurance page shows the nearest
anniversary, the loans page the end of the fixation, the bonds page the next maturity. Nothing brings
them together, so the owner found a contract anniversary three months ahead only by browsing. Moony is
a local desktop app without a tray process, so the reliable channels are inside the app: a dashboard
card and a top-bar indicator visible on every page.

The watchlist has a related bug. A target counts as reached when `current >= target`, so a target
below the price (waiting for a dip to buy) shows "Cíl dosažen" right away (CAT: target 600 USD, price
845 USD). Milestones would repeat that false alarm, so the target gets a direction first.

### Owner decisions (2026-10-05)

- Groups in v1: contract dates (A), money in and out (B), data upkeep (D) and, from the market group,
  only "a watched stock crossed its target". Taxes and positive net-worth milestones are out.
- Channels: a dashboard card and a top-bar indicator. No OS notifications, no calendar export in v1.
- Target direction: preselected from the price when the target is set (below the price = waiting for
  a dip, above = waiting for a rise), switchable in the dialog, stored.
- Items leave the indicator through "Vyřízeno" (this occurrence only) and "Odložit" (one week),
  stored in the database. Items backed by data disappear by themselves when the data changes.
- New field: "sazba platí do" (promotional rate end) on bank accounts. Dates for real-estate
  recurring costs are deferred.
- Milestones are computed in a Rust service (one command), not in the frontend.

## 2. Milestone model

One `Milestone` is one occurrence of something the user should know about.

| Field | Meaning |
|---|---|
| `key` | Stable occurrence key, `<kind>:<sourceId>:<dueDay>` for dated items, `<kind>:<sourceId>` or `<kind>` for data-upkeep items, `watch_target:<ticker>:<target>:<direction>` for targets. Dismissal state is stored by this key, so next year's anniversary (a new `dueDay`) appears again. |
| `kind` | One of the kinds in §3. |
| `stage` | `now` (act now) or `soon` (coming up, not yet in the reminder window). |
| `tone` | `action` (counts in the indicator) or `info` (card only). |
| `sourceId` | Id of the policy, loan, bond, account or property; the ticker for targets; null for backup and balances. |
| `title` | Entity name (policy name, loan name, ticker); empty for backup and balances (the UI names them). |
| `dueDay` | UTC day of the event; null for upkeep items and targets. |
| `actionDay` | Last day to act when it differs from `dueDay` (insurance cancellation deadline); else null. |
| `sinceDay` | For upkeep items: the day of the last backup, balance update, valuation or balance check; null when there never was one. |
| `amount`, `currency` | Payment, coupon or principal returned; the target price for targets. Money as TEXT. |
| `referenceAmount` | The current price for targets; else null. |
| `direction` | `below` or `above` for targets; else null. |
| `count` | Number of accounts for `balances_stale`; else null. |
| `canDismiss` | False for upkeep items and for `loan_fixation_expired` (only "Odložit"); true otherwise. |

All days are UTC midnights (ADR 0008). `today` is `today_utc_day()`.

**Stage.** Each kind defines a reminder start (`remindFrom`). An item is `now` when
`today >= remindFrom`. It is `soon` when it is not `now` and `dueDay <= today + 90 days`. Items with
neither are not returned. Upkeep items and crossed targets are always `now`.

**Filtering.** Items whose key is stored as `done`, or as `snoozed` with `until > today`, are left out.

**Order.** `now` before `soon`. Within `now`: dated items by `actionDay ?? dueDay` ascending, then
targets, then upkeep items. Within `soon`: by `dueDay` ascending. Ties by `title`.

## 3. Kinds and rules

| Kind | Source | `dueDay` | `remindFrom` | Tone | `canDismiss` |
|---|---|---|---|---|---|
| `insurance_anniversary` | Active policy (`status = 'active'`, end date not before today), next yearly anniversary of `startDate` after today, none when the contract ends on or before it | anniversary | `actionDay − 28 d`, where `actionDay = dueDay − 42 d` (a policy can be cancelled to the anniversary with at least six weeks' notice) | `action`; `info` once `today > actionDay` (the item stays until the anniversary, saying the notice period has passed) | yes |
| `insurance_end` | Active policy with `endDate >= today` | end date | `dueDay − 60 d` | `action` | yes |
| `insurance_payment` | Active policy paid quarterly, semi-annually or annually (never monthly, never one-time), next payment due after today | payment day | `dueDay − 14 d` | `action` | yes |
| `loan_fixation_end` | Loan not matured, `interestRateValidityDate > today` | validity date | `dueDay − 180 d` | `action` | yes |
| `loan_fixation_expired` | Loan not matured, `interestRateValidityDate <= today` (a rate change clears the date, which resolves the item) | validity date | always `now` | `action` | no |
| `loan_payoff` | Loan not matured with a payoff day from its schedule | payoff day | `dueDay − 30 d` | `info` | yes |
| `bond_maturity` | Bond with `maturityDate >= today`; `amount` = face value (coupon value × quantity) | maturity day | `dueDay − 30 d` | `action` | yes |
| `bond_coupon` | Bond with rate > 0 and a maturity date; the next yearly anniversary of the maturity strictly after today and strictly before the maturity (the last coupon is part of `bond_maturity`); `amount` = yearly coupon | coupon day | `dueDay − 7 d` | `info` | yes |
| `account_termination` | Bank account with `terminationDate >= today` | termination day | `dueDay − 28 d` | `action` | yes |
| `savings_rate_end` | Bank account with `interestRateValidUntil >= today`, not terminated before that day | rate end day | `dueDay − 14 d` | `action` | yes |
| `backup_stale` | Last backup older than 30 days; never backed up and the profile is at least 14 days old | — | always `now` | `action` | no |
| `balances_stale` | One item for all accounts counted in the balance (`excludeFromBalance = false`), not terminated, whose `updatedAt` is older than 30 days; `count` = their number, `sinceDay` = the oldest `updatedAt` | — | always `now` | `action` | no |
| `valuation_stale` | Property whose latest valuation (else purchase date, else creation) is older than 365 days | — | always `now` | `action` | no |
| `loan_balance_check` | Loan not matured whose balance anchor date (else start date) is older than 365 days; every extra payment, rate change and balance check moves the anchor | — | always `now` | `action` | no |
| `watch_target` | Watched stock with a target and a price that crossed it: `price <= target` for `below`, `price >= target` for `above` | — | always `now` | `action` | yes |

A loan is matured when its outstanding balance is 0 or its end date is before today (as on the loans
page). Monthly insurance payments are left out on purpose: they are routine.

## 4. Watchlist target direction

- `watched_stocks.target_direction TEXT` (`'below'` / `'above'`, NULL without a target).
- Migration backfill for existing targets: `below` when the stored price is known and the target is
  below it, else `above` (the current behavior). CAT becomes `below`.
- `set_watched_target_price(ticker, targetPrice, direction?)`: an explicit direction is stored; without
  one (MCP clients) the service infers it from the current price the same way. Clearing the target
  clears the direction.
- `WatchedStock`, `WatchedStockRow` and `StockMonitorDetail` gain `targetDirection`.
- "Reached" becomes direction-aware everywhere it is shown (table badge, "U cílové ceny" stat, detail
  hero) through one tested helper `targetReached(current, target, direction)`.
- The target dialog shows a two-option choice under the price: "Čekám na pokles – koupit /
  přikoupit" and "Čekám na růst – prodat". While the user has not touched it, it follows the typed
  price (below the current price → pokles). A line under it says what will happen: "Upozorním, až cena
  klesne na 600 USD (−29 % od dnešní ceny)."
- The MCP `watchlist_set_target_price` tool accepts an optional `direction`; `watchlist_list` returns it.

## 5. Promotional rate end on bank accounts

- `bank_accounts.interest_rate_valid_until INTEGER` (UTC day, NULL when unknown or not promotional).
- `BankAccount` and `InsertBankAccount` gain `interestRateValidUntil` (optional, serde default).
- The account form gets an optional date field "Sazba platí do" with the hint "Konec zvýhodněné
  sazby; Moony upozorní 14 dní předem." next to the interest rate.

## 6. Backend

- Migration `005_milestones`: the two columns above, the backfill, and
  `milestone_states(key TEXT PRIMARY KEY, state TEXT NOT NULL CHECK (state IN ('done','snoozed')),
  until_day INTEGER, updated_at INTEGER NOT NULL DEFAULT (unixepoch()))`.
- Service `services/milestones.rs`: one builder function per domain (insurance, loans, bonds, bank
  accounts, backup, real estate, watchlist) returning `Vec<Milestone>`, and
  `list_milestones(conn, today)` that combines them, applies stage, filtering and order.
  `set_milestone_state(conn, key, state, today)` writes `done` or `snoozed` (until `today + 7 d`);
  `clear_milestone_state(conn, key)` removes the row (undo).
- Insurance date math moves to Rust using the existing `loan_amortization::due_day` (same rules as
  `src/utils/insurance.ts`: anniversaries and payments fall on the start day of month, clamped to the
  month's end). Mirrored test cases keep the two in step (31 January start, 29 February start).
- Commands `commands/milestones.rs` (thin): `get_milestones`, `set_milestone_state`,
  `clear_milestone_state`. Validation: state is `done` or `snoozed`; key non-empty, at most 256
  characters (`validation.milestoneKeyInvalid`, `validation.milestoneStateInvalid`).

## 7. Frontend

- `milestonesApi` in `tauri-api.ts`; `Milestone` in `shared/schema.ts`.
- `use-milestones.ts`: query key `["milestones"]`, `refetchInterval` 5 minutes (prices and the day
  change). The query client's mutation cache invalidates `["milestones"]` after every successful
  mutation, so any edit anywhere refreshes the card and the indicator without touching every
  mutation hook. `use-milestone-mutations.ts`: done, snooze, undo.
- `src/utils/milestones.ts` (pure, tested): grouping, the indicator count, the link of a milestone,
  days until a day. The "Teď jednat" group and the indicator count are the items with
  `stage = now` and `tone = action`; everything else (`soon` items and `info` items such as a coupon
  next week or an anniversary whose notice period has passed) goes to "Brzy", keeping the order of §2.
- **Dashboard card "Co vás čeká"**: a full-width section between the stats row and the
  allocation/moves grid, hidden when there are no milestones. Two eyebrow groups, "Teď jednat" and
  "Brzy". Rows follow the recent-moves row: icon in a `well` square, title and one sub-line, a
  relative date right ("za 38 dní", "dnes", "před 41 dny"). Hover reveals ghost icon buttons
  "Odložit o týden" and "Vyřízeno" (only "Odložit" when `canDismiss` is false). The whole row links to
  the source. At most 6 rows are shown; "Zobrazit vše (N)" expands.
- **Top-bar indicator**: left of the update status, shown only when the count is above 0. `.status`
  styling with a neutral dot and ink-2 text: "2 k vyřízení". Clicking opens a popover with the `now`
  items (same rows) and a link "Otevřít přehled".
- Done and snooze show a toast ("Označeno jako vyřízené", "Odloženo o týden") with "Vrátit", which
  clears the state.
- Links: insurance → `/insurance/:id`, loans → `/loans/:id`, bonds → `/bonds`, accounts →
  `/bank-accounts/:id`, balances → `/bank-accounts`, property → `/real-estate/:id`, backup →
  `/settings/data`, target → `/stock-monitor/:ticker`.
- Copy lives in a new `milestones` namespace (cs + en). Amounts in their own currency through
  `useFormat().money`; days through `fmt.day`.

## 8. Copy (cs)

| Kind | Title | Sub-line |
|---|---|---|
| `insurance_anniversary` | policy name | "Výročí 24. 12. · výpověď nejpozději 12. 11."; after the deadline "Výročí 24. 12. · lhůta pro výpověď uplynula" |
| `insurance_end` | policy name | "Smlouva končí 1. 3." |
| `insurance_payment` | policy name | "Platba 4 200 Kč · 15. 1." |
| `loan_fixation_end` | loan name | "Konec fixace 1. 3. 2027 · čas porovnat nabídky" |
| `loan_fixation_expired` | loan name | "Fixace skončila 1. 9. · doplňte novou sazbu" |
| `loan_payoff` | loan name | "Poslední splátka 15. 11." |
| `bond_maturity` | bond name | "Splatnost 1. 12. · vrátí se 100 000 Kč" |
| `bond_coupon` | bond name | "Kupón 4 500 Kč · 1. 12." |
| `account_termination` | account name | "Účet končí 30. 11." |
| `savings_rate_end` | account name | "Zvýhodněná sazba končí 31. 10." |
| `backup_stale` | "Záloha dat" | "Poslední záloha 24. 8." or "Zatím žádná záloha" |
| `balances_stale` | "Zůstatky účtů" | "3 účty bez aktualizace přes 30 dní" |
| `valuation_stale` | property name | "Odhad hodnoty z 12. 3. 2025 · přeceňte" |
| `loan_balance_check` | loan name | "Zůstatek ověřen 1. 1. 2025 · porovnejte s výpisem" |
| `watch_target` | ticker | "Cena 598 USD klesla na cíl 600 USD" / "Cena 905 USD dosáhla cíle 900 USD" |

The right side shows the relative distance to `actionDay ?? dueDay` for dated items and to
`sinceDay` for upkeep items ("před 41 dny"); targets show the price change against the target in
percent.

## 9. Testing

- Rust (`services/milestones.rs`, in-memory schema): every kind's inclusion, stage boundary
  (`remindFrom` − 1 day / on the day), exclusion rules (monthly insurance, matured loans, terminated
  accounts, ended policies), dismissal and snooze filtering and expiry, ordering, the insurance date
  math edge cases, the bond coupon anniversary (29 February maturity), target direction inference.
- Rust (`services/stock_monitor.rs`): explicit and inferred direction, clearing.
- Migration: snapshot regenerated; backfill test (below / above / unknown price).
- TypeScript: `targetReached`, `src/utils/milestones.ts`.
- Manual: the demo profile shows the card and the indicator; done, snooze and undo work; the CAT case
  shows "čeká na pokles".

## 10. Out of scope

OS notifications, calendar export, tax dates, positive net-worth milestones, real-estate recurring cost
dates, configurable lead times, an MCP `milestones_list` tool (the service makes it easy later).
