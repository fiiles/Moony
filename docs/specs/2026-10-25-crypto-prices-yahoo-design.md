# Crypto prices from Yahoo Finance (CoinGecko removed) — design

Date: 2026-10-25. Status: **approved** by the owner in chat on 2026-10-25 ("Ano, nahraď
CoinGecko Yahoo Finance. Nejdřív ověř vzorek tickerů proti Yahoo, pak udělej spec a plán a
implementuj ve vlastní větvi."). The owner's framing: CoinGecko should disappear from the app
entirely, and crypto prices should come from Yahoo Finance like stock prices do. The plan is
`docs/plans/2026-10-25-crypto-prices-yahoo.md`. This spec is also the hand-off brief: an
implementer should not need the chat. Every decision below (D1–D20) is the author's call unless
it quotes the owner.

## 1. Problem

Moony prices crypto holdings through two providers:

- **CoinGecko** (`services/price_api.rs`): the current USD price (`/simple/price`), the coin
  search in the add dialog (`/search`) and the daily history of the last 365 days
  (`/coins/{id}/market_chart/range`). Its free tier is rate-limited (about 30 requests a minute,
  no history older than a year), which is why Settings → Integrations has an optional CoinGecko
  API key, the crypto page shows a banner asking for one, and the history rebuild sleeps 1.5 s
  between coins.
- **Yahoo Finance** (same file): the daily `{TICKER}-USD` closes for the days older than
  CoinGecko's year (`get_historical_crypto_prices_yahoo`, merged by
  `merge_crypto_price_series`).

So Yahoo already prices crypto in this app, just not all of it. The coin's identity is the
CoinGecko id (`crypto_investments.coingecko_id NOT NULL`, required by the MCP import tool for a
new coin), which couples every holding, the wire contract, the MCP contract, the settings page,
the privacy policy and ten locales to a provider the owner wants gone.

A probe on 2026-10-25 against Yahoo's chart endpoint (`v8/finance/chart/{SYMBOL}-USD`,
`range=5d`, no cookie or crumb needed) and search endpoint (`v1/finance/search?q=…`) found:

| Finding | Examples |
|---|---|
| Plain `TICKER-USD` quotes the expected coin | BTC, ETH, SOL, ADA, XRP, DOGE, DOT, LINK, AVAX, ATOM, LTC, USDT, USDC, BNB, SHIB, TIA, KAS, INJ, NEAR, AAVE, HBAR, LDO, WIF, BONK, SEI, ONDO (46 of 60 probed) |
| Plain `TICKER-USD` is **missing** although the coin is on Yahoo | SUI (`SUI20947-USD`), PEPE (`PEPE24478-USD`), APT (`APT21794-USD`), UNI (`UNI7083-USD`), TAO (`TAO22974-USD`), GRT (`GRT6719-USD`), IMX (`IMX10603-USD`), STX (`STX4847-USD`), ZK (`ZK24091-USD`), POL (`POL28321-USD`) |
| Plain `TICKER-USD` quotes a **different coin** of the same ticker | `ARB-USD` is "ARbit", Arbitrum is `ARB11841-USD`; `TON-USD` is "TON Token", Toncoin is `TON11419-USD`; `STRK-USD` is "Strike", Starknet is `STRK22691-USD`; Yahoo lists two "Jupiter USD" entries, `JUP-USD` and `JUP29210-USD`, with identical names |
| Search returns crypto rows as `quoteType = "CRYPTOCURRENCY"`, exchange `CCC`, sometimes only a non-USD pair | "polygon" → `POL28321-EUR` only; "the graph" → `GRT6719-ETH`, `GRT6719-EUR`, …; the `-USD` pair of the same base exists in every probed case |
| Search returns **at most 7 quotes** whatever `quotesCount` says, stocks, ETFs and non-USD pairs included | "bitcoin" yields two coins; "ARB", "JUP" and "Jupiter" yield no crypto row at all |
| Appending ` USD` to the query yields crypto-only rows, for names and tickers alike | "ARB USD" → `ARB-USD`, `ARB11841-USD`, …; "SUI USD" → `SUI20947-USD` first; "TON USD" → `TON11419-USD` among seven; "Cardano USD" → `ADA-USD`, `WADA-USD`; but "polygon USD" misses `POL28321`, which the plain query finds |
| A suffixed symbol serves a full daily history | `SUI20947-USD`, `PEPE24478-USD`, `ARB11841-USD`, `GRT6719-USD`, `JUP29210-USD`: 366 daily closes, no nulls, over `range=1y` |
| Daily bars are stamped at 00:00 UTC, currency `USD` | every probed symbol; `exchangeTimezoneName: UTC` |

