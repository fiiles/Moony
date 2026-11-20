//! Removing devices on this device, which always changes the key (phase 5 spec §6.4, §10.4;
//! revision §2.5, §2.7, §4): the plan and its consent, the Kit it replaces and the devices the
//! removed ones added, the key change with its Kit envelope and every answer the token call can
//! get, and a pending change resolved after a crash or a lost answer.

use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::FutureExt;

use super::rekey_fixture::*;
use super::*;
use crate::error::AppError;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::error::SyncError;
use zeroize::Zeroizing;

use crate::services::sync::identity::trust::{self, ExclusionReason, TrustState};
use crate::services::sync::identity::{self, cert, CertKind, Certificate, KitSecret, SignSeed};
use crate::services::sync::keys;
use crate::services::sync::rekey::bundle::{self, Opened};
use crate::services::sync::rekey::kit::{self, KitStorage};
use crate::services::sync::rekey::rotate::{self, Outcome, Removal, ReplacesKit};
use crate::services::sync::rekey::{self, envelope, head};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::Relay;
use crate::services::sync::relay::RelayError;

fn validation_key(error: &SyncError) -> Option<String> {
    match error {
        SyncError::Local(AppError::Validation(detail)) => {
            Some(detail.split('|').next().unwrap_or_default().to_string())
        }
        _ => None,
    }
}

fn removal_record(conn: &rusqlite::Connection, device: DeviceId) -> (Option<String>, i64) {
    conn.query_row(
        "SELECT removal_record, removed FROM sync_devices WHERE id = ?1",
        [device.to_hex()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .expect("row")
}

fn queued(conn: &rusqlite::Connection, device: DeviceId) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sync_outbox WHERE tbl = 'sync_devices' AND pk = ?1)",
        [device.to_hex()],
        |r| r.get(0),
    )
    .expect("outbox")
}

fn rotations(three: &Three) -> usize {
    three
        .vault
        .relay
        .calls()
        .iter()
        .filter(|r| **r == Route::RotateToken)
        .count()
}

#[tokio::test]
async fn the_plan_lists_who_gets_the_key_and_its_consent_follows_what_is_confirmed() {
    let three = three().await;
    let a = three.vault.a.lock();

    let plan = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");

    let recipients: Vec<DeviceId> = plan.recipients.iter().map(|e| e.device).collect();
    assert_eq!(recipients, vec![A, B]);
    assert_eq!(plan.excluded, vec![(C, ExclusionReason::Removing)]);
    assert_eq!(plan.removing, vec![C]);
    assert_eq!(plan.epoch, 0);
    assert_eq!(plan.kit, None);
    assert_eq!(plan.replaces_kit, ReplacesKit::No);
    let with_kit = rotate::plan(&a, &[C], &[], true, NOW).expect("plan");
    let other_set = rotate::plan(&a, &[B, C], &[], false, NOW).expect("plan");
    let nobody = rotate::plan(&a, &[], &[], true, NOW).expect("plan");
    assert_eq!(with_kit.replaces_kit, ReplacesKit::Asked);
    assert_eq!(
        rotate::plan(&a, &[B], &[C], false, NOW)
            .expect("plan")
            .consent,
        other_set.consent,
        "a ticked device is removed like one asked for"
    );
    let consents = [
        &plan.consent,
        &with_kit.consent,
        &other_set.consent,
        &nobody.consent,
    ];
    for (i, x) in consents.iter().enumerate() {
        for y in &consents[i + 1..] {
            assert_ne!(x, y);
        }
    }
    assert_eq!(
        rotate::plan(&a, &[C], &[], false, NOW)
            .expect("plan")
            .consent,
        plan.consent,
        "the same plan, the same consent"
    );
    // The epoch is part of it.
    let mut later = plan.clone();
    later.epoch = 1;
    assert_ne!(rotate::consent(&later), plan.consent);
    // So is each recipient's certificate.
    let mut other_cert = plan.clone();
    other_cert.recipients[1] = other_cert.recipients[0].clone();
    assert_ne!(rotate::consent(&other_cert), plan.consent);
}

#[tokio::test]
async fn a_stale_consent_is_refused_and_nothing_is_written() {
    let three = three().await;
    let stale = rotate::plan(&three.vault.a.lock(), &[C], &[], false, NOW)
        .expect("plan")
        .consent;

    let error = rotate::remove(
        &three.vault.relay,
        &three.vault.a,
        Removal {
            removing: &[B, C],
            new_kit: false,
            consent: &stale,
        },
        three.dir.path(),
        &|| {},
        NOW,
    )
    .await
    .expect_err("another plan");

    assert_eq!(
        validation_key(&error).as_deref(),
        Some(rotate::PLAN_CHANGED)
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 0);
    assert_eq!(removal_record(&a, C).0, None);
    assert_eq!(rotations(&three), 0);
}

