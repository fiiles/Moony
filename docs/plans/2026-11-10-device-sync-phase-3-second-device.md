# Device Sync Phase 3 (Second Device) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A second device joins the vault (spec §17 phase 3): QR pairing through the relay's
pairing mailbox in both directions, joining with the Sync Recovery Kit, a device list with "rename
this device" on desktop and phone, and a Settings → Sync screen in the mobile shell, all still
behind the runtime flag, which a phone debug build can now turn on at build time.

**Architecture:** A pure `services/sync/pairing/` module pins the bytes (X25519 → HKDF transcript,
six-digit comparison code, sealed credentials) and a `PairingSession` state machine drives them over
a small `Mailbox` trait that `HttpRelay` and `FakeRelay` implement next to `Relay`. Every join (pairing
or Kit) ends in one service, `join::join_new_device`: verified backup, then phase 2's
`join_by_replace` under a new device ID. The command layer (`commands/sync_pairing.rs`) keeps one
session per unlocked profile session in memory, never sends a secret over IPC, and runs joins
under the existing one-change guard. The desktop shows the QR (rendered in Rust); the phone scans
it with `@tauri-apps/plugin-barcode-scanner` and has typed/pasted fallbacks.

**Tech Stack:** Rust 2021, rusqlite 0.40 (SQLCipher), tokio 1, reqwest 0.13 via `services/http.rs`,
`x25519-dalek` 3 (`static_secrets`, default `zeroize`), `hkdf` 0.13, `sha2` 0.11,
`chacha20poly1305` 0.11, `qrcode` 0.14; `tauri-plugin-barcode-scanner` 2.5.3 (mobile only);
React 19, TanStack Query 5, i18next (ten locales), Vitest (node); Swift/CryptoKit (iOS plugin);
Moony-relay under `wrangler dev` (branch `feat/dev-grant`, unchanged).

**Spec:** `docs/specs/2026-11-06-device-sync-client-design.md` §11.3 (D21, D22), §11.4, §11.5,
§17 phase 3, §20; ADR 0024 and ADR 0025 (phase 2 as built); the phase 2 plan
`docs/plans/2026-11-07-device-sync-phase-2-relay-client.md` and its ledger
`.superpowers/sdd/2026-11-07-device-sync-phase-2-relay-client/` (`progress.md`,
`preflight-rulings.md`); the relay contract `/Users/filipkral/Programování/FinanceApp/Moony-relay/DESIGN.md`
("Pairing mailbox") and `src/mailbox.ts`; this plan's pre-flight
`.superpowers/sdd/2026-11-10-device-sync-phase-3-second-device/preflight.md`.

## Spec and brief corrections (the code and this plan win)

1. **The pairing code names its sync server when it is not the default.** The QR is
   `moony-pair:1:<mailbox id>:<base64url S public key>` and, only for a sync server that is not
   `DEFAULT_RELAY_URL`, a fourth field `:<base64url(normalized sync server address)>` (the Recovery
   Kit's SH-2 convention). Without it a phone with sync off cannot find a mailbox on a self-hosted
   server, and a desktop joiner showing the code cannot know the phone's server. The address is not
   secret; the comparison code protects the exchange whatever server carries it.
2. **Which server holds the mailbox.** Holder showing: its stored `relay_url`. Joiner showing: the
   address typed in the dialog's Advanced field, else its stored account address, else
   `default_base_url()`. Scanner: the code's address, else `DEFAULT_RELAY_URL`. The credentials
   carry the vault's own address; the joiner uses that one from then on.
3. **Mailbox roles of the two slots.** Slot 1 is written by the scanner (C) and read by the shower
   (S); slot 2 is written by the holder, whichever side it is on, and read by the joiner. Write-once
   and read-once are per slot, so this works in both directions with no relay change.
4. **Nobody polls before the person confirmed.** S polls slot 1 every 2 s while it shows the code.
   C polls nothing before confirming. After confirming, a joiner polls slot 2 every 2 s; a holder
   writes slot 2 once. Two devices behind one address stay under 60 requests a minute (the relay
   allows 120 per address).
5. **Joining replaces local data and needs the device password** (as re-sync does, ruling T8-4):
   the joiner gives its device name and password before the code is shown or scanned.
6. **The Kit holds no epoch.** A Kit join derives the token (it does not depend on the epoch
   number), reads `GET /v1/vaults/:id`, and takes `rotation` as the key's epoch (correction 3 of
   phase 2: epochs follow `rotation`; it is 0 until phase 5).
7. **"Last synced" in the device list** is derived, never stored: this device's
   `sync_state.last_success_at`; another device's newest surviving clock
   (`MAX(hlc) FROM sync_clock WHERE device = ?`, physical part) — "last change synced". A device
   that only pulls shows none.
8. **No checkout on the phone** (§16a Q4: no purchase or subscribe link in the phone app). The
   phone's first-device setup shows the self-hosted instruction or "Paying for sync is not
   available in the phone app" and keeps waiting; debug builds keep "Grant a test subscription".

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-sync` on branch `feat/device-sync-phase-3`; run `git branch --show-current` before every commit. Never touch `../Moony-tauri`. No Moony-relay change (pre-flight: the mailbox API supports this design); Tasks 14 and 16 only run `../Moony-relay` on `feat/dev-grant`.
- Not in this phase: device removal, key epochs/rotation/rekey, token rotation, attachments, prices, plan limits.
- Flag: `feature::RELEASED` stays `false`. Every new command starts with `require_feature()?`. Release builds read neither `MOONY_DEV_SYNC` nor `MOONY_RELAY_URL`, at run time or build time.
- The ten rules of `AGENTS.md`; the 7-step type-contract procedure for every command and wire type; per-field `#[serde(rename = "camelCase")]`; thin commands; `AppError`/`SyncError`, never `Result<_, String>`; no `.unwrap()` outside tests; no migration in this phase.
- Rule 8 per task: every key a task emits or renders lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`) in that task, English and Czech written, the other eight translated with `docs/standards/i18n-glossary.md`; gate `npx vitest run src/i18n/` and `cargo test --lib every_sync_validation_key`. User copy says "sync server" (cs "synchronizační server"), never "relay".
- Wire types: a task that registers a type in `bindings.rs` runs `cd src-tauri && cargo test generate_bindings -- --ignored`, mirrors it in `shared/schema.ts` and commits the regenerated `shared/generated-types.ts` with it.
- Secrets: `PairingSession`, `Transcript`, `Credentials`, `PairingPayload` print `<redacted>` in `Debug`; keys and the payload JSON live in `Zeroizing`; ephemeral secrets are `x25519_dalek::StaticSecret` (ZeroizeOnDrop). Nothing secret crosses IPC: the QR and the six digits are not secret, credentials never leave Rust. Mailbox IDs, account numbers, codes and addresses are never logged above `debug`.
- Pairing bytes (pinned; Task 1 has the vectors): mailbox ID = 16 OS-random bytes, base64url without padding (22 chars, canonical); X25519 ephemeral keys on both sides; shared = X25519, refused when not contributory; `OKM = HKDF-SHA256(salt = mailbox ID 16 raw bytes, ikm = shared, info = "moony pair v1" ‖ S pub ‖ C pub, L = 40)`; session key = `OKM[0..32]`; code = `u64::from_be_bytes(OKM[32..40]) % 1_000_000`, six digits zero-padded; slot 1 = `0x01 | role | C pub` (34 bytes; role `0x01` holder, `0x02` joiner); slot 2 = `0x01 | nonce (24) | XChaCha20-Poly1305(session key, AD = mailbox ID (16) ‖ S pub (32) ‖ C pub (32), payload JSON)`.
- Pairing timing: a session lives 600 s from `show`/`scan`; poll interval 2,000 ms; a best-effort mailbox delete gets 3 s.
- Tests: Rust services use `test_support::migrated()`, `engine::tests::{TestConn, vault}`, `FakeRelay`; never `Database`. Frontend tests are pure functions in node. Components and hooks have no tests (testing policy).
- Every task ends with its filter test and, from `src-tauri/`: `cargo test --lib`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; tasks touching the frontend or a locale also `npm test`, `npm run typecheck`, `npm run lint`. Never `--no-verify`.
- Files stay under ~800 lines; `commands/sync.rs` (over it) only changes visibility and the flag test; new commands go in `commands/sync_pairing.rs`.
- Commits: Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **Credentials move before both people confirmed** (someone writes slot 1 or slot 2 first): the holder writes nothing before its own confirm, the joiner opens nothing before its own, and a forged slot 2 never replaces data — Task 3 `the_holder_writes_slot_2_only_after_its_own_confirm`, `a_forged_slot_2_fails_and_nothing_is_joined`.
2. **A self-hosted sync server** (the owner's `https://sync.home.example.com`): the phone, with nothing configured, reaches the mailbox the code names — Task 1 `a_code_names_a_sync_server_that_is_not_the_default`, Task 3 `the_scanner_uses_the_sync_server_the_code_names`, Task 14 (the code carries `http://127.0.0.1:8787`).
3. **A join interrupted after the backup** (Wi-Fi drop during the snapshot download): profile unchanged, sync still off, backup kept, the received credentials still there for "Try again" — Task 4 `a_join_whose_snapshot_download_fails_changes_nothing_but_the_backup`, Task 7 `a_transient_join_failure_keeps_the_received_session`.
4. **The app locks or another profile unlocks mid-pairing:** the session never carries over and is wiped — Task 7 `a_pairing_session_belongs_to_one_profile_session`.
5. **Relay hiccups inside the 10 minutes** (429 behind one home address, a dropped answer to a read-once GET): transient errors keep the session; a consumed slot ends it with a clear message — Task 3 `transient_errors_keep_the_session`, `a_lost_read_ends_the_pairing_with_a_clear_error`.

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/pairing/mod.rs` (+ `tests.rs`) | Pairing bytes: mailbox ID, code text, slot 1, transcript, sealed credentials, roles |
| `services/sync/pairing/session.rs` (+ `session_tests.rs`) | `PairingSession` over a `Mailbox`: stages, confirm gating, expiry, abandon |
| `services/sync/relay/mod.rs`, `relay/http.rs`, `relay/fake.rs` (+ their tests) | `Mailbox` trait and its four `RelayError`s; HTTP and fake implementations |
| `services/sync/error.rs` | Mailbox errors → pairing validation keys |
| `services/sync/join.rs`, `recovery.rs` | `join_new_device`, `recovery_credentials`; `read_recovery_input` |
| `services/sync/engine/tests/join_new.rs` | Joins end to end in memory |
| `services/sync/devices.rs` | `list_devices`, `rename_this_device` |
| `services/sync/feature.rs`, `relay/http.rs` | Build-time dev flag and dev sync server |
| `src-tauri/src/models/sync.rs`, `bindings.rs` | `SyncDevice`, `PairingView`, `PairingStage`, `PairingRole` |
| `src-tauri/src/commands/sync_pairing.rs` | Pairing, join, Kit join, device list and rename commands; the session store |
| `src-tauri/src/services/sync/relay_e2e_pairing.rs` | Pairing and Kit join against `wrangler dev` |
| `src/utils/pairing-code.ts`, `src/utils/sync-devices.ts` (+ tests) | Pure UI helpers |
| `src/hooks/use-device-pairing.ts`, `use-device-sync.ts`, `use-device-sync-mutations.ts` | Queries and mutations |
| `src/components/settings/sync/{SyncDevicesCard,AddDeviceDialog,JoinSyncDialog,RecoveryJoinDialog,PairingSteps}.tsx` | Desktop UI |
| `src/mobile/pages/settings/SyncSection.tsx`, `src/mobile/components/sync/*`, `src/mobile/lib/{sync-summary,qr-scanner}.ts` | Phone UI |
| `src-tauri/plugins/moony-keychain/{src/mobile.rs,src/error.rs,ios/Sources/*}` | iOS binding and clipboard |
| `src-tauri/Info.ios.plist`, `gen/apple/Localizations/*/InfoPlist.strings`, `capabilities/mobile.json` | Camera and local-network texts, scanner permissions |

---

### Task 1: Pairing protocol bytes

**Files:**
- Create: `src-tauri/src/services/sync/pairing/mod.rs`, `src-tauri/src/services/sync/pairing/tests.rs`
- Modify: `src-tauri/src/services/sync/mod.rs` (`pub mod pairing;`), `src-tauri/src/services/sync/keys.rs` (`os_random` becomes `pub`; `VaultKeys::epochs`; `SecretString` becomes `pub(super)`), `src/i18n/locales/<10>/common.json` (`validation.syncPairingCodeInvalid`, `syncPairingFailed`, `syncPairingBothOn`, `syncPairingBothOff`)

**Interfaces:**
- Consumes: `keys::{Credentials, VaultId, VaultKeys}`, `relay::http::{normalize_base_url, is_default_relay, DEFAULT_RELAY_URL}`.
- Produces (`services::sync::pairing`):

```rust
pub const QR_PREFIX: &str = "moony-pair:1:";
pub const MAX_CODE_BYTES: usize = 512;
pub const KDF_INFO: &[u8] = b"moony pair v1";
pub const SLOT_VERSION: u8 = 0x01;
pub const SLOT1_LEN: usize = 34;
pub const MAX_PAYLOAD_JSON: usize = 16 * 1024;
pub const MAX_PAYLOAD_KEYS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role { Holder, Joiner }                       // bytes 0x01 / 0x02
impl Role { pub fn of(sync_on: bool) -> Role; pub fn byte(self) -> u8; pub fn from_byte(b: u8) -> Option<Role> }

