//! Task 10, step 1, reader B (snapshots, joins and the gates): what the adversarial reading of the
//! key-change build found, as tests. A test that shows a way through is `#[ignore]`d with its
//! reason (so the suite stays green); one that shows an attempt does not get through runs.
//!
//! Findings:
//! - B1: a removed device's old-key op above the head, stored once by a server that works with
//!   it, ends the pull of every device for good. No device then passes it, so none uploads the
//!   snapshot under the new key that rule 10's waits and refusals count on, and no re-sync or join
//!   gets past it.
//! - B3: a device that turns sync off right after it adopted a key change pushes the envelopes it
//!   sealed under the old key before, with the new token: older-key ops above the head, which
//!   end every other device's pull for good (B1).
//! - B2: a device the server kept in the dark applies the removed device's later write, learns
//!   the truth, and is then the first to upload the new-key snapshot (every verified device owes
//!   one): the write is in the vault's newest snapshot, sealed under the new key.

use super::pull::write_and_seal;
use super::rekey_audit2::{
    forged_old_key_snapshot, last_seq, push_a_record, server_takes_snapshot,
};
use super::rekey_fixture::*;
use super::*;
use crate::models::sync::SyncNoticeKind;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::Runtime;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::envelope::envelope_epoch;
use crate::services::sync::error::SyncResult;
use crate::services::sync::keys;
use crate::services::sync::rekey;
use crate::services::sync::rekey::pickup;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{Relay, RelayError};
use crate::services::sync::setup;
use crate::services::sync::view;