#[tokio::test]
async fn removal_refuses_an_older_vault_this_device_and_a_paused_profile() {
    // A vault set up before phase 5: no certificates.
    let old = vault().await;
    assert_eq!(
        rotate::plan(&old.a.lock(), &[B], &[], false, NOW).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::REMOVAL_UNSUPPORTED.into()).to_string())
    );

    let three = three().await;
    let a = three.vault.a.lock();
    assert_eq!(
        rotate::plan(&a, &[A], &[], false, NOW).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::REMOVE_THIS_DEVICE.into()).to_string())
    );
    // Nobody removed is no key change, unless it makes a new Kit (D2: no "change the key").
    assert_eq!(
        rotate::plan(&a, &[], &[], false, NOW).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::PLAN_CHANGED.into()).to_string())
    );
    state::mark_capture_broken(&a, NOW).expect("broken");
    assert_eq!(
        rotate::plan(&a, &[C], &[], false, NOW).map_err(|e| e.to_string()),
        Err(AppError::Validation("validation.syncChangesPaused".into()).to_string())
    );
}

#[tokio::test]
async fn a_key_change_commits_everything_and_seals_the_key_to_the_others_only() {
    let three = three().await;
    // B's removal record reached A ahead of any key change: B still knows the key until this one.
    record_removal(&three.vault.a.lock(), A, B);
    let forgot = AtomicBool::new(false);
    let relay = &three.vault.relay;
    let consent = rotate::plan(&three.vault.a.lock(), &[C], &[], false, NOW)
        .expect("plan")
        .consent;

    let outcome = rotate::remove(
        relay,
        &three.vault.a,
        Removal {
            removing: &[C],
            new_kit: false,
            consent: &consent,
        },
        three.dir.path(),
        &|| {
            assert!(
                !relay.calls().contains(&Route::RotateToken),
                "before the call"
            );
            forgot.store(true, Ordering::SeqCst);
        },
        NOW,
    )
    .await
    .expect("removed");

    assert_eq!(
        outcome,
        Outcome::Changed {
            epoch: 1,
            removed: vec![C],
            kit: None
        }
    );
    assert!(
        forgot.load(Ordering::SeqCst),
        "the pairing in progress is given up"
    );
    let vault_id = three.vault.creds.vault_id.to_b64();
    let head = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    {
        let a = three.vault.a.lock();
        assert_eq!(key_epoch(&a), 1);
        assert_eq!(state::epochs(&a).expect("epochs").get(&1), Some(&head));
        assert!(!rotation_pending(&a));
        assert!(state::snapshot_epoch_owed(&a).expect("owed"));
        assert_eq!(state::rekey_seen(&a).expect("seen"), 1);
        let (record, removed) = removal_record(&a, C);
        assert!(record.is_some() && removed == 1);
        assert!(queued(&a, C));
        for (device, cut_off) in [(C, Some(1)), (B, Some(1))] {
            let entry = trust::entry(&a, device).expect("entry").expect("pinned");
            assert_eq!(entry.state, TrustState::Removed, "{device:?}");
            assert_eq!(entry.cut_off_epoch, cut_off, "{device:?}");
        }
    }
    // A gets the key; B (list-removed) and C (removed now) only their notices.
    let stored = three
        .vault
        .relay
        .stored_rekey(&vault_id, 1)
        .expect("the bundle");
    for (device, recipient) in [(A, true), (B, false), (C, false)] {
        let conn = three.device(device).lock();
        let secret = keys::device_box_secret(&conn)
            .expect("secret")
            .expect("box key");
        match bundle::open(&stored, &three.vault.creds.vault_id.0, &secret).expect("open") {
            Opened::Recipient(_) => assert!(recipient, "{device:?} got the key"),
            Opened::Notice { record, .. } => {
                assert!(!recipient, "{device:?}");
                assert_eq!(record.subject, device);
            }
            Opened::Nothing => panic!("{device:?} opened nothing"),
        }
    }
    // The next clean round uploads a snapshot under the new key (`snapshot_epoch_due`).
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(report.snapshot_uploaded, "{report:?}");
    assert!(!state::snapshot_epoch_owed(&three.vault.a.lock()).expect("owed"));
}

#[tokio::test]
async fn a_new_kit_with_nobody_removed_reaches_every_device() {
    let three = three().await;

    let outcome = new_kit(&three, A).await.expect("changed");

    assert!(
        matches!(&outcome, Outcome::Changed { epoch: 1, removed, kit: Some(_) } if removed.is_empty()),
        "{outcome:?}"
    );
    let stored = three
        .vault
        .relay
        .stored_rekey(&three.vault.creds.vault_id.to_b64(), 1)
        .expect("bundle");
    for device in [A, B, C] {
        let conn = three.device(device).lock();
        let secret = keys::device_box_secret(&conn)
            .expect("secret")
            .expect("box key");
        assert!(matches!(
            bundle::open(&stored, &three.vault.creds.vault_id.0, &secret).expect("open"),
            Opened::Recipient(_)
        ));
    }
}

#[tokio::test]
async fn a_lost_race_picks_the_other_change_up_and_commits_nothing_of_its_own() {
    let three = three().await;
    // B changes the key first, removing nobody; A's removal of C then meets a retired token.
    new_kit(&three, B).await.expect("B changes the key");
    // A's plan was made before it knew of B's change: refused as changed after the pickup.
    let error = remove(&three, A, &[C]).await.expect_err("raced");
    assert_eq!(
        validation_key(&error).as_deref(),
        Some(rotate::PLAN_CHANGED)
    );
    {
        let a = three.vault.a.lock();
        assert_eq!(key_epoch(&a), 1, "A picked B's change up first");
        assert!(!rotation_pending(&a));
        assert_eq!(removal_record(&a, C).0, None);
    }
}

