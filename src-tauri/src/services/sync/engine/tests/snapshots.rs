//! Snapshots in the engine: the upload policy, the sealed dump and its caps, the restore by merge
//! after `410 snapshot_required`, and the restore that heals a below-barrier miss.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

use super::*;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::push::push_all;
use crate::services::sync::engine::snapshots::{
    dump_sealed, dump_sealed_capped, heal_missed, inflate_capped, maybe_upload, maybe_upload_with,
    open_sealed, open_sealed_capped, restore_after_410, Due, SnapshotPolicy, MAX_SNAPSHOT_BYTES,
    MAX_SNAPSHOT_PLAIN, SNAPSHOT_BYTES, SNAPSHOT_ENVELOPES, SNAPSHOT_MIN_INTERVAL,
};
use crate::services::sync::engine::{PullReport, PullStop};
use crate::services::sync::envelope::stream::{snapshot_object_id, StreamSealer};
use crate::services::sync::error::{SyncError, SyncResult, TooLarge, Transient};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{RelayError, SnapshotBlob, VaultInfo};
use crate::services::sync::state::EngineMarks;

const HOUR: i64 = 3_600;
const DAY: i64 = 86_400;

fn info(head: u64, snapshot_seq: Option<u64>) -> VaultInfo {
    VaultInfo {
        head,
        floor: 0,
        snapshot_seq,
        paid_until: NOW + 365 * DAY,
        bytes: 0,
        rotation: 0,
    }
}

fn marks(pulled_bytes: u64, snapshot_uploaded_at: Option<i64>) -> EngineMarks {
    EngineMarks {
        marker_barrier: 0,
        capture_broken_at: None,
        vault_gone_at: None,
        pulled_bytes,
        pulled_bytes_base: 0,
        snapshot_uploaded_at,
        snapshot_too_large_at: None,
        binding_written: false,
        last_success_at: None,
        last_error: None,
    }
}

fn puts(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .iter()
        .filter(|r| **r == Route::PutSnapshot)
        .count()
}

fn gets(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .iter()
        .filter(|r| **r == Route::GetSnapshot)
        .count()
}

async fn vault_info(vault: &Vault) -> VaultInfo {
    vault
        .relay
        .vault_info(&vault.creds.auth())
        .await
        .expect("vault info")
}

/// `maybe_upload` on `device` with the relay's current vault info.
async fn upload(
    vault: &Vault,
    device: &TestConn,
    policy: &mut SnapshotPolicy,
    now: i64,
    force: bool,
) -> SyncResult<bool> {
    let info = vault_info(vault).await;
    maybe_upload(
        &vault.relay,
        device,
        &vault.creds,
        policy,
        &info,
        now,
        0,
        force,
    )
    .await
}

/// Writes `id` on `device`, then pushes and pulls it, so the device's cursor covers it.
async fn write_push_pull(vault: &Vault, device: &TestConn, id: &str) {
    institution(&device.lock(), id);
    push_all(&vault.relay, device, &vault.creds)
        .await
        .expect("push");
    assert!(pull_all(&vault.relay, device, &vault.creds, NOW)
        .await
        .expect("pull")
        .done());
}

/// `n` opaque ops, so the relay's head moves without any device's chain.
async fn opaque_ops(vault: &Vault, n: u8) {
    let cts: Vec<Vec<u8>> = (1..=n).map(|i| vec![i; 200]).collect();
    push(&vault.relay, &vault.creds, &cts).await;
}

/// The plaintext record stream `snapshot::dump` writes for `conn` at `seq`.
fn plain_dump(conn: &Connection, seq: u64) -> Vec<u8> {
    let mut out = Vec::new();
    snapshot::dump(conn, seq, &mut out).expect("dump");
    out
}

fn last_error(conn: &Connection) -> Option<String> {
    state::marks(conn).expect("marks").expect("row").last_error
}

fn missed(conn: &Connection) -> Option<u64> {
    state::load(conn)
        .expect("state")
        .expect("row")
        .missed_below_barrier
}

