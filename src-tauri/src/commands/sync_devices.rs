//! Removing devices from sync, keeping one, taking former ones off the list, checking the
//! recovery kit and dismissing notices
//! (phase 5 revision §4, §6.5). Thin: the flag, the input, the service in
//! `services::sync::{rekey::rotate, identity::trust, setup, view}`, its `SyncError` mapped for the
//! server. A removal is a sync change (`guarded_change`: the engine stops, the removal runs, the
//! engine starts again with a round that sends the records and the re-sealed changes) and needs
//! the device password. No command logs its arguments.

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use rusqlite::Connection;

use super::sync::{
    data_dir, engine_runtime, guarded_change, kit_owner, mapped_for_server, now, open_kit_draft,
    password_given, profile_relay, require_feature, stored_relay_url, verify_password, wake_engine,
    AppEngine, KitOwner,
};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{
    KitCheck, RecoveryKit, RemovalOutcome, RemovalOutcomeKind, RemovalPlan, SyncNoticeKind,
};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::trust;
use crate::services::sync::identity::KitSecret;
use crate::services::sync::rekey::kit::NewKit;
use crate::services::sync::rekey::rotate::{self, Outcome, Removal, ReplacesKit};
use crate::services::sync::{devices, join, keys, setup, view};

/// The plan changed meanwhile, or never made sense: the dialog reloads and asks again.
const PLAN_CHANGED: &str = rotate::PLAN_CHANGED;

/// A device ID the UI sent, as 32 lowercase hex characters; anything else is a plan that does not
/// hold (`validation.syncRemovalPlanChanged`).
fn parse_ids(ids: &[String]) -> Result<Vec<DeviceId>> {
    ids.iter()
        .map(|id| DeviceId::from_hex(id).map_err(|_| AppError::Validation(PLAN_CHANGED.into())))
        .collect()
}

/// The devices a removal takes: those asked for and the ticked ones, once each.
fn union(removing: &[DeviceId], also_remove: &[DeviceId]) -> Vec<DeviceId> {
    let mut all = [removing, also_remove].concat();
    all.sort();
    all.dedup();
    all
}

/// What removing `device_ids` (and the ticked `also_remove`, and a new recovery kit when
/// `new_kit`) would do, with the consent `sync_remove_devices` takes back. With `refresh` (the
/// dialog's first read, once) the engine is woken so the list is as new as it can be; the dialog's
/// ticks read without waking a round (review of Task 24, M1). The removal itself catches up
/// before it acts and refuses a changed plan (`validation.syncRemovalPlanChanged`), so the dialog
/// reloads.
#[tauri::command]
pub async fn sync_removal_plan(
    app: AppHandle,
    db: State<'_, Database>,
    device_ids: Vec<String>,
    also_remove: Vec<String>,
    new_kit: bool,
    refresh: bool,
) -> Result<RemovalPlan> {
    require_feature()?;
    let removing = parse_ids(&device_ids)?;
    let also_remove = parse_ids(&also_remove)?;
    if refresh {
        wake_engine(&app);
    }
    db.with_conn(|conn| {
        let at = now();
        let plan = rotate::plan(conn, &removing, &also_remove, new_kit, at)?;
        view::plan_view(conn, &plan, at)
    })
}

/// Removes the devices (and makes a new recovery kit when the plan says so) once the device
/// password is checked and within the plan the person confirmed (`consent`): a new vault key the
/// removed devices never get. The outcome says whether it is done, waits for the sync server
/// (`pending`), was done by another device first (`alreadyRemoved`) or removed this device
/// (`removedHere`); a new kit comes with it, once, and the copy window opens for it.
#[tauri::command]
pub async fn sync_remove_devices(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
    device_ids: Vec<String>,
    also_remove: Vec<String>,
    new_kit: bool,
    consent: String,
) -> Result<RemovalOutcome> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let removing = union(&parse_ids(&device_ids)?, &parse_ids(&also_remove)?);
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let owner = kit_owner(&app, &db)?;
    // Everything the new kit's view needs is checked before anything is sent: after the commit
    // the old kit is gone and the new one's seed lives only in memory. Any removal that makes a
    // kit, asked for or not (its creator or a cloud kit removed with it; final review M-2); a plan
    // that does not build is left to the removal, which refuses it the same way.
    let makes_kit = new_kit
        || db
            .with_conn(|conn| rotate::plan(conn, &removing, &[], false, now()))
            .is_ok_and(|plan| plan.replaces_kit != ReplacesKit::No);
    if makes_kit {
        db.with_conn(precheck_kit_view)?;
    }
    let removal = mapped_for_server(stored_relay_url(&db)?, async {
        rotate::remove(
            &relay,
            &session,
            Removal {
                removing: &removing,
                new_kit,
                consent: &consent,
            },
            &dir,
            &super::sync_pairing::forget_pairing,
            now(),
        )
        .await
    });
    let outcome = guarded_change(&AppEngine { app: &app, db: &db }, removal).await?;
    outcome_view(&db, owner, outcome)
}

