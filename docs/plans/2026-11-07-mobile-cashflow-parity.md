# Plan: phone Cashflow parity (B2, B5, A16, B9, B11, C10)

Spec: `docs/specs/2026-11-07-mobile-cashflow-parity-design.md`.

## Task 1 — shared limit helpers (TDD)

- `src/utils/cashflow-budget-rows.ts`: `storedLimitGoal(row, history)`, `isPartialLimit(goal, start, end)`.
- `src/utils/cashflow-budget-rows.test.ts`: stored goal by category and timeframe (period goal's,
  else monthly); partial for a quarter covered 1 of 3 months, not for full or none.
- `src/pages/Cashflow.tsx`: `storedGoal` and `limitNote` call them (no behaviour change).

## Task 2 — phone pure logic (TDD)

- `src/mobile/lib/budget-rows.ts`: `rowLimitMode(row, goals, periodStart, opts)` replaces
  `reportRowLimit` (tests rewritten): edit the stored goal, else the period copy, else add; `null`
  for a row that cannot have a limit; `ready: false` → disabled.
- `src/mobile/lib/cashflow-period.ts`: `openingState(state, chosen, latest, now)`; drop
  `latestMonthOffset` (tests).
- `src/mobile/lib/cashflow-rows.ts`: drop `RowLimit`, the budgets argument of `categoryRows`,
  `budgetCounts`; `bodyKind` takes `hasLimits` (empty only without transactions and limits); add
  `accountScope(accountIds, accounts)` (tests).

## Task 3 — one query and the opening period

- `src/mobile/hooks/use-cashflow.ts`: one overview query (desktop key), `enabled`, `expenses`.
- `useCashflowState` setter takes `{ pin }` to write `o=` even when the offset is unchanged.
- `src/mobile/pages/Cashflow.tsx`: latest-transaction query, `openingState`, report waits.

## Task 4 — the screen

- `ProgressBar`: optional `mark` (0–1).
- `CategoryRow`: formatted `value` from the page, desktop count words, no limit.
- `ExpenseCategoryRow`: rewritten on `EntityRow` with the `below` limit line and the long-press menu.
- `ExpensesSection` (new, `src/mobile/components/cashflow/`): head with "Spravovat limity", filter
  `Segment` + `SortPill`, rows, footer.
- `Cashflow.tsx`: `useReportMoney` everywhere, income/transfer footers, signed transfers, history
  error line, scope subtitle; delete `BudgetSummaryNote`.
- i18n: `mobile.cashflow.subtitleAll` in ten locales; drop unused `cashflow.budgetSummary`, `near`,
  `overLimit`, `limitProgress`.

## Task 5 — gates, self-review, report

`npm run typecheck`, `npm run lint`, `npm run format:check`, `npx vitest run`; read
`git diff main...HEAD`; write `.superpowers/audit-report.md`.