async fn round_result(three: &Three, id: DeviceId) -> SyncResult<RoundReport> {
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

fn true_head(three: &Three) -> u64 {
    *state::epochs(&three.vault.a.lock())
        .expect("epochs")
        .get(&1)
        .expect("A's head")
}

/// The epoch the vault's newest snapshot is sealed under.
async fn newest_snapshot_epoch(three: &Three) -> Option<u32> {
    let creds = keys::credentials(&three.vault.a.lock())
        .expect("creds")
        .expect("bound");
    let blob = three.vault.relay.get_snapshot(&creds.auth()).await.ok()?;
    blob.bytes
        .get(4..8)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// B1. C, removed, writes one op under the old key after the head; the server takes it (once:
/// afterwards it is honest again) and the op stays in the log. Every pull of every device ends on
/// it as `Invalid` (an older-key op above the head, `epoch_fits_position`), the cursor stays before
/// it, nothing after it is read, and a device whose pull never ends done uploads no snapshot. What
/// must hold: the vault recovers on an honest server, i.e. the devices that stayed go past the op
/// and the snapshot under the new key that rule 10's waits depend on gets uploaded.
#[tokio::test]
#[ignore = "audit-2 reading B1: open, the owner decides (ruling R22)"]
async fn an_old_key_op_above_the_head_stored_once_does_not_wedge_the_vault_for_good() {
    let three = three().await;
    let end = last_seq(&three);
    remove(&three, A, &[C]).await.expect("removed");
    // The server works with C for one write.
    three.vault.relay.set_accept_retired_writes(true);
    let poison = write_and_seal(&three.vault, &three.c, "c-after-removal");
    push(&three.vault.relay, &three.vault.creds, &[poison]).await;
    three.vault.relay.set_accept_retired_writes(false);
    assert_eq!(last_seq(&three), end + 1);

    let mut stuck = Vec::new();
    let mut uploaded = false;
    for _ in 0..6 {
        for id in [A, B] {
            let report = round_result(&three, id).await;
            if let Ok(report) = &report {
                uploaded |= report.snapshot_uploaded;
                stuck.push(report.problem);
            }
        }
    }

    assert!(
        stuck.last() == Some(&None) && uploaded,
        "six rounds of each device on an honest server later: problems {stuck:?}, a snapshot \
         under the new key uploaded: {uploaded}, the vault's newest snapshot: {:?}",
        newest_snapshot_epoch(&three).await
    );
}

/// B2's set-up: A removes C, the server hides the key change from B and serves C's write past the
/// head, B applies it, then the server is honest again. Returns the head of the new epoch.
async fn b_applies_the_forged_write(three: &Three) -> u64 {
    remove(three, A, &[C]).await.expect("removed");
    let head = true_head(three);
    // A's signed head goes out by hand (a round of A would also upload the new-key snapshot, and
    // nobody else would owe it; the server can as easily make A's upload fail).
    push_a_record(three).await;
    let a_sent = last_seq(three);
    // Hidden from B: no rotation in the info, nothing of A's under the new key.
    for seq in head + 1..=a_sent {
        three.vault.relay.drop_op(seq);
    }
    three
        .vault
        .relay
        .set_rotation(&three.vault.creds.vault_id.to_b64(), 0);
    three.vault.relay.set_accept_retired_writes(true);
    three.vault.relay.set_accept_retired_reads(true);
    let forged = write_and_seal(&three.vault, &three.c, "c-forged");
    push(&three.vault.relay, &three.vault.creds, &[forged]).await;
    round_of(&three.vault, &three.vault.b).await;
    assert!(has_institution(&three.vault.b.lock(), "c-forged"));

    // The server is honest again.
    for seq in head + 1..=a_sent {
        three.vault.relay.undrop_op(seq);
    }
    three
        .vault
        .relay
        .set_rotation(&three.vault.creds.vault_id.to_b64(), 1);
    three.vault.relay.set_accept_retired_writes(false);
    three.vault.relay.set_accept_retired_reads(false);
    head
}

/// B2. The server hides the key change from B and serves C's write past the head; B applies it. B
/// then learns of the key change, verifies the head and notes the overrun (`keyChangeOverrun`;
/// "nothing is undone"). Every verified device owes the new-key snapshot, and B is the one that
/// uploads it: it dumps its profile, C's row included. What must hold: a device that knows it
/// applied what the removed device wrote does not seal that into the vault's snapshot, so A, which
/// never applied it, does not get it when it restores that snapshot. (As built, per ruling R19:
/// B uploads no snapshot at all.)
#[tokio::test]
async fn a_device_that_applied_the_removed_devices_write_does_not_upload_it_in_the_new_key_snapshot(
) {
    let three = three().await;
    let head = b_applies_the_forged_write(&three).await;
    let mut uploaded = false;
    let mut seen = Vec::new();
    for _ in 0..3 {
        let report = round_result(&three, B).await;
        if let Ok(report) = &report {
            uploaded |= report.snapshot_uploaded;
        }
        let (owed, at) = {
            let b = three.vault.b.lock();
            (state::snapshot_epoch_owed(&b), cursor(&b))
        };
        seen.push(format!(
            "{:?} owed {owed:?} cursor {at} vault snapshot {:?} head {head} last {}",
            report.map(|r| (r.problem, r.snapshot_uploaded, r.snapshot_outdated)),
            three.vault.relay.snapshot_seq(),
            last_seq(&three)
        ));
    }
    {
        let b = three.vault.b.lock();
        assert!(
            rekey::key_change_overrun(&b).expect("overrun").is_some(),
            "B noted the overrun: {seen:?}"
        );
        // Ruling R19: B holds an overrun, so it uploads no snapshot and owes none.
        assert!(
            !uploaded && !state::snapshot_epoch_owed(&b).expect("owed"),
            "B uploaded or still owes the snapshot: rounds {seen:?}"
        );
    }

    // A is told its place was pruned and restores the vault's snapshot (sealed under the key it
    // holds), as a lagging or re-synced device would.
    three
        .vault
        .relay
        .fail_next(Route::Pull, RelayError::SnapshotRequired);
    let _ = round_result(&three, A).await;

    assert!(
        !has_institution(&three.vault.a.lock(), "c-forged"),
        "A, which never applied the removed device's write, got it from B's snapshot"
    );
}

/// An attempt that does not get through: C's forged old-key snapshot at the log's end does not
/// make the honest devices' duty done on an honest server (the relay refuses a position above its
/// head, and a device at the end uploads at the same position, which replaces it).
#[tokio::test]
async fn a_forged_old_key_snapshot_at_the_end_of_the_log_is_replaced_by_the_new_key_one() {
    let three = three().await;
    remove(&three, A, &[C]).await.expect("removed");
    push_a_record(&three).await;
    let end = last_seq(&three);
    assert!(end > true_head(&three));
    server_takes_snapshot(&three, end, forged_old_key_snapshot(&three, end)).await;
    assert_eq!(newest_snapshot_epoch(&three).await, Some(0));

    let mut uploaded = false;
    for _ in 0..3 {
        for id in [A, B] {
            if let Ok(report) = round_result(&three, id).await {
                uploaded |= report.snapshot_uploaded;
            }
        }
    }

    assert!(uploaded, "a device above the head uploaded");
    assert_eq!(
        newest_snapshot_epoch(&three).await,
        Some(1),
        "the vault's newest snapshot is under the new key"
    );
}

/// B3. B sealed a change under the old key and never sent it (it was offline). A removes C; B's
/// round picks the new key up, but the pull stops or the re-seal has not run yet, and the person
/// turns sync off on B. The leave (`setup::leave_best_effort`, `push_all`) sends what waits in
/// `sync_unsent` as it is: B's old-key envelope goes to the server with the new token, above the
/// head, where every device stops on it as `Invalid` (B1). What must hold: nothing sealed under
/// an older key goes out above a key change's head.
#[tokio::test]
async fn leaving_right_after_adopting_a_key_change_sends_no_old_key_op_above_the_head() {
    let three = three().await;
    write_and_seal(&three.vault, &three.vault.b, "b-unsent");
    remove(&three, A, &[C]).await.expect("removed");
    let head = true_head(&three);
    round_of(&three.vault, &three.vault.a).await;
    pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    assert_eq!(key_epoch(&three.vault.b.lock()), 1);

    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.b,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;

    let old_above_head: Vec<u64> = three
        .vault
        .relay
        .ops()
        .iter()
        .filter(|op| op.seq > head && envelope_epoch(&op.ct) == Some(0))
        .map(|op| op.seq)
        .collect();
    let a_after = round_of(&three.vault, &three.vault.a).await.problem;
    assert!(
        old_above_head.is_empty(),
        "old-key ops above the head {head}: {old_above_head:?}; A's next round: {a_after:?}"
    );
}

/// R19: the mark behind the overrun is not the notice. Dismissing the notice does not lift it (the
/// device still uploads nothing and owes nothing); a re-sync from a clean snapshot does, and the
/// device then uploads again.
#[tokio::test]
async fn only_a_replace_lifts_the_overrun_mark_not_dismissing_the_notice() {
    let three = three().await;
    b_applies_the_forged_write(&three).await;
    for _ in 0..3 {
        let report = round_result(&three, B).await.expect("round");
        assert!(
            !report.snapshot_uploaded,
            "a tainted device uploads nothing"
        );
    }
    {
        let b = three.vault.b.lock();
        assert!(rekey::key_change_overrun(&b).expect("notice").is_some());
        assert!(state::overrun_tainted(&b).expect("mark"));
    }

    // Dismissing the notice is not enough.
    view::dismiss_notice(
        &three.vault.b.lock(),
        SyncNoticeKind::KeyChangeOverrun,
        None,
        &Runtime::default(),
        NOW,
    )
    .expect("dismissed");
    {
        let b = three.vault.b.lock();
        assert!(rekey::key_change_overrun(&b).expect("notice").is_none());
        assert!(state::overrun_tainted(&b).expect("mark"), "the mark stays");
        b.execute("UPDATE sync_state SET snapshot_epoch_due = 1", [])
            .expect("force a duty");
        assert!(!state::snapshot_epoch_owed(&b).expect("owed"));
    }
    let report = round_result(&three, B).await.expect("round");
    assert!(
        !report.snapshot_uploaded,
        "still nothing after the dismissal"
    );

    // The vault gets a clean snapshot under the new key (A's profile never applied C's write;
    // A itself cannot upload while C's old-key op sits above the head, B1); B re-syncs from it.
    let at = last_seq(&three);
    let sealed = {
        let a = three.vault.a.lock();
        let creds = keys::credentials(&a).expect("creds").expect("bound");
        a.execute_batch("DELETE FROM sync_unsent; DELETE FROM sync_outbox;")
            .expect("unsent");
        let mut plain = Vec::new();
        crate::services::sync::snapshot::dump(&a, at, &mut plain).expect("dump");
        let mut deflate = flate2::write::DeflateEncoder::new(
            crate::services::sync::envelope::stream::StreamSealer::new(
                1,
                creds.keys.current(),
                &creds.vault_id.0,
                &crate::services::sync::envelope::stream::snapshot_object_id(at),
            ),
            flate2::Compression::default(),
        );
        std::io::Write::write_all(&mut deflate, &plain).expect("deflate");
        let sealed = deflate.finish().expect("deflate").finish().expect("seal");
        (creds, sealed)
    };
    three
        .vault
        .relay
        .put_snapshot(&sealed.0.auth(), at, sealed.1)
        .await
        .expect("the vault takes it");
    let consent = agreed(&three.vault.b);
    setup_resync(&three, consent).await;
    {
        let b = three.vault.b.lock();
        assert!(
            !state::overrun_tainted(&b).expect("mark"),
            "a replace lifts the mark"
        );
        assert!(
            !has_institution(&b, "c-forged"),
            "and the profile is the clean snapshot's"
        );
        b.execute("UPDATE sync_state SET snapshot_epoch_due = 1", [])
            .expect("force a duty");
        assert!(
            state::snapshot_epoch_owed(&b).expect("owed"),
            "the duty counts again"
        );
    }
}

async fn setup_resync(three: &Three, consent: crate::services::sync::impact::ReplaceConsent) {
    crate::services::sync::join::resync_by_replace(
        &three.vault.relay,
        &three.vault.b,
        consent,
        three.dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("re-sync");
}

/// R20: `push_all` itself sends nothing sealed under an epoch below the current key's, and nothing
/// behind such an envelope; after the re-seal everything goes, in order.
#[tokio::test]
async fn push_all_holds_an_older_key_envelope_and_what_is_behind_it_until_the_reseal() {
    let three = three().await;
    write_and_seal(&three.vault, &three.vault.b, "b-unsent");
    remove(&three, A, &[C]).await.expect("removed");
    round_of(&three.vault, &three.vault.a).await;
    pickup::pick_up(&three.vault.relay, &three.vault.b, None, NOW)
        .await
        .expect("pickup");
    let creds = keys::credentials(&three.vault.b.lock())
        .expect("creds")
        .expect("bound");
    assert_eq!(creds.keys.current_epoch(), 1);
    // A newer change, sealed under the new key, queued behind the old-key one.
    institution(&three.vault.b.lock(), "b-newer");
    let before = last_seq(&three);

    let report =
        crate::services::sync::engine::push::push_all(&three.vault.relay, &three.vault.b, &creds)
            .await
            .expect("push_all");

    assert_eq!(
        report.pushed, 0,
        "nothing goes: the head of the queue is old-key"
    );
    assert_eq!(last_seq(&three), before, "the server got nothing");
    let waiting: Vec<Option<u32>> = {
        let b = three.vault.b.lock();
        let mut stmt = b
            .prepare("SELECT envelope FROM sync_unsent ORDER BY seq")
            .expect("prepare");
        stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))
            .expect("rows")
            .map(|e| envelope_epoch(&e.expect("row")))
            .collect()
    };
    assert_eq!(
        waiting.first(),
        Some(&Some(0)),
        "the old-key envelope is still first"
    );

    // The re-seal (a round does it once the pull has reached the head); then everything goes.
    for _ in 0..3 {
        round_of(&three.vault, &three.vault.b).await;
    }
    let after: Vec<Option<u32>> = three
        .vault
        .relay
        .ops()
        .iter()
        .filter(|op| op.seq > before)
        .map(|op| envelope_epoch(&op.ct))
        .collect();
    assert!(!after.is_empty(), "the queue went out after the re-seal");
    assert!(
        after.iter().all(|e| *e == Some(1)),
        "all under the new key: {after:?}"
    );
}
