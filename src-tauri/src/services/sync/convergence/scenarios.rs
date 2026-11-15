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

use super::{ops, PullEnd, Vault, STEP_MS};
use crate::models::categorization::InsertScopedLearning;
use crate::models::investments::InsertInvestmentTransaction;
use crate::models::spending_plans::InsertSpendingPlan;
use crate::models::stock_monitor::InsertWatchedStock;
use crate::models::AttachmentKind;
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
    attachments, bank_accounts, categories, csv_import, investments, spending_plans, stock_monitor,
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

// ----------------------------------------------------------------------------------------------
// Documents and photos (ADR 0030). Content is not transferred here: a device may hold a row whose
// content is on another device, and its queue then says `down`.
// ----------------------------------------------------------------------------------------------

/// The vault's first data for the attachment scenarios: a policy with a document, a property
/// with a document, and a batch with a photo, all with content on device 0.
fn attachment_seed(conn: &mut Connection, dir: &Path) {
    let mut rng = StdRng::seed_from_u64(7);
    for name in [
        "insurance::create_policy",
        "real_estate::create_property",
        "attachments::add_policy_document",
        "attachments::add_property_document",
        "attachments::add_photo",
    ] {
        let step = ops::run_op(conn, &mut rng, dir, name)
            .unwrap_or_else(|e| panic!("seeding {name}: engine error: {e}"));
        assert_eq!(step.outcome, ops::Outcome::Done, "seeding {name}: {step:?}");
    }
}

fn attachment_vault(label: &str, devices: usize) -> Vault {
    Vault::new(label, devices, attachment_seed)
}

/// Runs the named operation on device `d`, which must succeed.
fn op_on(vault: &mut Vault, d: usize, name: &str) {
    step();
    vault.run_op(d, name, &mut rng());
    let last = vault.trace.last().cloned().unwrap_or_default();
    assert!(last.ends_with("Done"), "{name} on device {d}: {last}");
}

/// The rows agree everywhere (`assert_converged`), no device has a row without its parent, and
/// every device's queue agrees with its rows and its store.
fn assert_attachments_converged(vault: &Vault) {
    vault.assert_converged();
    for (d, device) in vault.devices.iter().enumerate() {
        let orphans = count(
            &device.conn,
            "SELECT COUNT(*) FROM pragma_foreign_key_check",
        );
        assert_eq!(orphans, 0, "device {d} holds rows without their parent");
        ops::queue_matches_rows(&device.conn);
        ops::queue_consistent(&device.conn);
    }
}

const ATTACHMENT_ROWS: &str = "SELECT (SELECT COUNT(*) FROM insurance_documents) \
    || ' ' || (SELECT COUNT(*) FROM real_estate_documents) \
    || ' ' || (SELECT COUNT(*) FROM real_estate_photos) \
    || ' ' || (SELECT COUNT(*) FROM real_estate_photo_batches)";

/// A document and a photo added on one device, a document on another: every device ends with the
/// same rows; only the adding device holds the content, and the others queue it `down`.
#[test]
fn documents_and_photos_added_on_two_devices_reach_every_device() {
    let mut vault = attachment_vault("documents and photos added", DEVICES);
    op_on(&mut vault, 0, "attachments::add_policy_document");
    op_on(&mut vault, 0, "attachments::add_photo");
    op_on(&mut vault, 1, "attachments::add_property_document");

    exchange(&mut vault, 0, 1);

    assert_attachments_converged(&vault);
    let rows = |d: usize| texts(&vault.devices[d].conn, ATTACHMENT_ROWS);
    assert_eq!(rows(0), rows(1));
    assert_eq!(rows(0), rows(2));
    let down = |d: usize| {
        count(
            &vault.devices[d].conn,
            "SELECT COUNT(*) FROM sync_blobs WHERE state = 'down'",
        )
    };
    assert_eq!(down(0), 1, "device 0 lacks only device 1's document");
    assert!(down(2) >= 5, "device 2 holds none of the content");
}

