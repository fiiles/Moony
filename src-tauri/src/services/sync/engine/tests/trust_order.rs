//! Where entries were first seen, and the holder's ticket announcements (phase 5 revision §3.1,
//! the R2 mitigation): a holder announces the ticket of a pairing in its own chain before anything
//! it writes later, its `left` record included; every device notes the relay position at which
//! it first saw each certificate, `left` record and announcement, and a restored snapshot's rows
//! count as seen at the snapshot's position.

use rusqlite::Connection;

use super::rekey_fixture::{round_of, three};
use super::*;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::changeset::from_json;
use crate::services::sync::engine::pull::note_trust_positions;
use crate::services::sync::envelope::open_op;
use crate::services::sync::hlc::{pack, Clock};
use crate::services::sync::identity::cert::{self, CertFields};
use crate::services::sync::identity::trust::{self, ExclusionReason};
use crate::services::sync::identity::{self, sign_pub, Record, RecordType, SignSeed};
use crate::services::sync::pairing::holder;
use crate::services::sync::setup;

const N: DeviceId = DeviceId([0x0e; 16]);

fn seen(conn: &Connection, device: DeviceId, what: &str) -> Option<i64> {
    conn.query_row(
        "SELECT pos FROM sync_trust_seen WHERE device = ?1 AND what = ?2",
        params![device.to_hex(), what],
        |r| r.get(0),
    )
    .ok()
}

