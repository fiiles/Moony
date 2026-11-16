//! The pinned trust store (phase 5 spec §5.4, §5.5) over an in-memory migrated profile, with
//! certificates and records from the issuers of `cert.rs` and `record.rs`.

use rusqlite::{params, Connection};
use zeroize::Zeroizing;

use super::cert::{self, CertFields, CertKind, Certificate, TicketSecret};
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
    fn kit(self) -> Certificate {
        cert::issue_self(CertKind::RecoveryKit, &self.fields(NOW), &self.seed()).expect("kit")
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
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).expect("refresh");
    let entry = trust::entry(&conn, B.id()).unwrap().expect("pinned");
    assert_eq!(entry.kind, CertKind::RecoveryKit);
    assert_eq!(entry.issuer, B.id());
}

#[test]
fn a_rejoin_certificate_is_pinned_when_its_predecessor_is_trusted() {
    let conn = supported();
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
    let conn = supported();
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
    let other_vault = cert::issue_self(
        CertKind::RecoveryKit,
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
    let conn = supported();
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
    let conn = supported();
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
    trust::import_bundle(&conn, &ME.genesis(), &[B.paired_by(ME, 0x70)], &[], 1, NOW).unwrap();
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
    let conn = supported();
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
    let conn = supported();
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
    let conn = supported();
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
    let conn = supported();
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
                e.kit_unconfirmed,
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
    // This device knows B only; the signer C and the recipient D come in by the bundle, which
    // also removes B.
    let conn = supported();
    with_b(&conn);
    let signer = C.paired_by(B, 0x71);
    let d = D.paired_by(E, 0x72); // E is unknown here: endorsed by the signer anyway.

    trust::import_bundle(
        &conn,
        &signer,
        &[ME.genesis(), d.clone()],
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
}

#[test]
fn a_bundle_never_replaces_a_pinned_certificate_and_writes_nothing_when_it_fails() {
    let conn = supported();
    with_b(&conn);
    let before = trust::entries(&conn).unwrap();

    // B's certificate in the bundle differs from the pinned one: the pin stays.
    trust::import_bundle(&conn, &C.kit(), &[B.kit()], &[], 1, NOW).unwrap();
    assert_eq!(
        trust::entry(&conn, B.id()).unwrap().unwrap().cert,
        B.paired_by(ME, 0x70)
    );

    // A recipient that does not verify fails the whole import.
    let conn = supported();
    with_b(&conn);
    let mut broken = D.kit();
    broken.subject_sig[0] ^= 1;
    assert!(trust::import_bundle(&conn, &C.kit(), &[broken], &[C.removes(B)], 1, NOW).is_err());
    assert_eq!(trust::entries(&conn).unwrap(), before);
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

    trust::tofu_from_rows(&conn, NOW).expect("tofu");

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
    let conn = supported();
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

    let plan = trust::recipients(&conn, &[C.id()], &[]).expect("recipients");

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
    // C re-joined as D at NOW + 100.
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.rejoin_of(C, NOW + 100)));
    trust::refresh(&conn, NOW).unwrap();
    clock(&conn, C, NOW + 50);

    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    assert!(plan
        .excluded
        .contains(&(C.id(), ExclusionReason::SupersededQuiet)));
    assert!(plan.included.iter().any(|e| e.device == D.id()));

    // C wrote a change after its successor appeared: a live copy, it stays a recipient (O11).
    clock(&conn, C, NOW + 200);
    let plan = trust::recipients(&conn, &[], &[]).unwrap();
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
    let error = trust::recipients(&conn, &[], &[]).expect_err("65 recipients");
    assert_eq!(
        error.to_string(),
        "Validation error: validation.syncTooManyDevices"
    );
    // Removing one brings it to 64.
    let plan = trust::recipients(&conn, &[Dev(0x10).id()], &[]).expect("64");
    assert_eq!(plan.included.len(), 64);
}

// ── Recovery Kit certificates (fix round) ───────────────────────────────────────────────────

fn kit_flag(conn: &Connection, dev: Dev) -> Option<bool> {
    trust::entry(conn, dev.id())
        .unwrap()
        .map(|e| e.kit_unconfirmed)
}

