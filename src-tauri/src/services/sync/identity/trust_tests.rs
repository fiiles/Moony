//! The pinned trust store (phase 5 spec §5.4, §5.5) over an in-memory migrated profile, with
//! certificates and records from the issuers of `cert.rs` and `record.rs`.

use rusqlite::{params, Connection};
use zeroize::Zeroizing;

use super::cert::{self, CertFields, CertKind, Certificate, TicketSecret};
use super::kit::{KitKeys, KitSecret};
use super::record::{Record, RecordType};
use super::trust::{self, ConflictKind, ExclusionReason, TrustExport, TrustState};
use super::{sign_pub, SignSeed};
use crate::services::sync::hlc::{pack, DeviceId};
use crate::services::sync::keys::VaultId;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const VAULT: [u8; 16] = [0x11; 16];
const NOW: i64 = 1_794_816_000;

/// A device of the test vault: its ID, signing seed and box key are derived from one byte.
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
    /// Joined with the test vault's Kit (kind 3, issued by the Kit).
    fn kit(self) -> Certificate {
        cert::issue_kit_join(&the_kit(), &self.fields(NOW), &self.seed()).expect("kit join")
    }
    /// Paired by `holder` with the one-time ticket whose seed is `ticket` repeated.
    fn paired_by(self, holder: Dev, ticket: u8) -> Certificate {
        self.paired_by_at(holder, ticket, NOW, NOW)
    }
    /// `paired_by` with the ticket issued at `ticket_at` and the certificate at `cert_at`.
    fn paired_by_at(self, holder: Dev, ticket: u8, ticket_at: i64, cert_at: i64) -> Certificate {
        let (secret, ticket) = cert::ticket_from_secret(
            TicketSecret::from_bytes(Zeroizing::new([ticket; 32])),
            &holder.seed(),
            VAULT,
            holder.id(),
            ticket_at,
        );
        cert::issue_paired(
            &secret,
            &ticket,
            holder.id(),
            &sign_pub(&holder.seed()),
            &self.fields(cert_at),
            &self.seed(),
        )
        .expect("paired")
    }
    fn rejoin_of(self, old: Dev, at: i64) -> Certificate {
        cert::issue_rejoin(&old.seed(), old.id(), &self.fields(at), &self.seed()).expect("rejoin")
    }
    fn left(self) -> Record {
        self.left_at(NOW)
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
    /// `self` removes `subject`.
    fn removes(self, subject: Dev) -> Record {
        self.removes_at(subject, NOW)
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

/// The test vault's Recovery Kit (revision §2.3), from a fixed seed.
fn the_kit() -> KitKeys {
    KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&VAULT)
}

const ME: Dev = Dev(0x01);
const B: Dev = Dev(0x02);
const C: Dev = Dev(0x03);
const D: Dev = Dev(0x04);
const E: Dev = Dev(0x05);

/// A sync-enabled profile of `ME` in the test vault, its own certificate not yet pinned.
fn profile() -> Connection {
    let conn = migrated();
    state::enable(&conn, ME.id()).expect("enable");
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1",
        [VaultId(VAULT).to_b64()],
    )
    .expect("vault");
    conn
}

/// `supported()` with the test vault's Kit pinned (created by this device), so a Kit joiner
/// (`Dev::kit`) verifies.
fn kit_supported() -> Connection {
    let conn = supported();
    let kit = the_kit();
    kit_row(&conn, &kit, &ME.creates_kit(&kit, NOW - 100));
    trust::refresh(&conn, NOW).expect("the Kit");
    conn
}

/// `profile()` with this device's genesis certificate pinned.
fn supported() -> Connection {
    let conn = profile();
    trust::seed_own(&conn, &ME.genesis(), NOW).expect("seed own");
    conn
}

/// Writes `dev`'s `sync_devices` row with `cert` (and its box key as `public_key`).
fn row(conn: &Connection, dev: Dev, cert: Option<&Certificate>) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, ?2, ?3, 'macos', '1.0', 1, ?4) \
         ON CONFLICT(id) DO UPDATE SET cert = excluded.cert",
        params![
            dev.id().to_hex(),
            "PK",
            format!("Device {}", dev.0),
            cert.map(Certificate::to_b64)
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

fn state_of(conn: &Connection, dev: Dev) -> Option<TrustState> {
    trust::entry(conn, dev.id())
        .expect("entry")
        .map(|e| e.state)
}

fn pinned(conn: &Connection) -> Vec<DeviceId> {
    trust::entries(conn)
        .expect("entries")
        .into_iter()
        .map(|e| e.device)
        .collect()
}

// ── Own certificate, supported ──────────────────────────────────────────────────────────────

#[test]
fn seed_own_pins_this_devices_certificate_and_makes_the_vault_supported() {
    let conn = profile();
    assert!(!trust::is_supported(&conn).unwrap(), "pre-phase-5 (§5.5)");

    trust::seed_own(&conn, &ME.genesis(), NOW).expect("seed");

    assert!(trust::is_supported(&conn).unwrap());
    let entry = trust::entry(&conn, ME.id()).unwrap().expect("pinned");
    assert_eq!(entry.state, TrustState::Trusted);
    assert_eq!(entry.kind, CertKind::Genesis);
    assert_eq!(entry.cert, ME.genesis());
    assert_eq!(entry.sign_pub, sign_pub(&ME.seed()));
    assert_eq!(entry.box_pub, [0x01 ^ 0xb0; 32]);
    assert_eq!(entry.issuer, ME.id());
    assert_eq!(entry.pinned_at, NOW);
    // The same certificate again changes nothing; another one is refused.
    trust::seed_own(&conn, &ME.genesis(), NOW + 1).expect("same again");
    assert!(trust::seed_own(&conn, &ME.kit(), NOW).is_err());
}

#[test]
fn seed_own_refuses_another_devices_or_another_vaults_certificate() {
    let conn = profile();
    assert!(trust::seed_own(&conn, &B.genesis(), NOW).is_err());
    let other_vault = cert::issue_self(
        CertKind::Genesis,
        &CertFields {
            vault_id: [0x22; 16],
            ..ME.fields(NOW)
        },
        &ME.seed(),
    )
    .unwrap();
    assert!(trust::seed_own(&conn, &other_vault, NOW).is_err());
    assert!(!trust::is_supported(&conn).unwrap());
}

#[test]
fn a_profile_with_sync_off_is_not_supported() {
    let conn = migrated();
    assert!(!trust::is_supported(&conn).unwrap());
}

// ── refresh: certificates ───────────────────────────────────────────────────────────────────

#[test]
fn a_paired_certificate_from_a_trusted_holder_is_pinned() {
    let conn = supported();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));

    let report = trust::refresh(&conn, NOW).expect("refresh");

    assert_eq!(report.pinned, vec![B.id()]);
    let entry = trust::entry(&conn, B.id()).unwrap().expect("pinned");
    assert_eq!(entry.kind, CertKind::Paired);
    assert_eq!(entry.issuer, ME.id());
    assert_eq!(entry.state, TrustState::Trusted);
    assert!(entry.ticket_pub.is_some());
    assert!(report.conflicts.is_empty());
}

#[test]
fn a_paired_certificate_waits_for_its_holder() {
    let conn = supported();
    // C was paired by B, whom this device does not know yet.
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    let report = trust::refresh(&conn, NOW).expect("refresh");
    assert!(report.pinned.is_empty());
    assert_eq!(state_of(&conn, C), None);

    // B arrives (paired by this device): both are pinned in one call.
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    let report = trust::refresh(&conn, NOW).expect("refresh");
    assert_eq!(report.pinned, vec![B.id(), C.id()]);
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
}

#[test]
fn a_chain_arriving_together_is_pinned_in_one_call() {
    let conn = supported();
    // D was paired by C, C by B, B by this device; the rows arrive in the "wrong" order.
    row(&conn, D, Some(&D.paired_by(C, 0x72)));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));

    let report = trust::refresh(&conn, NOW).expect("refresh");

    assert_eq!(report.pinned, vec![B.id(), C.id(), D.id()]);
}

#[test]
fn a_genesis_certificate_arriving_through_sync_is_never_pinned() {
    let conn = supported();
    row(&conn, B, Some(&B.genesis()));
    let report = trust::refresh(&conn, NOW).expect("refresh");
    assert!(report.pinned.is_empty());
    assert_eq!(state_of(&conn, B), None);
}

#[test]
fn a_recovery_kit_certificate_is_pinned_with_its_kind() {
    let conn = kit_supported();
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).expect("refresh");
    let entry = trust::entry(&conn, B.id()).unwrap().expect("pinned");
    assert_eq!(entry.kind, CertKind::RecoveryKit);
    assert_eq!(entry.issuer, the_kit().id());
}

#[test]
fn a_rejoin_certificate_is_pinned_when_its_predecessor_is_trusted() {
    let conn = kit_supported();
    row(&conn, C, Some(&C.rejoin_of(B, NOW)));
    trust::refresh(&conn, NOW).expect("refresh");
    assert_eq!(state_of(&conn, C), None, "B is unknown");

    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).expect("refresh");
    let entry = trust::entry(&conn, C.id()).unwrap().expect("pinned");
    assert_eq!(entry.kind, CertKind::Rejoin);
    assert_eq!(entry.issuer, B.id());
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted), "B stays");
}

#[test]
fn a_removed_issuer_vouches_only_for_what_it_issued_before_its_removal() {
    let conn = supported();
    with_b(&conn);
    set_record(&conn, B, "removal_record", &ME.removes_at(B, NOW + 10));
    // C was paired before B's removal, D after it.
    row(&conn, C, Some(&C.paired_by_at(B, 0x71, NOW + 5, NOW + 10)));
    row(&conn, D, Some(&D.paired_by_at(B, 0x72, NOW + 5, NOW + 11)));
    // E's ticket is from after the removal (a certificate is never older than its ticket).
    row(&conn, E, Some(&E.paired_by_at(B, 0x73, NOW + 11, NOW + 11)));

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, B), Some(TrustState::Removed));
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, D), None);
    assert_eq!(state_of(&conn, E), None);
}

