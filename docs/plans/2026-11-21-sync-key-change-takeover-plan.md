# Finishing an unfinished key change (takeover) Implementation Plan

> **Parked on 2026-11-21.** The owner stopped this plan after five of nine tasks (branch `feat/sync-takeover`); nothing of it is on this branch. See the status line of the spec.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A key change whose maker never signs its head no longer freezes the vault: every device takes a first look at the stretch of a key change, and after 24 hours a person can let another device take the change over with a new, signed key change.

**Architecture:** The first look stores envelope hashes of the stretch next to rule 9a's notes and the pull refuses anything else there. A takeover is an ordinary key change whose signed head record (version 3) names the epochs it takes over; a device that verifies it drops those unverified heads and marks the epochs, and rules 6, 9, 10 and the after-restore step read the mark. No server, pairing or snapshot format change.

**Tech Stack:** Rust (`src-tauri/src/services/sync/rekey/`, `engine/`, `view.rs`, `state.rs`, `db/migrations.rs`, `commands/sync_devices.rs`, `models/sync.rs`), TypeScript/React (`src/components/settings/sync/`, `src/components/shell/`, `src/mobile/components/sync/`, `src/utils/`), i18n JSON in ten locales.

**Spec:** `docs/specs/2026-11-21-sync-key-change-takeover-design.md` (approved 2026-11-21, T1–T5). It builds on `docs/specs/2026-11-20-sync-audit-2-fixes-design.md` §2.3 and ADR 0034.

## Global Constraints