#[test]
fn a_kit_device_is_pinned_unconfirmed_and_gets_the_key_only_when_ticked() {
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), Some(TrustState::Trusted));
    assert_eq!(kit_flag(&conn, B), Some(true));
    assert_eq!(kit_flag(&conn, ME), Some(false), "this device's own");

    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id()]);
    assert_eq!(
        plan.excluded,
        vec![(B.id(), ExclusionReason::KitUnconfirmed)]
    );

    let plan = trust::recipients(&conn, &[], &[B.id()]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id(), B.id()]);
    assert!(plan.excluded.is_empty());
}

#[test]
fn this_devices_own_kit_certificate_is_always_its_own() {
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    assert_eq!(kit_flag(&conn, ME), Some(false));
    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    assert_eq!(plan.included.len(), 1);
}

#[test]
fn unticked_kit_devices_do_not_count_toward_the_cap_and_ticked_ones_do() {
    let conn = supported();
    for i in 0..64u8 {
        let dev = Dev(0x10 + i);
        row(&conn, dev, Some(&dev.kit()));
    }
    trust::refresh(&conn, NOW).unwrap();

    let plan = trust::recipients(&conn, &[], &[]).expect("only this device");
    assert_eq!(plan.included.len(), 1);
    assert_eq!(plan.excluded.len(), 64);

    let all: Vec<DeviceId> = (0..64u8).map(|i| Dev(0x10 + i).id()).collect();
    assert!(
        trust::recipients(&conn, &[], &all).is_err(),
        "65 recipients"
    );
    let plan = trust::recipients(&conn, &[], &all[1..]).expect("64");
    assert_eq!(plan.included.len(), 64);
}

#[test]
fn an_unconfirmed_device_removes_nobody_and_what_it_pairs_is_unconfirmed_too() {
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    // D paired by the Kit device B; E re-joined from it.
    row(&conn, D, Some(&D.paired_by(B, 0x72)));
    row(&conn, E, Some(&E.rejoin_of(B, NOW)));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, C, "removal_record", &B.removes(C));
    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(
        state_of(&conn, C),
        Some(TrustState::Trusted),
        "B is not confirmed"
    );
    assert_eq!(kit_flag(&conn, D), Some(true));
    assert_eq!(kit_flag(&conn, E), Some(true));
    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id(), C.id()]);
}

#[test]
fn a_confirmed_kit_device_confirms_what_it_paired_and_its_removals_count() {
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.paired_by(B, 0x72)));
    trust::refresh(&conn, NOW).unwrap();
    set_record(&conn, C, "removal_record", &B.removes(C));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, C), Some(TrustState::Trusted));

    // The person ticked B in a key change that committed.
    conn.execute(
        "UPDATE sync_state SET device_sign_key = ?1",
        [ME.seed().expose().to_vec()],
    )
    .unwrap();
    assert_eq!(trust::confirm(&conn, &[B.id()], NOW).unwrap().len(), 1);
    let report = trust::refresh(&conn, NOW).unwrap();

    assert_eq!(kit_flag(&conn, B), Some(false));
    assert_eq!(kit_flag(&conn, D), Some(false), "promoted with its issuer");
    assert_eq!(state_of(&conn, C), Some(TrustState::Removed));
    assert_eq!(report.moved, vec![(C.id(), TrustState::Removed)]);
}

#[test]
fn a_bundle_confirms_the_kit_devices_it_names() {
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.kit()));
    trust::refresh(&conn, NOW).unwrap();

    // Signed by B (as the caller checked) and naming C a recipient; a bundle's new certificates
    // are confirmed too.
    trust::import_bundle(&conn, &B.kit(), &[C.kit(), D.kit()], &[], 1, NOW).unwrap();

    assert_eq!(kit_flag(&conn, B), Some(false));
    assert_eq!(kit_flag(&conn, C), Some(false));
    assert_eq!(kit_flag(&conn, D), Some(false));
}

#[test]
fn an_export_carries_the_unconfirmed_flag_and_refuses_an_unconfirmed_holder() {
    let holder = supported();
    row(&holder, B, Some(&B.kit()));
    trust::refresh(&holder, NOW).unwrap();
    let export = trust::export(&holder).unwrap();
    let json = serde_json::to_string(&export).unwrap();
    assert!(json.contains("\"kitUnconfirmed\":true"), "{json}");
    assert_eq!(
        json.matches("kitUnconfirmed").count(),
        1,
        "false is not written: {json}"
    );

    let conn = joiner();
    trust::import_export(&conn, &export, ME.id(), NOW).unwrap();
    assert_eq!(kit_flag(&conn, B), Some(true));
    assert_eq!(kit_flag(&conn, ME), Some(false));

    // B claims to be the holder: it is not confirmed in its own export.
    let conn = joiner();
    assert!(trust::import_export(&conn, &export, B.id(), NOW).is_err());
}