#[test]
fn a_device_that_left_vouches_only_for_what_it_issued_before_it_left() {
    let conn = supported();
    with_b(&conn);
    set_record(&conn, B, "left_record", &B.left_at(NOW + 10));
    row(&conn, C, Some(&C.paired_by_at(B, 0x71, NOW + 5, NOW + 10)));
    row(&conn, D, Some(&D.paired_by_at(B, 0x72, NOW + 5, NOW + 11)));
    row(&conn, E, Some(&E.paired_by_at(B, 0x73, NOW + 11, NOW + 11)));

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, B), Some(TrustState::Left));
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, D), None, "certificate after leaving");
    assert_eq!(
        state_of(&conn, E),
        None,
        "ticket (and so certificate) after leaving"
    );

    // A re-join successor of a device that left: the same bound.
    let conn = supported();
    with_b(&conn);
    set_record(&conn, B, "left_record", &B.left_at(NOW + 10));
    row(&conn, C, Some(&C.rejoin_of(B, NOW + 11)));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, C), None);
}

#[test]
fn an_issuer_whose_pinned_key_differs_from_the_certificates_is_not_trusted() {
    let conn = kit_supported();
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).unwrap();
    // A certificate naming B as the holder but vouched for by another key (E's).
    let (secret, ticket) = cert::ticket_from_secret(
        TicketSecret::from_bytes(Zeroizing::new([0x73; 32])),
        &E.seed(),
        VAULT,
        B.id(),
        NOW,
    );
    let forged = cert::issue_paired(
        &secret,
        &ticket,
        B.id(),
        &sign_pub(&E.seed()),
        &C.fields(NOW),
        &C.seed(),
    )
    .expect("well-formed");
    row(&conn, C, Some(&forged));

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, C), None);
}

#[test]
fn a_certificate_for_another_vault_or_another_row_is_not_pinned() {
    let conn = supported();
    let other_kit = KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&[0x22; 16]);
    let other_vault = cert::issue_kit_join(
        &other_kit,
        &CertFields {
            vault_id: [0x22; 16],
            ..B.fields(NOW)
        },
        &B.seed(),
    )
    .unwrap();
    row(&conn, B, Some(&other_vault));
    // C's row carries D's certificate.
    row(&conn, C, Some(&D.kit()));
    // Junk in the column.
    conn.execute(
        "INSERT INTO sync_devices (id, name, platform, app_version, joined_at, cert) \
         VALUES (?1, 'x', 'ios', '1', 1, 'not a certificate')",
        [E.id().to_hex()],
    )
    .unwrap();

    let report = trust::refresh(&conn, NOW).unwrap();

    assert!(report.pinned.is_empty());
    assert_eq!(pinned(&conn), vec![ME.id()]);
}

// ── Pinning ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_second_certificate_for_a_pinned_id_changes_nothing_and_is_reported() {
    let conn = kit_supported();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    trust::refresh(&conn, NOW).unwrap();
    let before = trust::entry(&conn, B.id()).unwrap().expect("pinned");

    // The synced column now holds a different, valid certificate for B.
    row(&conn, B, Some(&B.kit()));
    let report = trust::refresh(&conn, NOW + 5).unwrap();

    assert_eq!(trust::entry(&conn, B.id()).unwrap(), Some(before));
    assert!(report.pinned.is_empty());
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].device, B.id());
    assert_eq!(report.conflicts[0].kind, ConflictKind::OtherCertificate);
}

#[test]
fn a_ticket_used_by_a_second_id_is_refused_and_reported() {
    let conn = supported();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    trust::refresh(&conn, NOW).unwrap();
    // C was certified with the same ticket.
    row(&conn, C, Some(&C.paired_by(ME, 0x70)));

    let report = trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, C), None);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].device, C.id());
    assert_eq!(report.conflicts[0].kind, ConflictKind::TicketReused);
}

#[test]
fn two_ids_with_one_ticket_in_one_refresh_pin_only_the_first() {
    let conn = supported();
    row(&conn, C, Some(&C.paired_by(ME, 0x70)));
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));

    let report = trust::refresh(&conn, NOW).unwrap();

    assert_eq!(
        report.pinned,
        vec![B.id()],
        "the lower ID, whatever the order"
    );
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].kind, ConflictKind::TicketReused);
}

// ── Records ─────────────────────────────────────────────────────────────────────────────────

fn with_b(conn: &Connection) {
    row(conn, B, Some(&B.paired_by(ME, 0x70)));
    trust::refresh(conn, NOW).unwrap();
}

#[test]
fn a_left_record_signed_by_its_subject_moves_it_to_left() {
    let conn = supported();
    with_b(&conn);
    set_record(&conn, B, "left_record", &B.left());

    let report = trust::refresh(&conn, NOW).unwrap();

    let entry = trust::entry(&conn, B.id()).unwrap().unwrap();
    assert_eq!(entry.state, TrustState::Left);
    assert_eq!(entry.state_record, Some(B.left()));
    assert_eq!(entry.state_by, Some(B.id()));
    assert_eq!(entry.state_at, Some(NOW));
    assert_eq!(entry.cut_off_epoch, None);
    assert_eq!(report.moved, vec![(B.id(), TrustState::Left)]);
}

#[test]
fn a_left_record_signed_by_another_key_is_ignored() {
    let conn = supported();
    with_b(&conn);
    // Shaped as B's, signed with C's key.
    let forged = Record::sign(RecordType::Left, VAULT, B.id(), B.id(), NOW, 0, &C.seed()).unwrap();
    set_record(&conn, B, "left_record", &forged);
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
}

#[test]
fn a_removed_record_from_a_trusted_signer_removes_with_no_cut_off() {
    let conn = supported();
    with_b(&conn);
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, C, "removal_record", &B.removes(C));

    trust::refresh(&conn, NOW).unwrap();

    let entry = trust::entry(&conn, C.id()).unwrap().unwrap();
    assert_eq!(entry.state, TrustState::Removed);
    assert_eq!(entry.cut_off_epoch, None, "it still knows the key");
    assert_eq!(entry.state_by, Some(B.id()));
    assert_eq!(entry.state_record, Some(B.removes(C)));
}

#[test]
fn a_removed_record_from_an_unknown_or_removed_signer_is_ignored() {
    let conn = supported();
    with_b(&conn);
    // D is unknown here.
    set_record(&conn, B, "removal_record", &D.removes(B));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));

    // C is removed (by this device) before its record about B arrives.
    let conn = kit_supported();
    with_b(&conn);
    row(&conn, C, Some(&C.kit()));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, C, "removal_record", &ME.removes(C));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, C), Some(TrustState::Removed));
    // Signed after C's removal (fix round: one signed before it counts).
    set_record(&conn, B, "removal_record", &C.removes_at(B, NOW + 1));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
}

#[test]
fn removed_never_moves_back_and_left_moves_on_to_removed() {
    let conn = supported();
    with_b(&conn);
    set_record(&conn, B, "left_record", &B.left());
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Left));
    set_record(&conn, B, "removal_record", &ME.removes(B));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Removed));

    // Nothing brings it back: not a refresh, not a mark, not a bundle naming it a recipient.
    assert!(!trust::mark_left(&conn, &B.left()).unwrap());
    trust::import_bundle(
        &conn,
        &ME.genesis(),
        &[B.paired_by(ME, 0x70)],
        None,
        &[],
        1,
        NOW,
    )
    .unwrap();
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Removed));
}

#[test]
fn a_record_naming_this_device_is_reported_about_me() {
    let conn = supported();
    with_b(&conn);
    row(&conn, ME, Some(&ME.genesis()));
    set_record(&conn, ME, "removal_record", &B.removes(ME));

    let report = trust::refresh(&conn, NOW).unwrap();

    assert_eq!(report.about_me, vec![B.removes(ME)]);
    assert_eq!(
        state_of(&conn, ME),
        Some(TrustState::Trusted),
        "the caller turns sync off; the store is not touched"
    );

    // A `left` record for this device it did not make (another holder of its key).
    let conn = supported();
    row(&conn, ME, Some(&ME.genesis()));
    set_record(&conn, ME, "left_record", &ME.left());
    let report = trust::refresh(&conn, NOW).unwrap();
    assert_eq!(report.about_me, vec![ME.left()]);

    // A forged record naming this device is not.
    let conn = supported();
    row(&conn, ME, Some(&ME.genesis()));
    set_record(&conn, ME, "removal_record", &D.removes(ME));
    assert!(trust::refresh(&conn, NOW).unwrap().about_me.is_empty());
}

#[test]
fn records_and_removals_arriving_together_are_judged_against_the_store_before_them() {
    // B removed C and this device removed B; both records arrive in one pull: B was trusted when
    // it removed C, so both apply, in either row order.
    let conn = kit_supported();
    with_b(&conn);
    row(&conn, C, Some(&C.kit()));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, C, "removal_record", &B.removes(C));
    set_record(&conn, B, "removal_record", &ME.removes(B));

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, B), Some(TrustState::Removed));
    assert_eq!(state_of(&conn, C), Some(TrustState::Removed));
}

/// Every order of `items`.
fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for i in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            out.push(tail);
        }
    }
    out
}

/// A store that knows B, C, D and E, each paired by this device.
fn with_four() -> Connection {
    let conn = supported();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.paired_by(ME, 0x72)));
    row(&conn, E, Some(&E.paired_by(ME, 0x73)));
    trust::refresh(&conn, NOW).unwrap();
    conn
}