#[derive(Clone, Copy, PartialEq, Eq)]                  // Debug: "MailboxId(<redacted>)"
pub struct MailboxId(pub [u8; 16]);
impl MailboxId { pub fn random() -> Result<Self>; pub fn to_b64(&self) -> String; pub fn from_b64(s: &str) -> Result<Self> }

pub struct PairingCode { pub mailbox: MailboxId, pub shower_public: [u8; 32], pub relay_url: Option<String> }
pub fn pairing_code_text(code: &PairingCode) -> String;
pub fn parse_pairing_code(text: &str) -> Result<PairingCode>;
pub fn slot1(role: Role, scanner_public: &[u8; 32]) -> [u8; SLOT1_LEN];
pub fn parse_slot1(bytes: &[u8]) -> Result<(Role, [u8; 32])>;
pub fn check_roles(shower: Role, scanner: Role) -> Result<()>;
pub struct Transcript { /* key: Zeroizing<[u8; 32]>, code: u32, ad: [u8; 80] */ }   // Debug redacted
impl Transcript { pub fn code(&self) -> u32; pub fn code_text(&self) -> String }
pub fn derive(own: &StaticSecret, peer_public: &[u8; 32], mailbox: &MailboxId,
              shower_public: &[u8; 32], scanner_public: &[u8; 32]) -> Result<Transcript>;
