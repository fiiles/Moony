//! The second sync audit (2026-11-20), finding R2-H1: the remover signs the head of its key change
//! as the sync server reports it and never compares it with what it saw itself. A server that
//! works with a removed device raises that head for the remover, keeps the positions below it
//! free, and later fills them with what the removed device seals under the old key. The signed
//! head then confirms the server's number on every device, so the removed device's writes are
//! applied by devices that were fully up to date when it was removed, and by the remover itself.
//!
//! `…_is_not_applied_by_the_remover` is the finding as a test of what must hold on the remover (spec
//! `docs/specs/2026-11-20-sync-audit-2-fixes-design.md` §2.3 steps 1–6); the same test for another
//! device that was up to date now runs too (step 7 is built: every device checks the range). The
//! control next to them shows the same write refused when the server reports the true head. The
//! R2-H3 test is the same harm through a snapshot, found while that spec was reviewed. The rest
//! are the remover's own behaviour: it signs what it found, and holds still until it has.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use rusqlite::Connection;

use super::pull::write_and_seal;
use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::changeset;
use crate::services::sync::engine::snapshots::{heal_missed, SnapshotPolicy};
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::envelope::stream::{snapshot_object_id, StreamSealer};
use crate::services::sync::envelope::{envelope_hash, open_op, vault_token};
use crate::services::sync::error::SyncError;
use crate::services::sync::keys;
use crate::services::sync::rekey::head::{self, HeadRecord};
use crate::services::sync::rekey::range::{self, RangeNote, RangeRead};
use crate::services::sync::rekey::rotate::{self, Outcome};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;

pub(super) fn last_seq(three: &Three) -> u64 {
    three.vault.relay.ops().last().map_or(0, |op| op.seq)
}

/// Where epoch 1 started, as the remover A recorded and signed it.
fn signed_head(three: &Three) -> u64 {
    *state::epochs(&three.vault.a.lock())
        .expect("epochs")
        .get(&1)
        .expect("A's head")
}

/// C, removed but still holding the old key and token, writes `id`; a server working with it
/// takes the write.
async fn write_from_removed_c(three: &Three, id: &str) -> u64 {
    three.vault.relay.set_accept_retired_writes(true);
    let sealed = write_and_seal(&three.vault, &three.c, id);
    let id = crate::services::sync::envelope::op_id(&sealed);
    push(&three.vault.relay, &three.vault.creds, &[sealed]).await;
    three
        .vault
        .relay
        .ops()
        .iter()
        .find(|op| op.id == id)
        .expect("the server took it")
        .seq
}

/// A few rounds of the devices that stayed: A sends its signed head, B picks the key up and
/// verifies the head, both pull to the end.
async fn stayers_sync(three: &Three) -> Vec<Option<SyncProblem>> {
    let mut problems = Vec::new();
    for _ in 0..3 {
        for id in [A, B] {
            problems.push(round_of(&three.vault, three.device(id)).await.problem);
        }
    }
    problems
}

/// Every device has pulled everything the server holds: nobody is "behind" (ADR 0032 R4).
fn assert_everyone_is_up_to_date(three: &Three) -> u64 {
    let end = last_seq(three);
    for id in [A, B, C] {
        assert_eq!(
            cursor(&three.device(id).lock()),
            end,
            "{id:?} is up to date"
        );
    }
    end
}

/// The sync problems each of the devices that stayed showed, in the order of the rounds.
async fn stayers_sync_by_device(
    three: &Three,
) -> (Vec<Option<SyncProblem>>, Vec<Option<SyncProblem>>) {
    let (mut of_a, mut of_b) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        of_a.push(round_of(&three.vault, &three.vault.a).await.problem);
        of_b.push(round_of(&three.vault, &three.vault.b).await.problem);
    }
    (of_a, of_b)
}

fn unsent_envelopes(conn: &Connection) -> Vec<Vec<u8>> {
    conn.prepare("SELECT envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| r.get(0))
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect()
}

/// The signed head record `id` holds in `sync_unsent` (its first envelope under the new key).
fn queued_head(three: &Three, id: DeviceId) -> Option<HeadRecord> {
    let conn = three.device(id).lock();
    let creds = keys::credentials(&conn).expect("creds").expect("bound");
    unsent_envelopes(&conn).iter().find_map(|envelope| {
        let json = open_op(|e| creds.keys.get(e), &creds.vault_id.0, envelope).ok()?;
        let cs = changeset::from_json(&json).ok()?;
        HeadRecord::decode(cs.rekey_head.as_deref()?).ok()
    })
}

/// What `conn` noted for the range of `epoch`: position and hash, ascending.
fn range_notes(conn: &Connection, epoch: u32) -> Vec<(u64, Vec<u8>)> {
    conn.prepare("SELECT pos, hash FROM sync_range_notes WHERE epoch = ?1 ORDER BY pos")
        .expect("notes")
        .query_map([epoch], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?)))
        .expect("rows")
        .map(|row| row.expect("row"))
        .collect()
}

fn unverified_of(conn: &Connection, epoch: u32) -> Option<head::UnverifiedEpoch> {
    head::unverified_epochs(conn)
        .expect("unverified")
        .into_iter()
        .find(|e| e.epoch == epoch)
}

/// What a change of A's cursor to the head has to note: every op of the server's log in
/// `(base, head]`, as position and envelope hash.
fn ops_between(three: &Three, base: u64, head: u64) -> Vec<(u64, Vec<u8>)> {
    three
        .vault
        .relay
        .ops()
        .iter()
        .filter(|op| op.seq > base && op.seq <= head)
        .map(|op| (op.seq, envelope_hash(&op.ct).to_vec()))
        .collect()
}

fn digest_of(three: &Three, base: u64, head: u64, notes: &[(u64, Vec<u8>)]) -> [u8; 32] {
    let notes: Vec<RangeNote> = notes
        .iter()
        .map(|(pos, hash)| RangeNote {
            pos: *pos,
            hash: hash.as_slice().try_into().expect("32 bytes"),
        })
        .collect();
    range::range_digest(&three.vault.creds.vault_id.0, 1, base, head, &notes).expect("digest")
}

/// A round whose result the test does not need (it may be an error: the server is made to fail).
async fn round_ignoring_errors(three: &Three, id: DeviceId) {
    let _ = run_round(
        &three.vault.relay,
        three.device(id),
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;
}

/// R2-H1, the remover's half. What must hold: the server lifts the head and keeps the positions
/// below it free; the remover reads them, finds nothing and signs "empty" (`base == end`,
/// `count == 0`); what the removed device seals there later has no note, so the remover does not
/// apply it. It reports `InvalidChain`: an old-key op at a position of the range that nothing was
/// noted for is not one the remover signed, and an honest server never makes one.
#[tokio::test]
async fn a_removed_devices_write_below_a_head_the_server_raised_is_not_applied_by_the_remover() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);

    // The server lies to the remover: the key change is reported 8 positions above the real end.
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    assert_eq!(
        signed_head(&three),
        end + 8,
        "the server's number is recorded"
    );
    assert!(
        range_notes(&three.vault.a.lock(), 1).is_empty(),
        "A found nothing in the range"
    );
    let record = queued_head(&three, A).expect("A signed its head inside the call");
    assert_eq!((record.base, record.head, record.count), (end, end + 8, 0));
    assert_eq!(
        record.digest,
        digest_of(&three, end, end + 8, &[]),
        "the digest of an empty range"
    );

    // After the removal, C writes; the server gives it a position it kept free.
    let at = write_from_removed_c(&three, "c-after-removal").await;
    assert!(at > end && at <= end + 8, "below the raised head: {at}");

    let (of_a, _) = stayers_sync_by_device(&three).await;

    assert!(
        !has_institution(&three.vault.a.lock(), "c-after-removal"),
        "the remover applied the removed device's later write: {of_a:?}"
    );
    assert!(
        of_a.contains(&Some(SyncProblem::InvalidChain)),
        "the remover says so: {of_a:?}"
    );
}

/// R2-H1, the half of a device that was up to date at the removal. B applies nothing, but at the
/// time of Task 4 not for the reason that matters: A stops at the op without a note and never sends
/// its signed head, so B waits for it for good. The test that shows B's own check is
/// `other_contents_for_another_device_refute_the_head` (the server shows the removed device's op
/// to B only, A signs an empty range and does send its record, and B must refute it); this one
/// stays as the finding itself, with the real log.
#[tokio::test]
async fn a_removed_devices_write_below_a_head_the_server_raised_is_not_applied_by_a_device_that_was_up_to_date(
) {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    let at = write_from_removed_c(&three, "c-after-removal").await;
    assert!(at > end && at <= end + 8, "below the raised head: {at}");

    let (of_a, of_b) = stayers_sync_by_device(&three).await;

    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1, "B took the new key");
    assert!(
        !has_institution(&b, "c-after-removal"),
        "up to date at the removal, yet applied the removed device's later write \
         (problems: A {of_a:?}, B {of_b:?})"
    );
}

