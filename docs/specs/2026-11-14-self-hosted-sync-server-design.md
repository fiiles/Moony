# Self-hosted sync server — design

Date: 2026-11-14. Status: **approved for implementation** on 2026-11-14 ("ano zařadit a implementuj to"), including the change-address action (§9.3a). A builder check on 2026-11-14 is
folded in (test seam §6.1, join flows §9.1a, server version check §9.3, blocked and deleted
people §5.4 and §9.1, scanner routing §8, http warning without stored state §9.2).

This replaces the Cloudflare-only relay (`../Moony-relay`, protocol in its `DESIGN.md`) with one
ordinary server that anyone can run from a single Docker container, with a small web admin. The
paid "Moony cloud" will later run the same image; it is out of scope here and stays disabled in
the app.

Decisions the owner made are marked **(owner)**. Decisions Claude made while designing are marked
*(my call)* and listed again in §14 so the owner can check them.

## 1. Why

Device sync was built as a paid service on Cloudflare (Workers, Durable Objects, R2). Self-hosting
existed only as a side mode: `SELF_HOSTED=1` on Cloudflare's local simulator (`wrangler dev`),
an allow-list edited by hand in a config file, a restart to admit an account, and no interface.
That is how the owner's home server runs it today.

The owner changed the decision on 2026-11-14 **(owner)**: self-hosting becomes the native,
first-class way to run sync, simple enough for a person with a NAS or a home server. A commercial
Moony cloud is offered later as the alternative for people who do not want to host anything. In
the app a person picks "own server" or "Moony cloud".

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. One Docker image, one container, one port, one data volume. `docker run` or a short
  `compose.yml` is the whole installation.
- G2. A web admin on the same port, in Moony's design system, for the person who runs the server.
- G3. The server operator sees **people with names**, not anonymous numbers, and adds a person in
  one action.
- G4. The sync protocol the app speaks (`/v1/...`) stays as it is, so the audited device side
  (ADR 0024–0027) is not reopened. Additions are listed in §6.
- G5. Plain `http://` to the person's own server is allowed, with a clear warning **(owner)**.
- G6. The same image can later run Moony cloud with payments switched on. This design leaves the
  seam (§5.4) and builds nothing of the cloud.

**Non-goals (this work)**

- Moony cloud: Polar, checkout, webhooks, subscription management, moving a subscription to a new
  account. In the app the "Moony cloud" choice is visible and disabled **(owner)**.
- Named user logins. A sync account stays a 16-digit number with no name, e-mail or password on
  the wire **(owner, after weighing the alternatives)**. Only the admin has a login.
- Migrating data from the Cloudflare simulator. The only existing data is the owner's test data
  (1.8 MB on `vnl`); sync is set up again on the new server *(my call)*.
- Removing a lost device (phase 5 of device sync), attachments/blobs, WebSocket push.
- Flipping `feature::RELEASED`. Sync stays hidden until the owner decides to release it.
- High availability, clustering, external databases. One process, one SQLite file.

**Anti-goals** (would make this a failure even if it works)

- The server or its admin being able to read, or appearing to read, any synced content.
- The server contacting anything on its own: no telemetry, no update check, no e-mail.
- Weakening the device-side crypto, pairing or Recovery Kit to make the server simpler.
- An admin interface that is reachable without authentication, even for a moment after install.

## 3. Approaches

1. **One native server for everyone (chosen, owner).** Rewrite the relay as a plain Node server
   on SQLite and files, in one Docker image with a web admin. Moony cloud later runs the same
   image on a VPS with payments on. One codebase, one test suite, one audit; "you host exactly
   what we run". Cost: the owner operates a VPS for the cloud (backups, updates), and one machine
   is the scaling ceiling (thousands of paying users; beyond that is a later problem).
2. **One codebase, two runtimes.** Logic behind a storage interface; Cloudflare (Durable Objects,
   R2) for the cloud, SQLite for Docker. The cloud stays serverless, but two storage layers must
   behave identically forever, and the admin would exist only for Docker.
3. **Package today's Cloudflare simulator in Docker and add an interface.** Fastest, but it
   rests on a development tool with no durability promise. Rejected for other people's data.

The owner picked 1. The cloud has no users yet, so leaving Cloudflare is cheapest now.

