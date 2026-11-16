//! A phase-5 vault of three devices for the key-change tests: `A` set sync up (genesis), `B` and
//! `C` joined from `A`'s snapshot and were paired by `A` (paired certificates, `A`'s store
//! seeding theirs); every device has synced its row and verified the others.

use std::path::Path;

use rusqlite::Connection;

use super::*;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::identity::{self, cert, trust, Certificate, Issuer};
use crate::services::sync::identity::{KitKeys, KitSecret};
use crate::services::sync::rekey::kit::{self, KitStorage};
use crate::services::sync::rekey::rotate::{self, Outcome, Removal};
use crate::services::sync::relay::Relay;
use crate::services::sync::{devices, keys, snapshot, state};

pub struct Three {
    pub vault: Vault,
    pub c: TestConn,
    pub dir: tempfile::TempDir,
}

impl Three {
    pub fn device(&self, id: DeviceId) -> &TestConn {
        match id {
            A => &self.vault.a,
            B => &self.vault.b,
            C => &self.c,
            _ => panic!("no such device"),
        }
    }
}

/// `holder` pairs `joiner`: a one-time ticket, the holder's store seeding the joiner's, the
/// joiner's paired certificate and its row.
pub fn pair_with(holder: &Connection, joiner: &Connection, name: &str) -> Certificate {
    let holder_id = state::load(holder).expect("state").expect("row").device;
    let seed = identity::own_seed(holder)
        .expect("seed")
        .expect("holder seed");
    let vault = keys::credentials(holder)
        .expect("creds")
        .expect("bound")
        .vault_id
        .0;
    let (secret, ticket) = cert::issue_ticket(&seed, vault, holder_id, NOW).expect("ticket");
    trust::import_export(
        joiner,
        &trust::export(holder).expect("export"),
        holder_id,
        NOW,
    )
    .expect("import");
    let own = identity::create_own(
        joiner,
        Issuer::Ticket {
            secret: &secret,
            ticket: &ticket,
            holder: holder_id,
            holder_sign_pub: identity::sign_pub(&seed),
        },
        NOW,
    )
    .expect("paired certificate");
    let public = keys::ensure_device_keypair(joiner).expect("box key");
    devices::record_this_device(joiner, name, &public, Some(&own), NOW).expect("own row");
    own
}

pub async fn round_of(vault: &Vault, device: &TestConn) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round")
}

/// The three-device vault (module doc).
pub async fn three() -> Three {
    let vault = vault().await;
    let c = {
        let a = vault.a.lock();
        let mut stream = Vec::new();
        snapshot::dump(&a, 0, &mut stream).expect("dump A");
        let c = migrated();
        keys::save_credentials(&c, &vault.creds).expect("credentials C");
        snapshot::join(&c, C, &mut stream.as_slice(), NOW).expect("join C");
        TestConn::new(c)
    };
    with_genesis(&vault.a.lock(), "Mac");
    pair_with(&vault.a.lock(), &vault.b.lock(), "iPhone");
    pair_with(&vault.a.lock(), &c.lock(), "iPad");
    let three = Three {
        vault,
        c,
        dir: tempfile::tempdir().expect("dir"),
    };
    settle(&three).await;
    for id in [A, B, C] {
        let conn = three.device(id).lock();
        trust::refresh(&conn, NOW).expect("refresh");
        let trusted = trust::entries(&conn)
            .expect("entries")
            .iter()
            .filter(|e| e.state == trust::TrustState::Trusted)
            .count();
        assert_eq!(trusted, 3, "{id:?} verified the others");
    }
    three
}

/// Rounds on every device until each has sent and received everything (two passes).
pub async fn settle(three: &Three) {
    for _ in 0..2 {
        for id in [A, B, C] {
            let report = round_of(&three.vault, three.device(id)).await;
            assert_eq!(report.problem, None, "{id:?}: {report:?}");
        }
    }
}

fn no_pairing() {}

/// `remover` removes `removing` with the plan as it stands (the person confirmed it); the Kit
/// is replaced only where the plan says so.
pub async fn remove(
    three: &Three,
    remover: DeviceId,
    removing: &[DeviceId],
) -> crate::services::sync::error::SyncResult<Outcome> {
    remove_in(
        &three.vault,
        three.device(remover),
        three.dir.path(),
        removing,
        false,
    )
    .await
}

/// `device` makes a new Recovery Kit ("Vytvořit novou sadu"): a key change that removes nobody.
pub async fn new_kit(
    three: &Three,
    device: DeviceId,
) -> crate::services::sync::error::SyncResult<Outcome> {
    remove_in(
        &three.vault,
        three.device(device),
        three.dir.path(),
        &[],
        true,
    )
    .await
}

