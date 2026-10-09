# Architecture Overview

System map for Moony: stack, data flow, canonical examples, and the domain map.

## Stack

- **Tauri 2** — Rust backend; the `#[tauri::command]` handlers live in `src-tauri/src/commands/` (`mod.rs` contains none).
- **React 19 / TypeScript** — frontend.
- **SQLCipher-encrypted SQLite** — single baseline `001_initial_schema` plus append-only migrations in `src-tauri/src/db/migrations.rs` (see `docs/architecture/database.md`).
- **wouter** — routing (not React Router).
- **TanStack React Query** — all server state; no Redux/Zustand.
- **shadcn/ui** (new-york) + **Tailwind** — UI components and styling.
- **react-i18next** — en/cs locales, one namespace per domain.
- **sonner** — toasts.

## The Sacred Data Flow

Every feature follows this path — no layer-skipping:

```
UI component (src/pages/, src/components/<domain>/)
  → hook: src/hooks/use-<domain>.ts (useQuery) + use-<domain>-mutations.ts (useMutation)
    → src/lib/tauri-api.ts — <domain>Api namespace (the only place app commands are invoked)
      → #[tauri::command] in src-tauri/src/commands/<domain>.rs (thin)
        → business logic in src-tauri/src/services/<domain>.rs (takes &Connection)
          → SQLCipher SQLite
```

To see the flow end to end, read the bonds domain top to bottom: `src/pages/Bonds.tsx` → `src/hooks/use-bonds.ts` + `use-bond-mutations.ts` → `bondsApi` in `src/lib/tauri-api.ts` → `src-tauri/src/commands/bonds.rs` (minimal CRUD, so the command talks to SQLite directly). For the cleanest command → service split, read budgeting (`commands/budgeting.rs` → `services/budgeting.rs`) — backend only: its UI calls `budgetingApi` via inline `useQuery`/`useMutation` instead of dedicated hooks, which is exactly the layer-skipping this diagram forbids.

## Canonical Examples to Copy

When building something new, copy these — never the outliers:

- **Thin command + service + tests:** `src-tauri/src/commands/budgeting.rs` + `src-tauri/src/services/budgeting.rs` (`#[cfg(test)]` at the bottom).
- **Minimal CRUD domain:** `src-tauri/src/commands/bonds.rs` + `src/hooks/use-bonds.ts` + `use-bond-mutations.ts`.
- **Mutation hook with correct cache invalidation + snapshot:** `src/hooks/use-bank-account-mutations.ts`.
- **Dialog/form page pattern:** `src/pages/Loans.tsx` (lifted dialog state via `useState`) + `src/components/loans/LoanFormDialog.tsx` (react-hook-form + zodResolver).

## Domain Map