/// The devices settle their queues as the app does before the lanes take work and before their
/// changes are pushed.
fn settle_all(vault: &Vault) {
    for device in &vault.devices {
        ops::settle(&device.conn);
    }
}

/// `exchange` after every device settled its queue.
fn settled_exchange(vault: &mut Vault, x: usize, y: usize) {
    settle_all(vault);
    exchange(vault, x, y);
}

fn first_id(conn: &Connection, table: &str) -> String {
    texts(conn, &format!("SELECT id FROM {table}")).remove(0)
}

fn present(conn: &Connection, table: &str, id: &str) -> bool {
    count(
        conn,
        &format!("SELECT COUNT(*) FROM {table} WHERE id = '{id}'"),
    ) == 1
}

/// The rows of the seed on device `d`: the policy and property documents, the photo and its batch.
struct Seeded {
    policy_doc: String,
    property_doc: String,
    photo: String,
    batch: String,
}

fn seeded(vault: &Vault, d: usize) -> Seeded {
    let conn = &vault.devices[d].conn;
    Seeded {
        policy_doc: first_id(conn, "insurance_documents"),
        property_doc: first_id(conn, "real_estate_documents"),
        photo: first_id(conn, "real_estate_photos"),
        batch: first_id(conn, "real_estate_photo_batches"),
    }
}

/// `deleter` deletes both documents and the photo by ID; `editor`, offline, edits those very rows
/// (the documents' names, and the photo's batch: no service renames a document, so the same
/// statements a rename would run). A delete beats an edit (design D13): the documents and the
/// photo are gone on every device, the batch the photo was in stays and keeps the edit, and no
/// queue entry names the deleted content.
fn delete_races_edit(label: &str, deleter: usize, editor: usize) {
    let mut vault = attachment_vault(label, DEVICES);
    let rows = seeded(&vault, deleter);
    let blobs = texts(
        &vault.devices[deleter].conn,
        "SELECT blob_id FROM insurance_documents UNION SELECT blob_id FROM real_estate_documents \
         UNION SELECT blob_id FROM real_estate_photos \
         UNION SELECT thumb_blob_id FROM real_estate_photos",
    );
    assert_eq!(blobs.len(), 4);
    step();
    let conn = &vault.devices[deleter].conn;
    let dir = vault.dir.path();
    for (kind, id) in [
        (AttachmentKind::InsuranceDocument, &rows.policy_doc),
        (AttachmentKind::RealEstateDocument, &rows.property_doc),
    ] {
        attachments::delete_document(conn, dir, kind, id).expect("delete a document");
    }
    attachments::delete_photo(conn, dir, &rows.photo).expect("delete the photo");
    step();
    vault.devices[editor]
        .conn
        .execute_batch(&format!(
            "UPDATE insurance_documents SET name = 'Renamed', description = 'Edited' \
               WHERE id = '{}'; \
             UPDATE real_estate_documents SET name = 'Renamed' WHERE id = '{}'; \
             UPDATE real_estate_photo_batches SET description = 'Edited batch' WHERE id = '{}';",
            rows.policy_doc, rows.property_doc, rows.batch
        ))
        .expect("edits");

    settled_exchange(&mut vault, deleter, editor);

    assert_attachments_converged(&vault);
    for (d, device) in vault.devices.iter().enumerate() {
        let conn = &device.conn;
        assert!(
            !present(conn, "insurance_documents", &rows.policy_doc),
            "device {d}"
        );
        assert!(
            !present(conn, "real_estate_documents", &rows.property_doc),
            "device {d}"
        );
        assert!(
            !present(conn, "real_estate_photos", &rows.photo),
            "device {d}"
        );
        assert_eq!(
            texts(
                conn,
                &format!(
                    "SELECT description FROM real_estate_photo_batches WHERE id = '{}'",
                    rows.batch
                )
            ),
            vec!["Edited batch"],
            "device {d} keeps the batch edit"
        );
        assert_eq!(
            count(conn, "SELECT COUNT(*) FROM sync_blobs"),
            0,
            "device {d} still queues deleted content"
        );
    }
}

