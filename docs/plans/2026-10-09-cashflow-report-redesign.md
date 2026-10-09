# Cashflow report by period and category implementation plan

> **For agentic workers:** executed inline by the main session (it holds the design
> context); reviews go to Sonnet subagents. Steps use checkbox (`- [ ]`) syntax.

**Goal:** The Cashflow report shows a calendar period (month / quarter / year) with
income and expenses by category, a separate card for investments, savings and
transfers, and a drilldown dialog that recategorizes in place. Flow roles collapse to
income / expense / transfer.

**Architecture:** One migration renames and seeds income categories and collapses the
roles. The actuals service takes `start`/`end`, reads one 12-month chart window and
groups period rows by `(kind, category)`. The page shares a `PeriodNav` with Budgets;
the shared drilldown dialog snapshots its rows and embeds the ledger's
`CategorySelector`.

**Tech Stack:** Tauri 2, Rust + rusqlite, React 19, TanStack Query, Recharts, Vitest.

**Spec:** `docs/specs/2026-10-09-cashflow-report-redesign-design.md`
**Branch:** `codex/cashflow-report-redesign` from `main` (in place, local merge, no push).

## Global Constraints

- `CategorySelector.tsx` and `BankAccountDetail.tsx` carry uncommitted changes from
  another session. Never stage them; stage only this plan's hunk in the ledger page.
- Verify `git branch --show-current` before every commit.
- Every user-facing string lands in `en/` and `cs/`.
- Money stays decimal TEXT, timestamps unix seconds UTC, reports declare their currency.
- Services take `&Connection` and return `AppError`; no `.unwrap()` outside tests.
- Migrations append-only: `010_income_categories` after `009_scoped_learning`.
- Rust tests use raw in-memory connections; TS tests cover pure utilities only.

---

### Task 1: Three flow roles and migration 010

**Files:**
- Modify: `src-tauri/src/models/bank_transactions.rs` (enum, `Display`, `FromStr`)
- Modify: `src-tauri/src/db/migrations.rs` (const `MIGRATION_010`, Vec entry, tests), `src-tauri/src/db/schema.snapshot`
- Modify tests using removed kinds: `services/financial_semantics.rs`, `services/bank_accounts.rs`, `services/budgeting.rs`, `services/reporting.rs`
- Modify: `src-tauri/src/services/mcp/mod.rs` (budgeting tool description)
- Modify: `shared/schema.ts` (`FlowKind`), `src/components/categories/CategoryFormDialog.tsx`, `src/pages/BankAccountDetail.tsx` (flow select only), `src/utils/money-flows.test.ts`, `src/utils/category-name.ts`
- Modify locales: `bank_accounts.json`, `reports.json` (both languages)

**Interfaces:**
- Produces: `FlowKind = 'income' | 'expense' | 'transfer'` (Rust `FlowKind::{Income, Expense, Transfer}`); system categories `cat_investment_income`, `cat_other_income`; `cat_income` seeded name `Salary`.

- [ ] Write failing tests in `migrations.rs`:
  `income_categories_migration_renames_seeds_and_collapses_roles` (chain to 009, insert
  custom categories with kinds investment/financing/adjustment and a transaction with
  `flow_kind='financing'`, run all; expect `cat_income` named `Salary`, two new income
  categories, `cat_savings` and every collapsed kind `transfer`, the row `transfer`),
  `income_categories_migration_keeps_user_choices` (renamed `cat_income` stays renamed,
  `cat_savings` set to `income` stays `income`). In `bank_transactions.rs`:
  `flow_kind_rejects_removed_roles` (`"investment".parse::<FlowKind>()` is `Err`).
- [ ] Run `cargo test income_categories_migration flow_kind_rejects` and see them fail.
- [ ] Implement:

```sql
-- MIGRATION_010
UPDATE transaction_categories SET name = 'Salary' WHERE id = 'cat_income' AND name = 'Income';
INSERT OR IGNORE INTO transaction_categories (id, name, icon, color, parent_id, sort_order, is_system, kind) VALUES
    ('cat_investment_income', 'Investment income', 'building', '#14B8A6', NULL, 10, 1, 'income'),
    ('cat_other_income', 'Other income', 'wallet', '#84CC16', NULL, 10, 1, 'income');
UPDATE transaction_categories SET kind = 'transfer' WHERE id = 'cat_savings' AND kind = 'expense';
UPDATE transaction_categories SET kind = 'transfer' WHERE kind IN ('investment', 'financing', 'adjustment');
UPDATE bank_transactions SET flow_kind = 'transfer' WHERE flow_kind IN ('investment', 'financing', 'adjustment');
```

  Drop the three variants from the enum; fix compile errors and the tests that used the
  removed kinds (`'investment'`/`'adjustment'` literals become `'transfer'`). Limit the
  existing 006 test to the chain through 006 so it keeps asserting 006.
- [ ] `cargo test regenerate_schema_snapshot -- --ignored`; the diff shows only seed rows.
- [ ] Frontend: `FlowKind` in `schema.ts`; both selects list three roles; labels
  `readiness.flow.transfer` = "Přesun peněz" / "Money transfer", `expense` = "Výdaj" /
  "Expense", remove `investment`/`financing`/`adjustment` keys in both namespaces;
  `roleImpact` help text from the spec; `categoryNames.cat_income` = "Mzda" / "Salary",
  new `cat_investment_income`, `cat_other_income`; `SYSTEM_CATEGORY_DEFAULT_NAMES`.
