# Device Sync Phase 5 (Removing a Device, Changing the Key) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A device can be removed from sync from any other device, with a new vault key that the removed device never gets; a device can leave by itself; stale rows can be taken off the list without a key change; every other device picks the new key up, offline ones included, without losing a change; the remover saves a new Recovery Kit. Device rows carry signed certificates and every device pins what it verified, so nobody can quietly add a recipient of the new key.

**Architecture:** Two new Rust modules under `src-tauri/src/services/sync/`: `identity/` (Ed25519 signing identities, certificates, leave/removal records, the pinned Local trust store) and `rekey/` (the signed key-change bundle, the rotation state machine with a persisted pending change, the pickup with re-sealing of unsent envelopes). Migration 040 adds three synced write-once columns to `sync_devices` and Local bookkeeping (`sync_trust`, `sync_epochs`, `sync_state` columns). Pairing becomes protocol 3 (the holder hands the joiner a one-time signed ticket in slot 2). The sync server is unchanged: the app starts using its existing `POST /token` and `GET /rekey`. The UI adds removal to the device lists on desktop and phone, a Kit step, and notices.

**Tech Stack:** Rust (rusqlite/SQLCipher, Tauri 2, `ed25519-dalek` new, `x25519-dalek`, `chacha20poly1305`, `hkdf`, `sha2`), React 19 + TanStack Query, react-i18next (ten locales), Vitest.

