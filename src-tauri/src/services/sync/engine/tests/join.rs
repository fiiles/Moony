//! Joining a vault by replace (spec §8, §11.3 D22): the joiner's cursor, identity, binding and
//! marks, and what a refused or failed join leaves behind. The helpers are shared with the repair
//! tests (`repair.rs`).

use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zeroize::Zeroizing;

use super::*;
use crate::services::sync::binding::{
    self, startup_check, BindingFailure, BindingNative, NoBinding, StartupCheck, REJOIN_MARKER,
};
use crate::services::sync::capture;
use crate::services::sync::devices;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::snapshots::MAX_SNAPSHOT_BYTES;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::error::{SyncError, SyncResult, TooLarge};
use crate::services::sync::hlc;
use crate::services::sync::join::join_by_replace;
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, OutgoingOp, PullPage, PushReceipt, RelayResult, SnapshotBlob,
    VaultInfo,
};
use crate::services::sync::snapshot::RestoreReport;
use crate::services::sync::test_support::assert_converged;

/// Seals by XOR with one byte: a binding that opens on this "machine" only.
pub(super) struct XorNative;

impl BindingNative for XorNative {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Ok(secret.iter().map(|b| b ^ 0x5a).collect())
    }

    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        let mut out = Zeroizing::new([0u8; 32]);
        if blob.len() != out.len() {
            return Err(BindingFailure::Failed("wrong length".into()));
        }
        for (o, b) in out.iter_mut().zip(blob) {
            *o = b ^ 0x5a;
        }
        Ok(out)
    }
}

pub(super) async fn round(vault: &Vault, device: &TestConn, mode: RoundMode) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        mode,
        NOW,
        0,
    )
    .await
    .expect("round")
}

