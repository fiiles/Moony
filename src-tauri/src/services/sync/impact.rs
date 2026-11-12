//! What a replace (a join, a repair, a re-sync) puts at stake on this device, and the check that
//! nothing changed since the person agreed to it (audit M8; pre-flight C8, Minor 4 and Minor 5).
//!
//! The person agrees to the numbers a confirmation showed. Consent is a clock watermark, not a
//! count: `changes_through` is the newest clock of this device's hybrid logical clock that the
//! profile records (`sync_clock`, the own log's `max_hlc`, `sync_outbox.queued_at`), and every
//! local change after it carries a larger one. The check names exactly the changes made after the
//! person looked, whatever happened to the counts meanwhile: an outbox row queued after it, a
//! pending outbox key that this device clocked again after it (a row keeps its first `queued_at`
//! when it is edited again, and nothing is sealed while an envelope waits), or a sealed envelope
//! whose newest clock is after it and that a re-sync's own push did not hand to the relay. Clocks
//! other devices wrote, merged meanwhile, are not local changes. Items (outbox rows) and batches
//! (sealed envelopes) are different units and are counted apart.
//!
//! A batch stays in `sync_unsent` until this device pulls it back from the relay (audit M4), so
//! `pending_batches` counts envelopes the relay has not been seen to hold: never sent, or sent and
//! not echoed yet. A re-sync, which replaces right after its push without pulling, leaves out what
//! that push handed to the relay (`unsent_batches_besides`, `check_consent_after_push`).

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// Something changed on this device after the person agreed to the replace.
pub const IMPACT_CHANGED: &str = "validation.syncReplaceImpactChanged";
/// A repair replaces nothing while a change of this device has not reached the sync server.
pub const DRAIN_INCOMPLETE: &str = "validation.syncRepairDrainIncomplete";

