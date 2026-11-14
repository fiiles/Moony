# Self-hosted sync server — Plan 1: the server

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Cloudflare Worker relay with one Node server on SQLite and files, with a
web admin, shipped as one Docker image.

**Architecture:** One process serves the sync API (`/v1`), the admin API (`/admin/api`) and the
static admin interface on one port. The Durable Objects become plain modules over one SQLite
database; R2 becomes files; alarms become one housekeeping loop. The `/v1` contract stays
byte-compatible with `DESIGN.md` v1.1 as on branch `feat/pairing-ack`, except the changes in the
spec §6.

**Tech Stack:** Node 22 LTS, TypeScript (ESM), Hono + `@hono/node-server` (fetch-style handlers,
so `src/index.ts` ports with small changes), `better-sqlite3` (synchronous, WAL), Vitest on Node,
`@node-rs/argon2`, Preact + Vite for the admin interface, Docker (multi-arch).

**Spec:** `/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/specs/2026-11-14-self-hosted-sync-server-design.md`
(read §2, §5–§8, §10–§12 before any task). Plan 2 (the Moony app) is written after this plan is
done.

**Where:** repository `/Users/filipkral/Programování/FinanceApp/Moony-relay`, new branch
`feat/node-server` cut from `feat/pairing-ack`, in a new worktree
`/Users/filipkral/Programování/FinanceApp/Moony-relay-node`. Never work in `Moony-relay` or
`Moony-relay-ack` themselves. Commit after every task; never push.

## Global Constraints

- The old Worker sources stay in place (read-only reference) until Task 9 removes them. New code
  lives under `server/` so both can coexist: `server/src`, `server/test`, `server/admin-ui`.
  Task 9 moves `server/*` to the repository root.
- Error bodies are `{"error": "<code>"}`; every status code and error name of the old relay is
  kept unless the spec §6 changes it.
- All limits come from the old `src/limits.ts`, copied unchanged (drop only the Polar ones).
- No outbound network calls from the server. No dependency that phones home.
- Logs (stdout) never contain account numbers, tokens, vault IDs, mailbox IDs or client
  addresses.
- Time is `nowSeconds()` from one injectable clock (`Clock = () => number`), so tests move time
  without sleeping.
- SQLite: WAL, `foreign_keys=ON`, `synchronous=NORMAL`; money-style rule of the project does not
  apply here, but timestamps are unix seconds INTEGER.
- Every time-based rule (mailbox expiry, token expiry, read grace) is enforced at request time as
  well as by housekeeping.
- Tests call the app in-process (`app.fetch(new Request(...))`) with a temporary `DATA_DIR`; no
  sockets, no Docker, except Task 9's smoke run.
- Commit messages in the repository's style (`feat: …`, `test: …`, `docs: …`), ending with
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. A crash or `docker stop` between writing a snapshot file and recording it must leave no file
   under a final name and no record without a file (Task 3: test that a failed record removes the
   file and that start-up sweeps `*.tmp`).
2. The container stopped for days: on start, expired mailboxes, retired tokens and rekey bundles
   are refused at request time even before housekeeping runs (Task 5).
3. A fresh install reachable from the network before the owner finishes setup: every
   `/admin/api` route except `state` and `setup` answers 401, and five wrong setup codes lock
   setup attempts for a while (Task 6).
4. Deleting a person while their device is uploading a snapshot: the upload is refused and its
   file removed, as the old vault deletion did (Task 7).
5. `X-Forwarded-For` sent by a client when `TRUST_PROXY` is off must not change the rate-limit
   key (Task 5).

## File Structure

