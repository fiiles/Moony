//! Named convergence scenarios (spec §18): one test per scenario the spec and its reviews raise
//! that no other test covers through the real services and the relay (the map from every
//! scenario to its test is in the Task 19 report). Each drives the services on specific devices
//! of a `Vault`, exchanges in both relevant orders where order matters, and ends with
//! `assert_converged`. Plus the `#[ignore]` measurement of the capture overhead (§18, R19).

use std::path::Path;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::SeedableRng;
use rusqlite::Connection;

use super::{PullEnd, Vault, STEP_MS};
use crate::models::categorization::InsertScopedLearning;
use crate::models::investments::InsertInvestmentTransaction;
use crate::models::spending_plans::InsertSpendingPlan;
use crate::models::stock_monitor::InsertWatchedStock;
use crate::models::{InsertBankTransaction, InsertTransactionCategory, UpdateTransactionCategory};
use crate::services::categorization::{engine::CategorizationEngine, learn, packs};
use crate::services::stock_splits::{self, DetectedSplit, StockSplitSource};
use crate::services::sync::chain::{judge, EnvelopeMeta, Verdict};
use crate::services::sync::hlc::{fake_clock, DeviceId};
use crate::services::sync::natural_id::natural_id;
use crate::services::sync::registry::keys::barrier_number;
use crate::services::sync::test_support::migrated;
use crate::services::sync::{self as sync, capture, state};
use crate::services::{
    bank_accounts, categories, csv_import, investments, spending_plans, stock_monitor,
};

/// Four devices: the two that act, and two more that get their changes in opposite orders.
const DEVICES: usize = 4;
const DAY: i64 = 86_400;
/// 2026-01-01.
const JAN_1: i64 = 1_767_225_600;
/// 2026-02-02: the seeded AAPL trade.
const TRADE_DAY: i64 = JAN_1 + 32 * DAY;
/// 2026-03-01: a buy entered offline, still before the split.
const BUY_DAY: i64 = JAN_1 + 59 * DAY;
/// 2026-06-01: the ex-date of the 4:1 split.
const SPLIT_DAY: i64 = JAN_1 + 151 * DAY;

/// Moves the shared fake wall clock on, so the next step's clocks are newer than the last's on
/// every device.
fn step() {
    fake_clock::advance(STEP_MS);
}

fn rng() -> StdRng {
    StdRng::seed_from_u64(19)
}