- Sync stays behind `feature::RELEASED = false`. Never push; never `git stash`.
- `cargo test` by filter only, from `src-tauri/` (the full suite takes about 27 minutes and is run once, in Task 9). `cargo fmt`, `cargo clippy --lib --tests -- -D warnings` before each commit. No `.unwrap()` outside tests; no key, token or device ID in a log line beyond what the surrounding code logs.
- Migrations are append-only (`src-tauri/src/db/migrations.rs`: `043_…`, one `Vec` entry; `docs/playbooks/add-db-migration.md`).
- The wire contract is `shared/schema.ts`: the seven steps of `docs/standards/type-contract.md` for every change (per-field `#[serde(rename)]`, `bindings.rs`, `cargo test generate_bindings -- --ignored` leaves no diff).
- Every user-facing string in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`); the Czech and English texts of the spec's §3 and §4 are used verbatim; the other eight are translated from the English with each file's own terms. The glossary forbids "klíč synchronizace" and "výměna klíče".
- Exact values from the spec: the waiting time is the existing `view::WAITING_AFTER_SECS` (86 400 s), counted from `sync_epochs.adopted_at`; record version 3 is the domain `"moony rekey head v3"`, the version-2 fields in order, `over (u32, big-endian)`, the signature (183 bytes); a verifier requires `1 <= over <= from_epoch`; validation keys `validation.syncTakeoverNotYet`, `validation.syncTakeoverNotHere`, `validation.syncTakeoverServerChanged`.
- A takeover is never automatic; it is the one key change allowed while a head is unverified; every other gate of an unverified head stays.
- A refuted head is never dropped, marked or taken over. The remover itself (an own unsigned head) cannot take over.
- No existing security assertion is weakened. Each security test is written first and seen failing for its reason.

## Review Focus

1. The person presses "Finish from this device" and the device that did not finish comes online a minute later with changes it never sent: it must pick the takeover up, sign nothing for its own change, and its unsent changes must reach the others (re-sealed under the new key), not be lost.
2. A phone whose pull has been stopped inside the stretch for days: after the takeover it applies the stretch and everything behind it in one go, with no `Invalid` on an honest server.
3. The takeover dialog for a change whose maker has no name on this device: the texts without a name, no checkbox, still usable.
4. "Finish" pressed on two devices within the same minute: one wins, the other ends with the existing "check the list" refusal and no half-made change.
5. The app is closed or the network drops at any point between the tap and the signed record: the next start finishes or cleanly undoes, and no second takeover is offered meanwhile.

---

### Task 1: Signed head record version 3

**Files:** Modify `src-tauri/src/services/sync/rekey/head.rs` (`HeadRecord`, `sign`, `encode`, `decode`, `verify`, `signed_bytes`). Test: the unit tests in the same file.

**Interfaces:**
- Produces: `HeadRecord.over: Option<u32>` (`None` is a version-2 record). `HeadRecord::sign_takeover(vault_id, signer, from_epoch, to_epoch, rotation, head, base, count, digest, over, key) -> HeadRecord`. `encode`/`decode` choose the version by `over` / by the domain string and length (179 or 183 bytes). `decode` refuses a version-3 record with `over == 0` or `over > from_epoch`. `HeadRecord::takes_over(&self) -> Option<std::ops::Range<u32>>` (`over..to_epoch`).

- [ ] **Step 1: Failing tests:** `the_takeover_record_is_frozen_byte_for_byte` (seed `[0x77; 32]`, the fields of the existing version-2 frozen test plus `over = 3`; freeze the 183 signed bytes as hex, computed once and checked by hand against the layout), `a_version_2_record_still_decodes_and_verifies`, `a_takeover_record_round_trips`, `over_out_of_bounds_is_refused` (0, and `from_epoch + 1`), `a_flipped_byte_anywhere_in_a_takeover_record_fails` (every offset including the four of `over`), `a_version_2_record_cannot_be_read_as_version_3` and the reverse (domain and length).
- [ ] **Step 2: Run `cargo test --lib rekey::head::tests` — they fail. Step 3: Implement. Step 4: pass, with the existing version-2 frozen test unchanged. Step 5: Commit** (`feat(sync): signed head record version 3 names the key changes it takes over`).

### Task 2: Storage for the look and for the mark

**Files:** Modify `src-tauri/src/db/migrations.rs` (043), `engine/seen.rs`, `state.rs`, `rekey/head.rs` (`UnverifiedEpoch`), the clears in `state::disable`, `join.rs`, `snapshot/mod.rs`. Test: `migrations.rs`, `engine/seen.rs`, `state.rs` unit tests; `src-tauri/src/services/backup.rs::export_skips_every_sync_table` (the count stays 16: no new table).

**Interfaces:**
- Migration `043_sync_takeover`: `ALTER TABLE sync_seen_ops ADD COLUMN looked INTEGER NOT NULL DEFAULT 0;` `ALTER TABLE sync_epochs ADD COLUMN taken_over_by INTEGER;` `ALTER TABLE sync_state ADD COLUMN looked_through INTEGER;`
- `seen::note_looked(conn, pos, hash)` (a row with `looked = 1`; an existing handled row keeps `looked = 0` and its hash), `seen::looked(conn, pos) -> Result<Option<[u8; 32]>>` (any row at `pos`), `seen::looked_through(conn) -> Result<Option<u64>>`, `seen::raise_looked_through(conn, end)`. `seen::place` ignores rows with `looked = 1`; `watermark` is unchanged. `seen::forget_through(cursor)` also clears `looked_through` when the cursor reached it. A handled op noted by `seen::note` at a looked position sets `looked = 0` (it is handled now).
- `UnverifiedEpoch.look: Look` with `#[serde(rename = "look", default)]`, `enum Look { Pending, Done, None }` (default `Pending`).
- `state::mark_taken_over(conn, epoch, by: u32)`, `state::taken_over_by(conn, epoch) -> Result<Option<u32>>`.
- Lifecycle: the look's rows and `looked_through` go where rule 9a's notes go (cursor passing, replace, sync off) and stay on a re-join under a new ID and on a server that went back; `taken_over_by` goes with its `sync_epochs` row.

