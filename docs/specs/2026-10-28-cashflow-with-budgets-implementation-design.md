# Cashflow with budgets — implementation design

Date: 2026-10-28. Status: approved by the owner's handoff brief
(`docs/plans/2026-10-28-cashflow-budgets-handoff-prompt.md`). Visual reference:
`docs/specs/2026-10-28-cashflow-budgets-prototype.html` (layout and hierarchy only; its data model,
rounding and DOM code are not a pattern). The earlier snapshots
`2026-10-28-cashflow-budgets-design.md` and `…-revision-2.md` contain elements the owner removed
afterwards (attention card, budget summary strip, export) and are not followed.

## 1. Goal

One desktop screen, **Cashflow** (`/reports/cashflow`), replaces the separate Budgets page
(`/reports/budgeting`). Budgets live inside the *Výdaje podle kategorií* table; category management
opens from that card. Income, *Investice, spoření a převody*, drilldowns with recategorization and
the management of limit intervals stay.

Explicitly **not** on the screen: an attention card, a budget summary strip ("Rozpočty celkem",
"Zbývá v rozpočtech", …), an export (icon, menu or CSV). Shared export utilities used by other
screens (`ExportButton`, `buildCashflowCsv` for the phone) stay.

## 2. Data

No new command, wire type or migration. The two existing reports already compute everything:

| Need | Source |
|---|---|
| stats, averages, chart months, expense/income/transfer groups, bank cash movement, quality | `get_cashflow_actuals` (`services/cashflow_actuals.rs`) |
| limit in effect for the period (monthly versions integrated over the quarter/year), native spend, native limit, percentage, budget currency | `get_budgeting_report` (`services/budgeting.rs`) |
| stored versions (scheduled change, partial coverage, edit target) | `get_budget_goal_history` |

Both reports read the same ledger rows (`reporting::transactions_in_currency`), so their per-category
expense amounts agree. A new service test pins that agreement for refunds, uncategorized rows, the
account scope and a paired own-account transfer.

**One query, both reports.** The page fetches both reports in one `useQuery` with
`Promise.all`, keyed `['cashflow-report', 'overview', start, end, timeframe, accountIds,
reportCurrency, fxRevision]`. The `cashflow-report` prefix is in `MONEY_FLOW_KEYS`, so every money
write refreshes it. A single query means the expense rows are never built from one report of the new
filter and one of the old: loading, placeholder (previous period dimmed), error and retry are one
state. `keepPreviousData` keeps the old pair visible while the new one loads.

