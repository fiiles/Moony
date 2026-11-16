//! Per-column merge of a live row (D12): last writer wins by `(hlc, device)`, column groups taken
//! whole from the newer side, the weak `pending` of `stock_splits.status` (correction 9), and the
//! write-once `sync_devices` columns, where a public key, certificate or `left` record stamped
//! with the device's own ID beats one stamped with any other (audit M6; see `own_key` for what
//! that stamp is worth), and a certificate or record that holds up on its own beats one that does
//! not (`identity::write_once_value_valid`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::row::{Columns, RowImage, Table};
use crate::services::sync::changeset::{RowChange, SqlValue};
use crate::services::sync::hlc::Clock;
use crate::services::sync::identity;
use crate::services::sync::registry::{column_class, group_of, ColumnClass};

/// The value a detection inserts and that a later apply or dismiss always beats.
const WEAK_STATUS: &str = "pending";

/// What merging one incoming row into the live local row writes.
pub(super) struct Merged {
    /// column → (value, its new clock), for every column the incoming side won.
    pub writes: BTreeMap<String, (SqlValue, Clock)>,
    pub warnings: Vec<String>,
}

/// Merges the carried `incoming` columns into the local row `local`, whose column clocks are
/// `clocks` (a column without one has the row clock `generation`). `vault` is the bound vault's
/// ID, which a `sync_devices` certificate or record must name (unchecked when `None`).
#[allow(clippy::too_many_arguments)]
pub(super) fn merge(
    table: &Table,
    change: &RowChange,
    incoming: &Columns,
    local: &RowImage,
    clocks: &HashMap<String, Clock>,
    generation: Clock,
    pk: &str,
    vault: Option<&[u8; 16]>,
) -> Merged {
    let name = table.spec.name;
    let local_clock = |column: &str| clocks.get(column).copied().unwrap_or(generation);
    let mut merged = Merged {
        writes: BTreeMap::new(),
        warnings: Vec::new(),
    };
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for (column, (value, clock)) in incoming {
        if done.contains(column.as_str()) {
            continue;
        }
        let ours = local_clock(column);
        let local_value = local.get(column).unwrap_or(&SqlValue::Null);
        let theirs_win = match column_class(name, column) {
            // Weak loses to non-weak whatever the clocks; two weak values are equal.
            ColumnClass::Monotonic => {
                let ours_weak = is_weak_local(name, column, local_value, clocks);
                let theirs_weak = is_weak_incoming(name, column, value, *clock, change);
                match (ours_weak, theirs_weak) {
                    (true, true) | (false, true) => false,
                    (true, false) => true,
                    (false, false) => *clock > ours,
                }
            }
            // Non-NULL beats NULL. A device's public key, certificate or `left` record stamped
            // with the device's own ID (the clock's device is the row's, `own_key`) beats one
            // stamped with any other ID (audit
            // M6); between two alike the older clock wins (write-once, as `devices.rs` keeps the
            // first key with `COALESCE`). A fixed order on (value, clock), so the outcome does not
            // depend on the order the devices see them in.
            ColumnClass::WriteOnce => match (local_value, value) {
                (SqlValue::Null, SqlValue::Null) => *clock > ours,
                (SqlValue::Null, _) => true,
                (_, SqlValue::Null) => false,
                // A certificate or record that holds up on its own beats one that does not,
                // whatever the clocks and stamps: an older value that never verifies cannot lock
                // the column. A `left` record is checked against the row's certificate as this
                // merge leaves it ("cert" sorts before "left_record", so it is decided already).
                (kept, offered)
                    if is_checked(name, column)
                        && valid(column, pk, kept, vault, &merged.writes, local)
                            != valid(column, pk, offered, vault, &merged.writes, local) =>
                {
                    let theirs_win = valid(column, pk, offered, vault, &merged.writes, local);
                    let refused = if theirs_win { ours } else { *clock };
                    merged.warnings.push(format!(
                        "{name} {pk}: device {} offered a {column} that does not verify; kept \
                         the one that does",
                        refused.device.to_b64()
                    ));
                    theirs_win
                }
                (kept, offered) => {
                    let own = |clock: Clock| own_key(name, column, pk, clock);
                    let theirs_win = match (own(ours), own(*clock)) {
                        (false, true) => true,
                        (true, false) => false,
                        _ => *clock < ours,
                    };
                    if kept != offered && own(ours) != own(*clock) {
                        let refused = if theirs_win { ours } else { *clock };
                        merged.warnings.push(format!(
                            "{name}: device {} offered a {column} stamped with another device's \
                             ID; kept the one stamped with the row's own device",
                            refused.device.to_b64()
                        ));
                    } else if kept != offered {
                        merged.warnings.push(format!(
                            "{name} {pk}: two different values of {column}; kept the older one \
                             (written by device {})",
                            ours.min(*clock).device.to_b64()
                        ));
                    }
                    theirs_win
                }
            },
            _ => {
                let members: Vec<&str> = match group_of(name, column) {
                    Some(group) => group.iter().copied().filter(|m| table.has(m)).collect(),
                    None => vec![column.as_str()],
                };
                done.extend(members.iter().copied());
                let theirs = members
                    .iter()
                    .filter_map(|m| incoming.get(*m).map(|(_, c)| *c))
                    .max();
                let ours = members.iter().map(|m| local_clock(m)).max();
                if theirs > ours {
                    for member in members {
                        if let Some(entry) = incoming.get(member) {
                            merged.writes.insert(member.to_string(), entry.clone());
                        }
                    }
                }
                continue;
            }
        };
        if theirs_win {
            merged
                .writes
                .insert(column.clone(), (value.clone(), *clock));
        }
    }
    merged
}

