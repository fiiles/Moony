//! Re-join after a clone or rollback (§11.4) and the re-queues of this device's own rows that it
//! and a barrier crossing need.

use rand::{Rng, RngExt};
use rusqlite::{params, Connection};

use super::{loaded, set_head, unsigned, EnvelopeMeta};
use crate::error::{AppError, Result};
use crate::services::sync::capture;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::in_savepoint;

/// Queues every key (live or tombstoned) that has a clock of `device` newer than `since_hlc`, so
/// it goes out again with its original clocks. `queued_at` is the key's oldest such clock. Returns
/// how many keys joined the outbox (keys already queued stay as they are).
pub fn requeue_own_since(conn: &Connection, device: DeviceId, since_hlc: u64) -> Result<usize> {
    match since_hlc.checked_add(1) {
        Some(from) => requeue_own_from(conn, device, from),
        None => Ok(0),
    }
}

/// `requeue_own_since` with an inclusive bound: every key with a clock of `device` at `from_hlc`
/// or newer.
fn requeue_own_from(conn: &Connection, device: DeviceId, from_hlc: u64) -> Result<usize> {
    let from = i64::try_from(from_hlc)
        .map_err(|_| AppError::Internal(format!("sync chain: clock {from_hlc} is out of range")))?;
    Ok(conn.execute(
        "INSERT INTO sync_outbox (tbl, pk, queued_at) \
         SELECT tbl, pk, MIN(hlc) FROM sync_clock WHERE device = ?1 AND hlc >= ?2 GROUP BY tbl, pk \
         ON CONFLICT DO NOTHING",
        params![device.0.to_vec(), from],
    )?)
}

/// A device ID for a re-join of `old`, drawn uniformly from the IDs above it (T8-1 addendum).
/// The new ID must sort above the old one: a re-stamped column clock `(h, new)` then beats the
/// `(h, old)` the relay may still carry on every device, so the clocks converge. An error only
/// when `old` is the largest ID.
pub fn rejoin_device_id(old: DeviceId) -> Result<DeviceId> {
    id_above(old, &mut rand::rng())
}

fn id_above(old: DeviceId, rng: &mut impl Rng) -> Result<DeviceId> {
    let floor = u128::from_be_bytes(old.0);
    if floor == u128::MAX {
        return Err(AppError::Internal(
            "sync chain: no device ID sorts above the largest one".into(),
        ));
    }
    Ok(DeviceId(
        rng.random_range(floor + 1..=u128::MAX).to_be_bytes(),
    ))
}

/// What a re-join did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RejoinReport {
    /// The device ID this profile used before the re-join.
    pub old_device: DeviceId,
    /// Keys that joined the outbox (keys already queued stay as they are).
    pub queued: usize,
    /// Column clocks of the old ID moved to the new ID with a fresh clock.
    pub restamped: usize,
}

/// One `sync_own_log` entry, with whether it still waits in `sync_unsent`.
struct OwnEntry {
    seq: u64,
    hash: [u8; 32],
    min_hlc: u64,
    max_hlc: u64,
    unsent: bool,
}

fn own_log(conn: &Connection) -> Result<Vec<OwnEntry>> {
    let raw: Vec<(i64, Vec<u8>, i64, i64, bool)> = conn
        .prepare(
            "SELECT l.seq, l.hash, l.min_hlc, l.max_hlc, \
               EXISTS (SELECT 1 FROM sync_unsent u WHERE u.seq = l.seq) \
             FROM sync_own_log l ORDER BY l.seq",
        )?
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    raw.into_iter()
        .map(|(seq, hash, min_hlc, max_hlc, unsent)| {
            Ok(OwnEntry {
                seq: unsigned(seq, "seq")?,
                hash: hash
                    .as_slice()
                    .try_into()
                    .map_err(|_| AppError::Internal("sync_own_log.hash is not 32 bytes".into()))?,
                min_hlc: unsigned(min_hlc, "min_hlc")?,
                max_hlc: unsigned(max_hlc, "max_hlc")?,
                unsent,
            })
        })
        .collect()
}