/// `A` writes `id` and uploads its snapshot in a first-snapshot round; returns the snapshot's `S`.
pub(super) async fn snapshot_of_a(vault: &Vault, id: &str) -> u64 {
    institution(&vault.a.lock(), id);
    let report = round(vault, &vault.a, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");
    let seq = vault.relay.snapshot_seq().expect("the vault's snapshot");
    assert_eq!(seq, cursor(&vault.a.lock()), "S is A's applied cursor");
    seq
}

pub(super) async fn join(
    vault: &Vault,
    device: &TestConn,
    dir: &Path,
    native: &dyn BindingNative,
) -> SyncResult<RestoreReport> {
    join_by_replace(
        &vault.relay,
        device,
        vault.creds.clone(),
        "Laptop",
        dir,
        native,
        NOW,
    )
    .await
}

pub(super) fn device_of(conn: &Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

pub(super) fn validation_key(error: &SyncError) -> Option<&str> {
    match error {
        SyncError::Local(AppError::Validation(key)) => Some(key),
        _ => None,
    }
}

/// Everything a refused or failed join must leave as it was: the `sync_state` row, the heads,
/// the own log, the unsent envelopes, the outbox, the device rows and the institutions.
pub(super) fn profile(conn: &Connection) -> Vec<String> {
    let mut out = sync_rows(conn);
    let mut stmt = conn
        .prepare("SELECT id, public_key, name FROM sync_devices ORDER BY id")
        .expect("devices");
    let rows = stmt
        .query_map([], |r| {
            Ok(format!(
                "sync_devices: {:?} {:?} {:?}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?
            ))
        })
        .expect("devices");
    out.extend(rows.map(|r| r.expect("device row")));
    out
}

/// What a stopped repair must leave as it was (ruling P3): the device and its chain, the cursor
/// and heads, the pending changes in the outbox and `sync_unsent`, the capture mark and every
/// synced row. The round's own bookkeeping (`last_error`) may change.
pub(super) fn repair_state(conn: &Connection) -> Vec<String> {
    let mut out: Vec<String> = sync_rows(conn)
        .into_iter()
        .filter(|line| !line.starts_with("SELECT * FROM sync_state"))
        .collect();
    let sync = state::load(conn).expect("state").expect("row");
    let marks = state::marks(conn).expect("marks").expect("row");
    out.push(format!(
        "device {:?} next_seq {} last_hash {:?} cursor {} capture_broken_at {:?}",
        sync.device, sync.next_seq, sync.last_hash, sync.cursor, marks.capture_broken_at
    ));
    out
}

pub(super) fn repair_backups(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("data dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("sync-repair-"))
        .collect();
    names.sort();
    names
}

/// Makes `device` a capture-broken profile as an unlock leaves it: capture not installed on the
/// connection, `capture_broken_at` set, and the guard imposed (as `sync::on_open` does).
pub(super) fn break_capture(device: &TestConn) {
    let mut conn = device.lock();
    capture::uninstall(&conn).expect("uninstall");
    state::mark_capture_broken(&conn, NOW - 60).expect("mark");
    capture::guard::impose(&mut conn).expect("guard");
}

#[tokio::test]
async fn a_joined_device_matches_the_vault_and_starts_after_the_snapshot() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    institution(&vault.a.lock(), "a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert_eq!(vault.relay.ops().last().map(|op| op.seq), Some(s + 1));
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect("join");

    {
        let c = joiner.lock();
        assert_eq!(cursor(&c), s, "the joiner starts at the snapshot's S");
        assert!(has_institution(&c, "a1"));
        assert!(!has_institution(&c, "a2"), "pulled after the join");
        assert!(!has_institution(&c, "local-only"), "replaced, not merged");
        assert!(state::load(&c).expect("state").expect("row").enabled);
        assert!(capture::is_installed(&c).expect("installed"));
    }
    // The relay drops everything up to S: a pull asking below S would need a snapshot (`410`).
    vault.relay.prune_to(s);
    let first = round(&vault, &joiner, RoundMode::Normal).await;
    assert!(!first.restored, "the first pull asked after S: {first:?}");
    assert_eq!(first.problem, None);
    assert_eq!(first.pull.as_ref().map(|p| p.applied), Some(1), "only a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    assert_converged(&[&*vault.a.lock(), &*joiner.lock()]);
}

/// Phone live run, 2026-11-17 (As built R-116): a device another one removed keeps the removed
/// notice until it is dismissed. Joining again answers it: a join that did not go through
/// "Připojit znovu" (which dismisses first) left the notice, and the unlock sheet then said the
/// device was removed while sync was on again.
#[tokio::test]
async fn a_join_clears_the_removed_notice() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    joiner
        .lock()
        .execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                state::REMOVED_NOTICE_KEY,
                r#"{"by":"Mac","at":1,"unsent":0,"left":false}"#
            ],
        )
        .expect("notice");
    assert!(crate::services::sync::rekey::removed_notice(&joiner.lock())
        .expect("notice")
        .is_some());
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect("join");

    assert_eq!(
        crate::services::sync::rekey::removed_notice(&joiner.lock()).expect("notice"),
        None
    );
}

#[tokio::test]
async fn the_joiner_records_itself_with_a_public_key_and_pushes_it() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    let dir = tempfile::tempdir().expect("dir");

    join_by_replace(
        &vault.relay,
        &joiner,
        vault.creds.clone(),
        "  Work laptop ",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");

    let (id, public_key) = {
        let c = joiner.lock();
        let id = device_of(&c).to_hex();
        let public_key = URL_SAFE_NO_PAD.encode(keys::ensure_device_keypair(&c).expect("key"));
        let row: (String, String, String, i64) = c
            .query_row(
                "SELECT public_key, name, platform, joined_at FROM sync_devices WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("the joiner's row");
        assert_eq!(
            row,
            (
                public_key.clone(),
                "Work laptop".to_string(),
                devices::platform().to_string(),
                NOW
            )
        );
        assert_eq!(
            count(
                &c,
                &format!(
                    "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'sync_devices' AND pk = '{id}'"
                )
            ),
            1,
            "queued for the next push"
        );
        (id, public_key)
    };
    let pushed = round(&vault, &joiner, RoundMode::Normal).await;
    assert_eq!(pushed.push.as_ref().map(|p| p.pushed), Some(1));
    round(&vault, &vault.a, RoundMode::Normal).await;
    let a = vault.a.lock();
    let seen: String = a
        .query_row(
            "SELECT public_key FROM sync_devices WHERE id = ?1",
            [&id],
            |r| r.get(0),
        )
        .expect("A sees the joiner");
    assert_eq!(seen, public_key);
}

#[tokio::test]
async fn joining_a_vault_without_a_snapshot_is_refused_and_changes_nothing() {
    let vault = vault().await;
    assert_eq!(vault.relay.snapshot_seq(), None);
    let joiner = TestConn::new(migrated());
    institution(&joiner.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");
    binding::write_rejoin_marker(dir.path()).expect("marker");
    let before = profile(&joiner.lock());

    let error = join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect_err("no snapshot");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncVaultHasNoSnapshot"),
        "{error:?}"
    );
    let c = joiner.lock();
    assert_eq!(profile(&c), before);
    assert!(state::load(&c).expect("state").is_none(), "no sync row");
    assert!(keys::credentials(&c).expect("credentials").is_none());
    assert!(dir.path().join(REJOIN_MARKER).exists(), "the marker stays");
}

#[tokio::test]
async fn joining_clears_pauses_and_counters() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        seal_from(&b, &vault.creds);
        exec(
            &b,
            "UPDATE sync_state SET vault_gone_at = 11, capture_broken_at = 12, \
               snapshot_uploaded_at = 13, snapshot_too_large_at = 14, pulled_bytes = 500, \
               pulled_bytes_base = 7, binding_written = 1 WHERE id = 1",
        );
        assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_own_log"), 1);
    }
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &vault.b, dir.path(), &NoBinding)
        .await
        .expect("join");

    let b = vault.b.lock();
    let marks = state::marks(&b).expect("marks").expect("row");
    assert_eq!(
        (
            marks.vault_gone_at,
            marks.capture_broken_at,
            marks.snapshot_uploaded_at,
            marks.snapshot_too_large_at,
            marks.pulled_bytes,
            marks.pulled_bytes_base,
            marks.binding_written,
        ),
        (None, None, None, None, 0, 0, false)
    );
    let sync = state::load(&b).expect("state").expect("row");
    assert_ne!(sync.device, B, "a new device ID");
    assert_eq!((sync.next_seq, sync.last_hash), (1, None), "a new chain");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_own_log"), 0);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_unsent"), 0);
    assert!(
        !has_institution(&b, "b1"),
        "the unsent change went with the data"
    );
}

