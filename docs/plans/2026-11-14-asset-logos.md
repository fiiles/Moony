# Asset Logos Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stocks show logos fetched from Yahoo and cached in the encrypted database; coins show icons bundled with the app; everything else keeps initials.

**Architecture:** A Rust service owns a `Local` cache table and the pure parsing/normalising logic; an async fetcher in `price_api.rs` asks Yahoo's search endpoint for `logoUrl` and downloads from `s.yimg.com` only. The frontend reads every cached logo once as data URLs through one React Query key and looks coin icons up in a lazily globbed bundled SVG set.

**Tech Stack:** Rust (rusqlite, reqwest via `services::http::client()`, `image` 0.25, `base64`), React 19 + TanStack Query, Vite `import.meta.glob`, `@web3icons/core`.

**Spec:** `docs/specs/2026-11-14-asset-logos-design.md` (read it first; it wins over this plan on any conflict).

## Global Constraints

- Read `AGENTS.md` (the ten rules) and the playbooks `docs/playbooks/add-db-migration.md`, `docs/playbooks/add-tauri-command.md`, `docs/standards/type-contract.md` before touching those layers.
- Network: only `query2.finance.yahoo.com` (search) and `https://s.yimg.com/…` (image). Any other logo host is a miss. Errors are logged with `e.without_url()`; a ticker never appears in a log above debug.
- The frontend never passes tickers or names to the backend for logo fetching.
- No new user-facing strings. CSP in `src-tauri/tauri.conf.json` is not changed.
- Work only in the worktree `/Users/filipkral/Programování/FinanceApp/Moony-tauri-logos`, branch `feat/asset-logos`. Check `git branch --show-current` before each commit. Never push. Commit with explicit paths (another agent may be committing other files in the same worktree).
- npm registry is npmjs (`--registry https://registry.npmjs.org` if the default differs).
- Commit messages: Conventional Commits, ending with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. Yahoo answers with a quote for a *different* symbol first (`q=CEZ.PR` → other listing): only an exact, case-insensitive symbol match may supply the logo. (Task 1 test.)
2. `logoUrl` on a foreign host (coinmarketcap) or `http:`: must be a miss, and no request may be sent to it. (Task 1 test.)
3. HTTP 429 / offline: nothing is written, so the ticker is retried next run instead of becoming a 30-day miss. (Task 2: `outcome` mapping test.)
4. An image that is not a PNG/JPEG/WebP, is truncated, or exceeds 512 KB: miss, no panic. (Task 1 test.)
5. A row passing a non-ticker name (loan, insurer) must never show a fetched image or cause a request: those call sites use `type="initials"`. (Task 4: grep check + typecheck.)

---

### Task 1: Cache table and pure service logic

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (migration `034_asset_logos`), `src-tauri/src/db/schema.snapshot` (regenerate the way the playbook says)
- Modify: `src-tauri/src/services/sync/registry/tables.rs` (`asset_logos`, `TableClass::Local`)
- Create: `src-tauri/src/services/asset_logos.rs`; register in `services/mod.rs`
- Create/modify: model `AssetLogo` in `src-tauri/src/models/` (per-field `#[serde(rename = "...")]`), `bindings.rs`, `shared/schema.ts`, regenerate `shared/generated-types.ts`
- Modify: `docs/architecture/database.md` (new table row)

**Interfaces (produces):**

```sql
CREATE TABLE asset_logos (
    id TEXT PRIMARY KEY,
    ticker TEXT NOT NULL UNIQUE,   -- upper case
    image BLOB,                    -- NULL = remembered miss
    mime TEXT,
    checked_at INTEGER NOT NULL
);
```