/// `plain` (a record stream), raw-deflated and sealed for relay sequence `seq`.
fn seal_plain(creds: &Credentials, seq: u64, plain: &[u8]) -> Vec<u8> {
    let mut deflate = DeflateEncoder::new(
        StreamSealer::new(
            creds.keys.current_epoch(),
            creds.keys.current(),
            &creds.vault_id.0,
            &snapshot_object_id(seq),
        ),
        Compression::default(),
    );
    deflate.write_all(plain).expect("deflate");
    deflate.finish().expect("deflate").finish().expect("seal")
}

/// `conn`'s dump at `seq` with the header's schema number replaced by `schema`, sealed.
fn sealed_with_schema(creds: &Credentials, conn: &Connection, seq: u64, schema: u32) -> Vec<u8> {
    let plain = plain_dump(conn, seq);
    let from = format!("\"schema\":{}", schema_number()).into_bytes();
    let to = format!("\"schema\":{schema}").into_bytes();
    assert_eq!(from.len(), to.len(), "the record lengths stay");
    let at: Vec<usize> = plain
        .windows(from.len())
        .enumerate()
        .filter(|(_, w)| *w == from.as_slice())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(at.len(), 1, "only the header names the schema");
    let mut patched = plain;
    patched[at[0]..at[0] + to.len()].copy_from_slice(&to);
    seal_plain(creds, seq, &patched)
}

/// `sync_rows` without the `sync_state` row (where a stopped restore notes `last_error`).
fn rows_but_state(conn: &Connection) -> Vec<String> {
    sync_rows(conn)
        .into_iter()
        .filter(|line| !line.starts_with("SELECT * FROM sync_state"))
        .collect()
}

fn has_key(error: &SyncError, expected: &str) -> bool {
    matches!(error, SyncError::Local(AppError::Validation(key)) if key == expected)
}

fn invalid_snapshot(error: &SyncError) -> bool {
    matches!(error, SyncError::Local(AppError::Validation(key)) if key == "validation.syncSnapshotInvalid")
}

// --- The policy ---------------------------------------------------------------------------------

#[test]
fn policy_uploads_at_once_when_the_vault_has_no_snapshot() {
    let mut policy = SnapshotPolicy::default();
    // No delay and no daily limit: the vault has nothing to restore from.
    assert_eq!(
        policy.decide(&info(0, None), &marks(0, Some(NOW - 60)), NOW, 1_800),
        Due::Now
    );
    assert_eq!(policy.scheduled_at, None);
    assert_eq!(policy.checked_at, Some(NOW));
}

#[test]
fn policy_waits_for_2000_envelopes_or_8_mib() {
    let long_ago = Some(NOW - 2 * DAY);
    let decide = |head: u64, bytes: u64| {
        SnapshotPolicy::default().decide(&info(head, Some(100)), &marks(bytes, long_ago), NOW, 0)
    };
    assert_eq!(SNAPSHOT_ENVELOPES, 2_000);
    assert_eq!(SNAPSHOT_BYTES, 8 * 1024 * 1024);
    assert_eq!(decide(100 + 1_999, 0), Due::No);
    assert_eq!(decide(100 + 2_000, 0), Due::Now);
    assert_eq!(decide(100, 8 * 1024 * 1024 - 1), Due::No);
    assert_eq!(decide(100, 8 * 1024 * 1024), Due::Now);
    assert_eq!(
        decide(100 + 1_999, 8 * 1024 * 1024 - 1),
        Due::No,
        "the two are not added up"
    );
}

