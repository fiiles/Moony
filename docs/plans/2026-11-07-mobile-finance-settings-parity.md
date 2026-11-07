# Plan: mobile finance and settings parity

Spec: `docs/specs/2026-11-07-mobile-finance-settings-parity-design.md`. One commit per task; gates
(typecheck, lint, format:check, the relevant vitest) green before each. Every new string in all ten
locales.

| # | Task | Files | Interfaces | Tests |
|---|---|---|---|---|
| 1 | A6 JSON export, integrity, attachments, country | `src-tauri/src/services/backup.rs`, `commands/backup.rs`, `lib.rs`; `src/lib/tauri-api.ts` (`dataApi.stageDataExport`); `src/utils/attachment-status.ts`; `components/settings/AttachmentsStatusRow.tsx`; `mobile/pages/settings/BackupSection.tsx`, `ProfileSection.tsx` | `stage_data_export() -> ExportSummary`; `attachmentStatusLines(status)` | Rust allow-list test; `attachment-status.test.ts` |
| 2 | A7 delete a balance record | `mobile/sheets/BalanceHistorySheet.tsx`, `AccountDetail.tsx` | sheet takes `accountId` | — (component) |
| 3 | A8 auto-categorize + Úročení | `AccountDetail.tsx`, `mobile/lib/account-menu.ts` | `showsInterest(account)` | `account-menu.test.ts` |
| 4 | A12 Začínáme + add account directly | `mobile/components/GettingStartedCard.tsx`, `mobile/lib/getting-started.ts`, `Overview.tsx`, `Finance.tsx` | `phoneGettingStartedItems(progress, prefs)` | `getting-started.test.ts` |
| 5 | B3 loan reminders in Údržba | `mobile/lib/upkeep.ts`, `UpkeepCard.tsx`, `LoanDetail.tsx`, `mobile/lib/loan-event-param.ts` | `UpkeepKind` + 2; `loanEventFromSearch()` | `upkeep.test.ts`, `loan-event-param.test.ts` |
| 6 | B4 hide/snooze phone rows | `mobile/lib/upkeep-dismissals.ts`, `mobile/hooks/use-upkeep-dismissals.ts`, `upkeep.ts`, `use-upkeep.ts`, `UpkeepCard.tsx` | `dismissKey`, `isDismissed`, `hide`, `snooze` | `upkeep-dismissals.test.ts`, `upkeep.test.ts` |
| 7 | B6 Nechat undo + way back | `src-tauri/src/services/categorize_queue.rs`, `commands/categorization.rs`, `models/categorization.rs`, `lib.rs`; `tauri-api.ts`; `use-categorize-queue.ts`; `CategorizeQueue.tsx` | `unsnooze_categorization(input)` | Rust service tests |
| 8 | C9 Připomínky lead | `RemindersSection.tsx`, locales | — | locale parity |
| 9 | E Finance export | `use-class-export.ts`, `Finance.tsx`, `backup.rs` allow-list | `ExportKind` + `loans`, `insurance` | Rust allow-list test |
| 10 | E Rozpočty | `mobile/lib/budget-rows.ts`, `Budgets.tsx` | `BudgetRow.history` | `budget-rows.test.ts` |
| 11 | E stale comments + STALE_BALANCE_DAYS pin | `use-cashflow.ts`, `Bonds.tsx`, `Hero.tsx`, `PropertyDetail.tsx`, `upkeep.test.ts` | — | pin test reads `services/milestones/accounts.rs` |
| 12 | C11 recent moves open; tx sheet from `?tx=` | `mobile/lib/move-path.ts`, `Overview.tsx`, `AccountDetail.tsx` | `movePath(move)` | `move-path.test.ts` |
| 13 | C11 loan payments in Nejbližší platby | `upcoming-premiums.ts`, `UpcomingPremiums.tsx`, `Finance.tsx` | `upcomingLoanPayments(rows, today)` | `upcoming-premiums.test.ts` |
| 14 | C11 legend, Toky placeholder, Údržba unfolded, one day formatter, TransactionSheet footer, position price refresh | `Overview.tsx`, `AccountDetail.tsx`, `UpkeepCard.tsx`, `mobile/hooks/use-transaction-day.ts`, `QueueRow.tsx`, `CashflowCategory.tsx`, `TransactionSheet.tsx`, `PositionMenu.tsx` | `useTransactionDay()` | — |
| 15 | Self-review of `git diff main...HEAD`, full `npx vitest run`, report | `.superpowers/audit-report.md` | | |
