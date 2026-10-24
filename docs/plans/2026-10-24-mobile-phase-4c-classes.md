# Mobile Phase 4c — Krypto, Dluhopisy, Ostatní aktivum, Sledované — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The remaining Majetek class pages of spec §8.8 on the phone — Krypto with its position detail, Dluhopisy, Ostatní aktiva with the light detail, and the watch detail with the Sledované search — on the existing Tauri commands, plus (budget allowing) Akcie's Uzavřené pozice and Export.

**Architecture:** Phase 4a built Akcie and the stock position detail as one-off pages; Task 1 lifts their parts into class-neutral pieces (`ClassHero`, `PositionList`, `PositionRow`, `PositionHeroCard`, `PositionStats`, `PositionLedger`, `TradeRow`, `useTickerHistory`, `useLeaveDeleted`) without changing a pixel of Akcie, so Krypto (Task 2) is a thin page on them and the other-asset detail (Task 4) reuses the ledger. Bonds (Task 3) and the watch detail (Task 5) are new pages on the Phase 1 components. Every pure decision (sorting, the bond payment schedule, bond rows, the other-asset valuation trace, the watch period mapping and target state) lives in `src/mobile/lib/*.ts` with Vitest tests; components render. Reads use the desktop's query keys so the caches are shared; writes go through mutations that follow AGENTS rule 7. Desktop dialogs stand in where they fit (Finance ruling 2, Phase 4a ruling 5); the spec's phone sheets (Nastavit cíl, the folded add-asset form) are new sheets.

**Tech Stack:** React 19, TypeScript, wouter, TanStack Query, Radix (Dialog as the sheet, DropdownMenu as the menus), Recharts through `ScrubChart`, Lucide, react-markdown + remark-gfm (already a dependency), react-i18next (`mobile` namespace, ten locales), Tailwind on the design tokens.

**Spec:** `docs/specs/2026-10-16-mobile-app-design.md` §8.8 (Akcie "Krypto mirrors it", Sledované, position detail, Dluhopisy, Ostatní aktivum — binding), §10.3–10.5; prototype `docs/specs/2026-10-16-mobile-app-prototype.html` frames 10–12; spec wins (Phase 1 ruling R7). Previous part: `docs/plans/2026-10-17-mobile-phase-4-wealth.md` (rulings 1–25 stand unless a ruling here says otherwise). Phone component contract: `docs/design-system/mobile.css`, `mobile.html`, `docs/design-system/README.md` §13. Out of scope: Nemovitost, Úvěr, Pojistka details (a sibling session), dark mode and Dynamic Type (a sibling session), broker import on the phone.

## Global Constraints

Process (from the parent task):

- Worktree `.claude/worktrees/mobile-classes-0f3c0c`, branch `claude/mobile-classes-0f3c0c` cut from `feat/mobile` at d0128630; `git branch --show-current` before every commit; never push, never merge, never touch `main`, `feat/mobile` or another worktree (the parent session merges this branch).
- One plain shell command per call (no `&&` / `;` chains, no `git -C`); stage explicit paths only; never `git stash`.
- Two sibling branches run at the same time (loan/policy/real-estate details; dark mode + Dynamic Type). Touch shared files minimally and additively: `src/mobile/shell/routes.ts` + `routes.test.ts` and `MobileShell.tsx` (add routes, swap the three class placeholders), the ten `mobile.json` (add keys under `wealth` only, never rename or delete), `docs/design-system/{README.md,mobile.css,mobile.html}` (add blocks, rows, frames). Never edit: `Hero.tsx`, `ScrubChart.tsx`, `StatRow.tsx`, `EntityRow.tsx`, `SectionHead.tsx`, `Sheet.tsx`, `Screen.tsx`, `NavBar.tsx`, `TabBar.tsx`, `use-wealth.ts`, `Wealth.tsx`, `WealthPlaceholder.tsx`, `tokens.css`, `src/index.css`, anything under `src/mobile/pages/finance/`, `src/hooks/**` (reuse only), `src/components/**` (desktop; reuse only). `src/utils/positions.ts` gets one additive change (Task 1).
- `invoke()` only in `src/lib/tauri-api.ts`. Rust only where Task 6 says (the export allow-list), then also `cd src-tauri && cargo test`, `cargo clippy -- -D warnings`, `cargo clippy --target aarch64-apple-ios-sim --lib -- -D warnings`; no blanket `cargo update`.
- Gate per task before committing: `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check` (run `npm run format` first when it complains). Conventional Commit, trailer `Co-Authored-By: <the model that wrote the code> <noreply@anthropic.com>`.
- Subagents: implementer `sonnet`, task reviewer `sonnet`, mechanical steps `haiku`, final whole-branch review `opus`; always pass `model`; never Fable. Ledger `.superpowers/sdd/2026-10-24-mobile-phase-4c-classes/progress.md`.
- Simulator (controller only): "iPhone 17 Pro Max" UDID `CD3961FF-A79E-4DFA-9A44-B591B7E694E6`, Vite on port 1450; never `booted`, never the other simulators or ports.

From the spec (as the Phase 1 and 4a plans):