- [ ] `cargo test`, `npm run typecheck`, `npm test`; commit (ledger page via a
  hunk-only index update).

### Task 2: Period-based actuals service and contract (Rust)

**Files:**
- Modify: `src-tauri/src/services/cashflow_actuals.rs` (rewrite), `src-tauri/src/models/cashflow_actuals.rs`, `src-tauri/src/models/reporting.rs` (`ReportTransaction` +2 fields), `src-tauri/src/services/reporting.rs` (fill them, drop `source` filter), `src-tauri/src/commands/cashflow.rs`, `src-tauri/src/services/budgeting.rs` (`CashflowGroup` shape), `shared/generated-types.ts`

**Interfaces:**
- Produces:
  `cashflow_actuals::validate_period(start: i64, end: i64) -> Result<()>`,
  `cashflow_actuals::chart_window_start(end: i64) -> i64`,
  `cashflow_actuals::get_actuals_in_currency(conn, start, end, today, accounts: Option<&[String]>, report_currency: &str) -> Result<CashflowActuals>`;
  `reporting::filtered_transactions_in_currency(conn, start, end, accounts, kind, category, report_currency)`;
  commands `get_cashflow_actuals(start_date, end_date, account_ids?, report_currency?)`,
  `get_report_transactions(start_date, end_date, account_ids?, report_currency?, flow_kind?, category_id?)`;
  `CashflowGroup { key, name, flow_kind, amount, count }`, `CashflowMonth { month, income, expenses }`,
  `CashflowActuals` per the spec (`transfers`, `incomeCategories`, `periodStart/End`).

- [ ] Replace the range tests with failing period tests: chart window for a month, a
  quarter and a year; window rows outside the period feed `months` but not totals;
  refunds net inside expense categories; income by category with `uncategorized`;
  transfers by category with one uncategorized group; `bank_cash_movement` =
  income − expenses + transfers; averages need completed months inside the period;
  account scope; `end < start` rejected with `validation.cashflowPeriodInvalid`;
  counts include unconverted rows. Keep the FX, AED/JPY and refund tests, ported to periods.
- [ ] Run `cargo test cashflow_actuals` and see them fail.
- [ ] Implement; one category-name lookup per report; group key `(kind, category)`.
- [ ] Update the budgeting parity test to call the period API.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test generate_bindings -- --ignored`; commit.

### Task 3: Report UI, shared period control and categorizing dialog

**Files:**
- Create: `src/components/reports/PeriodNav.tsx`, `src/hooks/use-period-selection.ts`, `src/utils/report-drilldown.ts` + `.test.ts`
- Modify: `shared/schema.ts`, `src/lib/tauri-api.ts`, `src/utils/report-currencies.ts` + test, `src/utils/period.ts` + test (`periodFileStem`), `src/components/charts/MoonyBarChart.tsx` (`BarGroup.muted`), `src/pages/Cashflow.tsx` (rewrite), `src/pages/Budgeting.tsx` (use `PeriodNav`), `src/components/reports/ReportTransactionsDialog.tsx`, locales `reports.json`, `budgeting.json`, `common.json`

**Interfaces:**
- Consumes: Task 2 commands and types.
- Produces: `usePeriodSelection(initial: PeriodTimeframe) -> { timeframe, setTimeframe, offset, setOffset, range, label }`;
  `<PeriodNav selection={...} />`; `periodFileStem(timeframe, startDate) -> string`;
  `isMovedOut(targetCategoryId: string | undefined, currentCategoryId: string | null) -> boolean`;
  `buildCashflowCsv(report, monthLabel, groupLabel)`.

- [ ] Failing Vitest: `periodFileStem` (2026, 2026-q4, 2026-10), `isMovedOut`
  (uncategorized target, category target, moved back, no target), CSV sections without
  previous columns. Run `npm test` and see them fail; implement; pass.
- [ ] Mirror the contract in `schema.ts` and `tauri-api.ts`; remove `CashflowRange`.
- [ ] `PeriodNav` + hook; Budgets switches to it (strings move to `common.periods.*`).
- [ ] `MoonyBarChart`: optional `muted` per bar, drawn at 35 % opacity via `Cell`.
- [ ] Cashflow page per the spec: head with `PeriodNav` (default yearly), account scope,
  export; three stats; chart; grid with expenses left and income + transfers right;
  whole-row clicks; empty period state; quality note.
- [ ] Dialog: query key `['report-drilldown', …]`, `gcTime: 0`; local category changes;
  `CategorySelector` column; dimmed moved rows; footer count; quiet flow label.
- [ ] `npm run lint`, `npm run typecheck`, `npm test`, locale key parity; commit.

### Task 4: Docs, verification, review, merge

- [ ] Design system README decision 4, `docs/architecture/overview.md`,
  `docs/architecture/database.md`, `CHANGELOG.md` Unreleased.
- [ ] Full gates: `npm run lint && npm run typecheck && npm test`,
  `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`, bindings drift.
- [ ] Visual check in the demo profile (`moony-demo` launch config): year, quarter and
  month; muted months; whole-row click; recategorize in the dialog and see the report update.
- [ ] Code review by a Sonnet subagent against the spec; fix findings.
- [ ] Merge to `main` locally with `--no-ff`; no push.
