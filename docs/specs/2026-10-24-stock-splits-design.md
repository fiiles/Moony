# Stock splits — design

Date: 2026-10-24. Status: **approved** by the owner in chat on 2026-10-24 ("ano tvé návrhy jsou
ok"). The owner's own framing: warn at import when a split fell between the file's first and
last trade so the user checks quantities and prices; when a split happens after the import,
tell the user proactively and, after confirmation, convert the stored transactions too. The
plan is `docs/plans/2026-10-24-stock-splits.md`. This spec is also the hand-off brief: an
implementer should not need the chat. Decisions D18–D24 and the refinements in section 3 came
from a builder check of the first draft; they are the author's calls.

## 1. Problem

Moony has no notion of a stock split. A position's quantity is the sum of buys minus sells
(`services/investments.rs`, `services/cost_basis.rs`); the only transaction types are `buy` and
`sell`. Three things go wrong around a split (say 4:1):

1. **Split after the import.** Yahoo starts returning the post-split price, Moony keeps the
   pre-split quantity, the position's value shows at a quarter. Yahoo's historical `close` is
   split-adjusted for the whole series, so the history rebuild (`services/history_recalc.rs`,
   `services/ticker_history.rs`) multiplies the unadjusted quantity by an adjusted price and the
   history before the split is wrong too.
2. **Split row inside the import file.** The CSV wizard's type classifier
   (`stock_import/columns.rs`, `suggest_type_action`) skips anything that is not a purchase or
   a sale, so the split row ends up in "skipped" as `importWizard.row.notATrade` and the trades
   before it are imported in pre-split units. Same outcome as 1.
3. **File already adjusted by the broker.** Nothing to fix; Moony is right. The danger is a
   split mechanism that converts such rows a second time.

Nothing in the code reads Yahoo's split events today, although every chart request the app
makes already asks for `events=div|split|capitalGains` and the `yahoo_finance_api` crate
exposes them (`YResponse::splits() -> Vec<Split { date, numerator, denominator, split_ratio }>`).

## 2. Goals and non-goals

Goals:

- Every stored stock transaction is in **current share units**. A split is applied by
  rewriting the transactions dated before its effective day: quantity × factor, price ÷ factor.
  Cost basis is unchanged; quantity, average price, history, TWR/MWR and sells all become
  consistent with Yahoo's adjusted prices without touching the computation layer.
- Detect splits automatically from the Yahoo responses Moony already fetches plus one cheap
  daily check per held ticker, remember them, and tell the user proactively through the
  existing agenda ("Co vás čeká") plus a banner on the position's detail page. Nothing is
  rewritten without the user's confirmation.
- Tell apart "already adjusted" from "pre-split" transactions by comparing the purchase price
  with Yahoo's adjusted close of that day, so the confirmation can be pre-filled correctly
  and a second conversion is never proposed by default.
- In the import wizard: when a split falls inside a ticker's trade range in the file, say so,
  classify the file's trades the same way, and offer the conversion at import time.
- A manual "Record a split" action for tickers Yahoo does not know or when detection fails.
  Reverse splits use the same mechanism with a factor below 1.
- An applied split can be reverted (the inverse rewrite) in case the user confirmed wrongly.

Non-goals:

- Fractional shares, cash in lieu and rounding at reverse splits: Moony applies the exact
  ratio; the user adjusts the residual by editing a transaction.
- Crypto (no splits), bonds, other assets.
- MCP: no new tool. `stock_transactions_create` keeps importing what it is given.
- Changing dividend history semantics. Yahoo dividend amounts are per current share already.
- The mobile app's own UI. The new milestone kind must not break the mobile agenda
  (`src/mobile/components/AgendaRow.tsx` holds the per-kind mapping), that is all.
- A split "transaction type". Splits are a separate record, never a row in
  `investment_transactions`.
- A split marker on rewritten transaction rows. An "applied splits" line above the ledger is
  enough.

## 3. Decisions

| # | Decision |
|---|---|
| D1 | **Units.** Stored `investment_transactions.quantity` and `price_per_unit` are in today's share units. Applying a split rewrites every transaction of the ticker with `transaction_date < effective_day` (UTC midnight of the ex-date): `quantity' = quantity × n / d`, `price' = price × d / n`. Transactions dated on the ex-date or later are post-split and untouched. Units are a property of the date, not of when the row was entered: revert (D10) rewrites the same set of rows at revert time, including rows added after the apply. |
| D2 | **Factor.** `n:d` means one old share becomes `n/d` new shares. Yahoo's `Split { numerator: n, denominator: d, split_ratio: "n:d" }` (AAPL 2020-08-31 is `4, 1, "4:1"`; the crate's doc comment describes it the other way round and is wrong). The implementer records a real Yahoo payload as a test fixture and the parser cross-checks `split_ratio` against `numerator/denominator`; a disagreement is logged and the event is dropped. A reverse split is `1:10` (`n=1, d=10`). |
| D3 | **Exact arithmetic on the stored text.** The apply/revert rewrite uses base-ten digit arithmetic on the stored decimal text (no `f64`): multiply the digits by one integer, divide by the other, and when the quotient does not terminate within 8 fractional digits round **buys half-up and sells down (toward zero)**, so a history that was valid stays valid (sells never exceed holdings after rounding); prices round half-up. The result goes through `dedup::canonical_amount`. Yahoo's `n` and `d` are read as integers; a non-integer value is dropped with a log line. The import path (D13) works on the wizard's `f64` trades and renders through `amount_to_text`, as every other import rule does. |
| D4 | **Record.** New table `stock_splits` (migration 023): one row per `(ticker, effective_day)`, with the ratio, where it came from (`yahoo` / `manual` / `import`), its status (`pending` / `applied` / `dismissed`), when it was detected and resolved, and how many transactions the apply rewrote. Section 5. |
| D5 | **Detection.** Three places parse `splits()` from a chart response and call `stock_splits::record_detected`: (a) the per-ticker history rebuild (`price_api::historical_stock_points` → a new return struct carrying the splits; `history_recalc::rebuild_stock_ticker_with_policy`); (b) `refresh_dividends` (366-day window, every 30 days); (c) a new daily split check in the quote refresh (D19). An upsert never changes an existing row's status. |
| D6 | **Relevance.** A detected split is stored only when the ticker has at least one transaction dated before its effective day. Splits before the first transaction are irrelevant and not stored. The relevance check also runs when the agenda is built and when splits are listed, so a split whose pre-split transactions were all deleted disappears from both. |
| D7 | **Classification ("already adjusted?").** For each pre-split transaction with a Yahoo close for its day in the same currency: `r = price_per_unit / close`. `r` within `[0.7, 1.4]` → `adjusted`; `r` within `[0.7 × n/d, 1.4 × n/d]` → `unadjusted`; otherwise, no close, or a currency mismatch → `unknown`. When the two bands overlap (`0.5 < n/d < 2`, e.g. 3:2) every transaction is `unknown`. The split's verdict is `unadjusted` when at least one transaction is `unadjusted` and none is `adjusted`; `adjusted` when at least one is `adjusted` and none is `unadjusted`; otherwise `unknown`. |
| D8 | **Notice.** New milestone kind `stock_split` (info tone, no due date, not in the top bar) built from `pending` rows: "NVDA: split 10:1 (10. 6. 2024) — 3 transakce před splitem". Key `stock_split:<split id>` (no trailing day, so the snooze cap in `last_shown_day` does not apply). It links to the position's detail. Hide and mute work as for every kind and do not change the split's status; "remind later" is not offered (as for `watch_target`, `canRemindLater` is false for an info row). Settings group: `targets` (next to watchlist targets). |
| D9 | **Resolution UI.** The detail page shows a banner for each pending split with the verdict and two buttons: **Přepočítat transakce** (default when the verdict is `unadjusted`), **Už je zohledněno** (default when `adjusted`; with `unknown` neither is emphasised and the banner says Moony could not tell). A new "Splity" dialog, opened from the detail page's "…" menu ("Zaznamenat split"), lists the ticker's splits with status and actions (apply / dismiss / revert / delete-manual) and has a form for a manual split (ex-date, n, d). |
| D10 | **Apply.** One SQL transaction: rewrite the pre-split transactions (D1, D3); scale the ticker's per-share side data by `d/n` where its `updated_at < effective_day` (`stock_price_overrides.price`, `watched_stocks.target_price`, `dividend_overrides.yearly_dividend_sum`) without touching `updated_at`; delete the ticker's `dividend_data` cache row; set the split `applied` with the rewritten count; `ensure_valid_holdings_history` (a failure aborts the whole transaction with `validation.stockSplitOversold`); `recalculate_investment_metrics`. Then, outside the DB lock, update today's snapshot and queue `Job::StockTicker(ticker, first_transaction_day)` so the whole ticker history is rebuilt against the adjusted closes (ADR 0016). Revert is the same with `n` and `d` swapped, the same side-data rule, and status back to `pending`. Dismiss only sets the status. |
| D11 | **Manual split.** `InsertStockSplit { ticker, effective_day, numerator, denominator }` with `validate()`: positive integers, `n ≠ d`, each ≤ 1 000 000, `effective_day` a UTC midnight not in the future. Creating a manual split applies it immediately (D10) in one command. A manual split on a `(ticker, day)` that Yahoo already reported is the same row: the existing row is applied instead of inserting a duplicate. |
| D12 | **Import: lookup is backend-parsed.** New command `check_stock_splits(file_path, config) -> StockSplitCheck[]`, one call for the whole file, outside the DB lock for the network part: it parses the file and runs `simulate` with the config as given **but with every `splits` override removed**, groups the trades that would be imported by ticker, fetches Yahoo history for `[first trade day, today]` per ticker (the existing chunked fetch), returns the splits dated after the first trade with the D7 classification per trade and per split, and writes nothing. A ticker whose fetch fails (offline, unknown symbol) is reported `checked: false`; the wizard says splits could not be checked for it. The wizard runs it once per file after instrument resolution finishes and again when an instrument's ticker override changes (debounced like the preview). |
| D13 | **Import: choice and preview.** `StockInstrumentOverride` gains `splits: [{ effectiveDay, numerator, denominator, action: 'convert' \| 'keep' }]`, filled by the wizard from D12 with the default from the verdict (`unadjusted` → convert, else keep). `simulate` scales a file trade dated before a `convert` split before anything else runs, so duplicate detection and the sell-within-holdings check see converted numbers. The review step shows, per instrument, a split line with the ratio, date, verdict and a switch; the summary strip counts converted rows. |
| D14 | **Import: record and stored rows.** `import` records each split of an instrument into `stock_splits` with source `import` and status `applied` (convert) or `dismissed` (keep) when no row for `(ticker, day)` exists. When a row exists it keeps its status, except: a `convert` action on a split that is `pending` in Moony (the ticker already holds pre-split rows) **also applies the split to the stored rows in the same import transaction** (D10's rewrite, side data and status), and `simulate` replays the stored rows scaled the same way so the sell check is consistent. The review step's split line says "přepočítá i N uložených transakcí" in that case. A `keep` action leaves a pending split pending; the agenda notice remains the place to resolve it. Undo of the batch removes the transactions as today and leaves the split rows. |
| D15 | **Import: the split row.** `columns.rs` gets `SPLIT_WORDS` (`split`, `štěpení`, `aktiensplit`, `división`, `frazionamento`, `splitt`, `podział`, `分割`, `拆股`); a type value with one of them is skipped as `importWizard.row.splitRow` instead of `notATrade` (the existing parse test that expects `notATrade` for `split` changes accordingly). It is a row message only: the instrument list is built from trades, so no per-instrument flag. It changes no default. |
| D16 | **Existing users.** `REBUILD_VERSION` goes to 4 so the first start after the update rebuilds every ticker from its first transaction day, which is exactly the full-range fetch that detects past splits and rewrites `stock_value_history` with adjusted closes. Users then get the agenda notice for each affected ticker. |
| D17 | **Duplicate detection across a split.** After an apply, re-importing the original pre-split file matches only through `external_id` or, after the wizard converts the rows (D13), through the numeric rules. This is accepted; the wizard's default for an `unadjusted` verdict is convert, so the common case still deduplicates. |
| D18 | **Fresh closes for the verdict.** The verdict of a stored split is computed on read from `stock_value_history.price`, which holds adjusted closes only after a rebuild that covered the pre-split days. So whenever `record_detected` inserts a **new** pending row, the caller queues `Job::StockTicker(ticker, first_transaction_day)` (the rebuild worker queues it for itself through the coalescing queue; the dividend and quote refresh commands queue it the way `rebuild_history_for_unit_changes` does). Until that rebuild lands the verdict may read `adjusted`; the banner therefore also shows the ratio and the date so the user can judge, and the dialog can always override. |
| D19 | **Daily split check.** Migration 023 also adds `stock_data.splits_checked_at INTEGER`. In `refresh_stock_prices_yahoo_with_ttl`, after a ticker's quote is fetched, when `splits_checked_at` is older than 24 h the ticker gets one more request, `get_quote_range(ticker, "1d", "1mo")`, its splits go to `record_detected`, and `splits_checked_at` is set. A fresh split is therefore noticed within a day of use, not within the 30-day dividend cycle. |
| D20 | **Entering transactions dated before an applied split.** The Buy, Sell and Edit transaction modals show a one-line hint when the chosen date is before an applied split of the ticker: "K {date} proběhl split {n}:{d}; zadejte množství a cenu v dnešních jednotkách." No conversion, no block. |
| D21 | **Cached quote after apply.** The apply mutation on the frontend triggers the same forced price refresh as the detail page's Refresh button, so `stock_data` and the monitor do not show a stale pre-split quote for up to 4 hours. |
| D22 | **Verdict for the import check uses the fetched series directly** (D12), never `stock_value_history`. |
| D23 | **`STOCK_TRANSACTION_KEYS`** is exported from `use-investment-mutations.ts` and reused by the split mutations. |
| D24 | **Demo profile.** `seed_demo.rs` replaces the NVDA entry: a buy of 3 shares at 950 USD dated 900 days before today (before the 10:1 split of 2024-06-10) plus the existing-style later buy (20 at 120 USD, 200 days ago), current price 178.90. A freshly seeded demo has no rebuild-version flag, so the first start with network rebuilds, detects the split and shows the notice. |

## 4. Architecture

```
Yahoo chart response ──► price_api: HistoricalSeries { prices, splits }
        │                            │                       │
        │ history rebuild            │ refresh_dividends     │ daily split check (D19)
        ▼                            ▼                       ▼
   stock_splits::record_detected(conn, ticker, &splits) -> Vec<new rows>   (D5, D6)
        │                     └──► new row ⇒ Job::StockTicker(ticker, first_tx_day) (D18)
        ├──► milestones::stock_splits builder ──► agenda row (D8) ──► StockDetail banner (D9)
        │
        └──► commands: list_stock_splits / apply_stock_split / dismiss_stock_split /
             revert_stock_split / record_stock_split (manual) / delete_stock_split
                       │
                       ▼
             stock_splits::apply(conn, split, direction)  (D1, D3, D10)
                       │
                       └──► update_todays_snapshot + Job::StockTicker(ticker, first_day)

Import wizard: resolve instruments ──► check_stock_splits(file, config) (D12, network, no DB write)
               ──► overrides.splits (D13) ──► preview/simulate (scaled trades, scaled stored state)
               ──► import ──► bulk_create_stock_transactions + apply pending (D14) + stock_splits rows
```

### 4.1 Backend

New service module `src-tauri/src/services/stock_splits/`:

| File | Responsibility |
|---|---|
| `mod.rs` | `list_for_ticker`, `record_detected` (D5/D6/D18), `apply`, `revert`, `dismiss`, `record_manual`, `delete_manual`, `applied_before(conn, ticker, day)` (D20); every function takes `&Connection` |
| `scale.rs` | `scale_amount(text, mul: u64, div: u64, rounding) -> Result<String>` exact decimal (D3) |
| `classify.rs` | D7: `classify(price, close, n, d) -> Verdict`, `verdict_of(&[Verdict]) -> Verdict`, pure |
| `types.rs` | `StockSplit`, `InsertStockSplit`, `StockSplitVerdict`, `StockSplitApplyResult`, `DetectedSplit` (section 6) |

Touched existing code:

- `price_api.rs`: `historical_stock_points` returns prices and splits; the public history fetch
  gains a sibling that returns `HashMap<String, HistoricalSeries>` while the benchmark caller
  keeps `Vec<HistoricalPrice>`; `refresh_dividends` and the quote refresh (D19) collect
  `DetectedSplit`s and return them beside their results (the `unit_changed` precedent).
- `history_recalc.rs`: after a successful ticker fetch, `record_detected`; a new row queues the
  full-range job (D18) through the existing `schedule`.
- `commands/price_api.rs`: `refresh_stock_prices` and `refresh_dividends` queue the jobs for
  new rows the way `rebuild_history_for_unit_changes` does.
- `milestones/`: new builder `stock_splits.rs`, kind registered in `MILESTONE_KINDS`.
- `stock_import/`: `columns.rs` + `parse.rs` (D15), `types.rs` (override field, counts),
  `simulate.rs` (scaling of file trades and of the stored state), `import.rs` (D14), new
  `splits.rs` for `check_stock_splits` (D12) with the Yahoo call injectable like
  `resolve_with`, commands in `commands/stock_import.rs`.
- `commands/stock_splits.rs` (new): the six commands, registered in `lib.rs`; write commands
  follow `create_investment_transaction` (validate → `db.with_conn` → `update_todays_snapshot`
  → `schedule_history_rebuild` → `recalculation-complete`).
- `history_plan.rs`: `REBUILD_VERSION = 4` (D16).
- `bin/local_anonymize_profile.rs`: the new table carries tickers only; the implementer checks
  whether tickers are anonymised there and treats `stock_splits.ticker` the same way.
- `bin/seed_demo.rs`: D24.

### 4.2 Frontend

- `src/lib/tauri-api.ts`: `stockSplitsApi` (list, apply, dismiss, revert, record, remove) and
  `stockImportApi.checkSplits`.
- `src/hooks/use-stock-splits.ts` (query `['stock-splits', ticker]`) and
  `src/hooks/use-stock-split-mutations.ts` following `use-investment-mutations.ts`: invalidate
  `STOCK_TRANSACTION_KEYS` + `['stock-splits']` + `['milestones']` + `['portfolio-metrics']` +
  `['cashflow-report']`, then `recordSnapshot()` + `['portfolio-history']` (rule 7), then the
  forced price refresh (D21).
- `src/components/stocks/StockSplitBanner.tsx` (D9) rendered on `src/pages/StockDetail.tsx`
  under `ConversionQualityNote` (imported from `@/components/reports/ReportQualityNote`);
  `src/components/stocks/StockSplitsDialog.tsx` (list + manual form) opened from the "…" menu;
  the D20 hint in `BuyInvestmentModal`, `SellInvestmentModal`, `EditTransactionModal`.
- Milestones: `MilestoneKind` union in `shared/schema.ts`, `GROUP_OF` + `INFO_KINDS` +
  `milestoneHref` (→ `/stocks/:id`) in `src/utils/milestones.ts`, the sub-line in
  `src/hooks/use-milestone-sub-line.ts`, the settings switch label, the mobile
  `AgendaRow.tsx` mapping.
- Import wizard (`src/components/stocks/import/`): `StockImportDialog.tsx` owns the split check
  state (runs `checkSplits` when resolution finishes and on ticker override changes);
  `import-config.ts` carries `splits` in the overrides and the default action from a verdict
  (`defaultSplitAction(verdict)`); `ReviewStep.tsx` / `InstrumentEditor` show the split line
  and switch; `row-messages.ts` knows `importWizard.row.splitRow`; `ResultStep.tsx` shows the
  converted tile.

## 5. Data model

Migration `023_stock_splits`:

```sql
CREATE TABLE IF NOT EXISTS stock_splits (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL,
    effective_day INTEGER NOT NULL,          -- UTC midnight of the ex-date
    numerator TEXT NOT NULL,                 -- integer as text, e.g. '4'
    denominator TEXT NOT NULL,               -- integer as text, e.g. '1'
    source TEXT NOT NULL CHECK (source IN ('yahoo', 'manual', 'import')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'applied', 'dismissed')),
    detected_at INTEGER NOT NULL DEFAULT (unixepoch()),
    resolved_at INTEGER,
    applied_count INTEGER,                   -- transactions rewritten by the last apply
    UNIQUE (ticker, effective_day)
);
CREATE INDEX IF NOT EXISTS idx_stock_splits_ticker ON stock_splits(ticker);
ALTER TABLE stock_data ADD COLUMN splits_checked_at INTEGER;
```

`ticker` is the uppercase Moony ticker (`stock_investments.ticker`), not a FK: a position may be
deleted and recreated, and the split record is still true. `docs/architecture/database.md`
gets the table and the column; `schema.snapshot` is regenerated.

## 6. Wire types

All new Rust types use per-field `#[serde(rename = "camelCase")]`, `specta::Type`, are
registered in `bindings.rs`, regenerated into `generated-types.ts` and hand-mirrored into
`shared/schema.ts` (rules 2 and 3).

```ts
type StockSplitStatus = 'pending' | 'applied' | 'dismissed';
type StockSplitSource = 'yahoo' | 'manual' | 'import';
type StockSplitVerdict = 'adjusted' | 'unadjusted' | 'unknown';
type StockSplitAction = 'convert' | 'keep';

interface StockSplit {
  id: string; ticker: string; effectiveDay: number;
  numerator: string; denominator: string;           // integers as text
  source: StockSplitSource; status: StockSplitStatus;
  detectedAt: number; resolvedAt: number | null; appliedCount: number | null;
  preSplitCount: number;                             // transactions dated before effectiveDay, now
  verdict: StockSplitVerdict;                        // D7 from stock_value_history; 'unknown' unless pending
}
interface InsertStockSplit { ticker: string; effectiveDay: number; numerator: string; denominator: string }
interface StockSplitApplyResult { split: StockSplit; rewritten: number; earliestDay: number | null }

// import wizard
interface StockSplitCheckSplit {
  effectiveDay: number; numerator: string; denominator: string;
  verdict: StockSplitVerdict;
  trades: { line: number; verdict: StockSplitVerdict }[];
  storedStatus: StockSplitStatus | null;            // the stock_splits row of (ticker, day), if any
  storedPreSplitCount: number;                      // stored rows dated before the day (D14 line)
}
interface StockSplitCheck { key: string; ticker: string; checked: boolean; splits: StockSplitCheckSplit[] }
interface StockSplitOverride { effectiveDay: number; numerator: string; denominator: string; action: StockSplitAction }
// StockInstrumentOverride gains: splits?: StockSplitOverride[]
// StockImportCounts gains: converted: number
// StockImportResult gains: converted: number
```

`Milestone` is unchanged: a split row is `kind: 'stock_split'`, `sourceId: <investment id>`,
`title: <ticker>`, `count: preSplitCount`, `direction: '<n>:<d>'` (the ratio text),
`actionDay: effectiveDay`.

## 7. Behaviour details

- **Ex-date day.** Yahoo's `date` is a timestamp around market open of the ex-date;
  `effective_day` is the UTC midnight of that calendar day (the `floor_day` helpers in the
  history code; `models::validation::utc_day` on the validation side). Transactions are
  compared by `transaction_date < effective_day`.
- **Ticker currency vs Yahoo currency.** Closes in `stock_value_history` and in the fetched
  series are already in the position's currency (pence normalised to pounds). D7 compares only
  when `transaction.currency == close currency`.
- **Apply twice.** `apply` refuses a split that is not `pending` (`validation.stockSplitNotPending`);
  `revert` refuses one that is not `applied` (`validation.stockSplitNotApplied`); `delete`
  refuses a non-manual or applied row (`validation.stockSplitNotDeletable`).
- **Deleting a position** leaves `stock_splits` rows; D6 makes them invisible until pre-split
  transactions exist again.
- **A split with no pre-split rows at apply time** (user deleted them) is set `applied` with
  `applied_count = 0`.
- **Toasts.** Apply: "NVDA: 3 transakce přepočítány (10:1)" with a "Vrátit" action calling
  revert, the pattern of `use-milestone-mutations.ts`. Dismiss: plain toast.
- **Import summary.** The result step counts converted rows in a tile ("Přepočteno kvůli
  splitu: 5").
- **Offline.** Detection simply does not happen; the manual path works. `check_stock_splits`
  returns `checked: false` per ticker and the wizard shows one line "Splity se nepodařilo
  ověřit".

## 8. i18n

All strings in all ten locales (rule 8): `stocks.json` for the banner, dialog, toasts, wizard
split line, the D20 hint and `importWizard.row.splitRow`; `common.json` for
`validation.stockSplit*` keys (`validation.stockSplitRatioInvalid`,
`validation.stockSplitDayInvalid`, `validation.stockSplitNotPending`,
`validation.stockSplitNotApplied`, `validation.stockSplitNotDeletable`,
`validation.stockSplitOversold`); the milestone kind label and sub-line in `milestones.json`
as the other kinds. `src/i18n/locales.test.ts` enforces parity.

## 9. Testing

- `scale.rs`: exact products and quotients, the non-terminating rounding rules per direction,
  canonical output, `n/d` ≥ 1 and < 1, large quantities, the 1:3 oversold example stays valid.
- `classify.rs`: the bands of D7, overlap case, currency mismatch, missing close.
- `stock_splits/mod.rs` service tests with an in-memory `Connection` and a hand-written schema
  (`investment_transactions`, `stock_investments`, `stock_splits`, `stock_value_history`,
  `stock_price_overrides`, `watched_stocks`, `dividend_overrides`, `dividend_data`): apply
  rewrites only pre-split rows, cost basis invariant, side data scaled only when older than
  the ex-date, `dividend_data` row deleted, revert restores the original text exactly for
  terminating ratios, `record_detected` ignores irrelevant splits, never downgrades a status,
  and reports only new rows; D6 on read; `applied_before`.
- `price_api.rs`: a captured Yahoo fixture with an events block parses the split; a
  disagreeing `split_ratio` is dropped.
- `milestones/stock_splits.rs`: builder output, D6, key format.
- `stock_import`: `columns.rs` split words; `parse.rs` split row key; `simulate` scales
  converted trades and the stored state and the sell check passes with converted quantities;
  `import` records split rows, applies a pending split to stored rows (D14) and leaves an
  existing non-pending row alone; `check_stock_splits` classification against a stubbed series.
- Migration 023 test in `migrations.rs` following `stock_import_batch_tests`.
- Frontend: `src/utils` / `import-config.ts` helpers only (`defaultSplitAction`), per the
  testing policy; components are out of scope.

## 10. Left to the builder

- Exact layout and copy of the banner, dialog, wizard line and D20 hint within the design
  system (`docs/design-system/README.md`).
- The name and internals of the exact-decimal helper (`walk_holdings_history` only adds and
  subtracts digits, so multiply/divide is new code).
- How `simulate` carries the scaled stored state (a scaled copy of `TickerState` is the
  obvious shape).
- Ordering of the plan's tasks and the commit granularity.
