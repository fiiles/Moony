//! Leaving, and records about this device (phase 5 plan Task 11, revision Task 20): a device that
//! turns sync off signs a `left` record and sends it within a short budget; a removal (or a
//! `left` record someone else made with this device's key) that reaches a device through sync
//! turns sync off there; the trust store catches up after every merge that touched the device
//! list and at unlock.

use std::time::Duration;

use super::rekey_fixture::*;
use super::runtime_loop::{until, Sealing};
use super::*;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::identity::trust::{self, TrustState};
use crate::services::sync::identity::{self, sign_pub, Record, RecordType};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::rekey::rotate;
use crate::services::sync::rekey::{self, RemovedNotice};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, PullPage, PushReceipt,
    RelayResult, ServerInfo, SnapshotBlob, VaultInfo,
};
use crate::services::sync::setup;

fn left_record(conn: &rusqlite::Connection, device: DeviceId) -> Option<Record> {
    let value: Option<String> = conn
        .query_row(
            "SELECT left_record FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| r.get(0),
        )
        .ok()?;
    Record::from_b64(&value?).ok()
}

fn state_of(conn: &rusqlite::Connection, device: DeviceId) -> Option<TrustState> {
    trust::entry(conn, device).expect("entry").map(|e| e.state)
}

/// A leaves: its `left` record is signed by its own key, written into its row, and sent; every
/// device that pulls it sees A as `left`.
#[tokio::test]
async fn a_device_that_leaves_signs_and_sends_its_left_record() {
    let three = three().await;
    let a_pub = {
        let a = three.vault.a.lock();
        sign_pub(&identity::own_seed(&a).expect("seed").expect("key"))
    };
    let pushed_before = three.vault.relay.ops().len();

    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;

    let record = left_record(&three.vault.a.lock(), A).expect("the row holds a left record");
    assert_eq!(record.kind, RecordType::Left);
    assert_eq!((record.subject, record.signer), (A, A));
    assert!(record.verify(&a_pub).is_ok());
    assert!(
        three.vault.relay.ops().len() > pushed_before,
        "the record went out"
    );

    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert_eq!(
        state_of(&three.vault.b.lock(), A),
        Some(TrustState::Left),
        "the pull's refresh applied the record"
    );
    assert!(sync_on(&three.vault.b.lock()));
}

/// A second leave (a turn-off that failed after the first, say) writes no second record.
#[tokio::test]
async fn leaving_twice_keeps_the_first_record() {
    let three = three().await;
    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;
    let first = left_record(&three.vault.a.lock(), A).expect("record");

    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;

    assert_eq!(left_record(&three.vault.a.lock(), A), Some(first));
}

