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
the lock, talks to the relay unlocked, and merges each pulled changeset in its own transaction. The
Tauri layer (`commands/sync.rs`) is thin; the React side gets one hidden Settings section, a status
indicator, and an event listener that runs the rule-7 refresh after remote changes.

**Tech Stack:** Rust 2021, rusqlite 0.40 (SQLCipher), tokio 1, reqwest 0.13 through
`services/http.rs`, RustCrypto (`chacha20poly1305` 0.11, `hkdf` 0.13, `sha2` 0.11), `x25519-dalek` 3,
`qrcode` 0.14 (SVG), flate2 (raw deflate), base64 0.23; React 19, TanStack Query 5, i18next (ten
locales), Vitest (node). Moony-relay (Cloudflare Worker, TypeScript, Vitest in workerd).

**Spec:** `docs/specs/2026-11-06-device-sync-client-design.md` — read all of it, starting with §16a
(owner answers) and §20 (as built). Also `docs/architecture/decisions/0024-device-sync-capture-clocks-merge.md`,
the phase 1 record in `.superpowers/sdd/device-sync-phase-1-record/` (`progress.md`,
`preflight-rulings.md`, `rulings.md`), and the relay contract
`/Users/filipkral/Programování/FinanceApp/Moony-relay/DESIGN.md` (protocol v1.1).

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
   header carries it, and a device with nothing to send after crossing a barrier pushes an empty
   marker changeset (`rows: []`). `BelowBarrier` applies only to envelopes placed after the marker.
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
  `#[serde(rename = "camelCase")]`; thin commands, logic in `services/`, `AppError` only; no
  `.unwrap()` outside tests; migration 030 is append-only and 029 is never edited; every string in
  all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`); timestamps are unix
  seconds `INTEGER`.
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
- Sizes: envelope overhead 48 bytes; relay op limit 196,608 bytes (192 KiB); push ≤ 100 ops and
  ≤ 3 MiB of base64 per request; pull limit 500; snapshot ≤ 104,857,600 bytes sealed; decoded
  changeset JSON ≤ 4 MiB.
- Engine timing: debounce poll 5 s, forced after 60 s; periodic round every 300 s; back-off 30 s
  doubling to 1,800 s, reset by a clean round; snapshot policy checked at most hourly, due at
  2,000 envelopes or 8 MiB since the vault's newest snapshot, at most once per 24 h per device,
  after a random delay of 0–3,600 s.
- Relay: default base URL `https://sync.moonyfinance.com` (placeholder until phase 6); debug builds
  read `MOONY_RELAY_URL`; `http://` is allowed only for `localhost` / `127.0.0.1`. The URL used to
  create the account is stored in `sync_state.relay_url` and used for that profile from then on.
- Events: `sync-data-changed` (no payload; `merge::DATA_CHANGED_EVENT`) and `sync-status-changed`
  (payload `SyncStatus`).
- No prices in the app (§16a): plans read "Monthly" / "Yearly"; Polar's page shows the amount.
- Logging: never log account numbers, vault IDs, tokens, keys or recovery codes above `debug`.
- Tests: Rust services use `services::sync::test_support::migrated()` (in-memory, real migrations);
  never the `Database` struct. Frontend tests are pure functions in node (no jsdom).
- Every task ends with its filter test, then, from `src-tauri/`: `cargo test --lib`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; frontend tasks also run
  `npm test`, `npm run typecheck`, `npm run lint`. Never `--no-verify`.