```
server/
  package.json, tsconfig.json, vitest.config.ts
  src/
    config.ts            env → Config
    clock.ts             Clock, nowSeconds
    db.ts                open(dataDir) → Database, numbered migrations
    encoding.ts          copied from src/encoding.ts
    limits.ts            copied from src/limits.ts minus Polar
    http.ts              copied from src/http.ts (json, errorResponse, readBody, readJson, Result)
    people.ts            people + accounts store, entitlement
    vaults.ts            port of VaultDO
    snapshots.ts         snapshot file store
    mailboxes.ts         port of MailboxDO
    ratelimit.ts         in-memory limiters, clientKey
    housekeeping.ts      periodic tasks
    sync-api.ts          /v1 router (port of src/index.ts)
    admin/auth.ts        setup code, password, sessions, guards
    admin/totp.ts        RFC 6238 + backup codes
    admin/api.ts         /admin/api router
    admin/backup.ts      archive stream
    connection-code.ts   encode/decode
    app.ts               createApp(deps) → { fetch, close }
    main.ts              listen, graceful shutdown
    cli.ts               `moony-sync reset-admin`
  admin-ui/              Preact + Vite app (Task 8)
  test/                  helpers.ts + *.test.ts
Dockerfile, compose.yml, README.md, DESIGN.md   (Task 9)
```

---

### Task 1: Scaffold, config, database, app shell

**Files:** create `server/package.json`, `server/tsconfig.json`, `server/vitest.config.ts`,
`server/src/{config,clock,db,encoding,limits,http,app,main}.ts`, `server/test/{helpers,app}.test.ts`.

**Interfaces (produces):**

```ts
// config.ts
export interface Config { port: number; dataDir: string; publicUrl: string | null;
  trustProxy: boolean; require2fa: boolean; setupCode: string | null; version: string }
export function loadConfig(env: NodeJS.ProcessEnv): Config   // defaults per spec §5.2
// clock.ts
export type Clock = () => number            // unix seconds
// db.ts
export type Db = import("better-sqlite3").Database
export function openDb(dataDir: string): Db // creates dir, WAL, runs migrations, refuses a newer schema
export const MIGRATIONS: readonly string[]  // append-only; index+1 = version in table schema_version
// app.ts
export interface Deps { config: Config; db: Db; clock: Clock; log: (line: string) => void }
export function createApp(deps: Deps): { fetch(req: Request, clientAddress?: string): Promise<Response>; close(): void }
export const PROTOCOL = 2
// test/helpers.ts
export function testApp(overrides?: Partial<Config>): { app, db, clock: { now: number }, dataDir, logs: string[], call(method, path, init?) }
```

- [ ] Write failing tests in `server/test/app.test.ts`:
  - `GET /health` → 200 `{"ok":true}`.
  - `GET /v1/server` → 200 `{"name":"moony-sync","version":<config.version>,"protocol":2,"mode":"self_hosted"}`.
  - unknown path → 404 `{"error":"not_found"}`.
  - `loadConfig({})` → port 8787, dataDir `/data`, trustProxy false, require2fa false; `PORT=9000 TRUST_PROXY=1 REQUIRE_2FA=1 PUBLIC_URL=https://x.example SETUP_CODE=abc` are read.
  - `openDb` twice on the same dir keeps data; a database whose `schema_version` is higher than `MIGRATIONS.length` throws `database_newer_than_server`.
- [ ] Run `cd server && npm test` → fails (nothing exists).
- [ ] Implement. `encoding.ts`, `limits.ts`, `http.ts` are copies of the old files (keep
  `crypto.subtle` usage; Node 22 has it globally; replace `crypto.subtle.timingSafeEqual` with
  `node:crypto` `timingSafeEqual`). `main.ts` starts `@hono/node-server`, passes the socket
  address as `clientAddress`, handles SIGTERM/SIGINT: stop accepting, `close()`, WAL checkpoint.
  Version comes from `package.json` (start at `1.0.0`).
- [ ] `npm test` and `npm run typecheck` pass. Commit.

### Task 2: People, accounts, entitlement

**Files:** create `server/src/people.ts`, `server/src/sync-api.ts` (accounts part),
`server/test/accounts.test.ts`; extend `MIGRATIONS`, `test/helpers.ts`.

**Interfaces (produces):**

