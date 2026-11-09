# Device Sync Phase 2 (Crypto and Relay Client) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put phase 1's engine on the wire for the desktop (spec §17 phase 2): sealed envelopes and
snapshot streams, an HTTP client for Moony-relay, the engine loop of §9.4, first-device setup with
checkout and a Recovery Kit, turning sync off, and a Settings → Sync section with a shell indicator,
all hidden behind a runtime flag and tested against `wrangler dev` of Moony-relay.

**Architecture:** New files under `src-tauri/src/services/sync/` take `&Connection`, plain data or
two small traits (`Relay` for the network, `ConnAccess` for the session-pinned connection), so the
engine runs in unit tests against an in-memory `FakeRelay` and in one ignored test against the real
relay. The engine never holds the connection mutex across a network call: it seals and stores under
the lock, talks to the relay unlocked, and merges each pulled changeset in its own transaction. Engine
functions return a typed `SyncError`, so a round can tell a gone vault from a lapsed subscription
from a network blip. The Tauri layer (`commands/sync.rs`) is thin; the React side gets one hidden
Settings section, a status indicator, and an event listener that runs the rule-7 refresh after
remote changes.

**Tech Stack:** Rust 2021, rusqlite 0.40 (SQLCipher), tokio 1, reqwest 0.13 through
`services/http.rs`, RustCrypto (`chacha20poly1305` 0.11, `hkdf` 0.13, `sha2` 0.11, `subtle` 2),
`x25519-dalek` 3, `qrcode` 0.14 (SVG), flate2 (raw deflate), base64 0.23; React 19, TanStack Query 5,
i18next (ten locales), Vitest (node). Moony-relay (Cloudflare Worker, TypeScript, Vitest in workerd).

**Spec:** `docs/specs/2026-11-06-device-sync-client-design.md` — read all of it, starting with §16a
(owner answers) and §20 (as built). Also `docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md`,
the phase 1 record in `.superpowers/sdd/device-sync-phase-1-record/` (`progress.md`,
`preflight-rulings.md`, `rulings.md`), the relay contract
`/Users/filipkral/Programování/FinanceApp/Moony-relay/DESIGN.md` (protocol v1.1), and this plan's
pre-flight in `.superpowers/sdd/2026-11-07-device-sync-phase-2-relay-client/` (`preflight.md`,
`preflight-rulings.md` P1–P18; this revision implements them).

## Spec corrections (the code and this plan win; the spec is not edited)

1. **AAD and header encodings (§9.2).** The spec names the fields, not their bytes. Here: the vault
   ID is its 16 raw bytes (never the base64url text); the epoch is `u32` big-endian; a snapshot's
   object ID is `S` as `u64` big-endian. Op AAD = `"MNY1" | epoch | vault_id` (24 bytes). Stream
   header = `"MNY1" | epoch | salt (32) | nonce prefix (19)` (59 bytes); chunk AAD =
   `"MNY1" | epoch | vault_id | object_id`.
2. **Deflate is raw deflate (RFC 1951)** with flate2's default level, for envelopes (the phase 1
   ruling: it must match `changeset::padded_deflated_len`) and for snapshot streams alike.
3. **Key epochs start at 0**, matching the relay's `rotation` counter (0 = never rotated).
4. **The device key pair is generated at setup (and at every join and re-join), not only "when it
   joins" in phase 3.** `sync_devices.public_key` is write-once in the merge (ADR 0024 D12–D13), so a
   device row recorded without a key could never gain one.
5. **Barrier marker (§14, phase 1 ruling on relay order).** "Records the barrier in the next
   envelopes" assumes there is a next envelope. The marker of barrier `B` is the first envelope in
   relay order whose `barrier ≥ B`; `sync_state.barrier_seq` records its relay position, the snapshot
   header carries it, and a device that has sealed nothing at barrier `B` pushes one empty marker
   changeset (`rows: []`), at most once per barrier (`sync_state.marker_barrier`, ruling P5).
   `BelowBarrier` applies only to envelopes placed after the marker.
6. **Snapshot policy bytes (§8).** The relay reports total stored op bytes, not bytes since the
   newest snapshot. "8 MiB accumulated" is counted locally: the ciphertext bytes this device pulled
   at relay positions after the vault's newest snapshot. The first snapshot of a vault (and a vault
   with none) is uploaded at once, outside the once-a-day limit and the random delay.
7. **Re-join re-stamps (phase 1 ruling).** §11.4 says re-sending an edit with its original clock is
   harmless; it is not when a rolled-back copy reissued an old `(hlc, device)` with another value. A
   re-join re-stamps the re-queued **column** clocks of the old ID with fresh clocks of the new ID.
   Row clocks (`'*'`) and tombstones (`'-'`) keep their value: re-stamping them would move delete
   epochs and resurrect or kill rows.
8. **`chain::rejoin` takes the triggering envelope's meta**, not its sequence number, so a re-join
   with an empty confirmed prefix can take `(seq − 1, prev)` of the trigger as the old ID's head
   (phase 1 carry "empty-prefix head").
9. **Restore marker (§11.4).** Phase 1 did not write it. `backup::restore_backup` now writes
   `sync-rejoin.marker` next to the profile.
10. **Keychain binding (§9.3).** The desktop keychain plugin stores biometry-gated blobs only, and the
    macOS app has no keychain entitlement. The binding is a 32-byte value sealed by a Secure Enclave
    key without user presence (macOS) or by DPAPI with machine scope (Windows), kept in
    `sync-binding.bin` next to the profile; Linux has none. It catches a profile moved to another
    machine; a copy on the same machine is left to the relay check (§9.3 own log).
11. **Recovery Kit (§11.5, D24).** The app has no PDF stack. The Kit is a self-contained printable
    HTML page (inline SVG QR, print CSS) plus the same as plain text, both saved through the save
    dialog. The QR encodes `moony-recover:1:<account number>:<80-character code>`.
12. **"A local write (debounced 5 s)" (§9.4)** is detected by polling the outbox every 5 s, not by
    a hook: a round starts when the outbox is non-empty and did not change for one poll, or 60 s
    after it was first seen non-empty. Every write path (UI, MCP, imports, self-heals) is covered
    without touching the triggers.
13. **Push acknowledgement.** The relay's push answer has no per-op positions. A `200` means every op
    of the batch is stored (accepted or a duplicate ID), so the batch leaves `sync_unsent`; the pull
    later judges each one `OwnAcknowledged`.
14. **Quarantine and the cursor (§9.3).** "Everything else keeps syncing" holds for applying; the
    persisted cursor stays before the first held envelope (ruling), and later rounds re-judge the
    envelopes after it (`SkipReplay` for what already applied).
15. **Account and vault secrets before `enabled = 1` (§11.2).** The Kit (step 3) needs the vault ID
    and key before the vault exists, and a person who pays and quits must find the same account on
    the next start. Both are stored in `sync_state` with `enabled = 0` as soon as they exist.
16. **Capture-broken sessions persist.** `on_open` failing records `sync_state.capture_broken_at`
    (new column); sync stays stopped until a repair (re-join by replace after a backup) or until sync
    is turned off, not only for that session (phase 1 ruling).
17. **Relay entitlement in development.** The relay has no way to mark an account paid without
    Polar; Task 17 adds a development-only grant endpoint to Moony-relay.
18. **Timeouts.** `services/http.rs` bounds every request at 15 s. Snapshot `PUT`/`GET` (up to
    100 MiB) override it per request with 600 s.
19. **Pull before push is kept as the spec states it (§9.3, ruling P1).** A round pushes only when
    its pull ended with no stop, with only held (quarantined) envelopes left, or with `NewerSchema`
    (§14: an older device keeps pushing). After `Gap`, `Invalid`, an undecryptable envelope, a merge
    stop, or a re-join that did not finish, the round does not push: a clone whose pull stops before
    its own duplicate would otherwise fork its chain. (The first draft's plan's call P7 is withdrawn.)
20. **When a snapshot is taken (§8, rulings P4, P14).** `S` is the cursor after a pull that ended
    with no stop and nothing held, with `sync_outbox` and `sync_unsent` empty. A round that pushed
    pulls once more before dumping, so `S` covers its own envelopes; the first snapshot of a vault
    is therefore at the relay head after setup's push (spec §8 "after a pull that returned
    `more = false`").
21. **§11.2 step 4 order.** Setup creates the vault, enables capture, records the device, pushes, pulls
    and only then uploads the first snapshot, so the snapshot holds the device row and the rule packs.
22. **D16 older key epochs in the snapshot** are not built: there is one epoch (0) until phase 5,
    which adds the back-links.
23. **Schema number.** Migration 030 is Local-only but raises `schema_number()` to 30 (phase 1 rule:
    the count of migrations), so a 029 build answers `NewerSchema` to this build's envelopes.
    Recorded in ADR 0025; no change.

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-sync` on branch
  `feat/device-sync-phase-2`; run `git branch --show-current` before every commit. Never touch
  `/Users/filipkral/Programování/FinanceApp/Moony-tauri`. Task 17 alone works in
  `/Users/filipkral/Programování/FinanceApp/Moony-relay` on a new branch `feat/dev-grant`.
- Desktop only. No pairing, QR scanning, phone UI, device removal, rekey, attachments or prices.
- Nothing visible to users by default: `services::sync::feature::enabled()` is
  `RELEASED || (cfg!(debug_assertions) && env MOONY_DEV_SYNC == "1")` with `RELEASED = false`. Every
  sync command except `sync_feature_enabled` returns `validation.syncUnavailable` when it is false,
  and the engine never starts.
- G5: a profile without sync starts no engine task, makes no network call and costs one `SELECT`.
- The engine never holds the `Database` mutex across `.await` on the network.
- Ten rules of `AGENTS.md`: no `invoke()` outside `src/lib/tauri-api.ts`; the 7-step procedure of
  `docs/standards/type-contract.md` for every command and wire type; per-field
  `#[serde(rename = "camelCase")]`; thin commands, logic in `services/`, `AppError` only at the
  command boundary; no `.unwrap()` outside tests (slice-to-array conversions use
  `try_into().map_err(..)`); migration 030 is append-only and 029 is never edited; timestamps are
  unix seconds `INTEGER`.
- **Rule 8 per task (ruling P13).** A task that introduces a user-facing string or a
  `validation.sync*` key adds it to all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`,
  `ja`, `zh`) in that task, with `npx vitest run src/i18n/` in its gates. Task 4 adds the test
  `every_sync_validation_key_has_an_english_translation` and the five phase 1 keys; every later key
  lands with the task that first emits it.
- **Wire types (ruling P15).** A task that registers a type in `bindings.rs` runs
  `cd src-tauri && cargo test generate_bindings -- --ignored`, mirrors it in `shared/schema.ts` and
  commits the regenerated `shared/generated-types.ts` in the same commit.
- Crypto (D17): XChaCha20-Poly1305; keys by HKDF-SHA256, 32-byte outputs.
  - op key = HKDF(ikm = vault_key[epoch], salt = vault_id (16 raw bytes), info = `moony sync ops v1`)
  - vault token = base64url-no-pad(HKDF(vault_key[epoch], vault_id, `moony sync token v1`)), 43 chars
  - snapshot stream key = HKDF(vault_key[epoch], salt = stream salt (32 random bytes), info =
    `moony sync snapshot v1`)
  - envelope = `"MNY1" | epoch u32 BE | nonce (24 random) | ciphertext+tag`; plaintext = raw
    deflate(changeset JSON) zero-padded to a multiple of 1,024 (minimum 1,024)
  - op ID = first 32 characters of base64url-no-pad(SHA-256(envelope)); chain hash = SHA-256(envelope)
  - stream chunks: 65,536 plaintext bytes; nonce = prefix (19) | counter u32 BE | last flag (0x00 /
    0x01); exactly one last chunk, possibly empty; nothing after it
- **Secrets hygiene (ruling P16).** `Credentials`, `VaultId`, `VaultKeys`, `VaultAuth` and
  `RecoveryKit` have no derived `Debug`: either none or a hand-written one that prints
  `<redacted>` for every secret field. The vault-keys JSON and the token are `Zeroizing<String>`. The
  binding comparison uses `subtle::ConstantTimeEq`. Never log account numbers, vault IDs, tokens,
  keys, recovery codes, relay URLs with paths, or `MissingParent` details above `debug`.
- **HTTP (ruling P7).** Relay calls use a client without redirects; error strings carry no URL
  (`reqwest::Error::without_url`); response bodies are capped (JSON 64 KiB, a pull page 6 MiB, a
  snapshot 104,857,600 bytes); the base URL is parsed, not prefix-matched.
- Sizes: envelope overhead 48 bytes; relay op limit 196,608 bytes (192 KiB); push ≤ 100 ops and
  ≤ 3 MiB of base64 per request; pull limit 500; snapshot ≤ 104,857,600 bytes sealed; decoded
  changeset JSON ≤ 4 MiB; inflated snapshot ≤ 512 MiB.
- Engine timing: debounce poll 5 s, forced after 60 s; periodic round every 300 s; back-off 30 s
  doubling to 1,800 s, reset by a clean round; snapshot policy checked at most hourly, due at
  2,000 envelopes or 8 MiB since the vault's newest snapshot, at most once per 24 h per device,
  after a random delay of 0–3,600 s.
- Relay: default base URL `https://sync.moonyfinance.com` (placeholder until phase 6); debug builds
  read `MOONY_RELAY_URL`; `http://` is allowed only for the hosts `localhost` / `127.0.0.1`. The URL
  used to create the account is stored in `sync_state.relay_url` and used for that profile from then
  on.
- Events: `sync-data-changed` (no payload; `merge::DATA_CHANGED_EVENT`) and `sync-status-changed`
  (payload `SyncStatus`).
- No prices in the app (§16a): plans read "Monthly" / "Yearly"; Polar's page shows the amount.
- Sync files next to the profile — `sync-binding.bin`, `sync-rejoin.marker`, `sync-repair-*.zip` —
  are removed by Delete Account (`auth::remove_account_files`) and denied in the `fs:scope` of
  `capabilities/default.json` (ruling P8).
- Tests: Rust services use `services::sync::test_support::migrated()` (in-memory, real migrations);
  never the `Database` struct. Frontend tests are pure functions in node (no jsdom). A measurement
  is `#[ignore]`d, prints its numbers and is not counted as a test.
- Every task ends with its filter test, then, from `src-tauri/`: `cargo test --lib`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; tasks that touch the frontend or
  a locale also run `npm test`, `npm run typecheck`, `npm run lint`. Never `--no-verify`.
- New functions a later task calls are `pub` (a `pub(crate)` item unused until then fails clippy).
- Files stay under ~800 lines. Existing files already over it (`db/migrations.rs`,
  `services/backup.rs`, `services/auth.rs`, `src/lib/tauri-api.ts`, `shared/schema.ts`, the locale
  files) only get small additions.
- Commit messages: Conventional Commits, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **The app is locked, quit or loses the network between storing a sealed batch and the relay's
   answer** (the relay stored it, the answer never came): the next round re-pushes the same op IDs,
   the relay ignores them, the pull acknowledges them, and the chain never forks — Task 10
   `a_push_whose_answer_was_lost_is_acknowledged_by_the_next_pull`.
2. **A person pays, then quits before saving the Recovery Kit:** the next start resumes with the
   same account and the same vault secrets, no second account — Task 16
   `setup_resumes_with_the_stored_account_and_vault_after_a_restart`.
3. **A large local change set** (a 5,000-row statement import with sync on): the push respects the
   relay's per-request limits and drains in order — Task 10 `push_batches_respect_the_relay_limits`.
4. **A captive portal or proxy answers with HTML, truncated JSON or a redirect:** a typed transient
   error, a back-off and an "offline / relay error" status, never a panic, a stop, or the account
   header sent elsewhere — Task 6 `an_html_answer_is_a_protocol_error`, `a_redirect_is_not_followed`.
5. **The profile is locked in the middle of a round:** the engine ends quietly (no error status, no
   toast), and the next unlock starts a fresh engine — Task 13
   `a_locked_session_ends_the_loop_without_an_error`.

---

## Plan's calls