- New functions a later task calls are `pub` (a `pub(crate)` item unused until then fails clippy).
- Files stay under ~800 lines; `db/migrations.rs` and the locale files are exempt.
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
4. **A captive portal or proxy answers with HTML or truncated JSON:** a typed protocol error, a
   back-off and an "offline / relay error" status, never a panic or a stop — Task 6
   `an_html_answer_is_a_protocol_error`.
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
| P4 | Recovery code checksum = the first 15 bits of SHA-256(`moony recovery code v1` ‖ vault_id ‖ key), 3 Crockford characters; the 384-bit payload is padded with one zero bit to 77 characters. |
| P5 | Recovery Kit as printable HTML + plain text (correction 11); QR content `moony-recover:1:<account>:<code>`, SVG rendered in Rust. |
| P6 | Local-write trigger by outbox polling (correction 12). |
| P7 | A round still pushes after a pull stop (`Gap`, `Invalid`, undecryptable, `NewerSchema`, missing parent): own changes never depend on what was not applied. Only a `Clone` changes the push (it re-joins first). |
| P8 | Back-off applies to network, 5xx, 429 and protocol errors; "Sync now" bypasses it. |
| P9 | `404 vault_not_found` persists a pause (`vault_gone_at`); "Sync now" retries one `GET` vault and clears it on success; "Turn off" ends it. |
| P10 | `403 token_read_only` and `401` stop pushing with problem `keyRotated` / `unauthorized`; their resolution is phase 5. |
| P11 | Quarantine is shown (`heldDevices`) but not resolved in phase 2; resolution comes with the device list (phase 3). |
| P12 | The capture-broken repair (backup → drain round → join by replace under a new device ID) is built now, since the join core is needed for the end-to-end test anyway. |
| P13 | The keychain binding writes a hash `SHA-256("moony sync binding v1" ‖ device_id)`; `sync_state.binding_written` tells "never written here" from "missing on this machine". |
| P14 | A re-join copies the name and platform of the old device row into the new one; the old row stays (it may still be the original's ID). |
| P15 | Device names come from the UI (prefilled with a translated platform default, editable, 1–60 characters). |
| P16 | `sync-data-changed` invalidates every query (a merge can touch any table), records today's snapshot and invalidates `["portfolio-history"]`. |
| P17 | Status phases: `off`, `settingUp`, `idle`, `syncing`, `paused` (vault gone, lapsed, quota, newer schema, offline, rate limited), `error` (everything else). |
| P18 | The engine runs DB steps directly in its tokio task (as `mcp_refresh` does); a pull page is at most 500 envelopes, each merged in its own short transaction. |
| P19 | The outbox re-sort cost (phase 1 note) is measured, not optimised: Task 10's ignored measurement must drain a 50,000-row outbox in under 30 s release; only above that does a follow-up cache the order. |
| P20 | Phone capture overhead is measured with a debug-only command run from the Web Inspector console on the simulator and on the owner's iPhone; optimising triggers is a follow-up only if a 5,000-row import costs over 1 s more with capture on the device. |
| P21 | Two-desktop manual testing waits for phase 3's recovery-code join; phase 2's two-device proof is the end-to-end Rust test against `wrangler dev`. |

## Dependencies

Add to `src-tauri/Cargo.toml` under `# Cryptography` (checked 2026-11-07 with `cargo search` and a
scratch resolution: one RustCrypto family with `aead 0.6`, `cipher 0.5`, `digest 0.11`,
`rand_core 0.10`; no duplicate versions):

```toml
chacha20poly1305 = { version = "0.11", default-features = false, features = ["alloc", "zeroize"] }  # Device sync envelopes and snapshot streams (XChaCha20-Poly1305, spec §9.2)
hkdf = "0.13"                    # Device sync key derivation (already in Cargo.lock through another crate)
x25519-dalek = { version = "3", features = ["static_secrets"] }  # Device sync: this device's key pair (spec §11.1)
qrcode = { version = "0.14", default-features = false, features = ["svg"] }  # Sync Recovery Kit QR (SVG only)
```

New `Cargo.lock` packages: `chacha20poly1305 0.11.0`, `poly1305 0.9.1`, `x25519-dalek 3.0.0`,
`curve25519-dalek 5.0.0` (plus its target-specific helpers), `qrcode 0.14.1`. `hkdf 0.13.0`,
`chacha20 0.10.2`, `aead 0.6.1`, `sha2 0.11.1`, `zeroize 1.9.1` are already locked. Nonces and keys
come from `rand 0.10` (`rand::rng().fill_bytes`), so `chacha20poly1305`'s `getrandom` feature stays
off. Task 14 adds the `windows` feature `Win32_Security_Cryptography` to
`plugins/moony-keychain/Cargo.toml`. `cargo audit` (`scripts/cargo-audit.sh`) must stay green.

## Phase 1 carries and where they land

| Carry (phase 1 record) | Task |
|---|---|
| Capture-broken sessions: no push/pull, Settings shows it, recovery = re-join by replace after a backup (progress L61) | 3 (column), 13 (stop), 15 (repair), 22 (UI) |
| Locale entries for `validation.sync*` / `validation.syncSnapshot*` (L67) | 22 |
| Raw deflate in the envelope, same as `padded_deflated_len` (L68) | 1 |
| Changeset chain shape (`seq` 1 with `prev`, `seq` > 1 without, duplicate `devs`) (L70) | done in phase 1 (`from_json`, `judge` → `Invalid`); 9 stops on `Invalid` without moving the cursor |
| Outbox re-sort O(n²/k) at 50k rows (L81, phase 2 note) | 10 (measurement, P19) |
| Barrier placement by relay order: record the marker's relay position, `BelowBarrier` only beyond it (L130) | 8 |
| Clone → re-join before any push in the round; re-judge the triggering envelope; `Gap`/`Invalid`/`NewerSchema` stop without advancing the cursor; `Held` keeps the cursor; no old-ID push in flight (L131) | 9 (verdicts), 13 (rounds are serial) |
| Empty confirmed prefix with trigger seq > 1 writes no head (L136) | 8 |
| Merge stop reason collapsed to one key on restore (L142, phase 2 note) | 11 |
| Re-join / barrier re-queues never ship a child ahead of its parent's incarnation (L170) | 8 (tests through `build_next`) |
| Joins start the cursor at the snapshot seq (L173) | 15 |
| Dump with exclusive connection use (L176; T18e "engine pushes and pulls before dumping") | 11 |
| Capture overhead profiled on a phone (L202) | 23 (P20) |
| Re-join re-stamps re-queued own rows with fresh clocks under the new ID (L203) | 8 (correction 7) |
| Registry test for fixed seed values after 029 (L209) | 3 |
| `build_next` + seal + store in `sync_unsent` in one transaction (R11) | 10 |

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/envelope/mod.rs` | HKDF labels, op envelope seal/open, op ID, hash |
| `services/sync/envelope/stream.rs` | STREAM sealer (`Write`) and opener for snapshots |
| `services/sync/envelope/tests.rs` | Known-answer, tamper and key-separation tests |
| `services/sync/state.rs` (modify) | New `sync_state` columns: barrier marker, pauses, policy counters, binding flag |
| `services/sync/keys.rs` | `VaultId`, `VaultKeys`, `Credentials`, device key pair, their storage |
| `services/sync/devices.rs` | This device's `sync_devices` row |
| `services/sync/recovery.rs` | Recovery code (Crockford base32 + checksum), QR payload and SVG |
| `services/sync/relay/mod.rs` | `Relay` trait, wire types, `RelayError` and its mapping |
| `services/sync/relay/http.rs` | `HttpRelay` over `services/http.rs`, base URL rules |
| `services/sync/relay/fake.rs` (`#[cfg(test)]`) | `FakeRelay` with the relay's semantics and fault knobs |
| `services/sync/chain.rs` (modify) | Relay positions, barrier marker, empty-prefix head, re-stamp |
| `services/sync/journal/mod.rs` (modify) | `build_marker` |
| `services/sync/snapshot/{mod,stream}.rs` (modify) | Header `barrierSeq`; dump doc line |
| `services/sync/engine/mod.rs` | `ConnAccess`, `EngineEvents`, round entry point, `RoundReport` |
| `services/sync/engine/pull.rs` | Pull pages, per-envelope verdict handling, re-join |
| `services/sync/engine/push.rs` | Seal batches into `sync_unsent`, push, acknowledge, marker |
| `services/sync/engine/snapshots.rs` | `410` restore by merge, upload policy, sealed dump |
| `services/sync/engine/runtime.rs` | Loop, triggers, back-off, start/stop, startup checks |
| `services/sync/engine/status.rs` | `SyncStatus` from state + runtime |
| `services/sync/engine/tests/*.rs` (`#[cfg(test)]`) | Engine tests with `FakeRelay` |
| `services/sync/binding.rs` | Restore marker, keychain binding check and write |
| `services/sync/join.rs` | Join a vault by replace; capture-broken repair |
| `services/sync/setup.rs` | Account, checkout, Kit, finish setup, turn off, delete synced data |
| `services/sync/feature.rs` | The hidden flag |
| `services/sync/relay_e2e.rs` (`#[cfg(test)]`, ignored) | Two devices through `wrangler dev` |
| `services/sync/capture/bench.rs` | The 5,000-row import measurement, callable from a debug command |
| `src-tauri/src/models/sync.rs` | `SyncStatus`, `SyncPhase`, `SyncProblem`, `SyncAccount`, `SyncPlan`, `RecoveryKit` |
| `src-tauri/src/commands/sync.rs` | Thin commands, engine start/stop glue, Tauri adapters |
| `plugins/moony-keychain/…` | Binding seal/open natives (macOS Secure Enclave, Windows DPAPI) |
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
(`Padding`). Keys are `Zeroizing`.

- [ ] **Step 1: Add the dependencies** of the Dependencies section; `cargo build` resolves them; diff `Cargo.lock` shows only the listed new packages.

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
    assert!(journal::SINGLE_ROW_LIMIT + ENVELOPE_OVERHEAD <= MAX_OP_BYTES);
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
chunks (`Malformed`).

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
pub struct EngineMarks { pub capture_broken_at: Option<i64>, pub vault_gone_at: Option<i64>,
    pub pulled_bytes: u64, pub pulled_bytes_base: u64, pub snapshot_uploaded_at: Option<i64>,
    pub binding_written: bool, pub last_success_at: Option<i64>, pub last_error: Option<String> }
pub fn marks(conn: &Connection) -> Result<Option<EngineMarks>>;
pub fn mark_capture_broken(conn: &Connection, at: i64) -> Result<()>;
pub fn clear_capture_broken(conn: &Connection) -> Result<()>;
pub fn set_vault_gone(conn: &Connection, at: Option<i64>) -> Result<()>;
pub fn set_cursor(conn: &Connection, cursor: u64) -> Result<()>;          // never lowers: MAX(cursor, ?)
pub fn add_pulled_bytes(conn: &Connection, bytes: u64) -> Result<()>;
pub fn rebase_pulled_bytes(conn: &Connection, vault_snapshot_seq: u64) -> Result<()>; // base := seq, bytes := 0 when the base differs
pub fn set_barrier_seq(conn: &Connection, seq: Option<u64>) -> Result<()>;
pub fn record_snapshot_uploaded(conn: &Connection, seq: u64, max_hlc: u64, at: i64) -> Result<()>;
    // snapshot_seq = MAX, snapshot_hlc = MAX, snapshot_uploaded_at = at, pulled_bytes = 0, pulled_bytes_base = seq
pub fn record_success(conn: &Connection, at: i64) -> Result<()>;         // last_success_at, last_error = NULL
pub fn set_binding_written(conn: &Connection, written: bool) -> Result<()>;
pub fn reset_chain(conn: &Connection) -> Result<()>;  // next_seq = 1, last_hash = NULL, empties sync_own_log and sync_unsent
```

`on_open`: when `open_capture` fails **and** the row is enabled, call `mark_capture_broken(now)` next
to `record_error`. `chain::cross_barrier` sets `barrier_seq = NULL` when it raises the barrier
(Task 8 tests it).

- [ ] **Step 1: Failing tests**

```rust
// db/migrations.rs, mod migration_030
#[test] fn sync_state_gains_the_engine_columns_with_defaults()
// state.rs
#[test] fn set_cursor_never_moves_back()
#[test] fn pulled_bytes_reset_when_the_vault_snapshot_moves() // rebase 10, +100, +50 → 150; rebase 10 → 150; rebase 20, +7 → 7
#[test] fn record_snapshot_uploaded_takes_maxima_and_resets_the_counter()
#[test] fn reset_chain_empties_the_own_log_and_unsent()
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

### Task 4: Keys, credentials and this device's identity

**Files:**
- Create: `src-tauri/src/services/sync/keys.rs`, `src-tauri/src/services/sync/devices.rs`; Modify: `services/sync/mod.rs`

**Interfaces:**
- Consumes: `envelope::vault_token`, `state::ensure_row`.
- Produces:

```rust
// keys.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub struct VaultId(pub [u8; 16]);
impl VaultId { pub fn random() -> Self; pub fn to_b64(&self) -> String; pub fn from_b64(s: &str) -> Result<Self>; }
pub struct VaultKeys { /* current: u32, keys: BTreeMap<u32, Zeroizing<[u8; 32]>> */ }
impl VaultKeys {
    pub fn new_random() -> Self;                                  // epoch 0
    pub fn from_parts(current: u32, keys: BTreeMap<u32, [u8; 32]>) -> Result<Self>;
    pub fn current_epoch(&self) -> u32;
    pub fn current(&self) -> &[u8; 32];
    pub fn get(&self, epoch: u32) -> Option<Zeroizing<[u8; 32]>>;
}
pub struct Credentials { pub relay_url: String, pub account: String, pub vault_id: VaultId, pub keys: VaultKeys }
impl Credentials { pub fn token(&self) -> String; }               // vault_token(current key, vault id)
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

Storage (P3): `vault_id` TEXT base64url, `key_epoch`, `vault_keys` JSON, `device_secret_key` BLOB
(32 bytes, `x25519_dalek::StaticSecret` from random bytes; public = `PublicKey::from(&secret)`).
`sync_devices`: `id` = 32 lowercase hex, `public_key` base64url, `app_version` =
`env!("CARGO_PKG_VERSION")`, `joined_at` = now.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn save_account_creates_a_disabled_row_and_never_changes_the_device()
#[test] fn ensure_vault_secrets_is_generated_once()          // two calls → same VaultId and key
#[test] fn credentials_round_trip_through_sync_state()
#[test] fn vault_keys_json_keeps_every_epoch()
#[test] fn the_token_is_43_base64url_characters_and_matches_the_envelope_derivation()
#[test] fn device_keypair_is_stable_until_renewed()
#[test] fn a_recorded_device_row_is_captured_and_keeps_its_public_key() // enable, record, record again with another name → public_key unchanged, outbox has sync_devices/<hex>
#[test] fn device_names_outside_1_to_60_characters_are_refused()
#[test] fn disable_forgets_the_account_and_every_secret()   // state::disable → keys::account(conn) == None
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::keys services::sync::devices` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): vault credentials and the device key pair in sync_state`.

---

### Task 5: Recovery code and QR

**Files:**
- Create: `src-tauri/src/services/sync/recovery.rs`; Modify: `services/sync/mod.rs`

**Interfaces:**
- Consumes: `keys::VaultId`.
- Produces:

```rust
pub const RECOVERY_CODE_LEN: usize = 80;
pub fn encode_recovery_code(vault_id: &VaultId, key: &[u8; 32]) -> String;   // 80 chars, no separators
pub fn format_recovery_code(code: &str) -> String;                            // 16 groups of 5, joined by '-'
pub fn decode_recovery_code(input: &str) -> Result<(VaultId, Zeroizing<[u8; 32]>)>; // validation.syncRecoveryCodeInvalid
pub fn recovery_qr_payload(account: &str, code: &str) -> String;              // "moony-recover:1:{account}:{code}"
pub fn qr_svg(payload: &str) -> Result<String>;                               // EcLevel::M, quiet zone, min 240×240
```

Alphabet `0123456789ABCDEFGHJKMNPQRSTVWXYZ`; payload = vault_id ‖ key (48 bytes) big-endian with one
zero padding bit → 77 characters; checksum per P4 (bits 14–10, 9–5, 4–0 of
`u16::from_be_bytes(h[0..2]) >> 1`). Decode uppercases, maps `I`/`L` → `1`, `O` → `0`, ignores `-`
and whitespace, and requires 80 characters, a zero padding bit and a matching checksum.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn recovery_code_matches_frozen_vectors() {
    let code = encode_recovery_code(&VaultId([0x22; 16]), &[0x11; 32]);
    assert_eq!(format_recovery_code(&code), "48H24-8H248-H248H-248H2-48H24-88H24-8H248-H248H-248H2-48H24-8H248-H248H-248H2-48H24-8H248-H2XSY");
    let ids: [u8; 16] = std::array::from_fn(|i| i as u8);
    let key: [u8; 32] = std::array::from_fn(|i| (i + 16) as u8);
    assert_eq!(encode_recovery_code(&VaultId(ids), &key),
        "000G40R40M30E209185GR38E1W8124GK2GAHC5RR34D1P70X3RFJ08924CJ2A9H750MJMASC5MQ2YWPJ");
}   // computed independently in Python (big-endian Crockford, SHA-256 checksum)
#[test] fn decoding_accepts_lowercase_dashes_spaces_and_confusable_letters()
#[test] fn a_single_wrong_character_fails_the_checksum()      // every position × one substitution
#[test] fn a_set_padding_bit_is_refused()
#[test] fn the_qr_payload_and_svg_are_well_formed()           // payload starts "moony-recover:1:", svg starts "<?xml" or "<svg", contains "</svg>"
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::recovery` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): recovery code with checksum and the Recovery Kit QR`.

---

### Task 6: Relay contract and HTTP client

**Files:**
- Create: `src-tauri/src/services/sync/relay/mod.rs`, `src-tauri/src/services/sync/relay/http.rs`; Modify: `services/sync/mod.rs`

**Interfaces:**
- Consumes: `services::http::{client, REQUEST_TIMEOUT}`.
- Produces:

```rust
pub struct VaultAuth { pub vault_id: String, pub token: String }
pub enum CheckoutPlan { Monthly, Yearly }              // "monthly" | "yearly" on the wire
pub struct AccountStatus { pub paid_until: Option<i64>, pub active: bool, pub vaults: u32 }
pub struct VaultInfo { pub head: u64, pub floor: u64, pub snapshot_seq: Option<u64>, pub paid_until: i64, pub bytes: u64, pub rotation: u32 }
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
pub type RelayResult<T> = std::result::Result<T, RelayError>;
impl RelayError {
    pub fn from_response(status: u16, body: &[u8]) -> Self;   // by (status, "error" code)
    pub fn is_transient(&self) -> bool;                        // Network, Protocol, RateLimited, Server
    pub fn to_app_error(&self) -> AppError;                    // Validation keys below
}
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
pub fn default_base_url() -> String;                 // MOONY_RELAY_URL in debug builds, else DEFAULT_RELAY_URL
pub fn validate_base_url(url: &str) -> Result<()>;   // https, or http on localhost/127.0.0.1 → validation.syncRelayUrlInvalid
pub struct HttpRelay { base: String }
impl HttpRelay { pub fn new(base: &str) -> Result<Self>; }
impl Relay for HttpRelay { … }
```

Status mapping (code from the `{"error": …}` body): 400 → `BadRequest(code)`; 401 → `Unauthorized`;
402 → `PaymentRequired`; 403 `token_read_only` → `TokenReadOnly`, `vault_limit_reached` →
`VaultLimitReached`; 404 `vault_not_found` / `account_not_found` / `no_snapshot`; 409 `vault_exists`
/ `account_has_vaults` / `snapshot_outdated`; 410 → `SnapshotRequired`; 413 `quota_exceeded` /
`snapshot_too_large`; 429 → `RateLimited`; 502/503 with `checkout_not_configured` or on the
checkout route → `CheckoutUnavailable`; other 5xx → `Server(status)`; an unknown code or a body
that is not the JSON shape → `Protocol`. App error keys: `validation.syncRelayUnreachable`
(Network), `validation.syncRelayError` (Protocol, Server, BadRequest, conflicts),
`validation.syncRateLimited`, `validation.syncPaymentRequired`, `validation.syncVaultLimitReached`,
`validation.syncAccountNotFound`, `validation.syncVaultNotFound`, `validation.syncCheckoutUnavailable`,
`validation.syncSnapshotTooLarge`, `validation.syncQuotaExceeded`, `validation.syncKeyRotated`
(TokenReadOnly, Unauthorized). Headers: `Moony-Account` for account calls and vault creation,
`Authorization: Bearer <token>` for vault calls; snapshot calls `application/octet-stream` with a
600 s per-request timeout; `GET snapshot` reads `Moony-Snapshot-Seq`.

- [ ] **Step 1: Failing tests** (`relay/mod.rs` tests; the HTTP shape tests run a one-shot `std::net::TcpListener` like `services/http.rs`'s tests, answering a canned response and returning the request head and body):

```rust
#[test] fn every_documented_status_and_code_maps_to_its_variant()      // table-driven, all codes of DESIGN.md
#[test] fn an_html_answer_is_a_protocol_error()                        // 200 "<html>…" on pull → Protocol; 502 HTML → Server(502)
#[test] fn transient_errors_are_network_protocol_rate_limit_and_5xx()
#[test] fn release_builds_refuse_plain_http_except_localhost()
#[tokio::test] async fn push_sends_bearer_json_and_base64_ops()
#[tokio::test] async fn pull_sends_after_and_limit_and_decodes_ops()
#[tokio::test] async fn create_vault_sends_the_account_header_and_reports_creation()
#[tokio::test] async fn get_snapshot_reads_the_sequence_header()
#[tokio::test] async fn checkout_posts_the_plan_and_returns_the_url()
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::relay` → FAIL.
- [ ] **Step 3: Implement** (base64 standard alphabet with padding for `ct`, as the relay expects).
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): relay client with typed errors`.