**Spec:** `docs/specs/2026-11-16-device-sync-phase-5-removal-design.md`. Read §2–§7 before any Rust task and §9 before any UI task. The spec wins over this plan on any conflict; its owner decisions (O1–O12) are binding until the owner vetoes one.

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-phase5`, branch `feat/sync-device-removal`. Check `git branch --show-current` before each commit. Never push, never merge. Commit with explicit paths (`git add <path> …`, never `-A` or `.`), never `--no-verify`. One writer in this worktree at a time; reviewers read only.
- **Precondition (before Task 1):** this branch is rebased onto `main` after `fix/sync-live-test` merged. Check all three: `grep -rn "Heslo do Moony" src/i18n/locales/cs/ | head -1` prints a line; `grep -n "delete_synced_data: Option<bool>" src-tauri/src/commands/auth.rs` prints a line; `git log --oneline -1 main` is an ancestor of `HEAD`. If any fails, stop and report. Where a task touches a file that branch changed (the pairing code entry, the phone sync rows, `DangerZoneCard.tsx`, `DeleteProfileSheet.tsx`, the status refresh), find the spot by role with a grep, never by line number, and never undo what it did.
- Read `AGENTS.md` (ten rules), `docs/standards/` (rust-backend, typescript-frontend, type-contract, testing, i18n, i18n-glossary), `docs/DEPRECATED.md`, and ADRs 0025–0027 and 0030 before your first change.
- **Migration number:** this plan writes **040**. At build time take the next free number after the last entry of `all_migrations()` and use it everywhere this plan says 040.
- **Everything stays behind the flag.** Every new command refuses while `feature::enabled()` is false (the source-scan test `every_sync_command_checks_the_flag_first` must cover the new `commands::sync_devices::` prefix). Capture-or-guard invariants of ADR 0027 hold: an enabled profile is captured, guarded or read-only; every write to `sync_devices` from sync code goes through the capture (it is a synced table) except inside `turn_off`, which drops capture first.
- **Secrets:** signing seeds, ticket secrets, new vault keys and `rotation_pending` live in `Zeroizing` buffers, never derive `Debug` (print `<redacted>`), never cross IPC, never appear in a log line. A log names a device by the first 8 hex characters of its ID at most.
- **Errors:** services return `AppError`/`SyncError`, never `Result<_, String>`; no `.unwrap()` outside tests; new validation keys are `validation.sync…` and get a translation in all ten locales in the task that introduces the UI that shows them (Rust-only tasks list the keys they add; Task 16 adds their texts).
- **Bytes are contracts.** Every format task (1, 5, 6, 8) commits known-answer vectors built from fixed seeds and nonces; a later change to those bytes needs a new version string and new vectors.
- Tests: Rust services and pure TS utils only (`docs/standards/testing.md`); no tests of command handlers, components, hooks or `tauri-api.ts`. Rust tests use `Connection::open_in_memory()` / `test_support::migrated` and the fake relay; never the `Database` struct.
- Gates after every task: Rust tasks `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`; frontend tasks `npm run lint && npm run typecheck && npm run format:check && npm test`; Task 16 both. Task 2 and Task 16 also `cd src-tauri && cargo test generate_bindings -- --ignored` when they change wire types, then commit `shared/generated-types.ts`.
- **Commit split:** Rust-only tasks commit Rust (and `docs/architecture/database.md` where named); frontend-only tasks commit `src/` only. The one exception is Task 16, the wire-contract task, which follows the seven steps of `docs/standards/type-contract.md` and therefore touches `src-tauri/src/models/`, `bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts` and `src/lib/tauri-api.ts` together.
- Every user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`) in the task that introduces it, with each language's plural forms. The Czech and English texts in the spec (§7, §8, §9) are binding; translate the other eight per `docs/standards/i18n-glossary.md`. "Heslo do Moony" / "Moony password"; "synchronizační server" / "sync server", never "relay".
- Look and feel: `docs/design-system/README.md`. Desktop: `SettingsCard`/`SettingsRow`, `Dialog` 560 px, `ChoiceCard` in a `RadioGroup`, destructive solid only in the confirm step. Phone: `EntityRows`, sheets, rows that wrap.
- Never run the app before Task 24, and then only as Task 24 says (own identifiers, ports 1424 and 1425, never 1420–1423, never the owner's real profile).
- **Model per task:** **[opus]** marks design-heavy tasks (crypto formats, trust rules, state machines, multi-device scenarios, the final review); **[sonnet]** the routine ones. Reviews of [opus] tasks use opus; others sonnet.
- Commits: Conventional Commits; the message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (or the attribution line your own session gives you).

## Review Focus

1. **No recipient without a verified, pinned certificate** (spec §5.4): `trust::recipients` never returns an unverified, left or removed entry, nor a superseded one without a later change; a second certificate for a pinned ID changes nothing (Task 3 tests).
2. **The removed device gets nothing new**: it opens no entry of type 0x01 and no body; its old token cannot push, upload, delete a blob or delete the vault (Tasks 6, 7, 14, 15).
3. **No change lost or applied twice across a key change**: the pull reaches the rotation point before re-sealing; acknowledged envelopes are never re-sealed; the re-sealed chain verifies on other devices (Tasks 10, 14).
4. **A pending key change always resolves** (crash at any point, lost answer, race) and no round pushes while one is pending (Task 9).
5. **A server alone never turns sync off**: only a verified notice or record does; anything else is `keyNotGiven`/`keyChangeUnverified` (Task 10).
6. **Bytes**: vectors for certificates, records, bundles, pairing slot 2 v2 and the snapshot Keys record; any flipped byte fails (Tasks 1, 5, 6, 8).
7. **Copy**: every new string in ten locales; nothing still claims a lost device "cannot be cut off" except where still true (Tasks 18–22).

## File map

| Path | Responsibility | Task |
|---|---|---|
| `src-tauri/Cargo.toml`, `Cargo.lock` | `ed25519-dalek` | 1 |
| `services/sync/identity/{mod,cert,record}.rs`, `identity/tests.rs` | identities, certificate and record bytes | 1 |
| `db/migrations.rs`, `db/schema.snapshot`, `services/sync/registry/{tables,keys}.rs`, `merge/columns.rs`, `docs/architecture/database.md` | migration 040, write-once columns | 2 |
| `services/sync/identity/trust.rs`, `identity/trust_tests.rs` | pinned trust store | 3 |
| `services/sync/{setup,join,devices}.rs`, `chain/rejoin.rs`, `engine/runtime.rs` (`rejoin_if_copy`), `identity/mod.rs` | own identity at setup, join, re-join, Kit; pre-phase-5 vaults | 4 |
| `services/sync/pairing/{mod,session}.rs`, `pairing/tests.rs`, `commands/sync_pairing.rs` | pairing protocol 3 | 5 |
| `services/sync/rekey/{mod,bundle}.rs`, `rekey/bundle_tests.rs` | bundle bytes | 6 |
| `services/sync/relay/{mod,http,fake}.rs`, `relay/fake/tests.rs`, `relay/http/tests.rs`, `relay_e2e*.rs` wrappers | `rotate_token`, `rekeys` | 7 |
| `services/sync/snapshot/mod.rs`, `engine/snapshots.rs`, `engine/pull.rs`, `state.rs` | Keys record, `sync_epochs`, epoch rule | 8 |
| `services/sync/rekey/rotate.rs`, `engine/mod.rs`, `engine/status.rs`, `engine/tests/rekey_rotate.rs` | plan, consent, pending, commit, resolve | 9 |
| `services/sync/rekey/{pickup,reseal}.rs`, `engine/{mod,pull,push}.rs`, `engine/tests/rekey_pickup.rs` | triggers, walk, re-seal, removed here | 10 |
| `services/sync/setup.rs`, `identity/trust.rs`, `commands/auth.rs`, `engine/tests/rekey_leave.rs` | leaving, list-only, records naming this device | 11 |
| `services/sync/{join,kit_file,recovery}.rs`, `engine/tests/join_new.rs` | Kit after a key change | 12 |
| `services/sync/blobs/{transfer,queue,pass}.rs`, `engine/tests/blobs.rs` | uploads across a key change | 13 |
| `engine/tests/rekey_scenarios.rs` | multi-device scenarios, property test | 14 |
| `services/sync/relay_e2e_rekey.rs`, `services/sync/mod.rs` | e2e against the real server | 15 |
| `commands/sync_devices.rs`, `commands/sync.rs`, `lib.rs`, `models/sync.rs`, `bindings.rs`, `services/sync/devices.rs`, `engine/status.rs`, `shared/schema.ts`, `shared/generated-types.ts`, `src/lib/tauri-api.ts` | commands and wire | 16 |
| `src/hooks/use-device-sync*.ts`, `src/utils/sync-devices.ts` (+ test), `src/utils/sync-removal.ts` (+ test), `src/utils/sync-state.ts` (+ test) | hooks, mutations, pure helpers | 17 |
| `src/components/settings/sync/SyncDevicesCard.tsx`, `DeviceRow.tsx` (new) | desktop list | 18 |
| `src/components/settings/sync/RemoveDeviceDialog.tsx` (new), `RemovalRecipients.tsx` (new), `NewKitStep.tsx` (new), `RecoveryKitPanel.tsx`, `ShowRecoveryKitFlow.tsx` | desktop removal and Kit step | 19 |
| `src/components/settings/sync/{SyncSection,SyncProblemAlert,SyncStatusCard,KeyChangeNotice,RemovedNotice,KitOutdatedCard}.tsx`, top-bar popover | desktop notices | 20 |
| `src/mobile/components/sync/{SyncDeviceRows,DeviceSheet,RemoveDeviceSheet,NewKitSheet}.tsx`, `src/mobile/pages/settings/SyncSection.tsx` | phone list and removal | 21 |
| `src/mobile/components/sync/{SyncStatusBlock,KeyChangeBlock,RemovedBlock}.tsx` | phone notices | 22 |
| `docs/architecture/decisions/0032-…md`, `decisions/README.md`, ADR 0027 status line, spec §15 "As built", `docs/architecture/overview.md`, `docs/standards/i18n-glossary.md` | docs | 23 |
| — | live checks, final review | 24 |

**Relay (Moony-relay) tasks: none.** Spec §11.3: `POST /v1/vaults/:id/token`, `GET /v1/vaults/:id/rekey`, `rotation` in vault info, previous-token read rules and blob token rules already exist on the server's main. Task 15 proves it against a real server; if it finds a server defect, stop and report it rather than working around it.

---

## Group A — Rust (each task its own commit)

### Task 1 [opus]: Signing identities, certificates and records (spec §5.2–§5.4 bytes)

**Files:** `src-tauri/Cargo.toml` (+ lock), new `src-tauri/src/services/sync/identity/{mod.rs,cert.rs,record.rs,tests.rs}`, `services/sync/mod.rs` (`pub mod identity;`).

- [ ] Add `ed25519-dalek` (the 3.x release that resolves to the same `curve25519-dalek` as `x25519-dalek = "3"`; `cargo tree -i curve25519-dalek` must show one version) with `zeroize` and `rand_core` as needed; comment the purpose like the neighbouring lines. Run `scripts/cargo-audit.sh`.
- [ ] Write the tests first in `identity/tests.rs`:
  - vectors: from fixed 32-byte seeds and a fixed vault ID, device IDs and `issued_at`, the encoded certificate of each kind and both record types match committed hex (generate once, check by hand that each field sits where spec §5.3 says, then freeze);
  - every kind round-trips (`decode(encode(c)) == c`) and `verify_signatures` passes;
  - flipping any single byte of an encoded certificate or record makes `decode` or `verify` fail (loop over every index);
  - kind 2 without a ticket, or kinds 1/3/4 with one, are refused at decode; a ticket whose `holder_sig` is by another key fails; a kind-1/3 certificate whose `issuer_*` fields differ from the subject's fails; a `subject_sig` by another key fails;
  - a `left` record signed by a key other than the one passed to `verify` fails.
- [ ] Implement `identity/mod.rs`: `SignSeed` (a `Zeroizing<[u8; 32]>` newtype, no `Debug`), `fn new_sign_seed() -> Result<SignSeed>` (from `keys::os_random`), `fn sign_pub(&SignSeed) -> [u8; 32]`, and re-exports.
- [ ] Implement `cert.rs`: `CertKind {Genesis = 1, Paired = 2, RecoveryKit = 3, Rejoin = 4}`; `CertBody {kind, vault_id: [u8;16], device: DeviceId, sign_pub, box_pub, issued_at: i64, issuer_device: DeviceId, issuer_sign_pub}`; `Ticket {ticket_pub, issued_at, holder_sig: [u8;64]}`; `Certificate {body, ticket: Option<Ticket>, issuer_sig, subject_sig}`; `encode() -> Vec<u8>`, `decode(&[u8]) -> Result<Self>` (exact lengths), `to_b64`/`from_b64` (base64url, no padding), `hash() -> [u8;32]` (SHA-256 of the encoding), `verify_signatures(&self) -> Result<()>` (the per-kind rules of spec §5.3, no trust lookup); issuers: `issue_self(kind, body_fields, &SignSeed)`, `issue_ticket(holder: &SignSeed, vault, holder_device, now) -> (TicketSecret, Ticket)` (`TicketSecret` zeroizing, no `Debug`), `issue_paired(&TicketSecret, ticket, body, subject: &SignSeed)`, `issue_rejoin(old: &SignSeed, body, subject: &SignSeed)`. Domain strings exactly as the spec.
- [ ] Implement `record.rs`: `RecordType {Left = 1, Removed = 2}`, `Record {kind, vault_id, subject, signer, at, epoch, sig}`, `sign(…, &SignSeed)`, `encode`/`decode`/`to_b64`/`from_b64`, `verify(signer_pub: &[u8;32])`.
- [ ] Verify: the Rust gates. Commit: `feat(sync): signing identities, device certificates and signed records (phase 5)`.

### Task 2 [sonnet]: Migration 040 and the write-once columns (spec §11.1)

**Files:** `src-tauri/src/db/migrations.rs`, `db/schema.snapshot`, `services/sync/registry/tables.rs`, `registry/keys.rs` (schema number test), `merge/columns.rs`, `docs/architecture/database.md`.

- [ ] Tests first: a `migration_040` test module (the pattern of `migration_039`): the three `sync_devices` columns exist and are NULL on old rows; `sync_trust` and `sync_epochs` exist with the spec's columns and CHECK; an enabled `sync_state` with `key_epoch = 0` gets one `sync_epochs` row `(0, 0, …)`, a row without `key_epoch` none; `rekey_seen` and `snapshot_epoch_due` default 0. Registry tests: the three new columns are Synced and in `write_once`; the new tables are Local; the schema number is 40 and the barrier unchanged. Merge test in `merge/tests/`: a later `cert` value for a row that has one is ignored; a `cert` or `left_record` stamped with the row's own device ID beats one stamped with another (as `public_key` does); `removal_record` keeps the older value.
- [ ] Implement `MIGRATION_040` exactly as spec §11.1 (doc comment in the style of 029–039), append `("040_sync_device_removal", MIGRATION_040)`; registry: `write_once: &["public_key", "cert", "left_record", "removal_record"]`; the own-ID stamp rule in `merge/columns.rs` covers `public_key`, `cert` and `left_record`. Update `schema.snapshot` (the snapshot test tells you how) and the sync tables part of `docs/architecture/database.md`.
- [ ] Make the export skip the new Local tables (find the export's sync-table skip list with `grep -rn "sync_unsent" src-tauri/src/services/*export*`), and `setup::turn_off` delete `sync_trust` and `sync_epochs` and clear the new `sync_state` columns (it deletes the row: check). Add both to the existing tests that assert this for the other sync tables.
- [ ] Verify: gates. Commit: `feat(sync): migration 040 for device certificates, the trust store and key epochs`.

### Task 3 [opus]: The pinned trust store (spec §5.4, §5.5)

**Files:** new `services/sync/identity/{trust.rs,trust_tests.rs}`.

- [ ] Tests first (`trust_tests.rs`, in-memory `migrated` DB, certificates from Task 1 issuers):
  - `seed_own` pins this device's certificate; `is_supported` is true only then (false for a pre-phase-5 profile, §5.5);
  - `refresh` over `sync_devices` rows: a paired certificate whose holder is trusted is pinned; one whose holder is unknown stays out, and is pinned in a later `refresh` once the holder is (fixpoint inside one call when both arrive together); a genesis certificate arriving through sync is never pinned; a Kit certificate is pinned with kind 3; a re-join certificate is pinned when its issuer is trusted;
  - pinning: a second, different certificate for a pinned ID changes nothing and is reported in `RefreshReport.conflicts`; a ticket used by a second ID is refused and reported;
  - records: a `left` record signed by its subject moves `trusted → left`; a `removed` record from a trusted signer moves to `removed` with `cut_off_epoch = NULL`; from an unknown or removed signer it is ignored; `removed` never moves back; a record naming this device that this device did not sign is returned in `RefreshReport.about_me`;
  - `export`/`import_export`: an export imports on an empty store; an export without the holder's certificate, or with an entry that does not verify, is refused and writes nothing;
  - `import_bundle` pins the signer and recipients (endorsed) and applies the removal records, all in one savepoint;
  - `tofu_from_rows` (Kit joiner) pins every row whose signatures verify, genesis included;
  - `recipients(conn, removing, now)`: excludes removing, left, removed, unverified rows and rows without `public_key`; includes a superseded ID only when its newest `sync_clock` time is after its successor's `issued_at` (O11); returns each exclusion with its `ExclusionReason`; refuses more than 64 (`validation.syncTooManyDevices`).
- [ ] Implement `trust.rs` with these functions (all synchronous, `&Connection`): `seed_own`, `is_supported`, `refresh(conn, now) -> Result<RefreshReport>`, `export(conn) -> Result<TrustExport>` (serde, base64url fields), `import_export(conn, &TrustExport, holder: DeviceId)`, `import_bundle(conn, signer_cert, recipients, removed_records, to_epoch)`, `tofu_from_rows`, `entry(conn, DeviceId) -> Result<Option<TrustEntry>>`, `entries(conn)`, `recipients(conn, &[DeviceId], now) -> Result<Recipients>`, `mark_left`, `mark_removed(conn, device, record, cut_off_epoch: Option<u32>)`, `cut_off_all_pending(conn, epoch)` (left/removed entries with `cut_off_epoch IS NULL` get `epoch`). `ExclusionReason {Removing, Left, Removed, Unverified, SupersededQuiet, NoKey}`.
- [ ] Verify: gates. Commit: `feat(sync): the pinned trust store of verified devices`.

### Task 4 [sonnet]: This device's identity at setup, join, re-join and Kit join (spec §5.2, §5.3, §5.5)

**Files:** `services/sync/identity/mod.rs`, `setup.rs`, `join.rs`, `devices.rs`, `chain/rejoin.rs` (or wherever `rejoin_as_new_device` creates the new key pair: `grep -rn "new_device_keypair" src-tauri/src/services/sync`), `keys.rs`, engine tests `setup.rs`, `join_new.rs`, `binding_failure.rs`.

- [ ] Tests first: after `finish_setup` the profile has a signing seed, a pinned genesis certificate and its row's `cert` set (captured: it is in the outbox); after a re-join the new ID has a re-join certificate whose issuer is the old ID, the old ID's entry stays `trusted`; a re-join of a pre-phase-5 profile (no old seed) creates a seed but no certificate; a Kit join pins a kind-3 certificate and runs `tofu_from_rows`; at unlock a pre-phase-5 enabled profile gets a seed and no certificate and `is_supported` stays false.
- [ ] Implement `identity::create_own(conn, kind, issuer: Issuer, now)` where `Issuer` is `SelfSigned`, `Ticket(TicketSecret, Ticket)` or `Predecessor(SignSeed, DeviceId)`; store the seed in `sync_state.device_sign_key`; `devices::record_this_device` gains an optional certificate and writes `cert` with the write-once SQL it already uses for `public_key`. Wire it into `finish_setup` (genesis), the re-join path (predecessor = the old seed, read before it is replaced), the Kit join (self-signed, then `tofu_from_rows`). The pairing join is wired in Task 5. Never log a seed.
- [ ] Verify: gates. Commit: `feat(sync): each device signs its own certificate at setup, re-join and Kit join`.

### Task 5 [opus]: Pairing protocol 3 — the holder vouches for the joiner (spec §5.3, §11.2)

**Files:** `services/sync/pairing/{mod.rs,session.rs,tests.rs,session_tests.rs}`, `commands/sync_pairing.rs`, `join.rs` (`join_new_device`), `src/utils/pairing-code.ts` only if the code-prefix check lives there too (then it is a frontend change: put it in Task 17 instead and note it here).

- [ ] Tests first: new vectors for slot 2 v2 (fixed session key and nonce; the JSON with `identity` and `head`); `QR_PREFIX` is `moony-pair:3:`; a `:2:` code answers `OTHER_NEEDS_UPDATE`, `:4:` `THIS_NEEDS_UPDATE`; slots 1 and 3, the HKDF info and the acknowledgement vectors are unchanged (the old ack vectors still pass); a v2 payload over 48 KiB is refused; `identity: null` (a pre-phase-5 holder) is accepted; unknown fields still refused; the manual code entry path of branch (d) parses a `:3:` code (find it with `grep -rn "moony-pair" src-tauri/src src/ | grep -v test`).
- [ ] Implement: payload `v: 2` with `keys: [{epoch, key, head}]` and `identity: {holder, export, ticket: {secret, holderSig, issuedAt}} | null`; the holder fills it at `sync_pairing_confirm` (under `engine::exclusive`, credentials read then): `trust::export` and `cert::issue_ticket`, the ticket secret wiped right after sealing; the received session keeps the ticket next to the credentials and the same wiper wipes both; `join_new_device` imports the export (`trust::import_export`) and creates its identity with `Issuer::Ticket` (Task 4) inside the join's savepoint, after the replace; a holder whose `is_supported` is false sends `identity: null`.
- [ ] Verify: gates. Commit: `feat(sync): pairing protocol 3, the holder vouches for the joiner with a one-time ticket`.

### Task 6 [opus]: The key-change bundle (spec §6.2, §6.3 bytes)

**Files:** new `services/sync/rekey/{mod.rs,bundle.rs,bundle_tests.rs}`, `services/sync/mod.rs`.

- [ ] Tests first: a vector built from a fixed ephemeral secret, fixed nonces, two recipients and one notice (inject randomness through a `&mut dyn FnMut(&mut [u8])` filler so tests are deterministic); each recipient opens type 0x01 with the new key, the removed device opens only its notice, a stranger opens nothing; the body opens with the new key alone (Kit path) and its signature verifies with the signer's key; every flipped byte of header, entries or body fails; an entry moved to another bundle (other header) fails; reordering entries fails the body (hash of entries in the AAD); over 64 entries or over 256 KiB refuses to build; `keys` round-trip with heads.
- [ ] Implement `BundleInput`, `build(&BundleInput, fill) -> Result<Vec<u8>>`, `open(bytes, vault_id, my_box_secret) -> Result<Opened>` with `Opened::{Recipient {new_key, header, body: BundleBody}, Notice {header, record}, Nothing}`, `open_body_with_key(bytes, vault_id, key) -> Result<(Header, BundleBody)>` (Kit path), `BundleBody::verify(&self, signer_pub)`. Domain strings exactly as spec §6.2. Secrets zeroized.
- [ ] Verify: gates. Commit: `feat(sync): the signed key-change bundle`.

### Task 7 [sonnet]: `rotate_token` and `rekeys` in the relay client and the fake (spec §11.3)

**Files:** `services/sync/relay/{mod.rs,http.rs,fake.rs}`, `relay/fake/tests.rs`, `relay/http/tests.rs`, every `impl Relay for` wrapper (`grep -rn "impl Relay for" src-tauri/src`).

- [ ] Tests first: HTTP (the mock server pattern of `relay/http/tests.rs`): `POST /v1/vaults/:id/token` with `{"token", "rekey"}` (standard base64 with padding: the server checks `length % 4`), the answer `{rotation, head}`; `GET …/rekey?after=&limit=` paged with `more`; error codes map as the table in `relay/mod.rs` already lists (`token_read_only`, `invalid_rekey`, `rekey_too_large`, `too_many_rotations` now its own `RelayError::TooManyRotations`). Fake: after a rotation the old token reads (pull, vault info, snapshot, rekey, blob get/head/list) and gets `TokenReadOnly` on push, snapshot put, blob put/delete, vault delete and a second rotation; after 30 days on the fake's clock the old token is `Unauthorized` and its bundle gone; the 33rd rotation inside 30 days is `TooManyRotations`; `vault_info.rotation` counts; the head recorded per bundle is the head at rotation.
- [ ] Implement `Relay::rotate_token(&self, vault: &VaultAuth, new_token: &str, bundle: &[u8]) -> RelayResult<Rotated>` and `Relay::rekeys(&self, vault: &VaultAuth, after: u32, limit: u32) -> RelayResult<RekeyPage>` (`Rekey {rotation, head, at, bundle: Vec<u8>}`; the client caps the page at 4 MiB of bundles and refuses rotations not ascending above `after`, like `check_blob_page`); HTTP timeouts 15 s; the fake models the server rules above. Update every wrapper.
- [ ] Verify: gates. Commit: `feat(sync): the relay client rotates the vault token and fetches key-change bundles`.

### Task 8 [sonnet]: Older keys in snapshots, epoch heads and the epoch rule (spec §6.1, §8.4)

**Files:** `services/sync/snapshot/{mod.rs,stream.rs}`, `snapshot/tests/`, `engine/snapshots.rs`, `engine/pull.rs`, `state.rs`, `join.rs` (credentials carry heads), `keys.rs`.

- [ ] Tests first: a dump writes one `Keys` record with every epoch and its head; a replace restore adds the keys and `sync_epochs` rows; a merge restore adds only missing ones; a `Keys` record that disagrees with a key this device has for that epoch refuses the snapshot (`validation.syncSnapshotInvalid`); vector for the `Keys` record's JSON shape. Pull: with `sync_epochs` (1, head 50), an op at 51 under epoch 0 stops `Invalid`; an op at 50 under epoch 1 stops `Invalid`; an op at 60 under epoch 1 applies; an epoch with no known head is not checked (logged once).
- [ ] Implement the record (after `Header`, before rows), the restore handling, `state::epochs(conn)`, `state::record_epoch(conn, epoch, head, by, now)`, and the check in `engine/pull.rs::handle_op` after the envelope opens (the epoch is authenticated then). `Credentials` gains the heads (`VaultKeys` stays as is; a side map `BTreeMap<u32, u64>` is enough).
- [ ] Verify: gates. Commit: `feat(sync): snapshots carry every older key; ops are checked against key-change points`.

### Task 9 [opus]: Removing and changing the key on this device (spec §6.4, §6.6, §10)

**Files:** new `services/sync/rekey/rotate.rs`, `engine/mod.rs` (resolve first), `engine/runtime.rs` (resolve at start), `engine/status.rs` (`rotation_pending`), `state.rs`, new `engine/tests/rekey_rotate.rs`.

> **Fix round (spec As built 26–31):** `plan`/`remove` take the Kit device IDs the person ticked and pass them to `trust::recipients(conn, removing, kit_ticked)`; the ticks are part of the consent; ticked devices are recipients of the bundle, and the `200` commit calls `trust::confirm_kit(conn, ticked)` in its savepoint. `mark_removed` accepts a signer that left or was removed no earlier than the record (item 26); an unconfirmed signer's removal is refused.

- [ ] Tests first (fake relay, engine test helpers of `engine/tests/mod.rs`):
  - `plan` for `[C]` lists A and B as recipients and C as removing; the consent changes when a recipient's certificate, the epoch, the mode or the removed set changes; `remove` with a stale consent answers `validation.syncRemovalPlanChanged` and writes nothing;
  - preconditions: not supported (§5.5) → `validation.syncRemovalUnsupported`; removing itself → `validation.syncRemoveThisDevice`; capture broken → `validation.syncChangesPaused`;
  - list-only: removal records written to the rows (in the outbox), entries `removed` with `cut_off_epoch NULL`, no relay call;
  - rotate `200`: one new key, `key_epoch` +1, `sync_epochs` row with the answered head, removal records written, entries cut off at the new epoch (including earlier list-only and left entries), `rotation_pending` NULL, `snapshot_epoch_due` set; the bundle opens for A and B and only the notice for C;
  - `403` → pending wiped, `validation.syncRotationRaced` (the other bundle is picked up by Task 10; here assert the error and that nothing was committed); `409` → `validation.syncTooManyKeyChanges`; network error → pending kept, outcome `Pending`;
  - crash points: pending stored then the process "dies" (drop state, reopen DB): `resolve_pending` sends and commits; `POST` landed but answer lost: `resolve_pending` probes with the new token and commits without a second `POST`; `POST` never landed and another device rotated meanwhile: resolves as raced;
  - a round never pushes while `rotation_pending` is set (it resolves first, and if that fails, ends with `rotationPending` and pushes nothing);
  - any pairing session is forgotten when a key change starts (assert through the pairing slot's owner API).
- [ ] Implement `RemovalMode {ListOnly, Rotate}`, `plan(conn, removing, mode, now) -> Result<Plan>`, `consent(&Plan) -> String` (hex SHA-256 over a canonical encoding of the fields the spec names), `async fn remove<R, D>(relay, db, consent, mode, removing, now) -> SyncResult<Outcome>` under `engine::exclusive` (password checked by the command, Task 16), `async fn resolve_pending<R, D>(relay, db) -> SyncResult<Option<Outcome>>`. The pending JSON (secret-bearing) is written and read only in `rotate.rs`. Commit everything of a `200` in one savepoint; the re-seal of the remover's own unsent envelopes calls Task 10's `reseal::reseal_unsent` (write a stub in this task that Task 10 fills, or order Task 10's `reseal.rs` part first: implementer's choice, note it in the commit).
- [ ] Verify: gates. Commit: `feat(sync): remove a device or change the key, with a pending change that always resolves`.

### Task 10 [opus]: Picking up a new key; the removed device (spec §7.1–§7.5)

**Files:** new `services/sync/rekey/{pickup.rs,reseal.rs}`, `engine/{mod.rs,pull.rs,push.rs,status.rs,runtime.rs}`, `setup.rs` (turn off as removed), `models/sync.rs` (two problems; the wire part is Task 16, so add the variants here with their serde names), new `engine/tests/rekey_pickup.rs`.

> **Fix round (spec As built 29, 31):** a bundle's signer must be pinned, `trusted` and not `kit_unconfirmed` to verify; `trust::import_bundle` now returns the removal records naming this device (not applied) — handle them like `RefreshReport::about_me`.

- [ ] Tests first:
  - triggers: a push answered `TokenReadOnly`, an op whose header names a higher epoch, and `vault_info.rotation > rekey_seen` at engine start each run the pickup once;
  - B with two unsent envelopes, one of which the relay accepted before the rotation: the pickup acknowledges that one through the pull, re-seals only the other with `prev` = the acknowledged hash, the round pushes it, and A applies it (chain verifies, no quarantine);
  - B behind by two key changes walks both bundles and ends at epoch n+2 with every key;
  - the pull held by a gap before the rotation head: the own-envelope scan acknowledges B's envelopes up to the head; when the scan cannot reach it, nothing is re-sealed and the round pushes nothing;
  - C (removed) receives the notice: sync is off, local data unchanged, `sync.removed_notice` holds `{by, at, unsent}` with the right count, no leave record was written;
  - a bundle with no entry for this device and no notice → `keyNotGiven`, sync stays on, nothing deleted; a bundle signed by an unknown device → `keyChangeUnverified`; a junk bundle → `keyNotGiven`;
  - `sync.key_change_notice` is written with the signer's and the removed devices' names; `rekey_seen` advances; `KeyRotated` shows only while a pickup could not finish.
- [ ] Implement `reseal::reseal_unsent(conn, creds) -> Result<usize>` (one savepoint; leaves envelopes already under the current epoch alone, so it is idempotent), `pickup::pick_up<R, D>(relay, db, now) -> SyncResult<Pickup>` with `Pickup::{Nothing, Adopted {..}, RemovedHere {..}, NotGiven, Unverified}`, `rekey::removed_here(conn, data_dir, by, at)` (counts unsent, writes the notice key, calls `setup::turn_off`), the hooks in `push.rs` / `pull.rs` / engine start, the problems `KeyNotGiven` and `KeyChangeUnverified` with the precedence of spec §7.5.
- [ ] Verify: gates. Commit: `feat(sync): devices pick up a new key, re-seal what they had not sent, and a removed device turns sync off`.

### Task 11 [sonnet]: Leaving, list-only removals arriving, records about this device (spec §6.5, §7.4)

**Files:** `services/sync/setup.rs`, `identity/trust.rs` (refresh after merge), `engine/pull.rs` (refresh hook), `commands/auth.rs` (`delete_account` else-branch), `commands/sync.rs` (`sync_turn_off` calls the leave first), new `engine/tests/rekey_leave.rs`.

- [ ] Tests first: `leave_best_effort` signs a `left` record, writes it to this device's row and pushes it within its budget (fake relay with a delay over 5 s: turn-off still completes and nothing waits); after a pull that merged a `left` row, the other device's entry is `left`; after a pull that merged a `removed` record naming this device (list-only removal elsewhere), this device turns sync off through `removed_here`; a `left` record for this device that it did not sign does the same; `trust::refresh` runs after every merge that touched `sync_devices` and at unlock.
- [ ] Implement `setup::leave_best_effort<R, D>(relay, db, budget)` (sign, write captured, one push round without pull, ignore every failure), call it from `sync_turn_off` (sync on) and from `delete_account` when `delete_synced_data` is false and sync is on (rebased branch (a)); the refresh hook and its `about_me` handling.
- [ ] Verify: gates. Commit: `feat(sync): a device that turns sync off or is deleted says it left`.

### Task 12 [sonnet]: The Recovery Kit after a key change (spec §8)

**Files:** `services/sync/join.rs` (`recovery_credentials`), `kit_file.rs`, `recovery.rs`, `setup.rs` (`kit_epoch_saved`), `commands/sync_kit.rs` (a completed save marks it), `engine/tests/join_new.rs`.

- [ ] Tests first: a Kit made at epoch 2 joins a vault after two key changes using the newest bundle (keys and heads from its body); the same with the bundles expired (fake clock + 30 days) using the snapshot's `Keys` record; an epoch-0 Kit after a change is refused with `validation.syncRecoveryKitRotated`; no bundle and a snapshot under an older epoch → `validation.syncRecoveryKitNeedsSnapshot`; the Kit page and text carry "Sada č. {n} · platí od {date}" (the HTML template test pattern of `kit_file.rs`; the line is localised by the frontend string passed in, check how the existing labels reach the template); a completed save sets `kit_epoch_saved = key_epoch`.
- [ ] Implement the four steps of spec §8.3 in `recovery_credentials`, the template line, `setup::mark_kit_saved(conn)`.
- [ ] Verify: gates. Commit: `feat(sync): the Recovery Kit works after a key change and old Kits are refused`.

### Task 13 [sonnet]: Documents and photos across a key change (spec §7.6)

**Files:** `services/sync/blobs/{transfer.rs,queue.rs,pass.rs,stream.rs}`, `engine/tests/blobs.rs`.

- [ ] Tests first: an upload answered `TokenReadOnly` rests (no retry storm) and, after the pickup, is sealed again under the new epoch and stored; an upload whose token rotates at completion behaves the same; a blob sealed under epoch 0 downloads and opens on a device at epoch 1; a removed device's blob delete is refused by the fake; the daily clean-up does not run while a pickup is due.
- [ ] Implement what the tests need; check first how `lane_may_run` treats `KeyRotated` (`engine/runtime.rs`) and keep downloads running.
- [ ] Verify: gates. Commit: `fix(sync): uploads interrupted by a key change are sealed again under the new key`.

### Task 14 [opus]: Multi-device scenarios and a property test (spec §10, §14.1)

**Files:** new `src-tauri/src/services/sync/engine/tests/rekey_scenarios.rs` (register in `engine/tests/mod.rs`).

- [ ] Write these as tests over the fake relay with real rounds (helpers of `join_new.rs`): A removes C while B is offline with unsent changes, B returns; A and B remove each other at once (exactly one wins; the loser is removed, or sees the race); A removes C while B is pairing D (D ends `keyNotGiven`, then pairs again); a list-only removal of C on B followed by a key change on A (C gets the notice and turns off); a pre-phase-5 vault (no certificates) syncs and refuses removal.
- [ ] Property test (the crate's existing property-test style, a fixed seed list plus N random seeds, bounded runtime under 60 s): four devices, random edits, offline periods, list-only removals, key changes and removals; at the end every remaining device's synced tables are equal (`test_support::divergence`), every own envelope was applied exactly once on every other remaining device, and no removed device holds a key of an epoch after its removal.
- [ ] Verify: gates. Commit: `test(sync): multi-device removal and key-change scenarios`.

### Task 15 [sonnet]: Against a real sync server (spec §14.2)

**Files:** new `src-tauri/src/services/sync/relay_e2e_rekey.rs`, `services/sync/mod.rs` (`#[cfg(test)] mod relay_e2e_rekey;`).

- [ ] Write ignored tests following `relay_e2e.rs` and `relay_e2e_blobs.rs`: a rotation with a real bundle; the old token's pull, rekey fetch and vault read succeed, its push, blob upload, blob delete and vault delete get `TokenReadOnly`; two concurrent rotations, exactly one `200`; A/B/C end to end (C cannot open new ops; B re-seals and A applies); a Kit join after a rotation; a blob upload whose token rotates while the body streams.
- [ ] Run (record the output in the commit body):
  ```bash
  cd /Users/filipkral/Programování/FinanceApp/Moony-relay && npm ci && npm run build
  DATA_DIR=$(mktemp -d) PORT=8795 SETUP_CODE=moony-e2e-setup-code node dist/main.js &
  cd /Users/filipkral/Programování/FinanceApp/Moony-tauri-phase5/src-tauri
  MOONY_RELAY_E2E=http://127.0.0.1:8795 cargo test --lib relay_e2e -- --ignored --test-threads=1
  ```
  Stop the server afterwards. The relay checkout is read-only for this plan; if a test exposes a server defect, stop and report.
- [ ] Commit: `test(sync): key changes against a real sync server`.

### Task 16 [sonnet]: Commands, status and the wire contract (spec §9, §11.4) — the one mixed commit

**Files:** new `src-tauri/src/commands/sync_devices.rs`, `commands/mod.rs`, `commands/sync.rs` (flag scan prefix), `lib.rs`, `models/sync.rs`, `bindings.rs`, `services/sync/devices.rs`, `engine/status.rs`, `shared/schema.ts`, `shared/generated-types.ts`, `src/lib/tauri-api.ts`, `src/i18n/locales/*/common.json` (the new `validation.sync…` texts).

> **Fix round (spec As built 29):** `sync_removal_plan` and `sync_remove_devices` also take `kitTicked: string[]` (device IDs); the plan's excluded rows gain the reason `kitUnconfirmed`; `SyncDevice` may carry `kitUnconfirmed` for the list label.

- [ ] Tests first (Rust): `list_devices` returns every row with `state`, `verified`, `joinedVia`, `addedBy`, `stateAt`, `stateBy`, `keepsKey`, `pending` (left and removed rows included; ordering: this device, active by newest change, then the rest); status: `removalSupported`, `rotationPending`, `kitOutdated` (`kit_epoch_saved < key_epoch`, NULL counts as 0), `keyChange`, `removedNotice` (also with sync off), `keptKeyCount`; the flag scan covers `commands::sync_devices::`.
- [ ] Commands (each checks the flag first, the password where named, maps errors with `app_error_for`): `sync_removal_plan(device_ids, mode, also_remove) -> RemovalPlan`; `sync_remove_devices(password, mode, device_ids, consent) -> RemovalOutcome` (password required for both modes; runs as a sync change, `guarded_change`); `sync_kit_saved()`; `sync_dismiss_notice(kind: "keyChange" | "removed")`. Register in `lib.rs`.
- [ ] Wire, the seven steps of `docs/standards/type-contract.md`: the `SyncDevice` fields, `RemovalPlan`, `RemovalOutcome`, `SyncStatus` fields and `SyncProblem` variants exactly as spec §11.4, per-field `#[serde(rename)]`, registered in `bindings.rs`, `cargo test generate_bindings -- --ignored`, `shared/schema.ts` by hand, `tauri-api.ts` wrappers (`removalPlan`, `removeDevices`, `kitSaved`, `dismissSyncNotice`).
- [ ] Error texts in all ten locales: `syncRemovalPlanChanged`, `syncRotationRaced`, `syncTooManyKeyChanges`, `syncTooManyDevices`, `syncRemovalUnsupported`, `syncRemoveThisDevice`, `syncRecoveryKitNeedsSnapshot`, and the reworded `syncRecoveryKitRotated` (spec §8.3).
- [ ] Verify: both gate sets. Commit: `feat(sync): device removal commands and status on the wire`.

## Group B — Frontend (each task its own commit, `src/` only)

### Task 17 [sonnet]: Hooks, mutations and pure helpers

**Files:** `src/hooks/use-device-sync.ts`, `use-device-sync-mutations.ts`, `src/utils/sync-devices.ts` (+ test), new `src/utils/sync-removal.ts` (+ test), `src/utils/sync-state.ts` (+ test).

- [ ] Tests first (Vitest): `groupDevices(devices)` → `{active, folded}` (left, removed, superseded folded; this device first); `joinedViaKey(device)` and `stateBadges(device)` give the right i18n keys for every combination of the spec §9.1 table; `defaultRemovalMode(device)` = `listOnly` for left, superseded and unverified rows, else `rotate`; `availableModes(device)` has only `listOnly` for a left row; `excludedReasonKey(reason)`; `statusPopoverLines(status)` adds `kitOutdated` and `keyChange` lines; `rotationPending` reads as syncing in `sync-state.ts`.
- [ ] Hooks: `useRemovalPlan(ids, mode, also)` (query, `gcTime: 0`, disabled until the dialog opens), mutations `removeDevices` (password-bearing: `gcTime: 0`, never kept in state), `kitSaved`, `dismissSyncNotice`; each mutation invalidates the sync status and device list keys (find them in `use-device-sync.ts`). Rule 7's portfolio keys do not apply (no finance data changes).
- [ ] Verify: frontend gates. Commit: `feat(sync-ui): hooks and helpers for removing devices`.

### Task 18 [sonnet]: The desktop device list (spec §9.1)

**Files:** `src/components/settings/sync/SyncDevicesCard.tsx`, new `DeviceRow.tsx`, locales `deviceSync.json` ×10.

- [ ] Rows per spec §9.1: badges, hint line with how it joined, the folded group "Odpojená a odebraná zařízení (n)" in a `Collapsible`, actions "Odebrat…" (outline sm), "Odebrat ze seznamu", "Vyměnit klíč"; a pending removal shows "Odebírá se…"; a pre-phase-5 vault shows the O3 note and no actions; the "Ztratili jste zařízení…" collapsible text replaced (spec §9.1).
- [ ] Strings (cs binding, en source): `devices.joinedVia.{created,paired,recoveryKit,rejoined}`, `devices.state.{left,removed,removedKeepsKey,superseded,unverified,pending}`, `devices.folded_*` (plural), `devices.remove`, `devices.removeFromList`, `devices.changeKey`, `devices.unsupported`, `limits.lostDevice` (new text). Ten locales.
- [ ] Verify: frontend gates. Commit: `feat(sync-ui): the desktop device list shows how each device joined and lets you remove it`.

### Task 19 [sonnet]: The desktop removal dialog and the Kit step (spec §9.2, §8.1)

**Files:** new `src/components/settings/sync/{RemoveDeviceDialog,RemovalRecipients,NewKitStep}.tsx`, `RecoveryKitPanel.tsx` / `ShowRecoveryKitFlow.tsx` (reuse, do not fork), `SyncDevicesCard.tsx` (open it), locales ×10.

> **Fix round (spec §9.2, As built 29):** step 2 lists `kitUnconfirmed` devices with the opt-in tick "Poslat nový klíč i tomuto zařízení — připojeno záchrannou sadou" (unticked) and the line "Zařízení, které nezaškrtnete, nový klíč nedostane a bude se muset připojit znovu."; ticking reloads the plan with `kitTicked`. Strings in ten locales.

- [ ] Steps 1–4 and the failure table of spec §9.2 exactly: choice cards (`ChoiceCard` in `RadioGroup`, "Doporučeno" badge), recipient and excluded lists, "Odebrat také" ticks that reload the plan, the "Heslo do Moony" field focused, the destructive primary named like the title, progress in the dialog body, the Kit step with the tick "Novou záchrannou sadu mám uloženou" and "Hotovo"; `syncRemovalPlanChanged` reloads step 2; the pending outcome closes with its sentence; "Vyměnit klíč" opens the same dialog at step 2 with nobody removed (title "Vyměnit klíč synchronizace?").
- [ ] Strings: `remove.*` (titles, both cards, list heads, reasons, progress steps, every failure line), `newKit.*`. Ten locales.
- [ ] Verify: frontend gates. Commit: `feat(sync-ui): remove a device or change the key on the desktop`.

### Task 20 [sonnet]: Desktop notices and texts (spec §7.3, §7.4, §7.5, §8.1, §9.3)

**Files:** new `src/components/settings/sync/{KeyChangeNotice,RemovedNotice,KitOutdatedCard}.tsx`, `SyncSection.tsx`, `SyncProblemAlert.tsx`, `SyncStatusCard.tsx`, the top-bar status popover (find with `grep -rln "deviceSync" src/components/shell`), `RecoveryKitPanel.tsx` (the "Ztratili jste záchrannou sadu? Vyměnit klíč" link), locales ×10.

- [ ] The three notices with their buttons; the problems `keyNotGiven` (button "Vypnout synchronizaci", reusing `TurnOffSyncDialog`), `keyChangeUnverified`, the transient `keyRotated` text and the extended `unauthorized` text; the popover lines; reword every remaining "cannot be cut off" claim (`grep -rn "odříznout" src/i18n/locales/cs/`) except `turnOff.noCutOff`.
- [ ] Verify: frontend gates. Commit: `feat(sync-ui): key-change, removed and new-Kit notices on the desktop`.

### Task 21 [sonnet]: The phone device list, device sheet and removal sheet (spec §9.1, §9.2)

**Files:** `src/mobile/components/sync/SyncDeviceRows.tsx`, new `DeviceSheet.tsx`, `RemoveDeviceSheet.tsx`, `NewKitSheet.tsx` (reusing `KitBody.tsx`), `src/mobile/pages/settings/SyncSection.tsx`, locales `mobile.json`/`deviceSync.json` ×10.

> **Fix round (spec §9.2, As built 29):** the removal sheet's step 2 has the same Kit opt-in ticks and line as the desktop (same keys).

- [ ] Other devices' rows open the device sheet (label-value rows, "Odebrat zařízení" as a danger text button); the folded group as one row opening a sheet of the folded rows; the removal sheet runs the same four steps as the desktop dialog with the same keys where the text is the same; the password field uses `SyncPasswordSheet`'s field pattern (16 px field floor); the Kit sheet with the tick. Rows wrap (branch (f)).
- [ ] Verify: frontend gates. No `mobile.css` change is expected; if you do touch it, update `docs/design-system/mobile.html` and run its overflow check (README §13). Commit: `feat(mobile): remove a device or change the key on the phone`.

### Task 22 [sonnet]: Phone notices (spec §7.3, §7.4, §7.5, §8.1)

**Files:** `src/mobile/components/sync/SyncStatusBlock.tsx`, new `KeyChangeBlock.tsx`, `RemovedBlock.tsx`, `SyncSection.tsx`, locales ×10.

- [ ] The removed block in place of the setup entry when `removedNotice` is set (with "Připojit znovu" opening `JoinChoiceSheet`), the key-change block, the Kit-outdated row, the problem texts.
- [ ] Verify: frontend gates. Commit: `feat(mobile): key-change, removed and new-Kit notices on the phone`.

## Group C — Docs and verification

### Task 23 [sonnet]: ADR 0032 and the docs

**Files:** new `docs/architecture/decisions/0032-device-sync-removal-and-key-rotation.md`, `decisions/README.md`, ADR 0027 (status line: "Amended by ADR 0032"; its H1 residual points to 0032), ADR 0026 (amended: protocol 3), the spec's new "§15 As built" (differences found while building), `docs/architecture/overview.md` (the two new modules), `docs/standards/i18n-glossary.md` (spec §9.4 terms).

- [ ] The ADR records context, the decision (trust model, bundle, procedure, pickup, owner decisions O1–O12), consequences, accepted residuals (spec §12 "does not do") and the "Not in phase 5" list of spec §13 verbatim in substance.
- [ ] Verify: `npm run format:check`. Commit: `docs(sync): ADR 0032 for removing devices and changing the key`.

### Task 24 [opus]: Live checks and the final review (spec §14.3)

- [ ] The controller runs spec §14.3 steps 1–7 with two desktop dev instances (own identifiers, ports 1424 and 1425, demo profiles, the `backgroundThrottling` flag as the `moony-demo` launch config does) and the iPhone simulator (debug build with the flag via the gitignored `.cargo/config.toml`, deleted afterwards), against a local server with its own `DATA_DIR` (not the e2e one), never the owner's real profile, never ports 1420–1423. Screenshots of each language pass (en, cs, de, ja) and of the phone at 360 pt with the largest text size go to the scratchpad, not the repo.
- [ ] Final review [opus] of the whole branch against the Review Focus above; fix findings in their own commits.
- [ ] Report to the owner: what was checked live, what was not, the owner decisions O1–O12, and the "Not in phase 5" list. Do not merge.

## Spec coverage

| Spec | Tasks |
|---|---|
| §5.2 identities, §5.3 certificates and ticket | 1, 4, 5 |
| §5.4 trust store, §5.5 pre-phase-5 vaults | 3, 4, 18 |
| §6.1 epochs and the epoch rule | 8 |
| §6.2–§6.3 bundle | 6, 10 |
| §6.4 procedure, §6.6 key change alone | 9, 16, 19, 21 |
| §6.5 leaving | 11 |
| §7.1–§7.3 pickup and notice | 10, 20, 22 |
| §7.4 removed device | 10, 11, 20, 22 |
| §7.5 problems | 10, 16, 20, 22 |
| §7.6 blobs | 13 |
| §7.7 offline > 30 days | 10 (text 20, 22) |
| §8 Recovery Kit | 8, 12, 19, 21 |
| §9 interface | 17–22 |
| §10 concurrency | 9, 10, 14, 15 |
| §11.1 migration | 2 |
| §11.2 formats | 1, 5, 6, 8 |
| §11.3 relay: no change | 7, 15 |
| §14 verification | every task's tests, 14, 15, 24 |

## Decisions made by this plan

- The wire-contract task (16) is the only commit that touches Rust and `src/` together; `shared/` counts as the contract, not as frontend.
- Multi-device tests live in `engine/tests/` over the fake relay, not in the changeset-level `convergence/` harness (it has no envelopes or epochs).
- The relay client gets its own `RelayError::TooManyRotations` instead of folding `409 too_many_rotations` into an unknown code.
- No new engine event for a key change: the notice travels on `SyncStatus` like every other state.
