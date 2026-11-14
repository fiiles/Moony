# 0028. Device sync: own sync servers in the app

Date: 2026-11-14

## Status

Accepted. Amends [ADR 0025](0025-device-sync-relay-client.md) (the HTTP rule and the setup path)
and [ADR 0027](0027-device-sync-audit-fixes.md) (L9, and the `ALLOWED_ACCOUNTS` step of the manual
recovery). Records the app side of `docs/specs/2026-11-14-self-hosted-sync-server-design.md` as
built on `feat/self-hosted-sync` (plan `docs/plans/2026-11-14-self-hosted-sync-server-plan-2-app.md`).
The server it talks to is the Node sync server in the Moony-relay repository, branch
`feat/node-server` (`DESIGN.md` there is authoritative for the server); that branch is not merged
into that repository's main. Sync is still hidden behind the runtime flag
(`feature::RELEASED` is `false`). Where the spec differs, this ADR and the code win; the spec's §15
"As built" lists the differences.

## Context

Until now a person could set up sync only on the Moony cloud, which is not built, or on a
self-hosted Cloudflare relay whose operator added each account number to `ALLOWED_ACCOUNTS` by
hand while the app polled `GET /v1/accounts/me` and showed a payment step. Nobody but the author
can run that. The owner's decision (2026-11-14) is that the way to sync today is a server a person
runs on their own hardware, and that the app has to make that the normal path, not a hidden
"Advanced" field: the server's admin adds a person and hands out a connection code, and the app
takes the code. The Moony cloud stays as a later alternative for people who host nothing.

Three rules of earlier ADRs stood in the way. ADR 0025 allowed plain `http` only for `localhost`
and `127.0.0.1`, and only in debug builds; ADR 0027 (L9) kept that and showed a non-default
server's host. A home server on a LAN address has no certificate a phone would accept, so those
rules make it unusable. The dev grant (`POST /v1/dev/grant`) and the `paidUntil ≥ year 9000` test
for "this is a self-hosted server" were tools of the old path.

## Decision

**An own server is the setup path.** Setup takes a connection code
(`services/sync/connection_code.rs`): `moony-sync1:<16-digit account>:<base64url, no padding, of
the UTF-8 normalised server address>`. Reading drops white space, zero-width characters and one
trailing `.` or `,`, caps the text at 512 characters, accepts only the canonical encoding, and
answers `validation.syncConnectionCodeInvalid`, or `…Newer` for `moony-sync<n>:` with n above 1.
`server_address_from_input` takes either a plain address or a code and returns only the address;
the join dialogs and the change-address action use it, so they never take an account from a code
(the same rules live in `src/utils/sync-server.ts`; the backend stays authoritative). The code is
a credential: it lets its holder create up to 5 vaults for that person; it cannot read an existing
vault.
- The app does not create an account. `setup::connect_own_server` adopts the account the code
  names: it asks `GET /v1/server`, then `account_status`, and stores the address and the number
  (`keys::save_account`) only when the person exists and is not blocked
  (`validation.syncConnectionCodeRevoked`, `validation.syncAccountBlocked`). With sync on and a
  different address or account it refuses (`syncAlreadyEnabled`). Setup is three steps: server,
  Recovery Kit, device.
- The Moony cloud card is shown, disabled, with "Coming soon". The cloud branch of the setup
  dialog (`createAccount`, `startCheckout`, the pay step) and `checkout_url` stay in the code, with
  no way to select them. They are unreachable on purpose, but switching the cloud on still needs
  wiring: the choice's state (`kind` in the setup dialog has no setter), the phone's cloud branch
  (the phone sets up on an own server only) and ChoiceCard selection from the RadioGroup value. A
  reader must not mistake that code for live or finished.
- `SyncStatus` gains `ownServer`, `serverAddress` and `serverInsecure`, computed from the stored
  address (`engine/status.rs`: own server means the stored address is not the Moony cloud). An own
  server has no deletion date: a lapse reads "blocked on your sync server" with no renewal, on
  every path that talks to the server: setup, change address, pairing join, Recovery Kit join,
  repair and re-sync map their errors with `app_error_for(stored or given URL, …)`, which picks
  `to_own_server_app_error` for any server that is not the Moony cloud.