```ts
export type PersonState = "active" | "blocked"
export interface Person { id: string; name: string; state: PersonState; account: string; createdAt: number; blockedAt: number | null }
export interface People {
  create(name: string): Person                         // generates id (uuid) and a unique 16-digit account
  list(): Person[]; get(id: string): Person | null; byAccount(account: string): Person | null
  rename(id: string, name: string): Result; setState(id: string, state: PersonState): Result
  remove(id: string): Result                           // Task 7 extends it to wipe vaults
  paidUntil(account: string): number | null            // null = unknown account; active → SELF_HOSTED_PAID_UNTIL; blocked → blockedAt
}
export function createPeople(db: Db, clock: Clock): People
// helpers.ts
export function addPerson(t, name?): Person
```

Name rule: trimmed, 1–80 characters, else `invalid_name` (400).

- [ ] Failing tests (`accounts.test.ts`):
  - `POST /v1/accounts` → 403 `registration_closed`.
  - `GET /v1/accounts/me` without header → 400 `missing_account`; unknown number → 404
    `account_not_found`; active person → `{"paidUntil":253402300799,"active":true,"vaults":0}`;
    blocked person (clock at T) → `paidUntil: T`, `active:false`.
  - `POST /v1/accounts/me/checkout` → 409 `self_hosted`; `DELETE /v1/accounts/me` → 403
    `managed_by_operator`; `/v1/webhooks/polar` and `/v1/dev/grant` → 404.
  - unblocking restores `paidUntil` 253402300799; two created people never share an account.
- [ ] Implement; `sync-api.ts` exports `syncRoutes(deps, stores): Hono` mounted by `app.ts`.
  Response shapes of `GET /v1/accounts/me` copy the old `AccountDO.status()` (read
  `src/account.ts`).
- [ ] Tests and typecheck pass. Commit.

### Task 3: Vaults, ops, snapshots, rotation

The largest task: port `src/vault.ts` (565 lines on `feat/pairing-ack`) and the vault part of
`src/index.ts`. Port the logic statement by statement; change only storage and entitlement.

**Files:** create `server/src/vaults.ts`, `server/src/snapshots.ts`, `server/test/vault.test.ts`;
extend `sync-api.ts`, `MIGRATIONS`, helpers.

**Interfaces (produces):**

```ts
export interface Vaults {            // same method names and Result shapes as VaultDO's RPC
  init(vaultId, account, token): Promise<Result<{created: boolean}>>
  info(vaultId, token); destroy(vaultId, token); push(vaultId, token, ops); pull(vaultId, token, after, limit)
  authorizeSnapshot(vaultId, token, seq); recordSnapshot(vaultId, token, {file, seq, size}); latestSnapshot(vaultId, token)
  rotateToken(vaultId, token, next, rekey); rekeys(vaultId, token, after, limit)
  housekeep(vaultId): void           // the old alarm body
  listByAccount(account): { vaultId: string; bytes: number; lastActivity: number | null }[]
  wipe(vaultId): void                // unconditional delete (admin), Task 7
}
export interface Snapshots {         // files under <dataDir>/snapshots/<vaultId>/
  write(vaultId, seq, body: ReadableStream<Uint8Array>, maxBytes): Promise<{ file: string; size: number } | "too_large">
  open(vaultId, file): { stream: ReadableStream<Uint8Array>; size: number } | null
  remove(vaultId, file): void; removeVault(vaultId): void; sweepTemp(): void; listFiles(vaultId): { file: string; mtime: number }[]
}
```

Decisions:
- Entitlement: wherever `VaultDO` used its cached `paidUntil`, re-asked the account or was poked,
  call `people.paidUntil(account)` directly. The self-hosted "lapse starts when first seen" logic
  and the 90-day lapsed delete are **not** ported (spec §5.4): `housekeep` never deletes a vault
  for lapse.
