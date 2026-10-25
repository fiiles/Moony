# DEPRECATED — Do Not Copy

Things that exist in this repo but must never be copied, extended, or used as
examples. Before reusing any pattern, check this list.

## Dead frontend code

| Path | Why it's dead | Use instead |
|---|---|---|
| Any shadcn primitive in `src/components/ui/` without importers | Never imported — harmless, but do **not** take their presence as evidence the codebase uses them | Check actual imports before citing a component as "used" |
| `src/utils/estimated-series.ts` (+ its test), `src/components/dashboard/EstimatedLegend.tsx` | 0 importers. They drew `source === 'backfill'` days dashed, a convention from when backfill rows carried static classes from a live row; backfill rows are now derived from dated facts (ADR 0016) and every history day is drawn alike | Nothing; do not wire them back in without a new decision |

## Removed from the frontend (do not re-add)

Deleted from the frontend (0 importers, English-only strings, or arithmetic bugs). They are
listed so nobody resurrects them from git history or copies them as examples.

| Removed | Why it was dead | Use instead |
|---|---|---|
| `src/hooks/use-finance-data.ts` | Express-era React Query keys (`["/api/instruments"]`); nothing serves these routes since the Tauri migration | Domain hooks (`use-bank-accounts*.ts`, `use-investments.ts`, …) backed by `src/lib/tauri-api.ts` |
| `src/hooks/use-stocks.ts` (`useInvestments`) | Legacy instruments support; mixed native-currency average price with CZK current price | Current investments hooks |
| `src/hooks/use-instrument-mutations.ts` | Legacy instruments support | Current investments hooks |
| `src/components/legacy/` (`DeleteInstrumentDialog.tsx`, `InstrumentFormDialog.tsx`) | Imported nowhere | The live investments dialogs |
| `src/components/dashboard/{AssetAllocationChart,NetWorthSummary,RecentTransactions,StockPortfolioCard,WealthTrendChart}.tsx` | Imported nowhere, hardcoded English | `AssetAllocationDonut`, `NetWorthTrendChart`, `AssetClassCard`, `AssetClassTrendChart` |
| `src/components/insurance/{InsuranceCard,InsuranceDetailModal}.tsx` | Imported nowhere, hardcoded English | `InsuranceList`, `pages/InsuranceDetail.tsx` |
| `src/components/common/ThemeToggle.tsx` | Imported nowhere | — |
| Savings page: `src/pages/Accounts.tsx` (route `/accounts`), `src/hooks/use-savings-accounts.ts`, `src/hooks/use-savings-account-mutations.ts`, `src/components/savings/{SavingsAccountCard,SavingsAccountFormDialog,SavingsAccountsSummary,SavingsAccountsTable,SavingsAccountZonesModal,DeleteSavingsAccountDialog}.tsx` | Parallel implementation of `BankAccounts` over the same tables; only reachable by typing the URL | `src/pages/BankAccounts.tsx`, `use-bank-account*.ts` hooks, `components/bank-accounts/` |
| `src/components/bank-accounts/ZonesInfoModal.tsx` | Unused since the zones moved to the detail page; replaced by the Úročení modal | `src/components/bank-accounts/InterestDialog.tsx` |

## Removed in the redesign sweep (do not re-add)

Deleted when the UI was rebuilt on the design system (`docs/design-system/README.md`). The design-system
primitives cover every case they handled.

| Removed | Why it was dead | Use instead |
|---|---|---|
| `src/components/common/{SummaryCard,StatCard,TickerValueTrendChart,TableSearchInput}.tsx` | 0 importers after the page rebuilds | `HeroCard` / `HeroValue`, `Stat` / `Stats`, `MoonyLineChart`, `InputWrap icon=` |
| `src/components/common/IconButton.tsx` | Wrapped a shadcn tooltip around every icon button; the design system names icon buttons with `aria-label` only | `<Button variant="ghost" size="icon-sm" aria-label=…>` (`variant="danger"` for delete) |
| `src/components/common/InputWithSuffix.tsx` | Duplicate of the `.input-wrap` unit adornment | `<InputWrap unit="%"><Input … /></InputWrap>` |
| Legacy shadcn colour names (`bg-background`, `text-foreground`, `text-muted-foreground`, `bg-muted`, `text-primary`, `bg-card`, `text-destructive`, `text-positive/negative`, `border-border`, `chart-1…8`, `rounded-lg/md/sm`) and their HSL variables in `src/index.css` / `tailwind.config.ts` | Transition mapping for not-yet-restyled pages; the last page is restyled | The tokens in `docs/design-system/README.md` §2 (`bg-canvas`, `text-ink(-2/-3/-4/-5)`, `bg-well`, `text-gain`, `text-loss`, `border-line`, `rounded-r1…r5`, `s1…s4`) |
| `.page-title`, `.page-subtitle`, `card-hover` CSS roles | Interim roles for the transition | `PageHead`, the card's own hover state |
| Per-component `AlertDialog` delete confirmations | Each dialog restated the same two buttons and the red button class | `ConfirmDeleteDialog` (destructive settings dialogs keep `AlertDialog` with the solid `AlertDialogAction`) |

