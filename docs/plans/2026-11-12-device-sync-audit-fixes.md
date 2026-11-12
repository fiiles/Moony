# Device Sync Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the findings of the independent device-sync security audit
(`docs/audits/2026-11-11-device-sync-security-audit.md`) that the owner ruled "fix now" (H2–H4,
M1–M4, M7–M9, M11–M13 and the cheap lows), state H1 honestly in the UI and the docs, and correct
the documentation for the accepted M5, M6 and M10 with the cheap hardening the audit names. Sync
stays hidden behind the runtime flag.

**Architecture:**
1. **Capture or guard.** An enabled profile's connection always has change capture installed, or
   an SQLite authorizer (`capture::guard`) that refuses every write to a Synced table. A failed
   capture install no longer opens the profile for uncaptured edits (H2); the repair lifts the
   guard only after capture installs again.
2. **Safe replaces.** Joining, repairing and re-syncing take the verified backup and replace the
   profile in one connection hold, after checking that no more is at stake than the person agreed
   to (`impact::ReplaceConsent`, M8). The replace carries this device's documents and photos over
   to items the vault still has (H3). Re-sync first pushes what it can. Sync backups are flushed
   through a write handle (M9), appear only complete, are pruned (L11), are listed in Restore on
   desktop and phone, and on iOS can be copied to Files (H4).
3. **Pairing with an acknowledgement.** A third mailbox slot (relay branch `feat/pairing-ack`)
   carries the shower's HMAC over the transcript. The scanner shows its six digits only after that
   acknowledgement verifies, so nobody can learn them before the shower has fixed the key it uses
   (M2, L8). The holder never shows digits: the person types the joiner's six digits on the holder,
   and Rust checks them (three attempts) before anything is sealed (M1). The HKDF output grows from
   40 to 72 bytes; the first 40 are unchanged, so the frozen key and code vectors still hold.
4. **Relay faults stop being fatal.** A missing envelope holds the cursor like a quarantined
   author's while everything else, pushing included, goes on (M3). Sealed envelopes stay in
   `sync_unsent` until this device pulls them back, so an acknowledgement the relay did not honour
   is sent again (M4; no migration). Repeated `410` and endless pulls back off (L2, L3).
