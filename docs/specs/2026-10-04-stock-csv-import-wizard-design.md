# Stock CSV import wizard — design

Date: 2026-10-04 · Status: approved by the owner on 2026-10-04 · Companion spec:
`2026-10-04-overview-polish-and-fixes-design.md` (part A; it moves the import button into the
stocks page head).

## 1. Context

Users bring transaction exports from different brokers, each in its own shape. The current
importer (`ImportInvestmentsModal.tsx` + the `import_investment_transactions` command) asks for six
columns by hand and has no presets, no preview, no duplicate check and no undo. It writes row by
row without a transaction, guesses the date format per row, stores numbers such as `1,234.50` as
invalid text that later reads as 0, reports English prose with row numbers that do not match the
file, and skips part of the mutation contract. The bank CSV wizard (four steps, Rust services over
a file path, presets, dry-run preview, duplicate detection, import batches with undo) is the model.

### Owner decisions (2026-10-04)

- Presets for **XTB, Trading 212, Degiro and Interactive Brokers**, plus Moony's own format and a
  generic "other broker" mapping. No sample files were provided; adapters are built from the
  brokers' documented export formats with synthetic fixtures, and real files can be added later.
- A hand-made file must stay a first-class path: a user with an unsupported broker fills a simple
  table in Excel (or any spreadsheet), saves it as CSV and imports it (owner review of the first
  draft).
- **Undo** of an import, with a migration (batch id on stock transactions).
- **Fees are not imported** (as today); purchase prices exclude them. The preview says so when the
  file has a fee column.

## 2. What broker exports look like

| Broker | Export to use | Shape | Consequences |
|---|---|---|---|
| XTB | Account history → Cash operations → Export, CSV | `;`, `ID;Type;Time;Symbol;Comment;Amount`, `dd.MM.yyyy HH:mm:ss`; trades are `Stocks/ETF purchase` / `Stocks/ETF sale`; quantity and price live in the comment (`OPEN BUY 34/42.5658 @ 11.7480`, `CLOSE BUY 1 @ 26.280`); symbols carry XTB suffixes (`AAPL.US`, `VUSA.UK`, `SPYL.DE`); no currency column; the file also holds deposits, dividends, taxes, interest | Comment extraction, suffix → Yahoo symbol mapping (`.US` → none, `.UK` → `.L`, `.FR` → `.PA`, `.NL` → `.AS`, `.IT` → `.MI`, `.ES` → `.MC`, `.PL` → `.WA`, `.CZ` → `.PR`, `.CH` → `.SW`, …), currency from the instrument, non-trade rows skipped |
| Trading 212 | History → Export, CSV (at most 12 months per file) | `,`, `Action,Time,ISIN,Ticker,Name,No. of shares,Price / share,Currency (Price / share),…,ID,…`; actions `Market buy`, `Limit sell`, … mixed with `Deposit`, `Dividend (…)`, `Interest on cash`, `Currency conversion`; `yyyy-MM-dd HH:mm:ss(.SSS)`; tickers without exchange suffix; `GBX` prices | Action value mapping, ISIN-based symbol resolution for non-US listings, GBX → GBP (÷ 100), broker id for duplicates, several files per history |
| Degiro | Activity → Transactions → Export, CSV (not the account statement) | Headers in the UI language; `Date,Time,Product,ISIN,Reference exchange,Venue,Quantity,Price,,Local value,,…,Order ID`; the price currency is the unnamed column right after Price; `dd-MM-yyyy`; sells have a negative quantity; decimal comma in some locales | Header matching in several languages, column addressed relative to Price, direction by quantity sign, ISIN-based symbol resolution |
| Interactive Brokers | Flex Query (Activity, section Trades), CSV | User-selected fields with stable names: `Symbol, ISIN, TradeDate, Buy/Sell, Quantity, TradePrice, CurrencyPrimary, AssetClass, TradeID`; `yyyyMMdd` or `yyyy-MM-dd`; sells negative; options and FX rows present | Guide lists the fields to tick; non-stock asset classes skipped; separator-less dates |
| Moony | Stocks → Export, or the sample file filled in by hand | `Date,Type,Ticker,Name,Quantity,Price,Currency` (`Name` optional) | Round trip; the hand-made table path (§3) |