| # | Call |
|---|---|
| P1 | Runtime flag, not a Cargo feature: everything compiles and is tested in every build; `RELEASED` const flips in phase 6; developers set `MOONY_DEV_SYNC=1` (debug builds only). |
| P2 | Generate the device X25519 key pair now (correction 4); stored in `sync_state.device_secret_key`, public half in this device's `sync_devices` row. |
| P3 | Vault keys live in `sync_state.vault_keys` as JSON `{"<epoch>": "<base64url key>"}`; `key_epoch` is the current epoch. |
| P4 | Recovery code checksum = the first 15 bits of SHA-256(`moony recovery code v1` ‖ vault_id ‖ key), 3 Crockford characters; the 384-bit payload is padded with one zero bit to 77 characters. A typo guard, not an authenticator (doc comment says so). |
| P5 | Recovery Kit as printable HTML + plain text (correction 11); QR content `moony-recover:1:<account>:<code>`, SVG rendered in Rust. |
| P6 | Local-write trigger by outbox polling (correction 12). |
| P7 | *Withdrawn by ruling P1* (see correction 19). |
| P8 | Back-off applies to `SyncError::{Transient, RateLimited, Local}`; every other class becomes a problem in the round report with no back-off (the 300 s periodic round retries). "Sync now" bypasses the back-off. |
| P9 | `404 vault_not_found` persists a pause (`vault_gone_at`); "Sync now" retries one `GET` vault and clears it on success; "Turn off" ends it. |
| P10 | `403 token_read_only` and `401` stop the round with problem `keyRotated` / `unauthorized`; their resolution is phase 5. |
| P11 | Quarantine is shown (`heldDevices`) but not resolved in phase 2; resolution comes with the device list (phase 3). |
| P12 | The capture-broken repair (backup → clean drain round → join by replace under a new device ID) is built now, since the join core is needed for the end-to-end test anyway. |
| P13 | The keychain binding writes a hash `SHA-256("moony sync binding v1" ‖ device_id)`; `sync_state.binding_written` tells "never written here" from "missing on this machine". |
| P14 | A re-join copies the name and platform of the old device row into the new one; the old row stays (it may still be the original's ID). |
| P15 | Device names come from the UI (prefilled with a translated platform default, editable, 1–60 characters). |
| P16 | `sync-data-changed` invalidates every query (a merge can touch any table), records today's snapshot and invalidates `["portfolio-history"]`. |
| P17 | Status phases: `off`, `settingUp`, `idle`, `syncing`, `paused` (vault gone, lapsed, quota, newer schema, offline, rate limited), `error` (everything else). |
| P18 | The engine runs short DB steps directly in its tokio task (as `mcp_refresh` does); a pull page is at most 500 envelopes, each merged in its own short transaction; the snapshot dump runs in `spawn_blocking` (it holds the session mutex for the whole dump, memory about twice the sealed size, at most once a day). |
| P19 | The outbox re-sort cost (phase 1 note) is measured, not optimised: Task 10's ignored measurement prints the time to drain a 50,000-row outbox; above 30 s in release a follow-up caches the order. |
| P20 | Phone capture overhead is measured with a debug-only command and a production fixture (`capture/bench.rs`: plain inserts into `bank_transactions` in one transaction, with and without capture), run from the Web Inspector console; optimising triggers is a follow-up only if 5,000 rows cost over 1 s more with capture on the device. The phase 1 statement-import measurement stays test-only (ruling P10, option 2). |
| P21 | Two-desktop manual testing waits for phase 3's recovery-code join; phase 2's two-device proof is the end-to-end Rust test against `wrangler dev`. |
| P22 | `SyncError` adds one class to the ruling's list, `Refused(RelayError)`, for relay answers that are neither transient nor a sync state (400, 409, `no_snapshot`, `account_not_found`, `vault_limit_reached`, checkout unavailable). |
| P23 | A held envelope blocks the snapshot: "pull ended Done" means no stop **and** nothing held. |
| P24 | The repair backup `sync-repair-<unix>.zip` stays after a repair (it is the person's backup of the replaced data, and the success toast names it); Delete Account removes it. Turning off or deleting synced data removes the binding and the marker, not the zip. |
| P25 | The "Copy code" clipboard is cleared natively (Task 14) — see the pushback on ruling P16 below. |
| P26 | Inflated snapshot cap 512 MiB (`validation.syncSnapshotInvalid` above it). |

### Pushback on ruling P16 (clipboard), and the safe alternative

"The UI clears the clipboard 60 s after Copy code if it still holds the code" cannot be done safely
in the webview. To know whether the clipboard still holds the code the UI must call
`navigator.clipboard.readText()`. In WKWebView that call, made from a timer without a user gesture,
is either refused or, when another app wrote the pasteboard in the meantime, shows the system
"Paste" callout to the person 60 s later out of nowhere. Clearing without checking (writing `""`)
destroys whatever the person copied in another app during that minute. Closest safe alternative
(Task 14, Task 19): the code never crosses IPC for copying. `sync_copy_recovery_code` writes it to
the system clipboard natively (macOS `NSPasteboard` with the concealed type
`org.nspasteboard.ConcealedType`; Windows `CF_UNICODETEXT` plus the
`ExcludeClipboardContentFromMonitorProcessing` format), remembers the pasteboard change count /
clipboard sequence number, and after 60 s clears the clipboard only if that number is unchanged.
Linux answers "unsupported": the UI then copies with `navigator.clipboard.writeText` and shows one
line, "Clear your clipboard after pasting the code."

## Dependencies

Added by Task 1 to `src-tauri/Cargo.toml` under `# Cryptography` (checked 2026-11-07 with
`cargo search` and a scratch resolution: one RustCrypto family with `aead 0.6`, `cipher 0.5`,
`digest 0.11`, `rand_core 0.10`; no duplicate versions; already applied in the worktree):

```toml
chacha20poly1305 = { version = "0.11", default-features = false, features = ["alloc", "zeroize"] }  # Device sync envelopes and snapshot streams (XChaCha20-Poly1305, spec §9.2)
hkdf = "0.13"                    # Device sync key derivation (already in Cargo.lock through another crate)
x25519-dalek = { version = "3", features = ["static_secrets"] }  # Device sync: this device's key pair (spec §11.1)
qrcode = { version = "0.14", default-features = false, features = ["svg"] }  # Sync Recovery Kit QR (SVG only)
```

New `Cargo.lock` packages: `chacha20poly1305 0.11.0`, `poly1305 0.9.1`, `x25519-dalek 3.0.0`,
`curve25519-dalek 5.0.0` (plus its target-specific helpers), `qrcode 0.14.1`. `hkdf 0.13.0`,
`chacha20 0.10.2`, `aead 0.6.1`, `sha2 0.11.1`, `subtle 2.6.1`, `zeroize 1.9.1` are already locked.
Nonces and keys come from `rand 0.10` (`rand::rng().fill_bytes`), so `chacha20poly1305`'s
`getrandom` feature stays off. Task 14 adds the `windows` features `Win32_Security_Cryptography`,
`Win32_System_DataExchange`, `Win32_System_Memory` and `Win32_System_Ole` to
`plugins/moony-keychain/Cargo.toml`. `cargo audit` (`scripts/cargo-audit.sh`) must stay green.

## Phase 1 carries and where they land

| Carry (phase 1 record) | Task |
|---|---|
| Capture-broken sessions: no push/pull, Settings shows it, recovery = re-join by replace after a backup (progress L61) | 3 (column), 13 (stop), 15 (repair), 22 (UI) |
| Locale entries for `validation.sync*` / `validation.syncSnapshot*` (L67) | 4 (with the key test, ruling P13) |
| Raw deflate in the envelope, same as `padded_deflated_len` (L68) | 1 |
| Changeset chain shape (`seq` 1 with `prev`, `seq` > 1 without, duplicate `devs`) (L70) | done in phase 1 (`from_json`, `judge` → `Invalid`); 9 stops on `Invalid` without moving the cursor, 13 does not push after it |
| Outbox re-sort O(n²/k) at 50k rows (L81, phase 2 note) | 10 (measurement, P19) |
| Barrier placement by relay order: record the marker's relay position, `BelowBarrier` only beyond it (L130) | 8 (judge), 10 (one marker per barrier) |
| Clone → re-join before any push in the round; re-judge the triggering envelope; `Gap`/`Invalid`/`NewerSchema` stop without advancing the cursor; `Held` keeps the cursor; no old-ID push in flight (L131) | 9 (verdicts), 13 (serial rounds; no push after a stopped pull, correction 19) |
| Empty confirmed prefix with trigger seq > 1 writes no head (L136) | 8 |
| Merge stop reason collapsed to one key on restore (L142, phase 2 note) | 11 |
| Re-join / barrier re-queues never ship a child ahead of its parent's incarnation (L170) | 8 (tests through `build_next`) |
| Joins start the cursor at the snapshot seq (L173) | 15 |
| Dump with exclusive connection use (L176; T18e "engine pushes and pulls before dumping") | 11 (dump under the mutex), 13 (dump only after a clean pull, correction 20) |
| Capture overhead profiled on a phone (L202) | 19 (command), 23 (measurement, P20) |
| Re-join re-stamps re-queued own rows with fresh clocks under the new ID (L203) | 8 (correction 7) |
| Registry test for fixed seed values after 029 (L209) | 3 |
| `build_next` + seal + store in `sync_unsent` in one transaction (R11) | 10 |

## Pre-flight rulings and where they land

| Ruling | Task(s) |
|---|---|
| P1 pull before push | correction 19; 13 (`run_round` push gate, tests); 9 (Clone → no push until the re-join finished) |
| P2 typed `SyncError` | 6 (defined), 9–13 and 15–16 (returned), 19 (mapped to keys) |
| P3 clean drain before repair | 15 |
| P4 first snapshot after push → pull | correction 20; 13 (re-pull before dump); 16 (test expects `S` = relay head) |
| P5 one marker per barrier | 3 (`marker_barrier`), 10 |
| P6 `open_sealed`, `maybe_upload`/`run_round` signatures | 11, 13, 15, 16 |
| P7 HTTP hardening | 6 (and `services/http.rs`) |
| P8 sync files on Delete Account, fs scope | 12 (binding, marker), 15 (repair zip), 16 (turn off / delete synced data) |
| P9 `SyncSection` only with existing cards | 20, 21, 22 |
| P10 bench | 19 (production fixture, Create), P20 |
| P11 weak tests | 1, 5, 10 |
| P12 split `chain.rs`, stale comment | 8 |
| P13 rule 8 per task | Global Constraints; 4, 5, 6, 15, 16, 19, 20, 21, 22 |
| P14 upload only after a clean pull | 11, 13 |
| P15 `generate_bindings` per type task | 13, 16, 19 |
| P16 secrets hygiene | Global Constraints; 4, 6, 12, 14, 16, 19, 21 (pushback on the clipboard part) |
| P17 backup carries vault keys (ADR) | 23 |
| P18 housekeeping | 1 (Step 1 already done), 18 (`dev_grant` debug-only), 23 (ledger path, schema-number note) |

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/envelope/mod.rs` | HKDF labels, op envelope seal/open, op ID, hash |
| `services/sync/envelope/stream.rs` | STREAM sealer (`Write`) and opener for snapshots |
| `services/sync/envelope/tests.rs` | Known-answer, tamper and key-separation tests |
| `services/sync/state.rs` (modify) | New `sync_state` columns: barrier marker, marker flag, pauses, policy counters, binding flag |
| `services/sync/keys.rs` | `VaultId`, `VaultKeys`, `Credentials`, `VaultAuth`, device key pair, their storage |
| `services/sync/devices.rs` | This device's `sync_devices` row |
| `services/sync/recovery.rs` | Recovery code (Crockford base32 + checksum), QR payload and SVG |
| `services/sync/error.rs` | `SyncError`, `SyncResult`, mapping to `AppError` |
| `services/sync/relay/mod.rs` | `Relay` trait, wire types, `RelayError` and its status mapping |
| `services/sync/relay/http.rs` | `HttpRelay` over `services/http.rs`, base URL rules, body caps |
| `services/sync/relay/fake.rs` (`#[cfg(test)]`) | `FakeRelay` with the relay's semantics and fault knobs |
| `services/sync/chain/mod.rs`, `chain/rejoin.rs` (split from `chain.rs`) | Verdicts, relay positions, barrier marker; re-join, empty-prefix head, re-stamp |
| `services/sync/journal/mod.rs` (modify) | `build_marker`; budget comment |
| `services/sync/snapshot/{mod,stream}.rs` (modify) | Header `barrierSeq`; dump doc line |
| `services/sync/engine/mod.rs` | `ConnAccess`, `EngineEvents`, `run_round`, `RoundMode`, `RoundReport` |
| `services/sync/engine/pull.rs` | Pull pages, per-envelope verdict handling, re-join |
| `services/sync/engine/push.rs` | Seal batches into `sync_unsent`, push, acknowledge, marker |
| `services/sync/engine/snapshots.rs` | `open_sealed`, `410` restore by merge, upload policy, sealed dump |
| `services/sync/engine/runtime.rs` | Loop, triggers, back-off, start/stop, startup checks |
| `services/sync/engine/status.rs` | `SyncStatus` from state + runtime |
| `services/sync/engine/tests/*.rs` (`#[cfg(test)]`) | Engine tests with `FakeRelay` |
| `services/sync/binding.rs` | Restore marker, keychain binding check and write |
| `services/sync/join.rs` | Join a vault by replace; capture-broken repair |
| `services/sync/setup.rs` | Account, checkout, Kit, finish setup, turn off, delete synced data |
| `services/sync/feature.rs` | The hidden flag |
| `services/sync/relay_e2e.rs` (`#[cfg(test)]`, ignored) | Two devices through `wrangler dev` |
| `services/sync/capture/bench.rs` (new) | Production fixture for the debug capture measurement |
| `src-tauri/src/models/sync.rs` | `SyncStatus`, `SyncPhase`, `SyncProblem`, `SyncAccount`, `SyncPlan`, `RecoveryKit` |
| `src-tauri/src/commands/sync.rs` | Thin commands, engine start/stop glue, Tauri adapters, clipboard timer |
| `plugins/moony-keychain/…` | Binding seal/open and clipboard natives (macOS, Windows) |
| `src-tauri/capabilities/default.json` (modify) | `fs:scope` deny for the sync files |
| `src/lib/tauri-api.ts` (modify), `shared/schema.ts` (modify) | `deviceSyncApi`, wire types |
| `src/hooks/use-device-sync.ts`, `use-device-sync-mutations.ts`, `use-device-sync-events.ts` | Queries, mutations, event listener |
| `src/lib/device-sync-refresh.ts` (+ test) | The rule-7 refresh after remote changes |
| `src/utils/device-sync-indicator.ts` (+ test), `src/utils/recovery-kit.ts` (+ test) | Pure UI logic |
| `src/components/settings/sync/*.tsx`, `src/components/shell/SyncStatusIndicator.tsx` | UI |
| `src/i18n/locales/<10>/deviceSync.json`, `common.json`, `settings.json` (modify) | Copy |
| `../Moony-relay/src/dev.ts`, `src/index.ts`, `test/dev-grant.test.ts`, docs | Dev grant (Task 17) |

---

### Task 1: Op envelopes — dependencies, key derivation, seal/open, op ID

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, `src-tauri/src/services/sync/mod.rs` (`pub mod envelope;`)
- Create: `src-tauri/src/services/sync/envelope/mod.rs`, `src-tauri/src/services/sync/envelope/tests.rs`

**Interfaces:**
- Consumes: `changeset::padded_deflated_len(json: &[u8]) -> Result<usize>` (phase 1).
- Produces (`services::sync::envelope`):

```rust
pub const MAGIC: [u8; 4] = *b"MNY1";
pub const OPS_INFO: &[u8] = b"moony sync ops v1";
pub const TOKEN_INFO: &[u8] = b"moony sync token v1";
pub const SNAPSHOT_INFO: &[u8] = b"moony sync snapshot v1";
pub const PAD: usize = 1024;
pub const ENVELOPE_OVERHEAD: usize = 48;
pub const MAX_OP_BYTES: usize = 196_608;
pub const MAX_CHANGESET_JSON: usize = 4 * 1024 * 1024;

pub fn hkdf32(ikm: &[u8; 32], salt: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]>;
pub fn vault_token(vault_key: &[u8; 32], vault_id: &[u8; 16]) -> String;
pub fn compress_padded(json: &[u8]) -> Result<Vec<u8>>;                 // raw deflate, zero pad to 1 KiB
pub fn seal_op(epoch: u32, vault_key: &[u8; 32], vault_id: &[u8; 16], json: &[u8]) -> Result<Vec<u8>>;
pub fn open_op(key_for: impl Fn(u32) -> Option<Zeroizing<[u8; 32]>>, vault_id: &[u8; 16],
               envelope: &[u8]) -> std::result::Result<Vec<u8>, OpenError>;   // returns the JSON
pub fn envelope_hash(envelope: &[u8]) -> [u8; 32];
pub fn op_id(envelope: &[u8]) -> String;
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError { #[error("malformed")] Malformed, #[error("unknown epoch {0}")] UnknownEpoch(u32),
                     #[error("authentication failed")] Authentication, #[error("bad padding")] Padding,
                     #[error("too large")] TooLarge }
```

`seal_op` = `seal_op_with_nonce(.., nonce: [u8; 24])` (private, used by tests) with a random nonce
from `rand::rng().fill_bytes`. `open_op` checks length ≥ 48 + 1,024 and ≤ `MAX_OP_BYTES`, magic, epoch,
decrypts, inflates with `flate2::Decompress::new(false)` until `StreamEnd` (output capped at
`MAX_CHANGESET_JSON` → `TooLarge`), and requires every byte after the deflate stream to be zero
(`Padding`). Keys are `Zeroizing`. (`vault_token` returns a plain `String`; Task 4 wraps it in
`Zeroizing` where it is kept.)

- [x] **Step 1: Add the dependencies** of the Dependencies section — already applied in the worktree by the running Task 1 (ruling P18); `cargo build` resolves them; diff `Cargo.lock` shows only the listed new packages.

- [ ] **Step 2: Write the failing tests** in `envelope/tests.rs`:

```rust
const KEY: [u8; 32] = [0x11; 32];
const VID: [u8; 16] = [0x22; 16];

#[test] fn derivations_match_frozen_vectors() {
    assert_eq!(hex::encode(*hkdf32(&KEY, &VID, OPS_INFO)),
        "09f78d58938f72bc6f179f74097299ca56abab13c933c18f7f684e127d66ce03");
    assert_eq!(vault_token(&KEY, &VID), "xEoN5KY965vOsERjjinYPzJYkS-KXnBEs1SCP_enuvQ");
    assert_eq!(hex::encode(*hkdf32(&KEY, &[0x33; 32], SNAPSHOT_INFO)),
        "45f7a1d21bc1b4cf86eb4983adfe38ad5e638139b58c2152c674fce8ee0021e0");
}   // computed with Python's hmac/hashlib HKDF (RFC 5869), independent of the hkdf crate
#[test] fn op_id_is_the_first_32_base64url_characters_of_the_sha256() {
    assert_eq!(op_id(b"example envelope"), "cI0pGQzv4c-0fC9x6sZvDPiCRmqc-xG0");
}
#[test] fn xchacha_usage_matches_the_draft_vector()   // draft-irtf-cfrg-xchacha-03 §A.3.1, copied from the draft
#[test] fn envelope_layout_is_magic_epoch_nonce_ciphertext() {
    // seal_op_with_nonce(7, &KEY, &VID, json, [0x44; 24]): bytes 0..4 == b"MNY1", 4..8 == 7u32 BE,
    // 8..32 == [0x44; 24], len == 48 + compress_padded(json).len(); decrypting bytes 32.. directly with
    // XChaCha20Poly1305(op key) and AAD b"MNY1" | 7u32 BE | VID gives compress_padded(json).
}
#[test] fn padded_plaintext_length_matches_the_journal_estimate() {
    // for several JSONs (empty rows, 70 KiB of incompressible base64): compress_padded(j).len()
    // == changeset::padded_deflated_len(j).unwrap()   (phase 1 carry: raw deflate)
}
#[test] fn seal_then_open_round_trips_and_nonces_differ()
#[test] fn every_flipped_byte_fails_to_open()         // flip each byte of a sealed 1 KiB envelope → Err (Authentication, Malformed or UnknownEpoch)
#[test] fn wrong_vault_id_or_epoch_fails_authentication()
#[test] fn unknown_epoch_is_reported()
#[test] fn non_zero_padding_is_refused()              // re-encrypt a plaintext whose pad has a 0x01 byte → Padding
#[test] fn oversize_inflation_is_refused()            // 5 MiB of zeros deflated → TooLarge
#[test] fn the_largest_single_row_envelope_fits_the_relay_limit() {
    // ruling P11: no constant assertion. Build a JSON of incompressible base64 (seeded StdRng) whose
    // compress_padded(json).len() == journal::SINGLE_ROW_LIMIT (184,320; search the payload length);
    // let env = seal_op(0, &KEY, &VID, &json).unwrap();
    // assert_eq!(env.len(), journal::SINGLE_ROW_LIMIT + ENVELOPE_OVERHEAD);   // 184,368
    // assert!(env.len() <= MAX_OP_BYTES);
    // assert_eq!(open_op(|e| (e == 0).then(|| Zeroizing::new(KEY)), &VID, &env).unwrap(), json);
}
```

- [ ] **Step 3: Run** `cd src-tauri && cargo test --lib services::sync::envelope` → FAIL (module missing).
- [ ] **Step 4: Implement** `envelope/mod.rs` per the Interfaces block (`chacha20poly1305::XChaCha20Poly1305`, `aead::{Aead, KeyInit, Payload}`, `hkdf::Hkdf<sha2::Sha256>`).
- [ ] **Step 5: Run** the filter → PASS; then the four gates.
- [ ] **Step 6: Commit** `feat(sync): sealed op envelopes with HKDF keys and op IDs`.

---

### Task 2: STREAM for snapshots

**Files:**
- Create: `src-tauri/src/services/sync/envelope/stream.rs`; Modify: `envelope/mod.rs` (`pub mod stream;`), `envelope/tests.rs`

**Interfaces:**
- Produces (`envelope::stream`):

```rust
pub const CHUNK: usize = 65_536;
pub const HEADER_LEN: usize = 59;
pub struct StreamSealer { /* key, prefix, counter, buffer, out: Vec<u8> */ }
impl StreamSealer {
    pub fn new(epoch: u32, vault_key: &[u8; 32], vault_id: &[u8; 16], object_id: &[u8]) -> Self;
    pub fn finish(self) -> Result<Vec<u8>>;                // emits the last chunk
}
impl std::io::Write for StreamSealer { /* buffers, seals every full CHUNK */ }
pub fn open_stream(key_for: impl Fn(u32) -> Option<Zeroizing<[u8; 32]>>, vault_id: &[u8; 16],
                   object_id: &[u8], sealed: &[u8]) -> std::result::Result<Vec<u8>, OpenError>;
pub fn snapshot_object_id(seq: u64) -> [u8; 8];             // seq.to_be_bytes()
```

`new` draws the 32-byte salt and the 19-byte prefix from `rand::rng()`; a private
`new_with(epoch, key, vid, object_id, salt, prefix)` serves the tests. `open_stream` refuses a
missing last chunk, a chunk after the last one, and a sealed length that is not header + whole
chunks (`Malformed`). The chunk counter is a `u32`: under the 100 MiB cap it reaches 1,601 at most;
a `debug_assert!` guards overflow and the sealer returns `Internal` past `u32::MAX` chunks.

- [ ] **Step 1: Failing tests** (in `envelope/tests.rs`):

```rust
#[test] fn stream_round_trips_empty_one_byte_exact_chunk_and_multi_chunk_inputs() // 0, 1, 65_536, 200_001 bytes
#[test] fn stream_header_layout_is_frozen() {
    // new_with(3, &KEY, &VID, &snapshot_object_id(42), [0x33; 32], [0x55; 19]) on b"": bytes 0..4 "MNY1",
    // 4..8 == 3u32 BE, 8..40 == [0x33; 32], 40..59 == [0x55; 19]; total len 59 + 16 (one empty last chunk);
    // the chunk decrypts with key hkdf32(&KEY, &[0x33; 32], SNAPSHOT_INFO), nonce [0x55;19]|0u32|0x01,
    // AAD b"MNY1"|3u32|VID|42u64 BE
}
#[test] fn two_streams_never_share_a_key() {
    // two StreamSealer::new over the same input: salts differ, and hkdf32(KEY, salt_a) != hkdf32(KEY, salt_b);
    // also the frozen vector: hkdf32(&KEY, &[0x34; 32], SNAPSHOT_INFO) ==
    // "ae037ce329d9c0b7aeb9ad37db5dcae61f3035a4b42dee2f9327411c8f805368"
}
#[test] fn truncated_stream_fails()             // drop the last chunk → Err
#[test] fn reordered_chunks_fail()               // swap chunks 0 and 1 of a 3-chunk stream → Authentication
#[test] fn appended_bytes_fail()
#[test] fn a_stream_opened_under_another_object_id_fails()   // sealed for seq 42, opened for 43 → Authentication
#[test] fn every_flipped_header_byte_fails()
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::envelope` → FAIL.
- [ ] **Step 3: Implement** `stream.rs`.
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): STREAM encryption for snapshots with a per-stream key`.

---

### Task 3: Migration 030 and the engine's `sync_state` bookkeeping

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (append `MIGRATION_030` + Vec entry), `src-tauri/src/db/schema.snapshot` (regenerated), `src-tauri/src/services/sync/registry/keys.rs` (`("030_sync_engine", A)`; rename test `schema_number_is_29_and_the_newest_barrier_is_025` → `schema_number_is_30_and_the_newest_barrier_is_025`, asserting 30 and 25), `src-tauri/src/services/sync/state.rs`, `src-tauri/src/services/sync/mod.rs` (`on_open` records the broken capture), `docs/architecture/database.md`

**Interfaces:**
- Produces:

```sql
-- MIGRATION_030 ("030_sync_engine"), Local columns only
ALTER TABLE sync_state ADD COLUMN barrier_seq INTEGER;
ALTER TABLE sync_state ADD COLUMN marker_barrier INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN capture_broken_at INTEGER;
ALTER TABLE sync_state ADD COLUMN vault_gone_at INTEGER;
ALTER TABLE sync_state ADD COLUMN pulled_bytes INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN pulled_bytes_base INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sync_state ADD COLUMN snapshot_uploaded_at INTEGER;
ALTER TABLE sync_state ADD COLUMN binding_written INTEGER NOT NULL DEFAULT 0 CHECK (binding_written IN (0, 1));
```

```rust
// state.rs
pub struct SyncState { /* existing fields */ pub barrier_seq: Option<u64> }
pub struct EngineMarks { pub marker_barrier: u32, pub capture_broken_at: Option<i64>,
    pub vault_gone_at: Option<i64>, pub pulled_bytes: u64, pub pulled_bytes_base: u64,
    pub snapshot_uploaded_at: Option<i64>, pub binding_written: bool,
    pub last_success_at: Option<i64>, pub last_error: Option<String> }
pub fn marks(conn: &Connection) -> Result<Option<EngineMarks>>;
pub fn mark_capture_broken(conn: &Connection, at: i64) -> Result<()>;
pub fn clear_capture_broken(conn: &Connection) -> Result<()>;
pub fn set_vault_gone(conn: &Connection, at: Option<i64>) -> Result<()>;
pub fn set_cursor(conn: &Connection, cursor: u64) -> Result<()>;          // never lowers: MAX(cursor, ?)
pub fn add_pulled_bytes(conn: &Connection, bytes: u64) -> Result<()>;
pub fn rebase_pulled_bytes(conn: &Connection, vault_snapshot_seq: u64) -> Result<()>; // base := seq, bytes := 0 when the base differs
pub fn set_barrier_seq(conn: &Connection, seq: Option<u64>) -> Result<()>;
pub fn note_sealed_at_barrier(conn: &Connection, barrier: u32) -> Result<()>;   // marker_barrier = MAX(marker_barrier, barrier)
pub fn record_snapshot_uploaded(conn: &Connection, seq: u64, max_hlc: u64, at: i64) -> Result<()>;
    // snapshot_seq = MAX, snapshot_hlc = MAX, snapshot_uploaded_at = at, pulled_bytes = 0, pulled_bytes_base = seq
pub fn record_success(conn: &Connection, at: i64) -> Result<()>;         // last_success_at, last_error = NULL
pub fn set_binding_written(conn: &Connection, written: bool) -> Result<()>;
pub fn reset_chain(conn: &Connection) -> Result<()>;  // next_seq = 1, last_hash = NULL, empties sync_own_log and sync_unsent
pub fn reset_engine_marks(conn: &Connection) -> Result<()>; // vault_gone_at, capture_broken_at, snapshot_uploaded_at = NULL; pulled_bytes(_base) = 0; binding_written = 0
```

`on_open`: when `open_capture` fails **and** the row is enabled, call `mark_capture_broken(now)` next
to `record_error`. `chain::cross_barrier` sets `barrier_seq = NULL` when it raises the barrier
(Task 8 tests it). `record_error` callers pass a fixed reason text, never an error that may carry a
URL or a key.

- [ ] **Step 1: Failing tests**

```rust
// db/migrations.rs, mod migration_030
#[test] fn sync_state_gains_the_engine_columns_with_defaults()
// state.rs
#[test] fn set_cursor_never_moves_back()
#[test] fn pulled_bytes_reset_when_the_vault_snapshot_moves() // rebase 10, +100, +50 → 150; rebase 10 → 150; rebase 20, +7 → 7
#[test] fn record_snapshot_uploaded_takes_maxima_and_resets_the_counter()
#[test] fn the_marker_flag_never_goes_back()                 // note 25, note 24 → 25
#[test] fn reset_chain_empties_the_own_log_and_unsent()
#[test] fn reset_engine_marks_clears_pauses_and_counters()
#[test] fn a_failed_capture_install_marks_the_profile_capture_broken()
    // enabled row, then make install fail (drop sync_outbox before on_open) → capture_broken_at is Some
// registry/keys.rs (phase 1 carry L209)
#[test] fn migrations_after_029_seed_synced_rows_with_literal_values() {
    // for every migration after 029: no INSERT/UPDATE into a Synced table uses unixepoch(, datetime(,
    // strftime(, julianday(, random(, randomblob(, CURRENT_TIMESTAMP, CURRENT_DATE (outside comments);
    // plus a self-test that the scanner flags "INSERT INTO institutions (id, created_at) VALUES ('x', unixepoch())"
    // and passes "INSERT INTO institutions (id, created_at) VALUES ('x', 1767225600)"
}
```

- [ ] **Step 2: Run** `cargo test --lib migration_030 services::sync::state services::sync::registry` → FAIL.
- [ ] **Step 3: Implement** the migration (playbook `docs/playbooks/add-db-migration.md`: Vec entry, class `A`, `cargo test regenerate_schema_snapshot -- --ignored`), the state functions, the scanner `pub fn seeds_with_volatile_values(sql: &str, synced: &[&str]) -> bool` in `registry/keys.rs`, and the `database.md` row change.
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): migration 030 with the engine's sync_state bookkeeping`.