/// C, removed, writes a row and seals a snapshot of its profile under the old key (epoch 0), claiming
/// relay position `at`. (A dump refuses while anything waits to be sent, so the row is sealed into
/// C's next envelope first and that envelope dropped; it is never sent.)
pub(super) fn forged_old_key_snapshot(three: &Three, at: u64) -> Vec<u8> {
    write_and_seal(&three.vault, &three.c, "c-in-snapshot");
    let c = three.c.lock();
    exec(&c, "DELETE FROM sync_unsent");
    let mut plain = Vec::new();
    snapshot::dump(&c, at, &mut plain).expect("dump");
    let old = &three.vault.creds;
    assert_eq!(old.keys.current_epoch(), 0);
    let mut deflate = DeflateEncoder::new(
        StreamSealer::new(
            0,
            old.keys.current(),
            &old.vault_id.0,
            &snapshot_object_id(at),
        ),
        Compression::default(),
    );
    deflate.write_all(&plain).expect("deflate");
    deflate.finish().expect("deflate").finish().expect("seal")
}

/// The server takes `sealed` as the vault's snapshot at `at` (a relay that works with the removed
/// device accepts the retired token's upload).
pub(super) async fn server_takes_snapshot(three: &Three, at: u64, sealed: Vec<u8>) {
    three.vault.relay.set_accept_retired_writes(true);
    three
        .vault
        .relay
        .put_snapshot(&three.vault.creds.auth(), at, sealed)
        .await
        .expect("the server takes it");
}

/// A round of `id` that may fail: the result as it is.
async fn round_result(
    three: &Three,
    id: DeviceId,
) -> crate::services::sync::error::SyncResult<crate::services::sync::engine::RoundReport> {
    run_round(
        &three.vault.relay,
        three.device(id),
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
}

/// `conn` is put back to where a device behind by `to` would stand: its cursor, the notes of
/// handled positions and the position of the last restore. What it has already verified stays.
fn lag_behind(conn: &Connection, to: u64) {
    exec(
        conn,
        &format!("UPDATE sync_state SET cursor = {to}, restored_seq = NULL"),
    );
    exec(conn, "DELETE FROM sync_seen_ops");
}

fn invalid_key(result: &crate::services::sync::error::SyncResult<impl std::fmt::Debug>) -> bool {
    matches!(result, Err(error) if super::kit_join::key_of(error).as_deref() == Some("validation.syncSnapshotInvalid"))
}

fn vault_b64(three: &Three) -> String {
    three.vault.creds.vault_id.to_b64()
}

/// The token A reads with now (the vault's current one, after a key change).
fn new_token(three: &Three) -> String {
    let creds = keys::credentials(&three.vault.a.lock())
        .expect("creds")
        .expect("bound");
    vault_token(creds.keys.current(), &creds.vault_id.0)
}

/// Pushes what A holds unsent (its signed head, sealed under the new key) with its new token,
/// without a round.
pub(super) async fn push_a_record(three: &Three) {
    let (creds, envelopes) = {
        let a = three.vault.a.lock();
        (
            keys::credentials(&a).expect("creds").expect("bound"),
            unsent_envelopes(&a),
        )
    };
    assert!(!envelopes.is_empty(), "A holds its signed head");
    push(&three.vault.relay, &creds, &envelopes).await;
}

/// Puts the cursor of `id` back to `to`, as a device whose cursor was held back has it.
fn rewind(three: &Three, id: DeviceId, to: u64) {
    exec(
        &three.device(id).lock(),
        &format!("UPDATE sync_state SET cursor = {to}"),
    );
}

/// §2.3 step 7, the honest case: B was behind the removal (its place lies below the head). It
/// reads the range from the signed base itself, finds the number and the digest A signed, keeps
/// the very notes A has and only then applies what lies in the range.
#[tokio::test]
async fn a_device_checks_the_range_before_it_takes_the_head() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    for id in ["c-1", "c-2"] {
        institution(&three.c.lock(), id);
        round_of(&three.vault, &three.c).await;
    }
    let head = last_seq(&three);
    assert!(head >= base + 2);
    assert_eq!(cursor(&three.vault.b.lock()), base, "B is behind");
    remove(&three, A, &[C]).await.expect("removed");
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    assert!(unverified_of(&b, 1).is_none(), "the head is taken");
    assert_eq!(range::base_of(&b, 1).expect("base"), Some(base));
    let notes = range_notes(&b, 1);
    assert_eq!(notes, ops_between(&three, base, head));
    assert_eq!(notes, range_notes(&three.vault.a.lock(), 1), "A's notes");
    assert!(has_institution(&b, "c-1") && has_institution(&b, "c-2"));
}

/// §2.3 step 7, the lie: the removed device's op is shown to B alone, at a free position of a
/// range the server lifted. A found nothing there and signed "empty" (and does send its record);
/// B reads the range itself, finds one op more and refutes the head: `keyChangeUnverified`, no note
/// kept, nothing of the range applied.
#[tokio::test]
async fn other_contents_for_another_device_refute_the_head() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    let record = queued_head(&three, A).expect("A signed its head");
    assert_eq!((record.base, record.head, record.count), (end, end + 8, 0));
    push_a_record(&three).await;
    let forged = write_and_seal(&three.vault, &three.c, "c-for-b");
    three.vault.relay.serve_instead(
        &vault_b64(&three),
        end + 1,
        forged,
        Some(&new_token(&three)),
    );

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(
        report.problem,
        Some(SyncProblem::KeyChangeUnverified),
        "{report:?}"
    );
    let b = three.vault.b.lock();
    assert!(!has_institution(&b, "c-for-b"), "nothing of the range");
    assert!(range_notes(&b, 1).is_empty());
    assert_eq!(range::base_of(&b, 1).expect("base"), None);
    assert!(unverified_of(&b, 1).is_some_and(|e| e.refuted));
    assert!(cursor(&b) <= end);
}

/// Rule 6 after the check: both devices hold the notes; the server then serves another op at a
/// noted position to a device that reads it again (its cursor held back). The pull stops there as
/// `Invalid` and applies nothing of it.
#[tokio::test]
async fn an_op_replaced_in_the_range_after_verification_stops_the_pull() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    institution(&three.c.lock(), "c-1");
    round_of(&three.vault, &three.c).await;
    remove(&three, A, &[C]).await.expect("removed");
    for id in [A, B] {
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(report.problem, None, "{id:?}: {report:?}");
    }
    let (noted_at, _) = range_notes(&three.vault.b.lock(), 1)
        .into_iter()
        .next()
        .expect("a note");
    assert_eq!(
        range_notes(&three.vault.a.lock(), 1),
        range_notes(&three.vault.b.lock(), 1)
    );
    let forged = write_and_seal(&three.vault, &three.c, "c-replaces");
    three
        .vault
        .relay
        .serve_instead(&vault_b64(&three), noted_at, forged, None);

    for id in [A, B] {
        rewind(&three, id, base);
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(
            report.problem,
            Some(SyncProblem::InvalidChain),
            "{id:?}: {report:?}"
        );
        assert!(
            !has_institution(&three.device(id).lock(), "c-replaces"),
            "{id:?}"
        );
    }
}

/// Rule 6, the other way: an op at a position of the range where nothing was noted (the server
/// kept it free, then filled it) ends the pull on the remover and on a device that checked the
/// empty range.
#[tokio::test]
async fn an_op_at_an_unnoted_position_of_the_range_stops_the_pull() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(3);
    remove(&three, A, &[C]).await.expect("removed");
    for id in [A, B] {
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(report.problem, None, "{id:?}: {report:?}");
        assert!(range_notes(&three.device(id).lock(), 1).is_empty());
    }
    let forged = write_and_seal(&three.vault, &three.c, "c-late");
    three
        .vault
        .relay
        .serve_instead(&vault_b64(&three), end + 1, forged, None);

    for id in [A, B] {
        rewind(&three, id, end);
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(
            report.problem,
            Some(SyncProblem::InvalidChain),
            "{id:?}: {report:?}"
        );
        assert!(
            !has_institution(&three.device(id).lock(), "c-late"),
            "{id:?}"
        );
    }
}

/// A device whose place is at the head when the record is found has nothing to check: no read of
/// the range, no notes, no base. It goes on with what comes above the head.
#[tokio::test]
async fn a_device_already_past_the_head_checks_nothing_and_goes_on() {
    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    {
        let b = three.vault.b.lock();
        assert!(unverified_of(&b, 1).is_none(), "the head is taken");
        assert_eq!(range::base_of(&b, 1).expect("base"), None);
        assert!(range_notes(&b, 1).is_empty());
    }
    institution(&three.vault.a.lock(), "a-after");
    round_of(&three.vault, &three.vault.a).await;
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(has_institution(&three.vault.b.lock(), "a-after"));
}

/// A device that has applied part of the range reads it from the signed base again, not from its
/// own place: its notes cover the positions below its place too.
#[tokio::test]
async fn a_device_between_base_and_head_reads_from_the_base_again() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    institution(&three.vault.b.lock(), "b-1");
    round_of(&three.vault, &three.vault.b).await;
    for id in ["c-1", "c-2"] {
        institution(&three.c.lock(), id);
        round_of(&three.vault, &three.c).await;
    }
    let head = last_seq(&three);
    let place = cursor(&three.vault.b.lock());
    assert!(place > base && place < head, "{base} < {place} < {head}");
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    let b = three.vault.b.lock();
    assert_eq!(range::base_of(&b, 1).expect("base"), Some(base));
    let notes = range_notes(&b, 1);
    assert_eq!(notes, ops_between(&three, base, head));
    assert!(
        notes.iter().any(|(pos, _)| *pos <= place),
        "below its place"
    );
    assert!(has_institution(&b, "c-1") && has_institution(&b, "c-2"));
}

