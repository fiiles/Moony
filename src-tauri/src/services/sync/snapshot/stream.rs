//! The snapshot record stream (§8, D16): each record is a 4-byte big-endian length followed by
//! one JSON object whose `"r"` names it — `devs`, `header`, `row`, `seed` or `end`. A row record
//! is `{"r":"row", …encode_row(…)}` (the changeset's row codec, R8), and its clocks index the
//! stream's one `devs` record, which comes first. Phase 2 deflates and encrypts the same bytes.
//!
//! Order: `devs`, `header`, every `row`, every `seed`, `end`. A reader takes the whole stream
//! (phase 1 holds it in memory) and rejects one without `end`, with counts that differ, or with
//! anything after `end`, before the caller writes anything.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Map, Value as Json};

use crate::error::{AppError, Result};
use crate::services::sync::changeset::{
    decode_row, encode_row, seed_value_from, value_json, DevTable, RowChange, SqlValue,
};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::registry::{self, TableClass};

/// The largest record a reader accepts (the relay's 100 MiB request limit).
const MAX_RECORD: u64 = 100 * 1024 * 1024;

/// A device's chain head as `sync_heads` holds it; on the wire `[device, seq, hash, quarantined]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub device: DeviceId,
    pub seq: u64,
    pub hash: [u8; 32],
    /// The author's chain forked on the dumping device (§9.3): restoring keeps it held.
    pub quarantined: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Header {
        schema: u32,
        barrier: u32,
        seq: u64,
        heads: Vec<Head>,
        /// The relay sequence of the barrier marker the dumping device saw (`sync_state.barrier_seq`);
        /// `"barrierSeq"`, omitted when `None`.
        barrier_seq: Option<u64>,
        /// The dumping device's pending below-barrier miss (`sync_state.missed_below_barrier`):
        /// such a snapshot heals nobody (ruling T8-6).
        missed_below_barrier: Option<u64>,
        /// The dumping device's miss of a skipped change (`state::skipped_miss`, Task 17 I1): such
        /// a snapshot heals nobody either, and a restorer inherits it.
        ///
        /// On the wire `"missedBelowBarrier"` is the max of the two (omitted when both are
        /// `None`), so a reader that knows only that key never heals from either; with a skipped
        /// miss, `"missedSkipped"` carries it and `"missedBarrier"` the below-barrier one alone.
        missed_skipped: Option<u64>,
    },
    Devices(Vec<DeviceId>),
    /// Live rows and tombstone-only changes alike.
    Row(RowChange),
    Seed {
        table: String,
        row: BTreeMap<String, SqlValue>,
    },
    End {
        rows: u64,
        seeds: u64,
    },
}

/// A whole stream, read and checked.
#[derive(Debug)]
pub(super) struct Snapshot {
    pub schema: u32,
    pub barrier: u32,
    pub seq: u64,
    pub heads: Vec<Head>,
    pub barrier_seq: Option<u64>,
    pub missed_below_barrier: Option<u64>,
    pub missed_skipped: Option<u64>,
    pub rows: Vec<RowChange>,
    pub seeds: Vec<(String, BTreeMap<String, SqlValue>)>,
}

pub(super) fn invalid() -> AppError {
    AppError::Validation("validation.syncSnapshotInvalid".to_string())
}

/// The record as JSON. A row's clocks are indexed into `devs`.
pub(super) fn encode(record: &Record, devs: &mut DevTable) -> Json {
    let mut out = Map::new();
    match record {
        Record::Devices(list) => {
            out.insert("r".into(), json!("devs"));
            let list: Vec<String> = list.iter().map(DeviceId::to_b64).collect();
            out.insert("devs".into(), json!(list));
        }
        Record::Header {
            schema,
            barrier,
            seq,
            heads,
            barrier_seq,
            missed_below_barrier,
            missed_skipped,
        } => {
            out.insert("r".into(), json!("header"));
            out.insert("schema".into(), json!(schema));
            out.insert("barrier".into(), json!(barrier));
            out.insert("seq".into(), json!(seq));
            let heads: Vec<Json> = heads
                .iter()
                .map(|h| {
                    json!([
                        h.device.to_b64(),
                        h.seq,
                        URL_SAFE_NO_PAD.encode(h.hash),
                        h.quarantined
                    ])
                })
                .collect();
            out.insert("heads".into(), Json::Array(heads));
            if let Some(barrier_seq) = barrier_seq {
                out.insert("barrierSeq".into(), json!(barrier_seq));
            }
            if let Some(missed) = missed_below_barrier.max(missed_skipped) {
                out.insert("missedBelowBarrier".into(), json!(missed));
            }
            if let Some(skipped) = missed_skipped {
                out.insert("missedSkipped".into(), json!(skipped));
                if let Some(missed) = missed_below_barrier {
                    out.insert("missedBarrier".into(), json!(missed));
                }
            }
        }
        Record::Row(row) => {
            out.insert("r".into(), json!("row"));
            // `encode_row` always returns an object.
            if let Json::Object(fields) = encode_row(row, devs) {
                out.extend(fields);
            }
        }
        Record::Seed { table, row } => {
            out.insert("r".into(), json!("seed"));
            out.insert("t".into(), json!(table));
            let values: Map<String, Json> = row
                .iter()
                .map(|(column, value)| (column.clone(), value_json(value)))
                .collect();
            out.insert("v".into(), Json::Object(values));
        }
        Record::End { rows, seeds } => {
            out.insert("r".into(), json!("end"));
            out.insert("rows".into(), json!(rows));
            out.insert("seeds".into(), json!(seeds));
        }
    }
    Json::Object(out)
}