## Removed in the overview polish (do not re-add)

| Removed | Why it was dead | Use instead |
|---|---|---|
| `investmentsApi.refreshMetadata` (`refresh_stock_metadata`) | Invoked a command that never existed | `investmentsApi.getCompanyInfo(ticker, refresh)` (`get_stock_company_info`) |
| Company metadata on `StockInvestmentWithPrice` (`sector`, `peRatio`, `marketCap`, …) | Typed but never sent by `get_investment`, so the stock detail showed dashes | `StockCompanyInfo` from `getCompanyInfo` |
| `PortfolioTrendCard` `transactionMarkers` / `TransactionMarker` | Only its earliest date was used, and its default array changed the query key every render (a request loop) | `earliest` (first day with data) and `chartPeriodStart` |
| `AddInvestmentModal` `onImportCsv` | Import moved to the stocks page head | The page-head "Importovat CSV" button |

## Removed with the stock import wizard (do not re-add)

The one-table stock import was replaced by the broker-aware wizard
(`docs/specs/2026-10-04-stock-csv-import-wizard-design.md`).

| Removed | Why it was dead | Use instead |
|---|---|---|
| `src/components/stocks/ImportInvestmentsModal.tsx` | Six hand-picked columns, no presets, no preview, no duplicate check, no undo; split the CSV in the webview with a naive parser, guessed the date format per row, and stored numbers such as `1,234.50` as text that later read as 0 | `src/components/stocks/import/StockImportDialog.tsx` over `src-tauri/src/services/stock_import/` |
| `import_investment_transactions` (command), `investmentsApi.importTransactions`, the `stock-import-progress` event | Wrote row by row without a transaction, returned English error prose with row numbers that did not match the file, and skipped part of the mutation contract | `import_stock_csv` (`commands/stock_import.rs`), `stockImportApi.import`, `src/hooks/use-stock-import-mutations.ts` |
| `stocks.import.*` locale keys | Orphaned with the modal | `stocks.importWizard.*` |
| `src/components/auth/LanguageSegment.tsx`, the language tiles of `LanguageCard`, `LANGUAGE_NAMES` | One segment or tile per language does not fit ten languages | `src/components/common/LanguageSelect.tsx`; names and locales from `src/i18n/languages.ts` |

## Retired statement balance path

The CSV import `balanceUpdatePayload` helper and the later `setBalanceAnchor` / `set_bank_account_balance_anchor` command are gone. A balance is recorded with `bankAccountsApi.recordBalance` (`record_bank_account_balance`): every call is a dated row in `bank_balance_records`, the statement path passes `sourceBatchId`, and `confirmUnknownAsOf` is needed only while the account has no dated record and an undated nonzero balance. `update_bank_account` ignores `balance`/`balanceAsOf`; the detail page no longer reconstructs a balance line from transactions (`reconstructBalance`, `significantMovements`, `movementThreshold` were removed with it).

## Removed with explicit currencies (do not re-add)

Implicit-CZK machinery deleted once every page and command declared its currency
(ADR 0012, building on ADR 0011; `docs/plans/2026-10-09-multi-currency.md`). It converted through a global CZK
table, valued unknown currencies 1:1 and returned unlabelled CZK numbers.