#[tokio::test]
async fn a_join_writes_the_binding_of_the_new_device_and_clears_the_marker() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    binding::write_binding(&vault.b.lock(), dir.path(), &XorNative).expect("old binding");
    binding::write_rejoin_marker(dir.path()).expect("marker");

    join(&vault, &vault.b, dir.path(), &XorNative)
        .await
        .expect("join");

    let b = vault.b.lock();
    assert_ne!(device_of(&b), B);
    assert!(
        !dir.path().join(REJOIN_MARKER).exists(),
        "binding::join cleared it"
    );
    assert!(
        state::marks(&b)
            .expect("marks")
            .expect("row")
            .binding_written
    );
    assert_eq!(
        startup_check(&b, dir.path(), &XorNative).expect("check"),
        StartupCheck::Fine,
        "the binding matches the new device ID"
    );
}

#[tokio::test]
async fn joining_another_vault_starts_its_chain_and_cursor_over() {
    let vault = vault().await;
    // B is well into the first vault: a cursor of 3 and a chain of its own.
    let cts = {
        let a = vault.a.lock();
        institution(&a, "a1");
        let first = seal_from(&a, &vault.creds);
        institution(&a, "a2");
        vec![first, seal_from(&a, &vault.creds)]
    };
    push(&vault.relay, &vault.creds, &cts).await;
    institution(&vault.b.lock(), "b1");
    round(&vault, &vault.b, RoundMode::Normal).await;
    assert_eq!(cursor(&vault.b.lock()), 3);
    assert!(head(&vault.b.lock(), A).is_some());

    // A second vault, re-created with a lower head: its snapshot is at S = 1.
    let second = Credentials {
        relay_url: vault.creds.relay_url.clone(),
        account: vault.creds.account.clone(),
        vault_id: VaultId::random().expect("vault id"),
        keys: VaultKeys::new_random().expect("vault keys"),
    };
    assert!(vault
        .relay
        .create_vault(&second.account, &second.auth())
        .await
        .expect("vault"));
    let d = TestConn::new(migrated());
    {
        let conn = d.lock();
        keys::save_credentials(&conn, &second).expect("credentials D");
        state::enable(&conn, C).expect("enable D");
        exec(&conn, "DELETE FROM sync_outbox");
        institution(&conn, "d1");
    }
    let other = Vault {
        relay: vault.relay.clone(),
        creds: second.clone(),
        a: d.clone(),
        b: vault.b.clone(),
    };
    let first = round(&other, &d, RoundMode::FirstSnapshot).await;
    assert!(first.snapshot_uploaded);
    let s = cursor(&d.lock());
    assert_eq!(s, 1);

    join(
        &other,
        &vault.b,
        tempfile::tempdir().expect("dir").path(),
        &NoBinding,
    )
    .await
    .expect("join");

    {
        let b = vault.b.lock();
        assert_eq!(
            cursor(&b),
            1,
            "the cursor is the new vault's S, not the old one's"
        );
        assert_eq!(head(&b, A), None, "no head of the old vault");
        assert_eq!(
            keys::credentials(&b)
                .expect("credentials")
                .expect("bound")
                .vault_id,
            second.vault_id
        );
        assert!(has_institution(&b, "d1") && !has_institution(&b, "a1"));
    }
    institution(&d.lock(), "d2");
    round(&other, &d, RoundMode::Normal).await;
    let next = round(&other, &vault.b, RoundMode::Normal).await;
    assert_eq!(next.problem, None, "{next:?}");
    assert_eq!(next.pull.as_ref().map(|p| p.applied), Some(1));
    assert!(has_institution(&vault.b.lock(), "d2"));
}

