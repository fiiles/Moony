# Sync audit 2, plan 1: the sync server (protocol 3 and the server findings) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sync server answers `protocol: 3` (a retired token reads only vault info and key changes; vault IDs are unique ignoring letter case) and closes A2-M1, S2-M3, S2-M4 and the cheap halves of S2-M1 and S2-M2.

**Architecture:** All changes are in the server repository `Moony-relay` (Node, TypeScript, Hono, better-sqlite3, vitest). One new access level in the vault store, one migration, a two-step pull query, two in-memory proxy observations surfaced in the admin overview, and a way to clear the admin lockout from the command line. No new error code, no new route.

**Tech Stack:** Node ≥ 22, TypeScript, `hono`, `better-sqlite3`, `vitest`; the admin UI is Preact under `admin-ui/`.

**Spec:** `Moony-tauri` repository, `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`: §4.5, §4.7–§4.10, §5.2 (option 3, approved), §7. Read them before starting. The server's contract is `Moony-relay/DESIGN.md`; its rules for contributors are `AGENTS.md` and `CONTRIBUTING.md` in that repository and take precedence over this plan where they differ.

## Global Constraints

- Repository: `/Users/filipkral/Programování/FinanceApp/Moony-relay`. Work in **its own worktree and branch** `feat/protocol-3`, cut from `main` (at `85035d6` when this plan was written; other sessions prepare this repository for publication, so rebase on `main` before the final task). Never push, never merge; the owner does both.
- **Do not contact or redeploy the server running on the owner's home network.** Tests run in process (`testApp` in `test/helpers.ts`).
- **No new error code on any existing route** (spec §7.1): an older app keeps an unknown code during a key change as "no answer". Use `403 token_read_only` and `409 vault_exists`.
- Nothing the server logs or stores may gain an account number, token, vault or mailbox ID, file path or client address (DESIGN.md "Logging", "What the server knows").
- `npm test` and `npm run typecheck` pass after every task. Commit after every task.
- `DESIGN.md` is updated in the same commit as the behaviour it describes.

## Review Focus

1. A device that lagged over a key change: its first pull with the retired token must get `403 token_read_only` (not `401`, not `410`), so the app picks the new key up.
2. A blocked person's read grace (30 days after the block) must still read with the **current** token; only the token's role changes, not the entitlement rule.
3. A database written before migration 9 that happens to hold two IDs equal ignoring case (cannot happen with honest apps): the server must refuse to start with one line, not half-migrate.
4. A vault of very large ops (192 KiB each): a page is never empty while ops remain; the first op after `after` is always returned, and `more` is true exactly when ops above the page's last one exist.
5. An operator behind a proxy that sends `X-Forwarded-For` on health checks only: the warning must not flap or log more than one line.

---

### Task 1: A retired token reads only vault info and key changes (D4)

**Files:**
- Modify: `src/vaults.ts` (`Access`, `verdict`, `entitled`, `info`, `rekeys`), `src/app.ts` (`PROTOCOL`), `src/limits.ts` (the header comment "protocol v1" → "protocol 3")
- Modify: `DESIGN.md` (the paragraph "A previous token may `GET` the vault, pull ops, fetch the snapshot and fetch rekey bundles"; the rows of `GET …/ops`, `GET …/snapshot`, the three blob read routes; "People and accounts"; the `GET /v1/server` row)
- Test: `test/vault-rotation.test.ts`, `test/blob-api.test.ts`, `test/app.test.ts`

**Interfaces:**
- Produces: `export type Access = "lookup" | "read" | "write" | "owner";` `lookup` is allowed to the current and to a previous token; `read`, `write`, `owner` need the current token (`403 token_read_only` for a previous one). Entitlement: `lookup` and `read` share today's read rule (until 30 days after `paidUntil`). `export const PROTOCOL = 3;`
- `vaults.info` and `vaults.rekeys` authorise with `"lookup"`. `pull`, `latestSnapshot` and the blob store's `find`/`list`/`open` keep `"read"`, which now needs the current token.

