//! Picking up another device's key change (phase 5 spec §7.1, §7.2, §7.4, §7.5).
//!
//! The engine notices a key change three ways (a push answered `403 token_read_only`, a pulled op
//! whose header names a later epoch, and the vault's info, read before the round's pull, with a
//! `rotation` above `sync_state.rekey_seen`) and then calls `pick_up`, which fetches every bundle
//! after `rekey_seen` and opens them in order, each from the epoch the one before gave. Each page
//! must list rotations strictly above the last one taken, and one walk takes at most
//! `MAX_BUNDLES_PER_WALK` bundles: anything else is a relay error (a server that pages for ever).
//!
//! - **Adopted:** an entry seals the new key to this device's box key, the body opens with it, its
//!   signer's certificate is the one pinned here and its device may speak (`trusted` and not left
//!   out for its issuer, `trust::speaker`), no removal
//!   or leave of the signer that this device already knows of (in its list, verified, or in its
//!   own pending change) counts, the signature verifies with the pinned key, the bundle moves this
//!   device's epoch on by exactly one, what it says of older epochs (keys, heads) agrees with what
//!   this device holds, and the server knows the token of the new key (`vault_info` with it, so a
//!   signer this device still trusts cannot hand it a key nobody rotated to). Then, in one
//!   savepoint, the bundle's certificates, its Kit and its removals (the Kit it replaces among
//!   them) go into the trust store (`trust::import_bundle`), the new key becomes the current one, `sync_epochs` records where
//!   the epoch starts (the server's head, provisional until the remover's signed head confirms it:
//!   `rekey::head`), devices that left or were removed from the list are cut off, `rekey_seen`
//!   moves on and the person's notice is written.
//! - **Removed here:** the entry this device opens is a removal notice whose record verifies
//!   against this device's store, or the adopted bundle carries a counted removal of this device:
//!   sync turns off here (`rekey::removed_here`), local data stays.
//! - **Not given:** no entry opens for this device, or the bundle does not parse: sync pauses
//!   (`keyNotGiven`) and is never turned off on a server's word.
//! - **Unverified:** an entry opened but anything after it failed (an unknown or untrusted signer,
//!   a signature, a contradiction, a token the server does not know): sync pauses
//!   (`keyChangeUnverified`).
//!
//! A bundle of a key change this device already holds (`to_epoch` at or below its own; a device
//! that joined after it, or the remover itself) is passed over only when its body opens with the
//! key this device holds for that epoch; one that does not (a header the server made up, with a
//! rotation number that would hide the real ones) moves nothing and is `keyChangeUnverified`.
//!
//! **The head is the server's, then the signer's.** The head the server reports with the bundle
//! (`Rekey::head`) is recorded but only provisionally (`head::note_unverified`): until the
//! remover's signed head confirms it, no op under an older epoch past where this device stood is
//! applied, and nothing is sealed again under the new key (`rekey::head` module doc).

use std::path::Path;

use zeroize::Zeroizing;

use super::bundle::{self, BundleBody, Header, Opened, Unverified, Verified};
use super::{device_name, head, removed_here, rotate, write_key_change_notice, KeyChangeNotice};
use crate::error::{AppError, Result};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::vault_token;
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{trust, Record};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::{self, Credentials, OlderKey, VaultAuth, VaultKeys};
use crate::services::sync::relay::{Rekey, RekeyPage, Relay, RelayError, MAX_REKEY_PAGE};
use crate::services::sync::state::{self, EpochRecord};
use rusqlite::{Connection, OptionalExtension};

/// Bundles one walk takes at most: the server keeps a vault's bundles for 30 days and refuses a
/// 33rd change in that time, so an honest walk is far below this.
pub const MAX_BUNDLES_PER_WALK: usize = 64;

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

/// A bundle that passed every check made under the connection lock; the server's knowledge of
/// the new key's token is checked next, without it. Never printed (it holds the new key).
pub(crate) struct Prepared {
    next: VaultKeys,
    header: Header,
    body: Box<BundleBody>,
    new_heads: Vec<(u32, u64)>,
    by: DeviceId,
}

