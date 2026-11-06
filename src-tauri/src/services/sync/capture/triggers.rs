//! SQL of the capture triggers (spec §6.1): `TEMP` triggers per Synced table (`_ins`, `_del`,
//! `_upd` when the table has a captured column, `_pk` unless it is a singleton), generated from the
//! registry and `PRAGMA table_info` of the migrated schema, so they always match the columns that
//! exist.
//!
//! Every trigger reads the connection's mode from `temp.sync_mode` and clocks from the
//! connection functions `sync_hlc_tick()` / `sync_hlc_now()` that `capture::install` registers.

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::registry::{
    self, column_class, group_of, schema, ColumnClass, KeyShape, TableSpec, APP_CONFIG_SYNCED_KEYS,
};

/// Conflict clause of every `sync_clock` write: a stored clock is replaced only by a greater
/// `(hlc, device)`. Appended to `INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES (…)` or
/// `… SELECT … WHERE …` (a SELECT needs its WHERE so `ON` reads as the upsert, not a join).
pub const CLOCK_UPSERT: &str = "ON CONFLICT(tbl, pk, col) DO UPDATE SET hlc = excluded.hlc, \
     device = excluded.device WHERE (excluded.hlc, excluded.device) > (sync_clock.hlc, sync_clock.device)";

const MODE: &str = "(SELECT mode FROM sync_mode)";
const LOCAL: &str = "(SELECT mode FROM sync_mode) = 'local'";

/// The `CREATE TEMP TRIGGER` statements for every Synced table, one statement per string.
pub fn trigger_sql(conn: &Connection, device: DeviceId) -> Result<Vec<String>> {
    let device = format!("x'{}'", device.to_hex());
    let mut out = Vec::new();
    for spec in registry::synced_tables() {
        out.extend(table_triggers(conn, spec, &device)?);
    }
    Ok(out)
}