- [ ] **Step 1: Failing tests.** In `test/vault-rotation.test.ts` change "makes the new token current and the old one read-only" so that, after a rotation, with the **old** token: `GET /v1/vaults/:id` → 200, `GET …/rekey` → 200, `GET …/ops` → `403 {"error":"token_read_only"}`, `GET …/snapshot` → 403 same, `POST …/ops` → 403 same; and with the new token all of them work. Add:

```ts
it("refuses a retired token on pull before looking at the floor", async () => {
  // prune so that after < floor, then pull with the retired token: 403 token_read_only, not 410
});
it("keeps the read grace of a blocked person for the current token", async () => {
  // block the person; current token: GET ops 200 within 30 days; write 402
});
```

In `test/blob-api.test.ts` the two tests that pin "a previous token reads and lists" (around lines 142 and 461) flip: `HEAD`, `GET` and the listing with the previous token answer `403` with `Moony-Error: token_read_only`. In `test/app.test.ts`: `GET /v1/server` answers `protocol: 3`.

- [ ] **Step 2: `npx vitest run test/vault-rotation.test.ts test/blob-api.test.ts test/app.test.ts`: the changed tests fail.**
- [ ] **Step 3: Implement.** In `verdict`: `if (role === "previous" && access !== "lookup") return fail(403, "token_read_only");`. Keep the check order: unknown vault `404`, unknown token `401`, role, entitlement.
- [ ] **Step 4: `npm test` passes.** Fix any other test that read with a retired token by design (search the tests for a rotation followed by a read with the old `Vault`).
- [ ] **Step 5: `DESIGN.md`** as listed, then **commit** (`feat: protocol 3, a retired token reads only vault info and key changes`).

### Task 2: Vault IDs are unique ignoring letter case (A2-M1)

**Files:**
- Modify: `src/db.ts` (migration 9), `src/vaults.ts` (`doInit`, the prepared statements)
- Modify: `DESIGN.md` (storage layout; the `PUT /v1/vaults/:vaultId` row)
- Test: `test/vault-api.test.ts`, `test/start.test.ts`

**Interfaces:**
- Migration 9: `CREATE UNIQUE INDEX vaults_id_fold ON vaults (lower(vault_id))`.
- `doInit`: a vault whose ID equals the requested one ignoring ASCII case but not exactly → `409 vault_exists`. A pending deletion is matched the same way → `409 vault_deleting`.

- [ ] **Step 1: Failing tests.**

```ts
it("refuses a vault ID that differs from an existing one only by letter case", async () => {
  // same account, id "…AbC…" created; PUT "…abc…" with another token → 409 vault_exists;
  // the first vault's snapshot and one blob are still served unchanged
});
it("treats a pending deletion ignoring case", async () => { /* → 409 vault_deleting */ });
it("refuses to start on a database that already holds two IDs equal ignoring case", () => {
  // build a schema-8 database by hand with two such rows; openDb throws; the message carries no ID
});
```

- [ ] **Step 2: Run them, see them fail.**
- [ ] **Step 3: Implement.** Migration 9 fails on existing duplicates by itself (the unique index); make `openDb`'s caller print one line without the IDs (`start.ts` already prints refusals in one line; reuse that path). Add `foldedVault: db.prepare("SELECT vault_id FROM vaults WHERE lower(vault_id) = lower(?)")` and the same for `pending_wipes`.
- [ ] **Step 4: `npm test` passes. Commit** (`fix: vault IDs are unique ignoring letter case`).

### Task 3: A pull reads only what it sends (S2-M4)

**Files:**
- Modify: `src/vaults.ts` (`pull`, `rekeys`, statements)
- Test: `test/vault-api.test.ts`, `test/vault-rotation.test.ts`

