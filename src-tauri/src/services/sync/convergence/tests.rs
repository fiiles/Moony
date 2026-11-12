//! The property tests over the 24 fixed seeds (`CONVERGENCE_SEEDS=a..b` runs another range, for
//! exploration), the replay test, and the sanity tests that prove the comparison catches a
//! divergence.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Once;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rusqlite::Connection;

use super::*;
use crate::models::categorization::InsertScopedLearning;
use crate::models::investments::InsertInvestmentTransaction;
use crate::services::categorization::{engine::CategorizationEngine, learn};
use crate::services::investments;
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::test_support::{clock_dump_live, dump};

const SEEDS: [u64; 24] = [
    1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946,
    17711, 28657, 46368, 75025,
];
const DEVICES: usize = 3;
const ROUNDS: usize = 6;
const ADVERSARIAL_ROUNDS: usize = 3;
/// UTC midnight of 2025-12-01: the rate history runs 200 days from here.
const RATES_FROM: i64 = 1_764_547_200;
/// The operations that seed the vault, in order: one of every kind of entity, so every
/// operation finds something to act on.
const SEED_OPS: [&str; 39] = [
    "categories::create_category",
    "categories::create_category",
    "bank_accounts::create_account",
    "bank_accounts::create_account",
    "bank_accounts::create_transaction",
    "bank_accounts::create_transaction",
    "bank_accounts::record_balance",
    "csv_import::import_transactions",
    "csv_import::learned::save_format",
    "categorization::learn::learn_scoped",
    "auth::apply_profile_update",
    "investments::create_investment_with_transaction",
    "investments::add_transaction_to_investment",
    "investments::bulk_create_stock_transactions",
    "stock_dividends::create",
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
    "real_estate::create_one_time_cost",
    "insurance::create_policy",
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

/// The two rngs of a run: the schedule (operation kinds, devices, pushes, pulls, orders) and the
/// targets and arguments of the operations.
pub(super) struct Rngs {
    pub(super) schedule: StdRng,
    pub(super) target: StdRng,
}

impl Rngs {
    pub(super) fn new(seed: u64) -> Self {
        Rngs {
            schedule: StdRng::seed_from_u64(seed),
            target: StdRng::seed_from_u64(seed ^ 0x007A_86E7_5EED),
        }
    }
}

/// The vault's first data, on device 0 before sync is on: one rate history (a Seed table, so
/// every device restores the same one), the profile row the first setup writes, a position, and
/// one of every other entity made by the operations themselves.
fn seed_vault(conn: &mut Connection, dir: &Path) {
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
        "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES \
         ('CZK', 24.6, 1767225600), ('USD', 1.09, 1767225600) \
         ON CONFLICT(currency) DO UPDATE SET rate = excluded.rate, \
           fetched_at = excluded.fetched_at; \
         INSERT INTO user_profile (name, surname, email, menu_preferences, currency, language, \
           exclude_personal_real_estate) VALUES ('Alex', 'Demo', 'alex@example.com', NULL, 'CZK', \
           'en', 0);",
    )
    .expect("rates and profile");
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
    let mut rng = StdRng::seed_from_u64(11);
    for name in SEED_OPS {
        let step = ops::run_op(conn, &mut rng, dir, name)
            .unwrap_or_else(|e| panic!("seeding {name}: engine error: {e}"));
        assert_eq!(step.outcome, Outcome::Done, "seeding {name}: {step:?}");
    }
}

fn vault(label: String) -> Vault {
    Vault::new(label, DEVICES, seed_vault)
}

/// One to eight operations on random devices.
fn random_ops(vault: &mut Vault, rngs: &mut Rngs) {
    for _ in 0..rngs.schedule.random_range(1..=8) {
        let d = rngs.schedule.random_range(0..vault.devices.len());
        vault.op(d, &mut rngs.schedule, &mut rngs.target);
    }
}

/// A round of the random-order test: operations, random devices push, random devices pull in
/// their own random order (some only part of what waits).
fn random_round(vault: &mut Vault, rngs: &mut Rngs, round: usize) {
    vault.trace.push(format!("--- round {round}"));
    random_ops(vault, rngs);
    for d in 0..vault.devices.len() {
        if rngs.schedule.random_bool(0.6) {
            vault.push(d);
        }
    }
    for d in 0..vault.devices.len() {
        if rngs.schedule.random_bool(0.6) {
            let partial = rngs.schedule.random_bool(0.3);
            vault.pull_random(d, &mut rngs.schedule, partial);
        }
    }
}