| Domain | Tables | Rust | Frontend |
|---|---|---|---|
| auth/profile | `app_config`, `user_profile` | `commands/auth.rs` + `services/auth.rs` | `use-auth.tsx`, `auth-page.tsx`, `Settings.tsx` (frame; one section per `/settings/*` route — general, security, data, integrations, categorization, account — with the cards in `components/settings/`), `use-settings-mutations.ts` |
| bank accounts | `institutions`, `bank_accounts`, `bank_account_zones`, `bank_transactions`, `csv_import_batches` | `commands/bank_accounts.rs` + `services/{bank_accounts,date_parser}.rs` + CSV import: `services/csv_import/` (encoding/delimiter/header-row detection, multilingual column patterns, amount + date-format consensus, `parse_rows` shared by `preview_import` and `import_transactions`) and bank formats as JSON presets under `src-tauri/resources/csv-presets/` (`services/csv_presets.rs`, gated by the `all_presets_are_valid` test; see "Adding a bank preset" in `CONTRIBUTING.md`) | `BankAccounts.tsx`, `BankAccountDetail.tsx`, `use-bank-account*.ts`, `components/bank-accounts/` |
| categorization | `transaction_categories`, `categorization_rules`, `learned_payees` (+ `app_config` keys `rule_packs_*`) | `commands/categorization.rs` + `services/categorization/` — deterministic waterfall: own-account identity → custom rules → learned payees → locale rule packs (`packs.rs`, JSON under `src-tauri/resources/rules/`, gated by the `all_packs_are_valid` test; the `global` pack also carries language-generic keyword rules `global/kw-*` at priority 30, below every merchant rule); `apply.rs` is the definitive-only resolver used by the import write paths; `learn.rs` persists human corrections (manual picks and confirmed MCP suggestions — `bank_transactions.suggested_category_id`) as scoped rules (account, direction, identity or normalized name; DB commit before engine update — the broad `learn_payee` is test-only); learned payees are keyed by `tokenizer::normalize_payee_key` (reference numbers stripped) and IBANs by `normalize_iban` on both the learning and the matching side, and rows stored under older raw keys are normalized and de-duplicated on read (engine load, rules list), never migrated; `own_accounts.rs` re-reads the user's own IBANs after every account create/update/delete. No ML. Category CRUD (create / rename / restyle / delete-with-reassign) lives in `services/categories.rs` behind `commands/categories.rs`; system `cat_*` categories can be renamed, never deleted. | `CategorizationRules.tsx`, `useCategorization.ts`, `CategorySelector.tsx`, `use-categories.ts`, `use-category-mutations.ts`, Settings → Rule Packs, Settings → Categories |
| budgeting | `budget_goals` | `commands/budgeting.rs` + `services/budgeting.rs` | `Budgeting.tsx`, `components/budgeting/` |
| stocks | `stock_investments`, `investment_transactions`, `stock_data`, `stock_price_overrides`, `dividend_data`, `dividend_overrides`, `stock_value_history`, `stock_tags`, `stock_investment_tags`, `stock_tag_groups`, `stock_import_batches` (+ `app_config` key `stock_import_formats`) | `commands/{investments,stock_tags,price_api,company_info,stock_import}.rs` + `services/{investments,price_api,pricing,positions,position_reports,cost_basis,ticker_history,history_recalc,company_info}.rs` — `positions.rs` values stock and crypto positions in a report currency (cost basis from `cost_basis.rs`, each transaction at its own day) for the UI and the MCP tools; `pricing.rs` is the single current-price resolver for stocks and crypto (a manual override wins until a newer API price arrives); the dashboard and the projection both use it. CSV import: `services/stock_import/` — `inspect` (layout detection, broker adapters under `adapters/`, gated by the fixture guard test; see "Adding a broker format" in `CONTRIBUTING.md`), `parse`, `simulate` (duplicates by broker id or values, holdings and currency checks; the single rule set behind `preview` and `import`), `import` (one recorded batch through the shared bulk writer `investments::bulk_create_stock_transactions`), `batches` (undo), `formats` (remembered mappings), `instruments` (Yahoo lookups) | `Stocks.tsx`, `StockDetail.tsx`, `StocksAnalysis.tsx`, `components/stocks/` (the import wizard in `components/stocks/import/`) |
| crypto | `crypto_investments`, `crypto_transactions`, `crypto_prices`, `crypto_price_overrides`, `crypto_value_history` | `commands/crypto.rs` + `services/{crypto,crypto_investments}.rs` | `Crypto.tsx`, `CryptoDetail.tsx` |
| bonds | `bonds` | `commands/bonds.rs` | `Bonds.tsx`, `use-bond*.ts` |
| loans | `loans` — valued at the amortized outstanding balance (annuity from the principal at the start date, or from the user's manual balance anchor), never at the static principal | `commands/loans.rs` + `services/{loans,loan_amortization}.rs` (the maths is mirrored in `shared/calculations/loan-amortization.ts`, pinned by one fixture) | `Loans.tsx`, `LoanDetail.tsx` (repayment schedule), `AnnuityCalculator.tsx`, `src/utils/annuity.ts` |
| real estate | `real_estate` plus its child tables, `real_estate_valuations` | `commands/real_estate.rs` + `services/{real_estate,valuations}.rs` (property list/get/create/update and the valuation log — every property and other asset has an estimate from its creation; costs, photos, documents and links are still inline in the command file) | `RealEstate*.tsx`, `EstateCalculator.tsx`, `components/real-estate/`, `src/utils/valuation-trace.ts` |
| insurance | `insurance_policies`, `insurance_documents` | `commands/insurance.rs` | `Insurance*.tsx` |
| other assets | `other_assets`, `other_asset_transactions` | `commands/other_assets.rs` | `OtherAssets.tsx` |
| portfolio/net-worth | `portfolio_metrics_history` (native breakdowns), `stock_value_history`, `crypto_value_history` | `commands/portfolio.rs` (also hosts the FX commands) + `services/{net_worth,portfolio_history,positions,position_reports}.rs` — net worth and its history are summed from native breakdowns into the requested report currency; positions are valued by `positions.rs` for the UI and the MCP tools alike | `Dashboard.tsx`, `SyncProvider.tsx`, `use-portfolio.ts`, `PortfolioValueTrendChart.tsx` |
| currency | `exchange_rates`, `exchange_rate_history` (units per 1 EUR; pegs derived, never stored), `app_config.fx_reference` | `get_fx_snapshot`, `get_fx_history`, `refresh_exchange_rates`, `get_price_status` inside `portfolio.rs` + `services/{fx,fx_backfill,currency}.rs` — `fx.rs` is the only conversion policy, `fx_backfill.rs` keeps rate history covering every held currency, `currency.rs` fetches ECB/Frankfurter rates and persists them (history rows keyed by the ECB reference date) | `src/lib/currency-provider.tsx` + `currency.ts` (`useCurrency()`), `src/hooks/use-fx.ts`, `shared/fx.ts`, `shared/fx-supported.json`, `shared/currencies.ts` |
| cashflow/projection | `cashflow_items`, `projection_settings` | `commands/cashflow.rs` (planned report and actual flows via `services/cashflow_actuals.rs`), `commands/projection.rs` + `services/projection.rs` (report-currency projection; contributions stored in the currency typed) | `CashflowPlanning.tsx` (planned items), `Cashflow.tsx` (actual flows from bank transactions), `Projection.tsx` |
| stock monitor | `watched_stocks` ⋈ `stock_data`; never enters net worth | `commands/stock_monitor.rs` + `services/stock_monitor.rs` | `StockMonitor.tsx`, `StockMonitorDetail.tsx`, `use-stock-monitor*.ts`, `components/stock-monitor/` |
| milestones | `milestone_states` (done/snoozed occurrences); reads `insurance_policies`, `loans`, `bonds`, `bank_accounts`, `real_estate` (+ `real_estate_valuations`), `watched_stocks` ⋈ `stock_data`, `user_profile` and `app_config` (last backup); reads and writes `app_config` `milestones.mutedKinds` (the kinds the user turned off); never enters net worth | `commands/milestones.rs` + `services/milestones/` — one builder per source domain (`insurance`, `loans`, `bonds`, `accounts`, `watchlist`, `upkeep`), `mod.rs` stages dated items, drops done/snoozed occurrences and, in `list_milestones`, the muted kinds, and orders the list | `Dashboard.tsx` (`components/dashboard/UpcomingCard.tsx`), `components/shell/MilestonesStatus.tsx`, `components/milestones/MilestoneRows.tsx`, `components/settings/MilestoneSettingsCard.tsx`, `use-milestones.ts`, `use-milestone-mutations.ts`, `src/utils/milestones.ts` |
| backup / data | — (files in the app data dir: `moony.db`, `salt`, `key.enc`, `recovery.enc`, attachment dirs) | `commands/backup.rs` + `services/backup.rs` — zip backup (`VACUUM INTO` snapshot), inspect/restore with `pre-restore-<ts>/` safety copy, full JSON export, `PRAGMA quick_check` | `components/settings/DataSection.tsx` (`dataApi`) |
| export/local API | — | `commands/export.rs`; `services/local_api.rs` + `services/mcp/` — embedded MCP server (rmcp Streamable HTTP at `/mcp`, fixed port, stable token), off by default, enabled via `user_profile.mcp_server_enabled` + write tools (ADR 0007); aggregates are computed by the same services as the UI (`net_worth`, `positions`, `position_reports`, `ticker_history`) in the user's main currency with a `mainCurrency` field (`services/mcp/money.rs`); amounts that cannot be converted are `null` and named in `missingCurrencies` (`fxEstimated` when a cost used today's rate), history rows convert at their day's rate, and `exchange_rates_list` quotes against an explicit `baseCurrency`; the `cashflow_report` and `budgeting_report` tools are built on `cashflow_planning::get_report` and `budgeting::get_report`, so the assistant sees the same money semantics (booked only, signed refunds, flow resolver, limit versions, FX quality) as the app | — |