#[test]
fn records_give_the_same_store_in_one_pass_and_in_every_order_across_passes() {
    // B removed C before this device removed B; D removed E before it left. Each record was
    // signed while its signer was in, so each counts however the records arrive.
    let records: Vec<(Dev, &str, Record)> = vec![
        (C, "removal_record", B.removes_at(C, NOW + 10)),
        (B, "removal_record", ME.removes_at(B, NOW + 20)),
        (E, "removal_record", D.removes_at(E, NOW + 5)),
        (D, "left_record", D.left_at(NOW + 8)),
    ];

    let together = with_four();
    for (dev, column, record) in &records {
        set_record(&together, *dev, column, record);
    }
    trust::refresh(&together, NOW).unwrap();
    let expected = comparable(&together);
    assert_eq!(state_of(&together, B), Some(TrustState::Removed));
    assert_eq!(state_of(&together, C), Some(TrustState::Removed));
    assert_eq!(state_of(&together, D), Some(TrustState::Left));
    assert_eq!(state_of(&together, E), Some(TrustState::Removed));

    for order in permutations(&records) {
        let conn = with_four();
        for (dev, column, record) in &order {
            set_record(&conn, *dev, column, record);
            trust::refresh(&conn, NOW).unwrap();
        }
        let names: Vec<String> = order
            .iter()
            .map(|(d, c, _)| format!("{}:{c}", d.0))
            .collect();
        assert_eq!(comparable(&conn), expected, "order {names:?}");
    }
}

#[test]
fn a_removal_signed_after_its_signer_left_or_was_removed_does_not_count() {
    let conn = with_four();
    set_record(&conn, B, "removal_record", &ME.removes_at(B, NOW + 20));
    set_record(&conn, D, "left_record", &D.left_at(NOW + 8));
    trust::refresh(&conn, NOW).unwrap();

    set_record(&conn, C, "removal_record", &B.removes_at(C, NOW + 21));
    set_record(&conn, E, "removal_record", &D.removes_at(E, NOW + 9));
    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, E), Some(TrustState::Trusted));
}

#[test]
fn a_device_that_left_still_vouches_for_what_it_paired_before() {
    // The holder pairs a new phone and is then retired; this device hears of both at once.
    let conn = supported();
    with_b(&conn);
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    set_record(&conn, B, "left_record", &B.left());

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, B), Some(TrustState::Left));
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
}

#[test]
fn mark_left_and_mark_removed_verify_the_record() {
    let conn = kit_supported();
    with_b(&conn);
    row(&conn, C, Some(&C.kit()));
    trust::refresh(&conn, NOW).unwrap();

    assert!(trust::mark_left(&conn, &B.left()).unwrap());
    assert_eq!(state_of(&conn, B), Some(TrustState::Left));
    // A removal by an unknown signer is refused.
    assert!(trust::mark_removed(&conn, &D.removes(C), Some(1)).is_err());
    assert!(trust::mark_removed(&conn, &ME.removes(C), Some(2)).unwrap());
    let entry = trust::entry(&conn, C.id()).unwrap().unwrap();
    assert_eq!(entry.state, TrustState::Removed);
    assert_eq!(entry.cut_off_epoch, Some(2));
    // Removed again changes nothing.
    assert!(!trust::mark_removed(&conn, &ME.removes(C), Some(3)).unwrap());
    assert_eq!(
        trust::entry(&conn, C.id()).unwrap().unwrap().cut_off_epoch,
        Some(2)
    );
}

#[test]
fn cut_off_all_pending_sets_the_epoch_of_left_and_list_removed_entries_only() {
    let conn = kit_supported();
    with_b(&conn);
    row(&conn, C, Some(&C.kit()));
    row(&conn, D, Some(&D.kit()));
    row(&conn, E, Some(&E.kit()));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &B.left()).unwrap();
    trust::mark_removed(&conn, &ME.removes(C), None).unwrap();
    trust::mark_removed(&conn, &ME.removes(D), Some(1)).unwrap();

    assert_eq!(trust::cut_off_all_pending(&conn, 2).unwrap(), 2);

    let cut = |dev: Dev| {
        trust::entry(&conn, dev.id())
            .unwrap()
            .unwrap()
            .cut_off_epoch
    };
    assert_eq!(cut(B), Some(2));
    assert_eq!(cut(C), Some(2));
    assert_eq!(cut(D), Some(1), "already cut off");
    assert_eq!(cut(E), None, "trusted");
    assert_eq!(cut(ME), None);
}

// ── Export and import ───────────────────────────────────────────────────────────────────────

/// A holder's store: itself (genesis), B paired by it, C who left, D removed by it.
fn holder_store() -> Connection {
    let conn = kit_supported();
    with_b(&conn);
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.kit()));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &C.left()).unwrap();
    trust::mark_removed(&conn, &ME.removes(D), Some(1)).unwrap();
    conn
}

/// A fresh joiner E in the same vault, nothing pinned.
fn joiner() -> Connection {
    let conn = migrated();
    state::enable(&conn, E.id()).expect("enable");
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1",
        [VaultId(VAULT).to_b64()],
    )
    .unwrap();
    conn
}

type Comparable = (DeviceId, TrustState, Option<Record>, Option<u32>, bool);

fn comparable(conn: &Connection) -> Vec<Comparable> {
    trust::entries(conn)
        .unwrap()
        .into_iter()
        .map(|e| {
            (
                e.device,
                e.state,
                e.state_record,
                e.cut_off_epoch,
                e.confirmed,
            )
        })
        .collect()
}

#[test]
fn an_export_imports_on_an_empty_store() {
    let holder = holder_store();
    let export = trust::export(&holder).expect("export");
    let json = serde_json::to_string(&export).expect("json");
    let export: TrustExport = serde_json::from_str(&json).expect("parse");

    let conn = joiner();
    trust::import_export(&conn, &export, ME.id(), NOW).expect("import");

    assert_eq!(comparable(&conn), comparable(&holder));
    let genesis = trust::entry(&conn, ME.id()).unwrap().unwrap();
    assert_eq!(
        genesis.kind,
        CertKind::Genesis,
        "a genesis comes in by the export"
    );
    assert!(
        !trust::is_supported(&conn).unwrap(),
        "the joiner's own certificate is pinned by the join"
    );
}

#[test]
fn an_export_without_the_holders_certificate_is_refused() {
    let holder = holder_store();
    let mut export = trust::export(&holder).unwrap();
    let conn = joiner();
    // Named holder B is in it but this claims C as the holder... C left, so is not trusted.
    assert!(trust::import_export(&conn, &export, C.id(), NOW).is_err());
    assert!(trust::import_export(&conn, &export, E.id(), NOW).is_err());
    let own = ME.genesis().to_b64();
    export.entries.retain(|e| e.cert != own);
    assert!(trust::import_export(&conn, &export, ME.id(), NOW).is_err());
    assert!(trust::entries(&conn).unwrap().is_empty(), "nothing written");
}

#[test]
fn an_export_with_an_entry_that_does_not_verify_is_refused_and_writes_nothing() {
    let holder = holder_store();
    let good = trust::export(&holder).unwrap();
    let conn = joiner();

    // A certificate with a flipped byte.
    let mut bad = good.clone();
    let mut bytes = B.paired_by(ME, 0x70).encode();
    bytes[120] ^= 1;
    let b = B.paired_by(ME, 0x70).to_b64();
    let entry = bad.entries.iter_mut().find(|e| e.cert == b).unwrap();
    entry.cert = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes);
    assert!(trust::import_export(&conn, &bad, ME.id(), NOW).is_err());

    // A `left` state whose record another key signed.
    let mut bad = good.clone();
    let c = C.paired_by(ME, 0x71).to_b64();
    let entry = bad.entries.iter_mut().find(|e| e.cert == c).unwrap();
    entry.record = Some(
        Record::sign(RecordType::Left, VAULT, C.id(), C.id(), NOW, 0, &B.seed())
            .unwrap()
            .to_b64(),
    );
    assert!(trust::import_export(&conn, &bad, ME.id(), NOW).is_err());

    // A removed state without a record.
    let mut bad = good.clone();
    let d = D.kit().to_b64();
    bad.entries.iter_mut().find(|e| e.cert == d).unwrap().record = None;
    assert!(trust::import_export(&conn, &bad, ME.id(), NOW).is_err());

    // A trusted state with a cut-off epoch.
    let mut bad = good.clone();
    let b = B.paired_by(ME, 0x70).to_b64();
    bad.entries
        .iter_mut()
        .find(|e| e.cert == b)
        .unwrap()
        .cut_off_epoch = Some(1);
    assert!(trust::import_export(&conn, &bad, ME.id(), NOW).is_err());

    assert!(trust::entries(&conn).unwrap().is_empty(), "nothing written");
    trust::import_export(&conn, &good, ME.id(), NOW).expect("the good one still imports");
}

#[test]
fn an_export_refuses_a_store_that_is_not_empty() {
    let holder = holder_store();
    let export = trust::export(&holder).unwrap();
    let conn = joiner();
    trust::import_export(&conn, &export, ME.id(), NOW).unwrap();
    assert!(trust::import_export(&conn, &export, ME.id(), NOW).is_err());
}

#[test]
fn an_export_rejects_unknown_fields() {
    let json = r#"{"entries":[],"extra":1}"#;
    assert!(serde_json::from_str::<TrustExport>(json).is_err());
    let json = r#"{"entries":[{"cert":"x","state":"trusted","surprise":true}]}"#;
    assert!(serde_json::from_str::<TrustExport>(json).is_err());
}

// ── Bundles and the Kit joiner ──────────────────────────────────────────────────────────────

