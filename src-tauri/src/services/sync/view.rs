//! What the UI is told about device sync beyond the engine's phase (revision §6.4, §6.5): the
//! status fields that read the trust store, the key-change notices and the recovery kit, a
//! removal plan and its outcome as wire types. Everything here is read-only except the dismissals;
//! the decisions live in `identity::trust`, `rekey` and `setup`.

use rusqlite::{Connection, OptionalExtension};

use super::engine::status::{self, Runtime};
use super::hlc::DeviceId;
use super::identity::trust::{self, ExclusionReason, TrustState};
use super::identity::CertKind;
use super::rekey::kit::{self, KitStorage};
use super::rekey::rotate::{self, Plan, ReplacesKit};
use super::rekey::{self, head};
use super::{devices, join, state};
use crate::error::Result;
use crate::models::sync::{
    KitCheck, KitCheckResult, RemovalDroppingOut, RemovalPlan, RemovalPlanDevice,
    SyncAfterKitDevice, SyncAfterKitJoin, SyncCheckReason, SyncKeyChangeOverrun, SyncKitInfo,
    SyncKitStorage, SyncNewDevice, SyncNoticeKind, SyncRemovalNotDone, SyncRemovalNotice,
    SyncRemovedNotice, SyncWaitingForRemover,
};
use crate::models::ReplacesKit as ReplacesKitView;
use crate::services::sync::setup;

/// A signed key change whose confirmation has not come for this long is "waiting for {by}".
pub const WAITING_AFTER_SECS: i64 = 86_400;

/// The status fields of an enabled profile that are read from its stores.
#[derive(Debug, Default)]
pub struct Extras {
    pub removal_supported: bool,
    pub removal_pending: bool,
    pub removal_notice: Option<SyncRemovalNotice>,
    pub removal_not_done: Option<SyncRemovalNotDone>,
    pub key_change_overrun: Option<SyncKeyChangeOverrun>,
    pub rejoin_by: Option<String>,
    pub waiting_for_remover: Option<SyncWaitingForRemover>,
    pub new_devices: Vec<SyncNewDevice>,
    pub recovery_kit: Option<SyncKitInfo>,
    pub kit_used: Option<i64>,
    pub after_kit_join: Option<SyncAfterKitJoin>,
    pub syncing_since: Option<i64>,
    pub first_upload: bool,
}

/// Why sync is off on this device, when another device removed it and the person has not
/// dismissed it. Readable with sync off.
pub fn removed_notice(conn: &Connection) -> Result<Option<SyncRemovedNotice>> {
    Ok(rekey::removed_notice(conn)?.map(|n| SyncRemovedNotice {
        by: n.by,
        at: n.at,
        unsent: n.unsent,
        left: n.left,
    }))
}

fn name_of_hex(conn: &Connection, hex: &str) -> Result<Option<String>> {
    match DeviceId::from_hex(hex) {
        Ok(device) => Ok(Some(rekey::device_name(conn, device)?)),
        Err(_) => Ok(None),
    }
}

/// When this device adopted `epoch` (unix seconds), if it recorded it.
fn adopted_at(conn: &Connection, epoch: u32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT adopted_at FROM sync_epochs WHERE epoch = ?1",
            [epoch],
            |r| r.get(0),
        )
        .optional()?)
}

fn kit_storage_view(storage: KitStorage) -> SyncKitStorage {
    match storage {
        KitStorage::Paper => SyncKitStorage::Paper,
        KitStorage::Copied => SyncKitStorage::Copied,
        KitStorage::File => SyncKitStorage::File,
        KitStorage::Cloud => SyncKitStorage::Cloud,
    }
}

/// The notice of who made a key change this device could not verify (`rejoinBy`).
fn rejoin_by(conn: &Connection) -> Result<Option<String>> {
    let Some(value) = state::config_text(conn, state::KEY_CHANGE_UNVERIFIED_KEY)? else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<serde_json::Value>(&value)
        .ok()
        .and_then(|v| v.get("by").and_then(|by| by.as_str().map(str::to_owned))))
}

