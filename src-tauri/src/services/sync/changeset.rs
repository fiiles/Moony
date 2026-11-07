//! The plaintext changeset (§9.1): what one device sends, before it is encrypted. Rust is the only
//! reader, because HLCs exceed 2^53 and a JavaScript `JSON.parse` would round them. The row-level
//! `encode_row`/`decode_row` are public so snapshots reuse them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::{json, Map, Value as Json};

use crate::error::{AppError, Result};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::registry::{self, KeyShape, TableClass, TableSpec};

pub const CHANGESET_VERSION: u32 = 1;

/// A SQLite value as it travels.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl From<rusqlite::types::Value> for SqlValue {
    fn from(value: rusqlite::types::Value) -> Self {
        use rusqlite::types::Value;
        match value {
            Value::Null => SqlValue::Null,
            Value::Integer(i) => SqlValue::Integer(i),
            Value::Real(r) => SqlValue::Real(r),
            Value::Text(t) => SqlValue::Text(t),
            Value::Blob(b) => SqlValue::Blob(b),
        }
    }
}

impl From<SqlValue> for rusqlite::types::Value {
    fn from(value: SqlValue) -> Self {
        use rusqlite::types::Value;
        match value {
            SqlValue::Null => Value::Null,
            SqlValue::Integer(i) => Value::Integer(i),
            SqlValue::Real(r) => Value::Real(r),
            SqlValue::Text(t) => Value::Text(t),
            SqlValue::Blob(b) => Value::Blob(b),
        }
    }
}

/// A row's logical key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKey {
    Single(String),
    Composite(Vec<String>),
    Singleton,
}

const SINGLETON_KEY: &str = "profile";

fn invalid() -> AppError {
    AppError::Validation("validation.syncChangesetInvalid".to_string())
}

impl RowKey {
    /// The text stored in the journal and outbox. A composite key is the compact JSON array
    /// SQLite's `json_array` writes (identical for the ASCII ids the tables use).
    pub fn to_pk_text(&self) -> String {
        match self {
            RowKey::Single(text) => text.clone(),
            RowKey::Composite(parts) => {
                serde_json::to_string(parts).expect("a string array always serializes")
            }
            RowKey::Singleton => SINGLETON_KEY.to_string(),
        }
    }

