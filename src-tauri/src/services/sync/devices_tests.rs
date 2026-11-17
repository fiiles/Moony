//! The device list's verdicts (revision §4.1, §6.1, §6.5) over a profile with a real trust
//! store: certificates and records from the issuers of `identity`, rows as sync writes them.

use rusqlite::{params, Connection};
use zeroize::Zeroizing;

use super::devices::{list_devices, FORMER_LISTED_SECS};
use super::hlc::{pack, DeviceId};
use super::identity::cert::{self, CertFields, CertKind, Certificate, TicketSecret};
use super::identity::trust::{self, SeenWhat};
use super::identity::{sign_pub, KitKeys, KitSecret, Record, RecordType, SignSeed};
use super::keys::VaultId;
use super::state;
use super::test_support::migrated;
use crate::models::sync::{
    SyncCheckReason, SyncDevice, SyncDeviceStatus, SyncFormerReason, SyncJoinedVia,
};

const VAULT: [u8; 16] = [0x11; 16];
const NOW: i64 = 1_794_816_000;
const DAY: i64 = 86_400;

#[derive(Clone, Copy)]
struct Dev(u8);

impl Dev {
    fn id(self) -> DeviceId {
        DeviceId([self.0; 16])
    }
    fn seed(self) -> SignSeed {
        SignSeed::from_bytes(Zeroizing::new([self.0; 32]))
    }
    fn fields(self, at: i64) -> CertFields {
        CertFields {
            vault_id: VAULT,
            device: self.id(),
            box_pub: [self.0 ^ 0xb0; 32],
            issued_at: at,
        }
    }
    fn genesis(self) -> Certificate {
        cert::issue_self(CertKind::Genesis, &self.fields(NOW), &self.seed()).expect("genesis")
    }
    fn paired_by(self, holder: Dev, ticket: u8) -> Certificate {
        let (secret, ticket) = cert::ticket_from_secret(
            TicketSecret::from_bytes(Zeroizing::new([ticket; 32])),
            &holder.seed(),
            VAULT,
            holder.id(),
            NOW,
        );
        cert::issue_paired(
            &secret,
            &ticket,
            holder.id(),
            &sign_pub(&holder.seed()),
            &self.fields(NOW),
            &self.seed(),
        )
        .expect("paired")
    }
    fn rejoin_of(self, old: Dev, at: i64) -> Certificate {
        cert::issue_rejoin(&old.seed(), old.id(), &self.fields(at), &self.seed()).expect("rejoin")
    }
    fn left_at(self, at: i64) -> Record {
        Record::sign(
            RecordType::Left,
            VAULT,
            self.id(),
            self.id(),
            at,
            0,
            &self.seed(),
        )
        .expect("left")
    }
    fn removes_at(self, subject: Dev, at: i64) -> Record {
        Record::sign(
            RecordType::Removed,
            VAULT,
            subject.id(),
            self.id(),
            at,
            0,
            &self.seed(),
        )
        .expect("removed")
    }
}

const ME: Dev = Dev(0x01);
const B: Dev = Dev(0x02);
const C: Dev = Dev(0x03);
const D: Dev = Dev(0x04);
const E: Dev = Dev(0x05);

fn the_kit() -> KitKeys {
    KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&VAULT)
}

/// A sync-enabled profile of `ME`; with `phase5`, its genesis certificate pinned.
fn profile(phase5: bool) -> Connection {
    let conn = migrated();
    state::enable(&conn, ME.id()).expect("enable");
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1",
        [VaultId(VAULT).to_b64()],
    )
    .expect("vault");
    row(&conn, ME, phase5.then(|| ME.genesis()).as_ref());
    if phase5 {
        trust::seed_own(&conn, &ME.genesis(), NOW).expect("seed own");
    }
    conn
}

fn row(conn: &Connection, dev: Dev, cert: Option<&Certificate>) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, ?2, ?3, 'macos', '1.0', ?5, ?4) \
         ON CONFLICT(id) DO UPDATE SET cert = excluded.cert",
        params![
            dev.id().to_hex(),
            "PK",
            format!("Device {}", dev.0),
            cert.map(Certificate::to_b64),
            NOW - 100 * DAY
        ],
    )
    .expect("row");
}

fn set_record(conn: &Connection, dev: Dev, column: &str, record: &Record) {
    conn.execute(
        &format!("UPDATE sync_devices SET {column} = ?1 WHERE id = ?2"),
        params![record.to_b64(), dev.id().to_hex()],
    )
    .expect("record");
}