#[tokio::test]
async fn a_join_that_fails_leaves_the_profile_as_it_was() {
    const FAILING_NOW: i64 = 4_242;
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join(REJOIN_MARKER), b"left by a restore").expect("marker");
    // B's clocks run 30 days ahead of the wall clock and of the vault's snapshot; its capture was
    // installed from them, as an unlock does.
    let ahead = hlc::pack(
        u64::try_from(chrono::Utc::now().timestamp_millis()).expect("ms") + 30 * 86_400_000,
        0,
    );
    {
        let b = vault.b.lock();
        institution(&b, "b-local");
        b.execute(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) \
             VALUES ('institutions', 'b-local', 'note', ?1, ?2)",
            rusqlite::params![i64::try_from(ahead).expect("hlc"), B.0.to_vec()],
        )
        .expect("clock ahead");
        capture::install(&b, B).expect("capture from the stored clocks");
        // The last step of the join, recording the new device, fails.
        exec(
            &b,
            &format!(
                "CREATE TRIGGER fail_join BEFORE INSERT ON sync_devices \
                 WHEN NEW.joined_at = {FAILING_NOW} BEGIN SELECT RAISE(ABORT, 'injected'); END;"
            ),
        );
    }
    let before = profile(&vault.b.lock());

    let result = join_by_replace(
        &vault.relay,
        &vault.b,
        vault.creds.clone(),
        "Laptop",
        dir.path(),
        &XorNative,
        FAILING_NOW,
    )
    .await;

    assert!(result.is_err(), "{result:?}");
    let b = vault.b.lock();
    assert_eq!(profile(&b), before);
    assert_eq!(
        std::fs::read(dir.path().join(REJOIN_MARKER)).expect("marker"),
        b"left by a restore",
        "the marker was never touched"
    );
    assert!(!dir.path().join(binding::BINDING_FILE).exists());
    // Capture still runs under the old device, its clock above every stored one.
    institution(&b, "after");
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'institutions' AND pk = 'after'"
        ),
        1
    );
    let (after, device): (i64, Vec<u8>) = b
        .query_row(
            "SELECT MAX(hlc), device FROM sync_clock WHERE tbl = 'institutions' AND pk = 'after'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("the edit's clock");
    assert!(
        u64::try_from(after).expect("hlc") > ahead,
        "{after} is not above {ahead}"
    );
    assert_eq!(device, B.0.to_vec());
}

/// Serves `blob` as the vault's snapshot; everything else goes to the fake relay.
struct ServingSnapshot {
    relay: FakeRelay,
    blob: SnapshotBlob,
}

