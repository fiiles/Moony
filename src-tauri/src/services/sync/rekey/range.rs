//! The digest of a range of the server's log that a key change's signed head binds (version 2
//! of `head::HeadRecord`): every op above `base` up to and including `head`, as the remover
//! read it, noted by position and envelope hash.

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::services::sync::in_savepoint;

const RANGE_DOMAIN: &[u8] = b"moony rekey range v1";

/// One noted op of the range: where it stands and the envelope hash over its whole sealed bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeNote {
    pub pos: u64,
    pub hash: [u8; 32],
}

/// `SHA-256("moony rekey range v1" ‖ vault_id ‖ to_epoch (u32) ‖ base (u64) ‖ head (u64) ‖
/// count (u32) ‖ for each note in ascending position: position (u64) ‖ hash (32))`, integers
/// big-endian. Refuses notes that are not strictly ascending or lie outside `(base, head]`.
pub fn range_digest(
    vault_id: &[u8; 16],
    to_epoch: u32,
    base: u64,
    head: u64,
    notes: &[RangeNote],
) -> Result<[u8; 32]> {
    let bad = || AppError::Internal("sync: the range of a key change is not valid".into());
    let count = u32::try_from(notes.len()).map_err(|_| bad())?;
    if base > head {
        return Err(bad());
    }
    let mut previous = base;
    for note in notes {
        if note.pos <= previous || note.pos > head {
            return Err(bad());
        }
        previous = note.pos;
    }
    let mut hasher = Sha256::new();
    hasher.update(RANGE_DOMAIN);
    hasher.update(vault_id);
    hasher.update(to_epoch.to_be_bytes());
    hasher.update(base.to_be_bytes());
    hasher.update(head.to_be_bytes());
    hasher.update(count.to_be_bytes());
    for note in notes {
        hasher.update(note.pos.to_be_bytes());
        hasher.update(note.hash);
    }
    Ok(hasher.finalize().into())
}

fn signed(value: u64, what: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync: {what} is out of range: {value}")))
}

/// Replaces the notes of `epoch` with `notes` and records the `base` and the number of notes on
/// that epoch's `sync_epochs` row, in one savepoint. An epoch without a row is an error: notes
/// are never stored for an epoch this device does not know.
pub fn store_notes(conn: &Connection, epoch: u32, base: u64, notes: &[RangeNote]) -> Result<()> {
    let base = signed(base, "range base")?;
    let count = signed(notes.len() as u64, "range count")?;
    in_savepoint(conn, "sync_store_range_notes", || {
        let marked = conn.execute(
            "UPDATE sync_epochs SET range_base = ?2, range_count = ?3 WHERE epoch = ?1",
            params![epoch, base, count],
        )?;
        if marked == 0 {
            return Err(AppError::Internal(format!(
                "sync: no epoch {epoch} to store a range for"
            )));
        }
        conn.execute("DELETE FROM sync_range_notes WHERE epoch = ?1", [epoch])?;
        let mut insert = conn.prepare_cached(
            "INSERT INTO sync_range_notes (epoch, pos, hash) VALUES (?1, ?2, ?3)",
        )?;
        for note in notes {
            insert.execute(params![
                epoch,
                signed(note.pos, "note position")?,
                note.hash
            ])?;
        }
        Ok(())
    })
}

/// The base of the range stored for `epoch`; `None` while none is stored.
pub fn base_of(conn: &Connection, epoch: u32) -> Result<Option<u64>> {
    let base: Option<Option<i64>> = conn
        .query_row(
            "SELECT range_base FROM sync_epochs WHERE epoch = ?1",
            [epoch],
            |r| r.get(0),
        )
        .optional()?;
    base.flatten()
        .map(|b| {
            u64::try_from(b).map_err(|_| {
                AppError::Internal(format!("sync_epochs.range_base is out of range: {b}"))
            })
        })
        .transpose()
}

/// The hash noted for `pos` in the range stored for `epoch`.
pub fn noted(conn: &Connection, epoch: u32, pos: u64) -> Result<Option<[u8; 32]>> {
    let hash: Option<Vec<u8>> = conn
        .query_row(
            "SELECT hash FROM sync_range_notes WHERE epoch = ?1 AND pos = ?2",
            params![epoch, signed(pos, "note position")?],
            |r| r.get(0),
        )
        .optional()?;
    hash.map(|h| {
        <[u8; 32]>::try_from(h.as_slice())
            .map_err(|_| AppError::Internal("sync_range_notes.hash is not 32 bytes".into()))
    })
    .transpose()
}

