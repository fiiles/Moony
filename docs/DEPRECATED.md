# DEPRECATED — Do Not Copy

Things that exist in this repo but must never be copied, extended, or used as
examples. Before reusing any pattern, check this list.

## Dead frontend code

| Path | Why it's dead | Use instead |
|---|---|---|
| Any shadcn primitive in `src/components/ui/` without importers | Never imported — harmless, but do **not** take their presence as evidence the codebase uses them | Check actual imports before citing a component as "used" |

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