5. **Platform and flag.** Builds without the sync flag keep capturing an enabled profile (no
   uncaptured edits, a deliberate deviation from the audit's M12 fix, see corrections), and CI runs
   the release-flag test (L12). The binding fails closed (M10). Envelopes from the far future wait,
   a device's public key comes only from itself (M6), and re-join IDs keep room (L1).
6. **Recovery Kit.** No plain clipboard fallback on a native failure; saved owner-only from Rust
   with a synced-folder warning; the password is needed once the vault secrets exist; the main
   database deletes securely; the Kit leaves the screen after five minutes or when the app is
   hidden; screen capture is off while it shows (M11, L16).
7. **Relay.** `feat/pairing-ack` (slot 3, resumable vault deletion L14) and `feat/dev-grant`
   (loopback-only dev grant, M13).

**Tech Stack:** Rust 2021, rusqlite 0.40 (SQLCipher; the `hooks` feature added for the
authorizer), tokio 1, `hkdf` 0.13, `hmac` 0.13 (already in Cargo.lock through `hkdf`), `sha2`
0.11, `subtle` 2, `x25519-dalek` 3, `zip` 9; Tauri 2 (`tauri-plugin-dialog` from Rust,
`WebviewWindow::set_content_protected`); React 19, TanStack Query 5, i18next (ten locales),
Vitest (node); Moony-relay (TypeScript Workers, `@cloudflare/vitest-plugin`) under `wrangler dev`.

**Spec:** the audit and its PoC patch (`docs/audits/2026-11-11-device-sync-audit-poc-tests.patch`);
`docs/specs/2026-11-06-device-sync-client-design.md` (§2 G3, §9.3, §11.5, §13, §20); ADR 0024,
0025, 0026; the phase 3 plan `docs/plans/2026-11-10-device-sync-phase-3-second-device.md`; the
relay contract `/Users/filipkral/Programování/FinanceApp/Moony-relay/DESIGN.md` ("Pairing
mailbox") and `src/mailbox.ts`; this plan's pre-flight
`.superpowers/sdd/2026-11-12-device-sync-audit-fixes/preflight.md`.

## Audit corrections (the code and this plan win)

1. **PoC names do not follow finding numbers.** In the patch `audit_m1_…` proves M3, `audit_m2_…`
   M4, `audit_f1_…` M1, `audit_f2_…` M2, `audit_f3_…` L7, `audit_f4_…` L8. Task 1 renames them
   `poc_<finding>_…`.
2. **H2 is fixed by prevention, not by diffing.** A diff against the vault's snapshot cannot tell
   an uncaptured edit from a stale value once either side has a newer clock, and the repair's drain
   merges remote changes over uncaptured ones before any diff could run. The guard makes uncaptured
   edits impossible instead. Edits made before this build in a capture-broken session (developer
   profiles only) stay recoverable from the repair's pre-drain backup, which is kept.
3. **M12's proposed fix would lose data.** Returning early from `on_open` in a build without the
   flag lets that build write an enabled profile uncaptured; a later sync build never sends those
   edits and does not know they exist. Builds without the flag therefore keep capturing an enabled
   profile (nothing goes over the network; the outbox waits for a sync build). Task 18 pins this
   decision with tests and the ADR; the audit's concern (a hidden UI cannot turn it off) remains a
   developer-profile-only cost.
4. **M4 needs no migration.** Rows stay in `sync_unsent` after the push answer; `push_all` skips
   the sequences it already sent in the same call; `chain::acknowledge_own` (already there) deletes
   a row when the pull sees it. A relay that lost an envelope gets it again in the next round (the
   op ID keeps an honest relay from storing it twice).
5. **M3 reuses the held cursor of a quarantined author.** A gap is a hold, not a stop: the cursor
   stays before it, later envelopes are handled, the next pull meets the held one again and applies
   it once its predecessor arrived. `PullStop::Gap` goes; `SyncProblem::Gap` stays as the shown
   problem.
6. **M2 needs a relay change.** Two write-once, read-once slots carry two messages, and the fix
   needs three (scanner key, shower acknowledgement, sealed credentials). A second mailbox derived
   from the first ID would avoid the relay change but doubles the relay's state per pairing, two
   lifetimes and burns to keep in step, and hides the protocol from the relay contract; rejected
   (pre-flight C3). Pairing against a sync server without slot 3 answers
   `validation.syncServerNeedsUpdate`.
7. **L8 is closed by M2's acknowledgement.** A scanner that took an impostor's slot 1 for its own
   write never gets a valid acknowledgement and never shows digits.
8. **L10, holder's confirm:** the password is checked at pairing start (phase 3 PF1) and the confirm
   now needs the six digits from the other device's screen; a second password there adds nothing.
   Turning sync off and repairing get the password (Task 7).
9. **H3's citation:** the blob purge runs in `repair::invariants::run` (the `files` sweep), reached
   from `repair::full` because `everything()` marks the attachment owners deleted.

## Global Constraints

- Work only in `/Users/filipkral/Programování/FinanceApp/Moony-tauri-sync` on branch
  `fix/device-sync-audit`; run `git branch --show-current` before every commit. Never touch
  `../Moony-tauri`. Relay work: Tasks 11 and 25 in a new worktree
  `/Users/filipkral/Programování/FinanceApp/Moony-relay-ack` on a new branch `feat/pairing-ack`
  cut from the relay's `main` (a2ec09d); Task 24 in `/Users/filipkral/Programování/FinanceApp/Moony-relay`
  on `feat/dev-grant` (checked out there; check `git branch --show-current`); Task 28's scratch
  branch `e2e/audit-fixes` only in the `-ack` worktree, deleted after the run. Never commit to the
  relay's `main`. Never push, never deploy.
- Flag: `feature::RELEASED` stays `false`. Every new sync command starts with `require_feature()?`.
- The ten rules of `AGENTS.md`; the 7-step type-contract procedure for every command and wire type
  (`bindings.rs`, `shared/schema.ts`, regenerated `shared/generated-types.ts`); per-field
  `#[serde(rename = "camelCase")]`; thin commands; `AppError`/`SyncError`; no `.unwrap()` outside
  tests. **No migration** (pre-flight C6). No new `SyncProblem` variant.
- Rule 8 per task: every key a task emits or renders lands in all ten locales in that task, with
  the plural forms of `docs/standards/i18n.md` §2; English and Czech written, the other eight
  translated with `docs/standards/i18n-glossary.md` (German formal Sie, Spanish tú); "sync server"
  (cs "synchronizační server"), never "relay". Gates `npx vitest run src/i18n/` and
  `cargo test --lib every_sync_validation_key`.
- Data safety: no join, repair or re-sync may lose anything without a backup the person can reach
  (listed in Restore, and on iOS copyable to Files) and a confirmation that says what is at stake,
  with counts.
- Pairing: any byte change updates the frozen vectors (Task 10) and ADR 0026's amendment (Task 27).
  The gates stay: password before any QR or slot write, both confirm gates (the holder's is now the
  typed code), contested-scan handling, ambiguous writes counted as ours, a lost read ends a
  pairing.
- Secrets: the acknowledgement key and typed code live in `Zeroizing`; `Debug` stays redacted;
  nothing secret in errors or logs above `debug`. Kit contents passed to Rust are zeroized after the
  write.
- Tests: Rust services use `test_support::migrated()`, `engine::tests::{TestConn, vault}`,
  `FakeRelay`; never `Database`. Test files stay under 800 lines: new tests go into the new files
  each task names (`engine/tests/pull.rs`, `setup.rs`, `snapshots.rs`, `pairing/session_tests.rs`,
  `repair/tests.rs` are at or over the limit). Frontend tests are pure functions in node.
- Every task ends with its filter test and, from `src-tauri/`: `cargo test --lib`,
  `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`; tasks touching the frontend or a
  locale also `npm test`, `npm run typecheck`, `npm run lint`, `npm run format:check`. Relay tasks:
  `npm test` and `npx tsc --noEmit` in the relay worktree. Never `--no-verify`.
- `commands/sync.rs` is over 800 lines: only parameter changes there; new commands go to
  `commands/sync_backups.rs` and `commands/sync_kit.rs`, each covered by the command-file test that
  checks every command starts with `require_feature()?`.
- Stage by explicit path. Commits: Conventional Commits ending with
  `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. **A write lands between the backup and the replace** (the MCP server, a price refresh, a
   person on another screen): the replace happens in the hold that wrote the backup, after the
   consent check — Task 6 `the_join_backs_up_and_replaces_in_one_connection_hold`,
   `a_change_after_the_person_agreed_stops_the_resync`.
2. **Blobs purged before the attachments are re-attached** (the full repair's sweep runs inside the
   replace): re-attach precedes the repair, and a failed replace keeps rows and blobs — Task 4
   `a_resync_keeps_a_policys_document_and_its_blob`,
   `a_failed_replace_leaves_attachments_and_drops_the_temp_tables`.
3. **An enabled connection with neither capture nor guard** (a join that fails and cannot reinstall
   capture, a repair whose reinstall fails, a cached statement prepared before the guard) —
   Task 2 `install_or_guard_imposes_the_guard_when_install_fails`,
   `a_statement_cached_before_the_guard_is_refused_after_it`,
   `repair_lifts_the_guard_only_after_capture_installs`.
4. **The acknowledgement's lost answers** (a 503 after the relay stored slot 3; a read-once answer
   lost on the way back): the shower takes `SlotTaken` after an ambiguous put as its own, a lost
   read ends the pairing clearly, digits never show without a verified acknowledgement — Task 13
   `a_lost_ack_answer_still_reaches_compare`, `a_lost_ack_read_ends_the_pairing_clearly`,
   `a_scanner_shows_no_digits_before_a_valid_ack`.
5. **Gap hold and echo together** (a relay that drops an envelope it acknowledged): the author sends
   it again, peers holding the gap apply it, nobody stops pushing, and `push_all` never loops on its
   own unechoed rows — Task 16 `an_envelope_the_relay_acknowledged_and_lost_is_sent_again`,
   `one_push_all_sends_each_envelope_once`; Task 15
   `a_withheld_envelope_holds_the_cursor_while_others_sync_and_pushes_go_on`.

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/capture/guard.rs` | The authorizer that closes Synced tables when capture is off |
| `src-tauri/src/services/sync/backups.rs` | Verified, atomic, pruned, listed sync backups |
| `src-tauri/src/services/sync/impact.rs` | What a replace puts at stake; the consent check |
| `src-tauri/src/services/sync/snapshot/attachments.rs` | Keeping and re-attaching Local attachment rows across a replace |
| `src-tauri/src/services/sync/join.rs` | Join, repair, re-sync: one hold, consent, push-first, drain to echo |
| `src-tauri/src/services/sync/pairing/{mod.rs,session.rs}` | Acknowledgement bytes, typed-code check, the session's new stages |
| `src-tauri/src/services/sync/relay/{mod.rs,http.rs,fake.rs}` | Slot 3; plain http only in debug builds |
| `src-tauri/src/services/sync/engine/{pull.rs,push.rs,mod.rs,status.rs,snapshots.rs}` | Gap hold, echo, back-off |
| `src-tauri/src/services/sync/{binding.rs,chain/mod.rs,chain/rejoin.rs,merge/columns.rs}` | Fail-closed binding, far-future clocks, public key rule, ID headroom |
| `src-tauri/src/services/sync/{kit_file.rs,setup.rs}`, `src-tauri/src/db/mod.rs` | Kit file rules, Kit password gate, secure delete |
| `src-tauri/src/commands/{sync_backups.rs,sync_kit.rs,sync.rs,sync_pairing.rs}` | Commands |
| `src-tauri/src/models/sync.rs`, `bindings.rs`, `shared/schema.ts`, `src/lib/tauri-api.ts` | Wire types |
| `src/utils/{replace-impact,recovery-kit-lifetime,device-sync-problem,pairing-code,sync-devices}.ts` (+ tests) | Pure UI helpers |
| `src/components/settings/sync/*`, `src/components/common/RestoreBackup.tsx` | Desktop UI |
| `src/mobile/components/sync/*`, `src/mobile/pages/settings/{SyncSection,BackupSection}.tsx` | Phone UI |
| `Moony-relay` `src/index.ts`, `src/vault.ts`, `src/dev.ts`, `DESIGN.md`, `test/*` | Relay branches |
| `docs/architecture/decisions/0027-device-sync-audit-fixes.md` | ADR |

---

### Task 1: The audit's proofs of concept in the code

The audit files themselves were committed with this plan. This task brings the nine PoC tests into
the code, renamed by finding, still asserting today's faulty behaviour; each later task flips its
own.

**Files:**
- Create: `src-tauri/src/services/sync/engine/tests/relay_faults.rs` (the M3 and M4 PoCs),
  `src-tauri/src/services/sync/pairing/session_audit_tests.rs` (M1, L7, L8)
- Modify: `engine/tests/mod.rs` (`mod relay_faults;`), `pairing/session.rs`
  (`#[cfg(test)] #[path = "session_audit_tests.rs"] mod audit_tests;`), `pairing/session_tests.rs`
  (the helpers `pair`, `id`, `creds`, `public`, `assert_same_creds` and the secret constants become
  `pub(super)`), `pairing/tests.rs` (M2, ignored), `merge/tests/special.rs` (two M6 PoCs),
  `chain/rejoin.rs` (L1), `engine/tests/pull.rs` (only the helper visibility the moved tests need)

- [ ] **Step 1:** `git apply docs/audits/2026-11-11-device-sync-audit-poc-tests.patch`, then move the
  two `engine/tests/pull.rs` additions into `relay_faults.rs` and the three
  `session_tests.rs` additions into `session_audit_tests.rs` (both source files are at the 800-line
  limit). Rename: `poc_m3_out_of_order_author_wedges_the_pull_for_good`,
  `poc_m4_acknowledged_envelope_the_relay_drops_is_never_sent_again`,
  `poc_m1_impostor_first_scanner_gets_keys_on_lone_holder_confirm`,
  `poc_m2_relay_that_sees_the_phones_digits_grinds_a_matching_scanner_key` (`#[ignore]` kept),
  `poc_l7_impostor_holder_fills_slot_2_before_the_cancel_marker`,
  `poc_l8_impostor_after_a_failed_announce_is_taken_for_our_own_write`,
  `poc_m6_forged_older_clock_takes_over_a_devices_public_key`,
  `poc_m6_maximum_clock_pins_a_value_and_later_edits_diverge`,
  `poc_l1_repeated_rejoins_exhaust_the_id_space`. Each gets a one-line comment naming the audit
  finding and the task that will flip it.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib poc_` → 8 PASS, 1 ignored (they prove the
  faults). Gates.
- [ ] **Step 3: Commit** `test(sync): the audit's proofs of concept, asserting today's behaviour`.

---

### Task 2: A profile whose change capture failed closes its synced tables (H2)

**Files:**
- Create: `src-tauri/src/services/sync/capture/guard.rs` (with its tests)
- Modify: `src-tauri/Cargo.toml` (rusqlite features + `"hooks"`), `capture/mod.rs`
  (`pub mod guard;`, `install_or_guard`), `services/sync/mod.rs` (`on_open` imposes the guard when
  `open_capture` fails on an enabled profile), `join.rs` (`replace_profile` lifts the guard inside
  its savepoint before it restores, and its failure branch uses `install_or_guard`;
  `repair_capture`'s reinstall uses `install_or_guard` and lifts the guard only after a successful
  install), `setup.rs` (turning sync off and deleting the synced data lift the guard after
  `capture::uninstall`), `src-tauri/src/error.rs` (`SQLITE_AUTH` → `validation.syncChangesPaused`;
  the key joins `every_sync_validation_key_has_an_english_translation`), the `break_capture` test
  helper (imposes the guard as `on_open` would), `engine/tests/repair.rs`,
  `src/i18n/locales/<10>/common.json` (`validation.syncChangesPaused`)

**Interfaces:**
```rust
// capture/guard.rs
pub const CHANGES_PAUSED: &str = "validation.syncChangesPaused";
/// Registers an authorizer on this connection that denies INSERT, UPDATE and DELETE on every
/// Synced table (`registry::synced_tables`, `main` schema) and allows everything else (reads, Local
/// and Seed tables, `temp`, the `files` store). Creates `temp.sync_guard` as the marker. Setting
/// the authorizer expires every prepared statement, so cached ones are re-checked.
pub fn impose(conn: &Connection) -> Result<()>;
/// Removes the authorizer and the marker; a no-op when none is imposed.
pub fn lift(conn: &Connection) -> Result<()>;
pub fn is_imposed(conn: &Connection) -> Result<bool>;
// capture/mod.rs
/// `install`; when it fails, `guard::impose` (its own failure is logged) and the install's error.
pub fn install_or_guard(conn: &Connection, device: DeviceId) -> Result<()>;
```
Invariant (documented on `on_open` and in ADR 0027): an enabled profile's connection has capture
installed or the guard imposed after `on_open`, after any failed join, repair or re-sync, and after
a successful one (capture). Turning sync off removes both.

- [ ] **Step 1: Failing tests** (`capture/guard.rs`, `engine/tests/repair.rs`):
  - `the_guard_refuses_writes_to_synced_tables_only` — `impose` on `migrated()`: `INSERT INTO
    institutions` → `Err(AppError::Validation(CHANGES_PAUSED))`; an UPDATE and a DELETE likewise;
    `UPDATE sync_state …` and inserts into `mcp_pending_refresh` and `attachment_leftovers`
    (Local) succeed; `SELECT COUNT(*) FROM institutions` succeeds.
  - `a_replace_on_a_guarded_profile_ends_with_capture` — guard imposed, a successful replace →
    capture installed, guard gone; a failing replace → guard still imposed.
  - `a_statement_cached_before_the_guard_is_refused_after_it` — `prepare_cached` an institutions
    insert and run it once, `impose`, run it again → `CHANGES_PAUSED`.
  - `lifting_the_guard_lets_writes_through` — `impose`, `lift`, insert OK, `is_imposed` false.
  - `install_or_guard_imposes_the_guard_when_install_fails` — `ALTER TABLE sync_clock RENAME TO
    gone` makes `install` fail → `Err`, `is_imposed` true, `capture::is_installed` false.
  - `a_failed_capture_install_at_unlock_closes_the_synced_tables` — an enabled profile whose
    install fails (as `state::tests::a_failed_capture_install_marks_the_profile_capture_broken`
    breaks it) → `on_open` → `capture_broken_at` set, guard imposed, an institutions insert →
    `CHANGES_PAUSED`.
  - Flip `repair_backs_up_drains_and_rejoins_under_a_new_device`: after `break_capture`, writing
    `b-uncaptured` returns `CHANGES_PAUSED`; after the repair, every institution B ever wrote is on
    B and on the relay, and `guard::is_imposed` is false.
  - `repair_lifts_the_guard_only_after_capture_installs` — reinstall still failing →
    `validation.syncRepairDrainIncomplete`, guard still imposed; reinstall succeeding → guard lifted
    before the drain round.
  - `turning_sync_off_lifts_the_guard` — guard imposed, `setup::turn_off_between_rounds` → writes
    succeed, no capture, no guard.
  - `error.rs`: `an_authorizer_denial_reads_as_changes_paused`.
- [ ] **Step 2: Run** `cargo test --lib guard` and `cargo test --lib engine::tests::repair` → FAIL.
- [ ] **Step 3: Implement.** Locale `validation.syncChangesPaused` — en "Changes can't be saved on
  this device until sync is repaired in Settings → Sync.", cs "Na tomto zařízení nelze ukládat
  změny, dokud v Nastavení → Synchronizace neopravíte synchronizaci." (+ eight).
- [ ] **Step 4: Run** the same → PASS; `npx vitest run src/i18n/`; gates.
- [ ] **Step 5: Commit** `fix(sync): a profile whose change capture failed refuses edits to synced data instead of losing them`.

---

### Task 3: The unlock sweep deletes only rows a newly applied migration seeded (M7)

**Files:**
- Create: `src-tauri/src/services/sync/repair/sweep_tests.rs` (`repair/tests.rs` is at the limit;
  registered from `repair/mod.rs`)
- Modify: `registry/keys.rs` (`seeded_tables`), `repair/mod.rs` (`delete_revived_seeds` takes the
  tables), `services/sync/mod.rs` (`open_capture`), `repair/tests.rs` (the two existing sweep
  tests move to `sweep_tests.rs`)

**Interfaces:**
```rust
// registry/keys.rs — statements parsed like `seeds_with_volatile_values`
/// The Synced tables that a migration numbered above `after` inserts into or updates.
pub fn seeded_tables(after: u32) -> BTreeSet<&'static str>;
// repair/mod.rs
pub fn delete_revived_seeds(conn: &Connection, tables: &BTreeSet<&str>) -> Result<usize>;
```
`open_capture`: no sweep while `capture_broken_at` is set; otherwise the sweep runs over
`seeded_tables(sync.schema_seen)` (empty when no migration ran since the last unlock), before
`note_schema_seen`.

- [ ] **Step 1: Failing tests** (`sweep_tests.rs`, `registry/keys.rs`):
  - `seeded_tables_reads_the_migrations_after_a_number` — `seeded_tables(0)` contains
    `transaction_categories`; `seeded_tables(schema_number())` is empty; every name is Synced.
  - `an_ordinary_row_revived_while_capture_was_off_survives_the_unlock` (the flip of
    `opening_a_synced_profile_deletes_revived_seeds_before_capture`): `i1` deleted with capture,
    re-inserted without → `on_open` → `i1` still there, capture installed.
  - `a_revived_seed_of_a_new_migration_still_goes` — the old `cat_entertainment` test, calling
    `delete_revived_seeds(conn, &{"transaction_categories"})` → 1, then 0.
  - `the_sweep_waits_while_capture_is_marked_broken` — `capture_broken_at` set, `schema_seen`
    lowered below a seeding migration, a revived seed → `on_open` → the row stays.
- [ ] **Step 2: Run** `cargo test --lib sweep seeded_tables` → FAIL. **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): the unlock sweep deletes only rows a newly applied migration seeded`.

---

### Task 4: Documents and photos survive a replace (H3)

**Files:**
- Create: `src-tauri/src/services/sync/snapshot/attachments.rs`,
  `src-tauri/src/services/sync/snapshot/tests/attachments.rs`
- Modify: `snapshot/mod.rs` (`replace()`: keep before `clear()`, re-attach before `repair::full`;
  `RestoreReport.attachments`; the doc of `restore_replace` corrected), `snapshot/tests/mod.rs`

**Interfaces:**
```rust
// snapshot/attachments.rs
/// The Local attachment rows hanging off Synced parents, copied into TEMP tables
/// (`temp.sync_kept_<table>`) for real_estate_photo_batches, real_estate_photos,
/// real_estate_documents and insurance_documents.
pub(super) struct Kept { /* row counts per table */ }
pub(super) fn keep(conn: &Connection) -> Result<Kept>;
/// Re-inserts, parents first, every kept row whose parent exists after the restore (same ID);
/// drops the TEMP tables in every case.
pub(super) fn reattach(conn: &Connection, kept: Kept) -> Result<AttachmentCarry>;
// snapshot/mod.rs
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentCarry { pub kept: u32, pub orphaned: u32 }   // documents + photos; batches not counted
pub struct RestoreReport { pub stats: DumpStats, pub warnings: Vec<String>, pub attachments: AttachmentCarry }
```
Order inside `replace()`: `keep` → `clear` → the snapshot's rows → `reattach` → `repair::full`, so
`attachment_store::delete_unreferenced` only removes blobs of orphaned rows. The whole replace is
already one savepoint; a failure drops the TEMP tables (`reattach` or a cleanup in the error path).

- [ ] **Step 1: Failing tests** (`snapshot/tests/attachments.rs`, with the attachment store
  attached as `services/attachments.rs`'s tests do):
  - `a_resync_keeps_a_policys_document_and_its_blob` — policy `p1`, document `d1` with blob `b1`;
    dump this profile's snapshot; `restore_replace` → `d1` present, `b1` readable,
    `report.attachments == AttachmentCarry { kept: 1, orphaned: 0 }`.
  - `photos_keep_their_batch_and_both_blobs` — property `r1`, batch, photo (blob + thumbnail) →
    both blobs readable after the replace, `kept: 1`.
  - `a_document_whose_parent_the_vault_deleted_goes_and_is_counted` — the snapshot lacks `p2` →
    `d2` and its blob gone, `orphaned: 1`.
  - `a_failed_replace_leaves_attachments_and_drops_the_temp_tables` — a snapshot whose rows fail
    the merge → `Err`, documents and blobs unchanged, no `sqlite_temp_master` name like
    `sync_kept_%`.
  - `a_new_devices_own_documents_count_as_orphaned` — a joiner's own policy with a document, a
    vault snapshot without it → `orphaned: 1`.
- [ ] **Step 2: Run** `cargo test --lib snapshot::tests::attachments` → FAIL. **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): a replace keeps this device's documents and photos where their item is in the vault`.

---

### Task 5: Sync backups: a write handle, an atomic name, pruning and a list (M9, L11; H4's groundwork)

**Files:**
- Create: `src-tauri/src/services/sync/backups.rs` (with tests)
- Modify: `services/sync/mod.rs` (`pub mod backups;`), `join.rs` (`back_up`, `free_backup_path`,
  `read_back` move out; `REPAIR_BACKUP_PREFIX`/`SUFFIX` re-exported from `backups`),
  `services/auth.rs` (Delete Account also removes `.zip.partial` sync backups)

**Interfaces:**
```rust
pub const PREFIX: &str = "sync-repair-";
pub const SUFFIX: &str = ".zip";
pub const PARTIAL_SUFFIX: &str = ".zip.partial";
pub const KEEP_NEWEST: usize = 3;
pub const KEEP_DAYS: i64 = 30;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBackup { pub path: PathBuf, pub created_at: i64, pub bytes: u64 }
/// `backup::create_backup` into `<PREFIX><now>[-n]<PARTIAL_SUFFIX>`, `flush_file`, the read-back
/// (`backup::inspect_backup`, not newer than this build, every entry read to its end), rename to the
/// `.zip` name, `crypto::sync_parent_dir`, then `prune`. A failure removes the partial file.
pub fn write_verified(conn: &Connection, data_dir: &Path, now: i64) -> Result<PathBuf>;
/// Opens the file for reading and writing and `sync_all`s it (Windows' FlushFileBuffers needs write access).
pub fn flush_file(path: &Path) -> io::Result<()>;
/// The finished sync backups, newest first; partial and foreign names ignored; `created_at` from the name.
pub fn list(data_dir: &Path) -> Result<Vec<SyncBackup>>;
/// Keeps the newest `KEEP_NEWEST` and every backup younger than `KEEP_DAYS`; removes partial files
/// older than a day. Returns how many went.
pub fn prune(data_dir: &Path, now: i64) -> Result<usize>;
/// `path` is a finished sync backup directly inside `data_dir`.
pub fn is_sync_backup_path(data_dir: &Path, path: &Path) -> bool;
```

- [ ] **Step 1: Failing tests:**
  - `a_backup_appears_only_complete` — `write_verified` → `sync-repair-NOW.zip`, no `.partial`
    left; a second call in the same second → `sync-repair-NOW-2.zip`.
  - `a_backup_that_does_not_read_back_leaves_nothing_under_the_final_name` — through a test seam
    `write_verified_with(.., read_back: impl FnOnce(&Path) -> Result<()>)` failing → `Err`, neither
    `.zip` nor `.zip.partial` exists.
  - `flush_file_asks_for_write_access` (`#[cfg(unix)]`) — Ok on a normal file; `Err` with
    `PermissionDenied` on a file made read-only (a read-only handle would have passed).
  - `prune_keeps_the_newest_three_and_anything_recent` — ages 1, 10, 40, 50, 60, 70 days → 3
    removed (50, 60, 70); five backups all under 30 days → none removed; a two-day-old `.partial`
    removed, an hour-old one kept.
  - `list_reads_names_newest_first_and_ignores_others` — `.partial`, `pre-restore-…`,
    `sync-repair-x.zip` ignored.
  - `is_sync_backup_path_wants_a_finished_backup_in_the_folder` — a subfolder, `..`, another
    folder, a partial → false.
  - `services/auth.rs`: extend the existing Delete Account files test with a `.zip.partial`.
- [ ] **Step 2: Run** `cargo test --lib backups` → FAIL. **Step 3: Implement**; `join.rs` calls
  `backups::write_verified` (the hold change is Task 6).
- [ ] **Step 4: Run** → PASS; `cargo test --lib engine::tests::repair engine::tests::join_new`;
  gates.
- [ ] **Step 5: Commit** `fix(sync): sync backups are flushed with a write handle, appear only complete, and old ones are pruned`.

---

### Task 6: Replaces back up and replace in one hold, within what the person agreed to (M8; H2–H4 data)

**Files:**
- Create: `src-tauri/src/services/sync/impact.rs` (with tests),
  `src-tauri/src/services/sync/engine/tests/replace_safety.rs`
- Modify: `services/sync/mod.rs` (`pub mod impact;`), `join.rs`, `engine/tests/mod.rs`,
  `engine/tests/{join.rs,join_new.rs,repair.rs}` (new signatures), `relay_e2e.rs`,
  `relay_e2e_pairing.rs` (signatures), `src/i18n/locales/<10>/common.json`
  (`validation.syncReplaceImpactChanged`)

**Interfaces:**
```rust
// impact.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceKind { Join, Repair, Resync }
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceImpact {
    pub unsent_changes: u32,     // sync_outbox rows + sync_unsent rows
    pub documents: u32,          // real_estate_documents + insurance_documents
    pub photos: u32,             // real_estate_photos
    pub accounts: u32,           // bank_accounts, loans, real_estate, other_assets, stock_investments, crypto_investments, insurance_policies
    pub transactions: u32,       // bank_transactions
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceConsent { pub unsent_changes: u32, pub attachments: u32 }
pub const IMPACT_CHANGED: &str = "validation.syncReplaceImpactChanged";
// join.rs
/// Drain rounds a repair runs at most before the replace (Task 16 adds the wait for echoes).
pub const MAX_DRAIN_ROUNDS: usize = 3;
pub fn replace_impact(conn: &Connection) -> Result<ReplaceImpact>;
/// Join: documents + photos above `consent.attachments` → IMPACT_CHANGED. Resync: unsent above
/// `consent.unsent_changes` → IMPACT_CHANGED. Repair: any unsent change → `validation.syncRepairDrainIncomplete`.
pub fn check_consent(kind: ReplaceKind, now: &ReplaceImpact, consent: &ReplaceConsent) -> Result<()>;
// join.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceOutcome { pub backup: PathBuf, pub attachments: AttachmentCarry, pub unsent_in_backup_only: u32 }
pub async fn join_new_device<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: Credentials, device_name: &str,
    consent: ReplaceConsent, data_dir: &Path, native: &dyn BindingNative, now: i64) -> SyncResult<ReplaceOutcome>;
pub async fn repair_capture<R: Relay, D: ConnAccess>(relay: &R, db: &D, data_dir: &Path,
    native: &dyn BindingNative, now: i64) -> SyncResult<ReplaceOutcome>;
pub async fn resync_by_replace<R: Relay, D: ConnAccess>(relay: &R, db: &D, consent: ReplaceConsent,
    data_dir: &Path, native: &dyn BindingNative, now: i64) -> SyncResult<ReplaceOutcome>;
/// The replace without a backup: tests and the e2e harness only.
#[cfg(test)] pub(crate) async fn join_by_replace<R: Relay, D: ConnAccess>(/* as today */) -> SyncResult<RestoreReport>;
```
Flows (all under `engine::exclusive`; the snapshot is fetched and opened before the hold):
- **Join:** checks → fetch + open → one `with_conn` hold { `may_join`; `check_consent(Join)`;
  `backups::write_verified`; `replace_profile` } → binding.
- **Re-sync:** `rejoin_if_copy_under_lock`; best-effort `push::push_all` (a failure is logged; the
  hold decides) → fetch + open → one hold { `check_consent(Resync)`; `write_verified`;
  `replace_profile` }; `unsent_in_backup_only` = the unsent count read in that hold.
- **Repair:** as today up to the drain (the pre-drain backup stays, see correction 2) → drain →
  fetch + open → one hold { `check_consent(Repair)`; `replace_profile` }; a change found in the hold
  sends the repair back to the drain (at most `MAX_DRAIN_ROUNDS` drain rounds in all), then
  `syncRepairDrainIncomplete`.
The check runs before the backup in each hold, so a refused replace writes no backup.

- [ ] **Step 1: Failing tests** (`replace_safety.rs`, `impact.rs`):
  - `impact.rs`: `the_impact_counts_what_a_replace_touches`, `consent_is_checked_per_kind` (a
    table over the three kinds, equal/above/below).
  - `the_join_backs_up_and_replaces_in_one_connection_hold` — a `HoldLog` `ConnAccess` wrapper
    around `TestConn` records, for every `with_conn`, whether a new `sync-repair-*.zip` appeared and
    whether `sync_state.device_id` changed inside it; exactly one hold has both, none has only one.
  - `a_resync_pushes_what_it_can_before_it_replaces` — B with two unsent institutions → resync
    with consent 2 → the relay holds both envelopes under B's old ID; after a round, B has both
    institutions; `unsent_in_backup_only == 0`.
  - `an_offline_resync_keeps_unsent_changes_in_the_backup_and_says_how_many` — the push fails
    (`relay.fail_next(Route::Push, Server(503))`), consent 2 → `unsent_in_backup_only == 2`, the
    backup exists.
  - `a_change_after_the_person_agreed_stops_the_resync` — consent 0, one unsent change and a
    failing push → `Err(IMPACT_CHANGED)`; B's device ID and rows unchanged; no new backup file.
  - `a_join_with_more_attachments_than_agreed_stops` — consent attachments 0, one document →
    `Err(IMPACT_CHANGED)`, nothing replaced, no backup.
  - `a_repair_with_a_change_after_its_drain_drains_again` — a relay wrapper whose `get_snapshot`
    first writes (captured) an institution into B → the hold finds it, the repair drains again and
    succeeds; the institution is on the relay.
  - `a_resync_of_a_copied_profile_rejoins_before_it_pushes` — restore marker present → the pushed
    envelopes carry the new device ID.
  - Update the existing join/repair/join_new tests to the new signatures and to `outcome.backup`.
- [ ] **Step 2: Run** `cargo test --lib replace_safety impact` → FAIL. **Step 3: Implement.**
  Locale `validation.syncReplaceImpactChanged` — en "Something changed on this device while you
  were deciding. Check the numbers again and confirm.", cs "Během rozhodování se na tomto zařízení
  něco změnilo. Zkontrolujte čísla znovu a potvrďte." (+ eight).
- [ ] **Step 4: Run** → PASS; `cargo test --lib engine::tests`; gates.
- [ ] **Step 5: Commit** `fix(sync): joins, repairs and re-syncs back up and replace in one hold, within what the person agreed to`.

---

### Task 7: Commands: replace impact, consent and outcome; sync backups; passwords for repair and turning off (H4, L10)

**Files:**
- Create: `src-tauri/src/commands/sync_backups.rs`
- Modify: `commands/sync.rs` (parameters only: `sync_turn_off` and `sync_repair_device` take
  `password`; `sync_repair_device`/`sync_resync_device` return `ReplaceOutcomeView`;
  `sync_resync_device` takes `consent`; the require-feature test covers `sync_backups.rs`),
  `commands/sync_pairing.rs` (`sync_pairing_join` and `sync_join_with_recovery_kit` take `consent`
  and return `ReplaceOutcomeView`), `commands/mod.rs`, `src-tauri/src/lib.rs` (registration),
  `services/backup.rs` (`is_export_file_name` also accepts `moony-sync-backup-YYYY-MM-DD.zip`),
  `models/sync.rs`, `bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`,
  `src/lib/tauri-api.ts`

**Interfaces:**
```rust
#[tauri::command] pub async fn sync_replace_impact(db: State<'_, Database>) -> Result<ReplaceImpactView>;
#[tauri::command] pub async fn sync_list_backups(app: AppHandle) -> Result<Vec<SyncBackupEntry>>;
/// Phones: copies a finished sync backup (`backups::is_sync_backup_path`) to
/// `Documents/moony-sync-backup-<date>.zip` for the Files sheet; `finish_backup_export` removes it.
#[tauri::command] pub async fn stage_sync_backup_export(app: AppHandle, path: String) -> Result<String>;
// changed
pub async fn sync_turn_off(app: AppHandle, db: State<'_, Database>, password: String) -> Result<()>;
pub async fn sync_repair_device(app: AppHandle, db: State<'_, Database>, password: String) -> Result<ReplaceOutcomeView>;
pub async fn sync_resync_device(app: AppHandle, db: State<'_, Database>, password: String, consent: ReplaceConsentView) -> Result<ReplaceOutcomeView>;
pub async fn sync_pairing_join(app: AppHandle, db: State<'_, Database>, consent: ReplaceConsentView) -> Result<ReplaceOutcomeView>;
pub async fn sync_join_with_recovery_kit(/* as today */, consent: ReplaceConsentView) -> Result<ReplaceOutcomeView>;
// models/sync.rs (camelCase per field)
pub struct ReplaceImpactView { unsent_changes: u32, documents: u32, photos: u32, accounts: u32, transactions: u32 }
pub struct ReplaceConsentView { unsent_changes: u32, attachments: u32 }
pub struct ReplaceOutcomeView { backup_path: String, attachments_kept: u32, attachments_in_backup_only: u32, unsent_in_backup_only: u32 }
pub struct SyncBackupEntry { path: String, created_at: i64, bytes: u64 }
```
```ts
// src/lib/tauri-api.ts (deviceSyncApi)
replaceImpact(): Promise<ReplaceImpactView>;
listBackups(): Promise<SyncBackupEntry[]>;
stageSyncBackupExport(path: string): Promise<string>;
turnOff(password: string): Promise<void>;
repairDevice(password: string): Promise<ReplaceOutcomeView>;
resyncDevice(password: string, consent: ReplaceConsentView): Promise<ReplaceOutcomeView>;
pairingJoin(consent: ReplaceConsentView): Promise<ReplaceOutcomeView>;
joinWithRecoveryKit(/* as today */, consent: ReplaceConsentView): Promise<ReplaceOutcomeView>;
```

- [ ] **Step 1: Failing tests:** `services/backup.rs` `a_sync_backup_copy_has_an_export_name`
  (`moony-sync-backup-2026-11-12.zip` accepted, `moony-sync-backup-x.zip` refused); a pure
  `commands/sync_backups.rs` helper `export_name(created_at: i64) -> String` tested in place;
  the command-file test fails until `sync_backups.rs` is listed and every command starts with
  `require_feature()?`.
- [ ] **Step 2: Run** `cargo test --lib export_name every_command` → FAIL. **Step 3: Implement**;
  `cargo test generate_bindings -- --ignored`; mirror in `shared/schema.ts`; update the callers in
  `src/hooks/use-device-sync-mutations.ts` to compile (UI text comes in Tasks 8–9).
- [ ] **Step 4: Run** → PASS; `npm run typecheck`; gates.
- [ ] **Step 5: Commit** `feat(sync): commands for the replace impact, sync backups and their Files copy; repair and turning sync off ask for the password`.

---

### Task 8: Desktop: what a replace puts at stake, sync backups in Restore, the password for repair and turning off

**Files:**
- Create: `src/utils/replace-impact.ts`, `src/utils/replace-impact.test.ts`
- Modify: `src/hooks/use-device-sync.ts` (impact and backups queries), `src/hooks/use-device-sync-mutations.ts`,
  `src/components/settings/sync/{SyncRepairDialogs,TurnOffSyncDialog,JoinSyncDialog,RecoveryJoinDialog,PairingSteps,SyncDangerCard}.tsx`,
  `src/components/common/RestoreBackup.tsx`, `src/i18n/locales/<10>/{deviceSync,settings}.json`

**Interfaces:**
```ts
export type ReplaceFlow = 'join' | 'repair' | 'resync';
export interface ImpactLine { key: string; count: number }
/** The lines a confirmation shows, in this order, each only when its count is above 0. */
export function impactLines(flow: ReplaceFlow, impact: ReplaceImpactView): ImpactLine[];
/** The consent a confirmation sends: exactly what it showed. */
export function consentOf(flow: ReplaceFlow, impact: ReplaceImpactView): ReplaceConsentView;
/** The toast after a replace: the backup's file name, then each count that is only in the backup. */
export function outcomeLines(outcome: ReplaceOutcomeView): ImpactLine[];
```
Confirmations load the impact when they open (no cache) and send `consentOf`; on
`validation.syncReplaceImpactChanged` they reload the impact and stay open. Join: "This device's
data is replaced by the synced data: {accounts} accounts and assets, {transactions} transactions,
{documents} documents and {photos} photos. Documents and photos are not synced; they stay only in
the backup." Re-sync: "{unsent} changes have not reached the sync server. Moony sends them first;
any it cannot send stay only in the backup." Repair: "Documents and photos stay attached." Both:
"The backup is listed under Settings → Backup → Restore." Turn-off and repair open
`SyncPasswordDialog` first. Restore gains "Backups made before sync replaced data" (date, size,
Restore → the existing inspect and restore flow).

- [ ] **Step 1: Failing tests** (`replace-impact.test.ts`): `join lists accounts, transactions,
  documents and photos`, `resync lists only unsent changes`, `zero counts are left out`,
  `consent repeats what was shown` (join → `{ unsentChanges: 0, attachments: documents + photos }`,
  resync → `{ unsentChanges, attachments: 0 }`), `outcome lines name what stayed in the backup`.
- [ ] **Step 2: Run** `npx vitest run src/utils/replace-impact.test.ts` → FAIL. **Step 3:
  Implement** UI and the ten locales (plural forms per `docs/standards/i18n.md` §2).
- [ ] **Step 4: Run** → PASS; `npm test`, `npm run typecheck`, `npm run lint`, `npm run format:check`.
- [ ] **Step 5: Commit** `feat(sync): desktop confirmations say what a replace puts at stake, and Restore lists sync backups`.

---

### Task 9: Phone: the same confirmations, sync backups in Záloha, a Files copy of the backup (H4)

**Files:**
- Modify: `src/mobile/pages/settings/{SyncSection,BackupSection}.tsx`,
  `src/mobile/components/sync/{SyncStatusBlock,PairingSheet,RecoveryJoinSheet,SyncPasswordSheet,SyncBusyCover}.tsx`,
  `src/mobile/lib/export-file.ts` (+ its test: `syncBackupExportName`),
  `src/i18n/locales/<10>/{deviceSync,mobile}.json`

Flow: the confirmations use `impactLines`/`consentOf` (Task 8). After a join, repair or re-sync,
on iOS (`exportsOnThisDevice`) a sheet says "A backup was saved on this phone" with "Save a copy to
Files" (`stageSyncBackupExport` → `save()` → `finishBackupExport`); elsewhere it says where the
backup is listed. Záloha lists the sync backups with Restore (the existing `useRestoreBackup`
flow with the listed path). Turning off and repair ask the password (`SyncPasswordSheet`).

- [ ] **Step 1: Failing test** (`export-file.test.ts`): `syncBackupExportName` gives
  `moony-sync-backup-YYYY-MM-DD.zip` from a backup's `createdAt` (UTC day).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; ten locales.
- [ ] **Step 4: Run** `npm test`, typecheck, lint, format; build the phone bundle once
  (`npm run build`).
- [ ] **Step 5: Commit** `feat(sync): phone confirmations with counts, sync backups in Záloha, and a Files copy of the backup`.

---

### Task 10: Pairing bytes: the shower's acknowledgement and the typed-code check (M1, M2)

**Files:**
- Modify: `src-tauri/Cargo.toml` (`hmac = "0.13"`), `src-tauri/Cargo.lock`,
  `services/sync/pairing/mod.rs`, `services/sync/pairing/tests.rs`

**Interfaces:**
```rust
pub const ACK_DIRECTION: &[u8] = b"shower->scanner";     // 15 ASCII bytes
pub const ACK_SLOT_LEN: usize = 33;                       // 0x01 ‖ 32-byte tag
pub const OKM_LEN: usize = 72;                            // was 40; bytes 0..40 unchanged
impl Transcript {
    /// Slot 3: `SLOT_VERSION ‖ HMAC-SHA256(ack key, ACK_DIRECTION ‖ mailbox ‖ S pub ‖ C pub)`;
    /// ack key = OKM[40..72] (Zeroizing).
    pub fn ack(&self) -> [u8; ACK_SLOT_LEN];
    /// In constant time; any other length, version or a cancel marker → false.
    pub fn verify_ack(&self, slot3: &[u8]) -> bool;
    /// Six ASCII digits, optionally one space or `-` after the third; equal to `code_text()` in
    /// constant time. Anything else → false.
    pub fn code_matches(&self, typed: &str) -> bool;
}
```
`derive` keeps one HKDF expand, now of 72 bytes (HKDF-Expand is prefix-consistent, so the session
key and the code are the frozen ones). The transcript keeps the ack key in `Zeroizing`; the message
reuses the AD's last 80 bytes (mailbox ‖ S pub ‖ C pub).

- [ ] **Step 1: Failing tests** (`pairing/tests.rs`; vectors computed by the planner with a
  pure-Python RFC 7748 X25519, RFC 5869 HKDF and RFC 2104 HMAC, independent of the crates):
  - `the_transcript_matches_the_frozen_vector` — unchanged (key `01b8d605…527b`, code 482686).
  - `the_ack_matches_the_frozen_vector` — S `[0x11; 32]`, C `[0x22; 32]`, mailbox `[0x33; 16]`:
    ack key (`#[cfg(test)] fn ack_key(&self)`) =
    `f2131f574e31de0d056d117dab2d59525121e925e2ab79436137e5f3987ae644`; `ack()` = `[0x01]` ‖
    `b473ae20bc53682f690b9ab3c17a0696027843f17d0988e72df937e5d0a8bced`, the same from
    `derive(S, Shower, C pub)` and `derive(C, Scanner, S pub)`.
  - `an_ack_for_a_substituted_key_does_not_verify` — `derive(S, Shower, M pub ff2ee456…3e6b)`:
    `ack()` = `[0x01]` ‖ `261a64208961cfca3bcbbb003c1c61a1fc75de1d4ba8b71dec0d9298966dace2`;
    C's transcript `verify_ack` of it → false; of its own S's ack → true.
  - `a_tampered_or_short_ack_is_refused` — one flipped bit, 32 and 34 bytes, version `0x02`,
    `CANCEL_MARKER` → false.
  - `the_typed_code_is_checked_exactly` — for 482686: `"482686"`, `"482 686"`, `"482-686"` true;
    `"482687"`, `"48268"`, `"4826860"`, `"48268a"`, `" 482686"`, `""` false; a transcript whose
    code is 7: `"000007"` true, `"7"` false.
  - extend `secrets_are_wiped_on_drop` to the ack key.
- [ ] **Step 2: Run** `cargo test --lib services::sync::pairing::tests` → FAIL. **Step 3:
  Implement.** **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): pairing acknowledgement and typed-code check, with frozen vectors`.

---

### Task 11 (relay, `feat/pairing-ack`): a third pairing slot

**Repo:** `git -C /Users/filipkral/Programování/FinanceApp/Moony-relay worktree add ../Moony-relay-ack -b feat/pairing-ack main`;
work in `/Users/filipkral/Programování/FinanceApp/Moony-relay-ack` (`git branch --show-current`
→ `feat/pairing-ack`).

**Files:** `src/index.ts` (slot `"1" | "2" | "3"`), `src/mailbox.ts` (comment: three slots),
`DESIGN.md` ("Pairing mailbox": slot 1 scanner → shower, slot 2 holder → joiner, slot 3 shower →
scanner; slot `1`–`3`, `400 bad_slot` otherwise), `README.md` if it names the slots,
`test/pairing.test.ts`

- [ ] **Step 1: Failing tests** (`test/pairing.test.ts`): `slot 3 is write-once and read-once`
  (put → 204, put again → 409 `slot_taken`, get → 200 bytes, get → 410 `consumed`), `slot 4 is
  bad_slot`, `a burn erases slot 3` (get → 410 `mailbox_gone`).
- [ ] **Step 2: Run** `npm test` → FAIL. **Step 3: Implement.** **Step 4: Run** `npm test`,
  `npx tsc --noEmit` → PASS.
- [ ] **Step 5: Commit** (in the relay worktree, no trailer rule there beyond the global one)
  `feat: a third pairing slot for the shower's acknowledgement`.

Parallel: independent of every app task.

---

### Task 12: The client's mailbox carries slot 3

**Files:**
- Modify: `services/sync/relay/mod.rs` (doc: three slots), `relay/http.rs` (`pairing_url` accepts
  1–3), `relay/fake.rs` (`slots: [Option<PairingSlot>; 3]`; `write_slot_raw`, `peek_slot` for 3;
  a `lose_next_answer(Route)` switch that stores a put, then answers `Server(503)`),
  `relay/http/mailbox_tests.rs`, `relay/fake/tests.rs`

- [ ] **Step 1: Failing tests:** `slot_3_round_trips_over_http` (the existing mock-server pattern),
  `slot_4_is_refused_before_any_request` (`BadRequest("bad_slot")`, no request made),
  `the_fake_keeps_slot_3_write_once_and_read_once`, `a_lost_put_answer_is_stored_anyway` (fake).
- [ ] **Step 2: Run** `cargo test --lib relay` → FAIL. **Step 3: Implement.** **Step 4: Run** →
  PASS; gates.
- [ ] **Step 5: Commit** `feat(sync): the pairing mailbox client carries slot 3`.

---

### Task 13: The session: digits only after the acknowledgement, the holder types them (M1, M2, L8)

**Files:**
- Modify: `services/sync/pairing/session.rs`, `pairing/session_tests.rs` (existing tests move
  through the new stage; no new tests here), `pairing/session_audit_tests.rs` (the PoC flips and
  the new tests), `src/i18n/locales/<10>/common.json` (`validation.syncPairingCodeMismatch`,
  `syncPairingCodeMismatchFinal`, `syncPairingInterfered`, `syncServerNeedsUpdate`),
  `src-tauri/src/error.rs` (the four keys in the translation test)

**Interfaces:**
```rust
pub const ACK_SLOT: u8 = 3;
pub const MAX_TYPED_ATTEMPTS: u8 = 3;
pub enum Stage {
    Announcing, WaitingForScan,
    /// Scanner: slot 1 written, waiting for the shower's acknowledgement in slot 3.
    AwaitingAck,
    /// Shower: slot 1 read, its acknowledgement not stored yet.
    Acknowledging,
    Compare, WaitingForOther, Sent, Received,
}
impl PairingSession {
    /// The joiner's six digits from Compare on; always `None` on the holder and on a contested scanner.
    pub fn comparison_code(&self) -> Option<String>;
    /// Holder: `typed` must match (`Transcript::code_matches`) before slot 2 is sealed; a mismatch
    /// is `syncPairingCodeMismatch` (session kept) until the third, which ends the pairing with
    /// `syncPairingCodeMismatchFinal` (cancel markers written). Joiner: `typed` is ignored.
    pub async fn confirm<M: Mailbox>(&mut self, mailbox: &M, holder_creds: Option<&Credentials>,
        typed: Option<&str>, now: i64) -> SyncResult<Stage>;
    pub fn attempts_left(&self) -> u8;
}
/// Also keeps the session for a typed-code mismatch with attempts left.
pub fn keeps_session(error: &SyncError) -> bool;
```
Rules: the scanner's announce → `AwaitingAck`; its poll takes slot 3: nothing → stay; cancel
marker → `cancelled`; a verifying ack → `Compare`; other bytes or `SlotConsumed` →
`syncPairingInterfered` (ends; markers). The shower's read of slot 1 → derive → `Acknowledging` →
put slot 3 (same call, retried by poll): Ok, or `SlotTaken` after an ambiguous put → `Compare`;
unambiguous `SlotTaken` → `cancelled`; `BadRequest("bad_slot")` → `syncServerNeedsUpdate`. A
contested scanner stays in `AwaitingAck` with `contested`, shows no digits, ignores slot 3 and only
cancels. `abandon` writes the cancel marker into every slot 1–3 this side has not written.

- [ ] **Step 1: Failing tests** (`session_audit_tests.rs`):
  - flip `poc_m1…` → `an_impostor_who_answers_first_gets_nothing_without_the_joiners_digits` —
    the holder-shower acks the impostor; `comparison_code()` is `None` on the holder; confirm with
    `None` or three wrong codes → mismatch, mismatch, `syncPairingCodeMismatchFinal`; slot 2 never
    holds sealed credentials (`peek_slot(2)` is `None` or the cancel marker).
  - `the_holder_seals_only_after_the_joiners_digits_are_typed` — joiner scans → `AwaitingAck`;
    holder polls → `Compare`; joiner polls → `Compare` with six digits; holder confirms with them →
    `Sent`; joiner confirms → polls → `Received`, credentials equal.
  - replace `poc_m2…` (the grind stays `#[ignore]` as
    `the_six_digits_alone_can_be_ground`, with a comment that the acknowledgement is the defence)
    by `a_scanner_shows_no_digits_before_a_valid_ack` — a `Mailbox` wrapper hands the shower M's
    slot 1 instead of C's; the shower acks (S, M); the scanner's poll → `syncPairingInterfered`;
    `comparison_code()` was `None` at every step.
  - flip `poc_l8…` → `an_impostor_after_a_lost_announce_answer_is_caught_by_the_ack` — as the PoC,
    then the scanner's poll → `syncPairingInterfered`, no digits, no slot 2.
  - keep `poc_l7…` as `residual_l7_a_joiner_who_confirms_anyway_can_join_an_impostor` (accepted,
    ADR 0027), adapted to the new stages.
  - `a_lost_ack_answer_still_reaches_compare` — `lose_next_answer(Route::MailboxPut)` on the
    shower's slot-3 put → `Acknowledging`; next poll → `SlotTaken` taken as ours → `Compare`.
  - `a_lost_ack_read_ends_the_pairing_clearly` — the fake's erase-then-timeout on slot 3 → the
    scanner's next poll → `syncPairingInterfered`.
  - `a_sync_server_without_slot_3_says_it_needs_an_update` — a wrapper answering `bad_slot` for
    slot 3 → the shower's poll → `syncServerNeedsUpdate`.
  - `a_contested_scanner_shows_no_digits_and_only_cancels`.
  - `abandoning_marks_slot_3_too`.
- [ ] **Step 2: Run** `cargo test --lib services::sync::pairing` → FAIL. **Step 3: Implement**;
  update `session_tests.rs` to the new stage order. Locales — en: mismatch "That code doesn't match.
  Check the six digits on your other device and type them again."; final "The code was typed wrong
  three times. Start pairing again on both devices."; interfered "Someone else answered this
  pairing. Nothing was sent. Start again on both devices."; server "This sync server needs an update
  before devices can be paired." (cs + eight).
- [ ] **Step 4: Run** → PASS; `cargo test --lib poc_ residual_`; gates.
- [ ] **Step 5: Commit** `fix(sync): pairing shows the code only after the other device proves its key, and the holder types it`.

---

### Task 14: Pairing commands and screens: typed code, connecting, the sync server shown (M1, L9)

**Files:**
- Modify: `commands/sync_pairing.rs` (`sync_pairing_confirm(typed_code)`; `view_of`: the holder
  never gets `comparison_code`, `server_host` when not default, `attempts_left` on the holder),
  `models/sync.rs` (`PairingStage::Connecting` for `AwaitingAck` and `Acknowledging`;
  `PairingView.server_host`, `attempts_left`), `bindings.rs`, `shared/schema.ts`,
  `shared/generated-types.ts`, `src/lib/tauri-api.ts` (`pairingConfirm(typedCode?: string)`),
  `services/sync/relay/http.rs` (L9) and `relay/http/tests.rs`, `src/utils/pairing-code.ts` (+
  test), `src/components/settings/sync/{PairingSteps,AddDeviceDialog,JoinSyncDialog}.tsx`,
  `src/mobile/components/sync/{PairingSheet,ScanFlow}.tsx`, `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
```rust
#[tauri::command] pub async fn sync_pairing_confirm(app: AppHandle, db: State<'_, Database>, typed_code: Option<String>) -> Result<PairingView>;
pub struct PairingView { /* as today */, #[serde(rename = "serverHost")] pub server_host: Option<String>,
    #[serde(rename = "attemptsLeft")] pub attempts_left: Option<u8> }
// relay/http.rs
/// `http` only for localhost/127.0.0.1 and only in a debug build; `https` always.
fn scheme_allowed(scheme: &str, host: Option<&str>, debug: bool) -> bool;
```
```ts
// src/utils/pairing-code.ts
/** Keeps digits only, at most six; "482 686" → "482686". */
export function normalizeTypedCode(input: string): string;
export function isCompleteCode(input: string): boolean;
```
Screens: holder at Compare — "Type the six digits shown on your other device" (a six-digit input,
`inputMode="numeric"`, `autocomplete="one-time-code"`), Confirm, the mismatch error with attempts
left. Joiner at Compare — the digits large, "Type this code on your other device. Continue here
once it says the code is right.", Continue (its confirm gate) and Cancel. `connecting` — "Connecting
securely…". With a server host: "Sync server: {{host}}".

- [ ] **Step 1: Failing tests:** `relay/http/tests.rs` `plain_http_is_a_debug_build_convenience`
  (`scheme_allowed("http", Some("127.0.0.1"), false)` false, `…, true)` true, `"https"` true either
  way, `"http"` with `example.com` false); `pairing-code.test.ts`: `normalizeTypedCode` keeps six
  digits, `isCompleteCode`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; bindings; ten locales.
- [ ] **Step 4: Run** `cargo test --lib relay::http`, `npm test`, typecheck, lint, format; gates.
- [ ] **Step 5: Commit** `feat(sync): pairing screens ask the holder to type the code and show the sync server`.

---

### Task 15: A missing envelope holds the cursor, not the device (M3)

**Files:**
- Create: `src-tauri/src/services/sync/engine/tests/gaps.rs`
- Modify: `engine/pull.rs` (`Verdict::Gap` → `OpStep::Hold`, `report.gaps += 1`), `engine/mod.rs`
  (`PullReport.gaps`, `PullEnd::Waiting`, `PullStop::Gap` removed, `may_push`), `engine/status.rs`
  (`problem_of_pull`: `Waiting` → `SyncProblem::Gap`), `engine/tests/{mod.rs,relay_faults.rs}`,
  `src/utils/device-sync-problem.ts` (+ test), `src/components/settings/sync/SyncProblemAlert.tsx`,
  `src/mobile/components/sync/SyncStatusBlock.tsx`, `src/i18n/locales/<10>/deviceSync.json` (the
  gap text)

**Interfaces:**
```rust
pub struct PullReport { /* as today */, /// Envelopes held because an earlier one of their author is missing.
    pub gaps: usize }
pub enum PullEnd { Done, Held, Waiting, Unfinished, Stopped(PullStop) }   // a stop first, then unfinished, then waiting (gaps), then held
// may_push: Done | Held | Waiting | Stopped(NewerSchema). may_snapshot: done() only.
```
```ts
export function problemAction(problem: SyncProblem, selfHosted: boolean, pendingChanges: number): SyncProblemAction;
// 'gap': pendingChanges > 0 → 'none'; 0 → 'resync'
```

- [ ] **Step 1: Failing tests:**
  - flip `poc_m3…` → `an_out_of_order_author_is_applied_on_the_next_pull` — first pull: `end()
    == Waiting`, `gaps == 1`, `may_push()`, cursor 0, `b1` applied; second pull: done, cursor 2,
    `b1` and `b2` present.
  - `a_withheld_envelope_holds_the_cursor_while_others_sync_and_pushes_go_on` (`gaps.rs`) — the
    relay drops B's first envelope; C's envelope after it applies; A's round pushes its own change;
    the status problem is `Gap`.
  - `a_held_gap_never_uploads_a_snapshot` — a due upload is skipped while `gaps > 0`.
  - `own_envelopes_after_a_gap_are_still_acknowledged` — A's unsent row after B's gap is removed by
    the pull.
  - `device-sync-problem.test.ts`: gap with pending 3 → `'none'`, with 0 → `'resync'`; other
    problems unchanged.
- [ ] **Step 2: Run** `cargo test --lib gaps relay_faults`, `npx vitest run src/utils/device-sync-problem.test.ts` → FAIL.
- [ ] **Step 3: Implement.** Gap text — en "A change from another device hasn't arrived yet. This
  device keeps syncing and sending its own changes." (cs + eight).
- [ ] **Step 4: Run** → PASS; `cargo test --lib engine::tests`; `npm test`; gates.
- [ ] **Step 5: Commit** `fix(sync): a missing envelope holds the cursor while this device keeps syncing and pushing`.

---

### Task 16: Sealed envelopes stay until this device pulls them back (M4)

**Files:**
- Create: `src-tauri/src/services/sync/engine/tests/echo.rs`
- Modify: `engine/push.rs` (no delete on the push answer; `next_push_batch_with` takes `after`;
  `push_all` tracks the last sequence it sent), `join.rs` (`drained`: up to `MAX_DRAIN_ROUNDS`
  drain rounds until `sync_outbox` and `sync_unsent` are empty), `chain/mod.rs` (doc of
  `acknowledge_own`), `engine/tests/{mod.rs,relay_faults.rs,push.rs}` (tests that asserted deletion
  on the answer now assert it after the pull)

**Interfaces:**
```rust
/// The waiting envelopes above `after` (all when `None`), within the limits.
pub fn next_push_batch_with(conn: &Connection, after: Option<u64>, max_ops: usize, max_b64: usize) -> Result<Vec<(u64, OutgoingOp)>>;
pub fn next_push_batch(conn: &Connection, after: Option<u64>) -> Result<Vec<(u64, OutgoingOp)>>;
// `acknowledge_pushed` is removed: a row leaves `sync_unsent` only through `chain::acknowledge_own`.
// join.rs: `drained` runs up to Task 6's MAX_DRAIN_ROUNDS drain rounds until both tables are empty.
```

- [ ] **Step 1: Failing tests:**
  - flip `poc_m4…` → `an_envelope_the_relay_acknowledged_and_lost_is_sent_again` — after
    `drop_op(1)`, A's next round sends `a1` again; B (pulling twice) holds then applies both
    institutions; A's `sync_unsent` is empty after A's next pull.
  - `a_push_answer_alone_never_empties_sync_unsent` (`echo.rs`).
  - `one_push_all_sends_each_envelope_once` — the fake relay sees each op ID once per call.
  - `the_repair_drains_until_its_envelopes_come_back` — a clean relay → repaired after two drain
    rounds; a relay that drops every push → `syncRepairDrainIncomplete` after three, nothing
    replaced.
- [ ] **Step 2: Run** `cargo test --lib echo relay_faults` → FAIL. **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; `cargo test --lib engine::tests`; gates.
- [ ] **Step 5: Commit** `fix(sync): sealed envelopes stay until this device pulls them back from the sync server`.

---

### Task 17: Back off repeated snapshot demands, push after endless pulls, never overflow the deletion date (L2, L3, L5)

**Files:**
- Create: `src-tauri/src/services/sync/engine/tests/backoff.rs`
- Modify: `engine/snapshots.rs` (`SnapshotPolicy` state), `engine/mod.rs` (`round_steps`),
  `engine/status.rs` (`checked_add`), `engine/tests/mod.rs`

**Interfaces:**
```rust
pub const RESTORE_BACKOFF_SECS: i64 = 6 * 3600;
pub const UNFINISHED_BEFORE_PUSH: u32 = 3;
impl SnapshotPolicy {
    /// True at most once per RESTORE_BACKOFF_SECS; records `now` when true.
    pub fn may_restore_after_410(&mut self, now: i64) -> bool;
    /// The run of consecutive unfinished pulls, this one included (0 after any other end).
    pub fn note_pull(&mut self, report: &PullReport) -> u32;
}
```
A second `410` inside the back-off → no restore, `SyncProblem::RelayError`. After
`UNFINISHED_BEFORE_PUSH` unfinished pulls in a row the round pushes and reports `RelayError` (it no
longer records success). `deletes_at = paid_until.and_then(|at| at.checked_add(DELETE_AFTER_LAPSE))`.

- [ ] **Step 1: Failing tests:** `a_relay_that_answers_410_every_round_gets_one_restore_per_six_hours`,
  `three_unfinished_pulls_in_a_row_let_the_device_push`, `a_paid_until_near_the_maximum_has_no_deletion_date`.
- [ ] **Step 2: Run** `cargo test --lib backoff` → FAIL. **Step 3: Implement.** **Step 4: Run** →
  PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): back off repeated snapshot demands, push after endless pulls, and never overflow the deletion date`.

---

### Task 18: Builds without sync keep capturing an enabled profile; CI runs the release-flag test (M12, L12)

**Files:**
- Modify: `services/sync/mod.rs` (`on_open` → `on_open_with(conn, feature::enabled())`, the
  decision in its doc; tests), `services/sync/feature.rs` (doc), `.github/workflows/ci.yml`

**Interfaces:**
```rust
pub fn on_open(conn: &Connection);                         // on_open_with(conn, feature::enabled())
/// `feature_on` decides only the log line: an enabled profile is captured (or guarded) in every
/// build, so a build without sync never writes it uncaptured (audit M12, ADR 0027). No round, no
/// network: the engine never starts without the flag.
pub(crate) fn on_open_with(conn: &Connection, feature_on: bool);
```
CI, in the Linux `rust` job after "Rust tests":
```yaml
      - name: Release builds ignore the sync dev variables
        working-directory: src-tauri
        env:
          MOONY_DEV_SYNC: "1"
          MOONY_RELAY_URL: "http://127.0.0.1:8787"
        run: cargo test --release --lib services::sync::feature
```

- [ ] **Step 1: Failing tests** (`services/sync/mod.rs`): `a_build_without_sync_still_captures_an_enabled_profile`
  (`on_open_with(conn, false)` → capture installed; an edit → one outbox row),
  `a_build_without_sync_installs_nothing_on_a_profile_without_sync`,
  `a_build_without_sync_guards_an_enabled_profile_whose_capture_fails`.
- [ ] **Step 2: Run** → FAIL (no `on_open_with`). **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; locally `MOONY_DEV_SYNC=1 MOONY_RELAY_URL=http://127.0.0.1:8787 cargo test --release --lib services::sync::feature` → PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): builds without sync keep recording an enabled profile's changes; CI runs the release flag test`.

