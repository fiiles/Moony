//! This device's own identity (phase 5 spec §5.2, §5.3, §5.5): the seed it stores, the
//! certificate it signs at setup, re-join and Kit join, and the profiles that stay without one.

use rusqlite::Connection;
use zeroize::Zeroizing;

use super::cert::{self, CertKind};
use super::trust::{self, TrustState};
use super::{
    create_own, ensure_seed, own_seed, predecessor, sign_pub, successor, Issuer, SignSeed,
};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::keys::{self, VaultId};
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const VAULT: [u8; 16] = [0x22; 16];
const ME: DeviceId = DeviceId([0x01; 16]);
const NEXT: DeviceId = DeviceId([0x09; 16]);
const NOW: i64 = 1_794_816_000;

/// A sync-enabled profile of `ME` in the test vault with its box key, no identity yet.
fn profile() -> Connection {
    let conn = migrated();
    state::enable(&conn, ME).expect("enable");
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1",
        [VaultId(VAULT).to_b64()],
    )
    .expect("vault");
    keys::ensure_device_keypair(&conn).expect("box key");
    conn
}

fn box_pub(conn: &Connection) -> [u8; 32] {
    keys::ensure_device_keypair(conn).expect("box key")
}

/// The profile continues as `NEXT` with a new box key, as a re-join leaves it.
fn moved_on(conn: &Connection) {
    conn.execute(
        "UPDATE sync_state SET device_id = ?1 WHERE id = 1",
        [NEXT.0.to_vec()],
    )
    .expect("new ID");
    keys::new_device_keypair(conn).expect("new box key");
}

#[test]
fn a_genesis_certificate_is_stored_signed_and_pinned() {
    let conn = profile();
    let cert = create_own(&conn, Issuer::Genesis, NOW).expect("genesis");

    let seed = own_seed(&conn).expect("seed").expect("stored");
    assert_eq!(cert.body.kind, CertKind::Genesis);
    assert_eq!(cert.body.device, ME);
    assert_eq!(cert.body.vault_id, VAULT);
    assert_eq!(cert.body.sign_pub, sign_pub(&seed));
    assert_eq!(cert.body.box_pub, box_pub(&conn));
    assert_eq!(cert.body.issued_at, NOW);
    cert.verify_signatures().expect("verifies");
    let entry = trust::entry(&conn, ME).expect("entry").expect("pinned");
    assert_eq!(entry.cert, cert);
    assert_eq!(entry.state, TrustState::Trusted);
    assert!(trust::is_supported(&conn).expect("supported"));
}

#[test]
fn a_device_with_its_certificate_keeps_it_when_asked_again() {
    let conn = profile();
    let first = create_own(&conn, Issuer::Genesis, NOW).expect("genesis");
    let seed = *own_seed(&conn).expect("seed").expect("stored").expose();

    let again = create_own(&conn, Issuer::Genesis, NOW + 60).expect("resumed");

    assert_eq!(again, first, "a resumed setup signs nothing new");
    assert_eq!(
        *own_seed(&conn).expect("seed").expect("stored").expose(),
        seed
    );
}

#[test]
fn a_recovery_kit_certificate_is_self_issued_with_its_kind() {
    let conn = profile();
    let cert = create_own(&conn, Issuer::RecoveryKit, NOW).expect("kit");
    assert_eq!(cert.body.kind, CertKind::RecoveryKit);
    assert_eq!(cert.body.issuer_device, ME);
    assert_eq!(
        trust::entry(&conn, ME)
            .expect("entry")
            .expect("pinned")
            .kind,
        CertKind::RecoveryKit
    );
}

#[test]
fn a_paired_certificate_is_issued_with_the_holders_ticket() {
    let conn = profile();
    let holder = SignSeed::from_bytes(Zeroizing::new([0x77; 32]));
    let holder_id = DeviceId([0x07; 16]);
    let (secret, ticket) = cert::issue_ticket(&holder, VAULT, holder_id, NOW).expect("ticket");

    let cert = create_own(
        &conn,
        Issuer::Ticket {
            secret: &secret,
            ticket: &ticket,
            holder: holder_id,
            holder_sign_pub: sign_pub(&holder),
        },
        NOW,
    )
    .expect("paired");

    assert_eq!(cert.body.kind, CertKind::Paired);
    assert_eq!(cert.body.issuer_device, holder_id);
    assert_eq!(cert.ticket.as_ref(), Some(&ticket));
    cert.verify_signatures().expect("verifies");
}

