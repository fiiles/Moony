# Architecture Overview

System map for Moony: stack, data flow, canonical examples, and the domain map.

## Stack

- **Tauri 2** — Rust backend; the `#[tauri::command]` handlers live in `src-tauri/src/commands/` (`mod.rs` contains none).
- **React 19 / TypeScript** — frontend.
- **SQLCipher-encrypted SQLite** — single baseline `001_initial_schema` plus append-only migrations in `src-tauri/src/db/migrations.rs` (see `docs/architecture/database.md`).
- **wouter** — routing (not React Router).
- **TanStack React Query** — all server state; no Redux/Zustand.
- **shadcn/ui** (new-york) + **Tailwind** — UI components and styling.
- **react-i18next** — ten locales (en, cs, de, es, it, nb, pl, sv, ja, zh), one namespace per domain.
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
| categorization | `transaction_categories`, `categorization_rules`, `learned_payees` (+ `app_config` keys `rule_packs_*`) | `commands/categorization.rs` + `services/categorization/` — deterministic waterfall: own-account identity → custom rules → learned payees → locale rule packs (`packs.rs`, JSON under `src-tauri/resources/rules/`, gated by the `all_packs_are_valid` test; the `global` pack also carries language-generic keyword rules `global/kw-*` at priority 30, below every merchant rule); `apply.rs` is the definitive-only resolver used by the import write paths; `learn.rs` persists human corrections (manual picks and confirmed MCP suggestions — `bank_transactions.suggested_category_id`) as scoped rules (account, direction, identity or normalized name; DB commit before engine update — the broad `learn_payee` is test-only); learned payees are keyed by `tokenizer::normalize_payee_key` (reference numbers stripped) and IBANs by `normalize_iban` on both the learning and the matching side, and rows stored under older raw keys are normalized and de-duplicated on read (engine load, rules list), never migrated; `recategorize.rs` answers which payments a learned scope or a custom rule covers today (replayed per rule, all time), breaks them down by current category, applies a category to them with the layer's provenance label and restores them (undo); `engine.explain` names the deciding layer (own account, custom, learned, pack, none) so the ledger picker can say what a remembered correction rewrites — a custom rule with precedence is retargeted instead of shadowing a learned row. `own_accounts.rs` re-reads the user's own account identifiers (IBANs and digits-only keys) after every account create/update/delete. No ML. Category CRUD (create / rename / restyle / delete-with-reassign) lives in `services/categories.rs` behind `commands/categories.rs`; system `cat_*` categories can be renamed, never deleted. | `CategorizationRules.tsx`, `useCategorization.ts`, `CategorySelector.tsx`, `use-categories.ts`, `use-category-mutations.ts`, Settings → Rule Packs, Settings → Categories |
| budgeting | `budget_goals` | `commands/budgeting.rs` + `services/budgeting.rs` | `Budgeting.tsx`, `components/budgeting/` |
| stocks | `stock_investments`, `investment_transactions`, `stock_data`, `stock_price_overrides`, `dividend_data`, `dividend_overrides`, `stock_value_history`, `stock_tags`, `stock_investment_tags`, `stock_tag_groups`, `stock_import_batches` (+ `app_config` key `stock_import_formats`) | `commands/{investments,stock_tags,price_api,company_info,stock_import}.rs` + `services/{investments,price_api,pricing,positions,position_reports,cost_basis,ticker_history,history_recalc,company_info}.rs` — `positions.rs` values stock and crypto positions in a report currency (cost basis from `cost_basis.rs`, each transaction at its own day) for the UI and the MCP tools; `pricing.rs` is the single current-price resolver for stocks and crypto (a manual override wins until a newer API price arrives); the dashboard and the projection both use it. CSV import: `services/stock_import/` — `inspect` (layout detection, broker adapters under `adapters/`, gated by the fixture guard test; see "Adding a broker format" in `CONTRIBUTING.md`), `parse`, `simulate` (duplicates by broker id or values, holdings and currency checks; the single rule set behind `preview` and `import`), `import` (one recorded batch through the shared bulk writer `investments::bulk_create_stock_transactions`), `batches` (undo), `formats` (remembered mappings), `instruments` (Yahoo lookups) | `Stocks.tsx`, `StockDetail.tsx`, `StocksAnalysis.tsx`, `components/stocks/` (the import wizard in `components/stocks/import/`) |
| crypto | `crypto_investments`, `crypto_transactions`, `crypto_prices`, `crypto_price_overrides`, `crypto_value_history` | `commands/crypto.rs` + `services/{crypto,crypto_investments}.rs` | `Crypto.tsx`, `CryptoDetail.tsx` |
| bonds | `bonds` | `commands/bonds.rs` | `Bonds.tsx`, `use-bond*.ts` |
| loans | `loans`, `loan_events` — valued at the balance of the loan ledger (`loan_balance_at`: the annuity from the principal at the start date with the loan's extra payments and rate changes replayed; a manual balance record holds exactly until the user changes it), never at the static principal | `commands/loans.rs` + `services/{loans,loan_events,loan_amortization}.rs` (the maths is mirrored in `shared/calculations/loan-amortization.ts`, pinned by one fixture) | `Loans.tsx`, `LoanDetail.tsx` (repayment schedule), `AnnuityCalculator.tsx`, `src/utils/annuity.ts` |
| real estate | `real_estate` plus its child tables, `real_estate_valuations` | `commands/real_estate.rs` + `services/{real_estate,valuations}.rs` (property list/get/create/update and the valuation log — every property and other asset has an estimate from its creation; costs, photos, documents and links are still inline in the command file) | `RealEstate*.tsx`, `EstateCalculator.tsx`, `components/real-estate/`, `src/utils/valuation-trace.ts` |
| insurance | `insurance_policies`, `insurance_documents` | `commands/insurance.rs` | `Insurance*.tsx` |
| other assets | `other_assets`, `other_asset_transactions` | `commands/other_assets.rs` | `OtherAssets.tsx` |
| portfolio/net-worth | `portfolio_metrics_history` (native breakdowns), `stock_value_history`, `crypto_value_history` | `commands/portfolio.rs` (also hosts the FX commands) + `services/{net_worth,portfolio_history,history_days,history_plan,history_recalc,positions,position_reports}.rs` — net worth and its history are summed from native breakdowns into the requested report currency; `portfolio_history.rs` is the single writer of the snapshot table, `history_days.rs` derives every past day from dated facts and `history_recalc.rs` / `history_plan.rs` queue and plan the rebuilds (see "History recalculation"); positions are valued by `positions.rs` for the UI and the MCP tools alike | `Dashboard.tsx`, `SyncProvider.tsx`, `use-portfolio.ts`, `PortfolioValueTrendChart.tsx` |
| currency | `exchange_rates`, `exchange_rate_history` (units per 1 EUR; pegs derived, never stored), `app_config.fx_reference` | `get_fx_snapshot`, `get_fx_history`, `refresh_exchange_rates`, `get_price_status` inside `portfolio.rs` + `services/{fx,fx_backfill,currency}.rs` — `fx.rs` is the only conversion policy, `fx_backfill.rs` keeps rate history covering every held currency, `currency.rs` fetches ECB/Frankfurter rates and persists them (history rows keyed by the ECB reference date) | `src/lib/currency-provider.tsx` + `currency.ts` (`useCurrency()`), `src/hooks/use-fx.ts`, `shared/fx.ts`, `shared/fx-supported.json`, `shared/currencies.ts` |
| cashflow/projection | `cashflow_items`, `cashflow_plan_exclusions`, `projection_settings` | `commands/cashflow.rs` (planned report and actual flows via `services/cashflow_actuals.rs`), `commands/projection.rs` + `services/projection.rs` (report-currency projection; contributions stored in the currency typed) | `CashflowPlanning.tsx` + `components/cashflow/Plan*.tsx` (planned items as collapsible category groups, rows in or out of the plan), `Cashflow.tsx` (actual flows from bank transactions), `Projection.tsx` |
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
codes are never guessed. Other identifiers stay verbatim. A domestic number written as sort
code, routing number or BSB plus account is a second own-account identity next to IBANs
(`account_number_key`: digits only, 11 to 34 of them, never letters, never a Czech
`prefix-account`; matched whole or as account plus a bank-code column); a bare short number
(an 8-digit UK or 9-digit US account exists at many banks), an incomplete Czech number and
opaque foreign routing values cannot establish an own-transfer identity. Expense credits are negative
expenses (refunds), income debits are negative income (reversals); `signed_flow_amount`
keeps these signs. There are three roles: income, expense and transfer (money moved between
the user's own pots: investing, saving, account transfers). What the money is lives in the
category; the role only decides how it counts (migration 010 collapsed the former investment,
financing and adjustment roles into transfer).

Own transfers without an identifier are recognised by rule-pack wording (`global` for English
own-account and credit-card wording, country packs for dialect and issuer brands; all
`cat_internal_transfers`). Pack priorities form tiers, highest first: 60 brokers, 50 and up
merchants, 45 mortgage, interest and card-issuer-charge guards (`gb/cc-fees`, `kr/deposit-interest`),
44 own-transfer wording, 40 ATM, 30/31 generic `kw-*` keywords. So "Transfer to Vanguard" stays
an investment and "BARCLAYCARD ANNUAL FEE" is a fee, not a repayment (the UK issuer brands are
the one bare-brand exception, and exactly why their guards exist). Other wording must name an
action (transfer, deposit, repay), never a bare product word that also appears on interest,
fee or earnings lines; "card payment", direct debit, standing order, faster payment, Zelle and
Venmo are deliberately not own transfers. Matching runs on folded text (`fold_for_rules`:
`fold_text`, which decomposes Hangul and kana, plus `ø æ ł đ ß œ` written as `o ae l d ss oe`, so
packs spell `lonn` and `haevning` in ASCII) while a `regex` pattern is compiled as written apart
from those letters, so Hangul rules must be `contains` (both sides are folded, so a short Hangul
rule matches inside longer words: interest is matched as phrases such as `예금이자`, never as the
bare `이자`, which is in "이자카야" and "...페이자동...") and a regex may only contain characters
that fold to themselves (no diacritics, Hangul or voiced kana marks). Learned-payee keys keep
plain `fold_text`.

## Bootstrap & Auth

Provider nesting from `src/App.tsx` (outermost first):

```
QueryClientProvider → AuthProvider → I18nProvider → CurrencyProvider → SyncProvider
  → TooltipProvider → ErrorBoundary → UpdaterProvider → AppLayout (AppShell → Router)
```

- Auth states: `needs_setup | locked | unlocked`.
- `SyncProvider` refreshes stale prices 5 seconds after unlock (`setTimeout(runStartupSync, 5000)` in `src/hooks/SyncProvider.tsx`), then queues the history rebuild (`start_snapshot_backfill`, see "History recalculation"); it invalidates the relevant React Query caches when a rebuild batch completes.
- `UpdaterProvider` (`src/hooks/use-updater.tsx`) is the only updater: one automatic GitHub check per session, 5 seconds after unlock, only while the device-level setting `moony-auto-update-check` is on (Settings → Security → Updates); the badge, the update dialog and About all read it.
- Auto-lock: `useAutoLock()` (`src/hooks/use-auto-lock.ts`, mounted in `AppLayout`) locks the app after N minutes without input and on wake-up once the deadline has passed; the device-level setting is the `localStorage` key `moony-auto-lock-minutes` (off / 5 / 15 / 30 / 60, default 15), edited in Settings → Security (`AutoLockSlot`); deadline logic is pure and tested in `src/utils/auto-lock.ts`.

## History recalculation

Every past day of `portfolio_metrics_history` is derived from dated facts (ADR 0016); a write that changes a past day queues a rebuild and returns, and one background worker re-derives the days.

- **Queue.** `services/history_recalc.rs` holds three job kinds, each carrying the earliest affected day: `StockTicker` and `CryptoTicker` rebuild one ticker's value history from quotes, `Days` re-derives the portfolio days. The queue keeps one entry per key and the earliest date, so a bulk delete or an import costs one rebuild per ticker, and work queued during a run joins the next batch. Jobs dated today or later are dropped (today's row belongs to the live snapshot, `update_todays_snapshot`).
- **Worker.** One batch first extends the exchange-rate history over any missing range (`fx_backfill::ensure_fx_coverage`; no request when nothing is missing), then rebuilds the stock tickers (Yahoo), then the crypto tickers (CoinGecko, which serves 365 days; the id is read from `crypto_investments`), then re-derives the days from the earliest date of the whole batch, so a ticker job needs no separate `Days` job. Quotes are fetched without the database lock; `services/ticker_history.rs` plans a ticker's rows in one pass (running position, FX rates preloaded; about 0.09 s for a 630-day ticker, `cargo test --lib ticker_history::golden::benchmark -- --ignored --nocapture`). A failed ticker is logged and reported `failed` without stopping the days rebuild. Held tickers without a row in the batch window get a quote lookup too, but only a ticker held in that window (a position at its start, or a trade inside it): a holding sold out before it never costs a provider call. A batch that panics marks the queue idle again (`WorkerGuard`), so later rebuilds are not stalled.
- **`rebuild_days`** (`services/history_days.rs`) reads every fact once and writes one row per day from the earliest dated fact (`earliest_fact_day`, never before 2000-01-01) to yesterday, in 60-day chunks, each its own transaction and lock hold, with the worker yielding between chunks, so reads are served throughout. It is idempotent, deletes backfill rows before the first fact, and runs under one gate shared with the durable MCP worker. All writes go through `services/portfolio_history.rs`, the single writer of the table.
- **Class rules.** Savings take the newest balance record on or before the day and count nothing before the first one (an account without records is undated); loans are the ledger walk `loan_balance_at` (0 before the start; manual balance records hold); real estate counts from the purchase date at the newest valuation, and not at all without one; other assets are the quantity their transactions leave times that day's valuation (one with a quantity and no transactions is undated, like an account without records); a bond counts `coupon_value × quantity` from its purchase date on and nothing before it (one without a purchase date is undated, like an account without records); stocks and crypto come from the ticker histories. A backfill or new row gets all eight breakdowns, and what the facts do not date (undated items) is the "unknown remainder" of the nearest live row that counts the same items as the day, `recorded - derived` per class and currency, so a class does not jump between rebuilt and live days and no item is counted twice (the undated items at today's value only when no live row qualifies; a class the facts cover on a day takes no remainder; the code is `LiveRemainder` in `history_days.rs`). A live row is rewritten only for the classes the facts cover completely that day (undated accounts, undated other assets, undated bonds and the estimated past of an overridden loan are not), stocks and crypto always. A dated bond counts from its purchase date for the remainder rule too, so a live row before it does not hold it.
- **Triggers.** UI write commands (`bank_accounts`, `investments`, `stock_import`, `crypto`, `other_assets`, `real_estate`, `loans`, `bonds`, and a CoinGecko key change in `price_api`) queue their jobs through `portfolio::schedule_history_rebuild`. A write to a **static class** (bank accounts and balance records, real estate and valuations, loans and their events or manual balances, bonds, other assets with their transactions and valuations: create, edit, delete) always queues `Days(0)` (`history_plan::all_days`); `plan_rebuild` clamps it to the earliest fact, and a days-only rebuild is local and cheap. The day of the edit is no bound on the days it changes: a day the facts do not cover takes the remainder of a live row (`recorded - derived`), and `derived` moves with every item, so one change (a purchase date set on a property, a first balance record, a valuation) can change every earlier day (a rebuild from the edit's day left the old remainder on the days before it, a cliff on the purchase day). `services/history_plan.rs` only decides whether an edit touches a fact the history reads (a rename or a note queues nothing). Only `StockTicker` and `CryptoTicker` jobs, which cost a network fetch, keep the earliest affected day. MCP write tools record a durable follow-up (domain and earliest day, ADR 0014; a follow-up without a day is "from the first fact" and wins over dated ones queued next to it) and the worker in `commands/mcp_refresh.rs` runs it: tickers strictly, so a provider failure keeps the intent pending, then `rebuild_days_async`; the static domains (`other-assets`, `bank-accounts`, `real-estate`, `loans`, `bonds`) rebuild every day from the first fact whatever day the follow-up holds.
- **Startup.** `start_snapshot_backfill` (called by `SyncProvider` after unlock) only plans and queues, then returns. Below `app_config.history_rebuild_version` 2 it queues every ticker from its first transaction plus `Days` from the first fact; the version is stored once the days rebuild succeeded, whatever the tickers did (a ticker the provider keeps rejecting is retried by later writes, not by every start). Version 1 was the first rebuild from dated facts; version 2 repeats it for profiles that ran version 1, whose days a date-precise static trigger may have left on an old remainder. Otherwise it queues the held tickers and the days from the first missing day. `recalculate_all_portfolio_history` queues the same as the one-time rebuild.
- **Events.** `history-recalculation` `{ ticker, status: 'running' | 'done' | 'failed' }` per ticker (the header's `HistoryRecalculationBadge` shows a running rebuild; on done or failed it refetches `portfolio-history`, `ticker-history` and `stock-performance`), and after each finished batch `recalculation-complete`, on which the history charts and position views refetch. A days-only batch has no ticker, hence no status event.

## Logging

`log` macros go through `tauri-plugin-log` (`src-tauri/src/services/logging.rs`) to `moony.log` in the OS app-log directory — macOS `~/Library/Logs/com.moonyfinance.desktop/`, Windows `%LOCALAPPDATA%\com.moonyfinance.desktop\logs\`, Linux `~/.local/share/com.moonyfinance.desktop/logs/` — UTC timestamps, `info` in release / `debug` in debug builds, 2 MB per file with two dated archives kept. The `open_logs_folder` command reveals the folder; panics are logged too.

## Outbound HTTP

ECB rates, Frankfurter history and CoinGecko go through one shared `reqwest::Client` (`src-tauri/src/services/http.rs`: 15 s connect + total timeout, `User-Agent: Moony/<version>`). Yahoo Finance uses the `yahoo_finance_api` connector, built with the same 15 s bound in `services/price_api.rs`. Never call `reqwest::get` (no timeout).

## Known Structural Debt

Fat command files hold logic that belongs in services (link, don't fix):

- `portfolio.rs`
- `projection.rs`
- `real_estate.rs` (costs, photos, documents and links)
- `cashflow.rs`

See `docs/DEPRECATED.md` for patterns not to copy.

Actual cashflow and budgeting use `services/reporting.rs` for the same booked ledger rows, dynamic own-account recognition, explicit flow overrides, signed refunds/reversals and strict recorded historical FX. Missing FX rows remain visible with a null contribution in the report currency and quality counts; they do not enter numeric totals. SQL read failures are errors. Report account scope is independent of portfolio exclusion. `netBalance` describes ordinary income less expense; `bankCashMovement` additionally includes signed transfers. Cashflow reports one calendar period (month, quarter or year) and groups income, expenses and transfers by category; uncategorized rows form their own group per role. Its chart window is the twelve months ending with the period's last month.

Observed completed months are a sample, not evidence of full statement coverage. Cashflow averages exclude partial/unobserved months and months with missing FX; the response names the actual average sample months. There is no comparison period. Planning remains a monthly/yearly equivalent estimate with its as-of date and current-FX quality; it uses loan amortization to omit repaid/expired loans and supports explicit replacement sources.

## Money flow review

The account ledger uses server filters and stable pages, while native summaries and balance estimates traverse every page and exclude pending or foreign-unit rows. Transaction details retain statement provenance and use the server effective-flow resolver. Cashflow and budgets share account scope, the period control, historical report-currency contributions, quality notes and counted drilldowns. The drilldown dialog recategorizes rows in place: it keeps the list it opened with (its query sits outside the refresh keys) and marks rows that left the line as moved while the report refreshes.

`src/lib/money-flow-refresh.ts` refreshes transaction/report/goal/drilldown domains and records a portfolio snapshot after writes, including external bank events. Category assignment awaits persistence. Learning is a separate opt-in for one account and direction, committed before changing engine memory; accepting a suggestion or picking a CSV category alone learns nothing. New scoped rows coexist with clearly labeled legacy broad learned rules.

Budget edits preserve original money text when unchanged and select an effective month. Report limits use historical versions, while edit/delete actions refer only to current goal IDs. Planning labels monthly-equivalent estimates and exposes explicit replacement links to automatic sources.

Ledger effective meaning stays unresolved while its server read is loading, missing a row or failing; failure exposes retry. Budget actuals consume every server-resolved expense category independent of fixed category IDs. The uncategorized expense drilldown uses a dedicated booked expense count (including missing-FX rows), preserving access when debit/refund contributions cancel without counting uncategorized income.