---

### Task 19: The device binding fails closed (M10)

**Files:**
- Modify: `services/sync/binding.rs` (`BindingFailure::Failed` → `Err`), its tests,
  `engine/tests/runtime.rs` (one test; the file has room)

`startup_check` answers a native `Failed` with
`Err(AppError::Internal("sync binding: could not be checked at this start"))`; the engine start
reports it through `CopyCheckFailure::Check` → `start_failed` (problem `Internal`, no round, the next
trigger tries again); the repair and a resumed setup stop with it before pushing.

- [ ] **Step 1: Failing tests:** flip the binding test that expects `Fine` for `Failed` →
  `a_binding_that_cannot_be_opened_now_is_an_error`; `a_momentary_binding_failure_runs_no_round_and_retries`
  (a native answering `Failed` once, then the right value: the first start makes no relay request,
  the next trigger runs a round).
- [ ] **Step 2: Run** `cargo test --lib binding engine::tests::runtime` → FAIL. **Step 3:
  Implement.** **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): a binding that cannot be checked at this start stops the round instead of passing`.

---

### Task 20: Far-future clocks wait, a device's public key is its own, re-join IDs leave room (M6, L1)

**Files:**
- Create: `src-tauri/src/services/sync/engine/tests/clocks.rs`
- Modify: `chain/mod.rs` (`too_far_ahead`), `changeset.rs` (`Changeset::max_hlc` if absent),
  `engine/pull.rs` (`act`: an `Apply` whose changeset is too far ahead → `OpStep::Hold`,
  `report.ahead += 1`), `engine/mod.rs` (`PullReport.ahead`; counts like `held`), `merge/columns.rs`
  (the `sync_devices.public_key` rule), `chain/rejoin.rs` (`id_above` headroom, the L1 flip),
  `merge/tests/special.rs` (the M6 PoCs: the public-key one flips; the maximum-clock one is removed
  there and replaced at the pull level in `clocks.rs`), `engine/tests/mod.rs`

**Interfaces:**
```rust
pub const MAX_CLOCK_AHEAD_MS: u64 = 24 * 3600 * 1000;
/// The changeset's newest clock is more than MAX_CLOCK_AHEAD_MS past both `now_ms` and this
/// device's newest stored clock (`MAX(hlc) FROM sync_clock`), physical parts compared.
pub fn too_far_ahead(conn: &Connection, newest_hlc: u64, now_ms: u64) -> Result<bool>;
pub const REJOIN_STEP: u128 = 1 << 96;
/// Uniform in (old, min(old + REJOIN_STEP, u128::MAX)]; an error only for u128::MAX.
fn id_above(old: DeviceId, rng: &mut impl Rng) -> Result<DeviceId>;
```
Public key: an incoming non-NULL `sync_devices.public_key` is taken only when its clock's device is
the row's device (`pk == clock.device.to_hex()`); otherwise the local value stays and a merge
warning names the table, never the key.

- [ ] **Step 1: Failing tests:**
  - `a_clock_far_in_the_future_waits_instead_of_pinning` (`clocks.rs`) — B seals an institution
    rename at clock `i64::MAX`; A's pull holds it (`ahead == 1`, `may_push`), A's value unchanged;
    a later normal rename from B also waits (gap); A's own later rename propagates to C.
  - `a_clock_a_little_ahead_applies` — now + 1 h → applied.
  - flip `poc_m6_forged…` → `only_the_device_itself_sets_its_public_key` (the fixture's row ID
    becomes A's hex): B's forged row keeps `"KA"`, a warning recorded; A's own row with clock
    device A applies.
  - `a_rejoined_devices_restamped_key_clock_keeps_the_stored_key`.
  - flip `poc_l1…` → `ten_thousand_rejoins_never_run_out_of_ids` — from `[0x80; 16]`, 10,000 calls,
    each `Ok` and strictly above the last; from `u128::MAX - 1` → `Ok(MAX)`; from `MAX` → `Err`.
- [ ] **Step 2: Run** `cargo test --lib clocks special rejoin` → FAIL. **Step 3: Implement.**
  **Step 4: Run** → PASS; gates.
- [ ] **Step 5: Commit** `fix(sync): far-future clocks wait, public keys come from their own device, re-join IDs leave room`.

---

### Task 21: The Recovery Kit in Rust (M11)

**Files:**
- Create: `src-tauri/src/commands/sync_kit.rs`, `src-tauri/src/services/sync/kit_file.rs` (with tests)
- Modify: `commands/sync.rs` (parameters and the copy fallback only: `sync_copy_recovery_code`
  falls back only on `Unsupported`; `sync_prepare_recovery_kit` takes `password: Option<String>`
  and opens the copy window), `services/sync/setup.rs` (`setup_recovery_kit` gate; turning sync off
  runs `PRAGMA wal_checkpoint(TRUNCATE)` when the journal is WAL), `services/sync/keys.rs`
  (`vault_secrets_exist`), `src-tauri/src/db/mod.rs` (`configure(conn)` extracted from
  `open_with_key`, with `PRAGMA main.secure_delete = ON`), `commands/mod.rs`, `lib.rs`,
  `models/sync.rs`, `bindings.rs`, `shared/schema.ts`, `shared/generated-types.ts`,
  `src/lib/tauri-api.ts`, `src-tauri/src/error.rs` and `src/i18n/locales/<10>/common.json`
  (`validation.syncCopyFailed`)

**Interfaces:**
```rust
#[derive(Deserialize, Type)] pub enum KitFileKind { #[serde(rename = "page")] Page, #[serde(rename = "text")] Text }
#[derive(Serialize, Type)] pub struct KitSaveOutcome { #[serde(rename = "saved")] pub saved: bool,
    #[serde(rename = "syncedFolder")] pub synced_folder: bool }