pub fn seal_credentials(t: &Transcript, creds: &Credentials) -> Result<Vec<u8>>;
pub fn open_credentials(t: &Transcript, sealed: &[u8]) -> Result<Credentials>;
// keys.rs
pub fn os_random(buf: &mut [u8]) -> Result<()>;
impl VaultKeys { pub fn epochs(&self) -> impl Iterator<Item = (u32, Zeroizing<[u8; 32]>)> + '_ }  // ascending
```

Rules: `MailboxId::from_b64` accepts only the canonical 22-character spelling (decode to 16 bytes,
re-encode equal), else `validation.syncPairingCodeInvalid`. `pairing_code_text` writes the fourth
field only when `relay_url` is `Some` and not the default. `parse_pairing_code`: ≤ 512 bytes before
anything is decoded; exact prefix; 2 or 3 `:`-fields; canonical mailbox; S pub = canonical 43-char
base64url of 32 bytes; the address field decodes to UTF-8 and goes through `normalize_base_url`
(`validation.syncRelayUrlInvalid`), the default reads as `None`; anything else
`validation.syncPairingCodeInvalid`; the error never holds the input. `parse_slot1`: exactly 34 bytes,
version 1, a known role, else `validation.syncPairingFailed`. `check_roles`: equal roles →
`validation.syncPairingBothOn` (both holders) / `syncPairingBothOff` (both joiners). `derive`: one
`Hkdf::<Sha256>::new(Some(&mailbox.0), shared)` expand of 40 bytes into a `Zeroizing` buffer;
`!shared.was_contributory()` → `validation.syncPairingFailed`. Payload JSON, built in one
`Zeroizing<String>` in this field order, keys in ascending epoch:
`{"v":1,"relayUrl":"…","accountNumber":"…","vaultId":"…","keys":[{"epoch":0,"key":"…"}]}`.
`open_credentials`: version byte, length ≥ 1 + 24 + 16, AEAD open (any failure
`validation.syncPairingFailed`), JSON ≤ `MAX_PAYLOAD_JSON`, `deny_unknown_fields`, `v == 1`, 16-digit
account, canonical vault ID, 1..=64 keys with unique epochs and 32-byte keys, `relayUrl` through
`normalize_base_url` (`validation.syncRelayUrlInvalid`); current epoch = the largest; key strings
read into `Zeroizing` (`keys::SecretString`). A private `seal_credentials_with_nonce(.., [u8; 24])`
serves the tests.

- [ ] **Step 1: Failing tests** (`pairing/tests.rs`; vectors computed with a pure-Python RFC 7748 X25519, checked against RFC 7748 §6.1, and RFC 5869 HKDF, independent of the crates)

```rust
const S_SECRET: [u8; 32] = [0x11; 32];
const C_SECRET: [u8; 32] = [0x22; 32];
const M_SECRET: [u8; 32] = [0x44; 32];
const MAILBOX: MailboxId = MailboxId([0x33; 16]);

#[test] fn the_transcript_matches_the_frozen_vector() {
    // S pub 7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13
    // C pub 0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f20
    // shared 9e004098efc091d4ec2663b4e9f5cfd4d7064571690b4bea97ab146ab9f35056
    // derive on both sides → same key == 01b8d60583d5b13dc2ac3426e3bb2550bc24e241f7fea9f2787597d0c03d527b
    //   (a #[cfg(test)] fn key_bytes(&self)), code() == 482686, code_text() == "482686"
}
#[test] fn a_substituted_scanner_key_shows_another_code() {
    // S derives with M's public key (ff2ee45601ec1b67310c7790404585ae697331eee1c1f8cf2419731c1fff3e6b):
    // code 763075, key 0efa12d5cc75e952aa8a22e2676ec387480c9f8124a440153a79019911756f36 ≠ C's 482686 / key
}
#[test] fn the_code_text_is_frozen() {
    // MAILBOX + S pub, default server → "moony-pair:1:MzMzMzMzMzMzMzMzMzMzMw:e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM"
    // and parse_pairing_code of it round-trips with relay_url None
}
#[test] fn a_code_names_a_sync_server_that_is_not_the_default() {
    // relay "https://Sync.Home.Example.com/" → text ends ":" + b64url("https://sync.home.example.com");
    // parse → Some("https://sync.home.example.com"); "http://127.0.0.1:8787" round-trips;
    // a fourth field naming DEFAULT_RELAY_URL parses to None
}
#[test] fn malformed_codes_are_refused_without_echoing_them() {
    // wrong prefix, version 2, 21/23-char or non-canonical mailbox, 42-char key, 5 fields, 513 bytes,
    // fourth field "http://evil.example" (→ syncRelayUrlInvalid), empty fourth field
    // → Err whose Display/Debug does not contain the input
}
#[test] fn slot_1_layout_and_roles() {
    // slot1(Joiner, &pub) == [0x01, 0x02, pub…]; parse_slot1 round-trips; 33/35 bytes, version 2,
    // role 0x03 → syncPairingFailed; check_roles(Holder, Holder) → syncPairingBothOn,
    // (Joiner, Joiner) → syncPairingBothOff, mixed → Ok
}
#[test] fn a_low_order_public_key_is_refused()          // peer [0; 32] → syncPairingFailed
#[test] fn credentials_round_trip_through_slot_2() {
    // creds with epochs {0, 1}, relay "https://sync.home.example.com" → seal_with_nonce([0x55; 24]):
    // bytes[0] == 0x01, bytes[1..25] == [0x55; 24]; open → same relay, account, vault ID, current epoch 1, both keys
}
#[test] fn slot_2_is_bound_to_this_exchange() {
    // opening under a transcript of another mailbox, or with S/C swapped, or after flipping any byte → syncPairingFailed
}
#[test] fn a_hostile_payload_is_refused() {
    // sealed with the right transcript but: relayUrl "http://evil.example", "https://a.b/path",
    // account "123", unknown field, v 2, 65 keys, duplicate epoch, 31-byte key, 17 KiB JSON
    // → Err (syncRelayUrlInvalid for the addresses, syncPairingFailed otherwise)
}
#[test] fn secrets_print_redacted()                    // Debug of MailboxId, Transcript has no hex of the key or the ID
#[test] fn secrets_are_wiped_on_drop() {
    fn wiped<T: zeroize::ZeroizeOnDrop>() {}
    wiped::<x25519_dalek::StaticSecret>(); wiped::<zeroize::Zeroizing<[u8; 32]>>();
}
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::pairing::tests` → FAIL (module missing).
- [ ] **Step 3: Implement** per the Interfaces block; add the four keys to all ten `common.json`.
- [ ] **Step 4: Run** the filter → PASS; `npx vitest run src/i18n/`; the gates.
- [ ] **Step 5: Commit** `feat(sync): pairing protocol bytes, transcript and sealed credentials`.

---

### Task 2: The relay's pairing mailbox — trait, errors, HTTP, fake

**Files:**
- Modify: `src-tauri/src/services/sync/relay/mod.rs` (trait, variants, mapping table test), `relay/http.rs`, `relay/http/tests.rs`, `relay/fake.rs`, `relay/fake/tests.rs`, `services/sync/error.rs`, `src/i18n/locales/<10>/common.json` (`validation.syncPairingCodeUsed`, `syncPairingExpired`)

**Interfaces:**
- Produces:

```rust
// relay/mod.rs
pub const MAX_SLOT_BYTES: usize = 64 * 1024;
pub trait Mailbox: Send + Sync {
    /// PUT /v1/pairing/:id/:slot (raw body, 1 B–64 KiB) → 204.
    fn put_slot(&self, mailbox: &str, slot: u8, bytes: &[u8]) -> impl Future<Output = RelayResult<()>> + Send;
    /// GET /v1/pairing/:id/:slot → Some(bytes) (erased on the relay) | None (404 empty).
    fn take_slot(&self, mailbox: &str, slot: u8) -> impl Future<Output = RelayResult<Option<Vec<u8>>>> + Send;
    /// DELETE /v1/pairing/:id → 204, idempotent.
    fn burn_mailbox(&self, mailbox: &str) -> impl Future<Output = RelayResult<()>> + Send;
}
// RelayError gains: SlotTaken (409 slot_taken), SlotEmpty (404 empty), SlotConsumed (410 consumed),
// MailboxGone (410 mailbox_gone)
// fake.rs: Route gains MailboxPut, MailboxTake, MailboxBurn
impl FakeRelay {
    pub fn peek_slot(&self, mailbox: &str, slot: u8) -> Option<Vec<u8>>;   // without consuming
    pub fn write_slot_raw(&self, mailbox: &str, slot: u8, bytes: Vec<u8>); // an attacker's write, same rules
    pub fn expire_mailbox(&self, mailbox: &str);                            // as if 10 minutes passed
    pub fn lose_next_take_answer(&self);                                    // the relay erased it, the answer is lost
}
```

`HttpRelay` checks the arguments first (`BadRequest("bad_mailbox")` unless 22 base64url characters,
`BadRequest("bad_slot")` unless 1 or 2, `BadRequest("empty_body")` / `BadRequest("body_too_large")`
for 0 or > 64 KiB), sends no account header and no token, keeps the default 15 s bound, caps a GET
body at `MAX_SLOT_BYTES` (`Protocol("body too large")`), and maps `SlotEmpty` to `Ok(None)` in
`take_slot`. `FakeRelay` follows `src/mailbox.ts`: unknown or unwritten ID → `None` and stores
nothing; the first write opens the mailbox; a second write to a slot, read or not, → `SlotTaken`; a
second read → `SlotConsumed`; burn or expiry → every later put/take `MailboxGone`; burning an unknown
ID stores nothing. `error.rs`: the four variants map to `SyncError::Refused`; `to_app_error` gives
`Refused(SlotTaken)` → `validation.syncPairingCodeUsed`, `Refused(MailboxGone)` →
`validation.syncPairingExpired`, `Refused(SlotConsumed)` → `validation.syncPairingFailed`.

- [ ] **Step 1: Failing tests**

```rust
// relay/mod.rs: every_documented_status_and_code_maps_to_its_variant — the rows
// (409 slot_taken, 404 empty, 410 consumed, 410 mailbox_gone) now expect SlotTaken, SlotEmpty,
// SlotConsumed, MailboxGone (they were unknown_code())
// relay/http/tests.rs (serve_once pattern)
#[test] fn a_slot_is_put_as_raw_bytes_without_credentials()   // request line "PUT /v1/pairing/<id>/1", content-type application/octet-stream, no moony-account / authorization header
#[test] fn an_empty_slot_reads_as_none_and_a_written_one_as_its_bytes()
#[test] fn mailbox_answers_map_to_their_errors()              // 409 slot_taken, 410 consumed, 410 mailbox_gone, 429
#[test] fn bad_mailbox_ids_and_slots_are_refused_before_sending() // no connection accepted
#[test] fn an_oversized_slot_answer_is_a_protocol_error()     // 64 KiB + 1
// relay/fake/tests.rs
#[tokio::test] async fn the_fake_mailbox_is_write_once_and_read_once()
#[tokio::test] async fn a_burnt_or_expired_mailbox_is_gone_for_every_call()
#[tokio::test] async fn unknown_mailboxes_store_nothing()       // take → None; burn → Ok; then a put still opens it
#[tokio::test] async fn a_lost_take_answer_leaves_the_slot_consumed() // lose_next_take_answer → Err(Network); next take → SlotConsumed
// error.rs
#[test] fn mailbox_errors_read_as_pairing_keys()
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::relay services::sync::error` → FAIL.
- [ ] **Step 3: Implement**; the two keys in all ten locales.
- [ ] **Step 4: Run** → PASS; `npx vitest run src/i18n/`; gates.
- [ ] **Step 5: Commit** `feat(sync): the relay's pairing mailbox in the client and the fake relay`.

---

### Task 3: Pairing session

**Files:**
- Create: `src-tauri/src/services/sync/pairing/session.rs`, `pairing/session_tests.rs`
- Modify: `pairing/mod.rs` (`pub mod session;`), `src/i18n/locales/<10>/common.json` (`validation.syncPairingCancelled`)

**Interfaces:**
- Consumes: Task 1, Task 2's `Mailbox`.
- Produces (`pairing::session`):

```rust
pub const PAIRING_LIFETIME_SECS: i64 = 600;
pub const BURN_BUDGET: Duration = Duration::from_secs(3);
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Side { Shower, Scanner }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Stage { WaitingForScan, Compare, WaitingForOther, Sent, Received }
pub struct PairingSession { /* side, role, mailbox, relay_url, secret: StaticSecret, own and peer publics,
                               transcript: Option<Transcript>, stage, received: Option<Credentials>, expires_at */ }
impl PairingSession {
    pub fn show(role: Role, relay_url: &str, now: i64) -> Result<Self>;                 // WaitingForScan; nothing sent
    pub fn scan(role: Role, code_text: &str, now: i64) -> Result<Self>;                 // relay = code's or DEFAULT_RELAY_URL
    pub fn code(&self) -> PairingCode;                                                   // shower only
    pub fn side(&self) -> Side; pub fn role(&self) -> Role; pub fn stage(&self) -> Stage;
    pub fn relay_url(&self) -> &str; pub fn expires_at(&self) -> i64;
    pub fn comparison_code(&self) -> Option<String>;                                     // Some from Compare on
    pub fn received(&self) -> Option<&Credentials>;
    pub async fn announce<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<()>;            // scanner: slot 1 → Compare
    pub async fn poll<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<Stage>;
    pub async fn confirm<M: Mailbox>(&mut self, mailbox: &M, holder_creds: Option<&Credentials>, now: i64) -> SyncResult<Stage>;
    pub async fn abandon<M: Mailbox>(self, mailbox: &M);                                 // burn within BURN_BUDGET, then drop
}
pub fn keeps_session(error: &SyncError) -> bool;   // RateLimited | Transient(_)
```