## Currencies and conversion (ADR 0012)

Native amount, currency and date are the stored facts; there is no base currency and no stored
converted total. A monetary command takes the report currency (the main currency, chosen by
the frontend) and returns it as `currency` with an `fxQuality` (`missingCurrencies`,
`missingCount`, `estimated`); converted amounts are decimal TEXT at the currency's ISO minor
unit, `null` when not convertible.

- **One policy.** `services/fx.rs` converts: same currency is identity without a quote; a current
  conversion uses one `Quotes` snapshot; a historical one (`History::convert_on`) uses one UTC day
  that quotes both currencies, on or before the requested day, at most ten days back. `shared/fx.ts`
  mirrors it for the frontend and both are pinned by `shared/fx-test-vectors.json`.
- **EUR reference.** `exchange_rates` and `exchange_rate_history` hold units per 1 EUR; the
  reference is internal to `fx.rs`. AED, SAR, QAR, BHD, OMR (per USD) and BGN (per EUR) are derived
  at read time from `shared/fx-supported.json` and never stored. Source quotes plus derived codes are
  the 36 currencies allowed as the main currency; entity currencies stay free.
- **Missing is explicit.** An item that cannot be converted stays visible natively, the totals
  leave it out and name its currency; nothing is valued 1:1. `estimated` marks a cost that used
  today's rate because its own day had no common quote.