/// The server pruned the range (a snapshot under the new key lies above it): B cannot read it, and
/// the snapshot, which names the head, settles it as before. No notes are kept.
#[tokio::test]
async fn a_pruned_range_is_settled_by_a_new_key_snapshot() {
    let three = three().await;
    institution(&three.c.lock(), "c-1");
    round_of(&three.vault, &three.c).await;
    remove(&three, A, &[C]).await.expect("removed");
    let head = signed_head(&three);
    round_of(&three.vault, &three.vault.a).await;
    let seq = three.vault.relay.snapshot_seq().expect("A's snapshot");
    assert!(seq > head, "{seq} {head}");
    three.vault.relay.prune_to(seq);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert_eq!(report.problem, None, "{report:?}");
    assert!(report.restored, "{report:?}");
    let b = three.vault.b.lock();
    assert!(unverified_of(&b, 1).is_none(), "settled by the snapshot");
    assert_eq!(range::base_of(&b, 1).expect("base"), None);
    assert!(range_notes(&b, 1).is_empty());
    assert!(has_institution(&b, "c-1"), "A's snapshot has it");
}

/// An honest server: the head is verified and signed inside the removal call. Nothing waits: no
/// unverified entry, the signed head is the only envelope A has unsent, it is sealed under the new
/// key, and its digest is that of the (empty) range.
#[tokio::test]
async fn an_honest_removal_signs_its_head_inside_the_call() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);

    let outcome = remove(&three, A, &[C]).await.expect("removed");

    assert!(
        matches!(outcome, Outcome::Changed { epoch: 1, .. }),
        "{outcome:?}"
    );
    let a = three.vault.a.lock();
    assert!(
        head::unverified_epochs(&a).expect("unverified").is_empty(),
        "verified inside the call"
    );
    assert!(!head::own_unverified(&a).expect("own"));
    assert!(!rotation_pending(&a));
    let epochs: Vec<u32> = unsent_epochs(&a).into_iter().map(|(_, e)| e).collect();
    assert_eq!(epochs, vec![1], "the signed head only, under the new key");
    assert_eq!(
        range::base_of(&a, 1).expect("base"),
        Some(end),
        "the stored range starts at A's cursor"
    );
    assert_eq!(
        state::epochs(&a).expect("epochs").get(&1),
        Some(&end),
        "the honest head is the end of the log"
    );
    drop(a);
    let record = queued_head(&three, A).expect("the signed head");
    assert_eq!(
        (record.signer, record.from_epoch, record.to_epoch),
        (A, 0, 1)
    );
    assert_eq!((record.base, record.head, record.count), (end, end, 0));
    assert_eq!(record.digest, digest_of(&three, end, end, &[]));
}

/// A remover that was behind notes every op between its cursor and the head, signs them, and then
/// applies them through the notes: its ordinary pull takes exactly those.
#[tokio::test]
async fn a_remover_that_was_behind_notes_every_op_between_its_cursor_and_the_head() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    for id in ["b-1", "b-2"] {
        institution(&three.vault.b.lock(), id);
        round_of(&three.vault, &three.vault.b).await;
    }
    institution(&three.c.lock(), "c-1");
    round_of(&three.vault, &three.c).await;
    let head = last_seq(&three);
    assert!(head >= base + 3, "A is three or more positions behind");
    assert_eq!(cursor(&three.vault.a.lock()), base);

    remove(&three, A, &[C]).await.expect("removed");

    let expected = ops_between(&three, base, head);
    {
        let a = three.vault.a.lock();
        assert_eq!(range_notes(&a, 1), expected);
        assert_eq!(range::base_of(&a, 1).expect("base"), Some(base));
        assert_eq!(cursor(&a), base, "the read applied nothing");
        assert!(!has_institution(&a, "b-1"), "and opened nothing");
    }
    let record = queued_head(&three, A).expect("the signed head");
    assert_eq!((record.base, record.head), (base, head));
    assert_eq!(record.count as usize, expected.len());
    assert_eq!(record.digest, digest_of(&three, base, head, &expected));

    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    for id in ["b-1", "b-2", "c-1"] {
        assert!(has_institution(&a, id), "{id} was noted and is applied");
    }
}

/// An op of the removed device offered at a position where another op was noted is not applied:
/// `Invalid`, and the cursor stays where it was.
#[tokio::test]
async fn an_op_that_is_not_the_one_noted_at_its_position_stops_the_pull() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    institution(&three.vault.b.lock(), "b-1");
    round_of(&three.vault, &three.vault.b).await;
    let head = last_seq(&three);
    remove(&three, A, &[C]).await.expect("removed");
    let (noted_at, _) = range_notes(&three.vault.a.lock(), 1)
        .into_iter()
        .next()
        .expect("a note");
    assert!(noted_at > base && noted_at <= head);

    // Another op of the removed device, offered at the noted position.
    let forged = write_and_seal(&three.vault, &three.c, "c-replaces");
    let op = crate::services::sync::relay::PulledOp {
        seq: noted_at,
        id: crate::services::sync::envelope::op_id(&forged),
        ct: forged,
    };
    let cursor_before = cursor(&three.vault.a.lock());
    let creds = keys::credentials(&three.vault.a.lock())
        .expect("creds")
        .expect("bound");
    let mut report = crate::services::sync::engine::PullReport::default();
    let step = super::super::pull::handle_op(&three.vault.a.lock(), &creds, &op, NOW, &mut report)
        .expect("judged");

    assert_eq!(
        step,
        super::super::pull::OpStep::Stop(crate::services::sync::engine::PullStop::Invalid)
    );
    assert!(!has_institution(&three.vault.a.lock(), "c-replaces"));
    assert_eq!(cursor(&three.vault.a.lock()), cursor_before);
}

/// `read_range`: the ops of `(base, head]` as position and hash, none above the head, no call for
/// an empty range, the page limit as "did not finish", and `410` as it is.
#[tokio::test]
async fn read_range_notes_what_the_server_holds_up_to_the_head_and_no_further() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    for id in ["b-1", "b-2", "b-3"] {
        institution(&three.vault.b.lock(), id);
        round_of(&three.vault, &three.vault.b).await;
    }
    let end = last_seq(&three);
    let auth = three.vault.creds.auth();
    let relay = &three.vault.relay;
    let all = ops_between(&three, base, end);
    assert!(all.len() >= 3);
    let found = |read: RangeRead| match read {
        RangeRead::Complete(notes) => notes
            .iter()
            .map(|n| (n.pos, n.hash.to_vec()))
            .collect::<Vec<_>>(),
        other => panic!("not complete: {other:?}"),
    };

    let whole = range::read_range(relay, &auth, base, end, 10)
        .await
        .expect("read");
    assert_eq!(found(whole), all);

    // A head inside the log: what lies above it is not noted.
    let mid = all[1].0;
    let part = range::read_range(relay, &auth, base, mid, 10)
        .await
        .expect("read");
    assert_eq!(found(part), all[..2].to_vec());

    // An empty range reads nothing.
    let calls = relay.calls().len();
    let none = range::read_range(relay, &auth, end, end, 10)
        .await
        .expect("read");
    assert_eq!(found(none), Vec::new());
    assert_eq!(relay.calls().len(), calls, "no call for an empty range");

    // The page limit comes first: nothing is kept.
    let more = range::read_range(relay, &auth, base, end, 0)
        .await
        .expect("read");
    assert_eq!(more, RangeRead::More { scanned: base });

    // The server pruned above the base: `410`, returned as it is.
    relay
        .put_snapshot(&auth, end, vec![7; 64])
        .await
        .expect("a snapshot");
    relay.prune_to(end);
    let pruned = range::read_range(relay, &auth, base, end, 10).await;
    assert!(
        matches!(pruned, Err(SyncError::SnapshotRequired)),
        "{pruned:?}"
    );
}

/// While its own head is not signed the remover starts no other removal or Kit and its blob
/// lane is held. Here the server answers no pull at all, so its rounds stop at the pull: this
/// test does not show the gate in the round (the next one does), only that nothing goes out and
/// that the state is held.
#[tokio::test]
async fn a_remover_whose_pulls_all_fail_sends_nothing_and_starts_nothing_new() {
    let three = three().await;
    // B has written what A has not pulled: A's range is not empty, so there is something to read.
    institution(&three.vault.b.lock(), "b-ahead");
    round_of(&three.vault, &three.vault.b).await;
    let base = cursor(&three.vault.a.lock());
    let end = last_seq(&three);
    assert!(end > base);
    three
        .vault
        .relay
        .set_failing(Route::Pull, Some(RelayError::Network("offline".into())));

    let outcome = remove(&three, A, &[C]).await.expect("the key changed");

    assert!(
        matches!(outcome, Outcome::Changed { epoch: 1, .. }),
        "{outcome:?}"
    );
    {
        let a = three.vault.a.lock();
        let waiting = unverified_of(&a, 1).expect("its own head is not verified");
        assert_eq!(waiting.by, A.to_hex());
        assert_eq!(waiting.head, Some(end));
        assert_eq!(waiting.after, base, "the base");
        assert!(head::own_unverified(&a).expect("own"));
        assert!(
            crate::services::sync::engine::runtime::uploads_held(&a).expect("held"),
            "the blob lane waits too"
        );
        assert!(unsent_envelopes(&a).is_empty(), "no signed head yet");
        institution(&a, "a-while-unverified");
    }
    let before = three.vault.relay.ops().len();
    let from = three.vault.relay.calls().len();
    for _ in 0..3 {
        round_ignoring_errors(&three, A).await;
    }
    let calls = three.vault.relay.calls()[from..].to_vec();
    assert!(!calls.contains(&Route::Push), "{calls:?}");
    assert!(!calls.contains(&Route::PutSnapshot), "{calls:?}");
    assert_eq!(
        three.vault.relay.ops().len(),
        before,
        "the server holds no new op"
    );

    let a = three.vault.a.lock();
    let error = rotate::plan(&a, &[B], &[], false, NOW).expect_err("a second removal");
    assert!(
        error.to_string().contains("syncChangeInProgress"),
        "{error}"
    );
    let error = rotate::plan(&a, &[], &[], true, NOW).expect_err("a new Kit");
    assert!(
        error.to_string().contains("syncChangeInProgress"),
        "{error}"
    );
}

