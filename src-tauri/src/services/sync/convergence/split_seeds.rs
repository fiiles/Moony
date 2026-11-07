//! Ruling T18c as named tests: the split cases the exploration found (seeds 1044 and 2175 of the
//! Task 18 report), reduced to their operations. The current units of a split-affected row are
//! derived once from its entry values, so every path ends on the same values.

use rusqlite::Connection;

use super::tests::{synced_vault, Rngs};
use super::{Vault, STEP_MS};
use crate::models::investments::InsertInvestmentTransaction;
use crate::services::investments;
use crate::services::stock_splits::{self, DetectedSplit, StockSplitSource};
use crate::services::sync::hlc::fake_clock;

const DAY: i64 = 86_400;
/// 2026-02-02, after the vault's seeded AAPL buy of 2026-01-01.
const TRADE_DAY: i64 = 1_767_225_600 + 32 * DAY;
/// 2026-06-01: the ex-date of the split.
const SPLIT_DAY: i64 = 1_767_225_600 + 151 * DAY;

/// The trade the split rewrites: 7 AAPL at 1044.365, whose price divided by 3 does not
/// terminate (the 8-decimal rounding of seed 1044).
fn trade_data(quantity: &str, price: &str) -> InsertInvestmentTransaction {
    InsertInvestmentTransaction {
        investment_id: None,
        tx_type: "buy".into(),
        ticker: "AAPL".into(),
        company_name: "Apple".into(),
        quantity: quantity.into(),
        price_per_unit: price.into(),
        currency: "USD".into(),
        transaction_date: TRADE_DAY,
        fee: None,
    }
}

/// On device 0: the trade, and a pending `numerator`:1 split of AAPL on `SPLIT_DAY`. Returns
/// `(split id, trade id)`.
fn trade_and_split(vault: &Vault, numerator: u64) -> (String, String) {
    let conn = &vault.devices[0].conn;
    let investment: String = conn
        .query_row(
            "SELECT id FROM stock_investments WHERE ticker = 'AAPL'",
            [],
            |r| r.get(0),
        )
        .expect("the seeded AAPL position");
    let trade =
        investments::add_transaction_to_investment(conn, &investment, &trade_data("7", "1044.365"))
            .expect("trade");
    let detected = DetectedSplit {
        effective_day: SPLIT_DAY,
        numerator,
        denominator: 1,
    };
    stock_splits::record_detected(conn, "AAPL", &[detected], StockSplitSource::Yahoo)
        .expect("detect");
    let split = stock_splits::find(conn, "AAPL", SPLIT_DAY)
        .expect("find")
        .expect("the split is recorded");
    (split.id, trade.id)
}

