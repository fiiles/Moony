# Mobile Finance tab and account detail (Phase 2a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The phone's Finance tab (accounts, loans, policies, cash trend) and the account detail (balance hero, flows strip, ledger with infinite scroll, filters, transaction sheet, category picker with learning) on the existing Tauri commands, without any Rust change.

**Architecture:** Three pages under `src/mobile/pages/` on the Phase 1 shell (`Screen`, `NavBar`, `EntityRow`, `Hero`, `ScrubChart`, `Sheet`, `StatRow`), fed by the hooks the desktop pages already use (`useBankAccounts`, `useBankAccountActivity`, `useLoans`, `useInsurance`, `useBalanceRecords`, `usePortfolioHistory`) plus thin mobile hooks in `src/mobile/hooks/` for the ledger (`useInfiniteQuery` over `get_bank_transactions`, 100 rows a page), the account detail bundle and the categorization flow ported from `CategorySelector.tsx`. Every pure decision (row sub-lines, sorting, day grouping, filters, the balance step line, the learning outcome) lives in `src/mobile/lib/*.ts` with Vitest tests; components only render. Writes go through `useMutation` + `refreshMoneyFlows` (frontend rule 3 / AGENTS rule 7).

**Tech Stack:** React 19, TypeScript, wouter, TanStack Query (`useInfiniteQuery`), Radix (Dialog as the sheet, DropdownMenu as the context menu, ToggleGroup as the segment, Switch), Recharts through `ScrubChart`, Lucide 1.55, react-i18next (`mobile` namespace, ten locales), Tailwind on the design tokens.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.4 (binding), §10.3–10.5; prototype `docs/specs/2026-10-16-mobile-app-prototype.html` frames 03 and 04, notes 7 and 8; the phone component contract `docs/design-system/mobile.css` + `mobile.html` + `docs/design-system/README.md` §13. Parent plan: `docs/plans/2026-10-16-mobile-app.md` (Phase 2 outline). Out of scope here: the categorize queue (§8.5), the import ladder (§8.6), loan and policy details (Phase 4), the add forms, any Rust change.

## Global Constraints