---

### Task 7: Fake relay for engine tests

**Files:**
- Create: `src-tauri/src/services/sync/relay/fake.rs` (`#[cfg(test)] pub mod fake;` in `relay/mod.rs`)

**Interfaces:**
- Produces:

```rust
#[derive(Clone, Default)] pub struct FakeRelay { /* Arc<Mutex<FakeState>> */ }
pub enum Route { CreateAccount, AccountStatus, Checkout, CreateVault, VaultInfo, Push, Pull, PutSnapshot, GetSnapshot, DeleteVault }
impl FakeRelay {
    pub fn new() -> Self;
    pub fn fail_next(&self, route: Route, error: RelayError);
    pub fn lose_next_push_answer(&self);          // stores the ops, then answers Network
    pub fn set_paid(&self, account: &str, paid: bool);
    pub fn prune_to(&self, floor: u64);           // floor = min(floor, snapshot seq); later pulls below it → SnapshotRequired
    pub fn drop_op(&self, seq: u64); pub fn replay_op(&self, seq: u64);  // dishonest relay knobs
    pub fn ops(&self) -> Vec<PulledOp>; pub fn calls(&self) -> Vec<Route>;
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

- [ ] **Step 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::relay::fake` PASS → gates.
- [ ] **Step 5: Commit** `test(sync): in-memory fake relay with the relay's semantics`.

---

### Task 8: Chain and journal for relay order — barrier marker, empty prefix, re-stamp, marker envelope

**Files:**
- Modify: `src-tauri/src/services/sync/chain.rs`, `services/sync/journal/mod.rs`, `services/sync/snapshot/mod.rs`, `services/sync/snapshot/stream.rs`, `services/sync/convergence/mod.rs` (positions; `rejoin` call), the phase 1 tests that build `EnvelopeMeta` or call `rejoin`

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
`sync.barrier_seq.is_some_and(|m| meta.position > m)`; otherwise `Apply`. `cross_barrier` sets
`barrier_seq = NULL` with the new barrier. `rejoin`: as phase 1, then — when the confirmed prefix is
empty and `trigger.seq > 1` — set the old ID's head to `(trigger.seq − 1, trigger.prev)`; reinstall
capture for the new ID; then `restamp_own_columns(old, new, W)`: every `sync_clock` row with
`device = old`, `hlc >= W`, `col NOT IN ('*', '-')` gets `(capture::tick(conn), new)`, one tick per
`(tbl, pk)` so a column group keeps one clock. Snapshot `dump` writes `barrier_seq`; `replace` sets it
from the header when `header.barrier == sync.barrier` (else NULL); `merge` keeps a local value and
otherwise takes the header's under the same condition. Add to `snapshot::dump`'s doc: "Call with
exclusive use of the connection (the engine holds the session mutex for the whole dump)."

