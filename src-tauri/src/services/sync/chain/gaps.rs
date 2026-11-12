//! Held gaps (audit M3's way out, Task 17): an author whose envelopes wait for one that never
//! arrives, for a parent no envelope brings, or for the date of a change stamped more than ten
//! years ahead (audit M6, `chain::too_far_ahead`), is noted with the time this device first held
//! it.
//! While it is held the cursor stays before the held envelope, so every pull fetches again from
//! there (up to `pull::MAX_PULL_PAGES`), nothing is dumped as a snapshot and the relay cannot prune.
//! Once a gap has waited `GAP_SKIP_AFTER_SECS`, the person may skip it (after a confirmation that
//! names the author and how many changes go): the author's head moves past what is missing, so
//! the held envelopes apply at the next pull and the cursor is released, and the miss is recorded
//! as a skipped one (`state::note_skipped_miss`, ruling I1). This device still uploads snapshots,
//! with the miss in their header, so no device heals from them and a restorer inherits it; a
//! snapshot from a device that has the change heals it (`chain::snapshot_heals_missed`). While a
//! skipped miss stands, a change held for a parent the skipped one may have created is skippable
//! as early as the skip was (ruling I2).
//!
//! One record per author, in the Local `app_config` key `sync.held_gap.<device hex>` (JSON; never
//! synced, never captured; no migration): the lowest held sequence of that author, kept with the
//! first time any of them was held. Forgotten when the author's head passes it
//! (`forget_healed_gaps`), when a pull reaches the relay's end with nothing waiting
//! (`forget_all`), when sync is turned off and when a replace starts the chain over.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{head_of, set_head, EnvelopeMeta};
use crate::error::{AppError, Result};
use crate::models::sync::{HeldGapKind, HeldGapView};
use crate::services::sync::engine::status::MAX_DISPLAY_SECS;
use crate::services::sync::hlc::{unpack, DeviceId};
use crate::services::sync::{devices, in_savepoint, state};

/// How long a gap is held before the person may skip it.
pub const GAP_SKIP_AFTER_SECS: i64 = 14 * 86_400;

/// The key a skip asked for too early is refused with.
const NOT_SKIPPABLE_YET: &str = "validation.syncGapNotSkippableYet";

/// The prefix of the Local `app_config` keys, followed by the author's device ID in hex.
const KEY_PREFIX: &str = "sync.held_gap.";

/// What an author's held envelope waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapKind {
    /// An earlier envelope of the author has not arrived (`Verdict::Gap`). Skipping moves the
    /// head to just before the held envelope, which then applies.
    #[serde(rename = "missing")]
    Missing,
    /// The held envelope is the author's next one, but a row it changes references a parent row
    /// no envelope has brought (`StopReason::MissingParent`). Skipping skips the envelope itself.
    #[serde(rename = "parent")]
    Parent,
    /// The held envelope is the author's next one, but its newest clock is more than ten years
    /// past this device's time (`chain::too_far_ahead`). Skipping skips the envelope itself; the
    /// author's later envelopes, held behind it as ordinary gaps, then apply.
    #[serde(rename = "ahead")]
    Ahead,
}

/// One author's held gap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldGap {
    pub device: DeviceId,
    pub kind: GapKind,
    /// The author's lowest held envelope: its sequence, `prev` (`None` only for a first envelope,
    /// which can wait only for a parent), hash and relay position.
    pub held_seq: u64,
    pub held_prev: Option<[u8; 32]>,
    pub held_hash: [u8; 32],
    pub position: u64,
    /// When this device first held an envelope of this author (kept when the gap is met again).
    pub since: i64,
    /// `Ahead`: the physical part (ms) of the held envelope's newest clock, the date it carries.
    pub dated_ms: Option<u64>,
}

impl HeldGap {
    /// When the person may skip it.
    pub fn skippable_at(&self) -> i64 {
        self.since.saturating_add(GAP_SKIP_AFTER_SECS)
    }

    /// The head the author has once this gap is filled or skipped: just before the held envelope
    /// (`Missing`), or the held envelope itself (`Parent`).
    fn head_past(&self) -> Result<(u64, [u8; 32])> {
        match self.kind {
            GapKind::Missing => match (self.held_seq.checked_sub(1), self.held_prev) {
                (Some(seq @ 1..), Some(prev)) => Ok((seq, prev)),
                _ => Err(AppError::Internal(
                    "sync gaps: a missing envelope's record has no earlier link".into(),
                )),
            },
            GapKind::Parent | GapKind::Ahead => Ok((self.held_seq, self.held_hash)),
        }
    }

