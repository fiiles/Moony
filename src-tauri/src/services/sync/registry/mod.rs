//! The sync registry: which tables and columns are captured, merged, recomputed or kept local.
//! Capture, journal, merge and snapshot all read it.

pub mod keys;
pub mod schema;
mod tables;

pub use tables::APP_CONFIG_SYNCED_KEYS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableClass {
    Synced,
    Seed,
    Derived,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnClass {
    Synced,
    Local,
    Recomputed,
    RecomputedOrTyped,
    WriteOnce,
    Monotonic,
}

/// The rows a recomputed-or-typed column is derived from: `table.fk` points at the owner row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRows {
    pub table: &'static str,
    pub fk: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyShape {
    Primary,
    /// A single-row table whose logical key is this constant (`user_profile` -> `"profile"`).
    Singleton(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct TableSpec {
    pub name: &'static str,
    pub class: TableClass,
    pub key: KeyShape,
    /// Columns that never travel.
    pub local: &'static [&'static str],
    /// Columns the receiver recomputes from its own rows.
    pub recomputed: &'static [&'static str],
    /// Columns recomputed when source rows exist, typed by the user otherwise.
    pub recomputed_or_typed: &'static [(&'static [&'static str], SourceRows)],
    /// Columns that must always be written together (an amount and its currency).
    pub groups: &'static [&'static [&'static str]],
    pub write_once: &'static [&'static str],
    pub monotonic: &'static [&'static str],
    /// A row syncs only while these columns are all set (not NULL): a document syncs while it
    /// names its blob, a photo while it names both its blobs (the receiving device's CHECK takes
    /// both blobs or both paths), and a legacy row (plain files the conversion has not turned
    /// into blobs) stays on the device. Capture, the journal and snapshots all apply it
    /// (`row_condition`). Empty: every row syncs.
    pub requires: &'static [&'static str],
}

pub fn tables() -> &'static [TableSpec] {
    tables::TABLES
}

pub fn spec(table: &str) -> Option<&'static TableSpec> {
    tables().iter().find(|t| t.name == table)
}

/// Number of primary-key columns of a synced table: 2 for the three link tables, else 1. Pinned
/// against `schema::primary_key` by a test, so a new composite key cannot slip through.
pub fn key_arity(table: &str) -> usize {
    match table {
        "stock_investment_tags" | "real_estate_loans" | "real_estate_insurances" => 2,
        _ => 1,
    }
}

/// Synced tables whose columns are all key columns (link tables): an insert carries no `c`.
/// Pinned against `schema::columns` by a test.
pub fn is_key_only(table: &str) -> bool {
    matches!(table, "real_estate_loans" | "real_estate_insurances")
}

pub fn synced_tables() -> impl Iterator<Item = &'static TableSpec> {
    tables().iter().filter(|t| t.class == TableClass::Synced)
}

/// The SQL condition a row of `spec` must meet to sync, over the row named `alias` (a table name or
/// alias, `NEW` or `OLD`): `{alias}."c" IS NOT NULL` for each column of `requires`, joined with
/// `AND` (in parentheses when there are several); `None` when every row syncs.
pub fn row_condition(spec: &TableSpec, alias: &str) -> Option<String> {
    let each: Vec<String> = spec
        .requires
        .iter()
        .map(|column| format!("{alias}.\"{}\" IS NOT NULL", column.replace('"', "\"\"")))
        .collect();
    match each.as_slice() {
        [] => None,
        [one] => Some(one.clone()),
        _ => Some(format!("({})", each.join(" AND "))),
    }
}

/// Class of one column. Anything not listed is `Synced`.
pub fn column_class(table: &str, column: &str) -> ColumnClass {
    let Some(spec) = spec(table) else {
        return ColumnClass::Synced;
    };
    if spec.local.contains(&column) {
        ColumnClass::Local
    } else if spec.recomputed.contains(&column) {
        ColumnClass::Recomputed
    } else if spec
        .recomputed_or_typed
        .iter()
        .any(|(cols, _)| cols.contains(&column))
    {
        ColumnClass::RecomputedOrTyped
    } else if spec.write_once.contains(&column) {
        ColumnClass::WriteOnce
    } else if spec.monotonic.contains(&column) {
        ColumnClass::Monotonic
    } else {
        ColumnClass::Synced
    }
}

/// The group a column belongs to, if any.
pub fn group_of(table: &str, column: &str) -> Option<&'static [&'static str]> {
    spec(table)?
        .groups
        .iter()
        .copied()
        .find(|g| g.contains(&column))
}

#[cfg(test)]
mod tests;