#[test]
fn a_ticket_of_another_holder_signs_nothing_and_stores_nothing() {
    let conn = profile();
    let holder = SignSeed::from_bytes(Zeroizing::new([0x77; 32]));
    let other = SignSeed::from_bytes(Zeroizing::new([0x78; 32]));
    let holder_id = DeviceId([0x07; 16]);
    let (secret, ticket) = cert::issue_ticket(&holder, VAULT, holder_id, NOW).expect("ticket");

    let refused = create_own(
        &conn,
        Issuer::Ticket {
            secret: &secret,
            ticket: &ticket,
            holder: holder_id,
            holder_sign_pub: sign_pub(&other),
        },
        NOW,
    );

    assert!(refused.is_err());
    assert!(own_seed(&conn).expect("seed").is_none(), "no seed stored");
    assert!(trust::entries(&conn).expect("entries").is_empty());
}

#[test]
fn a_rejoin_successor_is_vouched_for_by_the_old_id() {
    let conn = profile();
    let old = create_own(&conn, Issuer::Genesis, NOW).expect("genesis");
    let handed = predecessor(&conn).expect("predecessor");
    assert!(handed.is_some(), "a device with a certificate hands it on");

    moved_on(&conn);
    let cert = successor(&conn, handed, NOW + 10)
        .expect("successor")
        .expect("a certificate");

    assert_eq!(cert.body.kind, CertKind::Rejoin);
    assert_eq!(cert.body.device, NEXT);
    assert_eq!(cert.body.issuer_device, ME);
    assert_eq!(cert.body.issuer_sign_pub, old.body.sign_pub);
    assert_eq!(cert.body.box_pub, box_pub(&conn));
    assert_ne!(cert.body.sign_pub, old.body.sign_pub, "a new signing key");
    assert_eq!(
        own_seed(&conn)
            .expect("seed")
            .map(|s| sign_pub(&s))
            .expect("stored"),
        cert.body.sign_pub
    );
    let old_entry = trust::entry(&conn, ME).expect("entry").expect("pinned");
    assert_eq!(old_entry.state, TrustState::Trusted, "the old ID stays");
    assert_eq!(
        trust::entry(&conn, NEXT)
            .expect("entry")
            .expect("pinned")
            .kind,
        CertKind::Rejoin
    );
    assert!(trust::is_supported(&conn).expect("supported"));
}

#[test]
fn a_rejoin_without_a_certificate_makes_a_seed_and_no_certificate() {
    // A vault set up before phase 5: no seed at all, or a seed made at unlock without a
    // certificate. Either way the successor signs nothing it could not be vouched for.
    for with_seed in [false, true] {
        let conn = profile();
        if with_seed {
            assert!(ensure_seed(&conn).expect("seed at unlock"));
        }
        let before = own_seed(&conn).expect("seed").map(|s| *s.expose());
        let handed = predecessor(&conn).expect("predecessor");
        assert!(handed.is_none(), "nothing to hand on");

        moved_on(&conn);
        let cert = successor(&conn, handed, NOW).expect("successor");

        assert!(cert.is_none());
        let after = own_seed(&conn).expect("seed").expect("a new seed");
        assert_ne!(
            Some(*after.expose()),
            before,
            "the successor's seed is its own"
        );
        assert!(trust::entries(&conn).expect("entries").is_empty());
        assert!(!trust::is_supported(&conn).expect("supported"));
    }
}

#[test]
fn at_unlock_a_profile_without_a_seed_gets_one_and_no_certificate() {
    let conn = profile();
    assert!(ensure_seed(&conn).expect("made"));
    let seed = *own_seed(&conn).expect("seed").expect("stored").expose();
    assert!(!ensure_seed(&conn).expect("again"), "made once");
    assert_eq!(
        *own_seed(&conn).expect("seed").expect("kept").expose(),
        seed
    );
    assert!(trust::entries(&conn).expect("entries").is_empty());
    assert!(!trust::is_supported(&conn).expect("supported"));
}

#[test]
fn a_profile_with_sync_off_gets_no_seed() {
    let conn = migrated();
    assert!(!ensure_seed(&conn).expect("no row"));
    state::enable(&conn, ME).expect("enable");
    conn.execute("UPDATE sync_state SET enabled = 0", [])
        .expect("off");
    assert!(!ensure_seed(&conn).expect("off"));
    assert!(own_seed(&conn).expect("seed").is_none());
}
