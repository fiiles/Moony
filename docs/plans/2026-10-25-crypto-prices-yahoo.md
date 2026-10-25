# Crypto prices from Yahoo Finance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Price crypto holdings through Yahoo Finance only (current price, coin search, daily
history) and remove every trace of CoinGecko: code, columns, API key, settings card, banner,
strings and docs.

**Architecture:** A holding stores its Yahoo USD pair symbol (`crypto_investments.yahoo_symbol`,
`BTC-USD`, `SUI20947-USD`), chosen in the search or resolved lazily by name and ticker without
guessing. A new `services/crypto_symbols.rs` owns the pure symbol logic (display ticker, USD
pair, search parsing, the chooser) and the lazy resolution; `services/price_api.rs` keeps the
Yahoo fetches (one chart request per holding, one history request per rebuild) and loses its
CoinGecko half. The frontend swaps the CoinGecko id for the symbol, drops the API key card and
banner, and gains a symbol picker reachable from a banner and the position menus.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher, `yahoo_finance_api` 5, `reqwest` through
`services/http.rs`, `serde_json`), React 19 + TypeScript, TanStack Query, i18next, Vitest.

**Spec:** `docs/specs/2026-10-25-crypto-prices-yahoo-design.md`. Decisions D1–D20 and §1
(the probe findings) below refer to it. The owner approved the direction in chat on 2026-10-25.

## Global Constraints

- Work in the sibling worktree `../Moony-tauri-yahoo-crypto` on branch `feat/yahoo-crypto`
  (D19). Never commit in the shared checkout; check `git branch --show-current` before every
  commit.
- The ten rules of `AGENTS.md`. Read `docs/standards/rust-backend.md`,
  `docs/standards/type-contract.md`, `docs/standards/i18n.md`, `docs/standards/testing.md`,
  `docs/DEPRECATED.md` before coding.
- Money and quantities are TEXT; timestamps unix seconds; PKs TEXT UUIDs; no `.unwrap()`
  outside tests; services take `&Connection`; commands are thin; errors are `AppError`.
- A Yahoo symbol is valid when it is 3–40 ASCII characters of `A–Z`, `0–9`, `.`, `-` and ends in
  `-USD` (D2); commands and the MCP tool trim and uppercase input before validating.
- The stored crypto price is always `USD`; a quote reported in another currency is skipped (D7).
- No network in migrations or tests. Network functions are not unit-tested (as today); the
  pure functions around them are.
- Every new or changed user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`,
  `it`, `nb`, `pl`, `sv`, `ja`, `zh`) in the same commit; `npm test` runs
  `src/i18n/locales.test.ts`. The word "CoinGecko" must not remain in any locale file.
- After any change to a type registered in `bindings.rs`: `cd src-tauri && cargo test
  generate_bindings -- --ignored`, then hand-mirror into `shared/schema.ts`.
- Frozen docs (`docs/specs/`, `docs/plans/`, `docs/audits/`) are never edited; the new spec
  and this plan are the exception.
- Gate per task before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run typecheck && npm test
```