Transitions. `announce` (scanner, Compare not yet): put slot 1 (`slot1(role, own pub)`), derive →
Compare. `poll`: shower in WaitingForScan takes slot 1 (`None` → stays), `parse_slot1`,
`check_roles(own, theirs)`, derive → Compare; joiner in WaitingForOther takes slot 2 (`None` → stays),
`open_credentials` → Received, then `burn_mailbox` best effort; every other stage returns its stage
untouched and sends nothing. `confirm` only from Compare: holder → `seal_credentials(holder_creds)`
and put slot 2 → Sent (`holder_creds` missing → `Internal`); joiner → WaitingForOther, sending
nothing. Any call at `now ≥ expires_at` → `validation.syncPairingExpired`. Relay answers:
`MailboxGone` → `syncPairingExpired` for a shower before Compare and `syncPairingCancelled` after;
`SlotTaken` on slot 1 → `syncPairingCodeUsed`; `SlotTaken` on slot 2 or `SlotConsumed` →
`syncPairingFailed`. The session never retries on its own; the caller decides with `keeps_session`.

- [ ] **Step 1: Failing tests** (`FakeRelay`; a helper drives S and C to Compare)

```rust
#[tokio::test] async fn a_joiner_scans_a_holders_code_and_receives_the_credentials() {
    // S = show(Holder), C = scan(Joiner, code); C.announce; S.poll → Compare; codes equal (6 digits);
    // C.confirm → WaitingForOther; C.poll → WaitingForOther (slot 2 empty); S.confirm(Some(creds)) → Sent;
    // C.poll → Received; C.received() == creds (relay, account, vault ID, keys); mailbox burnt (put → MailboxGone)
}
#[tokio::test] async fn a_holder_scans_a_joiners_code_and_sends_the_credentials()   // S = Joiner, C = Holder; mirror
#[tokio::test] async fn the_holder_writes_slot_2_only_after_its_own_confirm() {
    // both at Compare, joiner confirmed and polled: peek_slot(id, 2) == None; only after holder.confirm is it Some
}
#[tokio::test] async fn a_forged_slot_2_fails_and_nothing_is_joined() {
    // write_slot_raw(id, 2, garbage) before anyone confirms: joiner.poll at Compare sends nothing (calls unchanged);
    // after joiner.confirm, poll → Err(syncPairingFailed), received() None; the holder's later put → SlotTaken → syncPairingFailed
}
#[tokio::test] async fn a_man_in_the_middle_who_writes_slot_1_first_is_caught() {
    // M writes slot1(Joiner, m_pub) first: C.announce → Err(syncPairingCodeUsed);
    // S.poll → Compare with a code ≠ C's would-be code; S.abandon → peek_slot(id, 2) == None, mailbox gone
}
#[tokio::test] async fn equal_roles_are_refused_by_the_shower() {
    // (Holder, Holder) → S.poll Err(syncPairingBothOn); (Joiner, Joiner) → syncPairingBothOff;
    // after S.abandon the scanner's next call → Err(syncPairingCancelled)
}
#[tokio::test] async fn an_expired_pairing_is_refused() {
    // poll at expires_at → syncPairingExpired; expire_mailbox then C.announce → syncPairingExpired
}
#[tokio::test] async fn a_lost_read_ends_the_pairing_with_a_clear_error() {
    // lose_next_take_answer on S's slot-1 read → Err(Transient); next poll → Err(syncPairingFailed) (SlotConsumed)
}
#[tokio::test] async fn transient_errors_keep_the_session() {
    // fail_next(MailboxTake, RateLimited) and (.., Network("timeout")): poll Err, keeps_session true,
    // stage unchanged; the next poll proceeds
}
#[tokio::test] async fn the_scanner_uses_the_sync_server_the_code_names() {
    // show(Holder, "https://sync.home.example.com"); scan(code) → relay_url() == that; default → DEFAULT_RELAY_URL
}
#[tokio::test] async fn confirm_outside_compare_sends_nothing()   // confirm in WaitingForScan → Err, no put
#[test] fn a_session_prints_nothing_secret()
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::pairing` → FAIL.
- [ ] **Step 3: Implement**; the key in all ten locales.
- [ ] **Step 4: Run** → PASS; i18n; gates.
- [ ] **Step 5: Commit** `feat(sync): pairing session over the relay mailbox`.

---

### Task 4: Joining a new device — pairing credentials or the Recovery Kit

**Files:**
- Modify: `src-tauri/src/services/sync/join.rs`, `src-tauri/src/services/sync/recovery.rs` (+ its tests), `src-tauri/src/services/sync/engine/tests/mod.rs` (`mod join_new;`), `src/i18n/locales/<10>/common.json` (`validation.syncRecoveryAccountInvalid`, `syncRecoveryVaultNotFound`)
- Create: `src-tauri/src/services/sync/engine/tests/join_new.rs`

**Interfaces:**
- Consumes: `join::{join_by_replace, backed_up}` (phase 2), `relay::Relay::vault_info`, `recovery::{parse_recovery_qr_payload, decode_recovery_code, RecoveryKey}`, Task 3 (tests only).
- Produces:

```rust
// join.rs
pub async fn join_new_device<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: Credentials,
    device_name: &str, data_dir: &Path, native: &dyn BindingNative, now: i64) -> SyncResult<PathBuf>;
pub async fn recovery_credentials<R: Relay>(relay: &R, input: RecoveryInput) -> SyncResult<Credentials>;
// recovery.rs
pub struct RecoveryInput { pub relay_url: String, pub account: String, pub vault_id: VaultId, pub key: RecoveryKey } // Debug redacted
pub fn read_recovery_input(code: &str, account: Option<&str>, relay_url: Option<&str>, default_relay: &str) -> Result<RecoveryInput>;
```

`join_new_device`: `validate_device_name`, `validate_base_url(&creds.relay_url)`, refuse when
`state::load` says enabled (`validation.syncAlreadyEnabled`) — all before any backup or network
call; then the verified backup (`backed_up`, the zip named as repair's) and `join_by_replace`;
returns the zip path. `read_recovery_input`: a trimmed code starting `moony-recover:` →
`parse_recovery_qr_payload` (account and address from the QR, `None` address → `DEFAULT_RELAY_URL`;
the arguments are ignored); otherwise `decode_recovery_code`, `account` must be 16 digits
(`validation.syncRecoveryAccountInvalid`), `relay_url` empty or `None` → `default_relay`, else
`normalize_base_url`. `recovery_credentials`: keys `{0: key}`, `vault_info(&creds.auth())`;
`Unauthorized` / `VaultNotFound` → `validation.syncRecoveryVaultNotFound`; else rebuild the keys at
epoch `info.rotation` (correction 6).

- [ ] **Step 1: Failing tests** (`engine/tests/join_new.rs`; `vault()` gives A on, B joined — use a third fresh `TestConn` C with its own rows as the newcomer; `NoBinding`; a temp data dir)

```rust
#[tokio::test] async fn a_new_device_joins_by_pairing_and_converges() {
    // A holds and shows, C (own bank account "Local only") scans as joiner; Task 3 sessions over the same FakeRelay;
    // join_new_device(C, received creds, "iPhone") → zip exists under data_dir;
    // C no longer has "Local only"; run_round on C, A, C → assert_converged(&[A, C]);
    // C's sync_devices row has platform() and a public key; after A's round A lists C's device ID
}
#[tokio::test] async fn a_desktop_joins_from_a_phone_that_holds_the_vault()   // C shows as joiner, A scans as holder; same assertions
#[tokio::test] async fn a_recovery_kit_join_finds_the_epoch_and_converges() {
    // kit = setup::recovery_kit(A); read_recovery_input(recovery_qr_payload(&kit.account_number,
    // &kit.recovery_code, Some("https://relay.test")), None, None, default) and
    // (typed code, Some(account), Some("https://relay.test"), default) both → recovery_credentials → epoch 0 →
    // join_new_device → converged
}
#[tokio::test] async fn a_code_of_another_vault_is_refused_before_any_backup()   // syncRecoveryVaultNotFound; no zip; C unchanged
#[tokio::test] async fn joining_while_sync_is_on_is_refused()                    // A → syncAlreadyEnabled; no zip; FakeRelay calls empty
#[tokio::test] async fn a_hostile_sync_server_in_the_credentials_is_refused_before_anything() // "http://evil.example" → syncRelayUrlInvalid; no zip; no relay call
#[tokio::test] async fn a_join_whose_snapshot_download_fails_changes_nothing_but_the_backup() {
    // fail_next(GetSnapshot, Network) → Err(Transient); C's rows unchanged, sync off, zip exists; a second join succeeds
}
#[tokio::test] async fn a_device_that_turned_sync_off_joins_again() {
    // C joins, setup::turn_off, join_new_device again → new device ID ≠ the first, converged
}
// recovery.rs tests
#[test] fn the_recovery_qr_brings_its_own_account_and_server()
#[test] fn a_typed_code_needs_the_account_and_takes_the_default_server()   // None/"" → default_relay; "123" → syncRecoveryAccountInvalid; "http://evil.example" → syncRelayUrlInvalid
```

- [ ] **Step 2: Run** `cargo test --lib services::sync::engine::tests::join_new services::sync::recovery` → FAIL.
- [ ] **Step 3: Implement**; the two keys in all ten locales.
- [ ] **Step 4: Run** → PASS; i18n; gates.
- [ ] **Step 5: Commit** `feat(sync): join a new device from pairing credentials or the Recovery Kit`.

---

### Task 5: Device list and renaming this device

**Files:**
- Modify: `src-tauri/src/services/sync/devices.rs` (+ tests), `src-tauri/src/models/sync.rs`, `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerated), `shared/schema.ts`

**Interfaces:**
- Produces:

```rust
// models/sync.rs
pub struct SyncDevice {
    id: String,                    // "id": 32 hex
    name: String,                  // "name"
    platform: String,              // "platform": macos | windows | linux | ios | android
    app_version: String,           // "appVersion"
    joined_at: i64,                // "joinedAt"
    last_active_at: Option<i64>,   // "lastActiveAt" (correction 7), unix seconds
    this_device: bool,             // "thisDevice"
    held: bool,                    // "held": sync_heads.quarantined for that device
}
// devices.rs
pub fn list_devices(conn: &Connection) -> Result<Vec<SyncDevice>>;
pub fn rename_this_device(conn: &Connection, name: &str) -> Result<()>;
```

`list_devices`: rows with `removed = 0`; this device first, then `last_active_at` descending
(`None` last), then name; an old row of a re-joined or turned-off device stays listed (P14, no removal
until phase 5). `rename_this_device`: `validate_device_name`, sync on (else
`validation.syncSetupIncomplete`), `UPDATE sync_devices SET name = trim` for this device — a captured
write, so it syncs.

- [ ] **Step 1: Failing tests**

```rust
#[test] fn the_list_marks_this_device_and_orders_by_last_change() {
    // enable DEVICE, record it; insert another row + a sync_clock row of that device at hlc::pack(1_800_000_000_000, 0):
    // [this (last_active_at = sync_state.last_success_at), other (1_800_000_000)]; removed rows absent
}
#[test] fn a_held_device_is_flagged()                    // sync_heads.quarantined = 1 → held
#[test] fn a_device_without_clocks_has_no_last_change()
#[test] fn renaming_this_device_is_captured_and_validated() {
    // rename "  Pracovní Mac " → name "Pracovní Mac", outbox holds ('sync_devices', hex); "" and 61 chars → syncDeviceNameInvalid;
    // sync off → syncSetupIncomplete; another device's row unchanged
}
```

- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::devices` PASS.
- [ ] **Step 5: Wire type.** Register `SyncDevice` in `collect_types()` under "Device sync setup"; `cargo test generate_bindings -- --ignored`; mirror in `shared/schema.ts`; `npm run typecheck`; gates.
- [ ] **Step 6: Commit** (with `shared/generated-types.ts`) `feat(sync): device list and renaming this device`.