/// Every table whose rows count as an account or asset in `ReplaceImpact::accounts`.
const ACCOUNT_TABLES: [&str; 8] = [
    "bank_accounts",
    "loans",
    "real_estate",
    "other_assets",
    "stock_investments",
    "crypto_investments",
    "bonds",
    "insurance_policies",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceKind {
    Join,
    Repair,
    Resync,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceImpact {
    /// `sync_outbox` rows: changed items not sealed yet.
    pub pending_items: u32,
    /// `sync_unsent` rows: sealed batches this device has not pulled back from the sync server
    /// (not sent yet, or sent and not echoed).
    pub pending_batches: u32,
    /// `real_estate_documents` + `insurance_documents`.
    pub documents: u32,
    /// `real_estate_photos`.
    pub photos: u32,
    /// The rows of `ACCOUNT_TABLES` (bonds included).
    pub accounts: u32,
    /// `bank_transactions`.
    pub transactions: u32,
    /// The newest clock the profile records: the largest of `sync_clock.hlc`,
    /// `sync_own_log.max_hlc` (a barrier marker ticks the clock without a `sync_clock` row) and
    /// `sync_outbox.queued_at`, 0 without any. Every later local change gets a larger clock.
    pub changes_through: u64,
}

impl ReplaceImpact {
    /// The consent of a confirmation that showed this impact: its watermark, and its documents and
    /// photos.
    pub fn consent(&self) -> ReplaceConsent {
        ReplaceConsent {
            changes_through: self.changes_through,
            attachments: self.documents.saturating_add(self.photos),
        }
    }
}

/// What the person agreed to: the impact's watermark, and the documents and photos a join showed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceConsent {
    pub changes_through: u64,
    pub attachments: u32,
}

/// The impact of a replace on this profile now.
pub fn replace_impact(conn: &Connection) -> Result<ReplaceImpact> {
    let mut accounts = 0u32;
    for table in ACCOUNT_TABLES {
        accounts = accounts
            .checked_add(rows(conn, table)?)
            .ok_or_else(too_many)?;
    }
    let documents = rows(conn, "real_estate_documents")?
        .checked_add(rows(conn, "insurance_documents")?)
        .ok_or_else(too_many)?;
    let newest: Option<i64> = conn.query_row(
        "SELECT MAX(hlc) FROM (SELECT MAX(hlc) AS hlc FROM sync_clock \
                               UNION ALL SELECT MAX(max_hlc) FROM sync_own_log \
                               UNION ALL SELECT MAX(queued_at) FROM sync_outbox)",
        [],
        |r| r.get(0),
    )?;
    Ok(ReplaceImpact {
        pending_items: rows(conn, "sync_outbox")?,
        pending_batches: rows(conn, "sync_unsent")?,
        documents,
        photos: rows(conn, "real_estate_photos")?,
        accounts,
        transactions: rows(conn, "bank_transactions")?,
        changes_through: newest.map_or(0, |hlc| u64::try_from(hlc).unwrap_or(0)),
    })
}

/// `check_consent_after_push` for a replace with no push before it.
pub fn check_consent(conn: &Connection, kind: ReplaceKind, consent: &ReplaceConsent) -> Result<()> {
    check_consent_after_push(conn, kind, consent, &[])
}

/// The `sync_unsent` rows other than `acknowledged` (what a re-sync's push just handed to the
/// relay): the batches that stay only in the backup.
pub fn unsent_batches_besides(conn: &Connection, acknowledged: &[u64]) -> Result<u32> {
    let left = unsent_seqs(conn)?
        .into_iter()
        .filter(|(seq, _)| !acknowledged.contains(seq))
        .count();
    u32::try_from(left).map_err(|_| too_many())
}

/// `(seq, max_hlc)` of every `sync_unsent` row, `max_hlc` from the own log (always there: both
/// rows are written in one transaction).
fn unsent_seqs(conn: &Connection) -> Result<Vec<(u64, Option<i64>)>> {
    let rows: Vec<(i64, Option<i64>)> = conn
        .prepare(
            "SELECT u.seq, l.max_hlc FROM sync_unsent u \
             LEFT JOIN sync_own_log l ON l.seq = u.seq ORDER BY u.seq",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .map(|(seq, max_hlc)| {
            let seq = u64::try_from(seq).map_err(|_| {
                AppError::Internal("sync impact: a negative unsent sequence".into())
            })?;
            Ok((seq, max_hlc))
        })
        .collect()
}

/// Join and Resync: a watermark above the newest clock the profile records → `IMPACT_CHANGED`
/// (a forged consent only ever refuses). Join: documents + photos other than
/// `consent.attachments` → `IMPACT_CHANGED`. Resync: an outbox row
/// queued after `consent.changes_through` (`queued_at`), a pending outbox key with a `sync_clock`
/// entry of this device after it, or an unsent envelope whose own-log `max_hlc` is after it and
/// that is not in `acknowledged` (the re-sync's push handed those to the relay: not lost, so not a
/// change the backup alone holds) → `IMPACT_CHANGED`. Repair: any pending item or batch →
/// `validation.syncRepairDrainIncomplete` (the caller drains again; its drain ends only once
/// everything came back).
pub fn check_consent_after_push(
    conn: &Connection,
    kind: ReplaceKind,
    consent: &ReplaceConsent,
    acknowledged: &[u64],
) -> Result<()> {
    if kind != ReplaceKind::Repair {
        // A forged consent only ever refuses: clocks only grow, so a watermark above the newest
        // clock this profile records, or a join's attachments other than the ones it has, is not
        // an impact this profile ever showed.
        let impact = replace_impact(conn)?;
        let forged = consent.changes_through > impact.changes_through
            || (kind == ReplaceKind::Join
                && consent.attachments != impact.documents.saturating_add(impact.photos));
        if forged {
            return Err(AppError::Validation(IMPACT_CHANGED.into()));
        }
    }
    match kind {
        ReplaceKind::Join => {}
        ReplaceKind::Resync => {
            // Clocks are stored as INTEGER; a watermark beyond i64 covers every one of them.
            //
            // The `sync_clock` test names this device's ID as it is now. A re-sync of a copied
            // profile re-joins between the consent and this hold (`rejoin_if_copy_under_lock`):
            // an edit after consent then keeps the old ID on its row clock or tombstone (the
            // re-join moves only column clocks), and that test misses it. It is caught anyway
            // because the re-join wipes `sync_unsent` and its `record_this_device` queues this
            // device's `sync_devices` row at a clock above the watermark: the re-sync's push seals
            // the edit again under the new ID (the third test reads its clock in `max_hlc` while
            // that push has not handed it to the relay; once it has, the edit is not lost and
            // does not count), and what it could not seal waits in the outbox next to that row
            // (the first two) until a push delivers it. Left: a push that delivers the row but
            // leaves an old-ID edit unsealed (a row too large to send); the outcome still counts it
            // as unsent and the backup stays kept. Keep this in mind when changing the re-join or
            // this check.
            let through = i64::try_from(consent.changes_through).unwrap_or(i64::MAX);
            let queued_later: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM sync_outbox WHERE queued_at > ?1) \
                     OR EXISTS (SELECT 1 FROM sync_outbox o \
                                JOIN sync_clock c ON c.tbl = o.tbl AND c.pk = o.pk \
                                WHERE c.hlc > ?1 \
                                  AND c.device = (SELECT device_id FROM sync_state WHERE id = 1))",
                [through],
                |r| r.get(0),
            )?;
            let sealed_later = unsent_seqs(conn)?.into_iter().any(|(seq, max_hlc)| {
                max_hlc.is_some_and(|hlc| hlc > through) && !acknowledged.contains(&seq)
            });
            if queued_later || sealed_later {
                return Err(AppError::Validation(IMPACT_CHANGED.into()));
            }
        }
        ReplaceKind::Repair => {
            let impact = replace_impact(conn)?;
            if impact.pending_items > 0 || impact.pending_batches > 0 {
                return Err(AppError::Validation(DRAIN_INCOMPLETE.into()));
            }
        }
    }
    Ok(())
}

