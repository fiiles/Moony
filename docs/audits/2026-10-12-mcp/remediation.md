# Remediation ledger

Scope: F01–F15 and the audit's actionable secondary issues. Original report and proofs are historical evidence, not assertions of current behavior. This ledger supersedes the in-progress entries of earlier runs.

Design and plan: `docs/specs/2026-10-12-mcp-integrity-design.md`, `docs/plans/2026-10-12-mcp-integrity.md`, ADR 0014, append-only migration `016_mcp_pending_refresh`.

Working state: branch `codex/mcp-integrity`, committed locally, not pushed or merged. The original `main` checkout was not touched by this work. No personal database was opened or altered; every test uses a minimal in-memory schema.

## Resolution matrix

| Finding | Resolution | Status |
| --- | --- | --- |
| F01 sells above holdings | Exact decimal chronological holdings over the final merged batch (backdated buy may cover a later sell; write order irrelevant); same check on initial create, edit, delete and stock batch undo, each in one transaction | Fixed |
| F02 other-asset initial buy leaves quantity 0 | Initial transaction and derived totals written atomically; exact non-negative holdings incl. initial sell, single sell, merged batch; full rollback | Fixed |
| F03 CoinGecko ID overwrites another coin | Conflicting identity rejected; recalculation reads the authoritative ID from the stored holding | Fixed |
| F04 savings without interest breaks cashflow | Interest defaults to 0; legacy NULL read tolerantly | Fixed |
| F05 NaN/inf accepted | Shared finite validation (no empty/padded numbers); overflow-safe aggregates | Fixed |
| F06 stock MCP import has no batch/undo | `stock_import_batch` with `source = mcp`, returns `importBatchId`; UI shows the source; undo touches only the batch's own rows. Old MCP rows are not guessed into batches | Fixed |
| F07 dedup compares spelling, not value | Exact numeric comparison | Fixed |
| F08 dedup ignores currency | Key includes currency and UTC day; distinct source IDs keep real fills apart; heuristic match is `possibleDuplicate` with `importAnyway` | Fixed |
| F09 wrong property type offered | `personal` / `investment` in schema and validation | Fixed |
| F10 other-asset prices mixed across currencies | Dated FX conversion into the asset currency; missing rate or overflow errors and rolls back | Fixed |
| F11 case changes financial meaning | Canonical enums for accounts and insurance | Fixed |
| F12 bank MCP import skips FX backfill | Durable intent saved before the domain write (same DB mutex); backend worker refreshes FX, prices, history, snapshot without a renderer and resumes after unlock | Fixed |
| F13 local midnight suppresses recalculation | UTC days throughout the worker; frontend events only invalidate caches | Fixed |
| F14 invalid / non-canonical dates | UTC-day canonical dates, years 1..9999; safe frontend formatting of legacy invalid values | Fixed |
| F15 crypto MCP rejects UI-supported currencies | Same currency policy as the UI | Fixed |

Secondary: numbered insurance contract retries, empty premium normalized to 0, atomic Stock Monitor implicit follow, bank import refreshes its own IBAN identities from the DB, legacy intraday bank rows compared by UTC day against normalized reimports.

## Late review findings

Independent review after the main implementation found four gaps, then a final review found two more. All are fixed with tests:

1. Strict price API for the durable worker: network, HTTP, parse and provider errors never acknowledge a job; UI callers keep the best-effort policy.
2. Worker DB access pinned to the open session epoch; a close/reopen during a network await cannot write into a different session. A generation token keeps a newer write's job.
3. Bank composite key compares UTC days on both sides (RED then GREEN).
4. Other assets: exact chronological non-negative holdings and merged-batch rollback.
5. Strict Yahoo history treated a window without a trading day (no timestamps / all-null closes) as a failure, so a weekend-dated MCP write kept the stocks job pending. Now an empty Ok; provider errors and malformed payloads still fail. RED against the previous code, then GREEN.
6. Strict crypto history asked CoinGecko for ranges older than 365 days, which the public/demo tier rejects, so any year-old MCP crypto transaction kept the crypto job pending forever. Provider requests are clamped to the last 365 days (`coingecko_history_start`); earlier days use the last known price.

Final reviews of strict API + session guard, bank dedup + other assets, and stock/crypto edit/delete/undo found no further defects.

## Verification (2026-10-12, final, after all source changes)

- `cargo fmt --check`: clean.
- `cargo clippy --offline -- -D warnings`: passed.
- `cargo test --offline`: 1768 passed, 0 failed, 5 ignored.
- `regenerate_schema_snapshot` and `generate_bindings` reproduce the checked-in files byte for byte. The `generated-types.ts` diff is layout and doc comments only (no type shape changes). Its trailing whitespace after `;` before a doc comment is canonical specta output (281 such lines already on `main`), so `git diff --check` flags it; every other path is clean.
- Node 24.14.0: `npm run lint`, `npm run typecheck`, `npm run format:check` passed; Vitest 93 files / 1468 tests passed. Node 22 fails the pre-existing RSD CLDR assertion, unrelated.

## Known limits (not defects of this fix)

- Legacy positions whose stored history is already oversold (or holds zero/empty quantities) now reject edit, delete and undo on that position with `validation.sellExceedsHoldings` until the offending sell is fixed; the app still opens (startup recalculation only warns). Run the release dry run on a copy of the real profile to see whether any exist.
- A held stock whose provider returns an error (e.g. delisted) keeps the stocks refresh job pending unless a manual price override exists. Retries happen only on the next mutation or unlock, never in a loop.
- Crypto history older than 365 days uses the last known price (provider tier limit).
- Other-asset average purchase price is still computed in f64; the holdings invariant itself is exact.
- Crypto and other assets have no import batches or undo, in MCP or in the UI; not built here.
- Bank MCP has no explicit balance-record tool (parity gap outside import integrity).