---

### Task 6: The flag and the dev sync server at build time (phones)

**Files:**
- Modify: `src-tauri/src/services/sync/feature.rs`, `src-tauri/src/services/sync/relay/http.rs` (`default_base_url`, its test in `relay/http/tests.rs`)

**Interfaces:**
- Produces:

```rust
// feature.rs
#[cfg(debug_assertions)] const BUILT_WITH: Option<&str> = option_env!("MOONY_DEV_SYNC");
#[cfg(not(debug_assertions))] const BUILT_WITH: Option<&str> = None;
fn decide(released: bool, debug: bool, runtime: Option<&str>, built_with: Option<&str>) -> bool;
// released || (debug && (runtime == Some("1") || built_with == Some("1")))
// relay/http.rs
fn pick_dev_base(runtime: Option<&str>, built_with: Option<&str>) -> Option<String>;  // first non-empty, runtime first
```

`default_base_url()` in debug builds: runtime `MOONY_RELAY_URL`, else `option_env!("MOONY_RELAY_URL")`,
else the default; release: the default. Doc comments say an iPhone app has no environment, so a
debug build made with `MOONY_DEV_SYNC=1` carries the flag (cargo rebuilds when the variable changes).

- [ ] **Step 1: Failing tests** — `the_flag_is_off_in_this_build_unless_the_dev_variable_is_set` gains rows `(false, true, None, Some("1")) → true`, `(false, true, Some("0"), Some("1")) → true`, `(false, false, None, Some("1")) → false`, `(false, true, None, Some("yes")) → false`; `fn the_dev_base_prefers_the_runtime_variable()` (`pick_dev_base(Some("http://localhost:1"), Some("http://127.0.0.1:8787"))` → runtime; `(None, Some(x))` → x; `(Some(""), None)` → None).
- [ ] **Steps 2–4:** run (FAIL) → implement → `cargo test --lib services::sync::feature services::sync::relay::http` PASS; `cargo build --release` compiles; gates.
- [ ] **Step 5: Commit** `feat(sync): a debug build can carry the sync flag and dev sync server`.

---

### Task 7: Commands — pairing, joins, devices

**Files:**
- Create: `src-tauri/src/commands/sync_pairing.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!`, under `// Device sync`), `src-tauri/src/commands/sync.rs` (helpers `require_feature`, `app_error`, `now`, `data_dir`, `sync_on`, `profile_relay`, `guarded_change`, `AppEngine`, `verify_password`, `KitOwner`, `kit_owner`, `KeychainBinding` become `pub(super)`; the source-scan test covers both files), `src-tauri/src/commands/auth.rs` (lock, logout, delete account call `forget_pairing()` beside `forget_kit_window()`), `src-tauri/src/commands/backup.rs` (restore, same), `src-tauri/src/models/sync.rs`, `src-tauri/src/bindings.rs`, `shared/generated-types.ts`, `shared/schema.ts`, `src/lib/tauri-api.ts`, `src/i18n/locales/<10>/common.json` (`validation.syncPairingNotStarted`, `syncJoinPasswordRequired`)

**Interfaces:**
- Consumes: Tasks 3, 4, 5.
- Produces:

```rust
// models/sync.rs
pub enum PairingStage { WaitingForScan, Compare, WaitingForOther, Sent, Received }   // camelCase wire names
pub enum PairingRole { Holder, Joiner }                                               // "holder" | "joiner"
pub struct PairingView {
    stage: PairingStage,                 // "stage"
    role: PairingRole,                   // "role"
    qr_svg: Option<String>,              // "qrSvg"  — shower, waitingForScan (recovery::qr_svg of the code text)
    pairing_code: Option<String>,        // "pairingCode" — the same text, for Copy / paste
    comparison_code: Option<String>,     // "comparisonCode" — six digits, from compare on
    expires_at: i64,                     // "expiresAt"
}
// sync_pairing.rs
pub(crate) fn forget_pairing();          // spawns: take the slot, abandon (best-effort burn)
```

| Command | Args | Returns |
|---|---|---|
| `sync_pairing_start` | `relayUrl: Option<String>, deviceName: Option<String>, password: Option<String>` | `PairingView` |
| `sync_pairing_scan` | `code: String, deviceName: Option<String>, password: Option<String>` | `PairingView` |
| `sync_pairing_poll` | — | `PairingView` |
| `sync_pairing_confirm` | — | `PairingView` |
| `sync_pairing_cancel` | — | `()` |
| `sync_pairing_join` | — | `String` (backup path) |
| `sync_join_with_recovery_kit` | `code: String, account: Option<String>, relayUrl: Option<String>, deviceName: String, password: String` | `String` (backup path) |
| `sync_list_devices` | — | `Vec<SyncDevice>` |
| `sync_rename_device` | `name: String` | `()` |

Behaviour. The role is `Role::of(sync_on)` at start/scan. A joiner must pass `deviceName` (validated)
and `password` (`verify_password`; missing → `validation.syncJoinPasswordRequired`) before anything
is shown or written; a holder ignores them. The shower's server follows correction 2 (a holder needs
its stored account; `relayUrl` normalized). One slot,
`static PAIRING: tokio::sync::Mutex<Option<PairingSlot>>` with
`PairingSlot { owner: KitOwner, session: PairingSession, device_name: Option<String> }`; start/scan
abandon an older session first. Every other command takes the slot only when its owner equals
`kit_owner(app, db)`; otherwise it abandons it and answers `validation.syncPairingNotStarted`. The
mailbox calls go to `HttpRelay::new(session.relay_url())`. On an error, `keeps_session` keeps the
slot (poll then answers the unchanged view without an error); any other error abandons it and is
returned. `sync_pairing_confirm` for a holder reads `keys::credentials` with sync on.
`sync_pairing_join` (stage Received only): `guarded_change(AppEngine, join::join_new_device(
HttpRelay::new(&creds.relay_url), …, device_name))`; success drops the slot; a `keeps_session` error
keeps it (stage Received) for "Try again"; any other error abandons it.
`sync_join_with_recovery_kit`: password, `read_recovery_input(.., &default_base_url())`,
`HttpRelay::new(&input.relay_url)`, `guarded_change` around `recovery_credentials` then
`join_new_device`. `sync_rename_device`: `devices::rename_this_device` (no guard). The slot logic sits
in small pure functions so it is testable without Tauri: `fn slot_for(owner: &KitOwner, slot: Option<PairingSlot>) -> (Option<PairingSlot>, Option<PairingSlot> /* to abandon */)`
and `fn after_join_error(error: &SyncError) -> bool /* keep */`. `src/lib/tauri-api.ts` gains on
`deviceSyncApi`: `pairingStart(opts)`, `pairingScan(code, opts)`, `pairingPoll()`, `pairingConfirm()`,
`pairingCancel()`, `pairingJoin()`, `joinWithRecoveryKit(args)`, `listDevices()`, `renameDevice(name)`.

- [ ] **Step 1: Failing tests** (pure, in `sync_pairing.rs`)

