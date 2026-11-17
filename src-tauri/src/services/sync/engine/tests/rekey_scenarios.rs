//! Multi-device scenarios of removal and key changes over the fake relay with real rounds (phase 5
//! plan Task 14, revision Task 22): a device offline over a removal, two devices removing each
//! other at once, a pairing that a removal overtakes, a vault from before phase 5, the D3 trap, a
//! backdated certificate from a left device's copy, a chain a removed device forges in a forked
//! slot, a Kit-joined device offline over a new Kit, a key change given up by its maker; and a
//! seeded property test over four devices (`property` below).

use std::path::Path;

use rusqlite::Connection;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::kit_join::{kit_join, me, new_secret, newcomer, round_mode, snapshot_by};
use super::rekey_fixture::*;
use super::rekey_review::{pin_raw, stray_kit};
use super::runtime::write_and_seal;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::hlc::{pack, Clock};
use crate::services::sync::identity::cert::{self, CertFields};
use crate::services::sync::identity::trust::{self, ExclusionReason, TrustState};
use crate::services::sync::identity::{self, CertKind, Certificate, Issuer, SignSeed};
use crate::services::sync::join::{self, join_new_device, JoinIdentity};
use crate::services::sync::pairing::holder;
use crate::services::sync::rekey::bundle::{self, Opened};
use crate::services::sync::rekey::kit::KitStorage;
use crate::services::sync::rekey::rotate::{self, Outcome, Removal};
use crate::services::sync::rekey::{envelope, key_change_notice};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::Relay;
use crate::services::sync::test_support::assert_converged;
use crate::services::sync::{devices, setup};

mod property;

fn vault_b64(vault: &Vault) -> String {
    vault.creds.vault_id.to_b64()
}

fn seed_of(conn: &Connection) -> SignSeed {
    identity::own_seed(conn)
        .expect("seed")
        .expect("signing key")
}

fn validation_key(error: &AppError) -> Option<String> {
    match error {
        AppError::Validation(detail) => Some(detail.split('|').next().unwrap_or_default().into()),
        _ => None,
    }
}

/// An X25519 key pair from a fixed byte (a phantom device's box key).
fn box_key(byte: u8) -> (StaticSecret, [u8; 32]) {
    let secret = StaticSecret::from([byte; 32]);
    let public = PublicKey::from(&secret).to_bytes();
    (secret, public)
}

/// `subject`'s paired certificate by `holder` (signing with `holder_seed`), its box key
/// `box_pub`, ticket and certificate dated `at`.
fn paired_cert(
    vault: [u8; 16],
    holder: DeviceId,
    holder_seed: &SignSeed,
    subject: DeviceId,
    subject_seed: &SignSeed,
    box_pub: [u8; 32],
    at: i64,
) -> Certificate {
    let (secret, ticket) = cert::issue_ticket(holder_seed, vault, holder, at).expect("ticket");
    cert::issue_paired(
        &secret,
        &ticket,
        holder,
        &identity::sign_pub(holder_seed),
        &CertFields {
            vault_id: vault,
            device: subject,
            box_pub,
            issued_at: at,
        },
        subject_seed,
    )
    .expect("paired")
}

/// The row of `device` carrying `cert`, as the first change of its own chain, sealed under
/// `creds`' current key (what a device of a copy writes when it joins).
fn own_row(creds: &Credentials, device: DeviceId, cert: &Certificate, at: i64) -> Vec<u8> {
    let clock = Clock {
        hlc: pack(at as u64 * 1000, 0),
        device,
    };
    let cs = changeset(
        device,
        1,
        None,
        vec![row(
            "sync_devices",
            &device.to_hex(),
            clock,
            &[
                ("public_key", text("box")),
                ("name", text("Phantom")),
                ("platform", text("ios")),
                ("app_version", text("1.0")),
                ("joined_at", SqlValue::Integer(at)),
                ("cert", text(&cert.to_b64())),
            ],
        )],
    );
    forge(creds, &cs)
}

