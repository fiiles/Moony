# Cashflow report by period and category

Approved by the owner on 2026-10-09 after a design conversation. The Cashflow report
(`/reports/cashflow`) is rebuilt around a calendar period, income by category, and a
visible place for investments, savings and transfers. The drilldown dialog lets the user
fix a category without leaving the report.

## Problem

- **Income by counterparty misleads.** A credit without a category counts as income by
  direction and the report lists it under the counterparty's name. A friend's
  reimbursement for a dinner therefore shows up as an "income source" until it is
  categorized, and nothing on the page says so or lets the user fix it.
- **The range model adds noise.** Six months, twelve months or year-to-date, each
  compared with the preceding range, produces a "vs. previous" column that forces a
  horizontal scroll in a half-width card and rarely changes a decision.
- **Investments and savings are hidden.** Money moved to a broker sits in a bottom table
  grouped by flow kind. The system category Savings has kind `expense`, so a transfer to
  an untracked savings product counts as spending and lowers the savings rate.
- **The drilldown is read-only.** Seeing a wrongly categorized row means opening the
  bank account ledger, finding the row again and changing it there.

## Money model (unchanged rules, made visible)

The backend already resolves the meaning of a booked row in this order: explicit choice
on the row, own-account identity, the category's kind, and only then credit/debit
direction. The report makes that model the organizing principle:

- **Income** is every row whose effective kind is `income`: rows in a category of kind
  income, uncategorized credits, explicit choices. Grouped by category; uncategorized
  credits form their own row so they are visible and fixable.
- **Expenses** are rows of kind `expense`, grouped by category, signed so that a credit in
  an expense category is a refund that lowers the category. A category may go negative
  in a month when the refund arrives after the month boundary; the row keeps its sign.
- **Investments, savings and transfers** are rows of kind `investment`, `transfer`,
  `financing` or `adjustment`. They are not consumption and never enter income, expenses
  or the savings rate. They are grouped by category (Investments, Savings, Internal
  Transfers, any custom category of those kinds); uncategorized rows of a kind form one
  row named after the kind. Amounts are credit-positive: money leaving the tracked
  accounts is negative.
- **Investment yield is income.** Dividends, interest and rent belong in a category of
  kind income, never in Investments (there they would read as returned capital).

### Migration 010 `010_income_categories`

- Renames the system category `cat_income` from "Income" to **"Salary"** ("Mzda") when
  its stored name is still the seeded "Income"; a user who renamed it keeps their name.
  `SYSTEM_CATEGORY_DEFAULT_NAMES.cat_income` becomes `Salary` and `categoryNames.cat_income`
  reads "Salary" / "Mzda". The icon is unchanged.
- Seeds two system categories of kind `income`, translated like the existing ones:
  `cat_investment_income` ("Investment income" / "Investiční příjem") and
  `cat_other_income` ("Other income" / "Ostatní příjem"). Icons come from the existing
  category icon map; sort order places them right after Salary.
- Changes `cat_savings` from kind `expense` to `transfer`, only when it is still
  `expense`. A user who already chose another kind keeps it. Historical reports change
  accordingly: savings leave expenses and the savings rate rises. The changelog says so.
- `SYSTEM_CATEGORY_DEFAULT_NAMES` and `categoryNames.*` in both locales gain the two ids.

The category editor's "Role v přehledech" select keeps its six roles. Savings is not a
role because saving is a transfer; financing (loan principal) and adjustment
(corrections) are rare but have no other home.

## Page design

The page keeps the design system (§7 Report): page head with actions, three stats, a
hero chart card, table cards. Both locales are updated for every string.

### Period navigation

The same control as Budgets, extracted into a shared `PeriodNav` component that both
pages use: a segmented Month / Quarter / Year switch and ‹ label › buttons, "next"
disabled at the current period. Default on Cashflow is **Year** (the current year), on
Budgets it stays Month. Boundaries are UTC calendar periods from `getUtcPeriodRange`.
The 6m / 12m / YTD segmented control, the comparison range line, the comparison FX
warning and every "previous" value disappear. Account scope and CSV export stay in the
head.

### Chart

Twelve monthly bars (income, expenses) ending with the last month of the selected period:
for a year the months of that year, for a quarter or month the eleven months before it
plus the period itself. Months outside the period are **muted** (both series at 35 %
opacity); months of the period draw in full color. Future months of the current year are
empty bars. The tooltip is unchanged.