/// A retired token at the call with no other key change to pick up: the person is told the key
/// changed, and nothing of the removal is committed.
#[tokio::test]
async fn a_retired_token_at_the_call_wipes_the_pending_change() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::TokenReadOnly);
    let error = remove(&three, A, &[C]).await.expect_err("raced");
    // No other change exists to pick up: the person is told the key changed.
    assert_eq!(
        validation_key(&error).as_deref(),
        Some("validation.syncKeyRotated")
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a), "the pending change is wiped");
    assert_eq!(key_epoch(&a), 0);
    assert_eq!(removal_record(&a, C).0, None);
}

/// The race of §10.1 when the vault check passed: the other device's change lands after it.
#[tokio::test]
async fn a_race_lost_at_the_call_names_the_winner_or_says_it_already_removed() {
    for b_removes_c in [false, true] {
        let three = three().await;
        let consent = rotate::plan(&three.vault.a.lock(), &[C], &[], false, NOW)
            .expect("plan")
            .consent;
        let relay = &three.vault.relay;
        // B's change lands while A stores its pending change (after A's vault check).
        let b_removing: &[DeviceId] = if b_removes_c { &[C] } else { &[] };
        // Removing nobody, B makes a new Kit.
        let b_new_kit = b_removing.is_empty();
        let b_consent = rotate::plan(&three.vault.b.lock(), b_removing, &[], b_new_kit, NOW)
            .expect("plan")
            .consent;
        let b_done = std::sync::Mutex::new(None);
        let outcome = rotate::remove(
            relay,
            &three.vault.a,
            Removal {
                removing: &[C],
                new_kit: false,
                consent: &consent,
            },
            three.dir.path(),
            &|| {
                // The fake answers at once and B's round lock is free: B's whole change runs
                // within this one poll.
                let done = rotate::remove(
                    relay,
                    &three.vault.b,
                    Removal {
                        removing: b_removing,
                        new_kit: b_new_kit,
                        consent: &b_consent,
                    },
                    three.dir.path(),
                    &|| {},
                    NOW,
                )
                .now_or_never()
                .expect("B's change completes at once");
                *b_done.lock().expect("lock") = Some(done.is_ok());
            },
            NOW,
        )
        .await;
        assert_eq!(
            *b_done.lock().expect("lock"),
            Some(true),
            "B changed the key"
        );
        if b_removes_c {
            assert_eq!(
                outcome.expect("already removed"),
                Outcome::AlreadyRemoved {
                    by: "iPhone".into()
                }
            );
        } else {
            let error = outcome.expect_err("raced");
            let SyncError::Local(app) = &error else {
                panic!("{error:?}");
            };
            let (key, params) = app.keyed().expect("keyed");
            assert_eq!(key, rotate::ROTATION_RACED);
            assert_eq!(params["name"], "iPhone");
        }
        let a = three.vault.a.lock();
        assert_eq!(key_epoch(&a), 1, "A holds B's key");
        assert!(!rotation_pending(&a));
    }
}

#[tokio::test]
async fn too_many_key_changes_and_a_refused_call_wipe_the_pending_change() {
    for (error, key) in [
        (
            RelayError::TooManyRotations,
            Some(rotate::TOO_MANY_KEY_CHANGES),
        ),
        (RelayError::BadRequest("invalid_rekey".into()), None),
    ] {
        let three = three().await;
        three.vault.relay.fail_next(Route::RotateToken, error);

        let error = remove(&three, A, &[C]).await.expect_err("refused");

        assert_eq!(validation_key(&error).as_deref(), key, "{error:?}");
        let a = three.vault.a.lock();
        assert!(!rotation_pending(&a));
        assert_eq!(key_epoch(&a), 0);
    }
}

/// §10.4: the call never made it (a crash before it, or the network): the change stays pending,
/// no round pushes meanwhile, and the next round sends it and commits.
#[tokio::test]
async fn a_change_that_never_reached_the_server_is_sent_again_and_commits() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));

    let outcome = remove(&three, A, &[C]).await.expect("pending");
    assert_eq!(outcome, Outcome::Pending { kit: None });
    assert!(rotation_pending(&three.vault.a.lock()));
    assert_eq!(key_epoch(&three.vault.a.lock()), 0);

    // While it cannot be sent, a round pushes nothing.
    institution(&three.vault.a.lock(), "a-while-pending");
    three.vault.relay.set_failing(
        Route::RotateToken,
        Some(RelayError::Network("offline".into())),
    );
    let from = three.vault.relay.calls().len();
    let failed = run_round(
        &three.vault.relay,
        &three.vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;
    assert!(failed.is_err(), "{failed:?}");
    let calls = three.vault.relay.calls()[from..].to_vec();
    assert!(!calls.contains(&Route::Push), "{calls:?}");
    assert!(!calls.contains(&Route::Pull), "{calls:?}");
    assert!(rotation_pending(&three.vault.a.lock()));

    three.vault.relay.set_failing(Route::RotateToken, None);
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 1);
    assert_eq!(
        rotations(&three),
        3,
        "two failed calls, then one that landed"
    );
    assert_eq!(
        trust::entry(&a, C).expect("entry").map(|e| e.state),
        Some(TrustState::Removed)
    );
}

