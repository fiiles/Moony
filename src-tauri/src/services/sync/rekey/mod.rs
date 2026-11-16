//! Changing the vault key (phase 5 spec §6, §7): the signed key-change bundle (`bundle`) that a
//! device stores on the sync server with the token rotation, which every remaining device opens
//! to pick the new key up and a removed device opens only as its removal notice. `rotate` makes a
//! key change (or a removal from the list) on this device, `pickup` takes another device's key
//! change in, `head` signs where a key change starts (on the remover) and checks the server's
//! word against that signature (everywhere else), and `reseal` seals again, under the new key,
//! what this device had not yet sent.

pub mod bundle;
pub mod envelope;
pub mod head;
pub mod kit;
pub mod pickup;
pub mod reseal;
pub mod rotate;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::services::sync::engine::status;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::{binding, devices, setup, state};

/// What another device's key change tells the person (phase 5 §7.3), kept in the Local
/// `app_config` key `state::KEY_CHANGE_NOTICE_KEY` until dismissed: who changed the key (its
/// name as this device knew it), when (the bundle's signed time), the names of the devices it
/// removed and whether it made a new Recovery Kit (revision §6.4, "vytvořilo novou záchrannou
/// sadu").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyChangeNotice {
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "removed")]
    pub removed: Vec<String>,
    #[serde(rename = "kitReplaced", default)]
    pub kit_replaced: bool,
}

/// Why sync went off on this device (phase 5 §7.4), kept in the Local `app_config` key
/// `state::REMOVED_NOTICE_KEY`, which outlives turning sync off: who removed it, when (the
/// removal record's signed time) and how many changes never left this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedNotice {
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "unsent")]
    pub unsent: u32,
}

/// This device had applied changes past where a key change started before it learnt of the key
/// change (the server hid it, phase 5 As built 65), kept in the Local `app_config` key
/// `state::KEY_CHANGE_OVERRUN_KEY` until dismissed: the epoch, its verified head and how far this
/// device's cursor had gone. Nothing was undone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyChangeOverrun {
    #[serde(rename = "epoch")]
    pub epoch: u32,
    #[serde(rename = "head")]
    pub head: u64,
    #[serde(rename = "through")]
    pub through: u64,
}

/// A removal or key change the person confirmed that a background resolution found not done
/// (phase 5 As built 69): another device changed the key first, or the server refused it. Kept in
/// the Local `app_config` key `state::REMOVAL_NOT_DONE_KEY` until dismissed: who changed the key
/// first (its name, when picked up), the names of the devices the person meant to remove,
/// whether that other change had already removed every one of them, and whether the change was
/// to make a new Recovery Kit, which then never started working (revision §2.7, §6.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalNotDone {
    #[serde(rename = "by")]
    pub by: Option<String>,
    #[serde(rename = "devices")]
    pub devices: Vec<String>,
    #[serde(rename = "alreadyRemoved")]
    pub already_removed: bool,
    #[serde(rename = "kitReplaced", default)]
    pub kit_replaced: bool,
}

/// A device's name as this device's list knows it, or the first 8 hex characters of its ID.
pub(crate) fn device_name(conn: &Connection, device: DeviceId) -> Result<String> {
    Ok(devices::device_row(conn, device)?
        .map(|(name, _)| name)
        .unwrap_or_else(|| device.to_hex()[..8].to_string()))
}

fn read_config<T: for<'de> Deserialize<'de>>(conn: &Connection, key: &str) -> Result<Option<T>> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM app_config WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    // A value that does not read is no notice (it is only ever written here).
    Ok(value.and_then(|v| serde_json::from_str(&v).ok()))
}

fn write_config<T: Serialize>(conn: &Connection, key: &str, value: &T) -> Result<()> {
    let json = serde_json::to_string(value)
        .map_err(|e| AppError::Internal(format!("sync: a notice does not serialise: {e}")))?;
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, json],
    )?;
    Ok(())
}

/// The newest key change another device made, if the person has not dismissed it.
pub fn key_change_notice(conn: &Connection) -> Result<Option<KeyChangeNotice>> {
    read_config(conn, state::KEY_CHANGE_NOTICE_KEY)
}

pub(crate) fn write_key_change_notice(conn: &Connection, notice: &KeyChangeNotice) -> Result<()> {
    write_config(conn, state::KEY_CHANGE_NOTICE_KEY, notice)
}

/// Changes applied past a key change the server hid, if any and not dismissed.
pub fn key_change_overrun(conn: &Connection) -> Result<Option<KeyChangeOverrun>> {
    read_config(conn, state::KEY_CHANGE_OVERRUN_KEY)
}

pub(crate) fn write_key_change_overrun(conn: &Connection, notice: &KeyChangeOverrun) -> Result<()> {
    write_config(conn, state::KEY_CHANGE_OVERRUN_KEY, notice)
}

/// A confirmed removal a background round found not done, if any and not dismissed.
pub fn removal_not_done(conn: &Connection) -> Result<Option<RemovalNotDone>> {
    read_config(conn, state::REMOVAL_NOT_DONE_KEY)
}

pub(crate) fn write_removal_not_done(conn: &Connection, notice: &RemovalNotDone) -> Result<()> {
    write_config(conn, state::REMOVAL_NOT_DONE_KEY, notice)
}

/// Why sync went off here, if another device removed this one and the person has not dismissed
/// it (readable with sync off).
pub fn removed_notice(conn: &Connection) -> Result<Option<RemovedNotice>> {
    read_config(conn, state::REMOVED_NOTICE_KEY)
}

/// This device was removed by `by` at `at`, by a record it verified (phase 5 §7.4): it counts what
/// never left (the outbox and the unsent envelopes), keeps that in `sync.removed_notice` with the
/// remover's name, and turns sync off as `setup::turn_off` does (keys, clocks, outbox and binding
/// go; local data stays; no leave record is signed). `data_dir` is the folder of the binding
/// file; a caller inside a round passes `None` and leaves the file to the engine's runtime
/// (`forget_binding_after_removal`). The notice is written first, so it is there whenever sync is
/// off because of this; a turn-off that fails takes it back.
pub fn removed_here(
    conn: &Connection,
    data_dir: Option<&Path>,
    by: DeviceId,
    at: i64,
) -> Result<()> {
    let notice = RemovedNotice {
        by: device_name(conn, by)?,
        at,
        unsent: status::unsent_changes(conn)?,
    };
    write_config(conn, state::REMOVED_NOTICE_KEY, &notice)?;
    let off = match data_dir {
        Some(dir) => setup::turn_off(conn, dir),
        None => setup::turn_off_keeping_binding_file(conn),
    };
    if let Err(error) = off {
        conn.execute(
            "DELETE FROM app_config WHERE key = ?1",
            [state::REMOVED_NOTICE_KEY],
        )?;
        return Err(error);
    }
    log::warn!("[SYNC] Another device removed this one from sync: sync is off here, data stays");
    Ok(())
}

/// The engine's runtime after a round that turned sync off because this device was removed: the
/// binding file and the restore marker go, as `setup::turn_off` removes them.
pub(crate) fn forget_binding_after_removal(data_dir: &Path) -> Result<()> {
    binding::forget_binding(data_dir)
}

#[cfg(test)]
mod bundle_tests;
#[cfg(test)]
mod envelope_tests;