/// Desktop: allowed when `copy_allowed` allows the copy; Rust opens the native save dialog
/// (tauri-plugin-dialog), writes `contents` (≤ 256 KiB, must contain the kit's recovery code)
/// owner-only and atomically (`crypto::replace_secret_file`), zeroizes it, and says whether the
/// folder looks cloud-synced. A cancelled dialog → `saved: false`.
#[tauri::command] pub async fn sync_save_recovery_kit(app: AppHandle, db: State<'_, Database>, kind: KitFileKind, contents: String) -> Result<KitSaveOutcome>;
#[tauri::command] pub async fn sync_prepare_recovery_kit(app: AppHandle, db: State<'_, Database>, password: Option<String>) -> Result<RecoveryKit>;
// kit_file.rs
pub fn looks_cloud_synced(path: &Path) -> bool;
/// Setup shows the kit without the password only the first time, before its secrets exist.
pub fn kit_needs_password(sync_on: bool, secrets_exist: bool) -> bool;
// commands/sync.rs, pure, tested in place
fn copy_outcome(result: std::result::Result<u64, KeychainError>) -> Result<Option<u64>>;  // Unsupported → Ok(None); other Err → Err(syncCopyFailed)
// db/mod.rs
pub(crate) fn configure(conn: &Connection) -> Result<()>;  // foreign_keys ON, main.secure_delete ON
```
The phone keeps its Files-picker save (`stage_text_export`, iOS sandbox) with the static warning of
Task 22.

- [ ] **Step 1: Failing tests:**
  - `copy_outcome_falls_back_only_when_unsupported`.
  - `cloud_folders_are_recognised` — iCloud Drive (`Library/Mobile Documents/com~apple~CloudDocs`),
    `OneDrive`, `Dropbox`, `Google Drive`/`My Drive`, `Box`, `pCloud Drive`, `Nextcloud` paths
    (case-insensitive, `/` and `\`) true; `~/Documents`, `~/Downloads`, `C:\Users\a\Desktop` false.
  - `the_kit_needs_the_password_once_its_secrets_exist` — table; `setup_recovery_kit` before and
    after `ensure_vault_secrets`.
  - `the_kit_file_is_owner_only` (`#[cfg(unix)]`) — mode `0o600`.
  - `secure_delete_is_on_for_the_main_database` — `configure` on a file database →
    `PRAGMA main.secure_delete` = 1.
  - `turning_sync_off_leaves_no_key_bytes_in_a_plain_database_file` — an unkeyed file database,
    migrated and configured, vault keys holding a marker pattern, turn off → the file's bytes do not
    contain the marker.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; bindings; locale `validation.syncCopyFailed`
  — en "The code couldn't be copied. Save the Kit as a file instead." (cs + eight).