- Worktree `.claude/worktrees/dark-mode-desktop-57bd25`, branch `claude/mobile-finance-2e89a2`; `git branch --show-current` before every commit; never push, never merge (the main mobile session merges this branch into `feat/mobile`).
- Files owned by the main mobile session are read-only here: `src/mobile/pages/Lock.tsx`, `src/mobile/MobileApp.tsx`, `src/mobile/pages/Overview.tsx`, `src/hooks/use-auth.tsx`, `src-tauri/**`, `src/components/**` (desktop; reuse only). `src/index.css` and `tailwind.config.ts`: only append new lines at the end of the relevant block, only when unavoidable. `src/mobile/shell/routes.ts` and `MobileShell.tsx`: add routes only. `src/i18n/locales/*/mobile.json`: add keys only, never rename or delete, in all ten locales, plural forms per `docs/standards/i18n.md` §2 (en/de/nb/sv `_one _other`; es/it `_one _many _other`; cs `_one _few _other`; pl `_one _few _many _other`; ja/zh `_other`).
- `invoke()` only in `src/lib/tauri-api.ts` (add a wrapper there if one is missing; none is expected to be missing — see the API inventory below).
- Money, days, numbers only through `useFormat()` / `useReportMoney` (frontend rule 8): `fmt.money(amount, currency, { signed, decimals })`, `fmt.day(unixSeconds, opts)`, `fmt.percent(ratio, digits)`, `fmt.month(date)`, `fmt.monthInSentence(date)`. Native amounts in their own currency; conversions through `useMainConvert()` / `useDatedFx()`; a missing rate renders "—", never 0 or 1:1.
- Tokens only (Tailwind token classes `bg-paper`, `text-ink-3`, `text-gain`, `bg-well-2`, `var(--s1)`…), no raw hex; dark mode comes free (spec §10.6).
- Contract values: touch targets ≥ 44 pt; side gutter 16 pt; entity row 60 pt (52 single-line); chevron only on rows without a trailing value (`EntityRow` enforces it); hero chart 160, trend chart 120, bars 110 (spec §10.4); type roles `text-m-*`; primary button = dark material, one per screen (the two-button pair under the detail's nav bar is the spec's kept exception).
- New components are added to the contract in the same commit: a class block in `docs/design-system/mobile.css`, a demo in `docs/design-system/mobile.html` (German copy, 360/390/430; the page's check must report no horizontal overflow at 360), a row in README §13's component table.
- Gate per task before committing: `npm run lint && npm run typecheck && npm test`. Commit: Conventional Commit (`feat(mobile): …`), trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>`. Once at the end: `cd src-tauri && cargo check` (nothing in Rust changed; it proves it).
- Simulator: "iPhone 17" (address it by its UDID, `xcrun simctl list devices`) with Vite on port 1440 — never "iPhone 17 Pro" or ports 1420/1430 (other sessions). Command (run in the background, it takes minutes):
  `npm run tauri -- ios dev "iPhone 17" --config '{"build":{"devUrl":"http://localhost:1440","beforeDevCommand":"npm run dev -- --port 1440 --strictPort"}}'`
  Screenshots: `xcrun simctl io <UDID> screenshot docs/screenshots/mobile/finance-NN-name.png` (the iPhone 17 simulator's UDID). Test data comes from a temporary dev button (kept in `.superpowers/sdd/2026-10-16-mobile-app-finance/seed.tsx`, pasted into `Finance.tsx` only while seeding, never committed — `git diff --stat` before every commit must not show it).

### API inventory (all existing, `src/lib/tauri-api.ts`)

| Need | Wrapper | Backing command |
|---|---|---|
| Accounts with institution | `bankAccountsApi.getAll()` / `useBankAccounts()` | `get_all_bank_accounts` |
| One account | `useBankAccount(id)` | `get_bank_account` |
| 30-day activity per account | `useBankAccountActivity(accounts)` (`src/hooks/use-bank-account-activity.ts`) | `get_bank_transactions` |
| Uncategorized count (booked, all accounts) | `cashflowApi.getTransactions(0, end, { categoryId: 'uncategorized', reportCurrency })` with the key of `useUpkeep` | `get_report_transactions` |
| Import batches | `bankAccountsApi.getImportBatches(accountId)` key `['import-batches', accountId]` | `get_import_batches` |
| Loans | `useLoans()` → `rows: LoanRow[]` (`outstanding`, `payment`, `currency`, `fixationEnd`, `matured`), `metrics.totalOutstanding`, `metrics.missing` | `get_all_loans` |
| Policies | `useInsurance()` → `rows: InsuranceRow[]` (`premiumNative`, `frequency`, `nextPayment`, `ended`), `metrics.yearlyTotal`, `metrics.missing` | `get_all_insurance_policies` |
| Cash history | `usePortfolioHistory(start)` → `points[].totalSavings` | `get_portfolio_history` |
| Balance records | `useBalanceRecords(accountId)`; write `useRecordBalances()` (the phone sheet) | `get_balance_records`, `record_bank_account_balance` |
| Zones (interest tiers) | `bankAccountsApi.getZones(accountId)` key `['bank-account-zones', accountId]` | `get_account_zones` |
| Complete history (flows strip) | `getCompleteBankHistory(accountId)` (`src/lib/bank-history.ts`) key `['bank-transactions', accountId, 'history']` | `get_bank_transactions` |
| Ledger page | `bankAccountsApi.getTransactions(accountId, { ...filters, limit: 100, offset })` → `{ transactions, total }` | `get_bank_transactions` |
| Effective flow per row | `cashflowApi.getEffectiveFlows(ids)` key `['effective-transaction-flows', ids]` | `get_effective_transaction_flows` |
| Categories | `useCategories()`, `useCategoryName()` (`src/hooks/use-categories.ts`) | `get_transaction_categories` |
| Set category | `bankAccountsApi.updateTransactionCategory(txId, categoryId \| null)` | `update_transaction_category` |
| Set meaning | `bankAccountsApi.updateTransactionFlow(txId, flowKind \| null)` | `update_transaction_flow` |
| Delete transaction | `bankAccountsApi.deleteTransaction(id)` | `delete_bank_transaction` |
| Learning | `categorizationApi.previewRecategorization(txId)`, `.learn(data, applyHistory)`, `.retargetCustomRule(ruleId, categoryId, applyHistory)`, `.restoreTransactionCategories(rows)` | `preview_recategorization`, `learn_categorization`, `retarget_custom_rule`, `restore_transaction_categories` |
| MCP suggestion | `categorizationApi.acceptSuggestion(txId)`, `.declineSuggestion(txId)` | `accept_suggestion`, `decline_suggestion` |
| Refresh after a write | `refreshMoneyFlows(queryClient)` (`src/lib/money-flow-refresh.ts`): invalidates the money keys, records the snapshot, invalidates `['portfolio-history']` | — |

Reusable i18n (do not duplicate): `bank_accounts:` `balanceHero.label` ("Balance as of {{date}}"), `balanceStats.*`, `balanceHistory.*`, `importHistory.*`, `flows.line_*` / `flows.noMoves` / `flows.income` / `flows.expense`, `filters.thisMonth|lastMonth|thisYear|allTime|custom|uncategorizedOnly`, `readiness.flow.income|expense|transfer`, `readiness.flow.auto`, `readiness.pending`, `readiness.flowRefund`, `categorization.rememberForFuture`, `categorization.applyHistory_*`, `categorization.historyChecking`, `categorization.ownAccountNote`, `categorization.notLearnable`, `categorization.learned`, `categorization.learnedApplied_*`, `categorization.undoHistory`, `categorization.historyRestored`, `categorization.ruleRetargeted*`, `categorization.searchCategory`, `categorization.noCategory`, `categoryManagement.newCategory`, `transaction.delete`, `confirmDeleteTransaction.*`, `confirmDelete.title|descriptionNamed`, `empty.*`, `table.excluded`; `loans:` `table.fixedUntil` ("fixace do {{month}}"), `table.fixedRate`, `table.matured`, `table.footer.outstanding` ("outstanding {{amount}}"); `insurance:` `table.footer.yearly` ("{{amount}} ročně"), `table.ended`, `frequencyShort.*`, `types.*`; `common:` `buttons.cancel|close|delete|edit|export|filter|add`, `time.today|yesterday`, `periodsLong.*`, `periods.label`, `labels.date|amount|description`; `mobile:` `tabs.finance`, `upkeep.uncategorizedFrom`, `upkeep.accounts_*`, `recordBalances.*`, `shell.placeholder`.

## Review Focus

1. **An account in a currency without a rate** (a CHF pocket with no ECB quote): the Finance hero leaves it out and names it in the conversion note, its row shows the native balance and "no rate for CHF" as the trailing sub, never 0 and never the native number relabelled — Task 1, `accountTrail` test with `converted: null`.
2. **A day split across two ledger pages** (page 1 ends on 14. 10., page 2 starts on 14. 10.): one day head, rows in one card; after a category change the infinite query refetches every loaded page and the scroll stays — Task 2, `groupByDay` and `nextOffset` tests.
3. **A flows month with no rows** tapped in the strip: the ledger shows the period's empty state and a "Show all" reset, the strip keeps the selection, the period chip reads that month — Task 2, `periodForMonth` test (UTC month boundaries) and `ledgerFilters` with `month` set.
4. **Learning when nothing can be learned** (own-account transfer, a row without counterparty and account number) and when a custom rule governs: no `learn_categorization` call, the switch is off and disabled with the desktop's note; a governing custom rule is retargeted instead — Task 3, `pickOutcome` test.
5. **360 dp with German copy**: the two-button pair wraps to two lines, the flows strip scrolls sideways, the sticky day head and the ledger rows never overflow — Tasks 2 and 3, the `mobile.html` check at 360 recorded in the run notes (no Vitest).

---

## Task 0: Dev loop and test profile (no commit)

**Files:**
- Create (gitignored): `.superpowers/sdd/2026-10-16-mobile-app-finance/run-notes.md`, `.superpowers/sdd/2026-10-16-mobile-app-finance/seed.tsx`

- [ ] **Step 1: Boot the simulator and start the dev loop in the background**

Run: `xcrun simctl boot <UDID>` for the iPhone 17 simulator (ignore "already booted"), then the dev command from Global Constraints in the background. Expected: the Moony first-run wizard on "iPhone 17".

- [ ] **Step 2: Create the test profile through the wizard**

Name "Tereza Nováková", language Czech, main currency CZK, password "the test profile's password" (a throwaway test profile on a simulator; write it to `run-notes.md`). Expected: the empty Přehled.

- [ ] **Step 3: Write the seed and run it once through a temporary button**

`seed.tsx` exports `seedFinance(): Promise<void>` that, through the `tauri-api.ts` wrappers only, creates: institutions via `getInstitutions()` (pick "Fio banka", "Česká spořitelna", "Revolut" by name, creating with `createInstitution` when absent); accounts "Běžný účet" (Fio, CZK, `balance: '86420'`, `balanceAsOf` today), "Spořicí účet" (ČS, CZK, savings, interest 3.5, `balance: '95300'`, `balanceAsOf` today − 44 days), "EUR účet" (Revolut, EUR, `balance: '1240'`, `balanceAsOf` today − 30 days), "Provozní" (Fio, CZK, `excludeFromBalance: true`, `balance: '2000'`); on "Běžný účet" 70 transactions over the last 7 months (`createTransaction`: salaries credit 61200 monthly from "Umbrella a.s.", groceries debits "Lidl Praha 4" / "Rohlik.cz", "Alza.cz a.s." uncategorized, one `status: 'pending'`, two in EUR, the newest three uncategorized) and 6 balance records (`recordBalance`, monthly); one loan (`loansApi.create`: "Hypotéka Lipová", principal 2650000, rate 3.1, payment 12340, start 2022-10-20, `interestRateValidityDate` 2027-10-20); two policies (`insuranceApi.create`: life monthly 1200 Kooperativa; vehicle annually 8400 ČPP). Paste `<button onClick={() => void seedFinance()}>seed</button>` into `Finance.tsx`, tap it once, remove the button and the import. Expected: `git status` shows only the plan file and no seed; Přehled shows the net worth.

## Task 1: Finance page

**Files:**
- Create: `src/mobile/lib/finance-rows.ts`, `src/mobile/lib/finance-rows.test.ts`, `src/mobile/lib/cash-trend.ts`, `src/mobile/lib/cash-trend.test.ts`
- Create: `src/mobile/hooks/use-finance.ts`, `src/mobile/hooks/use-cash-trend.ts`
- Create: `src/mobile/components/Segment.tsx`, `src/mobile/components/TrendCard.tsx`
- Create: `src/mobile/pages/finance/FinancePlaceholder.tsx`
- Modify: `src/mobile/components/Hero.tsx` (optional chart, uses `Segment`), `src/mobile/components/SectionHead.tsx` (`note`), `src/mobile/pages/Finance.tsx` (replace the placeholder), `src/mobile/shell/routes.ts` + `routes.test.ts`, `src/mobile/shell/MobileShell.tsx`
- Modify: `src/i18n/locales/{en,cs,de,es,it,nb,pl,sv,ja,zh}/mobile.json`
- Modify: `docs/design-system/mobile.css`, `docs/design-system/mobile.html`, `docs/design-system/README.md` (§13)

**Interfaces:**
- Produces (used by Tasks 2 and 3): `routes.accountDetail = '/finance/accounts/:id'`, `routes.loanDetail = '/finance/loans/:id'`, `routes.policyDetail = '/finance/policies/:id'`, `routes.uncategorized = '/finance/uncategorized'`; `accountPath(id: string): string` (`/finance/accounts/${id}`), `loanPath(id)`, `policyPath(id)`; `Segment<T extends string>({ value, options: { value: T; label: ReactNode }[], onChange, label, className })` — the 36 pt segmented control of the contract (`.m-seg`), scrolls sideways; `SectionHead({ title, action?, note? })` where `note` is the section's one number (`.m-sec__n`, not tappable); `Hero` keeps its props but `period`, `periods`, `onPeriodChange`, `points`, `chartLabel` become optional: without `points` no segment and no chart are drawn (the compact hero); `TrendCard({ title, note?, points, period, periods, onPeriodChange, formatValue, label, zeroBaseline? })` — section head + card with the segment and a 120 pt `ScrubChart`.
- Consumes: Phase 1 components and hooks listed in the Architecture paragraph.

- [ ] **Step 1: Write the failing tests for `finance-rows.ts`**

```ts
// src/mobile/lib/finance-rows.test.ts
import { describe, expect, it } from 'vitest';
import { accountSub, accountTrail, cashChange, sortAccounts, STALE_DAYS } from './finance-rows';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;

describe('accountSub', () => {
  it('names the bank, the currency and the newest statement', () => {
    expect(accountSub({ institution: { name: 'Fio' }, currency: 'CZK', balanceAsOf: TODAY - 20 * DAY }, TODAY - 6 * DAY))
      .toEqual({ institution: 'Fio', currency: 'CZK', stamp: { kind: 'statement', day: TODAY - 6 * DAY } });
  });
  it('falls back to the balance date and to no institution', () => {
    expect(accountSub({ institution: null, currency: 'CZK', balanceAsOf: TODAY - 44 * DAY }, null))
      .toEqual({ institution: null, currency: 'CZK', stamp: { kind: 'balance', day: TODAY - 44 * DAY } });
    expect(accountSub({ institution: null, currency: '', balanceAsOf: null }, null).stamp).toBeNull();
  });
});

describe('accountTrail', () => {
  const base = { currency: 'CZK', mainCurrency: 'CZK', todayDay: TODAY, converted: 86_420, stamp: null, last30: undefined };
  it('is the 30-day change when the account moved', () => {
    expect(accountTrail({ ...base, last30: { net: 9_870, count: 12 } })).toEqual({ kind: 'change', net: 9_870 });
  });
  it('is the main-currency equivalent for a foreign account without movements', () => {
    expect(accountTrail({ ...base, currency: 'EUR', converted: 30_660 })).toEqual({ kind: 'converted', value: 30_660 });
    expect(accountTrail({ ...base, currency: 'CHF', converted: null })).toEqual({ kind: 'converted', value: null });
  });
  it('says how old a stale statement or balance is', () => {
    expect(accountTrail({ ...base, stamp: { kind: 'statement', day: TODAY - 38 * DAY } })).toEqual({ kind: 'stale', what: 'statement', days: 38 });
    expect(accountTrail({ ...base, stamp: { kind: 'balance', day: TODAY - STALE_DAYS * DAY } })).toEqual({ kind: 'none' });
  });
  it('is nothing for a fresh main-currency account without movements', () => {
    expect(accountTrail({ ...base, stamp: { kind: 'statement', day: TODAY - 3 * DAY } })).toEqual({ kind: 'none' });
  });
});

describe('cashChange', () => {
  it('relates the 30-day net to the balance before it', () => {
    expect(cashChange(214_380, 12_400)).toEqual({ delta: 12_400, ratio: 12_400 / 201_980 });
  });
  it('is null when there is no base', () => {
    expect(cashChange(0, 0)).toBeNull();
    expect(cashChange(500, 500)).toBeNull();
  });
});

describe('sortAccounts', () => {
  const a = { id: 'a', name: 'Běžný', currency: 'CZK', balance: '100' };
  const b = { id: 'b', name: 'Alfa', currency: 'EUR', balance: '10' };
  const convert = (amount: number, currency: string) => (currency === 'EUR' ? amount * 25 : amount);
  const activity = (id: string) => (id === 'a' ? { net: -5, lastDay: TODAY } : { net: 40, lastDay: TODAY - DAY });
  it('sorts by converted balance descending, name ascending, change and last movement descending', () => {
    expect(sortAccounts([a, b], 'balance', { convert, activity }).map((x) => x.id)).toEqual(['b', 'a']);
    expect(sortAccounts([a, b], 'name', { convert, activity }).map((x) => x.id)).toEqual(['b', 'a']);
    expect(sortAccounts([a, b], 'change', { convert, activity }).map((x) => x.id)).toEqual(['b', 'a']);
    expect(sortAccounts([a, b], 'last', { convert, activity }).map((x) => x.id)).toEqual(['a', 'b']);
  });
  it('puts an account without a rate last', () => {
    const noRate = (amount: number, currency: string) => (currency === 'EUR' ? null : amount);
    expect(sortAccounts([b, a], 'balance', { convert: noRate, activity }).map((x) => x.id)).toEqual(['a', 'b']);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run src/mobile/lib/finance-rows.test.ts`. Expected: FAIL, module not found.

- [ ] **Step 3: Implement `src/mobile/lib/finance-rows.ts`**

```ts
export const STALE_DAYS = 30; // same threshold as `upkeep.ts` STALE_BALANCE_DAYS
export interface AccountSub { institution: string | null; currency: string; stamp: { kind: 'statement' | 'balance'; day: number } | null }
export function accountSub(account: { institution: { name: string } | null; currency: string; balanceAsOf: number | null }, lastImport: number | null): AccountSub
export type AccountTrail = { kind: 'change'; net: number } | { kind: 'converted'; value: number | null } | { kind: 'stale'; what: 'statement' | 'balance'; days: number } | { kind: 'none' };
export function accountTrail(args: { currency: string; mainCurrency: string; last30: { net: number; count: number } | undefined; stamp: AccountSub['stamp']; todayDay: number; converted: number | null }): AccountTrail
export function cashChange(balance: number, net30: number): { delta: number; ratio: number } | null
export type AccountSort = 'balance' | 'name' | 'change' | 'last';
export function sortAccounts<T extends { id: string; name: string; currency: string; balance: string }>(accounts: readonly T[], sort: AccountSort, ctx: { convert: (amount: number, currency: string) => number | null; activity: (id: string) => { net: number; lastDay: number | null } | undefined }): T[]
```
`accountSub`: `currency` through `recordCurrency` (`@shared/currencies`); `stamp` is the newest import when there is one, else `balanceAsOf`. `accountTrail` precedence: movements (`count > 0`) → `change`; foreign currency → `converted`; stamp older than `STALE_DAYS` → `stale` with `daysBetween(stamp.day, todayDay)` (`@/utils/milestones`); else `none`. `sortAccounts`: ties keep order; `null` conversions sort after every number (`compareConverted` from `@/utils/fx-totals`); `name` with `localeCompare(…, undefined, { sensitivity: 'base' })`.

- [ ] **Step 4: Write the failing test for `cash-trend.ts`**

```ts
// src/mobile/lib/cash-trend.test.ts
import { describe, expect, it } from 'vitest';
import { cashTrendPoints } from './cash-trend';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const point = (recordedAt: number, totalSavings: string | null) => ({ recordedAt, totalSavings, source: 'live', isStale: false, totalLoansPrincipal: null, totalInvestments: null, totalCrypto: null, totalBonds: null, totalRealEstatePersonal: null, totalRealEstateInvestment: null, totalOtherAssets: null, netWorth: null });

describe('cashTrendPoints', () => {
  it('plots the recorded cash and ends on today\'s total', () => {
    const history = { currency: 'CZK', points: [point(TODAY - 2 * DAY, '100'), point(TODAY - DAY, null), point(TODAY, '150')] };
    expect(cashTrendPoints(history, 'CZK', 214_380, TODAY + 3_600)).toEqual([{ t: TODAY - 2 * DAY, value: 100 }, { t: TODAY, value: 214_380 }]);
  });
  it('appends today when the history ends yesterday', () => {
    const history = { currency: 'CZK', points: [point(TODAY - DAY, '100')] };
    const out = cashTrendPoints(history, 'CZK', 120, TODAY + 3_600);
    expect(out).toHaveLength(2);
    expect(out[1].value).toBe(120);
  });
  it('ignores a history in another currency and a null total', () => {
    expect(cashTrendPoints({ currency: 'EUR', points: [point(TODAY - DAY, '1')] }, 'CZK', 5, TODAY)).toEqual([{ t: TODAY, value: 5 }]);
    expect(cashTrendPoints({ currency: 'CZK', points: [point(TODAY, '1')] }, 'CZK', null, TODAY)).toEqual([]);
  });
});
```

- [ ] **Step 5: Implement `src/mobile/lib/cash-trend.ts`**

`export function cashTrendPoints(history: { currency: string; points: PortfolioHistoryPoint[] } | undefined, mainCurrency: string, todayValue: number | null, nowSec: number): LinePoint[]` — the `'cash'` branch of `PortfolioTrendCard` (`src/components/common/PortfolioTrendCard.tsx` lines 109–122) as a pure function: `totalSavings` through `amountOf` (`@/utils/positions`), days with `null` left out, today's point replaced or appended with `todayValue` (`utcDayFloor` for the day match), today left out when `todayValue` is null.

- [ ] **Step 6: Run both lib tests**

Run: `npx vitest run src/mobile/lib/`. Expected: PASS (the existing mobile lib tests included).

- [ ] **Step 7: Routes**

Add to `routes.ts`: `accountDetail: '/finance/accounts/:id'`, `loanDetail: '/finance/loans/:id'`, `policyDetail: '/finance/policies/:id'`, `uncategorized: '/finance/uncategorized'` and `export const accountPath = (id: string) => \`/finance/accounts/${id}\``, `loanPath`, `policyPath`. Add to `routes.test.ts`: `expect(tabForPath(accountPath('7f3a'))).toBe('finance')`, `expect(tabForPath(routes.uncategorized)).toBe('finance')`. Run `npx vitest run src/mobile/shell/`. Expected: PASS.

- [ ] **Step 8: `Segment`, `SectionHead.note`, compact `Hero`, `TrendCard`**

`Segment.tsx`: the Radix ToggleGroup block of `Hero.tsx` (lines 116–137) moved into `Segment<T extends string>({ value, options, onChange, label, className })` with the same classes (`h-9`, `bg-well-2`, `data-[state=on]:bg-paper … shadow-seg`); `Hero` renders `<Segment>` and skips the segment and the `ScrubChart` when `points` is undefined (also `setScrub` is then never called). `SectionHead`: `note?: ReactNode` rendered as `<span className="flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 num">` when no `action`. `TrendCard.tsx`: `<SectionHead title note>` + a `.m-card` (`rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 pt-3 pb-2 shadow-e1`) holding `<Segment>` (mb-2) and `<ScrubChart height={120} zeroBaseline={period === 'All'} onScrub={…} label>`; while scrubbing the card's note shows the scrubbed value and day instead of `note` (the hero does the same). `npm run typecheck` must pass; `Overview.tsx` is not edited (its `Hero` call still passes every prop).

- [ ] **Step 9: `useFinance` and `useCashTrend`**

`src/mobile/hooks/use-finance.ts`:
```ts
export interface Finance {
  accounts: BankAccountWithInstitution[]; activity: Map<string, AccountActivity>;
  lastImportByAccount: Record<string, number | null>;
  totals: { balance: number; net30: number; excluded: number; missing: string[] };
  uncategorized: { count: number; accountIds: string[] };
  loans: ReturnType<typeof useLoans>; insurance: ReturnType<typeof useInsurance>;
  mainCurrency: string; fxReady: boolean; convert: ConvertFn; isLoading: boolean;
}
export function useFinance(): Finance
```
Composes `useBankAccounts()`, `useBankAccountActivity(accounts)`, `useQueries` over `['import-batches', account.id]` (as `useUpkeep`), the uncategorized query with exactly `useUpkeep`'s key (`['bank-transactions', 'uncategorized-all', currencyCode, uncategorizedEnd]` with `uncategorizedEnd = utcDayFloor(now) + 86400 − 1`), `accountTotals(accounts.map(a => ({ ...a, last30: activity.get(a.id)?.last30 })), convert)` from `@/utils/bank-account-summary` (`net30 = income − expense`), `useLoans()`, `useInsurance()`, `useCurrency()` (`currencyCode`, `fxReady`) and `useMainConvert()`.

`src/mobile/hooks/use-cash-trend.ts`: `useCashTrend(period: ChartPeriod, todayValue: number | null): { points: LinePoint[]; missing: string[] }` — `chartPeriodStart(period, Date.now() / 1000)`, `usePortfolioHistory(start)`, `cashTrendPoints(history, currencyCode, todayValue, Date.now() / 1000)`, `missing = history?.fxQuality.missingCurrencies ?? []`.

- [ ] **Step 10: i18n keys (all ten locales)**

Add to `mobile.json` (English shown; Czech by hand, the eight others translated with `docs/standards/i18n-glossary.md`; plural suffixes per language):
```json
"finance": {
  "accounts_one": "{{count}} account", "accounts_other": "{{count}} accounts",
  "loans_one": "{{count}} loan", "loans_other": "{{count}} loans",
  "policies_one": "{{count}} policy", "policies_other": "{{count}} policies",
  "total": "Total on accounts",
  "change30": "{{amount}} over 30 days",
  "noMovements": "no movements in 30 days",
  "uncategorized": "Uncategorized payments",
  "sections": { "accounts": "Accounts", "loans": "Loans", "insurance": "Insurance", "cashTrend": "Cash trend" },
  "manual": "by hand",
  "statementOn": "statement {{date}}", "balanceOn": "balance {{date}}",
  "trail": {
    "change": "{{amount}} · 30 days",
    "staleStatement_one": "statement {{count}} day old", "staleStatement_other": "statement {{count}} days old",
    "staleBalance_one": "balance {{count}} day old", "staleBalance_other": "balance {{count}} days old",
    "approx": "≈ {{amount}}", "noRate": "no rate for {{currency}}"
  },
  "perMonth": "{{amount}} / mo",
  "nextPayment": "next payment {{date}}",
  "menu": { "sort": "Sort", "sortBalance": "By balance", "sortName": "By name", "sortChange": "By 30-day change", "sortLast": "By last movement", "showMatured": "Show paid-off loans", "showEnded": "Show ended policies", "export": "Export" },
  "add": { "title": "Add", "account": "Account", "loan": "Loan", "policy": "Policy" },
  "cashTrendLabel": "Cash trend chart"
},
"placeholder": {
  "detail": "The detail comes in phase 4.",
  "queue": "The categorize queue comes in phase 2b.",
  "import": "Statement import comes in phase 2b.",
  "form": "The add form comes in phase 2b.",
  "edit": "Editing comes in phase 2b.",
  "delete": "Deleting comes in phase 2b.",
  "export": "Export comes later."
}
```
Czech: "Celkem na účtech", "{{amount}} za 30 dní", "bez pohybů za 30 dní", "Nezařazené platby", "Účty / Úvěry / Pojištění / Vývoj hotovosti", "ručně", "výpis {{date}}", "zůstatek {{date}}", "{{amount}} · 30 dní", "výpis {{count}} den starý / {{count}} dny starý / {{count}} dní starý", "≈ {{amount}}", "bez kurzu pro {{currency}}", "{{amount}} / měs", "příští platba {{date}}", "Řazení / Podle zůstatku / Podle názvu / Podle změny za 30 dní / Podle posledního pohybu / Zobrazit splacené úvěry / Zobrazit ukončené pojistky / Export", "Přidat / Účet / Úvěr / Pojistka", "Graf vývoje hotovosti"; placeholder: "Detail přijde ve fázi 4." etc. Run `npx vitest run src/i18n/`. Expected: PASS.

- [ ] **Step 11: `FinancePlaceholder` page**

`src/mobile/pages/finance/FinancePlaceholder.tsx`: `export default function FinancePlaceholder({ kind }: { kind: 'loan' | 'policy' | 'queue' })` — `useRoute(routes.loanDetail | policyDetail)` for `:id`; title = the loan's `name` / the policy's `policyName` from `useLoans()` / `useInsurance()` (`t('finance.uncategorized')` for the queue), `leading={<NavTextButton label={t('tabs.finance')} onClick={useBack(routes.finance)} chevron />}`, body `<p className="mx-0.5 mt-2 text-m-body text-ink-3">{t(kind === 'queue' ? 'placeholder.queue' : 'placeholder.detail')}</p>`. Register in `MobileShell.tsx`: `<ProtectedRoute path={routes.loanDetail}>` etc. (wouter's `component` prop cannot pass `kind`; use children render: `<ProtectedRoute path={routes.loanDetail} component={LoanPlaceholder} />` where `LoanPlaceholder = () => <FinancePlaceholder kind="loan" />` exported from the same file). Also register `routes.accountDetail` → a one-line `AccountDetail` stub page (`src/mobile/pages/finance/AccountDetail.tsx`: Screen with the account's name and `shell.placeholder`) that Task 2 replaces.

- [ ] **Step 12: The Finance page**

`src/mobile/pages/Finance.tsx` renders, in order (spec §8.4, frame 03):
1. `<Screen title={t('tabs.finance')} large subtitle={[accounts, loans, policies].join(' · ')} trailing={…}>`; subtitle counts through `finance.accounts_*` etc. (a zero count is left out). Trailing: `NavIconButton` `Plus` opening a `DropdownMenu` (items `finance.add.account|loan|policy`, `min-h-11 text-m-body` as `AgendaRow`'s), and `NavIconButton` `Ellipsis` opening the `···` menu: a `DropdownMenuRadioGroup` for the sort (`finance.menu.sort*`), `DropdownMenuCheckboxItem` ×2 (`showMatured`, `showEnded`), separator, `finance.menu.export` → `toast(t('placeholder.export'))`.
2. `<ConversionQualityNote quality={{ missingCurrencies: totals.missing }} currency={mainCurrency} />` when `missing.length > 0`.
3. Compact `<Hero label={t('finance.total')} value={totals.balance} formatValue={money0} change={cashChange(...) ? { ratio, text: t('finance.change30', { amount: signed }) } : undefined} note={t('finance.noMovements')} />` (no `points`).
4. Uncategorized row when `uncategorized.count > 0`: `<EntityRows><EntityRow chip={<RowChip tone="dark"><Tag/></RowChip>} title={t('finance.uncategorized')} sub={…} value={count} href={…} /></EntityRows>` — sub and href: one account → `t('upkeep.uncategorizedFrom', { account, date })` (last import) or the name, href `accountPath(id) + '?category=uncategorized'`; several → `t('upkeep.accounts', { count })`, href `routes.uncategorized`.
5. Accounts: `<SectionHead title={t('finance.sections.accounts')} />` + rows sorted with `sortAccounts`; chip = `initials(institution?.name ?? name)` (`@/mobile/lib/initials`); title `name` with `<Badge variant="outline">{t('bank_accounts:table.excluded')}</Badge>`-like inline badge (`.m-badge` style: `inline-flex h-[18px] rounded-r1 bg-well-2 px-1.5 text-m-sub`) when `excludeFromBalance`; sub = `[institution ?? t('finance.manual'), currency, stamp && t(stamp.kind === 'statement' ? 'finance.statementOn' : 'finance.balanceOn', { date: shortDay(stamp.day) })].filter(Boolean).join(' · ')`; value `fmt.money(balance, currency, { decimals: 0 })`; `valueSub` from `accountTrail`: `change` → `t('finance.trail.change', { amount: fmt.money(net, currency, { signed: true, decimals: 0 }) })`, `converted` → value null ? `t('finance.trail.noRate', { currency })` : `t('finance.trail.approx', { amount: money0(value) })`, `stale` → `t('finance.trail.stale' + What, { count: days })`; `href={accountPath(id)}`. `shortDay` = the `UpkeepCard` helper (day + month, year only when not this year) — move it to `src/mobile/lib/format-day.ts` as `export function shortDayOptions(unixSeconds: number, now = Date.now()): Intl.DateTimeFormatOptions` and use it from both (one-line change in `UpkeepCard.tsx`).
6. Loans (when `loans.rows.length > 0`): `<SectionHead title note={t('loans:table.footer.outstanding', { amount: money0(metrics.totalOutstanding) })} />`; rows = `rows.filter(r => showMatured || !r.matured)` sorted by `converted?.outstanding ?? -1` desc; chip `<RowChip><CreditCard/></RowChip>`; title `loan.name`; sub = `[fmt.percent(Number(loan.interestRate) / 100, 2), fixationEnd ? t('loans:table.fixedUntil', { month: monthYear(fixationEnd) }) : t('loans:table.fixedRate')].join(' · ')` (`monthYear` from `useLoanText()`; the data model has no bank on a loan, so the spec's "banka" is dropped — ruling); value `fmt.money(outstanding, currency, { decimals: 0 })` (no sign); valueSub `matured ? t('loans:table.matured') : t('finance.perMonth', { amount: fmt.money(payment, currency, { decimals: 0 }) })`; `href={loanPath(id)}`.
7. Insurance (when rows exist): `<SectionHead title note={t('insurance:table.footer.yearly', { amount: money0(metrics.yearlyTotal) })} />`; rows = `rows.filter(r => showEnded || !r.ended)` sorted by `nextPayment ?? Infinity` asc; chip icon by `policy.type`: `life: Heart, health: Stethoscope, property: House, vehicle: Car, travel: Plane, liability: Scale, accident: Bandage, other: Shield` (`POLICY_ICONS` in the page); title `policyName`; sub `[provider, frequency && t(\`insurance:frequencyShort.${frequency}\`)].join(' · ')`; value `fmt.money(premiumNative.amount, premiumNative.currency, { decimals: 0 })`; valueSub `ended ? t('insurance:table.ended') : nextPayment ? t('finance.nextPayment', { date: shortDay(nextPayment) }) : undefined`; `href={policyPath(id)}`.
8. `<TrendCard title={t('finance.sections.cashTrend')} note={t(\`overview.change.${period}\`)} points={useCashTrend(period, fxReady ? totals.balance : null).points} period={period} periods={CHART_PERIODS} onPeriodChange={setPeriod} formatValue={money0} label={t('finance.cashTrendLabel')} />` when `points.length > 1`; `period` state default `'30D'`.
9. Empty state when `!isLoading && accounts.length === 0 && loans.rows.length === 0 && insurance.rows.length === 0`: `<EmptyState icon={<Landmark/>} title={t('bank_accounts:empty.title')} description={t('bank_accounts:empty.description')} className="px-5 py-10" action={<Button …>{t('finance.add.account')}</Button>} />` (the button opens the add menu's account action). Skeleton while `isLoading`: the `OverviewSkeleton` shape (hero + three rows), inline in the page.
10. The `+` actions open the desktop dialogs as they are: `BankAccountFormDialog` (`onSubmit` → `useBankAccountMutations().createAccount.mutate({ data, zones })`), `LoanFormDialog` (`onSubmit` → `useLoanMutations().createMutation.mutate(data)`), `InsuranceFormDialog` (`open`/`onOpenChange`). State lifted to the page (`addKind: 'account' | 'loan' | 'policy' | null`).

- [ ] **Step 13: Contract**

`mobile.css`: `.m-card--compact` (the hero without chart: `padding-bottom: 14px`), `.m-trend` (`.m-card` with `.m-seg` margin `0 0 8px` and the 120 px chart `.m-trend__chart { height: 120px }`), `.m-row__badge` (18 px inline badge in a title: `display:inline-flex; height:18px; margin-left:6px; padding:0 6px; border-radius:var(--r-1); background:var(--well-2); color:var(--ink-3); font-size:var(--m-fs-caption); font-weight:600; vertical-align:2px`). `mobile.html`: a "Finance" variant of the root frame (German: "Konten", the compact hero "Gesamt auf Konten 214 380 Kč", the uncategorized dark row, four account rows with the three trail kinds, the loans head "offen 2 418 760 Kč", two policy rows, the trend card) — add a `data-variant="finance"` template rendered at 360/390/430 by the existing script; the check must read "bez vodorovného přetečení" at 360. README §13 table: rows for `Segment`, `TrendCard`, the compact hero and `.m-row__badge`; the `SectionHead` row gains `__n`.

- [ ] **Step 14: Gate, simulator, commit**

Run `npm run lint && npm run typecheck && npm test`. Expected: all PASS. In the simulator: Finance tab shows the four seeded accounts with the three trail kinds, the loan and the two policies, the trend; tap each `+` item: a dialog that is cut off or cannot scroll at 390 × 844 with the keyboard up is replaced by `toast(t('placeholder.form'))` for that kind (record which in `run-notes.md`). Screenshots `finance-01-finance.png`, `finance-02-finance-menu.png`. `git status` shows no seed. Commit:
```bash
git add -A src/mobile src/i18n/locales docs/design-system docs/plans/2026-10-16-mobile-app-finance.md
git commit -m "feat(mobile): Finance tab with accounts, loans, policies and the cash trend" -m "Co-Authored-By: <model> <noreply@anthropic.com>"
```

## Task 2: Account detail

**Files:**
- Create: `src/mobile/lib/ledger.ts`, `src/mobile/lib/ledger.test.ts`, `src/mobile/lib/balance-line.ts`, `src/mobile/lib/balance-line.test.ts`
- Create: `src/mobile/hooks/use-account-detail.ts`, `src/mobile/hooks/use-ledger.ts`, `src/mobile/hooks/use-ledger-mutations.ts`
- Create: `src/mobile/components/FlowStrip.tsx`, `src/mobile/components/SearchField.tsx`, `src/mobile/components/DayHead.tsx`, `src/mobile/components/LedgerRow.tsx`, `src/mobile/components/ButtonPair.tsx`
- Create: `src/mobile/sheets/FilterSheet.tsx`, `src/mobile/sheets/BalanceHistorySheet.tsx`, `src/mobile/sheets/ImportHistorySheet.tsx`, `src/mobile/sheets/PlaceholderSheet.tsx`, `src/mobile/sheets/ConfirmSheet.tsx`
- Modify: `src/mobile/pages/finance/AccountDetail.tsx` (replace the Task 1 stub), `src/mobile/components/ScrubChart.tsx` (`step`), `src/mobile/components/StatRow.tsx` (one stat), `src/mobile/sheets/RecordBalancesSheet.tsx` (`title`/`description` props)
- Modify: ten `mobile.json`, `docs/design-system/mobile.css` + `mobile.html` + `README.md`

**Interfaces:**
- Consumes: Task 1's `routes.accountDetail`, `accountPath`, `Segment`, `SectionHead.note`, compact `Hero`, `shortDayOptions`.
- Produces (used by Task 3): `LedgerState` and `useLedger` (below); `LedgerRow({ tx, category, flow, onOpen, onCategorize, onDelete })`; `useLedgerMutations()` → `{ setCategory: UseMutationResult<void, Error, { id: string; categoryId: string | null }>; setFlowKind: UseMutationResult<void, Error, { id: string; flowKind: FlowKind | null }>; deleteTransaction: UseMutationResult<void, Error, string>; acceptSuggestion: UseMutationResult<void, Error, string>; declineSuggestion: UseMutationResult<void, Error, string> }`; `ConfirmSheet({ open, onOpenChange, title, description, confirmLabel, onConfirm, isPending? })`; `PlaceholderSheet({ open, onOpenChange, title, text })`; `AccountDetail` keeps `openTx: BankTransaction | null` and `pickFor: BankTransaction | null` in state and renders `<PlaceholderSheet>` for both — Task 3 swaps them for `TransactionSheet` and `CategoryPickerSheet`.

- [ ] **Step 1: Failing tests for `ledger.ts`**

```ts
// src/mobile/lib/ledger.test.ts
import { describe, expect, it } from 'vitest';
import { activeFilterCount, dayLabel, groupByDay, ledgerFilters, nextOffset, periodForMonth, type LedgerState } from './ledger';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const state: LedgerState = { kind: 'all', search: '', categoryId: null, preset: 'allTime', from: null, to: null, month: null };

describe('ledgerFilters', () => {
  it('is newest first, by date, unfiltered for the defaults', () => {
    expect(ledgerFilters(state, TODAY)).toEqual({ sortBy: 'date', ascending: false });
  });
  it('maps the kind, the category, the search and this month', () => {
    const f = ledgerFilters({ ...state, kind: 'in', categoryId: 'uncategorized', search: ' lidl ', preset: 'thisMonth' }, TODAY);
    expect(f).toMatchObject({ txType: 'credit', categoryId: 'uncategorized', search: 'lidl', dateFrom: Date.UTC(2026, 9, 1) / 1000, dateTo: Date.UTC(2026, 10, 1) / 1000 - 1 });
  });
  it('prefers a selected flows month over the preset', () => {
    const f = ledgerFilters({ ...state, preset: 'thisMonth', month: 2026 * 12 + 7 }, TODAY);
    expect(f.dateFrom).toBe(Date.UTC(2026, 7, 1) / 1000);
    expect(f.dateTo).toBe(Date.UTC(2026, 8, 1) / 1000 - 1);
  });
  it('reads a custom ISO range', () => {
    const f = ledgerFilters({ ...state, preset: 'custom', from: '2026-01-05', to: '2026-01-07' }, TODAY);
    expect(f.dateFrom).toBe(Date.UTC(2026, 0, 5) / 1000);
    expect(f.dateTo).toBe(Date.UTC(2026, 0, 8) / 1000 - 1);
  });
});

describe('periodForMonth', () => {
  it('spans the UTC month', () => {
    expect(periodForMonth(2026 * 12 + 1)).toEqual({ from: '2026-02-01', to: '2026-02-28' });
  });
});

describe('groupByDay', () => {
  it('keeps the input order and merges rows of one day across pages', () => {
    const rows = [{ id: 'a', bookingDate: TODAY }, { id: 'b', bookingDate: TODAY - DAY }, { id: 'c', bookingDate: TODAY - DAY }];
    expect(groupByDay(rows)).toEqual([{ day: TODAY, rows: [rows[0]] }, { day: TODAY - DAY, rows: [rows[1], rows[2]] }]);
  });
});

describe('dayLabel', () => {
  it('names today and yesterday, dates the rest with the year only when it differs', () => {
    expect(dayLabel(TODAY, TODAY)).toEqual({ key: 'today' });
    expect(dayLabel(TODAY - DAY, TODAY)).toEqual({ key: 'yesterday' });
    expect(dayLabel(TODAY - 2 * DAY, TODAY)).toEqual({ date: TODAY - 2 * DAY, withYear: false });
    expect(dayLabel(Date.UTC(2025, 11, 31) / 1000, TODAY)).toEqual({ date: Date.UTC(2025, 11, 31) / 1000, withYear: true });
  });
});

describe('nextOffset', () => {
  it('is the loaded count while rows remain', () => {
    expect(nextOffset(100, 250)).toBe(100);
    expect(nextOffset(250, 250)).toBeUndefined();
    expect(nextOffset(0, 0)).toBeUndefined();
  });
});

describe('activeFilterCount', () => {
  it('counts a category and a period, not the kind or the search', () => {
    expect(activeFilterCount(state)).toBe(0);
    expect(activeFilterCount({ ...state, kind: 'out', search: 'x', categoryId: 'c1', preset: 'lastMonth' })).toBe(2);
  });
});
```

- [ ] **Step 2: Implement `src/mobile/lib/ledger.ts`**

```ts
export type LedgerKind = 'all' | 'in' | 'out';
export type PeriodPreset = 'thisMonth' | 'lastMonth' | 'thisYear' | 'allTime' | 'custom';
export interface LedgerState { kind: LedgerKind; search: string; categoryId: string | null; preset: PeriodPreset; from: string | null; to: string | null; month: number | null }
export const DEFAULT_LEDGER: LedgerState
export function ledgerFilters(state: LedgerState, nowSec: number): TransactionFilters
export function periodForMonth(month: number): { from: string; to: string }   // monthStart/monthEnd from '@/utils/bank-activity', isoDateFromUtcTimestamp from '@/utils/period'
export function groupByDay<T extends { bookingDate: number }>(rows: readonly T[]): { day: number; rows: T[] }[]   // day = utcDayFloor(bookingDate)
export function dayLabel(day: number, todayDay: number): { key: 'today' | 'yesterday' } | { date: number; withYear: boolean }
export function nextOffset(loaded: number, total: number): number | undefined
export function activeFilterCount(state: LedgerState): number
```
Presets through `getUtcPeriodRange('monthly' | 'yearly', 0 | -1, new Date(nowSec * 1000))` (`@/utils/period`); custom through `utcDayStart` / `utcDayEnd`; `month` wins over `preset`. Run the test. Expected: PASS.

- [ ] **Step 3: Failing test and implementation for `balance-line.ts`**

```ts
// src/mobile/lib/balance-line.test.ts
import { describe, expect, it } from 'vitest';
import { balanceLine } from './balance-line';
const DAY = 86_400;
const TODAY = Date.UTC(2026, 9, 16) / 1000;
const rec = (daysAgo: number, amount: string, id = String(daysAgo)) => ({ id, amount, asOf: TODAY - daysAgo * DAY });

describe('balanceLine', () => {
  it('steps through the records and holds the newest value until today', () => {
    const { points, marks } = balanceLine([rec(40, '100'), rec(10, '150')], TODAY, undefined);
    expect(points).toEqual([{ t: TODAY - 40 * DAY, value: 100 }, { t: TODAY - 10 * DAY, value: 150 }, { t: TODAY, value: 150 }]);
    expect(marks.map((m) => m.id)).toEqual(['40', '10']);
  });
  it('enters the window with the balance held before it', () => {
    const { points, marks } = balanceLine([rec(40, '100'), rec(10, '150')], TODAY, TODAY - 30 * DAY);
    expect(points[0]).toEqual({ t: TODAY - 30 * DAY, value: 100 });
    expect(marks).toHaveLength(1);
  });
  it('is empty without a record and ignores records after today', () => {
    expect(balanceLine([], TODAY, undefined).points).toEqual([]);
    expect(balanceLine([rec(-3, '1')], TODAY, undefined).points).toEqual([]);
  });
  it('does not duplicate today', () => {
    expect(balanceLine([rec(0, '7')], TODAY, undefined).points).toEqual([{ t: TODAY, value: 7 }]);
  });
});
```
`export function balanceLine(records: readonly { id: string; amount: string | number; asOf: number }[], todayDay: number, start: number | undefined): { points: LinePoint[]; marks: { id: string; t: number }[] }`. Run. Expected: PASS.

- [ ] **Step 4: `ScrubChart.step`, `StatRow` with one stat, `RecordBalancesSheet` title**

`ScrubChart`: `step?: boolean` → `Line`/`Area` `type={step ? 'stepAfter' : 'linear'}`; `Hero` gains `step?: boolean` and passes it through; the scrub overlay's y for the crosshair uses the point's own value (unchanged); the mark y uses `valueAt` (`@/utils/chart-scale`), which interpolates — with `step` use the step value instead: add `stepValueAt(points, t)` (latest point with `t <= day`) in `src/mobile/lib/scrub.ts` with a test (`stepValueAt([{t:0,value:1},{t:10,value:5}], 7) === 1`, `… 10 === 5`, before the first point → the first value). `StatRow`: `grid-cols-1` for one stat. `RecordBalancesSheet`: optional `title?: string; description?: string` props defaulting to the current keys.

- [ ] **Step 5: Hooks**

`use-account-detail.ts`:
```ts
export function useAccountDetail(accountId: string | undefined): {
  account: BankAccountWithInstitution | null | undefined; records: BankBalanceRecord[]; zones: SavingsAccountZone[];
  history: BankTransaction[]; historyRows: BankTransaction[] /* booked, in the account currency */; foreignMissing: string[];
  batches: CsvImportBatch[]; lastImport: number | null; currency: string; isLoading: boolean;
}
```
Queries: `useBankAccount`, `useBalanceRecords`, zones (`enabled: !!account?.hasZoneDesignation`), history (`['bank-transactions', accountId, 'history']` → `getCompleteBankHistory`), batches (`['import-batches', accountId]`); `useDatedFx(firstForeignDay)` + `bookedInAccountCurrency(history, currency, convertAt)` exactly as `BankAccountDetail.tsx` lines 318–355.

`use-ledger.ts`:
```ts
export function useLedger(accountId: string | undefined, state: LedgerState): {
  rows: BankTransaction[]; total: number; hasMore: boolean; loadMore: () => void;
  isLoading: boolean; isFetchingMore: boolean; isError: boolean; retry: () => void;
  flowById: Map<string, EffectiveTransactionFlow>;
}
```
`useInfiniteQuery({ queryKey: ['bank-transactions', accountId, 'ledger', filters], initialPageParam: 0, queryFn: ({ pageParam }) => bankAccountsApi.getTransactions(accountId, { ...filters, limit: 100, offset: pageParam }), getNextPageParam: (last, pages) => nextOffset(pages.reduce((n, p) => n + p.transactions.length, 0), last.total), enabled: !!accountId })`; `filters = ledgerFilters(state, nowSec)` with `nowSec` floored to the UTC day so the key holds still; the effective flows query over the loaded ids (`['effective-transaction-flows', ids]`, `enabled: ids.length > 0`). The search is debounced by the page (`useDebouncedValue(search, 300)`), not here.

`use-ledger-mutations.ts`: five `useMutation`s over the wrappers in the API inventory, each `onSuccess: async () => { await refreshMoneyFlows(queryClient); toast(...) }` with toasts `bank_accounts:messages.transactionDeleted`, `categorization.suggestionAccepted`, `mobile:transaction.flowSaved`, none for set-category (the picker toasts), `onError: toast.error(tc('status.error'), { description: translateApiError(error, tc) })`.

- [ ] **Step 6: Components**

- `ButtonPair({ primary: { label, icon, onClick }, secondary: { label, icon, onClick } })` — `.m-btn-pair`: two 48 pt buttons sharing the row, labels 13 px wrapping (`whitespace-normal hyphens-auto`), `Button` variants `default` and `secondary` with `h-12 rounded-[10px]`.
- `SearchField({ value, onChange, placeholder })` — `.m-search`: 44 pt, `rounded-[12px] bg-well-2 pl-10 pr-10 text-m-body`, `Search` icon left (`ink-4`), a 44 pt clear button (`X`) when `value`, `type="search" inputMode="search" enterKeyHint="search" autoCorrect="off"`.
- `DayHead({ children })` — `.m-dayhead`: `sticky top-0 z-[2] -mx-4 bg-canvas px-[18px] pb-1.5 pt-3 text-m-caption font-semibold uppercase tracking-[0.06em] text-ink-3` (the canvas background hides the rows scrolling under it; `-mx-4` reaches the screen edges).
- `FlowStrip({ flows: MonthFlow[], selected: number | null, onSelect: (month: number) => void, formatValue, labels: { income, expense } })` — `.m-flows`: a `overflow-x-auto` strip scrolled to its end on mount (`scrollLeft = scrollWidth`), one 44 pt-wide button per month (`.m-flows__col`, `min-h-[44px]`, 110 pt bar area + the month label 10 / 500 `ink-4` under it: `fmt.month(monthLabelDate(month), { month: 'short' })`, with the 2-digit year on January), two bars side by side (in `var(--s1)`, out `var(--s3)`, 2 px gap, radius 2) scaled to the strip's max; months other than the selected are at 45 % opacity; `aria-pressed` on the selected; `role="group"` with the strip's label.
- `LedgerRow({ tx, category, flow, onOpen, onCategorize, onDelete, categoryName, deleteLabel… })` wraps `EntityRow`: chip = `flow?.effectiveFlowKind === 'transfer' ? <ArrowRightLeft/> : initials(title) || <Banknote/>` with `tone="gain"` for a credit; `title = counterpartyLabel(tx).title ?? transactionDetails(tx)[0] ?? t('bank_accounts:transaction.counterparty')`; sub = `[category ? categoryName(category) : <span class="m-row__badge">{t('ledger.noCategory')}</span>, tx.status === 'pending' && badge(t('bank_accounts:readiness.pending')), flow && rowFlowLabel(flow, isCredit, !!category) === 'refund' && badge(t('bank_accounts:readiness.flowRefund')), detail].filter(Boolean)` joined with ` · ` (a `ReactNode` sub; `EntityRow.sub` already accepts one); value `(isCredit ? '+ ' : '− ') + fmt.money(Math.abs(Number(tx.amount)), tx.currency, { decimals: currencyDecimals })`, `valueTone="gain"` for a credit; `onClick={onOpen}`; `onSwipeLeft={onCategorize}` label `t('ledger.categorize')` icon `Tag`; `onSwipeRight={onOpen}` label `t('ledger.details')` icon `Info`; `onLongPress` opens the `DropdownMenu` (accessory, as `AgendaRow`) with items Podrobnosti / Kategorie / separator / Smazat (`variant="danger"`).

- [ ] **Step 7: Sheets**

- `PlaceholderSheet({ open, onOpenChange, title, text })`: `Sheet` with `footer={<Button className="h-12 flex-1 …" onClick={close}>{tc('buttons.close')}</Button>}` and `<p className="text-m-body text-ink-3">{text}</p>`.
- `ConfirmSheet({ open, onOpenChange, title, description, confirmLabel, onConfirm, isPending })`: footer ghost Zrušit + `<Button variant="destructive" className="h-12 flex-1 rounded-[10px] text-m-row" loading={isPending}>` (the contract's solid destructive button only in confirm sheets).
- `FilterSheet({ open, onOpenChange, state, categories, onApply })`: a draft copy of `state`; section "Kategorie": a `Segment`-less list of 44 pt radio rows (`.m-pick`): Vše (`categoryId: null`), Nezařazené (`'uncategorized'`), then every category (`CategoryIconChip` + `categoryName`) — a `Check` on the chosen; section "Období": rows for the presets (`bank_accounts:filters.thisMonth|lastMonth|thisYear|allTime|custom`), with two `<input type="date">` (`.m-field__i`, 48 pt) for Od / Do under "Vlastní"; footer ghost `account.filter.reset` (back to `DEFAULT_LEDGER` kind/search kept) and primary `account.filter.apply`. Picking a period clears `month`.
- `BalanceHistorySheet({ open, onOpenChange, records, currency })`: rows newest first: title `fmt.day(asOf)`, sub `t(source === 'statement' ? 'bank_accounts:balanceHistory.sourceStatement' : '…sourceManual')`, value `money0(amount)`, valueSub the change against the previous record (signed) — read-only (deleting a record stays on the desktop; ruling). Description `bank_accounts:balanceHistory.sub_*` or `.empty`.
- `ImportHistorySheet({ open, onOpenChange, batches })`: rows: title `fileName`, sub `dateRange ? \`${fmt.day(from)} – ${fmt.day(to)}\` : fmt.dateTime(importedAt)`, value `importedCount`, valueSub `duplicateCount ? t('bank_accounts:importHistory.duplicates') + ' ' + duplicateCount : undefined` — read-only (undo stays on the desktop; ruling).

- [ ] **Step 8: i18n keys (ten locales)**

```json
"account": {
  "record": "Record balance", "import": "Import statement",
  "menu": { "edit": "Edit account", "balanceHistory": "Balance history", "importHistory": "Import history", "delete": "Delete account" },
  "stats": { "changeThisYear": "Change this year", "avgPerMonth": "Average per month", "yearlyInterest": "Yearly interest" },
  "flows": { "title": "Flows" },
  "ledger": { "title": "Movements", "search": "Search by counterparty or description", "all": "All", "in": "Incoming", "out": "Outgoing", "filter": "Filter", "noCategory": "No category", "categorize": "Category", "details": "Details", "count_one": "{{count}} movement", "count_other": "{{count}} movements", "empty": "No movements match", "emptyHint": "Change the period or the filter.", "showAll": "Show all", "loadError": "The movements could not be loaded.", "retry": "Try again" },
  "filter": { "title": "Filter", "category": "Category", "all": "All", "period": "Period", "from": "From", "to": "To", "apply": "Apply", "reset": "Reset" },
  "rowMenu": { "details": "Details", "category": "Category", "delete": "Delete" }
},
"transaction": { "flowSaved": "Meaning saved" }
```
(Czech: "Zaznamenat zůstatek", "Importovat výpis", "Upravit účet", "Historie zůstatku", "Historie importů", "Smazat účet", "Změna letos", "Průměrně za měsíc", "Roční úrok", "Toky", "Pohyby", "Hledat podle protistrany nebo popisu", "Vše / Příchozí / Odchozí", "Filtr", "Bez kategorie", "Kategorie", "Podrobnosti", "{{count}} pohyb / pohyby / pohybů", "Žádné pohyby neodpovídají", "Změňte období nebo filtr.", "Zobrazit vše", "Pohyby se nepodařilo načíst.", "Zkusit znovu", "Filtr", "Kategorie", "Vše", "Období", "Od", "Do", "Použít", "Zrušit filtry", "Podrobnosti / Kategorie / Smazat", "Význam uložen".) The stat labels reuse the desktop's wording but live here because `bank_accounts:balanceStats.*` keys are tied to the desktop stat notes; the notes themselves reuse `bank_accounts:balanceStats.vsRecordAt|noRecordBeforeYear|sinceFirstRecord_*|last12Months|simpleRate|effectiveZoned|noInterest`.

- [ ] **Step 9: The page**

`AccountDetail.tsx` (`useRoute(routes.accountDetail)`, `useSearch()` for `?category=uncategorized` → initial `categoryId`):
1. `<Screen title={account.name} leading={<NavTextButton label={t('tabs.finance')} onClick={back} chevron />} trailing={<NavIconButton Ellipsis> menu>}>`; the subtitle "banka · měna · číslo" is not a `Screen` prop: render it as the first line of the body (`<p className="mx-0.5 -mt-0.5 mb-2.5 text-center text-m-caption text-ink-3 truncate">`) — `[institution?.name, currency, completeAccountIdentifier(iban, bban, null)].filter(Boolean).join(' · ')` (`@/utils/iban-utils`). Menu items: Upravit → `toast(t('placeholder.edit'))`; Historie zůstatku → `BalanceHistorySheet`; Historie importů → `ImportHistorySheet`; separator; Smazat účet (`variant="danger"`) → `ConfirmSheet` (`bank_accounts:confirmDelete.title`, `.descriptionNamed`, confirm `common:buttons.delete`) whose confirm shows `toast(t('placeholder.delete'))` and closes (ruling: deletion itself stays a placeholder in 2a, as the task set).
2. `<ButtonPair primary={{ label: t('account.record'), icon: Pencil, onClick: () => setRecordOpen(true) }} secondary={{ label: t('account.import'), icon: Upload, onClick: () => setImportOpen(true) }} />` → `<RecordBalancesSheet open accountIds={[id]} title={t('account.record')} />` and `<PlaceholderSheet title={t('account.import')} text={t('placeholder.import')} />`.
3. Hero: `balanceLine(records, today, chartPeriodStart(period, now))`; `<Hero label={newest ? t('bank_accounts:balanceHero.label', { date: fmt.day(newest.asOf) }) : t('bank_accounts:balanceHero.unknownDate')} value={balance} formatValue={money0} change={last30.count > 0 && cashChange(balance, last30.net) ? { ratio, text: t('finance.change30', { amount: signed(last30.net) }) } : undefined} note={t('finance.noMovements')} period={period} periods={CHART_PERIODS} points={points} step events={marks.map(m => ({ id: m.id, t: m.t, type: 'mark' }))} renderEventTip={(c) => ({ title: money0(recordById(c.events[0].id).amount), lines: [t('bank_accounts:balanceHero.recordTip', { date, source })] })} chartLabel={t('bank_accounts:chart.balance')} />` — `last30` = `summarizeFlow(historyRows.filter(r => r.bookingDate >= recentWindowStart(now)), currency)`; default period `'1Y'`; the hero is drawn only when `balance !== 0 || records.length > 0` (`detailSections`).
4. Stats (`StatRow`, only those with data; omitted entirely when none): Změna letos (`changeThisYear`, signed, note `vsRecordAt`), Průměrně za měsíc (`averageMonthlyChange`, note `sinceFirstRecord_*` / `last12Months`), Roční úrok when `hasZoneDesignation && zones.length` or `interestRate > 0` (`calculateZonedInterest` or `balance × rate / 100`, note `effectiveZoned` / `simpleRate`).
5. Toky (when `hasRecentRows(history, currentMonth)`): `<SectionHead title={t('account.flows.title')} note={selected !== null ? fmt.month(monthLabelDate(selected), { month: 'long', year: 'numeric' }) : undefined} />`; `.m-card` with `<FlowStrip flows={monthlyFlows(historyRows, currentMonth)} selected={state.month ?? newestMonthWithRows(historyRows)} onSelect={(m) => setState(s => ({ ...s, month: s.month === m ? null : m, preset: 'custom', ...periodForMonth(m) }))} />` and the caption `t('bank_accounts:flows.line', { credits, debits, count })` or `flows.noMoves` for the selected month (`text-m-caption text-ink-3`).
6. Pohyby: `<SectionHead title={t('account.ledger.title')} note={t('account.ledger.count', { count: total })} />`; `<SearchField>`; a toolbar row (`.m-toolbar`: `flex items-center gap-2.5 mb-3`) with `<Segment value={kind} options=[all,in,out] className="flex-1" />` and a 44 pt `NavIconButton`-styled `ListFilter` button with a 6 px dot (`.m-iconbtn__dot`, `bg-dark`) when `activeFilterCount(state) > 0`, opening `FilterSheet`; then `groupByDay(rows)` → for each group `<DayHead>{label}</DayHead><EntityRows>…<LedgerRow/>…</EntityRows>`; a sentinel `<div ref>` observed with `IntersectionObserver` (root `null`, `rootMargin: '200px'`) calling `loadMore()` when `hasMore && !isFetchingMore`; a 44 pt `skeleton` row while fetching more; empty states: `total === 0 && history.length > 0` → `EmptyState bare` with `account.ledger.empty` / `emptyHint` and a ghost button `account.ledger.showAll` resetting the state to `DEFAULT_LEDGER`; no transaction at all → `bank_accounts:empty.noDataTitle` / `noDataDescription`; `isError` → `account.ledger.loadError` + `retry`.
7. `openTx` / `pickFor` state → two `PlaceholderSheet`s (titles `account.ledger.details` / `account.ledger.categorize`, text `shell.placeholder`) until Task 3; delete → `ConfirmSheet` (`bank_accounts:confirmDeleteTransaction.title`, `.description` with name/date/amount as `BankAccountDetail.tsx` lines 648–660 build it, confirm `bank_accounts:transaction.delete`) → `deleteTransaction.mutate(id)`.
8. Not found (`account === null`): `<p>{t('bank_accounts:detail.notFound')}</p>`; loading: the hero skeleton.

- [ ] **Step 10: Contract**

`mobile.css`: `.m-dayhead`, `.m-search` (`__i`, `__ic`, `__clear`), `.m-toolbar`, `.m-iconbtn` (44 px, radius 12, `__dot`), `.m-flows` (`__col`, `__bars`, `__bar--in` (`s1`) / `--out` (`s3`), `__l`, `.is-muted`), `.m-pick` (44 px radio row of the filter sheet: icon chip, label, trailing check), `.m-nav__sub` (the detail's sub-line under the nav bar, 11 / 500 `ink-3`, centred, ellipsis). `mobile.html`: a third template "detail frame" at 360/390/430 (German: back "Finanzen", title "Girokonto", sub "Fio · CZK · 2100123456/2010", the button pair "Kontostand erfassen / Kontoauszug importieren", the hero with a step line, two stats, the flows strip with 12 months — the strip must scroll, the body must not —, search, toolbar with the dotted filter button, two day groups of rows with the badges "Ausstehend" and "Rückerstattung", one row swiped left showing "Kategorie"). The check at 360 reads no overflow. README §13: rows for `ButtonPair`, `SearchField`, `DayHead`, `FlowStrip`, `LedgerRow`, `FilterSheet`, `ConfirmSheet`, `PlaceholderSheet`, the nav sub-line, and the `ScrubChart` row gains `step`.

- [ ] **Step 11: Gate, simulator, commit**

`npm run lint && npm run typecheck && npm test` PASS. Simulator: open "Běžný účet" from Finance; scrub the hero; tap a flows month (ledger filters to it, caption changes); search "lidl"; segment Příchozí; filter sheet → Nezařazené; scroll to the end of a 70-row ledger (one more page loads); swipe a row both ways (placeholders open); long press (menu); delete one transaction (confirm → toast). Screenshots `finance-03-account.png`, `finance-04-account-ledger.png`, `finance-05-filter.png`. Commit `feat(mobile): account detail with balance hero, flows strip and the infinite ledger`.

## Task 3: Transaction sheet and category picker

**Files:**
- Create: `src/mobile/lib/category-pick.ts`, `src/mobile/lib/category-pick.test.ts`
- Create: `src/mobile/hooks/use-categorize.ts`
- Create: `src/mobile/components/KeyValueRow.tsx`, `src/mobile/components/SwitchRow.tsx`
- Create: `src/mobile/sheets/TransactionSheet.tsx`, `src/mobile/sheets/CategoryPickerSheet.tsx`
- Modify: `src/mobile/pages/finance/AccountDetail.tsx` (swap the two placeholders), ten `mobile.json`, `docs/design-system/mobile.css` + `mobile.html` + `README.md`

**Interfaces:**
- Consumes: Task 2's `useLedgerMutations`, `ConfirmSheet`, `Sheet`, `Segment`, `KeyValue`-less `EntityRow`; desktop `CategoryIconChip` (`@/components/common/CategoryIcon`), `CategoryFormDialog` (`@/components/categories/CategoryFormDialog`), `useCategoryName`, `formatBreakdown` (`@/utils/recategorization`), `completeAccountIdentifier`.
- Produces: `TransactionSheet({ tx: BankTransaction | null, open, onOpenChange, account, category, flow, onPickCategory: () => void })`; `CategoryPickerSheet({ tx: BankTransaction | null, open, onOpenChange, categories, recentIds: string[] })`.

- [ ] **Step 1: Failing tests for `category-pick.ts`**

```ts
// src/mobile/lib/category-pick.test.ts
import { describe, expect, it } from 'vitest';
import { filterCategories, learnIdentity, pickOutcome, recentCategoryIds } from './category-pick';

describe('recentCategoryIds', () => {
  it('lists the newest rows\' categories once each, newest first, up to the limit', () => {
    const rows = [{ categoryId: 'a', bookingDate: 30 }, { categoryId: null, bookingDate: 29 }, { categoryId: 'b', bookingDate: 28 }, { categoryId: 'a', bookingDate: 27 }, { categoryId: 'c', bookingDate: 26 }];
    expect(recentCategoryIds(rows, 2)).toEqual(['a', 'b']);
    expect(recentCategoryIds([], 5)).toEqual([]);
  });
});

describe('filterCategories', () => {
  const cats = [{ id: '1', name: 'Potraviny', parentId: null, sortOrder: 2 }, { id: '2', name: 'Doprava', parentId: null, sortOrder: 1 }, { id: '3', name: 'Sub', parentId: '1', sortOrder: 0 }];
  const nameOf = (c: { name: string }) => c.name;
  it('keeps top-level categories in sort order and matches the query without accents or case', () => {
    expect(filterCategories(cats, '', nameOf).map((c) => c.id)).toEqual(['2', '1']);
    expect(filterCategories(cats, 'POTRA', nameOf).map((c) => c.id)).toEqual(['1']);
    expect(filterCategories(cats, 'dopr', nameOf).map((c) => c.id)).toEqual(['2']);
  });
});

describe('learnIdentity', () => {
  it('needs a payee longer than two characters or an account longer than five', () => {
    expect(learnIdentity({ counterpartyName: ' Lidl ', counterpartyIban: null })).toEqual({ payee: 'Lidl', iban: null });
    expect(learnIdentity({ counterpartyName: 'AB', counterpartyIban: 'CZ65' })).toBeNull();
    expect(learnIdentity({ counterpartyName: null, counterpartyIban: 'CZ6508000000192000145399' })).toEqual({ payee: null, iban: 'CZ6508000000192000145399' });
  });
});

describe('pickOutcome', () => {
  const preview = (p: Partial<{ governing: string; learnable: boolean; rule: { id: string } | null }>) => ({ governing: 'none', learnable: true, rule: null, matches: 0, byCategory: [], ...p }) as never;
  it('skips without remembering, on an own-account transfer and when nothing is learnable', () => {
    expect(pickOutcome(preview({}), false, true)).toBe('skip');
    expect(pickOutcome(preview({ governing: 'ownAccount' }), true, true)).toBe('skip');
    expect(pickOutcome(preview({ learnable: false }), true, true)).toBe('skip');
    expect(pickOutcome(preview({}), true, false)).toBe('skip');
  });
  it('retargets a governing custom rule, learns otherwise', () => {
    expect(pickOutcome(preview({ governing: 'custom', rule: { id: 'r' } }), true, true)).toBe('retarget');
    expect(pickOutcome(preview({ governing: 'custom', rule: { id: 'r' }, learnable: false }), true, true)).toBe('retarget');
    expect(pickOutcome(preview({ governing: 'learned' }), true, true)).toBe('learn');
    expect(pickOutcome(undefined, true, true)).toBe('learn');
  });
});
```

- [ ] **Step 2: Implement `src/mobile/lib/category-pick.ts`**

```ts
export function recentCategoryIds(rows: readonly { categoryId: string | null; bookingDate: number }[], max: number): string[]
export function filterCategories<T extends { id: string; name: string; parentId: string | null; sortOrder: number }>(categories: readonly T[], query: string, nameOf: (c: T) => string): T[]   // top-level only, by sortOrder, query folded with .normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase()
export function learnIdentity(tx: { counterpartyName: string | null; counterpartyIban: string | null }): { payee: string | null; iban: string | null } | null   // CategorySelector's thresholds: payee trimmed > 2 chars, iban trimmed > 5
export type PickOutcome = 'skip' | 'retarget' | 'learn';
export function pickOutcome(preview: RecategorizationPreview | undefined, remember: boolean, hasIdentity: boolean): PickOutcome
```
Run. Expected: PASS.

- [ ] **Step 3: `useCategorize`**

```ts
export function useCategorize(tx: BankTransaction | null, categories: TransactionCategory[]): {
  preview: RecategorizationPreview | undefined; previewLoading: boolean;
  pick: (categoryId: string, options: { remember: boolean; applyHistory: boolean; categoryName?: string }) => Promise<void>;
  busy: boolean;
}
```
Ported from `CategorySelector.handleSelect` (`src/components/bank-accounts/CategorySelector.tsx` lines 232–335), same keys and order: `updateTransactionCategory` → `refreshMoneyFlows`; `pickOutcome(info, remember, learnIdentity(tx) !== null)` with `info = preview ?? await queryClient.fetchQuery(previewKey)`; `retarget` → `retargetCustomRule(rule.id, categoryId, applyHistory)` + toast `ruleRetargeted[Applied]`; `learn` → `categorizationApi.learn({ payee, counterpartyIban: iban, categoryId, bankAccountId: tx.bankAccountId, isCredit: tx.type === 'credit', counterpartyBankCode, counterpartyCountry }, applyHistory)` + toast `learned` / `learnedApplied` with description `"${payee ?? iban}" → ${name}`; every toast with changed rows gets `action: { label: t('categorization.undoHistory'), onClick: restoreTransactionCategories(changed) → toast historyRestored → refreshMoneyFlows }` and `duration: 8000`; errors → `toast.error(tc('status.error'), { description: translateApiError(e, tc) })`. Preview query key `['recategorization-preview', tx.id]`, `enabled: open && !!tx` (the caller passes `open` through `tx !== null`).

- [ ] **Step 4: Components**

- `KeyValueRow({ label, value, sub?, onClick?, chevron? })` — `.m-kv`: 44 pt min, label 12 / 500 `ink-3` left, value 15 / 450 `ink` right-aligned (`text-right truncate`), optional sub under the value 11 `ink-4`; with `onClick` it is a `<button>` with `active:bg-well` and a `ChevronRight` 16 `ink-5` (this row has a value, but it is a settings-style row that opens a picker — the contract's chevron exception for "settings, documents" rows; note it in README).
- `SwitchRow({ id, label, sub?, checked, onCheckedChange, disabled? })` — `.m-switchrow`: 44 pt, label 15 / 450 `ink`, sub 12 `ink-4`, `Switch` (`@/components/ui/switch`) right; the whole row is the `<label>`.

- [ ] **Step 5: `TransactionSheet`**

`<Sheet full open title={counterpartyLabel(tx).title ?? t('bank_accounts:transaction.counterparty')} description={fmt.day(tx.bookingDate, { day: 'numeric', month: 'long', year: 'numeric' })} footer={<><Button variant="ghost" className="h-12 text-m-row text-loss" onClick={askDelete}>{t('bank_accounts:transaction.delete')}</Button><Button className="h-12 flex-1 rounded-[10px] text-m-row" onClick={close}>{t('transaction.done')}</Button></>}>`: the amount as `text-m-display num` (`+`/`−`, `text-gain` for a credit); `KeyValueRow`s: Protistrana (name; sub `completeAccountIdentifier(iban, null, bankCode)`), Datum, Kategorie (`onClick={onPickCategory}`, value = category name or `account.ledger.noCategory`, chevron), Význam pohybu (`Segment` of four: `bank_accounts:readiness.flow.auto|income|expense|transfer`, value `tx.flowKind ?? 'auto'`, `onChange → setFlowKind.mutate({ id, flowKind: v === 'auto' ? null : v })`; under it the effective meaning caption `t(\`bank_accounts:readiness.flow.${flow.effectiveFlowKind}\`) · t(\`bank_accounts:readiness.source.${flow.flowSource}\`)` when the segment is on Automaticky), Poznámka (`transactionDetails(tx)` joined by a line break; omitted when empty); a disclosure row "Podrobnosti" (`ChevronDown`/`Up`, `aria-expanded`) folding `KeyValueRow`s: ID banky `transactionId`, Valuta `valueDate`, Soubor `importFileName`, Řádek `importLine`, Zdroj `t(\`transaction.source.${dataSource}\`)`, Stav `status` (`bank_accounts:readiness.pending` or `transaction.booked`), Měna `currency`; rows with `null` are left out. Delete → `ConfirmSheet` → `deleteTransaction.mutate(id, { onSuccess: close })`.

- [ ] **Step 6: `CategoryPickerSheet`**

`<Sheet open title={t('picker.title')} footer={<Button variant="ghost" className="h-12 flex-1 …" onClick={close}>{tc('buttons.cancel')}</Button>}>` (half sheet, `full={false}`): `SearchField` (placeholder `bank_accounts:categorization.searchCategory`, autoFocus off); the MCP pill when `tx.suggestedCategoryId && !tx.categoryId`: `.m-pill` — `Sparkles` + `CategoryIconChip` + name, 36 pt, dashed `line-strong`, tap → `acceptSuggestion.mutate(tx.id)` then close, and a 44 pt `X` → `declineSuggestion.mutate(tx.id)`; `recentIds` (from the page: `recentCategoryIds(ledger.rows, 5)`) under the head `picker.recent` when the query is empty; then `picker.all` with `filterCategories(categories, query, categoryName)` as 44 pt rows (`.m-pick`: `CategoryIconChip category` + name + `Check` on `tx.categoryId`); a row tap → `pick(id, { remember, applyHistory: applyHistory && (preview?.matches ?? 0) > 0 })` then close; "Nová kategorie…" (`bank_accounts:categoryManagement.newCategory`, `Plus`) → `CategoryFormDialog` (`onSaved={(c) => pick(c.id, { remember, applyHistory: false, categoryName: categoryName(c) })}`); under the list two `SwitchRow`s: Zapamatovat (`bank_accounts:categorization.rememberForFuture`, default on; `disabled` with the sub `ownAccountNote` / `notLearnable` when `preview` says so, and the sub `readiness.specificLearningScope` otherwise, as the desktop) and Zařadit i další (`bank_accounts:categorization.applyHistory` with `count: preview.matches`, default on, shown only when `remember && preview.matches > 0`, sub `formatBreakdown(...)`); `categorization.historyChecking` while the preview loads.

- [ ] **Step 7: i18n keys (ten locales)**

```json
"transaction": { "flowSaved": "Meaning saved" /* from Task 2 */, "done": "Done", "counterparty": "Counterparty", "date": "Date", "category": "Category", "flow": "Meaning", "note": "Note", "details": "Details", "bankId": "Bank ID", "valueDate": "Value date", "file": "File", "line": "Line", "sourceLabel": "Source", "source": { "manual": "Entered by hand", "csv_import": "Imported", "api_sync": "Synced" }, "status": "Status", "booked": "Booked", "currency": "Currency" },
"picker": { "title": "Category", "recent": "Recently used", "all": "All categories", "suggested": "Suggested", "decline": "Decline suggestion", "empty": "No category matches" }
```
(Czech: "Hotovo", "Protistrana", "Datum", "Kategorie", "Význam pohybu", "Poznámka", "Podrobnosti", "ID banky", "Valuta", "Soubor", "Řádek", "Zdroj", "Zadáno ručně / Importováno / Synchronizováno", "Stav", "Zaúčtováno", "Měna"; "Kategorie", "Naposledy použité", "Všechny kategorie", "Návrh", "Odmítnout návrh", "Žádná kategorie neodpovídá".)

- [ ] **Step 8: Wire into `AccountDetail`**

Replace the two placeholders: `openTx` → `<TransactionSheet tx={openTx} open={!!openTx} …category flow onPickCategory={() => setPickFor(openTx)} />`; `pickFor` → `<CategoryPickerSheet tx={pickFor} open={!!pickFor} categories recentIds />`. The picker opened from the transaction sheet stacks over it (two Radix dialogs; `useSheetHistory` pushes one entry each, so Android back closes the picker first). Row swipe-left opens the picker directly; swipe-right and tap open the transaction sheet.

- [ ] **Step 9: Contract**

`mobile.css`: `.m-kv` (`__l`, `__v`, `__s`, `--tap`), `.m-switchrow` (`__main`, `__t`, `__s`), `.m-pill` (`__x`), `.m-disclose` (the "Podrobnosti" fold row), `.m-sheet__amount` (the display number in a sheet). `mobile.html`: the sheet frame gains the transaction sheet (German) and a second sheet frame with the picker (search, "Vorschlag" pill, "Zuletzt verwendet", rows, the two switches "Für dieses Konto und diese Richtung merken" / "Auch 3 weitere Zahlungen zuordnen"); checks at 360 clean. README §13: rows `KeyValueRow`, `SwitchRow`, `TransactionSheet`, `CategoryPickerSheet`, the pill, and a sentence on the chevron exception of a picker row.

- [ ] **Step 10: Gate, simulator, commit**

`npm run lint && npm run typecheck && npm test` PASS. Simulator: open a row → the sheet; change Význam to Přesun (toast, row chip changes to the transfer icon); swipe left on the uncategorized "Alza.cz" row → picker; pick "Nákupy" with Zapamatovat on → toast "Naučeno" with Vrátit zpět; a second "Alza" row now shows the count in "Zařadit i dalších N"; tap "Nová kategorie…" → the `CategoryFormDialog` (if it is cut off at 390 × 844, replace with `toast(t('placeholder.form'))` and record it); delete a transaction from the sheet. Screenshots `finance-06-transaction.png`, `finance-07-picker.png`. Commit `feat(mobile): transaction sheet and category picker with learning`.

## Task 4: Final review and hand-over

- [ ] **Step 1:** `cd src-tauri && cargo check` (expected: unchanged, compiles). `git log --oneline 150765c2..HEAD` lists the three feature commits plus any review fixes.
- [ ] **Step 2:** Whole-branch review by a fresh reviewer (opus) against this plan and spec §8.4; fix findings as `fix(mobile): …` commits.
- [ ] **Step 3:** Report to the parent session: branch, commits, what the simulator shows (screenshot list), what is unverified (Android, dark mode, 360 dp only through `mobile.html`), the rulings: loan rows drop "banka" (no bank on a loan); the `+` items open the desktop dialogs or a placeholder per kind as verified; Smazat účet is a confirm sheet ending in a placeholder toast; balance and import history sheets are read-only; the uncategorized row leads to the single account prefiltered or to the queue placeholder; the account detail's hero change line is the 30-day flow net (prototype), not the record delta; chevron exception on picker-style key-value rows. Leave the note in the shared side-session notes folder as `mobile-finance-2a.md`.

## As built / rulings (2026-10-17)

Rulings the controller took while executing this plan (spec §8.4 is the authority; each line says what was decided and what it costs if wrong):

1. The detail's "banka · měna · číslo" sub-line is the first body line under the nav bar — `Screen` has no subtitle slot for pushed screens; it scrolls away.
2. The `+` menu opens the desktop dialogs (`BankAccountFormDialog`, `LoanFormDialog`, `InsuranceFormDialog`) as they are — verified to fit and scroll at 390 × 844; cramped until Phase 2b's phone forms.
3. Loan rows drop the spec's "banka" — a loan has no bank in the data model.
4. "Smazat účet" is a confirm sheet ending in the placeholder toast — deletion is not wired in 2a.
5. The balance-history and import-history sheets are read-only — delete and undo stay on the desktop.
6. The "Nezařazené platby" row opens the single account prefiltered (`?category=uncategorized`), or the queue placeholder when several accounts hold payments.
7. The detail hero's change line is the 30-day flow net (prototype frame 04), not the balance-record delta.
8. A picker-style key-value row (Kategorie in the transaction sheet) carries a chevron although it shows a value — the contract's settings-row exception, written into README §13.
9. "Nová kategorie…" opens the desktop `CategoryFormDialog` as it is — verified to fit over the two stacked sheets.
10. The shared `useSheetHistory` became a stack so Android back closes the topmost sheet only — every `Sheet` uses it; traced in review without defect.
11. The detail's `···` keeps the four items the parent task listed (Upravit, Historie zůstatku, Historie importů, Smazat); the spec's Přidat transakci, Úročení and Automatická kategorizace arrive with their features (add forms and the categorize queue in Phase 2b, interest in Phase 4).
12. The ledger search is an always-visible field, not the spec's pull-down reveal — Phase 1 has no pull-to-reveal gesture; it costs one 44 pt row.
13. The button pair sits under the nav bar as prototype note 8 says, not under the hero as the spec sentence reads.
14. The transaction sheet's "Podrobnosti" shows the import file and line; the file name stands for the spec's "batch".
15. Finance's statement-staleness threshold is 30 days (`STALE_BALANCE_DAYS`) while Údržba's `statement_due` uses 35 — left as is; days 31–35 disagree.

Deviations from the plan text, as built:

- Task 3, `useCategorize().pick` resolves `Promise<boolean>`, not `Promise<void>`: `true` = saved (or already that category), `false` = the write failed (its toast has been shown). The sheet closes only on `true`; the learning branch runs detached after it resolves and never throws, it toasts.
- Task 3, `TransactionSheet` has no `account` prop — nothing used it (the amount is in the row's own currency, the counterparty comes from the row).
- Task 3, both sheets take an optional `onChange(patch)` (`{ categoryId }`, `{ suggestedCategoryId: null }`, `{ flowKind }`) and the page's `patchTx` applies it to its `openTx` and `pickFor` snapshots, since the ledger no longer lists a row that has a category under the uncategorized filter and cannot say what it became.
- Task 2, the ledger's IntersectionObserver root is the sentinel's nearest scrolling ancestor (the screen's `main`), not `null` — with an implicit root `rootMargin` cannot reach past the `main` scroll box, so the 200 px look-ahead would not work.
- Task 2, `useAccountDetail` also returns `historyLoaded`, to tell "no transaction" from "history not loaded yet" (the ledger shows its skeleton until then).
- Task 2, `DayHead` is `z-[3]`, not `z-[2]`: the rows' hairline dividers (`li::before`, `z-[2]`) would paint over a `z-[2]` head.
- Task 2, tapping the selected flows month again clears the period (back to "allTime", no range) instead of leaving the `custom` range of that month filtering the ledger while the strip moved its highlight.
- Task 3, the shared `useSheetHistory` became a stack in `src/mobile/lib/sheet-history.ts` (`createSheetHistory(port)`, unit-tested with a fake history): the plan's one-entry-per-sheet hook closed every open sheet on any back press.
- Task 2, `getNextPageParam` returns `undefined` after an empty page, so a backend that reports more rows than it returns cannot loop on one offset while the sentinel is in view.
- Final review, `useLedger.loadMore` does nothing while any fetch runs (`isFetching`, exposed by the hook and a dependency of the page's observer effect) and fetches with `cancelRefetch: false`: the default would cancel the refresh a write triggered, keep the old pages and append the next one to them, leaving stale rows until the next write (`staleTime` is `Infinity`).
- Final review, the picker's "Naposledy použité" is `recentCategoryIds(detail.history, 5)`, the account's whole history, not the plan's filtered ledger rows — under `?category=uncategorized` every loaded row is uncategorized and the list was always empty.
- Final review, `accountSub`'s stamp is the newer of the newest import and `balanceAsOf` (a tie is the statement) and carries `manual`, true only without an institution and without an import; the Finance row says "ručně" only then.
