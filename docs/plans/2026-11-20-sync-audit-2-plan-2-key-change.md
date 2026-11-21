# Sync audit 2, plan 2: a key change binds what was below it Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a device is removed, nothing it seals later under an old key is applied by the remover or by any device that had pulled as far as the remover, neither as an op nor as a snapshot (findings R2-H1 and R2-H3, decision D1); a retired token is handled on every read (D4, app side); turning sync off runs the copy check (D10).

**Architecture:** The remover commits a key change provisionally, reads every position between its own cursor (the base) and the head the server reported, and signs the head together with a digest of what it found (signed head record version 2). Every device reads the same range, compares the digest, and from then on applies an older-key op there only if its hash is the noted one. A device also notes what it handled above its stored cursor, so a position cannot be refilled on a re-read. A snapshot sealed under an older key is restored only by a device catching up from below the base, or by a device that is joining.

**Tech Stack:** Rust, `rusqlite`, the in-crate `FakeRelay`; no new dependency. One migration (two Local tables, three columns).

**Spec:** `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`: §1.1, §1.3, **§2 in full (§2.3 is the contract; this plan does not restate it)**, §5.2, §6 (M10), §11. Read §2.3 and §2.4 before every task.

## Global Constraints

- App only. The server's side of protocol 3 is plan 1; this plan makes the app and its test server (`relay/fake.rs`) follow it.
- §2.3 is binding to the letter where it is exact: the definitions of place, base, range and older-key op; the record layout and the digest; "the remover signs what it found and never waits for a position to fill"; "verified in the same savepoint that queues the record"; "no new precondition for a removal besides the floor".
- Old development vaults are not upgraded: a version-1 signed head no longer confirms a key change.
- Local tables only: nothing new is synced, captured or part of a snapshot. Register them in `registry/tables.rs` as Local (the registry tests list every table).
- Migrations are append-only: the next number after the newest in `src-tauri/src/db/migrations.rs` (042 when this was written; check, another branch may have taken it), one `Vec` entry, a `migration_0NN` test module like 041's.
- No `.unwrap()` outside tests; `AppError`/`SyncError` as today; secrets stay in `Zeroizing`; no log line names a key, token, vault ID or device ID beyond what the surrounding code already logs.
- `cargo test` **by filter only** (`cargo test --lib rekey`, `cargo test --lib engine::tests::pull`, …). The full suite takes about 27 minutes: run it once, in Task 10, in the background.
- The convergence suite must stay green: `CONVERGENCE_SEEDS=1000..1100 cargo test --lib convergence` after Tasks 6 and 7.
- Every user-facing string in ten locales; Czech binding. Wire changes follow `docs/standards/type-contract.md`.
- One writer in the worktree at a time. Commit after every task with explicit paths. Never push.

## Review Focus

1. A removal on an honest server while another device is pushing: the removal completes inside the call, the pushing device re-seals and nothing is lost or applied twice.
2. A remover that was thousands of ops behind: it verifies over several rounds, and meanwhile every other device shows "waiting", not an error.
3. The app is quit between the rotation landing and the head being signed; on the next start the removal finishes by itself.
4. A device paired ten seconds after a removal, from the remover and from another device: it joins, or it is told to wait and try again; it never joins without knowing the head.
5. A profile re-synced by replace after a key change it had already passed: it reads the range again and does not stop on honest ops.

## File Structure

| File | Responsibility |
|---|---|
| `rekey/range.rs` (new) | The range of a key change: the digest, the notes table, reading a range from the server, the verdict for an op in a range |
| `rekey/head.rs` | The signed head record (version 2), unverified heads, the remover's own verification, the gate |
| `rekey/rotate.rs` | The pending change carries the base; the commit is provisional |
| `engine/seen.rs` (new) | Notes of what this device handled above its cursor (rule 9a), and "place" |
| `engine/pull.rs` | Rules 6, 9 and 9a in the pull |
| `engine/snapshots.rs`, `snapshot/mod.rs`, `join.rs` | Rule 10 |
| `engine/mod.rs` | What a round does not do while a head is unverified; `403` on any read; who owes a snapshot |
| `relay/fake.rs`, `server.rs` | Protocol 3 in the test server; the app's minimum |
| `setup.rs` | The leave path's copy check |

All paths are under `src-tauri/src/services/sync/`.

---

### Task 1: Protocol 3 in the app (D4)