---

### Task 4: Keys, credentials, this device's identity, and the sync key test

**Files:**
- Create: `src-tauri/src/services/sync/keys.rs`, `src-tauri/src/services/sync/devices.rs`
- Modify: `services/sync/mod.rs`, `src-tauri/src/error.rs` (test), `src/i18n/locales/<10>/common.json` (`validation` block)

**Interfaces:**
- Consumes: `envelope::vault_token`, `state::ensure_row`.
- Produces:

```rust
// keys.rs — none of these types derives Debug; a hand-written Debug prints "<redacted>" (ruling P16)
#[derive(Clone, Copy, PartialEq, Eq)] pub struct VaultId(pub [u8; 16]);
impl VaultId { pub fn random() -> Self; pub fn to_b64(&self) -> String; pub fn from_b64(s: &str) -> Result<Self>; }
#[derive(Clone)] pub struct VaultKeys { /* current: u32, keys: BTreeMap<u32, Zeroizing<[u8; 32]>> */ }
impl VaultKeys {
    pub fn new_random() -> Self;                                  // epoch 0
    pub fn from_parts(current: u32, keys: BTreeMap<u32, [u8; 32]>) -> Result<Self>;
    pub fn current_epoch(&self) -> u32;
    pub fn current(&self) -> &[u8; 32];
    pub fn get(&self, epoch: u32) -> Option<Zeroizing<[u8; 32]>>;
}
#[derive(Clone)] pub struct VaultAuth { pub vault_id: String, pub token: Zeroizing<String> }
#[derive(Clone)] pub struct Credentials { pub relay_url: String, pub account: String, pub vault_id: VaultId, pub keys: VaultKeys }
impl Credentials {
    pub fn token(&self) -> Zeroizing<String>;                     // vault_token(current key, vault id)
    pub fn auth(&self) -> VaultAuth;                              // the one place a VaultAuth is built
}
pub fn save_account(conn: &Connection, relay_url: &str, account: &str) -> Result<()>;
    // creates the row with enabled = 0 and a random device ID when absent
pub fn account(conn: &Connection) -> Result<Option<(String, String)>>;  // (relay_url, account)
pub fn ensure_vault_secrets(conn: &Connection) -> Result<(VaultId, VaultKeys)>; // generates once; needs the account
pub fn save_credentials(conn: &Connection, creds: &Credentials) -> Result<()>;   // join path
pub fn credentials(conn: &Connection) -> Result<Option<Credentials>>;
pub fn ensure_device_keypair(conn: &Connection) -> Result<[u8; 32]>;   // returns the public key
pub fn new_device_keypair(conn: &Connection) -> Result<[u8; 32]>;      // re-join
// devices.rs
pub fn platform() -> &'static str;                                     // "macos" | "windows" | "linux" | "ios" | "android"
pub fn validate_device_name(name: &str) -> Result<()>;                 // 1..=60 chars after trim → validation.syncDeviceNameInvalid
pub fn record_this_device(conn: &Connection, name: &str, public_key: &[u8; 32], now: i64) -> Result<()>;
    // INSERT … ON CONFLICT(id) DO UPDATE SET name, platform, app_version; never touches public_key
pub fn this_device(conn: &Connection) -> Result<Option<(String, String)>>; // (name, platform) of the current ID
pub fn device_row(conn: &Connection, device: DeviceId) -> Result<Option<(String, String)>>;
```

Storage (P3): `vault_id` TEXT base64url, `key_epoch`, `vault_keys` JSON built and parsed as
`Zeroizing<String>`, `device_secret_key` BLOB (32 bytes, `x25519_dalek::StaticSecret` from random
bytes; public = `PublicKey::from(&secret)`). `sync_devices`: `id` = 32 lowercase hex, `public_key`
base64url, `app_version` = `env!("CARGO_PKG_VERSION")`, `joined_at` = now.

A row with `enabled = 0` now exists between account creation and setup. Before writing code, grep
every phase 1 caller of `state::load` (`rg "state::load|loaded\(" src-tauri/src/services/sync`) and
confirm each either checks `enabled` or runs only on an enabled profile (`on_open`/`open_capture`
check it; `build_next`, `chain::*`, `snapshot::*` run only from the engine, which requires
`enabled`); note the result in the commit body.

Locale keys (ruling P13), added to `common.json` `validation` in all ten locales:
`syncChangesetInvalid`, `syncSnapshotBelowBarrier`, `syncSnapshotInvalid`, `syncSnapshotNewerSchema`,
`syncSnapshotPending` (phase 1 carry L67) and `syncDeviceNameInvalid`.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn save_account_creates_a_disabled_row_and_never_changes_the_device()
#[test] fn a_disabled_row_installs_no_capture_and_queues_nothing()  // save_account; on_open; insert an institution → no TEMP triggers, sync_outbox empty
#[test] fn ensure_vault_secrets_is_generated_once()          // two calls → same VaultId and key
#[test] fn credentials_round_trip_through_sync_state()
#[test] fn vault_keys_json_keeps_every_epoch()
#[test] fn the_token_is_43_base64url_characters_and_matches_the_envelope_derivation()
#[test] fn debug_output_of_secrets_is_redacted()            // format!("{:?}", creds) contains no account digits, no key hex, no token
#[test] fn device_keypair_is_stable_until_renewed()
#[test] fn a_recorded_device_row_is_captured_and_keeps_its_public_key() // enable, record, record again with another name → public_key unchanged, outbox has sync_devices/<hex>
#[test] fn device_names_outside_1_to_60_characters_are_refused()
#[test] fn disable_forgets_the_account_and_every_secret()   // state::disable → keys::account(conn) == None
// error.rs (ruling P13)
#[test] fn every_sync_validation_key_has_an_english_translation() {
    // collect every "validation.sync[A-Za-z]+" literal under src-tauri/src (skipping error.rs itself)
    // and assert each key exists as validation.<key> in src/i18n/locales/en/common.json
}
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::keys services::sync::devices every_sync_validation_key` → FAIL.
- [ ] **Step 3: Implement**; add the six keys to the ten `common.json` files (English and Czech written, the others translated with `docs/standards/i18n-glossary.md`).
- [ ] **Step 4: Run** the filter → PASS; `npx vitest run src/i18n/` → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): vault credentials and the device key pair in sync_state`.

