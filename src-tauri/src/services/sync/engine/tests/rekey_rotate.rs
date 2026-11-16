//! Removing devices and changing the key on this device (phase 5 spec §6.4, §6.6, §10.4):
//! the plan and its consent, list-only removals, the key change and every answer the token call
//! can get, and a pending change resolved after a crash or a lost answer.

use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::FutureExt;

use super::rekey_fixture::*;
use super::*;
use crate::error::AppError;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::error::SyncError;
use crate::services::sync::identity::trust::{self, ExclusionReason, TrustState};
use crate::services::sync::keys;
use crate::services::sync::rekey::bundle::{self, Opened};
use crate::services::sync::rekey::rotate::{self, Outcome, Removal, RemovalMode};
use crate::services::sync::relay::fake::Route;
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

    let plan = rotate::plan(&a, &[C], RemovalMode::Rotate, &[]).expect("plan");

    let recipients: Vec<DeviceId> = plan.recipients.iter().map(|e| e.device).collect();
    assert_eq!(recipients, vec![A, B]);
    assert_eq!(plan.excluded, vec![(C, ExclusionReason::Removing)]);
    assert_eq!(plan.removing, vec![C]);
    assert_eq!(plan.epoch, 0);
    let other_mode = rotate::plan(&a, &[C], RemovalMode::ListOnly, &[]).expect("plan");
    let other_set = rotate::plan(&a, &[B, C], RemovalMode::Rotate, &[]).expect("plan");
    let nobody = rotate::plan(&a, &[], RemovalMode::Rotate, &[]).expect("plan");
    let consents = [
        &plan.consent,
        &other_mode.consent,
        &other_set.consent,
        &nobody.consent,
    ];
    for (i, x) in consents.iter().enumerate() {
        for y in &consents[i + 1..] {
            assert_ne!(x, y);
        }
    }
    assert_eq!(
        rotate::plan(&a, &[C], RemovalMode::Rotate, &[])
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
    let stale = rotate::plan(&three.vault.a.lock(), &[C], RemovalMode::Rotate, &[])
        .expect("plan")
        .consent;

    let error = rotate::remove(
        &three.vault.relay,
        &three.vault.a,
        Removal {
            mode: RemovalMode::Rotate,
            removing: &[B, C],
            opted_in: &[],
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
        rotate::plan(&old.a.lock(), &[B], RemovalMode::Rotate, &[]).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::REMOVAL_UNSUPPORTED.into()).to_string())
    );

    let three = three().await;
    let a = three.vault.a.lock();
    assert_eq!(
        rotate::plan(&a, &[A], RemovalMode::Rotate, &[]).map_err(|e| e.to_string()),
        Err(AppError::Validation(rotate::REMOVE_THIS_DEVICE.into()).to_string())
    );
    state::mark_capture_broken(&a, NOW).expect("broken");
    assert_eq!(
        rotate::plan(&a, &[C], RemovalMode::Rotate, &[]).map_err(|e| e.to_string()),
        Err(AppError::Validation("validation.syncChangesPaused".into()).to_string())
    );
}

#[tokio::test]
async fn a_list_only_removal_writes_records_and_calls_nobody() {
    let three = three().await;
    let calls = three.vault.relay.calls().len();
    let forgot = AtomicBool::new(false);
    let consent = rotate::plan(&three.vault.a.lock(), &[C], RemovalMode::ListOnly, &[])
        .expect("plan")
        .consent;

    let outcome = rotate::remove(
        &three.vault.relay,
        &three.vault.a,
        Removal {
            mode: RemovalMode::ListOnly,
            removing: &[C],
            opted_in: &[],
            consent: &consent,
        },
        three.dir.path(),
        &|| forgot.store(true, Ordering::SeqCst),
        NOW,
    )
    .await
    .expect("removed");

    assert_eq!(outcome, Outcome::Listed { removed: vec![C] });
    assert_eq!(three.vault.relay.calls().len(), calls, "no relay call");
    assert!(!forgot.load(Ordering::SeqCst), "a pairing is kept");
    let a = three.vault.a.lock();
    let (record, removed) = removal_record(&a, C);
    assert!(record.is_some());
    assert_eq!(removed, 1);
    assert!(queued(&a, C), "the record syncs");
    let entry = trust::entry(&a, C).expect("entry").expect("pinned");
    assert_eq!(entry.state, TrustState::Removed);
    assert_eq!(entry.cut_off_epoch, None, "it still knows the key");
    assert_eq!(key_epoch(&a), 0);
}