For the account model three options were weighed: keep app-created numbers approved by the admin;
**the admin creates people and hands out a connection code (chosen)**; real name/e-mail/password
logins with roles. The third was rejected because a password adds no protection (the vault key
protects the data), a login would not remove pairing or the Recovery Kit (the server never has the
key), it would make the owner a controller of personal data in the cloud, and it reopens audited
code. The chosen option gives the operator names without changing the protocol.

## 4. What exists today (context for the builder)

- `../Moony-relay`: ~1,900 lines of TypeScript. `src/index.ts` (routing, validation, rate limits),
  `src/vault.ts`, `src/account.ts`, `src/mailbox.ts` (Durable Objects with SQLite storage),
  `src/polar.ts`, `src/selfhosted.ts`. Tests: Vitest inside workerd. `DESIGN.md` is the protocol.
- Branch state: `main` lacks two things the app already depends on. **`feat/pairing-ack`**
  (pairing slot 3, `vault_deleting`, interrupted vault deletion finishing later) is what the home
  server runs and what the app's pairing protocol 2 needs. `feat/dev-grant` (a development-only
  entitlement grant) is unmerged. **The new server's behaviour starts from `feat/pairing-ack`.**
  The dev grant is not carried over (§5.4 makes it unnecessary).
- Moony app (`src-tauri/src/services/sync/`): `relay/http.rs` holds `DEFAULT_RELAY_URL`,
  `validate_base_url` / `scheme_allowed` (today: `https`, or `http` only for `localhost` /
  `127.0.0.1` in a debug build). Setup creates the account itself (`POST /v1/accounts`), then
  waits on a "pay" step; a self-hosted server is recognised by `409 self_hosted` from checkout
  (`CheckoutTarget::SelfHosted`) and by the year-9999 `paidUntil` (`isSelfHostedPaidUntil`).
  Desktop wizard: `src/components/settings/sync/SyncSetupDialog.tsx` (steps `account`, `pay`,
  `kit`, `device`); phone: `src/mobile/components/sync/SetupSheet.tsx`. The server address is a
  hidden "Advanced" field. The Recovery Kit QR and the pairing payload already carry the server
  address.
- User-facing copy never says "relay"; it says "synchronizační server" / "sync server".

## 5. The server

### 5.1 Shape

- One Node process (current LTS) serving, on one port (default 8787):
  - `/v1/...` the sync API for the app (§6),
  - `/admin/api/...` the admin API (session cookie),
  - `/` the admin web interface (static files built into the image).
- State in one volume, `/data`:
  - `moony-sync.db`: SQLite in WAL mode. Accounts, people, vaults, ops, rekey bundles, previous
    tokens, pairing mailboxes, admin, sessions, settings.
  - `snapshots/<vault id>/<seq>`: snapshot files (up to 100 MiB each, two kept per vault),
    written to a temporary name and renamed, so a crash never leaves a half file under a real
    name.
- Durable Object alarms become one in-process housekeeping loop (pairing mailbox expiry on its
  10-minute / 24-hour schedule; the daily per-vault tasks from `DESIGN.md` "Housekeeping"). It
  must be safe to miss while the container is stopped: every time-based rule is also enforced at
  request time, as the mailbox already does.
- Cloudflare rate-limit bindings become in-memory limiters with the same numbers
  (`DESIGN.md` "Abuse limits"). The client address is the socket address, or the first
  `X-Forwarded-For` entry only when `TRUST_PROXY=1` is set. `CF-Connecting-IP` is no longer read.
- The server makes **no outbound connections** in self-hosted mode.
- It logs to stdout: start-up, the one-time setup code, errors. Never account numbers, tokens,
  vault or mailbox IDs, or client addresses.

*(my calls)* TypeScript on Node, to reuse the relay's logic and tests; a fetch-style HTTP layer
(for example Hono) so `index.ts` ports with small changes. The SQLite driver, the HTTP library and
the admin front-end tooling are left to the builder (§13).

### 5.2 Configuration

Environment variables, all optional:

| Variable | Default | Meaning |
|---|---|---|
| `PORT` | `8787` | Listening port |
| `DATA_DIR` | `/data` | Database and snapshots |
| `PUBLIC_URL` | unset | The address apps use; also editable in the admin (§7.5) |
| `TRUST_PROXY` | `0` | `1` when a reverse proxy sets `X-Forwarded-For` |
| `REQUIRE_2FA` | `0` | `1` makes the admin's second factor mandatory (for the cloud) |
| `SETUP_CODE` | unset | A fixed first-run setup code instead of a random one, for automated installs and tests (§6.1). Ignored once an admin exists. |

Everything else is set in the admin and stored in the database. No config file to edit, no
restart to admit a person.

### 5.3 People and accounts

- A **person** is a row the admin creates: a name (free text, required, shown only in the admin),
  a state (`active` or `blocked`), a creation time, and exactly one **account number** generated
  by the server (16 digits, as today).
- The account number is still the credential the app sends (`Moony-Account`). A person's
  **connection code** (§8) is the server address plus that number.
- `POST /v1/accounts` (the app creating its own account) answers `403 {"error":
  "registration_closed"}` in self-hosted mode. There is no pending state and no "accept new
  accounts automatically" switch: the admin creating the person is the admission **(owner)**.
- One account may own up to 5 vaults (profiles), as today *(my call: keep the cap)*.
- What the server stores about a person beyond today's "What the server knows": the name the
  admin typed, and a last-activity time per vault (the time of the last authorised request,
  rounded to the minute). Device names and counts stay invisible to the server.

### 5.4 Entitlement seam

Today every rule hangs on `paid_until`. That stays the single decision point:

- `active` person → `paid_until` is the far-future constant (year 9999), exactly what the app
  already treats as "own server".
- `blocked` person → `paid_until` is the moment of blocking. Writes stop at once; reads continue
  for 30 days so a device can still be restored (the existing read grace).
- **Blocking never deletes.** The 90-day retention delete does not run in self-hosted mode; data
  goes only when the admin deletes the person or a profile, or the app deletes its vault
  *(my call)*.
- Unblocking restores the far-future constant.
- A blocked person's devices may still rotate the vault token and delete their vault with the
  current token, as `DESIGN.md` already allows without entitlement. After the 30 days reads
  answer as for any lapsed account until the person is unblocked; the data stays on the server
  *(my call: "blocked" means stopped, and the admin can unblock at any time to let a restore
  through)*.
- **Deleting a person** deletes their vaults, snapshots and account. Their devices then get
  `404 vault_not_found` on vault calls, which the app already shows as "the synced data were
  deleted from the sync server" with the offer to turn sync off. A connection code of a deleted
  person gets `404 account_not_found` at setup; the app says the code is no longer valid (new
  copy).
- Vault and account now live in one database, so a vault reads its account's state directly on
  every call; the Worker's cached `paid_until`, pokes and "lapse starts when first seen" logic
  are not ported.