#[test]
fn policy_uploads_at_most_once_a_day_after_the_random_delay() {
    let piled_up = info(5_000, Some(100));
    // Uploaded less than a day ago: not due, however much piled up.
    let mut policy = SnapshotPolicy::default();
    let recent = marks(0, Some(NOW - SNAPSHOT_MIN_INTERVAL + 1));
    assert_eq!(policy.decide(&piled_up, &recent, NOW, 600), Due::No);
    assert_eq!(policy.scheduled_at, None);

    // A day ago: due, scheduled after the delay drawn by the caller, and kept on later checks
    // (a later draw does not move it).
    let a_day = marks(0, Some(NOW - SNAPSHOT_MIN_INTERVAL));
    assert_eq!(
        policy.decide(&piled_up, &a_day, NOW, 600),
        Due::At(NOW + 600)
    );
    assert_eq!(policy.scheduled_at, Some(NOW + 600));
    assert_eq!(
        policy.decide(&piled_up, &a_day, NOW + 599, 3_000),
        Due::At(NOW + 600)
    );
    assert_eq!(policy.decide(&piled_up, &a_day, NOW + 600, 3_000), Due::Now);

    // The delay is at most an hour.
    let mut capped = SnapshotPolicy::default();
    assert_eq!(
        capped.decide(&piled_up, &a_day, NOW, 10 * HOUR),
        Due::At(NOW + HOUR)
    );

    // No longer due (another device uploaded): the schedule is dropped.
    assert_eq!(
        policy.decide(&info(5_000, Some(5_000)), &a_day, NOW + 700, 0),
        Due::No
    );
    assert_eq!(policy.scheduled_at, None);
}

#[test]
fn policy_asks_for_vault_info_at_most_hourly() {
    let mut policy = SnapshotPolicy::default();
    assert!(policy.wants_info(NOW), "never checked");
    let not_due = info(10, Some(5));
    policy.decide(&not_due, &marks(0, None), NOW, 0);
    assert!(!policy.wants_info(NOW + HOUR - 1));
    assert!(policy.wants_info(NOW + HOUR));

    // A schedule that falls due asks before the hour is up.
    let due = info(5_000, Some(5));
    assert_eq!(
        policy.decide(&due, &marks(0, None), NOW + HOUR, 900),
        Due::At(NOW + HOUR + 900)
    );
    assert!(!policy.wants_info(NOW + HOUR + 899));
    assert!(policy.wants_info(NOW + HOUR + 900));
}

#[test]
fn only_a_pull_that_ended_done_may_be_followed_by_a_dump() {
    let done = PullReport::default();
    let held = PullReport {
        held: 1,
        ..PullReport::default()
    };
    let unfinished = PullReport {
        unfinished: true,
        ..PullReport::default()
    };
    let newer = PullReport {
        stop: Some(PullStop::NewerSchema),
        ..PullReport::default()
    };
    let gap = PullReport {
        stop: Some(PullStop::Gap),
        ..PullReport::default()
    };
    let verdicts: Vec<(bool, bool)> = [&done, &held, &unfinished, &newer, &gap]
        .iter()
        .map(|r| (r.may_snapshot(), r.may_push()))
        .collect();
    assert_eq!(
        verdicts,
        vec![
            (true, true),
            (false, true),
            (false, false),
            (false, true),
            (false, false)
        ]
    );
}

// --- Upload -------------------------------------------------------------------------------------

#[tokio::test]
async fn upload_waits_for_an_empty_outbox_and_unsent() {
    let vault = vault().await;
    let mut policy = SnapshotPolicy::default();

    institution(&vault.a.lock(), "a1");
    assert!(
        !upload(&vault, &vault.a, &mut policy, NOW, true)
            .await
            .expect("upload"),
        "the outbox holds a1"
    );
    seal_batch_a(&vault);
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_outbox"),
        0
    );
    assert!(
        !upload(&vault, &vault.a, &mut policy, NOW, true)
            .await
            .expect("upload"),
        "a1 waits in sync_unsent"
    );
    assert_eq!(puts(&vault.relay), 0);

    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push");
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert!(upload(&vault, &vault.a, &mut policy, NOW, false)
        .await
        .expect("upload"));
    assert_eq!(vault.relay.snapshot_seq(), Some(1));
}

fn seal_batch_a(vault: &Vault) {
    crate::services::sync::engine::push::seal_batch(&vault.a, &vault.creds).expect("seal");
}