- [ ] **Step 1: Failing tests** (chain.rs and journal tests unless named)

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
#[test] fn restamped_columns_of_one_row_share_one_clock()
#[test] fn rejoin_requeue_ships_parents_before_children()
    // old ID created bank_account A and transaction T (child) after W; rejoin; build_next in a loop:
    // the envelope holding T never comes before the one holding A, and both carry A's current generation in T's "p"
#[test] fn barrier_requeue_ships_parents_before_children()
#[test] fn a_marker_envelope_has_no_rows_and_a_current_clock()
#[test] fn applying_a_marker_moves_the_head_and_changes_no_row()   // merge tests
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::chain services::sync::journal services::sync::snapshot` → FAIL.
- [ ] **Step 3: Implement**; update every phase 1 caller (`EnvelopeMeta::of(.., position)` — the convergence harness passes the relay log index + 1; `rejoin(.., Some(&meta))` / `None`).
- [ ] **Step 4: Run** the filter and `cargo test --lib services::sync::convergence` → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): barrier marker by relay order, re-join head and re-stamp`.

---

### Task 9: Engine pull — verdicts, cursor rules, re-join

**Files:**
- Create: `src-tauri/src/services/sync/engine/mod.rs`, `engine/pull.rs`, `engine/tests/mod.rs`, `engine/tests/pull.rs`; Modify: `services/sync/mod.rs`

**Interfaces:**
- Consumes: `Relay`, `FakeRelay`, `envelope::{open_op, envelope_hash}`, `changeset::from_json`, `chain::*`, `merge::apply_changeset`, `keys::{credentials, new_device_keypair}`, `devices::*`, `state::*`.
- Produces:

```rust
// engine/mod.rs
pub trait ConnAccess: Clone + Send + Sync + 'static {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>;
}
impl ConnAccess for crate::db::Database { … }            // delegates to Database::with_conn
// engine/tests/mod.rs (#[cfg(test)]): `locked` makes with_conn fail with AppError::Auth, as a locked Database does
#[derive(Clone)] pub struct TestConn { pub conn: Arc<Mutex<Connection>>, pub locked: Arc<AtomicBool> }
pub enum PullStop { Gap, Invalid, Undecryptable, NewerSchema, MissingParent(String) }
pub struct PullReport { pub applied: usize, pub stop: Option<PullStop>, pub rejoined: bool, pub held: usize,
                        pub restored: bool }
// engine/pull.rs
pub enum OpStep { Advance, Hold, Stop(PullStop), Rejoined }
pub fn handle_op(conn: &Connection, creds: &Credentials, op: &PulledOp, now: i64, report: &mut PullReport) -> Result<OpStep>;
pub async fn pull_all<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> Result<PullReport>;
pub fn rejoin_as_new_device(conn: &Connection, trigger: Option<&EnvelopeMeta>, now: i64) -> Result<RejoinReport>;
```

`handle_op` (under the lock): `open_op` (failure → `Stop(Undecryptable)`); `from_json` (failure →
`Stop(Invalid)`); `meta = EnvelopeMeta::of(&cs, envelope_hash(ct), op.seq)`;
`note_barrier_marker`; `judge`:
- `Apply` → `apply_changeset`; `Applied` → `Advance`, `applied += 1`; `Stopped(MissingParent)` → `Stop(MissingParent)`; `Stopped(NewerSchema)` → `Stop(NewerSchema)`.
- `SkipReplay` → `Advance`. `OwnAcknowledged` → `acknowledge_own(seq)`, `Advance`.
- `Clone` → `rejoin_as_new_device(conn, Some(&meta), now)`, then judge the same envelope again and act on the new verdict (never a second `Clone`: the ID changed); return `Rejoined` after the re-judged step advanced, or that step's `Hold` / `Stop`.
- `Quarantine` → `chain::quarantine(author)`, `Hold`. `Held` → `Hold`.
- `BelowBarrier` → `skip_below_barrier`, `Advance`.
- `Gap` → `Stop(Gap)`. `NewerSchema` → `Stop(NewerSchema)`. `Invalid` → `Stop(Invalid)`.

`pull_all`: from `cursor`, pages of 500 until `more == false` or a stop; a page is fetched unlocked,
then each op is handled in its own `with_conn`. The persisted cursor advances over consecutive
`Advance`/`Rejoined` ops only; after the first `Hold` it stays at the position before that op while
later ops are still handled (other authors apply). Every handled op adds its ciphertext length to
`pulled_bytes` (`state::add_pulled_bytes`; Task 11 rebases the counter). A stop ends the pull
without moving the cursor past the stopping op. `SnapshotRequired` is returned to the caller (Task 11
handles it). `rejoin_as_new_device`: `chain::rejoin(conn, DeviceId::random(), trigger)`,
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
#[tokio::test] async fn gap_stops_without_moving_the_cursor()               // FakeRelay.drop_op
#[tokio::test] async fn an_invalid_chain_link_stops_without_moving_the_cursor()
#[tokio::test] async fn an_undecryptable_envelope_stops_and_is_never_skipped()
#[tokio::test] async fn newer_schema_stops_pulling()
#[tokio::test] async fn quarantine_keeps_the_cursor_before_the_held_envelope_while_others_apply()
#[tokio::test] async fn a_missing_parent_rolls_back_and_keeps_the_cursor()
#[tokio::test] async fn below_barrier_after_the_marker_moves_head_and_cursor()
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
- Consumes: `journal::{build_next, record_sealed, build_marker, Refused}`, `changeset::to_json`, `envelope::{seal_op, op_id, envelope_hash}`.
- Produces:

```rust
pub const SEAL_BATCH: usize = 64;
pub const PUSH_MAX_OPS: usize = 100;
pub const PUSH_MAX_B64: usize = 3 * 1024 * 1024;
pub struct SealReport { pub sealed: usize, pub refused: Vec<Refused> }
pub fn seal_batch(conn: &Connection, creds: &Credentials) -> Result<SealReport>;
    // only when sync_unsent is empty; one transaction: up to SEAL_BATCH × (build_next → to_json → seal_op →
    // INSERT sync_unsent(seq, envelope) → record_sealed(hash)); a marker (build_marker) when the outbox is
    // empty, sync.barrier > 0 and barrier_seq is NULL and no own envelope was sealed since the barrier was raised
pub fn next_push_batch(conn: &Connection) -> Result<Vec<(u64, OutgoingOp)>>;   // unsent in seq order within the limits
pub fn acknowledge_pushed(conn: &Connection, seqs: &[u64]) -> Result<()>;
pub enum PushStop { PaymentRequired, QuotaExceeded, KeyRotated, Unauthorized, VaultGone }
pub struct PushReport { pub pushed: usize, pub refused: usize, pub stop: Option<PushStop> }
pub async fn push_all<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> Result<PushReport>;
```

"No own envelope since the barrier was raised" = `barrier_seq IS NULL`; the marker is sealed at most
once per round. `push_all` loops: seal (locked) → batches (locked read) → `relay.push` (unlocked) →
`acknowledge_pushed` (locked), until the outbox and `sync_unsent` are empty or a stop. Map:
`PaymentRequired`, `QuotaExceeded`, `TokenReadOnly` → `KeyRotated`, `Unauthorized`,
`VaultNotFound` → `VaultGone`; transient errors return `Err` (the runtime backs off). A refused row is
logged at `warn` with table and byte size only (no key) and counted.

- [ ] **Step 1: Failing tests** (`engine/tests/push.rs`)

```rust
#[tokio::test] async fn sealing_stores_envelopes_and_clears_the_outbox_in_one_transaction()
    // after seal_batch: sync_unsent has seq 1..n, sync_own_log has the same (seq, hash), next_seq = n+1, outbox empty
#[tokio::test] async fn a_crash_before_the_push_resends_the_same_op_ids()   // seal, drop, seal again → no new envelope; push → ids equal the sealed ones
#[tokio::test] async fn a_push_whose_answer_was_lost_is_acknowledged_by_the_next_pull()
    // FakeRelay.lose_next_push_answer; push_all → Err; next round: pull judges OwnAcknowledged, unsent empty; relay holds each op once
#[tokio::test] async fn push_batches_respect_the_relay_limits()  // 5,000-row import → every push ≤ 100 ops and ≤ 3 MiB base64; relay order = seq order
#[tokio::test] async fn payment_required_keeps_envelopes_and_stops_pushing()
#[tokio::test] async fn quota_and_rotated_tokens_stop_pushing()
#[tokio::test] async fn a_vault_gone_answer_is_reported()
#[tokio::test] async fn an_oversized_row_is_refused_and_stays_queued()
#[tokio::test] async fn a_device_past_a_new_barrier_with_nothing_to_send_pushes_one_marker()
#[test] #[ignore = "a measurement: run in release with --ignored --nocapture"]
fn draining_a_50000_row_outbox()    // prints the seconds for seal_batch until empty (P19: must be < 30 s release)
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine::tests::push` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; run the ignored measurement once in release and note the number in the commit body; gates.
- [ ] **Step 5: Commit** `feat(sync): engine push through sync_unsent with batching and stops`.

---

### Task 11: Engine snapshots — restore after `410`, upload policy, sealed dump

**Files:**
- Create: `src-tauri/src/services/sync/engine/snapshots.rs`, `engine/tests/snapshots.rs`