impl Relay for ServingSnapshot {
    fn server_info(
        &self,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::ServerInfo>> + Send
    {
        self.relay.server_info()
    }
    fn create_account(&self) -> impl std::future::Future<Output = RelayResult<String>> + Send {
        self.relay.create_account()
    }
    fn account_status(
        &self,
        account: &str,
    ) -> impl std::future::Future<Output = RelayResult<AccountStatus>> + Send {
        self.relay.account_status(account)
    }
    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl std::future::Future<Output = RelayResult<String>> + Send {
        self.relay.checkout(account, plan)
    }
    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<bool>> + Send {
        self.relay.create_vault(account, vault)
    }
    fn vault_info(
        &self,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<VaultInfo>> + Send {
        self.relay.vault_info(vault)
    }
    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl std::future::Future<Output = RelayResult<PushReceipt>> + Send {
        self.relay.push(vault, ops)
    }
    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl std::future::Future<Output = RelayResult<PullPage>> + Send {
        self.relay.pull(vault, after, limit)
    }
    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.put_snapshot(vault, seq, bytes)
    }
    fn get_snapshot(
        &self,
        _vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<SnapshotBlob>> + Send {
        let blob = self.blob.clone();
        async move { Ok(blob) }
    }
    fn delete_vault(
        &self,
        vault: &VaultAuth,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.delete_vault(vault)
    }
    fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::Rotated>> + Send
    {
        self.relay.rotate_token(vault, new_token, bundle)
    }
    fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: crate::services::sync::relay::KitRotation<'_>,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::Rotated>> + Send
    {
        self.relay
            .rotate_token_with_kit(vault, new_token, bundle, kit)
    }
    fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> impl std::future::Future<Output = RelayResult<bool>> + Send {
        self.relay.put_kit(vault, kit_token, envelope)
    }
    fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::KitRead>> + Send
    {
        self.relay.get_kit(vault_id, kit_token)
    }
    fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::RekeyPage>> + Send
    {
        self.relay.rekeys(vault, after, limit)
    }
    fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: crate::services::sync::relay::BlobBody,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.put_blob(vault, blob, sealed_len, body)
    }
    fn head_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
    ) -> impl std::future::Future<Output = RelayResult<Option<u64>>> + Send {
        self.relay.head_blob(vault, blob)
    }
    fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn crate::services::sync::relay::BlobSink + Send),
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::Delivery>> + Send
    {
        self.relay.get_blob(vault, blob, sink)
    }
    fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> impl std::future::Future<Output = RelayResult<crate::services::sync::relay::BlobPage>> + Send
    {
        self.relay.list_blobs(vault, after, limit)
    }
    fn delete_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
    ) -> impl std::future::Future<Output = RelayResult<()>> + Send {
        self.relay.delete_blob(vault, blob)
    }
}

/// B, with a marker next to it, joins from `relay`: the join fails and B and the marker are as
/// they were.
async fn refused_snapshot(vault: &Vault, relay: &impl Relay) -> SyncError {
    let dir = tempfile::tempdir().expect("dir");
    std::fs::write(dir.path().join(REJOIN_MARKER), b"left by a restore").expect("marker");
    institution(&vault.b.lock(), "b-local");
    let before = profile(&vault.b.lock());

    let error = join_by_replace(
        relay,
        &vault.b,
        vault.creds.clone(),
        "Laptop",
        dir.path(),
        &XorNative,
        NOW,
    )
    .await
    .expect_err("refused");

    assert_eq!(profile(&vault.b.lock()), before);
    assert_eq!(
        std::fs::read(dir.path().join(REJOIN_MARKER)).expect("marker"),
        b"left by a restore"
    );
    error
}