- One Conventional Commit per task, ending with
  `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Never type a password into any app window that does not greet "Alex" (demo profile). Demo
  runs from the worktree use port 1422 and a worktree copy of `demo-home`.

## Review Focus

1. A migrated holding whose plain `TICKER-USD` is another coin (`ARB`, name "Arbitrum") must
   resolve to `ARB11841-USD`, never be priced as "ARbit" (Task 2 chooser test, Task 4).
2. A holding Yahoo does not know at all stays unpriced with `yahooSymbol: null`, is listed in
   the banner, and never blocks the refresh of the other holdings (Task 4, Task 9).
3. A search row that is only a non-USD pair (`POL28321-EUR`) is offered as `POL28321-USD`
   (Task 2 parser test).
4. A durable MCP follow-up for crypto keeps its retry intent when one holding fails, exactly as
   today (Task 5 worker, Task 3 tests).
5. `update_crypto_price` callers (desktop, mobile) still work after the fourth argument is gone
   (Task 7 typecheck).

---

## Phase A — backend

### Task 1: Migration 025 `crypto_yahoo_symbol`

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (append `MIGRATION_025`, Vec entry, test)
- Modify: `src-tauri/src/db/schema.snapshot` (regenerated)
- Modify: `docs/architecture/database.md` (rows `crypto_investments`, `crypto_prices`,
  `user_profile`, `app_config`)
- Modify: `src-tauri/src/services/onboarding.rs` (test `only_whitelisted_flags_can_be_set`
  uses `'not_a_flag'` instead of `'api_key_coingecko'`)
- Modify: `src-tauri/src/services/backup.rs` (test seed row `('some_other_setting', 'x')`
  instead of the CoinGecko key)
- Modify: `src-tauri/src/bin/seed_demo.rs` (the `crypto_investments` insert writes
  `yahoo_symbol` `'BTC-USD'` / `'ETH-USD'` and no `coingecko_id`; the `crypto_prices` insert
  drops `coingecko_id`)

**Interfaces:**
- Produces: columns `crypto_investments.yahoo_symbol TEXT NULL`, no `coingecko_id` on
  `crypto_investments` or `crypto_prices`, no `user_profile.coingecko_modal_dismissed`, no
  `app_config` row `api_key_coingecko`.

- [ ] **Step 1: Write the failing migration test** in the `tests` module of `migrations.rs`,
  next to `migration_024_adds_a_nullable_snooze_column`:

```rust
#[test]
fn migration_025_moves_crypto_identity_to_the_yahoo_symbol() {
    // database_at("024_categorize_snooze") seeds: one crypto_investments row ('c1','BTC','bitcoin',
    // 'Bitcoin','1'), one crypto_prices row for BTC with coingecko_id 'bitcoin', the user_profile
    // row with coingecko_modal_dismissed = 1, app_config ('api_key_coingecko','cg-secret') and
    // ('fx_reference','EUR'); then apply_migrations
    let columns = |table: &str| -> Vec<String> { /* PRAGMA table_info names */ };
    assert!(columns("crypto_investments").contains(&"yahoo_symbol".to_string()));
    assert!(!columns("crypto_investments").contains(&"coingecko_id".to_string()));
    assert!(!columns("crypto_prices").contains(&"coingecko_id".to_string()));
    assert!(!columns("user_profile").contains(&"coingecko_modal_dismissed".to_string()));
    let symbol: Option<String> = conn.query_row("SELECT yahoo_symbol FROM crypto_investments WHERE ticker='BTC'", [], |r| r.get(0)).unwrap();
    assert_eq!(symbol, None);
    let price_rows: i64 = conn.query_row("SELECT COUNT(*) FROM crypto_prices", [], |r| r.get(0)).unwrap();
    assert_eq!(price_rows, 1);
    let keys: i64 = conn.query_row("SELECT COUNT(*) FROM app_config WHERE key='api_key_coingecko'", [], |r| r.get(0)).unwrap();
    assert_eq!(keys, 0);
    let fx: i64 = conn.query_row("SELECT COUNT(*) FROM app_config WHERE key='fx_reference'", [], |r| r.get(0)).unwrap();
    assert_eq!(fx, 1);
}
```

Use the same "database before migration N" helper the 022–024 tests use (filter
`all_migrations()` by name `< "025_crypto_yahoo_symbol"`).

- [ ] **Step 2: Run it** — `cd src-tauri && cargo test migration_025` — expected: FAIL
  (no such migration).

- [ ] **Step 3: Add `MIGRATION_025`** with exactly the SQL of spec D3, in that order, and the
  Vec entry `("025_crypto_yahoo_symbol", MIGRATION_025)`.

- [ ] **Step 4: Run the test and the snapshot test** — `cargo test migration_025` PASS;
  `cargo test baseline_matches_schema_snapshot` FAIL; run
  `cargo test regenerate_schema_snapshot -- --ignored`; re-run, PASS. Read the snapshot diff:
  only the four column changes.

- [ ] **Step 5: Fix the three test seeds and the demo seed** (files above); `cargo test`
  PASS. (`cargo check --bin seed_demo` too.)

- [ ] **Step 6: Amend `docs/architecture/database.md`**: `crypto_investments` → "`ticker
  UNIQUE`, `yahoo_symbol` (the Yahoo Finance USD pair, `BTC-USD`, `SUI20947-USD`; NULL until
  resolved, `services/crypto_symbols.rs`); `currency`"; `crypto_prices` → "Fetched Yahoo
  Finance price cache, `symbol UNIQUE`, always USD"; `user_profile` drops
  `coingecko_modal_dismissed`; `app_config` drops the API-key sentence ("no API key is stored
  since migration 025").

- [ ] **Step 7: Commit** — `feat(db): migration 025 stores the Yahoo symbol of a crypto holding`.

### Task 2: `services/crypto_symbols.rs` — the pure symbol logic

**Files:**
- Create: `src-tauri/src/services/crypto_symbols.rs`
- Modify: `src-tauri/src/services/mod.rs` (declare the module)

**Interfaces:**
- Produces:

```rust
pub struct CryptoSearchResult { pub symbol: String, pub ticker: String, pub name: String }
// Serialize + Type, serde rename not needed (single words)
/// `"SUI20947-USD"` → `"SUI"`; `"W-USD"` → `"W"`; `"1INCH-USD"` → `"1INCH"`; `"GRT6719-USD"` → `"GRT"`.
pub fn display_ticker(symbol: &str) -> String
/// `"GRT6719-ETH"` → `"GRT6719-USD"`; `"BTC-USD"` unchanged; a symbol without `-` gets `-USD` appended.
pub fn usd_pair(symbol: &str) -> String
/// D2 rule; used by validate() and the MCP tool.
pub fn is_valid_symbol(symbol: &str) -> bool
/// Trim + uppercase, then validate; Err(AppError::Validation("validation.yahooSymbolInvalid")).
pub fn normalize_symbol(input: &str) -> Result<String>
/// Parse a Yahoo `v1/finance/search` body (D6): CRYPTOCURRENCY rows only, USD pair, deduped by
/// base in Yahoo's order; `name` = shortname with a trailing " <QUOTE>" stripped, else longname, else symbol.
pub fn parse_search_response(body: &str) -> Result<Vec<CryptoSearchResult>>
/// The rows of the ` USD` response followed by the plain response, deduplicated by base (first wins).
pub fn merge_search_results(first: Vec<CryptoSearchResult>, second: Vec<CryptoSearchResult>) -> Vec<CryptoSearchResult>
/// D4 chooser over merged rows: candidates = display-ticker match (case-insensitive); exactly one → it;
/// several → those whose name equals `name` (case-insensitive); exactly one → it; else None.
pub fn choose_symbol(name: &str, ticker: &str, rows: &[CryptoSearchResult]) -> Option<String>
```

- [ ] **Step 1: Write the failing tests** in the module's `tests`:

```rust
#[test] fn display_ticker_strips_a_coinmarketcap_suffix() {
    assert_eq!(display_ticker("SUI20947-USD"), "SUI");
    assert_eq!(display_ticker("GRT6719-USD"), "GRT");
    assert_eq!(display_ticker("BTC-USD"), "BTC");
    assert_eq!(display_ticker("W-USD"), "W");
    assert_eq!(display_ticker("1INCH-USD"), "1INCH");
    assert_eq!(display_ticker("BTC2-USD"), "BTC2"); // fewer than four digits stay
}
#[test] fn usd_pair_replaces_the_quote_currency() {
    assert_eq!(usd_pair("GRT6719-ETH"), "GRT6719-USD");
    assert_eq!(usd_pair("POL28321-EUR"), "POL28321-USD");
    assert_eq!(usd_pair("BTC-USD"), "BTC-USD");
}
#[test] fn symbol_validation_follows_d2() {
    assert!(is_valid_symbol("BTC-USD"));
    assert!(is_valid_symbol("SUI20947-USD"));
    assert!(!is_valid_symbol("btc-usd"));
    assert!(!is_valid_symbol("BTC-EUR"));
    assert!(!is_valid_symbol("-USD"));
    assert!(!is_valid_symbol(&format!("{}-USD", "A".repeat(40))));
    assert_eq!(normalize_symbol(" btc-usd ").unwrap(), "BTC-USD");
    assert!(normalize_symbol("bitcoin").is_err());
}
#[test] fn search_parsing_keeps_coins_as_usd_pairs_in_yahoo_order() {
    // PEPE fixture (spec §1 / the probe): 7 CRYPTOCURRENCY rows
    let rows = parse_search_response(PEPE_BODY).unwrap();
    assert_eq!(rows[0], CryptoSearchResult { symbol: "PEPE24478-USD".into(), ticker: "PEPE".into(), name: "Pepe".into() });
    assert_eq!(rows[1].symbol, "PEP-USD");
    assert_eq!(rows.len(), 7);
    // GRAPH fixture: seven non-USD pairs of one base collapse to one USD pair
    let rows = parse_search_response(GRAPH_BODY).unwrap();
    assert_eq!(rows, vec![CryptoSearchResult { symbol: "GRT6719-USD".into(), ticker: "GRT".into(), name: "The Graph".into() }]);
    // ARBITRUM fixture: ETF and MUTUALFUND rows are dropped
    let rows = parse_search_response(ARBITRUM_BODY).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].symbol, "ARB11841-USD");
    assert!(parse_search_response("not json").is_err());
    assert!(parse_search_response(r#"{"quotes":[]}"#).unwrap().is_empty());
}
#[test] fn the_chooser_takes_a_unique_ticker_match_then_a_unique_name_match_else_nothing() {
    let row = |symbol: &str, ticker: &str, name: &str| CryptoSearchResult { symbol: symbol.into(), ticker: ticker.into(), name: name.into() };
    let arb = vec![row("ARB-USD", "ARB", "ARbit"), row("ARB11841-USD", "ARB", "Arbitrum"), row("ARCT-USD", "ARCT", "ArbitrageCT")];
    assert_eq!(choose_symbol("Arbitrum", "ARB", &arb).as_deref(), Some("ARB11841-USD"));
    assert_eq!(choose_symbol("arbitrum", "arb", &arb).as_deref(), Some("ARB11841-USD"));
    assert_eq!(choose_symbol("ARB", "ARB", &arb), None); // two ticker matches, no name match: no guess
    let btc = vec![row("BTC-USD", "BTC", "Bitcoin"), row("WBTC-USD", "WBTC", "Wrapped Bitcoin")];
    assert_eq!(choose_symbol("anything", "BTC", &btc).as_deref(), Some("BTC-USD")); // unique ticker match wins without the name
    let jup = vec![row("JUP-USD", "JUP", "Jupiter"), row("JUP29210-USD", "JUP", "Jupiter")];
    assert_eq!(choose_symbol("Jupiter", "JUP", &jup), None); // identical names: no guess
    assert_eq!(choose_symbol("Bitcoin", "BTC", &arb), None);
}
#[test] fn merged_search_results_keep_the_first_occurrence_of_a_base() {
    let row = |symbol: &str, ticker: &str, name: &str| CryptoSearchResult { symbol: symbol.into(), ticker: ticker.into(), name: name.into() };
    let merged = merge_search_results(vec![row("ADA-USD", "ADA", "Cardano")], vec![row("ADA-USD", "ADA", "Cardano"), row("POL28321-USD", "POL", "Polygon (prev. MATIC)")]);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[1].symbol, "POL28321-USD");
}
```

The three fixture consts hold the trimmed bodies recorded in the spec's probe (fields
`symbol`, `shortname`, `longname`, `quoteType`, `exchange` only):

- `PEPE_BODY`: `PEPE24478-USD` "Pepe USD", `PEP-USD` "Pepecoin USD", `PEPECOIN-USD`
  "PepeCoin USD", `PURPE-USD` "Purple Pepe USD", `PEPE24549-USD` "Arbi Pepe USD",
  `PORK29220-USD` "PepeFork USD", `DOPE42609-USD` "Doge Pepe (dogepepeeth.lol) USD", all
  `CRYPTOCURRENCY`/`CCC`.
- `GRAPH_BODY`: `GRT6719-ETH`, `-AUD`, `-INR`, `-GBP`, `-JPY`, `-RUB`, `-EUR` with shortname
  "The Graph <QUOTE>".
- `ARBITRUM_BODY`: `AARB.AS` (ETF, no longname field at all in one row), `ARB11841-USD`
  "Arbitrum USD" (CRYPTOCURRENCY), `AARB-EUR.PA` (ETF), `CH1304867463.SG` (MUTUALFUND),
  `VIRARB.ST` (ETF). Both `shortname` and `longname` are optional in the parser.

- [ ] **Step 2: Run** `cargo test crypto_symbols` — FAIL (module missing).

- [ ] **Step 3: Implement the seven functions** with the signatures above. Parsing uses a
  private `#[derive(Deserialize)] struct SearchBody { quotes: Option<Vec<SearchQuote>> }` with
  `symbol: String`, `shortname: Option<String>`, `longname: Option<String>`,
  `#[serde(rename = "quoteType")] quote_type: Option<String>`. A parse error is
  `AppError::ExternalApi("Yahoo search parse error: …")`.