#[tokio::test]
async fn the_snapshot_is_dumped_at_the_applied_cursor() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    // B pushes after A's pull: the relay's head (2) is past what A applied (1).
    institution(&vault.b.lock(), "b1");
    push_all(&vault.relay, &vault.b, &vault.creds)
        .await
        .expect("push B");
    let info = vault_info(&vault).await;
    assert_eq!(info.head, 2);
    let mut policy = SnapshotPolicy::default();

    let uploaded = maybe_upload(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &mut policy,
        &info,
        NOW,
        0,
        false,
    )
    .await
    .expect("upload");

    assert!(uploaded);
    assert_eq!(vault.relay.snapshot_seq(), Some(1));
    let blob = vault
        .relay
        .get_snapshot(&vault.creds.auth())
        .await
        .expect("snapshot");
    let plain = open_sealed(&vault.creds, &blob).expect("open");
    assert_eq!(*plain, plain_dump(&vault.a.lock(), 1));
    let a = vault.a.lock();
    let marks = state::marks(&a).expect("marks").expect("row");
    assert_eq!(marks.snapshot_uploaded_at, Some(NOW));
    assert_eq!(
        (marks.pulled_bytes, marks.pulled_bytes_base),
        (0, 1),
        "bytes count from the new snapshot"
    );
    assert_eq!(count(&a, "SELECT snapshot_seq FROM sync_state"), 1);
}

#[tokio::test]
async fn a_device_with_a_pending_miss_never_uploads() {
    let vault = vault().await;
    state::note_missed_below_barrier(&vault.a.lock(), 5).expect("miss");
    let mut policy = SnapshotPolicy::default();

    // Neither the policy (no snapshot yet: due at once) nor the forced upload after a crossing.
    for force in [false, true] {
        assert!(!upload(&vault, &vault.a, &mut policy, NOW, force)
            .await
            .expect("upload"));
    }

    assert_eq!(puts(&vault.relay), 0);
    assert_eq!(vault.relay.snapshot_seq(), None);
}

#[tokio::test]
async fn an_outdated_snapshot_is_dropped_quietly() {
    let vault = vault().await;
    opaque_ops(&vault, 2).await;
    exec(
        &vault.a.lock(),
        "UPDATE sync_state SET pulled_bytes = 8388608, pulled_bytes_base = 0",
    );
    // The info this device works from: the vault's snapshot at 0, 8 MiB pulled since.
    let stale = info(2, Some(0));
    let mut policy = SnapshotPolicy::default();
    let first = maybe_upload(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &mut policy,
        &stale,
        NOW,
        600,
        false,
    )
    .await
    .expect("upload");
    assert!(!first);
    assert_eq!(policy.scheduled_at, Some(NOW + 600));
    // Meanwhile another device uploads a newer snapshot.
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 2, vec![7; 100])
        .await
        .expect("other upload");

    let second = maybe_upload(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &mut policy,
        &stale,
        NOW + 600,
        600,
        false,
    )
    .await
    .expect("an outdated snapshot is no error");

    assert!(!second);
    assert_eq!(puts(&vault.relay), 2);
    assert_eq!(policy.scheduled_at, None);
    assert_eq!(vault.relay.snapshot_seq(), Some(2));
    let marks = state::marks(&vault.a.lock()).expect("marks").expect("row");
    assert_eq!(marks.snapshot_uploaded_at, None);
    assert_eq!(marks.last_error, None);
}