**The server check.** `GET /v1/server` (no authentication) answers `{name: "moony-sync", version,
protocol, mode}`. `server::check_own_server` refuses an address that does not answer that shape
(`syncServerNotFound`), a protocol below `MIN_PROTOCOL = 2` (`syncServerNeedsUpdate`, the key
pairing already used) and a mode other than `self_hosted` (`syncServerNotOwn`). The check runs
before any account call. The backend runs it in setup (`connect_own_server`) and in the change-address
action. The join dialogs run it through the UI (`sync_check_server`, which the server field calls)
and the join commands do not repeat it; the phone's pairing scan checks no server at all, because
it uses the holder's. A running device does not repeat it per round; a server that became too old
shows up as the errors it already had.

**Plain `http` for any host but the Moony cloud, in every build.** This reverses ADR 0025's HTTP
rule and ADR 0027 L9. `relay/http.rs::scheme_allowed(scheme, host)` takes `https` for any host and
`http` for any non-empty host except `sync.moonyfinance.com` (`DEFAULT_RELAY_HOST`); the old
`debug` parameter is gone. The reason is the one above: an own server on a LAN has no usable
certificate. What an eavesdropper on the same network gains from plain `http`: the account number
and the vault token, which let them push junk envelopes, rotate the token (locking the devices
out), delete the vault, and read ciphertext and its metadata (sizes, timing, op IDs). Never
plaintext: the vault key never crosses the wire, and the server cannot read what it stores
either. Pairing is unaffected, because its mailbox carries only a public key and sealed blobs
(ADR 0026, protocol 2); a wire reader learns nothing it can use. Redirects stay off, so the
account header and the token never reach another origin.
- The warning is a duty of the UI, not of the backend, which accepts `http` by design. Whenever an
  `http` address with a host other than loopback (`is_insecure_address`; only `localhost` and
  `127.0.0.1` are exempt, `[::1]` is treated as insecure) is typed, scanned, read from a payload or resumed, the dialog
  shows the warning and needs an explicit tick before it goes on: in setup, in the pairing join
  (also at the `received` step when the credentials name another server than the one typed), in
  the Recovery Kit join and when changing the address. Typing again unticks. Nothing is stored per
  address, so the warning shows every time and no migration is needed. Settings → Sync shows a
  standing "Unencrypted connection (http)" note next to the server address.
- The Moony cloud address over `http` is refused (`syncRelayUrlInvalid`), with trailing dots
  stripped before the comparison, so `http://sync.moonyfinance.com.` is no way around it.

**Changing the address.** A home server's address changes (a new IP, a port, `http` to `https`).
`sync_change_server_address` (password required, only the address of a typed code is used) calls
`server::change_server_address`. `sync_state.relay_url` is the only stored value that names the
server; everything else tied to it is handled explicitly:

| Tied to the old server | What the change does |
|---|---|
| the account number, vault ID, vault token | kept; they are what the new address is checked against |
| the cursor | checked: the vault's `head` at the new address must be at least the cursor (`syncServerAddressWentBack` otherwise) |
| the went-back mark (`sync.vault_went_back_at`) | kept, and not set by a refused change: data a restore lost is not healed by moving |
| the vault-gone pause (`vault_gone_at`) | cleared in the same transaction as the swap: the vault was just proven to exist |
| pairing sessions and the Kit window | forgotten on every attempt, after the password check and the address parse |
| the engine's client | rebuilt: the command runs under `guarded_change`, which stops the engine and starts it again, and `spawn_engine` builds a new `HttpRelay` from the stored address |
| Recovery Kits saved earlier | still name the old address; the text says to save the Kit again (the Kit join lets a person type another server) |

The checks, in order, under the profile's round lock (`engine::exclusive`): the address is
normalised (a bad one answers `syncRelayUrlInvalid` before any call); the same address is a no-op;
the Moony cloud is refused (`syncServerNotOwn`); `check_own_server`; `account_status` for the
stored account (a server that does not know it is `syncServerAddressOtherServer`; a blocked person
is not refused, the move should not need the operator); `vault_info` for the stored vault with the
stored token (not found, unauthorised or read-only is the same answer); then `head ≥ cursor`. The
write is one `UPDATE` (`keys::save_relay_url`) with the end of the vault-gone pause. Any failure
writes nothing. Each device changes its own address; nothing tells the others.