#[test]
fn documents_deleted_while_another_device_edits_them() {
    delete_races_edit("delete races edit", 0, 1);
}

#[test]
fn documents_edited_while_another_device_deletes_them() {
    delete_races_edit("edit races delete", 1, 0);
}

/// A deletes a photo batch (with its photo) by ID; B, offline, edits that batch's description:
/// the batch and its photo are gone on every device.
#[test]
fn batch_deleted_while_another_device_edits_it() {
    let mut vault = attachment_vault("batch delete races edit", DEVICES);
    let rows = seeded(&vault, 0);
    step();
    attachments::delete_photo_batch(&vault.devices[0].conn, vault.dir.path(), &rows.batch)
        .expect("delete the batch");
    step();
    vault.devices[1]
        .conn
        .execute(
            "UPDATE real_estate_photo_batches SET description = 'Edited batch' WHERE id = ?1",
            [&rows.batch],
        )
        .expect("edit the batch");

    settled_exchange(&mut vault, 0, 1);

    assert_attachments_converged(&vault);
    for (d, device) in vault.devices.iter().enumerate() {
        assert!(
            !present(&device.conn, "real_estate_photo_batches", &rows.batch),
            "device {d}"
        );
        assert!(
            !present(&device.conn, "real_estate_photos", &rows.photo),
            "device {d}"
        );
    }
}

/// The ids of `table` on `conn` that are not in `before`.
fn new_ids(conn: &Connection, table: &str, before: &[String]) -> Vec<String> {
    texts(conn, &format!("SELECT id FROM {table} ORDER BY id"))
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect()
}

/// After the exchange the rows `ids` of `table` are gone on every device, and no queue entry
/// names any of `blobs` (the content of the lost rows).
fn assert_lost(vault: &Vault, table: &str, ids: &[String], blobs: &[String]) {
    assert!(!ids.is_empty(), "the add made no row");
    for (d, device) in vault.devices.iter().enumerate() {
        for id in ids {
            assert!(
                !present(&device.conn, table, id),
                "{table} {id} survives on device {d}"
            );
        }
        for blob in blobs {
            assert_eq!(
                count(
                    &device.conn,
                    &format!("SELECT COUNT(*) FROM sync_blobs WHERE blob_id = '{blob}'")
                ),
                0,
                "device {d} still queues {blob}"
            );
        }
    }
}

/// A deletes the policy; B adds a document under it. The document is gone on every device with
/// its queue entry, and so are the policy and the seed's document.
#[test]
fn policy_deleted_while_another_device_adds_a_document() {
    let mut vault = attachment_vault("policy deleted under a document", DEVICES);
    let before = texts(&vault.devices[1].conn, "SELECT id FROM insurance_documents");
    op_on(&mut vault, 0, "insurance::delete_policy");
    op_on(&mut vault, 1, "attachments::add_policy_document");
    let added = new_ids(&vault.devices[1].conn, "insurance_documents", &before);
    let blobs = texts(
        &vault.devices[1].conn,
        "SELECT blob_id FROM insurance_documents",
    );

    settled_exchange(&mut vault, 0, 1);

    assert_attachments_converged(&vault);
    assert_lost(&vault, "insurance_documents", &added, &blobs);
    for device in &vault.devices {
        assert_eq!(
            count(&device.conn, "SELECT COUNT(*) FROM insurance_policies"),
            0
        );
    }
}