**Opening period.** Monthly, on the month of the newest booked transaction
(`budgeting-latest-transaction`, the Budgets page's behaviour); the report query waits for that
lookup once; a later refetch never moves a period the user picked.

## 3. Expense rows (pure logic, `src/utils/cashflow-budget-rows.ts`)

- Rows are the cashflow expense groups (a group with no transactions and no amount is dropped, as
  today) plus every category whose budgeting summary carries a `budgetGoal` for the period but has
  no group (limit with zero spend). Joined by the stable key (category id / `uncategorized`), never
  by a name. Amount and count come from the cashflow group; a budget-only row takes the budgeting
  summary's count and amount, so nothing is counted twice.
- Shares are absolute-movement shares over all rows (`absoluteMovementShares`): a refund-heavy
  category with a negative net gets a positive share and no negative bar width.
- Limit evaluation uses only `budgetCurrency`, `budgetSpent`, `budgetLimitNative`,
  `budgetPercentage`. Compared in integer minor units of the budget currency:
  `over` (spent > limit), `exhausted` (spent = limit — new label "Limit vyčerpán"), `near`
  (≥ 85 %), `ok`. Any missing native value or a non-finite percentage is `unavailable`: the row
  still has a limit ("Čerpání nelze vyhodnotit"), never "Bez limitu", never "V pořádku".
- Remaining = native limit − native spend in the budget currency, shown with that currency; a report
  amount in another currency sits in its own column, so the two are never subtracted.
- Filters `Vše` / `Nad limitem` / `Bez limitu`, counted over all rows. `Bez limitu` = rows without a
  period limit (uncategorized and non-expense categories included); unavailable rows are not in it.
- Sorts: amount (desc), usage (evaluated percentage desc, then unavailable, then no limit), name
  (`localeCompare` with the UI locale). Ties fall back to amount, then the key: deterministic.
- Filters and sorts never change the stats, the side cards or the table total; the footer names
  the filtered sum separately when a filter is on.

Partial coverage of a quarter/year ("Jen část období") is derived from the stored versions of the
period's goal: months of the period with a limit in effect < months of the period.

## 4. Screen

- Head: eyebrow *Report*, title *Cashflow*, the linked subtitle to *Plánování cashflow*,
  `PeriodNav` + `AccountScope`. No export.
- Three stats (unchanged maths), the bar chart card (unchanged chart). A bar click, or the month
  buttons under the axis for the keyboard, opens that month (monthly timeframe); months after the
  current one are not actionable.
- Grid: expenses card left; right column `clamp(300px, 24vw, 360px)` with income and *Investice,
  spoření a převody* stacked; below 1240 px viewport the right column moves under the expenses as
  two cards side by side.
- Expenses card head: title, "Součet za zvolené období", `Správa kategorií` (opens the shared
  category manager) and `Spravovat limity` beside it (the existing `BudgetManageDialog`).
  Then the two `Segmented` controls (filter with counts, sort), the table and a total footer.
- Row: icon chip + name (button that opens the transactions) with "N transakcí · X % výdajů";
  Utraceno (report currency); Limit (a button "5 000 Kč" → edit, or "Nastavit limit" for an expense
  category, "—" for uncategorized/non-expense) with the partial-period note;
  Čerpání (bar + % + state word; colour is never the only signal); Zbývá (remaining or "−X nad
  limitem" in the budget currency, "—" with a title when unavailable). The name button, labelled
  "Zobrazit transakce: …", opens the transactions; there is no separate icon column. The limit cell
  notes only "Jen část období"; earlier, ended and scheduled limits stay in Spravovat limity. The limit
  button stops propagation. Between 1240 and 1439 px (beside the right column) "Zbývá" moves under the
  usage bar, so no amount is cut.
- Dialogs return focus to the control that opened them (`DialogContent` remembers it; a menu item
  hands over to its menu's trigger).
- Drilldowns use `ReportTransactionsDialog` (expense rows with `flowKind: 'expense'`, income and
  transfer groups with their own `flowKind`), with the page's period, accounts and report currency.
- Empty state only when the period has no transaction **and** no limit; limits alone keep the
  table.

## 5. Category management

`CategoriesCard`'s list, create/edit (`CategoryFormDialog`) and delete-with-reassign move into
`src/components/categories/CategoryManager.tsx`. Settings renders it inside its card; the Cashflow
screen renders it in a dialog (`CategoryManagerDialog`). One implementation, the same backend rules
(system categories cannot be deleted, a used category is reassigned first).

## 6. Navigation

- `nav-config`: Peníze = Bankovní účty, Cashflow (also in the onboarding preview); the `nav.budgets`
  key goes.
- `/reports/budgeting` redirects (replace) to `/reports/cashflow`; dashboard and getting-started
  links point to Cashflow.
- `src/pages/Budgeting.tsx` and `CategoryTransactionsDialog` are deleted after the move.
  `periodFileStem` loses its only caller and goes too. Mobile is untouched (its own Budgets screen
  keeps using the shared dialogs' logic and the same reports).

## 7. Copy

Reuse existing keys (`reports.actuals.*`, `budgeting.actions.*`, `budgeting.table.state.*`,
`budgeting.noBudget`, `budgeting.manage.title`, `bank_accounts.categoryManagement.*`). New keys, in
all ten locales: column heads (Utraceno, Limit, Čerpání, Zbývá), filter and sort labels, "Limit
vyčerpán", "nad limitem", "X % výdajů", "Jen část období", the filtered-total footer, the
category-manager dialog title/description, the chart month button label.