The function that answers "until when is this account entitled" is the only place a later cloud
mode plugs in (subscription instead of the admin's switch). Nothing of Polar is ported now; the
last Worker commit is tagged so the Polar code can be found later.

### 5.5 Admin authentication

- **First run.** With no admin in the database the server prints a one-time setup code to the
  log (`docker logs`) and the web interface shows only the setup page: setup code, admin name,
  password (twice). Nothing else under `/admin/api` answers until setup is done. The code is
  re-generated at every start while no admin exists.
- **Login.** Name and password **(owner)**; password stored as an Argon2id hash. Session cookie:
  `HttpOnly`, `SameSite=Strict`, `Secure` when the request came over https. State-changing admin
  calls also check the `Origin` header. Sessions end after 12 hours without use and on logout.
- **Second factor.** TOTP (RFC 6238, authenticator app) with 10 one-time backup codes shown once
  at enrolment **(owner asked for 2FA)**. Optional by default; `REQUIRE_2FA=1` forces enrolment
  right after first login and removes the option to turn it off.
- **Lost password or lost second factor.** A command inside the container,
  `docker exec <container> moony-sync reset-admin`, clears the admin and all sessions and prints a
  new setup code. Whoever can run commands on the server owns it; the server never sends e-mail.
  People, accounts and data are untouched.
- **Guessing.** Login, setup-code and second-factor attempts are rate-limited per client address
  and globally (the builder picks numbers in the spirit of the existing limits; a locked-out
  admin still has the reset command).
- One admin. More admins and roles are not built.
- Over plain http the admin password and session cookie cross the network in clear text. The
  login page says so when the page was loaded over http from a host that is not `localhost`.

## 6. Sync API changes

The `/v1` API is `DESIGN.md` v1.1 as on `feat/pairing-ack`, with these changes only:

| Change | Detail |
|---|---|
| New `GET /v1/server` | No auth. → `{"name": "moony-sync", "version": "<semver>", "protocol": 2, "mode": "self_hosted"}`. The app uses it to confirm an address is a Moony sync server and to learn the mode, instead of asking for a checkout. Rate-limited like account lookups. |
| `POST /v1/accounts` | `403 registration_closed` (§5.3). |
| `POST /v1/accounts/me/checkout` | Still `409 self_hosted`, so older app builds keep working. |
| `DELETE /v1/accounts/me` | `403 managed_by_operator`: the admin owns the person record. |
| `/v1/webhooks/polar`, `/v1/dev/grant` | Do not exist (404). |
| Rate-limit key | Socket address / `X-Forwarded-For` (§5.1) instead of `CF-Connecting-IP`. |

### 6.1 Tests without open registration or a dev grant

The relay's test helpers (`test/helpers.ts`: the `applySubscription` Durable Object stub,
`cf-connecting-ip`) and the app's e2e suites (`relay_e2e.rs`: `create_account` →
`POST /v1/accounts`, `dev_grant` → `POST /v1/dev/grant`, and the `ALLOWED_ACCOUNTS` test) cannot
run unchanged. Their replacement is the real admin path, with no test-only backdoor in the
server: the harness starts the server with `SETUP_CODE` set, creates the admin through the setup
call, logs in, and creates, blocks and deletes people through `/admin/api`. Rewriting those
helpers and suites is in scope, and so is removing the app's `dev_grant` call, its "grant test
subscription" button and `MOONY_RELAY_E2E_*` self-hosted variables that no longer mean anything.

### 6.2 Compatibility

Everything else (vaults, ops, snapshots, token rotation, rekey bundles, pairing mailbox with slots
1–3, limits, status codes, error names) must be byte-for-byte compatible. The existing relay test
suite, moved to run over HTTP against the Node server, is the proof, together with the app's
`relay_e2e*.rs` suites run against a container.

## 7. Web admin

Built with the tokens and component classes of `docs/design-system/` (`tokens.css`, `moony.css`),
copied into the server repository as a snapshot; light and dark follow the system setting. The
same shell as the app: a sidebar with the moon mark, "moony" and the subtitle "Synchronizační
server"; pages with eyebrow, title, stat tiles, cards and tables. Wireframes approved in layout,
then redrawn in the design system at the owner's request:
`.superpowers/brainstorm/16180-1794645219/content/sync-server-screens-v3.html` (git-ignored; it
still shows the earlier "pending account / approve" model, which §5.3 replaced).

Colour rules of the design system hold: green only for gain, red-brown only for destructive
actions. Person states are neutral badges (`active` plain, `blocked` outlined).

### 7.1 Overview

- The address apps use, with a copy button (§7.5 explains where it comes from).
- Four tiles: people, profiles, space used (and free on the data volume), last sync.
- Server version and a status line. A sentence that the server stores only encrypted data.
- A notice when the page is served over plain http from a non-local host.

### 7.2 People

- Table: name, state, profiles, space, last activity, actions.
- **Add person**: asks the name, creates the person and account, then shows the connection code
  as text and as a QR code with "copy" and a short instruction for the person.
- Per person: rename, show connection code again (asks for the admin password again first, since
  the code is a credential), block / unblock, delete (confirmation names the person and says
  their data on the server is removed for all their devices and cannot be brought back from the
  server).
- Person detail lists their profiles (shortened vault ID, space, last activity) with a delete per
  profile. The account number is never shown in full outside the connection code.

### 7.3 Backup

- **Download backup**: one archive with a consistent copy of the database (SQLite online backup)
  and the snapshot files.
