//! Picking up another device's key change (phase 5 spec §7.1, §7.2, §7.4, §7.5).
//!
//! The engine notices a key change three ways (a push answered `403 token_read_only`, a pulled op
//! whose header names a later epoch, and at its start a vault whose `rotation` is above
//! `sync_state.rekey_seen`) and then calls `pick_up`, which fetches every bundle after
//! `rekey_seen` and opens them in order, each from the epoch the one before gave:
//!
//! - **Adopted:** an entry seals the new key to this device's box key, the body opens with it, its
//!   signer's certificate is the one pinned here, `trusted` and not `kit_unconfirmed`, the
//!   signature verifies with the pinned key, the bundle moves this device's epoch on by exactly
//!   one, and what it says of older epochs (keys, heads) agrees with what this device holds. Then,
//!   in one savepoint, the bundle's certificates and removals go into the trust store
//!   (`trust::import_bundle`), the new key becomes the current one, `sync_epochs` records where
//!   the epoch starts (the server's head of the rotation, below), devices that left or were
//!   removed from the list are cut off, `rekey_seen` moves on and the person's notice is written.
//! - **Removed here:** the entry this device opens is a removal notice whose record verifies
//!   against this device's store, or the adopted bundle carries a counted removal of this device:
//!   sync turns off here (`rekey::removed_here`), local data stays.
//! - **Not given:** no entry opens for this device, or the bundle does not parse: sync pauses
//!   (`keyNotGiven`) and is never turned off on a server's word.
//! - **Unverified:** an entry opened but anything after it failed (an unknown or untrusted signer,
//!   a signature, a contradiction): sync pauses (`keyChangeUnverified`).
//!
//! A bundle of a key change this device already holds (`to_epoch` at or below its own; a device
//! that joined after it, or the remover itself) is passed over by its unauthenticated header.
//!
//! **The head is the server's.** `sync_epochs` records the head the server reports with the
//! bundle (`Rekey::head`, `Rotated::head` on the remover); nothing signed carries it, since the
//! remover cannot know it before the server answers. A server that reports a wrong head can only
//! make an honest op look misplaced to the epoch rule (§6.1), which stops the pull (`Invalid`): a
//! stall it could cause anyway. It cannot make a device take an op it should refuse: the new key
//! exists only after the rotation, so no op under it can sit at or below the true head, and the
//! retired token is read-only, so no op under an older key can sit above it. Re-sealing waits for
//! the pull to reach the reported head; an own envelope a lying server hides below a head it
//! reports too low surfaces later above that head under the old epoch, where the epoch rule stops
//! it before the chain could read it as a copy of this device.

use std::path::Path;

use zeroize::Zeroizing;

use super::bundle::{self, Opened, Unverified, Verified};
use super::{device_name, removed_here, write_key_change_notice, KeyChangeNotice};
use crate::error::{AppError, Result};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::error::SyncResult;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{trust, Record};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::{self, Credentials, OlderKey};
use crate::services::sync::relay::{Rekey, Relay, MAX_REKEY_PAGE};
use crate::services::sync::state::{self, EpochRecord};
use rusqlite::Connection;

/// What `pick_up` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pickup {
    /// No bundle after `rekey_seen` changed this device's key (none, or only ones it held).
    Nothing,
    /// This device moved to `epoch`; `by` made the last key change it adopted.
    Adopted { epoch: u32, by: DeviceId },
    /// A verified removal of this device: sync is off here now.
    RemovedHere { by: DeviceId, at: i64 },
    /// A key change gave this device no key and no verifiable notice (`keyNotGiven`).
    NotGiven,
    /// A key change opened for this device but did not verify (`keyChangeUnverified`).
    Unverified,
}

/// What one bundle did.
enum Step {
    Passed,
    Adopted { epoch: u32, by: DeviceId },
    Removed { by: DeviceId, at: i64 },
    NotGiven,
    Unverified,
}