#[test]
fn a_kit_joiner_pins_other_kit_devices_unconfirmed() {
    let conn = profile();
    trust::seed_own(&conn, &ME.kit(), NOW).unwrap();
    row(&conn, B, Some(&B.genesis()));
    row(&conn, C, Some(&C.kit()));
    row(&conn, D, Some(&D.paired_by(C, 0x72)));

    trust::tofu_from_rows(&conn, NOW).unwrap();

    assert_eq!(kit_flag(&conn, ME), Some(false));
    assert_eq!(kit_flag(&conn, B), Some(false));
    assert_eq!(kit_flag(&conn, C), Some(true));
    assert_eq!(
        kit_flag(&conn, D),
        Some(true),
        "paired by an unconfirmed device"
    );
}

#[test]
fn a_kit_certificate_for_a_device_another_certificate_names_is_refused() {
    // C was paired by B (B's real key); B's own certificate has not arrived. A Kit certificate
    // for B's ID with another key would squat it.
    let squatter =
        cert::issue_self(CertKind::RecoveryKit, &B.fields(NOW), &Dev(0x0e).seed()).expect("kit");
    let conn = supported();
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    row(&conn, B, Some(&squatter));

    let report = trust::refresh(&conn, NOW).unwrap();

    assert_eq!(state_of(&conn, B), None);
    assert_eq!(state_of(&conn, C), None);
    assert!(report.conflicts.contains(&trust::TrustConflict {
        device: B.id(),
        kind: ConflictKind::KitClaimsKnownDevice,
    }));

    // A re-join certificate names its older ID the same way.
    let conn = supported();
    row(&conn, C, Some(&C.rejoin_of(B, NOW)));
    row(&conn, B, Some(&squatter));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(state_of(&conn, B), None);

    // B's real Kit certificate (the key C's ticket names) is pinned, and C with it.
    let conn = supported();
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(kit_flag(&conn, B), Some(true));
    assert_eq!(kit_flag(&conn, C), Some(true));
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
        &[B.removes(ME), B.removes(D)],
        2,
        NOW,
    )
    .expect("import");

    assert_eq!(about_me, vec![B.removes(ME)]);
    assert_eq!(state_of(&conn, ME), Some(TrustState::Trusted));
    assert_eq!(state_of(&conn, D), Some(TrustState::Removed));
}

// ── Confirming a device; devices whose issuer is gone (fix round 2) ──────────────────────────

impl Dev {
    /// `self` confirms `subject`.
    fn confirms(self, subject: Dev) -> Record {
        Record::sign(
            RecordType::Confirmed,
            VAULT,
            subject.id(),
            self.id(),
            NOW,
            0,
            &self.seed(),
        )
        .expect("confirmed")
    }
}

/// `supported()` with this device's signing seed stored, so it can sign.
fn signing() -> Connection {
    let conn = supported();
    conn.execute(
        "UPDATE sync_state SET device_sign_key = ?1",
        [ME.seed().expose().to_vec()],
    )
    .unwrap();
    conn
}

fn confirmed(conn: &Connection, dev: Dev) -> Option<bool> {
    trust::entry(conn, dev.id()).unwrap().map(|e| e.confirmed)
}