#[tokio::test]
async fn a_snapshot_that_does_not_open_changes_nothing() {
    let vault = vault().await;
    let s = snapshot_of_a(&vault, "a1").await;
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), s, vec![7; 4096])
        .await
        .expect("garbage snapshot");

    let error = refused_snapshot(&vault, &vault.relay).await;

    assert_eq!(
        validation_key(&error),
        Some("validation.syncSnapshotInvalid"),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_oversized_snapshot_changes_nothing() {
    let vault = vault().await;
    let relay = ServingSnapshot {
        relay: vault.relay.clone(),
        blob: SnapshotBlob {
            seq: 0,
            bytes: vec![0; MAX_SNAPSHOT_BYTES + 1],
        },
    };

    let error = refused_snapshot(&vault, &relay).await;

    assert!(
        matches!(error, SyncError::TooLarge(TooLarge::Body)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_join_keeps_the_relay_address_of_its_credentials() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    let dir = tempfile::tempdir().expect("dir");
    let mut custom = vault.creds.clone();
    custom.relay_url = "https://sync.example.org".into();

    join_by_replace(
        &vault.relay,
        &joiner,
        custom.clone(),
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect("join");
    assert_eq!(
        keys::credentials(&joiner.lock())
            .expect("credentials")
            .expect("bound")
            .relay_url,
        "https://sync.example.org"
    );

    // An address the device could never sync with is refused before the relay is asked.
    let fresh = TestConn::new(migrated());
    let mut plain_http = vault.creds.clone();
    plain_http.relay_url = "ftp://sync.example.org".into();
    let gets = vault
        .relay
        .calls()
        .iter()
        .filter(|r| **r == Route::GetSnapshot)
        .count();
    let error = join_by_replace(
        &vault.relay,
        &fresh,
        plain_http,
        "Laptop",
        dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("refused");
    assert_eq!(
        validation_key(&error),
        Some("validation.syncRelayUrlInvalid")
    );
    assert_eq!(
        vault
            .relay
            .calls()
            .iter()
            .filter(|r| **r == Route::GetSnapshot)
            .count(),
        gets
    );
    assert!(state::load(&fresh.lock()).expect("state").is_none());
}

/// Rule 10, a paired device (spec §2.3): the holder's heads give the head of the key change that
/// ended the key of the vault's snapshot. A snapshot at or below it is taken, one above it is
/// refused and replaces nothing. A uploads its snapshot, removes C and goes silent; B, the holder,
/// picks the new key up and pairs D.
#[tokio::test]
async fn a_paired_join_takes_an_old_key_snapshot_up_to_the_head() {
    use super::rekey_fixture::{key_epoch, remove, round_of, sync_on, three};
    use crate::services::sync::join::{join_new_device, JoinIdentity};
    use crate::services::sync::pairing::holder;

    async fn pair_d(
        three: &super::rekey_fixture::Three,
        d: &TestConn,
        dir: &Path,
    ) -> SyncResult<()> {
        let payload = holder::announced_payload(&three.vault.b.lock(), NOW)
            .expect("payload")
            .expect("sync is on");
        let crate::services::sync::pairing::SlotPayload {
            creds,
            heads,
            identity,
        } = payload;
        join_new_device(
            &three.vault.relay,
            d,
            creds,
            "Tablet",
            agreed(d),
            JoinIdentity::Paired(identity.expect("identity"), heads),
            dir,
            &NoBinding,
            NOW,
        )
        .await
        .map(|_| ())
    }

    let three = three().await;
    institution(&three.vault.a.lock(), "a-in-the-old-snapshot");
    super::kit_join::snapshot_by(&three, &three.vault.a).await;
    remove(&three, A, &[C]).await.expect("removed");
    super::rekey_audit2::push_a_record(&three).await;
    round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        key_epoch(&three.vault.b.lock()),
        1,
        "the holder has the new key"
    );
    let first = TestConn::new(crate::services::sync::test_support::migrated());
    let dir = tempfile::tempdir().expect("dir");

    pair_d(&three, &first, dir.path()).await.expect("joined");

    {
        let conn = first.lock();
        assert!(has_institution(&conn, "a-in-the-old-snapshot"));
        assert_eq!(key_epoch(&conn), 1);
    }

    // An old-key snapshot above the head is refused, and the profile stays as it was.
    let above = super::rekey_audit2::last_seq(&three);
    let sealed = super::rekey_audit2::forged_old_key_snapshot(&three, above);
    super::rekey_audit2::server_takes_snapshot(&three, above, sealed).await;
    let second = TestConn::new(crate::services::sync::test_support::migrated());
    institution(&second.lock(), "local-only");
    let dir = tempfile::tempdir().expect("dir");

    let refused = pair_d(&three, &second, dir.path()).await;

    let error = refused.expect_err("refused");
    assert_eq!(
        validation_key(&error),
        Some("validation.syncSnapshotInvalid"),
        "{error:?}"
    );
    let conn = second.lock();
    assert!(has_institution(&conn, "local-only") && !sync_on(&conn));
}