/// A deletes the property; B adds a document and a photo to it: all of it is gone on every
/// device with its queue entries.
#[test]
fn property_deleted_while_another_device_adds_attachments() {
    let mut vault = attachment_vault("property deleted under attachments", DEVICES);
    let conn = &vault.devices[1].conn;
    let docs = texts(conn, "SELECT id FROM real_estate_documents");
    let photos = texts(conn, "SELECT id FROM real_estate_photos");
    op_on(&mut vault, 0, "real_estate::delete_property");
    op_on(&mut vault, 1, "attachments::add_property_document");
    op_on(&mut vault, 1, "attachments::add_photo");
    let conn = &vault.devices[1].conn;
    let (new_docs, new_photos) = (
        new_ids(conn, "real_estate_documents", &docs),
        new_ids(conn, "real_estate_photos", &photos),
    );
    let blobs = texts(
        conn,
        "SELECT blob_id FROM real_estate_documents UNION SELECT blob_id FROM real_estate_photos \
         UNION SELECT thumb_blob_id FROM real_estate_photos",
    );

    settled_exchange(&mut vault, 0, 1);

    assert_attachments_converged(&vault);
    assert_lost(&vault, "real_estate_documents", &new_docs, &blobs);
    assert_lost(&vault, "real_estate_photos", &new_photos, &blobs);
    for device in &vault.devices {
        assert_eq!(count(&device.conn, "SELECT COUNT(*) FROM real_estate"), 0);
        assert_eq!(
            count(
                &device.conn,
                "SELECT COUNT(*) FROM real_estate_photo_batches"
            ),
            0
        );
    }
}

/// A deletes the batch with its photo; B adds a photo to that very batch: the new photo is gone
/// on every device with its queue entries, and so is the batch.
#[test]
fn batch_deleted_while_another_device_adds_a_photo() {
    let mut vault = attachment_vault("batch deleted under a photo", DEVICES);
    let rows = seeded(&vault, 0);
    attachments::delete_photo_batch(&vault.devices[0].conn, vault.dir.path(), &rows.batch)
        .expect("delete the batch");
    step();
    let conn = &vault.devices[1].conn;
    let before = texts(conn, "SELECT id FROM real_estate_photos");
    let prepared = crate::services::real_estate_photos::prepared_for_test(
        b"full image".to_vec(),
        b"thumb".to_vec(),
    );
    crate::services::real_estate_photos::record_photo(conn, &rows.batch, &prepared, 1_767_225_600)
        .expect("photo on B");
    let added = new_ids(conn, "real_estate_photos", &before);
    let blobs = texts(
        conn,
        &format!(
            "SELECT blob_id FROM real_estate_photos WHERE id = '{}' \
             UNION SELECT thumb_blob_id FROM real_estate_photos WHERE id = '{}'",
            added[0], added[0]
        ),
    );

    settled_exchange(&mut vault, 0, 1);

    assert_attachments_converged(&vault);
    assert_lost(&vault, "real_estate_photos", &added, &blobs);
    for (d, device) in vault.devices.iter().enumerate() {
        assert!(
            !present(&device.conn, "real_estate_photo_batches", &rows.batch),
            "device {d}"
        );
    }
}