    /// How many changes a skip gives up, with the author's head at `head_seq`: every sequence
    /// between the head and the held envelope (`Missing`, at least one), or the held envelope
    /// itself (`Parent`).
    fn missing_changes(&self, head_seq: Option<u64>) -> u64 {
        match self.kind {
            GapKind::Missing => self
                .held_seq
                .saturating_sub(head_seq.unwrap_or(0))
                .saturating_sub(1)
                .max(1),
            GapKind::Parent | GapKind::Ahead => 1,
        }
    }

    /// The author's head reached the gap (or passed it): it is filled.
    fn healed_by(&self, head_seq: u64) -> bool {
        match self.kind {
            GapKind::Missing => head_seq.saturating_add(1) >= self.held_seq,
            GapKind::Parent | GapKind::Ahead => head_seq >= self.held_seq,
        }
    }
}

/// The stored form (hex for the hashes).
#[derive(Serialize, Deserialize)]
struct Stored {
    kind: GapKind,
    seq: u64,
    prev: Option<String>,
    hash: String,
    position: u64,
    since: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dated_ms: Option<u64>,
}

fn key_of(device: DeviceId) -> String {
    format!("{KEY_PREFIX}{}", device.to_hex())
}

fn hash_of(hex_text: &str) -> Option<[u8; 32]> {
    hex::decode(hex_text).ok()?.try_into().ok()
}

fn parse(device: DeviceId, value: &str) -> Option<HeldGap> {
    let stored: Stored = serde_json::from_str(value).ok()?;
    let held_prev = match stored.prev {
        Some(prev) => Some(hash_of(&prev)?),
        None => None,
    };
    Some(HeldGap {
        device,
        kind: stored.kind,
        held_seq: stored.seq,
        held_prev,
        held_hash: hash_of(&stored.hash)?,
        position: stored.position,
        since: stored.since,
        dated_ms: stored.dated_ms,
    })
}

fn write(conn: &Connection, gap: &HeldGap) -> Result<()> {
    let stored = Stored {
        kind: gap.kind,
        seq: gap.held_seq,
        prev: gap.held_prev.map(hex::encode),
        hash: hex::encode(gap.held_hash),
        position: gap.position,
        since: gap.since,
        dated_ms: gap.dated_ms,
    };
    let value = serde_json::to_string(&stored)
        .map_err(|e| AppError::Internal(format!("sync gaps: the record does not encode: {e}")))?;
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key_of(gap.device), value],
    )?;
    Ok(())
}

fn forget(conn: &Connection, device: DeviceId) -> Result<()> {
    conn.execute("DELETE FROM app_config WHERE key = ?1", [key_of(device)])?;
    Ok(())
}

/// `device`'s held gap; a record that does not parse counts as none (the next hold writes it again).
fn held_gap(conn: &Connection, device: DeviceId) -> Result<Option<HeldGap>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [key_of(device)],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.and_then(|value| parse(device, &value)))
}

/// The pull held `meta` (`kind` says why) at `now`. The author's record keeps its lowest held
/// sequence, which a lower one replaces, and its first time, which nothing replaces. A new
/// `Parent` record while a skipped miss stands takes the skip's first time when that is earlier
/// (ruling I2): its parent may be a row the skipped change created, which is not coming, so the
/// person need not wait another 14 days to skip it too (still behind the password and its
/// confirmation). A `Missing` or `Ahead` record starts at `now`. An `Ahead` record is written with
/// `note_held_ahead`, which keeps the date it carries.
pub fn note_held_gap(
    conn: &Connection,
    meta: &EnvelopeMeta,
    kind: GapKind,
    now: i64,
) -> Result<()> {
    note(conn, meta, kind, None, now)
}

/// The pull held `meta`, the author's next envelope, at `now` because its newest clock
/// (`newest_hlc`) is more than ten years ahead (audit M6): an `Ahead` record (`note_held_gap`'s
/// rules), with the date it carries. The author's later envelopes are judged `Gap` behind it; their
/// higher sequences leave this record as it is, and when the relay served one first its `Missing`
/// record gives way to this lower one (keeping its first time).
pub fn note_held_ahead(
    conn: &Connection,
    meta: &EnvelopeMeta,
    newest_hlc: u64,
    now: i64,
) -> Result<()> {
    note(conn, meta, GapKind::Ahead, Some(unpack(newest_hlc).0), now)
}

fn note(
    conn: &Connection,
    meta: &EnvelopeMeta,
    kind: GapKind,
    dated_ms: Option<u64>,
    now: i64,
) -> Result<()> {
    let existing = held_gap(conn, meta.device)?;
    if existing
        .as_ref()
        .is_some_and(|gap| gap.held_seq <= meta.seq)
    {
        return Ok(());
    }
    write(
        conn,
        &HeldGap {
            device: meta.device,
            kind,
            held_seq: meta.seq,
            held_prev: meta.prev,
            held_hash: meta.hash,
            position: meta.position,
            since: match existing {
                Some(gap) => gap.since,
                None if kind == GapKind::Parent => {
                    state::skipped_miss_since(conn)?.map_or(now, |skipped| skipped.min(now))
                }
                None => now,
            },
            dated_ms,
        },
    )
}