/// Re-join after a clone or rollback (§11.4). `trigger` is the own-ID envelope that judged
/// `Clone`; `None` for a re-join found at unlock (keychain mismatch, restore marker).
///
/// The confirmed prefix is the run of own envelopes from the start of the log that the relay
/// acknowledged (no longer in `sync_unsent`), up to the first one that is not, and below the
/// trigger's sequence (an own envelope at or above it may be the losing side of a fork). Its newest
/// entry becomes the old ID's head, so the old self's later envelopes (the trigger included) apply
/// like any other device's. When the prefix ends below `trigger.seq − 1` (it may be empty), the
/// trigger's `prev` names the old ID's envelope before it, which this device has already passed in
/// relay order (a snapshot cursor jump can pass envelopes it never acknowledged):
/// `(trigger.seq − 1, trigger.prev)` becomes the head, so the trigger applies instead of waiting
/// for a gap that never fills (ruling T8-3).
///
/// Every own clock at or above
/// `W = min(max_hlc of the prefix, min_hlc of every own envelope outside it)` is queued again:
/// `max_hlc` is not monotonic in `seq`, and an envelope outside the prefix may carry clocks below
/// the prefix's newest. When the own log holds no entry `(trigger.seq − 1, trigger.prev)`, the
/// relay's chain up to the trigger is not known to be this device's, and `W` drops to the oldest
/// `min_hlc` of the whole log. Re-queuing too much is harmless; too little loses edits.
///
/// `new_device` must sort above the current ID (draw it with `rejoin_device_id`), or the re-join
/// is refused before anything is written.
///
/// Then the device continues as `new_device` with a fresh chain (its own log and unsent envelopes
/// go), capture is reinstalled for the new ID when it was installed, and the old ID's column
/// clocks from `W` on move to the new ID with their hlc kept (`restamp_own_columns`, ruling
/// T8-1). Row clocks and tombstones keep their old ID.
pub fn rejoin(
    conn: &Connection,
    new_device: DeviceId,
    trigger: Option<&EnvelopeMeta>,
) -> Result<RejoinReport> {
    in_savepoint(conn, "sync_rejoin", || {
        let old = loaded(conn)?.device;
        if new_device <= old {
            return Err(AppError::Internal(format!(
                "sync chain: the re-join device {} must sort above {} (rejoin_device_id)",
                new_device.to_hex(),
                old.to_hex()
            )));
        }
        let log = own_log(conn)?;
        let mut prefix = 0;
        for (i, entry) in log.iter().enumerate() {
            let contiguous = i == 0 || entry.seq == log[i - 1].seq + 1;
            if entry.unsent || !contiguous || trigger.is_some_and(|t| entry.seq >= t.seq) {
                break;
            }
            prefix = i + 1;
        }
        let (confirmed, outside) = log.split_at(prefix);
        let prefix_max = confirmed.iter().map(|e| e.max_hlc).max().unwrap_or(0);
        let outside_min = outside.iter().map(|e| e.min_hlc).min().unwrap_or(u64::MAX);
        let mut watermark = prefix_max.min(outside_min);
        // The old ID's envelope just before the trigger, as the trigger names it.
        let before_trigger = match trigger {
            Some(t) if t.seq > 1 => Some((
                t.seq - 1,
                t.prev.ok_or_else(|| {
                    AppError::Internal(format!("sync chain: re-join trigger {} has no prev", t.seq))
                })?,
            )),
            _ => None,
        };
        if let Some((seq, hash)) = before_trigger {
            if !log.iter().any(|e| e.seq == seq && e.hash == hash) {
                let oldest = log.iter().map(|e| e.min_hlc).min().unwrap_or(watermark);
                watermark = watermark.min(oldest);
            }
        }
        let head = match (confirmed.last(), before_trigger) {
            (Some(last), Some(named)) if last.seq < named.0 => Some(named),
            (Some(last), _) => Some((last.seq, last.hash)),
            (None, named) => named,
        };
        if let Some((seq, hash)) = head {
            set_head(conn, old, seq, hash)?;
        }
        let queued = requeue_own_from(conn, old, watermark)?;
        conn.execute(
            "UPDATE sync_state SET device_id = ?1, next_seq = 1, last_hash = NULL WHERE id = 1",
            [new_device.0.to_vec()],
        )?;
        conn.execute_batch("DELETE FROM sync_own_log; DELETE FROM sync_unsent;")?;
        if capture::is_installed(conn)? {
            capture::install(conn, new_device)?;
        }
        let restamped = restamp_own_columns(conn, old, new_device, watermark)?;
        Ok(RejoinReport {
            old_device: old,
            queued,
            restamped,
        })
    })
}

