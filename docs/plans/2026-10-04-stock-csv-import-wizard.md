# Stock CSV import wizard — implementation plan

> **For agentic workers:** executed by parallel subagents, one work package each, in separate git
> worktrees; the coordinator merges and verifies. Steps use checkbox (`- [ ]`) syntax. Read the
> spec first: `docs/specs/2026-10-04-stock-csv-import-wizard-design.md`.

**Goal:** replace the stock import modal with a broker-aware four-step wizard (Soubor · Sloupce ·
Kontrola · Hotovo) with presets for XTB, Trading 212, Degiro, Interactive Brokers and the Moony
table, a generic mapping, a dry-run preview with duplicates and holdings checks, an atomic import
recorded as a batch, undo, and remembered custom formats.

**Architecture:** contract first. The wire types, module layout and stub entry points are on the
base commit (`src-tauri/src/services/stock_import/` — `types.rs` is the source of truth — the
TypeScript mirror in `shared/schema.ts` section "Stock CSV import", and `stockImportApi` in
`src/lib/tauri-api.ts`). B1 implements reading files, B2 writing and the database side, B3 the
commands and the whole frontend. They work in parallel against the stubs' signatures; the
coordinator merges B1, B2, B3 in that order.

**Tech stack:** Rust (rusqlite/SQLCipher, csv, encoding_rs, chrono, yahoo_finance_api via
`services::price_api`), React 19 + TypeScript, TanStack Query, react-i18next, Tauri dialog/fs plugins.

## Global constraints (every package)

- `AGENTS.md` ten rules: `invoke()` only in `src/lib/tauri-api.ts`; Rust wire types with per-field
  camelCase renames, registered in `bindings.rs`, mirrored in `shared/schema.ts`, then
  `cd src-tauri && cargo test generate_bindings -- --ignored`; thin commands, logic in services
  taking `&Connection`, `AppError`, no `.unwrap()` outside tests; append-only migrations; mutation
  contract (stock keys + `portfolio-metrics` + `cashflow-report`, then `recordSnapshot()` +
  `portfolio-history`); money TEXT, days UTC-midnight seconds, PKs TEXT UUIDs.
- **Do not change any signature or type in `services/stock_import/types.rs` or the stub entry
  points** (`inspect::inspect`, `parse::parse_file`, `preview::preview`, `import::import`,
  `batches::{list_batches, undo_batch}`, `formats::{list_formats, save_format, delete_format}`,
  `instruments::resolve_instruments`). If the contract is wrong, say so in your report and work
  around it inside your own files.
- Messages are i18n keys, never English prose: row keys `importWizard.row.*` (`stocks` namespace),
  `validation.*` (`common` namespace). Every user-facing string in both `cs` and `en`.
- Tests: Rust service logic in `#[cfg(test)]` with `Connection::open_in_memory()` and a minimal
  schema; pure TS helpers in co-located `*.test.ts`; TDD (failing test first).
- No new dependencies. Do not edit `docs/design-system/`, `CHANGELOG.md`, `docs/specs/`,
  `docs/plans/`. Do not use the browser-pane tools. Never push, merge or switch branches in the main
  checkout; work only in your worktree.
- Gates before each commit: `npm run lint && npm run typecheck && npm test` and
  `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test` (export
  `CARGO_TARGET_DIR` first). Husky hooks do not run in worktrees: run the gates by hand.
- Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## File ownership

| Package | Owns |
|---|---|
| B1 reading | `services/stock_import/{inspect,parse,columns,detect}.rs`, `services/stock_import/adapters/*`, `services/csv_import/decode.rs` (a generic header-row helper only; the bank behaviour stays byte-identical), `services/date_parser.rs` (separator-less `%Y%m%d` support only), fixtures `src-tauri/tests/fixtures/csv/stocks-*.csv` (new ones; the two orphan `stocks-template.csv` / `stocks-messy.csv` may be rewritten) |
| B2 writing | migration `003_stock_import_batches` + `schema.snapshot` + `docs/architecture/database.md`, `services/stock_import/{preview,import,simulate,batches,formats,instruments}.rs`, `services/investments.rs` (transaction insert + the shared bulk writer), `services/mcp/investments.rs` (call the shared writer), `services/dedup.rs` (if needed), `services/price_api.rs` (`get_currency_from_ticker` visibility only) |
| B3 commands + frontend | new `commands/stock_import.rs`, `commands/mod.rs` and `lib.rs` (registration), removal of `import_investment_transactions` from `commands/investments.rs` and `lib.rs`, `src/lib/tauri-api.ts` (removal of `investmentsApi.importTransactions` and its inline `ImportResult`), everything under new `src/components/stocks/import/`, `src/hooks/use-stock-import-mutations.ts`, `src/pages/Stocks.tsx` (swap the dialog), removal of `src/components/stocks/ImportInvestmentsModal.tsx`, i18n `stocks` (`importWizard.*`; delete the orphan `import.*` keys) and `common` (`validation.*` keys of the contract), `docs/DEPRECATED.md`, `CONTRIBUTING.md` ("Adding a broker format") |