#[test]
fn a_bundle_pins_its_signer_and_recipients_and_applies_its_removals_at_once() {
    // This device knows B and the signer C; the recipient D comes in by the bundle, which also
    // removes B. (A signer not pinned here is refused: `a_bundle_signed_by_a_device_not_pinned…`.)
    let conn = supported();
    with_b(&conn);
    let signer = C.paired_by(B, 0x71);
    row(&conn, C, Some(&signer));
    trust::refresh(&conn, NOW).unwrap();
    let d = D.paired_by(E, 0x72); // E is unknown here: endorsed by the signer anyway.

    trust::import_bundle(
        &conn,
        &signer,
        &[ME.genesis(), d.clone()],
        None,
        &[C.removes(B)],
        3,
        NOW,
    )
    .expect("import");

    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, D), Some(TrustState::Trusted));
    assert_eq!(trust::entry(&conn, D.id()).unwrap().unwrap().cert, d);
    let b = trust::entry(&conn, B.id()).unwrap().unwrap();
    assert_eq!(b.state, TrustState::Removed);
    assert_eq!(b.cut_off_epoch, Some(3));
    // The signer is confirmed: B's removal does not drop it.
    assert!(trust::entry(&conn, C.id()).unwrap().unwrap().confirmed);
    assert!(included(&conn).contains(&C.id()));
}

#[test]
fn a_bundle_signed_by_a_device_not_pinned_here_or_with_another_certificate_is_refused() {
    let conn = supported();
    with_b(&conn);
    let before = trust::entries(&conn).unwrap();
    // C is not pinned here.
    assert!(trust::import_bundle(
        &conn,
        &C.paired_by(B, 0x71),
        &[ME.genesis()],
        None,
        &[],
        1,
        NOW
    )
    .is_err());
    // B is, with another certificate.
    assert!(trust::import_bundle(&conn, &B.kit(), &[ME.genesis()], None, &[], 1, NOW).is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before);
}

#[test]
fn a_bundle_never_replaces_a_pinned_certificate_and_writes_nothing_when_it_fails() {
    let with_c = || {
        let conn = kit_supported();
        with_b(&conn);
        row(&conn, C, Some(&C.kit()));
        trust::refresh(&conn, NOW).unwrap();
        conn
    };
    let conn = with_c();

    // B's certificate in the bundle differs from the pinned one: the pin stays.
    trust::import_bundle(&conn, &C.kit(), &[B.kit()], None, &[], 1, NOW).unwrap();
    assert_eq!(
        trust::entry(&conn, B.id()).unwrap().unwrap().cert,
        B.paired_by(ME, 0x70)
    );

    // A recipient that does not verify fails the whole import.
    let conn = with_c();
    let before = trust::entries(&conn).unwrap();
    let mut broken = D.kit();
    broken.subject_sig[0] ^= 1;
    assert!(
        trust::import_bundle(&conn, &C.kit(), &[broken], None, &[C.removes(B)], 1, NOW).is_err()
    );
    assert_eq!(trust::entries(&conn).unwrap(), before);
}

/// The genesis certificates a Kit joiner may trust on first use.
fn genesis(devices: &[Dev]) -> std::collections::BTreeSet<DeviceId> {
    devices.iter().map(|d| d.id()).collect()
}

/// As built R-28: a genesis row the Kit's chain and the confirmed set do not name is not trusted
/// on first use, and what it vouches for waits for it (never a recipient).
#[test]
fn a_kit_joiner_trusts_only_the_genesis_its_kit_or_the_confirmed_set_names() {
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, C, Some(&C.genesis()));
    row(&conn, D, Some(&D.paired_by(C, 0x72)));

    trust::tofu_from_rows(&conn, NOW, &genesis(&[B])).expect("tofu");

    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, C), None, "a genesis nothing names");
    assert_eq!(state_of(&conn, D), None, "it waits for its issuer");
    assert_eq!(
        trust::kit_join_genesis(&conn, ME.id(), &[(C.id(), C.genesis().hash())]).unwrap(),
        genesis(&[C]),
        "the confirmed set names C with its certificate"
    );
    assert!(
        trust::kit_join_genesis(&conn, ME.id(), &[(C.id(), [0; 32])])
            .unwrap()
            .is_empty(),
        "another certificate's hash names nothing"
    );
}

#[test]
fn a_kit_joiners_kit_chain_names_the_genesis_above_it() {
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    // The Kit, made by C (paired by B).
    let kit = KitSecret::from_bytes(Zeroizing::new([0x31; 32])).derive(&VAULT);
    kit_row(&conn, &kit, &C.creates_kit(&kit, NOW));

    assert_eq!(
        trust::kit_join_genesis(&conn, kit.id(), &[]).unwrap(),
        genesis(&[B])
    );
}

#[test]
fn a_kit_joiner_trusts_every_row_that_verifies_on_first_use() {
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    row(&conn, D, Some(&D.paired_by(E, 0x72)));
    let mut broken = E.kit();
    broken.issuer_sig[3] ^= 1;
    row(&conn, E, Some(&broken));
    set_record(&conn, C, "left_record", &C.left());

    trust::tofu_from_rows(&conn, NOW, &genesis(&[B])).expect("tofu");

    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted), "genesis too");
    assert_eq!(state_of(&conn, C), Some(TrustState::Left), "records apply");
    assert_eq!(
        state_of(&conn, D),
        Some(TrustState::Trusted),
        "its issuer is unknown"
    );
    assert_eq!(state_of(&conn, E), None, "does not verify");
}

// ── Recipients ──────────────────────────────────────────────────────────────────────────────

fn clock(conn: &Connection, dev: Dev, at_secs: i64) {
    conn.execute(
        "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES ('institutions', ?1, '*', ?2, ?3)",
        params![
            format!("pk-{}-{at_secs}", dev.0),
            pack(at_secs as u64 * 1000, 0) as i64,
            dev.id().0.to_vec()
        ],
    )
    .unwrap();
}

#[test]
fn recipients_are_the_trusted_devices_minus_the_removed_ones_with_reasons() {
    let conn = kit_supported();
    row(&conn, ME, Some(&ME.genesis()));
    with_b(&conn);
    row(&conn, C, Some(&C.kit()));
    row(&conn, D, Some(&D.kit()));
    row(&conn, E, Some(&E.kit()));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &D.left()).unwrap();
    trust::mark_removed(&conn, &ME.removes(E), None).unwrap();
    // F: a row whose certificate does not verify here; G: a row with no key at all.
    let f = Dev(0x06);
    row(&conn, f, Some(&f.genesis()));
    let g = Dev(0x07);
    conn.execute(
        "INSERT INTO sync_devices (id, name, platform, app_version, joined_at) \
         VALUES (?1, 'G', 'ios', '1', 1)",
        [g.id().to_hex()],
    )
    .unwrap();

    let plan = trust::recipients(&conn, &[C.id()]).expect("recipients");

    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id(), B.id()]);
    assert_eq!(
        plan.excluded,
        vec![
            (C.id(), ExclusionReason::Removing),
            (D.id(), ExclusionReason::Left),
            (E.id(), ExclusionReason::Removed),
            (f.id(), ExclusionReason::Unverified),
            (g.id(), ExclusionReason::NoKey),
        ]
    );
    // Each recipient is sealed to its pinned box key.
    assert_eq!(plan.included[1].box_pub, [0x02 ^ 0xb0; 32]);
}

#[test]
fn a_superseded_id_gets_the_key_only_after_a_change_since_its_successor() {
    let conn = supported();
    with_b(&conn);
    // C re-joined as D at NOW + 100; this device pinned D then.
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.rejoin_of(C, NOW + 100)));
    trust::refresh(&conn, NOW + 100).unwrap();
    clock(&conn, C, NOW + 50);

    let plan = trust::recipients(&conn, &[]).unwrap();
    assert!(plan
        .excluded
        .contains(&(C.id(), ExclusionReason::SupersededQuiet)));
    assert!(plan.included.iter().any(|e| e.device == D.id()));

    // C wrote a change after its successor appeared: a live copy, it stays a recipient (O11).
    clock(&conn, C, NOW + 200);
    let plan = trust::recipients(&conn, &[]).unwrap();
    assert!(plan.included.iter().any(|e| e.device == C.id()));
    assert!(plan.included.iter().any(|e| e.device == D.id()));
}

#[test]
fn more_than_64_recipients_are_refused() {
    let conn = supported();
    for i in 0..64u8 {
        let dev = Dev(0x10 + i);
        row(&conn, dev, Some(&dev.paired_by(ME, 0x80 + i)));
    }
    trust::refresh(&conn, NOW).unwrap();
    let error = trust::recipients(&conn, &[]).expect_err("65 recipients");
    assert_eq!(
        error.to_string(),
        "Validation error: validation.syncTooManyDevices"
    );
    // Removing one brings it to 64.
    let plan = trust::recipients(&conn, &[Dev(0x10).id()]).expect("64");
    assert_eq!(plan.included.len(), 64);
}

// ── import_bundle: who may remove, and records about this device (fix round) ────────────────

#[test]
fn a_bundle_removal_counts_by_the_rule_of_refresh() {
    // C left at NOW + 10: what it removed before counts, what it dates after does not.
    let conn = with_four();
    trust::mark_left(&conn, &C.left_at(NOW + 10)).unwrap();
    trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[],
        None,
        &[C.removes_at(D, NOW + 9)],
        2,
        NOW,
    )
    .expect("signed while C was in");
    assert_eq!(state_of(&conn, D), Some(TrustState::Removed));
    let before = trust::entries(&conn).unwrap();
    assert!(trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[],
        None,
        &[C.removes_at(E, NOW + 11)],
        2,
        NOW
    )
    .is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before, "nothing written");

    // An unconfirmed Kit device's removal does not count.
    let conn = with_four();
    row(&conn, Dev(0x06), Some(&Dev(0x06).kit()));
    trust::refresh(&conn, NOW).unwrap();
    assert!(trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[],
        None,
        &[Dev(0x06).removes(E)],
        2,
        NOW
    )
    .is_err());
    assert_eq!(state_of(&conn, E), Some(TrustState::Trusted));
}