#[tokio::test]
async fn an_oversized_snapshot_is_not_uploaded() {
    let vault = vault().await;
    let info = vault_info(&vault).await;
    let mut policy = SnapshotPolicy::default();

    let error = maybe_upload_with(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &mut policy,
        &info,
        NOW,
        0,
        false,
        1024,
    )
    .await
    .expect_err("too large");

    assert!(matches!(error, SyncError::TooLarge(TooLarge::Snapshot)));
    assert_eq!(vault.relay.snapshot_seq(), None);
    assert_eq!(puts(&vault.relay), 0, "nothing was sent");
    let too_large_at = |vault: &Vault| {
        state::marks(&vault.a.lock())
            .expect("marks")
            .expect("row")
            .snapshot_too_large_at
    };
    assert_eq!(too_large_at(&vault), Some(NOW));
    // No retry until the next day, forced or not, also after a restart (a fresh policy). The
    // dump itself is skipped: under the same 1 KiB limit it would be too large again.
    for force in [false, true] {
        let mut restarted = SnapshotPolicy::default();
        let skipped = maybe_upload_with(
            &vault.relay,
            &vault.a,
            &vault.creds,
            &mut restarted,
            &info,
            NOW + SNAPSHOT_MIN_INTERVAL - 1,
            0,
            force,
            1024,
        )
        .await
        .expect("skipped, not too large again");
        assert!(!skipped);
    }
    assert_eq!(puts(&vault.relay), 0);
    let mut restarted = SnapshotPolicy::default();
    assert!(upload(
        &vault,
        &vault.a,
        &mut restarted,
        NOW + SNAPSHOT_MIN_INTERVAL,
        false
    )
    .await
    .expect("upload"));
    assert_eq!(vault.relay.snapshot_seq(), Some(0));
    assert_eq!(too_large_at(&vault), None, "the upload clears the mark");
}

#[tokio::test]
async fn a_snapshot_the_relay_finds_too_large_waits_a_day() {
    let vault = vault().await;
    vault
        .relay
        .fail_next(Route::PutSnapshot, RelayError::SnapshotTooLarge);
    let mut policy = SnapshotPolicy::default();

    let error = upload(&vault, &vault.a, &mut policy, NOW, true)
        .await
        .expect_err("too large");

    assert!(matches!(error, SyncError::TooLarge(TooLarge::Snapshot)));
    let mut restarted = SnapshotPolicy::default();
    assert!(!upload(&vault, &vault.a, &mut restarted, NOW + HOUR, true)
        .await
        .expect("upload"));
    assert_eq!(puts(&vault.relay), 1);
    assert_eq!(vault.relay.snapshot_seq(), None);
}

// --- The sealed dump and its caps ---------------------------------------------------------------

#[tokio::test]
async fn a_sealed_dump_opens_to_the_same_record_stream() {
    let vault = vault().await;
    let a = vault.a.lock();
    institution(&a, "a1");
    exec(
        &a,
        "DELETE FROM sync_outbox; UPDATE sync_state SET cursor = 4",
    );

    let sealed = dump_sealed(&a, &vault.creds).expect("dump");

    assert_eq!(sealed.seq, 4, "S is the cursor");
    let mut plain = Vec::new();
    let stats = snapshot::dump(&a, 4, &mut plain).expect("dump");
    assert_eq!(sealed.stats, stats);
    let blob = SnapshotBlob {
        seq: sealed.seq,
        bytes: sealed.bytes.clone(),
    };
    assert_eq!(*open_sealed(&vault.creds, &blob).expect("open"), plain);
    assert!(a.is_autocommit(), "the dump's transaction is closed");
}

#[tokio::test]
async fn a_snapshot_served_under_another_sequence_fails() {
    let vault = vault().await;
    let sealed = {
        let a = vault.a.lock();
        exec(&a, "UPDATE sync_state SET cursor = 5");
        dump_sealed(&a, &vault.creds).expect("dump")
    };
    assert_eq!(sealed.seq, 5);
    opaque_ops(&vault, 6).await;
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 6, sealed.bytes.clone())
        .await
        .expect("mislabelled upload");
    let before = sync_rows(&vault.b.lock());

    let error = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect_err("another sequence");

    assert!(invalid_snapshot(&error), "{error:?}");
    assert_eq!(sync_rows(&vault.b.lock()), before, "nothing written");

    // Sealed for 6 but dumped at 5: the header names another sequence.
    let bytes = seal_plain(&vault.creds, 6, &plain_dump(&vault.a.lock(), 5));
    let error = open_sealed(&vault.creds, &SnapshotBlob { seq: 6, bytes }).expect_err("header");
    assert!(invalid_snapshot(&error), "{error:?}");
}