```rust
#[test] fn a_pairing_session_belongs_to_one_profile_session() {
    // slot_for(owner(path, 1), Some(slot of owner(path, 1))) keeps it; owner(path, 2) or another path → (None, Some(old))
}
#[test] fn a_transient_join_failure_keeps_the_received_session()  // Transient/RateLimited → true; Local(syncAlreadyEnabled), Refused(..) → false
// sync.rs: every_sync_command_checks_the_flag_first now scans commands/sync.rs and commands/sync_pairing.rs
```

- [ ] **Step 2: Implement** the commands, the models (register `PairingStage`, `PairingRole`, `PairingView`; `cargo test generate_bindings -- --ignored`; `shared/schema.ts`), the wrappers, the hook sites, and the two keys in all ten locales.
- [ ] **Step 3: Run** `cargo test --lib commands::sync`, gates, `git diff --exit-code shared/generated-types.ts` after regeneration, `npm run typecheck`, `npm run lint`, `npm test` → PASS.
- [ ] **Step 4: Commit** (with `shared/generated-types.ts`) `feat(sync): pairing, join and device commands`.

---

### Task 8: Frontend helpers and hooks

**Files:**
- Create: `src/utils/pairing-code.ts` (+ `.test.ts`), `src/utils/sync-devices.ts` (+ `.test.ts`), `src/hooks/use-device-pairing.ts`
- Modify: `src/hooks/use-device-sync.ts` (`useSyncDevices`), `src/hooks/use-device-sync-mutations.ts` (`joinWithRecoveryKit`, `renameDevice`)

**Interfaces:**
- Produces:

```ts
// pairing-code.ts
export type ScannedCodeKind = 'pair' | 'recover' | 'recoveryCode' | 'unknown';
export function scannedCodeKind(text: string): ScannedCodeKind;   // trims; 'moony-pair:' / 'moony-recover:' prefixes; 80 Crockford chars once spaces and dashes go
export function formatComparisonCode(code: string): string;        // '482686' → '482 686'; anything else unchanged
export function pairingPollInterval(view: PairingView | undefined): number | false; // 2000 in waitingForScan / waitingForOther
// sync-devices.ts
export function devicePlatformKey(platform: string): string;       // 'devices.platform.<macos|windows|linux|ios|android>', else 'devices.platform.other'
// use-device-pairing.ts — the view lives under ['device-sync-pairing']
export function useDevicePairing(): {
  view: PairingView | undefined;
  start: UseMutationResult<PairingView, unknown, { relayUrl?: string; deviceName?: string; password?: string }>;
  scan: UseMutationResult<PairingView, unknown, { code: string; deviceName?: string; password?: string }>;
  confirm: UseMutationResult<PairingView, unknown, void>;
  cancel: () => Promise<void>;                                     // cancel + removeQueries
  join: UseMutationResult<string, unknown, void>;                   // REPLACE_DEVICE_MUTATION_KEY, gcTime 0, refreshLocalData
};
// use-device-sync.ts
export function useSyncDevices(): UseQueryResult<SyncDevice[]>;    // ['device-sync-devices'], enabled with the flag and sync on
```

`start`/`scan` (gcTime 0: passwords) and `confirm` write their result into
`['device-sync-pairing']`; a query on that key polls `pairingPoll` at `pairingPollInterval(view)`.
`joinWithRecoveryKit` uses `REPLACE_DEVICE_MUTATION_KEY`, gcTime 0 and `refreshLocalData`;
`renameDevice` invalidates `['device-sync-devices']`. The existing refresh ritual
(`refreshAfterRemoteChanges`) already invalidates every query after remote changes.

- [ ] **Step 1: Failing tests**

```ts
// pairing-code.test.ts
it('tells pairing codes, kit QRs and typed recovery codes apart', …) // 'moony-pair:1:x', ' moony-recover:1:…', the 80-char V2 vector grouped with dashes, 'hello'
it('groups the comparison code in threes', …)                       // '482686' → '482 686'; '0042' unchanged
it('polls only while waiting for the other device', …)               // table over the five stages + undefined
// sync-devices.test.ts
it('maps known platforms to their label keys and the rest to other', …)
```

- [ ] **Step 2: Run** `npx vitest run src/utils/pairing-code.test.ts src/utils/sync-devices.test.ts` → FAIL.
- [ ] **Step 3: Implement** the helpers and hooks.
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 5: Commit** `feat(sync): pairing and device hooks with their pure helpers`.

---

### Task 9: Desktop UI — devices, adding a device, joining

**Files:**
- Create: `src/components/settings/sync/SyncDevicesCard.tsx`, `AddDeviceDialog.tsx`, `JoinSyncDialog.tsx`, `RecoveryJoinDialog.tsx`, `PairingSteps.tsx` (the compare and waiting steps both dialogs share)
- Modify: `SyncSection.tsx` (devices card in the sync-on phases; join dialogs' open state for off/settingUp), `SyncIntroCard.tsx` (two secondary actions), `TurnOffSyncDialog.tsx` (copy), `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
- Consumes: Task 8 hooks, `deviceSyncApi`, `SyncPasswordDialog` patterns, `SyncBusyModal` (already counts `REPLACE_DEVICE_MUTATION_KEY`).

UI (design system `docs/design-system/README.md`, existing sync cards as the model):
- **Devices card** (idle/syncing/paused/error): one row per device — platform label
  (`devicePlatformKey`), name, "This device" badge, joined day, "Last change synced {{when}}" or
  "No changes synced yet", app version, a "Changes held" badge when `held`; "Rename" on this device
  (inline dialog, 1–60 characters); a primary "Add a device". No removal action and no text about it.
- **Add a device** (holder): the QR (`<img src="data:image/svg+xml;base64,…">`), "Scan this with
  Moony on your phone: Settings → Sync → Scan a code", "Copy code" (`navigator.clipboard`, the code
  is not secret), "The code works for 10 minutes"; then the six digits large
  (`formatComparisonCode`), "Do these match the other device?" with "They match" / "They don't
  match" (cancel); then "Sent. Finish on the other device." Closing cancels.