- [ ] **Step 1: Failing tests:** the migration on a fresh and on a version-042 database; `the_looks_notes_do_not_move_the_place_or_the_watermark`; `a_handled_op_at_a_looked_position_becomes_handled`; `looked_through_goes_when_the_cursor_reaches_it`; `the_looks_notes_survive_a_rejoin_and_a_server_that_went_back_and_go_on_a_replace_and_sync_off`; `an_entry_stored_before_this_build_reads_with_a_pending_look`; `the_mark_goes_with_its_epoch_row`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib -- migration_043 engine::seen services::sync::state backup::` pass. Step 5: Commit** (`feat(sync): storage for the first look and for taken-over key changes`).

### Task 3: The first look

**Files:** Modify `rekey/head.rs` (new `look`), `engine/mod.rs` (`round_steps`: after `pickup::pick_up`, before `head::verify_heads`), `engine/pull.rs` (`pull_pages_from_cursor`, next to rule 9a), `rekey/range.rs` (reuse `read_range`). Test: new `engine/tests/rekey_look.rs`.

**Interfaces:**
- Consumes: Task 2's storage; `range::read_range(relay, auth, base, head, max_pages) -> SyncResult<RangeRead>`.
- Produces: `pub async fn look<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials) -> SyncResult<()>`: for every unverified entry with `look == Pending` and a known head, reads `(entry.after, head]`; `Complete(notes)` → in one savepoint `note_looked` for each, `raise_looked_through(head)`, `look = Done`; `More` (longer than one read) or `SnapshotRequired` → `look = None`; any other relay error → stays `Pending` (the round goes on). An entry with `head == None` gets `look = None`. An empty stretch (`after >= head`) is `Done` with no read.
- The pull, in the same closure as rule 9a and before `handle_op`: when `op.seq <= looked_through` (and above the stored cursor), the op's envelope hash must equal `seen::looked(op.seq)`; no row or another hash → `OpStep::Stop(PullStop::Invalid)`. Not applied in the newer-schema scan.

- [ ] **Step 1: Failing tests** (fixtures of `rekey_audit2.rs`; the fake's `serve_instead`, `lift_next_rotation_head`, `set_accept_retired_writes`, `set_failing`): `a_device_looks_at_its_stretch_when_it_takes_a_key` (notes equal the stretch, of any key; `look == Done`; place and watermark unmoved); `an_op_added_to_a_looked_stretch_stops_the_pull` (a lifted head, a removed device's write placed after the look: `Invalid`, not applied — with the remover's record withheld, so no range note exists); `an_op_replaced_in_a_looked_stretch_stops_the_pull`; `what_the_look_saw_is_applied_once_the_head_is_verified` (the honest path is unchanged); `a_stretch_longer_than_one_read_gets_no_look_and_is_not_asked_again` (count the pulls of two rounds); `a_pruned_stretch_gets_no_look`; `a_network_failure_leaves_the_look_pending_and_the_next_round_takes_it`; `the_look_runs_before_the_heads_are_verified_and_before_the_pull` (a record found in the same round still meets a done look); `a_kit_joiner_without_a_head_has_no_look`; `an_entry_made_after_a_replace_gets_its_look`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib -- rekey_look rekey engine::tests::pull engine::tests::gaps engine::tests::scan kit_join` pass (every existing test unchanged). Step 5: Commit** (`feat(sync): every device takes a first look at the stretch of a key change`).

### Task 4: The mark in the rules

**Files:** Modify `rekey/range.rs` (`range_verdict`, `must_follow_directly`), `engine/snapshots.rs` (`snapshot_admissible`), `rekey/head.rs` (`require_ranges`, `look_ahead`/`settle` for a record of a marked epoch). Test: unit tests in `range.rs`; `engine/tests/rekey_look.rs` or a new `rekey_takeover.rs` (state built by hand with `state::record_epoch`, `range::store_notes`, `state::mark_taken_over`).

**Interfaces:**
- Consumes: `state::taken_over_by`.
- `range_verdict`: an epoch that is marked is not asked for notes of its own (the epoch that took it over is judged like any other stored range).
- `must_follow_directly(pos)`: for a marked epoch the base is the base of the epoch that took it over (unknown while that one has none).
- `snapshot_admissible`: for the key a marked epoch ended, the base is `min(base of the taking epoch, the marked epoch's head)`; unknown while the taking epoch has none.
- `require_ranges`: no entry for a marked epoch.
- A signed record for a marked epoch is ignored (no entry exists; make sure no path re-creates one or refutes on it).