- [ ] **Step 4: Run** → PASS; `npm run typecheck`; gates.
- [ ] **Step 5: Commit** `fix(sync): the Recovery Kit is saved owner-only from Rust, a failed copy is an error, it needs the password once it exists, and its key is deleted securely`.

---

### Task 22: The Recovery Kit in the UI: shown briefly, the clipboard cleared, the synced-folder warning, screen capture off (M11, L16)

**Files:**
- Create: `src/utils/recovery-kit-lifetime.ts`, `src/utils/recovery-kit-lifetime.test.ts`
- Modify: `src/components/settings/sync/{RecoveryKitPanel,ShowRecoveryKitFlow,SyncSetupDialog}.tsx`,
  `src/hooks/use-device-sync-mutations.ts` (reset the kit mutations when the kit goes; prepare with
  a password), `src/mobile/components/sync/{KitBody,SyncKitSheet,SetupSheet}.tsx`,
  `src-tauri/capabilities/default.json` (`core:window:allow-set-content-protected`),
  `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
```ts
export const KIT_VIEW_MS = 5 * 60_000;
export const CLIPBOARD_CLEAR_MS = 60_000;
/** The kit leaves the screen after KIT_VIEW_MS or as soon as the app is hidden. */
export function kitExpired(shownAt: number, now: number, hidden: boolean): boolean;
/** Clear the clipboard only while it still holds exactly the copied code. */
export function shouldClearClipboard(current: string | null, code: string): boolean;
```
Behaviour: the kit (state and mutation data) is dropped when expired or on `visibilitychange` to
hidden; "Show again" asks the password. The webview copy (Linux, Android) clears after
`CLIPBOARD_CLEAR_MS` when `readText()` still equals the code (a failed read keeps today's hint).
Desktop "Save as…" calls `deviceSyncApi.saveRecoveryKit(kind, contents)`; `syncedFolder` shows a
warning toast. A static line on desktop and phone: "Keep the Kit on paper or in a password manager,
not in a folder that syncs to a cloud service." While a kit is on screen,
`getCurrentWindow().setContentProtected(true)`, back to false when it goes (desktop only). Setup
asks the password when `sync_prepare_recovery_kit` answers `validation.syncPasswordRequired`.

- [ ] **Step 1: Failing tests:** `kitExpired` (just before, at, hidden), `shouldClearClipboard`
  (`'ABC','ABC'` true; `''`, `null`, `'abc'`, `'ABC '` false).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; ten locales.
- [ ] **Step 4: Run** `npm test`, typecheck, lint, format.
- [ ] **Step 5: Commit** `fix(sync): the Recovery Kit leaves the screen after five minutes or when the app is hidden, its clipboard copy is cleared, and screen capture is off while it shows`.

---

### Task 23: The unlock password does not stay in mutation state (L16)

**Files:** `src/hooks/use-auth.tsx` (and the phone's lock page if it keeps the password in a
mutation)

The unlock mutation takes no variables: the password is read from a ref in `mutationFn`, which
clears the ref in `finally`; `onSettled` resets the mutation. Hooks are outside the test policy, so
there is no unit test; the reviewer checks that no `useMutation` in `src/hooks/use-auth.tsx` or
`src/mobile/pages/Lock.tsx` keeps a password in `variables` (`grep -n "password" … | grep mutate`).

- [ ] **Step 1: Implement.** **Step 2: Run** `npm run typecheck`, `npm run lint`, `npm test`.
- [ ] **Step 3: Commit** `fix(auth): the unlock password is not kept in mutation state`.

---

### Task 24 (relay, `feat/dev-grant`): the dev grant answers only loopback requests and is off in the example (M13)

**Repo:** `/Users/filipkral/Programování/FinanceApp/Moony-relay`, `git branch --show-current` →
`feat/dev-grant`.

**Files:** `src/dev.ts` (`devGrantEnabled(env, request)`: `DEV_GRANT === "1"` and the request's
host is `localhost`, `127.0.0.1` or `[::1]`), `src/index.ts` (call site), `.dev.vars.example`
(`# DEV_GRANT=1` with a comment: local testing only), `vitest.config.ts` (the self-hosted project
binds `DEV_GRANT: "0"` explicitly), `test/dev-grant.test.ts` (or the existing grant test file),
`DESIGN.md`/`README.md` ("Development only": add `DEV_GRANT=1` to `.dev.vars` yourself; loopback
only)

