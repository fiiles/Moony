//! Step 2 after a merge (spec §7.4): the rules the database does not enforce. Each repair is a
//! function of the merged rows and their row clocks, so every device deletes the same rows and
//! records the same tombstones without sending anything.

use std::collections::BTreeMap;

use rusqlite::Connection;

use super::{links, renames, row_clock, tombstone_local, Touched};
use crate::db::files_schema;
use crate::error::Result;
use crate::services::sync::blobs;
use crate::services::{attachment_store, loan_events};

/// Tables whose touched rows can break a rule below (`full` marks them all).
pub(crate) const TABLES: [&str; 4] = [
    "stock_investment_tags",
    "stock_tags",
    "loan_events",
    "cashflow_items",
];

/// Tables whose lost rows can leave attachment blobs nothing references.
pub(crate) const ATTACHMENT_OWNERS: [&str; 5] = [
    "real_estate",
    "insurance_policies",
    "real_estate_photos",
    "real_estate_documents",
    "insurance_documents",
];

pub fn run(conn: &Connection, touched: &Touched, now: i64) -> Result<()> {
    if touched.touches_any(&["stock_investment_tags", "stock_tags"]) {
        one_tag_per_group(conn)?;
    }
    if touched.touches_any(&["loan_events"]) {
        one_balance_check_per_day(conn, now)?;
    }
    for (table, scope_value) in &touched.rename_scopes {
        renames::recompute_scope(conn, renames::scope_of(table)?, scope_value)?;
    }
    if touched.touches_any(&["cashflow_items"]) {
        links::recompute(conn)?;
    }
    let owners_lost = ATTACHMENT_OWNERS
        .iter()
        .any(|table| touched.deleted.contains(*table));
    if owners_lost && files_schema::is_attached(conn)? {
        attachment_store::delete_unreferenced(conn)?;
    }
    // The transfer queue follows the rows: a merge or a restore that brought, changed or removed
    // attachment rows queues their content or drops it.
    let rows_touched = attachment_store::REFERENCES
        .iter()
        .any(|(table, _)| touched.rows.contains_key(*table));
    if owners_lost || rows_touched {
        blobs::queue::reconcile(conn, now)?;
    }
    Ok(())
}

/// The candidate rows of one group, `(sort id, pk text)`; the newest row clock wins, ties by
/// the lowest sort id. The others are deleted with a tombstone at the winner's clock. Returns
/// the deleted sort ids.
fn keep_newest(
    conn: &Connection,
    table: &str,
    members: &[(String, String)],
    delete: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<Vec<String>> {
    let mut clocked = Vec::with_capacity(members.len());
    for (id, pk) in members {
        clocked.push((row_clock(conn, table, pk)?, id, pk));
    }
    let Some(winner) = clocked.iter().map(|(clock, _, _)| *clock).max() else {
        return Ok(Vec::new());
    };
    let winner_id = clocked
        .iter()
        .filter(|(clock, _, _)| *clock == winner)
        .map(|(_, id, _)| *id)
        .min()
        .cloned();
    let mut deleted = Vec::new();
    for (_, id, pk) in &clocked {
        if Some(*id) == winner_id.as_ref() {
            continue;
        }
        delete(id)?;
        tombstone_local(conn, table, pk, winner)?;
        deleted.push((*id).clone());
    }
    Ok(deleted)
}

/// One tag per tag group per position: keep the assignment with the newest row clock (ties by
/// the lowest `tag_id`).
fn one_tag_per_group(conn: &Connection) -> Result<()> {
    let rows: Vec<(String, String, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT sit.investment_id, t.group_id, sit.tag_id,
                    json_array(sit.investment_id, sit.tag_id)
             FROM stock_investment_tags sit JOIN stock_tags t ON t.id = sit.tag_id
             WHERE t.group_id IS NOT NULL
               AND (sit.investment_id, t.group_id) IN (
                 SELECT s2.investment_id, t2.group_id
                 FROM stock_investment_tags s2 JOIN stock_tags t2 ON t2.id = s2.tag_id
                 WHERE t2.group_id IS NOT NULL
                 GROUP BY s2.investment_id, t2.group_id HAVING COUNT(*) > 1)
             ORDER BY sit.investment_id, t.group_id, sit.tag_id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    let mut groups: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    for (investment, group, tag, pk) in rows {
        groups
            .entry((investment, group))
            .or_default()
            .push((tag, pk));
    }
    for ((investment, _), members) in groups {
        keep_newest(conn, "stock_investment_tags", &members, &mut |tag| {
            conn.execute(
                "DELETE FROM stock_investment_tags WHERE investment_id = ?1 AND tag_id = ?2",
                [&investment, tag],
            )?;
            Ok(())
        })?;
    }
    Ok(())
}

/// One `balance_check` per loan per day: keep the event with the newest row clock (ties by the
/// lowest id), then re-mirror the loan's anchor, which may have shown a deleted record.
fn one_balance_check_per_day(conn: &Connection, now: i64) -> Result<()> {
    let rows: Vec<(String, i64, String)> = {
        let mut stmt = conn.prepare(
            "SELECT loan_id, event_day, id FROM loan_events
             WHERE kind = 'balance_check' AND (loan_id, event_day) IN (
               SELECT loan_id, event_day FROM loan_events WHERE kind = 'balance_check'
               GROUP BY loan_id, event_day HAVING COUNT(*) > 1)
             ORDER BY loan_id, event_day, id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    let mut groups: BTreeMap<(String, i64), Vec<(String, String)>> = BTreeMap::new();
    for (loan, day, id) in rows {
        groups
            .entry((loan, day))
            .or_default()
            .push((id.clone(), id));
    }
    let mut loans = Vec::new();
    for ((loan, _), members) in groups {
        let deleted = keep_newest(conn, "loan_events", &members, &mut |id| {
            conn.execute("DELETE FROM loan_events WHERE id = ?1", [id])?;
            Ok(())
        })?;
        if !deleted.is_empty() && !loans.contains(&loan) {
            loans.push(loan);
        }
    }
    for loan in loans {
        loan_events::refresh_anchor_mirror(conn, &loan, now)?;
    }
    Ok(())
}