/// Drops the range stored for `epoch`: its notes, base and count.
pub fn forget(conn: &Connection, epoch: u32) -> Result<()> {
    in_savepoint(conn, "sync_forget_range_notes", || {
        conn.execute("DELETE FROM sync_range_notes WHERE epoch = ?1", [epoch])?;
        conn.execute(
            "UPDATE sync_epochs SET range_base = NULL, range_count = NULL WHERE epoch = ?1",
            [epoch],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    const VAULT: [u8; 16] = [0x22; 16];

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect("count")
    }

    fn with_epochs() -> Connection {
        let conn = migrated();
        state::record_epoch(&conn, 0, 0, None, 1).expect("epoch 0");
        state::record_epoch(&conn, 1, 10, None, 2).expect("epoch 1");
        conn
    }

    #[test]
    fn stored_notes_round_trip_and_a_second_store_replaces_the_first() {
        let conn = with_epochs();
        assert_eq!(base_of(&conn, 1).expect("base"), None);
        store_notes(&conn, 1, 10, &notes()).expect("store");
        assert_eq!(base_of(&conn, 1).expect("base"), Some(10));
        assert_eq!(base_of(&conn, 0).expect("other"), None);
        assert_eq!(noted(&conn, 1, 11).expect("note"), Some([0xaa; 32]));
        assert_eq!(noted(&conn, 1, 13).expect("note"), Some([0xbb; 32]));
        assert_eq!(noted(&conn, 1, 12).expect("note"), None);
        assert_eq!(noted(&conn, 0, 11).expect("other epoch"), None);
        let count_of: i64 = count(&conn, "SELECT range_count FROM sync_epochs WHERE epoch = 1");
        assert_eq!(count_of, 2);

        let fresh = [RangeNote {
            pos: 20,
            hash: [0xcc; 32],
        }];
        store_notes(&conn, 1, 15, &fresh).expect("replace");
        assert_eq!(base_of(&conn, 1).expect("base"), Some(15));
        assert_eq!(noted(&conn, 1, 11).expect("old"), None);
        assert_eq!(noted(&conn, 1, 20).expect("new"), Some([0xcc; 32]));
        assert_eq!(
            count(&conn, "SELECT range_count FROM sync_epochs WHERE epoch = 1"),
            1
        );
        store_notes(&conn, 1, 20, &[]).expect("empty range");
        assert_eq!(base_of(&conn, 1).expect("base"), Some(20));
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_range_notes"), 0);
    }

    #[test]
    fn notes_for_an_unknown_epoch_are_refused_and_leave_nothing() {
        let conn = with_epochs();
        let error = store_notes(&conn, 7, 10, &notes()).expect_err("no such epoch");
        assert!(matches!(error, AppError::Internal(_)));
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_range_notes"), 0);
    }

    #[test]
    fn a_position_above_the_integer_range_is_refused_and_keeps_the_old_notes() {
        let conn = with_epochs();
        store_notes(&conn, 1, 10, &notes()).expect("store");
        let huge = [RangeNote {
            pos: u64::MAX,
            hash: [1; 32],
        }];
        assert!(store_notes(&conn, 1, 10, &huge).is_err());
        assert_eq!(noted(&conn, 1, 11).expect("kept"), Some([0xaa; 32]));
        assert_eq!(base_of(&conn, 1).expect("base"), Some(10));
        assert!(store_notes(&conn, 1, u64::MAX, &[]).is_err());
    }

    #[test]
    fn forget_drops_one_epochs_range_only() {
        let conn = with_epochs();
        store_notes(&conn, 0, 0, &notes()).expect("store 0");
        store_notes(&conn, 1, 10, &notes()).expect("store 1");
        forget(&conn, 1).expect("forget");
        assert_eq!(base_of(&conn, 1).expect("base"), None);
        assert_eq!(noted(&conn, 1, 11).expect("note"), None);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM sync_epochs WHERE epoch = 1 AND range_count IS NULL"
            ),
            1
        );
        assert_eq!(base_of(&conn, 0).expect("base"), Some(0));
        assert_eq!(noted(&conn, 0, 13).expect("note"), Some([0xbb; 32]));
        forget(&conn, 9).expect("an unknown epoch is nothing to forget");
    }

    fn notes() -> Vec<RangeNote> {
        vec![
            RangeNote {
                pos: 11,
                hash: [0xaa; 32],
            },
            RangeNote {
                pos: 13,
                hash: [0xbb; 32],
            },
        ]
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_range_digest_matches_the_frozen_vector() {
        let two = range_digest(&VAULT, 4, 10, 13, &notes()).expect("digest");
        let empty = range_digest(&VAULT, 4, 13, 13, &[]).expect("digest");
        assert_eq!(
            hex(&two),
            "0c4975dbc30e2f9866ff89c16bdd9d42330faf65f2e5e009a7b2ddd98b08e870"
        );
        assert_eq!(
            hex(&empty),
            "eb3fb1176f17d5054b36bc1a56ec7cc143283c12f9295db531502e5fa38e8a11"
        );
    }

    #[test]
    fn a_digest_changes_with_every_field_and_with_the_order_of_notes_refused() {
        let base = range_digest(&VAULT, 4, 10, 13, &notes()).expect("digest");
        let mut other = notes();
        other[1].hash[0] ^= 1;
        let mut moved = notes();
        moved[0].pos = 12;
        let variants = [
            range_digest(&[0x23; 16], 4, 10, 13, &notes()),
            range_digest(&VAULT, 5, 10, 13, &notes()),
            range_digest(&VAULT, 4, 9, 13, &notes()),
            range_digest(&VAULT, 4, 10, 14, &notes()),
            range_digest(&VAULT, 4, 10, 13, &notes()[..1]),
            range_digest(&VAULT, 4, 10, 13, &other),
            range_digest(&VAULT, 4, 10, 13, &moved),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(v.as_ref().expect("digest"), &base, "variant {i}");
        }
        let swapped = vec![notes()[1], notes()[0]];
        assert!(range_digest(&VAULT, 4, 10, 13, &swapped).is_err());
        let twice = vec![notes()[0], notes()[0]];
        assert!(range_digest(&VAULT, 4, 10, 13, &twice).is_err());
        assert!(
            range_digest(&VAULT, 4, 11, 13, &notes()).is_err(),
            "at the base"
        );
        assert!(
            range_digest(&VAULT, 4, 10, 12, &notes()).is_err(),
            "above the head"
        );
    }
}