/// The same, with a server that answers every pull but the range read: the round pulls, finds its
/// own head still unsigned and sends nothing, seals nothing again and uploads no snapshot; the
/// round after finishes the signature and then sends.
#[tokio::test]
async fn the_remover_holds_still_in_a_round_that_pulls_but_could_not_read_its_range() {
    let three = three().await;
    institution(&three.vault.b.lock(), "b-ahead");
    round_of(&three.vault, &three.vault.b).await;
    // The read inside the call, and the one at the start of the first round.
    for _ in 0..2 {
        three
            .vault
            .relay
            .fail_next(Route::Pull, RelayError::Network("offline".into()));
    }
    remove(&three, A, &[C]).await.expect("the key changed");
    assert!(head::own_unverified(&three.vault.a.lock()).expect("own"));
    institution(&three.vault.a.lock(), "a-after");
    let from = three.vault.relay.calls().len();
    let before = three.vault.relay.ops().len();

    let report = round_of(&three.vault, &three.vault.a).await;

    let calls = three.vault.relay.calls()[from..].to_vec();
    assert!(calls.contains(&Route::Pull), "the round pulled: {calls:?}");
    assert!(!calls.contains(&Route::Push), "{calls:?}");
    assert!(!calls.contains(&Route::PutSnapshot), "{calls:?}");
    assert_eq!(report.problem, Some(SyncProblem::KeyChangeWaiting));
    assert!(head::own_unverified(&three.vault.a.lock()).expect("own"));
    assert!(
        crate::services::sync::engine::runtime::uploads_held(&three.vault.a.lock()).expect("held"),
        "the blob lane is held while the round is"
    );

    let report = round_of(&three.vault, &three.vault.a).await;

    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    assert!(!head::own_unverified(&a).expect("own"));
    assert!(!crate::services::sync::engine::runtime::uploads_held(&a).expect("held"));
    drop(a);
    assert!(
        three.vault.relay.ops().len() > before,
        "the signed head and the row went out"
    );
}

/// Review of Task 4, finding 1: another device changes the key while the remover's head is
/// still unsigned (its range read kept failing). The remover picks the second change up, and its
/// next read works: it signs its head (sealed under the current key, which is above the record's
/// epoch), the record is found and its head verified by an up to date device, and none of the
/// remover's rounds fails on the way.
#[tokio::test]
async fn a_second_key_change_before_the_signature_does_not_wedge_the_remover() {
    let three = three().await;
    institution(&three.vault.b.lock(), "b-ahead");
    round_of(&three.vault, &three.vault.b).await;
    three
        .vault
        .relay
        .set_failing(Route::Pull, Some(RelayError::Network("offline".into())));
    remove(&three, A, &[C]).await.expect("the key changed");
    assert!(head::own_unverified(&three.vault.a.lock()).expect("own"));
    three.vault.relay.set_failing(Route::Pull, None);
    // B picks A's change up (A's head is not signed yet). An honest B then refuses to change the
    // key itself (task 8: a device with an unverified head starts no change); a B that does not
    // hold that gate (an older build) is made by taking its note of A's head away for the call
    // and putting it back, so that it still has to verify the head afterwards.
    round_of(&three.vault, &three.vault.b).await;
    let waiting = {
        let b = three.vault.b.lock();
        let waiting = unverified_of(&b, 1).expect("B waits for A's head");
        let error = rotate::plan(&b, &[], &[], true, NOW).expect_err("refused");
        assert!(
            error.to_string().contains("syncChangeInProgress"),
            "{error}"
        );
        exec(
            &b,
            &format!(
                "DELETE FROM app_config WHERE key = '{}'",
                state::UNVERIFIED_EPOCHS_KEY
            ),
        );
        waiting
    };
    new_kit(&three, B).await.expect("B changes the key");
    assert_eq!(key_epoch(&three.vault.b.lock()), 2);
    head::note_unverified(
        &three.vault.b.lock(),
        waiting.epoch,
        waiting.head,
        waiting.rotation,
        DeviceId::from_hex(&waiting.by).expect("device"),
        waiting.after,
    )
    .expect("B waits for A's head again");

    let mut problems = Vec::new();
    for _ in 0..4 {
        problems.push(round_of(&three.vault, &three.vault.a).await.problem);
        problems.push(round_of(&three.vault, &three.vault.b).await.problem);
    }

    let a = three.vault.a.lock();
    assert_eq!(
        key_epoch(&a),
        2,
        "A picked the second change up: {problems:?}"
    );
    assert!(
        !head::own_unverified(&a).expect("own"),
        "A signed its head: {problems:?}"
    );
    assert!(!range_notes(&a, 1).is_empty(), "with the range it read");
    drop(a);
    let b = three.vault.b.lock();
    assert!(
        unverified_of(&b, 1).is_none(),
        "B found A's record and verified the head: {problems:?}"
    );
}

/// A crash between the commit and the signature: the commit is in, the head is not signed (here:
/// the range read failed). The next round, as after a restart, reads the range, signs and sends.
#[tokio::test]
async fn a_crash_between_the_commit_and_the_signature_finishes_at_the_next_round() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    institution(&three.vault.b.lock(), "b-1");
    round_of(&three.vault, &three.vault.b).await;
    let head = last_seq(&three);
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("the key changed");
    {
        let a = three.vault.a.lock();
        assert_eq!(key_epoch(&a), 1, "the commit is in");
        assert!(unverified_of(&a, 1).is_some());
        assert!(unsent_envelopes(&a).is_empty());
        assert!(range_notes(&a, 1).is_empty());
    }

    let report = round_of(&three.vault, &three.vault.a).await;

    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    assert!(unverified_of(&a, 1).is_none(), "verified now");
    assert_eq!(range_notes(&a, 1), ops_between(&three, base, head));
    assert!(has_institution(&a, "b-1"));
    drop(a);
    // The signed head is the first envelope A pushed under the new key.
    let first_new_key = three
        .vault
        .relay
        .ops()
        .into_iter()
        .filter(|op| op.seq > head)
        .find_map(|op| {
            let a = three.vault.a.lock();
            let creds = keys::credentials(&a).expect("creds").expect("bound");
            let json = open_op(|e| creds.keys.get(e), &creds.vault_id.0, &op.ct).ok()?;
            changeset::from_json(&json).ok()
        })
        .expect("A's first envelope under the new key");
    assert!(
        first_new_key.rekey_head.is_some(),
        "and it carries the signed head"
    );
}

/// R2's accepted residual, pinned: a range the server filled with the removed device's writes
/// before the remover read it is signed with them and applied by the remover.
#[tokio::test]
async fn a_range_the_server_filled_before_the_remover_read_it_is_signed() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(2);
    // The read inside the call fails: the window the removed device and the server use.
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("the key changed");
    write_from_removed_c(&three, "c-first").await;
    write_from_removed_c(&three, "c-second").await;

    let report = round_of(&three.vault, &three.vault.a).await;

    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    assert_eq!(range_notes(&a, 1).len(), 2);
    assert!(has_institution(&a, "c-first") && has_institution(&a, "c-second"));
    assert_eq!(state::epochs(&a).expect("epochs").get(&1), Some(&(end + 2)));
    drop(a);
    let ops = three.vault.relay.ops();
    let signed: Vec<u32> = ops
        .iter()
        .filter(|op| op.seq > end + 2)
        .filter_map(|op| {
            let a = three.vault.a.lock();
            let creds = keys::credentials(&a).expect("creds").expect("bound");
            let json = open_op(|e| creds.keys.get(e), &creds.vault_id.0, &op.ct).ok()?;
            let record = changeset::from_json(&json).ok()?.rekey_head?;
            Some(HeadRecord::decode(&record).ok()?.count)
        })
        .collect();
    assert_eq!(signed, vec![2], "A signed what it found");
}

/// The control: the same write on a server that reports the true head is refused by both devices
/// (it sits above the head under the old key).
#[tokio::test]
async fn a_removed_devices_write_above_the_true_head_is_refused() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);

    remove(&three, A, &[C]).await.expect("removed");
    assert_eq!(signed_head(&three), end);
    let at = write_from_removed_c(&three, "c-after-removal").await;
    assert_eq!(at, end + 1);

    let problems = stayers_sync(&three).await;

    assert!(
        problems.contains(&Some(SyncProblem::InvalidChain)),
        "{problems:?}"
    );
    assert!(!has_institution(&three.vault.a.lock(), "c-after-removal"));
    assert!(!has_institution(&three.vault.b.lock(), "c-after-removal"));
}