**Files:** Modify `relay/fake.rs` (`Access`, `authorize`, `vault_info`, `rekeys`, the default `ServerInfo`), `server.rs` (`MIN_PROTOCOL`), `rekey/head.rs` (`verify_heads`' error), `engine/mod.rs` (`round_steps`), `blobs/transfer.rs` and `blobs/pass.rs` (classification), `join.rs` (the two join paths' refusal). Test: `relay/fake/tests.rs`, `engine/tests/rekey_pickup.rs`, `engine/tests/rekey_head.rs`, `engine/tests/blobs.rs`, `engine/tests/join.rs`.

**Interfaces:**
- `fake::Access` gains `Lookup`; a retired token is allowed `Lookup` only (`vault_info`, `rekeys`); `Read` (pull, snapshot, blob reads) answers `TokenReadOnly`. `set_accept_retired_writes` keeps meaning writes. The default `ServerInfo.protocol` is 3.
- `pub const MIN_PROTOCOL: u32 = 3;`
- In `engine::round_steps`, a `SyncError::ReadOnlyToken` from `head::verify_heads` is a `Trigger::ReadOnly` (a pickup), not the end of the round.
- A blob transfer or pass that meets `TokenReadOnly` asks the engine for a round (the existing wake-up the lanes use) in addition to stopping.
- A paired or Kit join that meets `TokenReadOnly` on its snapshot or first pull ends with the existing "the key changed while you were joining, try again" outcome of each path (pairing: `validation.syncPairingFailed` is wrong here; use what `KeyAhead` maps to in `join.rs`).

- [ ] **Step 1: Failing tests:** `a_retired_token_reads_only_the_vault_info_and_the_key_changes` (fake), `a_device_with_an_unverified_head_picks_up_a_second_key_change_from_its_look_ahead` (two key changes, no vault-info read in that round: the device ends on epoch 2, not `keyRotated`), `a_blob_download_with_a_retired_token_leads_to_a_pickup`, `a_join_overtaken_by_a_key_change_says_try_again`, `a_server_of_protocol_2_is_refused_at_setup`.
- [ ] **Step 2: Run by filter, see them fail. Step 3: Implement. Step 4: `cargo test --lib rekey` and `cargo test --lib engine::tests::blobs`, `…::join` pass** (several existing tests read with a retired token on purpose; each either moves to the new token first or asserts the `403`).
- [ ] **Step 5: Commit** (`feat(sync): protocol 3 in the app, a retired token only looks up`).

### Task 2: The record and the digest (pure)

**Files:** Create `rekey/range.rs`; modify `rekey/head.rs` (`HeadRecord`), `rekey/mod.rs` (`mod range`). Test: in both files.

**Interfaces:**
- `pub struct RangeNote { pub pos: u64, pub hash: [u8; 32] }`
- `pub fn range_digest(vault_id: &[u8; 16], to_epoch: u32, base: u64, head: u64, notes: &[RangeNote]) -> Result<[u8; 32]>`: spec §2.3 "The record and the digest"; refuses notes that are not strictly ascending or lie outside `(base, head]`.
- `HeadRecord` gains `pub base: u64, pub count: u32, pub digest: [u8; 32]`; `HEAD_DOMAIN = b"moony rekey head v2"`; `sign(vault_id, signer, from_epoch, to_epoch, rotation, head, base, count, digest, key)`. `decode` refuses a version-1 record (its domain string differs).

- [ ] **Step 1: Failing tests:** `the_range_digest_matches_the_frozen_vector` (vault `[0x22; 16]`, epoch 4, base 10, head 13, notes at 11 and 13 with hashes `[0xaa; 32]`, `[0xbb; 32]`; and the empty range), `a_digest_changes_with_every_field_and_with_the_order_of_notes_refused`, `a_signed_head_v2_round_trips_and_a_changed_byte_does_not_verify`, `a_version_1_record_does_not_decode`.
- [ ] **Step 2: fail. Step 3: implement, then freeze the vectors. Step 4: `cargo test --lib rekey::range rekey::head` pass. Step 5: Commit** (`feat(sync): signed head record version 2 and the range digest`).

### Task 3: Where the notes live

**Files:** Modify `src-tauri/src/db/migrations.rs` (next migration), `registry/tables.rs`, `registry/tests.rs`, `docs/architecture/database.md`; `rekey/range.rs`; create `engine/seen.rs`; `state.rs` (the epoch row).

