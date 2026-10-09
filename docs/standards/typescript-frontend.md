# TypeScript Frontend Standards

Rules for all code under `src/`. Each rule states the why and points to a real example
in this repo. Copy the canonical examples — never the legacy outliers (see also
`docs/DEPRECATED.md`).

## 1. `invoke()` only inside `tauri-api.ts`

Never call Tauri's `invoke()` outside `src/lib/tauri-api.ts`. Components and hooks
import the typed `*Api` namespaces (`bankAccountsApi`, `portfolioApi`, …) instead.
**Why:** one file owns the wire contract (command names, arg shapes, return types) —
scattered `invoke()` calls bypass the types and rot silently (see
`docs/standards/type-contract.md`).

## 2. File layout and naming

- **Pages:** PascalCase files with a **default export** in `src/pages/`, registered as
  routes in `src/App.tsx` inside `<ProtectedRoute>` (wouter `Switch`; only `/auth` is
  public).
- **Components:** **named exports** in `src/components/<domain>/`.
- **Hooks:** kebab-case files — `use-<domain>.ts` for reads (`useQuery`) and
  `use-<domain>-mutations.ts` for writes (`useMutation`). `useCategorization.ts` is the
  legacy naming outlier — don't copy it.
- **Aliases:** `@/` → `src/`, `@shared` → `shared/`.

**Why:** agents locate code by convention; every outlier makes the next grep lie.

## 3. React Query

- **Keys:** kebab-case array keys — `["bank-accounts"]`, `["bank-account", id]`.
- **Defaults:** global defaults are `staleTime: Infinity, retry: false`
  (`src/lib/queryClient.ts`). Data never refetches by itself — opt into freshness
  explicitly (per-query `staleTime`/`refetchOnMount`) when you need it.
- **Mutation contract** (every write that changes money or asset state): invalidate the
  domain key(s) **plus** `["portfolio-metrics"]` **plus** `["cashflow-report"]`, then
  `await portfolioApi.recordSnapshot()` and invalidate `["portfolio-history"]`.
  Canonical: `src/hooks/use-bank-account-mutations.ts`.
- **Milestones refresh:** every successful `useMutation` invalidates `["milestones"]` through the
  query client's `MutationCache` (`src/lib/queryClient.ts`), so mutation hooks do not list it.
  Writes outside `useMutation` — direct API calls, Tauri event listeners, background refreshes —
  must invalidate `["milestones"]` themselves.
- Put mutations in dedicated `use-<domain>-mutations.ts` hook files, not inline in
  modals. The inline `useMutation` calls in stocks/crypto/real-estate components are
  legacy debt, not a pattern.

**Why:** a mutation that skips an invalidation or the snapshot leaves the dashboard and
trend charts showing stale net worth with no error anywhere.

## 4. Forms and dialogs

- **Forms:** react-hook-form + `zodResolver` + shadcn `Form`/`FormField` components.
  Zod validation messages are i18n keys, not English strings (see
  `docs/standards/i18n.md`).
- **Dialogs:** shadcn `Dialog`/`AlertDialog` with open-state **lifted to the page** —
  `addDialogOpen`/`editDialogOpen`/`deleteDialogOpen` + `selectedItem`. Canonical:
  `src/pages/Loans.tsx`.

**Why:** lifted dialog state keeps modals stateless and reusable; the shared form stack
gives consistent validation display and translated errors for free.

## 5. Styling

- Tailwind utilities with **semantic tokens only**: `bg-background`,
  `text-muted-foreground`, and `positive`/`negative` for gains/losses. No inline hex
  colors.
- Compose classes with `cn()` from `@/lib/utils`.
- Charts: recharts.
- `src/components/ui/` is vendored shadcn (new-york) — it is ESLint-excluded; never
  hand-edit it casually.

**Why:** semantic tokens are what make dark mode and theming work; a hardcoded hex
looks right in one theme and breaks in the other.

## 6. Errors and toasts

Surface backend errors via sonner toasts. `tauriInvoke` (`src/lib/tauri-api.ts`) throws an
`ApiError` (`src/lib/api-error.ts`) whose `message` is already localized: i18n keys
(`validation.*`, `auth.*`) are translated, and technical text (rusqlite, IO, HTTP bodies) is
replaced by a generic `common:errors.*` sentence. `error.message` is therefore safe to show;
`error.kind` / `error.key` / `error.raw` are available for logic and logs. Never branch on
message text — use `error instanceof ApiError && error.kind === …`. `translateApiError`
(`src/lib/translate-api-error.ts`) still exists for plain strings and returns an
`ApiError`'s message untouched.
**Why:** raw backend errors look like `"Validation error: validation.bondNameRequired"` or
raw rusqlite text; showing them verbatim leaks implementation strings to the user.