- **Join from your phone** (joiner, from the intro card): warning that this device's data is
  replaced by the synced data after a backup next to the profile; device name (platform default),
  device password, Advanced sync server address; then the QR ("On the phone with sync on: Settings →
  Sync → Add a device"), compare, waiting; on `received` it calls `join` at once (busy modal); success
  toast "This device now syncs. Its earlier data is backed up in {{path}}"; a failure shows its
  message with "Try again" while the view is still `received`.
- **Use a Recovery Kit** (from the intro card): code field (typed, pasted, or the QR text), account
  number, Advanced sync server address, device name, password, the same warning → `joinWithRecoveryKit`.
- **Turn off** dialog: replaces the phase 2 sentence with "You can join again later by pairing with
  another device or with your Recovery Kit; this device's data is then replaced by the synced data
  after a backup."
- Keys (`deviceSync.json`): `devices.*` (title, thisDevice, joined, lastChange, noChange, version,
  held, rename, renameTitle, add, `platform.{macos,windows,linux,ios,android,other}`), `pairing.*`
  (addTitle, scanHint, copyCode, validFor, compareTitle, compareQuestion, match, noMatch, sent,
  waiting, joining, joined, tryAgain), `join.*` (introPair, introKit, warning, nameLabel,
  passwordLabel, advanced, serverLabel, pairTitle, pairHint, kitTitle, codeLabel, accountLabel,
  submit), `setup.defaultName.{ios,android}` ("iPhone", "Android phone"), `turnOff.body` (changed).

- [ ] **Step 1:** No new node test (components are out of scope); `every_sync_validation_key_has_an_english_translation` and the parity gate are the checks.
- [ ] **Step 2: Implement**; every string in all ten locales.
- [ ] **Step 3: Run** `npm test`, `npm run typecheck`, `npm run lint`, `cargo test --lib every_sync_validation_key` → PASS.
- [ ] **Step 4: Manual check** (no real profile): `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787` on the demo copy (port 1422, `demo-home`; relay as in Task 14): the devices card lists this device; rename shows; "Add a device" shows a QR and a code that `scannedCodeKind` reads as `pair`.
- [ ] **Step 5: Commit** `feat(sync): device list, adding a device and joining in desktop Settings`.

---

### Task 10: Phone — the Sync screen

**Files:**
- Create: `src/mobile/pages/settings/SyncSection.tsx`, `src/mobile/components/sync/SyncStatusBlock.tsx`, `SyncDeviceRows.tsx`, `RenameDeviceSheet.tsx`, `SyncPasswordSheet.tsx`, `SyncKitSheet.tsx`, `SyncBusyCover.tsx`, `src/mobile/lib/sync-summary.ts` (+ `.test.ts`)
- Modify: `src/mobile/shell/routes.ts` (+ `routes.test.ts`: `'sync'` after `'backup'`), `src/mobile/pages/settings/SettingsSection.tsx`, `src/mobile/pages/Settings.tsx` (a "Sync" row when `useDeviceSyncFeature()`), `src/mobile/shell/MobileShell.tsx` (mount `useDeviceSyncEvents()` and `<SyncBusyCover />`), `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
- Consumes: `useDeviceSyncStatus`, `useDeviceSyncMutations`, `useSyncDevices`, `useExportTextFile`, `recoveryKitHtml`/`recoveryKitText`, `problemAction`, `Sheet`, `SettingsRow`, `InlineAlert`, `ConfirmSheet`.
- Produces:

```ts
// sync-summary.ts
export type SyncRowSummary = { key: string; tone: 'fresh' | 'stale' | 'neutral' };
export function syncRowSummary(status: SyncStatus | undefined): SyncRowSummary;
// off/settingUp → 'phone.row.off' neutral; idle → 'phone.row.synced' fresh; syncing → 'phone.row.syncing';
// paused + offline → 'phone.row.offline' stale; paused → 'phone.row.paused' stale; error → 'phone.row.error' stale
```

Screen (mobile design system §13): with the flag off `/settings/sync` redirects to Nastavení. Sync
off or setting up: rows "Set up sync on this phone" (Task 12), "Scan a code from another device"
and "Type a Recovery Kit code" (Task 11) — Task 10 renders them disabled. Sync on: status block
(phase line, last synced, pending changes, "Sync now"), the problem alert with its action (repair,
re-sync with password and warning, renew → the self-hosted instruction only, never a checkout on
the phone), device rows with "This device" and rename sheet, "Add a device" (Task 11; disabled here),
"Show Recovery Kit" (password sheet → kit sheet: QR, grouped code, account, "Copy code" through
`copyRecoveryCode`, falling back to `navigator.clipboard` plus "Clear your clipboard after pasting
the code", "Save as text" / "Save as page" through `useExportTextFile`), "Turn off sync" (confirm
sheet), "Delete synced data" (password, confirm). `SyncBusyCover` covers the shell while
`useIsMutating({ mutationKey: REPLACE_DEVICE_MUTATION_KEY })` > 0. Password-bearing mutations keep
gcTime 0. Keys: `phone.row.*`, `phone.title`, `phone.setUp`, `phone.scan`, `phone.typeCode`,
`phone.addDevice`, `phone.busy`, `phone.saveText`, `phone.savePage`, `phone.turnOffBody`.

- [ ] **Step 1: Failing tests** — `sync-summary.test.ts`: `it('maps every phase and the offline problem to a row summary', …)` (table); `routes.test.ts` `lists the sections in root order` expects `'sync'` after `'backup'`.
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib/sync-summary.test.ts src/mobile/shell/routes.test.ts` → FAIL.
- [ ] **Step 3: Implement**; strings in all ten locales.
- [ ] **Step 4: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS (includes `no-colour-literals.test.ts`, `type-roles.test.ts`).
- [ ] **Step 5: Commit** `feat(sync): Settings → Sync in the phone app`.

---

### Task 11: Phone — scanning, pairing and the Recovery Kit join

**Files:**
- Modify: `package.json`, `package-lock.json` (`"@tauri-apps/plugin-barcode-scanner": "2.5.3"`, exact, npmjs registry), `src-tauri/Cargo.toml` + `Cargo.lock` (mobile target block: `tauri-plugin-barcode-scanner = "=2.5.3"  # Sync pairing: scanning the pairing and Recovery Kit QR codes`), `src-tauri/src/lib.rs` (`.plugin(tauri_plugin_barcode_scanner::init())` beside haptics, `#[cfg(mobile)]`), `src-tauri/capabilities/mobile.json` (`barcode-scanner:allow-scan`, `allow-cancel`, `allow-check-permissions`, `allow-request-permissions`, `allow-open-app-settings`; not `default`, not `allow-vibrate`; one sentence in the description), `src/index.css` (`[data-scanning]` rules), `src/mobile/pages/settings/SyncSection.tsx` (enables the rows), `src/i18n/locales/<10>/deviceSync.json`
- Create: `src/mobile/lib/qr-scanner.ts`, `src/mobile/components/sync/ScanOverlay.tsx`, `CodeEntrySheet.tsx`, `JoinDetailsSheet.tsx`, `PairingSheet.tsx`, `RecoveryJoinSheet.tsx`

**Interfaces:**
- Consumes: Task 8 (`useDevicePairing`, `scannedCodeKind`, `formatComparisonCode`), Task 10 screen.
- Produces:

```ts
// qr-scanner.ts — the only importer of the plugin
export type ScanOutcome = { kind: 'code'; text: string } | { kind: 'cancelled' } | { kind: 'denied' };
export async function scanQrCode(): Promise<ScanOutcome>;  // checkPermissions → requestPermissions → scan({ windowed: true, formats: [Format.QRCode] })
export async function cancelQrScan(): Promise<void>;
```

Scanning: the plugin's iOS non-windowed view covers the webview with no way back (checked in its
2.5.3 source), so the scan is `windowed: true`: the camera sits behind a transparent webview.
`ScanOverlay` sets `data-scanning` on `<html>` while open (`src/index.css`: `html[data-scanning],
html[data-scanning] body, html[data-scanning] #root { background: transparent }`, the shell hidden),
draws a frame, "Cancel" and "Enter the code instead", and cancels the scan on `visibilitychange` to
hidden and on unmount. `denied` shows "Allow camera access in Settings" with `openAppSettings`.
Flows by `scannedCodeKind` of the scanned or entered text:
- `pair` with sync off → `JoinDetailsSheet` first (the replace warning, name prefilled "iPhone" /
  "Android phone", password) → `scan` → `PairingSheet` (six digits, "They match" / "They don't
  match"; waiting; on `received` → `join`, busy cover; success toast with the backup path).
- `pair` with sync on ("Add a device") → `scan` with no details → compare → "Sent. Finish on the
  other device."
- `recover` (sync off) → `JoinDetailsSheet` → `joinWithRecoveryKit({ code })`; sync on → "Sync is
  already on" (existing `validation.syncAlreadyEnabled`).
- "Type a Recovery Kit code" → `RecoveryJoinSheet` (code, account, Advanced sync server, name,
  password).
- `CodeEntrySheet` accepts a pasted pairing code (the simulator, and Universal Clipboard from the
  Mac's "Copy code") or a recovery code; `unknown` → "This is not a Moony code".
Keys: `phone.scanTitle`, `phone.scanHint`, `phone.enterInstead`, `phone.cameraDenied`,
`phone.openSettings`, `phone.codeEntryTitle`, `phone.codeEntryLabel`, `phone.notAMoonyCode`,
`phone.addDeviceHint` and the `pairing.*`/`join.*` keys of Task 9.

- [ ] **Step 1:** Confirm `npm view @tauri-apps/plugin-barcode-scanner@2.5.3 time --registry https://registry.npmjs.org` (2026-10-14, ≥ 2 weeks old) and the crate 2.5.3 (2026-10-14; `tauri ^2.12`, rust-version 1.90; the app has tauri 2.12.5 and Rust 1.97); install with `npm install --save-exact @tauri-apps/plugin-barcode-scanner@2.5.3 --registry https://registry.npmjs.org`.
- [ ] **Step 2: Implement**; strings in all ten locales.
- [ ] **Step 3: Run** `npm test`, `npm run typecheck`, `npm run lint`, `cd src-tauri && cargo check` (host build unaffected), `cargo audit` via `scripts/cargo-audit.sh`, `npm audit --omit=dev` → no new findings.
- [ ] **Step 4: Commit** `feat(sync): scanning and pairing in the phone app`.

---

### Task 12: Phone — set up sync as the first device

**Files:**
- Create: `src/mobile/components/sync/SetupSheet.tsx`
- Modify: `src/mobile/pages/settings/SyncSection.tsx`, `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
- Consumes: `useDeviceSyncMutations().{createAccount, startCheckout, prepareRecoveryKit, copyRecoveryCode, finishSetup, devGrant}`, `useDeviceSyncAccount({ pollWhileInactive: true })`, `useExportTextFile`, the kit helpers.

Steps as the desktop's (Task 21 of phase 2), phone-shaped (full-height sheet, `StepProgress`):
account (with the Advanced sync server address) → payment → Recovery Kit (save as text / page,
copy; the "I saved my Sync Recovery Kit" checkbox enabled after a save or copy) → this device
(default "iPhone" / "Android phone") → "Turn on sync". Payment (correction 8): `startCheckout` is
called only to learn `selfHosted`; `selfHosted` → the account number with the ALLOWED_ACCOUNTS
instruction; an `open` answer is never opened — the step says "Paying for sync is not available in
the phone app" and keeps waiting for `active`; `status.devTools` adds "Grant a test subscription".
Keys: `phone.setupTitle`, `phone.payUnavailable`, `phone.payWaiting`.

- [ ] **Step 1:** No node test (component); parity gate.
- [ ] **Step 2: Implement**; strings in all ten locales.
- [ ] **Step 3: Run** `npm test`, `npm run typecheck`, `npm run lint` → PASS.
- [ ] **Step 4: Commit** `feat(sync): first-device setup in the phone app`.

---

### Task 13: iOS natives — this-phone binding, private clipboard, permission texts

**Files:**
- Create: `src-tauri/plugins/moony-keychain/ios/Sources/MoonyBinding.swift` (free functions only)
- Modify: `plugins/moony-keychain/ios/Sources/MoonyKeychainPlugin.swift` (four `@objc` commands; `KeychainCode.unsupported`), `plugins/moony-keychain/src/mobile.rs`, `plugins/moony-keychain/src/error.rs` (`CODE_UNSUPPORTED = "unsupported"` → `Unsupported`), `plugins/moony-keychain/src/models.rs` (bridge payloads + base64 helpers, host-compiled), `src-tauri/Info.ios.plist` (`NSCameraUsageDescription` text; `NSLocalNetworkUsageDescription`), `src-tauri/gen/apple/Localizations/<10 regions>.lproj/InfoPlist.strings`, `src/mobile/permission-strings.test.ts` (`KEYS` gains `NSLocalNetworkUsageDescription`; three comments), `src-tauri/src/commands/sync.rs` (doc of `KeychainBinding`: iOS binds with the Secure Enclave)

**Interfaces:**
- Produces (`Keychain` on mobile, same signatures as the desktop's): `bind_seal(&[u8; 32]) -> Result<Vec<u8>>`,
  `bind_open(&[u8]) -> Result<Zeroizing<[u8; 32]>>`, `copy_secret_text(&str) -> Result<u64>`,
  `clear_clipboard_if_unchanged(u64) -> Result<bool>`. iOS: `run_mobile_plugin` with
  `"bindSeal" {secret: b64}` → `{blob: b64}`, `"bindOpen" {blob}` → `{secret}`,
  `"copySecretText" {text}` → `{token}`, `"clearClipboardIfUnchanged" {token}` → `{cleared}`.
  Android: `Err(Unsupported)` without a bridge call (documented; the UI copies itself and shows the
  clear-your-clipboard line; the binding stays off, as on Linux).

Decision (pre-flight): the macOS design ports to iOS. CryptoKit's
`SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl:)` with
`kSecAttrAccessibleWhenUnlockedThisDeviceOnly` and `.privateKeyUsage` needs no keychain item or
entitlement; the blob carries the key's `dataRepresentation`, byte for byte the macOS binding format
(`0x42 | u16 BE length | key handle | ephemeral x963 (65) | AES.GCM combined`, HKDF label
`moony-binding-v1`). `SecureEnclave.isAvailable == false` (the simulator) rejects `unsupported`; a
blob from another phone fails to restore → `invalidated` (read as "not this device"). The clipboard
uses `UIPasteboard.general.setItems([[UTType.utf8PlainText.identifier: text]], options: [.localOnly:
true, .expirationDate: Date(timeIntervalSinceNow: 60)])` and answers `changeCount`; the clear
empties `items` only when `changeCount` is unchanged. Every `@objc` command stays in
`MoonyKeychainPlugin.swift` (a static library drops categories); the ECIES helpers are free functions
in `MoonyBinding.swift`. Bytes are wiped as on macOS. Texts: camera "Moony photographs documents and
your property, and scans the codes that pair devices for sync; photos are stored encrypted on this
phone."; local network "Moony connects to a sync server on your home network when you use one."
(needed for `sync.home.example.com`, which resolves on the LAN).

- [ ] **Step 1: Failing tests** — plugin `error.rs`: `fn unsupported_rejections_read_as_unsupported()`; `models.rs`: `fn bridge_payloads_round_trip_and_refuse_wrong_lengths()` (a 31-byte secret → `Failed`); `permission-strings.test.ts` expects three keys in each of the ten files.
- [ ] **Step 2: Run** `cd src-tauri && cargo test -p moony-keychain`, `npx vitest run src/mobile/permission-strings.test.ts` → FAIL.
- [ ] **Step 3: Implement**; the ten `InfoPlist.strings` translated (English equals `Info.ios.plist`).
- [ ] **Step 4: Run** the same → PASS; `cargo clippy --all-targets -- -D warnings`; `npm run tauri ios build -- --debug --target aarch64-sim` compiles the Swift (or `xcodebuild` as in Task 16).
- [ ] **Step 5: Commit** `feat(sync): iPhone binding with the Secure Enclave and a private clipboard`.

---

### Task 14: End to end against `wrangler dev`

**Files:**
- Create: `src-tauri/src/services/sync/relay_e2e_pairing.rs` (`#[cfg(all(test, debug_assertions))] mod relay_e2e_pairing;` in `services/sync/mod.rs`)
- Modify: `src-tauri/src/services/sync/relay_e2e.rs` (helper visibility only)

**Interfaces:**
- Consumes: Tasks 2–4; the helpers of `relay_e2e.rs` (`relay_url`, `fresh_account`, device setup) made `pub(super)` where needed.

```rust
#[tokio::test] #[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn the_mailbox_answers_as_documented()          // put 1 → take 1 → Some; take again → SlotConsumed; put 1 again → SlotTaken; burn → put/take MailboxGone; unknown id take → None
async fn a_phone_joins_a_desktop_that_shows_the_code() // A: create_account, dev_grant, setup ("A"); A shows (holder, the code names 127.0.0.1); B (fresh, own rows) scans as joiner; confirm both; join_new_device; rounds B, A → assert_converged; A lists B
async fn a_desktop_joins_from_a_phone_that_scans_its_code() // B shows as joiner, A scans as holder; same assertions
async fn a_device_joins_with_the_recovery_kit()        // typed code + account + relay URL → converged
async fn a_mismatch_burns_the_mailbox_and_sends_nothing()   // both at compare; joiner abandons → holder's confirm → Err(syncPairingCancelled or syncPairingExpired); peek via take → MailboxGone
```

- [ ] **Step 1: Write the tests** (each `#[ignore]`, failing with `NEEDS_RELAY` without the variable).
- [ ] **Step 2: Run** in `../Moony-relay` (branch `feat/dev-grant`; `git branch --show-current` there) `cp .dev.vars.example .dev.vars && npm run dev`, then `cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e -- --ignored --test-threads=1` → all pairing tests and the phase 2 ones PASS (failures are bugs in Tasks 2–4: fix there with a regression test).
- [ ] **Step 3: Run** `cargo test --lib` (ignored skipped) and `cargo test --release --lib` (module absent) → PASS; gates.
- [ ] **Step 4: Commit** `test(sync): pairing and Recovery Kit joins end to end through wrangler dev`.

---

### Task 15: Docs

**Files:**
- Create: `docs/architecture/decisions/0026-device-sync-pairing.md`
- Modify: `docs/architecture/decisions/README.md` (index), `docs/specs/2026-11-06-device-sync-client-design.md` (§20 gains "As built (phase 3)"), `docs/architecture/overview.md` (domain table row, "Device sync" section), `docs/standards/i18n-glossary.md` (if "pairing code" / "comparison code" are new terms)

- [ ] **Step 1: Write ADR 0026**: the pinned bytes (Global Constraints) and the vectors' home; why the shower's key needs no commitment (it travels only in the QR, so a relay or a QR photographer cannot steer the six digits; one guess in 10^6); the fourth code field and which server holds the mailbox (corrections 1–3); confirm gating on both sides; read-once means a lost answer ends a pairing; roles follow sync state and are not in the transcript (a flipped role only stops the pairing); the password before a join; Kit epoch from `rotation`; last change derived from clocks; the iOS binding and clipboard, Android unsupported; the build-time flag; known limits (old rows of re-joined devices stay listed until removal in phase 5; a holder whose slot-2 answer is lost reports failure though the joiner may finish; scanning needs the camera, typing is always offered).
- [ ] **Step 2:** Spec §20 "As built (phase 3, 2026-11-xx)": four bullets pointing at ADR 0026 (code field, roles/directions, device list derivation, phone). Overview: replace "Mobile. Not wired yet" with the phone screen and events; add pairing, the device list, `commands/sync_pairing.rs`, and how to run the phone against a local relay (`MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 npm run ios:dev`). No line numbers or file sizes.
- [ ] **Step 3: Commit** `docs(sync): phase 3 pairing ADR 0026, spec as-built note and overview`.

---

### Task 16: iOS simulator run; the owner's iPhone is a controller step

**Files:** none committed unless a bug is found (fix it in its owning task's files with a regression test, and commit that fix separately).

- [ ] **Step 1: Relay.** `../Moony-relay` on `feat/dev-grant`, `npm run dev` (127.0.0.1:8787; the simulator shares the Mac's loopback).
- [ ] **Step 2: Desktop.** A demo copy (never the real profile; port 1422 in this worktree, `demo-home`, greeting "Alex" before typing the demo password): `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787`; set up sync (dev grant).
- [ ] **Step 3: Simulator.** `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 npm run ios:dev` ("iPhone 17 Pro"). Check the flag reached the build: Nastavení shows "Sync". If it does not (Xcode's build phase dropped the variable), put both in `.env` (read by `dotenv` in `ios:dev`) and record it in the ledger.
- [ ] **Step 4: Pair.** Desktop "Add a device" → "Copy code"; simulator "Scan a code" → "Enter the code instead" → paste (the simulator shares the Mac's pasteboard) → name, password → both show the same six digits → confirm both → the phone joins (busy cover, toast); the desktop lists the phone after its next round; an edit on each side arrives on the other after "Sync now". Then the phone's own "Show Recovery Kit" → Copy (iOS clipboard) works. Also a Kit join on a second simulator profile (typed code + account + `http://127.0.0.1:8787`).
- [ ] **Step 5: Record** results, screenshots and anything not verifiable in the simulator (camera scan, Secure Enclave binding, local-network prompt) in `.superpowers/sdd/2026-11-10-device-sync-phase-3-second-device/progress.md`.
- [ ] **Step 6: Owner's iPhone — controller step, after the owner's go-ahead only.** It installs over his phone app and applies migrations 030–033 to his phone data. Release builds have no sync, so it is a debug build made with `MOONY_DEV_SYNC=1` (no `MOONY_RELAY_URL`: the pairing code brings `https://sync.home.example.com`), on the home LAN. Recipe: memory note `iphone-device-deploy.md` (`/Users/filipkral/.claude/projects/-Users-filipkral-Programov-n--FinanceApp-Moony-tauri/memory/iphone-device-deploy.md`: Xcode 27.1, team `4BN52M4YYK`, build without `--release`, install with `xcrun devicectl device install app`). Checks there: camera scan of the desktop QR, the local-network prompt, the Secure Enclave binding written (`binding_written = 1` after join), the clipboard expiring after 60 s.