/// The status fields of an enabled profile (`Extras`), at `now`.
pub fn extras(conn: &Connection, runtime: &Runtime, now: i64) -> Result<Extras> {
    let removal_notice = rekey::key_change_notice(conn)?.map(|n| SyncRemovalNotice {
        by: n.by,
        at: n.at,
        removed: n.removed,
        kit_replaced: n.kit_replaced,
    });
    let removal_not_done = rekey::removal_not_done(conn)?.map(|n| SyncRemovalNotDone {
        by: n.by,
        devices: n.devices,
        already_removed: n.already_removed,
        kit_replaced: n.kit_replaced,
    });
    let key_change_overrun = match rekey::key_change_overrun(conn)? {
        Some(overrun) => Some(SyncKeyChangeOverrun {
            at: adopted_at(conn, overrun.epoch)?,
        }),
        None => None,
    };
    // The oldest signed head that has not come for a day (a refuted one is a different state:
    // `keyChangeUnverified`).
    let mut waiting: Option<(i64, String)> = None;
    for epoch in head::unverified_epochs(conn)? {
        if epoch.refuted {
            continue;
        }
        let Some(since) = adopted_at(conn, epoch.epoch)? else {
            continue;
        };
        if now.saturating_sub(since) < WAITING_AFTER_SECS {
            continue;
        }
        if waiting.as_ref().is_none_or(|(oldest, _)| since < *oldest) {
            if let Some(by) = name_of_hex(conn, &epoch.by)? {
                waiting = Some((since, by));
            }
        }
    }
    let waiting_for_remover = waiting.map(|(since, by)| SyncWaitingForRemover { by, since });

    let notices = trust::new_devices(conn)?;
    let mut new_devices = Vec::new();
    if !notices.is_empty() {
        let late: Vec<DeviceId> = match trust::recipients(conn, &[]) {
            Ok(r) => r
                .excluded
                .into_iter()
                .filter(|(_, reason)| *reason == ExclusionReason::LateVouch)
                .map(|(device, _)| device)
                .collect(),
            Err(error) => {
                // The status still shows the list; only the "left out until kept" mark is lost.
                log::warn!(
                    "[SYNC] The recipients could not be read for the new-device list: {error}"
                );
                Vec::new()
            }
        };
        for notice in notices {
            let Ok(device) = DeviceId::from_hex(&notice.device) else {
                continue;
            };
            // A device that is gone (removed, left) or never pinned is no news any more.
            let Some(entry) = trust::entry(conn, device)? else {
                continue;
            };
            if entry.state != TrustState::Trusted {
                continue;
            }
            new_devices.push(SyncNewDevice {
                name: rekey::device_name(conn, device)?,
                by: if notice.via_kit || entry.kind == CertKind::RecoveryKit {
                    String::new()
                } else {
                    name_of_hex(conn, &notice.by)?.unwrap_or_default()
                },
                at: notice.at,
                via_kit: notice.via_kit,
                late_vouch: late.contains(&device),
                id: notice.device,
            });
        }
    }

    let recovery_kit = match kit::current(conn)? {
        Some(current) => Some(SyncKitInfo {
            created_at: current.created_at,
            created_by: devices::device_row(conn, current.created_by)?.map(|(name, _)| name),
            storage: current.storage.map(kit_storage_view),
            last_read_at: runtime.kit_last_read,
        }),
        None => None,
    };
    let after_kit_join = match join::after_kit_join(conn)? {
        Some(hold) => {
            let mut devices = Vec::new();
            for hex in &hold.devices {
                if let Some(name) = name_of_hex(conn, hex)? {
                    devices.push(SyncAfterKitDevice {
                        id: hex.clone(),
                        name,
                    });
                }
            }
            Some(SyncAfterKitJoin {
                devices,
                newest_change_at: hold.newest_change_at,
            })
        }
        None => None,
    };
    Ok(Extras {
        removal_supported: trust::is_supported(conn)?,
        removal_pending: rotate::pending(conn)?,
        removal_notice,
        removal_not_done,
        key_change_overrun,
        rejoin_by: rejoin_by(conn)?,
        waiting_for_remover,
        new_devices,
        recovery_kit,
        kit_used: status::kit_used(conn, runtime.kit_last_read, now)?,
        after_kit_join,
        syncing_since: if runtime.running_round {
            runtime.round_started_at
        } else {
            None
        },
        first_upload: state::snapshot_seq(conn)?.is_none()
            && state::marks(conn)?.is_some_and(|marks| marks.snapshot_uploaded_at.is_none()),
    })
}