/// A sync server that answers every push after 30 s: leaving ends at its budget, and turning sync
/// off right after works as always.
#[tokio::test(start_paused = true)]
async fn a_slow_server_never_holds_the_leave_past_its_budget() {
    let three = three().await;
    let relay = SlowPush(three.vault.relay.clone());
    let started = tokio::time::Instant::now();

    setup::leave_best_effort(
        &relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;

    let spent = started.elapsed();
    assert!(
        spent >= setup::LEAVE_BUDGET && spent < setup::LEAVE_BUDGET + Duration::from_secs(1),
        "{spent:?}"
    );
    setup::turn_off_between_rounds(&three.vault.a, three.dir.path())
        .await
        .expect("turned off");
    assert!(!sync_on(&three.vault.a.lock()));
}

/// Every failure of the leave is ignored: a refused push and a profile with sync off.
#[tokio::test]
async fn a_failed_leave_is_ignored_and_a_profile_without_sync_sends_nothing() {
    let three = three().await;
    three.vault.relay.set_failing(
        Route::Push,
        Some(crate::services::sync::relay::RelayError::Server(503)),
    );
    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;
    assert!(
        left_record(&three.vault.a.lock(), A).is_some(),
        "written; the push failed"
    );

    three.vault.relay.set_failing(Route::Push, None);
    setup::turn_off_between_rounds(&three.vault.b, three.dir.path())
        .await
        .expect("off");
    let calls = three.vault.relay.calls().len();
    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.b,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;
    assert_eq!(three.vault.relay.calls().len(), calls, "nothing is asked");
}

/// A device another one removed does not sign a `left` record on its way out (its key is gone).
#[tokio::test]
async fn a_removed_device_does_not_leave() {
    let three = three().await;
    record_removal(&three.vault.a.lock(), A, C);
    round_of(&three.vault, &three.vault.a).await;
    let report = round_of(&three.vault, &three.c).await;
    assert!(report.removed_here, "{report:?}");
    let calls = three.vault.relay.calls().len();

    setup::leave_best_effort(
        &three.vault.relay,
        &three.c,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;

    assert_eq!(three.vault.relay.calls().len(), calls);
    assert_eq!(
        count(&three.c.lock(), "SELECT COUNT(*) FROM sync_devices"),
        0
    );
}

/// A removal record naming this device that reaches it through sync (no key change came with it)
/// ends sync here: the notice names the remover, local data stays.
#[tokio::test]
async fn a_removal_record_arriving_through_sync_turns_sync_off() {
    let three = three().await;
    institution(&three.c.lock(), "c-local");
    record_removal(&three.vault.a.lock(), A, C);
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");

    let report = round_of(&three.vault, &three.c).await;

    assert!(report.removed_here, "{report:?}");
    {
        let c = three.c.lock();
        assert!(!sync_on(&c));
        assert!(has_institution(&c, "c-local"));
        assert_eq!(
            rekey::removed_notice(&c).expect("notice"),
            Some(RemovedNotice {
                by: "Mac".into(),
                at: NOW,
                unsent: 1,
                left: false,
            })
        );
    }
    // B only learns that C is removed.
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        state_of(&three.vault.b.lock(), C),
        Some(TrustState::Removed)
    );
}

/// A `left` record for this device that it did not sign (a copy of its profile left, or someone
/// who holds its key made it) does the same.
#[tokio::test]
async fn a_left_record_for_this_device_it_did_not_sign_turns_sync_off() {
    let three = three().await;
    // The record is made with C's own key, as another holder of that key would, and written into
    // C's row on A's copy of the list.
    let seed = identity::own_seed(&three.c.lock())
        .expect("seed")
        .expect("key");
    let record = Record::sign(
        RecordType::Left,
        three.vault.creds.vault_id.0,
        C,
        C,
        NOW,
        0,
        &seed,
    )
    .expect("left");
    three
        .vault
        .a
        .lock()
        .execute(
            "UPDATE sync_devices SET left_record = ?1 WHERE id = ?2",
            rusqlite::params![record.to_b64(), C.to_hex()],
        )
        .expect("row");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.c).await;

    assert!(report.removed_here, "{report:?}");
    assert!(!sync_on(&three.c.lock()));
    let notice = rekey::removed_notice(&three.c.lock())
        .expect("notice")
        .expect("written");
    // Its own `left` record: the notice says so, by this device's own name (review of Tasks
    // 20–21, finding 1), never "removed by".
    assert!(notice.left, "{notice:?}");
    assert_eq!(notice.by, "iPad");
}

/// A record that does not verify (a removal by a device that is not trusted here, say) changes
/// nothing: sync goes on.
#[tokio::test]
async fn a_record_that_does_not_verify_changes_nothing() {
    let three = three().await;
    // A removal of C signed by a key nobody here knows, in C's row.
    let stranger = identity::new_sign_seed().expect("seed");
    let record = Record::sign(
        RecordType::Removed,
        three.vault.creds.vault_id.0,
        C,
        B,
        NOW,
        0,
        &stranger,
    )
    .expect("removal");
    three
        .vault
        .a
        .lock()
        .execute(
            "UPDATE sync_devices SET removal_record = ?1 WHERE id = ?2",
            rusqlite::params![record.to_b64(), C.to_hex()],
        )
        .expect("row");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.c).await;

    assert!(!report.removed_here, "{report:?}");
    assert_eq!(report.problem, None, "{report:?}");
    assert!(sync_on(&three.c.lock()));
}