/// Moves every column clock of `old` at `from_hlc` or newer to `new`, keeping its hlc (ruling
/// T8-1): `(hlc, old)` becomes `(hlc, new)`. A rolled-back copy and the original can both have
/// issued `(hlc, old)` with different values; after the move the two differ by device and every
/// device orders them alike. Keeping the hlc means a re-sent edit never beats a later edit of
/// another device. The columns of a row (and so every column group) move together. Row clocks
/// (`'*'`) and tombstones (`'-'`) stay as they are. Returns how many column clocks moved.
pub fn restamp_own_columns(
    conn: &Connection,
    old: DeviceId,
    new: DeviceId,
    from_hlc: u64,
) -> Result<usize> {
    let from = i64::try_from(from_hlc)
        .map_err(|_| AppError::Internal(format!("sync chain: clock {from_hlc} is out of range")))?;
    Ok(conn.execute(
        "UPDATE sync_clock SET device = ?1 \
         WHERE device = ?2 AND hlc >= ?3 AND col NOT IN ('*', '-')",
        params![new.0.to_vec(), old.0.to_vec(), from],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::chain::tests::*;
    use crate::services::sync::chain::{judge, Verdict};
    use crate::services::sync::changeset::SqlValue;
    use crate::services::sync::hlc::Clock;
    use crate::services::sync::merge::apply_changeset;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    #[test]
    fn rejoin_requeues_own_rows_newer_than_the_last_confirmed_envelope() {
        let conn = profile();
        own_clocks(&conn);
        // max_hlc is not monotonic in seq: the watermark is the prefix's highest, 300.
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 280, 300, false);
        sealed(&conn, 3, 3, 150, 200, false);

        assert_eq!(
            rejoin(&conn, NEW, Some(&meta(ME, 4, Some(3), 44)))
                .unwrap()
                .queued,
            2
        );

        assert_eq!(
            outbox(&conn),
            vec![queued("dead", 360), queued("i350", 350)]
        );
        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 3, h(3).to_vec(), false)]);
    }

    #[test]
    fn rejoin_also_requeues_rows_of_own_envelopes_the_relay_never_confirmed() {
        let conn = profile();
        own_clocks(&conn);
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 150, 200, false);
        sealed(&conn, 3, 3, 260, 300, true);

        assert_eq!(rejoin(&conn, NEW, None).unwrap().queued, 3);

        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 2, h(2).to_vec(), false)]);
    }

    #[test]
    fn rejoin_after_a_forked_relay_chain_requeues_the_own_side_of_the_fork() {
        let conn = profile();
        own_clocks(&conn);
        // This copy's seq 3 (hash 3, holding i250 at 250/270) was confirmed by the relay, and so
        // was the other copy's seq 3 (hash 33): the trigger.
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 150, 200, false);
        sealed(&conn, 3, 3, 250, 270, false);
        let trigger = meta(ME, 3, Some(2), 33);
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Clone);

        assert_eq!(rejoin(&conn, NEW, Some(&trigger)).unwrap().queued, 3);

        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 2, h(2).to_vec(), false)]);
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Apply);
    }

    #[test]
    fn rejoin_requeues_an_unsent_envelope_below_a_confirmed_one() {
        let conn = profile();
        own_clocks(&conn);
        // Seq 2 never reached the relay and holds i250 (250/270), below seq 1's max_hlc of 300.
        sealed(&conn, 1, 1, 90, 300, false);
        sealed(&conn, 2, 2, 250, 270, true);
        sealed(&conn, 3, 3, 340, 360, false);

        assert_eq!(rejoin(&conn, NEW, None).unwrap().queued, 3);

        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 1, h(1).to_vec(), false)]);
    }

    #[test]
    fn rejoin_keeps_rows_already_in_the_outbox() {
        let conn = profile();
        own_clocks(&conn);
        conn.execute_batch(
            "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES \
               ('institutions', 'i350', 5), ('bonds', 'b1', 7)",
        )
        .unwrap();
        sealed(&conn, 1, 1, 90, 300, false);

        assert_eq!(rejoin(&conn, NEW, None).unwrap().queued, 1);

        assert_eq!(
            outbox(&conn),
            vec![
                ("bonds".to_string(), "b1".to_string(), 7),
                queued("dead", 360),
                queued("i350", 5)
            ]
        );
    }

    #[test]
    fn rejoin_takes_a_new_device_id_and_restarts_the_chain() {
        let conn = migrated();
        state::enable(&conn, ME).unwrap();
        conn.execute_batch("DELETE FROM sync_outbox; DELETE FROM sync_clock;")
            .unwrap();
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 150, 200, false);
        sealed(&conn, 3, 3, 250, 300, true);
        conn.execute(
            "UPDATE sync_state SET next_seq = 4, last_hash = ?1",
            [h(3).to_vec()],
        )
        .unwrap();

        rejoin(&conn, NEW, Some(&meta(ME, 3, Some(2), 33))).unwrap();

        let sync = state::load(&conn).unwrap().expect("row");
        assert_eq!(sync.device, NEW);
        assert_eq!(sync.next_seq, 1);
        assert_eq!(sync.last_hash, None);
        assert!(sync.enabled);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_own_log"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_unsent"), 0);
        // The old self's newest confirmed envelope is its head: its next one applies.
        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 2, h(2).to_vec(), false)]);
        assert_eq!(
            judge(&conn, &meta(ME, 3, Some(2), 33)).unwrap(),
            Verdict::Apply
        );
        assert_eq!(
            judge(&conn, &meta(NEW, 1, None, 1)).unwrap(),
            Verdict::Clone
        );
        // Capture now clocks local edits with the new ID.
        assert!(capture::is_installed(&conn).unwrap());
        conn.execute_batch("INSERT INTO institutions (id, name) VALUES ('fresh', 'Bank')")
            .unwrap();
        let device: Vec<u8> = conn
            .query_row(
                "SELECT device FROM sync_clock WHERE pk = 'fresh' AND col = '*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(device, NEW.0.to_vec());
    }

    #[test]
    fn rejoin_keeps_capture_off_when_it_was_off() {
        let conn = profile();
        rejoin(&conn, NEW, None).unwrap();
        assert!(!capture::is_installed(&conn).unwrap());
        assert!(heads(&conn).is_empty(), "nothing confirmed, no head");
    }

    /// The `(hlc, device)` of one `institutions` clock.
    fn clock_of(conn: &Connection, pk: &str, col: &str) -> (i64, DeviceId) {
        let (hlc, device): (i64, Vec<u8>) = conn
            .query_row(
                "SELECT hlc, device FROM sync_clock WHERE tbl = 'institutions' AND pk = ?1 AND col = ?2",
                [pk, col],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect(pk);
        (hlc, DeviceId(device.try_into().expect("16 bytes")))
    }

    #[test]
    fn rejoin_with_an_empty_prefix_takes_the_trigger_prev_as_the_old_head() {
        let conn = profile();
        sealed(&conn, 1, 1, 90, 100, true);
        sealed(&conn, 2, 2, 150, 200, true);
        sealed(&conn, 3, 3, 250, 300, true);
        let trigger = meta(ME, 4, Some(30), 4);
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Clone);

        let report = rejoin(&conn, NEW, Some(&trigger)).unwrap();

        assert_eq!(report.old_device, ME);
        assert_eq!(
            heads(&conn),
            vec![(ME.0.to_vec(), 3, h(30).to_vec(), false)]
        );
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Apply);
    }

    #[test]
    fn rejoin_restamps_own_column_clocks_under_the_new_device() {
        let conn = profile();
        own_clocks(&conn);
        clock(&conn, "i150", "name", 160, ME);
        clock(&conn, "theirs", "name", 410, A);
        // W = 200: i250's name (270) is re-stamped, i150's (160) is not.
        sealed(&conn, 1, 1, 90, 200, false);

        let report = rejoin(&conn, NEW, None).unwrap();

        assert_eq!(
            report,
            RejoinReport {
                old_device: ME,
                queued: 3,
                restamped: 1,
            }
        );
        assert_eq!(clock_of(&conn, "i250", "name"), (270, NEW));
        assert_eq!(clock_of(&conn, "i150", "name"), (160, ME));
        assert_eq!(clock_of(&conn, "theirs", "name"), (410, A));
        assert!(!capture::is_installed(&conn).unwrap(), "capture stays off");
    }

    #[test]
    fn rejoin_keeps_row_clocks_and_tombstones_of_the_old_device() {
        let conn = profile();
        own_clocks(&conn);
        sealed(&conn, 1, 1, 90, 200, false);

        rejoin(&conn, NEW, None).unwrap();

        assert_eq!(clock_of(&conn, "i250", "*"), (250, ME));
        assert_eq!(clock_of(&conn, "i350", "*"), (350, ME));
        assert_eq!(clock_of(&conn, "dead", "-"), (360, ME));
        assert_eq!(clock_of(&conn, "i150", "*"), (150, ME));
        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
    }

    #[test]
    fn restamped_columns_keep_their_hlc_under_the_new_device() {
        let conn = profile();
        clock(&conn, "r", "*", 100, ME);
        clock(&conn, "r", "name", 260, ME);
        clock(&conn, "r", "website", 280, ME);
        clock(&conn, "s", "*", 110, ME);
        clock(&conn, "s", "name", 270, ME);
        clock(&conn, "theirs", "*", 400, A);
        sealed(&conn, 1, 1, 90, 200, false);

        let report = rejoin(&conn, NEW, None).unwrap();

        assert_eq!(report.restamped, 3);
        assert_eq!(clock_of(&conn, "r", "name"), (260, NEW));
        assert_eq!(clock_of(&conn, "r", "website"), (280, NEW));
        assert_eq!(clock_of(&conn, "s", "name"), (270, NEW));
        assert_eq!(clock_of(&conn, "r", "*"), (100, ME));
        assert_eq!(clock_of(&conn, "s", "*"), (110, ME));
        assert_eq!(clock_of(&conn, "theirs", "*"), (400, A));
    }

    #[test]
    fn a_remote_edit_newer_than_the_own_clock_still_wins_after_a_rejoin() {
        let x = migrated();
        state::enable(&x, ME).unwrap();
        x.execute_batch(
            "DELETE FROM sync_outbox; DELETE FROM sync_clock; \
             INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             UPDATE institutions SET name = 'From X' WHERE id = 'i1';",
        )
        .unwrap();
        let from_x = sealed_changesets(&x);
        let z = migrated();
        state::enable(&z, A).unwrap();
        z.execute_batch("DELETE FROM sync_outbox; DELETE FROM sync_clock;")
            .unwrap();
        for (cs, hash) in &from_x {
            apply_changeset(&z, cs, *hash, 0).unwrap();
        }
        // Z renames after X's edit and before X re-joins.
        z.execute_batch("UPDATE institutions SET name = 'From Z' WHERE id = 'i1'")
            .unwrap();
        let from_z = sealed_changesets(&z);
        let (own_hlc, _) = clock_of(&x, "i1", "name");
        let (remote_hlc, _) = clock_of(&z, "i1", "name");
        assert!(own_hlc < remote_hlc);

        let report = rejoin(&x, NEW, None).unwrap();
        assert_eq!(report.restamped, 1);
        assert_eq!(clock_of(&x, "i1", "name"), (own_hlc, NEW));
        // X re-sends its edit, with its hlc, before it hears of Z's.
        let resent = sealed_changesets(&x);
        assert_eq!(
            resent[0].0.rows[0].columns["name"],
            (
                SqlValue::Text("From X".into()),
                Clock {
                    hlc: own_hlc as u64,
                    device: NEW
                }
            )
        );
        for (cs, hash) in &resent {
            apply_changeset(&z, cs, *hash, 0).unwrap();
        }
        for (cs, hash) in &from_z {
            apply_changeset(&x, cs, *hash, 0).unwrap();
        }

        for conn in [&x, &z] {
            let name: String = conn
                .query_row("SELECT name FROM institutions WHERE id = 'i1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(name, "From Z");
            assert_eq!(clock_of(conn, "i1", "name"), (remote_hlc, A));
        }
    }

    #[test]
    fn rejoin_after_a_snapshot_cursor_jump_takes_the_trigger_prev_as_the_old_head() {
        // Seq 2 and 3 reached the relay, but a restore by merge moved the cursor past them before
        // they were acknowledged; the clone's seq 4 follows the relay's seq 3.
        let conn = profile();
        own_clocks(&conn);
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 150, 200, true);
        sealed(&conn, 3, 3, 250, 300, true);
        let trigger = meta(ME, 4, Some(3), 4);

        let report = rejoin(&conn, NEW, Some(&trigger)).unwrap();

        assert_eq!(heads(&conn), vec![(ME.0.to_vec(), 3, h(3).to_vec(), false)]);
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Apply);
        // W = min(100, 150): the own log holds (3, h3), so it is not lowered.
        assert_eq!(report.queued, 4);
        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i150", 150),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
    }

    #[test]
    fn rejoin_lowers_the_watermark_when_the_trigger_prev_is_not_in_the_own_log() {
        let conn = profile();
        own_clocks(&conn);
        // Without the lowering W = min(300, 340) = 300 would queue only i350 and dead.
        sealed(&conn, 1, 1, 90, 300, false);
        sealed(&conn, 2, 2, 340, 360, true);
        // The relay's seq 2 is another copy's (hash 22), not this one's.
        let trigger = meta(ME, 3, Some(22), 3);

        let report = rejoin(&conn, NEW, Some(&trigger)).unwrap();

        assert_eq!(report.queued, 4);
        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i150", 150),
                queued("i250", 250),
                queued("i350", 350)
            ]
        );
        assert_eq!(
            heads(&conn),
            vec![(ME.0.to_vec(), 2, h(22).to_vec(), false)]
        );
        assert_eq!(judge(&conn, &trigger).unwrap(), Verdict::Apply);
    }

    #[test]
    fn rejoin_requeue_ships_parents_before_children() {
        let conn = migrated();
        state::enable(&conn, ME).unwrap();
        let created = account_then_transaction(&conn);
        // The relay confirmed an envelope up to just after the account's creation.
        sealed(&conn, 1, 1, created as i64, created as i64 + 1, false);

        let report = rejoin(&conn, NEW, None).unwrap();

        assert_eq!(report.queued, 2);
        assert_child_queued_first(&conn);
        assert_parent_first(&conn, &sealed_rows(&conn));
    }

    #[test]
    fn rejoin_refuses_a_new_id_that_does_not_sort_above_the_old_one() {
        let conn = profile();
        own_clocks(&conn);
        sealed(&conn, 1, 1, 90, 200, false);

        for lower in [DeviceId([3; 16]), ME] {
            assert!(rejoin(&conn, lower, None).is_err());
        }

        let sync = state::load(&conn).unwrap().expect("row");
        assert_eq!(sync.device, ME);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_own_log"), 1);
        assert!(outbox(&conn).is_empty());
        assert!(heads(&conn).is_empty());
        assert_eq!(clock_of(&conn, "i250", "name"), (270, ME));
    }

    #[test]
    fn a_rejoin_device_id_always_sorts_above_the_old_one() {
        let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(8);
        let mut near_max = [0xFF; 16];
        near_max[15] = 0xFE;
        for old in [DeviceId::ZERO, ME, DeviceId([0xF0; 16]), DeviceId(near_max)] {
            for _ in 0..1_000 {
                assert!(id_above(old, &mut rng).unwrap() > old);
            }
        }
        assert_eq!(
            id_above(DeviceId(near_max), &mut rng).unwrap(),
            DeviceId([0xFF; 16])
        );
        assert!(id_above(DeviceId([0xFF; 16]), &mut rng).is_err());
        for _ in 0..1_000 {
            assert!(rejoin_device_id(ME).unwrap() > ME);
        }
        assert!(rejoin_device_id(DeviceId([0xFF; 16])).is_err());
    }

    #[test]
    fn restamped_clocks_converge_when_a_random_id_would_have_sorted_below() {
        // Most random IDs sort below this one; the re-join draws one above it.
        let old = DeviceId([0xF0; 16]);
        let x = migrated();
        state::enable(&x, old).unwrap();
        x.execute_batch(
            "DELETE FROM sync_outbox; DELETE FROM sync_clock; \
             INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             UPDATE institutions SET name = 'From X' WHERE id = 'i1';",
        )
        .unwrap();
        let from_x = sealed_changesets(&x);
        let y = migrated();
        state::enable(&y, A).unwrap();
        y.execute_batch("DELETE FROM sync_outbox; DELETE FROM sync_clock;")
            .unwrap();
        for (cs, hash) in &from_x {
            apply_changeset(&y, cs, *hash, 0).unwrap();
        }
        let (h, _) = clock_of(&x, "i1", "name");
        assert_eq!(clock_of(&y, "i1", "name"), (h, old));

        let new = rejoin_device_id(old).unwrap();
        assert_eq!(rejoin(&x, new, None).unwrap().restamped, 1);
        for (cs, hash) in &sealed_changesets(&x) {
            apply_changeset(&y, cs, *hash, 0).unwrap();
        }

        assert_eq!(clock_of(&x, "i1", "name"), (h, new));
        assert_eq!(clock_of(&y, "i1", "name"), (h, new));
    }
}
