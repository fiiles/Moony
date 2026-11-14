# Self-hosted sync server — Plan 2: the Moony app

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Moony sets up sync on a person's own sync server from a connection code, checks every
server address before using it, allows plain `http` with a warning, lets a device change its
server address, and tests against the new Node server through its real admin path.

**Architecture:** A connection code (`moony-sync1:<account>:<base64url address>`) replaces account
creation and payment for an own server: setup adopts the account after `GET /v1/server` and
`GET /v1/accounts/me` agree. Every address that enters a flow (setup, pairing join, Kit join,
change address) goes through one Rust parser and one server check; the UI only displays their
answers. The cloud branch (create account, pay) stays in the code behind a disabled choice.

**Tech Stack:** Rust (Tauri 2, reqwest 0.13, rusqlite, tokio), React 19 + TypeScript, TanStack
Query, react-i18next (ten locales), Vitest, specta bindings.

**Spec:** `/Users/filipkral/Programování/FinanceApp/Moony-tauri/docs/specs/2026-11-14-self-hosted-sync-server-design.md`
(read §2, §5.3, §5.4, §6, §8, §9, §12 before any task). Server contract:
`docs/plans/2026-11-14-self-hosted-sync-server-plan-1-server.md` (header, Tasks 2, 6, 7).

## Global Constraints

- Work in the git worktree `../Moony-tauri-selfhost` on branch `feat/self-hosted-sync`, cut from local `main`; commit after every task; never push.
- AGENTS.md rules 1–10 hold; read `docs/DEPRECATED.md` before copying a pattern.
- `invoke()` only in `src/lib/tauri-api.ts` (rule 1).
- Any change to `shared/schema.ts` follows the 7-step procedure of `docs/standards/type-contract.md` (rule 2), including `cargo test generate_bindings -- --ignored`.
- New Rust wire types: per-field `#[serde(rename = "camelCase")]`, `specta::Type`, registered in `bindings.rs` (rule 3).
- Thin commands: logic in `src-tauri/src/services/sync/`, taking `ConnAccess`/`&Connection`; errors are `AppError` / `SyncError` with `validation.sync*` keys, never `Result<_, String>`; no `.unwrap()` outside tests (rules 4, 5).
- No migration expected: `sync_state.relay_url` and `sync_state.account_number` already hold the server address and account (rule 6, spec §9.4). If one turns out needed, it is append-only.
- Every new or changed user string lands in all ten locales in the same task: `en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh` (`src/i18n/locales.test.ts`, rule 8).
- User copy says "sync server" / "synchronizační server", never "relay"; Czech copy uses vykání, sentence case.
- Sync stays hidden: `feature::RELEASED` stays `false`.
- No error, log line or `Debug` output carries an account number, token, vault ID, connection code or address beyond `origin_of`.
- Rust service tests use in-memory profiles (`TestConn::new(migrated())`) and `FakeRelay`, as the existing `engine/tests/*.rs`; React components and command handlers are not unit-tested; pure TS utilities are (Vitest).

## Findings

Where the code contradicts or extends the spec's assumptions:

1. **Own-server detection breaks for a blocked person.** The UI recognises an own server by
   `paidUntil ≥ year 9000` (`isSelfHostedPaidUntil`, `src/utils/sync-status.ts`), but a blocked
   person's `paid_until` is the moment of blocking (spec §5.4), so the alert would offer "Renew" and
   `engine/status.rs` would add a deletion date (`paid_until + DELETE_AFTER_LAPSE`). The spec's
   "existing lapsed state with own-server wording" needs a new signal: `SyncStatus.ownServer` from
   the stored address (Task 3), and no `deletesAt` for an own server.
2. **Unknown error codes.** `RelayError::from_response` knows neither `403 registration_closed`
   nor `403 managed_by_operator` (both read as `Protocol` → "relay error"), and an old Worker relay
   answers `GET /v1/server` with `404 not_found` (→ `BadRequest("not_found")`). Task 2 maps them.
3. **The old http rule is pinned in more places than the spec names:** `relay/http/scheme_tests.rs`
   (2 tests), `relay/http/tests.rs` (`base_urls_are_parsed_not_prefix_matched`, the
   `normalize_base_url("http://relay.example.org")` refusal), `recovery.rs` (`http://evil.example`),
   `pairing/tests.rs` (line ~680), `engine/tests/join_new.rs` (~437), `engine/tests/join.rs` (~694),
   `engine/tests/setup.rs` (~524), and the ADR 0027 / 0025 text.
4. **Ten `impl Relay for` types** must gain the new trait method: `HttpRelay`, `FakeRelay`,
   `relay_e2e.rs` `Counting`, and the engine-test wrappers in `backoff.rs`, `pull.rs`, `push.rs`,
   `replace_safety.rs`, `join.rs`, `runtime_loop.rs`, `echo.rs`.
5. **`commands/sync.rs` tests pin the command set**: `assert_eq!(found.len(), 33)` and the list of
   password commands. Counts per task: Task 3 → 35, Task 8 → 36, Task 9 → 35.
6. **Platform checks are half done already.** `NSLocalNetworkUsageDescription` is in
   `Info.ios.plist`, localised in ten `InfoPlist.strings` and guarded by
   `src/mobile/permission-strings.test.ts`. App Transport Security governs `NSURLSession`, not
   reqwest's sockets, so it is verified by one device run (Task 5), not assumed. No Android
   project is committed (`src-tauri/gen/android` is absent): the cleartext-policy check is recorded
   as owed in the ADR.
7. **Encoding is shared.** The Recovery Kit QR and the pairing code already carry the server as
   base64url-without-padding of the UTF-8 address, the same as the connection code; the app
   re-normalises the decoded address with `normalize_base_url` (which also strips a trailing dot
   and punycodes), so codes from the server read as written.
8. **"Server went back" is `head < cursor`** (`engine/pull.rs` → `state::note_vault_went_back`).
   The change-address check compares `VaultInfo.head` with `SyncState.cursor` the same way.
9. **`syncServerNeedsUpdate`** exists only for pairing (`pairing/session.rs`); it is reworded in all
   locales to a general sentence, key kept (spec §9.3).
10. **Copy that is wrong on an own server:** `delete.afterwards` ("with a new sync account"),
    `setup.pay.selfHosted` / `selfHostedWaiting` (`ALLOWED_ACCOUNTS`), `setup.advanced.hint`.
11. **The only stored server address is `sync_state.relay_url`**; the running engine's
    `HttpRelay` and an in-memory pairing session hold copies. Changing the address is one `UPDATE`
    plus an engine restart (`guarded_change` already restarts it, and a spawn wakes a round).

## Review Focus

1. **A connection code mangled on its way** (wrapped lines, spaces, zero-width characters, a
   trailing full stop, `moony-sync2:`, a pairing or Kit code pasted in its place) reads correctly or
   is named precisely, never echoed. Pinned in Task 1.
2. **A blocked or deleted person** never sees a payment prompt or a deletion date: refused at setup
   before anything is stored, and on a running device shown as "ask whoever runs your sync server".
   Pinned in Task 3.
3. **An address that is not an own Moony sync server** (an old Worker relay's 404, an HTML page,
   wrong JSON, protocol 1, mode `cloud`) is refused with the right message before any account call,
   and nothing is stored. Pinned in Task 2.
4. **Plain http**: accepted for any host but the Moony cloud's, flagged `insecure` for every host but
   `localhost` / `127.0.0.1`, and Continue stays disabled until the warning is acknowledged in that
   flow. Pinned in Tasks 1 (Rust/TS rules) and 4 (`serverContinue`).
