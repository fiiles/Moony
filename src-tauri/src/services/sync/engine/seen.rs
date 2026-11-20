//! Notes of the ops this device has handled above its stored cursor (spec
//! 2026-11-20-sync-audit-2-fixes-design §2.3, rule 9a): the envelope hash by relay position, in
//! `sync_seen_ops` (Local). An op that is held (a gap in its author's chain, a quarantined
//! author, a date too far ahead) leaves the cursor before it while later ops are handled, so
//! those positions are read again in every pull, and nothing else ties a position to what was
//! found there the first time. The rule does not depend on a key change.
//!
//! Two positions say where a device stands. The watermark (`watermark`) is the position below
//! which it has handled every position: the higher of its stored cursor and the last snapshot it
//! restored. The place (`place`) is the highest of the watermark and any noted position: where the
//! device has been. A held op above the cursor raises the place and not the watermark, because it
//! says nothing of the positions between. The place judges a snapshot (`snapshot_admissible`,
//! rule 10); any check that skips a range because the device is past it uses the watermark (the
//! head's check, rule 7).
//!
//! The pull notes an op it handled without moving the cursor past it, in the same transaction as
//! the handling, and refuses a different op at a noted position (`PullStop::Invalid`). A position
//! above the cursor that has no note (a crash came between) is noted when it is next read.
//!
//! Notes go as the cursor passes them (`forget_through`, from `state::set_cursor`), as a merge
//! restore covers them (through the snapshot's position), all of them when the profile is replaced
//! and when sync is turned off. Two things leave them alone on purpose. A re-join under a new ID:
//! the notes are hashes by position, neither the cursor nor the held positions change with the
//! device ID, and a member can make a device re-join by claiming its ID, so wiping them there
//! would let a handled position be refilled. A server found to have gone back
//! (`state::note_vault_went_back`): that verdict rests on a number the server sends
//! unauthenticated, and one lie would wipe them. A device in that state is told to re-sync, and
//! the replace clears the notes; the state keeps its own message.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::services::sync::rekey::range::signed;

/// Notes the hash of the op handled at `pos`; a position noted again keeps the newest hash.
pub fn note(conn: &Connection, pos: u64, hash: &[u8; 32]) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_seen_ops (pos, hash) VALUES (?1, ?2) \
         ON CONFLICT(pos) DO UPDATE SET hash = excluded.hash",
        params![signed(pos, "seen position")?, hash],
    )?;
    Ok(())
}

/// The hash noted for `pos`, if any.
pub fn noted(conn: &Connection, pos: u64) -> Result<Option<[u8; 32]>> {
    let hash: Option<Vec<u8>> = conn
        .query_row(
            "SELECT hash FROM sync_seen_ops WHERE pos = ?1",
            [signed(pos, "seen position")?],
            |r| r.get(0),
        )
        .optional()?;
    hash.map(|h| {
        <[u8; 32]>::try_from(h.as_slice())
            .map_err(|_| AppError::Internal("sync_seen_ops.hash is not 32 bytes".into()))
    })
    .transpose()
}

/// Drops the notes at or below `cursor`: the cursor covers them.
pub fn forget_through(conn: &Connection, cursor: u64) -> Result<()> {
    conn.execute(
        "DELETE FROM sync_seen_ops WHERE pos <= ?1",
        [signed(cursor, "cursor")?],
    )?;
    Ok(())
}

/// Drops every note. Tests only: the replace and turning sync off clear the table with their own
/// statements, and a re-join under a new ID keeps it.
#[cfg(test)]
pub fn forget_all(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM sync_seen_ops", [])?;
    Ok(())
}

/// A device's place: the highest of its stored cursor, the highest noted position and the
/// position of the last snapshot it restored. `None` while sync is not set up (no `sync_state`
/// row).
pub fn place(conn: &Connection) -> Result<Option<u64>> {
    let row: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT cursor, restored_seq FROM sync_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((cursor, restored)) = row else {
        return Ok(None);
    };
    let highest: Option<i64> =
        conn.query_row("SELECT MAX(pos) FROM sync_seen_ops", [], |r| r.get(0))?;
    let place = [restored, highest]
        .into_iter()
        .flatten()
        .fold(cursor, i64::max);
    u64::try_from(place)
        .map(Some)
        .map_err(|_| AppError::Internal(format!("sync: the place is out of range: {place}")))
}