---

### Task 5: Recovery code and QR

**Files:**
- Create: `src-tauri/src/services/sync/recovery.rs`; Modify: `services/sync/mod.rs`, `src/i18n/locales/<10>/common.json` (`validation.syncRecoveryCodeInvalid`)

**Interfaces:**
- Consumes: `keys::VaultId`.
- Produces:

```rust
pub const RECOVERY_CODE_LEN: usize = 80;
pub fn encode_recovery_code(vault_id: &VaultId, key: &[u8; 32]) -> Zeroizing<String>;  // 80 chars, no separators
pub fn format_recovery_code(code: &str) -> Zeroizing<String>;                           // 16 groups of 5, joined by '-'
pub fn decode_recovery_code(input: &str) -> Result<(VaultId, Zeroizing<[u8; 32]>)>; // validation.syncRecoveryCodeInvalid
pub fn recovery_qr_payload(account: &str, code: &str) -> Zeroizing<String>;          // "moony-recover:1:{account}:{code}"
pub fn qr_svg(payload: &str) -> Result<String>;                               // EcLevel::M, quiet zone, min 240×240
```

Alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`; payload = vault_id ‖ key (48 bytes) big-endian with one
zero padding bit → 77 characters; checksum per P4 (bits 14–10, 9–5, 4–0 of
`u16::from_be_bytes([h[0], h[1]]) >> 1`). Decode uppercases, maps `I`/`L` → `1`, `O` → `0`, ignores
`-` and whitespace, and requires 80 characters, a zero padding bit and a matching checksum. The doc
comment states that the checksum guards against typos and is not an authenticator.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn recovery_code_matches_frozen_vectors() {
    let code = encode_recovery_code(&VaultId([0x22; 16]), &[0x11; 32]);
    assert_eq!(*format_recovery_code(&code), "48H24-8H248-H248H-248H2-48H24-88H24-8H248-H248H-248H2-48H24-8H248-H248H-248H2-48H24-8H248-H2XSY");
    let ids: [u8; 16] = std::array::from_fn(|i| i as u8);
    let key: [u8; 32] = std::array::from_fn(|i| (i + 16) as u8);
    assert_eq!(*encode_recovery_code(&VaultId(ids), &key),
        "000G40R40M30E209185GR38E1W8124GK2GAHC5RR34D1P70X3RFJ08924CJ2A9H750MJMASC5MQ2YWPJ");
}   // computed independently in Python (big-endian Crockford, SHA-256 checksum)
#[test] fn decoding_accepts_lowercase_dashes_spaces_and_confusable_letters()
#[test] fn every_single_substitution_of_the_frozen_vectors_is_rejected() {
    // ruling P11: for both frozen codes, replace each of the 80 positions by each of the 31 other
    // alphabet characters (2,480 mutations per code) and count decode failures.
    // assert_eq!(rejected, 2_480) for each code — verified once in Python: no mutation of either vector
    // passes the checksum and padding checks.
}
#[test] fn a_set_padding_bit_is_refused()
#[test] fn the_qr_payload_and_svg_are_well_formed()           // payload starts "moony-recover:1:", svg starts "<?xml" or "<svg", contains "</svg>"
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::recovery` → FAIL.
- [ ] **Step 3: Implement**; add `syncRecoveryCodeInvalid` to the ten `common.json` files.
- [ ] **Step 4: Run** → PASS; `npx vitest run src/i18n/`; gates.
- [ ] **Step 5: Commit** `feat(sync): recovery code with checksum and the Recovery Kit QR`.

---

### Task 6: Relay contract, HTTP client, and the typed `SyncError`

**Files:**
- Create: `src-tauri/src/services/sync/relay/mod.rs`, `src-tauri/src/services/sync/relay/http.rs`, `src-tauri/src/services/sync/error.rs`
- Modify: `services/sync/mod.rs`, `src-tauri/src/services/http.rs` (`no_redirect_client()`), `src/i18n/locales/<10>/common.json`

**Interfaces:**
- Consumes: `services::http::{REQUEST_TIMEOUT, USER_AGENT}`, `keys::VaultAuth`.
- Produces:

```rust
// services/http.rs
pub fn no_redirect_client() -> &'static Client;  // same UA and timeouts as client(), redirect::Policy::none()
// relay/mod.rs
pub enum CheckoutPlan { Monthly, Yearly }              // "monthly" | "yearly" on the wire
pub struct AccountStatus { pub paid_until: Option<i64>, pub active: bool, pub vaults: u32 }
#[derive(Clone, Debug)] pub struct VaultInfo { pub head: u64, pub floor: u64, pub snapshot_seq: Option<u64>, pub paid_until: i64, pub bytes: u64, pub rotation: u32 }
pub struct OutgoingOp { pub id: String, pub ct: Vec<u8> }
pub struct PushReceipt { pub head: u64, pub accepted: u32 }
pub struct PulledOp { pub seq: u64, pub id: String, pub ct: Vec<u8> }
pub struct PullPage { pub ops: Vec<PulledOp>, pub head: u64, pub more: bool }
pub struct SnapshotBlob { pub seq: u64, pub bytes: Vec<u8> }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayError { Network(String), Protocol(String), RateLimited, PaymentRequired, Unauthorized,
    TokenReadOnly, VaultLimitReached, AccountNotFound, VaultNotFound, NoSnapshot, VaultExists,
    AccountHasVaults, SnapshotOutdated, SnapshotRequired, QuotaExceeded, SnapshotTooLarge,
    CheckoutUnavailable, BadRequest(String), Server(u16) }
    // Network/Protocol carry a fixed reason (e.g. "timeout", "connect", "body too large", "not json"),
    // built from reqwest::Error::without_url() kinds, never a URL or a response body
pub type RelayResult<T> = std::result::Result<T, RelayError>;
impl RelayError { pub fn from_response(status: u16, body: &[u8]) -> Self; }   // by (status, "error" code)
pub trait Relay: Send + Sync {
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send;
    fn account_status(&self, account: &str) -> impl Future<Output = RelayResult<AccountStatus>> + Send;
    fn checkout(&self, account: &str, plan: CheckoutPlan) -> impl Future<Output = RelayResult<String>> + Send;
    fn create_vault(&self, account: &str, vault: &VaultAuth) -> impl Future<Output = RelayResult<bool>> + Send; // true = 201
    fn vault_info(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<VaultInfo>> + Send;
    fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> impl Future<Output = RelayResult<PushReceipt>> + Send;
    fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> impl Future<Output = RelayResult<PullPage>> + Send;
    fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> impl Future<Output = RelayResult<()>> + Send;
    fn get_snapshot(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<SnapshotBlob>> + Send;
    fn delete_vault(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<()>> + Send;
}
// relay/http.rs
pub const DEFAULT_RELAY_URL: &str = "https://sync.moonyfinance.com";
pub const MAX_JSON_BODY: usize = 64 * 1024;
pub const MAX_PULL_BODY: usize = 6 * 1024 * 1024;
pub const MAX_SNAPSHOT_BODY: usize = 104_857_600;
pub fn default_base_url() -> String;                 // MOONY_RELAY_URL in debug builds, else DEFAULT_RELAY_URL
pub fn validate_base_url(url: &str) -> Result<()>;   // validation.syncRelayUrlInvalid
pub struct HttpRelay { base: reqwest::Url }
impl HttpRelay { pub fn new(base: &str) -> Result<Self>; }
impl Relay for HttpRelay { … }
// error.rs (ruling P2)
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("snapshot required")] SnapshotRequired,
    #[error("vault not found")] VaultNotFound,
    #[error("payment required")] Lapsed,
    #[error("unauthorized")] Unauthorized,
    #[error("read-only token")] ReadOnlyToken,
    #[error("too large: {0:?}")] TooLarge(TooLarge),
    #[error("rate limited")] RateLimited,
    #[error("transient: {0:?}")] Transient(Transient),
    #[error("refused: {0}")] Refused(RelayError),              // P22
    #[error(transparent)] Local(#[from] AppError),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TooLarge { Quota, Snapshot, Body }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Transient { Offline, Relay }
pub type SyncResult<T> = std::result::Result<T, SyncError>;
impl From<RelayError> for SyncError { … }
impl SyncError {
    pub fn backs_off(&self) -> bool;          // RateLimited | Transient(_) | Local(_)   (P8)
    pub fn to_app_error(&self) -> AppError;   // Validation keys below; Local passes through
}
```

`validate_base_url` parses with `reqwest::Url::parse`: scheme `https`, or `http` with host exactly
`localhost` or `127.0.0.1`; no username, password, query or fragment; path empty or `/`.
`HttpRelay` builds every URL from the parsed base with `Url::join` and sends through
`no_redirect_client()`, so the `Moony-Account` header never leaves the base origin. Bodies are read
with `Response::chunk()` into a buffer capped at `MAX_JSON_BODY` (account, vault, push answers),
`MAX_PULL_BODY` (pull) or `MAX_SNAPSHOT_BODY` (snapshot); over the cap → `Protocol("body too
large")`. Status mapping (code from the `{"error": …}` body): 400 → `BadRequest(code)`; 401 →
`Unauthorized`; 402 → `PaymentRequired`; 403 `token_read_only` → `TokenReadOnly`,
`vault_limit_reached` → `VaultLimitReached`; 404 `vault_not_found` / `account_not_found` /
`no_snapshot`; 409 `vault_exists` / `account_has_vaults` / `snapshot_outdated`; 410 →
`SnapshotRequired`; 411 `length_required` → `BadRequest`; 413 `quota_exceeded` / `snapshot_too_large`,
413 `body_too_large` → `BadRequest`; 429 → `RateLimited`; 502/503 **on the checkout route only** →
`CheckoutUnavailable`; other 5xx → `Server(status)`; an unknown code or a body that is not the JSON
shape → `Protocol`. `SyncError` from `RelayError`: `SnapshotRequired`; `VaultNotFound`;
`PaymentRequired` → `Lapsed`; `Unauthorized`; `TokenReadOnly` → `ReadOnlyToken`; `QuotaExceeded` →
`TooLarge(Quota)`, `SnapshotTooLarge` → `TooLarge(Snapshot)`; `RateLimited`; `Network` →
`Transient(Offline)`; `Protocol`, `Server` → `Transient(Relay)`; everything else → `Refused`.
`to_app_error` keys (added to the ten `common.json` in this task): `validation.syncRelayUnreachable`
(Offline), `validation.syncRelayError` (Transient(Relay), Refused other than the next three),
`validation.syncRateLimited`, `validation.syncPaymentRequired`, `validation.syncVaultLimitReached`,
`validation.syncAccountNotFound`, `validation.syncVaultNotFound`, `validation.syncCheckoutUnavailable`,
`validation.syncSnapshotTooLarge`, `validation.syncQuotaExceeded`, `validation.syncKeyRotated`
(ReadOnlyToken, Unauthorized), `validation.syncRelayUrlInvalid`, and `validation.syncSnapshotInvalid`
for `TooLarge(Body)`. Headers: `Moony-Account` for account calls and vault creation,
`Authorization: Bearer <token>` for vault calls; snapshot calls `application/octet-stream` with a
600 s per-request timeout; `GET snapshot` reads `Moony-Snapshot-Seq`.

- [ ] **Step 1: Failing tests** (`relay/mod.rs` and `error.rs` tests; the HTTP shape tests run a one-shot `std::net::TcpListener` like `services/http.rs`'s tests, answering a canned response and returning the request head and body):

```rust
#[test] fn every_documented_status_and_code_maps_to_its_variant()      // table-driven, every code of DESIGN.md plus 411 length_required and 413 body_too_large → BadRequest
#[test] fn relay_errors_fall_into_the_sync_error_classes()            // table over every RelayError variant → SyncError class and backs_off()
#[test] fn base_urls_are_parsed_not_prefix_matched() {
    // ok: "https://sync.moonyfinance.com", "http://127.0.0.1:8787", "http://localhost:8787/"
    // refused: "http://sync.moonyfinance.com", "http://localhost.evil.com", "http://localhost@evil.com",
    //          "http://127.0.0.1.evil.com", "https://user:pw@sync.moonyfinance.com", "https://x/?q=1", "ftp://x"
}
#[tokio::test] async fn an_html_answer_is_a_protocol_error()          // 200 "<html>…" on pull → Protocol; 502 HTML on pull (not the checkout route) → Server(502)
#[tokio::test] async fn a_redirect_is_not_followed()                  // 302 to a second listener → error; the second listener received nothing
#[tokio::test] async fn an_oversized_body_is_cut_off()                // a pull answer of MAX_PULL_BODY + 1 bytes → Protocol("body too large")
#[tokio::test] async fn network_errors_carry_no_url()                 // connect to a closed port with a vault path → the error text contains neither the vault ID nor "/v1/"
#[tokio::test] async fn push_sends_bearer_json_and_base64_ops()
#[tokio::test] async fn pull_sends_after_and_limit_and_decodes_ops()
#[tokio::test] async fn create_vault_sends_the_account_header_and_reports_creation()
#[tokio::test] async fn get_snapshot_reads_the_sequence_header()
#[tokio::test] async fn checkout_posts_the_plan_and_returns_the_url()
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::relay services::sync::error services::http` → FAIL.
- [ ] **Step 3: Implement** (base64 standard alphabet with padding for `ct`, as the relay expects); add the keys to the ten `common.json`.
- [ ] **Step 4: Run** → PASS; `npx vitest run src/i18n/`; gates.
- [ ] **Step 5: Commit** `feat(sync): hardened relay client with typed sync errors`.

---

### Task 7: Fake relay for engine tests

**Files:**
- Create: `src-tauri/src/services/sync/relay/fake.rs` (`#[cfg(test)] pub mod fake;` in `relay/mod.rs`)

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Default)] pub struct FakeRelay { /* Arc<Mutex<FakeState>> */ }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route { CreateAccount, AccountStatus, Checkout, CreateVault, VaultInfo, Push, Pull, PutSnapshot, GetSnapshot, DeleteVault }
impl FakeRelay {
    pub fn new() -> Self;
    pub fn fail_next(&self, route: Route, error: RelayError);
    pub fn lose_next_push_answer(&self);          // stores the ops, then answers Network
    pub fn set_paid(&self, account: &str, paid: bool);
    pub fn prune_to(&self, floor: u64);           // floor = min(floor, snapshot seq); later pulls below it → SnapshotRequired
    pub fn drop_op(&self, seq: u64); pub fn replay_op(&self, seq: u64);  // dishonest relay knobs
    pub fn ops(&self) -> Vec<PulledOp>; pub fn calls(&self) -> Vec<Route>;
    pub fn push_sizes(&self) -> Vec<(usize, usize)>;   // per push: (ops, base64 bytes)
    pub fn snapshot_seq(&self) -> Option<u64>;
    pub fn wipe_vault(&self, vault_id: &str);
}
impl Relay for FakeRelay { … }
```

Semantics as `Moony-relay/src/vault.ts`: duplicate op IDs ignored while stored; `head`/`more`;
`after < floor` → `SnapshotRequired`; writes need a paid account (`PaymentRequired`), reads do not;
snapshot `seq > head` → `BadRequest("snapshot_ahead_of_head")`, `seq <` newest → `SnapshotOutdated`;
unknown vault → `VaultNotFound`; no snapshot → `NoSnapshot`; ops over 196,608 bytes → `BadRequest`.

- [ ] **Step 1: Failing tests**

```rust
#[tokio::test] async fn duplicate_op_ids_are_stored_once()
#[tokio::test] async fn pulls_below_the_floor_require_a_snapshot()
#[tokio::test] async fn writes_need_payment_and_reads_do_not()
#[tokio::test] async fn a_lost_push_answer_still_stores_the_ops()
#[tokio::test] async fn snapshots_must_not_go_backwards()
```

- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::relay::fake` PASS → gates.
- [ ] **Step 5: Commit** `test(sync): in-memory fake relay with the relay's semantics`.

---

### Task 8: Chain and journal for relay order — split, barrier marker, empty prefix, re-stamp, marker envelope

**Files:**
- Move: `src-tauri/src/services/sync/chain.rs` → `services/sync/chain/mod.rs` (verdicts, heads, barrier) and `services/sync/chain/rejoin.rs` (`rejoin`, `requeue_own_*`, `restamp_own_columns`, own log); the split is its own first commit with no behaviour change (ruling P12)
- Modify: `services/sync/journal/mod.rs` (`build_marker`; the `ENVELOPE_BUDGET` comment now reads "the relay accepts 192 KiB per op; 60 KiB keeps ordinary envelopes small"), `services/sync/snapshot/mod.rs`, `services/sync/snapshot/stream.rs`, `services/sync/convergence/mod.rs` (positions; `rejoin` call), the phase 1 tests that build `EnvelopeMeta` or call `rejoin`

**Interfaces:**
- Consumes: `capture::tick`, `capture::install`, `state::set_barrier_seq`.
- Produces:

```rust
pub struct EnvelopeMeta { /* existing */ pub position: u64 }          // relay seq of the op
impl EnvelopeMeta { pub fn of(cs: &Changeset, hash: [u8; 32], position: u64) -> Self; }
pub fn note_barrier_marker(conn: &Connection, meta: &EnvelopeMeta) -> Result<bool>;
    // when sync.barrier > 0, barrier_seq is NULL and meta.barrier >= sync.barrier: barrier_seq = position
pub struct RejoinReport { pub old_device: DeviceId, pub queued: usize, pub restamped: usize }
pub fn rejoin(conn: &Connection, new_device: DeviceId, trigger: Option<&EnvelopeMeta>) -> Result<RejoinReport>;
pub fn restamp_own_columns(conn: &Connection, old: DeviceId, new: DeviceId, from_hlc: u64) -> Result<usize>;
// journal
pub fn build_marker(tx: &Transaction<'_>) -> Result<Built>;   // rows: [], min_hlc = max_hlc = capture::tick
// snapshot: Record::Header gains `barrier_seq: Option<u64>` ("barrierSeq", omitted when None)
```