/// The refresh also runs at unlock: a removal that merged into the device list while no refresh
/// ran (the engine was not running) ends sync when the engine starts.
#[tokio::test]
async fn the_store_catches_up_at_unlock_and_a_removal_ends_sync_there() {
    let three = three().await;
    record_removal(&three.vault.a.lock(), A, C);
    round_of(&three.vault, &three.vault.a).await;
    // C merged the row without the hook (written by hand, as a merge before this build did).
    let record: String = three
        .vault
        .a
        .lock()
        .query_row(
            "SELECT removal_record FROM sync_devices WHERE id = ?1",
            [C.to_hex()],
            |r| r.get(0),
        )
        .expect("record");
    three
        .c
        .lock()
        .execute(
            "UPDATE sync_devices SET removal_record = ?1 WHERE id = ?2",
            rusqlite::params![record, C.to_hex()],
        )
        .expect("row");

    let running = super::runtime_loop::start(
        &three.vault,
        &three.c,
        tempfile::tempdir().expect("dir"),
        Box::new(Sealing),
    );
    until("the loop ended", || !running.handle.is_running()).await;

    assert!(!sync_on(&three.c.lock()));
    assert!(rekey::removed_notice(&three.c.lock())
        .expect("notice")
        .is_some());
}

/// The sync server, except that a push takes 30 s to answer.
struct SlowPush(FakeRelay);

impl Relay for SlowPush {
    async fn server_info(&self) -> RelayResult<ServerInfo> {
        self.0.server_info().await
    }
    async fn create_account(&self) -> RelayResult<String> {
        self.0.create_account().await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.0.account_status(account).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.0.checkout(account, plan).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.0.create_vault(account, vault).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.0.vault_info(vault).await
    }
    async fn push(
        &self,
        vault: &VaultAuth,
        ops: &[crate::services::sync::relay::OutgoingOp],
    ) -> RelayResult<PushReceipt> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        self.0.push(vault, ops).await
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.0.pull(vault, after, limit).await
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.0.put_snapshot(vault, seq, bytes).await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.0.get_snapshot(vault).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.0.delete_vault(vault).await
    }
    async fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.0.rotate_token(vault, new_token, bundle).await
    }
    async fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: crate::services::sync::relay::KitRotation<'_>,
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.0
            .rotate_token_with_kit(vault, new_token, bundle, kit)
            .await
    }
    async fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> RelayResult<bool> {
        self.0.put_kit(vault, kit_token, envelope).await
    }
    async fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> RelayResult<crate::services::sync::relay::KitRead> {
        self.0.get_kit(vault_id, kit_token).await
    }
    async fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::RekeyPage> {
        self.0.rekeys(vault, after, limit).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        self.0.put_blob(vault, blob, sealed_len, body).await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<Option<u64>> {
        self.0.head_blob(vault, blob).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        self.0.get_blob(vault, blob, sink).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        self.0.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
        self.0.delete_blob(vault, blob).await
    }
}

/// Review of Tasks 20–21, finding 1: a `left` record this device signed is in its own row (a
/// turn-off that failed after the leave): at the next unlock the store's refresh ends sync with
/// the notice that this device left, not that some device removed it.
#[tokio::test]
async fn an_own_left_record_ends_sync_at_unlock_with_the_left_notice() {
    let three = three().await;
    let dir = tempfile::tempdir().expect("dir");
    assert!(setup::write_left_record(&three.c.lock(), NOW).expect("left"));

    let ended = rekey::refresh_trust(&mut three.c.lock(), Some(dir.path()), NOW).expect("refresh");

    assert!(ended);
    let c = three.c.lock();
    assert!(!sync_on(&c));
    let notice = rekey::removed_notice(&c).expect("notice").expect("written");
    assert!(notice.left, "{notice:?}");
    assert_eq!(notice.by, "iPad");
}