/// Writes one length-prefixed record.
pub(super) fn write_record(out: &mut impl Write, record: &Json) -> Result<()> {
    let bytes = serde_json::to_vec(record)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| AppError::Internal("sync snapshot: a record exceeds 4 GiB".into()))?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(&bytes)?;
    Ok(())
}

/// The next record, `None` at the end of the input. A record cut short is an error. `devs` is
/// the stream's `devs` record (empty before it is read).
pub fn read_record(input: &mut impl Read, devs: &[DeviceId]) -> Result<Option<Record>> {
    let mut prefix = Vec::with_capacity(4);
    input.by_ref().take(4).read_to_end(&mut prefix)?;
    match prefix.len() {
        0 => return Ok(None),
        4 => {}
        _ => return Err(invalid()),
    }
    let len = u64::from(u32::from_be_bytes([
        prefix[0], prefix[1], prefix[2], prefix[3],
    ]));
    if len > MAX_RECORD {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    input.by_ref().take(len).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(invalid());
    }
    let value: Json = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    decode(&value, devs).map(Some)
}

fn decode(value: &Json, devs: &[DeviceId]) -> Result<Record> {
    let o = value.as_object().ok_or_else(invalid)?;
    let number = |key: &str| o.get(key).and_then(Json::as_u64).ok_or_else(invalid);
    let small = |key: &str| u32::try_from(number(key)?).map_err(|_| invalid());
    Ok(
        match o.get("r").and_then(Json::as_str).ok_or_else(invalid)? {
            "devs" => {
                let list = o
                    .get("devs")
                    .and_then(Json::as_array)
                    .ok_or_else(invalid)?
                    .iter()
                    .map(|d| {
                        DeviceId::from_b64(d.as_str().ok_or_else(invalid)?).map_err(|_| invalid())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut unique = list.clone();
                unique.sort();
                unique.dedup();
                if unique.len() != list.len() {
                    return Err(invalid());
                }
                Record::Devices(list)
            }
            "header" => {
                let optional = |key: &str| {
                    o.get(key)
                        .map(|v| v.as_u64().ok_or_else(invalid))
                        .transpose()
                };
                let missed_skipped = optional("missedSkipped")?;
                // With a skipped miss, `missedBelowBarrier` is the max of both and the
                // below-barrier one alone is `missedBarrier`.
                let missed_below_barrier = if missed_skipped.is_some() {
                    optional("missedBarrier")?
                } else {
                    optional("missedBelowBarrier")?
                };
                Record::Header {
                    schema: small("schema")?,
                    barrier: small("barrier")?,
                    seq: number("seq")?,
                    heads: o
                        .get("heads")
                        .and_then(Json::as_array)
                        .ok_or_else(invalid)?
                        .iter()
                        .map(decode_head)
                        .collect::<Result<_>>()?,
                    barrier_seq: o
                        .get("barrierSeq")
                        .map(|v| v.as_u64().ok_or_else(invalid))
                        .transpose()?,
                    missed_below_barrier,
                    missed_skipped,
                }
            }
            "row" => Record::Row(decode_row(value, devs).map_err(|_| invalid())?),
            "seed" => {
                let table = o.get("t").and_then(Json::as_str).ok_or_else(invalid)?;
                if !registry::spec(table).is_some_and(|s| s.class == TableClass::Seed) {
                    return Err(invalid());
                }
                let row = o
                    .get("v")
                    .and_then(Json::as_object)
                    .ok_or_else(invalid)?
                    .iter()
                    .map(|(column, value)| Ok((column.clone(), seed_value_from(value)?)))
                    .collect::<Result<_>>()?;
                Record::Seed {
                    table: table.to_string(),
                    row,
                }
            }
            "end" => Record::End {
                rows: number("rows")?,
                seeds: number("seeds")?,
            },
            _ => return Err(invalid()),
        },
    )
}

fn decode_head(value: &Json) -> Result<Head> {
    let [device, seq, hash, quarantined] =
        value.as_array().map(Vec::as_slice).ok_or_else(invalid)?
    else {
        return Err(invalid());
    };
    let device = DeviceId::from_b64(device.as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
    let seq = seq.as_u64().ok_or_else(invalid)?;
    let hash = URL_SAFE_NO_PAD
        .decode(hash.as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let hash = <[u8; 32]>::try_from(hash).map_err(|_| invalid())?;
    Ok(Head {
        device,
        seq,
        hash,
        quarantined: quarantined.as_bool().ok_or_else(invalid)?,
    })
}

/// Reads and checks the whole stream: `devs`, `header`, rows, seeds, `end` with matching counts,
/// and nothing after it.
pub(super) fn read_all(input: &mut impl Read) -> Result<Snapshot> {
    let Some(Record::Devices(devs)) = read_record(input, &[])? else {
        return Err(invalid());
    };
    let Some(Record::Header {
        schema,
        barrier,
        seq,
        heads,
        barrier_seq,
        missed_below_barrier,
        missed_skipped,
    }) = read_record(input, &devs)?
    else {
        return Err(invalid());
    };
    let mut rows = Vec::new();
    let mut seeds = Vec::new();
    loop {
        match read_record(input, &devs)? {
            Some(Record::Row(row)) if seeds.is_empty() => rows.push(row),
            Some(Record::Seed { table, row }) => seeds.push((table, row)),
            Some(Record::End {
                rows: row_count,
                seeds: seed_count,
            }) if row_count == rows.len() as u64 && seed_count == seeds.len() as u64 => break,
            _ => return Err(invalid()),
        }
    }
    if read_record(input, &devs)?.is_some() {
        return Err(invalid());
    }
    Ok(Snapshot {
        schema,
        barrier,
        seq,
        heads,
        barrier_seq,
        missed_below_barrier,
        missed_skipped,
        rows,
        seeds,
    })
}
