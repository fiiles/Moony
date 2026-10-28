# Cashflow with budgets — plan

Spec: `docs/specs/2026-10-28-cashflow-with-budgets-implementation-design.md`.
Worktree `../Moony-tauri-cashflow-budgets`, branch `feat/cashflow-budgets` (no commits until the
owner asks).

1. **Service test (Rust).** `services/budgeting.rs` tests: the budgeting and cashflow reports agree
   per expense line (refund, uncategorized, budget-only category, account scope, paired own-account
   transfer and bank cash movement). No production Rust change expected.
2. **Pure rows (TDD).** `src/utils/cashflow-budget-rows.ts` + test: `expenseRows`, `limitState`,
   `filterCounts`, `filterRows`, `sortRows`; `monthsWithLimit` in `budget-goal-versions.ts` + test.
3. **Category manager.** Extract `CategoryManager` from `CategoriesCard`; `CategoriesCard` wraps
   it; `CategoryManagerDialog` for Cashflow.
4. **Chart months for the keyboard.** `MoonyBarChart` optional `slotAction` → `CategoryAxis` renders
   buttons; Cashflow wires bar click + buttons to "open this month".
5. **Cashflow page.** Combined query, latest-transaction start, stats, chart, expenses card with
   filter/sort/limit cells/menu, income and transfer cards, dialogs (`BudgetGoalDialog`,
   `BudgetManageDialog`, `ReportTransactionsDialog`, category manager), no export.
6. **Navigation.** nav-config + test, `/reports/budgeting` redirect, dashboard/getting-started links;
   delete `Budgeting.tsx`, `CategoryTransactionsDialog.tsx`, `periodFileStem`.
7. **Locales.** New keys in en/cs/de/es/it/nb/pl/sv/ja/zh; parity test.
8. **Docs.** Design-system README (§5 nav, §7 page table, §11 Cashflow), architecture overview row,
   DEPRECATED entry for the Budgets page.
9. **Verify.** Gates (`lint`, `typecheck`, `format:check`, `test`, `build`, `cargo fmt/check/clippy/
   test`); run the app on a copied demo profile (port 1422, own identifier) and drive it through
   the Vite dev bridge with Playwright: scenarios 1–12 of the brief, light/dark, 1080/1280/1440,
   de + ja; screenshots.