| Removed | Why it was removed | Use instead |
|---|---|---|
| `useCurrency().formatCurrency`, `formatCurrencySigned`, `formatCurrencyShort`, `formatCurrencyRaw`, `formatPrice` | Assumed a CZK input and converted it to the display currency with the frontend rate table | `useFormat().money(amount, currency, opts)` with the amount's own currency; reports: `useReportMoney(report.reportCurrency)` / `formatReportMoney` |
| CZK-based `useCurrency().convert(amount, from, to)` (1:1 for unknown codes) and `ratesTimestamp` | Silent 1:1 fallback; refetch key tied to the old table | `useCurrency().convert(amount, from, to?)` (the former `fxConvert`, `null` without a rate) and `fxRevision` |
| `shared/currencies.ts`: `EXCHANGE_RATES`, `BUILT_IN_RATES`, `updateExchangeRates`, `hasKnownRate`, `convertToCzK`, `convertFromCzK`, `BASE_CURRENCY`, `ECB_CURRENCY_CODES`, `isEcbCurrency` | Mutable global CZK rate table with hardcoded fallback rates | `shared/fx.ts` (`convert`, `convertOn`) over `useFxSnapshot()` / `useFxHistory()`; `isFxSupported` / `FX_SOURCE_CODES` for "has a rate source" |
| `createFormatters(locale)` / `getFormatters(locale)` without a main currency (narrow symbol for every currency) | "$" and "kr" are several currencies | `getFormatters(locale, mainCurrency)` — required; `undefined` only where no profile context exists (every currency then shows its ISO code) |
| `src/hooks/use-dated-convert.ts` (`useDatedConvert`) | Nearest-day-with-any-quote lookups that fell back to today's rates | `useFxHistory()` + `convertOn` (common-day rule) or backend responses converted per day |
| `shared/calculations/{change-metrics,crypto-metrics,holding-metrics,portfolio-metrics,cost-basis}.ts`, `calculateYieldPercentOnCost/OnMarket` | No callers after the pages moved to backend reports | Backend `services/positions.rs`, `services/cost_basis.rs`, `services/net_worth.rs` responses |
| `resolveAssumptions`, `summarizeAssumptions` (`src/utils/projection-assumptions.ts`) | Test-only after the projection page moved to the backend response | `get_projection` response (`fxQuality`, class values) |
| `portfolioApi.getExchangeRates`, `getExchangeRatesForDate`, `getExchangeRatesForDateRange` and the commands `get_exchange_rates`, `get_exchange_rates_for_date`, `get_exchange_rates_for_date_range`; the CZK map returned by `refresh_exchange_rates` | CZK-per-unit maps for the frontend table | `get_fx_snapshot` / `get_fx_history` (`useFxSnapshot`, `useFxHistory`); `refresh_exchange_rates` only triggers the refresh |
| Rust `services/currency.rs`: `convert_to_czk`, `convert_to_czk_with_rates`, `convert_to_czk_at`, `get_all_rates`, `get_rates_for_date`, `get_rates_for_date_range`, `resolve_rates_for_day_from_range`, `breakdown_czk_at_rates` | Phase-1 CZK shims; the day lookups used the nearest day with *any* quote | `fx::current()` + `Quotes::convert`, `fx::History::convert_on` (common-day rule), `reporting` helpers |
| CZK caches: `portfolio_metrics_history.total_*`, `stock_value_history.value_czk`, `crypto_value_history.value_czk`, `portfolio_history::LegacyCzk`, the `backfill_currency_breakdowns` command (`portfolioApi.backfillCurrencyBreakdowns`), `currency::needs_breakdown_repair` / `breakdown_covers_total` / `BREAKDOWN_CONSISTENCY_TOLERANCE`, the `fx_backfill` CZK bridge | Derived CZK copies of native facts; dropped by migration 012 | Native `*_by_currency` breakdowns (personal real estate in `real_estate_personal_by_currency`) and native `quantity`/`price`/`currency` per ticker day, converted per response (`services/net_worth.rs`, `ticker_history::value_history`) |
| `fx::record_missing`, `fx::record_missing_all`, `fx::missing_now` (global missing-currency set) | Filled as a side effect of conversions, so it depended on which pages had run | Per-response `fxQuality` (`ConversionQuality`); `PriceStatus.missingCurrencies` = currencies entering converted totals that the current quotes cannot resolve (`fx_backfill::unquoted_currencies`; watch-only and closed-position quotes do not count) |
| `constants::DEFAULT_BASE_CURRENCY` | There is no base currency; reports declare theirs | The profile (main) currency or the request's `reportCurrency` |
| CZK-scale UI defaults (annuity/estate calculator amounts, the what-if 1 000 step) and scattered `\|\| 'CZK'` record-currency fallbacks | Meaningless outside CZK; fallbacks repeated by hand | EUR constants scaled by `scaledDefault` (filled once quotes load, empty without a rate), `whatIfStep` (1 % of the balance), `recordCurrency(code)` (the storage default `CZK` for a record without a currency, shared by labels and valuation) |

