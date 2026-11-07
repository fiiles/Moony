# Phone Cashflow: the desktop's expense rows, limit states and one query

Date: 2026-11-07 · Audit items B2, B5, A16, plus B9, B11 and C10 (`docs/audits/2026-11-06-mobile-ux-audit.md`)
· Reference: the desktop Cashflow (`src/pages/Cashflow.tsx`, `ExpensesByCategoryCard`, spec
`2026-10-28-cashflow-with-budgets-implementation-design.md`) · Owner: "desktopová verze je referenční
bod"; the decisions are delegated, mine are marked **(call)**.

## Why

The phone Cashflow still runs on the October model; the desktop moved on with the cashflow + budgets
merge (2026-10-28):

- **B2.** Rows come only from the cashflow groups, so a category with a limit and no spending (or
  whose payments all lack an FX rate) has no row, and a period with limits but no payments is
  "empty". The phone knows ok / near / over only (no "Limit vyčerpán", no "Čerpání nelze vyhodnotit",
  no "Zbývá", no "Jen část období"); spent = limit reads "blíží se limitu"; the comparison is after
  rounding to whole units. A failed budgeting report is swallowed. Actuals and budgets are two
  queries, so a period change briefly joins the new actuals with the old limits.
- **B5.** The subtitle counts accounts with movement ("z 1 účtu") while the menu says "Účty (3)".
- **A16.** No "Nepočítá se do příjmů ani výdajů", no "Celkový pohyb peněz na účtech …", no
  "Nezařazené kredity se počítají jako příjem"; transfer amounts have no sign.
- **B9.** The tab first loads the empty current month and then jumps (blink); with data older than
  the twelve-month window it stays on an empty month.
- **B11.** Every report amount is rounded to whole units (12,40 € → 12 €), rows do not add up.
- **C10.** Limits are set only by a long press (the `···` is screen-reader only); the note
  "Rozpočty: 5 v mezích · 0 nad limitem · 0 blíží se limitu" lists zeros and leads to a page that does
  not show usage.

## Goals

1. Expense rows and limit states from the desktop's pure code (`src/utils/cashflow-budget-rows.ts`:
   `expenseRows`, `limitState`, `filterCounts`, `filterRows`, `sortRows`), with the desktop's words
   (`budgeting.table.state.*`, `readiness.usageUnavailable`, `actuals.expenses.*`).
2. One query for both reports, keyed exactly like the desktop's
   (`['cashflow-report', 'overview', start, end, timeframe, accountIds, currency, fxRevision]`), so
   loading, placeholder, error and retry are one state and the desktop and phone share the cache.
   A failed budget report is the report's error with "Zkusit znovu"; a failed limit history is its
   own error line (the desktop's).
3. The opening period is the month of the newest transaction (`budgeting-latest-transaction`), decided
   before the report is fetched: no blink, no empty month when the data are old.
4. Report amounts at ISO minor units (`useReportMoney` → `formatReportMoney`), signed where the desktop
   signs (savings, net refunds, transfers, the cash movement).
5. A visible limit action on every expense-category row ("Nastavit limit", or the limit amount that
   edits it), the desktop's filter (Vše / Nad limitem / Bez limitu, with counts) and sort, and the
   budget summary note removed.
6. The subtitle states the filter scope ("ze všech účtů" or "z N účtů", N = the chosen accounts).
7. The desktop's explanatory lines under income, transfers and expenses (total / filtered total,
   "Limity vyhodnocené za …", pace).

## Non-goals

- The Spravovat rozpočty page (deleting a budget is still a swipe or the long press; C10's second
  bullet). It is a separate screen; the row's `···` there is the same pattern as everywhere.
- The CSV export of the phone (F: the desktop dropped it). It stays; deciding it is the owner's.
- Terminology outside this screen (D), the chart axis (B12), navigation memory (C3).
- Any backend change.

## Design

### Data (`use-cashflow.ts`)

`useCashflowReport(state, { enabled })` runs one `useQuery<{ actuals, budgeting }>` with
`Promise.all` of `cashflowApi.getActuals` and `budgetingApi.getReport`, `staleTime: 0`,
`refetchOnMount: 'always'`, `keepPreviousData` — the desktop's options. It returns `actuals`,
`budgeting`, `expenses` (`expenseRows(actuals.expenseCategories, budgeting.expenseCategories)`),
`isPlaceholder`, `isError`, `refetch`. The two old keys (`'actuals'` under `cashflow-report`, and
`budgeting-report` for the period) are no longer read by the tab; `budgeting-report` stays for the
six-month average of the limit sheet.

### Opening period (`cashflow-period.ts`, pure)

`openingState(state, chosen, latest, now)`: a search that names a period (`periodChosen`) is used
as it is. Otherwise, `latest` is `undefined` while the newest transaction date is not known (the
tab shows the skeleton and the report query waits), `null` when there is none or the lookup failed
(the current month), else that day's month, never a future one. The page writes the result into the
URL once (`o=`), so later refetches never move it. The phone observer has `staleTime: Infinity`:
a cached date is used at once, an invalidated one (after a money write) is waited for. Until the
month is known the body is the skeleton and the period label is hidden (its place kept), with the
stepper off, so no other month flashes first. `latestMonthOffset` goes.

### Rows