- `last_activity` (unix seconds rounded down to the minute) is updated on every authorised call.
- Snapshot upload: stream to `<seq>-<uuid>.tmp` with a running byte count, `fsync`, rename to
  `<seq>-<uuid>`; on a failed or stale record remove the file. `sweepTemp()` runs at start.
- Blocked person: writes → the old lapsed error (402 with the old error name; read it from
  `vault.ts`); reads allowed until `blockedAt + READ_GRACE_SECONDS`; `rotateToken` and `destroy`
  need only the current token.

- [ ] Port `test/vault.test.ts` (720 lines) to `server/test/vault.test.ts` first: keep every test
  name and assertion; replace `SELF.fetch`/DO stubs with `t.call`, paid accounts with
  `addPerson`, "lapsed" setups with `people.setState(id,"blocked")` plus clock moves, time travel
  with `t.clock.now`. Tests that only make sense with subscriptions (90-day lapsed delete, pokes)
  are replaced by: blocked for 31 days → pull refused with the lapsed read error; blocked for 400
  days then `housekeep` → vault still exists; unblock → pull works.
- [ ] Add Review Focus 1 tests: `recordSnapshot` failing leaves no file; a leftover `.tmp` is gone
  after `sweepTemp()`; a body longer than `Content-Length`/limit yields 413 and no file.
- [ ] Run → fail. Implement. Run → pass, typecheck. Commit.

### Task 4: Pairing mailboxes

**Files:** create `server/src/mailboxes.ts`, `server/test/pairing.test.ts`; extend `sync-api.ts`,
`MIGRATIONS`.

**Interfaces (produces):** `Mailboxes { put(id, slot: 1|2|3, body): Result; take(id, slot): Result<{data: Uint8Array}>; burn(id): void; housekeep(): void }`
— same results and error names as `MailboxDO` (`slot_taken`, `empty`, `consumed`, `mailbox_gone`).

- [ ] Port `test/pairing.test.ts` (230 lines) unchanged in names and assertions; alarms become
  clock moves, with and without calling `housekeep()` in between (a mailbox past 10 minutes
  answers `410 mailbox_gone` even when housekeeping never ran).
- [ ] Run → fail. Implement (port `src/mailbox.ts`). Pass. Commit.

### Task 5: Rate limits and housekeeping

**Files:** create `server/src/ratelimit.ts`, `server/src/housekeeping.ts`,
`server/test/{ratelimit,housekeeping}.test.ts`; wire into `sync-api.ts`, `app.ts`.

**Interfaces (produces):**

```ts
export interface Limiter { hit(key: string): boolean }            // true = over the limit
export function fixedWindow(limit: number, periodSeconds: number, clock: Clock): Limiter
export function clientKey(req: Request, clientAddress: string | undefined, trustProxy: boolean): string
export function startHousekeeping(deps, stores, intervalSeconds?): { runOnce(): void; stop(): void }
```

Limits (per minute, as `wrangler.jsonc`): account lookups 60/IP, vault 600/vault+token-prefix and
1200/IP, pairing 120/IP, `GET /v1/server` shares the account-lookup limiter. Limiter maps drop
expired windows so memory stays bounded.

- [ ] Failing tests: the 61st account lookup from one address within a minute → 429
  `rate_limited`, a different address is unaffected, the next minute is allowed again;
  `clientKey` ignores `X-Forwarded-For` when `trustProxy` is false and uses its first entry when
  true (Review Focus 5); `runOnce()` burns expired mailboxes, forgets previous tokens and rekey
  bundles older than 30 days, prunes ops covered by a snapshot and older than 30 days, removes
  snapshot files with no record older than an hour; retired token one second past 30 days is
  `401` **without** `runOnce()` (Review Focus 2).
