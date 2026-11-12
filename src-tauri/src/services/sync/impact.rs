//! What a replace (a join, a repair, a re-sync) puts at stake on this device, and the check that
//! nothing changed since the person agreed to it (audit M8; pre-flight C8, Minor 4 and Minor 5).
//!
//! The person agrees to the numbers a confirmation showed. Consent is a clock watermark, not a
//! count: every local change after it carries a larger clock (`sync_clock`, `queued_at` and the own
//! log's `max_hlc` are all clocks of the same hybrid logical clock), so the check names exactly the
//! changes made after the person looked, whatever happened to the counts meanwhile. Items (outbox
//! rows) and batches (sealed envelopes) are different units and are counted apart.

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
    /// `sync_unsent` rows: sealed batches not confirmed by the sync server.
    pub pending_batches: u32,
    /// `real_estate_documents` + `insurance_documents`.
    pub documents: u32,
    /// `real_estate_photos`.
    pub photos: u32,
    /// The rows of `ACCOUNT_TABLES` (bonds included).
    pub accounts: u32,
    /// `bank_transactions`.
    pub transactions: u32,
    /// The newest clock this device knows (`MAX(hlc)` of `sync_clock`, 0 without one): every later
    /// local change gets a larger clock.
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
    let newest: Option<i64> =
        conn.query_row("SELECT MAX(hlc) FROM sync_clock", [], |r| r.get(0))?;
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

/// Join: documents + photos above `consent.attachments` → `IMPACT_CHANGED`. Resync: an outbox row
/// queued after `consent.changes_through` (`queued_at`) or an unsent envelope whose own-log
/// `max_hlc` is after it → `IMPACT_CHANGED`. Repair: any pending item or batch →
/// `validation.syncRepairDrainIncomplete` (the caller drains again).
pub fn check_consent(conn: &Connection, kind: ReplaceKind, consent: &ReplaceConsent) -> Result<()> {
    match kind {
        ReplaceKind::Join => {
            let impact = replace_impact(conn)?;
            let attachments = impact.documents.saturating_add(impact.photos);
            if attachments > consent.attachments {
                return Err(AppError::Validation(IMPACT_CHANGED.into()));
            }
        }
        ReplaceKind::Resync => {
            // Clocks are stored as INTEGER; a watermark beyond i64 covers every one of them.
            let through = i64::try_from(consent.changes_through).unwrap_or(i64::MAX);
            let later: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM sync_outbox WHERE queued_at > ?1) \
                     OR EXISTS (SELECT 1 FROM sync_unsent u JOIN sync_own_log l ON l.seq = u.seq \
                                WHERE l.max_hlc > ?1)",
                [through],
                |r| r.get(0),
            )?;
            if later {
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
                changes_through: 0,
            },
            "bonds count among accounts; items and batches apart"
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
        let agreed = ReplaceConsent {
            changes_through: 100,
            attachments: 1,
        };
        for kind in [ReplaceKind::Join, ReplaceKind::Resync, ReplaceKind::Repair] {
            assert_eq!(key(check_consent(&conn, kind, &agreed)), None, "{kind:?}");
        }

        // Join: attachments above what was agreed.
        exec(
            &conn,
            "INSERT INTO insurance_policies
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency)
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly');
             INSERT INTO insurance_documents (id, insurance_id, name, blob_id) VALUES ('d1', 'p1', 'C', 'b1');",
        );
        assert_eq!(key(check_consent(&conn, ReplaceKind::Join, &agreed)), None);
        exec(
            &conn,
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) VALUES ('d2', 'p1', 'C', 'b2');",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Join, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );

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
            "INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (1, x'01', 50, 100), (2, x'02', 90, 140);
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
            "INSERT INTO sync_unsent (seq, envelope) VALUES (2, x'00');",
        );
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &agreed)).as_deref(),
            Some(IMPACT_CHANGED)
        );
        let everything = ReplaceConsent {
            changes_through: u64::MAX,
            attachments: 0,
        };
        assert_eq!(
            key(check_consent(&conn, ReplaceKind::Resync, &everything)),
            None
        );
    }
}