**Interfaces:**
- Consumes: `snapshot::{dump, restore_merge, DumpStats, RestoreReport}`, `envelope::stream::*`, `state::{record_snapshot_uploaded, marks}`.
- Produces:

```rust
pub const SNAPSHOT_ENVELOPES: u64 = 2_000;
pub const SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
pub const SNAPSHOT_MIN_INTERVAL: i64 = 86_400;
pub const SNAPSHOT_MAX_DELAY: i64 = 3_600;
pub const MAX_SNAPSHOT_BYTES: usize = 104_857_600;
pub struct SealedSnapshot { pub seq: u64, pub bytes: Vec<u8>, pub stats: DumpStats }
pub fn dump_sealed(conn: &Connection, creds: &Credentials) -> Result<SealedSnapshot>;
    // S = cursor; BEGIN DEFERRED … COMMIT around snapshot::dump writing through raw deflate into a StreamSealer
pub fn restore_sealed(conn: &Connection, creds: &Credentials, blob: &SnapshotBlob, now: i64) -> Result<RestoreReport>;
pub enum Due { No, Now, At(i64) }
pub struct SnapshotPolicy { pub checked_at: Option<i64>, pub scheduled_at: Option<i64> }   // in memory, per engine
impl SnapshotPolicy { pub fn decide(&mut self, info: &VaultInfo, marks: &EngineMarks, now: i64, jitter: i64) -> Due; }
pub async fn restore_after_410<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> Result<RestoreReport>;
pub async fn maybe_upload<R: Relay, D: ConnAccess>(relay: &R, db: &D, policy: &mut SnapshotPolicy, now: i64, force: bool) -> Result<bool>;
```

`maybe_upload` first calls `state::rebase_pulled_bytes(info.snapshot_seq.unwrap_or(0))`.
`decide`: `info.snapshot_seq == None` → `Now`; else due when
`head − snapshot_seq ≥ 2,000` or `pulled_bytes ≥ 8 MiB`,
and `now − snapshot_uploaded_at ≥ 86,400`; a due result is scheduled at `now + jitter`
(`jitter` ∈ `0..=3,600`, drawn by the caller) and returned as `At` until then; checked at most every
3,600 s. `maybe_upload` runs only with `sync_outbox` and `sync_unsent` empty (else `Ok(false)`); it
holds the session mutex for the whole `dump_sealed` (phase 1 carry: exclusive use), uploads unlocked,
then `record_snapshot_uploaded(seq, stats.max_hlc, now)`; `SnapshotOutdated` drops the schedule and
returns `Ok(false)`; a sealed size over `MAX_SNAPSHOT_BYTES` or `SnapshotTooLarge` →
`Err(validation.syncSnapshotTooLarge)` and no retry until the next day. `restore_after_410`:
`get_snapshot` → `restore_sealed` (opens with `snapshot_object_id(blob.seq)`, inflates, calls
`snapshot::restore_merge`), cursor = `MAX(cursor, seq)`; when `restore_merge` fails, log the merge
stop reason at `warn` and return `validation.syncSnapshotInvalid` with the reason in `last_error`
(phase 1 note L142).

- [ ] **Step 1: Failing tests**

```rust
#[test] fn policy_uploads_at_once_when_the_vault_has_no_snapshot()
#[test] fn policy_waits_for_2000_envelopes_or_8_mib()
#[test] fn policy_uploads_at_most_once_a_day_after_the_random_delay()
#[tokio::test] async fn upload_waits_for_an_empty_outbox_and_unsent()
#[tokio::test] async fn a_sealed_dump_opens_to_the_same_record_stream()
    // dump_sealed on A, open_stream + inflate == the bytes of snapshot::dump(A, S) written to a Vec
#[tokio::test] async fn a_snapshot_served_under_another_sequence_fails()   // relay returns bytes of seq 5 labelled 6 → error, nothing written
#[tokio::test] async fn snapshot_required_restores_by_merge_and_keeps_the_outbox()
    // A and B from the Task 9 fixture; B offline with local edits; A edits, pushes, uploads; FakeRelay.prune_to(S);
    // B pulls → SnapshotRequired → restore_after_410: A's edits present, cursor == S, B's edits still queued
#[tokio::test] async fn an_outdated_snapshot_is_dropped_quietly()
#[tokio::test] async fn an_oversized_snapshot_is_not_uploaded()            // MAX_SNAPSHOT_BYTES lowered through a test-only parameter
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine::tests::snapshots` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): snapshot upload policy and restore after 410`.

---

### Task 12: Restore marker and device binding (logic)

**Files:**
- Create: `src-tauri/src/services/sync/binding.rs`; Modify: `src-tauri/src/services/backup.rs` (`restore_backup` writes the marker after a successful restore), `services/sync/mod.rs`

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
pub fn forget_binding(conn: &Connection, data_dir: &Path) -> Result<()>;
```

`startup_check` (sync enabled only): marker present → `Rejoin(RestoreMarker)`; else when
`binding_written = 1`: file missing, `NotThisDevice`, or an opened value ≠
`SHA-256(BINDING_LABEL ‖ device_id)` → `Rejoin(BindingMismatch)`; `Unsupported` → `Fine`;
`Failed` → `Fine` with a `warn`. `write_binding` seals the hash, writes the file `0o600` (as
`moony-keychain`'s `blob_file`), sets `binding_written = 1`; `Unsupported` leaves
`binding_written = 0`. The marker is written by `backup::restore_backup` for every restore (the next
unlock decides: no sync row → the marker is just cleared).

- [ ] **Step 1: Failing tests** (a `FakeNative` that XORs with a per-instance "machine" byte; `NotThisDevice` when the byte differs)

```rust
#[test] fn a_restore_marker_asks_for_a_rejoin()
#[test] fn a_binding_written_on_another_machine_asks_for_a_rejoin()
#[test] fn a_missing_binding_file_after_one_was_written_asks_for_a_rejoin()
#[test] fn a_never_written_binding_is_fine()
#[test] fn unsupported_platforms_never_rejoin()
#[test] fn a_binding_of_the_old_device_id_asks_for_a_rejoin()
#[test] fn restore_backup_leaves_a_rejoin_marker()                  // services/backup.rs tests
```

- [ ] **Step 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::binding services::backup` PASS → gates.
- [ ] **Step 5: Commit** `feat(sync): restore marker and device binding check`.

---

### Task 13: Engine runtime — loop, triggers, back-off, status, events, start and stop

**Files:**
- Create: `src-tauri/src/services/sync/engine/runtime.rs`, `engine/status.rs`, `engine/tests/runtime.rs`, `src-tauri/src/services/sync/feature.rs`, `src-tauri/src/models/sync.rs`, `src-tauri/src/commands/sync.rs` (non-command glue only)
- Modify: `src-tauri/src/models/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`.manage(commands::sync::SyncEngineState::default())`), `src-tauri/src/commands/auth.rs` (`after_unlock` and `confirm_recover` call `commands::sync::start_engine`; `logout` and `delete_account` call `stop_engine`), `src-tauri/src/commands/backup.rs` (`restore_backup` calls `stop_engine` before the restore)

**Interfaces:**
- Consumes: Tasks 9–12.
- Produces:

```rust
// feature.rs
pub const RELEASED: bool = false;
pub fn enabled() -> bool;
pub fn dev_tools() -> bool;                                    // cfg!(debug_assertions) && enabled()
// models/sync.rs (derive Serialize, Deserialize, specta::Type; per-field camelCase renames)
pub enum SyncPhase { Off, SettingUp, Idle, Syncing, Paused, Error }    // "off" | "settingUp" | "idle" | "syncing" | "paused" | "error"
pub enum SyncProblem { CaptureBroken, VaultGone, Lapsed, QuotaExceeded, SnapshotTooLarge, NewerSchema,
    Gap, Undecryptable, InvalidChain, MissingParent, Quarantined, Offline, RelayError, RateLimited,
    Unauthorized, KeyRotated }                                     // camelCase strings
pub struct SyncStatus { phase: SyncPhase, problem: Option<SyncProblem>, last_success_at: Option<i64>,
    pending_changes: u32, held_devices: u32, refused_rows: u32, paid_until: Option<i64>,
    deletes_at: Option<i64>, dev_tools: bool }                    // fields pub; JSON camelCase
// engine/status.rs
pub struct Runtime { pub running_round: bool, pub problem: Option<SyncProblem>, pub paid_until: Option<i64>, pub refused_rows: u32 }
pub fn status(conn: &Connection, runtime: &Runtime) -> Result<SyncStatus>;
// engine/mod.rs
pub trait EngineEvents: Send + Sync + 'static { fn data_changed(&self); fn status_changed(&self, status: &SyncStatus); fn binding(&self) -> &dyn BindingNative; fn data_dir(&self) -> PathBuf; }
pub struct RoundReport { pub pull: PullReport, pub push: PushReport, pub snapshot_uploaded: bool, pub problem: Option<SyncProblem> }
pub enum RoundMode { Normal, FirstSnapshot, Drain }   // FirstSnapshot: maybe_upload(force = true); Drain: pull + push only, allowed while capture is broken
pub async fn run_round<R: Relay, D: ConnAccess>(relay: &R, db: &D, policy: &mut SnapshotPolicy, mode: RoundMode, now: i64, jitter: i64) -> Result<RoundReport>;
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

`run_round` order: refuse with `CaptureBroken` when `capture_broken_at` is set (except in `Drain`)
and with `VaultGone` when `vault_gone_at` is set; `pull_all` (on `SnapshotRequired` → `restore_after_410`, then pull again);
`push_all` (P7); `vault_info` at most hourly through the policy, `maybe_upload`; `record_success`
when nothing stopped. Problem mapping: pull stops → `Gap`, `InvalidChain`, `Undecryptable`,
`NewerSchema`, `MissingParent`; `held > 0` → `Quarantined`; push stops → `Lapsed` (and
`paid_until` from `vault_info`, `deletes_at = paid_until + 90 × 86,400`), `QuotaExceeded`,
`KeyRotated`, `Unauthorized`, `VaultGone` (`set_vault_gone(now)`); transient errors → `Offline`
(Network), `RateLimited`, `RelayError`. The loop: at start, `startup_check` → on `Rejoin`,
`rejoin_as_new_device(conn, None, now)`, clear the marker, then `write_binding`; after every round
with `rejoined`, `write_binding`. Then rounds on `Unlock`, on `OutboxWatch::observe` every 5 s, every
300 s, and on `SyncNow` (which also clears a back-off wait and, with `vault_gone_at`, tries one
`vault_info` and clears the pause on success, P9). Errors back off 30 s → 1,800 s. A
`with_conn` error of kind `AppError::Auth` (locked) ends the loop silently. `data_changed` fires
after any round with `applied > 0` or a restore, and `commands::mcp_refresh::resume` runs for the
history queue (the Tauri adapter); `status_changed` fires whenever the status differs from the last
one sent. Rounds never overlap (one task). Status phase per P17.

- [ ] **Step 1: Failing tests** (`engine/tests/runtime.rs`; pure parts directly, rounds with `FakeRelay` + `TestConn`, a recording `EngineEvents`)

```rust
#[test] fn backoff_doubles_from_30_seconds_to_30_minutes_and_resets()
#[test] fn a_local_write_runs_a_round_after_one_quiet_poll_or_after_60_seconds()
#[tokio::test] async fn two_devices_converge_through_rounds()              // A edits, round A, round B → assert_converged
#[tokio::test] async fn a_rejoin_happens_before_any_push_in_the_round()
    // a VACUUM INTO copy of A pushes under A's ID; A's run_round: FakeRelay.calls() has no Push before A's re-join,
    // and every op A pushes in that round decrypts to a changeset whose device is A's new ID