#[test]
fn a_bundle_removal_of_this_device_is_returned_not_applied() {
    let conn = with_four();

    let about_me = trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[C.paired_by(ME, 0x71)],
        None,
        &[B.removes(ME), B.removes(D)],
        2,
        NOW,
    )
    .expect("import");

    assert_eq!(about_me, vec![B.removes(ME)]);
    assert_eq!(state_of(&conn, ME), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, D), Some(TrustState::Removed));
}

// ── The Kit in the store (revision §2.3) ────────────────────────────────────────────────────

impl Dev {
    /// The Kit's own certificate (kind 5) with `self` as its creator.
    fn creates_kit(self, kit: &KitKeys, at: i64) -> Certificate {
        cert::issue_kit(&self.seed(), self.id(), kit, at).expect("kit")
    }
}

fn second_kit() -> KitKeys {
    KitSecret::from_bytes(Zeroizing::new([0x0d; 32])).derive(&VAULT)
}

/// Writes the Kit's `sync_devices` row (platform `recoveryKit`) with its certificate.
fn kit_row(conn: &Connection, kit: &KitKeys, cert: &Certificate) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, 'KITPK', '', 'recoveryKit', '1.0', 1, ?2) \
         ON CONFLICT(id) DO UPDATE SET cert = excluded.cert",
        params![kit.id().to_hex(), cert.to_b64()],
    )
    .expect("kit row");
}

fn kit_removal(signer: Dev, kit: &KitKeys, at: i64) -> Record {
    Record::sign(
        RecordType::Removed,
        VAULT,
        kit.id(),
        signer.id(),
        at,
        0,
        &signer.seed(),
    )
    .expect("removal of the Kit")
}

#[test]
fn the_kit_is_pinned_when_its_creator_vouches_and_never_listed_as_a_recipient() {
    let conn = supported();
    let kit = the_kit();
    kit_row(&conn, &kit, &ME.creates_kit(&kit, NOW));
    let report = trust::refresh(&conn, NOW).unwrap();
    assert_eq!(report.pinned, vec![kit.id()]);
    let entry = trust::entry(&conn, kit.id()).unwrap().expect("pinned");
    assert_eq!(entry.kind, CertKind::Kit);
    assert_eq!(&entry.box_pub, kit.box_pub());

    let plan = trust::recipients(&conn, &[]).unwrap();
    assert_eq!(plan.kit.map(|k| k.device), Some(kit.id()));
    assert!(plan.included.iter().all(|e| e.device != kit.id()));
    assert!(plan.excluded.iter().all(|(d, _)| *d != kit.id()));
    assert!(
        trust::new_devices(&conn).unwrap().is_empty(),
        "no notice for a Kit"
    );
}

#[test]
fn a_kit_whose_creator_is_unknown_waits_and_an_unpinned_kit_row_is_not_listed() {
    let conn = supported();
    let kit = the_kit();
    kit_row(&conn, &kit, &B.creates_kit(&kit, NOW));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(trust::entry(&conn, kit.id()).unwrap(), None);
    let plan = trust::recipients(&conn, &[]).unwrap();
    assert_eq!(plan.kit, None);
    assert!(plan.excluded.iter().all(|(d, _)| *d != kit.id()));
    // Once B is pinned, its Kit is.
    with_b(&conn);
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
    assert!(trust::entry(&conn, kit.id()).unwrap().is_some());
}

#[test]
fn a_second_kit_is_never_pinned_by_a_refresh_even_once_the_first_is_removed() {
    let conn = supported();
    let first = the_kit();
    let second = second_kit();
    kit_row(&conn, &first, &ME.creates_kit(&first, NOW));
    trust::refresh(&conn, NOW).unwrap();
    kit_row(&conn, &second, &ME.creates_kit(&second, NOW + 10));
    let report = trust::refresh(&conn, NOW).unwrap();
    assert_eq!(trust::entry(&conn, second.id()).unwrap(), None);
    assert!(report.conflicts.contains(&trust::TrustConflict {
        device: second.id(),
        kind: ConflictKind::SecondKit,
    }));

    // The first Kit's removal arrives with the second: the second still waits for the key
    // change that names it (R-24; `a_replacement_kit_is_pinned_only_through_…`).
    conn.execute(
        "UPDATE sync_devices SET removal_record = ?1 WHERE id = ?2",
        params![
            kit_removal(ME, &first, NOW + 10).to_b64(),
            first.id().to_hex()
        ],
    )
    .unwrap();
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(
        trust::entry(&conn, first.id()).unwrap().map(|e| e.state),
        Some(TrustState::Removed)
    );
    assert_eq!(trust::entry(&conn, second.id()).unwrap(), None);
    assert_eq!(trust::recipients(&conn, &[]).unwrap().kit, None);
}

#[test]
fn a_kit_joiner_is_pinned_through_the_kit_and_after_its_removal_only_if_dated_before() {
    let conn = supported();
    let kit = the_kit();
    // B joined with the Kit; the Kit itself is unknown here yet.
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), None, "the Kit is not pinned");
    kit_row(&conn, &kit, &ME.creates_kit(&kit, NOW - 100));
    let report = trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
    assert_eq!(
        trust::entry(&conn, B.id()).unwrap().unwrap().issuer,
        kit.id()
    );
    // A notice for the Kit joiner, none for the Kit.
    let notices = trust::new_devices(&conn).unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].device, B.id().to_hex());
    assert_eq!(notices[0].by, kit.id().to_hex());
    assert!(notices[0].via_kit);
    assert!(report.pinned.contains(&B.id()));

    // The Kit replaced at NOW: a joiner dated after is not pinned, one dated before is.
    let conn = supported();
    kit_row(&conn, &kit, &ME.creates_kit(&kit, NOW - 100));
    conn.execute(
        "UPDATE sync_devices SET removal_record = ?1 WHERE id = ?2",
        params![kit_removal(ME, &kit, NOW).to_b64(), kit.id().to_hex()],
    )
    .unwrap();
    trust::refresh(&conn, NOW).unwrap();
    let late = cert::issue_kit_join(&kit, &C.fields(NOW + 1), &C.seed()).unwrap();
    let early = cert::issue_kit_join(&kit, &D.fields(NOW - 1), &D.seed()).unwrap();
    row(&conn, C, Some(&late));
    row(&conn, D, Some(&early));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, C), None, "dated after the Kit's removal");
    assert_eq!(state_of(&conn, D), Some(TrustState::Trusted));
}

// ── Superseded IDs speak until their successor (revision §3.1) ─────────────────────────────

#[test]
fn a_superseded_id_vouches_only_for_what_it_issued_before_its_successor() {
    let conn = supported();
    with_b(&conn);
    // B re-joined as C at NOW + 100; this device pinned C then.
    row(&conn, C, Some(&C.rejoin_of(B, NOW + 100)));
    trust::refresh(&conn, NOW + 100).unwrap();
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    // A copy of B pairs D after that, and E before.
    row(
        &conn,
        D,
        Some(&D.paired_by_at(B, 0x73, NOW + 150, NOW + 150)),
    );
    row(&conn, E, Some(&E.paired_by_at(B, 0x74, NOW + 50, NOW + 50)));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, D), None, "issued after the successor");
    assert_eq!(state_of(&conn, E), Some(TrustState::Trusted));
    // A removal B signs after its successor does not count either.
    let f = Dev(0x06);
    row(&conn, f, Some(&f.paired_by(ME, 0x75)));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, f, "removal_record", &B.removes_at(f, NOW + 200));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, f), Some(TrustState::Trusted));
}

// ── IssuerRemoved, transitive (revision §3.1, review HIGH 1) ────────────────────────────────

fn reasons(conn: &Connection) -> Vec<(DeviceId, ExclusionReason)> {
    trust::recipients(conn, &[]).unwrap().excluded
}

fn included(conn: &Connection) -> Vec<DeviceId> {
    trust::recipients(conn, &[])
        .unwrap()
        .included
        .into_iter()
        .map(|e| e.device)
        .collect()
}

/// B paired by ME... no: A (= `Dev(0x0a)`) is paired by ME, B paired by A, C paired by B, D a
/// re-join of C; then A is removed by ME. Nothing is confirmed.
fn removal_chain() -> Connection {
    let a = Dev(0x0a);
    let conn = supported();
    row(&conn, a, Some(&a.paired_by(ME, 0x70)));
    row(&conn, B, Some(&B.paired_by(a, 0x71)));
    row(&conn, C, Some(&C.paired_by(B, 0x72)));
    row(&conn, D, Some(&D.rejoin_of(C, NOW + 10)));
    trust::refresh(&conn, NOW).unwrap();
    for dev in [a, B, C, D] {
        assert_eq!(state_of(&conn, dev), Some(TrustState::Trusted));
    }
    trust::mark_removed(&conn, &ME.removes_at(a, NOW + 20), Some(1)).unwrap();
    // D writes, so it is no quiet superseded copy: only its issuer chain decides.
    clock(&conn, D, NOW + 30);
    clock(&conn, C, NOW + 30);
    conn
}

#[test]
fn issuer_removed_is_transitive_down_the_whole_chain() {
    let conn = removal_chain();
    let a = Dev(0x0a);
    assert_eq!(
        reasons(&conn),
        vec![
            (B.id(), ExclusionReason::IssuerRemoved),
            (C.id(), ExclusionReason::IssuerRemoved),
            (D.id(), ExclusionReason::IssuerRemoved),
            (a.id(), ExclusionReason::Removed),
        ]
    );
    assert_eq!(included(&conn), vec![ME.id()]);
}

