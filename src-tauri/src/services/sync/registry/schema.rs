//! PRAGMA readers over the live schema.

use rusqlite::Connection;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub decl_type: String,
    pub not_null: bool,
    pub has_default: bool,
    /// 1-based position inside the primary key, 0 when the column is not part of it.
    pub pk_position: u32,
}

/// Columns of `table` in declaration order (`PRAGMA table_info`).
pub fn columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare(&format!(
        "PRAGMA table_info(\"{}\")",
        table.replace('"', "\"\"")
    ))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get(1)?,
                decl_type: row.get::<_, String>(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                has_default: row.get::<_, Option<String>>(4)?.is_some(),
                pk_position: row.get::<_, i64>(5)? as u32,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Primary key column names of `table`, in key order.
pub fn primary_key(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut pk: Vec<ColumnInfo> = columns(conn, table)?
        .into_iter()
        .filter(|c| c.pk_position > 0)
        .collect();
    pk.sort_by_key(|c| c.pk_position);
    Ok(pk.into_iter().map(|c| c.name).collect())
}