#[tokio::test]
async fn a_key_change_commits_everything_and_seals_the_key_to_the_others_only() {
    let three = three().await;
    // B was taken off the list earlier: it still knows the key until this change.
    remove(&three, A, &[B], RemovalMode::ListOnly)
        .await
        .expect("list-only");
    let forgot = AtomicBool::new(false);
    let relay = &three.vault.relay;
    let consent = rotate::plan(&three.vault.a.lock(), &[C], RemovalMode::Rotate, &[])
        .expect("plan")
        .consent;

    let outcome = rotate::remove(
        relay,
        &three.vault.a,
        Removal {
            mode: RemovalMode::Rotate,
            removing: &[C],
            opted_in: &[],
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
            removed: vec![C]
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
async fn a_key_change_with_nobody_removed_reaches_every_device() {
    let three = three().await;

    let outcome = remove(&three, A, &[], RemovalMode::Rotate)
        .await
        .expect("changed");

    assert_eq!(
        outcome,
        Outcome::Changed {
            epoch: 1,
            removed: vec![]
        }
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
    remove(&three, B, &[], RemovalMode::Rotate)
        .await
        .expect("B changes the key");
    // A's plan was made before it knew of B's change: refused as changed after the pickup.
    let error = remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect_err("raced");
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
    let error = remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect_err("raced");
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
        let consent = rotate::plan(&three.vault.a.lock(), &[C], RemovalMode::Rotate, &[])
            .expect("plan")
            .consent;
        let relay = &three.vault.relay;
        // B's change lands while A stores its pending change (after A's vault check).
        let b_removing: &[DeviceId] = if b_removes_c { &[C] } else { &[] };
        let b_consent = rotate::plan(&three.vault.b.lock(), b_removing, RemovalMode::Rotate, &[])
            .expect("plan")
            .consent;
        let b_done = std::sync::Mutex::new(None);
        let outcome = rotate::remove(
            relay,
            &three.vault.a,
            Removal {
                mode: RemovalMode::Rotate,
                removing: &[C],
                opted_in: &[],
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
                        mode: RemovalMode::Rotate,
                        removing: b_removing,
                        opted_in: &[],
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

        let error = remove(&three, A, &[C], RemovalMode::Rotate)
            .await
            .expect_err("refused");

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

    let outcome = remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect("pending");
    assert_eq!(outcome, Outcome::Pending);
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

    let outcome = remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect("pending");
    assert_eq!(outcome, Outcome::Pending);

    // A restart: the engine resolves it before anything else.
    let resolved = rotate::resolve_pending(&three.vault.relay, &three.vault.a, None, NOW)
        .await
        .expect("resolved");

    assert_eq!(
        resolved,
        Some(Outcome::Changed {
            epoch: 1,
            removed: vec![C]
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

/// §10.4 and §10.1: the call never landed and another device changed the key meanwhile: the
/// resolution sends it again, meets the retired token and picks the other change up.
#[tokio::test]
async fn a_pending_change_that_lost_a_race_meanwhile_resolves_as_raced() {
    let three = three().await;
    three
        .vault
        .relay
        .fail_next(Route::RotateToken, RelayError::Network("offline".into()));
    remove(&three, A, &[C], RemovalMode::Rotate)
        .await
        .expect("pending");
    remove(&three, B, &[], RemovalMode::Rotate)
        .await
        .expect("B changes the key");

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

/// The ticks of §9.2 are recipients, and the commit confirms them (As built 33).
#[tokio::test]
async fn a_ticked_kit_device_gets_the_key_and_is_confirmed() {
    let three = three().await;
    // On A, C is known only through a Recovery Kit vouching for it.
    three
        .vault
        .a
        .lock()
        .execute(
            "UPDATE sync_trust SET kit_unconfirmed = 1 WHERE device = ?1",
            [C.to_hex()],
        )
        .expect("kit");
    let unticked =
        rotate::plan(&three.vault.a.lock(), &[], RemovalMode::Rotate, &[]).expect("plan");
    assert!(unticked
        .excluded
        .contains(&(C, ExclusionReason::KitUnconfirmed)));
    let ticked = rotate::plan(&three.vault.a.lock(), &[], RemovalMode::Rotate, &[C]).expect("plan");
    assert_ne!(ticked.consent, unticked.consent);
    assert_eq!(ticked.confirming, vec![C]);

    rotate::remove(
        &three.vault.relay,
        &three.vault.a,
        Removal {
            mode: RemovalMode::Rotate,
            removing: &[],
            opted_in: &[C],
            consent: &ticked.consent,
        },
        three.dir.path(),
        &|| {},
        NOW,
    )
    .await
    .expect("changed");

    let a = three.vault.a.lock();
    let entry = trust::entry(&a, C).expect("entry").expect("pinned");
    assert!(entry.confirmed && !entry.kit_unconfirmed);
    let confirmed: Option<String> = a
        .query_row(
            "SELECT confirm_record FROM sync_devices WHERE id = ?1",
            [C.to_hex()],
            |r| r.get(0),
        )
        .expect("row");
    assert!(confirmed.is_some(), "the confirmation syncs");
}