- [ ] Implement (`startHousekeeping` uses an unref'd timer, default daily plus one run at start).
  Pass. Commit.

### Task 6: Admin authentication

**Files:** create `server/src/admin/{auth,totp}.ts`, `server/src/admin/api.ts` (auth routes),
`server/src/cli.ts`, `server/test/admin-auth.test.ts`; extend `MIGRATIONS`, `app.ts`, helpers.

**Routes (all JSON, under `/admin/api`):**

| Route | Body → answer |
|---|---|
| `GET state` | → `{setupNeeded, authenticated, name?, twoFactor: boolean, require2fa, mustEnrol2fa}` |
| `POST setup` | `{code, name, password}` → 204 + session; `403 setup_done`, `401 bad_code`, `400 weak_password` (< 10 chars), `400 invalid_name` |
| `POST login` | `{name, password, totp?}` → 204 + session; `401 bad_credentials`; `401 totp_required`; `401 bad_totp` |
| `POST logout`, `POST logout-all` | 204 |
| `POST password` | `{current, next}` → 204, ends other sessions |
| `POST 2fa/enrol` | → `{secret, otpauthUrl}` (pending) |
| `POST 2fa/confirm` | `{totp}` → `{backupCodes: string[10]}` |
| `POST 2fa/disable` | `{password}` → 204; `403 required` when `require2fa` |

Decisions:
- Setup code: `config.setupCode` or 24 random base32 characters, logged once at every start
  while no admin exists as the line `Setup code: <code>`. Compared in constant time.
- Password hash Argon2id (`@node-rs/argon2` defaults). Session: 32 random bytes, stored as
  SHA-256, cookie `moony_admin` `HttpOnly; SameSite=Strict; Path=/`, plus `Secure` when the
  request URL is https or (`trustProxy` and `X-Forwarded-Proto: https`). Idle timeout 12 h.
- Every non-GET admin route requires `Origin` to equal the request's own origin (403
  `bad_origin`); every route except `state`, `setup`, `login` requires a session (401
  `unauthorized`). With `mustEnrol2fa` only `state`, `2fa/*`, `logout` answer; others 403
  `enrol_2fa`.
- Limits: login, setup and TOTP attempts 5 per 15 minutes per client address and 50 per 15
  minutes overall → 429 `rate_limited`.
- TOTP: SHA-1, 6 digits, 30 s, accepts the previous and next step, a code cannot be used twice;
  backup codes: 10 × 10 characters, stored hashed, single use, accepted in the `totp` field.
- `cli.ts reset-admin`: opens the database in `DATA_DIR`, deletes admin, sessions, TOTP and
  backup codes, writes a fresh setup code into a `settings` row that the running server reads on
  the next `state`/`setup` call, and prints it. Exposed as the `moony-sync` bin.

- [ ] Failing tests: the whole table above; Review Focus 3; RFC 6238 appendix B vectors for
  SHA-1 (time 59 → `287082` with 6 digits, 1111111109 → `081804`); a reused TOTP code is refused;
  a backup code works once; session expires after 12 h idle (clock move); `reset-admin` leaves
  people untouched and makes `state.setupNeeded` true; `REQUIRE_2FA=1` → after login
  `mustEnrol2fa` and `GET /admin/api/people` → 403 `enrol_2fa`.
- [ ] Implement. Pass. Commit.

### Task 7: Admin API — people, connection code, overview, settings, backup

**Files:** create `server/src/connection-code.ts`, `server/src/admin/backup.ts`,
`server/test/{admin-people,connection-code,backup}.test.ts`; extend `admin/api.ts`, `people.ts`.

**Connection code (shared with the app, Plan 2):**
`moony-sync1:<16-digit account>:<base64url-no-padding of the UTF-8 normalised server address>`.

```ts
export function encodeConnectionCode(url: string, account: string): string
export function decodeConnectionCode(code: string): { url: string; account: string } | null
export function normalizeServerUrl(input: string): string | null
// http or https; host required; no user, password, query or fragment; no path but "/";
// lowercase host; default port dropped; no trailing "/"; at most 256 characters.
```

**Routes:**