`judge`: `BelowBarrier` only when `meta.barrier < sync.barrier` **and**
`sync.barrier_seq.is_some_and(|m| meta.position > m)`; otherwise `Apply`. `note_barrier_marker` runs
before `judge` on every envelope that decrypts, including ones that then stop or are held: a
position in relay order is the same for every device whatever its verdict, so recording it early is
intended. `cross_barrier` sets `barrier_seq = NULL` with the new barrier. `rejoin`: as phase 1, then
— when the confirmed prefix is empty and `trigger.seq > 1` — set the old ID's head to
`(trigger.seq − 1, trigger.prev)`; reinstall capture for the new ID; then
`restamp_own_columns(old, new, W)`: every `sync_clock` row with `device = old`, `hlc >= W`,
`col NOT IN ('*', '-')` gets `(capture::tick(conn), new)`, one tick per `(tbl, pk)` so a column
group keeps one clock. Snapshot `dump` writes `barrier_seq`; `replace` sets it from the header when
`header.barrier == sync.barrier` (else NULL); `merge` keeps a local value and otherwise takes the
header's under the same condition. Add to `snapshot::dump`'s doc: "Call with exclusive use of the
connection (the engine holds the session mutex for the whole dump)."

- [ ] **Step 1: Split** `chain.rs` into `chain/mod.rs` and `chain/rejoin.rs` (`pub use rejoin::*;`), run `cargo test --lib services::sync` → PASS, commit `refactor(sync): split chain into verdicts and re-join`.
- [ ] **Step 2: Failing tests** (chain and journal tests unless named)

```rust
#[test] fn below_barrier_envelope_before_the_marker_is_applied()
#[test] fn below_barrier_envelope_after_the_marker_is_skipped()
#[test] fn the_first_envelope_at_or_above_the_barrier_is_the_marker()
#[test] fn crossing_a_barrier_forgets_the_old_marker()
#[test] fn snapshot_header_carries_the_marker_to_a_joiner()        // snapshot tests: replace adopts it, merge keeps a local one
#[test] fn rejoin_with_an_empty_prefix_takes_the_trigger_prev_as_the_old_head()
    // own_log seq 1..3 all unsent; trigger seq 4 prev P → head(old) == (3, P); re-judging the trigger → Apply
#[test] fn rejoin_restamps_own_column_clocks_under_the_new_device()
#[test] fn rejoin_keeps_row_clocks_and_tombstones_of_the_old_device()
#[test] fn restamped_columns_of_one_row_share_one_clock() {
    // old ID edited two columns of row R and one column of row S after W at different clocks;
    // after rejoin: R's two column clocks are equal to each other, S's differs from R's, all three
    // carry the new device, and each is greater than every clock that existed before the rejoin
}
#[test] fn rejoin_requeue_ships_parents_before_children()
    // old ID created bank_account A and transaction T (child) after W; rejoin; build_next in a loop:
    // the envelope holding T never comes before the one holding A, and both carry A's current generation in T's "p"
#[test] fn barrier_requeue_ships_parents_before_children()
#[test] fn a_marker_envelope_has_no_rows_and_a_current_clock()
#[test] fn applying_a_marker_moves_the_head_and_changes_no_row()   // merge tests
```

- [ ] **Step 3: Run** `cargo test --lib services::sync::chain services::sync::journal services::sync::snapshot` → FAIL.
- [ ] **Step 4: Implement**; update every phase 1 caller (`EnvelopeMeta::of(.., position)` — the convergence harness passes the relay log index + 1; `rejoin(.., Some(&meta))` / `None`). Neither `chain/` file ends over ~800 lines.
- [ ] **Step 5: Run** the filter and `cargo test --lib services::sync::convergence` → PASS; gates.
- [ ] **Step 6: Commit** `feat(sync): barrier marker by relay order, re-join head and re-stamp`.

---

### Task 9: Engine pull — verdicts, cursor rules, re-join

**Files:**
- Create: `src-tauri/src/services/sync/engine/mod.rs`, `engine/pull.rs`, `engine/tests/mod.rs`, `engine/tests/pull.rs`; Modify: `services/sync/mod.rs`

**Interfaces:**
- Consumes: `Relay`, `FakeRelay`, `SyncError`/`SyncResult`, `envelope::{open_op, envelope_hash}`, `changeset::from_json`, `chain::*`, `merge::apply_changeset`, `keys::{credentials, new_device_keypair, Credentials}`, `devices::*`, `state::*`.
- Produces:

```rust
// engine/mod.rs
pub trait ConnAccess: Clone + Send + Sync + 'static {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>;
}
impl ConnAccess for crate::db::Database { … }            // delegates to Database::with_conn
// engine/tests/mod.rs (#[cfg(test)]): `locked` makes with_conn fail with AppError::Auth, as a locked Database does
#[derive(Clone)] pub struct TestConn { pub conn: Arc<Mutex<Connection>>, pub locked: Arc<AtomicBool> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStop { Gap, Invalid, Undecryptable, NewerSchema, MissingParent }
#[derive(Debug, Default)]
pub struct PullReport { pub applied: usize, pub stop: Option<PullStop>, pub rejoined: bool,
                        pub held: usize, pub restored: bool }
impl PullReport { pub fn done(&self) -> bool; }           // stop.is_none() && held == 0   (P23)
// engine/pull.rs
pub enum OpStep { Advance, Hold, Stop(PullStop), Rejoined }
pub fn handle_op(conn: &Connection, creds: &Credentials, op: &PulledOp, now: i64, report: &mut PullReport) -> Result<OpStep>;
pub async fn pull_all<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, now: i64) -> SyncResult<PullReport>;
pub fn rejoin_as_new_device(conn: &Connection, trigger: Option<&EnvelopeMeta>, now: i64) -> Result<RejoinReport>;
```

`handle_op` (under the lock): `open_op` (failure → `Stop(Undecryptable)`); `from_json` (failure →
`Stop(Invalid)`); `meta = EnvelopeMeta::of(&cs, envelope_hash(ct), op.seq)`;
`note_barrier_marker`; `judge`:
- `Apply` → `apply_changeset`; `Applied` → `Advance`, `applied += 1`; `Stopped(MissingParent{..})` → `Stop(MissingParent)` (the table/key/parent text goes to `log::debug!` only); `Stopped(NewerSchema)` → `Stop(NewerSchema)`.
- `SkipReplay` → `Advance`. `OwnAcknowledged` → `acknowledge_own(seq)`, `Advance`.
- `Clone` → `rejoin_as_new_device(conn, Some(&meta), now)` (an error ends the pull with `Err(Local)`, so the round never pushes, correction 19), then judge the same envelope again and act on the new verdict exactly as above — `Apply`/`SkipReplay` → `Rejoined`; `Gap` → `Stop(Gap)` (possible when the old ID's head is not `seq − 1`); `Held`/`Quarantine` → `Hold`; never a second `Clone` (the ID changed).
- `Quarantine` → `chain::quarantine(author)`, `Hold`. `Held` → `Hold`.
- `BelowBarrier` → `skip_below_barrier`, `Advance`.
- `Gap` → `Stop(Gap)`. `NewerSchema` → `Stop(NewerSchema)`. `Invalid` → `Stop(Invalid)`.

`pull_all`: from `cursor`, pages of 500 until `more == false` or a stop; a page is fetched unlocked
(a relay error is returned as its `SyncError` class: `410` → `Err(SnapshotRequired)`, `404` →
`Err(VaultNotFound)`, `402` → `Err(Lapsed)` …), then each op is handled in its own `with_conn`. The
persisted cursor advances over consecutive `Advance`/`Rejoined` ops only; after the first `Hold` it
stays at the position before that op while later ops are still handled (other authors apply). Every
handled op adds its ciphertext length to `pulled_bytes` (`state::add_pulled_bytes`; Task 11 rebases
the counter). A stop ends the pull without moving the cursor past the stopping op.
`rejoin_as_new_device`: `chain::rejoin(conn, DeviceId::random(), trigger)`,
`keys::new_device_keypair`, `devices::record_this_device` with the old row's name and platform (P14;
default name `"Moony"` when no row), sets `binding_written = 0` (Task 12 rewrites the binding).

- [ ] **Step 1: Failing tests** (`engine/tests/pull.rs`; fixture in `engine/tests/mod.rs`: device A = `migrated()` + `state::enable`; device B = `migrated()` joined from A's plaintext `snapshot::dump` with phase 1's `snapshot::join`; both get the same `Credentials` through `keys::save_credentials`; a helper `seal_from(conn, creds) -> Vec<u8>` runs `build_next` + `to_json` + `seal_op` + `record_sealed` and the test pushes the bytes to `FakeRelay`)

```rust
#[tokio::test] async fn apply_moves_the_cursor_and_the_author_head()
#[tokio::test] async fn replays_and_own_envelopes_move_the_cursor()
#[tokio::test] async fn an_own_envelope_leaves_sync_unsent_when_pulled()
#[tokio::test] async fn clone_rejoins_then_rejudges_the_triggering_envelope()
    // copy A (VACUUM INTO), both write, both push; A pulls the copy's envelope → new device ID,
    // the triggering envelope applied under the old ID's head, the copy's edits present, re-queued rows in the outbox
#[tokio::test] async fn a_clone_whose_trigger_then_gaps_stops_with_gap()
#[tokio::test] async fn gap_stops_without_moving_the_cursor()               // FakeRelay.drop_op
#[tokio::test] async fn an_invalid_chain_link_stops_without_moving_the_cursor()
#[tokio::test] async fn an_undecryptable_envelope_stops_and_is_never_skipped()
#[tokio::test] async fn newer_schema_stops_pulling()
#[tokio::test] async fn quarantine_keeps_the_cursor_before_the_held_envelope_while_others_apply()
#[tokio::test] async fn a_missing_parent_rolls_back_and_keeps_the_cursor()
#[tokio::test] async fn below_barrier_after_the_marker_moves_head_and_cursor()
#[tokio::test] async fn a_pruned_cursor_returns_snapshot_required()         // FakeRelay.prune_to → Err(SyncError::SnapshotRequired)
#[tokio::test] async fn a_wiped_vault_returns_vault_not_found()
#[tokio::test] async fn pulled_bytes_count_toward_the_snapshot_policy()
#[tokio::test] async fn a_page_boundary_never_loses_or_repeats_an_envelope() // 1,201 envelopes, limit 500
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine` → FAIL.
- [ ] **Step 3: Implement** `engine/mod.rs` and `engine/pull.rs`.
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): engine pull with chain verdicts and re-join`.

---

### Task 10: Engine push — seal into `sync_unsent`, push, acknowledge, marker

**Files:**
- Create: `src-tauri/src/services/sync/engine/push.rs`, `engine/tests/push.rs`

**Interfaces:**
- Consumes: `journal::{build_next, record_sealed, build_marker, Refused}`, `changeset::to_json`, `envelope::{seal_op, op_id, envelope_hash}`, `state::note_sealed_at_barrier`.
- Produces:

```rust
pub const SEAL_BATCH: usize = 64;
pub const PUSH_MAX_OPS: usize = 100;
pub const PUSH_MAX_B64: usize = 3 * 1024 * 1024;
pub struct SealReport { pub sealed: usize, pub marker: bool, pub refused: Vec<Refused> }
pub fn seal_batch(conn: &Connection, creds: &Credentials) -> Result<SealReport>;
pub fn next_push_batch(conn: &Connection) -> Result<Vec<(u64, OutgoingOp)>>;   // = next_push_batch_with(conn, PUSH_MAX_OPS, PUSH_MAX_B64)
pub fn next_push_batch_with(conn: &Connection, max_ops: usize, max_b64: usize) -> Result<Vec<(u64, OutgoingOp)>>;
pub fn acknowledge_pushed(conn: &Connection, seqs: &[u64]) -> Result<()>;
#[derive(Debug, Default)] pub struct PushReport { pub pushed: usize, pub refused: usize }
pub async fn push_all<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials) -> SyncResult<PushReport>;
```

`seal_batch` runs only when `sync_unsent` is empty; one transaction: up to `SEAL_BATCH` ×
(`build_next` → `to_json` → `seal_op` → `INSERT sync_unsent(seq, envelope)` →
`record_sealed(hash)`); when it sealed anything, `note_sealed_at_barrier(sync.barrier)` (any own
envelope carries the barrier, so it is a marker too). When it sealed nothing, the outbox holds no
sendable row, `sync.barrier > 0`, `barrier_seq IS NULL` and `marker_barrier < sync.barrier`, it seals
one `build_marker` envelope and notes it — at most one marker per barrier (ruling P5). `push_all`
loops: seal (locked) → batch (locked read) → `relay.push` (unlocked) → `acknowledge_pushed` (locked);
it ends when a seal produced nothing and `sync_unsent` is empty (a refused row stays in the outbox
and never keeps the loop alive), or on the first error, returned as its `SyncError` class
(`Lapsed`, `TooLarge(Quota)`, `ReadOnlyToken`, `Unauthorized`, `VaultNotFound`, `Transient`, …).
Sealed envelopes stay in `sync_unsent` across errors. A refused row is logged at `warn` with table
and byte size only (no key) and counted.

- [ ] **Step 1: Failing tests** (`engine/tests/push.rs`)

```rust
#[tokio::test] async fn sealing_stores_envelopes_and_clears_the_outbox_in_one_transaction()
    // after seal_batch: sync_unsent has seq 1..n, sync_own_log has the same (seq, hash), next_seq = n+1, outbox empty
#[tokio::test] async fn a_crash_before_the_push_resends_the_same_op_ids()   // seal, drop, seal again → no new envelope; push → ids equal the sealed ones
#[tokio::test] async fn a_push_whose_answer_was_lost_is_acknowledged_by_the_next_pull()
    // FakeRelay.lose_next_push_answer; push_all → Err(Transient(Offline)); then pull_all: OwnAcknowledged, unsent empty; relay holds each op once
#[test] fn push_batches_respect_the_relay_limits() {
    // ruling P11: 300 sealed small envelopes in sync_unsent (300 single-row edits, SEAL_BATCH loops);
    // next_push_batch_with(conn, 100, usize::MAX) → exactly 100 ops, seqs 1..=100 in order;
    // next_push_batch_with(conn, usize::MAX, 3 × one op's base64 length) → exactly 3 ops;
    // and through push_all against FakeRelay: push_sizes() has 3 pushes of 100 ops each, relay order = seq order
}
#[tokio::test] async fn payment_required_keeps_envelopes_and_stops_pushing()    // Err(SyncError::Lapsed), sync_unsent unchanged
#[tokio::test] async fn quota_and_rotated_tokens_stop_pushing()                  // Err(TooLarge(Quota)), Err(ReadOnlyToken)
#[tokio::test] async fn a_vault_gone_answer_is_reported()                         // Err(VaultNotFound)
#[tokio::test] async fn an_oversized_row_is_refused_and_the_loop_still_ends()   // refused == 1, the row stays queued, push_all returns Ok
#[tokio::test] async fn a_device_past_a_new_barrier_with_nothing_to_send_pushes_one_marker()
    // barrier raised, outbox empty: round 1 seals a marker; push_all again (barrier_seq still NULL) → no second marker
#[test] #[ignore = "a measurement: run in release with --ignored --nocapture"]
fn draining_a_50000_row_outbox()    // prints the seconds for seal_batch until empty (P19); no assertion
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine::tests::push` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; run the measurement once in release and note the number in the commit body; gates.
- [ ] **Step 5: Commit** `feat(sync): engine push through sync_unsent with batching`.

---

### Task 11: Engine snapshots — `open_sealed`, restore after `410`, upload policy, sealed dump

**Files:**
- Create: `src-tauri/src/services/sync/engine/snapshots.rs`, `engine/tests/snapshots.rs`

**Interfaces:**
- Consumes: `snapshot::{dump, restore_merge, DumpStats, RestoreReport}`, `envelope::stream::*`, `state::{record_snapshot_uploaded, rebase_pulled_bytes, marks}`.
- Produces:

```rust
pub const SNAPSHOT_ENVELOPES: u64 = 2_000;
pub const SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
pub const SNAPSHOT_MIN_INTERVAL: i64 = 86_400;
pub const SNAPSHOT_MAX_DELAY: i64 = 3_600;
pub const POLICY_CHECK_INTERVAL: i64 = 3_600;
pub const MAX_SNAPSHOT_BYTES: usize = 104_857_600;
pub const MAX_SNAPSHOT_PLAIN: usize = 512 * 1024 * 1024;
pub struct SealedSnapshot { pub seq: u64, pub bytes: Vec<u8>, pub stats: DumpStats }
pub fn dump_sealed(conn: &Connection, creds: &Credentials) -> Result<SealedSnapshot>;
    // S = cursor; BEGIN DEFERRED … COMMIT around snapshot::dump writing through raw deflate into a StreamSealer
pub fn open_sealed(creds: &Credentials, blob: &SnapshotBlob) -> SyncResult<Vec<u8>>;
    // open_stream with snapshot_object_id(blob.seq), raw inflate capped at MAX_SNAPSHOT_PLAIN → the record stream