- [ ] **Step 4: Run** `cargo test crypto_symbols` — PASS.

- [ ] **Step 5: Commit** — `feat(crypto): pure Yahoo symbol logic (display ticker, USD pair, search parsing, chooser)`.

### Task 3: Holding identity in models, services and MCP

**Files:**
- Modify: `src-tauri/src/models/crypto.rs` (`CryptoInvestment.yahoo_symbol: Option<String>`
  renamed `yahooSymbol`; `EnrichedCryptoInvestment` same; `InsertCryptoInvestment.yahoo_symbol:
  Option<String>` validated with `crypto_symbols::is_valid_symbol` when `Some` and non-empty →
  `validation.yahooSymbolInvalid`; `CryptoPrice` loses `coingecko_id`; remove the
  `coingeckoIdRequired` check)
- Modify: `src-tauri/src/models/user.rs` (remove `coingecko_modal_dismissed` from both structs)
- Modify: `src-tauri/src/services/auth.rs` (profile read/update without the column; its test
  schema)
- Modify: `src-tauri/src/commands/auth.rs` (the `UpdateUserProfile` literal)
- Modify: `src-tauri/src/services/crypto_investments.rs` (every `coingecko_id` parameter
  becomes `yahoo_symbol: Option<&str>`; conflict → `validation.yahooSymbolConflict`; a new
  coin without a symbol is created with `NULL`, the `coingeckoIdForNewCoin` path goes; new
  `pub fn set_symbol(conn, investment_id: &str, symbol: &str) -> Result<()>` storing the
  normalized symbol; `get_crypto_by_id` reads `yahoo_symbol`; test schemas and the three
  import tests follow: `import_creates_coin_without_symbol`, `import_rejects_conflicting_symbol`,
  `import_existing_coin_ignores_missing_symbol`)