---

### B1 — Reading files (inspect, parse, adapters)

Public API (stubs on the base commit, keep the signatures):
`inspect::inspect(bytes, file_name, &StockCsvInspectOptions, &[SavedStockImportFormat]) -> Result<StockCsvInspection>`
and `parse::parse_file(bytes, &StockImportConfig) -> Result<ParsedFile>`.

- [ ] **Decoding and shape.** Reuse `csv_import::decode::{decode_csv_content, detect_csv_delimiter, csv_reader, LineIndex, slice_from_line, read_headers}` and `csv_import::amounts::{clean_and_parse_amount, detect_decimal_separator}`. Add to `decode.rs` a generic `detect_header_row_by(content, delimiter, is_header: impl Fn(&[String]) -> bool) -> usize` and make the bank `detect_header_row` call it (bank tests unchanged). Stock header rule (`detect.rs`): first line with ≥ 3 non-empty cells whose cells match ≥ 2 stock roles, one of them date, quantity or price.
- [ ] **Roles and suggestions (`columns.rs`).** Roles `date, type, symbol, isin, name, quantity, price, currency, externalId, fee` with normalised (lowercase, no diacritics, punctuation → space) header patterns in EN, CS, DE, NL, FR, ES, IT, PL (e.g. date: date, datum, trade date, tradedate, time, čas, data, fecha; quantity: quantity, qty, shares, no of shares, počet, množství, kusy, anzahl, aantal, ilość, cantidad, quantità; price: price, unit price, price share, tradeprice, cena, kurs, koers, prix, precio, prezzo; currency: currency, ccy, měna, mena, währung, valuta, devise, moneda, waluta, currencyprimary; symbol: symbol, ticker, instrument; isin: isin; name: name, název, product, produkt, security, company; type: type, typ, action, side, direction, buy sell, směr, transaction type; externalId: id, order id, trade id, transaction id; fee: fee, fees, commission, poplatek, ibcommission, transaction and or third party fees). Confidence 0.95 exact, 0.85 contains, each header claimed once.
- [ ] **Type keywords.** `suggest_type_action(value) -> TypeValueAction`: normalised text containing a buy word (buy, bought, purchase, kauf, nakup, koupe, zakup, kupno, compra, achat, acquisto, aankoop, koop, open buy) → Buy; a sell word (sell, sold, sale, verkauf, prodej, sprzedaz, venta, vende, vente, vendita, verkoop, close buy) → Sell; everything else (dividend, deposit, withdrawal, interest, fee, tax, conversion, split, transfer, …) → Skip. `column_values`: every column with ≤ 30 distinct non-empty values, with counts, first line and the suggestion.
- [ ] **Dates.** Detection via `date_parser::detect_date_format` on the date part of the cells (text after the date such as ` 14:30:03.613`, `T08:23:08Z`, `;123456` is ignored); add separator-less `%Y%m%d` (IBKR) to detection and strict parsing in `date_parser.rs` with tests. Ambiguity flag as in the bank import.
- [ ] **Adapters (`adapters/`).** Each: `detect(headers, sample_rows) -> bool`, `config(headers, sample_rows) -> StockImportConfig` (columns resolved to the file's positions). Detection order: specific before generic; a saved format whose `header_signature` matches wins over adapters; `options.source` forces one.
  - `xtb`: `;`, headers `ID;Type;Time;Symbol;Comment;Amount` and translations (Typ/Tipo/Rodzaj, Čas/Czas/Hora/Data, Komentář/Komentarz/Comentario/Comentário, Částka/Kwota/Importe/Montante…) — detect by six columns in that order plus at least one comment matching the trade pattern. Trade rows: comment `^(OPEN|CLOSE) BUY (qty)(/total)? @ (price)`; quantity = the number before `/`, price after `@`; OPEN → buy, CLOSE → sell (the comment decides the direction; `typeValues` only decide trade vs skip: suggested buy for purchase types, sell for sale types, skip for the rest). `transforms.xtbComment = xtbSymbols = true`, `currencyMode = instrument`, `externalIdColumn = ID`. Suffix map: `.US` → none, `.UK` → `.L`, `.DE` → `.DE`, `.FR` → `.PA`, `.NL` → `.AS`, `.BE` → `.BR`, `.ES` → `.MC`, `.IT` → `.MI`, `.PT` → `.LS`, `.CH` → `.SW`, `.DK` → `.CO`, `.SE` → `.ST`, `.NO` → `.OL`, `.FI` → `.HE`, `.PL` → `.WA`, `.CZ` → `.PR`, `.AT` → `.VI`, `.IE` → `.IR`; unknown suffix kept. Date `%d.%m.%Y` (time ignored).
  - `trading212`: `,`, header starting `Action,Time,ISIN,Ticker,Name,No. of shares,Price / share,Currency (Price / share)`; actions `Market buy|Limit buy|Stop buy|Stop limit buy` → buy, `… sell` → sell, everything else skip (deposits, withdrawals, `Dividend (…)`, interest, currency conversion, lending interest, stock splits — reason `importWizard.row.notATrade`); date `%Y-%m-%d`; `externalIdColumn = ID`; currency column `Currency (Price / share)`; `GBX`/`GBp` → GBP ÷ 100 (always, in every source).
  - `degiro` (Transactions export, not the account statement): headers in the UI language; structure `Date, Time, Product, ISIN, Reference exchange, Venue, Quantity, Price, <blank: price currency>, Local value, <blank>, Value, <blank>, Exchange rate, [AutoFX fee,] Transaction and/or third party fees, <blank>, Total, <blank>, Order ID`. Detect structurally (≥ 17 columns, an `ISIN` header at index 3, a blank header right after the price column, ISIN-shaped values in samples, `dd-MM-yyyy` dates); `directionMode = quantitySign`, `currencyColumn = price + 1`, date `%d-%m-%Y`, decimal from the data (comma in some locales), `externalIdColumn` = the order id column (not unique per fill: rule 1 then needs the same quantity and price too — implement rule 1 as id + quantity + price).
  - `ibkr` (Flex Query, Trades section): header fields `Symbol, ISIN, TradeDate, Buy/Sell, Quantity, TradePrice, CurrencyPrimary, AssetClass, TradeID` (order free; `ISIN`, `Buy/Sell`, `AssetClass`, `TradeID` optional); direction from `Buy/Sell` when present else quantity sign; skip rows whose `AssetClass` is not `STK` (`transforms.assetClassColumn/Allowed`); date `%Y%m%d` or `%Y-%m-%d`.
  - `moony` (hand-made table and Moony's own export): headers EN `Date, Type, Ticker|Symbol, Name, Quantity, Price, Currency` or CS `Datum, Typ, Symbol|Ticker, Název, Počet, Cena, Měna` (any order, Name optional); type words buy/sell/nákup/prodej/koupě (case and diacritics free); any delimiter, encoding and decimal mark; dates `dd.MM.yyyy`, `d.M.yyyy` or `yyyy-MM-dd`.
- [ ] **Parsing (`parse.rs`).** Every data row ends in exactly one of `trades`, `skipped`, `errors` (`total_rows` = sum). Keys: `importWizard.row.notATrade` (detail: the type value), `importWizard.row.zeroQuantity`, `importWizard.row.assetClass` (detail), `importWizard.row.instrumentSkipped`, `importWizard.row.dateUnparseable` (detail: cell), `importWizard.row.numberUnparseable` (detail), `importWizard.row.commentUnparseable` (XTB, detail), `importWizard.row.symbolMissing`, `importWizard.row.currencyMissing`, `validation.currencyInvalid` (detail). Quantity/price absolute values (> 0; a zero price is an error `importWizard.row.priceMissing`), instrument key `isin:<ISIN>` when an ISIN cell is valid (12 chars, 2 letters + 9 alphanumerics + digit) else `symbol:<SYMBOL>`; symbols and ISINs uppercase; currencies uppercase; `has_fee_column` from the fee role. Overrides with `skip` exclude the instrument's rows (`instrumentSkipped`); other override fields are applied by B2, not here.
- [ ] **Fixtures + guard test.** Synthetic files (never real data) in `src-tauri/tests/fixtures/csv/`: `stocks-xtb.csv`, `stocks-trading212.csv`, `stocks-degiro.csv` (comma decimals), `stocks-ibkr.csv`, `stocks-moony.csv` (Moony export), `stocks-handmade-cz.csv` (written as Windows-1250 bytes, `;`, decimal comma, Czech headers and type words) — each with buys, sells and non-trade rows. Guard test `all_stock_adapters_detect_their_fixture`: every fixture is detected as its source, no other adapter claims it, `parse_file(inspect(..).config)` yields zero errors and the expected trade count, directions and the first trade's values.
- [ ] Gates; commits `feat(stocks): read broker exports for the import wizard` (split as you see fit).

### B2 — Writing (preview, import, batches, formats, instruments)

Public API (stubs, keep the signatures): `preview::preview(conn, &ParsedFile, &StockImportConfig)`,
`import::import(&mut conn, &ParsedFile, &StockImportConfig, file_name)`,
`batches::{list_batches, undo_batch}`, `formats::{list_formats, save_format, delete_format}`,
`instruments::resolve_instruments(Vec<StockInstrumentQuery>) -> Vec<StockInstrumentResolution>`.
Tests build `ParsedFile`/`ParsedTrade` values directly (B1 is not available in your worktree).

- [ ] **Migration `003_stock_import_batches`:**
  ```sql
  CREATE TABLE IF NOT EXISTS stock_import_batches (
      id TEXT PRIMARY KEY,
      file_name TEXT NOT NULL,
      source TEXT NOT NULL,
      trade_count INTEGER NOT NULL,
      created_at INTEGER NOT NULL DEFAULT (unixepoch())
  );
  ALTER TABLE investment_transactions ADD COLUMN import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET NULL;
  ALTER TABLE investment_transactions ADD COLUMN external_id TEXT;
  CREATE INDEX IF NOT EXISTS idx_investment_transactions_batch ON investment_transactions(import_batch_id);
  CREATE INDEX IF NOT EXISTS idx_investment_transactions_external ON investment_transactions(external_id);
  ```
  Name it `003_…` (the overview branch adds `002_real_estate_purchase_date`; the coordinator orders
  them at merge — append yours after `001` in your branch). Regenerate `schema.snapshot`, amend
  `docs/architecture/database.md`. Migration test.
- [ ] **Shared bulk writer (ADR 0007).** In `services/investments.rs`:
  `pub struct BulkStockRow { ticker, company_name: Option<String>, tx_type: String, quantity: String, price_per_unit: String, currency: String, transaction_date: i64, external_id: Option<String>, allow_duplicate: bool }` and
  `pub fn bulk_create_stock_transactions(conn: &mut Connection, rows: &[BulkStockRow], batch: Option<&NewStockImportBatch>) -> Result<BulkWriteReport>` with today's MCP semantics (validate all → one SQL transaction → order by date, buys first → duplicate check unless `allow_duplicate` → any business-rule failure rolls everything back) plus: the batch row is inserted in the same transaction and every created transaction stores `import_batch_id` and `external_id`. `create_transaction_internal` gains those two optional columns (other callers pass `None`). The MCP `stock_transactions_create` becomes a thin call (its nine bulk tests stay green, unmodified in intent). Decimal text for quantity/price: `csv_import::amounts::amount_to_text`.
- [ ] **Simulation (`simulate.rs`, TDD).** Apply overrides (ticker, name, currency, skip) to each trade; the ticker = override ticker → else the existing position matched by the file symbol → else the file symbol; `CurrencyMode::Instrument` takes the override currency, else `price_api::get_currency_from_ticker(ticker)` (make it `pub(crate)`). Rules in date order, buys before sells on a day, per ticker, starting from the existing transactions of that ticker: duplicate rule 1 (`external_id` stored as `<source>:<id>`; Degiro additionally same quantity and price) and rule 2 (same ticker, day, direction, quantity within 1e-9 relative, price within 1e-6 relative) → `duplicate` unless the line is in `importAnywayLines`; sell above the holding at that day → error `importWizard.row.sellExceedsHoldings` (detail: held quantity); trade currency ≠ existing position currency (or ≠ the first currency of a new position in the file) → error `importWizard.row.currencyMismatch` (detail: position currency); an instrument without a ticker → error `importWizard.row.symbolMissing`. Error rows are excluded and the simulation continues without them.
- [ ] **Preview (`preview.rs`).** Instruments in first-appearance order with status (`existing` when the resolved ticker is a position, `missingSymbol` for ISIN-only without override ticker, `skipped`, else `new`), `positionCurrency` when it differs; rows (first `PREVIEW_ROWS`, file order, parsed errors/skips included with their messages); counts over the whole file (`total` = `ParsedFile.total_rows`); `dateRange` of the importable trades; `hasFeeColumn` passed through.
- [ ] **Import (`import.rs`).** Re-run the simulation; write the importable rows through the bulk writer with a batch (`source` = config source, `trade_count` = rows written); nothing to write → no batch, `batchId: None`. Result lists new vs updated positions, messages in file order, earliest day. A rule failure at write time aborts with that row's message.
- [ ] **Batches (`batches.rs`, TDD).** `list_batches` newest first with `remaining_count`; `undo_batch` in one transaction: delete the batch's transactions, recalculate the affected positions (`recalculate_investment_metrics`), delete positions left without any transaction (tags cascade), delete the batch; return tickers, removed positions and the earliest day.
- [ ] **Formats (`formats.rs`, TDD).** `app_config` key `stock_import_formats` (JSON array), newest first, at most 20 (oldest dropped), id `format:<uuid>`, `header_signature` from `types::header_signature`.
- [ ] **Instruments (`instruments.rs`).** Sequential Yahoo lookups via `price_api::search_stock_tickers` (symbol first, else ISIN), 250 ms apart, at most 50 queries per call; candidates with the currency from `get_currency_from_ticker`; `best` = exact symbol (case-insensitive) → a listing in the query currency → the first; any error → `lookupFailed: true` for that key. Unit-test the candidate choice as a pure function.
- [ ] Gates; commits `feat(stocks): batch-recorded stock imports with undo`, `refactor(stocks): one bulk writer for the MCP tool and the CSV import`, …

### B3 — Commands and the wizard

- [ ] **Commands (`commands/stock_import.rs`, thin).** `inspect_stock_csv(file_path, options)` (reads the file like the bank `read_csv_file`, loads saved formats, calls `inspect`), `resolve_stock_instruments(queries)` (no DB), `preview_stock_csv_import(file_path, config)` (`config.validate()`, `parse_file`, `preview` in `with_conn`), `import_stock_csv(app, db, recalc, file_path, config)` (validate, parse, `import` in `with_conn_mut`; then `portfolio::update_todays_snapshot`, `portfolio::schedule_stock_history_rebuild` with one job per ticker from the earliest day, emit `recalculation-complete`), `list_stock_import_batches`, `undo_stock_import_batch(batch_id)` (then snapshot + rebuild like import), `list/save/delete_stock_import_format`. Register in `lib.rs`. Remove `import_investment_transactions` (command + registration) and `investmentsApi.importTransactions` with its inline `ImportResult` type.
- [ ] **Frontend (`src/components/stocks/import/`).** `StockImportDialog` (orchestrator, `max-w-[980px]`, `WizardSteps`, steps file → mapping → review → done, close blocked while importing), `SourceStep`, `MappingStep`, `ReviewStep` (with `InstrumentsPanel`), `ResultStep`; reuse the bank wizard's `use-file-drop`, `ColumnSelect`/`RawDataPreview` patterns (copy what is bank-specific rather than coupling) and the sample-file download of the old modal (plugin-dialog `save` + plugin-fs `writeTextFile`, Moony format with Czech or English headers by UI language). Pure helpers with tests in `import-config.ts` (mapping state ↔ `StockImportConfig`, required-field completeness, the summary sentence). Debounced preview query (`['stock-import-preview', path, config]`, `gcTime: 0`, keep the previous result while loading). Instruments panel: resolve the `new`/`missingSymbol` instruments once per file (`resolveInstruments`), apply each `best` as an override automatically (mark verified), show unverified ones, allow editing ticker (with the existing Yahoo ticker search component), name, currency, and skip. Recent imports list with undo in the source step and undo on the result step (`ConfirmDeleteDialog`-style confirmation).
- [ ] **Hooks.** `src/hooks/use-stock-import-mutations.ts`: import and undo invalidate `investments`, `investment`, `investment-transactions`, `transactions`, `all-stock-transactions`, `dividend-summary`, `stocks-analysis`, `tag-metrics`, `stock-twr`, `stock-import-batches`, `portfolio-metrics`, `cashflow-report`, then `portfolioApi.recordSnapshot()` and `portfolio-history`; after an import also `priceApi.refreshStockPrices()` and `refreshDividends()` in the background like the old modal.
- [ ] **Copy (cs + en, `stocks.importWizard.*`).** Steps "Soubor · Sloupce · Kontrola · Hotovo"; source names; guides:
  - XTB: "V xStation otevřete Historie účtu → Peněžní operace, zvolte období od prvního obchodu, klikněte na Exportovat a vyberte formát CSV." Note: "Soubor obsahuje i vklady, dividendy a poplatky – naimportují se jen nákupy a prodeje akcií a ETF. Měnu doplníme podle burzy."
  - Trading 212: "V aplikaci otevřete Historie → ikona exportu, zaškrtněte Objednávky, zvolte období a stáhněte CSV." Note: "Jeden export pokryje nejvýš 12 měsíců – nahrajte více souborů, duplicity poznáme."
  - Degiro: "Na webu otevřete Aktivita → Transakce, nastavte období od prvního obchodu a klikněte na Export → CSV. Výpis z účtu (Account statement) nepoužívejte."
  - Interactive Brokers: "V Client Portal otevřete Performance & Reports → Flex Queries, vytvořte Activity Flex Query se sekcí Trades a poli Symbol, ISIN, TradeDate, Buy/Sell, Quantity, TradePrice, CurrencyPrimary, AssetClass a TradeID, formát CSV, a spusťte ho pro celé období."
  - Vlastní tabulka (vzor Moony): "Stáhněte vzor, vyplňte ho v Excelu nebo jiné tabulce a uložte jako CSV." + the column list with an example row (Datum `15.01.2024` nebo `2024-01-15`; Typ `nákup`/`prodej` nebo `buy`/`sell`; Symbol jako na Yahoo Finance, např. `AAPL`, `VWCE.DE`; Počet; Cena za kus; Měna; Název nepovinný).
  - Jiný broker: "Potřebujeme jeden řádek na obchod: datum, směr, symbol nebo ISIN, počet kusů, cenu za kus a měnu. Sloupce přiřadíte v dalším kroku a Moony si mapování zapamatuje."
  - English versions with the same meaning; row messages for every `importWizard.row.*` key and the contract's `validation.*` keys (`stockImportSymbolRequired`, `stockImportCurrencyRequired`, `stockImportTypeRequired`, `dateFormatRequired`, `csvDelimiterInvalid`, `tickerInvalid` when missing). Czech plurals where counts appear.
- [ ] **Removals and docs.** Delete `ImportInvestmentsModal.tsx` and the orphan `stocks.import.*` keys; `Stocks.tsx` opens `StockImportDialog` from the page head and the empty state; record the removal in `docs/DEPRECATED.md`; add "Adding a broker format" to `CONTRIBUTING.md` (adapter module + fixture + guard test + guide text).
- [ ] Gates; commits `feat(stocks): import wizard for broker exports`, `refactor(stocks): retire the old import modal and command`, …

## Integration (coordinator)

- [ ] Merge B1, B2, B3 into `feature/stock-import-wizard`; then merge the result with `main` after
  the overview branch landed (migration order 002 then 003; shared files `lib.rs`, `bindings.rs`,
  `schema.ts`, `tauri-api.ts`, `Stocks.tsx`, `stocks.json`); regenerate bindings and the schema
  snapshot; run all gates.
- [ ] Verify end to end in the demo profile with every fixture: detection, mapping, preview
  statuses, instrument resolution online and offline, import, re-import (duplicates only), undo,
  saved format reuse; Czech and English; 1440 and 1080 px.
- [ ] Code review at high effort, fixes, final gates, local merge to `main`.