fn confirm_column(conn: &Connection, dev: Dev) -> Option<String> {
    conn.query_row(
        "SELECT confirm_record FROM sync_devices WHERE id = ?1",
        [dev.id().to_hex()],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn confirming_a_kit_device_signs_publishes_and_applies_a_record() {
    let conn = signing();
    row(&conn, B, Some(&B.kit()));
    row(&conn, D, Some(&D.paired_by(B, 0x72)));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(kit_flag(&conn, B), Some(true));

    let records = trust::confirm(&conn, &[B.id()], NOW).expect("confirm");

    assert_eq!(records, vec![ME.confirms(B)]);
    assert_eq!(kit_flag(&conn, B), Some(false));
    assert_eq!(confirmed(&conn, B), Some(true));
    assert_eq!(confirm_column(&conn, B), Some(ME.confirms(B).to_b64()));
    trust::refresh(&conn, NOW).unwrap();
    assert_eq!(kit_flag(&conn, D), Some(false), "what B paired follows");
}

#[test]
fn confirm_refuses_this_device_an_unknown_one_or_one_that_is_gone() {
    let conn = signing();
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.kit()));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_removed(&conn, &ME.removes(C), None).unwrap();
    for devices in [
        vec![ME.id()],
        vec![D.id()],
        vec![C.id()],
        vec![B.id(), D.id()],
    ] {
        assert!(trust::confirm(&conn, &devices, NOW).is_err(), "{devices:?}");
    }
    assert_eq!(kit_flag(&conn, B), Some(true), "nothing written");
    assert_eq!(confirm_column(&conn, B), None);

    // A device without its signing key confirms nothing.
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    trust::refresh(&conn, NOW).unwrap();
    assert!(trust::confirm(&conn, &[B.id()], NOW).is_err());
}

#[test]
fn a_confirmation_from_another_device_counts_by_the_signer_rule() {
    let conn = supported();
    row(&conn, B, Some(&B.kit()));
    row(&conn, C, Some(&C.paired_by(ME, 0x71)));
    row(&conn, D, Some(&D.kit()));
    row(&conn, E, Some(&E.kit()));
    trust::refresh(&conn, NOW).unwrap();
    // C (trusted) confirms B; D (an unconfirmed Kit device) confirms E.
    set_record(&conn, B, "confirm_record", &C.confirms(B));
    set_record(&conn, E, "confirm_record", &D.confirms(E));

    trust::refresh(&conn, NOW).unwrap();

    assert_eq!(kit_flag(&conn, B), Some(false));
    assert_eq!(confirmed(&conn, B), Some(true));
    assert_eq!(
        kit_flag(&conn, E),
        Some(true),
        "an unconfirmed signer confirms nobody"
    );
}

#[test]
fn a_device_whose_issuer_is_gone_gets_the_key_only_when_ticked_or_confirmed() {
    let conn = signing();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    row(&conn, D, Some(&D.rejoin_of(B, NOW)));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &B.left_at(NOW + 10)).unwrap();

    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id()]);
    assert!(plan
        .excluded
        .contains(&(C.id(), ExclusionReason::IssuerGone)));
    assert!(plan
        .excluded
        .contains(&(D.id(), ExclusionReason::IssuerGone)));
    assert!(plan.excluded.contains(&(B.id(), ExclusionReason::Left)));

    let plan = trust::recipients(&conn, &[], &[C.id()]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id(), C.id()]);

    // Confirmed once, it is a recipient from then on.
    trust::confirm(&conn, &[D.id()], NOW).unwrap();
    let plan = trust::recipients(&conn, &[], &[]).unwrap();
    let included: Vec<DeviceId> = plan.included.iter().map(|e| e.device).collect();
    assert_eq!(included, vec![ME.id(), D.id()]);
}

#[test]
fn an_export_carries_the_confirmation() {
    let holder = signing();
    row(&holder, B, Some(&B.kit()));
    trust::refresh(&holder, NOW).unwrap();
    trust::confirm(&holder, &[B.id()], NOW).unwrap();
    let export = trust::export(&holder).unwrap();
    let json = serde_json::to_string(&export).unwrap();
    assert!(json.contains("\"confirmed\":true"), "{json}");

    let conn = joiner();
    trust::import_export(&conn, &export, ME.id(), NOW).unwrap();
    assert_eq!(confirmed(&conn, B), Some(true));
    assert_eq!(kit_flag(&conn, B), Some(false));
}

#[test]
fn a_bundle_confirms_what_it_names() {
    let conn = supported();
    row(&conn, B, Some(&B.paired_by(ME, 0x70)));
    row(&conn, C, Some(&C.paired_by(B, 0x71)));
    trust::refresh(&conn, NOW).unwrap();
    trust::mark_left(&conn, &B.left_at(NOW + 10)).unwrap();
    trust::import_bundle(&conn, &ME.genesis(), &[C.paired_by(B, 0x71)], &[], 1, NOW).unwrap();
    assert_eq!(confirmed(&conn, C), Some(true));
}
