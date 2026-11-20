//! The second sync audit (2026-11-20), finding R2-H1: the remover signs the head of its key change
//! as the sync server reports it and never compares it with what it saw itself. A server that
//! works with a removed device raises that head for the remover, keeps the positions below it
//! free, and later fills them with what the removed device seals under the old key. The signed
//! head then confirms the server's number on every device, so the removed device's writes are
//! applied by devices that were fully up to date when it was removed, and by the remover itself.
//!
//! `…_is_applied_by_every_device` is the finding as a test of what must hold; it fails until the
//! fix (spec `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`), so it is ignored. The control
//! next to it shows the same write refused when the server reports the true head. The third test
//! is R2-H3, the same harm through a snapshot, found while that spec was reviewed.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use super::pull::write_and_seal;
use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::envelope::stream::{snapshot_object_id, StreamSealer};
use crate::services::sync::rekey::head;
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

/// R2-H1. What must hold: after C is removed, nothing C seals is applied by a device that was up
/// to date at the removal. Today the server's raised head is signed by the remover unchanged, so
/// both A and B apply C's write.
#[tokio::test]
#[ignore = "R2-H1 (audit 2026-11-20): fails until the remover signs a head it saw itself"]
async fn a_removed_devices_write_below_a_head_the_server_raised_is_applied_by_every_device() {
    let three = three().await;
    let end = assert_everyone_is_up_to_date(&three);

    // The server lies to the remover: the key change is reported 8 positions above the real end.
    three.vault.relay.lift_next_rotation_head(8);
    remove(&three, A, &[C]).await.expect("removed");
    assert_eq!(signed_head(&three), end + 8, "A signs the server's number");

    // After the removal, C writes; the server gives it a position it kept free.
    let at = write_from_removed_c(&three, "c-after-removal").await;
    assert!(at > end && at <= end + 8, "below the raised head: {at}");

    let problems = stayers_sync(&three).await;

    let a = three.vault.a.lock();
    let b = three.vault.b.lock();
    assert_eq!(key_epoch(&b), 1, "B took the new key");
    assert!(
        head::unverified_epochs(&b).expect("unverified").is_empty(),
        "B verified the head against A's signature: {problems:?}"
    );
    let applied: Vec<&str> = [("A, the remover", &a), ("B", &b)]
        .into_iter()
        .filter(|(_, conn)| has_institution(conn, "c-after-removal"))
        .map(|(name, _)| name)
        .collect();
    assert!(
        applied.is_empty(),
        "up to date at the removal, yet applied the removed device's later write: {applied:?} \
         (sync problems shown in the six rounds: {problems:?})"
    );
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