```rust
pub const MISS_RETRY_SECS: i64 = 30 * 24 * 3600;
pub const MAX_PER_RUN: usize = 40;
pub const MAX_SOURCE_BYTES: usize = 512 * 1024;
pub const LOGO_SIZE: u32 = 96;
pub const LOGO_HOST: &str = "s.yimg.com";

pub struct AssetLogo { pub ticker: String, pub data_url: String }   // wire: { ticker, dataUrl }

pub fn cached(conn: &Connection) -> Result<Vec<AssetLogo>>;          // rows with an image, data:image/png;base64,…
pub fn tracked_tickers(conn: &Connection) -> Result<Vec<String>>;    // distinct upper-case tickers of stock_investments ∪ watched_stocks
pub fn due(conn: &Connection, tickers: &[String], now: i64) -> Result<Vec<String>>; // no row, or miss with checked_at <= now - MISS_RETRY_SECS; capped at MAX_PER_RUN
pub fn logo_url_from_search(body: &str, ticker: &str) -> Option<String>;
pub fn normalize(bytes: &[u8]) -> Option<Vec<u8>>;                   // PNG bytes, fits LOGO_SIZE×LOGO_SIZE
pub fn store(conn: &Connection, ticker: &str, image: Option<&[u8]>, now: i64) -> Result<()>; // upsert; mime "image/png" when Some
```

- [ ] **Step 1: Failing tests** in `asset_logos.rs` (`setup_test_db` with `asset_logos`, minimal `stock_investments(id, ticker)` and `watched_stocks(id, ticker)`; check the real column names in `schema.snapshot`):
  - `due_lists_new_tickers_and_old_misses_only`: rows `AAPL` (image), `MISS1` (miss, `checked_at = now - MISS_RETRY_SECS`), `MISS2` (miss, `now - 60`); `due(["AAPL","MISS1","MISS2","NEW"])` == `["MISS1","NEW"]`.
  - `due_is_capped_at_max_per_run`: 45 new tickers → 40.
  - `tracked_tickers_unions_holdings_and_watchlist_upper_case_distinct`.
  - `search_takes_the_exact_symbol_only`: body with quotes `[{symbol:"CEZ.F", logoUrl:"https://s.yimg.com/a.png"},{symbol:"cez.pr"}]`, ticker `CEZ.PR` → `None`; with the logo on the second quote → `Some`.
  - `search_rejects_foreign_hosts_and_http`: `https://s2.coinmarketcap.com/x.png` → `None`; `http://s.yimg.com/x.png` → `None`; `https://s.yimg.com.evil.com/x.png` → `None`; malformed JSON → `None`.
  - `normalize_resizes_to_96_png`: a generated 150×150 PNG → decodes back as PNG with width == 96; a 40×20 image is not upscaled beyond its need (max side ≤ 96).
  - `normalize_rejects_garbage_gif_and_oversize`: `b"not an image"`, a GIF header, and `MAX_SOURCE_BYTES + 1` bytes → `None`.
  - `store_and_cached_round_trip`: store image then `cached` yields `data:image/png;base64,` + the bytes; storing `None` for another ticker is not in `cached`; storing an image over a miss replaces it.
- [ ] **Step 2:** `cd src-tauri && cargo test asset_logos` → fails to compile.
- [ ] **Step 3:** Implement. Parse the URL host with the `url` crate if already a dependency, otherwise a strict prefix check on `https://s.yimg.com/`. `normalize` uses `image::load_from_memory_with_format` after sniffing the format with `image::guess_format` (allow Png, Jpeg, WebP only) and `thumbnail`/`resize` to fit; if WebP decoding is not enabled in the `image` features, allow only PNG and JPEG and say so in the commit.
- [ ] **Step 4:** Add the migration, the registry entry, the model + bindings + `shared/schema.ts`, regenerate types (`cargo test generate_bindings -- --ignored`), update `database.md`.
- [ ] **Step 5:** `cargo fmt && cargo clippy -- -D warnings && cargo test` (whole suite: the sync registry and migration tests must pass) → green. `npm run typecheck` → green.
- [ ] **Step 6:** Commit `feat(logos): asset logo cache table and service`.

### Task 2: Yahoo fetch and commands

