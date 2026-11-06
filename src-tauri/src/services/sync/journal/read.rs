//! Reading one queued key as a full row change (R1). Shared with the snapshot writer.
//!
//! A live row carries every column except Local ones and the key columns, each with its column
//! clock, else the row clock `'*'`, else `Clock::ZERO` (a row that never had a clock: it existed
//! before sync was turned on). `Clock::ZERO` travels with the all-zero device.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::services::sync::changeset::{RowChange, RowKey, SqlValue};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::registry::keys::foreign_keys;
use crate::services::sync::registry::{
    self, column_class, schema, ColumnClass, KeyShape, TableClass, TableSpec,
    APP_CONFIG_SYNCED_KEYS,
};

/// The column a repair rename rewrites (`sync_renamed`, correction 17).
const RENAMED_COLUMN: &str = "name";

/// The full live row, a tombstone-only change, or `None` when the key has neither a row nor a
/// tombstone.
pub fn read_row(conn: &Connection, table: &str, pk: &str) -> Result<Option<RowChange>> {
    Reader::new(conn)?.read(table, pk)
}

/// What the reader needs to know about one table.
struct Shape {
    spec: &'static TableSpec,
    /// Columns read from the row: the key columns first, then the carried ones.
    selected: Vec<String>,
    key_len: usize,
    /// `WHERE …` selecting one row by its key (an ordering for the singleton).
    filter: String,
}

/// Reads row changes with each table's shape and the foreign keys looked up once.
pub(super) struct Reader<'c> {
    conn: &'c Connection,
    /// child table → (FK column, parent table).
    fks: HashMap<String, Vec<(String, String)>>,
    shapes: RefCell<HashMap<&'static str, Rc<Shape>>>,
}

impl<'c> Reader<'c> {
    pub(super) fn new(conn: &'c Connection) -> Result<Self> {
        let mut fks: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for fk in foreign_keys(conn)? {
            fks.entry(fk.child)
                .or_default()
                .push((fk.column, fk.parent));
        }
        Ok(Self {
            conn,
            fks,
            shapes: RefCell::new(HashMap::new()),
        })
    }