/// Fetches every key-change bundle after `rekey_seen`, page by page, and takes each in order
/// (module doc). Each page is fetched with the credentials as they stand after the one before
/// (a new key's token reads every bundle). A removal of this device turns sync off here
/// (`removed_here` with `data_dir`; `None` inside a round, whose runtime removes the binding
/// file). A relay failure is returned as its `SyncError`; nothing of a bundle not taken is kept.
pub async fn pick_up<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: Option<&Path>,
    now: i64,
) -> SyncResult<Pickup> {
    let mut result = Pickup::Nothing;
    loop {
        let ready = db.with_conn(|conn| {
            Ok(keys::credentials(conn)?.map(|creds| (creds, state::rekey_seen(conn))))
        })?;
        let Some((creds, after)) = ready else {
            return Ok(result);
        };
        let after = after?;
        let page = relay.rekeys(&creds.auth(), after, MAX_REKEY_PAGE).await?;
        for rekey in &page.rekeys {
            match db.with_conn(|conn| take(conn, rekey, now))? {
                Step::Passed => {}
                Step::Adopted { epoch, by } => {
                    log::info!("[SYNC] Picked up the new sync key of epoch {epoch}");
                    result = Pickup::Adopted { epoch, by };
                }
                Step::Removed { by, at } => {
                    db.with_conn(|conn| removed_here(conn, data_dir, by, at))?;
                    return Ok(Pickup::RemovedHere { by, at });
                }
                Step::NotGiven => {
                    log::warn!("[SYNC] The sync key changed and this device got no new key");
                    return Ok(Pickup::NotGiven);
                }
                Step::Unverified => {
                    log::warn!("[SYNC] A change of the sync key could not be verified: paused");
                    return Ok(Pickup::Unverified);
                }
            }
        }
        if !page.more || page.rekeys.is_empty() {
            return Ok(result);
        }
    }
}

/// One bundle, under the connection lock.
fn take(conn: &Connection, rekey: &Rekey, now: i64) -> Result<Step> {
    let creds = keys::credentials(conn)?
        .ok_or_else(|| AppError::Internal("sync pickup: sync is not set up".into()))?;
    let me = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync pickup: sync is not set up".into()))?
        .device;
    let current = creds.keys.current_epoch();
    let Ok(header) = bundle::peek(&rekey.bundle, &creds.vault_id.0) else {
        log::warn!("[SYNC] Key-change bundle {} does not parse", rekey.rotation);
        return Ok(Step::NotGiven);
    };
    if header.to_epoch <= current {
        state::note_rekey_seen(conn, rekey.rotation)?;
        return Ok(Step::Passed);
    }
    let Some(secret) = keys::device_box_secret(conn)? else {
        return Ok(Step::NotGiven);
    };
    let opened = match bundle::open(&rekey.bundle, &creds.vault_id.0, &secret) {
        Ok(opened) => opened,
        Err(_) => {
            log::warn!(
                "[SYNC] Key-change bundle {} opened and then failed its checks",
                rekey.rotation
            );
            return Ok(Step::Unverified);
        }
    };
    match opened {
        Opened::Nothing => Ok(Step::NotGiven),
        Opened::Notice { record, .. } => Ok(notice(conn, &record, me)?),
        Opened::Recipient(unverified) => accept(conn, &creds, unverified, rekey, me, now),
    }
}

/// A removal notice sealed to this device: it counts when it names this device and verifies
/// against this device's store (`trust::record_counts`); anything else only pauses.
fn notice(conn: &Connection, record: &Record, me: DeviceId) -> Result<Step> {
    if record.subject == me && trust::record_counts(conn, record)? {
        return Ok(Step::Removed {
            by: record.signer,
            at: record.at,
        });
    }
    log::warn!("[SYNC] A removal notice for this device does not verify");
    Ok(Step::Unverified)
}