/// §10.4: the call landed and its answer was lost: the next resolution reads the vault with the
/// new token and commits without a second call.
#[tokio::test]
async fn a_change_whose_answer_was_lost_commits_without_a_second_call() {
    let three = three().await;
    three.vault.relay.lose_next_answer(Route::RotateToken);

    let outcome = remove(&three, A, &[C]).await.expect("pending");
    assert_eq!(outcome, Outcome::Pending { kit: None });

    // A restart: the engine resolves it before anything else.
    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(
        resolved,
        Some(Outcome::Changed {
            epoch: 1,
            removed: vec![C],
            kit: None
        })
    );
    assert_eq!(rotations(&three), 1);
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 1);
    let head = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    assert_eq!(state::epochs(&a).expect("epochs").get(&1), Some(&head));
    assert_eq!(state::rekey_seen(&a).expect("seen"), 1);
}

/// Spec 2026-11-20 §2.3 step 2: the base of the range is the cursor the remover had when it sent
/// the change, kept in the pending change. A resolution after a lost answer commits with that base
/// even if the cursor moved on, and its range starts there.
#[tokio::test]
async fn a_lost_answer_commits_with_the_stored_base() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    institution(&three.vault.b.lock(), "b-while-away");
    round_of(&three.vault, &three.vault.b).await;
    let head = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    assert!(head > base);
    three.vault.relay.lose_next_answer(Route::RotateToken);

    let outcome = remove(&three, A, &[C]).await.expect("pending");
    assert_eq!(outcome, Outcome::Pending { kit: None });
    // The cursor moves on before the resolution (anything that pulls does it).
    state::set_cursor(&three.vault.a.lock(), head).expect("cursor");

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert!(
        matches!(resolved, Some(Outcome::Changed { epoch: 1, .. })),
        "{resolved:?}"
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(
        crate::services::sync::rekey::range::base_of(&a, 1).expect("base"),
        Some(base),
        "the range starts at the cursor of the first send"
    );
    let noted: Vec<u64> = a
        .prepare("SELECT pos FROM sync_range_notes WHERE epoch = 1 ORDER BY pos")
        .expect("notes")
        .query_map([], |r| r.get::<_, i64>(0).map(|p| p as u64))
        .expect("rows")
        .map(|p| p.expect("pos"))
        .collect();
    let expected: Vec<u64> = three
        .vault
        .relay
        .ops()
        .iter()
        .map(|op| op.seq)
        .filter(|seq| *seq > base && *seq <= head)
        .collect();
    assert_eq!(noted, expected);
    assert!(
        !head::own_unverified(&a).expect("own"),
        "signed in the resolution"
    );
}

/// §2.3 step 1: one new precondition, and only that one. A cursor below what the server still
/// keeps (its floor) means the ops between are gone: the removal is refused and nothing is
/// written. A vault snapshot above the cursor is no reason to refuse (the device being removed
/// could keep moving it).
#[tokio::test]
async fn a_removal_is_refused_only_when_the_cursor_is_below_the_floor() {
    let three = three().await;
    let base = cursor(&three.vault.a.lock());
    for id in ["b-1", "b-2", "b-3"] {
        institution(&three.vault.b.lock(), id);
        round_of(&three.vault, &three.vault.b).await;
    }
    let end = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    assert!(end > base + 2);
    three
        .vault
        .relay
        .put_snapshot(&three.vault.creds.auth(), end, vec![7; 64])
        .await
        .expect("a snapshot above A's cursor");

    // The floor still at 0: the snapshot alone does not stop the removal.
    let outcome = remove(&three, A, &[C]).await.expect("not refused");
    assert!(
        matches!(outcome, Outcome::Changed { epoch: 1, .. }),
        "{outcome:?}"
    );

    let three = self::three().await;
    let base = cursor(&three.vault.a.lock());
    for id in ["b-1", "b-2", "b-3"] {
        institution(&three.vault.b.lock(), id);
        round_of(&three.vault, &three.vault.b).await;
    }
    let end = three.vault.relay.ops().last().map_or(0, |op| op.seq);
    three
        .vault
        .relay
        .put_snapshot(&three.vault.creds.auth(), end, vec![7; 64])
        .await
        .expect("a snapshot");
    three.vault.relay.prune_to(end);
    assert!(base < end);

    let error = remove(&three, A, &[C]).await.expect_err("refused");

    assert_eq!(
        validation_key(&error).as_deref(),
        Some(rotate::REMOVAL_NEEDS_SYNC)
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 0);
    assert_eq!(rotations(&three), 0, "nothing was sent");
    assert_eq!(removal_record(&a, C).0, None);
}

/// §10.4 and §10.1: the call never landed and another device changed the key meanwhile: the
/// resolution sends it again, meets the retired token and picks the other change up.
#[tokio::test]
async fn a_pending_change_that_lost_a_race_meanwhile_resolves_as_raced() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    remove(&three, A, &[C]).await.expect("pending");
    new_kit(&three, B).await.expect("B changes the key");

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(
        resolved,
        Some(Outcome::Raced {
            by: Some("iPhone".into())
        })
    );
    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    assert_eq!(key_epoch(&a), 1, "B's key");
    assert_eq!(
        trust::entry(&a, C).expect("entry").map(|e| e.state),
        Some(TrustState::Trusted),
        "A's removal never happened"
    );
}