#[test]
fn a_confirmed_entry_stops_the_chain() {
    let conn = removal_chain();
    // B was named by the bundle that removed A: confirmed.
    trust::mark_confirmed(&conn, &[B.id()]).unwrap();
    assert_eq!(included(&conn), vec![ME.id(), B.id(), C.id(), D.id()]);
    // C confirmed instead: B is out, C and D stay.
    let conn = removal_chain();
    trust::mark_confirmed(&conn, &[C.id()]).unwrap();
    assert_eq!(included(&conn), vec![ME.id(), C.id(), D.id()]);
    assert!(reasons(&conn).contains(&(B.id(), ExclusionReason::IssuerRemoved)));
}

/// Final review M-1: "Je moje" is for a late vouch or an unknown issuer; a device whose issuer was
/// removed (or is left out for that) is not kept, whatever the UI sends.
#[test]
fn keep_refuses_a_device_left_out_for_its_removed_issuer() {
    let conn = removal_chain();
    for dev in [B, C, D] {
        assert!(!trust::keep(&conn, dev.id()).unwrap(), "{:?}", dev.id());
    }
    assert_eq!(included(&conn), vec![ME.id()]);
}

#[test]
fn a_chain_through_a_device_that_left_still_drops() {
    let conn = removal_chain();
    trust::mark_left(&conn, &B.left_at(NOW + 25)).unwrap();
    assert!(reasons(&conn).contains(&(C.id(), ExclusionReason::IssuerRemoved)));
    assert!(reasons(&conn).contains(&(B.id(), ExclusionReason::Left)));
}

#[test]
fn a_replaced_kits_joiners_and_what_they_paired_drop_unless_named() {
    let conn = supported();
    let kit = the_kit();
    kit_row(&conn, &kit, &ME.creates_kit(&kit, NOW - 100));
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(included(&conn), vec![ME.id(), B.id(), C.id()]);
    // The Kit is replaced (its removal applied); the new-Kit bundle did not name B.
    trust::mark_removed(&conn, &kit_removal(ME, &kit, NOW + 10), Some(1)).unwrap();
    assert_eq!(
        reasons(&conn),
        vec![
            (B.id(), ExclusionReason::IssuerRemoved),
            (C.id(), ExclusionReason::IssuerRemoved),
        ]
    );
    assert_eq!(trust::recipients(&conn, &[]).unwrap().kit, None);
}

#[test]
fn this_device_is_never_left_out_for_its_issuer() {
    // ME was paired by B, which is removed later; ME is not confirmed.
    let conn = profile();
    trust::seed_own(&conn, &ME.paired_by(B, 0x70), NOW).unwrap();
    // B arrives (vouched for by nobody here: trusted on first use).
    row(&conn, B, Some(&B.genesis()));
    trust::tofu_from_rows(&conn, NOW, &genesis(&[B])).unwrap();
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    conn.execute(
        "UPDATE sync_trust SET confirmed = 0, state = 'removed', state_at = ?1 WHERE device = ?2",
        params![NOW, B.id().to_hex()],
    )
    .unwrap();
    assert_eq!(included(&conn), vec![ME.id(), C.id()], "ME stops the chain");
}

// ── LateVouch (revision §3.1, the R2 mitigation) ────────────────────────────────────────────

const O: Dev = Dev(0x0b);
const N: Dev = Dev(0x0e);

fn ticket_pub(byte: u8) -> [u8; 32] {
    sign_pub(&SignSeed::from_bytes(Zeroizing::new([byte; 32])))
}

/// O (paired by ME) paired N with ticket 0x7e and left at `NOW + 50`; the caller sets where
/// things were seen.
fn left_holder() -> Connection {
    let conn = supported();
    row(&conn, O, Some(&O.paired_by(ME, 0x70)));
    row(&conn, N, Some(&N.paired_by_at(O, 0x7e, NOW + 10, NOW + 10)));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &O.left_at(NOW + 50)).unwrap();
    clock(&conn, N, NOW + 60);
    conn
}

fn seen(conn: &Connection, dev: Dev, what: trust::SeenWhat, pos: u64) {
    trust::note_seen(conn, dev.id(), what, pos).unwrap();
}

#[test]
fn a_pairing_seen_after_its_issuer_left_without_an_announcement_is_a_late_vouch() {
    let conn = left_holder();
    seen(&conn, O, trust::SeenWhat::Left, 100);
    seen(&conn, N, trust::SeenWhat::Cert, 150);
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    // The person keeps it ("Je moje"): included.
    assert!(trust::keep(&conn, N.id()).unwrap());
    assert!(included(&conn).contains(&N.id()));
    assert!(!trust::keep(&conn, N.id()).unwrap(), "already kept");
}

#[test]
fn an_announcement_before_the_left_record_makes_it_in_time_whatever_order_the_row_comes() {
    for cert_at in [80, 150] {
        let conn = left_holder();
        trust::note_ticket(&conn, &ticket_pub(0x7e), O.id(), 40).unwrap();
        seen(&conn, O, trust::SeenWhat::Left, 100);
        seen(&conn, N, trust::SeenWhat::Cert, cert_at);
        assert!(included(&conn).contains(&N.id()), "cert seen at {cert_at}");
    }
    // An announcement after the left record, or by another device, does not help.
    for (by, pos) in [(O, 120), (ME, 40)] {
        let conn = left_holder();
        trust::note_ticket(&conn, &ticket_pub(0x7e), by.id(), pos).unwrap();
        seen(&conn, O, trust::SeenWhat::Left, 100);
        seen(&conn, N, trust::SeenWhat::Cert, 150);
        assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    }
}

#[test]
fn equal_positions_are_late_and_unknown_ones_in_time() {
    // One restored snapshot holds both (a joiner, or a device catching up from a snapshot): the
    // order cannot be told, so it is late (review of Tasks 13–15, finding 2, R-23).
    let conn = left_holder();
    seen(&conn, O, trust::SeenWhat::Left, 100);
    seen(&conn, N, trust::SeenWhat::Cert, 100);
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    // Restored with the issuer's `left` record seen later (it left after the snapshot): in time.
    let conn = left_holder();
    seen(&conn, N, trust::SeenWhat::Cert, 100);
    seen(&conn, O, trust::SeenWhat::Left, 101);
    assert!(included(&conn).contains(&N.id()));
    // The cert came by a vetted path (no position), or the left record did.
    let conn = left_holder();
    seen(&conn, O, trust::SeenWhat::Left, 100);
    assert!(included(&conn).contains(&N.id()));
    let conn = left_holder();
    seen(&conn, N, trust::SeenWhat::Cert, 150);
    assert!(included(&conn).contains(&N.id()));
    // The first position wins.
    seen(&conn, N, trust::SeenWhat::Cert, 10);
    seen(&conn, O, trust::SeenWhat::Left, 100);
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
}

#[test]
fn a_late_vouch_cannot_launder_what_it_pairs() {
    let conn = left_holder();
    seen(&conn, O, trust::SeenWhat::Left, 100);
    seen(&conn, N, trust::SeenWhat::Cert, 150);
    let m = Dev(0x0f);
    row(&conn, m, Some(&m.paired_by(N, 0x7f)));
    trust::refresh(&conn, NOW).unwrap();
    seen(&conn, m, trust::SeenWhat::Cert, 160);
    let reasons = reasons(&conn);
    assert!(reasons.contains(&(N.id(), ExclusionReason::LateVouch)));
    assert!(reasons.contains(&(m.id(), ExclusionReason::LateVouch)));
}

#[test]
fn a_superseded_issuers_vouch_is_judged_against_its_successors_position() {
    let conn = supported();
    row(&conn, O, Some(&O.paired_by(ME, 0x70)));
    // O re-joined as C at NOW + 100; a copy of O paired N before that (dated NOW + 10).
    row(&conn, C, Some(&C.rejoin_of(O, NOW + 100)));
    row(&conn, N, Some(&N.paired_by_at(O, 0x7e, NOW + 10, NOW + 10)));
    trust::refresh(&conn, NOW).unwrap();
    clock(&conn, N, NOW + 200);
    clock(&conn, O, NOW + 200);
    seen(&conn, C, trust::SeenWhat::Cert, 100);
    seen(&conn, N, trust::SeenWhat::Cert, 150);
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    // The successor itself is in time (its own position is the end).
    assert!(included(&conn).contains(&C.id()));
    let conn2 = supported();
    row(&conn2, O, Some(&O.paired_by(ME, 0x70)));
    row(&conn2, C, Some(&C.rejoin_of(O, NOW + 100)));
    row(
        &conn2,
        N,
        Some(&N.paired_by_at(O, 0x7e, NOW + 10, NOW + 10)),
    );
    trust::refresh(&conn2, NOW).unwrap();
    seen(&conn2, C, trust::SeenWhat::Cert, 100);
    seen(&conn2, N, trust::SeenWhat::Cert, 90);
    assert!(included(&conn2).contains(&N.id()));
}

#[test]
fn keep_has_nothing_to_keep_for_this_device_an_unknown_one_or_one_that_is_gone() {
    let conn = left_holder();
    assert!(!trust::keep(&conn, ME.id()).unwrap());
    assert!(!trust::keep(&conn, Dev(0x33).id()).unwrap());
    assert!(!trust::keep(&conn, O.id()).unwrap(), "it left");
}

// ── Bundles, the Kit join's confirmed set, notices ─────────────────────────────────────────