So the symbol can **not** be derived from the ticker. Yahoo's crypto symbol is
`<TICKER>[<CoinMarketCap id>]-<QUOTE>`; the numeric suffix disambiguates tickers that several
tokens share, and plain `TICKER-USD` belongs to whichever token Yahoo listed first.

## 2. Goals and non-goals

Goals:

- **One price provider.** Yahoo Finance supplies the current crypto price, the coin search and
  the whole daily history. Nothing in the app, its docs or its privacy policy mentions
  CoinGecko any more; the optional API key, its settings card, its banner and its storage go.
- **A holding knows its Yahoo symbol.** The USD pair symbol (`BTC-USD`, `SUI20947-USD`) is the
  coin's identity for pricing, chosen in the search when the coin is added and stored on the
  holding. The display ticker stays the user's (`SUI`).
- **Existing holdings keep working without the user doing anything** wherever Yahoo can tell
  which coin is meant; where it cannot, the app says so and lets the user pick the symbol.
  It never prices a holding with a different coin's quote on a guess.
- **Same behaviour elsewhere.** Manual price overrides, the history rebuild queue, the MCP
  tools, the demo profile, exports and backups work as before, minus the CoinGecko fields.

Non-goals:

- No second crypto provider, no fallback chain (ADR 0005 accepted the single-provider risk for
  stocks; this extends it to crypto, see D1).
- No quote currencies other than USD (the stored price stays `USD`, converted through
  `services/fx.rs` as today).
- No change to how crypto value history is computed, stored or drawn; only where the daily
  prices come from.
- No new history-rebuild version (`app_config.history_rebuild_version` stays 4); history
  already stored from CoinGecko is kept (D13).
- Mobile gets the shared add dialog and menu change; no mobile-only screen is designed here.

Anti-goals (a failure even if it works):

- A holding silently priced with the wrong coin (`ARB` as "ARbit").
- A migration that needs the network, or that drops a holding, a transaction or a cached price.
- A CoinGecko id or key surviving anywhere: a column, a config row, an export, a locale string,
  a doc that is not a frozen snapshot.

## 3. Decisions

### D1 — Yahoo Finance is the only crypto price source

Current price, search and history all go through the same unofficial Yahoo interface the stock
code uses (`yahoo_finance_api` 5 for chart requests; the search endpoint directly, D6).
ADR 0020 records it and supersedes the sentence of ADR 0005 that kept the CoinGecko key. The
breakage risk is the one ADR 0005 accepted: when Yahoo changes, stock and crypto prices go stale
together, no data is lost, and `crypto_price_overrides` still lets the user pin a price.

### D2 — The holding stores the Yahoo USD pair symbol

`crypto_investments.yahoo_symbol TEXT` (nullable) holds the full Yahoo symbol of the coin's USD
pair exactly as Yahoo spells it: `BTC-USD`, `SUI20947-USD`, `POL28321-USD`. It is the only
identity the price code reads. `ticker` stays the user-facing symbol (`SUI`), still `UNIQUE`,
still editable in the add dialog, still what `crypto_prices.symbol`, `crypto_price_overrides`,
`crypto_transactions` and `crypto_value_history` are keyed by, so nothing downstream of pricing
changes.

A valid symbol is ASCII, uppercase letters, digits, `.` and `-`, 3–40 characters, and ends in
`-USD` (`validation.yahooSymbolInvalid`). The commands and the MCP tool normalise input to
uppercase and trim it before validating.

### D3 — Migration 025 `crypto_yahoo_symbol`

Append-only, no network, no table rebuild (the columns involved carry no index, key or foreign
key, so SQLite's `DROP COLUMN` applies; the migration test guards the SQLite version like the
earlier `DROP COLUMN` migration does):