/// Revision §3.1: the remover's commit marks every recipient, itself included, `confirmed` (it
/// replaces the person-signed confirmation records of As built 33, which are no longer written).
#[tokio::test]
async fn a_key_change_confirms_every_recipient() {
    let three = three().await;
    for device in [A, B, C] {
        assert!(
            !trust::entry(&three.vault.a.lock(), device)
                .expect("entry")
                .expect("pinned")
                .confirmed
        );
    }

    new_kit(&three, A).await.expect("changed");

    let a = three.vault.a.lock();
    for device in [A, B, C] {
        assert!(
            trust::entry(&a, device)
                .expect("entry")
                .expect("pinned")
                .confirmed,
            "{device:?}"
        );
    }
    let records: i64 = a
        .query_row(
            "SELECT COUNT(*) FROM sync_devices WHERE confirm_record IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .expect("rows");
    assert_eq!(records, 0, "no confirmation record is written any more");
}

// ── Revision §2.5, §2.7, §4.2: the Kit in a removal ─────────────────────────────────────────

fn vault_b64(three: &Three) -> String {
    three.vault.creds.vault_id.to_b64()
}

/// The envelope the sync server holds for `secret`'s Kit, opened with it.
async fn kit_envelope(three: &Three, secret: &KitSecret) -> envelope::Envelope {
    let keys = kit_keys(three, secret);
    let read = three
        .vault
        .relay
        .get_kit(&vault_b64(three), keys.token())
        .await
        .expect("the Kit reads");
    envelope::open(&read.envelope, &keys).expect("the Kit opens it")
}

/// What `conn`'s store holds `confirmed` among its `trusted` entries.
fn confirmed_set(conn: &rusqlite::Connection) -> Vec<(DeviceId, [u8; 32])> {
    trust::entries(conn)
        .expect("entries")
        .into_iter()
        .filter(|e| e.state == TrustState::Trusted && e.confirmed)
        .map(|e| (e.device, e.cert.hash()))
        .collect()
}

/// §2.7's table: which removals replace the Kit, by where it is kept and who is removed.
#[tokio::test]
async fn the_plan_replaces_the_kit_as_the_table_says() {
    use KitStorage::{Cloud, Copied, File, Paper};
    let three = three().await;
    let secret = with_kit(&three, C, Some(Paper)).await;
    let kit = kit_keys(&three, &secret).id();
    let a = three.vault.a.lock();
    let cases = [
        (Some(Paper), C, ReplacesKit::No),
        (Some(Paper), B, ReplacesKit::No),
        (Some(Copied), C, ReplacesKit::CreatorRemoved),
        (Some(Copied), B, ReplacesKit::No),
        (Some(File), C, ReplacesKit::CreatorRemoved),
        (Some(File), B, ReplacesKit::No),
        (Some(Cloud), C, ReplacesKit::CreatorRemoved),
        (Some(Cloud), B, ReplacesKit::Cloud),
        (None, C, ReplacesKit::No),
        (None, B, ReplacesKit::No),
    ];
    for (storage, removing, expected) in cases {
        set_kit_storage(&a, kit, storage);
        let plan = rotate::plan(&a, &[removing], &[], false, NOW).expect("plan");
        assert_eq!(
            plan.replaces_kit, expected,
            "{storage:?}, removing {removing:?}"
        );
        assert_eq!(plan.kit.as_ref().map(|k| k.device), Some(kit));
        assert!(
            plan.recipients.iter().all(|e| e.device != kit),
            "the Kit is never a recipient"
        );
    }
    set_kit_storage(&a, kit, Some(Paper));
    assert_eq!(
        rotate::plan(&a, &[], &[], true, NOW)
            .expect("plan")
            .replaces_kit,
        ReplacesKit::Asked
    );
    // The Kit is no device to remove.
    assert_eq!(
        rotate::plan(&a, &[kit], &[], false, NOW).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::PLAN_CHANGED.into()).to_string())
    );
}

#[tokio::test]
async fn the_consent_changes_with_the_kit() {
    let three = three().await;
    let without = rotate::plan(&three.vault.a.lock(), &[C], &[], false, NOW)
        .expect("plan")
        .consent;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let a = three.vault.a.lock();

    let with = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
    set_kit_storage(&a, kit_keys(&three, &secret).id(), Some(KitStorage::Cloud));
    let replacing = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");

    assert_ne!(with.consent, without, "a Kit appeared");
    assert_eq!(replacing.replaces_kit, ReplacesKit::Cloud);
    assert_ne!(replacing.consent, with.consent, "the Kit is replaced now");
    let mut no_kit = with.clone();
    no_kit.kit = None;
    assert_ne!(rotate::consent(&no_kit), with.consent);
    let mut other_kit = with.clone();
    other_kit.kit = Some(with.recipients[0].clone());
    assert_ne!(
        rotate::consent(&other_kit),
        with.consent,
        "the Kit's certificate"
    );
}

/// A device of the test vault that only `A`'s list knows: its row and certificate.
fn row_on(conn: &rusqlite::Connection, cert: &Certificate) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, 'k', 'extra', 'macos', '1', ?2, ?3)",
        rusqlite::params![cert.body.device.to_hex(), NOW, cert.to_b64()],
    )
    .expect("row");
}