/// Every row of `sql`'s first column, NULL as `-`.
fn texts(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect(sql);
    stmt.query_map([], |r| r.get::<_, Option<String>>(0))
        .expect(sql)
        .map(|r| r.expect("row").unwrap_or_else(|| "-".into()))
        .collect()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

/// The vault's first data, on device 0 before sync is on: a profile, account `acc`, categories
/// `k` and `t`, payment `q` in `k`, uncategorised payment `p`, and rule `r1` filing into `k`.
fn base_seed(conn: &mut Connection, _dir: &Path) {
    conn.execute_batch(
        "INSERT INTO user_profile (name, surname, email, currency, language) \
           VALUES ('Alex', 'Demo', 'alex@example.com', 'CZK', 'en'); \
         INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('acc', 'Main', 'CZK', '0'); \
         INSERT INTO transaction_categories (id, name) VALUES ('k', 'Kids'), ('t', 'Family'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, \
           booking_date, category_id) VALUES \
           ('q', 'acc', 'debit', '-10.00', 'CZK', 1767225600, 'k'), \
           ('p', 'acc', 'debit', '-20.00', 'CZK', 1767225600, NULL); \
         INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) \
           VALUES ('r1', 'Toys', 'contains', 'toys', 'k');",
    )
    .expect("base seed");
}

fn trade(quantity: &str, price: &str, day: i64) -> InsertInvestmentTransaction {
    InsertInvestmentTransaction {
        investment_id: None,
        tx_type: "buy".into(),
        ticker: "AAPL".into(),
        company_name: "Apple".into(),
        quantity: quantity.into(),
        price_per_unit: price.into(),
        currency: "USD".into(),
        transaction_date: day,
        fee: None,
    }
}

/// `base_seed`, an AAPL position with one trade (7 at 100 on `TRADE_DAY`), and with `detected`
/// a pending 4:1 split on `SPLIT_DAY`.
fn stock_seed(conn: &mut Connection, dir: &Path, detected: bool) {
    base_seed(conn, dir);
    investments::create_investment_with_transaction(
        conn,
        "AAPL",
        "Apple",
        Some(&trade("7", "100", TRADE_DAY)),
    )
    .expect("seed position");
    if detected {
        detect(conn);
    }
}

fn detect(conn: &Connection) {
    let split = DetectedSplit {
        effective_day: SPLIT_DAY,
        numerator: 4,
        denominator: 1,
    };
    stock_splits::record_detected(conn, "AAPL", &[split], StockSplitSource::Yahoo).expect("detect");
}

fn split_id() -> String {
    natural_id("stock_splits", &["AAPL", &SPLIT_DAY.to_string()])
}

fn position_id() -> String {
    natural_id("stock_investments", &["AAPL"])
}

/// `"quantity price | entry_quantity entry_price entry_basis | split_basis"` of every AAPL
/// trade, by trade day.
fn trades(conn: &Connection) -> Vec<String> {
    texts(
        conn,
        "SELECT quantity || ' ' || price_per_unit || ' | ' || entry_quantity || ' ' \
           || entry_price_per_unit || ' ' || COALESCE(entry_basis, '-') || ' | ' \
           || COALESCE(split_basis, '-') \
         FROM investment_transactions WHERE ticker = 'AAPL' ORDER BY transaction_date",
    )
}

fn split_status(conn: &Connection) -> Vec<String> {
    texts(conn, "SELECT status FROM stock_splits")
}

/// Devices `x` and `y` push what they did offline; `x` gets `y`'s envelopes and `y` gets `x`'s,
/// and of the other devices every other one gets `x`'s first and the rest `y`'s first. Every
/// pull must apply in full; then everyone syncs what is left (own acknowledgements, repairs).
fn exchange(vault: &mut Vault, x: usize, y: usize) {
    let from = vault.relay.len();
    vault.push(x);
    let mid = vault.relay.len();
    vault.push(y);
    let xs: Vec<usize> = (from..mid).collect();
    let ys: Vec<usize> = (mid..vault.relay.len()).collect();
    assert!(
        !xs.is_empty() && !ys.is_empty(),
        "both devices changed something"
    );
    let mut x_first = false;
    for d in 0..vault.devices.len() {
        let order = if d == x {
            ys.clone()
        } else if d == y {
            xs.clone()
        } else {
            x_first = !x_first;
            if x_first {
                [xs.clone(), ys.clone()].concat()
            } else {
                [ys.clone(), xs.clone()].concat()
            }
        };
        let end = vault.pull(d, &order);
        if end != PullEnd::Done {
            vault.fail(&format!("device {d} stopped on {end:?}"));
        }
    }
    vault.sync_all(&mut rng());
}

/// Everything on the relay that device `d` has not dealt with, in relay order; it must apply.
fn pull_all(vault: &mut Vault, d: usize) {
    let all: Vec<usize> = (0..vault.relay.len()).collect();
    let end = vault.pull(d, &all);
    if end != PullEnd::Done {
        vault.fail(&format!("device {d} stopped on {end:?}"));
    }
}

fn verdicts(vault: &Vault, verdict: &str) -> usize {
    vault.verdicts.get(verdict).copied().unwrap_or(0)
}

fn category(conn: &Connection, name: &str) -> String {
    categories::create_category(
        conn,
        &InsertTransactionCategory {
            kind: None,
            name: name.into(),
            icon: Some("tag".into()),
            color: Some("#336699".into()),
            parent_id: None,
            sort_order: None,
        },
    )
    .expect("create category")
    .id
}

fn category_names(conn: &Connection) -> Vec<String> {
    texts(
        conn,
        "SELECT name FROM transaction_categories WHERE is_system = 0 ORDER BY name",
    )
}

/// The brief's MSFT case of R2 (the hand-made-row version is
/// `merge::tests::basics::concurrent_creation_of_same_natural_id_merges_per_column`): both
/// devices follow MSFT offline, then A sets a target and B writes notes. The two creations
/// merge per column: one row with A's target and B's notes.
#[test]
fn msft_followed_on_two_devices_keeps_a_target_and_b_notes() {
    let mut vault = Vault::new("MSFT followed on two devices", DEVICES, base_seed);
    let follow = |conn: &Connection| {
        let ticker = InsertWatchedStock {
            ticker: "MSFT".into(),
        };
        stock_monitor::follow_stock(conn, &ticker).expect("follow");
    };
    step();
    follow(&vault.devices[0].conn);
    step();
    follow(&vault.devices[1].conn);
    step();
    stock_monitor::set_target_price(
        &vault.devices[0].conn,
        "MSFT",
        Some("420".into()),
        Some("above".into()),
    )
    .expect("target on A");
    step();
    stock_monitor::update_notes(&vault.devices[1].conn, "MSFT", "Cloud margins".into())
        .expect("notes on B");

    exchange(&mut vault, 0, 1);

    vault.assert_converged();
    let id = natural_id("watched_stocks", &["MSFT"]);
    for device in &vault.devices {
        assert_eq!(
            texts(
                &device.conn,
                "SELECT id || ' ' || target_price || ' ' || entry_target_price || ' ' \
                   || target_direction || ' ' || notes FROM watched_stocks"
            ),
            vec![format!("{id} 420 420 above Cloud margins")]
        );
    }
}

/// D14 orphan rules through the services: A deletes category `k` reassigning to `t`; B,
/// offline, files payment `p` into `k` and learns a payee for `k`. Every device ends with `p`
/// uncategorised (nullable, cleared), the learned payee dropped (required), and A's reassigned
/// rows (`q`, `r1`) on `t`.
#[test]
fn orphan_rule_category_deleted_while_categorising() {
    let mut vault = Vault::new("category deleted while categorising", DEVICES, base_seed);
    step();
    categories::delete_category(&vault.devices[0].conn, "k", Some("t")).expect("delete on A");
    step();
    let b = &vault.devices[1].conn;
    bank_accounts::set_transaction_category(b, "p", Some("k"), None).expect("categorise on B");
    let learning = InsertScopedLearning {
        bank_account_id: "acc".into(),
        is_credit: false,
        payee: Some("Albert".into()),
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        category_id: "k".into(),
    };
    learn::learn_scoped(b, &CategorizationEngine::new_empty(), &learning).expect("learn on B");
    assert_eq!(count(b, "SELECT COUNT(*) FROM learned_payees"), 1);

    exchange(&mut vault, 0, 1);

    vault.assert_converged();
    for device in &vault.devices {
        let conn = &device.conn;
        assert_eq!(
            texts(
                conn,
                "SELECT id FROM transaction_categories WHERE id IN ('k', 't')"
            ),
            vec!["t"]
        );
        assert_eq!(
            texts(
                conn,
                "SELECT id || ' ' || COALESCE(category_id, '-') FROM bank_transactions ORDER BY id"
            ),
            vec!["p -", "q t"]
        );
        assert_eq!(
            texts(
                conn,
                "SELECT category_id FROM categorization_rules WHERE id = 'r1'"
            ),
            vec!["t"]
        );
        assert_eq!(count(conn, "SELECT COUNT(*) FROM learned_payees"), 0);
        assert_eq!(
            count(conn, "SELECT COUNT(*) FROM pragma_foreign_key_check"),
            0
        );
    }
}

fn plan_data(name: &str) -> InsertSpendingPlan {
    InsertSpendingPlan {
        name: name.into(),
        year: 2026,
        amount: "1000.00".into(),
        currency: "CZK".into(),
        icon: "plane".into(),
        color: "#336699".into(),
        note: None,
    }
}

/// `"id name"` of every row of `table` (spending plans or custom categories), by id.
fn names_by_id(conn: &Connection, table: &str) -> Vec<String> {
    let filter = if table == "transaction_categories" {
        "WHERE is_system = 0 AND id NOT IN ('k', 't')"
    } else {
        ""
    };
    texts(
        conn,
        &format!("SELECT id || ' ' || name FROM {table} {filter} ORDER BY id"),
    )
}

/// Renames (§7.3, R9, U3/U4) for the two scopes the tag tests leave out: A and B each create a
/// spending plan "Holiday" (2026) and a category "Pets" offline. The lower id keeps the name
/// and the other shows "(2)" on every device, in both orders. A later rename of the holder on a
/// third device frees the name, and every device shows the typed name again.
#[test]
fn rename_spending_plans_and_categories() {
    let mut vault = Vault::new("rename plans and categories", DEVICES, base_seed);
    step();
    let a = &vault.devices[0].conn;
    let plan_a = spending_plans::create_plan(a, &plan_data("Holiday"))
        .expect("plan on A")
        .id;
    let pets_a = category(a, "Pets");
    step();
    let b = &vault.devices[1].conn;
    let plan_b = spending_plans::create_plan(b, &plan_data("Holiday"))
        .expect("plan on B")
        .id;
    let pets_b = category(b, "Pets");

    exchange(&mut vault, 0, 1);

    vault.assert_converged();
    let ordered = |x: String, y: String| if x < y { (x, y) } else { (y, x) };
    let (plan_holder, plan_renamed) = ordered(plan_a, plan_b);
    let (pets_holder, pets_renamed) = ordered(pets_a, pets_b);
    for device in &vault.devices {
        assert_eq!(
            names_by_id(&device.conn, "spending_plans"),
            vec![
                format!("{plan_holder} Holiday"),
                format!("{plan_renamed} Holiday (2)")
            ]
        );
        assert_eq!(
            names_by_id(&device.conn, "transaction_categories"),
            vec![
                format!("{pets_holder} Pets"),
                format!("{pets_renamed} Pets (2)")
            ]
        );
    }

    step();
    let c = &vault.devices[2].conn;
    spending_plans::update_plan(c, &plan_holder, &plan_data("Trip")).expect("rename plan");
    let rename = UpdateTransactionCategory {
        kind: None,
        name: Some("Animals".into()),
        icon: None,
        color: None,
    };
    categories::update_category(c, &pets_holder, &rename).expect("rename category");
    vault.sync_all(&mut rng());

    vault.assert_converged();
    for device in &vault.devices {
        let conn = &device.conn;
        let mut plans = vec![
            format!("{plan_holder} Trip"),
            format!("{plan_renamed} Holiday"),
        ];
        plans.sort();
        assert_eq!(names_by_id(conn, "spending_plans"), plans);
        let mut pets = vec![
            format!("{pets_holder} Animals"),
            format!("{pets_renamed} Pets"),
        ];
        pets.sort();
        assert_eq!(names_by_id(conn, "transaction_categories"), pets);
        assert_eq!(count(conn, "SELECT COUNT(*) FROM sync_typed"), 0);
    }
}

/// D27 with an offline buy: A applies the 4:1 split while B, which has not seen it, adds a
/// buy dated before the ex-date (entered in pre-split units, no basis). Every device scales the
/// buy exactly once (3 at 80 shows as 12 at 20), keeps its entry values, and lists the split
/// in its basis.
#[test]
fn split_applied_while_a_pre_split_buy_is_added_offline() {
    let mut vault = Vault::new("split with an offline pre-split buy", DEVICES, |c, d| {
        stock_seed(c, d, true)
    });
    step();
    stock_splits::apply(&vault.devices[0].conn, &split_id()).expect("apply on A");
    step();
    investments::add_transaction_to_investment(
        &vault.devices[1].conn,
        &position_id(),
        &trade("3", "80", BUY_DAY),
    )
    .expect("buy on B");

    exchange(&mut vault, 0, 1);

    vault.assert_converged();
    let basis = format!(r#"["{}"]"#, split_id());
    for device in &vault.devices {
        assert_eq!(split_status(&device.conn), vec!["applied"]);
        assert_eq!(
            trades(&device.conn),
            vec![
                format!("28 25 | 7 100 - | {basis}"),
                format!("12 20 | 3 80 - | {basis}"),
            ]
        );
        assert_eq!(
            texts(&device.conn, "SELECT quantity FROM stock_investments"),
            vec!["40"]
        );
    }
}

/// Task 13 worked example 1 with the units: A detects the split and applies it; B, offline,
/// detects the same split (a newer `pending` insert of the same natural id). `applied`
/// survives on every device and the trade is scaled once.
#[test]
fn split_detected_on_two_devices_after_apply() {
    let mut vault = Vault::new("split detected on two devices", DEVICES, |c, d| {
        stock_seed(c, d, false)
    });
    step();
    detect(&vault.devices[0].conn);
    stock_splits::apply(&vault.devices[0].conn, &split_id()).expect("apply on A");
    step();
    detect(&vault.devices[1].conn);
    assert_eq!(split_status(&vault.devices[1].conn), vec!["pending"]);

    exchange(&mut vault, 0, 1);

    vault.assert_converged();
    let basis = format!(r#"["{}"]"#, split_id());
    for device in &vault.devices {
        assert_eq!(split_status(&device.conn), vec!["applied"]);
        assert_eq!(
            trades(&device.conn),
            vec![format!("28 25 | 7 100 - | {basis}")]
        );
    }
}

/// A revert on a device that did not apply the split reaches every device: `pending`, and the
/// trade back in its entered units with no basis.
#[test]
fn split_revert_reaches_every_device() {
    let mut vault = Vault::new("split revert", DEVICES, |c, d| stock_seed(c, d, true));
    step();
    stock_splits::apply(&vault.devices[0].conn, &split_id()).expect("apply on A");
    vault.sync_all(&mut rng());
    for device in &vault.devices {
        assert_eq!(split_status(&device.conn), vec!["applied"]);
    }
    step();
    stock_splits::revert(&vault.devices[1].conn, &split_id()).expect("revert on B");
    vault.sync_all(&mut rng());

    vault.assert_converged();
    for device in &vault.devices {
        assert_eq!(split_status(&device.conn), vec!["pending"]);
        assert_eq!(trades(&device.conn), vec!["7 100 | 7 100 - | -"]);
    }
}

/// `rows` bank transactions of account `acc` in one transaction, as a statement import writes
/// them: batch `batch` first, then every row through `create_transaction_with_origin`.
fn import_statement(conn: &Connection, rows: usize) {
    let tx = conn.unchecked_transaction().expect("transaction");
    csv_import::insert_batch(&tx, "batch", "acc", "statement.csv", None, None).expect("batch");
    for i in 0..rows {
        let data = InsertBankTransaction {
            bank_account_id: "acc".into(),
            transaction_id: Some(format!("BANK-{i:05}")),
            tx_type: "debit".into(),
            flow_kind: None,
            amount: format!("{}.{:02}", 10 + i, i % 100),
            currency: Some("CZK".into()),
            description: Some(format!("Card payment {i}")),
            counterparty_name: Some(format!("Merchant {}", i % 37)),
            counterparty_iban: None,
            counterparty_bank_code: None,
            counterparty_country: None,
            booking_date: JAN_1 + (i as i64 % 300) * DAY,
            value_date: None,
            category_id: None,
            status: None,
        };
        bank_accounts::create_transaction_with_origin(&tx, &data, "csv", Some("batch"), None)
            .expect("import row");
    }
    tx.commit().expect("commit import");
}

const IMPORTED: &str = "SELECT COUNT(*) FROM bank_transactions WHERE import_batch_id = 'batch'";

/// §18: A imports 5,000 payments in one transaction, sealed into many envelopes pushed one at
/// a time. B pulls each as it lands and, after every second, files one imported payment into
/// `k` and pushes, so the relay log interleaves B's edits between A's envelopes. C pulls the
/// whole log in its order without a stop and ends with all 5,000 rows and B's edits.
#[test]
fn import_5000_rows_over_many_envelopes() {
    let mut vault = Vault::new("5,000-row import", 3, base_seed);
    step();
    import_statement(&vault.devices[0].conn, 5_000);
    let (mut envelopes, mut edits) = (0, 0);
    loop {
        step();
        if !vault.push_one(0) {
            break;
        }
        envelopes += 1;
        pull_all(&mut vault, 1);
        if envelopes % 2 == 0 {
            step();
            let b = &vault.devices[1].conn;
            let id: String = b
                .query_row(
                    "SELECT id FROM bank_transactions \
                     WHERE import_batch_id = 'batch' AND category_id IS NULL ORDER BY id LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .expect("an imported payment on B");
            bank_accounts::set_transaction_category(b, &id, Some("k"), None).expect("file");
            edits += 1;
            assert_eq!(vault.push(1), 1);
        }
    }
    assert!(envelopes >= 10, "5,000 rows in only {envelopes} envelopes");
    let authors: Vec<DeviceId> = vault.relay.iter().map(|(cs, _)| cs.device).collect();
    assert!(
        authors.windows(2).filter(|w| w[0] != w[1]).count() >= 2 * (edits - 1),
        "B's envelopes sit between A's on the relay"
    );

    pull_all(&mut vault, 2);
    let c = &vault.devices[2].conn;
    assert_eq!(count(c, IMPORTED), 5_000);
    assert_eq!(
        count(
            c,
            "SELECT COUNT(*) FROM bank_transactions WHERE import_batch_id = 'batch' AND category_id = 'k'"
        ),
        edits as i64
    );

    vault.sync_all(&mut rng());
    vault.assert_converged();
    for device in &vault.devices {
        assert_eq!(count(&device.conn, IMPORTED), 5_000);
    }
}

/// §14: B runs a build below the newest barrier (simulated: its `sync_state` says barrier and
/// schema 24) and pushes a new category; every other device is past the barrier and skips the
/// envelope (`BelowBarrier`). B then migrates: the open crosses the barrier, re-queues its rows
/// newer than its snapshot, and pushes them again in the new format, and every device applies
/// them.
#[test]
fn barrier_requeue_resends_rows_pushed_below_the_barrier() {
    let mut vault = Vault::new("barrier re-queue", 3, base_seed);
    let below = barrier_number() - 1;
    vault.devices[1]
        .conn
        .execute(
            "UPDATE sync_state SET barrier = ?1, schema_seen = ?1",
            [below],
        )
        .expect("an old build");
    step();
    category(&vault.devices[1].conn, "Garden");
    assert_eq!(vault.push(1), 1);
    assert_eq!(vault.relay.last().expect("B's envelope").0.barrier, below);
    pull_all(&mut vault, 0);
    pull_all(&mut vault, 2);
    assert_eq!(verdicts(&vault, "BelowBarrier"), 2);
    for d in [0, 2] {
        assert!(!category_names(&vault.devices[d].conn).contains(&"Garden".to_string()));
    }

    // B is migrated and opened again: no capture until the open installs it.
    let b = &vault.devices[1].conn;
    capture::uninstall(b).expect("close");
    sync::on_open(b);
    assert!(capture::is_installed(b).expect("installed"));
    let sync = state::load(b).expect("state").expect("sync row");
    assert_eq!(sync.barrier, barrier_number());
    assert!(count(b, "SELECT COUNT(*) FROM sync_outbox") > 0);

    vault.sync_all(&mut rng());

    vault.assert_converged();
    assert_eq!(
        vault.relay.last().expect("the re-sent envelope").0.barrier,
        barrier_number()
    );
    for device in &vault.devices {
        assert_eq!(
            category_names(&device.conn),
            vec!["Family", "Garden", "Kids"]
        );
    }
}

/// A copy of device `d`'s profile as it is now (`VACUUM INTO`, R6), in the vault's directory.
fn copy_of(vault: &Vault, d: usize, name: &str) -> std::path::PathBuf {
    let path = vault.dir.path().join(name);
    vault.devices[d]
        .conn
        .execute(
            "VACUUM INTO ?1",
            [path.to_str().expect("a UTF-8 temp path")],
        )
        .expect("vacuum into");
    path
}

/// The copy at `path` opened as the app opens a migrated profile: foreign keys on, no
/// migrations, then `sync::on_open` (which installs capture).
fn open_copy(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open the copy");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys");
    sync::on_open(&conn);
    assert!(capture::is_installed(&conn).expect("installed"));
    conn
}

/// §11.4 (R6): B's profile is copied before two edits, which B pushes; then B is rolled back to
/// the copy, which makes one more edit and pulls. Its own ID on envelopes it never sealed is a
/// `Clone`: it re-joins under a new ID, applies the lost edits as the old self's, and pushes its
/// own. Every edit is present everywhere.
#[test]
fn rejoin_after_simulated_rollback() {
    let mut vault = Vault::new("re-join after a rollback", 3, base_seed);
    step();
    category(&vault.devices[1].conn, "Before");
    vault.sync_all(&mut rng());
    let path = copy_of(&vault, 1, "b-rolled-back.db");
    let processed = vault.devices[1].processed.clone();
    let old = vault.devices[1].id;
    step();
    category(&vault.devices[1].conn, "Edit one");
    step();
    category(&vault.devices[1].conn, "Edit two");
    assert_eq!(vault.push(1), 1);

    vault.devices[1].conn = open_copy(&path);
    vault.devices[1].processed = processed;
    assert!(!category_names(&vault.devices[1].conn).contains(&"Edit one".to_string()));
    step();
    category(&vault.devices[1].conn, "Edit three");
    pull_all(&mut vault, 1);

    assert_eq!(verdicts(&vault, "Clone"), 1);
    assert_ne!(vault.devices[1].id, old, "B re-joined under a new ID");

    vault.sync_all(&mut rng());

    vault.assert_converged_after_rejoins(&[1]);
    for device in &vault.devices {
        assert_eq!(
            category_names(&device.conn),
            vec![
                "Before",
                "Edit one",
                "Edit three",
                "Edit two",
                "Family",
                "Kids"
            ]
        );
    }
}

/// §11.4: a clone of B (its profile copied and run as a fourth device under B's ID) that has
/// pushed nothing sees B's next envelope: `judge` says `Clone` before the clone ever pushes. It
/// re-joins under a new ID and both work on.
#[test]
fn clone_detection_from_an_own_id_envelope() {
    let mut vault = Vault::new("clone detection", 3, base_seed);
    step();
    category(&vault.devices[1].conn, "Before");
    vault.sync_all(&mut rng());
    let path = copy_of(&vault, 1, "b-clone.db");
    let b_id = vault.devices[1].id;
    vault.devices.push(super::Device {
        conn: open_copy(&path),
        id: b_id,
        processed: vault.devices[1].processed.clone(),
        held: Vec::new(),
    });
    step();
    category(&vault.devices[1].conn, "Original");
    assert_eq!(vault.push(1), 1);

    let (cs, hash) = vault.relay.last().expect("B's envelope").clone();
    let clone = &vault.devices[3].conn;
    assert_eq!(count(clone, "SELECT COUNT(*) FROM sync_unsent"), 0);
    assert_eq!(
        judge(
            clone,
            &EnvelopeMeta::of(&cs, hash, vault.relay.len() as u64)
        )
        .expect("judge"),
        Verdict::Clone
    );
    pull_all(&mut vault, 3);
    assert_eq!(verdicts(&vault, "Clone"), 1);
    step();
    category(&vault.devices[3].conn, "Clone's own");
    vault.sync_all(&mut rng());

    vault.assert_converged_after_rejoins(&[3]);
    assert_ne!(vault.devices[3].id, b_id);
    for device in &vault.devices {
        assert_eq!(
            category_names(&device.conn),
            vec!["Before", "Clone's own", "Family", "Kids", "Original"]
        );
    }
}

/// §14: A deletes a seeded category; B gets the tombstone, then a migration re-runs the seed SQL
/// on B (before capture is installed, as at unlock) and brings the row back. `sync::on_open`
/// deletes it again, queues nothing, and B agrees with everyone.
#[test]
fn migration_seed_revival_is_deleted_again() {
    let mut vault = Vault::new("seed revival", 3, base_seed);
    let seeded = "SELECT COUNT(*) FROM transaction_categories WHERE id = 'cat_other_income'";
    assert_eq!(count(&vault.devices[1].conn, seeded), 1);
    step();
    vault.devices[0]
        .conn
        .execute(
            "DELETE FROM transaction_categories WHERE id = 'cat_other_income'",
            [],
        )
        .expect("delete the seeded category on A");
    vault.sync_all(&mut rng());
    assert_eq!(count(&vault.devices[1].conn, seeded), 0);

    let b = &vault.devices[1].conn;
    capture::uninstall(b).expect("close");
    let seed_sql = crate::db::migration_sql("010_income_categories").expect("migration 010");
    b.execute_batch(seed_sql).expect("re-run the seed");
    assert_eq!(count(b, seeded), 1, "the seed brought the row back");
    // Migration 010 ran just now: the last unlock had seen the schema up to 009.
    b.execute("UPDATE sync_state SET schema_seen = 9", [])
        .expect("the schema seen before the migration");
    sync::on_open(b);

    assert_eq!(count(b, seeded), 0);
    assert_eq!(count(b, "SELECT COUNT(*) FROM sync_outbox"), 0);
    vault.sync_all(&mut rng());
    vault.assert_converged();
}

/// What must stay device-local on `conn`: the profile's local columns and the local
/// `app_config` keys.
fn local_values(conn: &Connection) -> Vec<String> {
    let mut values = texts(
        conn,
        "SELECT COALESCE(language, '-') || ' ' || COALESCE(theme, '-') || ' ' \
           || COALESCE(mcp_server_token, '-') || ' ' || mcp_server_enabled FROM user_profile",
    );
    values.extend(texts(
        conn,
        "SELECT key || '=' || value FROM app_config \
         WHERE key IN ('fx_reference', 'backup.lastCreatedAt') ORDER BY key",
    ));
    values
}

/// §5: A changes its language, theme and MCP token together with its name, and its local
/// `app_config` keys. The name reaches every device; nothing local does.
#[test]
fn local_keys_and_columns_never_travel() {
    let mut vault = Vault::new("local data", 3, base_seed);
    let before: Vec<Vec<String>> = vault
        .devices
        .iter()
        .map(|d| local_values(&d.conn))
        .collect();
    step();
    vault.devices[0]
        .conn
        .execute_batch(
            "UPDATE user_profile SET name = 'Alexandra', language = 'cs', theme = 'dark', \
               mcp_server_token = 'token-of-a', mcp_server_enabled = 1; \
             INSERT INTO app_config (key, value) VALUES ('fx_reference', 'USD'), \
               ('backup.lastCreatedAt', '1767225600') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        )
        .expect("local edits on A");
    let changed = local_values(&vault.devices[0].conn);
    assert_ne!(changed, before[0]);
    assert_eq!(vault.push(0), 1, "only the name is queued");
    vault.sync_all(&mut rng());

    vault.assert_converged();
    assert_eq!(local_values(&vault.devices[0].conn), changed);
    for (d, (device, before)) in vault.devices.iter().zip(&before).enumerate().skip(1) {
        let conn = &device.conn;
        assert_eq!(
            texts(conn, "SELECT name FROM user_profile"),
            vec!["Alexandra"]
        );
        assert_eq!(&local_values(conn), before, "device {d}");
    }
}

fn rule_pack_config(conn: &Connection) -> Vec<String> {
    texts(
        conn,
        "SELECT key || '=' || value FROM app_config \
         WHERE key IN ('rule_packs_enabled', 'rule_packs_disabled_rules') ORDER BY key",
    )
}

/// §5, R4: rule-pack choices written explicitly on A (the enabled packs and a disabled rule)
/// travel to every device.
#[test]
fn rule_packs_written_explicitly_travel() {
    let mut vault = Vault::new("rule packs", 3, base_seed);
    let before = rule_pack_config(&vault.devices[1].conn);
    let pack = &packs::available_packs()[0];
    let enabled = rule_pack_config(&vault.devices[0].conn)
        .iter()
        .any(|kv| kv.starts_with("rule_packs_enabled=") && kv != "rule_packs_enabled=[]");
    let choice: Vec<String> = if enabled {
        Vec::new()
    } else {
        vec![pack.pack_id.clone()]
    };
    step();
    let a = &vault.devices[0].conn;
    packs::write_enabled(a, &choice).expect("write enabled packs");
    packs::write_rule_disabled(a, &pack.rules[0].id, true).expect("disable a rule");
    let written = rule_pack_config(a);
    assert_ne!(written, before);
    vault.sync_all(&mut rng());

    vault.assert_converged();
    for device in &vault.devices {
        assert_eq!(rule_pack_config(&device.conn), written);
    }
}

/// §18's failure drill as a named test: a relay that drops A's second envelope for C and
/// replays A's first. C holds the third behind the gap (the first applied), skips the
/// replay, and once the missing envelope arrives applies the rest.
#[test]
fn dishonest_relay_drops_one_envelope_and_replays_another() {
    let mut vault = Vault::new("dishonest relay", 3, base_seed);
    let mut sent = Vec::new();
    for name in ["One", "Two", "Three"] {
        step();
        category(&vault.devices[0].conn, name);
        let at = vault.relay.len();
        assert_eq!(vault.push(0), 1);
        sent.push(at);
    }
    let (first, second, third) = (sent[0], sent[1], sent[2]);
    let replay = vault.relay.len();
    let copy = vault.relay[first].clone();
    vault.relay.push(copy);

    assert_eq!(
        vault.pull(2, &[first, third]),
        PullEnd::Waiting {
            gaps: 1,
            parents: 0
        }
    );
    assert_eq!(
        category_names(&vault.devices[2].conn),
        vec!["Family", "Kids", "One"]
    );
    let skipped = verdicts(&vault, "SkipReplay");
    assert_eq!(vault.pull(2, &[replay, second, third]), PullEnd::Done);
    assert_eq!(verdicts(&vault, "SkipReplay"), skipped + 1);

    vault.sync_all(&mut rng());
    vault.assert_converged();
    for device in &vault.devices {
        assert_eq!(
            category_names(&device.conn),
            vec!["Family", "Kids", "One", "Three", "Two"]
        );
    }
}

/// The time to import `rows` statement rows (`import_statement`) into a fresh migrated profile
/// with an account, with capture installed when `synced`.
fn timed_import(rows: usize, synced: bool) -> Duration {
    let conn = migrated();
    conn.execute(
        "INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('acc', 'Main', 'CZK', '0')",
        [],
    )
    .expect("account");
    if synced {
        state::enable(&conn, DeviceId([0x51; 16])).expect("enable");
    }
    let started = Instant::now();
    import_statement(&conn, rows);
    let took = started.elapsed();
    assert_eq!(count(&conn, IMPORTED), rows as i64);
    took
}

/// §18 performance target (R19): capture makes a 5,000-row statement import less than 20 %
/// slower. Best of three runs each way; prints the ratio. Run with
/// `cargo test --release --lib capture_overhead -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement: run in release with --ignored --nocapture"]
fn capture_overhead_on_a_5000_row_statement_import() {
    const ROWS: usize = 5_000;
    let best = |synced: bool| {
        (0..3)
            .map(|_| timed_import(ROWS, synced))
            .min()
            .expect("runs")
    };
    let plain = best(false);
    let captured = best(true);
    let overhead = (captured.as_secs_f64() / plain.as_secs_f64() - 1.0) * 100.0;
    println!(
        "capture overhead: {overhead:.1}% ({ROWS} rows: {:.1} ms without sync, {:.1} ms with)",
        plain.as_secs_f64() * 1e3,
        captured.as_secs_f64() * 1e3
    );
}