- One visual language, own chrome; reference width 390 pt, every layout checked at 360 and 430 (§5).
- Every class page has its own `+` (§6); Android back closes a sheet or pops a detail.
- Money, dates and numbers only through `useFormat()`; native amounts in their own currency; conversions through the backend's report-currency fields, `useCurrency().convert`, `useMainConvert()` or `useTransactionFx().convertAt`; a missing rate renders "—", never 0 and never 1:1 (ADR 0012).
- **Hide amounts:** every money string goes through `useMoneyText()` / `<Money>` (hero values, change lines, stats and notes, row values and subs, dated-list amounts, tips, delete sentences). Percentages, counts, dates and quantities stay. **The one exception:** public market prices of a watched ticker (Sledované rows, the watch detail's hero, chart, target and key data) stay visible — they are Yahoo's numbers, not the person's amounts. A held position's value is the person's and is masked. Reviewers grep the diff for `fmt.money(` and accept it only in the watch files (`WatchedRow.tsx`, `WatchDetail.tsx`, `WatchTargetSheet.tsx`, `watch-*.ts`).
- Mobile type scale (`text-m-*`), touch targets ≥ 44 pt, gutter 16 pt, entity row 60 pt (52 single-line), chevron only on rows without a trailing value, Lucide 24 at stroke 1.75 (18 in menus).
- Charts: scrub, callout on tap, hero chart 160 pt, colours from tokens only.
- **Tokens only** — Tailwind token classes or `var(--…)`; no hex, no `rgb()`, no `.dark` classes, no `text-white`/`bg-black`: the dark-mode sibling switches the phone by tokens alone.
- A new visual block (not a new page made of existing blocks) gets, in the same commit, a class block in `mobile.css`, a demo in `mobile.html` (German copy, 360/390/430, its check reports no overflow at 360) and a row in README §13's table.
- Strings in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`), plural forms per `docs/standards/i18n.md` §2; reuse the desktop namespaces where the words fit (`crypto:`, `bonds:`, `otherAssets:`, `stockMonitor:`, `stocks:`, `common:`) and add `mobile:wealth.*` keys only for phone wording.

### API inventory (all existing, `src/lib/tauri-api.ts`)

| Need | Wrapper / hook | Key |
|---|---|---|
| Crypto positions | `cryptoApi.getAll(currencyCode)` → `CryptoPositions`, rows `toCryptoRow` | `['crypto', currencyCode, fxRevision]` (there is no single-position command: the desktop detail reads the list) |
| All crypto trades | `cryptoApi.getAllTransactions()` | `['all-crypto-transactions']` |
| A coin's trades | `cryptoApi.getTransactions(id)` | `['crypto-transactions', id]` |
| A coin's value history | `cryptoApi.getHistory(ticker, currencyCode, start, end)`, backfill `cryptoApi.backfillHistory(ticker)` | `['ticker-history', 'crypto', ticker, start ?? 'all', currencyCode, fxRevision]` (desktop `PositionHero`) |
| Crypto price refresh | `priceApi.refreshCryptoPrices()` then `invalidateAfterPriceRefresh(queryClient)` | — |
| Delete a coin / a coin's trade | `cryptoApi.delete(id)` (keys as desktop `CryptoDetail`: `['crypto']`, `['all-crypto-transactions']`, `['crypto-transactions']` + metrics, cashflow, snapshot, history); `useCryptoTransactionMutations().deleteTransaction` | — |
| Crypto dialogs | `AddCryptoModal`, `BuyCryptoModal` / `SellCryptoModal` / `UpdateCryptoPriceModal` (`investment: CryptoRow`) | — |
| Bonds | `useBonds()` → `bonds`, `metrics` (`bondsSummary`: `totalValue`, `projectedYearlyIncome`, `averageYield`, `liveCount`, `maturedCount`, `nearest`, `nearestValue`, `missing`), `today`, `convert`; `src/utils/bonds.ts` (`bondFaceValue`, `bondYearlyCoupon`, `bondCurrency`, `isMatured`) | `['bonds']` |
| Bond writes | `useBondMutations()` → `createMutation`, `updateMutation`, `deleteMutation`; dialog `BondsFormDialog({ open, onOpenChange, onSubmit, bond?, isLoading? })` | — |
| Other assets | `otherAssetsApi.getAll()`; rows `otherAssetRow(asset, convert, convertAt, buys)` / `otherAssetsSummary` (`src/utils/other-assets.ts`) | `['other-assets']` |
| An asset's trades / valuations | `otherAssetsApi.getTransactions(id)` / `getValuations(id)` (oldest first) | `['other-asset-transactions', id]` / `['other-asset-valuations', id]` |
| Other-asset writes | `otherAssetsApi.create(data, initialTransaction?)`, `delete(id)`, `deleteTransaction(txId)` (contract as desktop `OtherAssets.tsx`: `['other-assets']`, the asset's two keys, metrics, cashflow, snapshot, history); dialogs `BuyOtherAssetModal` / `SellOtherAssetModal` / `EditOtherAssetModal` (`asset: OtherAsset`), `RevalueDialog({ kind: 'otherAsset', asset })` (props as `OtherAssets.tsx` passes them), `ValuationHistoryDialog({ kind, assetId, valuations })` | — |
| Watchlist | `useWatchedStocks()`, `useStockMonitorDetail(ticker)`, `useStockPriceRange(ticker, period)` (`src/hooks/use-stock-monitor.ts`; `period` is `src/utils/stock-monitor.ts`'s `ChartPeriod`: `1D 5D 1M 6M YTD 1Y 5Y MAX`); `useStockMonitorMutations()` → `followMutation`, `unfollowMutation`, `targetPriceMutation`, `notesMutation`, `refreshDetailMutation`; utils `dayChange`, `changeFromReference`, `targetDistance`, `inferTargetDirection`, `targetReached`, `formatMarketCap`, `percentFromFraction`, `exchangeName` | `['stock-monitor', …]` |
| Ticker search | `priceApi.searchStockTickers(query)` (desktop `StockSearchCombobox`: ≥ 2 characters, 300 ms debounce, a pick opens the detail) | — |
| Buy a watched stock | `AddInvestmentModal({ open, onOpenChange, initial: { ticker, companyName, currency } })` | — |
| CSV export (Task 6) | `exportApi.stockTransactions()` / `cryptoTransactions()` / `bonds()` / `otherAssets()` → `{ csv, filename, count }`; `useExportTextFile().exportText(fileName, contents)` (Rust allow-list `TEXT_EXPORT_FILE_NAMES` in `src-tauri/src/services/backup.rs`) | — |

## Review Focus

1. **A coin without a rate** (`marketValue === null`, priced in a currency the ECB set lacks): the Krypto row shows "—" with no change line and sorts last by value and by return, the hero total leaves it out and `ConversionQualityNote` names the currency — Task 1 `positionRows` test with crypto-shaped rows (`name`, no `companyName`) and `null` values.
2. **Bond anniversaries at the edges:** a maturity on 29 February (coupon on 28 February in other years), a maturity inside the horizon (one row: principal + last coupon), a matured bond, a bond without a maturity and one with rate 0 (left out of the dated list, still counted by the stats as `bondsSummary` does) — Task 3 `upcomingBondPayments` tests.
3. **An other asset whose currency was edited** (valuations in CZK and then EUR): today's price is the newest valuation in its currency, the trace uses only valuations in that currency, a buy or a sell steps the line by the quantity held from that day, an asset with no valuation in range is today's single point — Task 4 `otherAssetTrace` tests.
4. **Hide amounts on:** Krypto, the crypto detail, Dluhopisy (stats, dated list, rows), Ostatní aktiva and the asset detail mask every amount; Sledované rows and the watch detail keep the public prices — reviewers grep for `fmt.money(`; the controller's simulator pass screenshots each new page with the eye on.
5. **A watched ticker Yahoo knows nothing about** (`isError`, or `currentPrice === null`) and a target reached: the detail says so instead of a blank page, no target line without a target, "cíl dosažen" as a badge; `watchRange` maps every phone period (90D from `6M` trimmed to 90 days) — Task 5 tests on `watchRange` and `watchTargetState`.

(360 pt with German copy for the bond rows, the dated list and the watch stats is checked in `mobile.html`, recorded in the run notes; no Vitest.)

---

## Task 1: Class-neutral Akcie pieces (no visible change)

**Files:**
- Modify: `src/utils/positions.ts` (export `PositionRowBase` — the word `export` only)
- Modify: `src/mobile/lib/position-rows.ts` + `.test.ts`, `src/mobile/lib/position-series.ts` + `.test.ts`
- Modify: `src/mobile/hooks/use-position-ledger.ts`, `src/mobile/hooks/use-position.ts`
- Create: `src/mobile/hooks/use-ticker-history.ts`, `src/mobile/hooks/use-leave-deleted.ts`
- Create: `src/mobile/components/wealth/{ClassHero,PositionList,ClassSkeleton,ClassEmpty,PricesLine}.tsx`
- Modify: `src/mobile/components/wealth/{PositionRow,PositionHeroCard,PositionStats,PositionLedger,TradeRow}.tsx`
- Modify: `src/mobile/pages/Stocks.tsx`, `src/mobile/pages/PositionDetail.tsx`

**Interfaces:**
- Produces (lib): `LedgerTx = { id: string; type: string; ticker: string; quantity: string; pricePerUnit: string; currency: string; transactionDate: number }` in `position-series.ts`; `positionEvents<T extends LedgerTx>(txs: readonly T[]): PositionEvent<T>[]`, `PositionEvent<T extends LedgerTx = LedgerTx>` with `tx: T`; `filterTransactions<T extends Pick<LedgerTx, 'type' | 'transactionDate'>>(txs, filter): T[]`; `revealRow` generic the same way. `position-rows.ts`: `Sortable` takes `companyName?: string; name?: string`, new `displayName(row: { ticker: string; companyName?: string; name?: string }): string` (`companyName || name || ticker`), used by `searchMatches` and the name sort.
- Produces (hooks): `usePositionLedger<T extends LedgerTx>(transactions: readonly T[])` (state type `PositionLedgerState<T>`); `useTickerHistory(kind: 'stock' | 'crypto', ticker: string | undefined, period: ChartPeriod): TickerValueHistoryReport | undefined` (the history query, the once-per-ticker backfill and the `recalculation-complete` listener moved out of `usePosition`, key `['ticker-history', kind, ticker, periodStart ?? 'all', currencyCode, fxRevision]`, backfill through `investmentsApi` or `cryptoApi` by kind); `useLeaveDeleted(listRoute: string): (detailPath: string, queryKeys: readonly QueryKey[]) => void` (PositionDetail's `leaveDeleted`: `back()`, then after 250 ms replace with `listRoute` when still on `detailPath`, then `removeQueries` for each key).
- Produces (components): `ClassHero({ trendClass: TrendClass; label: string; totalValue: number | null; invested: number | null; currency: string; trades?: readonly (TradeLike & { transactionDate: number })[]; convertAt?: (amount: number, from: string, to: string, date: number) => number | null; rememberKey: string })` — Akcie's `PortfolioHero`, period remembered under `${rememberKey}.period`, marks and the legend only when `trades` is given; `PositionList<R extends PositionRowBase & { companyName?: string; name?: string }>({ rows: readonly R[]; hrefOf: (row: R) => string; currency: string; showClosed: boolean; dimmed: boolean; rememberKey: string })` — Akcie's `PositionsTab`, sort remembered under `${rememberKey}.sort`; `ClassSkeleton()` (Akcie's skeleton); `ClassEmpty({ icon: ReactNode; title: string; description: string; addLabel: string; onAdd: () => void })`; `PricesLine({ count: string; rows: readonly { fetchedAt: number | null }[] })` (the centred caption "3 pozice · ceny 9:40"); `PositionRow({ row: PositionRowBase; name: string; href: string; currency: string })`; `PositionHeroCard` with `holding: PositionRowBase`, `transactions: readonly LedgerTx[]`, `unit: string` (label, tips; Akcie passes `stocks:table.unit`); `PositionStats({ holding: PositionRowBase; reportCurrency: string; sells: number; sixth: StatItem })` plus `useDividendStat(holding: StockRow, reportCurrency: string): StatItem` exported from the same file; `PositionLedger<T extends LedgerTx>({ ledger: PositionLedgerState<T>; count: number; currency: string; convertAt; ratesReady: boolean; unit: string; deleteTransaction: { mutate: (id: string, opts: { onSuccess: () => void }) => void; isPending: boolean }; onEdit?: (tx: T) => void })`; `TradeRow` with `tx: LedgerTx`, `unit: string`, `onEdit?` (no Upravit item without it).
- Consumes: nothing new.

- [ ] **Step 1: Failing lib tests.** In `position-rows.test.ts`: `displayName({ ticker: 'BTC', name: 'Bitcoin' })` is `'Bitcoin'`, `displayName({ ticker: 'AAPL', companyName: '' })` is `'AAPL'`; `positionRows` over `[{ ticker: 'ETH', name: 'Ethereum', quantity: 1, marketValue: null, gainLossPercent: null }, { ticker: 'BTC', name: 'Bitcoin', quantity: 1, marketValue: 100, gainLossPercent: 5 }]` sorted `value` → `['BTC', 'ETH']`, `gain` → `['BTC', 'ETH']`, `name` → `['BTC', 'ETH']`, search `'ethe'` → `['ETH']`. In `position-series.test.ts`: `filterTransactions` and `positionEvents` accept a crypto-shaped trade (a `CryptoTransaction` literal) and keep it as the same object. Run `npx vitest run src/mobile/lib/position-rows.test.ts src/mobile/lib/position-series.test.ts` — FAIL.
- [ ] **Step 2: Implement the lib changes** above; existing tests unchanged. Run the same command — PASS.
- [ ] **Step 3: Hooks.** `useTickerHistory`, `useLeaveDeleted`, generic `usePositionLedger`; `usePosition` calls `useTickerHistory('stock', ticker, period)` and keeps its return shape.
- [ ] **Step 4: Components and the two pages.** Move code, do not rewrite it: `PortfolioHero` → `ClassHero`, `PositionsTab` → `PositionList`, `StocksSkeleton` → `ClassSkeleton`, `NoPositions` → `ClassEmpty` (Akcie passes the `stocks:` texts), the sub line → `PricesLine`; `PositionDetail` uses `useLeaveDeleted(routes.stocks)`, owns `EditTransactionModal` and passes `onEdit` + `useInvestmentTransactionMutations().deleteTransaction` to `PositionLedger`, `sixth={useDividendStat(holding, reportCurrency)}`, `unit={ts('table.unit')}`. The `MENU_ITEM` / `toneOf` copies these files touch move to one place each (`RowMenuItem` exists for menu items; `toneOf` goes next to `StatItem` users as `statTone(value: number | null): StatItem['tone']` in `src/mobile/lib/stat-money.ts` with a test: `statTone(null)` and `statTone(0)` undefined, `statTone(2)` `'gain'`, `statTone(-1)` `'loss'`).
- [ ] **Step 5: Gates.** `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`. Akcie and the stock detail render as before (the reviewer compares the JSX trees; the controller checks on the device in Task 7).
- [ ] **Step 6: Commit** `refactor(mobile): class-neutral pieces of Akcie and the position detail`.

## Task 2: Krypto and the crypto position detail

**Files:**
- Create: `src/mobile/hooks/use-crypto.ts`, `src/mobile/pages/Crypto.tsx`, `src/mobile/pages/CryptoDetail.tsx`, `src/mobile/components/wealth/CryptoMenu.tsx`
- Modify: `src/mobile/shell/routes.ts` + `routes.test.ts`, `src/mobile/shell/MobileShell.tsx`, ten `mobile.json`

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces: `routes.cryptoDetail = '/wealth/crypto/:id'`, `cryptoPath(id: string)`; `useCryptoPositions(): { positions: CryptoPositions | undefined; rows: CryptoRow[]; reportCurrency: string; isLoading: boolean; isError: boolean }`; `useCryptoTrades(): { transactions: CryptoTransaction[]; convertAt; ready }`; `useRefreshCryptoPrices()` (mutation; toast `crypto:detail.pricesRefreshed`, error `common:status.error`); `useCryptoPosition(id: string | undefined, period: ChartPeriod): { positions; holding: CryptoRow | null; reportCurrency; transactions: CryptoTransaction[]; convertAt; ratesReady; history; isLoading; notFound }`; `useDeleteCrypto(id)` (mutation contract with the keys of the inventory).

- [ ] **Step 1: Route test.** `expect(tabForPath(cryptoPath('c1'))).toBe('wealth')` in `routes.test.ts`; run `npx vitest run src/mobile/shell/` — FAIL, then add the route and helper — PASS.
- [ ] **Step 2: `use-crypto.ts`** as the interfaces say (copy the shape of `useStockPositions` / `useStockTrades` / `usePosition`; the coin is `positions.items.find(id)`, history through `useTickerHistory('crypto', …)`).
- [ ] **Step 3: `Crypto.tsx`** (`/wealth/crypto`): `Screen` titled `dashboard:cards.crypto`, back `mobile:tabs.wealth` to `routes.wealth`; trailing `···` (Aktualizovat ceny `crypto:` refresh label, Uzavřené pozice as Akcie's checkbox remembered under `crypto.showClosed`, separator, Export → toast `wealth.placeholder.later` until Task 6) and `+` → `AddCryptoModal`. Body: `ClassSkeleton` while loading; `ClassEmpty` (Bitcoin icon, `crypto:empty.*`, `crypto:` add label) when no rows; load failure line; else `PricesLine` (`crypto:summary.positions` count), `ConversionQualityNote`, `ClassHero trendClass="crypto" rememberKey="crypto"` with `useCryptoTrades()`, three stats (ruling 1), `PositionList rememberKey="crypto" hrefOf={(row) => cryptoPath(row.id)}`. No Pozice · Sledované segment (ruling 3).
- [ ] **Step 4: `CryptoDetail.tsx`** (`/wealth/crypto/:id`): as `PositionDetail` — title ticker, first line `name · originalCurrency`, `ConversionQualityNote`, `PositionHeroCard unit={holding.ticker}`, Koupit · Prodat (`BuyCryptoModal` / `SellCryptoModal`, Prodat disabled at quantity 0), `PositionStats` with the sixth stat Držím (ruling 2), `PositionLedger` with `useCryptoTransactionMutations().deleteTransaction` and no `onEdit`, no company card. `CryptoMenu` (`···`): Upravit cenu → `UpdateCryptoPriceModal`, separator, Smazat pozici → `ConfirmSheet` (`crypto:` confirm texts) → `leaveAfterSheet` → `useLeaveDeleted(routes.cryptoList)(cryptoPath(id), [['crypto-transactions', id]])`.
- [ ] **Step 5: Register** `Crypto` on `routes.cryptoList` (replacing `CryptoPlaceholder` in `MobileShell.tsx`; the placeholder export stays in `WealthPlaceholder.tsx`) and `CryptoDetail` on `routes.cryptoDetail`.
- [ ] **Step 6: i18n.** Only phone wording missing from `crypto:` / `stocks:` (expected: the three stat labels' notes if `crypto:summary.*` lacks them, `wealth.crypto.heldNote`); all ten locales; `npx vitest run src/i18n/` — PASS.
- [ ] **Step 7: Gates and commit** `feat(mobile): Krypto class page and the crypto position detail`.

## Task 3: Dluhopisy

**Files:**
- Create: `src/mobile/lib/bond-schedule.ts` + `.test.ts`, `src/mobile/lib/bond-rows.ts` + `.test.ts`, `src/mobile/pages/Bonds.tsx`, `src/mobile/components/wealth/{BondRow,BondPaymentRow}.tsx`
- Modify: `MobileShell.tsx`, ten `mobile.json`, `docs/design-system/{mobile.css,mobile.html,README.md}` (the dated list and the bond row's badge if no existing block covers them)

**Interfaces:**
- Produces: `BondPayment = { t: number; bond: Bond; kind: 'coupon' | 'maturity'; amount: number; currency: string }`; `upcomingBondPayments(bonds: readonly Bond[], todaySec: number, months = 12): BondPayment[]`; `BondRowModel = { bond: Bond; face: number; currency: string; rate: number; quantity: number; unitFace: number; maturity: number | null; badge: 'withinYear' | 'matured' | null }`; `bondRows(bonds: readonly Bond[], todaySec: number, showMatured: boolean): BondRowModel[]`.

- [ ] **Step 1: Failing tests** (`bond-schedule.test.ts`), today = 2026-10-24 UTC midnight, bonds built with a local `bond(overrides)` helper:
  - a 10 × 1 000 CZK bond at 5 % maturing 2028-03-15 → one row `{ t: 2027-03-15, kind: 'coupon', amount: 500, currency: 'CZK' }`;
  - maturing 2027-02-10 → one row `{ t: 2027-02-10, kind: 'maturity', amount: 10 500 }` (principal + last coupon);
  - maturing 2028-02-29 at 4 % on 1 × 1 000 EUR → `{ t: 2027-02-28, kind: 'coupon', amount: 40, currency: 'EUR' }`;
  - matured (2026-01-01), no maturity, rate 0 with maturity 2027-06-01 → coupon rows none; the rate-0 bond still gives its maturity row (amount 1 000 × qty) because principal is due;
  - several bonds → ascending by `t`, a tie keeps input order; nothing after `today + months`.
  `bond-rows.test.ts`: badge `withinYear` when `maturity - today ≤ 365 days`, `matured` when matured, `null` otherwise or without maturity; sorted by maturity ascending, no maturity last; matured left out unless `showMatured`. Run — FAIL.
- [ ] **Step 2: Implement** both (UTC dates; an anniversary that does not exist in a year falls on the month's last day). PASS.
- [ ] **Step 3: `Bonds.tsx`** (`/wealth/bonds`): `Screen` titled `dashboard:cards.bonds`, back to Majetek; trailing `···` (checkbox `bonds:table.showMatured` remembered under `bonds.showMatured`, Export → toast later until Task 6) and `+` → `BondsFormDialog` + `createMutation`. Body: skeleton / `ClassEmpty` (`bonds:empty.*`, `bonds:addBond`) / content: `ConversionQualityNote`-style note when `metrics.missing` is non-empty (`common:fxQuality.missing`); `StatRow` — Nominál (`metrics.totalValue`, note `bonds:summary.issues` count), Roční kupón (`metrics.projectedYearlyIncome`, note `bonds:summary.weightedRate`), Nejbližší splatnost (`fmt.day(nearest.maturityDate)`, note `bonds:summary.returns` with `metrics.nearestValue`, or `bonds:summary.noMaturity`); section "Nejbližší kupóny a splatnosti" (new `wealth.bonds.upcoming`) with `BondPaymentRow`s (title bond name, sub `{day} · {Kupón | Splatnost}`, value `+ amount` native, first eight then `MoreRow`) or the line `wealth.bonds.noUpcoming`; section `bonds:table.title` with `BondRow`s (title name + badge `bonds:table.withinYear` / `bonds:table.matured`, sub `ISIN · {quantity} × {unitFace} · {rate %}` with ISIN left out when null, value face total native, value sub maturity day or `bonds:table.noMaturity`).
- [ ] **Step 4: Row actions.** A tap opens `BondsFormDialog` with the bond (ruling 4) and `updateMutation`; a long press opens `RowMenu` (Upravit, Smazat); Smazat → `ConfirmSheet` (`bonds:` confirm texts) → `deleteMutation`.
- [ ] **Step 5: Register** `Bonds` on `routes.bondsList`; i18n in all ten locales; design-system blocks if new.
- [ ] **Step 6: Gates and commit** `feat(mobile): Dluhopisy with the upcoming coupons and maturities`.

## Task 4: Ostatní aktiva and the light detail

**Files:**
- Create: `src/mobile/lib/other-asset-trace.ts` + `.test.ts`, `src/mobile/hooks/use-other-asset.ts`, `src/mobile/pages/OtherAssets.tsx`, `src/mobile/pages/OtherAssetDetail.tsx`, `src/mobile/sheets/OtherAssetAddSheet.tsx`, `src/mobile/components/wealth/OtherAssetMenu.tsx`
- Modify: `routes.ts` + test, `MobileShell.tsx`, `src/mobile/components/wealth/AddAssetMenu.tsx` (the "Aktivum" item opens `OtherAssetAddSheet`), ten `mobile.json`, design-system files if the fold or the three-button block is new

**Interfaces:**
- Consumes: `ClassHero`, `ClassSkeleton`, `ClassEmpty`, `PositionLedger`, `usePositionLedger`, `useLeaveDeleted` (Task 1).
- Produces: `routes.otherAssetDetail = '/wealth/other-assets/:id'`, `otherAssetPath(id)`; `otherAssetTrace(valuations: readonly AssetValuation[], transactions: readonly OtherAssetTransaction[], priceCurrency: string, currentPrice: number, currentQuantity: number, nowSec: number): CostPoint[]`; `useOtherAssets()` (list + rows through `otherAssetRow` with `useMainConvert` / `useDatedMainConvert`, as desktop `OtherAssets.tsx`), `useOtherAsset(id)` (asset from the list, its transactions and valuations), `useOtherAssetMutations(id)` → `remove`, `deleteTransaction`, `create`.

- [ ] **Step 1: Failing tests** (`other-asset-trace.test.ts`): valuations 1. 3. (10 CZK), 1. 6. (12 CZK), 1. 9. (5 EUR) with `priceCurrency` EUR → only 1. 9. counts; a buy of 2 on 1. 2. and 1 more on 1. 7. with CZK valuations → points at 1. 3. (10 × 2), 1. 6. (12 × 2), 1. 7. (12 × 3), today (current price × current quantity); no valuations → `[today]`; a sell dropping the quantity to 0 → a 0 point that day; today replaces a point of the same UTC day. Run — FAIL; implement (a trade day takes the newest valuation on or before it, none → no point that day) — PASS.
- [ ] **Step 2: Hooks** (`use-other-asset.ts`), the mutation contract as the inventory says; `create(data, initialTransaction?)` invalidates `['other-assets']`, metrics, cashflow, snapshot, history.
- [ ] **Step 3: `OtherAssetAddSheet`** (spec §10.5: required fields first): Název, Měna (pick row → `CurrencyPickerSheet`), Dnešní cena za kus (`AmountField` with the currency); "Další údaje" fold (closed by default): první nákup — množství, cena za kus, datum (`DayPickerSheet`, today) — and výnos (type as `otherAssets:` yield types, value). Field labels and validation from `otherAssets:`; the first purchase is sent only when quantity and price are filled. Save → `create`, close, toast `otherAssets:` added text.
- [ ] **Step 4: `OtherAssets.tsx`** (`/wealth/other-assets`): `Screen` `dashboard:cards.otherAssets`, `+` → the add sheet, `···` → Export (toast later until Task 6); `ClassHero trendClass="otherAssets" rememberKey="otherAssets"` without trades (total from `otherAssetsSummary`); rows (chip Gem icon, title name, sub `{quantity} × {price}` native or `wealth.otherAssets.valuedOn`, value in the report currency or "—", value sub `ChangeLine` of the gain when known) → `otherAssetPath`.
- [ ] **Step 5: `OtherAssetDetail.tsx`**: title name, first line `{quantity} {otherAssets:table.unit} · {priceCurrency}`; `Hero` (native value `quantity × price` in `priceCurrency`, step line from `otherAssetTrace`, buy/sell marks from `positionEvents`, the remembered period trimming the points to `chartPeriodStart`); `ButtonPair` Koupit · Prodat (desktop modals) and under it a full-width secondary Přecenit (`RevalueDialog`, ruling 6); `StatRow` — Množství (note price per unit), Průměrná cena (note invested total), Nerealizovaný (`gainNative` signed with tone, "—" when the currencies differ, note percent); section transactions through `PositionLedger` (the transactions mapped to `{ ...tx, ticker: asset.name }`, `unit` `otherAssets:table.unit`, `deleteTransaction` from `useOtherAssetMutations`, no `onEdit`). `OtherAssetMenu` (`···`): Upravit (`EditOtherAssetModal`), Historie přecenění (`ValuationHistoryDialog`), separator, Smazat → confirm → `useLeaveDeleted(routes.otherAssetsList)`.
- [ ] **Step 6: Register** both routes; `AddAssetMenu`'s asset item opens `OtherAssetAddSheet`; i18n; design system.
- [ ] **Step 7: Gates and commit** `feat(mobile): Ostatní aktiva, the light asset detail and the folded add sheet`.

## Task 5: Sledované — search, rows and the watch detail

**Files:**
- Create: `src/mobile/lib/watch-detail.ts` + `.test.ts`, `src/mobile/hooks/use-ticker-search.ts`, `src/mobile/pages/WatchDetail.tsx`, `src/mobile/sheets/WatchTargetSheet.tsx`, `src/mobile/components/wealth/{WatchNotes,WatchKeyData,TickerResultRow}.tsx`
- Modify: `src/mobile/components/wealth/WatchedRow.tsx`, `src/mobile/pages/Stocks.tsx` (the Sledované segment), `routes.ts` + test, `MobileShell.tsx`, ten `mobile.json`, design-system files (the notes block)

**Interfaces:**
- Produces: `routes.watchDetail = '/wealth/watch/:ticker'`, `watchPath(ticker: string) = '/wealth/watch/' + encodeURIComponent(ticker)`; `watchRange(period: ChartPeriod /* utils/period */, nowSec: number): { range: WatchPeriod /* utils/stock-monitor ChartPeriod */; from: number | null }` — `30D → { '1M', null }`, `90D → { '6M', now − 90 days }`, `YTD → { 'YTD', null }`, `1Y → { '1Y', null }`, `All → { 'MAX', null }`; `watchTargetState(current: number | null, target: number | null, direction: TargetDirection | null): 'none' | 'reached' | 'pending'`; `watchPoints(range: readonly StockPricePoint[], from: number | null, current: number | null, fetchedAt: number | null, target: number | null): CostPoint[]` (desktop's `points` + the trim + `cost: target` on every point when a target exists); `useTickerSearch(query: string): { results; isFetching }` (debounced 300 ms, enabled from 2 characters, key `['stock-search', debounced]`).

- [ ] **Step 1: Failing tests** for the three pure functions: each period's mapping; `watchPoints` appends today's price after the last point when `fetchedAt` is later and overwrites the last point otherwise, drops points before `from`, sets `cost` only with a target; `watchTargetState(100, null, null)` `'none'`, `(120, 110, 'above')` `'reached'`, `(100, 110, 'above')` `'pending'`, `(null, 110, 'above')` `'pending'`. FAIL → implement → PASS.
- [ ] **Step 2: Sledované segment.** On top a `SearchField` (`stockMonitor:` search placeholder); with 2+ characters `TickerResultRow`s (symbol, name, exchange; tap → `watchPath(symbol)`, the field clears) or `stockMonitor:` no-results text; otherwise the watched rows. `WatchedRow`: tap → `watchPath(row.ticker)`, prices unmasked (`fmt.money` with `priceDecimals`), the target state as a badge (`wealth.stocks.target` pending, `wealth.stocks.targetReached` reached; none → nothing).
- [ ] **Step 3: `WatchDetail.tsx`**: title ticker, back `wealth.stocks.back` to `routes.stocks`; first line `name · exchangeName(exchange)`; error / no price → `EmptyState` with `stockMonitor:detail.noData`; `Hero` (unmasked native price, change over the shown points via `changeFromReference`, the remembered period `watch.period` over `CHART_PERIODS`, `points = watchPoints(...)`, `cost` when a target exists, legend price line + target dash); `ButtonPair`: followed → primary Nastavit cíl (`WatchTargetSheet`), secondary Koupit (`AddInvestmentModal` with `initial`); not followed → primary Sledovat (`followMutation`), secondary Koupit; `StatRow` — Dnes (`dayChange`, tone, note absolute), Cíl (target price or "—", note `targetDistance` / `wealth.stocks.targetReached` / `stockMonitor:` none text), Od sledování (change since `followedAt` from a second `useStockPriceRange` as the desktop's `sinceRange`, note the day; "—" when not followed); `WatchNotes`; `WatchKeyData`. `···`: Aktualizovat (`refreshDetailMutation`), and when followed a separator + Přestat sledovat → `ConfirmSheet` → `unfollowMutation` → `leaveAfterSheet` → back.
- [ ] **Step 4: `WatchTargetSheet`**: `Sheet` with `AmountField` in the ticker's currency (prefill the current target, else empty), `Segment` Nad · Pod (`stockMonitor:` direction labels; default `inferTargetDirection(value, current)` until the person picks), Uložit → `targetPriceMutation`; when a target exists a danger text button Odebrat cíl saves `null, null`. Validation: a positive number, the error under the field.
- [ ] **Step 5: `WatchNotes`**: section head `stockMonitor:` notes title with the action Upravit; view mode renders the markdown (`ReactMarkdown` + `remarkGfm`, the desktop's `MarkdownView` rules; empty → `stockMonitor:notes.empty`) in a paper card styled by a new `.m-notes` block (headings, lists, links, code on tokens); edit mode is a plain `textarea` (body type, 8 rows, autogrow not required) with Zrušit / Uložit → `notesMutation`. `WatchKeyData`: a `.m-kvcard` of `KeyValueRow`s — Burza, Měna, Tržní kapitalizace (`formatMarketCap`), P/E, Dividendový výnos (`percentFromFraction`), 52 týdnů (low – high), and when held V portfoliu → `positionPath(heldInvestmentId)` with a chevron.
- [ ] **Step 6: Register** the route; i18n; design system (`.m-notes`).
- [ ] **Step 7: Gates and commit** `feat(mobile): Sledované search, rows and the watch detail`.

## Task 6 (budget): Uzavřené pozice and Export

**Files:**
- Create: `src/mobile/sheets/ClosedPositionsSheet.tsx`, `src/mobile/lib/closed-positions.ts` + `.test.ts`, `src/mobile/hooks/use-class-export.ts`
- Modify: `Stocks.tsx`, `Crypto.tsx`, `Bonds.tsx`, `OtherAssets.tsx` (the `···` items), `src/mobile/components/wealth/PositionList.tsx` (`showClosed` goes), `src-tauri/src/services/backup.rs` (allow-list + test), ten `mobile.json`

**Interfaces:**
- Produces: `closedPositions<R extends PositionRowBase>(rows: readonly R[], lastSellDay: (id: string) => number | null): { row: R; closedAt: number | null }[]` (quantity 0, newest close first, unknown last); `useClassExport(kind: 'stocks' | 'crypto' | 'bonds' | 'otherAssets'): { exportCsv: () => Promise<void>; isPending: boolean }`.

- [ ] **Step 1: Closed positions.** Test `closedPositions` (order, open ones left out) — FAIL → implement — PASS. The `···` checkbox becomes the item Uzavřené pozice opening `ClosedPositionsSheet` (rows: ticker, name; realized result signed through the eye with tone; sub `wealth.closed.on` day of the last sell; tap → the detail, the sheet closes first via `leaveAfterSheet`); the list shows open positions only (ruling 11). Stocks and Krypto.
- [ ] **Step 2: Rust allow-list.** `TEXT_EXPORT_FILE_NAMES` gains `moony-stock-transactions.csv`, `moony-crypto-transactions.csv`, `moony-bonds.csv`, `moony-other-assets.csv`; a test that each is accepted and `../moony-bonds.csv` is not. Run the Rust gates of Global Constraints.
- [ ] **Step 3: `useClassExport`**: fetch the CSV (`exportApi.*`), `exportText(<allow-listed name>, csv)`, toast `common:` exported text on `'saved'`, nothing on `'cancelled'`, `common:status.error` with the message on failure. Wire Export in the four `···` menus.
- [ ] **Step 4: Gates (npm + Rust) and commit** `feat(mobile): closed positions sheet and CSV export of the class pages`.

## Task 7: Verification on the simulator (controller)

Demo profile (Alex Novák) on the Pro Max; every new page at the top and scrolled, the eye on for each, Krypto → detail → Koupit dialog, a bond's edit dialog and long-press menu, an asset detail with Přecenit, Sledované search → a ticker → Sledovat → Nastavit cíl → notes edit/view → key data; Akcie and the stock detail unchanged after Task 1. Screenshots `docs/screenshots/mobile/classes-NN-*.png`, committed with `docs(mobile): phase 4c verification screenshots`.

## Task 8: Final review and hand-over

Opus whole-branch review against `feat/mobile` (d0128630), one fix round, a narrowed re-review; plan rulings appended below; Czech hand-over `.superpowers/sdd/2026-10-24-mobile-phase-4c-classes/handover.md` and a note in the shared notes folder.

---

## As built / rulings

Rulings taken while planning (spec §8.8 is the authority; each says what it costs if wrong):

1. Krypto's three stats are the unrealized result, the realized result (with the number of sells) and the largest position (ticker, share of the class): "Krypto mirrors Akcie", but coins pay no dividends and there is no performance command for crypto. Costs a stat each if a TWR for crypto lands in the backend.
2. The crypto position's six stats are the desktop crypto detail's: Hodnota, Kurz, Vloženo, Nerealizovaný, Realizovaný, Držím (duration, since, share of the class) — the dividends stat has no meaning for a coin.
3. Krypto has no Pozice · Sledované segment (the watchlist is Yahoo stocks); its `···` is Aktualizovat ceny, Uzavřené pozice, Export; the coin's `···` is Upravit cenu and Smazat (no rename, dividend or follow command for crypto); a coin's trade has no Upravit (there is no update command), so its row menu is Smazat alone.
4. A bond row's tap opens the desktop `BondsFormDialog`, not a phone sheet: every field of it is required for an edit and it fits at 390 × 844 (Finance ruling 2). A long press offers Upravit and Smazat.
5. "Nejbližší kupóny a splatnosti" covers the next 12 months, puts each coupon on the anniversary of the maturity day (the desktop ladder's assumption) and the principal with the last coupon on the maturity day; bonds without a maturity have no day to show and are left out of the list (the stats still count them).
6. The other asset's hero is its native value in the price currency (ADR 0012: native is authoritative), its line a step trace of the valuations in that currency times the quantity held then; Koupit · Prodat sit as the position's pair with Přecenit full-width under it; the trade, edit, revalue and valuation-history dialogs are the desktop's; adding an asset is the new phone sheet everywhere the phone adds one (the folded first purchase the spec asks for).
7. The watch detail offers the phone's five periods, mapped onto Yahoo's ranges (90 days from the six-month range, trimmed); intraday 1D / 5D stay on the desktop. The target is drawn as the hero's dashed cost line at the target price.
8. A search result opens the watch detail (the desktop's flow); following happens there with Sledovat.
9. `use-wealth.ts` keeps its own crypto query (the same key, so the same cache) instead of importing `useCryptoPositions`: the file is outside this branch's scope and a sibling may touch Majetek.
10. The crypto and other-asset heroes reuse the `stocks:` legend and callout strings (Hodnota, Nákup, Prodej, Vloženo): the words are class-neutral.
11. (Task 6) Uzavřené pozice becomes a sheet listing the closed positions with their realized result instead of a toggle that mixed them into the list, as the spec's `···` item names a list.

Rulings taken while executing: (appended as the ledger records them)