- [ ] **Step 1: Failing tests:** `an_old_key_op_in_a_taken_over_stretch_is_judged_by_the_takers_range` (in the range: must match the taker's note; below the taker's base: no range note asked); `no_skipped_position_below_the_takers_base_for_a_marked_epoch`; `an_old_key_snapshot_below_the_takers_base_is_restored_by_a_device_catching_up` and `…above_it_waits`; `a_merge_restore_makes_no_entry_for_a_marked_epoch` and `…a_replace_neither` (it makes one for the taking epoch when that has no stored range and its head is above the watermark); `a_late_record_of_the_original_remover_for_a_marked_epoch_changes_nothing`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib -- rekey::range rekey_takeover rekey_audit2 engine::tests::snapshots` pass. Step 5: Commit** (`feat(sync): a taken-over key change is judged by the range of the change that took it over`).

### Task 5: Verifying a takeover

**Files:** Modify `rekey/head.rs` (`settle`, `commit`, `verify_own`, `after_restore`). Test: `engine/tests/rekey_takeover.rs`. The takeover records in these tests are made by a test helper that signs a version-3 record for a real second key change (the taker's side is Task 6).

**Interfaces:**
- Consumes: Task 1's `takes_over`, Task 2's `mark_taken_over`, Task 4's rules.
- `commit` (the savepoint that verifies a head): when the verified record is version 3, every unverified entry of this device for an epoch in `over..to_epoch` that is **not refuted** is removed, `own_pending` ones included, and each such epoch is marked `taken_over_by = to_epoch`. An epoch of that span this device holds as verified, or not at all, is left alone. A version-2 record drops nothing but its own entry (as today).
- `verify_own`: an own unsigned entry is not signed while this device holds an unverified entry for a later epoch.
- The record is verified exactly as a version-2 one first (numbers, the range read from the signed base, count, digest; the watermark shortcut).

- [ ] **Step 1: Failing tests:** `a_device_that_verifies_a_takeover_drops_the_head_it_waited_for` (entry gone, mark set, `any_unverified` false, pairing and snapshot upload work again); `an_ordinary_second_key_change_does_not_drop_an_earlier_unverified_head` (a joiner below two heads waits for both records); `a_refuted_head_survives_a_takeover` (still `keyChangeUnverified`); `the_device_that_did_not_finish_drops_its_own_head_when_it_verifies_the_takeover_and_signs_nothing` (nothing with a signed head in `sync_unsent`); `it_signs_nothing_for_its_own_change_once_it_holds_the_takeover_unverified`; `it_signed_before_it_saw_the_takeover` (devices that dropped the head ignore the late record; a device that had not verified the takeover verifies both); `a_device_shown_something_else_at_its_first_look_than_the_taker_signed_stops` (`Invalid`, nothing of the stretch applied); `a_device_behind_the_takers_cursor_applies_below_it_only_what_it_first_saw`; `a_takeover_of_a_takeover_drops_every_named_head`; `a_joiner_without_the_mark_sets_it_when_it_verifies_the_takeover`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib -- rekey_takeover rekey rekey_gate kit_join engine::tests::join` pass. Step 5: Commit** (`feat(sync): a verified takeover drops the unverified heads it names`).

### Task 6: The taker

**Files:** Modify `rekey/rotate.rs` (`check`, `plan`, `Removal`, `remove`, `prepare`, `PendingChange`, `commit`), `rekey/head.rs` (`verify_own`, `queue_signed_head`), `engine/status.rs` / the round's problem for the post-rotation mismatch. Test: `engine/tests/rekey_takeover.rs`, `rekey_rotate.rs`.