fn finish(vault: &mut Vault, rngs: &mut Rngs) {
    vault.trace.push("--- final exchange".into());
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();
}

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}
static QUIET_HOOK: Once = Once::new();

/// Runs `work` with the panic hook silent on this thread (the default hook again afterwards),
/// so the panics `every_seed` catches do not flood the output.
fn quietly<T>(work: impl FnOnce() -> T) -> T {
    QUIET_HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !QUIET.with(Cell::get) {
                default(info);
            }
        }));
    });
    QUIET.with(|quiet| quiet.set(true));
    let out = work();
    QUIET.with(|quiet| quiet.set(false));
    out
}

/// The seeds to run: the fixed 24, or `CONVERGENCE_SEEDS` (exploration): a range `a..b` or a
/// list `a,b,c`.
fn seeds() -> Vec<u64> {
    let Ok(spec) = std::env::var("CONVERGENCE_SEEDS") else {
        return SEEDS.to_vec();
    };
    let invalid = || -> ! { panic!("CONVERGENCE_SEEDS must be a..b or a,b,c, not {spec:?}") };
    match spec.split_once("..") {
        Some((from, to)) => match (from.parse::<u64>(), to.parse::<u64>()) {
            (Ok(from), Ok(to)) => (from..to).collect(),
            _ => invalid(),
        },
        None => spec
            .split(',')
            .map(|seed| seed.trim().parse::<u64>().unwrap_or_else(|_| invalid()))
            .collect(),
    }
}

/// Runs `run` for every seed, all of them even when some fail, then fails with every failing
/// seed's first difference and the first failure in full (its trace). Returns the passing
/// runs.
fn every_seed(run: impl Fn(u64) -> Vault) -> Vec<Vault> {
    let seeds = seeds();
    let mut passed = Vec::new();
    let mut failures = Vec::new();
    quietly(|| {
        for &seed in &seeds {
            match panic::catch_unwind(AssertUnwindSafe(|| run(seed))) {
                Ok(vault) => passed.push(vault),
                Err(panic) => {
                    let message = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "a panic without a message".into());
                    failures.push(format!("seed {seed}: {message}"));
                }
            }
        }
    });
    if !failures.is_empty() {
        let summary: Vec<String> = failures
            .iter()
            .map(|f| {
                f.lines()
                    .take_while(|l| !l.starts_with("--- trace"))
                    .take(5)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect();
        panic!(
            "{} of {} seeds did not converge:\n{}\n\nThe first in full:\n{}",
            failures.len(),
            seeds.len(),
            summary.join("\n"),
            failures[0]
        );
    }
    passed
}

fn random_run(seed: u64) -> Vault {
    let mut rngs = Rngs::new(seed);
    let mut vault = vault(format!("random orders, seed {seed}"));
    for round in 0..ROUNDS {
        random_round(&mut vault, &mut rngs, round);
    }
    finish(&mut vault, &mut rngs);
    vault
}

/// The fewest seeds over which the operation-coverage net is asserted, besides the fixed set: a
/// short exploration list (16 seeds) cannot reach every operation, while 100 consecutive seeds
/// do (the 600-seed explorations reach all of them).
const COVERAGE_MIN_SEEDS: usize = 100;

#[test]
fn random_operations_converge_under_any_relay_order() {
    let runs = every_seed(random_run);
    // The net only holds if every operation landed somewhere: asserted over the fixed seeds and
    // over explorations of at least `COVERAGE_MIN_SEEDS` seeds.
    let seeds = seeds();
    if seeds != SEEDS && seeds.len() < COVERAGE_MIN_SEEDS {
        return;
    }
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

/// Every author's head on `conn` (`sync_heads`): the newest sequence applied or covered.
fn heads_of(conn: &Connection) -> BTreeMap<DeviceId, u64> {
    let mut stmt = conn
        .prepare("SELECT device, seq FROM sync_heads")
        .expect("prepare heads");
    stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))
        .expect("query heads")
        .map(|r| {
            let (device, seq) = r.expect("head");
            (
                DeviceId(device.try_into().expect("16-byte device")),
                seq as u64,
            )
        })
        .collect()
}