- Restore is manual and documented: stop the container, replace the contents of `/data`, start.
- The page states the consequence plainly: restoring an older backup makes every device report
  that the sync server went back (ADR 0027), and the way out is to set sync up again from the
  device with the most complete data. A backup protects against losing the server, not against
  losing devices; the Recovery Kit is still needed.

### 7.4 Settings

- Admin name and password change (asks the current password).
- Second factor: enrol, show new backup codes, turn off (hidden when `REQUIRE_2FA=1`).
- Log out everywhere.

### 7.5 The address apps use

The server cannot know how it is reached from outside. The address in connection codes is, in
order: the value saved in Settings; else `PUBLIC_URL`; else the origin the admin's browser is
using right now, offered as a pre-filled suggestion the admin confirms before the first
connection code is made. It is validated with the app's rules (§9.2) and stored normalised.
Changing it later does not break joined devices (they keep the address they have); the page says
that devices must be able to reach the old address or be set up again.

### 7.6 Language

*(my call)* Czech and English at first, chosen from the browser language with a switch in
Settings, built so further languages are a translation file. The app's ten-locale rule (AGENTS.md
rule 8) applies to the app; the owner decides whether the admin must match it (§14).

## 8. Connection code

What the admin hands to a person so their app can use the server.

- Carries: a format version, the normalised server address, the 16-digit account number.
- Two renderings of the same content: a text code that survives copy-paste and chat apps (one
  line, URL-safe characters, a recognisable prefix such as `moony-sync1:`), and a QR code.
- It is a credential. Whoever has it can use the person's account (create up to 5 profiles, fill
  their space). It cannot touch an existing profile (vault calls need the vault token) and it
  cannot decrypt anything.
- Not single-use *(my call)*: the same person may set up a second profile, or set sync up again
  after turning it off, with the same code. Revocation is blocking or deleting the person.