#[tokio::test] async fn a_capture_broken_profile_never_syncs()
#[tokio::test] async fn lapsed_subscription_keeps_pulling_and_reports_the_deletion_date()
#[tokio::test] async fn a_vault_gone_answer_pauses_until_sync_now_finds_it_again()
#[tokio::test] async fn newer_schema_still_pushes_own_changes()
#[tokio::test] async fn data_changed_fires_only_when_something_was_applied()
#[tokio::test] async fn a_restore_marker_rejoins_before_the_first_round()
#[tokio::test] async fn a_locked_session_ends_the_loop_without_an_error()  // TestConn switched to "locked" → loop ends, no status_changed with phase error
#[test] fn status_phases_follow_the_problem()                               // table: each SyncProblem → phase per P17
#[test] fn the_flag_is_off_in_this_build_unless_the_dev_variable_is_set()  // RELEASED == false; enabled() == (debug && MOONY_DEV_SYNC=1)
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine services::sync::feature` → FAIL.
- [ ] **Step 3: Implement** the runtime, status, models, feature flag and glue. The Tauri `EngineEvents` adapter emits `sync-data-changed` / `sync-status-changed`, calls `mcp_refresh::resume`, and returns the platform binding native (Task 14 fills it; until then `NoBinding`).
- [ ] **Step 4: Run** → PASS; gates. Check by hand that a profile without sync starts no task (`start_engine` returns after one `SELECT`).
- [ ] **Step 5: Commit** `feat(sync): engine loop with triggers, back-off, status and events`.

---

### Task 14: Native device binding — macOS Secure Enclave, Windows DPAPI

**Files:**
- Modify: `src-tauri/plugins/moony-keychain/macos/Sources/MoonyBiometrics/MoonyBiometrics.swift` (two functions), `plugins/moony-keychain/src/desktop/macos.rs`, `src/desktop/windows.rs`, `src/desktop/mod.rs` (`bind_seal` / `bind_open` on `Keychain`), `plugins/moony-keychain/Cargo.toml` (`Win32_Security_Cryptography`), `src-tauri/src/commands/sync.rs` (adapter `KeychainBinding` implementing `BindingNative`)

**Interfaces:**
- Produces (plugin, desktop):

```rust
impl<R: Runtime> Keychain<R> {
    pub fn bind_seal(&self, secret: &[u8; 32]) -> Result<Vec<u8>>;               // KeychainError::Unsupported where none
    pub fn bind_open(&self, blob: &[u8]) -> Result<Zeroizing<[u8; 32]>>;          // KeychainError::Invalidated = not this machine
}
```

macOS: `moony_bind_seal(secret: SRData) -> SRData` / `moony_bind_open(blob: SRData) -> SRData` in
Swift, the same ECIES framing as `moony_bio_wrap` but with a Secure Enclave P-256 key whose access
control is `kSecAttrAccessibleWhenUnlockedThisDeviceOnly` + `.privateKeyUsage` (no user presence, no
prompt); the key's `dataRepresentation` travels inside the blob; a blob from another Mac fails to
restore the key → status `STATUS_INVALIDATED`. Windows: `CryptProtectData` /
`CryptUnprotectData` with `CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN` and the
description `"Moony sync binding"`. Linux and phones: `Unsupported` (phones arrive in phase 3).
The app adapter maps `Unsupported` → `BindingFailure::Unsupported`, `Invalidated` → `NotThisDevice`,
others → `Failed`.

- [ ] **Step 1: Failing tests** (plugin, platform-gated): `#[cfg(target_os = "windows")] fn dpapi_binding_round_trips()`; `#[cfg(target_os = "macos")] #[ignore = "needs a Mac with a Secure Enclave"] fn secure_enclave_binding_round_trips()`; `#[cfg(target_os = "linux")] fn linux_has_no_binding()`; app side `fn keychain_errors_map_to_binding_failures()`.
- [ ] **Step 2–4:** run (FAIL) → implement → `cargo test -p moony-keychain` and `cargo test --lib commands::sync` PASS; on the Mac run the ignored test once (`cargo test -p moony-keychain secure_enclave -- --ignored`) and note the result; gates.
- [ ] **Step 5: Commit** `feat(keychain): this-machine binding for device sync (Secure Enclave, DPAPI)`.

---

### Task 15: Join a vault by replace; repair a capture-broken profile

**Files:**
- Create: `src-tauri/src/services/sync/join.rs`, `engine/tests/join.rs`

**Interfaces:**
- Consumes: `snapshot::join`, `state::reset_chain`, `keys::*`, `devices::*`, `engine::snapshots::restore_sealed` pieces, `backup::create_backup`.
- Produces:

```rust
pub async fn join_by_replace<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: Credentials, device_name: &str, now: i64) -> Result<RestoreReport>;
pub async fn repair_capture<R: Relay, D: ConnAccess>(relay: &R, db: &D, data_dir: &Path, now: i64) -> Result<PathBuf>;
```

`join_by_replace`: `get_snapshot` (`NoSnapshot` → `validation.syncVaultHasNoSnapshot`), open and
inflate unlocked, then one savepoint under the lock: `state::reset_chain` when a row exists,
`snapshot::join(conn, DeviceId::random(), reader, now)` (cursor = `S`, phase 1 carry L173),
`keys::save_credentials`, `keys::new_device_keypair`, `devices::record_this_device`,
`state::clear_capture_broken`, `binding_written = 0`. `repair_capture`: `backup::create_backup`
into `data_dir/sync-repair-<unix>.zip`, `run_round(.., RoundMode::Drain, ..)`, then `join_by_replace` with the stored credentials and the current device name;
returns the backup path.

- [ ] **Step 1: Failing tests**

```rust
#[tokio::test] async fn a_joined_device_matches_the_vault_and_starts_after_the_snapshot()
    // A sets up and edits; B joins → assert_converged(A, B); B's cursor == S; B's first pull asks after=S
#[tokio::test] async fn the_joiner_records_itself_with_a_public_key_and_pushes_it()
#[tokio::test] async fn joining_a_vault_without_a_snapshot_is_refused_and_changes_nothing()
#[tokio::test] async fn repair_backs_up_drains_and_rejoins_under_a_new_device()
    // capture_broken_at set, outbox holds earlier captured rows → they reach the relay; new device ID; capture_broken_at NULL; backup file exists
```

- [ ] **Step 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::engine::tests::join` PASS → gates.
- [ ] **Step 5: Commit** `feat(sync): join a vault by replace and repair a capture-broken profile`.

---

### Task 16: First-device setup, turning off, deleting synced data

**Files:**
- Create: `src-tauri/src/services/sync/setup.rs`, `engine/tests/setup.rs`; Modify: `src-tauri/src/models/sync.rs`, `src-tauri/src/services/auth.rs` (extract `pub fn verify_password(db_path: &Path, password: &str) -> Result<()>` from `prepare_change_password`, which then calls it)

**Interfaces:**
- Produces:

```rust
// models/sync.rs
pub enum SyncPlan { Monthly, Yearly }                                             // "monthly" | "yearly"
pub struct SyncAccount { account_number: String, active: bool, paid_until: Option<i64> }  // JSON accountNumber, active, paidUntil
pub struct RecoveryKit { account_number: String, recovery_code: String, qr_svg: String } // code grouped by format_recovery_code
// setup.rs
pub async fn create_account<R: Relay, D: ConnAccess>(relay: &R, db: &D, relay_url: &str) -> Result<SyncAccount>; // returns the stored one if any
pub async fn account_status<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> Result<SyncAccount>;
pub async fn checkout_url<R: Relay, D: ConnAccess>(relay: &R, db: &D, plan: SyncPlan) -> Result<String>;
    // https only, host polar.sh or a subdomain of it → else validation.syncRelayError