5. **Changing the address to a different server or an older copy of the same one** leaves the old
   address and writes nothing. Pinned in Task 7.

---

### Task 1: Connection code, server input and the plain-http rule

**Files:**
- Create `src-tauri/src/services/sync/connection_code.rs`; modify `services/sync/mod.rs` (module).
- Modify `src-tauri/src/services/sync/relay/http.rs`, `relay/http/scheme_tests.rs`, `relay/http/tests.rs`,
  `recovery.rs` (tests), `pairing/tests.rs`, `engine/tests/{join_new,join,setup}.rs` (pinned examples).
- Create `src/utils/sync-server.ts`, `src/utils/sync-server.test.ts`; modify `src/utils/pairing-code.ts`,
  `src/utils/pairing-code.test.ts`, `src/mobile/lib/sync-scan.ts` (reuse the shared decode).
- Locales: `common.json` × 10 (`validation.syncConnectionCodeInvalid`, `validation.syncConnectionCodeNewer`),
  `deviceSync.json` × 10 (`codes.pairNotForSetup`, `codes.kitNotForSetup`, `codes.connectionNotHere`,
  `codes.notAServerAddress`).

**Interfaces — Produces:**

```rust
// services/sync/connection_code.rs
pub const CONNECTION_CODE_PREFIX: &str = "moony-sync1:";
pub const MAX_CONNECTION_CODE_CHARS: usize = 512;
pub struct ConnectionCode { pub server_url: String, pub account: String } // Debug: origin only
pub fn parse_connection_code(text: &str) -> crate::error::Result<ConnectionCode>;
pub fn is_connection_code(text: &str) -> bool;              // `moony-sync<digits>:` prefix, any version
pub fn server_address_from_input(input: &str) -> crate::error::Result<String>; // address or code → normalized address
#[cfg(test)] pub fn encode_connection_code(server_url: &str, account: &str) -> String;
// relay/http.rs
pub const DEFAULT_RELAY_HOST: &str = "sync.moonyfinance.com";
fn scheme_allowed(scheme: &str, host: Option<&str>) -> bool;  // no `debug` parameter any more
pub fn is_insecure_address(normalized: &str) -> bool;         // http, host not localhost/127.0.0.1
pub fn display_host(url: &str) -> Option<String>;             // `host[:port]`, moved from sync_pairing.rs `server_host`
```

```ts
// src/utils/sync-server.ts
export const CONNECTION_CODE_PREFIX = 'moony-sync1:';
export type ConnectionCodeRead =
  | { ok: true; server: string; account: string }
  | { ok: false; problem: 'validation.syncConnectionCodeInvalid' | 'validation.syncConnectionCodeNewer' | 'validation.syncRelayUrlInvalid' };
export function parseConnectionCode(text: string): ConnectionCodeRead;
export function decodeServerField(encoded: string): string | null;      // base64url → UTF-8 address
export function payloadServer(text: string): string | null;             // server a pairing code / Kit QR / connection code names
export type CodeContext = 'setup' | 'pairing' | 'kit' | 'server';
export function wrongCodeKey(context: CodeContext, text: string): string | null; // a `deviceSync` key
// src/utils/pairing-code.ts
export type ScannedCodeKind = 'pair' | 'recover' | 'recoveryCode' | 'connection' | 'unknown';
```