/// Device `d` gets the other authors' envelopes one author at a time (`flip` reverses the
/// author order), with one envelope withheld: the pull stops on the gap (or earlier, on a
/// parent of the author still to come). Then, after an edit made while behind, the rest
/// arrives in the opposite author order. Returns whether a gap was hit.
fn adversarial_pull(vault: &mut Vault, d: usize, flip: bool, rngs: &mut Rngs) -> bool {
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
    // replay of it). An envelope at or below the device's head of its author (a joiner's
    // snapshot covers it) is only a replay there, so withholding it opens no gap.
    let relay = &vault.relay;
    let applied: &BTreeSet<(DeviceId, u64)> = &vault.devices[d]
        .processed
        .iter()
        .map(|&i| (relay[i].0.device, relay[i].0.seq))
        .collect();
    let heads = &heads_of(&vault.devices[d].conn);
    let candidates: Vec<usize> = groups
        .iter()
        .flat_map(|g| {
            let last_seq = g.last().map(|&i| relay[i].0.seq);
            g.iter().copied().filter(move |&i| {
                let cs = &relay[i].0;
                Some(cs.seq) != last_seq
                    && !applied.contains(&(cs.device, cs.seq))
                    && heads.get(&cs.device).is_none_or(|&head| cs.seq > head)
            })
        })
        .collect();
    let withheld = (!candidates.is_empty())
        .then(|| candidates[rngs.schedule.random_range(0..candidates.len())]);
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
    // The pull holds the withheld author's later envelopes and goes on (audit M3); a change
    // whose parent is still to come is held too, and so are its author's later ones.
    let gap = withheld.is_some() && matches!(end, PullEnd::Waiting { gaps: 1.., .. });
    match (&end, withheld) {
        (PullEnd::Waiting { gaps: 1.., .. }, Some(_)) => {}
        (PullEnd::Waiting { .. }, None) => {}
        (PullEnd::Done, None) => {}
        _ => vault.fail(&format!(
            "device {d}: with {withheld:?} withheld the pull ended {end:?}"
        )),
    }
    vault.op(d, &mut rngs.schedule, &mut rngs.target);
    groups.reverse();
    let rest = groups.concat();
    match vault.pull(d, &rest) {
        PullEnd::Done | PullEnd::Waiting { .. } => {}
        other => vault.fail(&format!("device {d}: the resumed pull ended {other:?}")),
    }
    gap
}

#[test]
fn adversarial_orders_converge() {
    let runs = every_seed(|seed| {
        let mut rngs = Rngs::new(seed ^ 0x5EED_AD5E);
        let mut vault = vault(format!("adversarial orders, seed {seed}"));
        for round in 0..ADVERSARIAL_ROUNDS {
            vault.trace.push(format!("--- round {round}"));
            // Several envelopes per author, so one can be withheld ahead of another.
            for _ in 0..3 {
                random_ops(&mut vault, &mut rngs);
                for d in 0..DEVICES {
                    vault.push(d);
                }
            }
            // The relay delivers two envelopes twice.
            for _ in 0..2 {
                if !vault.relay.is_empty() {
                    let i = rngs.schedule.random_range(0..vault.relay.len());
                    let copy = vault.relay[i].clone();
                    vault
                        .trace
                        .push(format!("replay relay {i} -> relay {}", vault.relay.len()));
                    vault.relay.push(copy);
                }
            }
            for d in 0..DEVICES {
                let flip = (d + round) % 2 == 1;
                if adversarial_pull(&mut vault, d, flip, &mut rngs) {
                    *vault.verdicts.entry("withheld gap".into()).or_default() += 1;
                }
            }
        }
        finish(&mut vault, &mut rngs);
        vault
    });
    let count = |what: &str| -> usize {
        runs.iter()
            .map(|v| v.verdicts.get(what).copied().unwrap_or(0))
            .sum()
    };
    assert!(
        count("withheld gap") > 0,
        "no pull ever held a gap behind a withheld envelope"
    );
    assert!(count("SkipReplay") > 0, "no replay was ever judged");
}