pub fn recovery_kit(conn: &Connection) -> Result<RecoveryKit>;                    // ensure_vault_secrets
pub async fn finish_setup<R: Relay, D: ConnAccess>(relay: &R, db: &D, device_name: &str, kit_saved: bool, now: i64) -> Result<()>;
pub fn turn_off(conn: &Connection, data_dir: &Path) -> Result<()>;               // state::disable + binding::forget_binding
pub async fn delete_synced_data<R: Relay, D: ConnAccess>(relay: &R, db: &D, data_dir: &Path) -> Result<()>;
pub async fn delete_vault_best_effort<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> bool; // 5 s budget
```

`finish_setup` (resumable): `kit_saved` false → `validation.syncRecoveryKitNotConfirmed`;
`validate_device_name`; credentials missing → `validation.syncSetupIncomplete`; `create_vault`
(`PaymentRequired` → that key; `VaultLimitReached` → that key); when not enabled:
`state::enable(conn, current device)`, `keys::ensure_device_keypair`, `devices::record_this_device`;
then `run_round(.., RoundMode::FirstSnapshot, ..)`, which pushes the rule
packs and the device row, pulls them back as own, and uploads the first snapshot at `S` = cursor. A
failed upload does not fail setup (the policy uploads when `snapshot_seq` is null). Already enabled
with a snapshot → `validation.syncAlreadyEnabled`. `delete_synced_data`: `delete_vault`
(`VaultNotFound` counts as done), then `turn_off`.

- [ ] **Step 1: Failing tests**

```rust
#[tokio::test] async fn first_device_setup_creates_the_vault_pushes_and_uploads_a_snapshot()
    // FakeRelay: account → set_paid → kit → finish: vault exists, ops = [rule packs + device row], snapshot at seq 1, state enabled, capture installed
#[tokio::test] async fn setup_resumes_with_the_stored_account_and_vault_after_a_restart()
    // create_account, recovery_kit, "restart" (new FakeRelay handle, same state) → create_account returns the same number, recovery_kit the same code
#[tokio::test] async fn finishing_without_a_saved_kit_or_payment_is_refused()
#[tokio::test] async fn a_failed_first_upload_is_retried_by_the_policy()
#[tokio::test] async fn the_checkout_url_must_be_polar_https()
#[tokio::test] async fn turning_off_keeps_local_data_and_forgets_every_secret()
#[tokio::test] async fn deleting_synced_data_wipes_the_vault_and_turns_off()
#[tokio::test] async fn deleting_an_already_gone_vault_still_turns_off()
#[test] fn verify_password_accepts_the_password_and_refuses_another()      // services/auth.rs, existing key-file test pattern
```

- [ ] **Step 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::engine::tests::setup services::auth` PASS → gates.
- [ ] **Step 5: Commit** `feat(sync): first-device setup, turning sync off and deleting synced data`.

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

- [ ] **Step 1: Failing tests** (`test/dev-grant.test.ts`): `grants a paid period to an account`; `refuses days outside 1..365`; `answers 404 for an unknown account`; `devGrantEnabled is false without the variable` (unit test of the pure function with `{}` and `{DEV_GRANT: "0"}`).
- [ ] **Step 2: Run** `npm test` (in Moony-relay) → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `npm test && npm run typecheck` → PASS.
- [ ] **Step 5: Commit** (in Moony-relay) `feat: development-only entitlement grant for local testing` with the trailer. Do not merge or push; the owner decides.

---

### Task 18: End to end against `wrangler dev`

**Files:**
- Create: `src-tauri/src/services/sync/relay_e2e.rs` (`#[cfg(test)] mod relay_e2e;` in `services/sync/mod.rs`); Modify: `relay/http.rs` (`pub async fn dev_grant(&self, account: &str, days: u32) -> RelayResult<i64>`)

**Interfaces:**
- Consumes: Tasks 6, 13, 15, 16, 17.
- Produces: ignored tests reading `MOONY_RELAY_E2E` (e.g. `http://127.0.0.1:8787`); without it the test fails with the message `set MOONY_RELAY_E2E to a running wrangler dev of Moony-relay (npm run dev there, with DEV_GRANT=1 in .dev.vars)`.

- [ ] **Step 1: Write the tests**

```rust
#[tokio::test] #[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn two_devices_sync_through_the_relay() {
    // device A: create_account, dev_grant(30), recovery_kit, finish_setup("A"); add a bank account,
    // a category and 20 transactions; run_round. Device B: join_by_replace(A's credentials, "B"),
    // run_round. Both edit offline (same transaction's note on both, different columns elsewhere),
    // A round, B round, A round → assert_converged(&[A, B]); the newer note wins on both.
}
#[tokio::test] #[ignore = "…"] async fn an_unpaid_account_cannot_create_a_vault()   // a second account, never granted → finish_setup → validation.syncPaymentRequired, sync_state not enabled
#[tokio::test] #[ignore = "…"] async fn deleting_synced_data_makes_the_other_device_pause()
#[tokio::test] #[ignore = "…"] async fn snapshot_round_trip_through_r2()             // A uploads, B join_by_replace reads it
```

- [ ] **Step 2: Run** in Moony-relay `cp .dev.vars.example .dev.vars && npm run dev` (port 8787), then `cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e -- --ignored --test-threads=1` → PASS (iterate on failures; they are bugs in Tasks 6–16).
- [ ] **Step 3: Run** without the variable: `cargo test --lib relay_e2e -- --ignored` → the clear failure message; and `cargo test --lib` (ignored tests skipped) → PASS; gates.
- [ ] **Step 4: Commit** `test(sync): two devices end to end through wrangler dev`.

---

### Task 19: Commands and wire types (7-step procedure)

**Files:**
- Modify: `src-tauri/src/commands/sync.rs`, `src-tauri/src/lib.rs` (`generate_handler!` under `// Device sync`), `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerated), `shared/schema.ts`, `src/lib/tauri-api.ts`, `src-tauri/src/commands/auth.rs` (`delete_account` calls `delete_vault_best_effort` while the profile is still open and sync is on, before `stop_engine` and the wipe), `src-tauri/src/services/sync/capture/bench.rs` (moved `timed_import` / `import_statement` out of `convergence/scenarios.rs`, which now calls it)

**Interfaces:**
- Produces (every command `#[tauri::command] pub async fn`, first line `require_feature()?` except the flag itself; relay from `keys::account`'s URL or `relay::http::default_base_url()`):

| Command | Args | Returns |
|---|---|---|
| `sync_feature_enabled` | — | `bool` |
| `get_sync_status` | — | `SyncStatus` |
| `sync_create_account` | — | `SyncAccount` |
| `sync_get_account` | — | `SyncAccount` |
| `sync_start_checkout` | `plan: SyncPlan` | `String` (URL; the frontend opens it with the opener plugin) |
| `sync_prepare_recovery_kit` | — | `RecoveryKit` |
| `sync_finish_setup` | `deviceName: String, kitSaved: bool` | `SyncStatus` (starts the engine) |
| `sync_now` | — | `()` |
| `sync_show_recovery_kit` | `password: String` | `RecoveryKit` (after `auth::verify_password`) |
| `sync_turn_off` | — | `()` (stops the engine first) |
| `sync_delete_synced_data` | `password: String` | `()` (stops the engine first) |
| `sync_repair_device` | — | `String` (backup path) |
| `sync_dev_grant` | `days: u32` | `SyncAccount` (`validation.syncDevOnly` unless `feature::dev_tools()`) |
| `sync_debug_capture_overhead` | `rows: u32` | `String` (P20; `validation.syncDevOnly` in release) |

`shared/schema.ts` mirrors `SyncPhase`, `SyncProblem` (string unions), `SyncStatus`, `SyncAccount`,
`SyncPlan`, `RecoveryKit` exactly as serialized. `src/lib/tauri-api.ts` gets `deviceSyncApi` with one
method per command (`featureEnabled`, `getStatus`, `createAccount`, `getAccount`, `startCheckout`,
`prepareRecoveryKit`, `finishSetup`, `syncNow`, `showRecoveryKit`, `turnOff`, `deleteSyncedData`,
`repairDevice`, `devGrant`, `debugCaptureOverhead`).