#[tokio::test]
async fn the_sealed_dump_is_bounded_on_both_sides_of_the_deflate() {
    let vault = vault().await;
    let a = vault.a.lock();
    let plain_len = plain_dump(&a, 0).len();
    let sealed_len = dump_sealed(&a, &vault.creds).expect("dump").bytes.len();
    let dump = |max_bytes, max_plain| {
        dump_sealed_capped(&a, &vault.creds, max_bytes, max_plain)
            .expect("dump")
            .map(|s| s.bytes.len())
    };

    assert_eq!(dump(sealed_len, plain_len), Some(sealed_len));
    assert_eq!(dump(sealed_len - 1, plain_len), None, "sealed size");
    assert_eq!(dump(sealed_len, plain_len - 1), None, "record stream size");
    assert_eq!(dump(100, MAX_SNAPSHOT_PLAIN), None, "stops writing early");
    assert!(a.is_autocommit());
}

#[tokio::test]
async fn a_download_over_the_cap_is_refused_before_it_opens() {
    let vault = vault().await;
    assert_eq!(MAX_SNAPSHOT_BYTES, 104_857_600);
    let over = SnapshotBlob {
        seq: 1,
        bytes: vec![0; MAX_SNAPSHOT_BYTES + 1],
    };
    let at_cap = SnapshotBlob {
        seq: 1,
        bytes: vec![0; MAX_SNAPSHOT_BYTES],
    };

    assert!(matches!(
        open_sealed(&vault.creds, &over),
        Err(SyncError::TooLarge(TooLarge::Body))
    ));
    let error = open_sealed(&vault.creds, &at_cap).expect_err("not a stream");
    assert!(
        invalid_snapshot(&error),
        "at the cap it is opened: {error:?}"
    );
}

#[tokio::test]
async fn an_inflated_snapshot_is_bounded() {
    let vault = vault().await;
    let sealed = dump_sealed(&vault.a.lock(), &vault.creds).expect("dump");
    let plain_len = plain_dump(&vault.a.lock(), 0).len();
    let blob = SnapshotBlob {
        seq: sealed.seq,
        bytes: sealed.bytes,
    };

    assert_eq!(
        open_sealed_capped(&vault.creds, &blob, plain_len)
            .expect("open")
            .len(),
        plain_len
    );
    assert!(matches!(
        open_sealed_capped(&vault.creds, &blob, plain_len - 1),
        Err(SyncError::TooLarge(TooLarge::Body))
    ));
}

#[test]
fn inflating_grows_its_buffer_up_to_the_cap() {
    // 1 MiB of zeros deflates to about a kilobyte: the buffer grows many times.
    const MIB: usize = 1024 * 1024;
    let mut deflate = DeflateEncoder::new(Vec::new(), Compression::default());
    deflate.write_all(&vec![0u8; MIB]).expect("deflate");
    let deflated = deflate.finish().expect("deflate");
    assert!(deflated.len() * 4 < MIB);

    assert_eq!(
        *inflate_capped(&deflated, MIB).expect("inflate"),
        vec![0u8; MIB]
    );
    assert!(matches!(
        inflate_capped(&deflated, MIB - 1),
        Err(SyncError::TooLarge(TooLarge::Body))
    ));
    let cut = inflate_capped(&deflated[..deflated.len() / 2], MIB);
    assert!(
        matches!(&cut, Err(error) if invalid_snapshot(error)),
        "a stream cut short does not inflate"
    );
}