/// `subject` (seed byte `seed`) paired by `holder` (its signing seed): its certificate.
fn paired_by(
    vault: [u8; 16],
    holder: DeviceId,
    holder_seed: &SignSeed,
    subject: DeviceId,
    seed: &SignSeed,
) -> Certificate {
    let (secret, ticket) = cert::issue_ticket(holder_seed, vault, holder, NOW).expect("ticket");
    cert::issue_paired(
        &secret,
        &ticket,
        holder,
        &identity::sign_pub(holder_seed),
        &cert::CertFields {
            vault_id: vault,
            device: subject,
            box_pub: [subject.0[0]; 32],
            issued_at: NOW,
        },
        seed,
    )
    .expect("paired")
}

fn seed_of(byte: u8) -> SignSeed {
    SignSeed::from_bytes(Zeroizing::new([byte; 32]))
}

/// Revision §4.2 (security review HIGH 1): `added_by_removing` holds every descendant of the
/// removed devices, and the Kit's joiners only when the Kit is replaced.
#[tokio::test]
async fn added_by_removing_lists_grandchildren_and_kit_joiners_only_with_a_new_kit() {
    const D: DeviceId = DeviceId([0xd1; 16]);
    const E: DeviceId = DeviceId([0xe1; 16]);
    const K: DeviceId = DeviceId([0x0f; 16]);
    const F: DeviceId = DeviceId([0xf1; 16]);
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let kit = kit_keys(&three, &secret);
    let vault = three.vault.creds.vault_id.0;
    let b_seed = identity::own_seed(&three.vault.b.lock())
        .expect("seed")
        .expect("seed");
    let a = three.vault.a.lock();
    // B paired D, D paired E; K joined with the Kit and paired F.
    row_on(&a, &paired_by(vault, B, &b_seed, D, &seed_of(0xd1)));
    row_on(&a, &paired_by(vault, D, &seed_of(0xd1), E, &seed_of(0xe1)));
    let k_cert = cert::issue_kit_join(
        &kit,
        &cert::CertFields {
            vault_id: vault,
            device: K,
            box_pub: [0x0f; 32],
            issued_at: NOW,
        },
        &seed_of(0x0f),
    )
    .expect("Kit join");
    row_on(&a, &k_cert);
    row_on(&a, &paired_by(vault, K, &seed_of(0x0f), F, &seed_of(0xf1)));
    for _ in 0..3 {
        trust::refresh(&a, NOW).expect("refresh");
    }
    for device in [D, E, K, F] {
        assert_eq!(
            trust::entry(&a, device).expect("entry").map(|e| e.state),
            Some(TrustState::Trusted),
            "{device:?} pinned"
        );
    }

    let removing_b = rotate::plan(&a, &[B], &[], false, NOW).expect("plan");
    let ticked = rotate::plan(&a, &[B], &[D], false, NOW).expect("plan");
    let kept_kit = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");
    let new_kit = rotate::plan(&a, &[], &[], true, NOW).expect("plan");
    set_kit_storage(&a, kit.id(), Some(KitStorage::Cloud));
    let cloud = rotate::plan(&a, &[C], &[], false, NOW).expect("plan");

    assert_eq!(removing_b.added_by_removing, vec![D, E], "a grandchild too");
    assert_eq!(
        ticked.added_by_removing,
        vec![D, E],
        "a ticked one stays listed"
    );
    assert!(ticked.removing.contains(&D));
    assert_eq!(kept_kit.added_by_removing, Vec::<DeviceId>::new());
    assert_eq!(new_kit.added_by_removing, vec![K, F]);
    assert_eq!(cloud.replaces_kit, ReplacesKit::Cloud);
    assert_eq!(cloud.added_by_removing, vec![K, F]);
}

/// Revision §2.4, §2.5: every removal seals every key to the Kit, with the heads this device
/// verified (never the newest) and the confirmed set its store holds after the commit.
#[tokio::test]
async fn a_removal_seals_every_key_and_the_confirmed_set_to_the_kit() {
    let three = three().await;
    let secret = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let kit = kit_keys(&three, &secret).id();

    let outcome = remove(&three, A, &[C]).await.expect("removed");

    assert!(
        matches!(
            outcome,
            Outcome::Changed {
                epoch: 1,
                kit: None,
                ..
            }
        ),
        "{outcome:?}"
    );
    let opened = kit_envelope(&three, &secret).await;
    {
        let a = three.vault.a.lock();
        let creds = keys::credentials(&a).expect("creds").expect("bound");
        assert_eq!(opened.header.kit_id, kit);
        assert_eq!(opened.header.epoch, 1);
        assert_eq!(opened.by, A);
        let held: Vec<(u32, [u8; 32])> = creds.keys.epochs().map(|(e, k)| (e, *k)).collect();
        let sealed: Vec<(u32, [u8; 32])> = opened.keys.iter().map(|k| (k.epoch, *k.key)).collect();
        assert_eq!(sealed, held, "every key");
        let heads: Vec<Option<u64>> = opened.keys.iter().map(|k| k.head).collect();
        assert_eq!(
            heads,
            vec![Some(0), None],
            "the newest head is not known yet"
        );
        let confirmed: Vec<(DeviceId, [u8; 32])> = opened
            .confirmed
            .iter()
            .map(|c| (c.device, c.cert_hash))
            .collect();
        assert_eq!(confirmed, confirmed_set(&a), "the store's after the commit");
        let devices: Vec<DeviceId> = confirmed.iter().map(|(d, _)| *d).collect();
        assert!(devices.contains(&A) && devices.contains(&B) && devices.contains(&kit));
        assert!(!devices.contains(&C));
    }

    // A second removal: epoch 1's head, verified by its remover, travels now.
    round_of(&three.vault, &three.vault.a).await;
    remove(&three, A, &[B]).await.expect("removed");

    let opened = kit_envelope(&three, &secret).await;
    let a = three.vault.a.lock();
    let verified = head::verified_epochs(&a).expect("heads");
    assert_eq!(opened.header.epoch, 2);
    let heads: Vec<(u32, Option<u64>)> = opened.keys.iter().map(|k| (k.epoch, k.head)).collect();
    assert_eq!(
        heads,
        vec![(0, Some(0)), (1, verified.get(&1).copied()), (2, None)]
    );
    assert!(verified.contains_key(&1));
    let confirmed: Vec<(DeviceId, [u8; 32])> = opened
        .confirmed
        .iter()
        .map(|c| (c.device, c.cert_hash))
        .collect();
    assert_eq!(confirmed, confirmed_set(&a));
}