**Files:**
- Modify: `src-tauri/src/services/price_api.rs` (next to `yahoo_crypto_search`)
- Create: `src-tauri/src/commands/asset_logos.rs`; register in `commands/mod.rs` and the `invoke_handler` in `lib.rs` (and wherever the mobile builder registers commands, if separate)
- Modify: `src/lib/tauri-api.ts`

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces: commands `get_asset_logos() -> Vec<AssetLogo>` and `refresh_asset_logos() -> u32` (logos newly stored); `tauri-api.ts`: `assetLogosApi.getAll(): Promise<AssetLogo[]>`, `assetLogosApi.refresh(): Promise<number>` (follow the naming style of the neighbouring API objects).

```rust
/// What one ticker's lookup ended as.
pub enum LogoOutcome { Found(Vec<u8>), Miss, Retry }   // Retry = network error / non-2xx: write nothing
pub fn outcome_for_status(status: u16) -> Option<LogoOutcome>; // 429 and 5xx → Some(Retry); 404 on the image → Some(Miss); 2xx → None (continue)
pub async fn refresh_asset_logos(db: &Database) -> Result<u32>;
```

- [ ] **Step 1: Failing test** `outcome_for_status_keeps_rate_limits_retryable` (429 → Retry, 503 → Retry, 404 → Miss, 200 → None).
- [ ] **Step 2:** Implement `refresh_asset_logos`: read `tracked_tickers` + `due` under the DB lock, release it, then per ticker in sequence: GET `{YAHOO_SEARCH_URL}?q=<ticker>&quotesCount=5&newsCount=0&listsCount=0&enableLogoUrl=true`; `logo_url_from_search`; GET the image (reject `Content-Length` or body over `MAX_SOURCE_BYTES`); `normalize`; `store` Found/Miss; sleep 300 ms between tickers; stop the whole run at the first 429. Follow how the neighbouring functions take and release the `Database` lock around `await`s.
- [ ] **Step 3:** Thin commands (`AppError`, no logic), `tauri-api.ts` wrappers.
- [ ] **Step 4:** `cargo fmt && cargo clippy -- -D warnings && cargo test`, `npm run typecheck && npm run lint` → green.
- [ ] **Step 5:** Commit `feat(logos): fetch stock logos from Yahoo`.

### Task 3: Bundled coin icons (independent of Tasks 1–2)

**Files:**
- Modify: `package.json` / lockfile (`@web3icons/core`, exact version), `THIRD_PARTY_LICENSES.md` (follow how that file is produced; if a script generates it, run the script)
- Create: `src/utils/coin-icons.ts`, `src/utils/coin-icons.test.ts`

**Interfaces (produces):**

```ts
/** Lower-cased ticker → the key used in the glob map; null when the ticker has characters outside [a-z0-9]. */
export function coinIconKey(ticker: string): string | null;
/** Resolves to the bundled icon's URL, or null when the set has no icon for the ticker. Cached per ticker. */
export function loadCoinIcon(ticker: string): Promise<string | null>;
/** Synchronous read of an already loaded icon (undefined = not loaded yet). */
export function peekCoinIcon(ticker: string): string | null | undefined;
```

- [ ] **Step 1:** Install the package; inspect its folder to find the branded token SVGs (expected `node_modules/@web3icons/core/dist/svgs/tokens/branded/<SYMBOL>.svg`; use the real path and the real file-name case).
- [ ] **Step 2: Failing Vitest**: `coinIconKey('BTC') === 'btc'`, `coinIconKey(' eth ') === 'eth'`, `coinIconKey('BTC-USD') === null`, `coinIconKey('') === null`; `loadCoinIcon` with an injected map (export a pure `resolveCoinIcon(map, ticker)` so the test needs no Vite glob): known → URL string, unknown → null.
- [ ] **Step 3:** Implement with `import.meta.glob('<path>/*.svg', { query: '?url', import: 'default' })` (lazy). If Vite refuses a glob into `node_modules`, copy the branded token SVGs into `src/assets/coin-icons/` with a small `scripts/` copy script wired to `postinstall`-free manual use, commit the copies, and note the package version in a `README` there.
- [ ] **Step 4:** `npm test -- coin-icons`, `npm run typecheck`, `npm run lint`, `npm run build` → green. Record in the commit body how many icon files `dist/` gained and their total size (`du -sh`).
- [ ] **Step 5:** Commit `feat(logos): bundled coin icons`.

