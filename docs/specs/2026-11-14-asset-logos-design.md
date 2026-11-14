# Asset logos: company and coin images

Date: 2026-11-14. Status: draft for owner approval.

## Intent

The owner wants company and coin logos back in the app. They were removed on 2026-10-02
(`cccd3906`, audit finding REL-02) because the webview loaded them straight from
financialmodelingprep.com and coincap.io, which told two more companies every tracked ticker.
Logos return only in a way that tells **no new party** what the user holds.

## Goals

- A stock or ETF the user holds or watches shows its logo wherever its initials circle shows today
  (desktop and mobile).
- A coin shows its logo wherever its initials circle shows today, including the coin search list.
- Where no logo exists, the initials circle stays exactly as it is now.
- Each logo is downloaded once per device and then read from the encrypted database.

## Non-goals

- Logos for loans, bonds, insurance, real estate and other assets (they keep initials).
- Logos in the stock search / add dialogs for tickers the user does not hold or watch yet.
- A second logo source for tickers Yahoo has no logo for (owner decision, 2026-11-14).
- Syncing the logo cache between devices.

## Anti-goals

- Any request to a host other than Yahoo's own (`query*.finance.yahoo.com`, `s.yimg.com`).
- Any name that is not a stock ticker (a loan name, an insurer, a property) reaching the network.
- Logo files or ticker-named files on disk outside the encrypted database.
- The webview fetching an image from the network (the CSP stays as it is).

## Approaches considered

1. **Yahoo for stocks + bundled coin icons (chosen by the owner).** Yahoo already receives every
   ticker for prices; its search endpoint returns `logoUrl` on its own CDN when asked with
   `enableLogoUrl=true` (checked 2026-11-14: `AAPL` → `https://s.yimg.com/lb/brands/150x150_apple.png`;
   `CEZ.PR` → none). For coins Yahoo points at `s2.coinmarketcap.com`, a new party, so coin icons
   ship inside the app instead. Partial stock coverage; nothing new leaks.
2. **Third-party logo services for the gaps.** Better coverage, but a new company sees the
   tickers and the privacy copy would have to say so. Rejected by the owner.
3. **Everything bundled.** No network at all, but no complete, freely licensed stock logo set
   exists. Rejected.

## Design

### Stocks (backend)

- **Table `asset_logos`** (next migration): `id` TEXT PK (UUID), `ticker` TEXT UNIQUE (upper case),
  `image` BLOB NULL, `mime` TEXT NULL, `checked_at` INTEGER (unix). A row with `image` NULL is a
  remembered miss. Sync registry class: `Local` (a per-device cache, never synced, never merged).
- **Service `services/asset_logos.rs`** (takes `&Connection` for DB work):
  - `cached(conn) -> Vec<AssetLogo>`: every row with an image, as `{ ticker, dataUrl }`.
  - `due(conn, tickers, now)`: the tickers with no row, or a miss older than 30 days. A found logo
    is never re-fetched.
  - `logo_url_from_search(body, ticker)`: parses a Yahoo search response; takes the quote whose
    `symbol` equals the ticker exactly; accepts the URL only if it is `https` on host `s.yimg.com`.
    Anything else (no quote, no URL, another host) is a miss.
  - `normalize(bytes)`: decodes with the `image` crate (PNG/JPEG/WebP only, input capped at
    512 KB), resizes to fit 96×96, re-encodes as PNG. Bytes that do not decode are a miss.
  - `store(conn, ticker, Option<image>)`: upsert of the row.
- **Fetching** (async, in `price_api.rs` next to the existing Yahoo search, through
  `services::http::client()`): one search request and one image request per due ticker, in
  sequence, with errors logged without the URL (same rule as `yahoo_crypto_search`). A network
  error or HTTP 429 writes nothing (retried next session); a clean "no logo" writes a miss.
- **Which tickers:** the backend builds the list itself from `stock_investments` and the
  watchlist tables. The frontend never passes names, so a loan or insurer name cannot reach Yahoo.
- **Commands** (thin, `AppError`): `get_asset_logos()` → `Vec<AssetLogo>`;
  `refresh_asset_logos()` → number of logos newly stored. Wire type `AssetLogo { ticker, dataUrl }`
  goes through the type-contract procedure (`shared/schema.ts`, `bindings.rs`, regenerated types).

### Coins (bundled)

- Icon set: `@web3icons/core` (MIT, maintained), branded SVG token icons, matched by the holding's
  **ticker** (lower case). `cryptocurrency-icons` (CC0) was considered and dropped: unmaintained
  since 2020, so newer coins are missing.
- Loaded with a lazy `import.meta.glob` over the branded token SVGs, so only icons actually shown
  are read; all files are served from the app itself (`'self'`). No migration, no cache.
  The app bundle grows by the size of that icon folder (a few MB; measured during the build and
  reported to the owner).