/// What the bundle of `rotation` gives the holder of `secret`: `true` for the new key.
fn bundle_gives_key(vault: &Vault, rotation: u32, secret: &StaticSecret) -> bool {
    let bytes = vault
        .relay
        .stored_rekey(&vault_b64(vault), rotation)
        .unwrap_or_else(|| panic!("bundle {rotation}"));
    matches!(
        bundle::open(&bytes, &vault.creds.vault_id.0, secret).expect("opens"),
        Opened::Recipient(_)
    )
}

fn box_secret(conn: &Connection) -> StaticSecret {
    keys::device_box_secret(conn)
        .expect("secret")
        .expect("box key")
}

fn reason(plan: &trust::Recipients, device: DeviceId) -> Option<ExclusionReason> {
    plan.excluded
        .iter()
        .find(|(d, _)| *d == device)
        .map(|(_, r)| *r)
}

fn included(plan: &trust::Recipients, device: DeviceId) -> bool {
    plan.included.iter().any(|e| e.device == device)
}

/// `(seq, hash)` of the last envelope of `conn`'s own chain.
fn own_tip(conn: &Connection) -> Option<(i64, Vec<u8>)> {
    conn.query_row(
        "SELECT seq, hash FROM sync_own_log ORDER BY seq DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

/// Every envelope `author` sent was applied once and in order on `reader`: its head there is
/// the author's own last envelope.
fn applied_all_of(reader: &Connection, author: &Connection, id: DeviceId) {
    let tip = own_tip(author);
    let seen = head(reader, id).map(|(seq, hash, quarantined)| {
        assert!(!quarantined, "{id:?} quarantined");
        (seq, hash)
    });
    assert_eq!(seen, tip, "{id:?}'s chain");
}

/// A joiner paired by `holder` the way the app does it: the holder uploads a snapshot, reads its
/// slot-2 payload with the ticket announced in its own chain, and the joiner joins with it.
async fn join_paired(vault: &Vault, holder: &TestConn, dir: &Path, name: &str) -> TestConn {
    let report = run_round(
        &vault.relay,
        holder,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("round");
    assert!(report.snapshot_uploaded, "{report:?}");
    let payload = holder::announced_payload(&holder.lock(), NOW)
        .expect("payload")
        .expect("sync is on");
    joiner_with(vault, payload, dir, name)
        .await
        .expect("joined")
}

async fn joiner_with(
    vault: &Vault,
    payload: crate::services::sync::pairing::SlotPayload,
    dir: &Path,
    name: &str,
) -> crate::services::sync::error::SyncResult<TestConn> {
    let joiner = TestConn::new(migrated());
    join_named(vault, &joiner, payload, dir, name).await?;
    Ok(joiner)
}

/// `joiner` joins with what a holder sealed into slot 2.
async fn join_into(
    vault: &Vault,
    joiner: &TestConn,
    payload: crate::services::sync::pairing::SlotPayload,
    dir: &Path,
) -> crate::services::sync::error::SyncResult<()> {
    join_named(vault, joiner, payload, dir, "Tablet").await
}

async fn join_named(
    vault: &Vault,
    joiner: &TestConn,
    payload: crate::services::sync::pairing::SlotPayload,
    dir: &Path,
    name: &str,
) -> crate::services::sync::error::SyncResult<()> {
    let crate::services::sync::pairing::SlotPayload {
        creds,
        heads,
        identity,
    } = payload;
    join_new_device(
        &vault.relay,
        joiner,
        creds,
        name,
        agreed(joiner),
        JoinIdentity::Paired(identity.expect("identity"), heads),
        dir,
        &NoBinding,
        NOW,
    )
    .await
    .map(|_| ())
}

/// A device that turns sync off as the app does: its `left` record goes out, then sync is off.
async fn turn_off_and_leave(vault: &Vault, device: &TestConn, dir: &Path) {
    setup::leave_best_effort(&vault.relay, device, setup::LEAVE_BUDGET).await;
    setup::turn_off_between_rounds(device, dir)
        .await
        .expect("turned off");
}

/// Plan Task 14: A removes C while B is offline with changes it sealed but never sent and changes
/// still queued. B returns: it picks the key up, seals both again under the new key and sends
/// them; A applies each once; C, removed, opens none of them.
#[tokio::test]
async fn a_device_offline_with_unsent_changes_comes_back_after_a_removal() {
    let three = three().await;
    write_and_seal(&three.vault, &three.vault.b, "b-sealed");
    institution(&three.vault.b.lock(), "b-queued");
    institution(&three.c.lock(), "c-before");
    round_of(&three.vault, &three.c).await;

    assert!(matches!(
        remove(&three, A, &[C]).await.expect("removed"),
        Outcome::Changed { epoch: 1, .. }
    ));
    round_of(&three.vault, &three.vault.a).await;
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(unsent_epochs(&three.vault.b.lock()).is_empty());
    round_of(&three.vault, &three.vault.a).await;

    {
        let (a, b) = (three.vault.a.lock(), three.vault.b.lock());
        for id in ["b-sealed", "b-queued", "c-before"] {
            assert!(has_institution(&a, id) && has_institution(&b, id), "{id}");
        }
        applied_all_of(&a, &b, B);
        applied_all_of(&b, &a, A);
        assert_converged(&[&a, &b]);
    }
    let report = round_of(&three.vault, &three.c).await;
    assert!(report.removed_here, "{report:?}");
    let c = three.c.lock();
    assert!(!has_institution(&c, "b-sealed") && !has_institution(&c, "b-queued"));
}

/// Plan Task 14: A and B remove each other at once (both confirmed the plan before either
/// change reached the server): exactly one change wins; the loser is removed and never holds
/// the new key; C and the winner go on together.
#[tokio::test]
async fn two_devices_removing_each_other_at_once_leave_exactly_one() {
    let three = three().await;
    let secret_of = |id: DeviceId| box_secret(&three.device(id).lock());
    let (secret_a, secret_b) = (secret_of(A), secret_of(B));
    let consent_a = rotate::plan(&three.vault.a.lock(), &[B], &[], false, NOW)
        .expect("plan")
        .consent;
    let consent_b = rotate::plan(&three.vault.b.lock(), &[A], &[], false, NOW)
        .expect("plan")
        .consent;
    let forget = || {};
    let by_a = rotate::remove(
        &three.vault.relay,
        &three.vault.a,
        Removal {
            removing: &[B],
            new_kit: false,
            consent: &consent_a,
        },
        three.dir.path(),
        &forget,
        NOW,
    );
    let by_b = rotate::remove(
        &three.vault.relay,
        &three.vault.b,
        Removal {
            removing: &[A],
            new_kit: false,
            consent: &consent_b,
        },
        three.dir.path(),
        &forget,
        NOW,
    );

    let (by_a, by_b) = tokio::join!(by_a, by_b);

    let (by_a, by_b) = (by_a.expect("A's answer"), by_b.expect("B's answer"));
    let a_won = matches!(by_a, Outcome::Changed { epoch: 1, .. });
    let b_won = matches!(by_b, Outcome::Changed { epoch: 1, .. });
    assert!(a_won != b_won, "exactly one wins: {by_a:?} / {by_b:?}");
    let (winner, loser, loser_outcome) = if a_won { (A, B, by_b) } else { (B, A, by_a) };
    assert!(
        matches!(loser_outcome, Outcome::RemovedHere | Outcome::Raced { .. }),
        "{loser_outcome:?}"
    );
    let loser_conn = three.device(loser);
    if sync_on(&loser_conn.lock()) {
        let report = round_of(&three.vault, loser_conn).await;
        assert!(report.removed_here, "the loser is removed: {report:?}");
    }
    assert!(!sync_on(&loser_conn.lock()));
    let loser_secret = if loser == A { &secret_a } else { &secret_b };
    assert!(!bundle_gives_key(&three.vault, 1, loser_secret));

    round_of(&three.vault, three.device(winner)).await;
    institution(&three.c.lock(), "c-after");
    for id in [C, winner, C] {
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(report.problem, None, "{id:?}: {report:?}");
    }
    let (w, c) = (three.device(winner).lock(), three.c.lock());
    assert_eq!((key_epoch(&w), key_epoch(&c)), (1, 1));
    assert!(has_institution(&w, "c-after"));
    assert_converged(&[&w, &c]);
}

/// Plan Task 14: A removes C while B is pairing D: D received the credentials B sealed before the
/// change. Joining from the snapshot of before the change goes through and the change gave D
/// nothing (`keyNotGiven`); once a snapshot under the new key is on the server, the join itself
/// is refused as a failed pairing, with nothing replaced (As built R-73). Either way D pairs
/// again and syncs.
#[tokio::test]
async fn a_pairing_overtaken_by_a_removal_ends_key_not_given_and_pairs_again() {
    for newer_snapshot in [false, true] {
        let three = three().await;
        snapshot_by(&three, &three.vault.b).await;
        let payload = holder::announced_payload(&three.vault.b.lock(), NOW)
            .expect("payload")
            .expect("sync is on");
        remove(&three, A, &[C]).await.expect("removed");
        if newer_snapshot {
            snapshot_by(&three, &three.vault.a).await;
        }
        let dir = tempfile::tempdir().expect("dir");
        let d = TestConn::new(migrated());
        institution(&d.lock(), "local-only");

        let joined = join_into(&three.vault, &d, payload, dir.path()).await;

        let first = if newer_snapshot {
            let error = joined.expect_err("refused");
            assert_eq!(
                validation_key(&error.to_app_error()).as_deref(),
                Some("validation.syncPairingFailed"),
                "{error:?}"
            );
            let conn = d.lock();
            assert!(!sync_on(&conn) && has_institution(&conn, "local-only"));
            None
        } else {
            joined.expect("the join itself goes through");
            let report = round_mode(&three, &d, RoundMode::Normal).await;
            assert_eq!(report.problem, Some(SyncProblem::KeyNotGiven), "{report:?}");
            assert_eq!(key_epoch(&d.lock()), 0);
            let first = me(&d.lock());
            // "Připojit znovu": sync off on D first.
            setup::turn_off_between_rounds(&d, dir.path())
                .await
                .expect("off");
            Some(first)
        };

        round_of(&three.vault, &three.vault.a).await;
        round_of(&three.vault, &three.vault.b).await;
        let payload = holder::announced_payload(&three.vault.b.lock(), NOW)
            .expect("payload")
            .expect("sync is on");
        join_into(&three.vault, &d, payload, dir.path())
            .await
            .expect("joins again");
        for device in [&d, &three.vault.b, &three.vault.a, &d] {
            let report = round_mode(&three, device, RoundMode::Normal).await;
            assert_eq!(report.problem, None, "{report:?}");
        }
        let (a, b, d) = (three.vault.a.lock(), three.vault.b.lock(), d.lock());
        let again = me(&d);
        assert_ne!(Some(again), first);
        assert_eq!(key_epoch(&d), 1);
        let entry = trust::entry(&a, again).expect("entry").expect("A pins D");
        assert_eq!((entry.state, entry.issuer), (TrustState::Trusted, B));
        assert!(included(&trust::recipients(&a, &[]).expect("plan"), again));
        assert_converged(&[&a, &b, &d]);
    }
}

/// Plan Task 14: a vault set up before phase 5 (no certificates) syncs as before and refuses a
/// removal.
#[tokio::test]
async fn a_vault_from_before_phase_5_syncs_and_refuses_removal() {
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");
    for device in [&vault.a, &vault.b, &vault.a] {
        let report = round_of(&vault, device).await;
        assert_eq!(report.problem, None, "{report:?}");
    }
    let (a, b) = (vault.a.lock(), vault.b.lock());
    assert!(has_institution(&b, "a1"));
    assert_converged(&[&a, &b]);
    assert!(!trust::is_supported(&a).expect("supported"));
    let refused = rotate::plan(&a, &[B], &[], false, NOW).expect_err("refused");
    assert_eq!(
        validation_key(&refused).as_deref(),
        Some(rotate::REMOVAL_UNSUPPORTED)
    );
}

/// Revision §3.1, the D3 trap end to end: O (here B) pairs N, then turns sync off; D (here C) was
/// offline throughout and then removes X (here A). N keeps syncing, whether N's row reached the
/// server before or after B's `left` record: B's ticket announcement went out ahead of the
/// record in B's own chain.
#[tokio::test]
async fn the_d3_trap_a_device_paired_before_its_holder_left_keeps_syncing() {
    for row_first in [true, false] {
        let three = three().await;
        let dir = tempfile::tempdir().expect("dir");
        let n = join_paired(&three.vault, &three.vault.b, dir.path(), "New phone").await;
        let n_id = me(&n.lock());
        if row_first {
            let report = round_mode(&three, &n, RoundMode::Normal).await;
            assert_eq!(report.problem, None, "{report:?}");
            turn_off_and_leave(&three.vault, &three.vault.b, dir.path()).await;
        } else {
            turn_off_and_leave(&three.vault, &three.vault.b, dir.path()).await;
            let report = round_mode(&three, &n, RoundMode::Normal).await;
            assert_eq!(report.problem, None, "{report:?}");
        }

        // C comes back after everything and removes A.
        let report = round_of(&three.vault, &three.c).await;
        assert_eq!(report.problem, None, "{report:?}");
        {
            let c = three.c.lock();
            let plan = trust::recipients(&c, &[A]).expect("plan");
            assert!(included(&plan, n_id), "row first: {row_first}: {plan:?}");
            assert_eq!(reason(&plan, B), Some(ExclusionReason::Left));
        }
        let outcome = remove(&three, C, &[A]).await.expect("removed");
        assert!(
            matches!(outcome, Outcome::Changed { epoch: 1, .. }),
            "{outcome:?}"
        );
        round_of(&three.vault, &three.c).await;

        institution(&n.lock(), "n-after");
        for device in [&n, &three.c, &n] {
            let report = round_mode(&three, device, RoundMode::Normal).await;
            assert_eq!(report.problem, None, "row first: {row_first}: {report:?}");
        }
        {
            let (c, n) = (three.c.lock(), n.lock());
            assert_eq!(key_epoch(&n), 1, "row first: {row_first}");
            assert!(has_institution(&c, "n-after"));
            assert_converged(&[&c, &n]);
        }
        assert!(round_of(&three.vault, &three.vault.a).await.removed_here);
    }
}

/// Revision §3.1 (R2): a copy of B made while sync was on (a profile backup) keeps B's signing key
/// and the vault key. After B left, the copy vouches for M with a certificate dated before B
/// left, which reaches the log only after B's `left` record and without an announcement: a late
/// vouch. M gets no key until a person keeps it ("Je moje"); kept, it takes the next change.
#[tokio::test]
async fn a_backdated_certificate_from_a_left_devices_copy_stays_out_until_kept() {
    const M: DeviceId = DeviceId([0x4d; 16]);
    for kept in [false, true] {
        let three = three().await;
        let dir = tempfile::tempdir().expect("dir");
        // The copy: B's rows, store, signing key and the vault's credentials.
        let (copy_seed, copy_export, copy_stream) = {
            let b = three.vault.b.lock();
            let mut stream = Vec::new();
            crate::services::sync::snapshot::dump(&b, 0, &mut stream).expect("dump");
            (seed_of(&b), trust::export(&b).expect("export"), stream)
        };
        turn_off_and_leave(&three.vault, &three.vault.b, dir.path()).await;
        round_of(&three.vault, &three.vault.a).await;

        let m = {
            let m = migrated();
            keys::save_credentials(&m, &three.vault.creds).expect("credentials");
            crate::services::sync::snapshot::join(&m, M, &mut copy_stream.as_slice(), NOW)
                .expect("join");
            trust::import_export(&m, &copy_export, B, NOW).expect("store");
            let backdated = NOW - 3600;
            let (secret, ticket) =
                cert::issue_ticket(&copy_seed, three.vault.creds.vault_id.0, B, backdated)
                    .expect("ticket");
            let own = identity::create_own(
                &m,
                Issuer::Ticket {
                    secret: &secret,
                    ticket: &ticket,
                    holder: B,
                    holder_sign_pub: identity::sign_pub(&copy_seed),
                },
                backdated,
            )
            .expect("certificate");
            let public = keys::ensure_device_keypair(&m).expect("box key");
            devices::record_this_device(&m, "Copy", &public, Some(&own), NOW).expect("row");
            TestConn::new(m)
        };
        let report = round_of(&three.vault, &m).await;
        assert_eq!(report.problem, None, "the copy's row goes out: {report:?}");
        round_of(&three.vault, &three.vault.a).await;
        {
            let a = three.vault.a.lock();
            let entry = trust::entry(&a, M).expect("entry").expect("pinned");
            assert_eq!(entry.state, TrustState::Trusted);
            let plan = trust::recipients(&a, &[C]).expect("plan");
            assert_eq!(reason(&plan, M), Some(ExclusionReason::LateVouch));
            if kept {
                assert!(trust::keep(&a, M).expect("Je moje"));
                assert!(included(&trust::recipients(&a, &[C]).expect("plan"), M));
            }
        }

        assert!(matches!(
            remove(&three, A, &[C]).await.expect("removed"),
            Outcome::Changed { epoch: 1, .. }
        ));
        round_of(&three.vault, &three.vault.a).await;
        let report = round_of(&three.vault, &m).await;

        let m_secret = box_secret(&m.lock());
        assert_eq!(bundle_gives_key(&three.vault, 1, &m_secret), kept);
        if kept {
            assert_eq!(report.problem, None, "{report:?}");
            assert_eq!(key_epoch(&m.lock()), 1);
        } else {
            assert_eq!(report.problem, Some(SyncProblem::KeyNotGiven), "{report:?}");
            assert_eq!(key_epoch(&m.lock()), 0);
        }
    }
}

/// Revision §3.2 (R4, transitive `IssuerRemoved`): A removes C; a colluding server hides the key
/// change from B, which lags, and serves it C's later changes in what B takes for slots before
/// the change: the row of F, paired by C, and of G, paired by F. B pins both while it still
/// trusts C. Once B learns of the change, F and G are left out for their issuer however deep,
/// and B's next key change gives neither a key. A, which was not behind, never applies them.
#[tokio::test]
async fn a_chain_a_removed_device_forges_in_a_forked_slot_stays_out() {
    const F: DeviceId = DeviceId([0x46; 16]);
    const G: DeviceId = DeviceId([0x47; 16]);
    let three = three().await;
    let vault = three.vault.creds.vault_id.0;
    let c_seed = seed_of(&three.c.lock());
    remove(&three, A, &[C]).await.expect("removed");
    let head = state::epochs(&three.vault.a.lock()).expect("epochs")[&1];
    round_of(&three.vault, &three.vault.a).await;
    let a_sent = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    for seq in head + 1..=a_sent {
        three.vault.relay.drop_op(seq);
    }
    three.vault.relay.set_rotation(&vault_b64(&three.vault), 0);
    three.vault.relay.set_accept_retired_writes(true);
    let (f_secret, f_pub) = box_key(0x46);
    let (g_secret, g_pub) = box_key(0x47);
    let f_seed = SignSeed::from_bytes(Zeroizing::new([0x46; 32]));
    let g_seed = SignSeed::from_bytes(Zeroizing::new([0x47; 32]));
    let f_cert = paired_cert(vault, C, &c_seed, F, &f_seed, f_pub, NOW);
    let g_cert = paired_cert(vault, F, &f_seed, G, &g_seed, g_pub, NOW);
    push(
        &three.vault.relay,
        &three.vault.creds,
        &[
            own_row(&three.vault.creds, F, &f_cert, NOW),
            own_row(&three.vault.creds, G, &g_cert, NOW),
        ],
    )
    .await;

    round_of(&three.vault, &three.vault.b).await;
    {
        let b = three.vault.b.lock();
        let plan = trust::recipients(&b, &[]).expect("plan");
        assert!(included(&plan, F) && included(&plan, G), "{plan:?}");
    }
    for seq in head + 1..=a_sent {
        three.vault.relay.undrop_op(seq);
    }
    three.vault.relay.set_rotation(&vault_b64(&three.vault), 1);
    three.vault.relay.set_accept_retired_writes(false);
    round_of(&three.vault, &three.vault.b).await;

    {
        let b = three.vault.b.lock();
        assert_eq!(key_epoch(&b), 1);
        assert!(crate::services::sync::rekey::key_change_overrun(&b)
            .expect("overrun")
            .is_some());
        let plan = trust::recipients(&b, &[]).expect("plan");
        assert_eq!(reason(&plan, F), Some(ExclusionReason::IssuerRemoved));
        assert_eq!(reason(&plan, G), Some(ExclusionReason::IssuerRemoved));
    }
    assert!(matches!(
        new_kit(&three, B).await.expect("B's key change"),
        Outcome::Changed { epoch: 2, .. }
    ));
    for rotation in [1, 2] {
        assert!(!bundle_gives_key(&three.vault, rotation, &f_secret));
        assert!(!bundle_gives_key(&three.vault, rotation, &g_secret));
    }
    round_of(&three.vault, &three.vault.a).await;
    let a = three.vault.a.lock();
    for forged in [F, G] {
        let pinned = trust::entry(&a, forged).expect("entry");
        assert!(
            pinned.is_none() || !included(&trust::recipients(&a, &[]).expect("plan"), forged),
            "{forged:?}"
        );
    }
}

/// Revision §2.7: a device joined with the Kit is offline while A makes a new Kit (the old one,
/// its issuer, is removed). It was a recipient at plan time, so the change names it: it picks the
/// key up when it returns and stays, a recipient of the next change too.
#[tokio::test]
async fn a_kit_joined_device_offline_over_a_new_kit_picks_it_up_and_stays() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let old_id = kit_keys(&three, &old).id();
    snapshot_by(&three, &three.vault.a).await;
    let k = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &k, dir.path(), &old).await.expect("join");
    let k_id = me(&k.lock());
    join::lift_kit_join_hold(&k.lock()).expect("Mám je");
    round_mode(&three, &k, RoundMode::Normal).await;
    settle(&three).await;
    for id in [A, B, C] {
        let conn = three.device(id).lock();
        let entry = trust::entry(&conn, k_id).expect("entry").expect("pinned");
        assert_eq!((entry.kind, entry.issuer), (CertKind::RecoveryKit, old_id));
        assert!(included(
            &trust::recipients(&conn, &[]).expect("plan"),
            k_id
        ));
    }

    let new = new_secret(new_kit(&three, A).await.expect("a new Kit"));
    round_of(&three.vault, &three.vault.a).await;
    let report = round_mode(&three, &k, RoundMode::Normal).await;

    assert_eq!(report.problem, None, "{report:?}");
    {
        let k = k.lock();
        assert_eq!(key_epoch(&k), 1);
        let state_of = |id| trust::entry(&k, id).expect("entry").expect("pinned").state;
        assert_eq!(state_of(old_id), TrustState::Removed);
        assert_eq!(
            trust::sealing_kit_entry(&k).expect("Kit").map(|e| e.device),
            Some(kit_keys(&three, &new).id())
        );
    }
    settle(&three).await;
    {
        let b = three.vault.b.lock();
        let entry = trust::entry(&b, k_id).expect("entry").expect("pinned");
        assert!(entry.confirmed, "the change named it");
        assert!(included(&trust::recipients(&b, &[C]).expect("plan"), k_id));
    }
    assert!(matches!(
        remove(&three, B, &[C]).await.expect("removed"),
        Outcome::Changed { epoch: 2, .. }
    ));
    round_of(&three.vault, &three.vault.b).await;
    institution(&k.lock(), "k-after");
    for device in [&k, &three.vault.a, &three.vault.b, &k] {
        let report = round_mode(&three, device, RoundMode::Normal).await;
        assert_eq!(report.problem, None, "{report:?}");
    }
    let (a, b, k) = (three.vault.a.lock(), three.vault.b.lock(), k.lock());
    assert_eq!(key_epoch(&k), 2);
    assert!(has_institution(&a, "k-after"));
    assert_converged(&[&a, &b, &k]);
}

/// As built R-56, across devices: A's new-Kit change reaches the server but its commit can never
/// pass here (a second Kit appeared), so A gives it up and pauses. B and C take the change and
/// its new Kit; the Kit (shown to A's person with `Pending`) opens; a newcomer joins with it;
/// and B's next change seals to it with every key.
#[tokio::test]
async fn a_given_up_key_change_reaches_the_others_with_its_new_kit() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let old_id = kit_keys(&three, &old).id();
    three.vault.relay.lose_next_answer(Route::RotateToken);
    let Outcome::Pending { kit: Some(new) } = new_kit(&three, A).await.expect("pending") else {
        panic!("a pending new Kit");
    };
    pin_raw(
        &three.vault.a.lock(),
        &stray_kit(&three, B),
        "trusted",
        false,
        NOW,
    );
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, Some(SyncProblem::KeyChangeUnverified));
    assert!(rotate::stuck(&three.vault.a.lock()).expect("stuck"));

    for id in [B, C] {
        let report = round_of(&three.vault, three.device(id)).await;
        assert_eq!(report.problem, None, "{id:?}: {report:?}");
        let conn = three.device(id).lock();
        assert_eq!(key_epoch(&conn), 1);
        assert_eq!(
            trust::sealing_kit_entry(&conn)
                .expect("Kit")
                .map(|e| e.device),
            Some(new.id),
            "{id:?}"
        );
        assert!(
            key_change_notice(&conn)
                .expect("notice")
                .expect("written")
                .kit_replaced
        );
        // The rows A's commit would have written (R-74).
        let replaced: Option<String> = conn
            .query_row(
                "SELECT removal_record FROM sync_devices WHERE id = ?1",
                [old_id.to_hex()],
                |r| r.get(0),
            )
            .expect("the old Kit's row");
        assert!(replaced.is_some(), "{id:?}");
        assert!(devices::device_row(&conn, new.id).expect("rows").is_some());
    }
    snapshot_by(&three, &three.vault.b).await;
    let joiner = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    kit_join(&three, &joiner, dir.path(), &new.secret)
        .await
        .expect("the given-up change's Kit works");
    assert_eq!(key_epoch(&joiner.lock()), 1);

    assert!(matches!(
        remove(&three, B, &[C]).await.expect("removed"),
        Outcome::Changed {
            epoch: 2,
            kit: None,
            ..
        }
    ));
    let read = three
        .vault
        .relay
        .get_kit(
            &vault_b64(&three.vault),
            kit_keys(&three, &new.secret).token(),
        )
        .await
        .expect("read");
    let opened = envelope::open(&read.envelope, &kit_keys(&three, &new.secret)).expect("opens");
    let b_keys = keys::vault_keys(&three.vault.b.lock())
        .expect("keys")
        .expect("keys");
    let epochs: Vec<u32> = opened.keys.iter().map(|k| k.epoch).collect();
    assert_eq!(epochs, vec![0, 1, 2]);
    for key in &opened.keys {
        assert_eq!(Some(&key.key), b_keys.get(key.epoch).as_ref());
    }
    // A stays paused: its way out is turning sync off and joining again.
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, Some(SyncProblem::KeyChangeUnverified));
}

/// As built R-75 (found by the property test): leaving seals no envelope, so the Kit's envelope
/// stays the one the leaving device sealed until the next key change. A Kit join in between
/// works, and the newcomer syncs with the devices that stayed.
#[tokio::test]
async fn the_kit_still_works_after_the_device_that_sealed_its_envelope_left() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let dir = tempfile::tempdir().expect("dir");
    turn_off_and_leave(&three.vault, &three.vault.a, dir.path()).await;
    round_of(&three.vault, &three.vault.b).await;
    snapshot_by(&three, &three.vault.b).await;
    let joiner = newcomer();

    kit_join(&three, &joiner, dir.path(), &secret)
        .await
        .expect("the Kit joins");

    join::lift_kit_join_hold(&joiner.lock()).expect("Mám je");
    for device in [&joiner, &three.vault.b, &three.c, &joiner] {
        let report = round_mode(&three, device, RoundMode::Normal).await;
        assert_eq!(report.problem, None, "{report:?}");
    }
    let (b, c, k) = (three.vault.b.lock(), three.c.lock(), joiner.lock());
    assert_eq!(
        trust::entry(&k, A).expect("entry").map(|e| e.state),
        Some(TrustState::Left)
    );
    assert_converged(&[&b, &c, &k]);
}