- [ ] **Step 1: Failing tests:** `answers like an unknown path for a non-loopback host even with
  DEV_GRANT=1` (request URL `https://sync.example.com/v1/dev/grant`); the existing off-state test
  passes with a local `.dev.vars` that sets `DEV_GRANT=1`.
- [ ] **Step 2: Run** `cp .dev.vars.example .dev.vars && echo 'DEV_GRANT=1' >> .dev.vars && npm test`
  → FAIL. **Step 3: Implement.** **Step 4: Run** `npm test` (all pass), `npx tsc --noEmit`.
- [ ] **Step 5: Commit** `fix: the dev grant answers only loopback requests and is off in the example vars`.

Parallel: independent of every app task and of Tasks 11 and 25.

---

### Task 25 (relay, `feat/pairing-ack`): a vault deletion that fails half-way finishes later (L14)

**Repo:** `/Users/filipkral/Programování/FinanceApp/Moony-relay-ack` (after Task 11).

**Files:** `src/vault.ts` (`wipe`: in one synchronous step drop `meta` and write a
`pending_wipe (vault_id, account)` row, then set an alarm before the first `await`; the R2 deletes,
`unregisterVault` and `deleteAll` follow; `alarm()` resumes a pending wipe first), `test/vault.test.ts`,
`DESIGN.md` (deletion is resumable)