```sql
ALTER TABLE crypto_investments ADD COLUMN yahoo_symbol TEXT;
ALTER TABLE crypto_investments DROP COLUMN coingecko_id;
ALTER TABLE crypto_prices DROP COLUMN coingecko_id;
ALTER TABLE user_profile DROP COLUMN coingecko_modal_dismissed;
DELETE FROM app_config WHERE key = 'api_key_coingecko';
```

`yahoo_symbol` is `NULL` for every existing holding: the migration must not guess (`ARB-USD`
would be the wrong coin). The symbol is resolved lazily (D4). The cached `crypto_prices` rows
survive, so the app shows the last known price until the first refresh. The `DELETE` wipes the
secret the app no longer uses. The schema snapshot is regenerated; `database.md` rows for
`crypto_investments`, `crypto_prices`, `user_profile` and `app_config` are amended. The
`feat/mobile` worktree is ahead of `main`; if it lands a migration 025 first, this one is
renumbered at merge time (append-only: next free number, never a shared one).

### D4 — Lazy symbol resolution, never a guess

A holding whose `yahoo_symbol` is `NULL` (migrated, or added by typing a ticker without picking
a search result, or imported through MCP without a symbol) is resolved the first time the app
needs its price, by `services/crypto_symbols.rs`:

1. Search Yahoo (D6, which already pairs each query with its ` USD` variant) for the stored
   **name** (`Arbitrum`); if that yields no unique choice, search for the **ticker** (`ARB`).
2. From the rows (already USD pairs), the candidates are those whose display ticker (D5)
   equals the holding's ticker (case-insensitive).
3. Exactly one candidate → that symbol. Several → keep those whose name equals the stored name
   (case-insensitive); exactly one → that symbol. Anything else (none, or still several) →
   **no choice**: the app does not guess.
4. Store a choice on the holding. No choice → leave `NULL`, log at debug, report the ticker as
   unresolved (the banner and the picker, D9, are the recovery path). `ARB` named "Arbitrum"
   resolves to `ARB11841-USD` (the "ARbit" row loses on name); `JUP` named "Jupiter" stays
   unresolved because Yahoo names both `JUP-USD` and `JUP29210-USD` "Jupiter"; `BTC` named
   "Bitcoin" resolves to `BTC-USD` on the ticker alone.

Resolution runs inside the price refresh (D7) and the history rebuild (D8) and is best effort
there (a network failure is "not resolved this time", never an error); the pure choice
(`choose_symbol(name, ticker, rows)`) is unit-tested with the probe's cases. A resolution is
never repeated for a holding that already has a symbol. A holding resolved during a price
refresh gets its history rebuilt from its first transaction (D7), so a migrated holding ends
up on Yahoo's series like a newly added one.

### D5 — Display ticker of a Yahoo symbol

`display_ticker("SUI20947-USD") == "SUI"`: the part before the last `-`, with a trailing run
of **four or more digits** removed when at least one letter remains (`W-USD` → `W`,
`1INCH-USD` → `1INCH`, `GRT6719-USD` → `GRT`). CoinMarketCap ids on suffixed symbols have four
or five digits; no real ticker ends in four digits. The user can still overwrite the ticker in
the add dialog.

### D6 — Coin search goes to Yahoo's search endpoint directly

`search_crypto(query)` sends **two** requests to
`https://query2.finance.yahoo.com/v1/finance/search?q=…&newsCount=0&listsCount=0` through the
shared `services/http.rs` client (15 s timeout, `Moony/<version>` user agent): first for
`<query> USD`, then for `<query>`. Yahoo caps a search at seven quotes whatever `quotesCount`
says, and the plain query spends them on stocks, ETFs and non-USD pairs ("bitcoin" yields two
coins, "ARB" none), while the ` USD` variant yields crypto only but misses a coin Yahoo lists
under another quote currency ("polygon USD" misses `POL28321`). No cookie or crumb is needed.
The rows of both responses, in that order, are filtered to `quoteType == "CRYPTOCURRENCY"`,
mapped to the USD pair of their base (`GRT6719-ETH` → `GRT6719-USD`), deduplicated by base
keeping the first occurrence, and returned as