Sources used for the formats: the public sample exports of the Apache-2.0 project
Export-To-Ghostfolio and the brokers' help pages.

## 3. User experience

A wide modal (as the bank wizard, 980 px) opened from "Importovat CSV" on the stocks page head and
empty state. Steps: **1 Soubor · 2 Sloupce · 3 Kontrola · 4 Hotovo** (`WizardSteps`).

### Step 1 — Soubor

- Left column, "Odkud je export": XTB, Trading 212, Degiro, Interactive Brokers, **Vlastní
  tabulka (vzor Moony)**, saved custom formats, "Jiný broker". The selected source shows **"Jak
  získat export"**: numbered steps (where to click, which report, which period, which format), the
  broker's caveats ("Trading 212 exportuje nejvýš 12 měsíců – nahrajte víc souborů, duplicity
  poznáme") and a link to the broker's help page.
- **Vlastní tabulka (vzor Moony)** is the path for unsupported brokers and hand-kept records:
  "Stáhněte vzor, vyplňte ho v Excelu nebo jiné tabulce a uložte jako CSV." The guide lists the
  columns with an example row (Datum `15.01.2024` or `2024-01-15`; Typ `nákup`/`prodej` or
  `buy`/`sell`; Symbol as on Yahoo Finance, e.g. `AAPL`, `VWCE.DE`; Počet; Cena za kus; Měna;
  Název optional). Files saved by Excel in any locale are accepted: comma, semicolon or tab,
  UTF-8 or Windows-1250/1252, decimal comma or point, thousands separators, Czech or English
  headers. Moony's own export is the same format, so it is recognised automatically and needs no
  mapping.
- "Jiný broker" is for any other layout: it shows what the file must contain (one row per trade
  with date, direction, symbol or ISIN, quantity, price and currency) and leads to the mapping form
  in step 2; the mapping can be remembered for the next file.
- Right column: drop zone + "Vybrat soubor…" (Tauri dialog and drag and drop, as the bank wizard),
  `.csv`/`.txt`; "Stáhnout vzorový soubor" (Moony format). Excel files get the hint to save them as
  CSV.
- "Poslední importy" under the picker: date, file, source, number of trades, "Vrátit".
- After the file is read the source is detected from its headers ("Rozpoznáno: Trading 212"); a
  mismatch with the chosen source is shown, not silently overridden.

### Step 2 — Sloupce