pub fn restore_sealed(conn: &Connection, plain: &[u8], now: i64) -> Result<RestoreReport>;  // snapshot::restore_merge
pub enum Due { No, Now, At(i64) }
#[derive(Default)] pub struct SnapshotPolicy { pub checked_at: Option<i64>, pub scheduled_at: Option<i64> }   // in memory, per engine
impl SnapshotPolicy {
    pub fn wants_info(&self, now: i64) -> bool;                 // never checked, or POLICY_CHECK_INTERVAL elapsed, or a schedule fell due
    pub fn decide(&mut self, info: &VaultInfo, marks: &EngineMarks, now: i64, jitter: i64) -> Due;
}
pub async fn restore_after_410<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, now: i64) -> SyncResult<RestoreReport>;
pub async fn maybe_upload<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, policy: &mut SnapshotPolicy,
    info: &VaultInfo, now: i64, jitter: i64, force: bool) -> SyncResult<bool>;   // = maybe_upload_with(.., MAX_SNAPSHOT_BYTES)
pub async fn maybe_upload_with<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, policy: &mut SnapshotPolicy,
    info: &VaultInfo, now: i64, jitter: i64, force: bool, max_bytes: usize) -> SyncResult<bool>;
```

The caller (`run_round`, Task 13) fetches `vault_info` and calls `maybe_upload` only after a pull for
which `PullReport::done()` holds (rulings P4, P14). `maybe_upload` itself: `rebase_pulled_bytes(
info.snapshot_seq.unwrap_or(0))`; returns `Ok(false)` unless `sync_outbox` and `sync_unsent` are
empty; with `force` the upload is due now, otherwise `decide`: `info.snapshot_seq == None` → `Now`;
else due when `head − snapshot_seq ≥ 2,000` or `pulled_bytes ≥ 8 MiB`, and
`now − snapshot_uploaded_at ≥ 86,400`; a due result is scheduled at `now + jitter` (`jitter` ∈
`0..=3,600`, drawn by the caller) and returned as `At` until then. The dump runs in
`tokio::task::spawn_blocking(move || db.with_conn(|c| dump_sealed(c, &creds)))`: it holds the session
mutex for the whole dump (phase 1 carry: exclusive use; P18), never across the upload. A sealed
size over `max_bytes` or `TooLarge(Snapshot)` from the relay → `Err(TooLarge(Snapshot))` with no
retry until the next day; `SnapshotOutdated` (as `Refused`) drops the schedule and returns
`Ok(false)`; success → `record_snapshot_uploaded(seq, stats.max_hlc, now)`.
`restore_after_410`: `get_snapshot` → `open_sealed` → (locked) `restore_sealed` → cursor =
`MAX(cursor, seq)`; when `restore_merge` fails, log the merge stop reason at `debug`, write the fixed
text "snapshot restore stopped" to `last_error`, and return `Local(validation.syncSnapshotInvalid)`
(phase 1 note L142).

- [ ] **Step 1: Failing tests**

```rust
#[test] fn policy_uploads_at_once_when_the_vault_has_no_snapshot()
#[test] fn policy_waits_for_2000_envelopes_or_8_mib()
#[test] fn policy_uploads_at_most_once_a_day_after_the_random_delay()
#[test] fn policy_asks_for_vault_info_at_most_hourly()
#[tokio::test] async fn upload_waits_for_an_empty_outbox_and_unsent()
#[tokio::test] async fn a_sealed_dump_opens_to_the_same_record_stream()
    // dump_sealed on A; open_sealed(&creds, &SnapshotBlob{seq, bytes}) == the bytes of snapshot::dump(A, S) written to a Vec
#[tokio::test] async fn a_snapshot_served_under_another_sequence_fails()   // relay returns bytes of seq 5 labelled 6 → error, nothing written
#[tokio::test] async fn snapshot_required_restores_by_merge_and_keeps_the_outbox()
    // A and B from the Task 9 fixture; B offline with local edits; A edits, pushes, uploads; FakeRelay.prune_to(S);
    // B pull_all → Err(SnapshotRequired) → restore_after_410: A's edits present, cursor == S, B's edits still queued
#[tokio::test] async fn an_outdated_snapshot_is_dropped_quietly()
#[tokio::test] async fn an_oversized_snapshot_is_not_uploaded()            // maybe_upload_with(.., max_bytes = 1024) → Err(TooLarge(Snapshot)), FakeRelay.snapshot_seq() unchanged
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine::tests::snapshots` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): snapshot upload policy and restore after 410`.

---

### Task 12: Restore marker and device binding (logic)

**Files:**
- Create: `src-tauri/src/services/sync/binding.rs`
- Modify: `src-tauri/src/services/backup.rs` (`restore_backup` writes the marker after a successful restore), `src-tauri/src/services/auth.rs` (`remove_account_files` also removes `sync-binding.bin` and `sync-rejoin.marker`), `src-tauri/capabilities/default.json` (`fs:scope` deny `$APPDATA/sync-binding.bin`, `$APPDATA/sync-rejoin.marker`), `services/sync/mod.rs`

**Interfaces:**
- Produces:

```rust
pub const BINDING_FILE: &str = "sync-binding.bin";
pub const REJOIN_MARKER: &str = "sync-rejoin.marker";
pub const BINDING_LABEL: &[u8] = b"moony sync binding v1";
pub enum BindingFailure { Unsupported, NotThisDevice, Failed(String) }
pub trait BindingNative: Send + Sync {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure>;
    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure>;
}
pub struct NoBinding;                                        // Unsupported everywhere (Linux, tests)
pub enum StartupCheck { Fine, Rejoin(RejoinReason) }
pub enum RejoinReason { RestoreMarker, BindingMismatch }
pub fn write_rejoin_marker(data_dir: &Path) -> std::io::Result<()>;
pub fn clear_rejoin_marker(data_dir: &Path) -> std::io::Result<()>;
pub fn startup_check(conn: &Connection, data_dir: &Path, native: &dyn BindingNative) -> Result<StartupCheck>;
pub fn write_binding(conn: &Connection, data_dir: &Path, native: &dyn BindingNative) -> Result<()>;
pub fn forget_binding(data_dir: &Path) -> Result<()>;      // removes the binding file and the marker
```

`startup_check` (sync enabled only): marker present → `Rejoin(RestoreMarker)`; else when
`binding_written = 1`: file missing, `NotThisDevice`, or an opened value not equal (by
`subtle::ConstantTimeEq`, ruling P16) to `SHA-256(BINDING_LABEL ‖ device_id)` →
`Rejoin(BindingMismatch)`; `Unsupported` → `Fine`; `Failed` → `Fine` with one `warn` per start.
`write_binding` seals the hash, writes the file `0o600` (as `moony-keychain`'s `blob_file`), sets
`binding_written = 1`; `Unsupported` leaves `binding_written = 0`. The marker is written by
`backup::restore_backup` for every restore (the next unlock decides: no sync row → the marker is
just cleared). `backup::managed_names()` does not include the two files (they must survive a
restore).

- [ ] **Step 1: Failing tests** (a `FakeNative` that XORs with a per-instance "machine" byte; `NotThisDevice` when the byte differs)

```rust
#[test] fn a_restore_marker_asks_for_a_rejoin()
#[test] fn a_binding_written_on_another_machine_asks_for_a_rejoin()
#[test] fn a_missing_binding_file_after_one_was_written_asks_for_a_rejoin()
#[test] fn a_never_written_binding_is_fine()
#[test] fn unsupported_platforms_never_rejoin()
#[test] fn a_binding_of_the_old_device_id_asks_for_a_rejoin()
#[test] fn forget_binding_removes_the_file_and_the_marker()
#[test] fn restore_backup_leaves_a_rejoin_marker()                  // services/backup.rs tests
#[test] fn removing_account_files_takes_the_sync_files_too()        // services/auth.rs tests: both files present → removed
```

- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::binding services::backup services::auth` PASS → gates.
- [ ] **Step 5: Commit** `feat(sync): restore marker and device binding check`.

---

### Task 13: Engine runtime — rounds, triggers, back-off, status, events, start and stop

**Files:**
- Create: `src-tauri/src/services/sync/engine/runtime.rs`, `engine/status.rs`, `engine/tests/runtime.rs`, `src-tauri/src/services/sync/feature.rs`, `src-tauri/src/models/sync.rs`, `src-tauri/src/commands/sync.rs` (non-command glue only)
- Modify: `src-tauri/src/models/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`.manage(commands::sync::SyncEngineState::default())`), `src-tauri/src/commands/auth.rs` (`after_unlock` and `confirm_recover` call `commands::sync::start_engine`; `logout` and `delete_account` call `stop_engine`), `src-tauri/src/commands/backup.rs` (`restore_backup` calls `stop_engine` before the restore), `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerated), `shared/schema.ts`

**Interfaces:**
- Consumes: Tasks 6, 9–12.
- Produces:

```rust
// feature.rs
pub const RELEASED: bool = false;
pub fn enabled() -> bool;
pub fn dev_tools() -> bool;                                    // cfg!(debug_assertions) && enabled()
// models/sync.rs (derive Serialize, Deserialize, specta::Type, Debug; per-field camelCase renames)
pub enum SyncPhase { Off, SettingUp, Idle, Syncing, Paused, Error }    // "off" | "settingUp" | "idle" | "syncing" | "paused" | "error"
pub enum SyncProblem { CaptureBroken, VaultGone, Lapsed, QuotaExceeded, SnapshotTooLarge, NewerSchema,
    Gap, Undecryptable, InvalidChain, MissingParent, Quarantined, Offline, RelayError, RateLimited,
    Unauthorized, KeyRotated, Internal }                          // camelCase strings
pub struct SyncStatus { phase: SyncPhase, problem: Option<SyncProblem>, last_success_at: Option<i64>,
    pending_changes: u32, held_devices: u32, refused_rows: u32, paid_until: Option<i64>,
    deletes_at: Option<i64>, dev_tools: bool }                    // fields pub; JSON camelCase
// engine/status.rs
#[derive(Clone, Default)] pub struct Runtime { pub running_round: bool, pub problem: Option<SyncProblem>, pub paid_until: Option<i64>, pub refused_rows: u32 }
pub fn status(conn: &Connection, runtime: &Runtime) -> Result<SyncStatus>;
pub fn problem_of(err: &SyncError) -> SyncProblem;   // Transient(Offline) → Offline, Transient(Relay)/Refused → RelayError, RateLimited, Local → Internal, …
// engine/mod.rs
pub trait EngineEvents: Send + Sync + 'static { fn data_changed(&self); fn status_changed(&self, status: &SyncStatus); fn binding(&self) -> &dyn BindingNative; fn data_dir(&self) -> PathBuf; }
pub enum RoundMode { Normal, FirstSnapshot, Drain }
#[derive(Debug, Default)]
pub struct RoundReport { pub pull: Option<PullReport>, pub push: Option<PushReport>, pub restored: bool,
    pub snapshot_uploaded: bool, pub vault: Option<VaultInfo>, pub problem: Option<SyncProblem> }
pub async fn run_round<R: Relay, D: ConnAccess>(relay: &R, db: &D, policy: &mut SnapshotPolicy, mode: RoundMode, now: i64, jitter: i64) -> SyncResult<RoundReport>;
// engine/runtime.rs
pub struct Backoff { /* next: Duration */ } impl Backoff { pub fn on_error(&mut self) -> Duration; pub fn reset(&mut self); }
pub struct OutboxWatch { /* last_count, first_seen */ } impl OutboxWatch { pub fn observe(&mut self, pending: u64, now: i64) -> bool; }
pub enum Trigger { Unlock, LocalWrite, Periodic, SyncNow }
pub struct EngineHandle { /* Arc<Shared { wake: Notify, stop: AtomicBool, running: AtomicBool, runtime: Mutex<Runtime> }> */ }
impl EngineHandle { pub fn wake(&self, trigger: Trigger); pub fn stop(&self); pub fn is_running(&self) -> bool; pub fn runtime(&self) -> Runtime; }
pub fn spawn<R: Relay + 'static, D: ConnAccess, E: EngineEvents>(relay: R, db: D, events: E) -> EngineHandle;
// commands/sync.rs (glue, no #[tauri::command] yet)
#[derive(Default)] pub struct SyncEngineState(pub Mutex<Option<EngineHandle>>);
pub(crate) fn start_engine(app: &AppHandle, db: &Database);    // no-op unless feature::enabled() and the row is enabled
pub(crate) fn stop_engine(app: &AppHandle);
```

`run_round` (credentials from `keys::credentials`; none → `Ok` with phase off):
1. **Guards.** `capture_broken_at` set and mode ≠ `Drain` → `Ok(problem CaptureBroken)`;
   `vault_gone_at` set → `Ok(problem VaultGone)`.
2. **Pull.** `pull_all`. `Err(SnapshotRequired)` → `restore_after_410` (`restored = true`), then
   `pull_all` once more (a second `SnapshotRequired` → `Ok(problem RelayError)`).
3. **Push gate (correction 19, ruling P1).** Push only when `pull.stop` is `None` (held envelopes
   allowed) or `Some(NewerSchema)`. Any other stop → no push this round; the problem is `Gap`,
   `InvalidChain`, `Undecryptable` or `MissingParent`.
4. **Push.** `push_all`.
5. **Snapshot (rulings P4, P14).** Not in `Drain`. When `mode == FirstSnapshot` or
   `policy.wants_info(now)`: fetch `vault_info` (kept in `report.vault`). Only when the latest
   pull `done()`: if the push sent anything, `pull_all` again and require `done()` once more; then
   `maybe_upload(.., force = mode == FirstSnapshot)`. A held envelope or a stop skips the upload.
6. **Stops as problems, not errors (P8).** An `Err` from steps 2–5 of class `VaultNotFound` →
   `set_vault_gone(now)`, problem `VaultGone`; `Lapsed` → `vault_info` once for `paid_until`
   (ignored if it fails), problem `Lapsed`; `TooLarge(Quota)` → `QuotaExceeded`;
   `TooLarge(Snapshot)` → `SnapshotTooLarge`; `ReadOnlyToken` → `KeyRotated`; `Unauthorized`; these
   return `Ok(report)` with the problem. `Transient`, `RateLimited`, `Refused` and `Local` return
   `Err` (the runtime backs off for `backs_off()` classes; `Refused` is shown as `RelayError` and
   retried by the periodic round). `pull.held > 0` with no other problem → `Quarantined`.
7. `record_success(now)` when the report has no problem.

The loop: at start, `startup_check` → on `Rejoin`, `rejoin_as_new_device(conn, None, now)`, clear
the marker, then `write_binding`; after every round with `rejoined`, `write_binding`. Then rounds on
`Unlock`, on `OutboxWatch::observe` every 5 s, every 300 s, and on `SyncNow` (which also clears a
back-off wait and, with `vault_gone_at`, tries one `vault_info` and clears the pause on success,
P9). Errors with `backs_off()` wait 30 s → 1,800 s. A `with_conn` error of kind `AppError::Auth`
(locked) ends the loop silently. `data_changed` fires after any round with `applied > 0` or a
restore, and `commands::mcp_refresh::resume` runs for the history queue (the Tauri adapter);
`status_changed` fires whenever the status differs from the last one sent. Rounds never overlap
(one task). Status phase per P17; `deletes_at = paid_until + 90 × 86,400` when `Lapsed`.

- [ ] **Step 1: Failing tests** (`engine/tests/runtime.rs`; pure parts directly, rounds with `FakeRelay` + `TestConn`, a recording `EngineEvents`)

```rust
#[test] fn backoff_doubles_from_30_seconds_to_30_minutes_and_resets()
#[test] fn a_local_write_runs_a_round_after_one_quiet_poll_or_after_60_seconds()
#[tokio::test] async fn two_devices_converge_through_rounds()              // A edits, round A, round B → assert_converged
#[tokio::test] async fn a_rejoin_happens_before_any_push_in_the_round()
    // a VACUUM INTO copy of A pushes under A's ID; A's run_round: FakeRelay.calls() has no Push before A's re-join,
    // and every op A pushes in that round decrypts to a changeset whose device is A's new ID