### Stats

Three stat cards for the selected period:

- **Příjmy / Income**: period total. Note "⌀ {amount} / měsíc" when the period has at
  least two completed observed months (the existing average rule), otherwise no note.
- **Výdaje / Expenses**: the same.
- **Úspora / Savings**: income − expenses, signed, gain/loss tone. Note: "míra úspor
  {rate}" when income is positive, otherwise no note.

The two text lines above the stats ("Běžné příjmy − výdaje", "Celkový pohyb peněz na
účtech") are removed; the first equals Savings, the second moves to the third card.

### Tables

Layout: a two-column grid. Left column: **Výdaje podle kategorií**. Right column:
**Příjmy podle kategorií** and, under it, **Investice, spoření a převody**. The expense
table is expected to be the tallest, so the right column stacks the two shorter cards
(design system §12, "the shorter column receives the content").

Every table has three columns: name with the transaction count, share (progress bar and
percent, share of the sum of absolute amounts), amount. No "vs. previous" column. Rows
sort by amount, largest first (absolute amount in the third card). Uncategorized rows are
named "Nezařazeno" in the expense and income tables; in the third card an uncategorized
row is named after its kind ("Převod", "Financování", "Úprava", "Investice").

**The whole row is the click target.** The row shows the pointer and the table hover; the
existing text button stays for keyboard users and is stretched over the row with an
absolutely positioned pseudo-element. Clicking opens the drilldown dialog for that
category (or uncategorized rows) and flow kind.

Footers: the expense card keeps "Celkem {amount}" and the Budgets link; the income card
keeps "Celkem {amount}" and the note that uncategorized income counts; the third card's
footer reads "Celkový pohyb peněz na účtech {amount}" (income − expenses + these flows).

### Drilldown dialog with categorization

`ReportTransactionsDialog` (shared with Budgets through `CategoryTransactionsDialog`)
gains a **Kategorie** column with the same compact `CategorySelector` the bank ledger
uses, including "remember for next time" rule learning. Changing a category calls the
existing update command and the money-flows refresh, so the report behind the dialog
recomputes immediately.

**The list stays stable while the dialog is open.** The dialog keeps the rows it first
loaded; a change updates that row's category locally. A row that no longer belongs to
the dialog's target (its category differs from the target category, or an uncategorized
target's row now has a category) is dimmed and labeled "přesunuto"; the selector still
shows its new category so the change can be reverted. The footer reads
"{count} transakcí · {moved} přesunuto" (the second part only when moved > 0). Closing
and reopening loads fresh rows.

The technical "Příjem · Role kategorie" line under the description is replaced by the
ledger's quiet flow label: nothing for ordinary rows, "Vratka" for a credit resolved as
an expense, the kind for explicit choices and uncategorized own-account transfers.

### Empty period

A period without booked rows in scope keeps the page head (so the user can navigate) and
shows the empty state "V tomto období nejsou žádné transakce" with the existing link to
bank accounts.

### CSV export

Sections: months of the chart window (month, income, expenses); expense categories;
income categories; other flows (name, amount, transactions). No previous-period columns.
File name `cashflow-<period label>.csv`.

## Contract

### Commands (`src-tauri/src/commands/cashflow.rs`)

```
get_cashflow_actuals(start_date: i64, end_date: i64, account_ids: Option<Vec<String>>,
                     report_currency: Option<String>) -> CashflowActuals
get_report_transactions(start_date, end_date, account_ids?, report_currency?,
                        flow_kind: Option<FlowKind>, category_id: Option<String>)
```

`source_key` leaves the drilldown command and the service. `end_date < start_date` is
`AppError::Validation("validation.cashflowPeriodInvalid")`, translated in both locales.

### Types (`src-tauri/src/models/cashflow_actuals.rs`, mirrored in `shared/schema.ts`)

```ts
interface CashflowMonth { month: number; income: string; expenses: string }

interface CashflowGroup {
  /** Category id, or `uncategorized` */
  key: string;
  /** Stored category name; empty for uncategorized */
  name: string;
  flowKind: FlowKind;
  /** Report currency; expense rows debit-positive, every other kind credit-positive */
  amount: string;
  /** Booked rows behind the row, including rows without a report conversion */
  count: number;
}

interface CashflowActuals {
  reportCurrency: string;
  periodStart: number;
  periodEnd: number;
  quality: ReportQuality;
  /** Chart window, oldest first, zero-filled: 12 months ending with the period's last month */
  months: CashflowMonth[];
  totalIncome: string;
  totalExpenses: string;
  /** totalIncome − totalExpenses */
  netBalance: string;
  /** netBalance + the sum of otherFlows */
  bankCashMovement: string;
  averageIncome: string | null;
  averageExpenses: string | null;
  averageCompletedMonths: number[];
  expenseCategories: CashflowGroup[];
  incomeCategories: CashflowGroup[];
  /** investment, transfer, financing and adjustment rows by category */
  otherFlows: CashflowGroup[];
  accountCount: number;
  transactionCount: number;
}
```

Removed: `range`, `rangeStart/End`, `previousStart/End`, `previousQuality`,
`previousIncome/Expenses`, `incomeSources`, `nonconsumptionFlows`, `CashflowGroup.previousAmount`,
`CashflowMonth.nonconsumptionFlows` and `CashflowMonth.bankCashMovement`.
`BudgetingReport.nonconsumptionFlows` keeps using `CashflowGroup` with `key` and `name`
set to the kind and `flowKind` set to the same kind. `CashflowRange` leaves `schema.ts`.

### Frontend wrappers (`src/lib/tauri-api.ts`)

```ts
cashflowApi.getActuals(startDate, endDate, accountIds?, reportCurrency?)
cashflowApi.getTransactions(startDate, endDate, { accountIds?, flowKind?, categoryId?, reportCurrency? })
```

Query key: `['cashflow-report', 'actuals', start, end, accountIds, currencyCode, ratesTimestamp]`.

## Backend behavior (`services/cashflow_actuals.rs`)

- `chart_window(start, end) -> (window_start, end)`: `window_start` is the first day of
  the month eleven months before the month of `end`, or the first day of the month of
  `start` when that is earlier.
- One read of booked rows for `[window_start, end]` in the report currency through the
  shared `transactions_in_currency`. Rows with `booking_date` in `[start, end]` are the
  period rows; every window row feeds `months`.
- Period rows with a report conversion feed totals and groups by `(effective kind,
  category id or uncategorized)`. The count pass includes period rows without a
  conversion, as today, and a group with only unconverted rows is not added as a zero.
  Category names come from one lookup of `transaction_categories`, not a query per row.
- `quality`, `averageIncome`, `averageExpenses` and `averageCompletedMonths` keep the
  existing rules, evaluated over the period rows and the period's months.
- `accountCount` and `transactionCount` describe the period.

## Documentation

- Design system README decision 4 is rewritten: period navigation, no comparison,
  income by category, the third card, in-place categorization.
- `docs/architecture/database.md`: the `transaction_categories` row mentions migration
  010 (two income categories, Savings as transfer).
- `CHANGELOG.md` Unreleased: the redesign, the Salary rename, the two new income
  categories, and the Savings change with its effect on history.
- Budgets' period strings move to `common.json` (`periods.timeframe.*`, `periods.prev.*`,
  `periods.next.*`) because `PeriodNav` is shared.

## Testing (per `docs/standards/testing.md`)

- Rust service tests with a raw in-memory connection: chart window for a month, a
  quarter and a year; period totals exclude window rows outside the period; refunds in
  expense categories net and never enter income; income grouped by category with an
  uncategorized row; other flows grouped by category with per-kind uncategorized rows;
  bank cash movement reconciles with income − expenses + other flows; averages and
  quality scoped to the period; account scope; invalid period rejected; migration 010 on
  a fresh database and on one where the user had already changed Savings.
- TypeScript tests for pure utilities only: the CSV builder, the "moved row" rule of the
  dialog (`src/utils/report-drilldown.ts`), existing period utilities.
- No component or hook tests. Gates: lint, typecheck, vitest, cargo fmt/clippy/test,
  bindings regenerated without drift.

## Out of scope

The Cashflow planning page, MCP tools (their cashflow report is the planned one), the
bank ledger, automatic categorization of reimbursements, and any change to Budgets beyond
the shared period control and the shared dialog.