| Route | Answer |
|---|---|
| `GET overview` | `{address: string|null, addressSuggestion: string, people, profiles, bytesUsed, bytesFree, lastActivity: number|null, version, insecure: boolean}` |
| `GET people` | `[{id, name, state, profiles, bytes, lastActivity, createdAt}]` |
| `POST people` | `{name}` → 201 `{person, connectionCode}`; `409 address_not_set` until the address is saved |
| `GET people/:id` | person + `vaults: [{id, shortId, bytes, lastActivity}]` (`shortId` = first 6 characters) |
| `PATCH people/:id` | `{name?}` / `{state?}` |
| `POST people/:id/connection-code` | `{password}` → `{connectionCode}`; `401 bad_credentials` |
| `DELETE people/:id` | 204; wipes vaults, snapshot files and the account |
| `DELETE people/:id/vaults/:vaultId` | 204 |
| `GET settings`, `PUT settings` | `{publicUrl}`; `400 invalid_address` |
| `GET backup` | `application/x-tar` attachment `moony-sync-backup-<YYYYMMDD-HHmm>.tar` |

Decisions: address = saved setting, else `config.publicUrl`, else null with
`addressSuggestion` = the request's origin. `insecure` = the admin page was requested over http
from a host other than `localhost`/`127.0.0.1`. `bytes` = op bytes + snapshot sizes. `bytesFree`
from `fs.statfs(dataDir)`. Backup: SQLite online backup into a temporary file, then a tar with
`moony-sync.db` and `snapshots/**` (skip `*.tmp`); written with Node streams, no shelling out.
The account number never appears in any answer except inside a connection code.

- [ ] Failing tests: the table; round-trip and rejects for the connection code (`ftp://`, a
  path, credentials in the URL, 257 characters → null; `HTTP://Host.Example:80/` →
  `http://host.example`); deleting a person makes their vault calls answer 404 `vault_not_found`
  and `GET /v1/accounts/me` 404; Review Focus 4 (person deleted between `authorizeSnapshot` and
  `recordSnapshot` → the upload is refused and its file removed); the backup tar opened in the
  test contains a database with the people rows and the snapshot file bytes; a grep of every
  admin JSON answer for the 16-digit account finds none outside `connectionCode`.
- [ ] Implement. Pass. Commit.

### Task 8: Admin web interface

**Files:** `server/admin-ui/**` (Vite + Preact + TypeScript), `server/src/app.ts` (serve
`server/admin-ui/dist` at `/`, `index.html` fallback for unknown non-API GET paths, strict
`Content-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self'`,
`X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`), `server/test/static.test.ts`.

Source of the look: copy `tokens.css` and `moony.css` from
`/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/design-system/` into
`admin-ui/src/styles/` unchanged, plus the Inter Variable font from the `@fontsource-variable/inter`
package (latin + latin-ext). Use the existing classes (`app`, `sidebar`, `brand`, `moon`, `nav__item`,
`page`, `page-head`, `eyebrow`, `stats`, `stat`, `card`, `card--table`, `table__head`,
`table__row`, `badge`, `badge--outline`, `btn`, `btn--primary`, `btn--danger`,
`btn--destructive`, `alert`, `field`, `input`, `switch`, `modal`). Layout reference (earlier
account model, same look):
`/Users/filipkral/Programování/FinanceApp/Moony-tauri/.superpowers/brainstorm/16180-1794645219/content/sync-server-screens-v3.html`.
Dark theme: set `data-theme="dark"` on `<html>` from `prefers-color-scheme`. Green only for
gain, red-brown only for destructive actions; person states are neutral badges.

Screens (spec §7): Setup, Login (+ second-factor step, http notice), forced 2FA enrolment,
Overview, People (table, Add person modal that ends on the connection code as text + QR + copy,
row actions rename / show code (asks password) / block / unblock / delete with a typed-name
confirmation), Person detail (profiles with delete), Backup (download + the "server went back"
explanation from spec §7.3), Settings (address, password, second factor, log out everywhere,
language). First visit with no address saved: Overview asks to confirm `addressSuggestion`.
The sidebar brand subtitle is "Synchronizační server" / "Sync server". Narrow screens: the
sidebar collapses to a top bar below 820 px.