#[tokio::test] async fn a_stopped_pull_skips_the_push() {
    // ruling P1: for Gap (drop_op), an undecryptable envelope and an invalid chain link: A has queued rows;
    // run_round → report.push is None, FakeRelay.calls() contains no Push, A's outbox unchanged, problem set
}
#[tokio::test] async fn newer_schema_and_held_envelopes_still_push()
#[tokio::test] async fn a_snapshot_is_dumped_only_after_a_clean_pull() {
    // rulings P4, P14: A pushes 3 envelopes in a FirstSnapshot round → the snapshot seq == relay head (3);
    // with a held envelope in the pull → no upload; with a Gap → no upload
}
#[tokio::test] async fn a_capture_broken_profile_never_syncs()
#[tokio::test] async fn lapsed_subscription_keeps_pulling_and_reports_the_deletion_date()
#[tokio::test] async fn a_vault_gone_answer_pauses_until_sync_now_finds_it_again()
#[tokio::test] async fn a_vault_gone_during_the_pull_pauses_too()          // wipe_vault before the round → vault_gone_at set, no back-off
#[tokio::test] async fn data_changed_fires_only_when_something_was_applied()
#[tokio::test] async fn a_restore_marker_rejoins_before_the_first_round()
#[tokio::test] async fn a_locked_session_ends_the_loop_without_an_error()  // TestConn.locked → loop ends, no status_changed with phase error
#[test] fn status_phases_follow_the_problem()                               // table: each SyncProblem → phase per P17
#[test] fn the_flag_is_off_in_this_build_unless_the_dev_variable_is_set()  // RELEASED == false; enabled() == (debug && MOONY_DEV_SYNC=1)
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine services::sync::feature` → FAIL.
- [ ] **Step 3: Implement** the runtime, status, models, feature flag and glue. The Tauri `EngineEvents` adapter emits `sync-data-changed` / `sync-status-changed`, calls `mcp_refresh::resume`, and returns the platform binding native (Task 14 fills it; until then `NoBinding`).
- [ ] **Step 4: Wire types (ruling P15).** Register `SyncPhase`, `SyncProblem`, `SyncStatus` in `collect_types()`; `cd src-tauri && cargo test generate_bindings -- --ignored`; mirror them in `shared/schema.ts` (string unions for the enums, `SyncStatus` interface).
- [ ] **Step 5: Run** the filter → PASS; `npm run typecheck`; gates. Check by hand that a profile without sync starts no task (`start_engine` returns after one `SELECT`).
- [ ] **Step 6: Commit** (with the regenerated `shared/generated-types.ts`) `feat(sync): engine loop with triggers, back-off, status and events`.

---

### Task 14: Native device binding and secret clipboard — macOS, Windows

**Files:**
- Modify: `src-tauri/plugins/moony-keychain/macos/Sources/MoonyBiometrics/MoonyBiometrics.swift` (four functions), `plugins/moony-keychain/src/desktop/macos.rs`, `src/desktop/windows.rs`, `src/desktop/mod.rs` (methods on `Keychain`), `plugins/moony-keychain/Cargo.toml` (`windows` features `Win32_Security_Cryptography`, `Win32_System_DataExchange`, `Win32_System_Memory`, `Win32_System_Ole`), `src-tauri/src/commands/sync.rs` (adapter `KeychainBinding` implementing `BindingNative`)

**Interfaces:**
- Produces (plugin, desktop):

```rust
impl<R: Runtime> Keychain<R> {
    pub fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>>;               // KeychainError::Unsupported where none
    pub fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>>;          // KeychainError::Invalidated = not this machine
    pub fn copy_secret_text(&self, text: &str) -> Result<u64>;                    // returns the clipboard change token
    pub fn clear_clipboard_if_unchanged(&self, token: u64) -> Result<bool>;       // true = cleared
}
```

Binding — macOS: `moony_bind_seal(secret: SRData) -> SRData` / `moony_bind_open(blob: SRData) ->
SRData` in Swift, the same ECIES framing as `moony_bio_wrap` but with a Secure Enclave P-256 key
whose access control is `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` + `.privateKeyUsage` (no user
presence, no prompt); the key's `dataRepresentation` travels inside the blob; a blob from another
Mac fails to restore the key → status `STATUS_INVALIDATED`. Windows: `CryptProtectData` /
`CryptUnprotectData` with `CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN` and the
description `"Moony sync binding"`. Clipboard (the alternative to ruling P16's UI clearing, see the
pushback) — macOS: `moony_clip_copy(text: SRString) -> Int` (clear contents, write the string plus
the type `org.nspasteboard.ConcealedType`, return `changeCount`) and `moony_clip_clear_if(count:
Int) -> Bool`; Windows: `OpenClipboard`, `EmptyClipboard`, `SetClipboardData(CF_UNICODETEXT)` plus a
registered `ExcludeClipboardContentFromMonitorProcessing` format, return
`GetClipboardSequenceNumber()`; clear only when the sequence number is unchanged. Linux and phones:
`Unsupported` (phones arrive in phase 3). The app adapter maps `Unsupported` →
`BindingFailure::Unsupported`, `Invalidated` → `NotThisDevice`, others → `Failed`.

- [ ] **Step 1: Failing tests** (plugin, platform-gated): `#[cfg(target_os = "windows")] fn dpapi_binding_round_trips()`; `#[cfg(target_os = "windows")] fn the_clipboard_is_cleared_only_while_unchanged()`; `#[cfg(target_os = "macos")] #[ignore = "needs a Mac with a Secure Enclave"] fn secure_enclave_binding_round_trips()`; `#[cfg(target_os = "macos")] #[ignore = "touches the real pasteboard"] fn the_pasteboard_is_cleared_only_while_unchanged()`; `#[cfg(target_os = "linux")] fn linux_has_no_binding_or_clipboard()`; app side `fn keychain_errors_map_to_binding_failures()`.
- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test -p moony-keychain` and `cargo test --lib commands::sync` PASS; on the Mac run the ignored tests once (`cargo test -p moony-keychain -- --ignored`) and note the result; gates.
- [ ] **Step 5: Commit** `feat(keychain): this-machine binding and a clearing clipboard for device sync`.

---

### Task 15: Join a vault by replace; repair a capture-broken profile

**Files:**
- Create: `src-tauri/src/services/sync/join.rs`, `engine/tests/join.rs`
- Modify: `src-tauri/src/services/auth.rs` (`remove_account_files` also removes `sync-repair-*.zip`), `src-tauri/capabilities/default.json` (`fs:scope` deny `$APPDATA/sync-repair-*.zip`), `src/i18n/locales/<10>/common.json`

**Interfaces:**
- Consumes: `snapshot::join`, `state::{reset_chain, reset_engine_marks}`, `keys::*`, `devices::*`, `engine::snapshots::open_sealed`, `engine::run_round`, `backup::create_backup`.
- Produces:

```rust
pub async fn join_by_replace<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: Credentials, device_name: &str, now: i64) -> SyncResult<RestoreReport>;
pub async fn repair_capture<R: Relay, D: ConnAccess>(relay: &R, db: &D, data_dir: &Path, now: i64) -> SyncResult<PathBuf>;
```

`join_by_replace`: `get_snapshot` (`NoSnapshot` → `Local(validation.syncVaultHasNoSnapshot)`),
`open_sealed` unlocked, then one savepoint under the lock: `state::reset_chain` and
`state::reset_engine_marks` when a row exists, `snapshot::join(conn, DeviceId::random(), reader, now)`
(cursor = `S`, phase 1 carry L173), `keys::save_credentials`, `keys::new_device_keypair`,
`devices::record_this_device`. `repair_capture` (ruling P3): `backup::create_backup` into
`data_dir/sync-repair-<unix>.zip`, then `run_round(.., RoundMode::Drain, ..)`; it continues only if
the round returned `Ok` with no problem, its pull `done()`, and `sync_outbox` and `sync_unsent` are
empty afterwards — otherwise it returns `Local(validation.syncRepairDrainIncomplete)` having changed
nothing but the zip; then `join_by_replace` with the stored credentials and the current device name;
returns the zip path. Keys added in this task to the ten `common.json`: `syncVaultHasNoSnapshot`,
`syncRepairDrainIncomplete`.

- [ ] **Step 1: Failing tests**

```rust
#[tokio::test] async fn a_joined_device_matches_the_vault_and_starts_after_the_snapshot()
    // A sets up and edits; B joins → assert_converged(A, B); B's cursor == S; B's first pull asks after=S
#[tokio::test] async fn the_joiner_records_itself_with_a_public_key_and_pushes_it()
#[tokio::test] async fn joining_a_vault_without_a_snapshot_is_refused_and_changes_nothing()
#[tokio::test] async fn joining_clears_pauses_and_counters()
#[tokio::test] async fn repair_backs_up_drains_and_rejoins_under_a_new_device()
    // capture_broken_at set, outbox holds earlier captured rows → they reach the relay; new device ID; capture_broken_at NULL; zip exists
#[tokio::test] async fn repair_stops_when_the_drain_did_not_finish() {
    // ruling P3: FakeRelay fails the push with Network (and, separately, Lapsed; and a Gap in the pull):
    // repair_capture → Err(syncRepairDrainIncomplete); device ID, outbox, sync_unsent and every synced row unchanged; zip exists
}
#[test] fn removing_account_files_takes_the_repair_backups()        // services/auth.rs tests
```

- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::engine::tests::join services::auth` PASS; `npx vitest run src/i18n/`; gates.
- [ ] **Step 5: Commit** `feat(sync): join a vault by replace and repair a capture-broken profile`.

---

### Task 16: First-device setup, turning off, deleting synced data

**Files:**
- Create: `src-tauri/src/services/sync/setup.rs`, `engine/tests/setup.rs`
- Modify: `src-tauri/src/models/sync.rs`, `src-tauri/src/services/auth.rs` (extract `pub fn verify_password(db_path: &Path, password: &str) -> Result<()>` from `prepare_change_password`, which then calls it), `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerated), `shared/schema.ts`, `src/i18n/locales/<10>/common.json`

**Interfaces:**
- Produces:

```rust
// models/sync.rs
pub enum SyncPlan { Monthly, Yearly }                                             // "monthly" | "yearly"
pub struct SyncAccount { account_number: String, active: bool, paid_until: Option<i64> }  // JSON accountNumber, active, paidUntil
pub struct RecoveryKit { account_number: String, recovery_code: String, qr_svg: String } // code grouped; hand-written Debug prints "RecoveryKit { .. }"
// setup.rs
pub async fn create_account<R: Relay, D: ConnAccess>(relay: &R, db: &D, relay_url: &str) -> SyncResult<SyncAccount>; // returns the stored one if any
pub async fn account_status<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> SyncResult<SyncAccount>;
pub async fn checkout_url<R: Relay, D: ConnAccess>(relay: &R, db: &D, plan: SyncPlan) -> SyncResult<String>;
    // https only, host polar.sh or a subdomain of it → else Local(validation.syncRelayError)
pub fn recovery_kit(conn: &Connection) -> Result<RecoveryKit>;                    // ensure_vault_secrets
pub async fn finish_setup<R: Relay, D: ConnAccess>(relay: &R, db: &D, device_name: &str, kit_saved: bool, now: i64) -> SyncResult<()>;
pub fn turn_off(conn: &Connection, data_dir: &Path) -> Result<()>;               // state::disable + binding::forget_binding
pub async fn delete_synced_data<R: Relay, D: ConnAccess>(relay: &R, db: &D, data_dir: &Path) -> SyncResult<()>;
pub async fn delete_vault_best_effort<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> bool; // 5 s budget
```

`create_account` stores the number only after the relay answered; a crash in between leaves an
unpaid, empty relay account nobody knows — harmless (it holds nothing and costs nothing).
`finish_setup` (resumable): `kit_saved` false → `validation.syncRecoveryKitNotConfirmed` (the
frontend's confirmation is trusted: the person ticked it after a save or copy);
`validate_device_name`; credentials missing → `validation.syncSetupIncomplete`; `create_vault`
(`Lapsed` → `validation.syncPaymentRequired`; `VaultLimitReached` → that key); when not enabled:
`state::enable(conn, current device)`, `keys::ensure_device_keypair`, `devices::record_this_device`;
then `run_round(.., RoundMode::FirstSnapshot, ..)`, which pulls (nothing yet), pushes the rule packs
and the device row, pulls them back as own envelopes (`OwnAcknowledged`, cursor = relay head), and
uploads the first snapshot at `S` = that cursor (correction 20, ruling P4). A failed upload does
not fail setup (the policy uploads when `snapshot_seq` is null). Already enabled with a snapshot →
`validation.syncAlreadyEnabled`. `delete_synced_data`: `delete_vault` first (the row holds the
token; `VaultNotFound` counts as done), then `turn_off`. Keys added in this task to the ten
`common.json`: `syncRecoveryKitNotConfirmed`, `syncSetupIncomplete`, `syncAlreadyEnabled`.

- [ ] **Step 1: Failing tests**

```rust
#[tokio::test] async fn first_device_setup_creates_the_vault_pushes_and_uploads_a_snapshot() {
    // FakeRelay: account → set_paid → kit → finish: vault exists; ops = [one envelope: rule packs + device row];
    // FakeRelay.snapshot_seq() == Some(relay head after the second pull) == Some(1); state enabled, capture installed;
    // sync_unsent empty; the snapshot opens (open_sealed) to a stream whose header heads include this device at seq 1
}
#[tokio::test] async fn setup_resumes_with_the_stored_account_and_vault_after_a_restart()
    // create_account, recovery_kit, "restart" (new FakeRelay handle, same state) → create_account returns the same number, recovery_kit the same code
#[tokio::test] async fn finishing_without_a_saved_kit_or_payment_is_refused()
#[tokio::test] async fn a_failed_first_upload_is_retried_by_the_policy()
#[tokio::test] async fn the_checkout_url_must_be_polar_https()
#[tokio::test] async fn turning_off_keeps_local_data_and_forgets_every_secret()   // also: binding file and marker gone
#[tokio::test] async fn deleting_synced_data_wipes_the_vault_and_turns_off()
#[tokio::test] async fn deleting_an_already_gone_vault_still_turns_off()
#[test] fn recovery_kit_debug_output_hides_the_code()
#[test] fn verify_password_accepts_the_password_and_refuses_another()      // services/auth.rs, existing key-file test pattern
```

- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::engine::tests::setup services::auth` PASS.
- [ ] **Step 5: Wire types (ruling P15).** Register `SyncPlan`, `SyncAccount`, `RecoveryKit` in `collect_types()`; `cargo test generate_bindings -- --ignored`; mirror them in `shared/schema.ts`; add the three keys to the ten locales; `npm run typecheck`, `npx vitest run src/i18n/`; gates.
- [ ] **Step 6: Commit** (with the regenerated `shared/generated-types.ts`) `feat(sync): first-device setup, turning sync off and deleting synced data`.

---

### Task 17: Relay change — development entitlement grant (Moony-relay)

**Relay change, its own repository:** `/Users/filipkral/Programování/FinanceApp/Moony-relay`,
branch `feat/dev-grant` from `main` (run `git branch --show-current` there). Not deployed.

**Files:**
- Create: `src/dev.ts`, `test/dev-grant.test.ts`
- Modify: `src/index.ts` (route), `src/secrets.d.ts` (`DEV_GRANT?: string`), `vitest.config.ts` (binding `DEV_GRANT: "1"`), `.dev.vars.example` (`DEV_GRANT=1`), `DESIGN.md` ("Development only" table), `README.md` (one line under Commands)

**Interfaces:**
- Produces: `POST /v1/dev/grant`, header `Moony-Account`, body `{"days": n}` with `1 ≤ n ≤ 365` →
  `200 {"paidUntil": <unix s>}`, applying `{ subscriptionId: "dev_grant", paidUntil: now + n·DAY,
  version: Date.now() }` through `accountStub(env, account).applySubscription`; days outside
  `1..=365` → `400 invalid_days`; an unknown account → `404 account_not_found`. When
  `env.DEV_GRANT !== "1"` the route answers `404 {"error": "not_found"}` exactly like an unknown
  path. `wrangler.jsonc` stays without `DEV_GRANT`, so a deployed Worker never has it.
- `export function devGrantEnabled(env: { DEV_GRANT?: string }): boolean` in `src/dev.ts`.

- [ ] **Step 1: Failing tests** (`test/dev-grant.test.ts`, using `api`, `newAccount` and `now` from `test/helpers.ts`): `grants a paid period to an account` (then `GET /v1/accounts/me` reports `active: true`); `refuses days outside 1..365`; `answers 404 for an unknown account`; `devGrantEnabled is false without the variable` (the pure function with `{}` and `{DEV_GRANT: "0"}`).
- [ ] **Step 2: Run** `npm test` (in Moony-relay) → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npm test && npm run typecheck` → PASS.
- [ ] **Step 5: Commit** (in Moony-relay) `feat: development-only entitlement grant for local testing` with the trailer. Do not merge or push; the owner decides. Tasks 18 and 23 need this branch checked out in Moony-relay.

---

### Task 18: End to end against `wrangler dev`

**Files:**
- Create: `src-tauri/src/services/sync/relay_e2e.rs` (`#[cfg(test)] mod relay_e2e;` in `services/sync/mod.rs`); Modify: `relay/http.rs` (`dev_grant`)

**Interfaces:**
- Consumes: Tasks 6, 13, 15, 16, 17.
- Produces: `#[cfg(debug_assertions)] pub async fn dev_grant(&self, account: &str, days: u32) -> RelayResult<i64>` on `HttpRelay` (ruling P18: compiled only into debug builds; the release relay client has no such call). Ignored tests reading `MOONY_RELAY_E2E` (e.g. `http://127.0.0.1:8787`); without it the test fails with the message `set MOONY_RELAY_E2E to a running wrangler dev of Moony-relay (branch feat/dev-grant, npm run dev there, with DEV_GRANT=1 in .dev.vars)`.

- [ ] **Step 1: Write the tests**

```rust
#[tokio::test] #[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn two_devices_sync_through_the_relay() {
    // device A: create_account, dev_grant(30), recovery_kit, finish_setup("A"); add a bank account,
    // a category and 20 transactions; run_round. Device B: join_by_replace(A's credentials, "B"),
    // run_round. Both edit offline (same transaction's note on both, different columns elsewhere),
    // A round, B round, A round → assert_converged(&[A, B]); the newer note wins on both.
}
#[tokio::test] #[ignore = "…"] async fn an_unpaid_account_cannot_create_a_vault()   // a second account, never granted → finish_setup → Lapsed, sync_state not enabled
#[tokio::test] #[ignore = "…"] async fn deleting_synced_data_makes_the_other_device_pause()
#[tokio::test] #[ignore = "…"] async fn snapshot_round_trip_through_r2()             // A uploads, B join_by_replace reads it
```

- [ ] **Step 2: Run** in Moony-relay (branch `feat/dev-grant`) `cp .dev.vars.example .dev.vars && npm run dev` (port 8787), then `cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e -- --ignored --test-threads=1` → PASS (iterate on failures; they are bugs in Tasks 6–16).
- [ ] **Step 3: Run** without the variable: `cargo test --lib relay_e2e -- --ignored` → the clear failure message; `cargo test --lib` (ignored tests skipped) → PASS; `cargo build --release` compiles without `dev_grant`; gates.
- [ ] **Step 4: Commit** `test(sync): two devices end to end through wrangler dev`.

---

### Task 19: Commands, wrappers, the debug measurement fixture

