//! The convergence harness (spec §18): in-memory devices sharing one vault, a fake relay log, and
//! random operations through the real services. Changesets travel through the wire format
//! (`to_json` / `from_json`) and are pulled in random orders, adversarial ones included (one
//! author's envelopes all first, replays, a withheld envelope offered later); every pulled
//! envelope is judged by the chain as the engine judges it (R13). Devices also join mid-run from
//! another device's snapshot and restore by merge (a `410`). At the end every synced table must
//! be equal on every device (`test_support::divergence`).
//!
//! A seed replays its choices: the tests draw the schedule (operation kinds, devices, pushes,
//! pulls, orders) and the targets from two rngs, operations pick their rows by content, and the
//! hybrid logical clock reads a fake wall clock (`hlc::fake_clock`) that every vault restarts at
//! the same instant and moves on by a second before every step (operation, push, pull, join,
//! restore). Row IDs (random UUIDs minted by the services) and the services' own
//! wall-clock stamps still differ from run to run.

mod ops;
mod scenarios;
mod split_seeds;
mod tests;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::chain::{self, judge, EnvelopeMeta, Verdict};
use super::changeset::{from_json, to_json, Changeset};
use super::hlc::{fake_clock, DeviceId};
use super::journal::{build_next, record_sealed};
use super::merge::{apply_changeset, Outcome as Merged, StopReason};
use super::test_support::{divergence, migrated};
use super::{snapshot, state};
use crate::error::{AppError, Result};
use ops::Outcome;

/// Where every vault's fake wall clock starts: 2026-11-01 00:00 UTC, in milliseconds.
const FAKE_CLOCK_START_MS: u64 = 1_793_491_200_000;
/// How far the fake wall clock moves before every step.
const STEP_MS: u64 = 1_000;

pub struct Device {
    pub conn: Connection,
    pub id: DeviceId,
    /// Relay indices this device has dealt with (applied, skipped, acknowledged or held).
    pub processed: BTreeSet<usize>,
    /// Relay indices kept for a quarantined author.
    pub held: Vec<usize>,
}

pub struct Vault {
    pub devices: Vec<Device>,
    /// The relay log: every sealed envelope as it came off the wire, with its hash.
    pub relay: Vec<(Changeset, [u8; 32])>,
    /// What happened, in order, for the failure message.
    pub trace: Vec<String>,
    /// Names the run in a failure message (the seed).
    pub label: String,
    /// How often each verdict was handed out (and other events the tests count).
    pub verdicts: BTreeMap<String, usize>,
    /// Per operation: `[done, rejected, no target]`.
    pub stats: BTreeMap<&'static str, [usize; 3]>,
    clones: u8,
    /// The profiles' data directory (deletes clean attachment files there).
    dir: tempfile::TempDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullEnd {
    Done,
    Stopped(Verdict),
    MergeStopped(StopReason),
}

/// The device IDs of the vault, fixed so a seed replays the same tie-breaks.
fn device_id(i: usize) -> DeviceId {
    DeviceId([0x10 + i as u8; 16])
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A freshly migrated profile at `path`: a copy (`VACUUM INTO`) of one migrated template per
/// test process, since running the migrations for every device was most of a run's time.
fn fresh_profile(path: &Path) -> Result<Connection> {
    static TEMPLATE: OnceLock<Mutex<Connection>> = OnceLock::new();
    let template = TEMPLATE.get_or_init(|| Mutex::new(migrated()));
    let target = path
        .to_str()
        .ok_or_else(|| AppError::Internal(format!("{path:?} is not UTF-8")))?;
    template
        .lock()
        .map_err(|_| AppError::Internal("the template profile is poisoned".into()))?
        .execute("VACUUM INTO ?1", [target])?;
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA journal_mode = MEMORY; PRAGMA synchronous = OFF;",
    )?;
    Ok(conn)
}

impl Vault {
    /// Restarts the fake wall clock, then: device 0 gets `seed_ops`, then `state::enable`
    /// (capture installed, the rule-pack keys queued). Devices 1.. join from device 0's snapshot
    /// through `snapshot::join` (R4: state, restore without capture, install).
    pub fn new(
        label: impl Into<String>,
        n: usize,
        seed_ops: impl FnOnce(&mut Connection, &Path),
    ) -> Self {
        fake_clock::start(FAKE_CLOCK_START_MS);
        let label = label.into();
        let setup = |what: &str, e: AppError| -> ! {
            panic!("{label}: setting up the vault, {what} failed: {e}")
        };
        let dir = tempfile::tempdir().unwrap_or_else(|e| setup("the data directory", e.into()));
        let mut first = fresh_profile(&dir.path().join("device-0.db"))
            .unwrap_or_else(|e| setup("creating device 0", e));
        seed_ops(&mut first, dir.path());
        if let Err(e) = state::enable(&first, device_id(0)) {
            setup("enabling device 0", e);
        }
        let mut vault = Vault {
            devices: vec![Device {
                conn: first,
                id: device_id(0),
                processed: BTreeSet::new(),
                held: Vec::new(),
            }],
            relay: Vec::new(),
            trace: Vec::new(),
            label,
            verdicts: BTreeMap::new(),
            stats: BTreeMap::new(),
            clones: 0,
            dir,
        };
        for _ in 1..n {
            vault.join_device(0);
        }
        vault
    }