- [ ] **Step 1: Failing test** `fn sync_commands_refuse_while_the_feature_is_off()` (a pure `require_feature()` unit test in `commands/sync.rs` with the flag function injected) and the drift check: register the six types in `collect_types()`, run `cd src-tauri && cargo test generate_bindings -- --ignored` → `git diff shared/generated-types.ts` shows exactly the six types.
- [ ] **Step 2: Implement** the 14 commands (thin: validate → service → map), register them, mirror the types, add the wrappers; `delete_account` wiring.
- [ ] **Step 3: Run** `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `npm run typecheck`, `npm run lint`, `npm test` → PASS; `git diff --exit-code shared/generated-types.ts` clean after staging.
- [ ] **Step 4: Commit** `feat(sync): sync commands and wire types`.

---

### Task 20: Frontend data layer — hooks, events, refresh ritual, hidden section, indicator

**Files:**
- Create: `src/hooks/use-device-sync.ts`, `src/hooks/use-device-sync-mutations.ts`, `src/hooks/use-device-sync-events.ts`, `src/lib/device-sync-refresh.ts` (+ `.test.ts`), `src/utils/device-sync-indicator.ts` (+ `.test.ts`), `src/components/shell/SyncStatusIndicator.tsx`, `src/components/settings/sync/SyncSection.tsx` (shell of the section: picks the card by phase), `src/i18n/locales/<10>/deviceSync.json`
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
(`/settings/sync`, icon `RefreshCw`). The indicator renders left of `MilestonesStatus` as the
design-system `.status` (dot + text) and links to `/settings/sync`; nothing renders while the flag
is off or the phase is `off`/`settingUp`. Indicator labels: `indicator.synced` (fresh),
`indicator.syncing` (neutral), `indicator.paused` (stale), `indicator.error` (stale),
`indicator.offline` (stale).

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
- [ ] **Step 3: Implement**; add every new string to all ten `deviceSync.json` and `settings.json` (English and Czech written, the other eight translated with `docs/standards/i18n-glossary.md`).
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): hidden Settings section, status indicator and remote-change refresh`.

---

### Task 21: Frontend setup flow and the Recovery Kit

**Files:**
- Create: `src/components/settings/sync/SyncIntroCard.tsx`, `SyncSetupDialog.tsx`, `RecoveryKitPanel.tsx`, `src/utils/recovery-kit.ts` (+ `.test.ts`)
- Modify: `src/hooks/use-device-sync-mutations.ts`, `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
- Produces:

```ts
export interface RecoveryKitCopy { title: string; intro: string; accountLabel: string; accountNote: string;
  codeLabel: string; codeNote: string; qrLabel: string; differsFromRecoveryKey: string; created: string }
export function recoveryKitHtml(kit: RecoveryKit, copy: RecoveryKitCopy): string; // full <!doctype html> page, inline CSS with @media print and @page, the QR as <img src="data:image/svg+xml;base64,…">, every copy string HTML-escaped, no external URL
export function recoveryKitText(kit: RecoveryKit, copy: RecoveryKitCopy): string;
```

Flow (one dialog, steps; open state lifted to `SyncSection`; design-system modal 560 px):
1. **Account** — `sync_create_account`; shows the number in the dark key box with Copy, and the §11.2
   sentence ("This number is your sync account. Anyone who has it can pay for it and see whether it
   is paid. It does not open your data."). Resuming with a stored account starts here with it shown.
2. **Pay** — a two-option segmented control "Monthly" / "Yearly" (no prices), "Continue to payment"
   opens `sync_start_checkout(plan)`'s URL with `@tauri-apps/plugin-opener` `openUrl`; the step polls
   `useDeviceSyncAccount({ pollWhileInactive: true })` and moves on when `active`. In debug builds
   with `status.devTools`, a ghost "Grant a test subscription" calls `sync_dev_grant(30)`.
3. **Recovery Kit** — `sync_prepare_recovery_kit`; QR image, grouped code, account number; "Save as
   page…" (`save` dialog, `.html`, `writeTextFile`) and "Save as text…" (`.txt`); "Copy code"; a
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
- [ ] **Step 3: Implement** the util, the dialog and the mutations (`useCreateSyncAccount`, `useStartCheckout`, `usePrepareRecoveryKit`, `useFinishSyncSetup`, `useDevGrant`; each invalidates `['device-sync-status']` and `['device-sync-account']`); strings in all ten locales.
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): first-device setup dialog and the Sync Recovery Kit`.

---

### Task 22: Frontend sync-on states, turning off, deleting, and every sync error key

**Files:**
- Create: `src/components/settings/sync/SyncStatusCard.tsx`, `SyncProblemAlert.tsx`, `SyncDangerCard.tsx`
- Modify: `src/hooks/use-device-sync-mutations.ts`, `src/components/settings/DangerZoneCard.tsx` (one extra sentence in the Delete Account confirmation when sync is on), `src/i18n/locales/<10>/deviceSync.json`, `src/i18n/locales/<10>/common.json` (`validation` block)

**Interfaces:**
- Consumes: `deviceSyncApi.{syncNow, showRecoveryKit, turnOff, deleteSyncedData, repairDevice}`.
- Produces: mutations `useSyncNow`, `useShowRecoveryKit`, `useTurnOffSync`, `useDeleteSyncedData`, `useRepairSyncDevice` (each invalidates `['device-sync-status']`; turn off and delete also `['device-sync-account']`).

UI: **Status card** — phase line, last synced time (`useFormat().day`/time), pending changes count,
"Sync now" (primary), "Show Recovery Kit" (asks the device password, then shows
`RecoveryKitPanel` read-only). **Problem alert** (`ui/alert.tsx`, next to the cause, with its
action): `lapsed` "Sync paused — renew to continue" + "Synced data will be deleted on {{date}}" +
"Renew" (opens checkout); `vaultGone` "Your synced data is gone from the server. Turn sync off on
this device?" with "Turn off sync" and "Try again" (`sync_now`); `captureBroken` with "Repair this
device" (confirm dialog explaining the automatic backup and the replace); `newerSchema` "Update
Moony on this device to keep syncing"; the rest one sentence each from `status.problem.*`.
**Danger card** — "Turn off sync on this device" (AlertDialog: "Local data stays. Other devices keep
syncing. Turning sync on again later replaces this device's data with the synced data or starts a
new vault.") and "Delete synced data" (password, then AlertDialog: deletes the vault for every
device; local data on every device stays).

Locale keys added to `common.json` `validation` in all ten locales (phase 1 carry L67 included):
`syncChangesetInvalid`, `syncSnapshotBelowBarrier`, `syncSnapshotInvalid`, `syncSnapshotNewerSchema`,
`syncSnapshotPending`, `syncUnavailable`, `syncRelayUnreachable`, `syncRelayError`, `syncRateLimited`,
`syncPaymentRequired`, `syncVaultLimitReached`, `syncAccountNotFound`, `syncVaultNotFound`,
`syncCheckoutUnavailable`, `syncSnapshotTooLarge`, `syncQuotaExceeded`, `syncKeyRotated`,
`syncRelayUrlInvalid`, `syncDeviceNameInvalid`, `syncRecoveryCodeInvalid`, `syncRecoveryKitNotConfirmed`,
`syncSetupIncomplete`, `syncAlreadyEnabled`, `syncVaultHasNoSnapshot`, `syncDevOnly`.

- [ ] **Step 1: Failing test** `src-tauri/src/error.rs`: `fn every_sync_validation_key_has_an_english_translation()` — collects every `validation.sync*` literal under `src-tauri/src` and asserts each exists in `src/i18n/locales/en/common.json` (`validation.<key>`); the parity gate (`src/i18n/locales.test.ts`) carries it to the other nine.
- [ ] **Step 2: Run** `cargo test --lib every_sync_validation_key` → FAIL.
- [ ] **Step 3: Implement** the cards, alerts, mutations and copy in all ten locales.
- [ ] **Step 4: Run** `cargo test --lib`, `npm test` (parity gate), `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): sync status, problems, turn off and delete in Settings`.

---

### Task 23: Docs, phone capture measurement, final verification

**Files:**
- Modify: `docs/architecture/database.md` (030 columns, if Task 3 left anything), `docs/architecture/overview.md` ("Device sync (hidden until release)": modules, the flag, `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 npm run tauri dev`, the e2e command), `docs/standards/rust-backend.md` (§12: "network code of sync goes through `services/sync/relay/http.rs`; the engine never holds the session mutex across a network call"), `docs/architecture/decisions/0025-device-sync-relay-client.md` (new ADR: envelope byte layouts of corrections 1–3, barrier marker, re-stamp, quarantine cursor, keychain binding, Recovery Kit format, runtime flag), `docs/architecture/decisions/README.md` (index)

- [ ] **Step 1: Write the docs** above (living docs; no line numbers or file sizes).
- [ ] **Step 2: Phone measurement (P20).** Build the iOS simulator debug app (`npm run tauri ios dev`), open Safari's Web Inspector on it, run `await window.__TAURI_INTERNALS__.invoke('sync_debug_capture_overhead', { rows: 5000 })` three times; record the three strings in the phase 2 ledger (`.superpowers/sdd/device-sync-phase-2/progress.md`). Ask the owner to repeat it on his iPhone (memory: device deploy recipe) and record that too. Above 1 s extra per 5,000 rows on the device → a follow-up entry, not a fix in this phase.
- [ ] **Step 3: Manual desktop check against `wrangler dev`** (P21): one desktop dev build with a demo profile (never the real one), `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787`: the section appears; create account → grant test subscription → save the Kit (open the saved page in a browser and print preview it on one page) → turn on → indicator "Synced"; stop the relay → "Offline", restart → back to "Synced"; delete synced data → section returns to the intro. Without `MOONY_DEV_SYNC` the section and the indicator are absent.
- [ ] **Step 4: Full gates** from the worktree: `npm run lint && npm run typecheck && npm run format:check && npm test`; `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` ; `cargo test generate_bindings -- --ignored && git diff --exit-code ../shared/generated-types.ts`; the e2e run of Task 18 once more.
- [ ] **Step 5: Commit** `docs(sync): phase 2 architecture, ADR 0025 and development notes`.