## 7. Tauri event listeners

Every Tauri `listen()` call gets its `unlisten()` in the `useEffect` cleanup.
**Why:** un-cleaned listeners stack on every remount and fire handlers multiple times.

## 8. Currency

Rules of ADR 0012 (which carries ADR 0011's report rules over to the whole portfolio). There is
no base currency: every amount is shown in the currency its response or record declares.

- **Never convert a response again.** Commands that return money take a report currency
  (`useCurrency().currencyCode`) and return `currency` plus `fxQuality`; format the amounts with
  that response currency. Reports use `useReportMoney(report.reportCurrency)` (`money`,
  `signedMoney`, `nativeMoney` for native details) or `formatReportMoney`; other backend-computed
  pages use `useFormat().money(amount, response.currency)`. Converted wire amounts are decimal TEXT
  (nullable): `null` means "not convertible", shown as a dash plus the missing marker, never 0.
- **Native entities convert with `useCurrency().convert(amount, from, to?)`** (current snapshot,
  `to` defaults to the main currency) or the dated converters of `src/hooks/use-fx.ts`
  (`useDatedFx`, `useTransactionFx`, `useDatedMainConvert`; rate of the value's own day, common-day
  rule). Both return `null` without a rate: render the native amount or "—", exclude the item from
  the total and show the compact missing note (`fxReady` is false while quotes load; that is
  "pending", not "missing"). Never substitute 1:1, a hardcoded rate or another currency. Pure
  conversion lives in `shared/fx.ts` (`convert`, `convertOn`), the same rules as `services/fx.rs`.
- **`fmt.money(amount, currency, opts)` is the only money formatter.** Only the main currency gets
  its narrow symbol; every other currency shows its ISO code (`$` and `kr` are several
  currencies). `getFormatters(locale, mainCurrency)` requires the main currency; pass `undefined`
  explicitly where no profile context exists and every currency then shows its ISO code. Reports and
  their details show ISO 4217 minor units (`isoMinorUnits`, the table shared with the backend
  `reporting::minor_units`) so rows, totals and CSV reconcile: CZK 100.40 + 100.40 reads 200.80.
  Elsewhere `money()` keeps the cash style of `currencyDecimals` (CZK/HUF without decimals).
- **Main currency.** Only codes with a rate source (`FX_SUPPORTED_CODES`, `isFxSupported` from
  `shared/currencies.ts`) may be offered as the main currency; entity forms accept any selectable
  currency. Do not assume a currency in new defaults: scale constants through `scaledDefault`,
  default new records to the main currency, and use `recordCurrency(code)` only to label a stored
  record without a currency.
- **Query keys.** A query whose result depends on the report currency or on rates includes
  `currencyCode` and `fxRevision` (from `useCurrency()`), for example
  `["portfolio-metrics", currencyCode, fxRevision, excludePersonal]`. A profile currency change or
  a rate refresh then refetches it; never relabel an old response after a change. After a price or
  rate refresh call `invalidateAfterPriceRefresh(queryClient)` (`src/lib/price-refresh.ts`), which
  invalidates every price- and rate-dependent key, and add new such keys to its list.
- Everything else (native-currency amounts, plain numbers, percentages, dates) goes through
  `useFormat()` from `src/lib/format.ts`: `money`, `number`, `percent`, `day` (UTC calendar days —
  booking, transaction, maturity dates), `dateTime` and `time` (real instants), `date` (local `Date`
  objects), `month`. Output follows the **UI language**, never the currency's locale or the OS
  locale. ESLint (`no-restricted-syntax`) rejects `toLocaleString`/`toLocaleDateString`/
  `toLocaleTimeString` and `new Intl.NumberFormat`/`DateTimeFormat` outside that module. Pure
  utilities take a `locale` argument instead of calling `Intl` themselves.
- Chart axes: label days with `fmt.day`, thin the ticks with `evenTicks()` from
  `src/utils/chart-axis.ts` (at most 8, evenly spaced).

**Why:** hand-rolled conversion or formatting drifts from the rest of the app; a query that
ignores `fxRevision` shows values computed with stale rates next to fresh ones; an amount shown
without its currency is wrong for every user whose main currency differs from the author's.