/// The write-once columns whose values are checked on their own (`identity::write_once_value_valid`).
fn is_checked(table: &str, column: &str) -> bool {
    table == "sync_devices" && matches!(column, "cert" | "left_record" | "removal_record")
}

/// Whether `value` of the checked column holds up on its own. The row's certificate for a `left`
/// record is the one this merge writes, else the local one.
fn valid(
    column: &str,
    pk: &str,
    value: &SqlValue,
    vault: Option<&[u8; 16]>,
    writes: &BTreeMap<String, (SqlValue, Clock)>,
    local: &RowImage,
) -> bool {
    let SqlValue::Text(value) = value else {
        return false;
    };
    let row_cert = writes
        .get("cert")
        .map(|(v, _)| v)
        .or_else(|| local.get("cert"))
        .and_then(|v| match v {
            SqlValue::Text(text) => Some(text.as_str()),
            _ => None,
        });
    identity::write_once_value_valid(column, pk, value, vault, row_cert)
}

/// Whether `clock` stamps a device's public key, certificate or `left` record with the device's
/// own ID: the row of `sync_devices` is keyed by the device's ID in hex, and the clock's device is
/// that ID; each of the three is written by the device itself. Always `false` for every other
/// column (`removal_record` is signed by another device, so the older value stays). The stamp is
/// not proof: any holder of the vault key writes changesets with whatever clock device it likes,
/// so it can forge it. It only stops an honest device's stale or mistaken write from replacing
/// another device's value; what decides who gets a new key is the signed certificate, verified
/// and pinned by the trust store (phase 5), never this column alone.
fn own_key(table: &str, column: &str, pk: &str, clock: Clock) -> bool {
    table == "sync_devices"
        && matches!(column, "public_key" | "cert" | "left_record")
        && pk == clock.device.to_hex()
}

fn is_weak_value(table: &str, column: &str, value: &SqlValue) -> bool {
    column_class(table, column) == ColumnClass::Monotonic
        && matches!(value, SqlValue::Text(text) if text == WEAK_STATUS)
}

/// A local monotonic value is weak when it is `pending` and has no column clock of its own: the
/// insert (a detection) wrote it, never an update.
pub(super) fn is_weak_local(
    table: &str,
    column: &str,
    value: &SqlValue,
    clocks: &HashMap<String, Clock>,
) -> bool {
    is_weak_value(table, column, value) && !clocks.contains_key(column)
}

/// An incoming monotonic value is weak when it is `pending` and its clock is the row's
/// generation: it was never updated.
pub(super) fn is_weak_incoming(
    table: &str,
    column: &str,
    value: &SqlValue,
    clock: Clock,
    change: &RowChange,
) -> bool {
    is_weak_value(table, column, value) && clock == change.generation
}

/// The explicit column clock a column keeps once the row clock is `star`: none when its clock is
/// the row clock. A weak `pending` never gets one (that is what keeps it weak); a strong `pending`
/// (a revert) always does.
pub(super) fn explicit_clock(
    table: &str,
    column: &str,
    value: &SqlValue,
    clock: Clock,
    star: Clock,
    weak: bool,
) -> Option<Clock> {
    if is_weak_value(table, column, value) {
        return (!weak).then_some(clock);
    }
    (clock != star).then_some(clock)
}