/// Every held gap, the oldest first. Records that do not parse are left out.
pub fn held_gaps(conn: &Connection) -> Result<Vec<HeldGap>> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM app_config WHERE substr(key, 1, ?2) = ?1 ORDER BY key")?;
    let rows = stmt.query_map(params![KEY_PREFIX, KEY_PREFIX.len() as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut gaps = Vec::new();
    for row in rows {
        let (key, value) = row?;
        let Ok(device) = DeviceId::from_hex(&key[KEY_PREFIX.len()..]) else {
            continue;
        };
        if let Some(gap) = parse(device, &value) {
            gaps.push(gap);
        }
    }
    gaps.sort_by_key(|gap| gap.since);
    Ok(gaps)
}

/// The held gaps as Settings shows them (`held_gaps`, with each author's name from the device
/// list).
pub fn held_gap_views(conn: &Connection) -> Result<Vec<HeldGapView>> {
    held_gaps(conn)?
        .into_iter()
        .map(|gap| {
            Ok(HeldGapView {
                device_id: gap.device.to_hex(),
                device_name: devices::device_row(conn, gap.device)?.map(|(name, _)| name),
                kind: match gap.kind {
                    GapKind::Missing => HeldGapKind::Missing,
                    GapKind::Parent => HeldGapKind::Parent,
                    GapKind::Ahead => HeldGapKind::Ahead,
                },
                missing_changes: gap.missing_changes(head_of(conn, gap.device)?.map(|h| h.seq)),
                since: gap.since,
                skippable_at: gap.skippable_at(),
                dated_at: gap
                    .dated_ms
                    .and_then(|ms| i64::try_from(ms / 1000).ok())
                    .filter(|at| (0..=MAX_DISPLAY_SECS).contains(at)),
            })
        })
        .collect()
}

/// Forgets the gaps whose author's head reached them (the missing envelope arrived, the parent
/// came and the held envelope applied, or a snapshot restore moved the head on) and those of a
/// quarantined author (its envelopes are held for the fork, which is shown instead). Returns how
/// many were forgotten.
pub fn forget_healed_gaps(conn: &Connection) -> Result<usize> {
    let mut forgotten = 0;
    for gap in held_gaps(conn)? {
        let healed = head_of(conn, gap.device)?
            .is_some_and(|head| head.quarantined || gap.healed_by(head.seq));
        if healed {
            forget(conn, gap.device)?;
            forgotten += 1;
        }
    }
    Ok(forgotten)
}

/// Forgets every held gap: a pull reached the relay's end with nothing waiting, sync was turned
/// off, or a replace started the chain over.
pub fn forget_all(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM app_config WHERE substr(key, 1, ?2) = ?1",
        params![KEY_PREFIX, KEY_PREFIX.len() as i64],
    )?;
    Ok(())
}

/// Skips `device`'s held gap at `now`. Refused (`validation.syncGapNotSkippableYet`) before it has
/// been held `GAP_SKIP_AFTER_SECS`. Otherwise, in one savepoint: the author's head moves past what
/// is missing (`HeldGap::head_past`), so the held envelopes apply at the next pull and release the
/// cursor; the miss is recorded at the held envelope's position (`state::note_skipped_miss`, with
/// the gap's first time), so a later snapshot from a device that has the change heals it; the
/// record goes. `Ok(true)` when a change was skipped. A gap that is no longer held (it healed
/// meanwhile, or none was noted for `device`) is nothing to skip: `Ok(false)`, and nothing changes.
pub fn skip_missing_change(conn: &Connection, device: DeviceId, now: i64) -> Result<bool> {
    let Some(gap) = held_gap(conn, device)? else {
        return Ok(false);
    };
    if now < gap.skippable_at() {
        return Err(AppError::Validation(NOT_SKIPPABLE_YET.into()));
    }
    in_savepoint(conn, "sync_skip_missing_change", || {
        let reached = head_of(conn, device)?.map_or(0, |head| head.seq);
        let skipped = !gap.healed_by(reached);
        if skipped {
            let (seq, hash) = gap.head_past()?;
            set_head(conn, device, seq, hash)?;
            state::note_skipped_miss(conn, gap.position, gap.since)?;
            log::info!("[SYNC] Skipped a change that never arrived; it is recorded as missed");
        }
        forget(conn, device)?;
        Ok(skipped)
    })
}