// --- Restores -----------------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_required_restores_by_merge_and_keeps_the_outbox() {
    let vault = vault().await;
    // B is offline with a local edit.
    institution(&vault.b.lock(), "b1");
    // A edits, pushes, pulls and uploads at S = 2; the relay prunes up to it.
    write_push_pull(&vault, &vault.a, "a1").await;
    write_push_pull(&vault, &vault.a, "a2").await;
    let mut policy = SnapshotPolicy::default();
    assert!(upload(&vault, &vault.a, &mut policy, NOW, false)
        .await
        .expect("upload"));
    assert_eq!(vault.relay.snapshot_seq(), Some(2));
    vault.relay.prune_to(2);

    let pulled = pull_all(&vault.relay, &vault.b, &vault.creds, NOW).await;
    assert!(matches!(pulled, Err(SyncError::SnapshotRequired)));
    let report = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("restore");

    assert!(report.stats.rows > 0);
    {
        let b = vault.b.lock();
        assert!(has_institution(&b, "a1") && has_institution(&b, "a2"));
        assert!(has_institution(&b, "b1"));
        assert_eq!(cursor(&b), 2);
        assert_eq!(
            count(
                &b,
                "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'institutions' AND pk = 'b1'"
            ),
            1,
            "B's edit is still queued"
        );
        assert_eq!(last_error(&b), None);
    }
    // B goes on from S, and its edit reaches A.
    assert!(pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("pull")
        .done());
    push_all(&vault.relay, &vault.b, &vault.creds)
        .await
        .expect("push B");
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull A");
    assert!(has_institution(&vault.a.lock(), "b1"));
}

#[tokio::test]
async fn a_restore_that_stops_is_noted_with_a_fixed_text() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    let mut policy = SnapshotPolicy::default();
    assert!(upload(&vault, &vault.a, &mut policy, NOW, false)
        .await
        .expect("upload"));
    // B crossed a barrier the snapshot is below: the restore refuses it.
    exec(
        &vault.b.lock(),
        "UPDATE sync_state SET barrier = barrier + 1",
    );

    let error = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect_err("below the barrier");

    assert!(invalid_snapshot(&error), "{error:?}");
    let b = vault.b.lock();
    assert_eq!(last_error(&b).as_deref(), Some("snapshot restore stopped"));
    assert!(!has_institution(&b, "a1"));
    assert_eq!(cursor(&b), 0);
}

#[tokio::test]
async fn a_newer_schema_snapshot_says_so_on_the_410_path() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    let bytes = sealed_with_schema(&vault.creds, &vault.a.lock(), 1, schema_number() + 1);
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 1, bytes)
        .await
        .expect("upload");
    let before = rows_but_state(&vault.b.lock());

    let error = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect_err("newer schema");

    assert!(
        has_key(&error, "validation.syncSnapshotNewerSchema"),
        "{error:?}"
    );
    let b = vault.b.lock();
    assert_eq!(rows_but_state(&b), before, "nothing restored");
    assert_eq!(cursor(&b), 0);
    assert_eq!(last_error(&b).as_deref(), Some("snapshot restore stopped"));
}

#[tokio::test]
async fn a_local_failure_during_a_restore_is_passed_through() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    let mut policy = SnapshotPolicy::default();
    assert!(upload(&vault, &vault.a, &mut policy, NOW, false)
        .await
        .expect("upload"));
    // The restore's insert fails in SQLite (as on a full disk); `last_error` could still be written.
    exec(
        &vault.b.lock(),
        "CREATE TEMP TRIGGER fail_insert BEFORE INSERT ON main.institutions \
         BEGIN SELECT RAISE(ABORT, 'disk I/O error'); END",
    );

    let error = restore_after_410(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect_err("insert fails");

    assert!(
        matches!(&error, SyncError::Local(AppError::Database(_))),
        "a database error is not a damaged snapshot: {error:?}"
    );
    assert!(error.backs_off());
    let b = vault.b.lock();
    assert!(!has_institution(&b, "a1"));
    assert_eq!(last_error(&b), None, "not noted as a stopped restore");
}