/// The logical key of a row as SQL over `row` (`NEW` or `OLD`): the single primary key column,
/// a `json_array` of a composite key, or the singleton's constant.
pub fn pk_expr(spec: &TableSpec, pk: &[String], row: &str) -> String {
    match spec.key {
        KeyShape::Singleton(key) => text(key),
        KeyShape::Primary if pk.len() == 1 => format!("{row}.{}", pk[0]),
        KeyShape::Primary => format!(
            "json_array({})",
            pk.iter()
                .map(|c| format!("{row}.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A SQL string literal.
fn text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// One table's statements, built around its key expressions and row filter.
struct Table<'a> {
    name: String,
    device: &'a str,
}

impl Table<'_> {
    fn clock(&self, key: &str, col: &str, hlc: &str, device: &str, when: &str) -> String {
        format!(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) SELECT {}, {key}, {col}, {hlc}, {device} \
             WHERE {when} {CLOCK_UPSERT};",
            self.name
        )
    }

    fn local_clock(&self, key: &str, col: &str, when: &str) -> String {
        self.clock(key, col, "sync_hlc_now()", self.device, when)
    }

    /// Drops the key's column clocks; the row clock `'*'` and the tombstone `'-'` stay.
    fn clear_columns(&self, key: &str, when: &str) -> String {
        format!(
            "DELETE FROM sync_clock WHERE tbl = {} AND pk = {key} AND col NOT IN ('*', '-') AND {when};",
            self.name
        )
    }

    /// Queues the key; a row already queued keeps its first `queued_at`.
    fn queue(&self, key: &str, when: &str) -> String {
        format!(
            "INSERT INTO sync_outbox (tbl, pk, queued_at) SELECT {}, {key}, sync_hlc_now() \
             WHERE {when} ON CONFLICT DO NOTHING;",
            self.name
        )
    }
}

fn table_triggers(conn: &Connection, spec: &TableSpec, device: &str) -> Result<Vec<String>> {
    let t = spec.name;
    let columns = schema::columns(conn, t)?;
    if columns.is_empty() {
        return Err(AppError::Internal(format!(
            "sync capture: the synced table {t} does not exist"
        )));
    }
    let pk = schema::primary_key(conn, t)?;
    if spec.key == KeyShape::Primary && pk.is_empty() {
        return Err(AppError::Internal(format!(
            "sync capture: the synced table {t} has no primary key"
        )));
    }
    let new_key = pk_expr(spec, &pk, "NEW");
    let old_key = pk_expr(spec, &pk, "OLD");
    let (new_ok, old_ok) = row_filter(t);
    let table = Table {
        name: text(t),
        device,
    };

    let mut out = Vec::new();

    // INSERT: a row clock (the generation), no column clocks, queued.
    out.push(format!(
        "CREATE TEMP TRIGGER sync_{t}_ins AFTER INSERT ON main.{t} WHEN {LOCAL} AND {new_ok} BEGIN \
         SELECT sync_hlc_tick(); {} {} {} END;",
        table.local_clock(&new_key, "'*'", "1"),
        table.clear_columns(&new_key, "1"),
        table.queue(&new_key, "1"),
    ));

    // UPDATE of captured columns: one clock per changed column (every member of a changed group).
    let units = change_units(spec, &columns, &pk)?;
    if !units.is_empty() {
        let same_key = match spec.key {
            KeyShape::Singleton(_) => String::new(),
            KeyShape::Primary => pk
                .iter()
                .map(|c| format!(" AND OLD.{c} IS NEW.{c}"))
                .collect(),
        };
        let any_change = units
            .iter()
            .map(|u| u.condition.as_str())
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut body = String::from("SELECT sync_hlc_tick(); ");
        for unit in &units {
            for member in &unit.members {
                body.push_str(&table.local_clock(&new_key, &text(member), &unit.condition));
                body.push(' ');
            }
        }
        body.push_str(&table.queue(&new_key, "1"));
        out.push(format!(
            "CREATE TEMP TRIGGER sync_{t}_upd AFTER UPDATE ON main.{t} \
             WHEN {LOCAL} AND {new_ok}{same_key} AND ({any_change}) BEGIN {body} END;"
        ));
    }

    // Primary key change: a delete of the old key and an insert of the new one.
    if spec.key == KeyShape::Primary {
        let of = pk.join(", ");
        let changed = pk
            .iter()
            .map(|c| format!("OLD.{c} IS NOT NEW.{c}"))
            .collect::<Vec<_>>()
            .join(" OR ");
        out.push(format!(
            "CREATE TEMP TRIGGER sync_{t}_pk AFTER UPDATE OF {of} ON main.{t} \
             WHEN {LOCAL} AND ({changed}) AND ({old_ok} OR {new_ok}) BEGIN \
             SELECT sync_hlc_tick(); {} {} {} {} {} {} END;",
            table.local_clock(&old_key, "'-'", old_ok.as_str()),
            table.clear_columns(&old_key, &old_ok),
            table.queue(&old_key, &old_ok),
            table.local_clock(&new_key, "'*'", &new_ok),
            table.clear_columns(&new_key, &new_ok),
            table.queue(&new_key, &new_ok),
        ));
    }

    // DELETE: local → a fresh tombstone, queued; merge → the cascade clock, not queued.
    let merge_tombstone = format!(
        "INSERT INTO sync_clock (tbl, pk, col, hlc, device) \
         SELECT {}, {old_key}, '-', m.cascade_hlc, m.cascade_device FROM sync_mode m \
         WHERE m.mode = 'merge' {CLOCK_UPSERT};",
        table.name
    );
    out.push(format!(
        "CREATE TEMP TRIGGER sync_{t}_del AFTER DELETE ON main.{t} \
         WHEN {MODE} IN ('local', 'merge') AND {old_ok} BEGIN \
         SELECT RAISE(ABORT, 'sync: a merge deleted a row without a cascade clock') \
           WHERE {MODE} = 'merge' AND (SELECT cascade_hlc FROM sync_mode) IS NULL; \
         SELECT sync_hlc_tick() WHERE {LOCAL}; {} {merge_tombstone} {} {} END;",
        table.local_clock(&old_key, "'-'", LOCAL),
        table.clear_columns(&old_key, "1"),
        table.queue(&old_key, LOCAL),
    ));

    Ok(out)
}

/// `(NEW filter, OLD filter)`: `app_config` captures only its allowlisted keys.
fn row_filter(table: &str) -> (String, String) {
    if table == "app_config" {
        let keys = APP_CONFIG_SYNCED_KEYS
            .iter()
            .map(|k| text(k))
            .collect::<Vec<_>>()
            .join(", ");
        (
            format!("NEW.key IN ({keys})"),
            format!("OLD.key IN ({keys})"),
        )
    } else {
        ("1".to_string(), "1".to_string())
    }
}

/// Columns that are clocked together, and the condition under which they are.
struct ChangeUnit {
    members: Vec<String>,
    condition: String,
}

fn change_units(
    spec: &TableSpec,
    columns: &[schema::ColumnInfo],
    pk: &[String],
) -> Result<Vec<ChangeUnit>> {
    let captured: Vec<&str> = columns
        .iter()
        .map(|c| c.name.as_str())
        .filter(|c| !pk.iter().any(|p| p == c))
        .filter(|c| {
            matches!(
                column_class(spec.name, c),
                ColumnClass::Synced
                    | ColumnClass::WriteOnce
                    | ColumnClass::Monotonic
                    | ColumnClass::RecomputedOrTyped
            )
        })
        .collect();

    let mut units: Vec<ChangeUnit> = Vec::new();
    let mut grouped: Vec<&str> = Vec::new();
    for column in &captured {
        if grouped.contains(column) {
            continue;
        }
        let members: Vec<&str> = match group_of(spec.name, column) {
            Some(group) => group
                .iter()
                .copied()
                .filter(|m| captured.contains(m))
                .collect(),
            None => vec![column],
        };
        grouped.extend(&members);
        let condition = members
            .iter()
            .map(|m| changed(spec, m, pk))
            .collect::<Result<Vec<_>>>()?
            .join(" OR ");
        units.push(ChangeUnit {
            members: members.iter().map(|m| m.to_string()).collect(),
            condition: format!("({condition})"),
        });
    }
    Ok(units)
}

/// `OLD.c IS NOT NEW.c`, gated for a recomputed-or-typed column on the row having no source rows.
fn changed(spec: &TableSpec, column: &str, pk: &[String]) -> Result<String> {
    let change = format!("OLD.\"{column}\" IS NOT NEW.\"{column}\"");
    let Some((_, source)) = spec
        .recomputed_or_typed
        .iter()
        .find(|(cols, _)| cols.contains(&column))
    else {
        return Ok(change);
    };
    let [owner] = pk else {
        return Err(AppError::Internal(format!(
            "sync capture: {}.{column} is recomputed-or-typed but the table has no single key",
            spec.name
        )));
    };
    Ok(format!(
        "({change} AND NOT EXISTS (SELECT 1 FROM {} WHERE {} = NEW.{owner}))",
        source.table, source.fk
    ))
}