/// "Zavřít", "Rozumím", "Vím o tom": dismisses a notice. Each is a Local `app_config` key (the
/// new-device list and the Kit read mark included); `kit_read` is the minute `kitUsed` shows.
/// `devices` (the IDs of `newDevices` the person saw) limits the new-device dismissal to those
/// entries; `None` clears the list. Other kinds ignore it.
pub fn dismiss_notice(
    conn: &Connection,
    kind: SyncNoticeKind,
    devices: Option<&[String]>,
    runtime: &Runtime,
    now: i64,
) -> Result<()> {
    let delete = |key: &str| -> Result<()> {
        conn.execute("DELETE FROM app_config WHERE key = ?1", [key])?;
        Ok(())
    };
    match kind {
        SyncNoticeKind::Removal => delete(state::KEY_CHANGE_NOTICE_KEY),
        SyncNoticeKind::Removed => delete(state::REMOVED_NOTICE_KEY),
        SyncNoticeKind::RemovalNotDone => delete(state::REMOVAL_NOT_DONE_KEY),
        SyncNoticeKind::KeyChangeOverrun => delete(state::KEY_CHANGE_OVERRUN_KEY),
        SyncNoticeKind::NewDevices => trust::dismiss_new_devices(conn, devices),
        SyncNoticeKind::KitUsed => {
            if let Some(read) = status::kit_used(conn, runtime.kit_last_read, now)? {
                status::dismiss_kit_used(conn, read)?;
            }
            Ok(())
        }
    }
}

fn plan_device(
    conn: &Connection,
    device: DeviceId,
    this: DeviceId,
) -> Result<Option<RemovalPlanDevice>> {
    let row: Option<(String, String, i64)> = conn
        .query_row(
            "SELECT name, platform, joined_at FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(name, platform, joined_at)| RemovalPlanDevice {
        id: device.to_hex(),
        name,
        platform,
        joined_at,
        this_device: device == this,
    }))
}

fn replaces_kit_view(replaces: ReplacesKit) -> ReplacesKitView {
    match replaces {
        ReplacesKit::No => ReplacesKitView::No,
        ReplacesKit::CreatorRemoved => ReplacesKitView::CreatorRemoved,
        ReplacesKit::Cloud => ReplacesKitView::Cloud,
        ReplacesKit::Asked => ReplacesKitView::Asked,
        ReplacesKit::Missing => ReplacesKitView::Missing,
    }
}

/// A removal plan as the dialog shows it: the devices (with the names, platforms and dates the
/// list has) that go, that stay, that drop out with them and why (only rows the list shows as
/// active or "Zkontrolujte"), and the ones a removed device added. A device the list does not
/// know is left out of the lists (it has no name to show).
pub fn plan_view(conn: &Connection, plan: &Plan, now: i64) -> Result<RemovalPlan> {
    let this = state::load(conn)?
        .map(|sync| sync.device)
        .unwrap_or(DeviceId([0; 16]));
    let listed = devices::list_devices(conn, now)?;
    let shown = |device: &DeviceId| {
        listed
            .iter()
            .find(|d| d.id == device.to_hex())
            .map(|d| d.status)
    };
    let mut removing = Vec::new();
    for device in &plan.removing {
        removing.extend(plan_device(conn, *device, this)?);
    }
    let mut staying = Vec::new();
    for entry in &plan.recipients {
        staying.extend(plan_device(conn, entry.device, this)?);
    }
    let mut dropping_out = Vec::new();
    for (device, reason) in &plan.excluded {
        let reason = match reason {
            ExclusionReason::IssuerRemoved => SyncCheckReason::IssuerRemoved,
            ExclusionReason::LateVouch => SyncCheckReason::LateVouch,
            ExclusionReason::IssuerUnknown
            | ExclusionReason::Unverified
            | ExclusionReason::NoKey => SyncCheckReason::Unverified,
            // Gone already, being removed now, or not shown at all.
            ExclusionReason::Removing
            | ExclusionReason::Left
            | ExclusionReason::Removed
            | ExclusionReason::SupersededQuiet => continue,
        };
        if shown(device).is_none_or(|status| status == crate::models::SyncDeviceStatus::Former) {
            continue;
        }
        if let Some(device) = plan_device(conn, *device, this)? {
            dropping_out.push(RemovalDroppingOut { device, reason });
        }
    }
    let mut added = Vec::new();
    for device in &plan.added_by_removing {
        added.extend(plan_device(conn, *device, this)?);
    }
    Ok(RemovalPlan {
        removing,
        staying,
        dropping_out,
        added_by_removing: added,
        replaces_kit: replaces_kit_view(plan.replaces_kit),
        kit_not_updated: plan.kit_not_updated,
        consent: plan.consent.clone(),
    })
}