- Modify: `src-tauri/src/services/position_reports.rs` (`yahoo_symbol` into the enriched row)
- Modify: `src-tauri/src/services/positions.rs` (test schema)
- Modify: `src-tauri/src/services/mcp_refresh.rs` (`crypto_identity` returns the symbol;
  `crypto_identities` returns `Vec<(String /*ticker*/, Option<String> /*symbol*/)>`; test schema)
- Modify: `src-tauri/src/services/mcp/crypto.rs` (`CryptoTransactionRow.yahoo_symbol:
  Option<String>` renamed `yahooSymbol` with the D11 description, no `coingecko_id`; the
  batch-conflict and stored-conflict checks compare symbols with the messages
  "`{ticker}: conflicting yahooSymbol values in batch`" and "`{ticker}: yahooSymbol conflicts
  with stored holding`"; `crypto_list` selects `ci.yahoo_symbol` as `"yahooSymbol"`;
  `distinct_tickers` carries the symbol; tests: `new_coin_without_coingecko_id_rejects…` becomes
  `new_coin_without_symbol_is_created_unresolved`, the dedupe and conflict tests rename)
- Modify: `src-tauri/src/services/mcp/mod.rs` (tool descriptions: crypto list "with ticker,
  name, Yahoo Finance symbol, …"; crypto import "supply yahooSymbol (the Yahoo Finance USD pair,
  e.g. BTC-USD or SUI20947-USD) when you know it; otherwise Moony looks the coin up from its
  ticker and name. A yahooSymbol must match the existing holding.")
- Modify: `src-tauri/src/services/mcp/investments.rs` (the comment on line ~340)
- Modify: `src-tauri/src/commands/crypto.rs` (`create_crypto` passes
  `data.yahoo_symbol.as_deref()`; `update_crypto_price` loses `_coingecko_id`;
  `delete_crypto_transaction` and the service's `delete_transaction` return `(date, ticker)`
  instead of the 3-tuple with the id; new command below)
- Note: the profile `SELECT` in `services/auth.rs` reads columns by index; dropping
  `coingecko_modal_dismissed` shifts every later index — switch that mapping to names or
  renumber carefully and keep the existing profile tests green.
- Modify: `src-tauri/src/commands/portfolio.rs` (`trigger_import_recalculation` doc comment:
  the second element is unused)
- Modify: `src-tauri/src/lib.rs` (register `commands::crypto::set_crypto_symbol`, unregister
  `backfill_crypto_ticker_history` once Task 5 removes it — do the registration here, the
  removal in Task 5)
- Modify: `src-tauri/src/constants.rs` (comment: "Yahoo Finance quotes crypto in USD")

**Interfaces:**
- Consumes: `crypto_symbols::{is_valid_symbol, normalize_symbol}` (Task 2).
- Produces:

```rust
// services/crypto_investments.rs
pub fn get_or_create_crypto(conn: &Connection, ticker: &str, yahoo_symbol: Option<&str>, name: &str) -> Result<String>
pub fn create_crypto_with_transaction(conn, ticker, yahoo_symbol: Option<&str>, name, initial_transaction) -> Result<CryptoInvestment>
pub fn set_symbol(conn: &Connection, investment_id: &str, symbol: &str) -> Result<()>  // Err(NotFound) for an unknown id
// services/mcp_refresh.rs
pub fn crypto_identities(conn: &Connection) -> Result<Vec<(String, Option<String>)>> // (ticker, yahoo_symbol)
// commands/crypto.rs
#[tauri::command] pub async fn set_crypto_symbol(app, db, recalc, id: String, symbol: String) -> Result<()>
```

`set_crypto_symbol`: `normalize_symbol`, `set_symbol`, then
`price_api::refresh_crypto_prices_for(&db, &[ticker], true)` (Task 4), then
`commands::price_api::rebuild_crypto_history(&app, &db, &recalc, &[ticker])` (Task 5: queues
`Job::CryptoTicker(ticker, first_transaction_date)` for each ticker that has transactions,
`MIN(transaction_date) FROM crypto_transactions WHERE ticker = ?`), then
`update_todays_snapshot` and `app.emit("recalculation-complete", ())`.

- [ ] **Step 1: Write the failing tests** (service level, in-memory schemas as the existing
  modules do):

```rust
// crypto_investments.rs
#[test] fn a_new_coin_without_a_symbol_is_stored_unresolved() { /* get_or_create_crypto(.., None, "Sui") → yahoo_symbol IS NULL */ }
#[test] fn a_symbol_that_differs_from_the_stored_one_is_a_conflict() { /* stored BTC-USD, pass "ETH-USD" → validation.yahooSymbolConflict */ }
#[test] fn set_symbol_normalizes_and_stores() { /* set_symbol(conn, id, " sui20947-usd ") → "SUI20947-USD"; unknown id → Err */ }
#[test] fn set_symbol_rejects_an_invalid_symbol() { /* "bitcoin" → validation.yahooSymbolInvalid */ }
// models/crypto.rs
#[test] fn insert_crypto_accepts_a_missing_symbol_and_rejects_a_bad_one() { /* None → Ok; Some("btc") → Err */ }
// mcp/crypto.rs: rename/adjust the four existing CoinGecko tests to the symbol semantics (import without symbol creates the coin; conflicting symbol rejects and rolls back)
```

- [ ] **Step 2: Run** `cargo test` — FAIL to compile.
- [ ] **Step 3: Make the changes** listed under Files.
- [ ] **Step 4: Regenerate bindings** — `cargo test generate_bindings -- --ignored`; the diff
  of `shared/generated-types.ts` shows `yahooSymbol` in the crypto types and no
  `coingeckoModalDismissed`.
- [ ] **Step 5: Run** `cargo test` — PASS; `cargo clippy --all-targets -- -D warnings` clean.
- [ ] **Step 6: Commit** — `feat(crypto): a holding is identified by its Yahoo symbol`.

### Task 4: `price_api.rs` — Yahoo search, resolution and the crypto price refresh

**Files:**
- Modify: `src-tauri/src/services/price_api.rs` (remove: `CoinGeckoSearchResult`,
  `CoinGeckoSearchResponse`, `refresh_crypto_prices` (old), `coingecko_error_message`,
  `parse_simple_price_usd`, `search_crypto` (old), `coingecko_history_start`,
  `yahoo_crypto_symbol`, `get_historical_crypto_prices_yahoo`, `merge_crypto_price_series`,
  `get_historical_crypto_prices_coingecko{,_strict,_impl}`, `parse_crypto_history_response`
  and `collect_historical_symbol` if nothing else uses it, `ApiKeys`, `get_api_keys`,
  `set_api_keys`, `coingecko_key_changed`, their tests; the module doc comment)
- Modify: `src-tauri/src/services/crypto_symbols.rs` (add the async resolution)
- Modify: `src-tauri/src/services/http.rs` and `src-tauri/Cargo.toml` (comments: "ECB rates,
  Frankfurter history, Yahoo search")

**Interfaces:**
- Consumes: `crypto_symbols::{parse_search_response, choose_symbol, CryptoSearchResult}`,
  `read_chart_quote`, `yahoo_connector`, `price_text`, `PRICE_CACHE_HOURS`.
- Produces:

```rust
// price_api.rs
pub async fn search_crypto(query: &str) -> Result<Vec<CryptoSearchResult>>
//   two GETs to https://query2.finance.yahoo.com/v1/finance/search?q=<urlencoded>&newsCount=0&listsCount=0,
//   first q = "<query> USD", then q = "<query>" (D6), through crate::services::http::client();
//   each body → crypto_symbols::parse_search_response; merged with merge_search_results;
//   a non-2xx or failed second request only logs at debug when the first succeeded;
//   both failing → AppError::ExternalApi("Yahoo search failed: …")
/// What a crypto refresh did (the wire gets `updated` only; `resolved` drives the rebuild).
pub struct CryptoRefresh { pub updated: Vec<CryptoPriceResult>, pub resolved: Vec<String> }
pub async fn refresh_crypto_prices(db: &Database, force_refresh: bool) -> Result<CryptoRefresh>
//   every holding; D7
pub async fn refresh_crypto_prices_for(db: &Database, tickers: &[String], force_refresh: bool) -> Result<CryptoRefresh>
//   the listed holdings only (set_crypto_symbol, the MCP worker); refresh_crypto_prices calls it with every ticker
// crypto_symbols.rs
/// The holding's symbol: the stored one, else resolved (D4: search_crypto(name), choose; else
/// search_crypto(ticker), choose) and stored. `Resolved::Already(s)` / `Resolved::Now(s)` /
/// `Resolved::None`. A search failure is `None` (best effort, warn log), never an error.
pub enum Resolved { Already(String), Now(String), None }
pub async fn ensure_symbol(db: &Database, ticker: &str) -> Result<Resolved>
```

`refresh_crypto_prices_for`: for each ticker (uppercased): `ensure_symbol`; `None` → skip
(debug log "unresolved"); `Now(_)` → push the ticker into `resolved`; TTL check against
`crypto_prices.fetched_at` with `PRICE_CACHE_HOURS * 3600` unless `force_refresh` (no row →
fetch); `provider.get_quote_range(&symbol, "1d", "5d")`; `read_chart_quote`; skip unless
`quote.currency` is `USD` (case-insensitive); upsert `crypto_prices (id, symbol, price,
currency, fetched_at)` with `ON CONFLICT(symbol) DO UPDATE`; push `CryptoPriceResult`.
Summary logging like the stock refresh (counts at info, names at debug).

- [ ] **Step 1: Write the failing test** for the one pure piece added here:

```rust
#[test] fn a_crypto_quote_in_another_currency_is_not_stored() {
    // crypto_quote_to_store(&ChartQuote { price: 1.0, previous_close: None, currency: Some("EUR".into()) }) == None
    // crypto_quote_to_store(&ChartQuote { price: 80000.5, previous_close: None, currency: Some("usd".into()) }) == Some("80000.5")
    // currency None → None
}
```

with `fn crypto_quote_to_store(quote: &ChartQuote) -> Option<String>` (price text) as the
unit the refresh calls.

- [ ] **Step 2: Run** `cargo test crypto_quote_to_store` — FAIL.
- [ ] **Step 3: Implement** the functions above and delete the CoinGecko half. `cargo test`
  compiles only once Task 5 updates the callers — do Task 5's caller edits in the same working
  tree before running the gate, but keep the commits separate (`git add -p` by file).
- [ ] **Step 4: Run** `cargo test price_api` — PASS (after Task 5 compiles).
- [ ] **Step 5: Commit** — `feat(prices): crypto quotes, search and symbol resolution through Yahoo Finance`.

### Task 5: Callers — history rebuild, commands, MCP worker

**Files:**
- Modify: `src-tauri/src/services/history_recalc.rs` (`rebuild_crypto_ticker_with_policy`
  per D8; `Job::CryptoTicker` doc comment; `crypto_ticker_jobs` unchanged)
- Modify: `src-tauri/src/services/history_plan.rs` (two doc comments mentioning CoinGecko:
  reword to "the provider")
- Modify: `src-tauri/src/services/backfill.rs` (test comment: "Yahoo's daily crypto bars are
  stamped with the UTC day start")
- Modify: `src-tauri/src/commands/price_api.rs` (remove `get_api_keys`, `set_api_keys`;
  `refresh_crypto_prices(app, db, recalc, force_refresh: Option<bool>)` → `let result =
  price_api::refresh_crypto_prices(&db, force_refresh.unwrap_or(false)).await?;
  rebuild_crypto_history(&app, &db, &recalc, &result.resolved); update_todays_snapshot;
  Ok(result.updated)`; new `pub fn rebuild_crypto_history(app, db, recalc, tickers: &[String])`
  next to `rebuild_full_history`, using `history_recalc::crypto_rebuild_jobs` for the
  `(ticker, first transaction)` pairs and `Job::CryptoTicker`; `search_crypto(query)` →
  `price_api::search_crypto(&query)` returning `Vec<CryptoSearchResult>`)
- Modify: `src-tauri/src/commands/portfolio.rs` (`backfill_crypto_ticker_history`: no symbol
  check up front; drop the 365-day cap, `effective_from = from_timestamp`; the rebuild handles
  an unresolved holding (below) and the command then returns `completed: true, message: "No
  Yahoo Finance symbol for this crypto"` when `ensure_symbol` gave `None` — call
  `ensure_symbol` first and branch on it; `get_price_status`: the `crypto_missing` count joins
  only holdings `WHERE yahoo_symbol IS NOT NULL`)