**Files:**
- Modify: `src-tauri/src/commands/sync.rs`, `src-tauri/src/lib.rs` (`generate_handler!` under `// Device sync`), `src/lib/tauri-api.ts`, `src-tauri/src/commands/auth.rs` (`delete_account` calls `delete_vault_best_effort` while the profile is still open and sync is on, before `stop_engine` and the wipe), `src-tauri/src/services/sync/capture/mod.rs` (`pub mod bench;`), `src/i18n/locales/<10>/common.json`
- Create: `src-tauri/src/services/sync/capture/bench.rs` (ruling P10, plan's call P20: a new production fixture; nothing moves out of the convergence harness, whose statement-import measurement stays test-only)

**Interfaces:**
- Produces: every command `#[tauri::command] pub async fn`, first line `require_feature()?` except the flag itself; relay from `keys::account`'s URL or `relay::http::default_base_url()`; service results `SyncResult` mapped with `SyncError::to_app_error()` (ruling P2).

| Command | Args | Returns |
|---|---|---|
| `sync_feature_enabled` | — | `bool` |
| `get_sync_status` | — | `SyncStatus` |
| `sync_create_account` | — | `SyncAccount` |
| `sync_get_account` | — | `SyncAccount` |
| `sync_start_checkout` | `plan: SyncPlan` | `String` (URL; the frontend opens it with the opener plugin) |
| `sync_prepare_recovery_kit` | — | `RecoveryKit` |
| `sync_copy_recovery_code` | — | `bool` (true: copied natively, cleared after 60 s if unchanged; false: unsupported here) |
| `sync_finish_setup` | `deviceName: String, kitSaved: bool` | `SyncStatus` (starts the engine) |
| `sync_now` | — | `()` |
| `sync_show_recovery_kit` | `password: String` | `RecoveryKit` (after `auth::verify_password`) |
| `sync_turn_off` | — | `()` (stops the engine first) |
| `sync_delete_synced_data` | `password: String` | `()` (`auth::verify_password` first, then stops the engine) |
| `sync_repair_device` | — | `String` (backup path; stops the engine, repairs, starts it again) |
| `sync_dev_grant` | `days: u32` | `SyncAccount` (`validation.syncDevOnly` unless `feature::dev_tools()`; the call itself is `#[cfg(debug_assertions)]`) |
| `sync_debug_capture_overhead` | `rows: u32` | `String` (P20; `validation.syncDevOnly` in release) |

No command logs its arguments. `sync_copy_recovery_code` builds the code from `sync_state` (it never
crosses IPC for copying), calls `Keychain::copy_secret_text`, and spawns a 60 s task that calls
`clear_clipboard_if_unchanged(token)`. `capture/bench.rs`:
`pub struct Overhead { pub without: Duration, pub with: Duration, pub queued: u64 }` and
`pub fn insert_overhead(rows: usize) -> Result<Overhead>` open two in-memory connections with
`crate::db::run_migrations`, enable capture on the second (`state::enable(conn, DeviceId::random())`),
insert one bank account and `rows` `bank_transactions` in one transaction on each (plain `INSERT`s
with fixed values), and return both times plus `queued` = the captured connection's `sync_outbox`
count after the inserts minus before them; the command formats `"{rows} rows: {a:.1} ms without capture, {b:.1} ms with
({pct:.0}% more)"`. `src/lib/tauri-api.ts` gets `deviceSyncApi` with one method per command
(`featureEnabled`, `getStatus`, `createAccount`, `getAccount`, `startCheckout`, `prepareRecoveryKit`,
`copyRecoveryCode`, `finishSetup`, `syncNow`, `showRecoveryKit`, `turnOff`, `deleteSyncedData`,
`repairDevice`, `devGrant`, `debugCaptureOverhead`). Keys added in this task to the ten
`common.json`: `syncUnavailable`, `syncDevOnly`.

- [ ] **Step 1: Failing tests** `fn sync_commands_refuse_while_the_feature_is_off()` (a pure `require_feature()` unit test in `commands/sync.rs` with the flag function injected) and `fn insert_overhead_measures_both_runs()` in `capture/bench.rs` (`let o = insert_overhead(50).unwrap(); assert!(o.without > Duration::ZERO && o.with > Duration::ZERO); assert_eq!(o.queued, 51);` — 50 transactions and the account).
- [ ] **Step 2: Implement** the 15 commands (thin: validate → service → map), register them, add the wrappers and the keys; `delete_account` wiring.
- [ ] **Step 3: Run** `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo test generate_bindings -- --ignored && git diff --exit-code shared/generated-types.ts` (no change: the types were registered in Tasks 13 and 16), `npm run typecheck`, `npm run lint`, `npm test` → PASS.
- [ ] **Step 4: Commit** `feat(sync): sync commands and API wrappers`.

---

### Task 20: Frontend data layer — hooks, events, refresh ritual, hidden section, indicator

**Files:**
- Create: `src/hooks/use-device-sync.ts`, `src/hooks/use-device-sync-mutations.ts`, `src/hooks/use-device-sync-events.ts`, `src/lib/device-sync-refresh.ts` (+ `.test.ts`), `src/utils/device-sync-indicator.ts` (+ `.test.ts`), `src/components/shell/SyncStatusIndicator.tsx`, `src/components/settings/sync/SyncSection.tsx`, `src/i18n/locales/<10>/deviceSync.json`
- Modify: `src/components/settings/settings-sections.ts` (+ test), `src/components/settings/SettingsLayout.tsx`, `src/pages/Settings.tsx`, `src/components/shell/TopBar.tsx`, `src/components/shell/AppShell.tsx` (mount `useDeviceSyncEvents()`), `src/i18n/index.ts` (namespace `deviceSync`), `src/i18n/locales/<10>/settings.json` (`sections.sync`, `sections.description.sync`)

**Interfaces:**
- Produces:

```ts
// use-device-sync.ts — keys ['device-sync-feature'], ['device-sync-status'], ['device-sync-account']
export function useDeviceSyncFeature(): boolean;            // false while loading
export function useDeviceSyncStatus(): UseQueryResult<SyncStatus>;
export function useDeviceSyncAccount(opts?: { pollWhileInactive?: boolean }): UseQueryResult<SyncAccount>; // refetchInterval 3000 while !active
// device-sync-refresh.ts
export async function refreshAfterRemoteChanges(qc: Pick<QueryClient, 'invalidateQueries'>, recordSnapshot: () => Promise<unknown>): Promise<void>;
    // invalidateQueries() (all) → await recordSnapshot() → invalidateQueries({ queryKey: ['portfolio-history'] })  (P16, rule 7)
// device-sync-indicator.ts
export type SyncIndicator = { tone: 'fresh' | 'stale' | 'neutral'; labelKey: string } | null;
export function syncIndicator(status: SyncStatus | undefined): SyncIndicator;   // null for off/settingUp/undefined
// settings-sections.ts
export type SettingsSectionId = … | 'sync';
export function visibleSettingsSections(syncFeature: boolean): readonly SettingsSection[];
export function resolveSettingsSection(location: string, syncFeature?: boolean): SettingsSectionId; // 'sync' → 'general' when hidden
```

`use-device-sync-events.ts`: `listen('sync-status-changed')` → `setQueryData(['device-sync-status'], payload)`;
`listen('sync-data-changed')` → `refreshAfterRemoteChanges(queryClient, portfolioApi.recordSnapshot)`;
both `unlisten` in the cleanup (frontend rule 7). The section sits after Integrations
(`/settings/sync`, icon `RefreshCw`). `SyncSection.tsx` in this task (ruling P9) renders only what
exists now: one `SettingsCard` with the phase line (`status.phase.<phase>`) and, when there is one,
the problem sentence (`status.problem.<problem>`); Tasks 21 and 22 replace its body by phase. The
indicator renders left of `MilestonesStatus` as the design-system `.status` (dot + text) and links to
`/settings/sync`; nothing renders while the flag is off or the phase is `off`/`settingUp`. Indicator
labels: `indicator.synced` (fresh), `indicator.syncing` (neutral), `indicator.paused` (stale),
`indicator.error` (stale), `indicator.offline` (stale).

- [ ] **Step 1: Failing tests**

```ts
// device-sync-refresh.test.ts
it('invalidates everything, records a snapshot, then refreshes portfolio history', …) // call order recorded by fakes
// device-sync-indicator.test.ts
it('hides the indicator when sync is off or being set up', …)
it('maps every phase and the offline problem to a tone and label', …)   // table over SyncPhase × {offline}
// settings-sections.test.ts
it('lists the sync section only when the feature is on', …)
it('resolves /settings/sync to general while the feature is off', …)
```

- [ ] **Step 2: Run** `npx vitest run src/lib/device-sync-refresh.test.ts src/utils/device-sync-indicator.test.ts src/components/settings/settings-sections.test.ts` → FAIL.
- [ ] **Step 3: Implement**; add every new string (phase lines, every `status.problem.*`, indicator labels, section name and description) to all ten `deviceSync.json` and `settings.json` (English and Czech written, the other eight translated with `docs/standards/i18n-glossary.md`).
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): hidden Settings section, status indicator and remote-change refresh`.

---

### Task 21: Frontend setup flow and the Recovery Kit

**Files:**
- Create: `src/components/settings/sync/SyncIntroCard.tsx`, `SyncSetupDialog.tsx`, `RecoveryKitPanel.tsx`, `src/utils/recovery-kit.ts` (+ `.test.ts`)
- Modify: `src/components/settings/sync/SyncSection.tsx` (phases `off`/`settingUp` render `SyncIntroCard`; the dialog's open state lives here), `src/hooks/use-device-sync-mutations.ts`, `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
- Produces:

```ts
export interface RecoveryKitCopy { title: string; intro: string; accountLabel: string; accountNote: string;
  codeLabel: string; codeNote: string; qrLabel: string; differsFromRecoveryKey: string; created: string }
export function recoveryKitHtml(kit: RecoveryKit, copy: RecoveryKitCopy): string; // full <!doctype html> page, inline CSS with @media print and @page, the QR as <img src="data:image/svg+xml;base64,…">, every copy string HTML-escaped, no external URL
export function recoveryKitText(kit: RecoveryKit, copy: RecoveryKitCopy): string;
```

Flow (one dialog, steps; design-system modal 560 px):
1. **Account** — `sync_create_account`; shows the number in the dark key box with Copy, and the §11.2
   sentence ("This number is your sync account. Anyone who has it can pay for it and see whether it
   is paid. It does not open your data."). Resuming with a stored account starts here with it shown.
2. **Pay** — a two-option segmented control "Monthly" / "Yearly" (no prices), "Continue to payment"
   opens `sync_start_checkout(plan)`'s URL with `@tauri-apps/plugin-opener` `openUrl`; the step polls
   `useDeviceSyncAccount({ pollWhileInactive: true })` and moves on when `active`. In debug builds
   with `status.devTools`, a ghost "Grant a test subscription" calls `sync_dev_grant(30)`.
3. **Recovery Kit** — `sync_prepare_recovery_kit`; QR image, grouped code, account number; "Save as
   page…" (`save` dialog, `.html`, `writeTextFile`) and "Save as text…" (`.txt`); "Copy code" calls
   `deviceSyncApi.copyRecoveryCode()`; when it answers `false` the UI copies with
   `navigator.clipboard.writeText` and shows "Clear your clipboard after pasting the code."; a
   checkbox "I saved my Sync Recovery Kit" enabled only after a save or a copy; one line that says
   it differs from the device's recovery key.
4. **This device** — device name field prefilled by platform (`setup.defaultName.macos` "Mac",
   `.windows` "Windows PC", `.linux` "Linux PC"), "Turn on sync" → `sync_finish_setup`; success toast
   "Sync turned on".

- [ ] **Step 1: Failing tests** (`recovery-kit.test.ts`)

```ts
it('builds a self-contained page with the code, account and QR', …) // contains the grouped code, the account, 'data:image/svg+xml;base64,', '@media print', no 'http'
it('escapes every copy string', …)                                 // copy with '<script>' → '&lt;script&gt;'
it('writes the same facts as plain text', …)
```

- [ ] **Step 2: Run** `npx vitest run src/utils/recovery-kit.test.ts` → FAIL.
- [ ] **Step 3: Implement** the util, the cards, the dialog and the mutations (`useCreateSyncAccount`, `useStartCheckout`, `usePrepareRecoveryKit`, `useCopyRecoveryCode`, `useFinishSyncSetup`, `useDevGrant`; each invalidates `['device-sync-status']` and `['device-sync-account']`); strings in all ten locales.
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): first-device setup dialog and the Sync Recovery Kit`.

---

### Task 22: Frontend sync-on states, turning off, deleting

**Files:**
- Create: `src/components/settings/sync/SyncStatusCard.tsx`, `SyncProblemAlert.tsx`, `SyncDangerCard.tsx`
- Modify: `src/components/settings/sync/SyncSection.tsx` (phases `idle`/`syncing`/`paused`/`error` render the three cards in place of Task 20's phase card), `src/hooks/use-device-sync-mutations.ts`, `src/components/settings/DangerZoneCard.tsx` (one extra sentence in the Delete Account confirmation when sync is on), `src/i18n/locales/<10>/deviceSync.json`, `src/i18n/locales/<10>/settings.json` (if the Delete Account sentence lives there)

**Interfaces:**
- Consumes: `deviceSyncApi.{syncNow, showRecoveryKit, copyRecoveryCode, turnOff, deleteSyncedData, repairDevice}`.
- Produces: mutations `useSyncNow`, `useShowRecoveryKit`, `useTurnOffSync`, `useDeleteSyncedData`, `useRepairSyncDevice` (each invalidates `['device-sync-status']`; turn off and delete also `['device-sync-account']`).

UI: **Status card** — phase line, last synced time (`useFormat().day`/time), pending changes count,
"Sync now" (primary), "Show Recovery Kit" (asks the device password, then shows
`RecoveryKitPanel` read-only, Copy through `copyRecoveryCode`). **Problem alert** (`ui/alert.tsx`,
next to the cause, with its action): `lapsed` "Sync paused — renew to continue" + "Synced data will
be deleted on {{date}}" + "Renew" (opens checkout); `vaultGone` "Your synced data is gone from the
server. Turn sync off on this device?" with "Turn off sync" and "Try again" (`sync_now`);
`captureBroken` with "Repair this device" (confirm dialog explaining the automatic backup and the
replace; success toast names the backup file); `newerSchema` "Update Moony on this device to keep
syncing"; the rest show the Task 20 `status.problem.*` sentence. **Danger card** — "Turn off sync on
this device" (AlertDialog: "Local data stays. Other devices keep syncing. Turning sync on again later
replaces this device's data with the synced data or starts a new vault.") and "Delete synced data"
(password, then AlertDialog: deletes the vault for every device; local data on every device stays).

- [ ] **Step 1: Failing test** — none new in node (components are out of scope); the gates below are the check. Confirm `every_sync_validation_key_has_an_english_translation` (Task 4) and the parity gate still pass.
- [ ] **Step 2: Implement** the cards, alerts, mutations and copy in all ten locales.
- [ ] **Step 3: Run** `cargo test --lib every_sync_validation_key`, `npm test` (parity gate), `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 4: Commit** `feat(sync): sync status, problems, turn off and delete in Settings`.

---

### Task 23: Docs, phone capture measurement, final verification

**Files:**
- Modify: `docs/architecture/database.md` (030 columns, if Task 3 left anything), `docs/architecture/overview.md` ("Device sync (hidden until release)": modules, the flag, `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 npm run tauri dev`, the e2e command and the Moony-relay branch it needs), `docs/standards/rust-backend.md` (section 12 "Sync registry…" gains: "network code of sync goes through `services/sync/relay/http.rs`; the engine never holds the session mutex across a network call; sync errors travel as `SyncError` until the command boundary")
- Create: `docs/architecture/decisions/0025-device-sync-relay-client.md`; Modify: `docs/architecture/decisions/README.md` (index)

- [ ] **Step 1: Write the docs.** ADR 0025 records: the envelope and stream byte layouts (corrections 1–3); pull before push and snapshot only after a clean pull (corrections 19, 20); the barrier marker and the one-marker flag; re-stamp of column clocks; the quarantine cursor; the keychain binding and its limit; the Recovery Kit format; the runtime flag; the HTTP hardening; that migration 030 raises the schema number to 30 so a 029 build answers `NewerSchema` (correction 23); and (ruling P17) that a profile backup holds the vault keys: restored on another machine it carries them there, the restore marker makes that copy re-join under a new device ID, and the keys stay the same person's — accepted, the same trust as the backup itself. Living docs carry no line numbers or file sizes.
- [ ] **Step 2: Phone measurement (P20).** Build the iOS simulator debug app (`npm run tauri ios dev`), open Safari's Web Inspector on it, run `await window.__TAURI_INTERNALS__.invoke('sync_debug_capture_overhead', { rows: 5000 })` three times; record the three strings in the phase 2 ledger `.superpowers/sdd/2026-11-07-device-sync-phase-2-relay-client/progress.md`. Ask the owner to repeat it on his iPhone and record that too. Above 1 s extra per 5,000 rows on the device → a follow-up entry, not a fix in this phase.
- [ ] **Step 3: Manual desktop check against `wrangler dev`** (P21; Moony-relay on `feat/dev-grant`): one desktop dev build with a demo profile (never the real one), `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787`: the section appears; create account → grant test subscription → save the Kit (open the saved page in a browser and print preview it on one page) → Copy code, wait 60 s, the clipboard is empty → turn on → indicator "Synced"; stop the relay → "Offline", restart → back to "Synced"; delete synced data → section returns to the intro. Without `MOONY_DEV_SYNC` the section and the indicator are absent.
- [ ] **Step 4: Full gates** from the worktree: `npm run lint && npm run typecheck && npm run format:check && npm test`; `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`; `cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts`; the e2e run of Task 18 once more.
- [ ] **Step 5: Commit** `docs(sync): phase 2 architecture, ADR 0025 and development notes`.