#[test]
fn a_bundle_confirms_its_recipients_and_its_kit_and_applies_the_replaced_kits_removal() {
    let conn = supported();
    let first = the_kit();
    kit_row(&conn, &first, &ME.creates_kit(&first, NOW));
    with_b(&conn);
    trust::refresh(&conn, NOW).unwrap();
    let second = second_kit();
    let new_kit = B.creates_kit(&second, NOW + 5);
    let replaces = kit_removal(B, &first, NOW + 5);
    trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[ME.genesis(), C.paired_by(B, 0x71)],
        Some(trust::BundleKit {
            cert: &new_kit,
            replaces: Some(&replaces),
        }),
        &[],
        2,
        NOW,
    )
    .expect("import");
    for dev in [B.id(), ME.id(), C.id(), second.id()] {
        assert!(
            trust::entry(&conn, dev).unwrap().unwrap().confirmed,
            "{dev:?}"
        );
    }
    let old = trust::entry(&conn, first.id()).unwrap().unwrap();
    assert_eq!(old.state, TrustState::Removed);
    assert_eq!(old.cut_off_epoch, Some(2));
    assert_eq!(
        trust::recipients(&conn, &[]).unwrap().kit.map(|k| k.device),
        Some(second.id())
    );

    // A second Kit without the first's removal fails the whole import.
    let conn = supported();
    kit_row(&conn, &first, &ME.creates_kit(&first, NOW));
    with_b(&conn);
    let before = trust::entries(&conn).unwrap();
    assert!(trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[C.paired_by(B, 0x71)],
        Some(trust::BundleKit {
            cert: &new_kit,
            replaces: None
        }),
        &[],
        2,
        NOW,
    )
    .is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before);
    // A Kit that is no Kit certificate is refused.
    assert!(trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[],
        Some(trust::BundleKit {
            cert: &C.paired_by(B, 0x71),
            replaces: None
        }),
        &[],
        2,
        NOW,
    )
    .is_err());
}

#[test]
fn the_confirmed_set_marks_only_matching_certificates() {
    let conn = supported();
    with_b(&conn);
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    let matched = trust::apply_confirmed_set(
        &conn,
        &[
            (B.id(), B.paired_by(ME, 0x70).hash()),
            (C.id(), [0x55; 32]),
            (D.id(), D.genesis().hash()),
        ],
    )
    .unwrap();
    assert_eq!(matched, 1);
    assert!(trust::entry(&conn, B.id()).unwrap().unwrap().confirmed);
    assert!(!trust::entry(&conn, C.id()).unwrap().unwrap().confirmed);
    assert_eq!(trust::entry(&conn, D.id()).unwrap(), None);
}

#[test]
fn new_device_notices_are_written_for_pairings_and_kit_joins_only() {
    let conn = supported();
    // A device this one added says nothing here: the person added it here (live run, Task B2).
    with_b(&conn);
    assert!(trust::new_devices(&conn).unwrap().is_empty());
    // A re-join says nothing; a genesis is never pinned.
    row(&conn, C, Some(&C.rejoin_of(B, NOW + 1)));
    row(&conn, D, Some(&D.genesis()));
    let e = Dev(0x0e);
    row(&conn, e, Some(&e.paired_by(B, 0x7e)));
    trust::refresh(&conn, NOW + 7).unwrap();
    let notices = trust::new_devices(&conn).unwrap();
    assert_eq!(
        notices,
        vec![trust::NewDevice {
            device: e.id().to_hex(),
            by: B.id().to_hex(),
            at: NOW + 7,
            via_kit: false,
        }]
    );
    // At most 20, the newest kept.
    for i in 0..25u8 {
        let dev = Dev(0x40 + i);
        row(&conn, dev, Some(&dev.paired_by(B, 0x90 + i)));
        trust::refresh(&conn, NOW + i64::from(i)).unwrap();
    }
    let notices = trust::new_devices(&conn).unwrap();
    assert_eq!(notices.len(), trust::MAX_NEW_DEVICES);
    assert_eq!(notices.last().unwrap().device, Dev(0x40 + 24).id().to_hex());
    // A Kit joiner's first store and an import write none.
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    trust::tofu_from_rows(&conn, NOW, &genesis(&[B])).unwrap();
    assert!(trust::new_devices(&conn).unwrap().is_empty());
}

// ── Review of Tasks 13–15: who may speak (finding 1) ───────────────────────────────────────

/// `left_holder()` with N a late vouch (O's `left` record seen at 100, N's certificate at 150)
/// and D paired by this device.
fn late_n() -> Connection {
    let conn = left_holder();
    seen(&conn, O, trust::SeenWhat::Left, 100);
    seen(&conn, N, trust::SeenWhat::Cert, 150);
    row(&conn, D, Some(&D.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    conn
}

fn n_cert() -> Certificate {
    N.paired_by_at(O, 0x7e, NOW + 10, NOW + 10)
}

#[test]
fn a_late_vouch_signs_no_key_change_and_no_removal_until_it_is_kept() {
    let conn = late_n();
    let before = trust::entries(&conn).unwrap();
    // Its key change naming every device is refused, and does not confirm it.
    assert!(trust::import_bundle(
        &conn,
        &n_cert(),
        &[ME.genesis(), D.paired_by(ME, 0x71)],
        None,
        &[],
        1,
        NOW
    )
    .is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before, "nothing written");
    // Its removal of D counts nowhere: not in a row, not in a bundle another device signed.
    let removal = N.removes_at(D, NOW + 70);
    assert!(!trust::record_counts(&conn, &removal).unwrap());
    assert!(trust::mark_removed(&conn, &removal, None).is_err());
    assert!(trust::import_bundle(
        &conn,
        &ME.genesis(),
        &[],
        None,
        std::slice::from_ref(&removal),
        1,
        NOW
    )
    .is_err());
    set_record(&conn, D, "removal_record", &removal);
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, D), Some(TrustState::Trusted));

    // Kept by the person ("Je moje"): it speaks.
    assert!(trust::keep(&conn, N.id()).unwrap());
    assert!(trust::record_counts(&conn, &removal).unwrap());
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, D), Some(TrustState::Removed));
    trust::import_bundle(&conn, &n_cert(), &[ME.genesis()], None, &[], 1, NOW)
        .expect("a kept device's key change");
}

#[test]
fn a_device_left_out_because_its_issuer_was_removed_signs_nothing() {
    let conn = removal_chain();
    let f = Dev(0x06);
    row(&conn, f, Some(&f.paired_by(ME, 0x75)));
    trust::refresh(&conn, NOW).unwrap();
    let before = trust::entries(&conn).unwrap();
    // C (paired by B, paired by the removed A) makes a key change: refused.
    assert!(trust::import_bundle(
        &conn,
        &C.paired_by(B, 0x72),
        &[ME.genesis(), f.paired_by(ME, 0x75)],
        None,
        &[],
        2,
        NOW
    )
    .is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before, "nothing written");
    // B removes f: it does not count.
    let removal = B.removes_at(f, NOW + 40);
    assert!(!trust::record_counts(&conn, &removal).unwrap());
    set_record(&conn, f, "removal_record", &removal);
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, f), Some(TrustState::Trusted));
    // Named by a key change (confirmed), B speaks again.
    trust::mark_confirmed(&conn, &[B.id()]).unwrap();
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, f), Some(TrustState::Removed));
}

// ── Review of Tasks 13–15: restored positions, a pairing joiner, ghost issuers (finding 2) ─

#[test]
fn a_kit_joiner_restoring_a_left_issuer_and_its_late_vouch_together_leaves_it_out() {
    // The restored rows: O (genesis) left; N paired by O. Both seen at the snapshot's position.
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, O, Some(&O.genesis()));
    row(&conn, N, Some(&n_cert()));
    set_record(&conn, O, "left_record", &O.left_at(NOW + 50));
    trust::tofu_from_rows(&conn, NOW, &genesis(&[O])).unwrap();
    assert_eq!(state_of(&conn, O), Some(TrustState::Left));
    seen(&conn, O, trust::SeenWhat::Left, 300);
    seen(&conn, N, trust::SeenWhat::Cert, 300);
    assert!(reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)));
    // The envelope's confirmed set names it: included.
    trust::apply_confirmed_set(&conn, &[(N.id(), n_cert().hash())]).unwrap();
    assert!(included(&conn).contains(&N.id()));
}

#[test]
fn a_pairing_joiner_judges_late_vouches_as_its_holder_does() {
    // On the holder: N is late (never announced), P in time (announced before O left).
    let holder = left_holder();
    let p = Dev(0x0d);
    row(
        &holder,
        p,
        Some(&p.paired_by_at(O, 0x7d, NOW + 20, NOW + 20)),
    );
    trust::refresh(&holder, NOW).unwrap();
    clock(&holder, p, NOW + 60);
    trust::note_ticket(&holder, &ticket_pub(0x7d), O.id(), 40).unwrap();
    seen(&holder, O, trust::SeenWhat::Left, 100);
    seen(&holder, N, trust::SeenWhat::Cert, 150);
    seen(&holder, p, trust::SeenWhat::Cert, 160);
    assert!(reasons(&holder).contains(&(N.id(), ExclusionReason::LateVouch)));
    assert!(included(&holder).contains(&p.id()));
    let json = serde_json::to_string(&trust::export(&holder).unwrap()).unwrap();
    let export: TrustExport = serde_json::from_str(&json).unwrap();

    // The joiner restored a snapshot holding all of it (seen at its position), or nothing.
    for restored in [true, false] {
        let conn = joiner();
        if restored {
            seen(&conn, O, trust::SeenWhat::Left, 500);
            seen(&conn, N, trust::SeenWhat::Cert, 500);
            seen(&conn, p, trust::SeenWhat::Cert, 500);
        }
        trust::import_export(&conn, &export, ME.id(), NOW).expect("import");
        assert!(
            reasons(&conn).contains(&(N.id(), ExclusionReason::LateVouch)),
            "restored: {restored}"
        );
        assert!(included(&conn).contains(&p.id()), "restored: {restored}");
    }
}

