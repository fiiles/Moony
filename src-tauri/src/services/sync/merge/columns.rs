//! Per-column merge of a live row (D12): last writer wins by `(hlc, device)`, column groups taken
//! whole from the newer side, the weak `pending` of `stock_splits.status` (correction 9), and the
//! write-once `sync_devices.public_key`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::row::{Columns, RowImage, Table};
use crate::services::sync::changeset::{RowChange, SqlValue};
use crate::services::sync::hlc::Clock;
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
/// `clocks` (a column without one has the row clock `generation`).
pub(super) fn merge(
    table: &Table,
    change: &RowChange,
    incoming: &Columns,
    local: &RowImage,
    clocks: &HashMap<String, Clock>,
    generation: Clock,
    pk: &str,
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
            // Non-NULL beats NULL; between two values the older clock wins, so the outcome does
            // not depend on the order the devices see them in.
            ColumnClass::WriteOnce => match (local_value, value) {
                (SqlValue::Null, SqlValue::Null) => *clock > ours,
                (SqlValue::Null, _) => true,
                (_, SqlValue::Null) => false,
                (kept, offered) => {
                    if kept != offered {
                        merged.warnings.push(format!(
                            "{name} {pk}: two different values of {column}; kept the older one \
                             (written by device {})",
                            ours.min(*clock).device.to_b64()
                        ));
                    }
                    *clock < ours
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
