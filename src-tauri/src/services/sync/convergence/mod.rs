//! The convergence harness (spec §18): three in-memory devices sharing one vault, a fake relay
//! log, and random operations through the real services. Changesets travel through the wire
//! format (`to_json` / `from_json`) and are pulled in random orders, adversarial ones included
//! (one author's envelopes all first, replays, a withheld envelope offered later); every pulled
//! envelope is judged by the chain as the engine judges it (R13). At the end every synced table
//! must be equal on every device (`test_support::divergence`).

mod ops;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::chain::{self, judge, EnvelopeMeta, Verdict};
use super::changeset::{from_json, to_json, Changeset};
use super::hlc::DeviceId;
use super::journal::{build_next, record_sealed};
use super::merge::{apply_changeset, Outcome as Merged, StopReason};
use super::test_support::{divergence, migrated};
use super::{snapshot, state};
use ops::{random_op, Outcome};

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
    /// How often each verdict was handed out.
    pub verdicts: BTreeMap<String, usize>,
    /// Per operation: `[done, rejected, no target]`.
    pub stats: BTreeMap<&'static str, [usize; 3]>,
    clones: u8,
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

impl Vault {
    /// Device 0 gets `seed_ops`, then `state::enable` (capture installed, the rule-pack keys
    /// queued). Devices 1.. join from device 0's snapshot through `snapshot::join` (R4: state,
    /// restore without capture, install).
    pub fn new(n: usize, seed_ops: impl FnOnce(&Connection)) -> Self {
        let first = migrated();
        seed_ops(&first);
        state::enable(&first, device_id(0)).expect("enable the first device");
        let mut stream = Vec::new();
        snapshot::dump(&first, 0, &mut stream).expect("dump the first device");
        let mut devices = vec![Device {
            conn: first,
            id: device_id(0),
            processed: BTreeSet::new(),
            held: Vec::new(),
        }];
        for i in 1..n {
            let conn = migrated();
            let report = snapshot::join(&conn, device_id(i), &mut stream.as_slice(), now())
                .expect("join from the snapshot");
            assert!(report.warnings.is_empty(), "join warnings: {report:?}");
            devices.push(Device {
                conn,
                id: device_id(i),
                processed: BTreeSet::new(),
                held: Vec::new(),
            });
        }
        Vault {
            devices,
            relay: Vec::new(),
            trace: Vec::new(),
            label: String::new(),
            verdicts: BTreeMap::new(),
            stats: BTreeMap::new(),
            clones: 0,
        }
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

    /// One random operation on device `d`, through the real services.
    pub fn op(&mut self, d: usize, rng: &mut StdRng) {
        let step = random_op(&self.devices[d].conn, rng);
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

    /// Seals everything device `d` has queued, one envelope per transaction (`build_next`,
    /// hash, `record_sealed`, commit), and appends each to the relay as it decodes off the wire.
    /// Returns how many envelopes went out.
    pub fn push(&mut self, d: usize) -> usize {
        let mut sent = 0;
        loop {
            let conn = &self.devices[d].conn;
            let tx = conn.unchecked_transaction().expect("transaction");
            let (built, refused) = build_next(&tx).expect("build_next");
            if !refused.is_empty() {
                self.fail(&format!("device {d} refused rows: {refused:?}"));
            }
            let Some(built) = built else {
                tx.commit().expect("commit");
                break;
            };
            let bytes = to_json(&built.changeset);
            let hash: [u8; 32] = Sha256::digest(&bytes).into();
            record_sealed(&tx, &built, hash).expect("record_sealed");
            tx.commit().expect("commit");
            let wire = from_json(&bytes).expect("an envelope decodes");
            if wire != built.changeset {
                self.fail(&format!(
                    "device {d}: envelope {} changed on the wire",
                    built.changeset.seq
                ));
            }
            self.trace.push(format!(
                "push d{d} seq {} ({} rows) -> relay {}",
                wire.seq,
                wire.rows.len(),
                self.relay.len()
            ));
            self.relay.push((wire, hash));
            sent += 1;
        }
        sent
    }

    /// Offers device `d` the relay entries of `order` it has not dealt with, judging each (R13).
    pub fn pull(&mut self, d: usize, order: &[usize]) -> PullEnd {
        let end = self.pull_inner(d, order);
        self.trace.push(format!("pull d{d} {order:?} -> {end:?}"));
        end
    }

    fn pull_inner(&mut self, d: usize, order: &[usize]) -> PullEnd {
        for &i in order {
            if self.devices[d].processed.contains(&i) {
                continue;
            }
            let (cs, hash) = self.relay[i].clone();
            let meta = EnvelopeMeta::of(&cs, hash);
            loop {
                let device = &mut self.devices[d];
                let conn = &device.conn;
                let verdict = judge(conn, &meta).expect("judge");
                *self.verdicts.entry(format!("{verdict:?}")).or_default() += 1;
                match verdict {
                    Verdict::Apply => match apply_changeset(conn, &cs, hash, now()).expect("apply")
                    {
                        Merged::Applied(_) => {}
                        Merged::Stopped(reason) => return PullEnd::MergeStopped(reason),
                    },
                    Verdict::SkipReplay => {}
                    Verdict::OwnAcknowledged => {
                        chain::acknowledge_own(conn, cs.seq).expect("acknowledge");
                    }
                    Verdict::Clone => {
                        self.clones += 1;
                        let new = DeviceId([0xC0 + self.clones; 16]);
                        let device = &mut self.devices[d];
                        chain::rejoin(&device.conn, new, Some(cs.seq)).expect("rejoin");
                        device.id = new;
                        continue;
                    }
                    Verdict::Quarantine => {
                        chain::quarantine(conn, cs.device).expect("quarantine");
                        device.held.push(i);
                    }
                    Verdict::Held => device.held.push(i),
                    Verdict::BelowBarrier => {
                        chain::skip_below_barrier(conn, &meta).expect("skip below barrier");
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
    pub fn random_order(&self, d: usize, rng: &mut StdRng, partial: bool) -> Vec<usize> {
        let mut queues: Vec<VecDeque<usize>> = self
            .unprocessed_by_author(d)
            .into_values()
            .map(VecDeque::from)
            .collect();
        let mut order = Vec::new();
        while queues.iter().any(|q| !q.is_empty()) {
            let open: Vec<usize> = (0..queues.len())
                .filter(|&q| !queues[q].is_empty())
                .collect();
            let q = open[rng.random_range(0..open.len())];
            order.push(queues[q].pop_front().expect("non-empty"));
        }
        if partial && !order.is_empty() {
            order.truncate(rng.random_range(0..=order.len()));
        }
        order
    }

    /// Device `d` pulls in its own random order. Order alone may only stop a pull on a parent
    /// that arrives later (an author's envelope ahead of another author's it depends on);
    /// anything else fails the run.
    pub fn pull_random(&mut self, d: usize, rng: &mut StdRng, partial: bool) {
        let order = self.random_order(d, rng, partial);
        match self.pull(d, &order) {
            PullEnd::Done | PullEnd::MergeStopped(StopReason::MissingParent { .. }) => {}
            other => self.fail(&format!("device {d} stopped on {other:?}")),
        }
    }

    /// Everyone pushes, everyone pulls everything (a random order first, then the relay order
    /// if that stopped on a parent still to come), until nothing more is pushed.
    pub fn sync_all(&mut self, rng: &mut StdRng) {
        for _ in 0..6 {
            let pushed: usize = (0..self.devices.len()).map(|d| self.push(d)).sum();
            for d in 0..self.devices.len() {
                self.pull_random(d, rng, false);
                let all: Vec<usize> = (0..self.relay.len()).collect();
                if self.pull(d, &all) != PullEnd::Done {
                    self.fail(&format!("device {d} could not pull the relay in its order"));
                }
            }
            if pushed == 0 {
                return;
            }
        }
        self.fail("the devices still push after six exchanges");
    }

    /// Every device dealt with every envelope, nothing is held or re-joined, nothing waits in
    /// an outbox, and every synced table is equal on every device.
    pub fn assert_converged(&self) {
        for (d, device) in self.devices.iter().enumerate() {
            if device.processed.len() != self.relay.len() {
                self.fail(&format!("device {d} did not deal with every envelope"));
            }
            if !device.held.is_empty() || device.id != device_id(d) {
                self.fail(&format!(
                    "device {d} held {:?} or re-joined as {:?} on an honest relay",
                    device.held, device.id
                ));
            }
            let queued: i64 = device
                .conn
                .query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))
                .expect("outbox");
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

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;

    use rand::SeedableRng;

    use super::*;
    use crate::models::investments::InsertInvestmentTransaction;
    use crate::services::investments;
    use crate::services::sync::capture::{self, Mode};

    const SEEDS: [u64; 24] = [
        1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946,
        17711, 28657, 46368, 75025,
    ];
    const DEVICES: usize = 3;
    const ROUNDS: usize = 6;
    const ADVERSARIAL_ROUNDS: usize = 3;
    /// The operations that seed the vault, in order.
    const SEED_OPS: [&str; 30] = [
        "bank_accounts::create_account",
        "bank_accounts::create_account",
        "bank_accounts::create_transaction",
        "bank_accounts::create_transaction",
        "bank_accounts::record_balance",
        "categorization::learn::learn_scoped",
        "investments::create_investment_with_transaction",
        "investments::add_transaction_to_investment",
        "stock_monitor::follow_stock",
        "stock_monitor::set_target_price",
        "stock_tags::create_group",
        "stock_tags::create_tag",
        "stock_tags::set_investment_tags",
        "stock_splits::record_detected",
        "crypto_investments::create_crypto_with_transaction",
        "loans::create_loan",
        "loan_events::add_event",
        "real_estate::create_property",
        "other_assets::create_asset",
        "other_assets::create_asset_transaction",
        "valuations::add_valuation",
        "bonds::create_bond",
        "spending_plans::create_plan",
        "spending_plans::set_transaction_plans",
        "budgeting::upsert_goal_at",
        "cashflow_planning::create_cashflow_item",
        "cashflow_planning::create_cashflow_item",
        "milestones::set_milestone_state",
        "projection::save_settings",
        "stock_import::formats::save_format",
    ];
    /// UTC midnight of 2025-12-01: the rate history runs 200 days from here.
    const RATES_FROM: i64 = 1_764_547_200;

    /// The vault's first data, on device 0 before sync is on: one rate history (a Seed table,
    /// so every device restores the same one), a position, and one of every other entity made
    /// by the operations themselves.
    fn seed_vault(conn: &Connection) {
        for k in 0..200_i64 {
            let date = RATES_FROM + k * 86_400;
            conn.execute(
                "INSERT INTO exchange_rate_history (date, currency, rate) VALUES \
                 (?1, 'CZK', ?2), (?1, 'USD', ?3)",
                rusqlite::params![
                    date,
                    24.5 + (k % 7) as f64 * 0.013,
                    1.08 + (k % 5) as f64 * 0.0021
                ],
            )
            .expect("rate history");
        }
        conn.execute_batch(
            "INSERT OR REPLACE INTO exchange_rates (currency, rate, fetched_at) VALUES \
             ('CZK', 24.6, 1767225600), ('USD', 1.09, 1767225600)",
        )
        .expect("rates");
        let trade = InsertInvestmentTransaction {
            investment_id: None,
            tx_type: "buy".into(),
            ticker: "AAPL".into(),
            company_name: "Apple".into(),
            quantity: "10".into(),
            price_per_unit: "150.00".into(),
            currency: "USD".into(),
            transaction_date: 1_767_225_600,
            fee: None,
        };
        investments::create_investment_with_transaction(conn, "AAPL", "Apple", Some(&trade))
            .expect("seed position");
        // One of every kind of entity, so every operation finds something to act on.
        let mut rng = StdRng::seed_from_u64(0);
        for name in SEED_OPS {
            let step = ops::run_op(conn, &mut rng, name);
            assert_eq!(step.outcome, Outcome::Done, "seeding {name}: {step:?}");
        }
    }

    fn vault(label: String) -> Vault {
        let mut vault = Vault::new(DEVICES, seed_vault);
        vault.label = label;
        vault
    }

    fn random_ops(vault: &mut Vault, rng: &mut StdRng) {
        for _ in 0..rng.random_range(1..=8) {
            let d = rng.random_range(0..DEVICES);
            vault.op(d, rng);
        }
    }

    /// Runs `run` for every seed, all of them even when some fail, then fails with every failing
    /// seed's first difference and the first failure in full (its trace). Returns the passing
    /// runs.
    fn every_seed(run: impl Fn(u64) -> Vault) -> Vec<Vault> {
        let mut passed = Vec::new();
        let mut failures = Vec::new();
        for seed in SEEDS {
            match std::panic::catch_unwind(AssertUnwindSafe(|| run(seed))) {
                Ok(vault) => passed.push(vault),
                Err(panic) => failures.push(
                    panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| format!("seed {seed}: a panic without a message")),
                ),
            }
        }
        if !failures.is_empty() {
            let summary: Vec<String> = failures
                .iter()
                .map(|f| {
                    f.lines()
                        .take_while(|l| !l.starts_with("--- trace"))
                        .take(4)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect();
            panic!(
                "{} of {} seeds did not converge:\n{}\n\nThe first in full:\n{}",
                failures.len(),
                SEEDS.len(),
                summary.join("\n"),
                failures[0]
            );
        }
        passed
    }

    #[test]
    fn random_operations_converge_under_any_relay_order() {
        let runs = every_seed(|seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            let mut vault = vault(format!("seed {seed} (random orders)"));
            for round in 0..ROUNDS {
                vault.trace.push(format!("--- round {round}"));
                random_ops(&mut vault, &mut rng);
                for d in 0..DEVICES {
                    if rng.random_bool(0.6) {
                        vault.push(d);
                    }
                }
                for d in 0..DEVICES {
                    if rng.random_bool(0.6) {
                        let partial = rng.random_bool(0.3);
                        vault.pull_random(d, &mut rng, partial);
                    }
                }
            }
            vault.trace.push("--- final exchange".into());
            vault.sync_all(&mut rng);
            vault.assert_converged();
            vault
        });
        // The net only holds if every operation landed somewhere.
        let mut stats: BTreeMap<&'static str, [usize; 3]> = BTreeMap::new();
        for vault in &runs {
            for (name, counts) in &vault.stats {
                let total = stats.entry(name).or_default();
                for (sum, n) in total.iter_mut().zip(counts) {
                    *sum += n;
                }
            }
        }
        let never: Vec<_> = ops::names()
            .into_iter()
            .filter(|name| stats.get(name).is_none_or(|c| c[0] == 0))
            .collect();
        assert!(never.is_empty(), "never succeeded: {never:?}\n{stats:#?}");
    }

    /// Device `d` gets the other authors' envelopes one author at a time (`flip` reverses the
    /// author order), with one envelope withheld: the pull stops on the gap (or earlier, on a
    /// parent of the author still to come). Then, after an edit made while behind, the rest
    /// arrives in the opposite author order. Returns whether a gap was hit.
    fn adversarial_pull(vault: &mut Vault, d: usize, flip: bool, rng: &mut StdRng) -> bool {
        let own = vault.devices[d].id;
        let mut groups: Vec<Vec<usize>> = vault
            .unprocessed_by_author(d)
            .into_iter()
            .filter(|(author, _)| *author != own)
            .map(|(_, entries)| entries)
            .collect();
        if flip {
            groups.reverse();
        }
        // Withhold one envelope not applied yet that has a later one of its author (and every
        // replay of it).
        let relay = &vault.relay;
        let applied: &BTreeSet<(DeviceId, u64)> = &vault.devices[d]
            .processed
            .iter()
            .map(|&i| (relay[i].0.device, relay[i].0.seq))
            .collect();
        let candidates: Vec<usize> = groups
            .iter()
            .flat_map(|g| {
                let last_seq = g.last().map(|&i| relay[i].0.seq);
                g.iter().copied().filter(move |&i| {
                    let cs = &relay[i].0;
                    Some(cs.seq) != last_seq && !applied.contains(&(cs.device, cs.seq))
                })
            })
            .collect();
        let withheld =
            (!candidates.is_empty()).then(|| candidates[rng.random_range(0..candidates.len())]);
        let same = |i: usize, w: usize| {
            let (a, b) = (&relay[i].0, &relay[w].0);
            a.device == b.device && a.seq == b.seq
        };
        let first: Vec<usize> = groups
            .concat()
            .into_iter()
            .filter(|&i| withheld.is_none_or(|w| !same(i, w)))
            .collect();
        let end = vault.pull(d, &first);
        let gap = end == PullEnd::Stopped(Verdict::Gap);
        match (&end, withheld) {
            (PullEnd::Stopped(Verdict::Gap), Some(_)) => {}
            (PullEnd::MergeStopped(StopReason::MissingParent { .. }), _) => {}
            (PullEnd::Done, None) => {}
            _ => vault.fail(&format!(
                "device {d}: with {withheld:?} withheld the pull ended {end:?}"
            )),
        }
        vault.op(d, rng);
        groups.reverse();
        let rest = groups.concat();
        match vault.pull(d, &rest) {
            PullEnd::Done | PullEnd::MergeStopped(StopReason::MissingParent { .. }) => {}
            other => vault.fail(&format!("device {d}: the resumed pull ended {other:?}")),
        }
        gap
    }

    #[test]
    fn adversarial_orders_converge() {
        let runs = every_seed(|seed| {
            let mut rng = StdRng::seed_from_u64(seed ^ 0x5EED_AD5E);
            let mut vault = vault(format!("seed {seed} (adversarial orders)"));
            for round in 0..ADVERSARIAL_ROUNDS {
                vault.trace.push(format!("--- round {round}"));
                // Several envelopes per author, so one can be withheld ahead of another.
                for _ in 0..3 {
                    random_ops(&mut vault, &mut rng);
                    for d in 0..DEVICES {
                        vault.push(d);
                    }
                }
                // The relay delivers two envelopes twice.
                for _ in 0..2 {
                    if !vault.relay.is_empty() {
                        let i = rng.random_range(0..vault.relay.len());
                        let copy = vault.relay[i].clone();
                        vault
                            .trace
                            .push(format!("replay relay {i} -> relay {}", vault.relay.len()));
                        vault.relay.push(copy);
                    }
                }
                for d in 0..DEVICES {
                    let flip = (d + round) % 2 == 1;
                    if adversarial_pull(&mut vault, d, flip, &mut rng) {
                        *vault.verdicts.entry("withheld gap".into()).or_default() += 1;
                    }
                }
            }
            vault.trace.push("--- final exchange".into());
            vault.sync_all(&mut rng);
            vault.assert_converged();
            vault
        });
        let count = |what: &str| -> usize {
            runs.iter()
                .map(|v| v.verdicts.get(what).copied().unwrap_or(0))
                .sum()
        };
        assert!(
            count("withheld gap") > 0,
            "no pull ever stopped on a withheld envelope"
        );
        assert!(count("SkipReplay") > 0, "no replay was ever judged");
    }

    #[test]
    #[should_panic(expected = "synced rows of device 0 and device 1 differ: table bank_accounts")]
    fn the_harness_detects_a_divergence() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut vault = vault("divergence sanity".into());
        random_ops(&mut vault, &mut rng);
        vault.sync_all(&mut rng);
        let conns: Vec<&Connection> = vault.devices.iter().map(|d| &d.conn).collect();
        assert_eq!(
            divergence(&conns),
            None,
            "the vault converged before the tamper"
        );
        let tampered = &vault.devices[1].conn;
        capture::with_mode(tampered, Mode::Repair, || {
            tampered.execute("UPDATE bank_accounts SET name = 'Tampered'", [])?;
            Ok(())
        })
        .expect("tamper");
        vault.assert_converged();
    }
}