/// R2-H3, found while the fix for R2-H1 was reviewed (spec §1.3): the epoch rule covers ops only. A
/// snapshot is opened with whatever key its header names, wherever in the log it claims to be. So
/// C, removed, seals a snapshot of its own data under the old key at a position above the key
/// change; a server working with it stores it and answers the next pull of an up-to-date device
/// with "restore the snapshot". What must hold: nothing of C's reaches B. Today B merges it.
#[tokio::test]
async fn a_removed_devices_snapshot_above_the_head_is_refused_by_an_up_to_date_device() {
    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    let problems = stayers_sync(&three).await;
    assert!(problems.iter().all(Option::is_none), "{problems:?}");
    let head = signed_head(&three);
    let end = last_seq(&three);
    assert!(end > head, "A's signed head lies above the key change");
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);
    assert!(head::unverified_epochs(&three.vault.b.lock())
        .expect("unverified")
        .is_empty());

    // C writes a row after its removal and seals a snapshot of its profile under the old key,
    // claiming the log's end.
    let sealed = forged_old_key_snapshot(&three, end);
    three.vault.relay.set_accept_retired_writes(true);
    three
        .vault
        .relay
        .put_snapshot(&three.vault.creds.auth(), end, sealed)
        .await
        .expect("the server takes it");
    // The server tells B that its place was pruned.
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);

    // The round stops on the refused snapshot (`syncSnapshotInvalid`): no report, nothing merged.
    let outcome = run_round(
        &three.vault.relay,
        &three.vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;

    let restored = outcome.as_ref().is_ok_and(|report| report.restored);
    assert!(
        !has_institution(&three.vault.b.lock(), "c-in-snapshot") && !restored,
        "B, up to date and on the new key, merged a snapshot the removed device sealed under the \
         old key above the key change (restored: {restored}, outcome: {outcome:?})"
    );
    assert!(
        matches!(&outcome, Err(error) if super::kit_join::key_of(error).as_deref() == Some("validation.syncSnapshotInvalid")),
        "{outcome:?}"
    );
}

/// Rule 10, the honest case (spec §2.3): an old-key snapshot at or below the base of the key
/// change that ended its key, offered to a device that is behind it, is taken. B stayed behind,
/// A dumped before the removal, the server pruned up to the dump: B verifies the head, finds the
/// range above the dump and restores.
#[tokio::test]
async fn an_old_key_snapshot_below_the_base_catches_a_lagging_device_up() {
    let three = three().await;
    let behind = assert_everyone_is_up_to_date(&three);
    institution(&three.vault.a.lock(), "a-in-the-old-snapshot");
    super::kit_join::snapshot_by(&three, &three.vault.a).await;
    let dumped = three.vault.relay.snapshot_seq().expect("A's snapshot");
    assert!(dumped > behind);
    remove(&three, A, &[C]).await.expect("removed");
    // A's signed head goes out by hand: a round of A would also upload its new-key snapshot,
    // and the vault's newest one would no longer be the old-key one.
    push_a_record(&three).await;
    three.vault.relay.prune_to(dumped);
    assert_eq!(key_epoch(&three.vault.b.lock()), 0);

    let mut last = None;
    for _ in 0..3 {
        last = Some(round_result(&three, B).await);
    }

    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1);
    assert_eq!(
        range::base_of(&b, 1).expect("base"),
        Some(dumped),
        "B read the range"
    );
    assert!(
        has_institution(&b, "a-in-the-old-snapshot"),
        "B restored the old-key snapshot at the base: {last:?}"
    );
}

/// Rule 10: an old-key snapshot above the base and at or below the head may be honest (another
/// device was ahead of the remover), so a device that is behind it does not take it and does not
/// call it invalid: it waits for one under the new key and says so (`keyChangeWaiting`).
#[tokio::test]
async fn an_old_key_snapshot_between_base_and_head_waits_for_a_new_one() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    // B reads the range and verifies the head (its base is `end`), then falls behind again.
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        range::base_of(&three.vault.b.lock(), 1).expect("base"),
        Some(end)
    );
    lag_behind(&three.vault.b.lock(), end);
    let between = end + 3;
    server_takes_snapshot(&three, between, forged_old_key_snapshot(&three, between)).await;
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);

    let waiting = round_result(&three, B).await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-in-snapshot"),
        "nothing of the snapshot: {waiting:?}"
    );
    assert_eq!(
        waiting.as_ref().map(|report| report.problem).ok(),
        Some(Some(SyncProblem::KeyChangeWaiting)),
        "{waiting:?}"
    );
    assert!(
        !invalid_key(&waiting),
        "waiting is not syncSnapshotInvalid: {waiting:?}"
    );
}

/// Rule 10, the lifted head (§1.1 with §1.3): base == end, head == end + 8, and a forged old-key
/// snapshot at end + 3, inside the range the server kept free. A device that has gone past it (its
/// place is the signed head's op) is offered it by a `410` and refuses it: it is at or below the
/// device's place.
#[tokio::test]
async fn an_old_key_snapshot_offered_to_a_device_past_it_is_refused() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    // The server holds the forged snapshot before anyone uploads one under the new key (A's round
    // would: it owes one), so it is the vault's newest.
    push_a_record(&three).await;
    let forged = end + 3;
    server_takes_snapshot(&three, forged, forged_old_key_snapshot(&three, forged)).await;
    let verified = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(verified.problem, None, "{verified:?}");
    let place = cursor(&three.vault.b.lock());
    assert!(place > end + 8, "B is past the lifted head: {place}");
    assert_eq!(
        range::base_of(&three.vault.b.lock(), 1).expect("base"),
        Some(end)
    );
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);

    let refused = round_result(&three, B).await;

    assert!(invalid_key(&refused), "{refused:?}");
    assert!(!has_institution(&three.vault.b.lock(), "c-in-snapshot"));
}

/// The heal of a missed change: a snapshot under the new key heals even when its position is below
/// the device's place (it was never a restore to catch up).
#[tokio::test]
async fn a_heal_with_a_new_key_snapshot_below_the_place_still_works() {
    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    let problems = stayers_sync(&three).await;
    assert!(problems.iter().all(Option::is_none), "{problems:?}");
    let dumped = three
        .vault
        .relay
        .snapshot_seq()
        .expect("a snapshot under the new key");
    let b_creds = keys::credentials(&three.vault.b.lock())
        .expect("creds")
        .expect("bound");
    assert!(dumped <= cursor(&three.vault.b.lock()), "below B's place");
    exec(
        &three.vault.b.lock(),
        &format!("UPDATE sync_state SET missed_below_barrier = {dumped}"),
    );
    let info = three
        .vault
        .relay
        .vault_info(&b_creds.auth())
        .await
        .expect("info");

    let healed = heal_missed(
        &three.vault.relay,
        &three.vault.b,
        &b_creds,
        &mut SnapshotPolicy::default(),
        &info,
        NOW,
    )
    .await
    .expect("heal");

    assert!(
        healed.is_some(),
        "restored from a snapshot under the key B holds"
    );
    assert_eq!(
        state::load(&three.vault.b.lock())
            .expect("state")
            .expect("row")
            .missed_below_barrier,
        None
    );
}

/// The heal with an old-key snapshot below the device's place: never merged, and not refused as
/// invalid either: it waits (`SnapshotWaiting`), and is not fetched again until the vault has a
/// newer snapshot.
#[tokio::test]
async fn a_heal_with_an_old_key_snapshot_waits() {
    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    let problems = stayers_sync(&three).await;
    assert!(problems.iter().all(Option::is_none), "{problems:?}");
    let end = last_seq(&three);
    // The vault's newest snapshot is C's, under the old key, at B's place.
    let sealed = forged_old_key_snapshot(&three, end);
    server_takes_snapshot(&three, end, sealed).await;
    let b_creds = keys::credentials(&three.vault.b.lock())
        .expect("creds")
        .expect("bound");
    exec(
        &three.vault.b.lock(),
        &format!("UPDATE sync_state SET missed_below_barrier = {end}"),
    );
    let info = three
        .vault
        .relay
        .vault_info(&b_creds.auth())
        .await
        .expect("info");
    let mut policy = SnapshotPolicy::default();

    let waited = heal_missed(
        &three.vault.relay,
        &three.vault.b,
        &b_creds,
        &mut policy,
        &info,
        NOW,
    )
    .await;

    assert!(
        matches!(waited, Err(SyncError::SnapshotWaiting)),
        "{waited:?}"
    );
    assert!(!has_institution(&three.vault.b.lock(), "c-in-snapshot"));
    assert_eq!(policy.examined_seq, Some(end), "not fetched again");
    assert_eq!(
        state::load(&three.vault.b.lock())
            .expect("state")
            .expect("row")
            .missed_below_barrier,
        Some(end),
        "the miss stays"
    );
}

/// Rule 10, "so that an honest refusal ends": every device that verified a key change owes the
/// vault a snapshot under the new key, not only the remover. A removes C and sends its signed head
/// (and never syncs again); B verifies it and uploads the snapshot.
#[tokio::test]
async fn every_verified_device_owes_the_new_key_snapshot() {
    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    // A's one last send: its signed head. (A's own forced snapshot would also cover it, so A
    // sends the record by hand and takes no round.)
    push_a_record(&three).await;
    let head = signed_head(&three);
    let before = three.vault.relay.snapshot_seq();

    let mut uploaded = false;
    for _ in 0..3 {
        uploaded |= round_of(&three.vault, &three.vault.b)
            .await
            .snapshot_uploaded;
    }

    assert!(uploaded, "B uploaded the snapshot A owes");
    let at = three.vault.relay.snapshot_seq().expect("a snapshot");
    assert!(
        at > head && Some(at) != before,
        "above the key change: {at} > {head}"
    );
    let b = three.vault.b.lock();
    assert!(
        !state::snapshot_epoch_owed(&b).expect("owed"),
        "B owes nothing any more"
    );
}

