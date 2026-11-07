# Plan: phone navigation and resilience

Spec: `docs/specs/2026-11-07-mobile-navigation-resilience-design.md`. TDD applies to every pure
function. Hooks and components are not unit-tested (testing policy).

## Tasks

1. **Nav stack pops with a target.**
   - Files: `src/mobile/lib/nav-stack.ts` (+test), `src/mobile/hooks/use-nav-stack.ts`.
   - `NavEvent.pop` gains `to?: number`.
   - New store functions: `popNavTo(index)`, `pushNavTrail(entries, navigate)` and
     `navStackSnapshot()`.
2. **Tab memory.**
   - Files: `src/mobile/lib/tab-memory.ts` (+test), `TabBar.tsx`, `MobileShell.tsx`, `Screen.tsx`.
   - Interface: `tabTrail(stack, tab)` and `tabTapPlan(stack, tab, remembered)`, returning
     `{ kind: 'scrollTop' } | { kind: 'popTo'; index } | { kind: 'push'; entries }`.
   - `Screen` listens to the `moony:scroll-top` event.
3. **Lock return.**
   - Files: `src/mobile/lib/lock-return.ts` (+test), `MobileShell.tsx`, `Lock.tsx`.
   - Interface: `createLockReturn()` with `seen`, `locked(sheetDepth)`, `take()`, `clear()`.
   - Pure helper `isShellPath(path)` in `routes.ts` (+test).
4. **Sheet history refusals and the dirty guard.**
   - Files: `src/mobile/lib/sheet-history.ts` (+test), `use-sheet-history.ts`, `Sheet.tsx`,
     `src/mobile/lib/form-dirty.ts` (+test).
   - Sheets: RecordBalances, AddTransaction, Dividend, Cost, OtherAssetAdd, PlanForm, Revalue.
   - Strings `mobile:sheet.discard.*` in all ten locales.
5. **Desktop dialogs join the back handling.** File: `src/components/ui/dialog.tsx` (phone `Root`).
6. **Error states.**
   - Files: `src/mobile/lib/load-state.ts` (+test) and `src/mobile/components/LoadError.tsx`.
   - Hook changes: `useFinance`, `useWealth`, `useNetWorthOverview` (`metricsError`, `retry`),
     `useLoans` and `useInsurance` (`isError`, `refetch`, additive), and the detail hooks.
   - Pages: Overview, Finance, Wealth and the details. `LoadFailed` gains the retry.
   - Strings: `mobile:loadError.*`.
7. **Notification targets.**
   - Files: `notification-plan.ts` (+test, `path`, `canBackup`), `use-notification-schedule.ts`,
     `src/mobile/lib/notification-target.ts` (+test), and
     `src/mobile/components/NotificationTargetHost.tsx`, mounted in `MobileShell`. `Lock` takes the
     pending target.
8. **C7.**
   - Files: `upkeep.ts` (+test, `canBackup`), `use-upkeep.ts`, `first-run.ts` (+test,
     `canBackup`), `use-first-run.ts` (finish when no step follows).
9. **C8 and C11.**
   - `Lock.tsx`: save the recovered key to Files; restore from a backup.
   - `MobileApp.tsx`, `MobileShell.tsx`, `index.css`: the toast lift.
10. **Gates and self-review.**
    - Run typecheck, lint, format:check and the full vitest suite.
    - Read `git diff main...HEAD` and write the report at `.superpowers/audit-report.md`.