/// Revision §2.7: a new Kit is one key change; the old Kit's token is retired on the server
/// (`410`) and the new one opens the envelope.
#[tokio::test]
async fn a_new_kit_retires_the_old_token_and_the_new_one_opens() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;

    let outcome = new_kit(&three, A).await.expect("changed");

    let Outcome::Changed {
        epoch: 1,
        kit: Some(new),
        ..
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    let old_keys = kit_keys(&three, &old);
    let new_keys = kit_keys(&three, &new.secret);
    assert_eq!(new.id, new_keys.id());
    assert!(matches!(
        three
            .vault
            .relay
            .get_kit(&vault_b64(&three), old_keys.token())
            .await,
        Err(RelayError::KitReplaced { .. })
    ));
    let opened = kit_envelope(&three, &new.secret).await;
    assert_eq!(opened.header.kit_id, new.id);
    assert_eq!(opened.header.epoch, 1);
    let printed = format!("{new:?}");
    assert!(
        !printed.contains(&hex::encode(new.secret.expose())),
        "{printed}"
    );
}

/// Revision §2.5, §2.7: the commit confirms every recipient and the new Kit, writes the new
/// Kit's row and the old Kit's removal into its row (both sync), and pins the new Kit here
/// (As built R-28).
#[tokio::test]
async fn the_commit_writes_the_kit_rows_and_confirms_recipients_and_kit() {
    let three = three().await;
    // A, the creator, copied the code: removing A replaces the Kit.
    let old = with_kit(&three, A, Some(KitStorage::Copied)).await;
    let old_id = kit_keys(&three, &old).id();
    assert_eq!(
        rotate::plan(&three.vault.b.lock(), &[A], &[], false, NOW)
            .expect("plan")
            .replaces_kit,
        ReplacesKit::CreatorRemoved
    );

    let outcome = remove(&three, B, &[A]).await.expect("removed");

    let Outcome::Changed { kit: Some(new), .. } = outcome else {
        panic!("{outcome:?}");
    };
    let b = three.vault.b.lock();
    let (record, removed) = removal_record(&b, old_id);
    assert!(record.is_some() && removed == 1, "the old Kit's removal");
    assert!(queued(&b, old_id) && queued(&b, new.id), "both rows sync");
    let (platform, name, public_key, cert, storage): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = b
        .query_row(
            "SELECT platform, name, public_key, cert, kit_storage FROM sync_devices WHERE id = ?1",
            [new.id.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .expect("the new Kit's row");
    let new_keys = kit_keys(&three, &new.secret);
    assert_eq!(platform, kit::KIT_PLATFORM);
    assert_eq!(name, "");
    assert_eq!(
        public_key,
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            new_keys.box_pub()
        )
    );
    assert_eq!(storage, None, "not saved yet: \"neuložená\"");
    let cert = Certificate::from_b64(&cert).expect("cert");
    assert_eq!(cert.body.kind, CertKind::Kit);
    assert_eq!(cert.body.issuer_device, B);
    let entry = trust::entry(&b, new.id).expect("entry").expect("pinned");
    assert_eq!(entry.state, TrustState::Trusted);
    assert!(entry.confirmed);
    let old_entry = trust::entry(&b, old_id).expect("entry").expect("pinned");
    assert_eq!(old_entry.state, TrustState::Removed);
    assert_eq!(old_entry.cut_off_epoch, Some(1));
    for device in [B, C] {
        assert!(
            trust::entry(&b, device)
                .expect("entry")
                .expect("pinned")
                .confirmed,
            "{device:?}"
        );
    }
    assert_eq!(
        trust::recipients(&b, &[])
            .expect("recipients")
            .kit
            .map(|k| k.device),
        Some(new.id),
        "the next change seals to the new Kit"
    );
}

/// Revision §2.5, §2.7: a new Kit whose change is left pending is handed over at once; its Kit
/// part stays in the pending change and goes out unchanged when the change is sent again.
#[tokio::test]
async fn a_pending_new_kit_keeps_its_kit_part_and_sends_it_again() {
    let three = three().await;
    let old = with_kit(&three, A, Some(KitStorage::Paper)).await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));

    let outcome = new_kit(&three, A).await.expect("pending");

    let Outcome::Pending { kit: Some(new) } = outcome else {
        panic!("{outcome:?}");
    };
    let new_keys = kit_keys(&three, &new.secret);
    assert!(matches!(
        three
            .vault
            .relay
            .get_kit(&vault_b64(&three), new_keys.token())
            .await,
        Err(RelayError::Unauthorized)
    ));
    assert!(
        trust::entry(&three.vault.a.lock(), new.id)
            .expect("entry")
            .is_none(),
        "not pinned before it is valid"
    );

    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert!(
        matches!(
            resolved,
            Some(Outcome::Changed {
                epoch: 1,
                kit: None,
                ..
            })
        ),
        "{resolved:?}"
    );
    let opened = kit_envelope(&three, &new.secret).await;
    assert_eq!(opened.header.kit_id, new.id);
    assert!(matches!(
        three
            .vault
            .relay
            .get_kit(&vault_b64(&three), kit_keys(&three, &old).token())
            .await,
        Err(RelayError::KitReplaced { .. })
    ));
    let entry = trust::entry(&three.vault.a.lock(), new.id)
        .expect("entry")
        .expect("pinned at the commit");
    assert!(entry.confirmed);
}