- The exact encoding is the builder's, with one constraint: the scanner must tell a connection
  code from a pairing QR and from a Recovery Kit QR without guessing. `scannedCodeKind`
  (`src/utils/pairing-code.ts`) knows `moony-pair:`, `moony-recover:` and the bare recovery code
  today and gets a new kind. The setup camera accepts a connection code; the pairing and Kit
  scanners answer it with a named message ("this is a connection code for setting up sync, not
  …"), the way a wrong code kind is handled today, and the setup camera does the same for the
  other two kinds.

## 9. App changes (Moony-tauri)

### 9.1 Setup flow

Desktop wizard and phone sheet, first step:

```
Kde se mají data synchronizovat?            (v obou případech koncově šifrovaná)

( ) Moony cloud            [Připravujeme]   disabled
    Nic neinstalujete, o provoz se staráme my.

(•) Vlastní server
    Připojovací kód  [ moony-sync1:…                    ]   (phone: "Naskenovat kód")
    ● Server nalezen · verze 1.0.0
    ⚠ Spojení není šifrované (http) …                       only for http, §9.2
                                                   [Zrušit] [Pokračovat]
```

- The field takes the connection code, not a bare address **(follows from §5.3; the approved
  wireframe showed an address field)**. After a valid code the app calls `GET /v1/server` and
  shows the result; "Continue" needs a reachable Moony sync server in `self_hosted` mode.
- Setup **adopts** the account number from the code instead of creating one, checks
  `GET /v1/accounts/me` (`active`), and goes straight on. The `account` and `pay` steps are not
  shown for an own server: the steps are server → Recovery Kit → this device (3 steps).
- The account number is no longer presented as a step. It remains in the Recovery Kit and in
  Settings → Sync for support.
- A blocked or deleted person: the app shows the existing "lapsed" state with the own-server
  wording ("ask whoever runs your sync server"), never a payment prompt.
- All copy that mentions `ALLOWED_ACCOUNTS` or restarting the server goes away. The hidden
  "Advanced → server address" field of **setup** goes away.
- The cloud branch of the wizard (create account, pay, wait) stays in the code behind the
  disabled choice; it is not removed and not reachable.
- Joining a further device (QR pairing, Recovery Kit) keeps its protocol; both payloads already
  carry the server address.

### 9.1a Join dialogs

A joining device sometimes has to be told the server before it has any payload: a desktop that
shows the pairing QR opens its mailbox on a typed address (`shower_relay`,
`commands/sync_pairing.rs`), and a Recovery Kit typed as text may come without one. Today that is
the shared `JoinFields.tsx` "server address" field of `JoinSyncDialog` and `RecoveryJoinDialog`
(and their phone counterparts).

*(my call)* That field stays, with the same two-way choice as setup (Moony cloud disabled, own
server), and for an own server it accepts **either** a plain address **or** a connection code,
from which only the address is used. The http warning of §9.2 and the server check of §9.3 apply
there too.

### 9.2 Plain http

- `validate_base_url` / `scheme_allowed` accept `http` for any host, in release builds too, for
  an address that is not the Moony cloud **(owner)**. `DEFAULT_RELAY_URL` stays https.
- Whenever an `http://` address with a host other than `localhost` / `127.0.0.1` is entered,
  scanned or read from a payload in setup, pairing join or Recovery Kit join, that dialog shows
  the warning and needs an explicit continue: the data stays encrypted, but anyone on the same
  network can capture the access to sync and delete or block the synced data on the server. The
  warning is shown each time such a flow runs; nothing is remembered per address, so no storage
  and no migration *(my call)*.
- Settings → Sync shows a standing "unencrypted connection" note next to the server address.
- This reverses a recorded rule ("a release build never sends a pairing or a sealed batch over
  plain text", `relay/http.rs`, ADR 0025/0027). A new ADR records the reversal, its reason and
  what an eavesdropper gains: the account number, the vault token (so: push junk, rotate the
  token, delete the vault, read ciphertext and metadata), never plaintext. Existing tests that
  pin the old rule (for example a release build refusing `http://evil.example` in `recovery.rs`)
  change with it.
- Platform checks the builder must make, not assume: iOS shows the Local Network permission
  prompt for LAN addresses and needs `NSLocalNetworkUsageDescription` (localised); whether App
  Transport Security or Android's cleartext policy affects the Rust HTTP client at all.

### 9.3 Server check and version

`GET /v1/server` is a new call on the `Relay` trait, its HTTP client and `relay/fake.rs`. Setup
and the join dialogs call it before going on:

- not a Moony sync server (no answer, wrong shape) → "no Moony sync server answers at this
  address";
- `protocol` lower than the app needs → the app refuses with "this sync server needs an update".
  Copy for that exists only for pairing today (`syncServerNeedsUpdate`); it is generalised;
- `mode` other than `self_hosted` behind the "own server" choice → refused (the cloud is not
  built).

A running device does not re-check on every round; a too-old server shows up there as the errors
it already has.

### 9.3a Changing the server address **(owner, 2026-11-14)**

A home server's address can change (new IP, new port, http to https). Settings → Sync gets a
"change server address" action on a device that has sync on:

- It takes a plain address or a connection code (only the address is used), applies the http
  warning (§9.2) and the server check (§9.3).
- Before anything is stored it proves the new address is the same server state for this device:
  `GET /v1/accounts/me` answers for the stored account and `GET /v1/vaults/:id` answers for the
  stored vault with the stored token, and the vault's `head` is not behind what this device has
  already seen (otherwise it is the "server went back" case of ADR 0027 and the change is
  refused with that explanation).
- Only then the stored address is swapped, in one write, and a sync round runs. On any failure
  the old address stays.
- It needs the device password, like the other sensitive sync actions.
- Each device changes its own address; the Recovery Kit shown afterwards carries the new one. An
  older saved Kit still names the old address, and the Kit join already lets a person type a
  different server (§9.1a). The action's text says to save the Kit again.
- The new ADR (§9.2) covers this action too: which stored values name the server, and why the
  checks above are enough.

### 9.4 Rules that apply

AGENTS.md rules 1–10, in particular: `invoke()` only in `src/lib/tauri-api.ts`; `shared/schema.ts`
procedure for any wire type; logic in `services/sync/`, thin commands, `AppError`; every new or
changed string in all ten locales; no migration is expected (the account number and server
address are already stored) — if the builder finds one is needed, it is append-only.

## 10. Packaging and operation

- Multi-arch image (`linux/amd64`, `linux/arm64`), non-root user, `HEALTHCHECK` on
  `GET /v1/server`, `VOLUME /data`, graceful shutdown (finish requests, checkpoint the WAL).
- The repository ships a `compose.yml` and a README section that is the "Jak si server spustit"
  page the app links to: run, read the setup code, create the admin, add a person, paste the
  code into the app; then reverse proxy with https (Caddy and Nginx Proxy Manager examples),
  `TRUST_PROXY`, backup, update (pull the new image, restart), reset-admin.
- Database schema changes ship as numbered, append-only migrations run at start; a server never
  starts on a database newer than it knows.
- Versioning: the image carries a semver; `GET /v1/server` reports it. `protocol` rises only when
  the app must refuse an older server (§9.3).

## 11. Repository and old code

*(my call)* The server stays in `../Moony-relay` (same protocol, same history). The Cloudflare
Worker code, `wrangler.jsonc` and the cost model are removed once the Node server passes the
ported suite; the last Worker commit gets a tag. `feat/pairing-ack` is the starting point;
`feat/dev-grant` is dropped. `DESIGN.md` and `README.md` are rewritten for the new server.

Where the image is published and whether the repository becomes public are release decisions for
the owner (§14); nothing in the build depends on them.

## 12. Security notes for the builder

- The threat model of ADR 0025–0027 is unchanged: the server is untrusted for content and may
  drop, delay, replay or roll back; devices detect it. Nothing here may make devices trust the
  server more.
- New attack surface is the admin: setup-code race on a fresh install, login guessing, session
  theft over http, CSRF, and the connection code as a bearer credential shown in a browser.
  §5.5 and §7.2 are the mitigations; a security review of the admin is part of done.
- Vault tokens stay stored only as SHA-256; account numbers are stored in clear (they must be
  shown again in a connection code), so a database or backup leak exposes them. The backup page
  says the archive must be kept private.
- Request bodies keep the running byte count limits; snapshot uploads stream to disk.

## 13. Left to the builder

- HTTP library, SQLite driver, front-end tooling for the admin, test runner layout.
- Exact table layout, housekeeping scheduling, limiter implementation.
- Connection code encoding within §8's constraints.
- Rate-limit numbers for admin endpoints.
- How the admin UI is tested beyond API tests.
- Phasing. A sensible order: (1) server core at protocol parity, proven by the ported suite and
  the app's e2e suites; (2) admin API, authentication and web interface; (3) image, compose,
  docs; (4) app changes and ADR; (5) with the owner's go-ahead, replace the stack on the home
  server and run the live checks.

## 14. For the owner to check

Calls Claude made:

1. No migration from the Cloudflare simulator; sync is set up again.
2. Blocking a person never deletes their data; only deleting does.
3. The 5-profiles-per-person cap stays.
4. The connection code is reusable, revoked by blocking or deleting the person.
5. The "own server" field in the app takes a connection code, not an address.
6. Admin interface in Czech and English first.
9. A blocked person can read for 30 days, then nothing until unblocked; data is kept.
10. The join dialogs keep a server field that takes an address or a connection code.
11. The http warning appears every time an http address enters a setup or join flow; the app
    remembers no acknowledgement.
13. The change-address action refuses a server whose vault head is behind this device.
12. Tests reach the server through the real admin path (`SETUP_CODE`), and the app's dev grant
    and its "grant test subscription" button are removed.
7. The server stays in the `Moony-relay` repository; the Cloudflare code is removed after parity.
8. TypeScript on Node.

Open, not blocking the build:

- Whether the server repository is public, and where the image is published (for example GHCR
  under the Moony organisation).
- When `RELEASED` flips, and whether own-server sync ships before the cloud exists.

## 15. As built (2026-11-14)

The sections above are the design as approved and are not rewritten. This section lists where the
build differs. The server's `DESIGN.md` and `README.md` (Moony-relay repository, branch
`feat/node-server`, head 0313a86) are authoritative for the server; the app side is recorded in
[ADR 0028](../architecture/decisions/0028-self-hosted-sync-server-in-the-app.md).

Server

- §5.1: behind `TRUST_PROXY` the client address, scheme and host come from the last
  `X-Forwarded-For` / `-Proto` / `-Host` entry, not the first. The last is the one a single proxy
  appended; the earlier ones are what the client sent. Exactly one proxy may stand in front.
- §5.1: IPv6 clients are keyed by their /64 prefix (an IPv4-mapped address as the IPv4), so one
  host cannot take a new key per address.
- §5.1 and §10: the image runs Node 24 (the spec said only "current LTS"); `package.json` allows
  Node 22 or newer for running without Docker.
- §5.1 and §6: pairing mailboxes are swept every 60 seconds on their own, not only in the daily
  housekeeping, so slot contents exist at most about 11 minutes.
- §5.5: `2fa/enrol` asks for the password again; a successful confirm ends the other sessions;
  wrong-attempt limits are 5 per client and 50 overall per 15 minutes, with at most 4 concurrent
  Argon2 verifications. The spec had a per-client limit only. Reason: one client address must not
  be able to lock the admin out, and many addresses must not guess in parallel.
- §5.5: a setup code written by `reset-admin` stays valid across restarts until it is used (a
  random code is new at every start). Reason: a person who resets and then restarts the container
  would otherwise lose the code they were given.
- §5.5: the `Origin` check compares host[:port] with `Host` and ignores the scheme, so a proxy that
  terminates https in front of an http listener still works.
- §6: two new answers on the sync API. `503 busy` when the in-flight body budget (64 MiB, 8 MiB
  per client key) or the pairing mailbox caps (64 MiB in all, 8 MiB per client key) are full, and
  `408 body_timeout` after 30 seconds without a byte. Reason: a trickling client could otherwise
  hold the server's memory. Both are transient in the app.
- §6: a push or token rotation is judged by its token before its body is read, and the per-vault
  op quota charges ciphertext plus 128 bytes per op, so many tiny ops cannot hide from it.
- §7: the admin interface calls people "Osoby" / "People"; the backup page says the archive holds
  account numbers and the second-factor secret in readable form; backup tar entries are owned by
  uid/gid 1000 (the container's `node` user), files mode 0600, directories 0700.

App

- §9.1a: the server field is required for a typed Recovery Kit code, in both the desktop and the
  phone join. The cloud is disabled, so an empty field (the old meaning: the default server) has no
  use.
- §9.2: the http warning is also shown when a setup is resumed with a stored http address, and at
  the pairing join's `received` step when the credentials name a server other than the typed one.
- §9.3a: a successful address change also clears the "vault gone" pause (the vault was just proven
  to exist) and keeps the went-back mark (data lost by a restore is not healed by moving); the
  client is built from the checked address inside the service, so no other string reaches it.
  The same address typed again is a no-op, and the dialog says so.
- §9.1: the account number is NOT shown in Settings → Sync; it appears only inside the Recovery
  Kit, which needs the device password.
- Blocked person (final review I1): every path that talks to an own server (setup, change address,
  pairing join, Kit join, repair, re-sync) reads "blocked on your sync server", never the payment
  message.
- Resumed setup (I2): the server step does not contact the stored server until Continue; a failure
  (unreachable, rate-limited) shows its own message, and only a real "not active" reads blocked.
- Change address (M5): an address equal to the stored one is said so at the address step, and the
  password is never asked for.
- `403 managed_by_operator` is a permanent refusal in the app (`SyncError::Refused`), `503` and
  `408` are transient. The spec named the answers but not their classes.

Accepted limits

- §12 and §14: the admin has no `Host` allowlist. A DNS-rebinding page can reach only the
  unauthenticated routes of a LAN-only server. What protects it: setup needs the code from the log,
  login is attempt-limited, everything else needs a session or a vault token.
- Real disk use of many tiny changes can reach about twice the quota, because SQLite pages and
  indexes are not counted by the 128-byte charge.
- Parallel snapshot uploads are not limited (they stream to disk and are exempt from the body
  budget).

Not done, owed

- `docker build` was never run (no Docker on the build machine); the image is untested.
- The home server has not been switched to the Node server.
- iPhone device check: the Local Network prompt, plain http to a LAN IP, dark mode, Dynamic Type.
- Android's cleartext policy: no Android project exists yet.
- The server branch `feat/node-server` (Moony-relay repository, worktree `../Moony-relay-node`, head
  0313a86) is not merged into that repository's main.
- The end-to-end suites passed against the Node server (19 tests); one test added later, a
  blocked person who can still pull, has not run against a server.