- [ ] **Step 1: Failing test:** `a wipe whose snapshot delete fails once finishes on the next alarm
  and frees the account slot` (`runInDurableObject` makes `SNAPSHOTS.delete` throw once; the vault
  answers `vault_not_found` meanwhile; after the alarm no object under `v1/<vault>/` remains and
  the account can create a vault again).
- [ ] **Step 2: Run** `npm test` → FAIL. **Step 3: Implement.** **Step 4: Run** `npm test`,
  `npx tsc --noEmit`.
- [ ] **Step 5: Commit** `fix: a vault deletion interrupted after the record is dropped finishes on the next alarm`.

---

### Task 26: Say what sync cannot do yet; mark quiet devices (H1, M5)

**Files:**
- Modify: `src/utils/sync-devices.ts` (+ test), `src/components/settings/sync/{SyncDevicesCard,RecoveryKitPanel,ShowRecoveryKitFlow,TurnOffSyncDialog}.tsx`,
  `src/mobile/components/sync/{SyncDeviceRows,KitBody}.tsx`, `src/mobile/pages/settings/SyncSection.tsx`
  (the turn-off sheet), `src/i18n/locales/<10>/deviceSync.json`

**Interfaces:**
```ts
export const QUIET_AFTER_DAYS = 30;
/** Whole days since a device was last heard from, when that is more than QUIET_AFTER_DAYS; null otherwise or when unknown. */
export function quietDays(lastActiveAt: number | null, now: number): number | null;
```
Texts (en; cs and eight more): device list and Kit screens — "A lost device or Recovery Kit can't
be cut off from your synced data yet. If one goes missing, delete the synced data and set sync up
again with a new Kit." Turn-off — "Turning sync off here doesn't cut off any other device or Kit."
Quiet device — "Not heard from in {{count}} days" (plural forms).