- **Expenses** (`ExpenseCategoryRow`, rewritten on `EntityRow`): chip, name, spent in the report
  currency at minor units ("—" with "Kurz není k dispozici" when `amountUnavailable`; never red, as
  on the desktop), sub-line "47 % výdajů · 3 transakce".
  - A row **with a limit** has the usage bar (red when over) with the pace mark in a current period,
    and under the row (`below`, not part of the tap target) the limit line: the state word ("↑ Nad
    limitem", "Limit vyčerpán", "Blíží se limitu", "V pořádku") and the percentage, then "Zbývá 240
    Kč" or "−206 Kč nad limitem" in the budget's currency, then "Jen část období" when the limit
    covers only some months (or "Čerpání nelze vyhodnotit"); on the right the limit amount as a 44 pt
    text button with a dotted underline ("Upravit limit: X"), disabled while the stored limits load
    or the next period loads.
  - A row **without a limit** keeps the share bar (`s3`, like income). "Nastavit limit" is visible
    through the section head's "Spravovat limity" (Spravovat rozpočty, whose "Výdaje bez limitu"
    section sets new limits) and, while the filter is "Bez limitu", as a compact line "Bez limitu ·
    Nastavit limit" under each row.
  - The long press keeps its menu (Zobrazit transakce, Upravit / Nastavit limit). A limit-only row
    (no transactions) is not a link.
- **Filter and sort** above the rows: the phone `Segment` with the desktop's three filters and counts
  (hidden while no row has a limit: every row would be "Bez limitu"), and a `SortPill` (Částka /
  Čerpání / Název) beside it; the row wraps, so the pill drops under the filter when both do not fit
  (360 pt, German or Polish, large text).
- **Footer** under the expense rows: "Celkem X" and "Limity vyhodnocené za říjen 2026 · značka je
  dnešek, uplynulo 23 % období"; with a filter on, "Celkem za filtr X" and "N kategorií ve výběru".
- **Income** rows unchanged in shape (share bar); footer "Nezařazené kredity se počítají jako příjem".
- **Transfers** always shown (desktop), signed amounts; footer "Nepočítá se do příjmů ani výdajů" and
  "Celkový pohyb peněz na účtech +X".
- The expense section head's action is "Spravovat limity" → Spravovat rozpočty (the total moves to
  the footer, as on the desktop).

### Limit action (`budget-rows.ts`, pure)

`rowLimitAction(row, canHaveLimit, goals, periodStart, limitsReady)` — the desktop's `openLimit`:
`null` for a row without a limit that cannot get one; otherwise the category's stored goal of the
same timeframe — even one whose limit ended or starts later — (or the period's copy) is edited from
the shown month, any other expense category gets a new limit from that month. Not ready (limits
loading or failed) → the action is shown disabled.
`storedLimitGoal` and `isPartialLimit` move into `src/utils/cashflow-budget-rows.ts`; the desktop page
calls them instead of its inline copies (same logic).

### Empty state and errors

Empty only when the period has no transactions **and** no limit row (desktop). "noAccounts" is still
decided by the accounts list. The report error replaces the body when nothing is shown; while
something is shown it is a line above it. A failed `budget-goal-history` adds its own error line
with retry.

### Scope (B5)

`accountScope(accountIds, accounts)` → `null` (all) or the number of chosen accounts that exist. The
subtitle reads "Skutečné příjmy a výdaje ze všech účtů" (new key `cashflow.subtitleAll`) or the
existing `cashflow.subtitle` with that number; the menu's "Účty (N)" uses the same number.

## Decisions

- **(call) The budget summary note goes.** The desktop dropped it (merge spec §1), C10 asks for the
  filter instead. Cost if wrong: one component to restore from history.
- **(call, review ruling) The limit line only on rows with a limit.** A row without one keeps the
  share bar; "Nastavit limit" is offered through "Spravovat limity" and, in the "Bez limitu" filter,
  on each row. A per-row "Bez limitu · Nastavit limit" everywhere was noise for profiles that do not
  budget. Cost: setting a first limit from the report takes the filter or the head action.
- **(call) The limit line sits under the row (`below`)**, not in the row, so the button is not
  nested in the link and keeps 44 pt. Cost: a budgeted row is one line taller.
- **(call) Rows that can never have a limit (Nezařazeno, a non-expense category) have no limit line**,
  even in the "Bez limitu" filter; the desktop prints "—". Cost: none beyond the missing dash.
- **(call) The filter is hidden while no row has a limit**; the sort pill stays. Cost: a profile
  without limits does not learn about the filter until it sets one.
- **(call) Sort is a `SortPill` beside the filter, wrapping under it when they do not fit**, not a
  second segment. Cost: one more tap to change the sort.
- **(call) The spent amount is never red**, as in the desktop's Utraceno cell; the state line says
  "over" in words, colour and the arrow.
- **(call) Count words follow the desktop ("transakce")** on this screen's rows (D lists "pohyby" vs
  "transakce"; `cashflow.moves` stays for the Budgets page).
- **(call) Transfers section is always shown**, with "V tomto období nic" when empty, because its
  footer carries the cash movement. Cost: one more section in a quiet month.
- **(call) The newest-transaction date is read with `staleTime: Infinity`.** The desktop's observer
  has the default `staleTime: 0` but only waits while that refetch is in flight on its first mount;
  in practice both use a cached date and wait only while an invalidation (a money write) refetches.
  Cost: none known; every money write invalidates the key.
- **(call) The limit actions wait for the stored limits' data** (`history !== undefined`, the
  desktop's `history != null`), so a failed background refetch does not switch them off.
- **(call) The CSV export stays** in `···` (F leaves it to the owner).
- **(call) "Součet za zvolené období" (the desktop card description) is not repeated** on the phone:
  the period bar is right above.
