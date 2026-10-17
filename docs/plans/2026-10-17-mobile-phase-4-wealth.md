# Mobile Phase 4a — Majetek, Akcie and the position detail — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The phone's Majetek tab (assets only, plus the cash row that opens Finance), the Akcie class page (portfolio hero with trade marks, three stats including this year's TWR, Pozice · Sledované) and the position detail (value with the dashed Vloženo line and trade marks, six stats, Koupit · Prodat, transactions, O společnosti), on the existing Tauri commands, without any Rust change.

**Architecture:** Three pages under `src/mobile/pages/` on the Phase 1 shell (`Screen`, `NavBar`, `EntityRow`, `Hero`, `ScrubChart`, `StatRow`, `Sheet`), fed by the hooks and queries the desktop pages already use (`useNetWorthOverview`, `usePortfolioHistory`, the `['investments']` / `['crypto']` / `['bonds']` / `['real-estate']` / `['other-assets']` queries, `useStockPerformance`, `useWatchedStocks`, `useTransactionFx`) plus thin mobile hooks in `src/mobile/hooks/`. Every pure decision (class rows, allocation input, period change, position sorting and search, the position's value + cost series, transaction filtering) lives in `src/mobile/lib/*.ts` with Vitest tests; components only render. Shared phone pieces the Finance branch already built (`Segment`, `ButtonPair`, `KeyValueRow`, `SearchField`, `ConfirmSheet`, `PlaceholderSheet`, `format-day`, `SectionHead.note`, the compact `Hero`) are taken over byte-identical where they do not touch hide amounts, so the parent's merge of both branches is conflict-free there. Writes (delete a position or a transaction, rename, follow) go through `useMutation` and the mutation contract (AGENTS rule 7); every money string goes through `useMoneyText()` / `<Money>` so the eye on Přehled masks the whole tab.

**Tech Stack:** React 19, TypeScript, wouter, TanStack Query, Radix (Dialog as the sheet, DropdownMenu as the menus, ToggleGroup as the segment), Recharts through `ScrubChart`, Lucide, react-i18next (`mobile` namespace, ten locales), Tailwind on the design tokens.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.8 (Majetek, Akcie, position detail — binding), §4 principles, §6 navigation, §10.3–10.5 components; prototype `docs/specs/2026-10-16-mobile-app-prototype.html` frames 10–12 (notes 15–17). Spec wins over the prototype (Phase 1 ruling R7). Phone component contract: `docs/design-system/mobile.css` + `mobile.html` + `docs/design-system/README.md` §13. Parent plan: `docs/plans/2026-10-16-mobile-app.md` (Phase 4 outline). Out of scope here: Krypto / Dluhopisy / Nemovitosti / Ostatní aktiva class pages and details, the watch detail and Yahoo search, loan and policy details (frames 13–14), broker import on the phone, phone-native trade sheets (the desktop dialogs stand in), any Rust change.

## Global Constraints

Process (from the parent task):

- Worktree `.claude/worktrees/mobile-wealth-7ff6c7`, branch `claude/mobile-wealth-7ff6c7` cut from `feat/mobile` at 2d2c8d7b; `git branch --show-current` before every commit; never push, never merge, never touch `main`, `feat/mobile` or another worktree (the parent mobile session merges this branch into `feat/mobile`).
- One plain shell command per call (no `&&` / `;` chains, no `git -C`, no `HOME=` prefixes, no scripts); stage explicit paths only; never `git stash`.
- Files owned elsewhere are read-only here: `src-tauri/**`, `src/components/**` (desktop; reuse only), `src/hooks/**` (reuse only; a new hook goes under `src/mobile/hooks/`), `src/mobile/pages/{Overview,Finance,Cashflow,Lock,Settings,Agenda}.tsx`, `src/mobile/MobileApp.tsx`. `src/mobile/shell/routes.ts` and `MobileShell.tsx`: add routes only. `src/i18n/locales/*/mobile.json`: add keys under `wealth` only, never rename or delete, in all ten locales, plural forms per `docs/standards/i18n.md` §2 (en/de/nb/sv `_one _other`; es/it `_one _many _other`; cs `_one _few _other`; pl `_one _few _many _other`; ja/zh `_other`). Phase 1 components (`Hero`, `ScrubChart`, `StatRow`, `SectionHead`, `EntityRow`, `UpkeepCard`) may be changed additively only, and exactly as this plan says.
- `invoke()` only in `src/lib/tauri-api.ts`; nothing is expected to be missing (API inventory below). A command that turns out to be missing is a hand-over follow-up, never a Rust change here.
- Gate per task before committing: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check` (run `npm run format` first when it complains). No `cargo` runs. Conventional Commit, trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>`.
- Subagents: implementer `sonnet`, task reviewer `sonnet`, mechanical steps `haiku`, final whole-branch review `opus`; always pass `model`. Ledger `.superpowers/sdd/2026-10-17-mobile-phase-4-wealth/progress.md`.
- Simulator (controller only, never an implementer): "iPhone 17 Pro Max" UDID `CD3961FF-A79E-4DFA-9A44-B591B7E694E6` with Vite on port 1450 — never the booted "iPhone 17 Pro" / "iPhone 17" or ports 1420/1430/1440 (other sessions). Addressed by UDID, never `booted`.

From the spec (copied from the Phase 1 plan's constraints, with §8.8 added):

- Both platforms, one visual language, no platform chrome: own tab bar, sheets, segmented control, Inter bundled; reference width 390 pt, every layout checked at 360 and 430 (§5, D1).
- Four tabs Přehled · Finance · Cashflow · Majetek; "add" lives where the object lives: the Majetek `+` is the typed menu (investici, kryptoměnu, dluhopis, nemovitost, aktivum) and every class page has its own `+` (§6); Android system back closes a sheet or pops a detail, never leaves the app from a sheet.
- Majetek holds assets only (D10): investments, real estate and a last section Hotovost with one row "Na účtech" that opens the Finance tab, so the rows add up to the hero (§8.8). Loans and policies are never here. Classes hidden by the menu preference are not listed; empty classes lead to their empty state with the add button.
- Money, dates and numbers only through `useFormat()` (frontend rule 8): `fmt.money(amount, currency, { signed, decimals, compact })`, `fmt.day(unixSeconds, opts)`, `fmt.percent(ratio, digits, { signed })`, `fmt.time`, `fmt.dateTime`, `fmt.number`. Native amounts in their own currency; conversions through the backend's report-currency fields or `useTransactionFx().convertAt`; a missing rate renders "—", never 0 or 1:1 (ADR 0012).
- **Hide amounts (D6):** every money string on these screens is rendered through `useMoneyText()` or `<Money>` (`src/mobile/hooks/use-money-text.ts`, `src/mobile/components/Money.tsx`) — hero values, change lines, stats and their notes, row values and trailing subs, legend amounts, the company card's prices; the chart's value axis is masked by `ScrubChart` itself. Percentages, counts, dates and quantities stay visible.
- Mobile type scale §10.2 (`text-m-display|title-lg|title|section|stat|row|body|sub|caption`); touch targets ≥ 44 pt; side gutter 16 pt; entity row 60 pt (52 single-line); chevron only on rows without a trailing value (`EntityRow` enforces it, §10.3).
- Charts: scrub replaces hover, callout replaces tooltip, hero chart 160 pt, four axis labels; a tap on a mark opens a callout (28 pt hit area); chart colours from tokens only (`chartTokens()`: `line`, `area`, `cost`, `series`, `other`) (§10.4).
- Tokens only (Tailwind token classes `bg-paper`, `text-ink-3`, `text-gain`, `bg-well-2`, `var(--s1)`…), no raw hex and no `.dark` classes; the phone is light-only until Phase 6 (§10.6).
- New components are added to the contract in the same commit: a class block in `docs/design-system/mobile.css`, a demo in `docs/design-system/mobile.html` (German copy, 360/390/430; the page's check must report no horizontal overflow at 360), a row in README §13's component table.

### API inventory (all existing, `src/lib/tauri-api.ts`)

| Need | Wrapper / hook | Backing command |
|---|---|---|
| Today's class totals, assets, unknown classes, FX quality, 30-day history base | `useNetWorthOverview('30D')` (`src/hooks/use-net-worth-overview.ts`): `display.{assets,savings,investments,bonds,crypto,otherAssets,realEstate}`, `unknown.assets`, `portfolioMetrics.total*` (`null` = unknown), `oldest.{…}` (the class values 30 days ago), `assetsDelta`, `hasHistory`, `fxQuality`, `isCheckingPortfolio`, `isEmptyPortfolio` | `get_portfolio_metrics`, `get_portfolio_history`, `get_dated_facts` |
| Class history for the Akcie hero line | `usePortfolioHistory(start)` → `points[].totalInvestments` | `get_portfolio_history` |
| Stock positions | `investmentsApi.getAll(currencyCode)` key `['investments', currencyCode, fxRevision]`, rows via `toStockRow` (`src/utils/positions.ts`), `totals` | `get_all_investments` |
| One position | `investmentsApi.get(id, currencyCode)` key `['investment', id, currencyCode, fxRevision]` | `get_investment` |
| All stock trades (marks, counts) | `investmentsApi.getAllTransactions()` key `['all-stock-transactions']` | `get_all_stock_transactions` |
| A position's trades | `investmentsApi.getTransactions(id)` key `['investment-transactions', id]` | `get_investment_transactions` |
| A position's value + cost history | `investmentsApi.getHistory(ticker, currencyCode, start, end)` key `['ticker-history', 'stock', ticker, start ?? 'all', currencyCode, fxRevision]`; backfill `investmentsApi.backfillHistory(ticker)` once per mount (as `src/components/stocks/PositionHero.tsx`) | `get_stock_value_history`, `backfill_stock_ticker_history` |
| TWR / MWR this year | `useStockPerformance({ fromTs, toTs, reportCurrency, series: [] })` → `portfolio[last]`, `mwr[last]` | `get_stock_performance` |
| Company data | `investmentsApi.getCompanyInfo(ticker, refresh)` keys `['stock-company-info', ticker, 'stored' | 'fresh']` (as `StockDetail.tsx`) | `get_stock_company_info` |
| Price refresh | `priceApi.refreshStockPrices(true)` + `priceApi.refreshDividends()` then `invalidateAfterPriceRefresh(queryClient)` + `['dividend-summary']` (as `Stocks.tsx`) | `refresh_stock_prices`, `refresh_dividends` |
| Rates at a trade's day | `useTransactionFx(transactions)` → `{ ready, convertAt(amount, from, to, date) }` (`src/hooks/use-fx.ts`) | — |
| Watchlist | `useWatchedStocks()` (`src/hooks/use-stock-monitor.ts`) → `WatchedStockRow[]`; `stockMonitorApi.follow(ticker)` key to invalidate `['stock-monitor']` | `get_watched_stocks`, `follow_stock` |
| Crypto positions (count, tickers) | `cryptoApi.getAll(currencyCode)` key `['crypto', currencyCode, fxRevision]`, `toCryptoRow` | `get_all_crypto` |
| Bonds (count, nearest maturity, yearly coupon) | `useBonds()` (`src/hooks/use-bonds.ts`) → `metrics.liveCount`, `metrics.nearest`, `metrics.projectedYearlyIncome` | `get_all_bonds` |
| Properties and their valuations | the desktop's real-estate list query (read its key in `src/pages/RealEstate.tsx`), `useAllRealEstateValuations()` (`src/hooks/use-real-estate.ts`) | `get_all_real_estate`, `get_all_real_estate_valuations` |
| Other assets (count, names, last update) | key `['other-assets']` → `otherAssetsApi.getAll()` | `get_all_other_assets` |
| Bank accounts (cash row count) | `useBankAccounts()` (`excludeFromBalance`) | `get_all_bank_accounts` |
| Menu preference (hidden classes) | `useAuth().user.menuPreferences` (`investments`, `bonds`, `realEstate`, `crypto?`, `otherAssets?` — the last two default to true) | — |
| Delete a position | `investmentsApi.delete(id)` then invalidate `['investments']`, `['portfolio-metrics']`, `['cashflow-report']`, `['all-stock-transactions']`, `['stock-import-batches']`, `portfolioApi.recordSnapshot()`, `['portfolio-history']` | `delete_investment` |
| Delete / edit a trade | `useInvestmentTransactionMutations()` (`src/hooks/use-investment-mutations.ts`); edit through the desktop `EditTransactionModal` | `delete_investment_transaction`, `update_investment_transaction` |
| Rename | `investmentsApi.updateName(id, name, currencyCode)` then invalidate `['investment', id]`, `['investments']` | `update_investment_name` |
| Add dialogs (desktop, reused as they are) | `AddInvestmentModal`, `AddCryptoModal`, `BondsFormDialog` (+ `useBondMutations().createMutation`), `AddRealEstateModal` (controlled: `open`, `onOpenChange`), `AddOtherAssetModal`; trades `BuyInvestmentModal` / `SellInvestmentModal` (`investment: HoldingData`), `ManualPriceModal`, `ManualDividendModal` | — |

Reusable i18n (do not duplicate): `stocks:` `title` ("Akcie a ETF"), `addInvestment`, `refreshPrices`, `importHistory.title`, `summary.totalValue` ("Hodnota portfolia"), `summary.positions_*`, `summary.realizedNote` ("realizováno {{amount}}"), `summary.notUpdated`, `table.search`, `table.unit` ("ks"), `empty.title|description`, `chart.legend.value|buy|sell`, `chart.events.buy|sell|cluster_*`, `detail.manual` ("ručně"), `detail.currentPrice`, `detail.invested`, `detail.avgShort`, `detail.unrealized`, `detail.vsInvestedPct`, `detail.realized`, `detail.sells_*`, `detail.noSells`, `detail.dividends`, `detail.dividendNote`, `detail.noDividend`, `detail.buy|sell`, `detail.about`, `detail.source`, `detail.deleteTransaction`, `detail.menu.editPrice|editDividend|editName`, `detail.editName.*`, `detail.nameUpdated`, `detail.tabs.transactions`, `detail.ledger.filter.all|buys|sells`, `detail.transactions.empty`, `detail.companyInfo.*`, `detail.confirmDelete.*`, `detail.confirmDeleteTransaction.*`, `detail.legend.*`, `detail.events.*`, `detail.notFound`, `actions.deletePosition`, `toast.deleted|pricesRefreshed|pricesRefreshedDescription|rateLimitReached|rateLimitDescription|refreshFailed`; `dashboard:` `cards.investments|crypto|bonds|realEstate|otherAssets|other` and the savings card label, `allocation.title|center`; `realEstate:` `addProperty`; `bonds:` `addBond`; `common:` `buttons.buy|sell|cancel|close|delete|edit|export|add`, `labels.moreActions`, `periodsLong.*`, `periods.label`, `status.error|success`, `fxQuality.*`; `mobile:` `tabs.wealth|finance`, `overview.change.*`, `overview.stats.assets|noHistory`, `upkeep.accounts_*`, `shell.placeholder`.

## Review Focus

Inputs the spec implies and no task's tests exercise directly; each line's test is pinned to the owning task.

1. **A class none of whose amounts converted** (`portfolioMetrics.totalCrypto === null`, a coin priced only in a currency without a rate): the Majetek hero shows "—" when `unknown.assets`, the class row shows "—" with no change line, the allocation bar leaves the class out and the conversion note names the currency — never 0 and never 1:1 — Task 2, `allocationInput` test with an unknown class and the row's `value: null` branch.
2. **A hidden class with holdings** (`menuPreferences.crypto === false` while crypto is held): no row, but the hero and the bar still count it (the hero is total assets) — Task 2, `visibleClasses` test; the bar takes `display`, not the visible rows.
3. **A position without a rate** (`marketValue === null`): the Akcie row shows "—" and no change badge, sorting by value puts it last, by gain last too; the hero's total leaves it out and the note names it — Task 3, `positionRows` sort test with `null` values.
4. **Hide amounts on** (the eye on Přehled): every money string on the three screens is masked, the chart's value axis too; percentages, counts, dates and quantities stay — Tasks 2–4 use `useMoneyText()` / `<Money>` for every amount (the task reviewer greps the pages for `fmt.money(` and rejects any direct use), and the controller's simulator pass takes a screenshot with the eye on.
5. **360 dp with German copy**: the two rows of three stats, the trailing change line "▲ 12,2 % + 128 300 Kč" (at most 60 % of the row), the legend of the allocation bar and the transaction rows never overflow — Tasks 2–4, the `mobile.html` check at 360 recorded in the run notes (no Vitest).

---

## Task 1: Shared phone pieces (Finance parity), wealth routes and placeholders

**Files:**
- Create (byte-identical from `git show claude/mobile-finance-2e89a2:<path>`): `src/mobile/components/Segment.tsx`, `src/mobile/components/ButtonPair.tsx`, `src/mobile/components/KeyValueRow.tsx`, `src/mobile/components/SearchField.tsx`, `src/mobile/sheets/ConfirmSheet.tsx`, `src/mobile/sheets/PlaceholderSheet.tsx`, `src/mobile/lib/format-day.ts`
- Create: `src/mobile/pages/WealthPlaceholder.tsx`
- Modify: `src/mobile/components/SectionHead.tsx` (`note`, identical to the Finance branch), `src/mobile/components/Hero.tsx` (optional chart, see below), `src/mobile/components/UpkeepCard.tsx` (the one-line `shortDayOptions` change, identical to the Finance branch), `src/mobile/components/ButtonPair.tsx` (after the copy: `disabled?: boolean` on `PairButton`), `src/mobile/shell/routes.ts` + `routes.test.ts`, `src/mobile/shell/MobileShell.tsx`
- Modify: ten `src/i18n/locales/*/mobile.json` (the `wealth.placeholder.*` and `wealth.classes.*` keys only; the rest of `wealth` comes with Tasks 2–4)

**Interfaces:**
- Produces: `routes.stocks = '/wealth/stocks'`, `routes.positionDetail = '/wealth/stocks/:id'`, `routes.cryptoList = '/wealth/crypto'`, `routes.bondsList = '/wealth/bonds'`, `routes.otherAssetsList = '/wealth/other-assets'`, `routes.realEstateList = '/wealth/real-estate'`, `routes.propertyDetail = '/wealth/real-estate/:id'`; `positionPath(id: string)`, `propertyPath(id: string)`. `Segment<T extends string>({ value, options, onChange, label, className })`; `ButtonPair({ primary, secondary })` with `PairButton = { label; icon; onClick; disabled? }`; `KeyValueRow({ label, value, sub?, onClick?, chevron?, stack? })`; `SearchField({ value, onChange, placeholder, clearLabel })`; `ConfirmSheet({ open, onOpenChange, title, description, confirmLabel, onConfirm, isPending? })`; `PlaceholderSheet({ open, onOpenChange, title, text })`; `shortDayOptions(unixSeconds, now?)`; `SectionHead({ title, action?, note? })`; `Hero` with `period`, `periods`, `onPeriodChange`, `points`, `chartLabel` optional — without `points` no segment and no chart (the compact hero, `pb-3.5`), `formatValue` stays `(value: number) => string` and `useFitText(heroRef, shownValue)` stays (the hide-amounts base of this branch; the Finance branch's `ReactNode` variant is not taken). `WealthPlaceholder({ kind: 'crypto' | 'bonds' | 'otherAssets' | 'realEstate' | 'property' })` default export plus named `CryptoPlaceholder`, `BondsPlaceholder`, `OtherAssetsPlaceholder`, `RealEstatePlaceholder`, `PropertyPlaceholder` (zero-prop wrappers for wouter's `component`).

- [ ] **Step 1: Copy the seven files**

Run `git show claude/mobile-finance-2e89a2:src/mobile/components/Segment.tsx > src/mobile/components/Segment.tsx` and the same for the other six paths. Then `git diff --no-index <(git show claude/mobile-finance-2e89a2:PATH) PATH` is not available in this shell: instead run `git show claude/mobile-finance-2e89a2:PATH | diff - PATH` for each and expect no output.

- [ ] **Step 2: `SectionHead.note`, `UpkeepCard` and `Hero`**

Apply `git diff 2d2c8d7b claude/mobile-finance-2e89a2 -- src/mobile/components/SectionHead.tsx src/mobile/components/UpkeepCard.tsx` as they are (`git diff 2d2c8d7b claude/mobile-finance-2e89a2 -- <the two paths> | git apply`). For `Hero.tsx`, apply by hand only these parts of the Finance diff: the `Segment` import and the Radix import removal, the five optional props with the "compact hero" doc, the `charted` constant, the `cn(..., !charted && 'pb-3.5')` class, the `Segment` block replacing the inline ToggleGroup, `{charted && points.length > 1 && (<ScrubChart … label={chartLabel ?? label} />)}`. Do NOT take `formatValue: ReactNode`, `useFitText(heroRef, scrub ? scrub.value : value)` or the `step` prop. `Overview.tsx` keeps compiling unchanged.

- [ ] **Step 3: `ButtonPair.disabled`**

Add `disabled?: boolean` to `PairButton` and pass it to both `Button`s. (The only divergence from the Finance copy; noted for the parent's merge.)

- [ ] **Step 4: Routes and their test**

Add the seven routes and the two path helpers to `routes.ts` (comment: "Pushed on Majetek: the Akcie class page, a position, the class placeholders of the next Phase 4 part"). Add to `routes.test.ts`: `expect(tabForPath(routes.stocks)).toBe('wealth')`, `expect(tabForPath(positionPath('7f3a'))).toBe('wealth')`, `expect(tabForPath(propertyPath('x'))).toBe('wealth')`. Run `npx vitest run src/mobile/shell/`. Expected: PASS.

- [ ] **Step 5: `WealthPlaceholder` and registration**

`WealthPlaceholder`: `Screen` (not large) titled by kind — crypto `dashboard:cards.crypto`, bonds `dashboard:cards.bonds`, otherAssets `dashboard:cards.otherAssets`, realEstate `dashboard:cards.realEstate`, property = the property's `name` from the real-estate list query by `useRoute(routes.propertyDetail)` (`dashboard:cards.realEstate` while loading); `leading={<NavTextButton label={t('tabs.wealth')} onClick={useBack(routes.wealth)} chevron />}`; body `<p className="mx-0.5 mt-2 text-m-body text-ink-3">{t('wealth.placeholder.classPage')}</p>`. Register in `MobileShell.tsx` with `ProtectedRoute` for all seven paths (`routes.stocks` and `routes.positionDetail` point at `Wealth` for now — Tasks 3 and 4 swap them).

- [ ] **Step 6: i18n**

Add to every `mobile.json` under a new top-level `wealth` key: `"classes": { "investments": "Stocks and ETFs" }` is NOT added (reuse `stocks:title`); add `"placeholder": { "classPage": "This class page comes in the next part of phase 4.", "watchDetail": "The watch detail comes in the next part of phase 4.", "later": "Comes in the next part of phase 4." }`. Czech: "Stránka této třídy přijde v další části fáze 4.", "Detail sledované akcie přijde v další části fáze 4.", "Přijde v další části fáze 4." The other eight by the glossary (`docs/standards/i18n-glossary.md`). Run `npx vitest run src/i18n/`. Expected: PASS.

- [ ] **Step 7: Gate and commit**

`npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Commit `feat(mobile): shared phone pieces for Majetek, wealth routes and class placeholders` (stage the files listed above explicitly).

## Task 2: Majetek page

**Files:**
- Create: `src/mobile/lib/change.ts` + `change.test.ts`, `src/mobile/lib/wealth-rows.ts` + `wealth-rows.test.ts`, `src/mobile/hooks/use-wealth.ts`, `src/mobile/components/wealth/AllocationBar.tsx`, `src/mobile/components/wealth/ChangeLine.tsx`, `src/mobile/components/wealth/AddAssetMenu.tsx`
- Modify: `src/mobile/pages/Wealth.tsx` (replace the placeholder), `src/mobile/components/StatRow.tsx` (`tone`), `src/mobile/components/Hero.tsx` (`children`), ten `mobile.json`, `docs/design-system/mobile.css` + `mobile.html` + `README.md` §13

**Interfaces:**
- Produces (used by Tasks 3 and 4): `PeriodChange = { delta: number; ratio: number | null }`, `periodChange(today: number | null, base: number | null | undefined): PeriodChange | null`; `ChangeLine({ change: PeriodChange | null; currency: string; options?: MoneyOptions; trail?: ReactNode; className? })` — the badge (▲ / ▼ + `fmt.percent(|ratio|, 1)`, `gain-soft/gain`, `loss-soft/loss`, neutral `well-2/ink-2` when `ratio` is 0 or null) followed by the signed amount through `<Money>`, or by `trail` when given (a date, "dnes"); nothing when `change` is null; `StatItem.tone?: 'gain' | 'loss'` colours the value; `Hero.children?: ReactNode` rendered after the chart when charted, after the change line in the compact hero (the allocation bar, a chart legend); `AddAssetMenu({ trigger: ReactNode; open?: AddKind | null; onOpenChange?: (kind: AddKind | null) => void })` — the typed `+` menu that owns the five desktop add dialogs (`AddInvestmentModal`, `AddCryptoModal`, `BondsFormDialog` + `useBondMutations().createMutation`, `AddRealEstateModal` controlled, `AddOtherAssetModal`); with `open` / `onOpenChange` the page controls which dialog is open, so a row can open one directly.
- Consumes: Task 1's `SectionHead.note`, compact `Hero`, routes.

- [ ] **Step 1: Failing tests for `change.ts`**

```ts
// src/mobile/lib/change.test.ts
import { describe, expect, it } from 'vitest';
import { periodChange } from './change';

describe('periodChange', () => {
  it('relates today to the base', () => {
    expect(periodChange(6_775_680, 6_683_580)).toEqual({ delta: 92_100, ratio: 92_100 / 6_683_580 });
  });
  it('has no ratio against a zero base and measures against the magnitude of a negative one', () => {
    expect(periodChange(500, 0)).toEqual({ delta: 500, ratio: null });
    expect(periodChange(-50, -100)).toEqual({ delta: 50, ratio: 0.5 });
  });
  it('is null without a base or without today', () => {
    expect(periodChange(500, undefined)).toBeNull();
    expect(periodChange(500, null)).toBeNull();
    expect(periodChange(null, 100)).toBeNull();
  });
});
```

- [ ] **Step 2: Implement `src/mobile/lib/change.ts`**

`export interface PeriodChange { delta: number; ratio: number | null }` and `periodChange` as tested (`ratio = delta / Math.abs(base)`). Run the test. Expected: PASS.

- [ ] **Step 3: Failing tests for `wealth-rows.ts`**

```ts
// src/mobile/lib/wealth-rows.test.ts
import { describe, expect, it } from 'vitest';
import { allocationInput, propertyChange, topTickers, visibleClasses, INVESTMENT_CLASSES } from './wealth-rows';
const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;

describe('visibleClasses', () => {
  it('keeps the spec order and drops hidden classes; the optional flags default to shown', () => {
    expect(INVESTMENT_CLASSES).toEqual(['investments', 'crypto', 'bonds', 'otherAssets']);
    expect(visibleClasses({ investments: true, bonds: true })).toEqual(['investments', 'crypto', 'bonds', 'otherAssets']);
    expect(visibleClasses({ investments: true, bonds: false, crypto: false, otherAssets: true })).toEqual(['investments', 'otherAssets']);
  });
});

describe('topTickers', () => {
  const rows = [
    { ticker: 'CEZ', quantity: 10, value: 168_400 },
    { ticker: 'VWCE', quantity: 426, value: 1_182_600 },
    { ticker: 'OLD', quantity: 0, value: 0 },
    { ticker: 'CHF1', quantity: 3, value: null },
    { ticker: 'CSPX', quantity: 20, value: 612_300 },
    { ticker: 'AAPL', quantity: 5, value: 286_900 },
  ];
  it('names the open positions by value, unknown values last, the rest counted', () => {
    expect(topTickers(rows, 3)).toBe('VWCE, CSPX, AAPL +2');
    expect(topTickers(rows.slice(0, 2), 3)).toBe('VWCE, CEZ');
    expect(topTickers([], 3)).toBe('');
  });
});

describe('propertyChange', () => {
  it('is the last revaluation against the one before it, dated by the last', () => {
    expect(propertyChange([{ valuedAt: T - 400 * DAY, value: '2823000' }, { valuedAt: T - 11 * DAY, value: '2860000' }]))
      .toEqual({ ratio: 37_000 / 2_823_000, day: T - 11 * DAY });
  });
  it('has no ratio with one valuation or a zero base, and is null with none; order does not matter', () => {
    expect(propertyChange([{ valuedAt: T, value: '100' }])).toEqual({ ratio: null, day: T });
    expect(propertyChange([{ valuedAt: T - DAY, value: '0' }, { valuedAt: T, value: '5' }])).toEqual({ ratio: null, day: T });
    expect(propertyChange([{ valuedAt: T, value: '5' }, { valuedAt: T - DAY, value: '4' }])).toEqual({ ratio: 0.25, day: T });
    expect(propertyChange([])).toBeNull();
  });
});

describe('allocationInput', () => {
  const labels = { realEstate: 'Nemovitosti', investments: 'Akcie', bonds: 'Dluhopisy', crypto: 'Krypto', otherAssets: 'Ostatní aktiva', savings: 'Na účtech' };
  it('lists the six classes in the fixed order, leaving out unknown and empty ones', () => {
    const display = { realEstate: 2_860_000, investments: 2_650_000, bonds: 488_000, crypto: 214_500, otherAssets: 0, savings: 214_380 };
    const unknown = { realEstate: false, investments: false, bonds: false, crypto: true, otherAssets: false, savings: false };
    expect(allocationInput(display, unknown, labels).map((s) => s.key)).toEqual(['realEstate', 'investments', 'bonds', 'savings']);
    expect(allocationInput(display, unknown, labels)[0]).toEqual({ key: 'realEstate', label: 'Nemovitosti', value: 2_860_000 });
  });
});
```

- [ ] **Step 4: Implement `src/mobile/lib/wealth-rows.ts`**

```ts
export const INVESTMENT_CLASSES = ['investments', 'crypto', 'bonds', 'otherAssets'] as const;
export type InvestmentClass = (typeof INVESTMENT_CLASSES)[number];
export type WealthClass = InvestmentClass | 'realEstate' | 'savings';
export const ALLOCATION_ORDER: readonly WealthClass[] = ['realEstate', 'investments', 'bonds', 'crypto', 'otherAssets', 'savings'];
export function visibleClasses(prefs: { investments: boolean; bonds: boolean; crypto?: boolean; otherAssets?: boolean }): InvestmentClass[]
export function topTickers(rows: readonly { ticker: string; quantity: number; value: number | null }[], max: number): string  // open rows (quantity > 0), value desc with `compareKnown` (nulls last) then ticker asc; `tickerSummary(tickers, max)` from `@/utils/trade-events`
export function propertyChange(valuations: readonly { valuedAt: number; value: string }[]): { ratio: number | null; day: number } | null  // sorted copy by valuedAt asc; `periodChange(last, prev)?.ratio ?? null`
export function allocationInput(display: Record<WealthClass, number>, unknown: Record<WealthClass, boolean>, labels: Record<WealthClass, string>): AllocationSegment[]
```
Run the test. Expected: PASS.

- [ ] **Step 5: `StatRow.tone`**

`StatItem.tone?: 'gain' | 'loss'`; the value span gets `tone === 'gain' ? 'text-gain' : tone === 'loss' ? 'text-loss' : 'text-ink'` in place of `text-ink`. Nothing else changes (the Finance branch's `value: ReactNode` is not taken).

- [ ] **Step 6: `useWealth`**

`src/mobile/hooks/use-wealth.ts`:
```ts
export interface WealthData {
  overview: ReturnType<typeof useNetWorthOverview>;
  reportCurrency: string;
  /** Per class: today's value (0 while unknown), whether it is unknown (held, none converted), and the 30-day change. */
  classes: Record<WealthClass, { value: number; unknown: boolean; change: PeriodChange | null }>;
  stocks: { count: number; tickers: string; fxMissing: string[] };
  crypto: { count: number; tickers: string };
  bonds: { count: number; nearestMaturity: number | null; yearlyCoupon: number };
  otherAssets: { count: number; names: string; valuedAt: number | null };
  properties: { property: RealEstate; change: { ratio: number | null; day: number } | null }[];
  cash: { accounts: number };
  visible: InvestmentClass[];
  isLoading: boolean;
}
export function useWealth(): WealthData
```
Composes `useNetWorthOverview('30D')`, the five list queries (keys of the inventory), `useAllRealEstateValuations()`, `useBankAccounts()` (count = accounts not `excludeFromBalance`), `useAuth()`. `classes[cls].change = overview.hasHistory ? periodChange(unknown ? null : display[cls], overview.oldest?.[cls]) : null` (for `realEstate` and `savings` as well; the rows of properties use `propertyChange` instead). `otherAssets.names` = the first two names joined by ", " plus `+N` (reuse `tickerSummary(names, 2)`); `otherAssets.valuedAt` = the newest `updatedAt`. `isLoading = overview.isCheckingPortfolio`.

- [ ] **Step 7: `AllocationBar` and `ChangeLine`**

`AllocationBar({ segments, otherLabel, formatPercent, formatValue })`: `allocationSlices(segments, { otherLabel })` (`@/utils/allocation-percent`); a `div role="img"` with `aria-label` = slices as "label percent (value)"; the bar `flex h-2 overflow-hidden rounded-full bg-well-2` with one `span` per slice, `style={{ flexBasis: \`${percent}%\` , background: color }}` (colours `var(--s1..4)` and `var(--s-other)`, the other slice with `shadow-[inset_0_0_0_1px_var(--line)]`); the legend under it: `ul` `mt-2.5 flex flex-wrap gap-x-3 gap-y-1 text-m-sub text-ink-2`, each `li` `inline-flex items-center gap-1.5` with a 10 px swatch, the label `font-semibold text-ink` and the percent `num`. Returns null when no slice. `ChangeLine` as in the Interfaces block (the badge 18 px high, `rounded-r1 px-1.5 text-m-caption font-semibold leading-none`).

- [ ] **Step 8: `AddAssetMenu`**

`AddAssetMenu({ trigger, open, onOpenChange })`: a `DropdownMenu` around `trigger` (items `min-h-11 text-m-body [&_svg]:size-[18px]` as `AgendaRow`'s menu; icons TrendingUp, Bitcoin, FileText, House, Gem; labels `wealth.add.investment|crypto|bond|property|asset`, a `DropdownMenuLabel` `wealth.add.label` on top) and the five dialogs mounted once, driven by `kind: AddKind | null` state (controlled by `open` when given). `BondsFormDialog` gets `onSubmit={(data) => createMutation.mutate(data, { onSuccess: close })}` and `isLoading={createMutation.isPending}`. `export type AddKind = 'investment' | 'crypto' | 'bond' | 'property' | 'asset'`.

- [ ] **Step 9: i18n keys (all ten locales)**

Add under `wealth` (English shown; Czech by hand; the other eight by the glossary and the plural table):
```json
"subtitle": "Investments, property and cash",
"sections": { "investments": "Investments", "realEstate": "Property", "cash": "Cash" },
"cash": { "title": "On accounts" },
"bonds": { "count_one": "{{count}} issue", "count_other": "{{count}} issues", "nearest": "nearest maturity {{month}}", "coupon": "coupon {{amount}} / year" },
"otherAssets": { "count_one": "{{count}} item", "count_other": "{{count}} items", "valuedOn": "valued {{date}}" },
"realEstate": { "personal": "personal", "investment": "investment", "rent": "rent {{amount}} / month", "revaluedOn": "{{date}}" },
"empty": { "title": "Nothing owned yet", "description": "Add your first investment, property or other asset.", "class": "nothing yet" },
"add": { "label": "Add", "investment": "Investment", "crypto": "Cryptocurrency", "bond": "Bond", "property": "Property", "asset": "Asset" },
"noRate": "no rate for {{currency}}"
```
Czech: "Investice, nemovitosti a hotovost"; "Investice / Nemovitosti / Hotovost"; "Na účtech"; "{{count}} emise / {{count}} emise / {{count}} emisí", "nejbližší splatnost {{month}}", "kupón {{amount}} / rok"; "{{count}} položka / {{count}} položky / {{count}} položek", "oceněno {{date}}"; "osobní", "investiční", "nájem {{amount}} / měsíc", "{{date}}"; "Zatím žádný majetek", "Přidejte první investici, nemovitost nebo jiné aktivum.", "zatím nic"; "Přidat", "Investici", "Kryptoměnu", "Dluhopis", "Nemovitost", "Aktivum"; "bez kurzu pro {{currency}}". Run `npx vitest run src/i18n/`. Expected: PASS.

- [ ] **Step 10: The page**

`src/mobile/pages/Wealth.tsx` renders, in order (spec §8.8, frame 10):
1. `<Screen title={t('tabs.wealth')} large subtitle={t('wealth.subtitle')} trailing={<AddAssetMenu trigger={<NavIconButton aria-label={t('wealth.add.label')}><Plus strokeWidth={1.75} aria-hidden /></NavIconButton>} open={addKind} onOpenChange={setAddKind} />}>`.
2. Skeleton while `isLoading` (the `OverviewSkeleton` shape inline: hero + three rows); `EmptyState` when `overview.isEmptyPortfolio` (`icon={<Briefcase />}`, `wealth.empty.title|description`, `className="px-5 py-10"`, action = `AddAssetMenu` around a full-width primary `Button` labelled `wealth.add.label`).
3. `<ConversionQualityNote quality={overview.fxQuality} currency={reportCurrency} className="mx-0.5 mb-3.5" />`.
4. The hero: `assetsChange = overview.hasHistory && !unknown.assets ? periodChange(display.assets, overview.oldest?.assets) : null`; `<Hero label={t('overview.stats.assets')} value={unknown.assets ? null : display.assets} formatValue={(v) => moneyText(v, reportCurrency, { decimals: 0 })} change={assetsChange ? { ratio: assetsChange.ratio ?? 0, text: <><Money value={assetsChange.delta} currency={reportCurrency} options={{ signed: true, decimals: 0 }} /> {t('overview.change.30D')}</> } : undefined} note={t('overview.stats.noHistory')}>` with the `AllocationBar` as its `children` (the compact hero renders them after the change line: the bar, then the legend). Segments = `allocationInput(display, unknownByClass, labels)` where `unknownByClass[cls] = portfolioMetrics?.total<Cls> === null` (`totalSavings`, `totalInvestments`, `totalBonds`, `totalCrypto`, `totalOtherAssets`, `totalRealEstate`), labels `dashboard:cards.realEstate|investments|bonds|crypto|otherAssets|savings` ("Hotovost"); `otherLabel = t('dashboard:cards.other')`; `formatPercent = (r) => fmt.percent(r, 0)`; `formatValue = (v) => moneyText(v, reportCurrency, { decimals: 0 })`.
5. Section **Investice** (`wealth.sections.investments`, `note` = the sum of the visible classes' values through `moneyText`): one `EntityRow` per `visible` class, `href` to its page (`routes.stocks`, `routes.cryptoList`, `routes.bondsList`, `routes.otherAssetsList`), chip `RowChip` with the icon (TrendingUp, Bitcoin, FileText, Gem), title `stocks:title` / `dashboard:cards.crypto|bonds|otherAssets`; **empty class** (count 0 and value 0): sub `wealth.empty.class`, no value (so the chevron shows), `onClick` opens the class's add dialog (`setAddKind('crypto' | 'bond' | 'asset')`) — stocks keep `href={routes.stocks}` (its page has the empty state); **unknown class**: value "—", no `valueSub`; otherwise value `moneyText(value, reportCurrency, { decimals: 0 })`, sub and `valueSub`:
   - stocks: sub `[t('stocks:summary.positions', { count }), tickers].filter(Boolean).join(' · ')`, valueSub `<ChangeLine change={classes.investments.change} currency={reportCurrency} />`;
   - crypto: sub `[t('stocks:summary.positions', { count }), tickers].filter(Boolean).join(' · ')` (the same count wording as stocks), valueSub `<ChangeLine change={classes.crypto.change} currency={reportCurrency} />`;
   - bonds: sub `[t('wealth.bonds.count', { count }), nearestMaturity && t('wealth.bonds.nearest', { month: fmt.month(new Date(nearestMaturity * 1000)) })]`, valueSub `t('wealth.bonds.coupon', { amount: moneyText(yearlyCoupon, reportCurrency, { decimals: 0 }) })`;
   - other assets: sub `[t('wealth.otherAssets.count', { count }), names]`, valueSub `valuedAt && t('wealth.otherAssets.valuedOn', { date: fmt.day(valuedAt, shortDayOptions(valuedAt)) })`.
6. Section **Nemovitosti** (only when `menuPreferences.realEstate` and `properties.length > 0`; `note` = `classes.realEstate` value): one row per property, chip House, title `property.name`, sub `[t(\`wealth.realEstate.${property.type === 'personal' ? 'personal' : 'investment'}\`), rent > 0 && t('wealth.realEstate.rent', { amount: moneyText(rent, rentCurrency, { decimals: 0 }) })]`, value the property's `marketPrice` in `marketPriceCurrency` through `moneyText` (native; the section note is the converted total), valueSub `change && <ChangeLine change={{ delta: 0, ratio: change.ratio }} currency={reportCurrency} trail={fmt.day(change.day, shortDayOptions(change.day))} />` (`trail` replaces the amount: "▲ 1,3 % 5. 10."), `href={propertyPath(property.id)}`.
7. Section **Hotovost** (`note` = `classes.savings` value): one row, chip Landmark, title `wealth.cash.title`, sub `t('upkeep.accounts', { count: cash.accounts })`, value `classes.savings` through `moneyText`, valueSub `<ChangeLine change={classes.savings.change} currency={reportCurrency} />` (badge and amount, as the other rows — the prototype's "za 30 dní" is said once, in the hero), `href={routes.finance}`.

- [ ] **Step 11: Contract**

`mobile.css`: `.m-abar` (the 8 px bar: `display:flex; height:8px; overflow:hidden; border-radius:4px; background:var(--well-2)`), `.m-abar > i` (`display:block; height:100%`), `.m-abar__other` (inset `line` ring), `.m-legend` (`display:flex; flex-wrap:wrap; gap:4px 12px; margin-top:10px; font-size:var(--m-fs-sub)`), `.m-legend__item` (`display:inline-flex; align-items:center; gap:6px`), `.m-legend__swatch` (10 px, radius 3), `.m-change` (`display:inline-flex; align-items:center; gap:4px; white-space:nowrap`), `.m-change__badge` (18 px, `padding:0 6px; border-radius:var(--r-1); font-size:var(--m-fs-caption); font-weight:600; line-height:18px`, `--gain` / `--loss` tints), `.m-stat__v--gain/--loss` already exist. `mobile.html`: a third template `wealth-frame` (German: "Vermögen", the hero "Vermögenswerte 6 775 680 Kč" with the bar and a six-item legend, sections "Anlagen" with four rows (one empty "noch nichts" row with the chevron, one "—" row), "Immobilien" with one row, "Bargeld" with one row) rendered at 360/390/430 by the existing loop into a new `<div class="phones" id="wealth">` section; the check must read no horizontal overflow at 360. README §13: rows for "Allocation bar" (`.m-abar`, `.m-legend`), "Change line" (`.m-change`), and the stat row's `--gain/--loss` value tone; the Hero row mentions `children` (the bar under the change line) and the compact form.

- [ ] **Step 12: Gate and commit**

`npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Commit `feat(mobile): Majetek tab with the assets hero, allocation bar, class rows and the cash row`.

## Task 3: Akcie page

**Files:**
- Create: `src/mobile/lib/class-trend.ts` + `class-trend.test.ts`, `src/mobile/lib/position-rows.ts` + `position-rows.test.ts`, `src/mobile/hooks/use-stocks.ts`, `src/mobile/pages/Stocks.tsx`, `src/mobile/components/wealth/PositionRow.tsx`, `src/mobile/components/wealth/WatchedRow.tsx`, `src/mobile/components/wealth/SortPill.tsx`
- Modify: `src/mobile/shell/MobileShell.tsx` (`routes.stocks` → `Stocks`), ten `mobile.json`, `docs/design-system/mobile.css` + `mobile.html` + `README.md`

**Interfaces:**
- Produces (used by Task 4): `classTrendPoints(history, cls, mainCurrency, todayValue, nowSec): LinePoint[]`, `pointsChange(points): PeriodChange | null`; `priceStamp(fetchedAt, nowSec)`; `foldRows(rows, expanded, fold?)`; `useStockPositions()` → `{ positions: StockPositions | undefined; rows: StockRow[]; reportCurrency: string; isLoading: boolean }`; `useStockTrades()` → `{ transactions: InvestmentTransaction[]; convertAt; ready }`; `useRefreshStockPrices()` → `UseMutationResult` (the desktop's refresh mutation with its toasts); `SortPill<T extends string>({ value, options: { value: T; label: string }[], onChange, label })` — a 34 pt pill (`well-2`, radius 10, label 12 / 600 + chevron-down) opening a `DropdownMenuRadioGroup`; `PositionRow({ row, currency, onOpen })`.
- Consumes: Task 1's `Segment`, `SearchField`, `ButtonPair` (not here), `Hero`; Task 2's `ChangeLine`, `periodChange`, `StatRow.tone`.

- [ ] **Step 1: Failing tests for `class-trend.ts`**

```ts
// src/mobile/lib/class-trend.test.ts
import { describe, expect, it } from 'vitest';
import { classTrendPoints, pointsChange } from './class-trend';
const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;
const point = (recordedAt: number, totalInvestments: string | null) => ({ recordedAt, source: 'live', isStale: false, totalSavings: null, totalLoansPrincipal: null, totalInvestments, totalCrypto: null, totalBonds: null, totalRealEstatePersonal: null, totalRealEstateInvestment: null, totalOtherAssets: null, netWorth: null });

describe('classTrendPoints', () => {
  it('plots the recorded class, skips unknown days and ends on today\'s value', () => {
    const history = { currency: 'CZK', points: [point(T - 2 * DAY, '100'), point(T - DAY, null), point(T, '150')] };
    expect(classTrendPoints(history, 'investments', 'CZK', 2_650_000, T + 3_600)).toEqual([{ t: T - 2 * DAY, value: 100 }, { t: T, value: 2_650_000 }]);
  });
  it('appends today when the history ends yesterday and leaves today out when its value is unknown', () => {
    const history = { currency: 'CZK', points: [point(T - DAY, '100')] };
    expect(classTrendPoints(history, 'investments', 'CZK', 120, T + 3_600)).toHaveLength(2);
    expect(classTrendPoints({ currency: 'CZK', points: [point(T - DAY, '100'), point(T, '1')] }, 'investments', 'CZK', null, T + 3_600)).toEqual([{ t: T - DAY, value: 100 }]);
  });
  it('ignores a history in another currency', () => {
    expect(classTrendPoints({ currency: 'EUR', points: [point(T - DAY, '1')] }, 'investments', 'CZK', 5, T)).toEqual([{ t: T, value: 5 }]);
  });
});

describe('pointsChange', () => {
  it('is the last point against the first', () => {
    expect(pointsChange([{ t: 1, value: 100 }, { t: 2, value: 90 }, { t: 3, value: 125 }])).toEqual({ delta: 25, ratio: 0.25 });
  });
  it('is null with fewer than two points', () => {
    expect(pointsChange([{ t: 1, value: 100 }])).toBeNull();
    expect(pointsChange([])).toBeNull();
  });
});
```

- [ ] **Step 2: Implement `src/mobile/lib/class-trend.ts`**

`export type TrendClass = 'investments' | 'crypto' | 'realEstate' | 'otherAssets' | 'savings'`; `classHistoryValue(point, cls)` = `PortfolioTrendCard`'s `historyValue` (`src/components/common/PortfolioTrendCard.tsx`) as a pure function; `classTrendPoints` = its `points` memo as a pure function (`amountOf`, `utcDayFloor`; a history in another currency gives no days); `pointsChange(points) = points.length < 2 ? null : periodChange(last.value, first.value)`. Run the test. Expected: PASS.

- [ ] **Step 3: Failing tests for `position-rows.ts`**

```ts
// src/mobile/lib/position-rows.test.ts
import { describe, expect, it } from 'vitest';
import { foldRows, positionRows, priceStamp, searchMatches, ROWS_FOLD } from './position-rows';
const T = Date.UTC(2026, 9, 16, 9, 40) / 1000;
const row = (ticker: string, companyName: string, quantity: number, marketValue: number | null, gainLossPercent: number | null) => ({ ticker, companyName, quantity, marketValue, gainLossPercent });
const vwce = row('VWCE', 'Vanguard FTSE All-World', 426, 1_182_600, 12.2);
const cez = row('CEZ', 'ČEZ', 100, 168_400, -2.3);
const chf = row('NESN', 'Nestlé', 3, null, null);
const old = row('OLD', 'Closed Co', 0, 0, null);

describe('positionRows', () => {
  it('hides closed positions unless asked, sorts by value with unknown values last', () => {
    expect(positionRows([cez, chf, old, vwce], { sort: 'value', search: '', showClosed: false }).map((r) => r.ticker)).toEqual(['VWCE', 'CEZ', 'NESN']);
    expect(positionRows([cez, old, vwce], { sort: 'value', search: '', showClosed: true }).map((r) => r.ticker)).toEqual(['VWCE', 'CEZ', 'OLD']);
  });
  it('sorts by return and by name', () => {
    expect(positionRows([cez, chf, vwce], { sort: 'gain', search: '', showClosed: false }).map((r) => r.ticker)).toEqual(['VWCE', 'CEZ', 'NESN']);
    expect(positionRows([vwce, cez, chf], { sort: 'name', search: '', showClosed: false }).map((r) => r.ticker)).toEqual(['CEZ', 'NESN', 'VWCE']);
  });
  it('filters by ticker or name, without case and diacritics', () => {
    expect(positionRows([cez, vwce, chf], { sort: 'value', search: 'nestle', showClosed: false }).map((r) => r.ticker)).toEqual(['NESN']);
    expect(searchMatches(cez, ' cez')).toBe(true);
    expect(searchMatches(cez, 'vanguard')).toBe(false);
    expect(searchMatches(cez, '')).toBe(true);
  });
});

describe('foldRows', () => {
  it('shows the first eight and counts the rest until expanded', () => {
    const rows = Array.from({ length: 12 }, (_, i) => i);
    expect(ROWS_FOLD).toBe(8);
    expect(foldRows(rows, false)).toEqual({ shown: rows.slice(0, 8), hidden: 4 });
    expect(foldRows(rows, true)).toEqual({ shown: rows, hidden: 0 });
    expect(foldRows(rows.slice(0, 8), false)).toEqual({ shown: rows.slice(0, 8), hidden: 0 });
  });
});

describe('priceStamp', () => {
  it('tells today from an older day and from never', () => {
    expect(priceStamp(T - 60, T)).toEqual({ kind: 'today', at: T - 60 });
    expect(priceStamp(T - 3 * 86_400, T)).toEqual({ kind: 'day', at: T - 3 * 86_400 });
    expect(priceStamp(null, T)).toEqual({ kind: 'never' });
  });
});
```

- [ ] **Step 4: Implement `src/mobile/lib/position-rows.ts`**

```ts
export type PositionSort = 'value' | 'gain' | 'name';
export const ROWS_FOLD = 8;
interface Sortable { ticker: string; companyName: string; quantity: number; marketValue: number | null; gainLossPercent: number | null }
export function searchMatches(row: Pick<Sortable, 'ticker' | 'companyName'>, query: string): boolean  // fold(s) = s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase(); trimmed query
export function positionRows<T extends Sortable>(rows: readonly T[], opts: { sort: PositionSort; search: string; showClosed: boolean }): T[]  // value/gain desc through `compareKnown` (`@/utils/positions`) reversed so nulls come last; name through `localeCompare(…, undefined, { sensitivity: 'base' })`; ties keep order
export function foldRows<T>(rows: readonly T[], expanded: boolean, fold = ROWS_FOLD): { shown: T[]; hidden: number }
export function priceStamp(fetchedAt: number | null, nowSec: number): { kind: 'today'; at: number } | { kind: 'day'; at: number } | { kind: 'never' }  // `utcDayFloor`
```
Run the test. Expected: PASS.

- [ ] **Step 5: `useStocks` hooks**

`src/mobile/hooks/use-stocks.ts`: `useStockPositions()` (the `['investments', currencyCode, fxRevision]` query with `staleTime: 60_000`, `refetchOnMount: true`, `rows = items.map(toStockRow)`), `useStockTrades()` (`['all-stock-transactions']`, `useTransactionFx`), `useRefreshStockPrices()` (the `refreshPricesMutation` of `src/pages/Stocks.tsx` with its toasts), `useTwrThisYear(enabled: boolean)` → `{ twr: number | null; mwr: number | null; isLoading }` from `useStockPerformance(enabled ? { fromTs: chartPeriodStart('YTD', now)!, toTs: utcDayFloor(now), reportCurrency: currencyCode, series: [] } : null)` taking the last entries of `portfolio` and `mwr` (null when `dates.length < 2`).

- [ ] **Step 6: i18n keys (all ten locales)**

Add under `wealth.stocks`:
```json
"back": "Stocks", "pricesAt": "prices {{time}}", "pricesOn": "prices {{date}}",
"invested": "invested {{amount}}",
"dividendsYear": "Dividends / year", "yieldNote": "yield {{percent}}",
"twrYtd": "Return this year", "twrNote": "TWR · MWR {{mwr}}", "twrNoHistory": "no history yet",
"positionsTab": "Positions · {{count}}", "watchedTab": "Watched · {{count}}",
"sort": { "label": "Sort", "value": "By value", "gain": "By return", "name": "By name" },
"searchToggle": "Search", "clearSearch": "Clear search",
"showAll_one": "Show all {{count}} positions", "showAll_other": "Show all {{count}} positions",
"menu": { "import": "Import broker statement", "closed": "Closed positions" },
"closedBadge": "closed",
"watchedEmpty": "You are not watching any stocks yet.",
"target": "target {{price}}", "targetReached": "target reached",
"dayChange": "today"
```
Czech: "Akcie", "ceny {{time}}", "ceny {{date}}", "vloženo {{amount}}", "Dividendy / rok", "výnos {{percent}}", "Zhodnocení letos", "TWR · MWR {{mwr}}", "zatím bez historie", "Pozice · {{count}}", "Sledované · {{count}}", "Řazení / Podle hodnoty / Podle výnosu / Podle názvu", "Hledat", "Smazat hledání", "Zobrazit {{count}} pozici / Zobrazit všechny {{count}} pozice / Zobrazit všech {{count}} pozic", "Importovat výpis brokera", "Uzavřené pozice", "uzavřená", "Zatím nesledujete žádnou akcii.", "cíl {{price}}", "cíl dosažen", "dnes". Run `npx vitest run src/i18n/`. Expected: PASS.

- [ ] **Step 7: `PositionRow`, `WatchedRow`, `SortPill`**

`PositionRow`: `EntityRow` with chip `RowChip` holding `row.ticker.slice(0, 2).toUpperCase()`, title `row.ticker` plus an inline badge (`inline-flex h-[18px] items-center rounded-r1 bg-well-2 px-1.5 align-[2px] text-m-caption font-semibold text-ink-3 ml-1.5`) `stocks:detail.manual` when `row.isManualPrice` and `wealth.stocks.closedBadge` when `row.quantity <= 0`, sub `row.companyName`, value `row.marketValue === null ? '—' : moneyText(row.marketValue, currency, { decimals: 0 })`, valueSub `row.marketValue !== null && row.gainLoss !== null ? <ChangeLine change={{ delta: row.gainLoss, ratio: row.gainLossPercent === null ? null : row.gainLossPercent / 100 }} currency={currency} /> : undefined`, `href={positionPath(row.id)}`. `WatchedRow({ row })`: chip initials, title `row.ticker`, sub `row.shortName ?? row.longName ?? ''`, value the native price (`currentPrice` in `currency` through `moneyText` with `priceDecimals`), valueSub `dayChange(current, previousClose)` (`@/utils/stock-monitor`, read its signature) as a `ChangeLine` with `trail={t('wealth.stocks.dayChange')}` plus, when `targetPrice`, " · " and `targetReached(...)` ? `wealth.stocks.targetReached` : `wealth.stocks.target`; `onClick` → `toast(t('wealth.placeholder.watchDetail'))`. `SortPill` as in Interfaces (Radix `DropdownMenuRadioGroup`, items `min-h-11 text-m-body`).

- [ ] **Step 8: The page**

`src/mobile/pages/Stocks.tsx` (spec §8.8 "Akcie", frame 11):
1. `<Screen title={t('stocks:title')} leading={<NavTextButton label={t('tabs.wealth')} onClick={useBack(routes.wealth)} chevron />} trailing={<>···, +</>}>`: `···` = `NavIconButton` Ellipsis opening a `DropdownMenu` with `stocks:refreshPrices` (→ `refresh.mutate()`), `wealth.stocks.menu.import` (→ `toast(t('wealth.placeholder.later'))`), `stocks:importHistory.title` (→ the same toast), a `DropdownMenuCheckboxItem` `wealth.stocks.menu.closed` (`showClosed`), separator, `common:buttons.export` (→ the same toast); `+` opens `AddInvestmentModal`.
2. The sub-line as the first body line (`text-m-caption text-ink-3 text-center truncate mx-0.5 mb-3`): `[t('stocks:summary.positions', { count: open.length }), stamp.kind === 'today' ? t('wealth.stocks.pricesAt', { time: fmt.time(stamp.at) }) : stamp.kind === 'day' ? t('wealth.stocks.pricesOn', { date: fmt.day(stamp.at, shortDayOptions(stamp.at)) }) : t('stocks:summary.notUpdated')].join(' · ')`, where `stamp = priceStamp(latest fetchedAt among rows, now)`.
3. Skeleton while `isLoading`; `EmptyState` (`TrendingUp`, `stocks:empty.title|description`, action: full-width primary `Button` `stocks:addInvestment`) when `rows.length === 0`.
4. `<ConversionQualityNote quality={positions.fxQuality} currency={reportCurrency} />`.
5. Hero: `label={t('stocks:summary.totalValue')}`, `value={totalOf(totals.value)}` (`null` → "—"), `period` state default `'30D'`, `points = classTrendPoints(history, 'investments', reportCurrency, totalOf(totals.value), now)` with `history = usePortfolioHistory(chartPeriodStart(period, now, firstTradeDay(transactions)))`, `change = pointsChange(points)` → `{ ratio: change.ratio ?? 0, text: <><Money delta signed/> {t(\`overview.change.${period}\`)}{cost !== null && <> · {t('wealth.stocks.invested', { amount: moneyText(cost, reportCurrency, { decimals: 0 }) })}</>}</> }`, `note={t('overview.stats.noHistory')}`, `zeroBaseline={period === 'All'}`, `events = tradeDayEvents(transactions, (amount, currency, date) => convertAt(amount, currency, reportCurrency, date)).filter(e => start === undefined || e.t >= start)`, `renderEventTip` = the `trendEventTip` of `src/pages/Stocks.tsx` (`stocks:chart.events.*`), `chartLabel={t('stocks:summary.totalValue')}`. Under the chart, inside the hero (its `children`): `<ChartLegend items={[value line, buy, sell]} />` (`src/components/charts/ChartLegend.tsx`, labels `stocks:chart.legend.*`).
6. `<StatRow stats={[unrealized, dividends, twr]} />`: unrealized — label `stocks:detail.unrealized`, value `statMoney(unrealizedGain, signed)` (compact from a million as `Overview.tsx`), `tone` by sign, note `[fmt.percent(totals.unrealizedGainPercent / 100, 1, { signed: true }), realized !== null && t('stocks:summary.realizedNote', { amount: moneyText(realized, reportCurrency, { signed: true, decimals: 0 }) })].filter(Boolean).join(' · ')`; dividends — label `wealth.stocks.dividendsYear`, value `yearlyDividend ? statMoney(yearlyDividend) : '—'`, note `yearlyDividend && value > 0 ? t('wealth.stocks.yieldNote', { percent: fmt.percent(yearlyDividend / value, 1) }) : undefined`; TWR — label `wealth.stocks.twrYtd`, value `twr === null ? '—' : fmt.percent(twr / 100, 1, { signed: true })`, `tone` by sign, note `twr === null ? t('wealth.stocks.twrNoHistory') : t('wealth.stocks.twrNote', { mwr: fmt.percent((mwr ?? 0) / 100, 1, { signed: true }) })`.
7. `<Segment value={tab} options={[{ value: 'positions', label: t('wealth.stocks.positionsTab', { count: open.length }) }, { value: 'watched', label: t('wealth.stocks.watchedTab', { count: watched.length }) }]} onChange={setTab} label={…} className="mb-2" />`.
8. Positions tab: a toolbar row (`flex items-center justify-between gap-2 mb-2`): `<SortPill value={sort} options={[value, gain, name]} onChange={setSort} label={t('wealth.stocks.sort.label')} />` and a 44 pt `NavIconButton`-like button (Search icon, `aria-pressed`) toggling `searchOpen`; `searchOpen && <SearchField value={search} onChange={setSearch} placeholder={t('stocks:table.search')} clearLabel={t('wealth.stocks.clearSearch')} />`; `rows = positionRows(stockRows, { sort, search, showClosed })`, `{ shown, hidden } = foldRows(rows, expanded)`; `<EntityRows className={refresh.isPending ? 'opacity-50 transition-opacity' : undefined}>{shown.map(PositionRow)}{hidden > 0 && <li><button className="flex min-h-[52px] w-full items-center justify-center text-m-row text-ink-2 active:bg-well" onClick={() => setExpanded(true)}>{t('wealth.stocks.showAll', { count: rows.length })}</button></li>}</EntityRows>`; a search with no match shows `t('stocks:table.noResults.title')` ("Nic nenalezeno") as a centred `text-m-body text-ink-3` paragraph instead of the list.
9. Watched tab: `EntityRows` of `WatchedRow`, or the `wealth.stocks.watchedEmpty` paragraph.

- [ ] **Step 9: Register and contract**

`MobileShell.tsx`: `routes.stocks` → `Stocks` (lazy import not needed). `mobile.css`: `.m-sortpill` (34 px, `well-2`, radius 10, padding `0 10px`, 12 / 600 `ink-2`, chevron 14 px; a 44 px hit area through `::after`), `.m-toolbar` (`display:flex; align-items:center; justify-content:space-between; gap:8px; margin-bottom:8px`), `.m-nav-sub` (the sub-line under the nav bar: 11 / 500 `ink-3`, centred, one line with an ellipsis, `margin: 0 2px 12px`), `.m-rows__more` (the "Zobrazit všech N" row: 52 px, centred 15 / 600 `ink-2`). `mobile.html`: add a `stocks-frame` template (back "Vermögen", title "Aktien und ETFs", the sub-line, the hero with legend, three stats including "Rendite dieses Jahr + 7,8 %", the segment "Positionen · 12 / Beobachtet · 6", the toolbar, six rows with the change line and one "manuell" badge, the "Alle 12 Positionen anzeigen" row) rendered at the three widths into a new `#stocks` section by the same loop; the check must read no horizontal overflow at 360. README §13 rows: "Sort pill", "Toolbar" and "Nav sub-line" (the Finance branch adds rows of the same names for its account detail — word these so that one merged row covers both pages), "More row".

- [ ] **Step 10: Gate and commit**

`npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Commit `feat(mobile): Akcie page with the portfolio hero, TWR stat, positions and the watched segment`.

## Task 4: Position detail

**Files:**
- Create: `src/mobile/lib/position-series.ts` + `position-series.test.ts`, `src/mobile/hooks/use-position.ts`, `src/mobile/pages/PositionDetail.tsx`, `src/mobile/components/wealth/TradeRow.tsx`, `src/mobile/components/wealth/CompanyCard.tsx`, `src/mobile/sheets/PositionRenameSheet.tsx`
- Modify: `src/mobile/components/ScrubChart.tsx` (cost line, mark tap — additive), `src/mobile/components/Hero.tsx` (forwards `cost`, `onMarkTap`), `src/mobile/components/EntityRow.tsx` (`id?: string` on the `li`, `className?` for the hot flash — additive), `src/mobile/hooks/use-stocks.ts` (`useTwrThisYear` → `ytdRange`), `src/mobile/shell/MobileShell.tsx` (`routes.positionDetail` → `PositionDetail`), ten `mobile.json`, `docs/design-system/mobile.css` + `mobile.html` + `README.md`

**Interfaces:**
- Consumes: Task 1's `ButtonPair`, `KeyValueRow`, `ConfirmSheet`, `Segment`; Task 2's `ChangeLine`, `StatRow.tone`; Task 3's `pointsChange`, `foldRows`, `priceStamp`.
- Produces: `ScrubChart` props `points: CostPoint[]` (`LinePoint & { cost?: number }`), `cost?: boolean` (draws the dashed `stepAfter` cost line in `tk.cost`, 1.5 px, `strokeDasharray="4 4"`, and widens the value domain to the cost values), `onMarkTap?: (cluster: EventCluster<E>) => void` (called when a mark's callout opens); `Hero` forwards `cost` and `onMarkTap`.

- [ ] **Step 1: Failing tests for `position-series.ts`**

```ts
// src/mobile/lib/position-series.test.ts
import { describe, expect, it } from 'vitest';
import { filterTransactions, nativeTotal, positionEvents, positionPoints, ytdRange } from './position-series';
const DAY = 86_400;
const T = Date.UTC(2026, 9, 16) / 1000;
const h = (recordedAt: number, value: string | null, costBasis: string | null) => ({ ticker: 'VWCE', recordedAt, quantity: '1', price: '1', currency: 'EUR', value, costBasis, isStale: false });
const tx = (id: string, type: string, date: number, quantity = '10', pricePerUnit = '110.4') => ({ id, investmentId: 'i', type, ticker: 'VWCE', companyName: 'V', quantity, pricePerUnit, currency: 'EUR', transactionDate: date, createdAt: date });

describe('positionPoints', () => {
  it('carries value and cost per day and ends on today\'s figures', () => {
    const report = { currency: 'CZK', fxQuality: { missingCurrencies: [] }, points: [h(T - 2 * DAY, '1000', '900'), h(T - DAY, null, '900'), h(T, '1100', '950')] };
    expect(positionPoints(report, 'CZK', 1_182_600, 1_054_300, T + 3_600)).toEqual([{ t: T - 2 * DAY, value: 1000, cost: 900 }, { t: T, value: 1_182_600, cost: 1_054_300 }]);
  });
  it('appends today, leaves it out without a value, and ignores a report in another currency', () => {
    const report = { currency: 'CZK', fxQuality: { missingCurrencies: [] }, points: [h(T - DAY, '1000', null)] };
    expect(positionPoints(report, 'CZK', 1200, null, T + 3_600)).toEqual([{ t: T - DAY, value: 1000, cost: undefined }, { t: T + 3_600, value: 1200, cost: undefined }]);
    expect(positionPoints(report, 'CZK', null, null, T + 3_600)).toEqual([{ t: T - DAY, value: 1000, cost: undefined }]);
    expect(positionPoints({ ...report, currency: 'EUR' }, 'CZK', 5, null, T)).toEqual([{ t: T, value: 5, cost: undefined }]);
  });
});

describe('positionEvents and filterTransactions', () => {
  const txs = [tx('a', 'buy', T - 30 * DAY), tx('b', 'sell', T - 10 * DAY), tx('c', 'buy', T - DAY)];
  it('marks every trade by its day and direction', () => {
    expect(positionEvents(txs).map((e) => [e.id, e.t, e.type])).toEqual([['a', T - 30 * DAY, 'buy'], ['b', T - 10 * DAY, 'sell'], ['c', T - DAY, 'buy']]);
  });
  it('filters newest first', () => {
    expect(filterTransactions(txs, 'all').map((t) => t.id)).toEqual(['c', 'b', 'a']);
    expect(filterTransactions(txs, 'buy').map((t) => t.id)).toEqual(['c', 'a']);
    expect(filterTransactions(txs, 'sell').map((t) => t.id)).toEqual(['b']);
  });
  it('totals a trade natively', () => {
    expect(nativeTotal(tx('x', 'buy', T, '12', '110.4'))).toBeCloseTo(1_324.8);
    expect(nativeTotal(tx('x', 'buy', T, 'abc', '1'))).toBe(0);
  });
});

describe('ytdRange', () => {
  it('runs from 1 January UTC to today\'s UTC day', () => {
    expect(ytdRange(T + 3_600)).toEqual({ fromTs: Date.UTC(2026, 0, 1) / 1000, toTs: T });
  });
});
```

- [ ] **Step 2: Implement `src/mobile/lib/position-series.ts`**

```ts
export interface CostPoint extends LinePoint { cost?: number }
export interface PositionEvent extends ChartEvent { type: 'buy' | 'sell'; tx: InvestmentTransaction }
export type TxFilter = 'all' | 'buy' | 'sell';
export function positionPoints(report: Pick<TickerValueHistoryReport, 'currency' | 'points'> | undefined, reportCurrency: string, currentValue: number | null, currentCost: number | null, nowSec: number): CostPoint[]   // PositionHero.points as a pure function; `cost: amountOf(h.costBasis) ?? undefined`
export function positionEvents(transactions: readonly InvestmentTransaction[]): PositionEvent[]
export function filterTransactions(transactions: readonly InvestmentTransaction[], filter: TxFilter): InvestmentTransaction[]
export function nativeTotal(tx: Pick<InvestmentTransaction, 'quantity' | 'pricePerUnit'>): number
export function ytdRange(nowSec: number): { fromTs: number; toTs: number }   // `chartPeriodStart('YTD', nowSec)!` and `utcDayFloor(nowSec)`
```
Run the test. Expected: PASS. (Task 3's `useTwrThisYear` switches to `ytdRange` — one-line change.)

- [ ] **Step 3: `ScrubChart` cost line and mark tap**

Additive: `points: CostPoint[]`; `cost?: boolean`; the domain `valueDomain([...values, ...(cost ? costs : [])], …)`; a second `<Line type="stepAfter" dataKey="cost" stroke={tk.cost} strokeWidth={1.5} strokeDasharray="4 4" dot={false} activeDot={false} isAnimationActive={false} connectNulls />` before the value line when `cost`; `onMarkTap?.(marks[mark].cluster)` inside `onPointerUp` when a callout opens. `Hero` forwards `cost` and `onMarkTap` (two optional props). The hidden-amount axis mask stays as it is.

- [ ] **Step 4: `usePosition`**

`src/mobile/hooks/use-position.ts`: `usePosition(id: string | undefined, period: ChartPeriod)` → `{ position, holding: StockRow | null, reportCurrency, transactions, convertAt, ratesReady, history, companyInfo, companyPending, refetchCompany, isLoading, notFound }` composing the `['investment', …]`, `['investment-transactions', …]`, `['ticker-history', 'stock', ticker, …]` (with the `recalculation-complete` listener and the one-shot backfill of `PositionHero.tsx`) and the two company-info queries of `StockDetail.tsx`; `usePositionMutations(id, ticker)` → `{ remove, rename, follow }` (`remove`: `investmentsApi.delete` + the inventory's invalidations + `recordSnapshot` + `['portfolio-history']`, toast `stocks:toast.deleted`; `rename`: `updateName`; `follow`: `stockMonitorApi.follow(ticker)` + `['stock-monitor']`, toast `wealth.position.followed`).

- [ ] **Step 5: i18n keys (all ten locales)**

Add under `wealth.position`:
```json
"heroLabel": "Position value · {{quantity}} {{unit}}",
"perUnit": "{{price}} per unit",
"stats": { "value": "Value", "inCurrency": "in {{currency}}", "price": "Price" },
"tx": { "buy": "Buy · {{quantity}} {{unit}}", "sell": "Sell · {{quantity}} {{unit}}", "showAll_one": "Show all {{count}}", "showAll_other": "Show all {{count}}", "menu": "Transaction menu" },
"follow": "Watch", "following": "Watching", "followed": "Added to the watchlist",
"range52": "52-week range",
"renamed": "Name saved"
```
Czech: "Hodnota pozice · {{quantity}} {{unit}}", "{{price}} za kus", "Hodnota", "v {{currency}}", "Cena", "Nákup · {{quantity}} {{unit}}", "Prodej · {{quantity}} {{unit}}", "Zobrazit {{count}} / Zobrazit všechny {{count}} / Zobrazit všech {{count}}", "Nabídka transakce", "Sledovat", "Sledováno", "Přidáno do sledovaných", "52T rozpětí", "Název uložen". Run `npx vitest run src/i18n/`. Expected: PASS.

- [ ] **Step 6: `TradeRow`, `CompanyCard`, `PositionRenameSheet`**

`TradeRow({ tx, currency, convertAt, ratesReady, hot, onMenu })`: `EntityRow` with `id={\`tx-${tx.id}\`}`-equivalent (add an optional `id?: string` prop to `EntityRow`'s `li` — additive), chip `RowChip` TrendingUp (buy, default) / TrendingDown (sell, `tone="gain"`), title `t(\`wealth.position.tx.${type}\`, { quantity: fmt.number(qty, { maximumFractionDigits: 4 }), unit: t('stocks:table.unit') })`, sub `${fmt.day(tx.transactionDate)} · ${moneyText(price, txCurrency, { decimals: priceDecimals(price, txCurrency) })}`, value: `converted = convertAt(nativeTotal(tx), txCurrency, currency, tx.transactionDate)`; `converted !== null ? <Money value={converted} currency={currency} options={{ decimals: 0 }} sign={sell ? '+ ' : '− '} /> : <Money value={nativeTotal(tx)} currency={txCurrency} options={{ decimals: 0 }} sign={…} />`, `valueTone={sell ? 'gain' : undefined}`, valueSub `converted === null && ratesReady ? t('wealth.noRate', { currency: txCurrency }) : moneyText(nativeTotal(tx), txCurrency, { decimals: 2 })`; `onClick={onMenu}` and `onLongPress={onMenu}`; a hot row carries `data-hot` and the class `animate-[m-flash_1.2s_ease-out]` (keyframes `m-flash` from `bg-well` to transparent, defined in `src/index.css` under the mobile block — one additive line — and in `mobile.css`). `CompanyCard({ info, ticker, stockCurrency, pending, onRetry })`: a `.m-card` div with `KeyValueRow`s — Název (companyName), Měna (`info.currency ?? stockCurrency`), `stocks:detail.companyInfo.sectorIndustry` (stack), `peRatio`, `marketCap` (`formatMarketCap`), `wealth.position.range52` (low – high, `priceDecimals`), `dividendYield` (`fmt.percent`); rows with null values left out; the desktop's four fallbacks (`etfNote`, `noData`, `unavailable` + `retry` button, skeleton while `pending`) as `StockDetail.tsx` decides them. `PositionRenameSheet({ open, onOpenChange, name, onSave, isPending })`: `Sheet` titled `stocks:detail.editName.title` with the sentence `stocks:detail.editName.description`, one text field (`.m-field__i` classes: `h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-body text-ink`), footer Zrušit (ghost) + `stocks:detail.editName.save` (primary, disabled while empty or pending).

- [ ] **Step 7: The page**

`src/mobile/pages/PositionDetail.tsx` (spec §8.8 "Position detail", frame 12), `useRoute(routes.positionDetail)`:
1. `<Screen title={holding?.ticker ?? ''} leading={<NavTextButton label={t('wealth.stocks.back')} onClick={useBack(routes.stocks)} chevron />} trailing={···}>`; `···` menu: `stocks:detail.menu.editPrice` (ManualPriceModal), `editDividend` (ManualDividendModal), `editName` (the rename sheet), `wealth.position.follow` (disabled with `following` when the ticker is in `useWatchedStocks().data`), separator, `stocks:actions.deletePosition` (danger → `ConfirmSheet` with `stocks:detail.confirmDelete.title|description` and `stocks:actions.deletePosition`; on success `setLocation(routes.stocks, { replace: true })`).
2. Sub-line (`.m-nav-sub`): `[holding.companyName, stockCurrency].join(' · ')`.
3. Skeleton while `isLoading`; `stocks:detail.notFound` paragraph when `notFound`.
4. `<ConversionQualityNote quality={position.fxQuality} currency={reportCurrency} />`.
5. Hero: `label={t('wealth.position.heroLabel', { quantity: fmt.number(quantity, { maximumFractionDigits: 4 }), unit: t('stocks:table.unit') })}`, `value={holding.marketValue}`, `points = positionPoints(history, reportCurrency, holding.marketValue, holding.totalCost, now)`, `cost`, `change = pointsChange(points)` → text `<Money delta signed/> {t(\`overview.change.${period}\`)} · {t('wealth.position.perUnit', { price: moneyText(nativePrice, stockCurrency, { decimals: priceDecimals(nativePrice, stockCurrency) }) })}`, `period` state default `'30D'`, `zeroBaseline={period === 'All'}`, `events = positionEvents(transactions).filter(e => start === undefined || e.t >= start)`, `renderEventTip` = `PositionHero`'s `eventTip` (`stocks:detail.events.*`), `onMarkTap={(cluster) => flash(cluster.events.map(e => e.id))}` — `flash` expands the fold when a target row is hidden, sets `hotIds`, scrolls the first row into view (`document.getElementById(\`tx-${id}\`)?.scrollIntoView({ behavior: 'smooth', block: 'center' })`) and clears `hotIds` after 1.2 s; `children`: `<ChartLegend items={[value, dash invested, buy, sell]} />` (`stocks:detail.legend.*`).
6. `<ButtonPair primary={{ label: t('stocks:detail.buy'), icon: Plus, onClick: () => setModal('buy') }} secondary={{ label: t('stocks:detail.sell'), icon: Minus, onClick: () => setModal('sell'), disabled: quantity <= 0 }} />` → `BuyInvestmentModal` / `SellInvestmentModal` with `investment={holding}`.
7. Two `StatRow`s (the spec's six): row 1 — `wealth.position.stats.value` (value = `moneyText(quantity × nativePrice, stockCurrency, { decimals: 0 })`, note `t('wealth.position.stats.inCurrency', { currency: stockCurrency })`), `wealth.position.stats.price` (value = the converted unit price `moneyText(convert(nativePrice, stockCurrency, reportCurrency) ?? nativePrice, …)` with `priceDecimals`; note `holding.isManualPrice ? t('stocks:detail.manual') : stamp.kind === 'today' ? fmt.time(stamp.at) : stamp.kind === 'day' ? fmt.day(stamp.at, shortDayOptions(stamp.at)) : t('stocks:summary.notUpdated')`), `stocks:detail.invested` (value `statMoney(totalCost)`, note `t('stocks:detail.avgShort', { price: unitMoney(costPerUnit(totalCost, quantity)) })`); row 2 — `stocks:detail.unrealized` (signed, `tone`, note `t('stocks:detail.vsInvestedPct', { percent: fmt.percent(gainLossPercent / 100, 1, { signed: true }) })`), `stocks:detail.realized` (signed, `tone`, note `sells > 0 ? t('stocks:detail.sells', { count }) : t('stocks:detail.noSells')`), `stocks:detail.dividends` (value `yearlyDividend > 0 ? statMoney(yearlyDividend) : '—'`, note `yearlyDividend > 0 ? t('stocks:detail.dividendNote', { percent }) : t('stocks:detail.noDividend')`). `statMoney` / `unitMoney` as in `Overview.tsx` / `StockDetail.tsx`, through `moneyText`.
8. `<SectionHead title={t('stocks:detail.tabs.transactions')} note={String(transactions.length)} />`, `<Segment value={filter} options={[all, buys, sells]} … className="mb-2" />` (`stocks:detail.ledger.filter.*`), `rows = filterTransactions(transactions, filter)`, `{ shown, hidden } = foldRows(rows, expanded)`, `EntityRows` of `TradeRow` + the more row (`wealth.position.tx.showAll`); empty → `stocks:detail.transactions.empty`. The row menu (one `DropdownMenu` anchored like `AgendaRow`'s accessory, opened by `onMenu(tx)`): `common:buttons.edit` → `EditTransactionModal` (desktop), `stocks:detail.deleteTransaction` (danger) → `ConfirmSheet` (`stocks:detail.confirmDeleteTransaction.*` with the desktop's description values, `useInvestmentTransactionMutations().deleteTransaction`).
9. `<SectionHead title={t('stocks:detail.about')} note={t('stocks:detail.source')} />` + `<CompanyCard … />`.

- [ ] **Step 8: Register and contract**

`MobileShell.tsx`: `routes.positionDetail` → `PositionDetail`. `mobile.css`: `.m-stats` already covers the 2 × 3 (two rows); `@keyframes m-flash` + `.m-row.is-hot` (background from `well` to transparent over 1.2 s); `.m-kvcard` (a `.m-card` of `.m-kv` rows, `padding: 4px 16px`). `mobile.html`: a `position-frame` template (back "Aktien", title "VWCE", the sub-line, the hero with the dashed cost legend, the button pair "Kaufen / Verkaufen", two stat rows (one value in `--gain`), the transactions segment and four trade rows (one sell in `gain` with "+"), the "Alle 28 anzeigen" row, the "Über den Fonds" key-value card) at the three widths into `#position`; the check must read no horizontal overflow at 360. README §13 rows: "Trade row" (the entity row's trade variant), "Company card" (`.m-kvcard`), the ScrubChart row gains the cost line and the mark tap; the Button pair row gains `disabled`.

- [ ] **Step 9: Gate and commit**

`npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Commit `feat(mobile): position detail with the cost line, trade marks, six stats, transactions and the company card`.

## Task 5: Verification on the simulator, empty states, locale pass, screenshots

Controller-run except Step 3 (haiku) — the simulator is one device and the demo profile one database.

- [ ] **Step 1: Build and run** (started in the background right after this plan is committed, since the first build takes 5–10 minutes): `df -h /` (stop under 25 GB free), `xcrun simctl boot CD3961FF-A79E-4DFA-9A44-B591B7E694E6`, then from this worktree `npm run tauri -- ios dev "iPhone 17 Pro Max" --config '{"build":{"devUrl":"http://localhost:1450","beforeDevCommand":"npm run dev -- --port 1450 --strictPort"}}'` in the background. Expected: the app starts on the simulator (log "Moony … starting on ios").
- [ ] **Step 2: Demo profile**: `xcrun simctl terminate CD3961FF-… com.moonyfinance.mobile`; `xcrun simctl get_app_container CD3961FF-… com.moonyfinance.mobile data` → `<container>`; replace `<container>/Library/Application Support/com.moonyfinance.mobile` with a copy of `/private/tmp/claude-501/-Users-filipkral-Programov-n--FinanceApp-Moony-tauri/9ca3991e-1cce-4f7a-ac67-202f50de16cc/scratchpad/demo-profile`; relaunch (`xcrun simctl launch CD3961FF-… com.moonyfinance.mobile`). Unlock as Alex Novák (`demo1234`) through the dev browser-bridge (`preview_start` with `url: http://localhost:1450`, `resize_window` 390 × 844) or the simulator tool.
- [ ] **Step 3: Locale pass** (haiku): read the `wealth` key in the eight non-cs/en `mobile.json` files against `docs/standards/i18n-glossary.md` and the cs/en copy; fix wording, keep the plural forms; `npx vitest run src/i18n/` PASS; commit `fix(mobile): wealth locale pass`.
- [ ] **Step 4: Walk the three screens** on the demo data and record in `.superpowers/sdd/2026-10-17-mobile-phase-4-wealth/run-notes.md` what was seen; screenshots via `xcrun simctl io CD3961FF-… screenshot docs/screenshots/mobile/wealth-NN-name.png`: 01 Majetek top (hero + bar), 02 Majetek rows, 03 the `+` menu, 04 Akcie top (hero + stats), 05 Akcie rows + sort menu, 06 Sledované, 07 position hero + buttons, 08 position stats + transactions, 09 position menu, 10 the trade-row menu / confirm sheet, 11 Majetek with hide amounts on (review focus 4), 12 one scrub in progress. Verify: every money string masks with the eye on (Přehled → eye → Majetek → Akcie → detail); a mark tap flashes its row; Prodat disabled on a closed position (if the demo has one — else note it unverified); Android back is unverified (no SDK).
- [ ] **Step 5: Empty states**: terminate the app, move the demo container's `Application Support/com.moonyfinance.mobile` aside, relaunch → first run → a throwaway profile (name "Test Prázdný", CZK, password noted in the run notes, never the owner's), screenshots 13 Majetek empty, 14 Akcie empty (open it through the Majetek empty state's add → cancel → Majetek class row? the empty class row for stocks opens Akcie), 15 Sledované empty; restore the demo container.
- [ ] **Step 6: Commit the screenshots** `docs(mobile): phase 4a verification screenshots (Majetek, Akcie, position detail)` and stop the dev loop: `pkill -f "tauri ios dev"`, `pkill -f "vite --port 1450"`, `xcrun simctl shutdown CD3961FF-…`.

## Task 6: Final review and hand-over

- [ ] **Step 1:** Whole-branch review (opus) of `git diff 2d2c8d7b..HEAD` against spec §8.8, the Global Constraints and the Review Focus; one fix round (sonnet), re-review.
- [ ] **Step 2:** Write the Czech hand-over `.superpowers/sdd/2026-10-17-mobile-phase-4-wealth/handover.md` (what is on the branch, commits, verified, unverified, rulings, follow-ups, missing backend commands) and copy it to the shared notes folder `/Users/filipkral/Library/Application Support/Claude/side-session-notes/local_5f063dc4-d8a4-4b43-816b-1067b3357f71/mobile-wealth-4a.md`; append the as-built rulings below; report the branch and head SHA.

## As built / rulings (2026-10-17)

Rulings taken while planning (spec §8.8 is the authority; each says what was decided and what it costs if wrong):

1. Shared phone pieces are taken from the Finance branch `claude/mobile-finance-2e89a2` byte-identical (`Segment`, `ButtonPair`, `KeyValueRow`, `SearchField`, `ConfirmSheet`, `PlaceholderSheet`, `format-day`, `SectionHead.note`, `UpkeepCard`'s one line, the compact `Hero`), so the parent's merge is conflict-free there; `Hero` keeps this branch's hide-amounts base (`formatValue: string`, fit on the shown text). Divergences: `ButtonPair.disabled`, `Hero.children` / `cost` / `onMarkTap`, `StatRow.tone`, `ScrubChart` cost line — all additive.
2. Majetek shows the 30-day change on the hero and every row (no period segment: the prototype's frame 10 has none, and the hero's one number is Aktiva).
3. A property row's change is the last revaluation against the previous one, dated by the last (prototype), not a history period change; its sub is type · rent; LTV waits for the property detail.
4. Krypto, Dluhopisy, Ostatní aktiva and a property open placeholder pages (name + one sentence) until the next Phase 4 part; an empty class row opens the class's add dialog directly, the stocks row opens Akcie (which has the empty state).
5. `+`, Koupit, Prodat, Upravit cenu, Odhad dividendy and Upravit transakci open the desktop dialogs as they are (Finance ruling 2: they fit and scroll at 390 × 844); the rename is a phone sheet.
6. The Akcie search is a toggle button beside the sort pill, not a pull-down reveal (Finance ruling 12); pull-to-refresh is not built — Aktualizovat ceny lives in `···`.
7. Importovat výpis brokera, Historie importů, Export and the watch detail are placeholder toasts (follow-ups of the next Phase 4 part).
8. The position stats follow the spec's six (Hodnota, Cena, Vloženo, Nerealizovaný, Realizovaný, Dividendy); Hodnota is the native value in the quote currency, so it does not repeat the hero.
9. A tap on a transaction row opens its menu (Upravit, Smazat), as the long press does — a `···` is never the only way and there is no detail sheet for a trade.
10. The Majetek allocation includes cash, the four largest classes in `s1`–`s4`, the rest folded into Ostatní (as the ring).
11. The stocks class row names the top tickers by value instead of the prototype's brokers (no broker in the data model); the crypto row the same.
12. Both rows of position stats are two `StatRow`s (the contract's `.m-stats` twice), not a new 2 × 3 component.