/// A removal's outcome on the wire; a new kit is shown through the copy window. Infallible for a
/// committed removal: a kit that cannot be shown is logged and left out (`kit: None`), and the
/// status's missing kit leads to "Vytvořit novou sadu".
fn outcome_view(db: &Database, owner: KitOwner, outcome: Outcome) -> Result<RemovalOutcome> {
    let (kind, by, kit) = match outcome {
        Outcome::Changed { kit, .. } => (RemovalOutcomeKind::Done, None, kit),
        Outcome::Pending { kit } => (RemovalOutcomeKind::Pending, None, kit),
        Outcome::AlreadyRemoved { by } => (RemovalOutcomeKind::AlreadyRemoved, Some(by), None),
        Outcome::RemovedHere => (RemovalOutcomeKind::RemovedHere, None, None),
        // `remove` answers a lost race and a change it could not complete with errors.
        Outcome::Raced { .. } | Outcome::Stuck => {
            return Err(AppError::Internal(
                "sync: a removal ended in a state the command does not show".into(),
            ))
        }
    };
    Ok(RemovalOutcome {
        kind,
        by,
        kit: show_kit(db, owner, kit),
    })
}

/// The new kit's draft and its view (code and QR) from a stored vault; fails with the same
/// reasons whatever the seed is.
fn build_kit_view(conn: &Connection, kit: NewKit) -> Result<(setup::KitDraft, RecoveryKit)> {
    let vault_id = keys::credentials(conn)?
        .ok_or_else(|| AppError::Validation("validation.syncSetupIncomplete".into()))?
        .vault_id;
    let draft = setup::KitDraft::from_new_kit(vault_id, kit);
    let shown = setup::kit_view(conn, &draft)?;
    Ok((draft, shown))
}

/// Whether a kit's view can be built for this profile at all, tried with a throwaway seed before
/// a removal that makes a kit sends anything (review of Task 24, I3).
fn precheck_kit_view(conn: &Connection) -> Result<()> {
    let probe = NewKit {
        secret: KitSecret::generate()?,
        id: DeviceId([0; 16]),
        created_at: now(),
    };
    build_kit_view(conn, probe).map(|_| ())
}

/// The new kit as the person sees it, once: its draft goes into the process-global copy window
/// (copy, print and save work on it for ten minutes, `sync_copy_recovery_code` and friends).
/// Never fails: the removal is committed, so a problem is logged and the kit is not shown.
fn show_kit(db: &Database, owner: KitOwner, kit: Option<NewKit>) -> Option<RecoveryKit> {
    let kit = kit?;
    match db.with_conn(|conn| build_kit_view(conn, kit)) {
        Ok((draft, shown)) => {
            open_kit_draft(owner, draft);
            Some(shown)
        }
        Err(error) => {
            log::error!(
                "[SYNC] The new recovery kit could not be shown after the removal: {error}"
            );
            None
        }
    }
}

/// "Ověřit sadu": reads a typed code or a scanned QR and says whether it is this sync's current
/// kit. No password and no network; the secret is wiped before the answer.
#[tauri::command]
pub async fn sync_check_recovery_kit(db: State<'_, Database>, code: String) -> Result<KitCheck> {
    let code = Zeroizing::new(code);
    require_feature()?;
    let check = db.with_conn(|conn| setup::check_kit(conn, &code))?;
    Ok(view::kit_check_view(check))
}