/// Revision §2.7, §6.4: a new Kit whose change a background resolution finds lost says so.
#[tokio::test]
async fn a_new_kit_that_never_started_working_leaves_a_notice() {
    let three = three().await;
    with_kit(&three, A, Some(KitStorage::Paper)).await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    new_kit(&three, A).await.expect("pending");
    remove(&three, B, &[C]).await.expect("B removes C first");

    round_of(&three.vault, &three.vault.a).await;

    let a = three.vault.a.lock();
    assert!(!rotation_pending(&a));
    let notice = rekey::removal_not_done(&a)
        .expect("notice")
        .expect("written");
    assert!(notice.kit_replaced, "{notice:?}");
    assert_eq!(notice.by.as_deref(), Some("iPhone"));
}

// ---------------------------------------------------------------------------------------------
// What the wire shows (revision §6.5, Task 24): the plan with names, the list and the status after
// a removal, as `sync_removal_plan`, `sync_list_devices` and `get_sync_status` answer them.

#[tokio::test]
async fn the_plan_the_list_and_the_status_speak_in_names_across_a_removal() {
    use crate::models::sync::{
        ReplacesKit as ReplacesKitView, SyncDeviceStatus, SyncFormerReason, SyncJoinedVia,
        SyncRemovalNotice,
    };
    use crate::services::sync::devices::list_devices;
    use crate::services::sync::engine::status::{status_at, Runtime};
    use crate::services::sync::view;

    let three = three().await;
    {
        let a = three.vault.a.lock();
        let plan = rotate::plan(&a, &[C], &[], true, NOW).expect("plan");
        let shown = view::plan_view(&a, &plan, NOW).expect("view");
        let names = |devices: &[crate::models::sync::RemovalPlanDevice]| -> Vec<String> {
            devices.iter().map(|d| d.name.clone()).collect()
        };
        assert_eq!(names(&shown.removing), vec!["iPad"]);
        assert_eq!(names(&shown.staying), vec!["Mac", "iPhone"]);
        assert!(shown.staying[0].this_device);
        assert!(shown.dropping_out.is_empty());
        assert_eq!(shown.replaces_kit, ReplacesKitView::Asked);
        assert_eq!(shown.consent, plan.consent);

        let list = list_devices(&a, NOW).expect("list");
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|d| d.status == SyncDeviceStatus::Active));
        let phone = list.iter().find(|d| d.name == "iPhone").expect("phone");
        assert_eq!(phone.joined_via, SyncJoinedVia::Paired);
        assert_eq!(phone.added_by.as_deref(), Some("Mac"));
        assert!(
            !status_at(&a, &Runtime::default(), NOW)
                .expect("status")
                .removal_pending
        );
    }

    remove(&three, A, &[C]).await.expect("removal");
    settle(&three).await;

    // The remover lists the removed device as a former one, removed by it.
    let list = list_devices(&three.vault.a.lock(), NOW).expect("list");
    let ipad = list.iter().find(|d| d.name == "iPad").expect("iPad");
    assert_eq!(ipad.status, SyncDeviceStatus::Former);
    assert_eq!(ipad.former_reason, Some(SyncFormerReason::Removed));
    assert_eq!(ipad.state_by.as_deref(), Some("Mac"));
    // The other device that stayed learns who removed whom, and says so.
    let b = three.vault.b.lock();
    let status = status_at(&b, &Runtime::default(), NOW).expect("status");
    assert_eq!(
        status.removal_notice.as_ref().map(|n| (&n.by, &n.removed)),
        Some((&"Mac".to_string(), &vec!["iPad".to_string()]))
    );
    let _: Option<SyncRemovalNotice> = status.removal_notice;
    assert!(status.removal_supported);
    let list = list_devices(&b, NOW).expect("list");
    assert_eq!(
        list.iter().find(|d| d.name == "iPad").map(|d| d.status),
        Some(SyncDeviceStatus::Former)
    );
}