    /// Fails the test with the run's label, its trace and `message`.
    pub fn fail(&self, message: &str) -> ! {
        panic!(
            "{}: {message}\n--- trace ({} steps) ---\n{}",
            self.label,
            self.trace.len(),
            self.trace.join("\n")
        )
    }

    /// The value, or the run fails with what was being done and the engine's error.
    fn check<T>(&self, what: &str, result: Result<T>) -> T {
        result.unwrap_or_else(|e| self.fail(&format!("{what}: engine error: {e}")))
    }

    /// One operation on device `d`: its kind from `schedule`, its targets and arguments from
    /// `target`, through the real services.
    pub fn op(&mut self, d: usize, schedule: &mut StdRng, target: &mut StdRng) {
        let name = ops::random_name(schedule);
        self.run_op(d, name, target);
    }

    /// The operation `name` on device `d`.
    pub fn run_op(&mut self, d: usize, name: &str, target: &mut StdRng) {
        fake_clock::advance(STEP_MS);
        let step = ops::run_op(&mut self.devices[d].conn, target, self.dir.path(), name);
        let step = self.check(&format!("op d{d} {name}"), step);
        let slot = match step.outcome {
            Outcome::Done => 0,
            Outcome::Rejected(_) => 1,
            Outcome::NoTarget => 2,
        };
        self.stats.entry(step.name).or_default()[slot] += 1;
        self.trace.push(format!(
            "op d{d} {} {} -> {:?}",
            step.name, step.detail, step.outcome
        ));
    }

    /// Seals everything device `d` has queued, one envelope per transaction (`push_one`).
    /// Returns how many envelopes went out.
    pub fn push(&mut self, d: usize) -> usize {
        fake_clock::advance(STEP_MS);
        let mut sent = 0;
        while self.push_one(d) {
            sent += 1;
        }
        sent
    }

    /// Seals device `d`'s next envelope (`build_next`, hash, `record_sealed`, commit) and appends
    /// it to the relay as it decodes off the wire. `false` when nothing is queued.
    pub fn push_one(&mut self, d: usize) -> bool {
        let what = format!("push d{d}");
        let built = self.check(&what, seal_next(&self.devices[d].conn));
        let Some((changeset, bytes, hash)) = built else {
            return false;
        };
        let wire = self.check(&what, from_json(&bytes));
        if wire != changeset {
            self.fail(&format!(
                "device {d}: envelope {} changed on the wire",
                changeset.seq
            ));
        }
        self.trace.push(format!(
            "push d{d} seq {} ({} rows) -> relay {}",
            wire.seq,
            wire.rows.len(),
            self.relay.len()
        ));
        self.relay.push((wire, hash));
        true
    }

    /// Offers device `d` the relay entries of `order` it has not dealt with, judging each (R13).
    /// The trace records every entry offered with its verdict.
    pub fn pull(&mut self, d: usize, order: &[usize]) -> PullEnd {
        fake_clock::advance(STEP_MS);
        let mut seen = Vec::new();
        let end = self.pull_inner(d, order, &mut seen);
        self.trace
            .push(format!("pull d{d} [{}] -> {end:?}", seen.join(", ")));
        end
    }