#[test]
fn devices_joining_and_restoring_mid_run_converge() {
    every_seed(|seed| {
        let mut rngs = Rngs::new(seed ^ 0x10_1A57);
        let mut vault = vault(format!("join and restore mid-run, seed {seed}"));
        for round in 0..ROUNDS {
            if round == 2 {
                // A fourth device joins from whichever device it pairs with (which pushes and
                // pulls first, ruling T18e), and then works like the others.
                let from = rngs.schedule.random_range(0..vault.devices.len());
                vault.join_device(from);
            }
            if round == 4 {
                // A device that was told `410 snapshot_required` restores by merge from another
                // device's fresh snapshot, its own outbox intact.
                let n = vault.devices.len();
                let d = rngs.schedule.random_range(0..n);
                let from = (d + rngs.schedule.random_range(1..n)) % n;
                vault.restore_by_merge(d, from);
            }
            random_round(&mut vault, &mut rngs, round);
        }
        finish(&mut vault, &mut rngs);
        vault
    });
}

/// `text` with every UUID-shaped token as `<id>` and every ten-digit number inside `window`
/// (the services' own wall-clock stamps) as `<now>`: what a seed's run must repeat exactly.
fn normalized(text: &str, window: (i64, i64)) -> String {
    let bytes = text.as_bytes();
    let is_uuid = |at: usize| -> bool {
        bytes.len() >= at + 36
            && bytes[at..at + 36].iter().enumerate().all(|(k, b)| {
                if [8, 13, 18, 23].contains(&k) {
                    *b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            })
            && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
    };
    let mut out = String::new();
    let mut at = 0;
    while at < bytes.len() {
        if is_uuid(at) {
            out.push_str("<id>");
            at += 36;
            continue;
        }
        if bytes[at].is_ascii_digit() && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric()) {
            let end = (at..bytes.len())
                .find(|&k| !bytes[k].is_ascii_digit())
                .unwrap_or(bytes.len());
            let digits = &text[at..end];
            match digits.parse::<i64>() {
                Ok(n) if end - at == 10 && (window.0..=window.1).contains(&n) => {
                    out.push_str("<now>")
                }
                _ => out.push_str(digits),
            }
            at = end;
            continue;
        }
        let ch = text[at..].chars().next().unwrap_or_default();
        out.push(ch);
        at += ch.len_utf8();
    }
    out
}

/// Columns the services stamp with the wall clock themselves.
const WALL_CLOCK_STAMPS: [&str; 6] = [
    "created_at",
    "updated_at",
    "recorded_at",
    "imported_at",
    "detected_at",
    "resolved_at",
];

/// A run's trace and every device's synced rows and live clocks (to the millisecond), normalized
/// and sorted.
fn fingerprint(vault: &Vault, window: (i64, i64)) -> (Vec<String>, Vec<Vec<String>>) {
    let trace = vault
        .trace
        .iter()
        .map(|line| normalized(line, window))
        .collect();
    let devices = vault
        .devices
        .iter()
        .map(|device| {
            let mut lines: Vec<String> = dump(&device.conn)
                .into_iter()
                .flat_map(|(table, rows)| rows.into_iter().map(move |row| format!("{table} {row}")))
                .chain(
                    clock_dump_live(&device.conn)
                        .into_iter()
                        // Whether a stamp column changed (and got its own clock) depends on
                        // the services' wall-clock second; the counter within a step's
                        // millisecond may differ by it too.
                        .filter(|(_, _, col, _, _)| !WALL_CLOCK_STAMPS.contains(&col.as_str()))
                        .map(|(tbl, pk, col, hlc, device)| {
                            format!("clock {tbl} {pk} {col} {} {device:?}", hlc >> 16)
                        }),
                )
                .map(|line| normalized(&line, window))
                .collect();
            lines.sort();
            lines
        })
        .collect();
    (trace, devices)
}

#[test]
fn a_seed_replays_its_operations_and_outcomes() {
    let started = now() - 5;
    let (first, second) = (random_run(21), random_run(21));
    let window = (started, now() + 5);
    let (trace_a, devices_a) = fingerprint(&first, window);
    let (trace_b, devices_b) = fingerprint(&second, window);
    if let Some(k) =
        (0..trace_a.len().max(trace_b.len())).find(|&k| trace_a.get(k) != trace_b.get(k))
    {
        panic!(
            "the traces part at step {k}:\n{:?}\n{:?}",
            trace_a.get(k),
            trace_b.get(k)
        );
    }
    for (d, (a, b)) in devices_a.iter().zip(&devices_b).enumerate() {
        let first_difference = a.iter().zip(b).find(|(x, y)| x != y);
        assert!(
            a == b,
            "device {d} ends differently: {first_difference:?} ({} vs {} lines)",
            a.len(),
            b.len()
        );
    }
}