/// The rows of `table` (a name from this module, never from input).
fn rows(conn: &Connection, table: &str) -> Result<u32> {
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM main.{table}"), [], |r| {
        r.get(0)
    })?;
    u32::try_from(n).map_err(|_| too_many())
}

fn too_many() -> AppError {
    AppError::Internal("sync impact: more rows than a count holds".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::test_support::migrated;

    fn exec(conn: &Connection, sql: &str) {
        conn.execute_batch(sql).expect(sql);
    }

    fn key(result: Result<()>) -> Option<String> {
        match result {
            Ok(()) => None,
            Err(AppError::Validation(key)) => Some(key),
            Err(other) => panic!("not a validation error: {other:?}"),
        }
    }

    #[test]
    fn the_impact_counts_what_a_replace_touches() {
        let conn = migrated();
        exec(
            &conn,
            "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Checking'), ('a2', 'Savings');
             INSERT INTO bonds (id, name, coupon_value) VALUES ('bond1', 'Bond', '10');
             INSERT INTO insurance_policies
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency)
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly');
             INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat');
             INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date)
             VALUES ('t1', 'a1', 'debit', '-5', 'CZK', 1767225600),
                    ('t2', 'a1', 'credit', '7', 'CZK', 1767225600),
                    ('t3', 'a2', 'debit', '-1', 'CZK', 1767225600);
             INSERT INTO insurance_documents (id, insurance_id, name, blob_id) VALUES ('d1', 'p1', 'Contract', 'b1');
             INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id) VALUES ('d2', 'r1', 'Deed', 'b2');
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) VALUES ('pb', 'r1', 1767225600);
             INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) VALUES ('ph', 'pb', 'b3', 'b4');
             INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i1', 5), ('institutions', 'i2', 6);
             INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00');",
        );

        let impact = replace_impact(&conn).expect("impact");

        assert_eq!(
            impact,
            ReplaceImpact {
                pending_items: 2,
                pending_batches: 1,
                documents: 2,
                photos: 1,
                accounts: 5,
                transactions: 3,
                changes_through: 6,
            },
            "bonds count among accounts; items and batches apart; the outbox's newest clock"
        );
    }

    #[test]
    fn changes_through_is_the_newest_clock() {
        let conn = migrated();
        assert_eq!(replace_impact(&conn).expect("impact").changes_through, 0);
        exec(
            &conn,
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES
               ('institutions', 'i1', '*', 70, x'01'),
               ('institutions', 'i2', '*', 900, x'02'),
               ('institutions', 'i2', 'name', 300, x'01');",
        );

        assert_eq!(replace_impact(&conn).expect("impact").changes_through, 900);
    }

    #[test]
    fn consent_is_checked_per_kind() {
        let conn = migrated();
        this_device(&conn);
        exec(
            &conn,
            &format!(
                "INSERT INTO insurance_policies
                   (id, type, provider, policy_name, policy_number, start_date, payment_frequency)
                 VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly');
                 INSERT INTO insurance_documents (id, insurance_id, name, blob_id)
                 VALUES ('d1', 'p1', 'C', 'b1');
                 INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i0', '*', 100, {THIS});"
            ),
        );
        let agreed = replace_impact(&conn).expect("impact").consent();
        assert_eq!(
            agreed,
            ReplaceConsent {
                changes_through: 100,
                attachments: 1
            }
        );
        for kind in [ReplaceKind::Join, ReplaceKind::Resync, ReplaceKind::Repair] {
            assert_eq!(key(check_consent(&conn, kind, &agreed)), None, "{kind:?}");
        }

        // Join: attachments other than agreed.
        exec(
            &conn,
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) VALUES ('d2', 'p1', 'C', 'b2');",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Join, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );
        exec(&conn, "DELETE FROM insurance_documents WHERE id = 'd2'");

        // Resync: an item queued at or before the watermark passes, one after it does not.
        exec(
            &conn,
            "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i1', 100);",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );
        // Repair: any pending item.
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Repair, &agreed)).as_deref(),
            Some(DRAIN_INCOMPLETE)
        );
        exec(
            &conn,
            "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i2', 101);",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );

        // Resync: a sealed batch whose newest clock is after the watermark.
        exec(&conn, "DELETE FROM sync_outbox");
        exec(
            &conn,
            "INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (1, x'01', 50, 100);
             INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00');",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Repair, &agreed)).as_deref(),
            Some(DRAIN_INCOMPLETE),
            "a pending batch"
        );
        exec(
            &conn,
            "INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (2, x'02', 90, 140);
             INSERT INTO sync_unsent (seq, envelope) VALUES (2, x'00');",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );
    }

    /// Task 7 review: a forged consent can only ever refuse. Clocks only grow, so a real impact's
    /// watermark is never above the profile's newest clock, and a join's attachments are the ones
    /// the profile has.
    #[test]
    fn a_forged_consent_only_ever_refuses() {
        let conn = migrated();
        this_device(&conn);
        exec(
            &conn,
            &format!(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i0', '*', 100, {THIS});
                 INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i0', 100);"
            ),
        );
        let real = replace_impact(&conn).expect("impact").consent();
        assert_eq!(key(check_consent(&conn, ReplaceKind::Resync, &real)), None);
        assert_eq!(key(check_consent(&conn, ReplaceKind::Join, &real)), None);

        let everything = ReplaceConsent {
            changes_through: u64::MAX,
            ..real
        };
        let ahead = ReplaceConsent {
            changes_through: 101,
            ..real
        };
        let inflated = ReplaceConsent {
            attachments: 5,
            ..real
        };
        for (kind, forged) in [
            (ReplaceKind::Resync, everything),
            (ReplaceKind::Resync, ahead),
            (ReplaceKind::Join, everything),
            (ReplaceKind::Join, inflated),
        ] {
            assert_eq!(
                key(check_consent(&conn, kind, &forged)).as_deref(),
                Some(IMPACT_CHANGED),
                "{kind:?} {forged:?}"
            );
        }
    }

    const THIS: &str = "x'01010101010101010101010101010101'";
    const OTHER: &str = "x'02020202020202020202020202020202'";

    fn this_device(conn: &Connection) {
        exec(
            conn,
            &format!("INSERT INTO sync_state (id, device_id) VALUES (1, {THIS})"),
        );
    }

    #[test]
    fn an_edit_to_an_item_queued_before_consent_is_caught_by_its_clock() {
        let conn = migrated();
        this_device(&conn);
        exec(
            &conn,
            &format!(
                "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i1', 50);
                 INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i1', '*', 50, {THIS});"
            ),
        );
        let agreed = replace_impact(&conn).expect("impact").consent();
        assert_eq!(agreed.changes_through, 50);
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );

        // The edit keeps the row's first `queued_at`; only its clock is new.
        exec(
            &conn,
            &format!(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i1', 'name', 70, {THIS});"
            ),
        );

        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );
    }

    /// A clock another device wrote (merged after consent) on a pending key is no local change.
    #[test]
    fn a_remote_clock_merged_after_consent_is_no_local_change() {
        let conn = migrated();
        this_device(&conn);
        exec(
            &conn,
            &format!(
                "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i1', 50);
                 INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i1', '*', 50, {THIS});"
            ),
        );
        let agreed = replace_impact(&conn).expect("impact").consent();

        exec(
            &conn,
            &format!(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i1', 'name', 900, {OTHER}),
                        ('institutions', 'i9', '*', 950, {OTHER});"
            ),
        );

        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );
    }

    /// A barrier marker ticks the clock without a `sync_clock` row, and an outbox row may carry a
    /// clock above every stored one: the watermark covers both, so the consent of a reloaded
    /// impact passes instead of refusing every offline re-sync for good.
    #[test]
    fn changes_through_covers_the_own_log_and_the_outbox() {
        let conn = migrated();
        this_device(&conn);
        exec(
            &conn,
            &format!(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device)
                 VALUES ('institutions', 'i1', '*', 100, {THIS});
                 INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (1, x'01', 0, 500);
                 INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00');"
            ),
        );
        let agreed = replace_impact(&conn).expect("impact").consent();
        assert_eq!(agreed.changes_through, 500, "the waiting marker's clock");
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );

        exec(
            &conn,
            "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('institutions', 'i2', 600);",
        );
        let agreed = replace_impact(&conn).expect("impact").consent();
        assert_eq!(agreed.changes_through, 600, "the outbox's clock");
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)),
            None
        );
    }
}