### Task 4: Frontend rendering (desktop and mobile)

**Files:**
- Create: `src/hooks/use-asset-logos.ts`
- Modify: `src/components/common/AssetLogo.tsx`; the non-instrument call sites (`LoansTable`, `BondsTable`, `InsuranceTable`, `OtherAssetsTable`, `pages/RealEstate.tsx`, `pages/RealEstateDetail.tsx` ×2) → `type="initials"`
- Modify: `src/mobile/components/wealth/PositionRow.tsx` and any other mobile chip showing a stock/coin position (find with `grep -rn "slice(0, 2)" src/mobile`)
- Modify: the add-stock and follow-ticker mutations (call the refresh after success)
- Modify: `docs/design-system/README.md` (`.logo` row: image when cached/bundled, initials otherwise)

**Interfaces:**
- Consumes: `assetLogosApi` (Task 2), `loadCoinIcon`/`peekCoinIcon` (Task 3).
- Produces:

```ts
/** Map of upper-case ticker → data URL. Runs refresh once per unlock and invalidates ['asset-logos'] when it stored something. */
export function useAssetLogos(): ReadonlyMap<string, string>;
/** For mutations: refresh now, invalidate ['asset-logos'] if anything was stored. Never throws. */
export function refreshAssetLogos(queryClient: QueryClient): Promise<void>;
/** Stock data URL or coin icon URL for an instrument, undefined while unknown/absent. */
export function useInstrumentLogo(ticker: string, type: 'stock' | 'crypto'): string | undefined;
```

- [ ] **Step 1:** `AssetLogoProps.type` becomes `'stock' | 'crypto' | 'initials'`. `'initials'`: two letters, same series hash, never an image. `'stock'`/`'crypto'`: when `useInstrumentLogo` returns a URL, render `<img alt="" src=… loading="lazy">` filling the circle on a white background (`bg-white`, `object-contain`, small padding, `rounded-full overflow-hidden`), initials as the fallback on `onError`. `variant` and `className` behave as before.
- [ ] **Step 2:** `useAssetLogos`: query key `['asset-logos']`, `staleTime: Infinity`. The once-per-unlock refresh is a module-level in-flight promise reset when the query cache is cleared on lock (`queryClient.clear()` in `use-auth.tsx`) — simplest: a second query `['asset-logos-refresh']` with `staleTime: Infinity` whose `queryFn` calls `refresh()` and invalidates `['asset-logos']` when the result is > 0.
- [ ] **Step 3:** Switch the non-instrument call sites to `type="initials"`. Check: `grep -rn 'AssetLogo' src | grep 'type="stock"'` lists only rows whose `ticker` prop is a real stock ticker.
- [ ] **Step 4:** Mobile chips: show the image inside the existing chip shape with initials as fallback; update the stale "no logo fetch" comment.
- [ ] **Step 5:** Call `refreshAssetLogos(queryClient)` (not awaited) after a successful add-stock / import-stocks / follow-ticker mutation.
- [ ] **Step 6:** `npm run lint && npm run typecheck && npm run format:check && npm test` → green.
- [ ] **Step 7:** Commit `feat(logos): show logos in instrument rows`.

### Task 5: Docs

**Files:** `docs/architecture/decisions/0028-asset-logos.md` + the ADR index in `README.md` there; root `README.md` (the Yahoo bullet in the third-party/privacy section: Yahoo also supplies company logos, images come from its CDN `s.yimg.com`; coin icons ship with the app); any in-app privacy copy that enumerates what Yahoo receives (grep the `en` locale for "Yahoo"; change copy only if a sentence becomes untrue, and then in all ten locales).

- [ ] **Step 1:** Write the ADR in the format of the neighbouring ADRs: context (REL-02, `cccd3906`), decision (logos only from a party that already receives the ticker, fetched by the backend, cached encrypted; coins bundled), consequences (partial coverage, bundle size, `Local` table).
- [ ] **Step 2:** README edits; `npm test` (locale parity) if any locale changed.
- [ ] **Step 3:** Commit `docs(logos): ADR 0028 and privacy notes`.