Decisions:
- Reading a code: trim; remove every Unicode White_Space and U+200B/U+200C/U+200D/U+FEFF inside it
  (chat apps wrap long lines); drop one trailing `.` or `,` (outside the base64url alphabet, so
  safe) *(plan's call)*; then at most 512 characters. `moony-sync<n>:` with `n > 1` is
  `validation.syncConnectionCodeNewer` ("comes from a newer sync server; update Moony"), any other
  shape `validation.syncConnectionCodeInvalid`; exactly three `:`-separated fields after the prefix
  word; account 16 ASCII digits; server field non-empty base64url, valid UTF-8, then
  `normalize_base_url` (its own `validation.syncRelayUrlInvalid`) *(plan's call)*.
- `server_address_from_input`: a connection code yields its server (its account is ignored); anything
  else goes through `normalize_base_url`; blank is `validation.syncRelayUrlInvalid`.
- Plain http (spec §9.2): `http` is allowed for any non-empty host in every build, except the Moony
  cloud host `DEFAULT_RELAY_HOST` *(plan's call: "an address that is not the Moony cloud")*;
  `https` as before. `is_insecure_address` is the one rule for the warning; `[::1]` is not exempt
  (the spec names two hosts).
- `wrongCodeKey`: `setup` — pair → `codes.pairNotForSetup`, recover / recoveryCode →
  `codes.kitNotForSetup`; `pairing`, `kit` — connection → `codes.connectionNotHere`; `server` —
  pair / recover / recoveryCode → `codes.notAServerAddress`; else `null` (the backend answers).

- [ ] **Failing Rust tests** in `connection_code.rs`:
  - `a_connection_code_reads_its_server_and_account`:
    `parse_connection_code("moony-sync1:4417938205561273:aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn")` →
    `server_url == "https://sync.example.org"`, `account == "4417938205561273"`; `encode` round-trips.
  - `a_code_mangled_by_a_chat_app_still_reads`: the same code with `"\n"` after 20 characters, spaces
    every 8, a U+200B, leading/trailing spaces and a trailing `.` → the same two values.
  - `http_codes_and_uppercase_hosts_are_normalised`: `…:SFRUUDovL0hvc3QuRXhhbXBsZTo4MC8` →
    `"http://host.example"`; `…:aHR0cDovLzE5Mi4xNjguMS4yMDo4Nzg3` → `"http://192.168.1.20:8787"`.
  - `malformed_codes_are_refused_without_echoing_them`: 15 digits, a letter in the account, `!!` as
    server, empty server, a fourth field, 513 characters, `moony-sync0:` → `syncConnectionCodeInvalid`;
    `moony-sync2:…` → `syncConnectionCodeNewer`; server `ftp://x.example` or `https://x.example/p` →
    `syncRelayUrlInvalid`; no error's `Display`/`Debug` contains `4417938205561273`; `format!("{:?}",
    code)` contains no account digits.
  - `server_address_from_input_takes_an_address_or_a_code`: `" https://Sync.Example.org/ "` →
    `"https://sync.example.org"`; the code above → `"https://sync.example.org"`; `""` and
    `"moony-pair:2:x:y"` → `syncRelayUrlInvalid`.
- [ ] **Failing Rust tests** for the http rule (rewrite, do not delete, the pinned ones):
  - `scheme_tests.rs` → `plain_http_is_allowed_except_to_the_moony_cloud`:
    `scheme_allowed("http", Some("192.168.1.20"))`, `("http", Some("nas.local"))`,
    `("http", Some("localhost"))` true; `("http", Some("sync.moonyfinance.com"))`, `("https", Some(""))`,
    `("https", None)`, `("ftp", Some("localhost"))` false; `validate_base_url("http://sync.moonyfinance.com")`
    and `"http://sync.moonyfinance.com.:80/"` are `Err` in debug and release alike;
    `DEFAULT_RELAY_HOST == Url::parse(DEFAULT_RELAY_URL).host_str()`.
  - `an_http_address_is_insecure_unless_loopback`: `is_insecure_address("http://192.168.1.20:8787")`
    true, `("http://localhost:8787")` and `("http://127.0.0.1:8787")` false, `("https://x.example")` false.
  - `base_urls_are_parsed_not_prefix_matched`: `http://localhost.evil.com` and
    `http://127.0.0.1.evil.com` move to the accepted list; `http://localhost@evil.com` stays refused
    (user info); `normalize_base_url("http://relay.example.org")` → `Ok("http://relay.example.org")`.
  - In `recovery.rs`, `pairing/tests.rs`, `engine/tests/{join_new,join,setup}.rs`: every
    `http://evil.example` / `http://sync.example.org` / `http://relay.example.org` used as the refused
    example becomes `ftp://…` with the same assertion; `read_recovery_input` with
    `Some("http://192.168.1.20:8787")` now succeeds with that `relay_url`.
- [ ] **Failing TS tests** (`sync-server.test.ts`, `pairing-code.test.ts`): the same vectors for
  `parseConnectionCode` (ok, mangled, newer, invalid, bad server → `validation.syncRelayUrlInvalid`);
  `scannedCodeKind('moony-sync1:…') === 'connection'` and `('moony-sync2:abc') === 'connection'`;
  `payloadServer` of a pairing code with a fourth field, of a Kit QR with a server and of the code →
  the address, of a code without a server field → `null`; the `wrongCodeKey` table above, row by row.
- [ ] Run `cd src-tauri && cargo test --lib services::sync` and `npm test -- sync-server pairing-code`
  → fail (missing module, old rule).
- [ ] Implement. `kitQrSummary` uses `decodeServerField`; `sync_pairing.rs` imports `display_host`.
- [ ] Run both again → pass; `cargo clippy`, `npm run typecheck`. Commit
  `feat(sync): connection code, server input and plain http for own servers`.

### Task 2: Server check — `Relay::server_info`, fake, version and mode

**Files:**
- Modify `services/sync/relay/mod.rs`, `relay/http.rs`, `relay/http/tests.rs`, `relay/fake.rs`,
  `relay/fake/tests.rs`, the eight wrapper impls of Finding 4, `services/sync/error.rs`.
- Create `services/sync/server.rs` (+ `mod.rs`), its tests in `engine/tests/server.rs` (+ `engine/tests/mod.rs`).
- Modify `src-tauri/src/models/sync.rs` (`SyncServerCheck`), `bindings.rs`.
- Locales `common.json` × 10: `validation.syncServerNotFound`, `validation.syncServerNotOwn`;
  reword `validation.syncServerNeedsUpdate`.

**Interfaces — Consumes:** `is_insecure_address`, `display_host`, `normalize_base_url` (Task 1).
**Produces:**

```rust
// relay/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ServerMode { SelfHosted, Cloud, Other }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct ServerInfo { pub version: String, pub protocol: u32, pub mode: ServerMode }
// trait Relay: fn server_info(&self) -> impl Future<Output = RelayResult<ServerInfo>> + Send;  (no default body)
// RelayError::RegistrationClosed   ← 403 registration_closed
// (403, "managed_by_operator") → RelayError::BadRequest("managed_by_operator")
// relay/fake.rs
pub fn set_server_info(&self, info: Option<ServerInfo>); // None = an old relay: BadRequest("not_found")
// Route::ServerInfo
// server.rs
pub const MIN_PROTOCOL: u32 = 2;
pub async fn check_own_server<R: Relay>(relay: &R, address: &str) -> SyncResult<SyncServerCheck>;
// models/sync.rs
pub struct SyncServerCheck { address: String, host: String, version: String, insecure: bool } // serde: address, host, version, insecure
```

Decisions:
- `HttpRelay::server_info`: `GET v1/server`, no account header, the shared 15 s bound, `200` only,
  body capped at `MAX_JSON_BODY`; `name` must be `"moony-sync"` (else `Protocol("not a sync
  server")`); `version` 1–32 characters of `[0-9A-Za-z.+-]` (else `Protocol("version")`); `mode`
  `"self_hosted"` / `"cloud"` / anything else → `Other`.
- `check_own_server` *(plan's call on the mapping)*: `RateLimited` → `SyncError::RateLimited`; any
  other error (network, protocol, 404, 5xx) → `validation.syncServerNotFound` ("No Moony sync
  server answers at this address."); `protocol < MIN_PROTOCOL` → `validation.syncServerNeedsUpdate`
  ("This sync server needs an update before Moony can use it."); `mode != SelfHosted` →
  `validation.syncServerNotOwn` ("This address is not an own sync server. Use the connection code
  from whoever runs your sync server."). `address` is re-normalised; `host = display_host`;
  `insecure = is_insecure_address`.
- `FakeRelay` defaults to `ServerInfo { version: "1.0.0", protocol: 2, mode: SelfHosted }` and gains
  the people knobs Task 3 needs: `add_person() -> String` (active, `paid_until` 253402300799),
  `block(account, at)`, `unblock(account)`, `remove_account(account)` (its vaults go too),
  `set_registration_closed(bool)` (`create_account` → `RegistrationClosed`).
- Wrappers delegate `server_info` to their inner relay (one line each).

- [ ] **Failing tests:**
  - `relay/mod.rs` table: `(403, "registration_closed", E::RegistrationClosed)`,
    `(403, "managed_by_operator", bad("managed_by_operator"))`; `RegistrationClosed` falls into
    `SyncError::Refused` and reads `validation.syncRelayError` (`error.rs` table).
  - `relay/http/tests.rs` (local test server as the existing tests use):
    `server_info_reads_the_documented_answer` — `{"name":"moony-sync","version":"1.0.0","protocol":2,"mode":"self_hosted"}`
    → `ServerInfo { version: "1.0.0", protocol: 2, mode: SelfHosted }` and the request carried no
    `moony-account` header; `server_info_refuses_what_is_not_a_moony_server` — `404 {"error":"not_found"}`
    → `BadRequest("not_found")`, `200 <html>` → `Protocol("not json")`, `name:"other"` →
    `Protocol(..)`, a 40-character version → `Protocol(..)`, `mode:"weird"` → `Other`.
  - `engine/tests/server.rs`:
    `an_own_server_passes_the_check` — `check_own_server(&fake, "https://Relay.Example.org/")` →
    `SyncServerCheck { address: "https://relay.example.org", host: "relay.example.org", version: "1.0.0", insecure: false }`;
    with `"http://192.168.1.20:8787"` → `insecure: true`, `host: "192.168.1.20:8787"`.
    `what_is_not_an_own_moony_server_is_refused_before_any_account_call` — `set_server_info(None)`
    → key `validation.syncServerNotFound`; protocol 1 → `validation.syncServerNeedsUpdate`; mode
    `Cloud` and `Other` → `validation.syncServerNotOwn`; `fail_next(ServerInfo, Network("connect"))`
    → `syncServerNotFound`; `fail_next(ServerInfo, RateLimited)` → `validation.syncRateLimited`; in
    every case `fake.calls() == [Route::ServerInfo]`.
- [ ] Run `cargo test --lib services::sync` → fails to compile (no method) / fails.
- [ ] Implement; register `SyncServerCheck` in `bindings.rs`.
- [ ] `cargo test --lib services::sync`, `cargo clippy` pass. Commit
  `feat(sync): server check against GET /v1/server`.

### Task 3: Setup adopts the account; status knows an own server

**Files:**
- Modify `services/sync/setup.rs`, `engine/status.rs` (+ its tests), `services/sync/error.rs`,
  `engine/tests/setup.rs`, `src-tauri/src/models/sync.rs` (`SyncStatus`), `commands/sync.rs`,
  `src-tauri/src/lib.rs`, `bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`,
  `src/lib/tauri-api.ts`, `src/hooks/use-device-sync.ts`, `src/hooks/use-device-sync-mutations.ts`.
- Locales `common.json` × 10: `validation.syncAccountBlocked`, `validation.syncConnectionCodeRevoked`.

**Interfaces — Consumes:** `ConnectionCode`, `parse_connection_code` (Task 1); `check_own_server`,
`SyncServerCheck`, FakeRelay people knobs (Task 2).
**Produces:**

```rust
// setup.rs
pub async fn connect_own_server<R: Relay, D: ConnAccess>(relay: &R, db: &D, code: &ConnectionCode) -> SyncResult<SyncAccount>;
// error.rs
impl SyncError { pub fn to_own_server_app_error(&self) -> AppError; } // Lapsed → syncAccountBlocked, Refused(AccountNotFound) → syncConnectionCodeRevoked, else to_app_error()
// models/sync.rs — SyncStatus gains
#[serde(rename = "ownServer")] pub own_server: bool,
#[serde(rename = "serverAddress")] pub server_address: Option<String>,
#[serde(rename = "serverInsecure")] pub server_insecure: bool,
// commands/sync.rs
#[tauri::command] pub async fn sync_check_server(input: String) -> Result<SyncServerCheck>;
#[tauri::command] pub async fn sync_connect_own_server(db: State<'_, Database>, connection_code: String) -> Result<SyncAccount>;
```

```ts
// shared/schema.ts
export interface SyncServerCheck { address: string; host: string; version: string; insecure: boolean }
// SyncStatus gains: ownServer: boolean; serverAddress: string | null; serverInsecure: boolean;
// src/lib/tauri-api.ts, deviceSyncApi
checkServer: (input: string) => tauriInvoke<SyncServerCheck>('sync_check_server', { input }),
connectOwnServer: (connectionCode: string) => tauriInvoke<SyncAccount>('sync_connect_own_server', { connectionCode }),
// src/hooks/use-device-sync.ts
export function useSyncServerCheck(input: string, enabled: boolean): UseQueryResult<SyncServerCheck, Error>;
// src/hooks/use-device-sync-mutations.ts → connectOwnServer (onSuccess: refreshSyncState)
```

Decisions:
- `connect_own_server` order: read stored account and `enabled`; with sync on and a different
  `(url, account)` → `validation.syncAlreadyEnabled`; `check_own_server` (the service does not trust
  that the UI checked) *(plan's call)*; `account_status`: `AccountNotFound` →
  `validation.syncConnectionCodeRevoked`, `active == false` → `validation.syncAccountBlocked`; only
  then `keys::save_account(url, account)` (skipped when equal). Nothing is stored on any refusal.
  Replacing a stored account while sync is off keeps the vault secrets, as `create_account` does
  (the vault is created only at `finish_setup`); the UI drops a kit it showed.
- `sync_finish_setup` maps its error with `to_own_server_app_error` when the stored address is not
  the default relay (`is_default_relay`), so a person blocked between the steps never reads
  "subscription" copy *(plan's call)*.
- `status`: `server_address` = the stored address in every phase that has one (`settingUp` too);
  `own_server = !is_default_relay`; `server_insecure = is_insecure_address`; for an own server
  `deletes_at` is always `None` (blocking never deletes, spec §5.4).
- `useSyncServerCheck`: query key `['device-sync-server-check', input.trim()]`, input debounced 400 ms
  with `useDebouncedValue`, `retry: false`, `staleTime: 0`, `gcTime: 0` (the input may be a code).
- Commands start with `require_feature()?`; no argument is logged. The cloud commands
  (`sync_create_account`, `sync_start_checkout`) stay.

- [ ] **Failing tests** (`engine/tests/setup.rs`, with `encode_connection_code`):
  - `an_own_server_code_adopts_the_account_and_skips_payment` — `acct = fake.add_person()`; connect →
    `SyncAccount { account_number: acct, active: true, paid_until: Some(253402300799) }`;
    `keys::account == Some(("https://relay.example.org", acct))`; `fake.calls() == [ServerInfo,
    AccountStatus]` (no `CreateAccount`, no `Checkout`); `recovery_kit` then `finish` → enabled, and
    the kit's `relay_url == Some("https://relay.example.org")`.
  - `a_blocked_or_deleted_person_is_refused_before_anything_is_stored` — blocked → key
    `validation.syncAccountBlocked`; unknown account → `validation.syncConnectionCodeRevoked`; both
    leave `keys::account == None`.
  - `a_server_that_fails_the_check_stores_nothing` — `set_server_info(None)` →
    `validation.syncServerNotFound`, `keys::account == None`, no `AccountStatus` call.
  - `the_same_code_resumes_and_another_replaces_only_while_off` — same code twice → same account, one
    stored row; a second person's code while off → replaced; after `finish`, a different code →
    `validation.syncAlreadyEnabled`.
  - `a_person_blocked_before_finish_reads_the_own_server_key` — connect, `block`, `finish` →
    `Err(SyncError::Lapsed)` and `to_own_server_app_error() == Validation("validation.syncAccountBlocked")`.
  - `error.rs`: `own_server_errors_never_ask_for_payment` — `Lapsed` → `syncAccountBlocked`,
    `Refused(AccountNotFound)` → `syncConnectionCodeRevoked`, `Transient(Offline)` → `syncRelayUnreachable`.
  - `engine/status.rs` tests: `an_own_server_lapse_has_no_deletion_date` — stored
    `"https://relay.example.org"`, runtime problem `Lapsed`, `paid_until` 1_800_000_000 →
    `own_server: true`, `deletes_at: None`, `server_address: Some(..)`, `server_insecure: false`;
    `"http://192.168.1.20:8787"` → `server_insecure: true`; `DEFAULT_RELAY_URL` → `own_server: false`,
    `deletes_at == Some(1_800_000_000 + DELETE_AFTER_LAPSE)`; sync off without account → all three empty.
  - `commands/sync.rs`: the command count becomes `35`.
- [ ] Run `cargo test --lib services::sync commands::sync` → fail.
- [ ] Implement; schema.ts per the 7-step procedure; `cargo test generate_bindings -- --ignored`.
- [ ] `cargo test --lib`, `npm run typecheck`, `npm test` pass. Commit
  `feat(sync): set up on an own server from a connection code`.

### Task 4: Desktop setup — server step, three steps, own-server wording

**Files:**
- Create `src/utils/sync-setup.ts`, `src/utils/sync-setup.test.ts`,
  `src/components/settings/sync/ServerChoice.tsx`, `src/hooks/use-server-choice.ts`.
- Modify `SyncSetupDialog.tsx`, `SyncProblemAlert.tsx`, `SyncStatusCard.tsx`, `SyncDangerCard.tsx`,
  `SyncIntroCard.tsx` (resume text only if needed), `src/utils/device-sync-problem.ts` (parameter
  name), `src/utils/sync-status.ts` + test (remove `isSelfHostedPaidUntil`).
- Locales `deviceSync.json` × 10: `setup.server.*` (below), `alert.lapsed.titleOwnServer`,
  `delete.afterwardsOwnServer`, `setup.pay.ownServer` (replaces the two `ALLOWED_ACCOUNTS` keys).

**Interfaces — Consumes:** `parseConnectionCode`, `wrongCodeKey` (Task 1); `SyncServerCheck`,
`useSyncServerCheck`, `connectOwnServer`, `SyncStatus.ownServer/serverAddress` (Task 3).
**Produces:**

```ts
// src/utils/sync-setup.ts
export type ServerKind = 'cloud' | 'own';
export type SetupStep = 'server' | 'account' | 'pay' | 'kit' | 'device';
export function setupSteps(kind: ServerKind): readonly SetupStep[];
export function serverContinue(check: SyncServerCheck | undefined, acknowledged: boolean): boolean;
// src/hooks/use-server-choice.ts
export interface ServerChoiceState {
  input: string; setInput(v: string): void; accepts: 'connectionCode' | 'addressOrCode';
  check: SyncServerCheck | undefined; checking: boolean; problem: string | null; // translated
  acknowledged: boolean; setAcknowledged(v: boolean): void; ready: boolean;
}
export function useServerChoice(accepts: 'connectionCode' | 'addressOrCode', context: CodeContext): ServerChoiceState;
// ServerChoice.tsx
export function ServerChoice(props: { state: ServerChoiceState; id: string; showCloud: boolean }): JSX.Element;
```

Decisions:
- Step 1 is the wireframe of spec §9.1: a radio group "Moony cloud" (disabled, `Připravujeme`
  badge, neutral) / "Own server" (selected); under it the field, a status line ("Server found ·
  version 1.0.0" or the problem), and for `insecure` a warning `Alert` with a checkbox
  "I understand, continue anyway". Changing the input resets the checkbox; nothing is remembered
  *(plan's call: the explicit continue is that checkbox)*.
- `ready` = the input passes `parseConnectionCode` (or, for `addressOrCode`, is non-empty) and
  `wrongCodeKey(context, input) === null` and `serverContinue(check, acknowledged)`. A pasted
  pairing or Kit code shows its `codes.*` message instead of a check.
- Steps for an own server: server → kit → device (`setupSteps('own')`, "Step 1 of 3"). Continue on
  step 1 calls `connectOwnServer(input)`; a different account than one already shown resets the kit
  state as `handleAccount` does today. The account number is not shown as a step.
- Resume *(plan's call)*: when the dialog opens in `settingUp` with `status.serverAddress` set, step 1
  shows "Setup started with {{host}}"; Continue calls `getAccount` and moves on when the account is
  active (an inactive one shows `validation.syncAccountBlocked`); "Use another connection code"
  reveals the field.
- The cloud branch (`account`, `pay`, `createAccount`, `startCheckout`) stays in the component for
  `kind === 'cloud'`, which the disabled radio never sets; its self-hosted answer shows
  `setup.pay.ownServer`; the Advanced address field and the grant button are removed.
- Lapsed on an own server: `SyncProblemAlert` uses `status.ownServer` (title
  `alert.lapsed.titleOwnServer` "Sync paused", body `alert.lapsed.selfHosted`, no Renew, no date);
  `SyncStatusCard` hides the subscription row when `ownServer`; `SyncDangerCard` uses
  `delete.afterwardsOwnServer` ("Afterwards you can set sync up again with the same connection code
  and a new Recovery Kit.").
- Copy (en / cs): `setup.server.title` "Where should your data sync?" / "Kde se mají data
  synchronizovat?"; `setup.server.encrypted` "End-to-end encrypted either way" / "V obou případech
  koncově šifrovaná"; `setup.server.cloud` "Moony cloud" (brand, untranslated); `setup.server.soon`
  "Coming soon" / "Připravujeme"; `setup.server.cloudHint` "Nothing to install, we run it." / "Nic
  neinstalujete, o provoz se staráme my."; `setup.server.own` "Own server" / "Vlastní server";
  `setup.server.codeLabel` "Connection code" / "Připojovací kód"; `setup.server.codeHint` "You get
  it from whoever runs your sync server." / "Dostanete ho od toho, kdo provozuje váš synchronizační
  server."; `setup.server.found` "Server found · version {{version}}" / "Server nalezen · verze
  {{version}}"; `setup.server.checking`; `setup.server.insecureTitle` "The connection is not
  encrypted (http)" / "Spojení není šifrované (http)"; `setup.server.insecureBody` "Your data stays
  encrypted, but anyone on the same network can capture the access to sync and delete or block the
  synced data on the server." / "Data zůstávají šifrovaná, ale kdokoli ve stejné síti může zachytit
  přístup k synchronizaci a synchronizovaná data na serveru smazat nebo zablokovat.";
  `setup.server.insecureAck` "I understand, continue anyway" / "Rozumím, přesto pokračovat";
  `setup.server.resume` "Setup started with {{host}}" / "Nastavení začalo se serverem {{host}}";
  `setup.server.other` "Use another connection code" / "Použít jiný připojovací kód".

- [ ] **Failing tests** (`sync-setup.test.ts`): `setupSteps('own')` equals `['server','kit','device']`;
  `setupSteps('cloud')` equals `['server','account','pay','kit','device']`;
  `serverContinue(undefined, true) === false`; insecure + `false` → `false`; insecure + `true` → `true`;
  secure + `false` → `true`. `sync-status.test.ts`: the `isSelfHostedPaidUntil` block is deleted.
- [ ] Run `npm test -- sync-setup sync-status` → fail.
- [ ] Implement; add the keys to all ten locales.
- [ ] `npm run lint`, `npm run typecheck`, `npm test` pass. Verify by eye in a worktree dev run (a
  port other than 1420, `MOONY_DEV_SYNC=1`) against a Plan 1 server started with
  `SETUP_CODE=…`: add a person in the admin, paste the code, see "Server found", an
  `http://<LAN IP>:8787` code shows the warning and blocks Continue until ticked, finish setup in
  three steps; block the person in the admin → "Sync paused" with no Renew and no date; light and
  dark. Commit `feat(sync): desktop setup on an own server`.

### Task 5: Phone setup, the setup camera and scanner kinds

**Files:**
- Create `src/mobile/components/sync/ServerChoiceBlock.tsx`.
- Modify `src/mobile/components/sync/SetupSheet.tsx`, `ScanFlow.tsx`, `RecoveryJoinSheet.tsx`,
  `SyncStatusBlock.tsx`, `src/mobile/lib/sync-scan.ts`, `src/mobile/lib/sync-scan.test.ts`.
- Locales `deviceSync.json` × 10: `setup.server.scan` ("Scan code" / "Naskenovat kód"),
  `phone.notAConnectionCode`.

**Interfaces — Consumes:** `useServerChoice`, `setupSteps`, `serverContinue` (Task 4);
`wrongCodeKey`, `ScannedCodeKind 'connection'` (Task 1); `scanQrCode` (`src/mobile/lib/qr-scanner`).
**Produces:**

```ts
// src/mobile/lib/sync-scan.ts
export type ScanRoute = 'pair' | 'kitJoin' | 'alreadyOn' | 'connection' | 'unknown';
export type SetupScanRoute = 'connection' | 'pair' | 'recover' | 'unknown';
export function setupScanRoute(text: string): SetupScanRoute;
// kitCodeProblem / pairingCodeProblem answer `codes.connectionNotHere` for a connection code
// ServerChoiceBlock.tsx
export function ServerChoiceBlock(props: { state: ServerChoiceState; showCloud: boolean; onScan?: () => void }): JSX.Element;
```

Decisions:
- `SetupSheet` mirrors the desktop: server → kit → device, the "Scan code" button opens the camera
  (`scanQrCode`); `setupScanRoute` → `connection` fills the field and runs the check; `pair` →
  toast `codes.pairNotForSetup`, `recover` → `codes.kitNotForSetup`, `unknown` →
  `phone.notAConnectionCode`; the pay step's self-hosted branch and grant button go.
- `ScanFlow` (join / add scanner): `scanRoute` → `connection` shows the `notMoony` sheet with
  `codes.connectionNotHere` instead of `phone.unknownCode` *(plan's call: reuse the sheet, change
  its text)*. `RecoveryJoinSheet` refuses a connection code with the same key.
- `SyncStatusBlock` uses `status.ownServer` instead of `isSelfHostedPaidUntil` (row and action).
- Device check owed here (Finding 6): an iPhone build against `http://<LAN IP>:8787` shows the
  Local Network prompt once and syncs; record the result in the task report.

- [ ] **Failing tests** (`sync-scan.test.ts`): `scanRoute(code, 'join') === 'connection'` and
  `scanRoute(code, 'add') === 'connection'`; `setupScanRoute(code) === 'connection'`,
  `setupScanRoute('moony-pair:2:…') === 'pair'`, `setupScanRoute('moony-recover:1:…') === 'recover'`,
  `setupScanRoute('https://x') === 'unknown'`; `kitCodeProblem(code) === 'codes.connectionNotHere'`,
  `pairingCodeProblem(code) === 'codes.connectionNotHere'`.
- [ ] Run `npm test -- sync-scan` → fail. Implement. Run → pass; `npm run lint`, `typecheck`.
- [ ] Verify by eye on the iOS simulator (dev build with `MOONY_DEV_SYNC=1`): setup with a pasted
  code, the camera refusing a pairing QR by name, dark mode and the largest Dynamic Type size.
  Commit `feat(sync): phone setup on an own server`.

### Task 6: Join dialogs — server field, server check, warning before credentials travel

**Files:**
- Modify `src-tauri/src/commands/sync_pairing.rs` (`shower_relay`, `view_of`), `commands/sync_pairing_tests.rs`,
  `services/sync/recovery.rs` (`read_recovery_input`), `src-tauri/src/models/sync.rs` (`PairingView`),
  `shared/schema.ts`, `shared/generated-types.ts`.
- Modify `src/components/settings/sync/JoinFields.tsx`, `JoinSyncDialog.tsx`, `RecoveryJoinDialog.tsx`,
  `PairingSteps.tsx`; `src/mobile/components/sync/RecoveryJoinSheet.tsx`, `ScanFlow.tsx`, `PairingSheet.tsx`.
- Locales `deviceSync.json` × 10: `join.serverHint` (address or code), `join.insecureAtJoin`.

**Interfaces — Consumes:** `server_address_from_input`, `is_insecure_address`, `display_host`
(Task 1); `ServerChoice`, `ServerChoiceBlock`, `useServerChoice` (Tasks 4–5).
**Produces:**

```rust
// models/sync.rs — PairingView gains
#[serde(rename = "joinServer")] pub join_server: Option<String>,        // host[:port] of the received credentials' server (joiner, stage received)
#[serde(rename = "joinServerInsecure")] pub join_server_insecure: bool,
// recovery.rs — read_recovery_input's `relay_url` takes an address or a connection code
```

```ts
// PairingView gains: joinServer: string | null; joinServerInsecure: boolean;
```

Decisions:
- `shower_relay` (joiner's typed value) and `read_recovery_input` (typed Kit's server) go through
  `server_address_from_input`, so a connection code works and only its address is used.
- `JoinFields`' Advanced collapsible becomes `ServerChoice` with `showCloud` (the two-way choice,
  spec §9.1a) and `accepts: 'addressOrCode'`. In `JoinSyncDialog` the field is required (the joiner's
  mailbox is on the own server); in `RecoveryJoinDialog` / `RecoveryJoinSheet` it appears only for a
  typed recovery code (`scannedCodeKind === 'recoveryCode'`), since a Kit QR's text brings its server;
  then the check runs on `kitQrSummary(...).server` *(plan's call)*. The check result's `address` is
  what the dialog sends.
- Where the warning is asked *(plan's call)*: the pairing mailbox traffic is sealed end to end and
  carries no account or token (ADR 0026), so for pairing the warning is asked where credentials
  first travel: at stage `received`, before "Join and replace", when `joinServerInsecure` and
  `joinServer` differs from a host the same dialog already acknowledged (a desktop joiner that typed
  an http address acknowledges it inline). Kit joins and setup ask inline before their first call
  (those send the account number). Holder flows ask nothing: the holder already syncs with its server.
- Phone `ScanFlow`: a scanned Kit QR naming a non-default server runs `checkServer(server)` on the
  confirm sheet (status line + warning + checkbox gate "Join"); a scanned pairing code needs nothing
  before the scan; `PairingSheet` at `received` shows `join.insecureAtJoin` with the checkbox.
- Server-check failures show inline under the field (`problem`), never a toast.

- [ ] **Failing tests:**
  - `recovery.rs`: `a_typed_kit_takes_a_connection_code_as_its_server` — `read_recovery_input(&grouped,
    Some(ACCOUNT), Some(code("http://192.168.1.20:8787", "1111222233334444")), DEFAULT)` →
    `relay_url == "http://192.168.1.20:8787"`, `account == ACCOUNT` (the code's account is ignored).
  - `sync_pairing_tests.rs`: `a_joiner_may_type_a_connection_code` — `shower_relay(&conn, Role::Joiner,
    Some(code("https://sync.example.com", …)))` → `"https://sync.example.com"`;
    `the_view_names_an_insecure_join_server` — a joiner session at `Received` whose credentials name
    `http://192.168.1.20:8787` → `join_server == Some("192.168.1.20:8787")`, `join_server_insecure ==
    true`; the same with `https://sync.example.com` → `false`; a holder's view → `None` / `false`.
- [ ] Run `cargo test --lib commands::sync_pairing services::sync::recovery` → fail.
- [ ] Implement (schema.ts per the 7-step procedure, regenerate bindings); locales × 10.
- [ ] `cargo test --lib`, `npm run lint`, `typecheck`, `npm test` pass. Verify by eye: desktop joins a
  phone with an http server (warning once, inline), Kit join with a typed code and a connection code
  in the server field. Commit `feat(sync): join dialogs take an own server address or code`.

### Task 7: Settings shows the server; change the server address (service)

**Files:**
- Modify `services/sync/server.rs`, `services/sync/keys.rs`, `relay/fake.rs`; create
  `engine/tests/server_address.rs` (+ `engine/tests/mod.rs`).
- Locales `common.json` × 10: `validation.syncServerAddressOtherServer`,
  `validation.syncServerAddressWentBack`.

**Interfaces — Consumes:** `check_own_server` (Task 2), `normalize_base_url` (Task 1).
**Produces:**

```rust
// keys.rs
pub fn save_relay_url(conn: &Connection, relay_url: &str) -> Result<()>; // one UPDATE of sync_state.relay_url
// server.rs
pub async fn change_server_address<R: Relay, D: ConnAccess>(relay: &R, db: &D, new_address: &str) -> SyncResult<String>; // the stored, normalized address
// relay/fake.rs (test knob)
pub fn roll_back_head(&self, vault_id: &str, head: u64);
```

Decisions:
- Under `engine::exclusive(db)`: credentials and `enabled` (sync off or no credentials →
  `validation.syncSetupIncomplete`); the same address as stored → `Ok` without a call or a write
  *(plan's call)*; `check_own_server`; `account_status(stored account)` — `AccountNotFound` →
  `validation.syncServerAddressOtherServer` (a blocked person may still move: no `active` check);
  `vault_info(stored auth)` — `VaultNotFound` / `Unauthorized` / `TokenReadOnly` →
  `syncServerAddressOtherServer`, other errors as they are; `info.head < state.cursor` →
  `validation.syncServerAddressWentBack`; then `save_relay_url` in one write. Any failure leaves the
  old address (spec §9.3a).
- `syncServerAddressWentBack` (en): "The sync server at this address has older synced data than this
  device has already seen, so the address was not changed." `OtherServer`: "The sync server at this
  address does not have this device's synced data, so the address was not changed."

- [ ] **Failing tests** (`engine/tests/server_address.rs`, with the `set_up` helper of `setup.rs`
  moved to `engine/tests/mod.rs` if needed):
  - `a_new_address_of_the_same_server_is_stored_in_one_write` — set up on `RELAY`, push one change;
    `change_server_address(&relay.clone(), &device, "http://192.168.1.20:8787")` →
    `Ok("http://192.168.1.20:8787")`; `keys::account` names the new address and the same account;
    vault ID, keys, device ID, cursor unchanged; the calls since are `[ServerInfo, AccountStatus,
    VaultInfo]`; `recovery_kit(..).relay_url == Some("http://192.168.1.20:8787")`.
  - `another_server_leaves_the_old_address` — a fresh `FakeRelay::new()` → key
    `validation.syncServerAddressOtherServer`; a fake that knows the account but not the vault → the
    same key; the stored address is still `RELAY`.
  - `an_older_copy_of_the_server_is_refused` — after a round (cursor `N > 0`),
    `roll_back_head(vault, N - 1)` → `validation.syncServerAddressWentBack`; address unchanged.
  - `a_server_that_fails_the_check_or_sync_off_changes_nothing` — `set_server_info(None)` →
    `syncServerNotFound`; a profile with sync off → `syncSetupIncomplete`; same address → `Ok`, no call.
- [ ] Run `cargo test --lib services::sync` → fail. Implement. Run → pass; `cargo clippy`. Commit
  `feat(sync): change the sync server address after proving it is the same server`.

### Task 8: Change server address — command and UI, standing http note

**Files:**
- Modify `commands/sync.rs` (+ tests), `src-tauri/src/lib.rs`, `src/lib/tauri-api.ts`,
  `src/hooks/use-device-sync-mutations.ts`, `SyncStatusCard.tsx`, `src/mobile/components/sync/SyncStatusBlock.tsx`.
- Create `src/components/settings/sync/ChangeServerDialog.tsx`, `src/mobile/components/sync/ChangeServerSheet.tsx`.
- Locales `deviceSync.json` × 10: `server.*` (row label, insecure note, change button, dialog title,
  body, success).

**Interfaces — Consumes:** `change_server_address` (Task 7), `server_address_from_input` (Task 1),
`useServerChoice` (Task 4), `SyncStatus.serverAddress/serverInsecure` (Task 3).
**Produces:**

```rust
#[tauri::command]
pub async fn sync_change_server_address(app: AppHandle, db: State<'_, Database>, password: String, address: String) -> Result<SyncStatus>;
```

```ts
changeServerAddress: (password: string, address: string) =>
  tauriInvoke<SyncStatus>('sync_change_server_address', { password, address }),
// mutation changeServerAddress (variables {password, address}; onSuccess: refreshSyncState; reset on unmount)
```

Decisions:
- Command order: `Zeroizing` password; `require_feature()?`; `password_given`; `verify_password`;
  `server_address_from_input(&address)?`; `HttpRelay::new`; `forget_kit_window()`;
  `sync_pairing::forget_pairing()`; `guarded_change(…, server::change_server_address(…))` (stops the
  engine, restarts it on the stored address; the spawn's wake runs the round); `current_status`.
- Settings row (desktop and phone): "Sync server" with `host` of `serverAddress`, a "Change address"
  button, and for `serverInsecure` a standing note "Unencrypted connection (http)" (spec §9.2).
- Dialog: `ServerChoice` without the cloud choice (`showCloud: false`, `accepts: 'addressOrCode'`),
  the body says each device changes its own address and to save the Recovery Kit again; Continue
  (gated by `ready`) opens `SyncPasswordDialog`; on success a toast and the dialog closes.

- [ ] **Failing tests** (`commands/sync.rs`): command count `36`;
  `every_password_command_refuses_an_empty_password_then_checks_it` lists
  `sync_change_server_address` (password check before `guarded_change(`).
- [ ] Run `cargo test --lib commands::sync` → fail. Implement; locales × 10.
- [ ] `cargo test --lib`, `npm run lint`, `typecheck`, `npm test` pass. Verify by eye: change
  `http://127.0.0.1:8787` to `http://localhost:8787` against a Plan 1 server (same server, new
  address): status syncs, the Kit shows the new address; point it at a second fresh server →
  refused, old address kept. Commit `feat(sync): change the sync server address in Settings`.

### Task 9: Remove the dev grant; e2e suites against the admin path

**Files:**
- Modify `relay/http.rs` (remove `dev_grant`, `DevGrantWire`, `DevGrantAnswerWire`),
  `commands/sync.rs` (remove `sync_dev_grant`, both `dev_grant` fns, `DEV_ONLY` stays for the
  capture measurement), `src-tauri/src/lib.rs`, `src/lib/tauri-api.ts` (`devGrant`),
  `use-device-sync-mutations.ts` (`devGrant`), any remaining grant UI.
- Create `src-tauri/src/services/sync/relay_e2e_admin.rs` (`#[cfg(test)]`, + `mod.rs`).
- Rewrite `relay_e2e.rs` (header, `first_device`, `fresh_account`, the two account tests),
  adjust `relay_e2e_pairing.rs` imports.
- Locales × 10: remove `setup.pay.grantTest`.

**Interfaces — Consumes:** Plan 1 admin API (`GET state`, `POST setup`, `POST login`,
`GET/PUT settings`, `POST people`, `PATCH people/:id`, `DELETE people/:id`); `connect_own_server`,
`check_own_server`, `change_server_address`, `parse_connection_code`.
**Produces:**

```rust
// relay_e2e_admin.rs
pub(super) const SETUP_CODE_VAR: &str = "MOONY_RELAY_E2E_SETUP_CODE"; // default "moony-e2e-setup-code"
pub(super) struct Admin { base: String, cookie: String }
pub(super) async fn admin(base: &str) -> Admin;  // setup or login, once per process
impl Admin {
    pub(super) async fn add_person(&self, name: &str) -> Person;   // Person { id: String, code: ConnectionCode }
    pub(super) async fn block(&self, id: &str);
    pub(super) async fn unblock(&self, id: &str);
    pub(super) async fn delete(&self, id: &str);
}
```

Decisions:
- The admin client is a plain `reqwest::Client` with redirects off; it reads the `moony_admin=…`
  pair from `Set-Cookie` and sends it as `Cookie`, and sends `Origin: <base>` on every non-GET (the
  server checks it). `admin()` asks `GET state`: `setupNeeded` → `POST setup {code, name:
  "e2e-admin", password: "moony-e2e-admin-password"}`, else `POST login` with the same; then, when
  `GET settings` has no `publicUrl`, `PUT settings {publicUrl: base}`. The cookie is cached in a
  `OnceLock<String>` so one `cargo test` run logs in once (login is limited to 5 per 15 minutes)
  *(plan's call)*.
- `first_device`: `admin.add_person(name)` → `connect_own_server` → `recovery_kit` → `finish_setup`.
  `fresh_account` and its rate-limit wait go.
- `an_unpaid_account_cannot_create_a_vault` → `a_blocked_person_cannot_create_a_vault_until_unblocked`
  (connect, `block`, finish → `Err(Lapsed)` with own-server key `validation.syncAccountBlocked`;
  `unblock`, finish → enabled; `delete_synced_data`).
- `a_self_hosted_relay_serves_only_allowed_accounts` →
  `registration_is_closed_and_a_deleted_persons_code_is_refused`: `server_info` → protocol 2,
  `SelfHosted`, non-empty version; `create_account()` → `RegistrationClosed`; a person with one
  synced device is deleted → the device's next round has problem `VaultGone`, and connecting a new
  profile with that code → `validation.syncConnectionCodeRevoked`.
- New `an_address_change_goes_through_a_real_server`: set up at `http://127.0.0.1:<port>`, change to
  `http://localhost:<port>` → stored, a round succeeds; change to an address nobody answers →
  `syncServerNotFound`, old address kept.
- `NEEDS_SELF_HOSTED`, `MOONY_RELAY_E2E_SELF_HOSTED`, `MOONY_RELAY_E2E_ALLOWED_ACCOUNT` and every
  `ALLOWED_ACCOUNTS` mention go. The header documents the run: in `Moony-relay-node`,
  `SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start` (or the image with
  `-e SETUP_CODE=… -p 8787:8787`), then
  `MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e -- --ignored --test-threads=1`.
- `#[ignore]` reasons say "needs a Moony sync server (Plan 1) with SETUP_CODE".

- [ ] **Failing tests:** `commands/sync.rs` count `35` and `sync_dev_grant` absent from `lib.rs`;
  `cargo check --tests` fails until `relay_e2e*.rs` compile without `dev_grant` / `create_account`.
- [ ] Implement; remove the locale key in all ten files.
- [ ] `cargo test --lib` (ignored suites compile), `cargo clippy`, `npm run typecheck`, `npm test`
  pass. If Plan 1's server is built: run both e2e suites against it and paste the summary into the
  task report; otherwise record the run as owed. Commit
  `test(sync): e2e suites through the server's admin path; drop the dev grant`.

### Task 10: Locale sweep and copy audit

**Files:** `src/i18n/locales/*/deviceSync.json`, `src/i18n/locales/*/common.json` (10 locales each);
`src-tauri/gen/apple/Localizations/*/InfoPlist.strings` only if the wording needs it.

Decisions:
- Remove keys no code reads any more: `setup.advanced.*` (setup and join no longer use them),
  `setup.pay.selfHosted`, `setup.pay.selfHostedWaiting`, `setup.account.explain` only if unused.
  Check each with `grep -rn "'<key>'" src` (and the template-literal forms) before deleting.
- Reword `validation.syncServerNeedsUpdate` in all ten (Task 2 did en/cs) and confirm the pairing
  tests still expect the key, not the text.
- Audit: no value in any locale contains "relay", "ALLOWED_ACCOUNTS" or "restart" (case-insensitive);
  every new Czech value uses vykání ("Obraťte se", "Použijte", never "Obrať se"); "Moony cloud" is the
  untranslated brand in every locale; `NSLocalNetworkUsageDescription` still says the server may be on
  the home network.

- [ ] **Failing test:** add to `src/i18n/locales.test.ts` `no locale calls the sync server a relay
  or names ALLOWED_ACCOUNTS` — every string value of every locale's `deviceSync.json` and the
  `validation.sync*` block of `common.json`, lower-cased, contains neither `relay` nor
  `allowed_accounts`. Run `npm test -- locales` → fails while `setup.pay.selfHosted` remains.
- [ ] Sweep; run `npm test` → pass (parity included). Commit `chore(i18n): own-server sync copy in all locales`.

### Task 11: ADR 0028 and the docs

**Files:**
- Create `docs/architecture/decisions/0028-self-hosted-sync-server-in-the-app.md`.
- Modify `docs/architecture/decisions/README.md` (index row), `0025-…` and `0027-…` (a one-line
  "Amended by ADR 0028" under the HTTP paragraph / L9 sentence; the decisions themselves are not
  rewritten), `docs/architecture/overview.md` (Device sync section and the domain table row),
  `docs/architecture/database.md` (`sync_state.relay_url` note), `docs/DEPRECATED.md`.

Decisions:
- ADR 0028 (Context / Decision / Consequences, the README template): own server as the setup path
  (connection code format, adoption instead of account creation, the cloud branch kept unreachable);
  `GET /v1/server` and `MIN_PROTOCOL = 2`; plain http for any host but the Moony cloud, reversing ADR
  0025's HTTP rule and ADR 0027 L9, with what an eavesdropper gains (account number, vault token: push
  junk, rotate the token, delete the vault, read ciphertext and metadata; never plaintext) and why
  pairing is unaffected (sealed mailbox); the warning each time, nothing stored; the change-address
  action — `sync_state.relay_url` is the only stored value naming the server, the three checks
  (account, vault with token, `head ≥ cursor`) and why they suffice (the token is known only to
  devices of this vault, an older copy is the ADR 0027 "went back" case), one write, each device
  separately, old Kits still name the old address; own-server status from the stored address;
  dev grant removed; owed platform checks (Android cleartext policy once an Android project exists).
- `DEPRECATED.md` rows under "Retired with device sync": the dev grant (`POST /v1/dev/grant`,
  `sync_dev_grant`); recognising an own server by `paidUntil ≥ 9000` or a `409 self_hosted` checkout
  (→ `SyncStatus.ownServer`); setup's Advanced server field and the `ALLOWED_ACCOUNTS` waiting step
  (→ connection code); `scheme_allowed(.., debug)` (→ the http rule of ADR 0028).
- `overview.md`: name `connection_code.rs`, `server.rs`, the new commands, the own-server setup and
  the change-address action in the Device sync paragraph and the domain row.

- [ ] Write; check every path and name against the code (`grep`). No test cycle (docs only). Commit
  `docs(sync): ADR 0028 for own sync servers in the app`.

### Task 12: Final gates

- [ ] `npm run lint`, `npm run format:check`, `npm run typecheck`, `npm test`.
- [ ] `cd src-tauri && cargo clippy --all-targets`, `cargo test`, and the release flag check
  `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 cargo test --release --lib services::sync::feature`.
- [ ] `cargo test generate_bindings -- --ignored` then `git diff --exit-code shared/generated-types.ts`.
- [ ] `grep -rn "dev_grant\|devGrant\|isSelfHostedPaidUntil\|ALLOWED_ACCOUNTS" src src-tauri/src` → nothing.
- [ ] `feature::RELEASED` is still `false` (`git diff main -- src-tauri/src/services/sync/feature.rs` shows no change to it).
- [ ] Both e2e suites against a Plan 1 server (`Task 9` command), if it exists; else listed as owed.
- [ ] A fresh reviewer on the most capable model reads the branch against spec §9, §12 and the Review
  Focus; every confirmed finding gets a failing test, then a fix. Commit; report the branch tip, the
  owed checks (iPhone LAN run, e2e run, Android cleartext) and the plan's calls below to the owner.

## Plan's calls (for the owner)

1. `http` is refused only for the Moony cloud host; `[::1]` counts as insecure (the spec names two hosts).
2. A connection code tolerates whitespace, zero-width characters and one trailing `.`/`,`;
   `moony-sync2:` and later say "update Moony".
3. One message for every way an address is not an own Moony server (no answer, old relay, wrong
   shape); protocol below 2 says "needs an update"; mode `cloud` says "not an own server".
4. Setup re-runs the server check itself, refuses a blocked person ("ask whoever runs it") and a
   deleted one ("code no longer valid") before storing anything.
5. Own-server status comes from the stored address (`SyncStatus.ownServer`), not from `paidUntil`;
   an own server never shows a deletion date.
6. The http warning is a checkbox per flow, reset when the address changes; pairing asks it at
   "ready to join" for the received server (the mailbox is sealed), Kit joins and setup inline;
   holders are never asked.
7. The Kit join shows the server field only for a typed code; a Kit QR brings its server.
8. Setup resumed after a restart offers "Setup started with {host}" plus "Use another connection code".
9. Changing to the same address is a no-op; a blocked person may still change the address.
10. The e2e admin logs in once per test process with fixed test credentials and `SETUP_CODE`
    `moony-e2e-setup-code`.
11. "Delete synced data" on an own server says sync can be set up again with the same code.