/// Review of Tasks 20–21, finding 3: a `left` record naming this device that its key did not sign
/// (a stranger's) changes nothing here.
#[tokio::test]
async fn a_left_record_of_this_device_signed_by_a_stranger_changes_nothing() {
    let three = three().await;
    let stranger = identity::new_sign_seed().expect("seed");
    let record = Record::sign(
        RecordType::Left,
        three.vault.creds.vault_id.0,
        C,
        C,
        NOW,
        0,
        &stranger,
    )
    .expect("left");
    three
        .vault
        .a
        .lock()
        .execute(
            "UPDATE sync_devices SET left_record = ?1 WHERE id = ?2",
            rusqlite::params![record.to_b64(), C.to_hex()],
        )
        .expect("row");
    round_of(&three.vault, &three.vault.a).await;

    let report = round_of(&three.vault, &three.c).await;

    assert!(!report.removed_here, "{report:?}");
    assert_eq!(report.problem, None, "{report:?}");
    let c = three.c.lock();
    assert!(sync_on(&c));
    assert!(rekey::removed_notice(&c).expect("notice").is_none());
    assert_eq!(state_of(&c, C), Some(TrustState::Trusted));
}

/// Review of Tasks 20–21, finding 2: a refresh that fails after a merge brought a `left` record is
/// owed: the plan of a key change refuses until one runs, and the next round runs it first, so the
/// device that left is never a recipient.
#[tokio::test]
async fn a_failed_refresh_is_owed_until_one_runs() {
    let three = three().await;
    assert!(setup::write_left_record(&three.vault.b.lock(), NOW).expect("left"));
    round_of(&three.vault, &three.vault.b).await;
    // A store row that does not read makes every refresh on A fail.
    let broken = "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, state, \
                    pinned_at) VALUES ('ee', 'not a certificate', 2, 'ee', x'00', x'00', \
                    'trusted', 0)";
    three
        .vault
        .a
        .lock()
        .execute(broken, [])
        .expect("broken row");

    round_of(&three.vault, &three.vault.a).await;

    {
        let a = three.vault.a.lock();
        assert!(left_record(&a, B).is_some(), "the record merged");
        assert!(rekey::refresh_owed(&a).expect("owed"));
        assert!(
            rotate::plan(&a, &[C], &[], false, NOW).is_err(),
            "no plan on a stale store"
        );
        a.execute("DELETE FROM sync_trust WHERE device = 'ee'", [])
            .expect("repaired");
    }
    let calls = three.vault.relay.calls().len();
    round_of(&three.vault, &three.vault.a).await;
    let a = three.vault.a.lock();
    assert!(three.vault.relay.calls().len() > calls);
    assert!(!rekey::refresh_owed(&a).expect("owed"));
    assert_eq!(state_of(&a, B), Some(TrustState::Left));
    let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
    assert!(plan.recipients.iter().all(|e| e.device != B));
}

/// Review of Tasks 20–21, confirm item: this device's own entry is pinned once, by
/// `identity::create_own` (`trust::seed_own`: setup, every join, a re-join), with its own signing
/// key; another certificate in its own row later is only a conflict (`OtherCertificate`) and the
/// pinned one stays.
#[tokio::test]
async fn another_certificate_in_this_devices_own_row_stays_a_conflict() {
    let three = three().await;
    let a = three.vault.a.lock();
    let pinned = trust::entry(&a, A).expect("entry").expect("pinned");
    let other = identity::cert::issue_self(
        identity::CertKind::Genesis,
        &identity::cert::CertFields {
            vault_id: three.vault.creds.vault_id.0,
            device: A,
            box_pub: [0x0a; 32],
            issued_at: NOW,
        },
        &identity::new_sign_seed().expect("seed"),
    )
    .expect("cert");
    a.execute(
        "UPDATE sync_devices SET cert = ?1 WHERE id = ?2",
        rusqlite::params![other.to_b64(), A.to_hex()],
    )
    .expect("row");

    let report = trust::refresh(&a, NOW).expect("refresh");

    assert!(report
        .conflicts
        .iter()
        .any(|c| c.device == A && c.kind == trust::ConflictKind::OtherCertificate));
    let after = trust::entry(&a, A).expect("entry").expect("pinned");
    assert_eq!(after.cert, pinned.cert);
    assert_eq!(
        after.sign_pub,
        sign_pub(&identity::own_seed(&a).expect("seed").expect("seed"))
    );
}