/// Nothing changes for a vault whose key never changed: every snapshot is under the newest key.
#[tokio::test]
async fn a_vault_that_never_changed_its_key_restores_as_before() {
    let three = three().await;
    institution(&three.vault.a.lock(), "a-before-the-snapshot");
    super::kit_join::snapshot_by(&three, &three.vault.a).await;
    let dumped = three.vault.relay.snapshot_seq().expect("snapshot");
    three.vault.relay.prune_to(dumped);
    lag_behind(&three.vault.b.lock(), 0);
    assert_eq!(key_epoch(&three.vault.b.lock()), 0);

    let restored = round_result(&three, B).await.expect("round");

    assert!(restored.restored, "{restored:?}");
    assert!(has_institution(
        &three.vault.b.lock(),
        "a-before-the-snapshot"
    ));
}

/// The vault of the lifted-head set-up (base `end`, head `end + 8`) in which B is up to date and
/// has verified the change (its base is `end`); the server holds a forged old-key snapshot at
/// `at` (a position, or `None` for end + 3). Returns `end`.
async fn lifted_head_with_forged_snapshot(three: &Three, at: Option<u64>) -> u64 {
    let end = assert_everyone_is_up_to_date(three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(three, A, &[C]).await.expect("removed");
    push_a_record(three).await;
    let forged = at.unwrap_or(end + 3);
    server_takes_snapshot(three, forged, forged_old_key_snapshot(three, forged)).await;
    let verified = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(verified.problem, None, "{verified:?}");
    assert_eq!(
        range::base_of(&three.vault.b.lock(), 1).expect("base"),
        Some(end)
    );
    end
}

fn waiting_key(result: &crate::services::sync::error::SyncResult<impl std::fmt::Debug>) -> bool {
    matches!(result, Err(error) if super::kit_join::key_of(error).as_deref() == Some("validation.syncSnapshotWaiting"))
}

/// R2-H3 through the re-sync (review of Task 7, I1): a device that holds the heads does not take an
/// old-key snapshot above the base of the key change that ended its key when the person re-syncs
/// it. B is up to date and verified; the vault's newest snapshot is C's, forged inside the range the
/// server kept free. B is not replaced and C's rows do not reach it; it is told to wait for one
/// under the new key.
#[tokio::test]
async fn a_resync_does_not_take_an_old_key_snapshot_above_the_base() {
    use crate::services::sync::binding::NoBinding;
    use crate::services::sync::join::resync_by_replace;

    let three = three().await;
    lifted_head_with_forged_snapshot(&three, None).await;
    let dir = tempfile::tempdir().expect("dir");

    let result = resync_by_replace(
        &three.vault.relay,
        &three.vault.b,
        agreed(&three.vault.b),
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-in-snapshot"),
        "{result:?}"
    );
    assert!(waiting_key(&result), "{result:?}");
}

/// The same through the repair of a device whose change capture broke.
#[tokio::test]
async fn a_repair_does_not_take_an_old_key_snapshot_above_the_base() {
    use crate::services::sync::binding::NoBinding;
    use crate::services::sync::join::repair_capture;

    let three = three().await;
    lifted_head_with_forged_snapshot(&three, None).await;
    super::join::break_capture(&three.vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let result = repair_capture(
        &three.vault.relay,
        &three.vault.b,
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-in-snapshot"),
        "{result:?}"
    );
    assert!(waiting_key(&result), "{result:?}");
}

/// Above the head a re-sync is refused outright (`syncSnapshotInvalid`).
#[tokio::test]
async fn a_resync_refuses_an_old_key_snapshot_above_the_head() {
    use crate::services::sync::binding::NoBinding;
    use crate::services::sync::join::resync_by_replace;

    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    let above = last_seq(&three);
    assert!(above > end + 8);
    server_takes_snapshot(&three, above, forged_old_key_snapshot(&three, above)).await;
    round_of(&three.vault, &three.vault.b).await;
    let dir = tempfile::tempdir().expect("dir");

    let result = resync_by_replace(
        &three.vault.relay,
        &three.vault.b,
        agreed(&three.vault.b),
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await;

    assert!(invalid_key(&result), "{result:?}");
    assert!(!has_institution(&three.vault.b.lock(), "c-in-snapshot"));
}

/// The honest case: the only snapshot predates the removal (at or below the base), and a re-sync of
/// a device that verified the change takes it.
#[tokio::test]
async fn a_resync_takes_an_old_key_snapshot_at_or_below_the_base() {
    use crate::services::sync::binding::NoBinding;
    use crate::services::sync::join::resync_by_replace;

    let three = three().await;
    assert_everyone_is_up_to_date(&three);
    institution(&three.vault.a.lock(), "a-in-the-old-snapshot");
    super::kit_join::snapshot_by(&three, &three.vault.a).await;
    let dumped = three.vault.relay.snapshot_seq().expect("snapshot");
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        range::base_of(&three.vault.b.lock(), 1).expect("base"),
        Some(dumped)
    );
    let dir = tempfile::tempdir().expect("dir");

    resync_by_replace(
        &three.vault.relay,
        &three.vault.b,
        agreed(&three.vault.b),
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("resync");

    assert!(has_institution(
        &three.vault.b.lock(),
        "a-in-the-old-snapshot"
    ));
}

/// Review of Task 7, M2: a restore that waits is not downloaded again while the vault's newest
/// snapshot is the same one and the answer is still "wait".
#[tokio::test]
async fn a_waiting_restore_does_not_download_the_same_snapshot_every_round() {
    let three = three().await;
    let end = lifted_head_with_forged_snapshot(&three, None).await;
    lag_behind(&three.vault.b.lock(), end);
    let mut policy = SnapshotPolicy::default();
    let downloads = |three: &Three| {
        three
            .vault
            .relay
            .calls()
            .iter()
            .filter(|route| matches!(route, Route::GetSnapshot))
            .count()
    };
    for round in 1..=3 {
        three
            .vault
            .relay
            .fail_next(Route::Pull, RelayError::SnapshotRequired);
        let report = run_round(
            &three.vault.relay,
            &three.vault.b,
            &mut policy,
            RoundMode::Normal,
            NOW,
            0,
        )
        .await
        .expect("round");
        assert_eq!(
            report.problem,
            Some(SyncProblem::KeyChangeWaiting),
            "round {round}"
        );
        assert_eq!(downloads(&three), 1, "round {round}: downloaded once");
    }
}

/// Review of Task 7, M3 and round 2: a heal that waits for more to be known is not marked examined
/// for good, and is not downloaded again every round either: one download while nothing changes;
/// when the base becomes known and the snapshot fits, it is looked at again and restored.
#[tokio::test]
async fn a_heal_that_waits_is_asked_again_without_a_download_until_it_can_fit() {
    let three = three().await;
    let end = lifted_head_with_forged_snapshot(&three, None).await;
    lag_behind(&three.vault.b.lock(), end);
    let b_creds = keys::credentials(&three.vault.b.lock())
        .expect("creds")
        .expect("bound");
    exec(
        &three.vault.b.lock(),
        &format!("UPDATE sync_state SET missed_below_barrier = {}", end + 3),
    );
    let info = three
        .vault
        .relay
        .vault_info(&b_creds.auth())
        .await
        .expect("info");
    let mut policy = SnapshotPolicy::default();
    let downloads = |three: &Three| {
        three
            .vault
            .relay
            .calls()
            .iter()
            .filter(|route| matches!(route, Route::GetSnapshot))
            .count()
    };

    for _ in 0..3 {
        let waited = heal_missed(
            &three.vault.relay,
            &three.vault.b,
            &b_creds,
            &mut policy,
            &info,
            NOW,
        )
        .await;
        assert!(
            matches!(waited, Err(SyncError::SnapshotWaiting)),
            "{waited:?}"
        );
    }
    assert_eq!(policy.examined_seq, None, "not examined for good");
    assert_eq!(downloads(&three), 1, "downloaded once");

    // The base is now known and covers the snapshot: it fits.
    exec(
        &three.vault.b.lock(),
        &format!(
            "UPDATE sync_epochs SET range_base = {} WHERE epoch = 1",
            end + 3
        ),
    );
    let healed = heal_missed(
        &three.vault.relay,
        &three.vault.b,
        &b_creds,
        &mut policy,
        &info,
        NOW,
    )
    .await;

    assert!(matches!(healed, Ok(Some(_))), "{healed:?}");
    assert_eq!(downloads(&three), 2, "looked at again");
    assert_eq!(policy.waiting_snapshot, None);
}

/// Review of Task 7, M8: above the head a catching-up device that is *behind* the snapshot is
/// refused by the head rule, not by the place rule.
#[tokio::test]
async fn an_old_key_snapshot_above_the_head_is_refused_by_a_device_behind_it() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    let above = last_seq(&three);
    assert!(above > end + 8);
    server_takes_snapshot(&three, above, forged_old_key_snapshot(&three, above)).await;
    round_of(&three.vault, &three.vault.b).await;
    lag_behind(&three.vault.b.lock(), end);
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);

    let refused = round_result(&three, B).await;

    assert!(invalid_key(&refused), "{refused:?}");
    assert!(!has_institution(&three.vault.b.lock(), "c-in-snapshot"));
}

/// Review of Task 7, M8: with two key changes a snapshot sealed under epoch 0 is judged against
/// the first change's head and base, not the second's.
#[tokio::test]
async fn rule_10_uses_the_first_key_change_when_there_are_two() {
    use crate::services::sync::engine::snapshots::{snapshot_admissible, Admissible, Taker};

    let three = three().await;
    remove(&three, A, &[C]).await.expect("removed");
    stayers_sync(&three).await;
    new_kit(&three, A).await.expect("a second key change");
    stayers_sync(&three).await;
    let conn = three.vault.a.lock();
    assert_eq!(key_epoch(&conn), 2);
    let heads = state::epochs(&conn).expect("epochs");
    let (head1, head2) = (heads[&1], heads[&2]);
    let base1 = range::base_of(&conn, 1)
        .expect("base")
        .expect("A read the range");
    assert!(head1 < head2 && base1 >= 1, "{base1} {head1} {head2}");
    lag_behind(&conn, 0);
    let judged = |seq| snapshot_admissible(&conn, 0, seq, Taker::CatchingUp).expect("judged");

    assert_eq!(judged(base1), Admissible::Yes, "at the first change's base");
    assert_eq!(
        judged(head1 + 1),
        Admissible::No,
        "above the first change's head, though below the second's"
    );
}

/// Rule 9a (spec §2.3), the hole the third review read from the code: a device that holds its
/// cursor behind a gap has handled the positions above it, yet it would read them again at the
/// next pull and take whatever the server serves there. After a key change the server serves the
/// removed device's next old-key op at such a position, below the base (so rule 6 has no note for
/// it), where nothing is missing (so rule 9 has nothing to stop on): B must not apply it.
///
/// The position is held by a gap that heals before the key change (the server serves the missing
/// op again), so at the pull that matters every position is served and none is skipped.
#[tokio::test]
async fn a_position_already_handled_cannot_be_refilled_behind_a_held_cursor() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    // A writes two ops; the server withholds the first, so B holds the second as a gap and its
    // cursor stays before the missing position.
    let a1 = write_and_seal(&three.vault, &three.vault.a, "a1");
    let a2 = write_and_seal(&three.vault, &three.vault.a, "a2");
    push(&three.vault.relay, &three.vault.creds, &[a1, a2]).await;
    three.vault.relay.drop_op(end + 1);
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        cursor(&three.vault.b.lock()),
        end,
        "B holds its cursor before the missing position"
    );

    // The server serves the position again; A (whose own op never was a gap to it) removes C.
    three.vault.relay.undrop_op(end + 1);
    round_of(&three.vault, &three.vault.a).await;
    assert_eq!(cursor(&three.vault.a.lock()), end + 2, "A is up to date");
    remove(&three, A, &[C]).await.expect("removed");
    // A's next round sends its signed head, so that B can verify the key change when it picks it up.
    let sent = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(sent.problem, None, "{sent:?}");
    // C seals its next op under the old key; the server serves it where A's second op was.
    let refill = write_and_seal(&three.vault, &three.c, "c-refills");
    three
        .vault
        .relay
        .serve_instead(&vault_b64(&three), end + 2, refill, None);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-refills"),
        "B applied the removed device's op at a position it had already handled: {report:?}"
    );
    assert_eq!(
        report.problem,
        Some(SyncProblem::InvalidChain),
        "{report:?}"
    );
}