fn clock(conn: &Connection, dev: Dev, secs: i64) {
    conn.execute(
        "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES ('x', ?1, 'c', ?2, ?3)",
        params![
            format!("{:02x}-{secs}", dev.0),
            pack(u64::try_from(secs).unwrap() * 1000, 0) as i64,
            dev.id().0.to_vec()
        ],
    )
    .expect("clock");
}

fn listed(conn: &Connection) -> Vec<SyncDevice> {
    list_devices(conn, NOW).expect("list")
}

fn of(list: &[SyncDevice], dev: Dev) -> &SyncDevice {
    list.iter()
        .find(|d| d.id == dev.id().to_hex())
        .unwrap_or_else(|| panic!("device {} is listed", dev.0))
}

fn listed_ids(list: &[SyncDevice]) -> Vec<String> {
    list.iter().map(|d| d.name.clone()).collect()
}

#[test]
fn a_vault_from_before_phase_5_lists_every_row_as_active() {
    let conn = profile(false);
    row(&conn, B, None);
    row(&conn, C, None);
    conn.execute(
        "UPDATE sync_devices SET removed = 1 WHERE id = ?1",
        [C.id().to_hex()],
    )
    .unwrap();

    let list = listed(&conn);

    assert_eq!(of(&list, B).status, SyncDeviceStatus::Active);
    assert_eq!(of(&list, B).joined_via, SyncJoinedVia::Unknown);
    assert_eq!(of(&list, B).check_reason, None);
    assert!(
        list.iter().all(|d| d.id != C.id().to_hex()),
        "a removed row with no record and no time would never age out: not listed"
    );
    assert!(of(&list, ME).this_device);
    assert_eq!(list[0].id, ME.id().to_hex(), "this device first");
}

#[test]
fn active_check_and_former_rows_are_told_apart() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, None); // a key but no certificate: not verified
    row(&conn, D, Some(&D.paired_by(ME, 0x71)));
    row(&conn, E, Some(&E.paired_by(ME, 0x72)));
    set_record(&conn, D, "left_record", &D.left_at(NOW - 5 * DAY));
    set_record(&conn, E, "removal_record", &ME.removes_at(E, NOW - 2 * DAY));
    trust::refresh(&conn, NOW).unwrap();

    let list = listed(&conn);

    let me = of(&list, ME);
    assert_eq!(me.status, SyncDeviceStatus::Active);
    assert_eq!(me.joined_via, SyncJoinedVia::Created);
    let b = of(&list, B);
    assert_eq!(b.status, SyncDeviceStatus::Active);
    assert_eq!(b.joined_via, SyncJoinedVia::Paired);
    assert_eq!(b.added_by.as_deref(), Some("Device 1"));
    let c = of(&list, C);
    assert_eq!(c.status, SyncDeviceStatus::Check);
    assert_eq!(c.check_reason, Some(SyncCheckReason::Unverified));
    assert_eq!(c.joined_via, SyncJoinedVia::Unknown);
    let d = of(&list, D);
    assert_eq!(d.status, SyncDeviceStatus::Former);
    assert_eq!(d.former_reason, Some(SyncFormerReason::Left));
    assert_eq!(d.state_at, Some(NOW - 5 * DAY));
    assert_eq!(d.state_by, None);
    let e = of(&list, E);
    assert_eq!(e.former_reason, Some(SyncFormerReason::Removed));
    assert_eq!(e.state_at, Some(NOW - 2 * DAY));
    assert_eq!(e.state_by.as_deref(), Some("Device 1"));
    // Active before check before former, this device first.
    assert_eq!(
        listed_ids(&list),
        vec!["Device 1", "Device 2", "Device 3", "Device 4", "Device 5"]
    );
}

#[test]
fn a_former_device_is_listed_for_thirty_days() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    set_record(
        &conn,
        B,
        "left_record",
        &B.left_at(NOW - FORMER_LISTED_SECS + 10),
    );
    set_record(
        &conn,
        C,
        "left_record",
        &C.left_at(NOW - FORMER_LISTED_SECS - 10),
    );
    trust::refresh(&conn, NOW).unwrap();

    let list = listed(&conn);

    assert!(list.iter().any(|d| d.id == B.id().to_hex()));
    assert!(
        !list.iter().any(|d| d.id == C.id().to_hex()),
        "the rows and entries stay, only the list leaves it out"
    );
    assert!(trust::entry(&conn, C.id()).unwrap().is_some());
}