- A ticker the set does not know keeps its initials. Two coins sharing one ticker get the set's
  icon for that ticker (known limit; cosmetic only, prices still come from the stored Yahoo symbol).
- License text added to `THIRD_PARTY_LICENSES.md`.

### Frontend

- `AssetLogo` gets a third `type`, `'initials'`, for rows that are not an instrument. The loans,
  bonds, insurance, real-estate and other-assets call sites switch to it (today they pass
  `type="stock"` with a name). `type="stock"` then always means a real ticker.
- `tauri-api.ts` gets the two wrappers (rule 1). Hook `useAssetLogos()`: React Query key
  `["asset-logos"]`, `staleTime: Infinity`; once per unlocked session it calls
  `refresh_asset_logos()` in the background and invalidates the key when the count is above zero.
  It also re-runs after a stock is added or a ticker is followed (those mutations invalidate a
  `["asset-logos-refresh"]` trigger or call the refresh directly; builder's choice).
- `AssetLogo` renders `<img>` inside the same 30 px circle (white background in both themes,
  `object-contain`, initials underneath until the image loads or if it fails). Sizes passed by
  `className` keep working.
- Mobile `PositionRow` (and other mobile chips that show a stock or coin) use the same lookups.

### Docs

- New ADR `0028-asset-logos.md`: replaces the REL-02 "no logos" stance with "logos only from a
  party that already has the ticker, or bundled".
- README privacy/Yahoo paragraph: Yahoo also supplies company logos (search endpoint + `s.yimg.com`).
- `docs/architecture/database.md` (new table), `docs/design-system/README.md` (`.logo` row),
  the stale "no logo fetch" comments in `AssetLogo.tsx` and `PositionRow.tsx`.
- No user-facing strings are added (the image has empty `alt`; the ticker is already next to it).

## Tests

- Rust service tests with an in-memory DB (pattern of `services/budgeting.rs`): `due` (new ticker,
  fresh miss, 30-day-old miss, found logo), `logo_url_from_search` (exact symbol match, missing
  URL, non-`s.yimg.com` host such as coinmarketcap, `http`), `normalize` (valid PNG → 96 px PNG,
  garbage → miss, oversize → miss), `store`/`cached` round trip.
- Sync registry tests keep passing with the new `Local` table.
- Vitest for the pure coin-icon lookup (ticker → file key, unknown ticker → none).
- Live check in the demo profile: a US stock shows its logo, a ticker without one keeps initials,
  BTC/ETH show bundled icons, loans still show initials, light and dark.

## Settled after the builder check (my calls)

- **Ticker form.** A stock's stored ticker is its Yahoo symbol (prices are fetched by it), so it is
  used as the search query and cache key as-is; the symbol comparison is case-insensitive.
- **Which rows.** Every ticker in `stock_investments` (closed positions included, they still show
  in analysis) and every watched ticker.
- **Burst.** At most 40 due tickers per run, in sequence, about 300 ms apart; the rest follow on
  the next run. Stop the run at the first HTTP 429.
- **When.** Once after each unlock (desktop and mobile alike; the phone already asks Yahoo for
  prices), and after a stock is added or a ticker is followed. A run that ends after a lock simply
  fails its DB write and is logged; no cancellation.
- **Backups.** The table lives in the main database, so backups and device snapshots that copy the
  whole file carry it (a few KB per ticker). It is skipped by row-level sync (`Local`) and by MCP.
- **IPC size.** One response with every logo as a data URL (about 3–6 KB each); no paging.
- **Coins.** The lookup key is the bare ticker (`BTC`), which is what holdings and
  `CryptoSearchResult.ticker` carry; never the Yahoo symbol.
- **`type="initials"`.** Two letters and the same series hash as `type="stock"` today, so those
  rows look exactly as they do now; `variant="soft"` keeps working with every type.
- **Mobile.** The mobile chips keep their own components; `PositionRow` and any other mobile chip
  that shows a stock or coin position get the image from the shared `useAssetLogos()` /
  coin-icon lookup, initials as fallback.

## Left to the builder

- Exact migration number and the watchlist table names.
- How the refresh is re-triggered after adding a stock.
- Whether the glob points into `node_modules` or the SVGs are copied into `src/assets/`.

## My calls (not owner decisions)

- 30-day retry for misses; found logos kept forever.
- Re-encoding to 96 px PNG (smaller, and untrusted bytes never reach the webview as-is).
- White circle behind every logo in both themes.
- `@web3icons/core` over `cryptocurrency-icons`.
- Coin icons matched by ticker.
- Logos only for held and watched tickers, not in search dialogs.