#[tokio::test]
async fn a_pending_miss_heals_from_a_clean_snapshot_by_merge() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    // B skipped envelope 1 as below its barrier: its cursor is past it, the row is missing.
    {
        let b = vault.b.lock();
        state::note_missed_below_barrier(&b, 1).expect("miss");
        state::set_cursor(&b, 1).expect("cursor");
    }
    let mut policy = SnapshotPolicy::default();

    // No snapshot covers position 1 yet: nothing is fetched.
    let none = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &vault_info(&vault).await,
        NOW,
    )
    .await
    .expect("heal");
    assert!(none.is_none());
    assert_eq!(gets(&vault.relay), 0);

    // A's clean snapshot at 1 heals it, once the relay answers.
    assert!(upload(&vault, &vault.a, &mut policy, NOW, false)
        .await
        .expect("upload"));
    vault
        .relay
        .fail_next(Route::GetSnapshot, RelayError::Server(503));
    let failed = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &vault_info(&vault).await,
        NOW,
    )
    .await
    .expect_err("transient");
    assert!(matches!(failed, SyncError::Transient(Transient::Relay)));
    assert_eq!(
        policy.examined_seq, None,
        "a transient failure is no verdict"
    );
    let healed = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &vault_info(&vault).await,
        NOW,
    )
    .await
    .expect("heal");

    assert!(healed.is_some());
    let b = vault.b.lock();
    assert!(has_institution(&b, "a1"));
    assert_eq!(missed(&b), None);
    assert_eq!(gets(&vault.relay), 2);
}

#[tokio::test]
async fn a_snapshot_that_does_not_heal_is_not_restored_or_fetched_again() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    // A dumper with a pending miss of its own (its snapshot lacks what it skipped).
    let sealed = {
        let a = vault.a.lock();
        state::note_missed_below_barrier(&a, 1).expect("A's miss");
        dump_sealed(&a, &vault.creds).expect("dump")
    };
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), sealed.seq, sealed.bytes)
        .await
        .expect("upload");
    {
        let b = vault.b.lock();
        state::note_missed_below_barrier(&b, 1).expect("B's miss");
        state::set_cursor(&b, 1).expect("cursor");
    }
    let before = sync_rows(&vault.b.lock());
    let info = vault_info(&vault).await;
    let mut policy = SnapshotPolicy::default();

    for _ in 0..2 {
        let healed = heal_missed(
            &vault.relay,
            &vault.b,
            &vault.creds,
            &mut policy,
            &info,
            NOW,
        )
        .await
        .expect("heal");
        assert!(healed.is_none());
    }

    assert_eq!(gets(&vault.relay), 1, "the same snapshot is examined once");
    assert_eq!(sync_rows(&vault.b.lock()), before, "nothing restored");
    assert_eq!(missed(&vault.b.lock()), Some(1));
}

#[tokio::test]
async fn a_heal_from_a_newer_schema_snapshot_says_so() {
    let vault = vault().await;
    write_push_pull(&vault, &vault.a, "a1").await;
    let bytes = sealed_with_schema(&vault.creds, &vault.a.lock(), 1, schema_number() + 1);
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 1, bytes)
        .await
        .expect("upload");
    {
        let b = vault.b.lock();
        state::note_missed_below_barrier(&b, 1).expect("miss");
        state::set_cursor(&b, 1).expect("cursor");
    }
    let info = vault_info(&vault).await;
    let mut policy = SnapshotPolicy::default();

    let error = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &info,
        NOW,
    )
    .await
    .expect_err("newer schema");

    assert!(
        has_key(&error, "validation.syncSnapshotNewerSchema"),
        "{error:?}"
    );
    assert_eq!(
        policy.examined_seq,
        Some(1),
        "a validation stop is a verdict"
    );
    let b = vault.b.lock();
    assert!(!has_institution(&b, "a1"));
    assert_eq!(missed(&b), Some(1));
}

#[tokio::test]
async fn a_snapshot_that_does_not_open_is_not_fetched_again() {
    let vault = vault().await;
    opaque_ops(&vault, 2).await;
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 2, vec![7; 100])
        .await
        .expect("damaged upload");
    {
        let b = vault.b.lock();
        state::note_missed_below_barrier(&b, 1).expect("miss");
        state::set_cursor(&b, 1).expect("cursor");
    }
    let info = vault_info(&vault).await;
    let mut policy = SnapshotPolicy::default();

    let error = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &info,
        NOW,
    )
    .await
    .expect_err("does not open");
    assert!(invalid_snapshot(&error), "{error:?}");
    assert_eq!(policy.examined_seq, Some(2));
    let again = heal_missed(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &mut policy,
        &info,
        NOW,
    )
    .await
    .expect("not fetched again");

    assert!(again.is_none());
    assert_eq!(gets(&vault.relay), 1);
}