    fn shape(&self, table: &str) -> Result<Rc<Shape>> {
        let spec = registry::spec(table)
            .filter(|s| s.class == TableClass::Synced)
            .ok_or_else(|| AppError::Internal(format!("sync journal: {table} is not synced")))?;
        if let Some(shape) = self.shapes.borrow().get(spec.name) {
            return Ok(Rc::clone(shape));
        }
        let pk = schema::primary_key(self.conn, spec.name)?;
        let key_columns: Vec<String> = match spec.key {
            KeyShape::Singleton(_) => Vec::new(),
            KeyShape::Primary => pk.clone(),
        };
        let carried = schema::columns(self.conn, spec.name)?
            .into_iter()
            .map(|c| c.name)
            .filter(|c| !pk.contains(c))
            .filter(|c| column_class(spec.name, c) != ColumnClass::Local);
        let selected: Vec<String> = key_columns.iter().cloned().chain(carried).collect();
        let filter = match spec.key {
            KeyShape::Singleton(_) => "ORDER BY rowid LIMIT 1".to_string(),
            KeyShape::Primary => format!(
                "WHERE {}",
                key_columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{} = ?{}", quoted(c), i + 1))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            ),
        };
        let shape = Rc::new(Shape {
            spec,
            selected,
            key_len: key_columns.len(),
            filter,
        });
        self.shapes
            .borrow_mut()
            .insert(spec.name, Rc::clone(&shape));
        Ok(shape)
    }

    /// The logical key and its column values as bound parameters; `None` when the key never syncs
    /// (an `app_config` key outside the allowlist).
    fn key(shape: &Shape, pk: &str) -> Result<Option<(RowKey, Vec<String>)>> {
        if shape.spec.name == "app_config" && !APP_CONFIG_SYNCED_KEYS.contains(&pk) {
            return Ok(None);
        }
        let key = RowKey::from_pk_text(shape.spec, pk)?;
        let params = match &key {
            RowKey::Singleton => Vec::new(),
            RowKey::Single(text) => vec![text.clone()],
            RowKey::Composite(parts) => parts.clone(),
        };
        if params.len() != shape.key_len {
            return Err(AppError::Internal(format!(
                "sync journal: the {} key {pk} has the wrong shape",
                shape.spec.name
            )));
        }
        Ok(Some((key, params)))
    }

    /// Whether the key has a live row.
    pub(super) fn exists(&self, table: &str, pk: &str) -> Result<bool> {
        let shape = self.shape(table)?;
        let Some((_, params)) = Self::key(&shape, pk)? else {
            return Ok(false);
        };
        let sql = format!("SELECT 1 FROM {} {}", quoted(shape.spec.name), shape.filter);
        Ok(self
            .conn
            .prepare_cached(&sql)?
            .query_row(params_from_iter(params.iter()), |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub(super) fn read(&self, table: &str, pk: &str) -> Result<Option<RowChange>> {
        let shape = self.shape(table)?;
        let Some((key, params)) = Self::key(&shape, pk)? else {
            return Ok(None);
        };
        let clocks = self.clocks(shape.spec.name, pk)?;
        let generation = clocks.get("*").copied().unwrap_or(Clock::ZERO);
        let deleted = clocks.get("-").copied();

        let sql = format!(
            "SELECT {} FROM {} {}",
            shape
                .selected
                .iter()
                .map(|c| quoted(c))
                .collect::<Vec<_>>()
                .join(", "),
            quoted(shape.spec.name),
            shape.filter
        );
        let values: Option<Vec<Value>> = self
            .conn
            .prepare_cached(&sql)?
            .query_row(params_from_iter(params.iter()), |r| {
                (0..shape.selected.len())
                    .map(|i| r.get::<_, Value>(i))
                    .collect()
            })
            .optional()?;

        let Some(values) = values else {
            return Ok(deleted.map(|deleted| RowChange {
                table: shape.spec.name.to_string(),
                key,
                generation,
                insert: false,
                deleted: Some(deleted),
                columns: BTreeMap::new(),
                parents: BTreeMap::new(),
            }));
        };

        let row: Vec<(&str, SqlValue)> = shape
            .selected
            .iter()
            .map(String::as_str)
            .zip(values.into_iter().map(SqlValue::from))
            .collect();
        let typed_name = self.typed_name(shape.spec.name, pk)?;
        let mut columns = BTreeMap::new();
        for (name, value) in &row[shape.key_len..] {
            let clock = clocks.get(*name).copied().unwrap_or(generation);
            let value = match (&typed_name, value) {
                (Some((typed, assigned)), SqlValue::Text(stored))
                    if *name == RENAMED_COLUMN && stored == assigned =>
                {
                    SqlValue::Text(typed.clone())
                }
                _ => value.clone(),
            };
            columns.insert(name.to_string(), (value, clock));
        }
        let parents = self.parents(shape.spec.name, &row)?;
        Ok(Some(RowChange {
            table: shape.spec.name.to_string(),
            key,
            generation,
            insert: true,
            deleted,
            columns,
            parents,
        }))
    }

    fn clocks(&self, table: &str, pk: &str) -> Result<HashMap<String, Clock>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT col, hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2")?;
        let rows = stmt
            .query_map([table, pk], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(col, hlc, device)| Ok((col, clock_from(hlc, device)?)))
            .collect()
    }

    /// `(typed, assigned)` of a repair-renamed row.
    fn typed_name(&self, table: &str, pk: &str) -> Result<Option<(String, String)>> {
        Ok(self
            .conn
            .prepare_cached("SELECT typed, assigned FROM sync_renamed WHERE tbl = ?1 AND pk = ?2")?
            .query_row([table, pk], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?)
    }

    /// The parent's generation (`'*'`, else ZERO) for every FK column of the row holding a value,
    /// key columns included.
    fn parents(&self, table: &str, row: &[(&str, SqlValue)]) -> Result<BTreeMap<String, Clock>> {
        let mut parents = BTreeMap::new();
        let Some(fks) = self.fks.get(table) else {
            return Ok(parents);
        };
        for (column, parent) in fks {
            let parent_pk = match row.iter().find(|(name, _)| name == column) {
                Some((_, SqlValue::Text(text))) => text.clone(),
                Some((_, SqlValue::Integer(i))) => i.to_string(),
                _ => continue,
            };
            let generation = self
                .conn
                .prepare_cached(
                    "SELECT hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2 AND col = '*'",
                )?
                .query_row([parent.as_str(), parent_pk.as_str()], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
                })
                .optional()?
                .map(|(hlc, device)| clock_from(hlc, device))
                .transpose()?
                .unwrap_or(Clock::ZERO);
            parents.insert(column.clone(), generation);
        }
        Ok(parents)
    }
}

fn clock_from(hlc: i64, device: Vec<u8>) -> Result<Clock> {
    let hlc = u64::try_from(hlc)
        .map_err(|_| AppError::Internal(format!("sync_clock holds a negative clock {hlc}")))?;
    let device: [u8; 16] = device
        .try_into()
        .map_err(|_| AppError::Internal("sync_clock.device is not 16 bytes".into()))?;
    Ok(Clock {
        hlc,
        device: DeviceId(device),
    })
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