/// Rule 9a and the lie that would wipe the notes: the went-back verdict rests on a head the server
/// sends unauthenticated. One page with a low head must not make B forget what it handled; the
/// removed device's old-key op served afterwards at the handled position is still refused.
#[tokio::test]
async fn a_lying_low_head_does_not_let_a_handled_position_be_refilled() {
    use super::pull::Mangled;
    use crate::services::sync::relay::PullPage;

    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    let a1 = write_and_seal(&three.vault, &three.vault.a, "a1");
    let a2 = write_and_seal(&three.vault, &three.vault.a, "a2");
    push(&three.vault.relay, &three.vault.creds, &[a1, a2]).await;
    three.vault.relay.drop_op(end + 1);
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(cursor(&three.vault.b.lock()), end);

    // One page whose head is below B's cursor.
    let lying = Mangled {
        inner: three.vault.relay.clone(),
        mangle: |_, mut page: PullPage| {
            page.head = 0;
            page
        },
    };
    let lied = crate::services::sync::engine::pull::pull_all(
        &lying,
        &three.vault.b,
        &three.vault.creds,
        NOW,
    )
    .await
    .expect("pull");
    assert_eq!(
        lied.stop,
        Some(crate::services::sync::engine::PullStop::Invalid)
    );

    three.vault.relay.undrop_op(end + 1);
    round_of(&three.vault, &three.vault.a).await;
    remove(&three, A, &[C]).await.expect("removed");
    let sent = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(sent.problem, None, "{sent:?}");
    let refill = write_and_seal(&three.vault, &three.c, "c-refills");
    three
        .vault
        .relay
        .serve_instead(&vault_b64(&three), end + 2, refill, None);

    let report = round_of(&three.vault, &three.vault.b).await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-refills"),
        "one lying head made B forget a handled position: {report:?}"
    );
}

/// A device with a key change whose head lies above its cursor and a range base of `base` (the
/// notes of `notes`), and B's two ops of which the relay withholds the first: position 1 is
/// skipped, position 2 is served.
async fn skipped_first_position(
    base: Option<u64>,
    notes: &[RangeNote],
) -> (Vault, crate::services::sync::engine::PullReport) {
    let vault = vault().await;
    super::gap_skip::b1_withheld(&vault).await;
    if let Some(base) = base {
        let a = vault.a.lock();
        state::record_epoch(&a, 1, 10, None, NOW).expect("epoch");
        range::store_notes(&a, 1, base, notes).expect("range");
    }
    let report =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");
    (vault, report)
}

/// Rule 9 (spec §2.3): below the base of a key change an honest server skips no position, so a
/// skipped one ends the pull instead of waiting like a gap.
#[tokio::test]
async fn a_skipped_position_below_the_base_stops_the_pull() {
    let (vault, report) = skipped_first_position(Some(5), &[]).await;

    assert_eq!(
        report.stop,
        Some(crate::services::sync::engine::PullStop::Invalid),
        "{report:?}"
    );
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 0);
    assert!(!has_institution(&a, "b2"));
}

/// The same skipped position with no key change whose head is above the cursor is a gap as ever.
#[tokio::test]
async fn a_skipped_position_with_no_key_change_pending_does_not() {
    let (vault, report) = skipped_first_position(None, &[]).await;

    assert_eq!(report.stop, None, "{report:?}");
    assert_eq!(
        report.end(),
        crate::services::sync::engine::PullEnd::Waiting
    );
    assert_eq!(cursor(&vault.a.lock()), 0);
}

/// Rule 9 holds as well while the base of the key change is not known (the range is not verified
/// here yet), for every position up to its head.
#[tokio::test]
async fn a_skipped_position_stops_the_pull_when_the_base_is_not_known() {
    let vault = vault().await;
    super::gap_skip::b1_withheld(&vault).await;
    state::record_epoch(&vault.a.lock(), 1, 10, None, NOW).expect("epoch");
    assert_eq!(range::base_of(&vault.a.lock(), 1).expect("base"), None);

    let report =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");

    assert_eq!(
        report.stop,
        Some(crate::services::sync::engine::PullStop::Invalid),
        "{report:?}"
    );
}

/// A key change whose head lies below the skipped position says nothing about it (head 0: the
/// ops are epoch 0 and above it, as the epoch rule wants).
#[tokio::test]
async fn a_skipped_position_above_the_only_key_changes_head_does_not_stop_the_pull() {
    let vault = vault().await;
    super::gap_skip::b1_withheld(&vault).await;
    state::record_epoch(&vault.a.lock(), 0, 0, None, NOW).expect("epoch");

    let report =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");

    assert_eq!(report.stop, None, "{report:?}");
}

/// Inside a range (above the base, up to the head) a missing position is rule 6's business, not
/// rule 9's: the op that was signed there is held like any gap.
#[tokio::test]
async fn a_skipped_position_inside_the_range_is_left_to_rule_6() {
    let vault = vault().await;
    super::gap_skip::b1_withheld(&vault).await;
    let hash = {
        let ops = vault.relay.ops();
        envelope_hash(&ops.iter().find(|op| op.seq == 2).expect("op 2").ct)
    };
    {
        let a = vault.a.lock();
        state::record_epoch(&a, 1, 10, None, NOW).expect("epoch");
        range::store_notes(&a, 1, 0, &[RangeNote { pos: 2, hash }]).expect("range");
    }

    let report =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
            .await
            .expect("pull");

    assert_eq!(report.stop, None, "{report:?}");
}

// ---------------------------------------------------------------------------------------------
// The adversarial reading, reader A (the remover, the other devices, the pull).
// ---------------------------------------------------------------------------------------------