/// `(quantity, price_per_unit)` of the trade on device `d`.
fn current(vault: &Vault, d: usize, trade: &str) -> (String, String) {
    vault.devices[d]
        .conn
        .query_row(
            "SELECT quantity, price_per_unit FROM investment_transactions WHERE id = ?1",
            [trade],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("trade row")
}

/// `(entry_quantity, entry_price_per_unit, entry_basis)` of the trade on device `d`.
fn entry(vault: &Vault, d: usize, trade: &str) -> (String, String, Option<String>) {
    vault.devices[d]
        .conn
        .query_row(
            "SELECT entry_quantity, entry_price_per_unit, entry_basis
             FROM investment_transactions WHERE id = ?1",
            [trade],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("trade row")
}

fn status(conn: &Connection, split: &str) -> String {
    conn.query_row(
        "SELECT status FROM stock_splits WHERE id = ?1",
        [split],
        |r| r.get(0),
    )
    .expect("split row")
}

/// `(hlc, device)` of the split's `status` column clock on `conn`.
fn status_clock(conn: &Connection, split: &str) -> (i64, Vec<u8>) {
    conn.query_row(
        "SELECT hlc, device FROM sync_clock \
         WHERE tbl = 'stock_splits' AND pk = ?1 AND col = 'status'",
        [split],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .expect("status clock")
}

fn pair(quantity: &str, price: &str) -> (String, String) {
    (quantity.to_string(), price.to_string())
}

/// Seed 1044: a 3:1 apply followed by a revert used to leave `1044.36500001`; and an apply on
/// device 0 racing a dismiss on device 1 must end on the status with the newer clock and the
/// values it derives.
#[test]
fn seed_1044_a_3_to_1_apply_and_revert_round_trip_and_converge() {
    let mut rngs = Rngs::new(1044);
    let mut ids = None;
    let mut vault = synced_vault("T18c seed 1044", |vault, _| {
        ids = Some(trade_and_split(vault, 3));
    });
    let (split, trade) = ids.expect("setup ran");

    let d0 = &vault.devices[0].conn;
    stock_splits::apply(d0, &split).expect("apply");
    assert_eq!(current(&vault, 0, &trade), pair("21", "348.12166667"));
    stock_splits::revert(d0, &split).expect("revert");
    assert_eq!(current(&vault, 0, &trade), pair("7", "1044.365"));
    stock_splits::apply(d0, &split).expect("apply again");
    assert_eq!(current(&vault, 0, &trade), pair("21", "348.12166667"));
    stock_splits::revert(d0, &split).expect("revert again");

    // Device 0 applies while device 1, which has not seen it, dismisses.
    stock_splits::apply(&vault.devices[0].conn, &split).expect("apply on d0");
    let applied = status_clock(&vault.devices[0].conn, &split);
    stock_splits::dismiss(&vault.devices[1].conn, &split).expect("dismiss on d1");
    let dismissed = status_clock(&vault.devices[1].conn, &split);
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();

    // The status with the newer clock wins on every device.
    let merged = status(&vault.devices[0].conn, &split);
    let newer = if applied > dismissed {
        "applied"
    } else {
        "dismissed"
    };
    assert_eq!(
        merged, newer,
        "applied at {applied:?}, dismissed at {dismissed:?}"
    );
    for d in 0..vault.devices.len() {
        assert_eq!(status(&vault.devices[d].conn, &split), newer, "device {d}");
    }
    let expected = if merged == "applied" {
        pair("21", "348.12166667")
    } else {
        pair("7", "1044.365")
    };
    for d in 0..vault.devices.len() {
        assert_eq!(
            current(&vault, d, &trade),
            expected,
            "device {d} ({merged})"
        );
        assert_eq!(
            entry(&vault, d, &trade),
            ("7".into(), "1044.365".into(), None),
            "device {d}"
        );
    }

    // A device joining now gets the entry values and derives the same current values.
    let joined = vault.join_device(0);
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();
    assert_eq!(
        entry(&vault, joined, &trade),
        ("7".into(), "1044.365".into(), None)
    );
    assert_eq!(current(&vault, joined, &trade), expected);
}

/// Seed 2175: device 2, offline, edits the pre-split trade in today's units while device 1
/// reverts the applied 4:1 split a moment later. The revert must not undo the edit: every device
/// ends on the values the edit's entry and the merged status derive.
#[test]
fn seed_2175_an_offline_edit_during_a_revert_converges() {
    let mut rngs = Rngs::new(2175);
    let mut ids = None;
    let mut vault = synced_vault("T18c seed 2175", |vault, _| {
        let (split, trade) = trade_and_split(vault, 4);
        stock_splits::apply(&vault.devices[0].conn, &split).expect("apply");
        ids = Some((split, trade));
    });
    let (split, trade) = ids.expect("setup ran");
    for d in 0..vault.devices.len() {
        assert_eq!(
            current(&vault, d, &trade),
            pair("28", "261.09125"),
            "device {d}"
        );
    }

    // Device 2 edits first; device 1 reverts a second later, before either has pulled.
    let mut edit = trade_data("30", "300");
    let investment: String = vault.devices[2]
        .conn
        .query_row(
            "SELECT investment_id FROM investment_transactions WHERE id = ?1",
            [&trade],
            |r| r.get(0),
        )
        .expect("trade");
    edit.investment_id = Some(investment);
    investments::update_transaction(&vault.devices[2].conn, &trade, &edit).expect("edit on d2");
    assert_eq!(
        entry(&vault, 2, &trade),
        ("30".into(), "300".into(), Some(format!(r#"["{split}"]"#)))
    );
    fake_clock::advance(STEP_MS);
    stock_splits::revert(&vault.devices[1].conn, &split).expect("revert on d1");

    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();

    let merged = status(&vault.devices[0].conn, &split);
    let expected = if merged == "applied" {
        pair("30", "300")
    } else {
        pair("7.5", "1200")
    };
    for d in 0..vault.devices.len() {
        assert_eq!(
            entry(&vault, d, &trade),
            ("30".into(), "300".into(), Some(format!(r#"["{split}"]"#))),
            "device {d}"
        );
        assert_eq!(
            current(&vault, d, &trade),
            expected,
            "device {d} ({merged})"
        );
    }

    let joined = vault.join_device(0);
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();
    assert_eq!(current(&vault, joined, &trade), expected);
}

/// A manual 4:1 split of TSLA, applied before the upgrade, as migration 029's backfill leaves
/// it: the stored trades are in today's units and their entry basis names the split. Returns
/// the split id.
fn upgraded_tsla_split(conn: &Connection) -> String {
    let split = crate::services::sync::natural_id::natural_id(
        "stock_splits",
        &["TSLA", &SPLIT_DAY.to_string()],
    );
    let basis = format!(r#"["{split}"]"#);
    conn.execute_batch(&format!(
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
           VALUES ('inv-tsla', 'TSLA', 'Tesla', '40', 'USD');
         INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source,
             status, resolved_at, applied_count)
           VALUES ('{split}', 'TSLA', {SPLIT_DAY}, '4', '1', 'manual', 'applied', 1, 2);
         INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name,
             quantity, price_per_unit, currency, transaction_date, created_at, entry_quantity,
             entry_price_per_unit, entry_basis, split_basis)
           VALUES ('tsla-buy', 'inv-tsla', 'buy', 'TSLA', 'Tesla', '60', '41.325', 'USD',
                   {TRADE_DAY}, 0, '60', '41.325', '{basis}', '{basis}'),
                  ('tsla-sell', 'inv-tsla', 'sell', 'TSLA', 'Tesla', '20', '45', 'USD',
                   {TRADE_DAY} + 86400, 0, '20', '45', '{basis}', '{basis}');"
    ))
    .expect("the upgraded split");
    split
}

fn tsla(vault: &Vault, d: usize) -> Vec<(String, String)> {
    ["tsla-buy", "tsla-sell"]
        .iter()
        .map(|id| current(vault, d, id))
        .collect()
}

fn tsla_held(vault: &Vault, d: usize) -> String {
    vault.devices[d]
        .conn
        .query_row(
            "SELECT quantity FROM stock_investments WHERE ticker = 'TSLA'",
            [],
            |r| r.get(0),
        )
        .expect("position")
}

fn manual(numerator: &str) -> stock_splits::InsertStockSplit {
    stock_splits::InsertStockSplit {
        ticker: "TSLA".into(),
        effective_day: SPLIT_DAY,
        numerator: numerator.into(),
        denominator: "1".into(),
    }
}

/// The final fix wave: a wrong manual split recorded before the upgrade is fixed as before.
/// Device 0 reverts it, device 1 records it again at 2:1; every device ends on the trades
/// at 2:1 of what was originally entered.
#[test]
fn a_wrong_manual_split_from_before_the_upgrade_is_re_recorded_at_another_ratio() {
    let mut rngs = Rngs::new(29);
    let mut split = None;
    let mut vault = Vault::new("upgraded split re-recorded", 2, |conn, _| {
        split = Some(upgraded_tsla_split(conn));
    });
    let split = split.expect("seeded");

    stock_splits::revert(&vault.devices[0].conn, &split).expect("revert on d0");
    assert_eq!(tsla(&vault, 0), vec![pair("15", "165.3"), pair("5", "180")]);
    vault.sync_all(&mut rngs.schedule);
    stock_splits::record_manual(&vault.devices[1].conn, &manual("2")).expect("record on d1");
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();

    for d in 0..vault.devices.len() {
        assert_eq!(
            tsla(&vault, d),
            vec![pair("30", "82.65"), pair("10", "90")],
            "device {d}"
        );
        assert_eq!(tsla_held(&vault, d), "20", "device {d}");
        assert_eq!(status(&vault.devices[d].conn, &split), "applied");
    }
}

/// The final fix wave: a wrong manual split recorded before the upgrade can be deleted after a
/// revert, as before. Device 0 reverts, device 1 deletes; every device shows the trades at
/// their pre-split values and no row names the split any more.
#[test]
fn a_wrong_manual_split_from_before_the_upgrade_is_deleted_after_a_revert() {
    let mut rngs = Rngs::new(31);
    let mut split = None;
    let mut vault = Vault::new("upgraded split deleted", 2, |conn, _| {
        split = Some(upgraded_tsla_split(conn));
    });
    let split = split.expect("seeded");

    stock_splits::revert(&vault.devices[0].conn, &split).expect("revert on d0");
    vault.sync_all(&mut rngs.schedule);
    stock_splits::delete_manual(&vault.devices[1].conn, &split).expect("delete on d1");
    vault.sync_all(&mut rngs.schedule);
    vault.assert_converged();

    for d in 0..vault.devices.len() {
        let conn = &vault.devices[d].conn;
        assert_eq!(
            tsla(&vault, d),
            vec![pair("15", "165.3"), pair("5", "180")],
            "device {d}"
        );
        assert_eq!(tsla_held(&vault, d), "10", "device {d}");
        let named: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM investment_transactions WHERE entry_basis IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(named, 0, "device {d}");
        let splits: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_splits", [], |r| r.get(0))
            .expect("count");
        assert_eq!(splits, 0, "device {d}");
    }
}