- [ ] **Step 1: Failing test:** `quietDays` (null → null; 29 days → null; 31 → 31).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; ten locales. **Step 4: Run** `npm test`,
  typecheck, lint, format.
- [ ] **Step 5: Commit** `feat(sync): say that lost devices and Kits cannot be cut off yet, and mark devices not heard from in a month`.

---

### Task 27: Docs

**Files:**
- Create: `docs/architecture/decisions/0027-device-sync-audit-fixes.md`
- Modify: `docs/architecture/decisions/README.md` (row 0027; rows 0024–0026 "amended by 0027"),
  `0024`, `0025`, `0026` (Status line: "Amended by ADR 0027"; in 0026 one marked paragraph
  "Amended 2026-11-12 (ADR 0027)" under **Bytes** with slot 3, OKM 72, the typed code),
  `docs/specs/2026-11-06-device-sync-client-design.md` (§20 "As built (audit fixes, 2026-11-12)"
  and inline "(corrected, see §20)" markers at §2 G3, §9.3 gap and visibility, §11.5, §13),
  `docs/architecture/overview.md` (Device sync: the guard invariant, safe replaces, sync backups in
  Restore, pairing with slot 3 and its relay requirement, how to run the relay for e2e),
  `docs/standards/i18n-glossary.md` (typed code / acknowledgement terms if new)

- [ ] **Step 1: ADR 0027** — Context: the audit. Decision, per finding: the guard invariant (H2,
  rejected diff alternative); attachments carried (H3); backups listed, Files copy, pruning (H4,
  L11); one hold and consent (M8); pairing acknowledgement, slot 3, typed code, the vectors' home,
  the rejected derived-second-mailbox alternative (M1, M2, L8); gap hold, echo without migration,
  back-offs (M3, M4, L2, L3); builds without the flag keep capturing (M12, the audit's alternative
  and why not); fail-closed binding (M10); far-future clocks and the public-key rule (M6); Kit
  handling (M11, L16); relay branches and the redeploy the home server needs before pairing works
  there. Consequences: accepted residuals with reasons — H1 (no revocation until phase 5; the UI
  says so), M5 (a relay can withhold or fork; quiet-device hint only), M6 (a key holder can still
  forge an author; mass deletion unguarded), M10 (a clone with the password can re-seal; Windows
  machine-scope DPAPI), L4, L6, L7, L13, L15, clipboard up to 60 s and none after a crash. The
  hardware checks owed.
- [ ] **Step 2:** spec §20 as-built and the markers; ADR status lines; overview; README index. No
  line numbers or file sizes.
- [ ] **Step 3: Commit** `docs(sync): ADR 0027 audit fixes, corrected spec and ADR claims, overview`.

---

### Task 28: End to end against `wrangler dev`; the hardware checks still owed

**Files:**
- Modify: `src-tauri/src/services/sync/relay_e2e_pairing.rs`, `src-tauri/src/services/sync/relay_e2e.rs`,
  `.superpowers/sdd/2026-11-12-device-sync-audit-fixes/progress.md` (results, not committed)

**Relay:** in `/Users/filipkral/Programování/FinanceApp/Moony-relay-ack`:
`git switch -c e2e/audit-fixes feat/pairing-ack && git merge --no-edit feat/dev-grant`,
`cp .dev.vars.example .dev.vars && echo 'DEV_GRANT=1' >> .dev.vars && npm run dev`
(127.0.0.1:8787). After the run: `git switch feat/pairing-ack && git branch -D e2e/audit-fixes`.

```rust
#[tokio::test] #[ignore = "needs wrangler dev of Moony-relay: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn the_mailbox_has_three_slots()                          // put/take 3 once; slot 4 → bad_slot
async fn a_phone_joins_a_desktop_that_types_its_code()          // holder shows; joiner scans; ack; holder confirms with the joiner's digits; join; converged
async fn a_desktop_joins_from_a_phone_and_the_phone_types_the_code()
async fn three_wrong_codes_end_the_pairing_and_nothing_is_sealed()
async fn a_resync_pushes_unsent_changes_first()                 // B's unsent rows on the relay under the old ID, then on B again
async fn an_envelope_stays_until_it_comes_back()                // sync_unsent non-empty after push, empty after the next pull
```
The existing phase 2 and phase 3 e2e tests are updated to the new signatures and the typed confirm.

- [ ] **Step 1: Write the tests.**
- [ ] **Step 2: Run** `cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e -- --ignored --test-threads=1`
  → all PASS (a failure is a bug in its owning task: fix it there with a regression test and commit
  that fix separately).
- [ ] **Step 3: Run** `cargo test --lib`, `cargo test --release --lib` (the e2e modules absent).
- [ ] **Step 4: Record** in `progress.md` the results and the hardware checks still owed: one full
  Windows join (M9's flush, Task 5); the error a binding blob from another Mac or iPhone returns
  (M10, L13); a disk or VM clone against the Windows binding (M10); concealed clipboard types against
  the current macOS clipboard history (M11); the iPhone's Files copy of a sync backup (H4);
  content protection on macOS and Windows (Task 22); pairing with the phone's camera and the typed
  code on the owner's iPhone (controller step, owner's go-ahead); the home server
  `sync.home.example.com` redeployed with slot 3 before pairing works against it (owner).
- [ ] **Step 5: Commit** `test(sync): audit fixes end to end through wrangler dev`.

---

## Order and parallel work

Sequential by default, in task order. The relay tasks are the safe parallel work: Task 11 and
Task 24 run alongside any app task (different repositories); Task 25 follows Task 11 on the same
branch.

App tasks with disjoint files and no locale file (one crate, so two of them in one working tree
break each other's builds mid-edit; run each in its own temporary worktree off the branch tip and
cherry-pick, or run them one after the other):
- After Task 2: Tasks 3, 4 and 10 touch disjoint files; Task 5 follows Task 3 (both declare in
  `services/sync/mod.rs`).
- Tasks 18 and 19 touch disjoint files (after Task 17).
- Task 23 (frontend only, `src/hooks/use-auth.tsx`) can run beside any Rust-only task in the same
  tree, since it does not affect the Rust build.
Everything else (shared `join.rs`, `engine/*`, `commands/*`, wire types, locales) stays sequential.