**Interfaces:**
- `pub const TAKEOVER_NOT_YET: &str = "validation.syncTakeoverNotYet"`, `TAKEOVER_NOT_HERE = "validation.syncTakeoverNotHere"`, `TAKEOVER_SERVER_CHANGED = "validation.syncTakeoverServerChanged"`.
- `pub fn takeover_state(conn, now) -> Result<Takeover>` with `enum Takeover { Nothing, NotYet, NotHere, Ready { over: u32 } }`: `Nothing` when no unverified head, or one is refuted, or one is this device's own unsigned; `NotYet` when any has waited less than `WAITING_AFTER_SECS` since its `adopted_at`; `NotHere` when any has `look != Done`; else `Ready` with the lowest unverified epoch. (The "too far behind" case is found in `remove`, from the vault's info, and answers `TAKEOVER_NOT_HERE`.)
- `plan(conn, removing, also_remove, new_kit, takeover: bool, now)` and `Removal { removing, new_kit, takeover, consent }`: with `takeover` the plan may remove nobody; `check` lets it through only when `takeover_state` is `Ready` (else the matching refusal; without `takeover` the existing `IN_PROGRESS` stays). The consent hash covers `takeover` and `over`.
- `remove` with `takeover`: before the rotation is sent, a read of every looked stretch (`read_range` from each entry's `after` to its head) must equal the look (same positions, same hashes, nothing missing) → else `TAKEOVER_SERVER_CHANGED`, nothing sent, no pending change left.
- `PendingChange` carries `over: Option<u32>` (a new stored version; a stored older version is read as before).
- `verify_own` for a takeover: after the range read, the notes at positions inside looked stretches must equal the look, else `OwnHead::Waiting` and the round's problem is `TAKEOVER_SERVER_CHANGED`; on success one savepoint: `store_notes`, the version-3 record queued (`sign_takeover`), the own entry removed, every entry in `over..to` removed and marked, the snapshot owed.

- [ ] **Step 1: Failing tests:** `a_remover_that_never_signs_is_taken_over_after_a_day` (end to end with three devices: before 24 h `syncTakeoverNotYet`; after it the takeover; every device verifies; a device whose pull was stopped in the stretch applies it; pairing, snapshot upload and a later removal work); `the_takeover_removes_the_device_that_did_not_finish` and `…one_that_does_not`; `a_write_placed_in_the_stretch_after_the_takers_look_is_refused_before_the_rotation` (no rotation call on the fake; no pending change); `…placed_after_the_rotation_is_not_signed` (problem `syncTakeoverServerChanged`, nothing queued, the round holds still); `an_op_the_look_saw_that_is_missing_later_is_a_difference`; `a_write_placed_before_every_first_look_is_signed` (the stated residual; the test's doc says so); `takeover_is_refused` cases — with a refuted head, from the remover, with a look pending or given up, more than `REMOVAL_MAX_BEHIND` behind; `two_takeovers_at_once` (the loser meets the existing lost race and commits nothing); `a_takeover_of_a_takeover_waits_its_own_day`; `a_quit_at_every_await_point_of_a_takeover` (after the pre-flight, after the rotation answer, between commit and signing: the next start finishes; no second takeover is offered meanwhile); `the_consent_covers_the_takeover`.
- [ ] **Step 2–4: fail, implement, `cargo test --lib -- rekey_takeover rekey_rotate rekey rekey_gate` pass. Step 5: Commit** (`feat(sync): another device can take over a key change its maker never finished`).

### Task 7: The wire, the notice and the commands

**Files:** Modify `view.rs` (the waiting notice), `models/sync.rs`, `commands/sync_devices.rs` (`sync_removal_plan`, `sync_remove_devices`), `bindings.rs` if a type is added, `shared/schema.ts`, `shared/generated-types.ts`. Test: `view.rs` unit tests, `commands` tests, `src/utils/sync-state.test.ts`.

**Interfaces:**
- `SyncWaitingForRemover` becomes `{ by: string | null, since: number, canFinishHere: boolean, removable: string | null }`: the maker of the **newest** unverified head (`by` is `null` when it has no name here), `since` its `adopted_at`, `canFinishHere` = `takeover_state` is `Ready`, `removable` = the maker's device ID (hex) when it has a trusted row this device could remove, else `null`. Shown after `WAITING_AFTER_SECS` as today; never for an own unsigned head; not shown on a device whose own takeover is unsigned.
- `SyncStatus.ownChangeFinishingLong: boolean`: the own unsigned head is older than `WAITING_AFTER_SECS`.
- `sync_removal_plan(.., takeover: bool, ..)` and `sync_remove_devices(.., takeover: bool, ..)` pass the flag through; `RemovalPlan` gains `takeover: boolean`.

- [ ] **Step 1: Failing tests:** the notice names the newest head's maker; `canFinishHere` false before 24 h, with a look not done, on the remover, on a taker whose takeover is unsigned; `by` and `removable` null for a head with no known maker; `ownChangeFinishingLong`; the command mapping.
- [ ] **Step 2–4: fail, implement (schema in seven steps), `cargo test --lib -- services::sync::view sync_devices`, `cargo test generate_bindings -- --ignored` (no diff after), `npm run typecheck` pass. Step 5: Commit** (`feat(sync): the waiting notice says whether this device can finish the change`).

### Task 8: The dialog and the texts

**Files:** Modify the three renderers of the waiting notice (`src/components/settings/sync/SyncProblemAlert.tsx`, `src/components/shell/SyncStatusSection.tsx`, `src/mobile/components/sync/SyncStatusBlock.tsx`), `src/utils/sync-state.ts`, `src/utils/device-sync-problem.ts`; Create `src/components/settings/sync/FinishChangeDialog.tsx` and `src/mobile/components/sync/FinishChangeSheet.tsx` (built from the pieces of `RemoveDeviceDialog` / `RemoveDeviceSheet`: password, consent, `AlsoRemoveList`); Modify `src/i18n/locales/*/deviceSync.json` and `common.json`. Test: `src/utils/*.test.ts`, `src/i18n/locales.test.ts`, a server-render test of the dialog body like `AddedLine.test.tsx`.

**Interfaces:**
- Consumes: Task 7's fields; the existing `sync_removal_plan` / `sync_remove_devices` calls with `takeover: true`; `defaultAlsoRemove`.
- Texts, keys in `deviceSync.json` next to the waiting notice's: `waiting.noName`, `waiting.finishHere`, `finish.title`, `finish.body1`, `finish.body1NoName`, `finish.body2`, `finish.body2NoName`, `finish.removeIt`, `finish.confirm`, `status.problem.keyChangeWaitingOwnLong` (the spec's §4 sentence, appended to the "finishing" text when `ownChangeFinishingLong`), `waiting.setUpAgain` (the spec's §4 last-resort sentence, shown under the notice when the head has waited its 24 hours and `canFinishHere` is false); in `common.json` the three `validation.syncTakeover…` keys. Czech and English verbatim from the spec's §3 and §4.
- The button shows only when `canFinishHere`; the checkbox only when `removable` is not null, unticked; ticking it loads the plan with that device in `deviceIds` and shows the "also remove" list with its defaults; the confirm is disabled while the plan loads; the dates use the app's short date formatter.

- [ ] **Step 1: Failing tests:** the text key chosen for each state (name / no name; own finishing long; the last-resort sentence only where this device cannot finish); the dialog body with and without a name; the checkbox absent without `removable`; `locales.test.ts` (ten locales, placeholders `{{name}}`, `{{date}}`).
- [ ] **Step 2–4: fail, implement, `npm test`, `npm run typecheck`, `npm run lint` pass. Step 5: Look at it** as the repository's run instructions say (`MOONY_DEV_SYNC=1`); a waiting notice needs a second device whose removal never finished, so say in the report what was and was not seen rather than claiming it. **Step 6: Commit** (`feat(sync): finish an unfinished change from another device`).

### Task 9: Break it, run everything, write it down

- [ ] **Step 1: An adversarial reading** by a reviewer who did not build it, with the spec's §2.2–§2.5 and this list to try against the built code: a write added or replaced in the stretch before, between and after each device's look; a server that shows two devices different stretches; a lifted head; the look made to fail or to be given up on purpose (what does that device accept then); a takeover forced by withholding the remover's record, and the remover's record released afterwards; a version-3 record replayed for another epoch or with a widened `over`; a device removed by the takeover that signs a record; a re-join under a new ID at each stage; a quit at every await point; an honest vault that can no longer take over. Each attempt that gets through becomes a failing test and goes to the controller for a ruling; each that does not becomes one line in the report.
- [ ] **Step 2: The full suites**, in the background: `cargo test --lib`, `cargo clippy --workspace --all-targets -- -D warnings`, `npm test`, `npm run typecheck`, `npm run lint`.
- [ ] **Step 3: Docs:** the module docs of `rekey/head.rs`, `rekey/rotate.rs`, `rekey/range.rs`, `engine/pull.rs`, `engine/seen.rs`, `engine/snapshots.rs` as built; `docs/architecture/decisions/0035-device-sync-finishing-an-unfinished-key-change.md` in the form of ADR 0034 (the decision, T1–T5, what stays open per the spec's §2.5, where the build departed from the spec), its row in the index, and "Amended by ADR 0035" on ADR 0034's status line (its first open item is answered). Commit (`docs(sync): finishing an unfinished key change, as built`).

## Not in this plan

- A refuted head, a server that went back, finding B1 (spec §1).
- A takeover from the remover itself, and a guided new set-up (spec §4): only the one sentence and the help text.
- The bundle carrying the remover's base (spec §2.5, "later").