    fn pull_inner(&mut self, d: usize, order: &[usize], seen: &mut Vec<String>) -> PullEnd {
        for &i in order {
            if self.devices[d].processed.contains(&i) {
                continue;
            }
            let (cs, hash) = self.relay[i].clone();
            let meta = EnvelopeMeta::of(&cs, hash, i as u64 + 1);
            let what = format!("pull d{d} relay {i}");
            // As the engine does: the marker is noted before the verdict, whatever it is.
            let noted = chain::note_barrier_marker(&self.devices[d].conn, &meta);
            self.check(&what, noted);
            loop {
                let verdict = self.check(&what, judge(&self.devices[d].conn, &meta));
                seen.push(format!("{i} {verdict:?}"));
                *self.verdicts.entry(format!("{verdict:?}")).or_default() += 1;
                let conn = &self.devices[d].conn;
                match verdict {
                    Verdict::Apply => {
                        match self.check(&what, apply_changeset(conn, &cs, hash, now())) {
                            Merged::Applied(_) => {}
                            Merged::Stopped(reason) => return PullEnd::MergeStopped(reason),
                        }
                    }
                    Verdict::SkipReplay => {}
                    Verdict::OwnAcknowledged => {
                        self.check(&what, chain::acknowledge_own(conn, cs.seq));
                    }
                    Verdict::Clone => {
                        let new = DeviceId([0xC1 + self.clones; 16]);
                        self.check(&what, chain::rejoin(conn, new, Some(&meta)));
                        self.clones += 1;
                        self.devices[d].id = new;
                        continue;
                    }
                    Verdict::Quarantine => {
                        self.check(&what, chain::quarantine(conn, cs.device));
                        self.devices[d].held.push(i);
                    }
                    Verdict::Held => self.devices[d].held.push(i),
                    Verdict::BelowBarrier => {
                        self.check(&what, chain::skip_below_barrier(conn, &meta));
                    }
                    Verdict::Gap | Verdict::NewerSchema | Verdict::Invalid => {
                        return PullEnd::Stopped(verdict)
                    }
                }
                break;
            }
            self.devices[d].processed.insert(i);
        }
        PullEnd::Done
    }

    /// The relay entries device `d` has not dealt with, by author, each author's in sequence
    /// order (a replay right after its original).
    fn unprocessed_by_author(&self, d: usize) -> BTreeMap<DeviceId, Vec<usize>> {
        let mut by_author: BTreeMap<DeviceId, Vec<usize>> = BTreeMap::new();
        for (i, (cs, _)) in self.relay.iter().enumerate() {
            if !self.devices[d].processed.contains(&i) {
                by_author.entry(cs.device).or_default().push(i);
            }
        }
        for entries in by_author.values_mut() {
            entries.sort_by_key(|&i| (self.relay[i].0.seq, i));
        }
        by_author
    }

    /// A random interleaving of everything device `d` has not dealt with that keeps every
    /// author's sequence order; `partial` cuts it at a random point (each author's prefix).
    pub fn random_order(&self, d: usize, schedule: &mut StdRng, partial: bool) -> Vec<usize> {
        let mut queues: Vec<VecDeque<usize>> = self
            .unprocessed_by_author(d)
            .into_values()
            .map(VecDeque::from)
            .collect();
        let mut order = Vec::new();
        loop {
            let open: Vec<usize> = (0..queues.len())
                .filter(|&q| !queues[q].is_empty())
                .collect();
            if open.is_empty() {
                break;
            }
            let q = open[schedule.random_range(0..open.len())];
            order.extend(queues[q].pop_front());
        }
        if partial && !order.is_empty() {
            order.truncate(schedule.random_range(0..=order.len()));
        }
        order
    }

    /// Device `d` pulls in its own random order. Order alone may only stop a pull on a parent
    /// that arrives later (an author's envelope ahead of another author's it depends on);
    /// anything else fails the run.
    pub fn pull_random(&mut self, d: usize, schedule: &mut StdRng, partial: bool) {
        let order = self.random_order(d, schedule, partial);
        match self.pull(d, &order) {
            PullEnd::Done | PullEnd::MergeStopped(StopReason::MissingParent { .. }) => {}
            other => self.fail(&format!("device {d} stopped on {other:?}")),
        }
    }

    /// Everyone pushes, everyone pulls everything (a random order first, then the relay order
    /// if that stopped on a parent still to come), until nothing more is pushed.
    pub fn sync_all(&mut self, schedule: &mut StdRng) {
        for _ in 0..6 {
            let pushed: usize = (0..self.devices.len()).map(|d| self.push(d)).sum();
            for d in 0..self.devices.len() {
                self.pull_random(d, schedule, false);
                let all: Vec<usize> = (0..self.relay.len()).collect();
                let end = self.pull(d, &all);
                if end != PullEnd::Done {
                    self.fail(&format!(
                        "device {d} could not pull the relay in its order: {end:?}"
                    ));
                }
            }
            if pushed == 0 {
                return;
            }
        }
        self.fail("the devices still push after six exchanges");
    }

