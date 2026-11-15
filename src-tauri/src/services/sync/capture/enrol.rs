//! One-off enrolment of rows that became synced after the profile started syncing (ADR 0030): a
//! profile that had sync on before the documents and photos were synced tables holds attachment
//! rows nobody clocked or queued, so no other device would ever see them. `sync::open_capture`
//! enrols them once, keyed on `sync_state.schema_seen`; a joined profile's rows come from the vault
//! and are clocked already.

use rusqlite::{params, Connection};

use super::{is_installed, triggers::pk_expr, CLOCK_UPSERT};
use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::registry::{self, row_condition, schema};

/// The attachment tables, parents first: every unclocked photo batch goes too, so a photo the
/// conversion turns into a blob later never waits for a parent the vault lacks.
pub const ATTACHMENT_TABLES: [&str; 4] = [
    "real_estate_photo_batches",
    "real_estate_photos",
    "real_estate_documents",
    "insurance_documents",
];

/// Enrols, table by table in the order given, every row that passes its row condition
/// (`registry::row_condition`) and has neither a row clock `'*'` nor a tombstone `'-'`: a fresh
/// `'*'` from `sync_hlc_tick()` with `device`, and an outbox entry, as the insert trigger gives a
/// new row. A row with a clock (received from another device, or captured here) is left alone, so
/// running it again enrols nothing. Needs capture installed (the clock functions). Returns the
/// number of rows enrolled.
pub fn enrol_unclocked(conn: &Connection, device: DeviceId, tables: &[&str]) -> Result<usize> {
    if !is_installed(conn)? {
        return Err(AppError::Internal(
            "sync enrolment: change capture is not installed".into(),
        ));
    }
    let device = device.0.to_vec();
    let mut enrolled = 0;
    for table in tables {
        let spec = registry::spec(table)
            .filter(|spec| spec.class == registry::TableClass::Synced)
            .ok_or_else(|| {
                AppError::Internal(format!("sync enrolment: {table} is not a synced table"))
            })?;
        let pk = schema::primary_key(conn, table)?;
        let key = pk_expr(spec, &pk, "r");
        let condition = row_condition(spec, "r").unwrap_or_else(|| "1".to_string());
        let unclocked = format!(
            "{condition} AND NOT EXISTS (SELECT 1 FROM sync_clock c \
               WHERE c.tbl = ?1 AND c.pk = {key} AND c.col IN ('*', '-'))"
        );
        // Queued first, while the rows are still unclocked; a row already queued keeps its entry.
        conn.execute(
            &format!(
                "INSERT INTO sync_outbox (tbl, pk, queued_at) \
                 SELECT ?1, {key}, sync_hlc_now() FROM main.{table} r WHERE {unclocked} \
                 ON CONFLICT DO NOTHING"
            ),
            params![table],
        )?;
        enrolled += conn.execute(
            &format!(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device) \
                 SELECT ?1, {key}, '*', sync_hlc_tick(), ?2 FROM main.{table} r WHERE {unclocked} \
                 {CLOCK_UPSERT}"
            ),
            params![table, device],
        )?;
    }
    Ok(enrolled)
}