/// "Ověřit sadu"'s answer as the wire type.
pub fn kit_check_view(check: setup::KitCheck) -> KitCheck {
    match check {
        setup::KitCheck::Valid => KitCheck {
            result: KitCheckResult::Valid,
            replaced_at: None,
            current_created_at: None,
        },
        setup::KitCheck::Replaced {
            at,
            current_created_at,
        } => KitCheck {
            result: KitCheckResult::Replaced,
            replaced_at: at,
            current_created_at,
        },
        setup::KitCheck::OtherSync => KitCheck {
            result: KitCheckResult::OtherSync,
            replaced_at: None,
            current_created_at: None,
        },
        setup::KitCheck::Unknown => KitCheck {
            result: KitCheckResult::Unknown,
            replaced_at: None,
            current_created_at: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::models::sync::{SyncPhase, SyncProblem};
    use crate::services::sync::engine::status::{status_at, Runtime};
    use crate::services::sync::identity::cert::{self, CertFields, CertKind, TicketSecret};
    use crate::services::sync::identity::{sign_pub, KitSecret, SignSeed};
    use crate::services::sync::keys::VaultId;
    use crate::services::sync::rekey::{
        write_key_change_notice, write_key_change_overrun, write_removal_not_done, KeyChangeNotice,
        KeyChangeOverrun, RemovalNotDone,
    };
    use crate::services::sync::test_support::migrated;
    use rusqlite::params;
    use zeroize::Zeroizing;

    const VAULT: [u8; 16] = [0x11; 16];
    const NOW: i64 = 1_794_816_000;
    const DAY: i64 = 86_400;
    const ME: DeviceId = DeviceId([1; 16]);
    const B: DeviceId = DeviceId([2; 16]);

    fn seed(device: DeviceId) -> SignSeed {
        SignSeed::from_bytes(Zeroizing::new([device.0[0]; 32]))
    }

    fn fields(device: DeviceId) -> CertFields {
        CertFields {
            vault_id: VAULT,
            device,
            box_pub: [device.0[0] ^ 0xb0; 32],
            issued_at: NOW,
        }
    }

    /// An enabled profile of `ME` with its genesis pinned and rows for `ME` and `B` (paired).
    fn enabled() -> Connection {
        let conn = migrated();
        state::enable(&conn, ME).unwrap();
        conn.execute(
            "UPDATE sync_state SET vault_id = ?1",
            [VaultId(VAULT).to_b64()],
        )
        .unwrap();
        let genesis = cert::issue_self(CertKind::Genesis, &fields(ME), &seed(ME)).unwrap();
        trust::seed_own(&conn, &genesis, NOW).unwrap();
        insert_row(&conn, ME, "MacBook", Some(&genesis));
        let (secret, ticket) = cert::ticket_from_secret(
            TicketSecret::from_bytes(Zeroizing::new([0x70; 32])),
            &seed(ME),
            VAULT,
            ME,
            NOW,
        );
        let paired = cert::issue_paired(
            &secret,
            &ticket,
            ME,
            &sign_pub(&seed(ME)),
            &fields(B),
            &seed(B),
        )
        .unwrap();
        insert_row(&conn, B, "iPhone", Some(&paired));
        trust::refresh(&conn, NOW).unwrap();
        // Pinning B wrote a new-device notice; the tests start without news.
        conn.execute(
            "DELETE FROM app_config WHERE key = ?1",
            [trust::NEW_DEVICES_KEY],
        )
        .unwrap();
        conn
    }

    fn insert_row(
        conn: &Connection,
        device: DeviceId,
        name: &str,
        cert: Option<&crate::services::sync::identity::Certificate>,
    ) {
        conn.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
             VALUES (?1, 'PK', ?2, 'macos', '1.0', 1, ?3)",
            params![device.to_hex(), name, cert.map(|c| c.to_b64())],
        )
        .unwrap();
    }

    fn status(conn: &Connection, runtime: &Runtime) -> crate::models::SyncStatus {
        status_at(conn, runtime, NOW).unwrap()
    }

    #[test]
    fn why_sync_is_off_shows_while_sync_is_off() {
        let conn = migrated();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)",
            params![
                state::REMOVED_NOTICE_KEY,
                r#"{"by":"iPhone","at":1790000000,"unsent":3,"left":false}"#
            ],
        )
        .unwrap();

        let shown = status(&conn, &Runtime::default());

        assert_eq!(shown.phase, SyncPhase::Off);
        assert_eq!(
            shown.removed_notice,
            Some(SyncRemovedNotice {
                by: "iPhone".into(),
                at: 1_790_000_000,
                unsent: 3,
                left: false
            })
        );
        assert!(!shown.removal_supported);

        dismiss_notice(
            &conn,
            SyncNoticeKind::Removed,
            None,
            &Runtime::default(),
            NOW,
        )
        .unwrap();
        assert_eq!(status(&conn, &Runtime::default()).removed_notice, None);
    }

    #[test]
    fn a_phase_5_vault_supports_removal_and_a_plain_status_is_quiet() {
        let conn = enabled();

        let shown = status(&conn, &Runtime::default());

        assert!(shown.removal_supported);
        assert!(!shown.removal_pending);
        assert_eq!(shown.removal_notice, None);
        assert_eq!(shown.removal_not_done, None);
        assert_eq!(shown.key_change_overrun, None);
        assert_eq!(shown.rejoin_by, None);
        assert_eq!(shown.waiting_for_remover, None);
        assert!(shown.new_devices.is_empty());
        assert_eq!(shown.recovery_kit, None);
        assert_eq!(shown.kit_used, None);
        assert_eq!(shown.after_kit_join, None);
        assert_eq!(shown.syncing_since, None);
        assert!(shown.first_upload, "no snapshot uploaded or restored yet");
        conn.execute("UPDATE sync_state SET snapshot_seq = 4", [])
            .unwrap();
        conn.execute("UPDATE sync_state SET snapshot_uploaded_at = 5", [])
            .unwrap();
        assert!(!status(&conn, &Runtime::default()).first_upload);
    }

    #[test]
    fn the_notices_read_and_dismiss_one_by_one() {
        let conn = enabled();
        write_key_change_notice(
            &conn,
            &KeyChangeNotice {
                by: "iPhone".into(),
                at: NOW - 60,
                removed: vec!["iPad".into()],
                kit_replaced: true,
            },
        )
        .unwrap();
        write_removal_not_done(
            &conn,
            &RemovalNotDone {
                by: Some("iPhone".into()),
                devices: vec!["iPad".into()],
                already_removed: false,
                kit_replaced: false,
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sync_epochs (epoch, rotation_head, adopted_at) VALUES (1, 10, ?1)",
            [NOW - 3600],
        )
        .unwrap();
        write_key_change_overrun(
            &conn,
            &KeyChangeOverrun {
                epoch: 1,
                head: 10,
                through: 12,
            },
        )
        .unwrap();

        let shown = status(&conn, &Runtime::default());

        assert_eq!(
            shown.removal_notice,
            Some(SyncRemovalNotice {
                by: "iPhone".into(),
                at: NOW - 60,
                removed: vec!["iPad".into()],
                kit_replaced: true
            })
        );
        assert_eq!(
            shown.removal_not_done.as_ref().map(|n| n.devices.clone()),
            Some(vec!["iPad".to_string()])
        );
        assert_eq!(
            shown.key_change_overrun,
            Some(SyncKeyChangeOverrun {
                at: Some(NOW - 3600)
            })
        );

        for kind in [
            SyncNoticeKind::Removal,
            SyncNoticeKind::RemovalNotDone,
            SyncNoticeKind::KeyChangeOverrun,
        ] {
            dismiss_notice(&conn, kind, None, &Runtime::default(), NOW).unwrap();
        }
        let shown = status(&conn, &Runtime::default());
        assert_eq!(shown.removal_notice, None);
        assert_eq!(shown.removal_not_done, None);
        assert_eq!(shown.key_change_overrun, None);
    }

    #[test]
    fn who_made_a_key_change_this_device_cannot_verify_is_shown() {
        let conn = enabled();
        state::set_config_text(
            &conn,
            state::KEY_CHANGE_UNVERIFIED_KEY,
            Some(r#"{"by":"Stranger"}"#),
        )
        .unwrap();

        assert_eq!(
            status(&conn, &Runtime::default()).rejoin_by.as_deref(),
            Some("Stranger")
        );
        // Not a JSON object with a name: nothing to show.
        state::set_config_text(&conn, state::KEY_CHANGE_UNVERIFIED_KEY, Some("junk")).unwrap();
        assert_eq!(status(&conn, &Runtime::default()).rejoin_by, None);
    }

    #[test]
    fn a_head_that_has_not_come_for_a_day_is_waiting_for_the_remover() {
        let conn = enabled();
        // Adopted 25 hours ago from B; its signed head has not been seen.
        state::record_epoch(&conn, 1, 10, Some(B), NOW - 25 * 3600).unwrap();
        head::note_unverified(&conn, 1, Some(10), Some(1), B, 4).unwrap();

        let shown = status(&conn, &Runtime::default());

        assert_eq!(
            shown.waiting_for_remover,
            Some(SyncWaitingForRemover {
                by: "iPhone".into(),
                since: NOW - 25 * 3600
            })
        );
        // Younger than a day: nothing yet.
        conn.execute("UPDATE sync_epochs SET adopted_at = ?1", [NOW - 3600])
            .unwrap();
        assert_eq!(status(&conn, &Runtime::default()).waiting_for_remover, None);
    }

    #[test]
    fn a_device_pinned_outside_a_join_is_news_until_dismissed() {
        let conn = enabled();
        let json = serde_json::json!([
            {"device": B.to_hex(), "by": ME.to_hex(), "at": NOW - 10, "viaKit": false},
            {"device": DeviceId([9; 16]).to_hex(), "by": ME.to_hex(), "at": NOW - 5, "viaKit": false}
        ]);
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)",
            params![trust::NEW_DEVICES_KEY, json.to_string()],
        )
        .unwrap();

        let shown = status(&conn, &Runtime::default());

        assert_eq!(
            shown.new_devices,
            vec![SyncNewDevice {
                id: B.to_hex(),
                name: "iPhone".into(),
                by: "MacBook".into(),
                at: NOW - 10,
                via_kit: false,
                late_vouch: false
            }],
            "a device this store never pinned is no news"
        );
        // Only what the person saw goes: a device noticed after stays.
        dismiss_notice(
            &conn,
            SyncNoticeKind::NewDevices,
            Some(&[DeviceId([9; 16]).to_hex()]),
            &Runtime::default(),
            NOW,
        )
        .unwrap();
        assert_eq!(status(&conn, &Runtime::default()).new_devices.len(), 1);
        dismiss_notice(
            &conn,
            SyncNoticeKind::NewDevices,
            Some(&[B.to_hex()]),
            &Runtime::default(),
            NOW,
        )
        .unwrap();
        assert!(trust::new_devices(&conn).unwrap().is_empty());
    }

    #[test]
    fn dismissing_new_devices_without_ids_clears_the_list() {
        let conn = enabled();
        let json = serde_json::json!([
            {"device": B.to_hex(), "by": ME.to_hex(), "at": NOW - 10, "viaKit": false}
        ]);
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)",
            params![trust::NEW_DEVICES_KEY, json.to_string()],
        )
        .unwrap();
        dismiss_notice(
            &conn,
            SyncNoticeKind::NewDevices,
            None,
            &Runtime::default(),
            NOW,
        )
        .unwrap();
        assert!(trust::new_devices(&conn).unwrap().is_empty());
    }

    #[test]
    fn the_recovery_kit_is_the_one_this_device_can_seal_to() {
        let conn = enabled();
        let keys = KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&VAULT);
        let kit = cert::issue_kit(&seed(ME), ME, &keys, NOW - DAY).unwrap();
        trust::pin_created_kit(&conn, &kit, NOW).unwrap();
        let runtime = Runtime {
            kit_last_read: Some(NOW - 10 * DAY),
            ..Runtime::default()
        };

        // The row is not written yet ("neuložená"); the status still names the Kit.
        let shown = status(&conn, &runtime);
        assert_eq!(
            shown.recovery_kit,
            Some(SyncKitInfo {
                created_at: NOW - DAY,
                created_by: Some("MacBook".into()),
                storage: None,
                last_read_at: Some(NOW - 10 * DAY),
            })
        );

        kit::write_row(&conn, &kit, Some(KitStorage::Cloud), NOW).unwrap();
        let shown = status(&conn, &runtime);
        assert_eq!(
            shown.recovery_kit.and_then(|k| k.storage),
            Some(SyncKitStorage::Cloud)
        );
        // A read nobody explains is news; "Vím o tom" quiets it.
        assert_eq!(shown.kit_used, Some(NOW - 10 * DAY));
        dismiss_notice(&conn, SyncNoticeKind::KitUsed, None, &runtime, NOW).unwrap();
        assert_eq!(status(&conn, &runtime).kit_used, None);
    }

    #[test]
    fn the_hold_after_a_kit_join_lists_the_devices_by_name() {
        let conn = enabled();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)",
            params![
                state::AFTER_KIT_JOIN_KEY,
                serde_json::json!({"devices": [B.to_hex(), "zz"], "newestChangeAt": NOW - 5})
                    .to_string()
            ],
        )
        .unwrap();

        assert_eq!(
            status(&conn, &Runtime::default()).after_kit_join,
            Some(SyncAfterKitJoin {
                devices: vec![SyncAfterKitDevice {
                    id: B.to_hex(),
                    name: "iPhone".into()
                }],
                newest_change_at: Some(NOW - 5)
            })
        );
    }

    #[test]
    fn the_hold_after_a_kit_join_is_lifted_only_when_the_removal_is_done() {
        let hold = |conn: &Connection| {
            conn.execute(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES (?1, ?2)",
                params![
                    state::AFTER_KIT_JOIN_KEY,
                    serde_json::json!({"devices": [B.to_hex()], "newestChangeAt": null})
                        .to_string()
                ],
            )
            .unwrap();
        };
        let held = |conn: &Connection| join::after_kit_join(conn).unwrap().is_some();

        // "Odebrat je" while the device is still active: refused, the hold stays.
        let conn = enabled();
        hold(&conn);
        let error = join::decide_kit_join(&conn, true).expect_err("still active");
        assert!(matches!(error, AppError::Validation(key) if key == join::REMOVAL_PENDING));
        assert!(held(&conn));
        // "Mám je" keeps them: lifted.
        join::decide_kit_join(&conn, false).unwrap();
        assert!(!held(&conn));

        // Once the device is gone, "Odebrat je" lifts it.
        hold(&conn);
        conn.execute(
            "UPDATE sync_devices SET removed = 1 WHERE id = ?1",
            [B.to_hex()],
        )
        .unwrap();
        join::decide_kit_join(&conn, true).unwrap();
        assert!(!held(&conn));

        // A key change of this device pending: neither answer lifts it.
        hold(&conn);
        conn.execute("UPDATE sync_state SET rotation_pending = '{}'", [])
            .unwrap();
        for remove_them in [false, true] {
            assert!(join::decide_kit_join(&conn, remove_them).is_err());
            assert!(held(&conn));
        }
    }

    #[test]
    fn a_running_round_says_since_when() {
        let conn = enabled();
        let running = Runtime {
            running_round: true,
            round_started_at: Some(NOW - 70),
            ..Runtime::default()
        };
        let shown = status(&conn, &running);
        assert_eq!(shown.phase, SyncPhase::Syncing);
        assert_eq!(shown.syncing_since, Some(NOW - 70));
        let idle = Runtime {
            round_started_at: Some(NOW - 70),
            problem: Some(SyncProblem::Offline),
            ..Runtime::default()
        };
        assert_eq!(status(&conn, &idle).syncing_since, None);
    }

    #[test]
    fn the_kit_check_has_four_answers() {
        assert_eq!(
            kit_check_view(setup::KitCheck::Valid).result,
            KitCheckResult::Valid
        );
        let replaced = kit_check_view(setup::KitCheck::Replaced {
            at: Some(1),
            current_created_at: Some(2),
        });
        assert_eq!(replaced.result, KitCheckResult::Replaced);
        assert_eq!(
            (replaced.replaced_at, replaced.current_created_at),
            (Some(1), Some(2))
        );
        assert_eq!(
            kit_check_view(setup::KitCheck::OtherSync).result,
            KitCheckResult::OtherSync
        );
        assert_eq!(
            kit_check_view(setup::KitCheck::Unknown).result,
            KitCheckResult::Unknown
        );
    }
}