#[test]
fn a_device_whose_issuer_is_not_pinned_gets_nothing_unless_confirmed() {
    // A Kit joiner trusts D on first use: its issuer E has no row.
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, D, Some(&D.paired_by(E, 0x72)));
    row(&conn, C, Some(&C.paired_by(D, 0x73)));
    trust::tofu_from_rows(&conn, NOW, &genesis(&[B])).unwrap();
    assert_eq!(state_of(&conn, D), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));
    assert_eq!(included(&conn), vec![ME.id(), B.id()]);
    // Confirmed by the envelope's set: D, and what it paired.
    trust::apply_confirmed_set(&conn, &[(D.id(), D.paired_by(E, 0x72).hash())]).unwrap();
    assert_eq!(included(&conn), vec![ME.id(), B.id(), C.id(), D.id()]);
}

// ── Review of Tasks 13–15: a backdated re-join (finding 3) ──────────────────────────────────

#[test]
fn a_backdated_rejoin_of_a_live_device_does_not_void_what_it_signed_before_it_was_seen() {
    let conn = supported();
    with_b(&conn);
    row(&conn, D, Some(&D.paired_by(ME, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    // A copy of B posts a re-join E dated long before; this device pins it at NOW + 100.
    row(&conn, E, Some(&E.rejoin_of(B, NOW - 10_000)));
    trust::refresh(&conn, NOW + 100).unwrap();
    assert_eq!(state_of(&conn, E), Some(TrustState::Trusted));
    // What B signed before this device saw the re-join still counts.
    let removal = B.removes_at(D, NOW + 50);
    assert!(trust::record_counts(&conn, &removal).unwrap());
    let f = Dev(0x06);
    row(&conn, f, Some(&f.paired_by_at(B, 0x75, NOW + 60, NOW + 60)));
    trust::refresh(&conn, NOW + 110).unwrap();
    assert_eq!(state_of(&conn, f), Some(TrustState::Trusted));
    // A second, earlier-dated re-join pinned later does not move the bound.
    let g = Dev(0x07);
    row(&conn, g, Some(&g.rejoin_of(B, NOW - 20_000)));
    trust::refresh(&conn, NOW + 200).unwrap();
    assert_eq!(state_of(&conn, g), Some(TrustState::Trusted));
    assert!(trust::record_counts(&conn, &removal).unwrap());
    // A key change carrying B's earlier removal is taken.
    trust::import_bundle(
        &conn,
        &B.paired_by(ME, 0x70),
        &[ME.genesis()],
        None,
        &[removal],
        1,
        NOW,
    )
    .expect("B's removal signed before the re-join was seen");
    assert_eq!(state_of(&conn, D), Some(TrustState::Removed));
    // What B signs after this device saw its re-join does not.
    assert!(!trust::record_counts(&conn, &B.removes_at(f, NOW + 150)).unwrap());
    // B wrote after the re-join appeared here: no quiet copy whatever the re-join's own date.
    clock(&conn, B, NOW + 120);
    assert!(included(&conn).contains(&B.id()));
}

// ── Review of Tasks 13–15: choosing the Kit (finding 4) ─────────────────────────────────────

/// Two Kits other than `the_kit()`: (rogue, genuine), the rogue's ID the lower one.
fn rogue_and_genuine_kits() -> (KitKeys, KitKeys) {
    let a = second_kit();
    let b = KitSecret::from_bytes(Zeroizing::new([0x0e; 32])).derive(&VAULT);
    if a.id() < b.id() {
        (a, b)
    } else {
        (b, a)
    }
}

#[test]
fn a_replacement_kit_is_pinned_only_through_the_key_change_that_names_it() {
    for genuine_row in [true, false] {
        let conn = kit_supported();
        with_b(&conn);
        row(&conn, C, Some(&C.paired_by(ME, 0x71)));
        trust::refresh(&conn, NOW).unwrap();
        let first = the_kit();
        let (rogue, genuine) = rogue_and_genuine_kits();
        // A rogue member (C) posts its own Kit; B replaces the Kit with a key change.
        kit_row(&conn, &rogue, &C.creates_kit(&rogue, NOW + 1));
        let genuine_cert = B.creates_kit(&genuine, NOW + 5);
        if genuine_row {
            kit_row(&conn, &genuine, &genuine_cert);
        }
        let replaces = kit_removal(B, &first, NOW + 5);
        // The old Kit's removal reaches the rows first.
        conn.execute(
            "UPDATE sync_devices SET removal_record = ?1 WHERE id = ?2",
            params![replaces.to_b64(), first.id().to_hex()],
        )
        .unwrap();
        trust::refresh(&conn, NOW).unwrap();
        assert_eq!(
            trust::entry(&conn, first.id()).unwrap().map(|e| e.state),
            Some(TrustState::Removed)
        );
        assert_eq!(trust::entry(&conn, rogue.id()).unwrap(), None);
        assert_eq!(trust::entry(&conn, genuine.id()).unwrap(), None);
        // Then the bundle that names the new Kit.
        trust::import_bundle(
            &conn,
            &B.paired_by(ME, 0x70),
            &[ME.genesis(), C.paired_by(ME, 0x71)],
            Some(trust::BundleKit {
                cert: &genuine_cert,
                replaces: Some(&replaces),
            }),
            &[],
            2,
            NOW,
        )
        .expect("import");
        assert_eq!(
            trust::recipients(&conn, &[]).unwrap().kit.map(|k| k.device),
            Some(genuine.id()),
            "genuine row: {genuine_row}"
        );
        trust::refresh(&conn, NOW).unwrap();
        assert_eq!(trust::entry(&conn, rogue.id()).unwrap(), None);
    }
}

#[test]
fn a_first_kit_is_never_chosen_among_several_or_from_a_creator_that_may_not_speak() {
    // Two first Kits in one pass: neither.
    let conn = supported();
    with_b(&conn);
    let (one, two) = rogue_and_genuine_kits();
    kit_row(&conn, &one, &ME.creates_kit(&one, NOW));
    kit_row(&conn, &two, &B.creates_kit(&two, NOW));
    let report = trust::refresh(&conn, NOW).unwrap();
    assert_eq!(trust::entry(&conn, one.id()).unwrap(), None);
    assert_eq!(trust::entry(&conn, two.id()).unwrap(), None);
    assert_eq!(
        report
            .conflicts
            .iter()
            .filter(|c| c.kind == ConflictKind::SecondKit)
            .count(),
        2
    );
    // A Kit made by a late vouch: not pinned.
    let conn = late_n();
    let kit = the_kit();
    kit_row(&conn, &kit, &N.creates_kit(&kit, NOW + 70));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(trust::entry(&conn, kit.id()).unwrap(), None);
}

#[test]
fn the_kit_of_a_creator_left_out_for_its_issuer_is_not_sealed_to() {
    let a = Dev(0x0a);
    let conn = supported();
    row(&conn, a, Some(&a.paired_by(ME, 0x70)));
    row(&conn, B, Some(&B.paired_by(a, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    let kit = the_kit();
    kit_row(&conn, &kit, &B.creates_kit(&kit, NOW + 5));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(
        trust::recipients(&conn, &[]).unwrap().kit.map(|k| k.device),
        Some(kit.id())
    );
    trust::mark_removed(&conn, &ME.removes_at(a, NOW + 20), Some(1)).unwrap();
    assert_eq!(trust::recipients(&conn, &[]).unwrap().kit, None);
    // Named by a key change, it is again.
    trust::mark_confirmed(&conn, &[kit.id()]).unwrap();
    assert_eq!(
        trust::recipients(&conn, &[]).unwrap().kit.map(|k| k.device),
        Some(kit.id())
    );
}

// ── Review of Tasks 13–15: minors ───────────────────────────────────────────────────────────

#[test]
fn a_kit_labelled_row_without_a_kit_certificate_is_listed_as_unverified() {
    let conn = supported();
    // A row that calls itself a Kit, with no certificate, or a device's certificate.
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
         VALUES (?1, 'PK', '', 'recoveryKit', '1.0', 1)",
        [B.id().to_hex()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, 'PK', '', 'recoveryKit', '1.0', 1, ?2)",
        params![C.id().to_hex(), C.genesis().to_b64()],
    )
    .unwrap();
    // A real Kit's row whose Kit is not pinned (its creator is unknown): not listed.
    let kit = the_kit();
    kit_row(&conn, &kit, &D.creates_kit(&kit, NOW));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(
        reasons(&conn),
        vec![
            (B.id(), ExclusionReason::Unverified),
            (C.id(), ExclusionReason::Unverified),
        ]
    );
}

#[test]
fn only_a_value_the_row_holds_and_that_verifies_is_a_sighting() {
    let conn = supported();
    with_b(&conn);
    let held = B.paired_by(ME, 0x70);
    // Junk, or a valid certificate the row does not hold: nothing.
    trust::note_sighting(&conn, B.id(), trust::SeenWhat::Cert, "junk", 10).unwrap();
    trust::note_sighting(
        &conn,
        B.id(),
        trust::SeenWhat::Cert,
        &B.genesis().to_b64(),
        11,
    )
    .unwrap();
    trust::note_sighting(&conn, B.id(), trust::SeenWhat::Left, "junk", 12).unwrap();
    let forged = Record::sign(RecordType::Left, VAULT, B.id(), B.id(), NOW, 0, &C.seed()).unwrap();
    set_record(&conn, B, "left_record", &forged);
    trust::note_sighting(&conn, B.id(), trust::SeenWhat::Left, &forged.to_b64(), 13).unwrap();
    let count = |conn: &Connection| -> i64 {
        conn.query_row("SELECT COUNT(*) FROM sync_trust_seen", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count(&conn), 0);
    // The held certificate and a left record its subject signed: noted.
    trust::note_sighting(&conn, B.id(), trust::SeenWhat::Cert, &held.to_b64(), 20).unwrap();
    set_record(&conn, B, "left_record", &B.left());
    trust::note_sighting(&conn, B.id(), trust::SeenWhat::Left, &B.left().to_b64(), 21).unwrap();
    assert_eq!(count(&conn), 2);
}