pub async fn remove_in(
    vault: &Vault,
    device: &TestConn,
    dir: &Path,
    removing: &[DeviceId],
    new_kit: bool,
) -> crate::services::sync::error::SyncResult<Outcome> {
    let consent = rotate::plan(&device.lock(), removing, &[], new_kit)
        .expect("plan")
        .consent;
    rotate::remove(
        &vault.relay,
        device,
        Removal {
            removing,
            new_kit,
            consent: &consent,
        },
        dir,
        &no_pairing,
        NOW,
    )
    .await
}

/// `creator` sets the vault's first Recovery Kit up, as setup does (revision §2.2): the Kit's row
/// with `storage`, its certificate pinned there, the envelope on the sync server; then every
/// device syncs and pins it (its creator vouches). The Kit's secret.
pub async fn with_kit(three: &Three, creator: DeviceId, storage: Option<KitStorage>) -> KitSecret {
    let (secret, envelope, token) = {
        let conn = three.device(creator).lock();
        let creds = keys::credentials(&conn).expect("creds").expect("bound");
        let seed = identity::own_seed(&conn).expect("seed").expect("seed");
        let own = trust::entry(&conn, creator)
            .expect("entry")
            .expect("pinned");
        let (secret, keys, cert) =
            kit::create(creator, &seed, &creds.vault_id.0, NOW).expect("Kit");
        kit::write_row(&conn, &cert, storage, NOW).expect("row");
        trust::pin_created_kit(&conn, &cert, NOW).expect("pinned");
        let store = trust::entries(&conn).expect("entries");
        let confirmed = kit::confirmed_after(&store, std::slice::from_ref(&own), &[], None, &cert);
        let envelope = kit::seal(&conn, &creds.keys, &cert, &seed, &own.cert, &confirmed, NOW)
            .expect("envelope");
        (secret, envelope, keys.token().to_string())
    };
    three
        .vault
        .relay
        .put_kit(&three.vault.creds.auth(), &token, &envelope)
        .await
        .expect("put_kit");
    settle(three).await;
    let kit_id = kit_keys(three, &secret).id();
    for id in [A, B, C] {
        let conn = three.device(id).lock();
        trust::refresh(&conn, NOW).expect("refresh");
        let entry = trust::entry(&conn, kit_id).expect("entry");
        assert!(
            entry.is_some_and(|e| e.state == trust::TrustState::Trusted),
            "{id:?} pinned the Kit"
        );
    }
    secret
}

/// What `secret` derives in the test vault.
pub fn kit_keys(three: &Three, secret: &KitSecret) -> KitKeys {
    secret.derive(&three.vault.creds.vault_id.0)
}

/// Where the Kit `kit` is kept, set on `device`'s list as "Vytisknout" and the like would.
pub fn set_kit_storage(conn: &Connection, kit: DeviceId, storage: Option<KitStorage>) {
    conn.execute(
        "UPDATE sync_devices SET kit_storage = ?1 WHERE id = ?2",
        rusqlite::params![storage.map(KitStorage::as_str), kit.to_hex()],
    )
    .expect("storage");
}

/// A removal record with no key change (what a list-only removal wrote before revision D2; now
/// only a record that reached a device ahead of its key change looks like this): `remover` signs
/// the removal of `device` at its epoch into the row (captured) and into its store.
pub fn record_removal(conn: &Connection, remover: DeviceId, device: DeviceId) {
    let seed = identity::own_seed(conn).expect("seed").expect("seed");
    let vault = keys::credentials(conn)
        .expect("creds")
        .expect("bound")
        .vault_id
        .0;
    let record = identity::Record::sign(
        identity::RecordType::Removed,
        vault,
        device,
        remover,
        NOW,
        key_epoch(conn),
        &seed,
    )
    .expect("record");
    conn.execute(
        "UPDATE sync_devices SET removal_record = ?1, removed = 1 WHERE id = ?2",
        rusqlite::params![record.to_b64(), device.to_hex()],
    )
    .expect("row");
    trust::mark_removed(conn, &record, None).expect("store");
}

pub fn key_epoch(conn: &Connection) -> u32 {
    keys::vault_keys(conn)
        .expect("keys")
        .expect("bound")
        .current_epoch()
}

pub fn rotation_pending(conn: &Connection) -> bool {
    rotate::pending(conn).expect("pending")
}

/// The epochs of the envelopes waiting in `sync_unsent`, by sequence.
pub fn unsent_epochs(conn: &Connection) -> Vec<(i64, u32)> {
    conn.prepare("SELECT seq, envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
        .expect("rows")
        .map(|row| {
            let (seq, envelope) = row.expect("row");
            (
                seq,
                crate::services::sync::envelope::envelope_epoch(&envelope).expect("epoch"),
            )
        })
        .collect()
}

pub fn sync_on(conn: &Connection) -> bool {
    state::load(conn)
        .expect("state")
        .is_some_and(|sync| sync.enabled)
}