## Removed with the stocks analysis redesign (do not re-add)

The analysis page moved to one performance card over a daily engine
(`docs/specs/2026-10-11-stocks-analysis-redesign-design.md`). The old path computed one TWR series per
tag chip, so a position carrying two tags of one group showed three different values for the same tag.

| Removed | Why it was removed | Use instead |
|---|---|---|
| `get_stock_twr` (command), `investmentsApi.getStockTwr`, the types `TwrSeries`, `StockTwr`, `TwrDataPoint`, and `twr_series` / `compute_twr_for_tickers` in `services/investments.rs` | One series per tag, chain-linked per request; no contribution, money-weighted return or deposits, and no common partition of the portfolio | `get_stock_performance` (`investmentsApi.getStockPerformance`, `useStockPerformance`) over `services/stock_performance.rs`; query key `['stock-performance']` |
| `src/utils/twr.ts` (`twrPercent`, `twrTickDigits`) | Index-100 axis helpers of the old lines chart (`100 + twr`); 0 importers after the page rebuild | `src/utils/stock-performance.ts` |
| The stats row and the allocation ring of the stocks analysis page | The ring squeezed the TWR chart to half the width and 190 px, and split a position that carried several tags of one group | The performance card and its board (`src/components/stocks-analysis/`) |
| Several tags of one group on a position | A tag group is one partition of the portfolio: with two tags of a group on a position its slices counted it twice and did not add up to the portfolio | One tag per group, enforced by the tag service and migration 015 (ADR 0013) |
| `get_tag_metrics` (command), `stockTagsApi.getTagMetrics`, the `['tag-metrics']` query key and its invalidations, the `TagMetrics` / `TagMetricsReport` interfaces in `shared/schema.ts` | The page reads its per-tag figures from the performance response; nothing in the UI called the command after the rebuild. The Rust service `position_reports::tag_metrics` (with `tag_metrics_from`) and the `TagMetrics` / `TagMetricsReport` Rust types, their `bindings.rs` registration and `generated-types.ts` entries went with it (the MCP tool `tag_metrics` has its own `mcp::analytics::tag_metrics` on `positions::tag_totals`) | `get_stock_performance` (`useStockPerformance`) for return and contribution per part; `get_stocks_analysis` for positions with their tags |
| `src/components/stocks/TagShareBars.tsx`, `src/utils/tag-share.ts` (`buildTagShareRows`) and the keys `stocksAnalysis.metricsChart` / `stocksAnalysis.metricsChartDescription` | The "share of portfolio by tag" bars were not rendered anywhere after the redesign (0 importers); a position could carry several tags, so their shares could add up past 100 % | The value share column of the performance board and the contribution card |

## Removed with history from dated facts (do not re-add)

History days are derived from dated facts and rebuilt through one queue
(`docs/specs/2026-10-13-history-from-dated-facts-design.md`). The old paths UPDATEd only rows that
already existed, copied savings, loans, real estate and bonds from the nearest live row, valued
past days with today's holdings, and ran synchronously with their errors swallowed by `.ok()`.