Language: `cs` and `en` dictionaries in `admin-ui/src/i18n/{cs,en}.ts` with identical keys
(a unit test compares the key sets); initial language from `navigator.language`, choice kept in
`localStorage`. Czech copy: sentence case, vykání, "synchronizační server", never "relay".
QR: a small dependency-free generator (`qrcode-generator`), rendered as inline SVG.

- [ ] Failing tests: `GET /` serves `index.html` with the CSP header; `/admin/api/unknown` is
  JSON 404, not `index.html`; i18n key parity; a Vitest + `@testing-library/preact` test per
  screen for its main path against a mocked `fetch` (setup → overview; add person shows a code
  starting `moony-sync1:`; delete needs the typed name).
- [ ] Implement; `npm run build` in `server/` builds the UI then the server.
- [ ] Verify by eye: run the server on a temp data dir, open it, walk setup → address → add
  person → block → delete → 2FA enrol → logout → login, in light and dark, at 1280 px and 400 px
  wide. Fix what looks wrong. Commit.

### Task 9: Image, compose, documents, removal of the Worker

**Files:** `Dockerfile`, `compose.yml`, `.dockerignore`, `README.md`, `DESIGN.md`; move
`server/*` to the repository root; delete `src/` (old), `test/` (old), `wrangler.jsonc`,
`worker-configuration.d.ts`, `scripts/cost-model.mjs`, `.dev.vars.example`.

- [ ] Before deleting anything: `git tag worker-final` on the current `feat/pairing-ack` tip
  (local tag, no push).
- [ ] Dockerfile: multi-stage on `node:22-bookworm-slim`; build stage installs build tools for
  `better-sqlite3` and builds server + UI; runtime stage copies `dist`, production
  `node_modules`, runs as user `node`, `ENV DATA_DIR=/data`, `VOLUME /data`, `EXPOSE 8787`,
  `HEALTHCHECK` via `node -e` fetching `http://127.0.0.1:8787/v1/server`,
  `ENTRYPOINT ["node","dist/main.js"]`, and a `moony-sync` executable on `PATH` for
  `docker exec <c> moony-sync reset-admin`.
- [ ] `compose.yml`: service `moony-sync`, image `moony-sync:local` built from `.`, port
  `8787:8787`, volume `./data:/data`, `restart: unless-stopped`.
- [ ] Smoke run: `docker build -t moony-sync:local .`; run with a temp volume; read the setup
  code from `docker logs`; drive setup, add a person, `GET /v1/accounts/me` with the account from
  the code, `PUT` a vault, push one op, pull it, restart the container, pull again (same op),
  `docker exec … moony-sync reset-admin` prints a code and people survive. Record the commands
  and their output in the task report.
- [ ] `README.md`: what it is; run (compose), first setup, add a person, paste the code into
  Moony; https behind Caddy and Nginx Proxy Manager with `TRUST_PROXY=1`; plain http and what it
  exposes; backup and restore (with the "server went back" consequence); update; reset-admin;
  variables table; development commands. `DESIGN.md`: rewritten for this server — protocol v1.1
  tables kept, §6 changes applied, "Self-hosted mode", Polar, Cloudflare and cost sections
  removed, a new "What the server knows" including the person's name and last-activity time.
- [ ] `npm test`, `npm run typecheck`, `docker build` pass. Commit.

### Task 10: Security review and fixes

- [ ] A fresh reviewer on the most capable model reads the whole branch against spec §2
  (anti-goals), §5.5, §12 and the Review Focus list, and tries: setup race, session fixation and
  theft, CSRF with and without `Origin`, path traversal through vault IDs and backup, oversized
  and slow bodies, rate-limit bypass, log leaks, timing of code comparisons, the connection-code
  reveal. Every confirmed finding gets a failing test, then a fix. Commit.
