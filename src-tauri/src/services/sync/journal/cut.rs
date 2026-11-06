//! Cutting the ordered outbox into one envelope (spec §9.1): fill greedily, measure the padded
//! deflated size at checkpoints spaced by the observed compression ratio and always before
//! closing, and close before the row that pushes the envelope over `ENVELOPE_BUDGET`. Every closed
//! envelope was measured within its budget.
//!
//! A row that alone exceeds the budget travels alone up to `SINGLE_ROW_LIMIT`; above that it is
//! refused and stays queued, and so does every later row whose FK value names a key held back in
//! this call (a receiver stops on a missing parent).

use std::collections::HashSet;

use super::{Queued, Refused, ENVELOPE_BUDGET, SINGLE_ROW_LIMIT};
use crate::error::Result;
use crate::services::sync::changeset::{
    encode_row, padded_deflated_len, to_json, Changeset, DevTable, RowChange,
};

/// Smallest raw JSON growth between two measurements.
const MIN_MEASURE_STEP: usize = 8 * 1024;

/// A row as `cut` needs it: the change and the `(table, pk)` of every parent it references.
pub(super) struct ReadRow {
    pub row: RowChange,
    pub parents: Vec<(String, String)>,
}

pub(super) struct Cut {
    /// The envelope; no rows when nothing could be sent.
    pub changeset: Changeset,
    /// Outbox keys this call used up: the envelope's rows and keys with nothing to send.
    pub consumed: Vec<(String, String)>,
    pub refused: Vec<Refused>,
}

/// Fills `envelope` (which arrives without rows) from `queue` in order, reading each key with
/// `read`.
pub(super) fn cut<'q>(
    envelope: Changeset,
    queue: impl IntoIterator<Item = &'q Queued>,
    mut read: impl FnMut(&Queued) -> Result<Option<ReadRow>>,
) -> Result<Cut> {
    let mut filling = Filling::new(envelope);
    let mut nothing_to_send = Vec::new();
    // Refused keys and the rows held back because they reference one, transitively.
    let mut held: HashSet<(String, String)> = HashSet::new();
    let mut closed = false;
    for queued in queue {
        let key = (queued.table.clone(), queued.pk.clone());
        let Some(ReadRow { row, parents }) = read(queued)? else {
            nothing_to_send.push(key);
            continue;
        };
        if parents.iter().any(|p| held.contains(p)) {
            held.insert(key);
            continue;
        }
        filling.push(row, key);
        if filling.raw >= filling.next_measure {
            closed = filling.settle();
            // Only the first row of an envelope is ever refused (with rows before it, the
            // envelope closes before it instead), and such a row crosses the first checkpoint
            // on its own (over 180 KiB deflated is far more than `MIN_MEASURE_STEP` raw), so no
            // row after it has been pushed yet: holding back from here on is enough.
            held.extend(
                filling
                    .refused
                    .iter()
                    .map(|r| (r.table.clone(), r.pk.clone())),
            );
            if closed {
                break;
            }
        }
    }
    if !closed && filling.fits < filling.changeset.rows.len() {
        filling.settle();
    }
    let mut consumed = filling.keys;
    consumed.extend(nothing_to_send);
    Ok(Cut {
        changeset: filling.changeset,
        consumed,
        refused: filling.refused,
    })
}

struct Filling {
    changeset: Changeset,
    /// `(table, pk)` of each row, in step with `changeset.rows`.
    keys: Vec<(String, String)>,
    /// Leading rows known to fit the budget.
    fits: usize,
    /// Estimated raw JSON size of the envelope.
    raw: usize,
    /// The raw size at which to measure next.
    next_measure: usize,
    refused: Vec<Refused>,
}

impl Filling {
    fn new(changeset: Changeset) -> Self {
        let raw = to_json(&changeset).len();
        Self {
            changeset,
            keys: Vec::new(),
            fits: 0,
            raw,
            next_measure: raw + MIN_MEASURE_STEP,
            refused: Vec::new(),
        }
    }

    fn push(&mut self, row: RowChange, key: (String, String)) {
        // The row's own encoding plus its comma; device indices make this an estimate.
        self.raw += encode_row(&row, &mut DevTable::new()).to_string().len() + 1;
        self.changeset.rows.push(row);
        self.keys.push(key);
    }

    /// Padded deflated size of the envelope cut to its first `n` rows.
    fn measure(&mut self, n: usize) -> usize {
        let rest = self.changeset.rows.split_off(n);
        let len = padded_deflated_len(&to_json(&self.changeset));
        self.changeset.rows.extend(rest);
        len
    }

    fn truncate(&mut self, n: usize) {
        self.changeset.rows.truncate(n);
        self.keys.truncate(n);
    }

    /// After measuring `deflated` bytes at the current raw size, the next checkpoint is half the
    /// remaining room away at the observed ratio, and at least `MIN_MEASURE_STEP`.
    fn schedule(&mut self, deflated: usize) {
        let room = ENVELOPE_BUDGET.saturating_sub(deflated) as u128;
        let step = room * self.raw as u128 / deflated.max(1) as u128 / 2;
        let step = usize::try_from(step).unwrap_or(usize::MAX);
        self.next_measure = self.raw.saturating_add(step.max(MIN_MEASURE_STEP));
    }

    /// Measures the envelope. Returns `true` when it is closed (cut to the rows that fit, or to
    /// one row allowed to travel alone); `false` when every row so far fits.
    fn settle(&mut self) -> bool {
        loop {
            let len = self.changeset.rows.len();
            if len == 0 {
                self.fits = 0;
                self.next_measure = self.raw + MIN_MEASURE_STEP;
                return false;
            }
            let measured = self.measure(len);
            if measured <= ENVELOPE_BUDGET {
                self.fits = len;
                self.schedule(measured);
                return false;
            }
            // The longest prefix that fits: `lo` fits, `hi` does not.
            let (mut lo, mut hi) = (self.fits, len);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if self.measure(mid) <= ENVELOPE_BUDGET {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            if lo > 0 {
                self.truncate(lo);
                return true;
            }
            // The first row alone is over the budget.
            let alone = self.measure(1);
            if alone <= SINGLE_ROW_LIMIT {
                self.truncate(1);
                return true;
            }
            self.changeset.rows.remove(0);
            let (table, pk) = self.keys.remove(0);
            log::error!(
                "[SYNC] {table} {pk} is {alone} bytes deflated, over the {SINGLE_ROW_LIMIT}-byte \
                 limit of one row; it and the rows referencing it stay queued and are not sent"
            );
            self.refused.push(Refused {
                table,
                pk,
                bytes: alone,
            });
            self.raw = to_json(&self.changeset).len();
            self.fits = 0;
        }
    }
}