- **Frontend.** `useCurrency()` exposes the main currency, the snapshot `fx`, `fxRevision`, `fxReady`
  and `convert` (`null` without a rate); `useFxSnapshot` / `useFxHistory` (`src/hooks/use-fx.ts`)
  load quotes. Backend-computed pages format the response currency; pages that sum native entities
  convert with `convert`. Query keys of converted data carry the currency and `fxRevision`;
  `invalidateAfterPriceRefresh` (`src/lib/price-refresh.ts`) refreshes every price- and
  rate-dependent query after a refresh.

## Bank flow semantics

`services/financial_semantics.rs` shares account identity and meaning across categorization
and actual reports. Load `AccountIdentities::load(conn)` for each report so adding, editing
or deleting an own account changes automatic recognition of existing transactions.
Precedence is an explicit transaction `flowKind`, then known own account, then category
`kind`, then credit/income or debit/expense. Category-only updates preserve `flowKind`;
setting it to null restores automatic derivation. Own-account detection precedes merchant,
country and learned categorization rules. It never uses equal amounts or personal names.

Valid IBANs normalize whitespace/case. Valid Czech domestic identifiers require a bank
code, and normalize to their IBAN identity (including padded prefix/account and national
checksums). Optional structured bank codes can complete domestic identifiers; missing
codes are never guessed. Other identifiers stay verbatim; incomplete bare domestic and
opaque foreign routing values cannot establish an own-transfer identity. Expense credits are negative
expenses (refunds), income debits are negative income (reversals); `signed_flow_amount`
keeps these signs. Investment, financing, adjustment and transfer retain separate roles.

## Bootstrap & Auth

Provider nesting from `src/App.tsx` (outermost first):

```
QueryClientProvider → AuthProvider → I18nProvider → CurrencyProvider → SyncProvider
  → TooltipProvider → ErrorBoundary → UpdaterProvider → AppLayout (AppShell → Router)
```

- Auth states: `needs_setup | locked | unlocked`.
- `SyncProvider` runs a portfolio history backfill 5 seconds after unlock (`setTimeout(runStartupSync, 5000)` in `src/hooks/SyncProvider.tsx`), then invalidates the relevant React Query caches.
- `UpdaterProvider` (`src/hooks/use-updater.tsx`) is the only updater: one automatic GitHub check per session, 5 seconds after unlock, only while the device-level setting `moony-auto-update-check` is on (Settings → Security → Updates); the badge, the update dialog and About all read it.
- Auto-lock: `useAutoLock()` (`src/hooks/use-auto-lock.ts`, mounted in `AppLayout`) locks the app after N minutes without input and on wake-up once the deadline has passed; the device-level setting is the `localStorage` key `moony-auto-lock-minutes` (off / 5 / 15 / 30 / 60, default 15), edited in Settings → Security (`AutoLockSlot`); deadline logic is pure and tested in `src/utils/auto-lock.ts`.

## History recalculation

