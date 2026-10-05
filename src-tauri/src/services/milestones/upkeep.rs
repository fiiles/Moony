//! Data upkeep (backup, balances, valuations) milestones (filled in by the follow-up task).

use rusqlite::Connection;

use crate::error::Result;
use crate::models::Milestone;

pub(super) fn build(_conn: &Connection, _today: i64) -> Result<Vec<Milestone>> {
    Ok(Vec::new())
}