**Interfaces:**
- Migration: `CREATE TABLE sync_range_notes (epoch INTEGER NOT NULL, pos INTEGER NOT NULL, hash BLOB NOT NULL, PRIMARY KEY (epoch, pos)); CREATE TABLE sync_seen_ops (pos INTEGER PRIMARY KEY, hash BLOB NOT NULL); ALTER TABLE sync_epochs ADD COLUMN range_base INTEGER; ALTER TABLE sync_epochs ADD COLUMN range_count INTEGER;` plus `ALTER TABLE sync_state ADD COLUMN restored_seq INTEGER` (the last restored snapshot's position, for "place"). All Local.
- `range.rs`: `pub fn store_notes(conn, epoch: u32, base: u64, notes: &[RangeNote]) -> Result<()>` (replaces that epoch's notes and sets `range_base`, `range_count`); `pub fn base_of(conn, epoch: u32) -> Result<Option<u64>>`; `pub fn noted(conn, epoch: u32, pos: u64) -> Result<Option<[u8; 32]>>`; `pub fn forget(conn, epoch: u32) -> Result<()>`.
- `seen.rs`: `pub fn note(conn, pos: u64, hash: &[u8; 32]) -> Result<()>`; `pub fn noted(conn, pos: u64) -> Result<Option<[u8; 32]>>`; `pub fn forget_through(conn, cursor: u64) -> Result<()>`; `pub fn forget_all(conn) -> Result<()>`; `pub fn place(conn) -> Result<Option<u64>>` = the highest of the stored cursor, `MAX(pos)` here and `sync_state.restored_seq`; `None` while sync is not set up.

- [ ] Steps: migration test (`migration_0NN`: the tables exist, are empty, the columns are NULL), store round-trips, `place` over the three sources; fail, implement, pass (`cargo test --lib migration_0 registry rekey::range engine::seen`), commit (`feat(sync): local notes for key-change ranges and for handled positions`).

### Task 4: The remover (§2.3 steps 1–5)

**Files:** Modify `rekey/rotate.rs` (`PendingChange` v3 with `base`, `remove`, `send`, `commit`, `resolve_pending`), `rekey/head.rs` (`note_unverified` gets the base; new `verify_own`; `queue_signed_head`), `rekey/range.rs` (`read_range`), `engine/mod.rs` (the round calls `verify_own` before anything else when an own head is unverified; no push, no re-seal, no snapshot upload while it is), `engine/tests/rekey_audit2.rs`, `rekey_rotate.rs`, `rekey_head.rs`, `rekey_scenarios.rs`.

**Interfaces:**
- `PendingChange.base: u64` (JSON `"v":3`, `"base"`; a stored `v:2` change is wiped with the existing "removal was not done" notice).
- `pub async fn read_range<R: Relay>(relay: &R, auth: &VaultAuth, base: u64, head: u64, max_pages: usize) -> SyncResult<RangeRead>` with `pub enum RangeRead { Complete(Vec<RangeNote>), More { scanned: u64, notes: Vec<RangeNote> } }`: reads with `PULL_PAGE`, notes `envelope_hash(&op.ct)` of every op with `base < seq <= head`, ends at a page with `more == false` or at the first `seq > head`; a page outside `check_page`'s contract is `RelayError::Protocol`; `410` is returned as it is.
- `pub async fn verify_own<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> SyncResult<OwnHead>` with `pub enum OwnHead { None, Signed, Waiting }`: for an unverified entry whose signer is this device, `read_range` (resuming from the entry's `scanned`), then in **one savepoint**: `range::store_notes`, `queue_signed_head` with base, count and digest, remove the unverified entry.
- `head::own_unverified(conn) -> Result<bool>`; `head::any_unverified(conn) -> Result<bool>`.
- `remove()` calls `verify_own` right after a commit, so on an honest server `Outcome::Changed` is returned with the head already signed.
- `remove()` refuses with `validation.syncRemovalNeedsSync` when `info.floor > cursor` (the vault info it already reads) and for nothing else new.

- [ ] **Step 1: Tests first.** Remove `#[ignore]` from `a_removed_devices_write_below_a_head_the_server_raised_is_applied_by_every_device`, rename it `…_is_applied_by_nobody`, keep its assertions on A and B, and replace the two lines that asserted the old behaviour (`signed_head == end + 8`, "B verified the head") by: A's epoch-1 notes are empty and its record names `base == end`, `count == 0`. Add: `an_honest_removal_signs_its_head_inside_the_call` (after `remove` returns: no unverified entry on A, the signed head is A's only unsent envelope, `sync_range_notes` holds exactly the ops between A's cursor and the head); `a_remover_that_was_behind_notes_every_op_between_its_cursor_and_the_head`; `while_its_own_head_is_unverified_the_remover_sends_nothing` (make `Route::Pull` fail: after `remove` and three rounds the server holds no new op, no snapshot upload was called, `rotate::plan` answers `syncChangeInProgress`); `a_crash_between_the_commit_and_the_signature_finishes_at_the_next_round`; `a_lost_answer_commits_with_the_stored_base`; `a_removal_is_refused_only_when_the_cursor_is_below_the_floor` (and is **not** refused when the vault's snapshot is above the cursor); `a_range_the_server_filled_before_the_remover_read_it_is_signed` (the accepted residual: `lift_next_rotation_head(2)`, C writes two ops before A's read; A signs `count == 2` and applies them).
- [ ] **Step 2: `cargo test --lib rekey_audit2 rekey_rotate` — the new ones fail. Step 3: Implement. Step 4: `cargo test --lib rekey` passes** (existing tests that read A's signed head right after `remove` keep passing because `remove` verifies inside the call; those that script a lying server need the lie moved to where it still applies).
- [ ] **Step 5: Commit** (`feat(sync): the remover signs the head with what it found below it`).

### Task 5: Every other device (§2.3 steps 6–8)

**Files:** Modify `rekey/head.rs` (`look_ahead`, `settle`, `after_restore`), `rekey/pickup.rs` (`adopt` passes nothing new; the entry gets its base when the record is found), `engine/pull.rs` (`epoch_fits_position` → the range verdict), `pairing/holder.rs`, `snapshot/mod.rs` (`dumped_keys` unchanged; `finish` after a replace), `join.rs`. Test: `engine/tests/rekey_head.rs`, `rekey_audit2.rs`, `rekey_pickup.rs`, `join.rs`, `kit_join.rs`.

**Interfaces:**
- `settle` (a signed record was found): the head number must match as today; then `read_range(base, head)` with this device's token, `range_digest` must equal the record's and the count too → `store_notes`, the entry goes; else `refuted`. A range that needs more pages keeps the entry with its `scanned`.
- `pub fn range_verdict(conn, epoch_of_op: u32, pos: u64, hash: &[u8; 32]) -> Result<RangeVerdict>` with `enum RangeVerdict { NotInARange, Matches, Wrong }`: for every key change `k > epoch_of_op` whose `range_base < pos <= head`, the note must exist and equal `hash`. `handle_op` stops with `PullStop::Invalid` on `Wrong`. (New-key ops at or below a head are stopped by the existing rule.)
- A device that holds a head for epoch `k` with `range_base IS NULL` and a place below that head treats the epoch as unverified (an entry with the head and no rotation) and finds the record by the look-ahead, starting at `max(place, vault floor)`.
- After a replace (`snapshot::finish` with `replacing`): for every epoch whose head is above the snapshot's position, `range::forget` and a fresh unverified entry; `seen::forget_all`.

- [ ] **Step 1: Tests:** `a_device_checks_the_range_before_it_takes_the_head` (honest: B's notes equal A's); `other_contents_for_another_device_refute_the_head` (the fake serves B a different op at one position of the range than it served A: B is `keyChangeUnverified`, applies nothing of the range); `an_op_replaced_in_the_range_after_verification_stops_the_pull` (on A and on B); `an_op_at_an_unnoted_position_of_the_range_stops_the_pull`; `a_device_already_past_the_head_checks_nothing_and_goes_on`; `a_device_between_base_and_head_reads_from_the_base_again`; `a_device_paired_right_after_a_removal_finds_the_record_first`; `a_kit_joiner_finds_the_record_first`; `a_resync_after_a_key_change_reads_the_range_again_and_applies_it`; `a_pruned_range_is_settled_by_a_new_key_snapshot`. The fake needs one more switch for the second and third: `serve_instead(vault_id, seq, ct, to_token_only: Option<&str>)`.
- [ ] **Step 2: fail. Step 3: implement. Step 4: `cargo test --lib rekey kit_join engine::tests::join` pass. Step 5: Commit** (`feat(sync): every device checks a key change's range against the signed digest`).

### Task 6: Rules 9 and 9a in the pull

**Files:** Modify `engine/pull.rs` (`pull_pages_from_cursor`, `handle_op`), `engine/seen.rs`, `snapshot/mod.rs`, `chain/rejoin.rs` or `engine/pull.rs::rejoin_as_new_device`, `state.rs` (`note_vault_went_back`, `set_cursor`). Test: `engine/tests/pull.rs`, `gaps.rs`, `rekey_audit2.rs`, `went_back.rs`.

**Interfaces:**
- Rule 9a: in the lock that handles an op (`with_conn_mut` around `handle_op`), before the verdict is acted on: if `seen::noted(pos)` exists and differs from the op's envelope hash → `OpStep::Stop(PullStop::Invalid)`; after an op was handled (any step but `Stop`) and the stored cursor did not move past it, `seen::note(pos, hash)` in the same lock. `state::set_cursor` calls `seen::forget_through`. `seen::forget_all` on a replace, on a re-join under a new ID and in `note_vault_went_back`.
- Rule 9: while an epoch this device holds has a head above the stored cursor, an op at `seq <=` that head and (`seq <= range_base` or the base is unknown) must have `seq == previous + 1`, where `previous` starts at the pull's `after` (the stored cursor); otherwise the page is outside the contract (`PullStop::Invalid`). Not applied while `scanning`.

- [ ] **Step 1: Tests first, the first one as the finding it is** (spec §2.6: read from the code by the third review, never run): `a_position_already_handled_cannot_be_refilled_behind_a_held_cursor` — B holds a gap (the fake drops one op of an author), applies later ops, A removes C on an honest server, C seals its next op under the old key and the fake serves it to B at a position B already handled: B must not apply it. Run it **before** implementing and record in the task report whether it fails as predicted; if it passes on unchanged code, stop and report (the finding would not hold and rule 9a would need rethinking). Then: `the_same_op_read_again_is_passed_over_as_today`, `notes_go_as_the_cursor_passes_them`, `a_crash_before_the_note_is_made_up_at_the_next_read`, `a_server_that_went_back_clears_the_notes_and_keeps_its_own_message`, `a_skipped_position_below_the_base_stops_the_pull`, `a_skipped_position_with_no_key_change_pending_does_not`, `the_scan_after_a_newer_schema_stop_is_exempt`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib engine::tests::pull engine::tests::gaps engine::tests::went_back rekey` and `CONVERGENCE_SEEDS=1000..1100 cargo test --lib convergence` pass. Step 5: Commit** (`feat(sync): a handled position cannot be refilled; no skipped position below a key change`).

### Task 7: Snapshots (rule 10, R2-H3)

**Files:** Modify `engine/snapshots.rs` (`open_owned` returns the seal epoch; `restore_after_410`, `heal_missed`, `upload_if_due`/the policy), `snapshot/mod.rs` (`restore_merge`, `replace` take the seal epoch and position), `join.rs` (`fetched_snapshot_at` already has both), `state.rs` (`restored_seq`). Test: `engine/tests/rekey_audit2.rs`, `snapshots.rs`, `kit_join.rs`, `join.rs`.

**Interfaces:**
- `pub fn snapshot_admissible(conn, sealed_epoch: u32, seq: u64, joining: bool) -> Result<Admissible>` with `enum Admissible { Yes, Wait, No }`: the newest key held → `Yes`. An older key: `joining` → `Yes` when `seq <=` the head of the key change that ended that key (today's rule), else `No`; not joining → `Yes` only when `seq > place` and `seq <= range_base` of that key change; `Wait` when the base is not known yet or when the snapshot is honest-shaped but above the base (`seq <= head`); `No` otherwise. `No` → `syncSnapshotInvalid`; `Wait` → the round reports the existing waiting problem for a restore that cannot run yet and tries again next round.
- A successful restore sets `sync_state.restored_seq`.
- Every device: when a head it holds is verified and the vault info's `snapshot_seq` is not above that head, a snapshot is owed (the existing `snapshot_epoch_due` mechanism, now set by `settle` and `verify_own` alike, cleared when `snapshot_seq > head`).

- [ ] **Step 1: Tests:** remove `#[ignore]` from `a_removed_devices_snapshot_above_the_head_is_merged_by_an_up_to_date_device` (rename `…_is_refused_…`, assert `!report.restored` too). Add: `an_old_key_snapshot_below_the_base_catches_a_lagging_device_up`, `an_old_key_snapshot_between_base_and_head_waits_for_a_new_one`, `an_old_key_snapshot_offered_to_a_device_past_it_is_refused` (the lifted-head case: base == end, head == end + 8, a forged snapshot at end + 3), `a_heal_with_a_new_key_snapshot_below_the_place_still_works`, `a_heal_with_an_old_key_snapshot_waits`, `a_kit_join_takes_an_old_key_snapshot_up_to_the_head`, `a_paired_join_does_too`, `every_verified_device_owes_the_new_key_snapshot` (the remover never syncs again: B uploads it), `a_vault_that_never_changed_its_key_restores_as_before`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib rekey_audit2 engine::tests::snapshots kit_join engine::tests::join` and the convergence filter pass. Step 5: Commit** (`fix(sync): a snapshot must fit the key of its position`).

### Task 8: What a device with an unverified head does not do, and what it shows

**Files:** Modify `pairing/holder.rs` and `commands/sync_pairing.rs` (the holder side), `engine/snapshots.rs` (no upload), `rekey/rotate.rs` (`check`), `view.rs` and `models/sync.rs` (status), `shared/schema.ts`, `shared/generated-types.ts`, the status components that render `keyChangeWaiting` (`src/components/settings/sync/`, `src/mobile/`), `src/i18n/locales/*/deviceSync.json` and `common.json`.

**Interfaces:**
- While `head::any_unverified`: pairing as the holder, `rotate::plan`/`remove` and the snapshot upload answer or skip with the existing `validation.syncChangeInProgress` (commands) / do nothing (the round).
- `SyncStatus` gains `ownChangeFinishing: boolean` (`own_change_finishing`): the unverified head is this device's own.
- Texts: the status for `keyChangeWaiting` with `ownChangeFinishing` — cs `Odebrání se dokončuje. Synchronizační server zatím neposlal všechny změny z doby před odebráním. Moony to zkouší dál; data na tomto zařízení jsou v pořádku.` / en `The removal is finishing. The sync server has not yet sent every change from before the removal. Moony keeps trying; the data on this device are fine.` `validation.syncRemovalNeedsSync` — cs `Než zařízení odeberete, musí se dokončit synchronizace. Zkuste to za chvíli znovu.` / en `Sync has to finish before a device can be removed. Try again in a moment.`

- [ ] Steps: tests (`a_device_with_an_unverified_head_pairs_nobody`, `…uploads_no_snapshot`, `…starts_no_removal`, the status mapping, `locales.test.ts`), fail, implement (schema in seven steps), `cargo test --lib sync_pairing view rekey` and `npm test`, `npm run typecheck` pass, commit (`feat(sync): a device hands no keys on while a head is unverified, and says what it waits for`).

### Task 9: Turning sync off runs the copy check (D10, first audit M10)

**Files:** Modify `setup.rs` (`leave_best_effort`), Test: `engine/tests/binding_failure.rs`, `rekey_leave.rs`.

**Interfaces:** `leave_best_effort` signs and pushes the `left` record only when the binding is checked and says "this device" (the check the engine start runs, without counting a failed start); otherwise it sends nothing and logs one line. Turning sync off locally is unaffected.

- [ ] Steps: tests `a_copy_whose_binding_cannot_be_checked_does_not_say_left` and `…says_not_this_device_does_not_say_left` (the server holds no `left` record, the original device keeps syncing), the existing `a_device_that_leaves_signs_and_sends_its_left_record` still passes; fail, implement, pass, commit (`fix(sync): a profile copy cannot announce that the original device left`).

### Task 10: Break it, then run everything

- [ ] **Step 1: An adversarial reading of the code**, by a reviewer who did not build it, with the spec's §2.3, §2.4 and §11 and this list to try against the built code: a lifted head (unfilled, filled before the read, filled after it); other contents for another device; an op replaced on a re-read (in the range; behind a held cursor); an old-key snapshot (above the head; between base and head; below the place; at a heal; at a join); the device being removed pushing and uploading snapshots during the removal; a second removal right after the first; a pairing right after; a re-sync right after; the remover quitting at every await point of `remove` and `verify_own`. Each attempt that gets through becomes a failing test and a fix, each that does not becomes one line in the report.
- [ ] **Step 2: The full suites**, in the background: `cargo test --lib` (about 27 minutes), `CONVERGENCE_SEEDS=1000..1600 cargo test --lib convergence`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm test`, `npm run typecheck`, `npm run lint`.
- [ ] **Step 3: Update the module docs** of `rekey/head.rs`, `rekey/rotate.rs`, `rekey/pickup.rs`, `engine/pull.rs` and `rekey/reseal.rs` (`may_reseal`'s reasoning names the range now) so they describe the code as built. Commit (`docs(sync): the key-change modules as built after the second audit`).

## Not in this plan

- Closing R4 (the signed head attesting every author's chain, or per-op signatures) and lifting a removed device's quarantine: spec §9; each needs its own design.
- The ADR (plan 4) and the owner's checks on real devices and the home server.