fn announced(conn: &Connection, ticket_pub: &[u8; 32]) -> Option<(String, i64)> {
    conn.query_row(
        "SELECT issuer, pos FROM sync_tickets WHERE ticket_pub = ?1",
        [ticket_pub.to_vec()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// The changesets of `conn`'s `sync_unsent`, in sequence order.
fn unsent_changesets(conn: &Connection) -> Vec<Changeset> {
    let creds = keys::credentials(conn).expect("creds").expect("bound");
    conn.prepare("SELECT envelope FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .expect("rows")
        .map(|ct| {
            let ct = ct.expect("row");
            let json =
                open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &ct).expect("opens");
            from_json(&json).expect("changeset")
        })
        .collect()
}

/// A device writes its own `left` record into its row (captured, so it goes out with the next
/// push), as leaving does (`setup::write_left_record`, shared with `rekey_leave`).
fn leave(conn: &Connection, at: i64) {
    assert!(setup::write_left_record(conn, at).expect("left record"));
}

/// N's certificate, paired by A with `ticket`, as a forged changeset of N's own chain.
fn n_paired_by_a(
    vault: &Vault,
    secret: &cert::TicketSecret,
    ticket: &cert::Ticket,
    a_sign_pub: [u8; 32],
) -> Vec<u8> {
    let n_seed = SignSeed::from_bytes(zeroize::Zeroizing::new([0x4e; 32]));
    let fields = CertFields {
        vault_id: vault.creds.vault_id.0,
        device: N,
        box_pub: [0x5e; 32],
        issued_at: ticket.issued_at,
    };
    let cert =
        cert::issue_paired(secret, ticket, A, &a_sign_pub, &fields, &n_seed).expect("paired");
    let at = Clock {
        hlc: pack((NOW as u64 + 3600) * 1000, 0),
        device: N,
    };
    let cs = changeset(
        N,
        1,
        None,
        vec![row(
            "sync_devices",
            &N.to_hex(),
            at,
            &[
                ("public_key", text("NPK")),
                ("name", text("New phone")),
                ("platform", text("ios")),
                ("app_version", text("1.0")),
                ("joined_at", SqlValue::Integer(NOW)),
                ("cert", text(&cert.to_b64())),
            ],
        )],
    );
    forge(&vault.creds, &cs)
}

/// The holder's confirm queues its ticket announcement as the next envelope of its chain, with no
/// rows; it reaches the server before the holder's later `left` record, and every other device
/// records both positions in that order.
#[tokio::test]
async fn the_announcement_goes_out_before_the_holders_later_left_record() {
    let three = three().await;
    let before = unsent_changesets(&three.vault.a.lock()).len();
    let payload = holder::announced_payload(&three.vault.a.lock(), NOW)
        .expect("payload")
        .expect("sync is on");
    let ticket_pub = payload
        .identity
        .as_ref()
        .expect("identity")
        .ticket
        .ticket_pub;
    let unsent = unsent_changesets(&three.vault.a.lock());
    assert_eq!(unsent.len(), before + 1, "one envelope");
    let announcement = unsent.last().expect("queued");
    assert!(announcement.rows.is_empty());
    assert_eq!(announcement.tickets, vec![ticket_pub]);
    assert_eq!(announcement.device, A);
    drop(payload);

    // A leaves as turning sync off does: the announcement queued at the confirm is ahead of the
    // `left` record in the one push (`setup::leave_best_effort`).
    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(NoBinding),
        setup::LEAVE_BUDGET,
    )
    .await;
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(
        report.problem, None,
        "the announcement applies as an empty change"
    );

    let b = three.vault.b.lock();
    let (issuer, ticket_at) = announced(&b, &ticket_pub).expect("announcement recorded");
    assert_eq!(issuer, A.to_hex());
    let left_at = seen(&b, A, "left").expect("left record seen");
    assert!(ticket_at < left_at, "{ticket_at} < {left_at}");
}

/// The D3 trap on a device that was offline throughout: A pairs N and then leaves; N's row reaches
/// the server only after A's `left` record. N stays a recipient thanks to the announcement, while
/// a pairing whose ticket A never announced is a late vouch.
#[tokio::test]
async fn a_pairing_announced_before_its_holder_left_keeps_its_device_whatever_order_it_arrives() {
    for announce in [true, false] {
        let three = three().await;
        let a_seed = identity::own_seed(&three.vault.a.lock())
            .expect("seed")
            .expect("key");
        let (secret, ticket) = if announce {
            let payload = holder::announced_payload(&three.vault.a.lock(), NOW)
                .expect("payload")
                .expect("sync is on");
            let identity = payload.identity.expect("identity");
            (identity.ticket_secret, identity.ticket)
        } else {
            cert::issue_ticket(&a_seed, three.vault.creds.vault_id.0, A, NOW).expect("ticket")
        };
        leave(&three.vault.a.lock(), NOW + 60);
        round_of(&three.vault, &three.vault.a).await;
        push(
            &three.vault.relay,
            &three.vault.creds,
            &[n_paired_by_a(
                &three.vault,
                &secret,
                &ticket,
                sign_pub(&a_seed),
            )],
        )
        .await;

        round_of(&three.vault, &three.vault.b).await;
        // The pull's own refresh hook applied what arrived; no refresh by hand.
        let b = three.vault.b.lock();
        assert_eq!(
            trust::entry(&b, A).expect("entry").map(|e| e.state),
            Some(trust::TrustState::Left)
        );
        assert!(trust::entry(&b, N).expect("entry").is_some(), "N is pinned");
        assert!(seen(&b, N, "cert").expect("cert") > seen(&b, A, "left").expect("left"));
        let plan = trust::recipients(&b, &[]).expect("recipients");
        let included = plan.included.iter().any(|e| e.device == N);
        assert_eq!(included, announce, "announced: {announce}");
        if !announce {
            assert!(plan.excluded.contains(&(N, ExclusionReason::LateVouch)));
        }
    }
}

/// The pull records the first position of each certificate and `left` record it applies; a later
/// sighting does not move it.
#[tokio::test]
async fn the_pull_records_where_certificates_and_left_records_were_first_seen() {
    let three = three().await;
    let b = three.vault.b.lock();
    let a_cert = seen(&b, A, "cert").expect("A's certificate came by the pull");
    assert!(a_cert > 0);
    assert_eq!(seen(&b, A, "left"), None);
    // The first one wins.
    let clock = Clock {
        hlc: pack(NOW as u64 * 1000, 0),
        device: A,
    };
    let a_cert_value: String = b
        .query_row(
            "SELECT cert FROM sync_devices WHERE id = ?1",
            [A.to_hex()],
            |r| r.get(0),
        )
        .expect("A's row");
    let rows = vec![row(
        "sync_devices",
        &A.to_hex(),
        clock,
        &[("cert", text(&a_cert_value))],
    )];
    note_trust_positions(&b, &rows, 9_998).expect("note");
    assert_eq!(seen(&b, A, "cert"), Some(a_cert));
    // Junk, or a value the row does not hold (the merge discarded it), is no sighting (R-27).
    let rows = vec![row(
        "sync_devices",
        &A.to_hex(),
        clock,
        &[("cert", text("again")), ("left_record", text("left"))],
    )];
    note_trust_positions(&b, &rows, 9_999).expect("note");
    assert_eq!(seen(&b, A, "left"), None);
    // A `left` record the row holds and A signed: noted.
    let a_seed = identity::own_seed(&three.vault.a.lock())
        .expect("seed")
        .expect("A's key");
    let vault = three.vault.creds.vault_id.0;
    let left = Record::sign(RecordType::Left, vault, A, A, NOW, 0, &a_seed).expect("left");
    b.execute(
        "UPDATE sync_devices SET left_record = ?1 WHERE id = ?2",
        params![left.to_b64(), A.to_hex()],
    )
    .expect("held");
    let rows = vec![row(
        "sync_devices",
        &A.to_hex(),
        clock,
        &[("left_record", text(&left.to_b64()))],
    )];
    note_trust_positions(&b, &rows, 9_999).expect("note");
    assert_eq!(seen(&b, A, "left"), Some(9_999));
    // Other tables and NULL values are not sightings.
    let rows = vec![
        row("institutions", &C.to_hex(), clock, &[("cert", text("x"))]),
        row(
            "sync_devices",
            &C.to_hex(),
            clock,
            &[("cert", SqlValue::Null), ("name", text("C"))],
        ),
    ];
    let before: i64 = count(&b, "SELECT COUNT(*) FROM sync_trust_seen");
    note_trust_positions(&b, &rows, 10_000).expect("note");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_trust_seen"), before);
}

/// A device joining from a snapshot sees every restored certificate at the snapshot's position.
#[tokio::test]
async fn restored_rows_count_as_seen_at_the_snapshots_position() {
    let three = three().await;
    let mut stream = Vec::new();
    crate::services::sync::snapshot::dump(&three.vault.a.lock(), 77, &mut stream).expect("dump");
    let fresh = migrated();
    keys::save_credentials(&fresh, &three.vault.creds).expect("credentials");
    crate::services::sync::snapshot::join(
        &fresh,
        DeviceId([0x0d; 16]),
        &mut stream.as_slice(),
        NOW,
    )
    .expect("join");
    for device in [A, B, C] {
        assert_eq!(seen(&fresh, device, "cert"), Some(77), "{device:?}");
    }
}