**Interfaces:** the answers of `GET …/ops` and `GET …/rekey` are byte for byte what they are today.

- [ ] **Step 1: Failing test.** Count what the store reads: wrap the database's statement for the op ciphertexts in the test (or expose a test-only counter on `createVaults`' return value named `stats.opBytesRead`), push 60 ops of 150 KiB, pull with `limit=1000`, and assert the page holds the same ops as before **and** `opBytesRead ≤ 4 MiB + one op`. The same shape for `/rekey` with 32 bundles of 200 KiB.
- [ ] **Step 2: Run, see it fail** (today it reads all 60).
- [ ] **Step 3: Implement** in two steps: `SELECT seq, size FROM ops WHERE vault_id = ? AND seq > ? ORDER BY seq LIMIT ?`, choose the longest prefix whose sizes fit `MAX_PULL_BYTES` (always at least one row), then `SELECT seq, op_id, ct FROM ops WHERE vault_id = ? AND seq > ? AND seq <= ? ORDER BY seq`. `more` and `head` are computed as today. The same for rekeys with `length(rekey)`.
- [ ] **Step 4: `npm test` passes. Commit** (`fix: a pull reads only the rows it returns`).

### Task 4: Proxy mistakes are visible (S2-M1 cheap half, S2-M3)

**Files:**
- Create: `src/proxy-watch.ts`
- Modify: `src/app.ts` (one middleware before the routers), `src/admin/api.ts` (`GET /overview`), `admin-ui/src/screens/Overview.tsx`, `admin-ui/src/i18n/en.ts`, `admin-ui/src/i18n/cs.ts`, `compose.yml`, `README.md` (first-run section and "https behind a reverse proxy"), `DESIGN.md` (Configuration; Admin API `overview` row)
- Test: `test/proxy-watch.test.ts` (new), `test/app.test.ts`, `admin-ui/test/screens.test.tsx`

**Interfaces:**
- `export function createProxyWatch(trustProxy: boolean, log: (line: string) => void): { see(req: Request): void; warning(): "forwarded_untrusted" | "direct_client" | null }`
  - `trustProxy` off and a request carries `X-Forwarded-For` or `Forwarded` → `forwarded_untrusted`; logs once: `proxy: forwarding headers seen while TRUST_PROXY is off; all clients share one rate limit`.
  - `trustProxy` on and a request carries neither header → `direct_client`; logs once: `proxy: a request arrived without forwarding headers while TRUST_PROXY is on; bind the port to the proxy only`.
  - `/health` is not looked at (container health checks reach the server directly by design).
  - With `trustProxy` on, one line at start: `TRUST_PROXY is on: only your reverse proxy may reach this port`.
- `GET /admin/api/overview` gains `proxyWarning: "forwarded_untrusted" | "direct_client" | null`.
- Admin texts (English; Czech translated in `cs.ts`): `forwarded_untrusted` → "This server seems to be behind a proxy, but TRUST_PROXY is off. All devices share one rate limit, and anyone can use it up. Set TRUST_PROXY=1 if exactly one reverse proxy stands in front." `direct_client` → "TRUST_PROXY is on, but a request reached the server without passing the proxy. Publish the port to the proxy only (for example 127.0.0.1:8787:8787), otherwise clients can choose their own rate-limit key."
- `compose.yml`: keep `"8787:8787"`, add above it the commented alternative `# - "127.0.0.1:8787:8787"   # behind a reverse proxy on this machine (with TRUST_PROXY=1)`. README: the Caddy recipe uses the loopback binding, and the first-run section names the proxy step.

- [ ] **Step 1: Failing unit tests** for `createProxyWatch` (each state, logging exactly once, `/health` ignored, the warning stays once set) and one app test (`overview.proxyWarning` after a forwarded request with `trustProxy: false`; `null` on a clean server), one screen test for each banner.
- [ ] **Step 2: Run, fail. Step 3: Implement. Step 4: `npm test` and `npm run typecheck` pass. Step 5: Commit** (`feat: the admin overview warns about a proxy that is not declared or can be bypassed`).