A historical stock transaction (add, edit, delete, import, also the MCP-import trigger) changes the position on every later day, so the ticker's `stock_value_history` rows and the stocks class of `portfolio_metrics_history` are rebuilt from the earliest affected date. The write commands in `commands/investments.rs` only write their rows, queue the ticker through `portfolio::schedule_stock_history_rebuild` and return. `services/history_recalc.rs` owns the queue — one entry per ticker holding its earliest date, so a bulk delete or an import costs one rebuild per ticker and work queued during a run joins the next batch — and one worker drains it: quotes are fetched without the database lock, `services/ticker_history.rs` plans the rows in a single pass (running position, FX rates preloaded; golden-tested against the previous per-day implementation), and the rows plus the portfolio aggregate (`portfolio_history::aggregate_ticker_history`, one pass over all tickers) are written in 60-day chunks, each its own transaction and lock hold, so reads are served throughout. The worker emits `history-recalculation` `{ ticker, status: 'running' | 'done' | 'failed' }` (the header's `HistoryRecalculationBadge`; on done or failed it refetches `portfolio-history`, `ticker-history` and `stock-twr`) and, after each finished batch, the established `recalculation-complete` for the trend charts. One rebuild of a 630-day ticker in a 21-ticker book takes about 0.09 s instead of 1.3 s (debug build, `cargo test --lib ticker_history::golden::benchmark -- --ignored --nocapture`); the baseline schema indexes `portfolio_metrics_history(recorded_at)`. Crypto history still rebuilds synchronously (`recalculate_crypto_ticker_history`).

## Logging

`log` macros go through `tauri-plugin-log` (`src-tauri/src/services/logging.rs`) to `moony.log` in the OS app-log directory — macOS `~/Library/Logs/com.filipkral.moony/`, Windows `%LOCALAPPDATA%\com.filipkral.moony\logs\`, Linux `~/.local/share/com.filipkral.moony/logs/` — UTC timestamps, `info` in release / `debug` in debug builds, 2 MB per file with two dated archives kept. The `open_logs_folder` command reveals the folder; panics are logged too.

## Outbound HTTP

ECB rates, Frankfurter history and CoinGecko go through one shared `reqwest::Client` (`src-tauri/src/services/http.rs`: 15 s connect + total timeout, `User-Agent: Moony/<version>`). Yahoo Finance uses the `yahoo_finance_api` connector, built with the same 15 s bound in `services/price_api.rs`. Never call `reqwest::get` (no timeout).

## Known Structural Debt

Fat command files hold logic that belongs in services (link, don't fix):

- `portfolio.rs`
- `projection.rs`
- `real_estate.rs` (costs, photos, documents and links)
- `cashflow.rs`

See `docs/DEPRECATED.md` for patterns not to copy.

Actual cashflow and budgeting use `services/reporting.rs` for the same booked ledger rows, dynamic own-account recognition, explicit flow overrides, signed refunds/reversals and strict recorded historical FX. Missing FX rows remain visible with a null contribution in the report currency and quality counts; they do not enter numeric totals. SQL read failures are errors. Report account scope is independent of portfolio exclusion. `netBalance` describes ordinary income less expense; `bankCashMovement` additionally includes signed transfer, investment, financing and adjustment flows. Cashflow exposes these separated flows for both totals and individual months.

Observed completed months are a sample, not evidence of full statement coverage. Cashflow averages exclude partial/unobserved months and months with missing FX; the response names the actual average sample months. Current partial 6/12-month ranges compare to equally partial calendar periods; YTD compares the same calendar date in the previous year, clamping February 29 to February 28. Planning remains a monthly/yearly equivalent estimate with its as-of date and current-FX quality; it uses loan amortization to omit repaid/expired loans and supports explicit replacement sources.

## Money flow review

The account ledger uses server filters and stable pages, while native summaries and balance estimates traverse every page and exclude pending or foreign-unit rows. Transaction details retain statement provenance and use the server effective-flow resolver. Cashflow and budgets share account scope, historical contributions in the report currency, quality notes and counted drilldowns.

`src/lib/money-flow-refresh.ts` refreshes transaction/report/goal/drilldown domains and records a portfolio snapshot after writes, including external bank events. Category assignment awaits persistence. Learning is a separate opt-in for one account and direction, committed before changing engine memory; accepting a suggestion or picking a CSV category alone learns nothing. New scoped rows coexist with clearly labeled legacy broad learned rules.

Budget edits preserve original money text when unchanged and select an effective month. Report limits use historical versions, while edit/delete actions refer only to current goal IDs. Planning labels monthly-equivalent estimates and exposes explicit replacement links to automatic sources.

Ledger effective meaning stays unresolved while its server read is loading, missing a row or failing; failure exposes retry. Budget actuals consume every server-resolved expense category independent of fixed category IDs. The uncategorized expense drilldown uses a dedicated booked expense count (including missing-FX rows), preserving access when debit/refund contributions cancel without counting uncategorized income.