Why these checks are enough, and for what: the vault token is known only to devices of this vault
(and the server that holds its hash), so a server that accepts it for the stored vault ID and the
stored account is the server these devices used, or a copy of it. A copy that is older than the
cursor is the "went back" case of ADR 0027 and is refused. They are enough against an honest
server that is stale or simply a different one. They are not proof of the same server. A hostile
server at a typed address receives the account number, the vault ID and the token, the same
exposure as setup and a Kit join, and unavoidable with bearer tokens. A copy restored from an
older backup that another device has since pushed past this device's cursor passes the check; that
is ADR 0027's "not repaired" case, and it stays so. A blocked person past the server's 30-day read
grace gets `402` on the vault call, which surfaces as the usual blocked message until the operator
unblocks.

**Errors.** The server's new answers map as follows. `403 managed_by_operator` (the person record
belongs to the admin) is a permanent refusal (`RelayError::BadRequest`, so `SyncError::Refused`,
no back-off); it used to read as an unknown code. `403 registration_closed` is
`RelayError::RegistrationClosed`, also a refusal. `503 busy` (the body budget) and `408
body_timeout` (30 s without a byte) are transient: `from_response` takes `408` and 5xx by status
alone, so a proxy's HTML page counts the same, and the round backs off.

**The dev grant is gone.** `HttpRelay::dev_grant`, the `sync_dev_grant` command, its API function,
mutation and `setup.pay.grantTest` copy are removed. The end-to-end suites (`relay_e2e*.rs`) no
longer need a grant or an allow list: they set up the server's admin, add each device's person
through the admin API (`relay_e2e_admin.rs`) and set up from the person's connection code, as a
person does. They are `#[cfg(test)]`, ignored, and run against a server with
`MOONY_RELAY_E2E=<address>`.

## Consequences

- Invariants to keep: `sync_state.relay_url` is the only stored server name, and a change of it
  goes through `change_server_address`; the HTTP rule lives in `scheme_allowed` and the warning in
  every dialog that takes an address; an own server's status comes from the stored address, never
  from `paidUntil`; nothing in a log, error or `Debug` output carries an account number, token,
  vault ID, connection code or an address beyond `origin_of`.
- What this costs. A person on a LAN accepts an unencrypted connection (above). Every device
  changes its address on its own, and a device that is not told keeps failing until it does.
  The cloud branch is dead code until the cloud exists. User text says "sync server"; "relay" lives
  on only in the Rust names and the repository.
- Accepted residuals:
  - The checks of the change-address action prove an honest server is the stored one, not that it
    is the same server (above).
  - A typed address at setup or a join can be a hostile server, which then holds an account number
    or a token.
  - An active attacker on the same network can answer `GET /v1/server` over `http` and stand in for
    the server: the same exposure as a hostile server above. The detection of ADR 0027 (a vault
    that went back) still applies.
  - The server's admin interface is reachable by anyone who can reach the server; its protection
    is the admin's password and second factor (DESIGN.md of the server).
  - ADR 0027's manual recovery for a server restored from an older backup no longer says "add the
    account to `ALLOWED_ACCOUNTS`": the admin keeps the person, and setup again takes the
    person's connection code. The rest of that recovery is unchanged.
- Checks owed before release, none made here:
  - A device check on the iPhone (iOS shows the Local Network permission prompt once for a LAN
    address, and `NSLocalNetworkUsageDescription` is localised and still says a sync server on the
    home network): plain `http` to a LAN IP, the prompt allowed and denied, dark mode and the
    largest Dynamic Type on the server step.
  - Android's cleartext traffic policy, once an Android project exists (`src-tauri/gen` holds only
    `apple` and `schemas`); whether it affects the Rust HTTP client at all is not known.
  - The server's `docker build` was never run (no Docker on the build machine), and the home
    server has not been switched to it.
  - The end-to-end suites passed against the Node server on 2026-11-14 (19 tests, one run with
    the admin set up and one with it signed in); the newest test, a blocked person who can still
    pull, was added afterwards and compiles but has not run against a server.