/// "Je moje": keeps a device the list marks "Zkontrolujte" (it gets the next key and speaks
/// again), without a password. The next round brings the trust store up to date and owns what a
/// refresh can end here (a removal of this device): the command only marks and wakes the engine
/// (review of Task 24, I1). `false`: nothing to keep (already kept, this device, or an entry that
/// is not pinned, not trusted or gone).
#[tauri::command]
pub async fn sync_keep_device(
    app: AppHandle,
    db: State<'_, Database>,
    device_id: String,
) -> Result<bool> {
    require_feature()?;
    let device = parse_ids(&[device_id])?[0];
    let kept = db.with_conn(|conn| trust::keep(conn, device))?;
    if kept {
        wake_engine(&app);
    }
    Ok(kept)
}

/// "Odstranit ze seznamu" / "Odstranit vše": takes former devices off this device's list. No
/// password and no server: nothing about trust changes (their rows and trust entries stay, so
/// whatever they send later is still refused), and the list is this device's own. An ID that does
/// not read or is not a former device now is passed over.
#[tauri::command]
pub async fn sync_hide_former_devices(
    db: State<'_, Database>,
    device_ids: Vec<String>,
) -> Result<()> {
    require_feature()?;
    let devices: Vec<DeviceId> = device_ids
        .iter()
        .filter_map(|id| DeviceId::from_hex(id).ok())
        .collect();
    db.with_conn(|conn| devices::hide_former_devices(conn, &devices, now()))?;
    Ok(())
}

/// "Mám je" / "Odebrat je" at "Data jsou zpět": lifts the push hold of a device that joined with
/// the recovery kit, and the next round sends what waited. With `remove_them` the UI has removed
/// the listed devices first (`sync_remove_devices`) and calls this after it; while that removal
/// is pending or a listed device is still active it is `validation.syncRemovalPending`.
#[tauri::command]
pub async fn sync_kit_join_decided(
    app: AppHandle,
    db: State<'_, Database>,
    remove_them: bool,
) -> Result<()> {
    require_feature()?;
    db.with_conn(|conn| join::decide_kit_join(conn, remove_them))?;
    log::info!("[SYNC] The push hold after joining with the recovery kit was lifted (devices removed: {remove_them})");
    wake_engine(&app);
    Ok(())
}

/// "Zavřít", "Rozumím", "Vím o tom": dismisses one notice of the status. For `newDevices`,
/// `devices` are the IDs the person saw: only those entries go (`None`: the whole list).
#[tauri::command]
pub async fn sync_dismiss_notice(
    app: AppHandle,
    db: State<'_, Database>,
    kind: SyncNoticeKind,
    devices: Option<Vec<String>>,
) -> Result<()> {
    require_feature()?;
    let runtime = engine_runtime(&app);
    db.with_conn(|conn| view::dismiss_notice(conn, kind, devices.as_deref(), &runtime, now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_that_do_not_read_are_a_changed_plan() {
        let good = DeviceId([7; 16]).to_hex();
        assert_eq!(
            parse_ids(std::slice::from_ref(&good)).unwrap(),
            vec![DeviceId([7; 16])]
        );
        for bad in ["", "xyz", &good[..30]] {
            let error = parse_ids(&[bad.to_string()]).expect_err("refused");
            assert!(matches!(error, AppError::Validation(key) if key == PLAN_CHANGED));
        }
    }

    #[test]
    fn a_kit_that_cannot_be_shown_is_found_before_a_removal_sends_anything() {
        let conn = crate::services::sync::test_support::migrated();
        // No account and no vault yet: the view cannot be built, so the removal must not start.
        assert!(precheck_kit_view(&conn).is_err());
        keys::save_account(&conn, "https://sync.example.com", "1234567812345678").unwrap();
        keys::ensure_vault_secrets(&conn).unwrap();
        precheck_kit_view(&conn).expect("a vault with an account shows its kit");
        let kit = NewKit {
            secret: KitSecret::generate().unwrap(),
            id: DeviceId([1; 16]),
            created_at: 5,
        };
        let (draft, shown) = build_kit_view(&conn, kit).expect("view");
        assert_eq!(draft.created_at(), 5);
        assert_eq!(shown.created_at, 5);
        assert!(!shown.recovery_code.is_empty());
    }

    #[test]
    fn the_removed_devices_are_the_asked_and_the_ticked_once_each() {
        let (a, b, c) = (DeviceId([3; 16]), DeviceId([1; 16]), DeviceId([2; 16]));
        assert_eq!(union(&[a, b], &[b, c]), vec![b, c, a]);
        assert!(union(&[], &[]).is_empty());
    }
}