impl Prepared {
    /// The token the new key gives the vault.
    fn new_auth(&self, creds: &Credentials) -> VaultAuth {
        VaultAuth {
            vault_id: creds.vault_id.to_b64(),
            token: Zeroizing::new(vault_token(self.next.current(), &creds.vault_id.0)),
        }
    }
}

/// What one bundle did.
enum Step {
    Passed,
    Probe(Box<Prepared>),
    Adopted { epoch: u32, by: DeviceId },
    Removed { by: DeviceId, at: i64 },
    NotGiven,
    Unverified,
}

fn refused(what: &str) -> SyncError {
    log::warn!("[SYNC] The sync server's list of key changes is refused: {what}");
    SyncError::from(RelayError::Protocol(format!("key changes: {what}")))
}

/// A page of the walk: every rotation above `after` and strictly increasing (a page the server
/// repeats, or one going back, would be walked for ever).
fn check_walk_page(page: &RekeyPage, after: u32) -> SyncResult<()> {
    let mut previous = after;
    for rekey in &page.rekeys {
        if rekey.rotation <= previous {
            return Err(refused("a rotation is not above the one before"));
        }
        previous = rekey.rotation;
    }
    Ok(())
}

/// Fetches every key-change bundle after `rekey_seen`, page by page, and takes each in order
/// (module doc). Each page is fetched with the credentials as they stand after the one before
/// (a new key's token reads every bundle). A removal of this device turns sync off here
/// (`removed_here` with `data_dir`; `None` inside a round, whose runtime removes the binding
/// file). A relay failure is returned as its `SyncError`, and so is a page outside the walk's
/// rules (`check_walk_page`, `MAX_BUNDLES_PER_WALK`); nothing of a bundle not taken is kept.
pub async fn pick_up<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: Option<&Path>,
    now: i64,
) -> SyncResult<Pickup> {
    let mut result = Pickup::Nothing;
    let mut walked = 0usize;
    loop {
        let ready = db.with_conn(|conn| {
            Ok(keys::credentials(conn)?.map(|creds| (creds, state::rekey_seen(conn))))
        })?;
        let Some((creds, after)) = ready else {
            return Ok(result);
        };
        let after = after?;
        let page = relay.rekeys(&creds.auth(), after, MAX_REKEY_PAGE).await?;
        check_walk_page(&page, after)?;
        for rekey in &page.rekeys {
            walked += 1;
            if walked > MAX_BUNDLES_PER_WALK {
                return Err(refused("more bundles than one walk takes"));
            }
            let step = match db.with_conn(|conn| take(conn, rekey))? {
                Step::Probe(prepared) => match relay.vault_info(&prepared.new_auth(&creds)).await {
                    Ok(_) => db.with_conn(|conn| adopt(conn, *prepared, rekey, now))?,
                    Err(RelayError::Unauthorized) => {
                        log::warn!(
                            "[SYNC] A key change names a key the sync server has no token for"
                        );
                        Step::Unverified
                    }
                    Err(error) => return Err(error.into()),
                },
                step => step,
            };
            match step {
                Step::Passed => {}
                Step::Probe(_) => {
                    return Err(SyncError::Local(AppError::Internal(
                        "sync pickup: a bundle was probed twice".into(),
                    )))
                }
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

/// What the remover's commit writes into the device list (the removal records, the new Kit's
/// row), written here too where it is missing (As built R-74): a remover that gives its change up
/// (R-56) never commits, and without these rows a later join restores the removed devices as
/// active and finds no row of the new Kit. Captured, so they sync; the values are the bundle's
/// own, so they agree with the remover's wherever both arrive (write-once columns; a row already
/// there is left as it is).
fn write_change_rows(conn: &Connection, body: &BundleBody, now: i64) -> Result<()> {
    let replaced = body.kit.as_ref().and_then(|kit| kit.replaces.as_ref());
    for record in body.removed.iter().map(|(_, r)| r).chain(replaced) {
        conn.execute(
            "UPDATE sync_devices SET removal_record = ?1, removed = 1 \
             WHERE id = ?2 AND removal_record IS NULL",
            rusqlite::params![record.to_b64(), record.subject.to_hex()],
        )?;
    }
    if let Some(kit) = &body.kit {
        super::kit::write_row(conn, &kit.cert, None, now)?;
    }
    Ok(())
}

/// One bundle, under the connection lock.
fn take(conn: &Connection, rekey: &Rekey) -> Result<Step> {
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
        // Passed over only when it opens with the key this device holds for that epoch: a header
        // alone is the server's word, and its rotation would move `rekey_seen` past real ones.
        let opens = creds.keys.get(header.to_epoch).is_some_and(|key| {
            bundle::open_body_with_key(&rekey.bundle, &creds.vault_id.0, &key).is_ok()
        });
        if !opens {
            log::warn!(
                "[SYNC] Key-change bundle {} names an epoch this device holds but does not open",
                rekey.rotation
            );
            return Ok(Step::Unverified);
        }
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
        Opened::Recipient(unverified) => check(conn, &creds, unverified, rekey),
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

/// Whether this device already knows `device` was removed or left: a record in its list row
/// that counts here (the store may not have caught up with it yet), or this device's own pending
/// key change removing it.
fn known_gone(conn: &Connection, device: DeviceId) -> Result<bool> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT removal_record, left_record FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((removal, left)) = row {
        for value in [removal, left].into_iter().flatten() {
            if let Ok(record) = Record::from_b64(&value) {
                if record.subject == device && trust::record_counts(conn, &record)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(rotate::pending_removals(conn)?.contains(&device))
}

/// The checks of §6.3 on a bundle that sealed the new key to this device, short of the server's
/// token (`pick_up` probes it, then `adopt`).
fn check(
    conn: &Connection,
    creds: &Credentials,
    unverified: Unverified,
    rekey: &Rekey,
) -> Result<Step> {
    let current = creds.keys.current_epoch();
    let header = unverified.header();
    if header.from_epoch != current || Some(header.to_epoch) != current.checked_add(1) {
        log::warn!("[SYNC] A key change does not follow this device's epoch");
        return Ok(Step::Unverified);
    }
    let signer = unverified.signer().clone();
    let by = signer.body.device;
    // Pinned with this certificate, `trusted`, and not left out for its issuer (revision
    // As built R-22): a device that gets no key from this one makes no key change for it either.
    let Some(pinned) = trust::speaker(conn, &signer)? else {
        log::warn!(
            "[SYNC] A key change is signed by device {}, which may not speak here",
            &by.to_hex()[..8]
        );
        note_unverified_signer(conn, by)?;
        return Ok(Step::Unverified);
    };
    if known_gone(conn, by)? {
        log::warn!(
            "[SYNC] A key change is signed by device {}, which this device knows was removed",
            &by.to_hex()[..8]
        );
        note_unverified_signer(conn, by)?;
        return Ok(Step::Unverified);
    }
    let Ok(Verified {
        new_key,
        header,
        body,
    }) = unverified.verify(&pinned.sign_pub)
    else {
        log::warn!("[SYNC] A key change's signature does not verify");
        note_unverified_signer(conn, by)?;
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
    Ok(Step::Probe(Box::new(Prepared {
        next: keys.with_next(new_key)?,
        header,
        body,
        new_heads,
        by,
    })))
}

/// A key change this device cannot verify names its signer: the status says who made it
/// ("Změnu provedlo zařízení {by}, které toto zařízení nezná", `SyncStatus::rejoin_by`), by the
/// name the device list has for it or the first 8 hex characters of its ID (R-35). Local; the
/// next key change adopted clears it (`adopt`).
fn note_unverified_signer(conn: &Connection, by: DeviceId) -> Result<()> {
    let value = serde_json::json!({ "by": super::device_name(conn, by)? }).to_string();
    state::set_config_text(conn, state::KEY_CHANGE_UNVERIFIED_KEY, Some(&value))
}

/// Whether `device` is a Recovery Kit here: a Kit entry, or a row whose certificate is a Kit's
/// (`trust::is_kit_row`).
fn is_kit(conn: &Connection, device: DeviceId) -> Result<bool> {
    if let Some(entry) = trust::entry(conn, device)? {
        return Ok(entry.kind == crate::services::sync::identity::CertKind::Kit);
    }
    let cert: Option<Option<String>> = conn
        .query_row(
            "SELECT cert FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(trust::is_kit_row(
        device,
        cert.flatten().as_deref(),
        &trust::vault_id(conn)?,
    ))
}

/// The adoption of a bundle `check` passed and whose token the server knows, in one savepoint.
fn adopt(conn: &Connection, prepared: Prepared, rekey: &Rekey, now: i64) -> Result<Step> {
    let Prepared {
        next,
        header,
        body,
        new_heads,
        by,
    } = prepared;
    let sync = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync pickup: sync is not set up".into()))?;
    let me = sync.device;
    let recipients: Vec<_> = body.recipients.iter().map(|(_, c)| c.clone()).collect();
    let removed: Vec<Record> = body.removed.iter().map(|(_, r)| r.clone()).collect();
    if body.kit.is_none() && trust::sealing_kit_entry(conn)?.is_some() {
        // Only a speaking signer can sign this (review of Task 17, M2): a vault with a Kit here and
        // a key change that names none. Taken as it is; the Kit here stays the one sealed to.
        log::warn!("[SYNC] A key change names no Recovery Kit although this device has one");
    }
    in_savepoint(conn, "sync_pickup", || {
        let about_me = match trust::import_bundle(
            conn,
            &body.signer,
            &recipients,
            // The vault's Kit after the change: pinned and confirmed, the one it replaces
            // removed (revision §2.5).
            body.kit.as_ref().map(|kit| trust::BundleKit {
                cert: &kit.cert,
                replaces: kit.replaces.as_ref(),
            }),
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
        state::set_config_text(conn, state::KEY_CHANGE_UNVERIFIED_KEY, None)?;
        for (epoch, head) in &new_heads {
            state::record_epoch(conn, *epoch, *head, None, now)?;
        }
        match state::record_epoch(conn, header.to_epoch, rekey.head, Some(by), now)? {
            EpochRecord::Differs => {
                return Err(AppError::Internal(
                    "sync pickup: the new epoch's start changed under the pickup".into(),
                ))
            }
            // The server's head, until the remover's signed head confirms it.
            EpochRecord::Added => head::note_unverified(
                conn,
                header.to_epoch,
                Some(rekey.head),
                Some(rekey.rotation),
                by,
                sync.cursor,
            )?,
            EpochRecord::Same => {}
        }
        trust::cut_off_all_pending(conn, header.to_epoch)?;
        write_change_rows(conn, &body, now)?;
        state::note_rekey_seen(conn, rekey.rotation)?;
        // A removed Kit (one a new Kit retires besides the one it replaces, review of Task 17,
        // I2) is no device to name: it says that the Kit was replaced.
        let mut removed_names = Vec::new();
        let mut kit_replaced = body.kit.as_ref().is_some_and(|k| k.replaces.is_some());
        for (device, _) in &body.removed {
            if is_kit(conn, *device)? {
                kit_replaced = true;
            } else {
                removed_names.push(device_name(conn, *device)?);
            }
        }
        write_key_change_notice(
            conn,
            &KeyChangeNotice {
                by: device_name(conn, by)?,
                at: body.at,
                removed: removed_names,
                kit_replaced,
            },
        )?;
        Ok(Step::Adopted {
            epoch: header.to_epoch,
            by,
        })
    })
}