/// A vault of three devices that exchanged everything, after `setup` on device 0.
pub(super) fn synced_vault(label: &str, setup: impl FnOnce(&mut Vault, &mut Rngs)) -> Vault {
    let mut rngs = Rngs::new(1);
    let mut vault = vault(label.into());
    setup(&mut vault, &mut rngs);
    vault.sync_all(&mut rngs.schedule);
    let conns: Vec<&Connection> = vault.devices.iter().map(|d| &d.conn).collect();
    assert_eq!(
        divergence(&conns),
        None,
        "the vault converged before the tamper"
    );
    vault
}

/// Runs `sql` on device 1 with capture in repair mode (nothing is clocked or queued) and checks
/// that it changed exactly one row.
fn tamper(vault: &Vault, sql: &str) {
    let conn = &vault.devices[1].conn;
    let changed =
        capture::with_mode(conn, Mode::Repair, || Ok(conn.execute(sql, [])?)).expect("tamper");
    assert_eq!(changed, 1, "{sql}");
}

#[test]
#[should_panic(expected = "synced rows of device 0 and device 1 differ: table bank_accounts")]
fn the_harness_detects_a_changed_row() {
    let vault = synced_vault("changed row", |_, _| {});
    tamper(
        &vault,
        "UPDATE bank_accounts SET name = 'Tampered' WHERE id = (SELECT MIN(id) FROM bank_accounts)",
    );
    vault.assert_converged();
}

#[test]
#[should_panic(expected = "live clocks of device 0 and device 1 differ")]
fn the_harness_detects_a_changed_live_clock() {
    let vault = synced_vault("changed clock", |_, _| {});
    tamper(
        &vault,
        "UPDATE sync_clock SET hlc = hlc + 1 \
         WHERE tbl = 'app_config' AND pk = 'rule_packs_enabled' AND col = '*'",
    );
    vault.assert_converged();
}

#[test]
#[should_panic(expected = "tombstones of device 0 and device 1")]
fn the_harness_detects_a_removed_tombstone() {
    let vault = synced_vault("removed tombstone", |vault, rngs| {
        vault.run_op(0, "bank_accounts::delete_balance_record", &mut rngs.target);
    });
    tamper(
        &vault,
        "DELETE FROM sync_clock WHERE col = '-' \
         AND (tbl, pk) = (SELECT tbl, pk FROM sync_clock WHERE col = '-' ORDER BY tbl, pk LIMIT 1)",
    );
    vault.assert_converged();
}

/// T18e: a snapshot must not hand out rows its source has not pushed. Device 0 creates an
/// account and keeps it unpushed; device 1 restores by merge from device 0 and learns a payee on
/// that account; both push. Every device must still apply the relay in its own order: the dump
/// path pushes the source first, so the account is on the relay before the payee.
#[test]
fn a_snapshot_never_hands_out_rows_its_source_has_not_pushed() {
    let mut rngs = Rngs::new(1);
    let mut vault = synced_vault("T18e unpushed rows in a snapshot", |_, _| {});
    let accounts = |vault: &Vault, d: usize| -> BTreeSet<String> {
        let conn = &vault.devices[d].conn;
        let mut stmt = conn
            .prepare("SELECT id FROM bank_accounts")
            .expect("prepare");
        stmt.query_map([], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    };
    let before = accounts(&vault, 0);
    vault.run_op(0, "bank_accounts::create_account", &mut rngs.target);
    let account = accounts(&vault, 0)
        .difference(&before)
        .next()
        .cloned()
        .expect("a new account");

    vault.restore_by_merge(1, 0);
    let category: String = vault.devices[1]
        .conn
        .query_row(
            "SELECT id FROM transaction_categories ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .expect("a category");
    let learning = InsertScopedLearning {
        bank_account_id: account,
        is_credit: false,
        payee: Some("Albert".into()),
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        category_id: category,
    };
    learn::learn_scoped(
        &vault.devices[1].conn,
        &CategorizationEngine::new_empty(),
        &learning,
    )
    .expect("learn");
    vault.push(1);
    vault.push(0);

    let all: Vec<usize> = (0..vault.relay.len()).collect();
    let end = vault.pull(2, &all);
    assert_eq!(end, PullEnd::Done, "{}", vault.trace.join("\n"));
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();
}