- Modify: `src-tauri/src/commands/mcp_refresh.rs` (the `"crypto"` branch: `let identities =
  db.with_conn(mcp_refresh::crypto_identities)?; let tickers: Vec<String> = identities.iter().map(|(t, _)| t.clone()).collect();
  let result = price_api::refresh_crypto_prices_for(db, &tickers, true).await?;
  let priced = db.with_conn(|c| count of crypto_investments WHERE yahoo_symbol IS NOT NULL)?;
  if result.updated.len() < priced { return Err(AppError::Internal("Crypto price refresh incomplete".into())); }`;
  unresolved holdings are logged at warn and do not hold the job; then the existing rebuild of
  each coin from its first transaction)
- Modify: `src-tauri/src/lib.rs` (unregister `get_api_keys`, `set_api_keys`)

**Interfaces:**
- Consumes: Task 4's functions; `get_historical_stock_prices_yahoo{,_strict}(&[symbol], from, until)`.
- Produces: `commands::price_api::rebuild_crypto_history(app: &AppHandle, db: &Database, recalc: &HistoryRecalc, tickers: &[String])` (used by Task 3's `set_crypto_symbol`).

`rebuild_crypto_ticker_with_policy`: `has_transactions` as today; without transactions →
no prices; with transactions → `crypto_symbols::ensure_symbol(db, ticker).await?`; `None` →
`return Ok(RebuildStats::default())` with a debug log in **both** modes (D8: a retry cannot fix
it); `Already(s) | Now(s)` → fetch `[s]` over `from_day..=until_day` (strict variant in strict
mode, so a Yahoo failure keeps the retry intent), take the symbol's series, keep quotes whose
currency is `USD`; then `plan_ticker_history` as today.

- [ ] **Step 1: Make the edits** (no new pure logic; the existing `rebuild` tests in
  `history_recalc.rs` and `mcp_refresh.rs` must keep passing after their schemas changed in
  Task 3).
- [ ] **Step 2: Run the full backend gate** — `cargo fmt && cargo clippy --all-targets -- -D
  warnings && cargo test` — PASS. `grep -rn -i coingecko src-tauri/src` prints nothing.
- [ ] **Step 3: Commit** — `refactor(history): crypto history and the MCP worker use the Yahoo symbol`.

## Phase B — frontend

### Task 6: Wire contract and API wrappers

**Files:**
- Modify: `shared/schema.ts` (`CryptoInvestment.yahooSymbol: string | null`,
  `EnrichedCryptoInvestment.yahooSymbol: string | null`; remove
  `UserProfile.coingeckoModalDismissed` and from `UpdateUserProfile`)
- Modify: `src/lib/tauri-api.ts` (`cryptoApi.create(data: { ticker; name; yahooSymbol?: string },
  …)`; `cryptoApi.updatePrice(symbol, price, currency)`; keep `backfillHistory` (the detail
  page's `PositionHero` and `src/mobile/hooks/use-ticker-history.ts` call it); new
  `cryptoApi.setSymbol: (id: string, symbol: string) => tauriInvoke<void>('set_crypto_symbol',
  { id, symbol })`; remove `ApiKeys`, `getApiKeys`, `setApiKeys`, `CoinGeckoSearchResult`; add
  `export interface CryptoSearchResult { symbol: string; ticker: string; name: string }`;
  `searchCrypto: (query) => tauriInvoke<CryptoSearchResult[]>('search_crypto', { query })`;
  `refreshCryptoPrices: (forceRefresh?: boolean) =>
  tauriInvoke<CryptoPriceResult[]>('refresh_crypto_prices', { forceRefresh: forceRefresh ?? false })`;
  the section comment "Price API (Yahoo Finance for stocks and crypto)")
- Modify: `src/utils/positions.ts` (`CryptoRow.yahooSymbol: string | null`; `toCryptoRow`)

- [ ] **Step 1: Edit** the three files. `npm run typecheck` now fails in the components
  (expected until Task 7).
- [ ] **Step 2: Commit** — `feat(api): crypto wire contract carries the Yahoo symbol`.

### Task 7: Components — add dialog, settings, refresh buttons, removals

**Files:**
- Modify: `src/components/crypto/AddCryptoModal.tsx` (`CryptoSearchResult`; form field
  `yahooSymbol`; `selectCoin` sets ticker `result.ticker`, name `result.name`, `yahooSymbol`
  `result.symbol`; the list row shows `result.name` and `result.ticker`, no rank; `key`
  `result.symbol`; doc comment; `createInvestment` sends `yahooSymbol: values.yahooSymbol ||
  undefined`; the post-add refresh stays unforced)
- Modify: `src/components/crypto/UpdateCryptoPriceModal.tsx` (three-argument `updatePrice`;
  doc comment "overrides the Yahoo Finance rate")
- Delete: `src/components/settings/ApiKeysCard.tsx`, `src/components/crypto/CoinGeckoApiKeyBanner.tsx`
- Modify: `src/components/settings/SettingsSectionPages.tsx` (`IntegrationsSection` renders
  `McpServerCard` only)
- Modify: `src/components/settings/McpServerCard.tsx` (`t('mcpServer.showToken')` /
  `t('mcpServer.hideToken')` instead of `apiKeys.show/hide`)
- Modify: `src/pages/Crypto.tsx` (remove the banner import and element for now — Task 9 adds
  the new one; `refreshPricesMutation` calls `priceApi.refreshCryptoPrices(true)`)
- Modify: `src/pages/CryptoDetail.tsx` (`handleRefresh` passes `true`; the source eyebrow
  near line 340 shows the source when `yahooSymbol` is set instead of when `coingeckoId` is
  truthy)
- Modify: `src/lib/api-error.test.ts` (the fixture naming `coingeckoId` uses `yahooSymbol`)
- Modify: `src/components/shell/DataStatus.tsx` (passes `true`)
- Modify: `src/mobile/hooks/use-crypto.ts` (the refresh mutation passes `true`)
- Leave `src/hooks/use-auth.tsx` and `src/hooks/SyncProvider.tsx` unforced (D7); fix the
  comment "Crypto prices (CoinGecko)" → "(Yahoo Finance)".

- [ ] **Step 1: Edit** as listed.
- [ ] **Step 2: Run** `npm run typecheck && npm run lint` — PASS (locale keys are added in
  Task 8; `t()` keys are strings so typecheck passes now).
- [ ] **Step 3: Commit** — `feat(ui): crypto add dialog searches Yahoo Finance; API key card and banner removed`.

### Task 8: Locales

**Files:**
- Modify: `src/i18n/locales/<lang>/crypto.json`, `settings.json`, `auth.json`, `common.json`,
  `mobile.json` for all ten languages
- Modify: `docs/standards/i18n-glossary.md` (drop CoinGecko from the do-not-translate list)

Changes (English copy; Czech by the implementer in the owner's tone, the other eight following
the glossary):

- `crypto.json`: `modal.add.description` "Look the coin up on Yahoo Finance; its rate then
  updates automatically."; `modal.add.search` "Search Yahoo Finance"; `modal.add.searchHint`
  "Type a name or ticker to see matches."; `modal.add.searching` "Searching Yahoo Finance…";
  `modal.add.searchError` "Search failed. Check your connection."; remove `modal.add.rank`;
  `tooltip.manualPrice` "Manually set rate – overrides the Yahoo Finance rate until you remove
  it."; `modal.updatePrice.description` likewise; `toast.pricesRefreshed_*` "… from Yahoo
  Finance"; `toast.pricesRefreshedDescription` "Current rates fetched from Yahoo Finance";
  `detail.source` and the other `source` key "Yahoo Finance"; `empty.description` (line ~8)
  "Add your first coin: we look it up on Yahoo Finance and the rate keeps itself up to date.";
  remove the `coingeckoBanner` block; add
  `unpricedBanner: { title_one: "{{count}} coin has no rate source", title_other: "{{count}} coins have no rate source", message: "Moony could not tell which Yahoo Finance listing {{tickers}} is. Pick it and the rate and history follow.", pick: "Pick the listing" }`
  (plural forms per language), `actions.changeSymbol` "Rate source…",
  `detail.menu.changeSymbol` "Rate source…", and
  `modal.symbol: { title: "Rate source for {{name}}", description: "Pick the Yahoo Finance listing of this coin. The rate and the history are rebuilt from it.", current: "Current: {{symbol}}", none: "No listing chosen yet.", submit: "Use this listing", saved: "Rate source saved" }`.
- `settings.json`: remove the `apiKeys` block; add `mcpServer.showToken` "Show token",
  `mcpServer.hideToken` "Hide token"; `sections.integrations` description → "The local MCP
  server for AI assistants."
- `auth.json` and `mobile.json`: the "what leaves your computer" sentence drops CoinGecko:
  "(Yahoo Finance, ECB)".
- `common.json`: remove `validation.coingeckoIdRequired`, `validation.coingeckoIdConflict`,
  `validation.coingeckoIdForNewCoin`; add `validation.yahooSymbolInvalid` "The Yahoo Finance
  symbol must be the coin's USD pair, for example BTC-USD" and `validation.yahooSymbolConflict`
  "{{ticker}}: the Yahoo Finance symbol differs from the one stored for this coin."

- [ ] **Step 1: Run** `npm test -- locales` — FAIL (parity) before the edits are complete;
  edit all ten locales.
- [ ] **Step 2: Run** `npm test` — PASS; `grep -rli coingecko src/i18n` prints nothing.
- [ ] **Step 3: Commit** — `feat(i18n): Yahoo Finance replaces CoinGecko in every locale`.

### Task 9: Symbol picker, banner and menu entries

**Files:**
- Create: `src/components/crypto/CryptoSymbolDialog.tsx`
- Create: `src/components/crypto/UnpricedCoinsBanner.tsx`
- Modify: `src/components/crypto/CryptoTable.tsx` (prop `onChangeSymbol: (holding) => void`,
  menu item after "Upravit kurz" with `t('actions.changeSymbol')`)
- Modify: `src/pages/Crypto.tsx` (state for the symbol dialog, `openRowModal('symbol')`, the
  banner above the trend card where the old one was, with `onPick(row)` opening the dialog)
- Modify: `src/mobile/components/wealth/CryptoMenu.tsx` (item `detail.menu.changeSymbol`
  opening the dialog)

**Interfaces:**
- Consumes: `cryptoApi.setSymbol`, `priceApi.searchCrypto`, `CryptoRow.yahooSymbol`.

`CryptoSymbolDialog({ open, onOpenChange, investment: CryptoRow | null })`: the same debounced
search list as `AddCryptoModal` (extract the list into
`src/components/crypto/CryptoSearchList.tsx` with props `{ query, onPick(result) }` and reuse it
in both dialogs); shows `modal.symbol.current` or `modal.symbol.none`; picking a row calls
`cryptoApi.setSymbol(investment.id, result.symbol)`; on success invalidates `['crypto']`,
`['portfolio-metrics']`, `['cashflow-report']`, `['portfolio-history']` (rule 7, via
`invalidateAfterPriceRefresh`), toasts `modal.symbol.saved`, closes.

`UnpricedCoinsBanner({ rows: CryptoRow[], onPick(row) })`: renders nothing when no row has
`yahooSymbol === null`; else the `Alert` (as the removed banner used it) with the plural title,
the message with `tickers` joined by ", ", and one button `unpricedBanner.pick` calling
`onPick(firstUnpriced)`.

- [ ] **Step 1: Build** the two components and wire the three menus/pages.
- [ ] **Step 2: Run** `npm run typecheck && npm run lint && npm test` — PASS.
- [ ] **Step 3: Commit** — `feat(crypto): pick a holding's Yahoo listing from the banner or the menu`.

## Phase C — docs

### Task 10: ADR, policy, README, architecture, deprecation log, changelog

**Files:**
- Create: `docs/architecture/decisions/0020-yahoo-finance-for-crypto-prices.md` (format of
  0005: Context = the §1 findings in brief and the owner's wish; Decision = D1, D2, D4;
  Consequences = single provider risk, no key, unresolved holdings need a pick; "Supersedes the
  CoinGecko sentence of 0005")
- Modify: `docs/architecture/decisions/README.md` (row for 0020; next free number 0021)
- Modify: `PRIVACY.md` (D15), `README.md` (D15), `docs/architecture/overview.md` (the worker,
  triggers and HTTP paragraphs; "a CoinGecko key change in `price_api`" → "a symbol change in
  `crypto`"), `docs/DEPRECATED.md` (new table "Removed with CoinGecko (do not re-add)":
  `get_api_keys`/`set_api_keys`/`ApiKeys`,
  `merge_crypto_price_series`/`coingecko_history_start`/`get_historical_crypto_prices_coingecko*`/`get_historical_crypto_prices_yahoo`/`yahoo_crypto_symbol`,
  `CoinGeckoApiKeyBanner`, `ApiKeysCard`, `coingecko_modal_dismissed`, `api_key_coingecko` —
  each with "why" and "use instead"), `CHANGELOG.md` (Unreleased → Changed: "Crypto rates
  come from Yahoo Finance…"; Removed: "CoinGecko and its optional API key…"; MCP: "`coingeckoId`
  replaced by optional `yahooSymbol` in `crypto_transactions_create`"), `docs/mcp.md` if it
  names the field.

- [ ] **Step 1: Edit**; `grep -rn -i coingecko --exclude-dir=node_modules --exclude-dir=target
  --exclude-dir=.git . | grep -v "docs/specs\|docs/plans\|docs/audits\|CHANGELOG.md\|docs/DEPRECATED.md\|0005-\|0020-"`
  prints nothing.
- [ ] **Step 2: Commit** — `docs: ADR 0020, privacy policy and README without CoinGecko`.

## Phase D — verification

### Task 11: Gates, demo run, review, merge

- [ ] **Step 1: Full gate** in the worktree (Global Constraints block) — all PASS.
- [ ] **Step 2: Demo run (D20)**: build the worktree app against a copy of `demo-home` on
  port 1422 (recipe in the bank-account-detail memory / `.claude/launch.json`); unlock the
  demo profile; confirm the `[MIGRATION] 025_crypto_yahoo_symbol` log line; the crypto page
  shows BTC and ETH with fresh rates and no banner; add "Sui" through the search → the holding
  stores `SUI20947-USD` (check with the MCP crypto list or the detail); clear a symbol with
  `sqlite3`-free means (set it through the picker to another listing and back) and confirm the
  banner appears/disappears; the new coin's trend chart fills after the rebuild.
- [ ] **Step 3: Whole-branch review** by a fresh reviewer (opus) against the spec; fix rounds.
- [ ] **Step 4: Before the merge**, check `all_migrations()` on `main`: if another branch
  landed a `025_…` since this plan, renumber this migration to the next free number (const,
  name string, test, snapshot regenerated) and commit.
- [ ] **Step 5: Merge** into local `main` through a temporary worktree with `--no-ff`
  (`git worktree add <scratch>/main-merge main && git merge --no-ff feat/yahoo-crypto`), verify
  `git merge-base --is-ancestor feat/yahoo-crypto main`, do not push; remove the scratch worktree.