- File bar: name, data rows, delimiter, encoding, detected source.
- **Known source:** a compact summary of what the preset maps ("Datum ← Time · Směr ← Action ·
  Počet ← No. of shares …") and of the action values ("Market buy, Limit buy → Nákup; Deposit,
  Dividend… → přeskočit"), with "Upravit mapování" to open the full form. Most users continue
  straight to step 3.
- **Full form** (other brokers, saved formats, or edited presets):
  - Required: Datum, Symbol **or** ISIN, Počet kusů, Cena za kus, Měna (a column, one fixed
    currency for the whole file, or "podle burzy" — taken from the resolved instrument).
  - Direction: a type column with a value table (every distinct value with its count and an
    example row, each set to Nákup / Prodej / Přeskočit; suggested from keywords in EN, CZ, DE,
    NL, FR, ES, IT, PL), or "podle znaménka počtu kusů".
  - Optional: Název.
  - Each select shows example values from the file and a confidence mark when suggested.
  - Format: date format (auto, with the ambiguity warning of the bank wizard), decimal separator,
    header row and skipped rows.
  - "Zapamatovat mapování pro tento formát" (on by default for "Jiný broker"): stored with a name
    and matched next time by the header layout.
- Raw preview of the first rows with the mapped columns highlighted.

### Step 3 — Kontrola

- Summary strip: "K importu N · Duplicity D · Přeskočeno S · Chyby E · Období od – do", with the
  note "Poplatky se neimportují" when the file has a fee column.
- **Cenné papíry** (one row per instrument in the file): symbol or ISIN, name, currency, number of
  trades and a state — *Existující pozice* (matched to a Moony position), *Nová pozice* (verified on
  Yahoo Finance: symbol, exchange, currency), *Neověřeno* (Yahoo did not answer; importable with the
  given symbol, prices may not update), *Chybí symbol* (ISIN not resolved; must be entered). Symbol,
  name and currency are editable inline (with the existing ticker search); a change applies to every
  trade of that instrument and re-runs the preview. "Přeskočit" excludes all trades of an
  instrument (an option, a delisted share). Resolution runs automatically when the step opens, with
  progress, and never blocks offline use.
- **Řádky:** the first 12 rows with "Zobrazit vše" (up to 200): file line, date, direction, symbol,
  quantity, price, currency, state — *Nový*, *Duplicita* (with "importovat i tak"), *Přeskočeno*
  (reason: dividenda, vklad, jiný typ, jiný druh aktiva, nulový počet), *Chyba* (translated
  message: neplatné datum, neplatné číslo, prodej nad držbu k datu, měna se liší od pozice, …).
- Business rules are simulated in date order (buys before sells within a day) against existing
  positions and earlier rows: a sell above the holding at its date and a currency that differs from
  an existing position are row errors before anything is written. Error rows are excluded and the
  simulation continues without them.
- "Importovat N obchodů" is enabled when N > 0 and every instrument in *Chybí symbol* has a
  symbol or is skipped.

### Step 4 — Hotovo

- Tiles: Importováno, Duplicity, Přeskočeno, Chyby (expandable lists with file lines).
- "Nové pozice: …" and "Doplněno k existujícím: …".
- "Přepočítávám historii hodnoty portfolia…" while the existing history rebuild runs (the header
  badge already reports progress).
- Next steps: Zobrazit akcie · Přiřadit štítky (stocks analysis) · Importovat další soubor ·
  Vrátit import.

## 4. Architecture

Rust owns parsing, detection, simulation and writing; the frontend holds a file path and a config,
like the bank wizard.

### 4.1 Services (`src-tauri/src/services/stock_import/`)

- `detect` — reuses `csv_import::decode` (encoding, delimiter, `LineIndex`, reader) with a
  stock-specific header-row detector (stock column roles) and synthetic names for blank headers
  ("Sloupec 9", plus the header it follows for preset addressing).
- `adapters` — one module per source (`xtb`, `trading212`, `degiro`, `ibkr`, `moony`): header
  signatures (with language variants), the config they produce for the file (columns resolved to
  the file's own header positions) and the transforms the generic config cannot express (XTB
  comment extraction and suffix mapping, Degiro price-currency column, IBKR asset-class filter,
  GBX normalisation). The `moony` adapter is the hand-made table: English or Czech headers
  (`Date/Datum`, `Type/Typ`, `Ticker/Symbol`, `Name/Název`, `Quantity/Počet`, `Price/Cena`,
  `Currency/Měna`), `Name` optional, type words in both languages. Adding a broker = one module +
  fixture + guide text.
- `config` — `StockImportConfig` (validated, `validation.*` keys): delimiter, encoding, header row,
  skipped rows, source id, date column/format, symbol and/or ISIN column, name column, quantity
  column, price column, currency mode (column / fixed / instrument), direction mode (type column +
  value map / quantity sign), decimal separator, optional broker id column, transforms, row
  overrides ("import anyway" lines, instrument overrides).
- `parse` — rows → `ParsedTrade { line, day, direction, instrument key, quantity, price, currency,
  external id }` or a row message (`skipped` / `error` with key and detail). Numbers via
  `csv_import::amounts` (thousands separators, decimal consensus, currency signs), dates strict per
  file (`date_parser::parse_date_to_timestamp_strict`; `%Y%m%d` support added).
- `instruments` — unique instruments, matching to existing positions by ticker, and the Yahoo
  resolution (`price_api` search by ISIN or symbol, preferring the listing in the trade currency;
  quote currency for "podle burzy"). Network calls run outside the database lock.
- `simulate` — duplicates (rule 1: same source and broker id; rule 2: same ticker, day, direction,
  quantity and price compared numerically) and the holdings/currency rules from §3.
- `import` — writes through the shared bulk writer and records the batch.
- `batches` — list and undo.
- `formats` — saved custom formats in `app_config` (key `stock_import_formats`, at most 20).

### 4.2 One write path (ADR 0007)

The core of the MCP tool `stock_transactions_create` (validate all, order by date with buys
first, duplicate check, one SQL transaction, all-or-nothing on a business-rule failure) moves into
`services/investments.rs` as the single bulk writer, extended with an optional batch id and
external id per row. The MCP tool and the CSV import both call it; the MCP behaviour and its tests
stay unchanged. A CSV import is atomic: it re-runs the preview's simulation on the same file and
config, excludes exactly the rows the preview excluded, and any rule failure still found at write
time (the data changed in between) aborts the whole import with that row's message.

### 4.3 Commands (thin, `commands/stock_import.rs`)

`inspect_stock_csv(file_path, source?, header_row?, skip_rows?, encoding?)`,
`resolve_stock_instruments(instruments)`, `preview_stock_csv_import(file_path, config)`,
`import_stock_csv(file_path, config)` (then `schedule_stock_history_rebuild` and
`update_todays_snapshot`, emits `recalculation-complete`), `list_stock_import_batches`,
`undo_stock_import_batch(batch_id)`, `list_stock_import_formats`, `save_stock_import_format`,
`delete_stock_import_format`. Types registered in `bindings.rs`, mirrored in `shared/schema.ts`.

### 4.4 Migration `003_stock_import_batches`

- `stock_import_batches (id TEXT PK, file_name TEXT NOT NULL, source TEXT NOT NULL, trade_count
  INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()))`.
- `investment_transactions.import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET
  NULL` and `investment_transactions.external_id TEXT`, with indexes on both.
- Undo deletes the batch's transactions, deletes positions left without any transaction,
  recalculates the affected positions, schedules the history rebuild from the batch's earliest day
  and deletes the batch row. Transactions edited after the import still belong to the batch.

### 4.5 Frontend

- `src/components/stocks/import/`: `StockImportDialog` (orchestrator), `SourceStep`,
  `MappingStep`, `ReviewStep`, `InstrumentsPanel`, `ResultStep`, reusing `WizardSteps`,
  `ColumnSelect`, `RawDataPreview`, `use-file-drop` and the debounced preview pattern.
- Pure helpers with tests in `src/components/stocks/import/import-config.ts` (mapping state ↔
  config, value-map suggestions shown in the UI, summary).
- Mutations in `src/hooks/use-stock-import-mutations.ts`: import and undo invalidate the stock keys
  (`investments`, `investment`, `investment-transactions`, `all-stock-transactions`,
  `dividend-summary`, `stocks-analysis`, `tag-metrics`, `stock-twr`) plus `portfolio-metrics` and
  `cashflow-report`, then `recordSnapshot()` and `portfolio-history` (AGENTS rule 7).
- The old modal, the `import_investment_transactions` command and the orphan `stocks.import.*`
  keys are removed and listed in `docs/DEPRECATED.md`.
- i18n: `stocks.importWizard.*` in both locales, including the broker guides.

## 5. Testing

- Rust: one fixture per adapter in `src-tauri/tests/fixtures/csv/` (synthetic, the brokers' real
  shapes), plus a hand-made table as Czech Excel saves it (semicolon, Windows-1250, decimal comma,
  Czech headers and type words), and a guard test that every adapter detects its fixture, is not
  shadowed by another, and parses it with zero errors and the expected trades; parse tests (numbers with thousands
  separators, decimal comma, currency signs, GBX, sign direction, XTB comment, separator-less
  dates, blank headers); duplicate rules; holdings and currency simulation; atomic import and
  rollback; undo (orphan positions removed, others kept); the moved bulk writer keeps the nine MCP
  bulk tests green; migration snapshot.
- Vitest: the pure UI helpers.
- Verification in the demo profile: each fixture imported end to end (step 1 → 4), re-import shows
  only duplicates, undo restores the previous state, the history badge runs, Czech and English.
- `CONTRIBUTING.md` gains "Adding a broker format" next to the bank preset guide.

## 6. Out of scope

- Fees, dividends, splits and corporate actions (no data model for them).
- Excel files (users save them as CSV).
- A crypto import wizard (the engine is written so that a later crypto adapter set can reuse it).