    pub fn from_pk_text(spec: &TableSpec, pk: &str) -> Result<Self> {
        match spec.key {
            KeyShape::Singleton(constant) if pk == constant => Ok(RowKey::Singleton),
            KeyShape::Singleton(_) => Err(invalid()),
            KeyShape::Primary if pk.starts_with('[') => {
                let parts: Vec<String> = serde_json::from_str(pk).map_err(|_| invalid())?;
                if parts.len() < 2 {
                    return Err(invalid());
                }
                Ok(RowKey::Composite(parts))
            }
            KeyShape::Primary => Ok(RowKey::Single(pk.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RowChange {
    pub table: String,
    pub key: RowKey,
    pub generation: Clock,
    pub insert: bool,
    pub deleted: Option<Clock>,
    pub columns: BTreeMap<String, (SqlValue, Clock)>,
    pub parents: BTreeMap<String, Clock>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Changeset {
    pub schema: u32,
    pub barrier: u32,
    pub device: DeviceId,
    pub seq: u64,
    pub prev: Option<[u8; 32]>,
    pub rows: Vec<RowChange>,
}

/// The `devs` list: devices in first-use order with a reverse index.
#[derive(Default)]
pub struct DevTable {
    list: Vec<DeviceId>,
    at: HashMap<DeviceId, usize>,
}

impl DevTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn index(&mut self, d: DeviceId) -> usize {
        if let Some(i) = self.at.get(&d) {
            return *i;
        }
        let i = self.list.len();
        self.list.push(d);
        self.at.insert(d, i);
        i
    }

    pub fn into_vec(self) -> Vec<DeviceId> {
        self.list
    }
}

fn clock_json(c: &Clock, devs: &mut DevTable) -> Json {
    json!([c.hlc, devs.index(c.device)])
}

/// A value as it travels: in a changeset row and in a snapshot's seed rows.
pub(crate) fn value_json(v: &SqlValue) -> Json {
    match v {
        SqlValue::Null => Json::Null,
        SqlValue::Integer(i) => json!(i),
        // NaN and infinities are not SQLite values a synced column can hold.
        SqlValue::Real(r) => serde_json::Number::from_f64(*r).map_or(Json::Null, Json::Number),
        SqlValue::Text(t) => json!(t),
        SqlValue::Blob(b) => json!({ "b": STANDARD.encode(b) }),
    }
}

pub fn encode_row(row: &RowChange, devs: &mut DevTable) -> Json {
    let mut out = Map::new();
    out.insert("t".into(), json!(row.table));
    out.insert(
        "k".into(),
        match &row.key {
            RowKey::Single(text) => json!(text),
            RowKey::Composite(parts) => json!(parts),
            RowKey::Singleton => json!(SINGLETON_KEY),
        },
    );
    out.insert("g".into(), clock_json(&row.generation, devs));
    if row.insert {
        out.insert("ins".into(), json!(true));
    }
    if let Some(del) = &row.deleted {
        out.insert("del".into(), clock_json(del, devs));
    }
    if !row.columns.is_empty() {
        let columns = row
            .columns
            .iter()
            .map(|(name, (value, clock))| {
                let hlc_and_dev = clock_json(clock, devs);
                (
                    name.clone(),
                    json!([value_json(value), hlc_and_dev[0], hlc_and_dev[1]]),
                )
            })
            .collect::<Map<_, _>>();
        out.insert("c".into(), Json::Object(columns));
    }
    if !row.parents.is_empty() {
        let parents = row
            .parents
            .iter()
            .map(|(name, clock)| (name.clone(), clock_json(clock, devs)))
            .collect::<Map<_, _>>();
        out.insert("p".into(), Json::Object(parents));
    }
    Json::Object(out)
}

fn hlc_from(v: &Json) -> Result<u64> {
    let hlc = v.as_u64().ok_or_else(invalid)?;
    if hlc > i64::MAX as u64 {
        return Err(invalid());
    }
    Ok(hlc)
}

fn device_at(devs: &[DeviceId], v: &Json) -> Result<DeviceId> {
    let i = usize::try_from(v.as_u64().ok_or_else(invalid)?).map_err(|_| invalid())?;
    devs.get(i).copied().ok_or_else(invalid)
}

/// `[hlc, i]`.
fn clock_from(v: &Json, devs: &[DeviceId]) -> Result<Clock> {
    match v.as_array().map(Vec::as_slice) {
        Some([hlc, i]) => Ok(Clock {
            hlc: hlc_from(hlc)?,
            device: device_at(devs, i)?,
        }),
        _ => Err(invalid()),
    }
}

fn value_from(v: &Json) -> Result<SqlValue> {
    match v {
        Json::Null => Ok(SqlValue::Null),
        Json::String(t) => Ok(SqlValue::Text(t.clone())),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Ok(SqlValue::Integer(i)),
            // Synced tables never send REAL (§9.1).
            None => Err(invalid()),
        },
        Json::Object(o) if o.len() == 1 => {
            let b64 = o.get("b").and_then(Json::as_str).ok_or_else(invalid)?;
            Ok(SqlValue::Blob(STANDARD.decode(b64).map_err(|_| invalid())?))
        }
        _ => Err(invalid()),
    }
}

/// A value of a Seed row (snapshot, §8): as `value_from`, plus REAL (fetched prices and rates).
pub(crate) fn seed_value_from(v: &Json) -> Result<SqlValue> {
    match v {
        Json::Number(n) if n.as_i64().is_none() => {
            n.as_f64().map(SqlValue::Real).ok_or_else(invalid)
        }
        other => value_from(other),
    }
}

pub fn decode_row(value: &Json, devs: &[DeviceId]) -> Result<RowChange> {
    let o = value.as_object().ok_or_else(invalid)?;
    let table = o.get("t").and_then(Json::as_str).ok_or_else(invalid)?;
    let spec = registry::spec(table)
        .filter(|s| s.class == TableClass::Synced)
        .ok_or_else(invalid)?;
    let key = match (o.get("k").ok_or_else(invalid)?, spec.key) {
        (Json::String(text), KeyShape::Singleton(constant)) if text == constant => {
            RowKey::Singleton
        }
        (Json::String(text), KeyShape::Primary) if registry::key_arity(table) == 1 => {
            RowKey::Single(text.clone())
        }
        (Json::Array(parts), KeyShape::Primary) if parts.len() == registry::key_arity(table) => {
            RowKey::Composite(
                parts
                    .iter()
                    .map(|p| p.as_str().map(str::to_string).ok_or_else(invalid))
                    .collect::<Result<_>>()?,
            )
        }
        _ => return Err(invalid()),
    };
    let generation = clock_from(o.get("g").ok_or_else(invalid)?, devs)?;
    let insert = match o.get("ins") {
        None => false,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(invalid()),
    };
    let deleted = o.get("del").map(|d| clock_from(d, devs)).transpose()?;
    let mut columns = BTreeMap::new();
    if let Some(c) = o.get("c") {
        for (name, entry) in c.as_object().ok_or_else(invalid)? {
            let [value, hlc, i] = entry.as_array().map(Vec::as_slice).ok_or_else(invalid)? else {
                return Err(invalid());
            };
            let clock = Clock {
                hlc: hlc_from(hlc)?,
                device: device_at(devs, i)?,
            };
            columns.insert(name.clone(), (value_from(value)?, clock));
        }
    }
    let mut parents = BTreeMap::new();
    if let Some(p) = o.get("p") {
        for (name, entry) in p.as_object().ok_or_else(invalid)? {
            parents.insert(name.clone(), clock_from(entry, devs)?);
        }
    }
    if insert && columns.is_empty() && !registry::is_key_only(table) {
        return Err(invalid());
    }
    Ok(RowChange {
        table: table.to_string(),
        key,
        generation,
        insert,
        deleted,
        columns,
        parents,
    })
}

pub fn to_json(cs: &Changeset) -> Vec<u8> {
    let mut devs = DevTable::new();
    devs.index(cs.device);
    let rows: Vec<Json> = cs.rows.iter().map(|r| encode_row(r, &mut devs)).collect();
    let mut out = Map::new();
    out.insert("v".into(), json!(CHANGESET_VERSION));
    out.insert("schema".into(), json!(cs.schema));
    if cs.barrier != 0 {
        out.insert("barrier".into(), json!(cs.barrier));
    }
    out.insert("device".into(), json!(cs.device.to_b64()));
    out.insert("seq".into(), json!(cs.seq));
    if let (Some(prev), true) = (&cs.prev, cs.seq != 1) {
        out.insert("prev".into(), json!(URL_SAFE_NO_PAD.encode(prev)));
    }
    let devs: Vec<String> = devs.into_vec().iter().map(DeviceId::to_b64).collect();
    out.insert("devs".into(), json!(devs));
    out.insert("rows".into(), Json::Array(rows));
    serde_json::to_vec(&Json::Object(out)).expect("a JSON value always serializes")
}

pub fn from_json(bytes: &[u8]) -> Result<Changeset> {
    let v: Json = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let o = v.as_object().ok_or_else(invalid)?;
    if o.get("v").and_then(Json::as_u64) != Some(CHANGESET_VERSION as u64) {
        return Err(invalid());
    }
    let small = |key: &str| -> Result<Option<u32>> {
        o.get(key)
            .map(|n| {
                n.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(invalid)
            })
            .transpose()
    };
    let schema = small("schema")?.ok_or_else(invalid)?;
    let barrier = small("barrier")?.unwrap_or(0);
    let device = DeviceId::from_b64(o.get("device").and_then(Json::as_str).ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let seq = o.get("seq").and_then(Json::as_u64).ok_or_else(invalid)?;
    let prev = match o.get("prev") {
        None => None,
        Some(p) => {
            let raw = URL_SAFE_NO_PAD
                .decode(p.as_str().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            Some(<[u8; 32]>::try_from(raw).map_err(|_| invalid())?)
        }
    };
    let devs = o
        .get("devs")
        .and_then(Json::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|d| DeviceId::from_b64(d.as_str().ok_or_else(invalid)?).map_err(|_| invalid()))
        .collect::<Result<Vec<_>>>()?;
    // The author is always `devs[0]`, and every device appears once (§9.3: a chain violation).
    if devs.first() != Some(&device) || devs.iter().collect::<HashSet<_>>().len() != devs.len() {
        return Err(invalid());
    }
    let rows = o
        .get("rows")
        .and_then(Json::as_array)
        .ok_or_else(invalid)?
        .iter()
        .map(|r| decode_row(r, &devs))
        .collect::<Result<Vec<_>>>()?;
    Ok(Changeset {
        schema,
        barrier,
        device,
        seq,
        prev,
        rows,
    })
}

/// The size of the raw-deflated JSON rounded up to a multiple of 1024 (the padded plaintext
/// length the envelope hides the true size behind).
pub fn padded_deflated_len(json: &[u8]) -> usize {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(json)
        .expect("writing to a Vec never fails");
    let deflated = encoder.finish().expect("finishing a Vec never fails");
    deflated.len().div_ceil(1024).max(1) * 1024
}

/// The highest HLC carried by any clock in the changeset (0 when it has none).
pub fn max_hlc(cs: &Changeset) -> u64 {
    clocks(cs).map(|c| c.hlc).max().unwrap_or(0)
}

/// The lowest HLC among the clocks of the changeset's author (`max_hlc` when it carries none):
/// the oldest of the author's own changes the envelope holds. A re-join re-queues own clocks from
/// here for an envelope the relay may not hold (§11.4).
pub fn min_author_hlc(cs: &Changeset) -> u64 {
    clocks(cs)
        .filter(|c| c.device == cs.device)
        .map(|c| c.hlc)
        .min()
        .unwrap_or_else(|| max_hlc(cs))
}

fn clocks(cs: &Changeset) -> impl Iterator<Item = &Clock> {
    cs.rows.iter().flat_map(|row| {
        std::iter::once(&row.generation)
            .chain(row.deleted.as_ref())
            .chain(row.columns.values().map(|(_, c)| c))
            .chain(row.parents.values())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::services::sync::registry;

    fn dev(n: u8) -> DeviceId {
        let mut b = [0u8; 16];
        b[15] = n;
        DeviceId(b)
    }

    fn clock(hlc: u64, d: u8) -> Clock {
        Clock {
            hlc,
            device: dev(d),
        }
    }

    fn spec_example() -> Changeset {
        let mut columns = BTreeMap::new();
        columns.insert(
            "amount".to_string(),
            (
                SqlValue::Text("-1250.00".into()),
                clock(1_777_000_000_000_123, 1),
            ),
        );
        columns.insert(
            "currency".to_string(),
            (
                SqlValue::Text("CZK".into()),
                clock(1_777_000_000_000_123, 1),
            ),
        );
        let mut parents = BTreeMap::new();
        parents.insert("account_id".to_string(), clock(1_776_000_000_000_000, 2));
        Changeset {
            schema: 29,
            barrier: 0,
            device: dev(1),
            seq: 418,
            prev: Some([7u8; 32]),
            rows: vec![
                RowChange {
                    table: "bank_transactions".into(),
                    key: RowKey::Single("c0a8-1".into()),
                    generation: clock(1_777_000_000_000_123, 1),
                    insert: true,
                    deleted: None,
                    columns,
                    parents,
                },
                RowChange {
                    table: "stock_investment_tags".into(),
                    key: RowKey::Composite(vec!["inv-uuid".into(), "tag-uuid".into()]),
                    generation: clock(1_776_000_000_000_001, 1),
                    insert: false,
                    deleted: Some(clock(1_777_000_000_000_456, 1)),
                    columns: BTreeMap::new(),
                    parents: BTreeMap::new(),
                },
            ],
        }
    }

    fn invalid(r: Result<Changeset>) -> bool {
        matches!(r, Err(AppError::Validation(m)) if m == "validation.syncChangesetInvalid")
    }

    #[test]
    fn the_spec_example_round_trips() {
        let cs = spec_example();
        assert_eq!(from_json(&to_json(&cs)).unwrap(), cs);
    }

    #[test]
    fn the_json_uses_the_spec_field_names() {
        let v: Json = serde_json::from_slice(&to_json(&spec_example())).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["schema"], 29);
        assert_eq!(v["seq"], 418);
        assert!(v.get("barrier").is_none());
        assert_eq!(v["device"], dev(1).to_b64());
        assert_eq!(v["devs"][0], dev(1).to_b64());
        assert_eq!(v["devs"][1], dev(2).to_b64());
        assert_eq!(v["rows"][0]["ins"], true);
        assert!(v["rows"][1].get("ins").is_none());
        assert!(v["rows"][1].get("c").is_none());
        assert!(v["rows"][1].get("p").is_none());
        assert!(v["rows"][1]["k"].is_array());
        assert_eq!(
            v["rows"][1]["del"],
            serde_json::json!([1_777_000_000_000_456u64, 0])
        );
        assert_eq!(v["rows"][0]["c"]["amount"][0], "-1250.00");
        assert_eq!(
            v["rows"][0]["g"],
            serde_json::json!([1_777_000_000_000_123u64, 0])
        );
        assert_eq!(
            v["rows"][0]["p"]["account_id"],
            serde_json::json!([1_776_000_000_000_000u64, 1])
        );
    }

    #[test]
    fn prev_is_omitted_for_the_first_changeset_and_barrier_travels_when_set() {
        let mut cs = spec_example();
        cs.seq = 1;
        cs.prev = None;
        cs.barrier = 3;
        let v: Json = serde_json::from_slice(&to_json(&cs)).unwrap();
        assert!(v.get("prev").is_none());
        assert_eq!(v["barrier"], 3);
        assert_eq!(from_json(&to_json(&cs)).unwrap(), cs);
    }

    #[test]
    fn a_row_encodes_and_decodes_on_its_own_with_a_shared_dev_table() {
        let cs = spec_example();
        let mut devs = DevTable::new();
        let a = encode_row(&cs.rows[0], &mut devs);
        let b = encode_row(&cs.rows[1], &mut devs);
        let table = devs.into_vec();
        assert_eq!(table, vec![dev(1), dev(2)]);
        assert_eq!(decode_row(&a, &table).unwrap(), cs.rows[0]);
        assert_eq!(decode_row(&b, &table).unwrap(), cs.rows[1]);
    }

    #[test]
    fn ties_keep_the_author_of_each_clock() {
        let mut row = spec_example().rows.remove(1);
        row.generation = clock(500, 1);
        row.deleted = Some(clock(500, 2));
        let mut devs = DevTable::new();
        let v = encode_row(&row, &mut devs);
        assert_eq!(v["g"], serde_json::json!([500, 0]));
        assert_eq!(v["del"], serde_json::json!([500, 1]));
        let table = devs.into_vec();
        assert_eq!(table, vec![dev(1), dev(2)]);
        assert_eq!(decode_row(&v, &table).unwrap(), row);
    }

    #[test]
    fn the_zero_clock_travels_with_the_zero_device() {
        let mut cs = spec_example();
        cs.rows[1].generation = Clock::ZERO;
        let v: Json = serde_json::from_slice(&to_json(&cs)).unwrap();
        let devs: Vec<String> = v["devs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_str().unwrap().to_string())
            .collect();
        assert!(devs.contains(&DeviceId::ZERO.to_b64()));
        assert_eq!(from_json(&to_json(&cs)).unwrap(), cs);
    }

    #[test]
    fn blob_values_travel_as_base64_objects() {
        let mut cs = spec_example();
        cs.rows[0].columns.insert(
            "blob".into(),
            (SqlValue::Blob(vec![0, 1, 254, 255]), clock(5, 1)),
        );
        cs.rows[0]
            .columns
            .insert("n".into(), (SqlValue::Integer(-7), clock(5, 1)));
        cs.rows[0]
            .columns
            .insert("z".into(), (SqlValue::Null, clock(5, 1)));
        let v: Json = serde_json::from_slice(&to_json(&cs)).unwrap();
        assert_eq!(
            v["rows"][0]["c"]["blob"][0],
            serde_json::json!({"b": "AAH+/w=="})
        );
        assert_eq!(v["rows"][0]["c"]["n"][0], -7);
        assert!(v["rows"][0]["c"]["z"][0].is_null());
        assert_eq!(from_json(&to_json(&cs)).unwrap(), cs);
    }

    #[test]
    fn sql_values_convert_to_and_from_rusqlite() {
        use rusqlite::types::Value;
        for (s, r) in [
            (SqlValue::Null, Value::Null),
            (SqlValue::Integer(3), Value::Integer(3)),
            (SqlValue::Real(0.5), Value::Real(0.5)),
            (SqlValue::Text("a".into()), Value::Text("a".into())),
            (SqlValue::Blob(vec![1]), Value::Blob(vec![1])),
        ] {
            assert_eq!(Value::from(s.clone()), r);
            assert_eq!(SqlValue::from(r), s);
        }
    }

    #[test]
    fn a_composite_key_text_matches_sqlite_json_array() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        assert_eq!(
            RowKey::Composite(vec!["inv-1".into(), "tag-1".into()]).to_pk_text(),
            conn.query_row("SELECT json_array('inv-1','tag-1')", [], |r| r
                .get::<_, String>(0))
                .unwrap()
        );
    }

    #[test]
    fn pk_text_round_trips_through_the_registry_key_shape() {
        let plain = registry::spec("bank_transactions").unwrap();
        let tags = registry::spec("stock_investment_tags").unwrap();
        let profile = registry::spec("user_profile").unwrap();
        assert_eq!(RowKey::Single("abc".into()).to_pk_text(), "abc");
        assert_eq!(RowKey::Singleton.to_pk_text(), "profile");
        assert_eq!(
            RowKey::from_pk_text(plain, "abc").unwrap(),
            RowKey::Single("abc".into())
        );
        assert_eq!(
            RowKey::from_pk_text(tags, r#"["a","b"]"#).unwrap(),
            RowKey::Composite(vec!["a".into(), "b".into()])
        );
        assert_eq!(
            RowKey::from_pk_text(profile, "profile").unwrap(),
            RowKey::Singleton
        );
        assert!(RowKey::from_pk_text(profile, "other").is_err());
        assert!(RowKey::from_pk_text(tags, "[not json").is_err());
    }

    fn base(rows: &str, devs: &str) -> String {
        format!(
            r#"{{"v":1,"schema":29,"device":"{}","seq":1,"devs":{devs},"rows":{rows}}}"#,
            dev(1).to_b64()
        )
    }

    #[test]
    fn an_unknown_device_index_is_rejected() {
        let json = base(r#"[{"t":"bonds","k":"x","g":[1,0]}]"#, "[]");
        assert!(invalid(from_json(json.as_bytes())));
        let json = base(
            r#"[{"t":"bonds","k":"x","g":[1,0]}]"#,
            &format!(r#"["{}"]"#, dev(1).to_b64()),
        );
        assert!(from_json(json.as_bytes()).is_ok());
    }

    #[test]
    fn a_clock_above_i64_max_or_negative_is_rejected() {
        let devs = format!(r#"["{}"]"#, dev(1).to_b64());
        for hlc in ["9223372036854775808", "18446744073709551615", "-1", "1.5"] {
            let json = base(
                &format!(r#"[{{"t":"bonds","k":"x","g":[{hlc},0]}}]"#),
                &devs,
            );
            assert!(invalid(from_json(json.as_bytes())), "{hlc}");
        }
        let json = base(
            r#"[{"t":"bonds","k":"x","g":[9223372036854775807,0]}]"#,
            &devs,
        );
        assert!(from_json(json.as_bytes()).is_ok());
    }

    #[test]
    fn devs_must_start_with_the_author_and_hold_no_duplicates() {
        let rows = r#"[{"t":"bonds","k":"x","g":[1,1]}]"#;
        let list = |ds: &[DeviceId]| {
            let quoted: Vec<String> = ds.iter().map(|d| format!(r#""{}""#, d.to_b64())).collect();
            format!("[{}]", quoted.join(","))
        };
        assert!(from_json(base(rows, &list(&[dev(1), dev(2)])).as_bytes()).is_ok());
        assert!(invalid(from_json(
            base(rows, &list(&[dev(2), dev(1)])).as_bytes()
        )));
        assert!(invalid(from_json(
            base(rows, &list(&[dev(1), dev(1)])).as_bytes()
        )));
        assert!(invalid(from_json(
            base(rows, &list(&[dev(1), dev(2), dev(2)])).as_bytes()
        )));
        assert!(invalid(from_json(base("[]", "[]").as_bytes())));
    }

    #[test]
    fn a_version_other_than_1_is_rejected() {
        let ok = base("[]", &format!(r#"["{}"]"#, dev(1).to_b64()));
        assert!(from_json(ok.as_bytes()).is_ok());
        assert!(invalid(from_json(
            ok.replace(r#""v":1"#, r#""v":2"#).as_bytes()
        )));
        assert!(invalid(from_json(ok.replace(r#""v":1,"#, "").as_bytes())));
        assert!(invalid(from_json(b"not json")));
        assert!(invalid(from_json(b"[]")));
    }

    #[test]
    fn padded_length_is_a_multiple_of_1024() {
        assert_eq!(padded_deflated_len(b""), 1024);
        assert_eq!(padded_deflated_len(&to_json(&spec_example())), 1024);
        assert_eq!(padded_deflated_len(&vec![b'a'; 100_000]), 1024);
        // xorshift bytes do not compress, so 5000 of them deflate to a few bytes over 5000
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let noise: Vec<u8> = (0..5000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect();
        assert_eq!(padded_deflated_len(&noise), 5120);
    }

    #[test]
    fn min_author_hlc_is_the_oldest_clock_of_the_author() {
        // dev(2)'s parent clock (…000) is older, but not the author's.
        assert_eq!(min_author_hlc(&spec_example()), 1_776_000_000_000_001);
        let mut cs = spec_example();
        cs.device = dev(3);
        assert_eq!(min_author_hlc(&cs), max_hlc(&cs), "no author clock");
    }

    #[test]
    fn max_hlc_is_the_highest_clock_anywhere_in_the_changeset() {
        assert_eq!(max_hlc(&spec_example()), 1_777_000_000_000_456);
        let mut cs = spec_example();
        cs.rows[0]
            .parents
            .insert("x".into(), clock(1_900_000_000_000_000, 2));
        assert_eq!(max_hlc(&cs), 1_900_000_000_000_000);
        cs.rows.clear();
        assert_eq!(max_hlc(&cs), 0);
    }

    #[test]
    fn extreme_and_awkward_values_round_trip() {
        let mut cs = spec_example();
        let c = clock(9, 1);
        for (name, v) in [
            ("min", SqlValue::Integer(i64::MIN)),
            ("max", SqlValue::Integer(i64::MAX)),
            ("empty", SqlValue::Blob(vec![])),
            ("quotes", SqlValue::Text("a\"b'c\\d\n".into())),
            (
                "unicode",
                SqlValue::Text("Žluťoučký kůň 日本 \u{1F600}".into()),
            ),
        ] {
            cs.rows[0].columns.insert(name.into(), (v, c));
        }
        assert_eq!(from_json(&to_json(&cs)).unwrap(), cs);
    }

    #[test]
    fn malformed_rows_are_errors_not_panics() {
        let devs = [dev(1)];
        let cases = [
            ("unknown table", r#"{"t":"nope","k":"x","g":[1,0]}"#),
            (
                "non-synced table",
                r#"{"t":"stock_value_history","k":"x","g":[1,0]}"#,
            ),
            ("table not a string", r#"{"t":5,"k":"x","g":[1,0]}"#),
            (
                "array key on primary table",
                r#"{"t":"bonds","k":["a","b"],"g":[1,0]}"#,
            ),
            (
                "array key on singleton",
                r#"{"t":"user_profile","k":["profile"],"g":[1,0]}"#,
            ),
            (
                "wrong singleton key",
                r#"{"t":"user_profile","k":"other","g":[1,0]}"#,
            ),
            (
                "string key on composite table",
                r#"{"t":"stock_investment_tags","k":"x","g":[1,0]}"#,
            ),
            (
                "short composite",
                r#"{"t":"stock_investment_tags","k":["a"],"g":[1,0]}"#,
            ),
            (
                "long composite",
                r#"{"t":"stock_investment_tags","k":["a","b","c"],"g":[1,0]}"#,
            ),
            (
                "non-string part",
                r#"{"t":"stock_investment_tags","k":["a",1],"g":[1,0]}"#,
            ),
            ("k is a number", r#"{"t":"bonds","k":7,"g":[1,0]}"#),
            (
                "ins without c",
                r#"{"t":"bonds","k":"x","g":[1,0],"ins":true}"#,
            ),
            (
                "ins with empty c",
                r#"{"t":"bonds","k":"x","g":[1,0],"ins":true,"c":{}}"#,
            ),
            (
                "ins not a bool",
                r#"{"t":"bonds","k":"x","g":[1,0],"ins":1}"#,
            ),
            (
                "REAL value",
                r#"{"t":"bonds","k":"x","g":[1,0],"c":{"a":[1.5,1,0]}}"#,
            ),
            (
                "bad blob base64",
                r#"{"t":"bonds","k":"x","g":[1,0],"c":{"a":[{"b":"!!"},1,0]}}"#,
            ),
            (
                "blob object without b",
                r#"{"t":"bonds","k":"x","g":[1,0],"c":{"a":[{"x":"AA=="},1,0]}}"#,
            ),
            ("c is an array", r#"{"t":"bonds","k":"x","g":[1,0],"c":[]}"#),
            (
                "c entry not an array",
                r#"{"t":"bonds","k":"x","g":[1,0],"c":{"a":5}}"#,
            ),
            (
                "c entry wrong length",
                r#"{"t":"bonds","k":"x","g":[1,0],"c":{"a":["v",1]}}"#,
            ),
            ("clock too short", r#"{"t":"bonds","k":"x","g":[1]}"#),
            ("clock too long", r#"{"t":"bonds","k":"x","g":[1,0,0]}"#),
            ("clock not an array", r#"{"t":"bonds","k":"x","g":1}"#),
            (
                "p entry wrong length",
                r#"{"t":"bonds","k":"x","g":[1,0],"p":{"a":[1]}}"#,
            ),
            ("missing g", r#"{"t":"bonds","k":"x"}"#),
            ("row not an object", r#"[]"#),
        ];
        for (name, row) in cases {
            let value: Json = serde_json::from_str(row).unwrap();
            assert!(
                matches!(decode_row(&value, &devs), Err(AppError::Validation(m)) if m == "validation.syncChangesetInvalid"),
                "{name}"
            );
        }
    }

    #[test]
    fn a_key_only_table_insert_round_trips_without_columns() {
        let row = RowChange {
            table: "real_estate_loans".into(),
            key: RowKey::Composite(vec!["re-1".into(), "loan-1".into()]),
            generation: clock(7, 1),
            insert: true,
            deleted: None,
            columns: BTreeMap::new(),
            parents: BTreeMap::new(),
        };
        let mut devs = DevTable::new();
        let v = encode_row(&row, &mut devs);
        assert!(v.get("c").is_none());
        assert_eq!(decode_row(&v, &devs.into_vec()).unwrap(), row);
    }

    #[test]
    fn a_row_deleted_and_recreated_carries_both_ins_and_del() {
        let devs = [dev(1)];
        let row: Json = serde_json::from_str(
            r#"{"t":"bonds","k":"x","g":[1,0],"ins":true,"del":[2,0],"c":{"name":["n",1,0]}}"#,
        )
        .unwrap();
        let decoded = decode_row(&row, &devs).unwrap();
        assert!(decoded.insert && decoded.deleted.is_some());
    }
}