| Removed | Why it was removed | Use instead |
|---|---|---|
| `portfolio_history::{rebuild_ticker_class, create_missing_days, aggregate_ticker_history, aggregate_days}`, `history_recalc::{aggregate_stocks, aggregate_stock_chunk}` | Derived one ticker class from per-ticker rows (the interim fix created days with statics carried from a live row) | `history_days::rebuild_days` (`plan_rebuild`, `prune_before_first_fact`, `rebuild_chunk`) |
| `portfolio_history::{carried_statics_from_nearest_live, StaticBreakdowns, update_classes, AssetClassKind}` | Statics were copied, never derived; a manual balance or valuation changed nothing in the past | `history_days::Facts::day_values` (ADR 0016); `portfolio_history::{update_class_set, ClassSet}` |
| `commands/portfolio.rs`: `recalculate_history_from_date`, `recalculate_asset_history_from_date`, `write_recalculated_day`, `historical_transaction_breakdowns`, `calculate_asset_breakdown_for_day`, `AssetType`, `current_static_breakdowns` | Synchronous full recalculations that aborted on a network error even for other-asset-only changes and wrote nothing on interior gaps | `portfolio::schedule_history_rebuild` with a `Job` (`services/history_recalc.rs`) |
| `trigger_historical_recalculation`, `trigger_historical_recalculation_for_{asset,stock_ticker,crypto_ticker}`, `update_portfolio_{stocks,crypto,history}_from_ticker_table(s)`, `schedule_stock_history_rebuild` | Per-class triggers the UI commands awaited and `.ok()`-swallowed; most writes (balances, valuations, loans, bonds) had no trigger at all | `portfolio::schedule_history_rebuild` (single entry point; the table of triggers is A6 of the spec) |
| `recalculate_crypto_ticker_history{,_strict,_with_policy}`, `spawn_crypto_history_rebuild`, the triple-returning `crypto_rebuild_jobs` | The crypto rebuild ran synchronously inside the command or as an untracked task | `history_recalc::{rebuild_crypto_ticker, rebuild_crypto_ticker_strict, crypto_ticker_jobs}` and `Job::CryptoTicker` (the worker reads the CoinGecko id from `crypto_investments`) |
| `get_stock_tickers`, `get_crypto_id_map`, `resolve_coingecko_id`, `get_snapshot_date_info`, `get_{stock,crypto,other_asset}_quantity_at_date`, `RECALC_FLOOR_TS` | Per-day lookups of the old recalculation (one query per day and item), and a second copy of the 2000-01-01 floor | `history_days::Facts` (every fact read once, per-item cursors) and `history_days::EARLIEST_DAY_FLOOR` |
| `backfill_missing_snapshots`, `breakdowns_for_backfill_day`, `backfill::{plan_backfill, value_positions_on_day, BACKFILL_MAX_DAYS_PER_RUN, PRICE_LEAD_IN_DAYS}` | Filled gaps after the oldest row only (never extended history backwards) and valued past days with today's holdings | `history_plan::plan_startup` (the one-time rebuild at version 1, then the repair of missing days) |
| `mcp_refresh::earliest_other_asset_date` | Fallback that rebuilt from the first other-asset transaction when a write gave no date | Every MCP write tool emits its domain and earliest day (`bank-accounts`, `real-estate`, `loans`, `bonds`, `other-assets`, …) |

## Removed with encrypted attachments (do not re-add)

Attachment content moved into the encrypted store `attachments.db` (ADR 0017,
`docs/specs/2026-10-15-encrypted-attachments-design.md`).

| Removed | Why it was removed | Use instead |
|---|---|---|
| The `asset:` protocol (`assetProtocol` in `tauri.conf.json`, the `protocol-asset` feature) and `convertFileSrc(path, 'asset')` for photos | The webview read plain attachment files from disk | `attachmentUrl(route, id)` (`src/utils/attachment-url.ts`) over the `moony-file` scheme |
| `fs:allow-copy-file` on `$APPDATA/real_estate_photos` and `copyFile`/`appDataDir` in the photo gallery | Downloads copied plain files out of the data folder from the webview | `attachmentsApi.export` / `exportPhotoBatch` (the backend writes a decrypted copy where the user chose) |
| `RealEstatePhoto.filePath` / `thumbnailPath`, `*Document.filePath` on the wire | Storage paths reached the webview | Row ids; documents carry `mimeType` and `extension` |
| Writing new attachments as files (`store_photo`, `fs::copy` into `insurance_documents/`, `real_estate_documents/`) | Plain files on disk | `real_estate_photos::{prepare_photo, record_photo}`, `attachments::add_*_document` |
| Opening a document from its stored path (`resolve_document_path` + `open_with_system` in the open commands) | Handed the plain file itself to another app | `attachments::open_elsewhere` (a wiped temporary copy) or the in-app viewer |

`file_path` / `thumbnail_path` columns and `document_files::resolve_document_path` remain only
for legacy rows that the conversion has not reached yet; never write new paths into them.

## Removed with the phone polish (do not re-add)