    /// The snapshot of device `from`, taken as the engine takes it (§8, ruling T18e): the device
    /// pushes its outbox, pulls the relay in relay order, and only then dumps, so the snapshot
    /// never carries a row the relay does not have yet.
    fn snapshot_of(&mut self, from: usize) -> Vec<u8> {
        self.push(from);
        let all: Vec<usize> = (0..self.relay.len()).collect();
        let end = self.pull(from, &all);
        if end != PullEnd::Done {
            self.fail(&format!(
                "device {from} could not pull the relay before its snapshot: {end:?}"
            ));
        }
        fake_clock::advance(STEP_MS);
        let mut stream = Vec::new();
        let seq = self.relay.len() as u64;
        let dumped = snapshot::dump(&self.devices[from].conn, seq, &mut stream);
        self.check(&format!("dump d{from}"), dumped);
        stream
    }

    /// A new device joins from device `from`'s snapshot (`snapshot::join`, R4). It deals with
    /// the relay from scratch: the snapshot's heads decide what it skips. Returns its index.
    pub fn join_device(&mut self, from: usize) -> usize {
        let d = self.devices.len();
        let stream = self.snapshot_of(from);
        let conn = fresh_profile(&self.dir.path().join(format!("device-{d}.db")));
        let conn = self.check(&format!("create d{d}"), conn);
        let joined = snapshot::join(&conn, device_id(d), &mut stream.as_slice(), now());
        let report = self.check(&format!("join d{d} from d{from}"), joined);
        if !report.warnings.is_empty() {
            self.fail(&format!("join d{d} from d{from} warned: {report:?}"));
        }
        self.trace.push(format!(
            "join d{d} from d{from} ({} rows, {} seeds)",
            report.stats.rows, report.stats.seeds
        ));
        self.devices.push(Device {
            conn,
            id: device_id(d),
            processed: BTreeSet::new(),
            held: Vec::new(),
        });
        d
    }

    /// Device `d` restores by merge from a fresh snapshot of device `from`, as after a `410
    /// snapshot_required`: its outbox stays and is pushed later.
    pub fn restore_by_merge(&mut self, d: usize, from: usize) {
        let stream = self.snapshot_of(from);
        let restored =
            snapshot::restore_merge(&self.devices[d].conn, &mut stream.as_slice(), now());
        let report = self.check(&format!("restore d{d} from d{from}"), restored);
        self.trace.push(format!(
            "restore d{d} by merge from d{from} ({} rows, warnings {:?})",
            report.stats.rows, report.warnings
        ));
    }

    /// Every device dealt with every envelope, nothing is held or re-joined, nothing waits in
    /// an outbox, and every synced table is equal on every device.
    pub fn assert_converged(&self) {
        self.assert_converged_after_rejoins(&[]);
    }

    /// `assert_converged`, where the devices of `rejoined` (a clone or a rolled-back copy the
    /// test made) must have re-joined under a new ID and every other device kept its own.
    pub fn assert_converged_after_rejoins(&self, rejoined: &[usize]) {
        for (d, device) in self.devices.iter().enumerate() {
            if device.processed.len() != self.relay.len() {
                self.fail(&format!("device {d} did not deal with every envelope"));
            }
            let rejoined_as_expected = (device.id == device_id(d)) != rejoined.contains(&d);
            if !device.held.is_empty() || !rejoined_as_expected {
                self.fail(&format!(
                    "device {d} held {:?} or re-joined unexpectedly (now {:?}) on an honest relay",
                    device.held, device.id
                ));
            }
            let queued = device
                .conn
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(Into::into);
            let queued = self.check(&format!("outbox of d{d}"), queued);
            if queued != 0 {
                self.fail(&format!("device {d} still has {queued} rows queued"));
            }
        }
        let conns: Vec<&Connection> = self.devices.iter().map(|d| &d.conn).collect();
        if let Some(message) = divergence(&conns) {
            self.fail(&message);
        }
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        fake_clock::stop();
    }
}

/// A sealed envelope: the changeset, its wire bytes and their hash.
type Sealed = (Changeset, Vec<u8>, [u8; 32]);

/// Seals device `conn`'s next envelope in one transaction (`build_next`, SHA-256 of its JSON,
/// `record_sealed`, commit): the changeset, its wire bytes and hash, or `None` when nothing is
/// queued. A row too large to send is an engine error here.
fn seal_next(conn: &Connection) -> Result<Option<Sealed>> {
    let tx = conn.unchecked_transaction()?;
    let (built, refused) = build_next(&tx)?;
    if !refused.is_empty() {
        return Err(AppError::Internal(format!("rows refused: {refused:?}")));
    }
    let Some(built) = built else {
        tx.commit()?;
        return Ok(None);
    };
    let bytes = to_json(&built.changeset)?;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    record_sealed(&tx, &built, hash)?;
    tx.commit()?;
    Ok(Some((built.changeset, bytes, hash)))
}