/// The watermark: the position below which this device has handled EVERY position, the higher of
/// its stored cursor and the position of the last snapshot it restored. Unlike `place` it does
/// not count a held op above the cursor, which says nothing of the positions between the two; a
/// check that skips a range because the device is "past" it (spec §2.3 rule 7) must use this one.
/// `None` while sync is not set up.
pub fn watermark(conn: &Connection) -> Result<Option<u64>> {
    let row: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT cursor, restored_seq FROM sync_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((cursor, restored)) = row else {
        return Ok(None);
    };
    let mark = restored.map_or(cursor, |r| r.max(cursor));
    u64::try_from(mark)
        .map(Some)
        .map_err(|_| AppError::Internal(format!("sync: the watermark is out of range: {mark}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    fn set_up() -> Connection {
        let conn = migrated();
        state::ensure_row(&conn, DeviceId::random()).expect("row");
        conn
    }

    #[test]
    fn a_noted_hash_round_trips_and_a_second_note_replaces_it() {
        let conn = set_up();
        assert_eq!(noted(&conn, 5).expect("none"), None);
        note(&conn, 5, &[1; 32]).expect("note");
        assert_eq!(noted(&conn, 5).expect("noted"), Some([1; 32]));
        note(&conn, 5, &[2; 32]).expect("again");
        assert_eq!(noted(&conn, 5).expect("noted"), Some([2; 32]));
        assert_eq!(noted(&conn, 6).expect("other"), None);
        assert!(note(&conn, u64::MAX, &[3; 32]).is_err());
    }

    #[test]
    fn forgetting_through_the_cursor_keeps_only_what_is_above_it() {
        let conn = set_up();
        for pos in [3, 4, 5, 9] {
            note(&conn, pos, &[pos as u8; 32]).expect("note");
        }
        forget_through(&conn, 4).expect("forget");
        assert_eq!(noted(&conn, 4).expect("gone"), None);
        assert_eq!(noted(&conn, 5).expect("kept"), Some([5; 32]));
        forget_all(&conn).expect("all");
        assert_eq!(noted(&conn, 9).expect("gone"), None);
    }

    #[test]
    fn the_place_is_the_highest_of_the_cursor_the_noted_and_the_restored() {
        let conn = set_up();
        assert_eq!(place(&conn).expect("place"), Some(0), "a fresh row");
        state::set_cursor(&conn, 10).expect("cursor");
        assert_eq!(place(&conn).expect("place"), Some(10));
        note(&conn, 14, &[1; 32]).expect("note");
        note(&conn, 12, &[1; 32]).expect("note");
        assert_eq!(place(&conn).expect("place"), Some(14), "a held op above");
        state::set_restored_seq(&conn, 30).expect("restored");
        state::set_restored_seq(&conn, 20).expect("a merge never lowers it");
        assert_eq!(
            place(&conn).expect("place"),
            Some(30),
            "a restored snapshot"
        );
        forget_all(&conn).expect("forget");
        state::set_cursor(&conn, 40).expect("cursor");
        assert_eq!(place(&conn).expect("place"), Some(40));
    }

    #[test]
    fn the_watermark_ignores_a_held_op_above_the_cursor() {
        let conn = set_up();
        state::set_cursor(&conn, 10).expect("cursor");
        note(&conn, 14, &[1; 32]).expect("a held op above");
        assert_eq!(place(&conn).expect("place"), Some(14));
        assert_eq!(watermark(&conn).expect("mark"), Some(10));
        state::set_restored_seq(&conn, 30).expect("restored");
        assert_eq!(
            watermark(&conn).expect("mark"),
            Some(30),
            "a restored snapshot"
        );
        assert_eq!(watermark(&migrated()).expect("mark"), None, "no sync row");
    }

    #[test]
    fn a_replace_sets_the_place_to_the_snapshots_position_even_when_that_is_lower() {
        let conn = set_up();
        state::set_cursor(&conn, 50).expect("cursor");
        state::set_restored_seq(&conn, 60).expect("restored");
        note(&conn, 70, &[1; 32]).expect("note");
        assert_eq!(place(&conn).expect("place"), Some(70));
        // What a replace at 25 leaves: the notes wiped, the cursor and the restored position set.
        forget_all(&conn).expect("wipe");
        conn.execute("UPDATE sync_state SET cursor = 25", [])
            .expect("cursor");
        state::reset_restored_seq(&conn, 25).expect("reset");
        assert_eq!(place(&conn).expect("place"), Some(25));
    }

    #[test]
    fn the_place_is_none_while_sync_is_not_set_up() {
        let conn = migrated();
        assert_eq!(place(&conn).expect("place"), None);
        note(&conn, 5, &[1; 32]).expect("note");
        assert_eq!(
            place(&conn).expect("place"),
            None,
            "notes alone are no place"
        );
    }
}