/// Rows from before 0.13 (a plain file, no blob) never sync. A converts them to the store while B
/// adds an unrelated document: the converted rows then reach every device with their blobs, and
/// B's queue lists their content as `down`.
#[test]
fn legacy_rows_converted_on_one_device_while_others_sync_other_rows() {
    let legacy_seed = |conn: &mut Connection, dir: &Path| {
        attachment_seed(conn, dir);
        let policy = texts(conn, "SELECT id FROM insurance_policies").remove(0);
        let property = texts(conn, "SELECT id FROM real_estate").remove(0);
        let batch = texts(conn, "SELECT id FROM real_estate_photo_batches").remove(0);
        let doc_dir = dir.join("insurance_documents").join(&policy);
        let photo_dir = dir.join("real_estate_photos").join(&property).join(&batch);
        std::fs::create_dir_all(&doc_dir).expect("document folder");
        std::fs::create_dir_all(&photo_dir).expect("photo folder");
        let doc = doc_dir.join("old.pdf");
        std::fs::write(&doc, b"%PDF-1.4 legacy").expect("legacy document");
        std::fs::write(photo_dir.join("p.jpg"), b"legacy photo").expect("legacy photo");
        std::fs::write(photo_dir.join("p_thumb.jpg"), b"legacy thumb").expect("legacy thumb");
        conn.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, file_path, file_type) \
             VALUES ('legacy-doc', ?1, 'Old contract', ?2, 'contract')",
            rusqlite::params![policy, doc.to_string_lossy()],
        )
        .expect("legacy document row");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path) \
             VALUES ('legacy-photo', ?1, ?2, ?3)",
            rusqlite::params![
                batch,
                format!("real_estate_photos/{property}/{batch}/p.jpg"),
                format!("real_estate_photos/{property}/{batch}/p_thumb.jpg"),
            ],
        )
        .expect("legacy photo row");
    };
    let mut vault = Vault::new("legacy rows converted", DEVICES, legacy_seed);
    for device in &vault.devices[1..] {
        assert_eq!(
            count(
                &device.conn,
                "SELECT COUNT(*) FROM insurance_documents WHERE id = 'legacy-doc'"
            ),
            0,
            "a legacy row never syncs"
        );
    }
    // The devices first sync another row, so they are in step when the conversion arrives.
    op_on(&mut vault, 1, "attachments::add_property_document");
    op_on(&mut vault, 0, "attachments::add_policy_document");
    settled_exchange(&mut vault, 0, 1);
    step();
    let dir = vault.dir.path().to_path_buf();
    let conn = &vault.devices[0].conn;
    let items = crate::services::attachment_conversion::pending(conn).expect("pending");
    assert_eq!(items.len(), 2);
    for item in &items {
        let sources = crate::services::attachment_conversion::read_sources(&dir, item)
            .expect("read")
            .expect("files are there");
        crate::services::attachment_conversion::convert(conn, &dir, item, &sources)
            .expect("convert");
    }

    settle_all(&vault);
    vault.sync_all(&mut rng());

    assert_attachments_converged(&vault);
    for device in &vault.devices {
        assert_eq!(
            count(
                &device.conn,
                "SELECT COUNT(*) FROM insurance_documents \
                 WHERE id = 'legacy-doc' AND blob_id IS NOT NULL AND file_path IS NULL"
            ),
            1
        );
        assert_eq!(
            count(
                &device.conn,
                "SELECT COUNT(*) FROM real_estate_photos \
                 WHERE id = 'legacy-photo' AND blob_id IS NOT NULL AND thumb_blob_id IS NOT NULL"
            ),
            1
        );
    }
    assert_eq!(
        count(
            &vault.devices[1].conn,
            "SELECT COUNT(*) FROM sync_blobs WHERE state = 'down'"
        ),
        count(&vault.devices[0].conn, "SELECT COUNT(*) FROM sync_blobs") - 1,
        "B holds only its own document's content"
    );
}

/// A device joins after attachments exist and while more are added: it gets every row, and its
/// queue lists all the content as `down`.
#[test]
fn a_joining_device_gets_attachment_rows_without_their_content() {
    let mut vault = attachment_vault("joining with attachments", 2);
    op_on(&mut vault, 0, "attachments::add_photo");
    op_on(&mut vault, 1, "attachments::add_policy_document");
    exchange(&mut vault, 0, 1);
    op_on(&mut vault, 1, "attachments::add_property_document");
    let joiner = vault.join_device(0);
    op_on(&mut vault, 0, "attachments::add_policy_document");

    vault.sync_all(&mut rng());

    assert_attachments_converged(&vault);
    let conn = &vault.devices[joiner].conn;
    assert_eq!(count(conn, "SELECT COUNT(*) FROM files.blobs"), 0);
    assert!(count(conn, "SELECT COUNT(*) FROM sync_blobs") >= 8);
    assert_eq!(
        count(
            conn,
            "SELECT COUNT(*) FROM sync_blobs WHERE state <> 'down'"
        ),
        0
    );
}