/// The checks of §6.3 on a bundle that sealed the new key to this device, then the adoption.
fn accept(
    conn: &Connection,
    creds: &Credentials,
    unverified: Unverified,
    rekey: &Rekey,
    me: DeviceId,
    now: i64,
) -> Result<Step> {
    let current = creds.keys.current_epoch();
    let header = unverified.header();
    if header.from_epoch != current || Some(header.to_epoch) != current.checked_add(1) {
        log::warn!("[SYNC] A key change does not follow this device's epoch");
        return Ok(Step::Unverified);
    }
    let signer = unverified.signer().clone();
    let by = signer.body.device;
    let pinned = trust::entry(conn, by)?.filter(|entry| {
        entry.cert == signer && entry.state == trust::TrustState::Trusted && !entry.kit_unconfirmed
    });
    let Some(pinned) = pinned else {
        log::warn!(
            "[SYNC] A key change is signed by device {}, which this device does not trust",
            &by.to_hex()[..8]
        );
        return Ok(Step::Unverified);
    };
    let Ok(Verified {
        new_key,
        header,
        body,
    }) = unverified.verify(&pinned.sign_pub)
    else {
        log::warn!("[SYNC] A key change's signature does not verify");
        return Ok(Step::Unverified);
    };
    // What the bundle says of older epochs must agree with what this device holds.
    let mut keys = creds.keys.clone();
    let heads = state::epochs(conn)?;
    let mut new_heads = Vec::new();
    for old in &body.keys {
        if matches!(
            keys.insert_older(old.epoch, Zeroizing::new(*old.key)),
            OlderKey::Differs | OlderKey::Newer
        ) {
            log::warn!("[SYNC] A key change names another key for an epoch this device holds");
            return Ok(Step::Unverified);
        }
        if let Some(head) = old.head {
            match heads.get(&old.epoch) {
                Some(held) if *held != head => {
                    log::warn!("[SYNC] A key change names another start for a known epoch");
                    return Ok(Step::Unverified);
                }
                Some(_) => {}
                None => new_heads.push((old.epoch, head)),
            }
        }
    }
    if heads
        .get(&header.to_epoch)
        .is_some_and(|held| *held != rekey.head)
    {
        log::warn!("[SYNC] The new epoch is recorded here with another start");
        return Ok(Step::Unverified);
    }
    let next = keys.with_next(new_key)?;
    let recipients: Vec<_> = body.recipients.iter().map(|(_, c)| c.clone()).collect();
    let removed: Vec<Record> = body.removed.iter().map(|(_, r)| r.clone()).collect();
    in_savepoint(conn, "sync_pickup", || {
        let about_me = match trust::import_bundle(
            conn,
            &body.signer,
            &recipients,
            &removed,
            header.to_epoch,
            now,
        ) {
            Ok(about_me) => about_me,
            // A refusal of the bundle's contents (its own savepoint wrote nothing).
            Err(AppError::Internal(_)) => {
                log::warn!("[SYNC] A key change carries what this device's store refuses");
                return Ok(Step::Unverified);
            }
            Err(error) => return Err(error),
        };
        if let Some(record) = about_me.iter().find(|r| r.subject == me) {
            return Ok(Step::Removed {
                by: record.signer,
                at: record.at,
            });
        }
        keys::store_key_change(conn, &next)?;
        for (epoch, head) in &new_heads {
            state::record_epoch(conn, *epoch, *head, None, now)?;
        }
        if state::record_epoch(conn, header.to_epoch, rekey.head, Some(by), now)?
            == EpochRecord::Differs
        {
            return Err(AppError::Internal(
                "sync pickup: the new epoch's start changed under the pickup".into(),
            ));
        }
        trust::cut_off_all_pending(conn, header.to_epoch)?;
        state::note_rekey_seen(conn, rekey.rotation)?;
        let removed_names = body
            .removed
            .iter()
            .map(|(device, _)| device_name(conn, *device))
            .collect::<Result<Vec<_>>>()?;
        write_key_change_notice(
            conn,
            &KeyChangeNotice {
                by: device_name(conn, by)?,
                at: body.at,
                removed: removed_names,
            },
        )?;
        Ok(Step::Adopted {
            epoch: header.to_epoch,
            by,
        })
    })
}
