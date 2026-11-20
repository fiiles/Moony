//! The second sync audit (2026-11-20), finding R2-H1: the remover signs the head of its key change
//! as the sync server reports it and never compares it with what it saw itself. A server that
//! works with a removed device raises that head for the remover, keeps the positions below it
//! free, and later fills them with what the removed device seals under the old key. The signed
//! head then confirms the server's number on every device, so the removed device's writes are
//! applied by devices that were fully up to date when it was removed, and by the remover itself.
//!
//! `…_is_not_applied_by_the_remover` is the finding as a test of what must hold on the remover (spec
//! `docs/specs/2026-11-20-sync-audit-2-fixes-design.md` §2.3 steps 1–6); the same test for another
//! device that was up to date is ignored until the check on every device exists (step 7). The
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
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::envelope::stream::{snapshot_object_id, StreamSealer};
use crate::services::sync::envelope::{envelope_hash, open_op};
use crate::services::sync::error::SyncError;
use crate::services::sync::keys;
use crate::services::sync::rekey::head::{self, HeadRecord};
use crate::services::sync::rekey::range::{self, RangeNote, RangeRead};
use crate::services::sync::rekey::rotate::{self, Outcome};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;

fn last_seq(three: &Three) -> u64 {
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

/// R2-H1, the half of a device that was up to date at the removal. It needs the check every
/// device makes on the head (step 7), which is not built yet. (Today B applies nothing either,
/// but for another reason: A stops at the op without a note and never sends its signed head, so B
/// waits for it for good.)
#[tokio::test]
#[ignore = "R2-H1: B's half, un-ignored in Task 5"]
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

/// An op of the range replaced after it was noted (by an old-key one, or by another one under the
/// same key) is not applied: `Invalid`, with the cursor where it was.
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

/// While its own head is not signed the remover sends nothing, seals nothing again, uploads no
/// snapshot, and starts no other removal. Here the server answers no pull at all.
#[tokio::test]
async fn while_its_own_head_is_unverified_the_remover_sends_nothing() {
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

    let report = round_of(&three.vault, &three.vault.a).await;

    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    assert!(!head::own_unverified(&a).expect("own"));
    drop(a);
    assert!(
        three.vault.relay.ops().len() > before,
        "the signed head and the row went out"
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
#[ignore = "R2-H3 (found with audit 2026-11-20): fails until a restore checks the key epoch"]
async fn a_removed_devices_snapshot_above_the_head_is_merged_by_an_up_to_date_device() {
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
    // (A dump refuses while anything waits to be sent, so the row is sealed into C's next
    // envelope first and that envelope dropped; it is never sent.)
    write_and_seal(&three.vault, &three.c, "c-in-snapshot");
    let sealed = {
        let c = three.c.lock();
        exec(&c, "DELETE FROM sync_unsent");
        let mut plain = Vec::new();
        snapshot::dump(&c, end, &mut plain).expect("dump");
        let old = &three.vault.creds;
        assert_eq!(old.keys.current_epoch(), 0);
        let mut deflate = DeflateEncoder::new(
            StreamSealer::new(
                0,
                old.keys.current(),
                &old.vault_id.0,
                &snapshot_object_id(end),
            ),
            Compression::default(),
        );
        deflate.write_all(&plain).expect("deflate");
        deflate.finish().expect("deflate").finish().expect("seal")
    };
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

    let report = round_of(&three.vault, &three.vault.b).await;

    assert!(
        !has_institution(&three.vault.b.lock(), "c-in-snapshot"),
        "B, up to date and on the new key, merged a snapshot the removed device sealed under the \
         old key above the key change (restored: {}, problem: {:?})",
        report.restored,
        report.problem
    );
}