#[test]
fn a_device_a_removed_device_added_is_marked_check_and_cannot_be_kept() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(of(&listed(&conn), C).status, SyncDeviceStatus::Active);
    set_record(&conn, B, "removal_record", &ME.removes_at(B, NOW + 100));
    trust::refresh(&conn, NOW + 200).unwrap();

    let list = list_devices(&conn, NOW + 200).unwrap();

    assert_eq!(of(&list, B).former_reason, Some(SyncFormerReason::Removed));
    let c = of(&list, C);
    assert_eq!(c.status, SyncDeviceStatus::Check);
    assert_eq!(c.check_reason, Some(SyncCheckReason::IssuerRemoved));
    assert_eq!(c.added_by.as_deref(), Some("Device 2"));

    // Only a late vouch is the person's to keep (final review M-1).
    assert!(!trust::keep(&conn, C.id()).unwrap());
    let list = list_devices(&conn, NOW + 200).unwrap();
    assert_eq!(of(&list, C).status, SyncDeviceStatus::Check);
}

#[test]
fn a_certificate_seen_after_its_issuer_left_is_a_late_vouch() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    trust::refresh(&conn, NOW).unwrap();
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    set_record(&conn, B, "left_record", &B.left_at(NOW + 10));
    trust::note_seen(&conn, B.id(), SeenWhat::Left, 5).unwrap();
    trust::note_seen(&conn, C.id(), SeenWhat::Cert, 9).unwrap();
    trust::refresh(&conn, NOW + 20).unwrap();

    let list = list_devices(&conn, NOW + 20).unwrap();

    assert_eq!(of(&list, B).former_reason, Some(SyncFormerReason::Left));
    let c = of(&list, C);
    assert_eq!(c.status, SyncDeviceStatus::Check);
    assert_eq!(c.check_reason, Some(SyncCheckReason::LateVouch));
}

#[test]
fn an_old_id_that_still_writes_is_a_copy_and_a_quiet_one_is_not_listed() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    // B and C each connected again under a new ID (D and E).
    row(&conn, D, Some(&D.rejoin_of(B, NOW + 10)));
    row(&conn, E, Some(&E.rejoin_of(C, NOW + 10)));
    trust::refresh(&conn, NOW + 20).unwrap();
    // B kept writing after its successor appeared; C did not.
    clock(&conn, B, NOW + 5_000);

    let list = list_devices(&conn, NOW + 6_000).unwrap();

    let b = of(&list, B);
    assert_eq!(b.status, SyncDeviceStatus::Check);
    assert_eq!(b.check_reason, Some(SyncCheckReason::Copy));
    assert!(
        !list.iter().any(|d| d.id == C.id().to_hex()),
        "a quiet superseded ID is hidden"
    );
    assert_eq!(of(&list, D).joined_via, SyncJoinedVia::Rejoined);
    assert_eq!(of(&list, D).status, SyncDeviceStatus::Active);
}

#[test]
fn the_recovery_kits_row_is_never_listed() {
    let conn = profile(true);
    let kit = the_kit();
    let kit_cert = cert::issue_kit(&ME.seed(), ME.id(), &kit, NOW - 100).unwrap();
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, 'KITPK', '', 'recoveryKit', '1.0', 1, ?2)",
        params![kit.id().to_hex(), kit_cert.to_b64()],
    )
    .unwrap();
    trust::refresh(&conn, NOW).unwrap();
    assert!(trust::entry(&conn, kit.id()).unwrap().is_some(), "pinned");

    let list = listed(&conn);

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, ME.id().to_hex());
}

#[test]
fn an_active_device_unseen_for_thirty_days_says_how_long() {
    let conn = profile(true);
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    clock(&conn, B, NOW - 45 * DAY - 100);
    clock(&conn, C, NOW - 29 * DAY);

    let list = listed(&conn);

    assert_eq!(of(&list, B).inactive_days, Some(45));
    assert_eq!(of(&list, C).inactive_days, None);
    assert_eq!(
        of(&list, ME).inactive_days,
        None,
        "this device is never idle"
    );
}

#[test]
fn a_device_row_of_a_vault_with_no_state_row_lists_nothing() {
    assert!(list_devices(&migrated(), NOW).unwrap().is_empty());
}