/// Reading A, finding A-1 (the place shortcut, question 5). A device whose *place* counts the
/// highest position it holds a note for treats a held op above the head as "I have already handled
/// everything up to there": `plan_of` answers `PastHead`, it reads no range, stores no notes and no
/// base, and rule 6 then has nothing to compare with. But a held op above the head says nothing of
/// the positions below it: a server that skips the whole lifted range for B while B reads the new
/// key's ops (rule 9 only looks at the position it is shown, and above the head it asks nothing)
/// can have B hold one of them (A's second new-key op, whose predecessor, the signed record, is not
/// served yet), let B "verify" the head without a check, and then serve the range, filled after A
/// signed "empty", from B's held cursor: the removed device's later write is applied.
#[tokio::test]
async fn a_held_new_key_op_above_the_head_does_not_excuse_a_device_from_checking_the_range() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    let record = queued_head(&three, A).expect("A signed its head");
    assert_eq!((record.base, record.head, record.count), (end, end + 8, 0));
    push_a_record(&three).await;
    // A goes on writing under the new key: its second new-key envelope follows the record.
    institution(&three.vault.a.lock(), "a-after");
    let sent = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(sent.problem, None, "{sent:?}");
    assert_eq!(
        last_seq(&three),
        end + 10,
        "the record at end+9, a-after at end+10"
    );

    // B's first round: the server withholds the record (and the range is free). B takes the key,
    // looks ahead, finds no record, and holds a-after as a gap (its predecessor is missing).
    three.vault.relay.drop_op(end + 9);
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        key_epoch(&three.vault.b.lock()),
        1,
        "B took the new key: {report:?}"
    );
    assert_eq!(
        cursor(&three.vault.b.lock()),
        end,
        "the cursor stays before the held op"
    );

    // The removed device writes, and the server stores it in a position of the lifted range.
    let at = write_from_removed_c(&three, "c-after-removal").await;
    assert!(at > end && at <= end + 8, "inside the range: {at}");

    // B's next round: the record is served again (a copy at a new position, past where B's
    // look-ahead has read), B finds it, and then reads from its held cursor.
    three.vault.relay.undrop_op(end + 9);
    three.vault.relay.replay_op(end + 9);
    let mut problems = Vec::new();
    for _ in 0..3 {
        problems.push(round_of(&three.vault, &three.vault.b).await.problem);
    }

    let b = three.vault.b.lock();
    assert!(
        !has_institution(&b, "c-after-removal"),
        "B applied the removed device's write inside a range A signed as empty \
         (unverified: {:?}, base: {:?}, problems: {problems:?})",
        unverified_of(&b, 1),
        range::base_of(&b, 1).expect("base")
    );
}

/// Reading A, finding A-2 (question 1 and 9). The remover queues its signed head in `sync_unsent`
/// inside the removal call; it goes out with the next round's push, after that round's pull. If
/// that pull meets an envelope of the remover's own ID that it did not seal (the removed device,
/// still a member, pushed one before the rotation: it holds the vault key and every ID), the
/// verdict is `Clone`, and `rejoin_as_new_device` deletes `sync_unsent`, the signed head with it.
/// The new ID does not sign the old one's change and the old one never speaks again: no other
/// device ever verifies the key change. An honest server, no lie, one push by the removed device.
#[tokio::test]
async fn an_envelope_claiming_the_removers_id_does_not_cost_it_its_queued_head() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    // C, still a member, seals under the vault key an envelope that claims A's ID and a sequence
    // number A never used (a forgery needs nothing but the key).
    let claim = {
        let cs = super::changeset(A, 50, Some([7; 32]), Vec::new());
        forge_json(&three.vault.creds, &changeset::to_json(&cs).expect("json"))
    };
    push(&three.vault.relay, &three.vault.creds, &[claim]).await;
    assert_eq!(last_seq(&three), end + 1);

    remove(&three, A, &[C]).await.expect("removed");
    let record = queued_head(&three, A).expect("A queued its signed head inside the call");
    assert_eq!((record.base, record.head, record.count), (end, end + 1, 1));

    let (of_a, of_b) = stayers_sync_by_device(&three).await;

    assert_ne!(
        state::load(&three.vault.a.lock())
            .expect("state")
            .expect("row")
            .device,
        A,
        "the mechanism: A re-joined under a new ID at its first pull"
    );
    let b = three.vault.b.lock();
    assert!(
        unverified_of(&b, 1).is_none(),
        "B never saw A's signed head: the remover re-joined at its first pull and its queue went \
         (A: {of_a:?}, B: {of_b:?})"
    );
}

/// Reading A, finding A-3 (question 1). Rule 9a keeps a handled position from being refilled by
/// noting what was handled there; the notes go "when the device re-joins under a new ID". But a
/// re-join is something the removed device can cause on any device that has not read its envelope
/// yet: it seals, under the vault key it holds, an envelope that claims that device's ID (`Clone`).
/// A wrote a1 and a2 (positions end+1, end+2, below A's base). C, still a member, pushes a claim on
/// B's ID (end+3) and A removes C: the claim lies in A's signed range, so nobody has a reason to
/// refuse it. B has not pulled yet. The server serves B one more op of C's at end+1 instead of a1:
/// B applies it, holds a2 behind the missing a1 (noted, cursor at end+1), reaches the claim, judges
/// it `Clone`, re-joins, and `seen::forget_all` drops the note of end+2. The server then serves C's
/// next old-key write at end+2, at A's base, below the range: B applies it, which is what rule 9a
/// exists to prevent (the same pull without the claim stops at the position, `Invalid`).
#[tokio::test]
async fn a_rejoin_caused_by_a_claim_on_the_devices_id_does_not_wipe_the_handled_positions() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    let a1 = write_and_seal(&three.vault, &three.vault.a, "a1");
    let a2 = write_and_seal(&three.vault, &three.vault.a, "a2");
    push(&three.vault.relay, &three.vault.creds, &[a1, a2]).await;
    round_of(&three.vault, &three.vault.a).await;
    assert_eq!(cursor(&three.vault.a.lock()), end + 2);

    // C claims B's ID before it is removed; the honest server takes it, above A's cursor.
    let claim = {
        let cs = super::changeset(B, 99, Some([7; 32]), Vec::new());
        forge_json(&three.vault.creds, &changeset::to_json(&cs).expect("json"))
    };
    push(&three.vault.relay, &three.vault.creds, &[claim]).await;
    // C is removed (A's cursor, and so the base, is end+2); A signs its head and sends it.
    remove(&three, A, &[C]).await.expect("removed");
    // (A holds the claim on B's ID as a gap, which says nothing here; its record goes out.)
    let sent = round_of(&three.vault, &three.vault.a).await;
    assert!(sent.push.as_ref().is_some_and(|p| p.pushed > 0), "{sent:?}");

    // B has not pulled since. At end+1 the server serves it another op of C's instead of a1.
    let vault = vault_b64(&three);
    let filler = write_and_seal(&three.vault, &three.c, "c-filler");
    three
        .vault
        .relay
        .serve_instead(&vault, end + 1, filler, None);
    // (B's push after the re-join would be followed by an echo pull, in which a lying server
    // serves the replacement at once; the test keeps the push from going through so that the
    // order of the two pulls is the test's.)
    three
        .vault
        .relay
        .set_failing(Route::Push, Some(RelayError::Network("offline".into())));
    round_ignoring_errors(&three, B).await;
    three.vault.relay.set_failing(Route::Push, None);
    assert_ne!(
        state::load(&three.vault.b.lock())
            .expect("state")
            .expect("row")
            .device,
        B,
        "B re-joined under a new ID at the claim"
    );
    assert_eq!(
        cursor(&three.vault.b.lock()),
        end + 1,
        "a2 is held behind the missing a1"
    );
    assert!(!has_institution(&three.vault.b.lock(), "a2"));

    // The server now serves C's next write, sealed under the old key, where a2 was handled.
    let replacement = write_and_seal(&three.vault, &three.c, "c-replaces");
    three
        .vault
        .relay
        .serve_instead(&vault, end + 2, replacement, None);

    let mut problems = Vec::new();
    for _ in 0..3 {
        problems.push(round_of(&three.vault, &three.vault.b).await.problem);
    }

    assert!(
        !has_institution(&three.vault.b.lock(), "c-replaces"),
        "B applied the removed device's write at a position it had handled: {problems:?}"
    );
}

/// Reading A, finding A-4 (question 2, 3 and the R17 clause). The look-ahead for a key change's
/// signed record goes on from `scanned`, the last position it was shown, and stores it whatever
/// lay below. A server that withholds the record once (the removed device's accomplice for one
/// round, or only a flaky proxy) while it serves a later op leaves `scanned` above the record; the
/// honest server that serves the record again at its own position is never asked for it: the head
/// of an honest vault on an honest server stays unverified on that device for good, and with it
/// every pairing, snapshot, removal and Kit of that device.
#[tokio::test]
async fn a_record_withheld_once_is_found_when_the_server_serves_it_again() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    institution(&three.vault.a.lock(), "a-after");
    let sent = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(sent.problem, None, "{sent:?}");
    assert_eq!(
        last_seq(&three),
        end + 2,
        "the record at end+1, a-after at end+2"
    );

    // One round of B in which the server withholds the record and serves what came after it.
    three.vault.relay.drop_op(end + 1);
    round_of(&three.vault, &three.vault.b).await;
    three.vault.relay.undrop_op(end + 1);

    let mut problems = Vec::new();
    for _ in 0..4 {
        problems.push(round_of(&three.vault, &three.vault.b).await.problem);
    }

    assert!(
        unverified_of(&three.vault.b.lock(), 1).is_none(),
        "B never finds A's signed head again (scanned: {:?}, problems: {problems:?})",
        unverified_of(&three.vault.b.lock(), 1).map(|e| e.scanned)
    );
}