| Removed | Why it was removed | Use instead |
|---|---|---|
| The menu-owned delete on a phone detail: a `ConfirmSheet` plus the delete mutation inside the `···` menu, leaving through a per-call `onSuccess` (`src/mobile/hooks/use-leave-deleted.ts`, with `PositionMenu`, `CryptoMenu` and `OtherAssetMenu` as its users) | The menu unmounts when its record drops out of the list the page reads, the per-call callback is dropped with it, and the screen stays on the deleted record | `src/mobile/hooks/use-delete-and-leave.ts`, run by the page itself (the loan, policy, property, position, coin and other-asset details); the menu only asks (`onDelete`) |

## Removed with CoinGecko (do not re-add)

Crypto prices, search and history moved to Yahoo Finance and a holding stores its Yahoo symbol
(ADR 0020, `docs/specs/2026-10-25-crypto-prices-yahoo-design.md`).

| Removed | Why it was removed | Use instead |
|---|---|---|
| `get_api_keys` / `set_api_keys`, `ApiKeys`, the `api_key_coingecko` config row | The only API key in the app was CoinGecko's optional one; migration 025 wipes the stored secret | Nothing: Yahoo Finance needs no key |
| `ApiKeysCard` (Settings → Integrations) | Its only content was the CoinGecko key | Settings → Integrations holds the MCP card only |
| `CoinGeckoApiKeyBanner`, `UserProfile.coingeckoModalDismissed` / the `coingecko_modal_dismissed` column | Asked for a key that no longer exists | The unpriced-coins banner on the crypto page and the "Rate source…" picker (`CryptoSymbolDialog`, `set_crypto_symbol`) |
| `coingecko_id` columns and the `coingeckoId` field (wire contract, MCP `crypto_transactions_create`) | Tied a holding's identity to a provider that is gone | `crypto_investments.yahoo_symbol` / `yahooSymbol` (optional; resolved lazily by `services/crypto_symbols.rs`, never guessed) |
| `get_historical_crypto_prices_coingecko*`, `coingecko_history_start`, the 1.5 s pause between coins | CoinGecko's 365-day, rate-limited history | One Yahoo chart request per ticker for the whole range (`get_historical_stock_prices_yahoo` / `_strict`) |
| `merge_crypto_price_series`, `get_historical_crypto_prices_yahoo`, `yahoo_crypto_symbol` | Stitched CoinGecko's year to Yahoo's older days and derived `<TICKER>-USD` from the ticker, which is the wrong coin for some tickers (`ARB`, `TON`) and missing for others (`SUI`, `PEPE`) | The symbol stored on the holding; never build a Yahoo crypto symbol from a ticker |

## Deprecated type sources

| Path | Why deprecated | Use instead |
|---|---|---|
| `shared/types/extended-types.ts` | Conflicting redefinitions of canonical types | `shared/schema.ts` (the wire contract — ADR 0002) |
| Inline interfaces in `src/lib/tauri-api.ts` | Types belong in the shared contract; migrate opportunistically when touching a method | `shared/schema.ts` (ADR 0002) |

## Retired integration surfaces

| What | Why retired | Use instead |
|---|---|---|
| `session.json` discovery (ephemeral port + per-session token in the app data dir) | Plaintext token on disk; broke static client configs. Removed (ADR 0006) | The embedded MCP endpoint: fixed port + stable token shown in Settings |
| `moony-mcp` repo (external Node stdio proxy) | Replaced by the embedded MCP server | Point clients at `http://127.0.0.1:<port>/mcp` |
| REST routes on the local API (`/portfolio/metrics`, `/investments`, …) | Deleted in the MCP cutover (ADR 0006); do not re-add REST endpoints | MCP tools in `src-tauri/src/services/mcp/` |

## Anti-pattern files (working code, wrong shape — don't imitate)

| Path / pattern | What's wrong | Do this instead |
|---|---|---|
| `src-tauri/src/commands/categorization.rs` error style | Returns `Result<_, String>` | Return `crate::error::AppError` (see `docs/standards/rust-backend.md` §2) |
| Fat command files: `portfolio.rs`, `projection.rs`, `cashflow.rs`, `real_estate.rs` (costs, photos, documents, links) | Business logic inside command handlers | Logic belongs in `src-tauri/src/services/`; commands stay thin |
| Inline `useMutation` in stock/crypto/real-estate/insurance modals | Mutations defined inside components, bypassing shared cache-invalidation | Put mutations in domain hooks (`src/hooks/use-*.ts`) |
| `src/hooks/useCategorization.ts` | camelCase hook filename | Kebab-case: `use-<domain>.ts` |