```ts
interface CryptoSearchResult { symbol: string; ticker: string; name: string }
// { symbol: "SUI20947-USD", ticker: "SUI", name: "Sui" }
```

`name` is `shortname` with a trailing ` USD` (or ` <QUOTE>`) stripped, else `longname`, else the
symbol. There is no market-cap rank any more (Yahoo gives none); the list shows name and ticker.
The stock search (`search_stock_tickers`) is left as it is.

### D7 — Price refresh: one chart request per holding, stock-style caching

`refresh_crypto_prices(db, force_refresh)`:

- Reads every holding's `ticker` and `yahoo_symbol`; resolves the `NULL` ones (D4).
- Skips a ticker whose `crypto_prices.fetched_at` is younger than `PRICE_CACHE_HOURS` (the
  stock refresh's 4 h) unless `force_refresh`. A holding without a cached row is always
  fetched (a coin just added gets its price at once).
- For each remaining holding, `get_quote_range(symbol, "1d", "5d")` through the bounded Yahoo
  connector, `read_chart_quote` for the price, and requires the reported currency to be `USD`
  (else the quote is skipped with a warning: the stored price is always USD). Sequential, no
  sleep, like the stock refresh.
- Upserts `crypto_prices (symbol = ticker, price, currency 'USD', fetched_at)`.
- Returns the updated quotes plus, outside the wire contract, the tickers it resolved in this
  run (like the stock refresh's `unit_changed`). The command that ran the refresh queues
  `Job::CryptoTicker` from the first transaction for each resolved ticker, so the history
  moves to Yahoo's series (D13). The wire stays `CryptoPriceResult[]`, so the seven callers
  (unlock, sync provider, data-status menu, add dialog, crypto page, detail page, mobile hook)
  and the toasts keep their shape. An unresolved or failed holding is simply absent from the
  list; an unresolved one shows `yahooSymbol: null` in the positions list (the banner, D9).

The command gains `force_refresh: Option<bool>`; the explicit refresh buttons (crypto page,
detail page, the data-status menu, the mobile pull) pass `true`; the unlock pass, the sync
provider's stale check and the post-add refresh pass nothing.

The MCP worker's crypto branch (`commands/mcp_refresh.rs`) calls the refresh with
`force_refresh = true` and judges completeness over the holdings that **have a symbol** after
resolution: a quote missing for one of those is an incomplete refresh (the durable follow-up
stays pending and is retried by the next write, as today); a holding that could not be resolved
is logged and does not hold the job, because a retry cannot fix it and the picker can. The
alias handling for a CoinGecko id shared by two tickers goes: each holding has its own symbol.

`get_price_status` (`commands/portfolio.rs`) counts a crypto holding as missing a price only
when it has a symbol; an unresolved holding would otherwise keep the whole class "stale" and the
sync provider refreshing on every launch.

### D8 — History: one Yahoo request for the whole range

`history_recalc::rebuild_crypto_ticker_with_policy` reads `yahoo_symbol`, resolves it when
`NULL` (D4; a holding that cannot be resolved is left alone like the id-less holding today, in
strict mode too: a retry cannot fix it, the picker can, and a Yahoo failure *with* a symbol is
still the strict error that keeps the durable job's retry intent), then fetches the daily
closes from `from_day` to `until_day` with
`get_historical_stock_prices_yahoo` / `_strict` for the symbol, keeps the USD quotes, and plans
the rows as today (`plan_ticker_history`, last known price for a day without a quote).
Removed: `merge_crypto_price_series`, `coingecko_history_start`,
`get_historical_crypto_prices_coingecko{,_strict,_impl}`, `parse_crypto_history_response`,
`get_historical_crypto_prices_yahoo` (the generic stock fetch does it),
`yahoo_crypto_symbol`, the 1.5 s inter-coin sleep. Yahoo's daily crypto bars are stamped at
00:00 UTC, which is the day stamp the existing `backfill::price_at_or_before` test already
assumes.

The on-demand `backfill_crypto_ticker_history` command stays (the position hero of the detail
page and the mobile ticker-history hook call it on mount as the chart's fallback). It loses its
365-day cap, which was CoinGecko's: it rebuilds the ticker from its oldest transaction in one
Yahoo request, and for a holding without a symbol it returns `completed` with the message "No
Yahoo Finance symbol for this crypto" and touches nothing.

### D9 — Add dialog and symbol picker

`AddCryptoModal` (desktop and mobile share it): the search list shows name and ticker;
picking a row fills name, ticker and `yahooSymbol`; the hint and placeholder name Yahoo
Finance. Submitting without picking a row sends no symbol (D4 resolves it later).

New `CryptoSymbolDialog` (same search list, title "Zdroj kurzu"): picking a row calls the new
command `set_crypto_symbol(id, symbol)`, which validates (D2), stores the symbol, refreshes
that holding's price (`force`) and queues `Job::CryptoTicker` from its first transaction so the
history is rebuilt with the right coin. Reached from

- the crypto page's row menu (desktop) and `CryptoMenu` (mobile) as "Zdroj kurzu…", and
- a banner on the desktop crypto page listing the holdings with `yahooSymbol: null`
  ("Moony nenašel kurz pro SUI, ARB …"), replacing `CoinGeckoApiKeyBanner` in the same slot;
  one button opens the picker for the first listed coin (the rest through the menu).

The banner has no dismiss state: it disappears when every holding has a symbol.

### D10 — Wire contract

| Type / command | Change |
|---|---|
| `CryptoInvestment`, `EnrichedCryptoInvestment` | `coingeckoId: string` → `yahooSymbol: string \| null` |
| `InsertCryptoInvestment` | `coingeckoId: string` → `yahooSymbol?: string` |
| `create_crypto` | accepts the optional symbol; validates it when present |
| `update_crypto_price` | loses the ignored `coingeckoId` argument |
| `search_crypto` | returns `CryptoSearchResult[]` (D6) |
| `refresh_crypto_prices` | gains `forceRefresh?: boolean` |
| `set_crypto_symbol(id, symbol)` | new |
| `get_api_keys`, `set_api_keys`, `ApiKeys` | removed |
| `backfill_crypto_ticker_history` | kept; no 365-day cap; a holding without a symbol completes with a message (D8) |
| `UserProfile.coingeckoModalDismissed`, `UpdateUserProfile.coingeckoModalDismissed` | removed |
| `mcp-data-changed` tickers pairs, `trigger_import_recalculation` pairs | the second element (once the CoinGecko id) is `null`; the worker reads the symbol from the holding |
| `CryptoRow.coingeckoId` (`src/utils/positions.ts`), the detail page's source eyebrow | `yahooSymbol`; the eyebrow shows the source when the holding has a symbol |
| `trigger_import_recalculation` | keeps its `(ticker, Option<String>)` pairs (the second element was the CoinGecko hint and is ignored for crypto as it is for stocks); its doc comment says so |

Registered types are regenerated into `shared/generated-types.ts`; `shared/schema.ts` and
`src/lib/tauri-api.ts` are hand-mirrored (type-contract 7-step procedure).

### D11 — MCP tools

`crypto_transactions_create` rows: `coingeckoId` is removed; `yahooSymbol` (optional, "Yahoo
Finance symbol of the coin's USD pair, e.g. BTC-USD or SUI20947-USD; when omitted Moony looks it
up from the ticker and name") replaces it. A new coin no longer needs an identifier (the
`coingeckoIdForNewCoin` rejection goes); a provided symbol that differs from the stored one is
still rejected (`validation.yahooSymbolConflict`), and conflicting symbols for one ticker inside
a batch too. The crypto list tool returns `yahooSymbol`. Tool descriptions in `services/mcp/mod.rs`
and `docs/mcp.md` drop CoinGecko. A client that still sends `coingeckoId` gets it ignored (the
args structs do not deny unknown fields); the field's removal is noted in the CHANGELOG.

### D12 — Settings and profile

`ApiKeysCard` is deleted; Settings → Integrations keeps the MCP card. The `apiKeys.*` strings,
the `coingeckoBanner.*` strings and the `coingecko_modal_dismissed` flag go (D3, D10). There is
no other API key in the app, so no "API keys" section remains. The JSON export and the backup
keep excluding whatever `api_key_*` rows exist (none after the migration).

### D13 — Stored history is kept

The CoinGecko-sourced rows in `crypto_value_history` stay; new days come from Yahoo. The two
sources differ by at most the intraday timing of CoinGecko's last quote of a day, which does not
justify a one-time rebuild of every profile. A holding whose symbol is set through the picker
(D9) is rebuilt from its first transaction, because its old rows may be from the wrong coin's
history only if the user picked a different coin than CoinGecko had; that rebuild is cheap (one
request).

### D14 — Locales

Every CoinGecko string in the ten locales is replaced or removed: the add dialog's description,
search button, hint, "searching", error and no-results copy; the manual-price copy ("overrides
the Yahoo Finance rate"); the refresh toasts ("Updated {{count}} rates from Yahoo Finance");
the price source labels; the onboarding "what leaves your computer" sentences in `auth.json`
and `mobile.json`; the validation keys (`yahooSymbolInvalid`, `yahooSymbolConflict` replace
`coingeckoIdRequired`, `coingeckoIdConflict`, `coingeckoIdForNewCoin`); the new banner, menu
item and picker strings. The glossary's list of names not to translate drops CoinGecko.

### D15 — Docs and policy

- New ADR 0020 "Yahoo Finance for crypto prices; CoinGecko removed" (supersedes the CoinGecko
  sentence of ADR 0005; the index's next free number becomes 0021).
- `PRIVACY.md`: the CoinGecko row goes; the Yahoo row says the crypto symbols of your holdings
  and the coin names or tickers you search are sent, always, not only for history older than a
  year; the user-agent sentence names the ECB and Frankfurter only.
- `README.md`: "What leaves your computer" and "Data sources and attribution" drop CoinGecko
  and the crypto-id wording; Yahoo supplies crypto prices, search and history.
- `docs/architecture/overview.md` (worker, triggers, HTTP paragraph), `database.md` (rows),
  `DEPRECATED.md` (a "Removed with CoinGecko (do not re-add)" table), `CHANGELOG.md`
  (Unreleased: Changed + Removed), `src-tauri/Cargo.toml` and `services/http.rs` comments,
  `constants.rs` comment. Frozen specs, plans and audits are never edited.

### D16 — Demo seed and tests

`seed_demo` writes `yahoo_symbol` for its coins (`BTC-USD`, `ETH-USD`) and no `coingecko_id` in
`crypto_prices`. Test schemas in `services/positions.rs`, `services/mcp/crypto.rs`,
`services/crypto_investments.rs`, `services/mcp_refresh.rs` follow. The onboarding
whitelist test and the backup round-trip test use another non-whitelisted key name.

### D17 — What is logged

Resolution results at debug, Yahoo failures at warn without the symbol list at info (as the
stock refresh does: counts at info, names at debug).

### D18 — Rate limits

The stock refresh already issues one request per ticker with no delay and has not hit Yahoo's
limits; crypto adds a handful. No throttling, no key, no quota bookkeeping.

### D19 — Worktree and branch

`../Moony-tauri-yahoo-crypto` on `feat/yahoo-crypto`, merged into local `main` with `--no-ff`
at the end, not pushed (owner's standing instruction).

### D20 — Verification before the merge

Besides the gates: a debug run of the worktree build on the demo profile (port 1422, worktree
copy of `demo-home`): the migration applies, the demo coins (`BTC`, `ETH`) resolve and refresh,
adding `Sui` through the search stores `SUI20947-USD`, the picker fixes a holding whose symbol
was cleared, and the crypto trend chart rebuilds from Yahoo for a new coin.

## 4. What is left to the builder

- The exact layout of the banner and picker within the design system (`docs/design-system/`):
  reuse the add dialog's list rendering and the existing `Alert` of the removed banner.
- Where the row menu entry sits on the desktop crypto page (next to "Upravit kurz").
- Test fixtures for the search parser (a trimmed copy of the probe's `pepe` and `the graph`
  responses) and the chooser (the ARB / TON / STRK cases from §1).
- Translations for the eight non-owner locales, following `docs/standards/i18n-glossary.md`.