### Task 5: The admin lockout can be cleared, and a logged-in admin is not locked out by strangers (S2-M2 cheap half)

**Files:**
- Modify: `src/admin/auth.ts` (`attemptLimit`, the password re-checks, `resetAdmin`), `src/cli.ts`, `README.md` ("Lost password", "Known limits"), `DESIGN.md` (Authentication: Guessing; Lost password)
- Test: `test/admin-auth.test.ts`, a CLI test next to the existing `reset-admin` one

**Interfaces:**
- `moony-sync unlock-admin`: writes the setting `lockout_cleared_at` = now (unix seconds) and prints `Login attempts were reset.`; exit code 0; the same "no database" refusal as `reset-admin`. `reset-admin` writes the same setting.
- The server reads `lockout_cleared_at` wherever it reads the limiters (`locked`, `reserve`); a value newer than the one it last acted on clears the per-client and overall counters of the password, setup and second-factor limiters.
- A password re-check that arrives **with a valid session** (change password, enrol or disable the second factor, reveal a connection code) is counted against that client only, never against the overall count, and is not refused because the overall count is full.
- `runCli` usage line becomes `usage: moony-sync reset-admin | unlock-admin`.

- [ ] **Step 1: Failing tests.**

```ts
it("unlock-admin clears a full lockout on a running server", async () => { /* 50 failures from many addresses → login 429; runCli(["unlock-admin"]) → the right password logs in */ });
it("reset-admin clears the lockout too", async () => {});
it("a logged-in admin's password re-check is not refused by strangers' failures", async () => { /* fill the overall bucket; POST /password with the session and the right current password → 204 */ });
it("a logged-in admin's wrong re-checks still lock that client after five", async () => {});
```

- [ ] **Step 2: Run, fail. Step 3: Implement. Step 4: `npm test` passes.**
- [ ] **Step 5: README "Known limits"** gains one sentence: on a home network anyone can keep the login refused until the operator runs `moony-sync unlock-admin`. **Commit** (`feat: unlock-admin, and a logged-in admin is not locked out by others' failures`).

### Task 6: The contract says what it is

**Files:**
- Modify: `DESIGN.md`, `CHANGELOG.md`, `README.md`

- [ ] **Step 1:** `DESIGN.md`: the title and the opening paragraph say "protocol 3"; the section "Changes to the v1.1 API" becomes part of the API description (there is no public v1.1); `features` are described as `blobs` and `kit`, both part of protocol 3 and still listed. Add a section **Compatibility**: a server ignores request fields it does not know; an app ignores answer fields and features it does not know; a change an older app would mishandle gets a new protocol number, and a new error code on an existing route counts as such a change; error codes are never reused with another meaning; protocol 3 is the first published protocol; the Moony app versions that speak it (leave the app version as "the first release with sync", the owner fills the number).
- [ ] **Step 2:** `CHANGELOG.md`: the unreleased entry lists protocol 3 and Tasks 1–5 in the changelog's own style.
- [ ] **Step 3:** `rg -n "protocol 2|protocol v1|v1\\.1" .` in the repository (outside `node_modules`, `dist`) finds nothing that still describes the current server.
- [ ] **Step 4: Rebase on `main`, `npm test`, `npm run typecheck`, `npm run build`. Commit** (`docs: protocol 3 and the compatibility rules`).

## Not in this plan

- Strict spelling of IDs and tokens (spec §7.3 item 4) and the `kit-history` feature (D7): not approved yet; each is one task when the owner says yes.
- The deferred halves of S2-M1, S2-M2, S2-M3 (proxy address lists, a per-browser cookie against lockout): with the hosted service.
- The app's side of D4 (every read treats `403 token_read_only` as "pick the key up") and the app's test server: plan 2.
- Deploying to the home server: the owner's.
