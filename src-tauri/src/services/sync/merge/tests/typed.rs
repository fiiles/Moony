//! Recomputed-or-typed columns (ruling T18b): computed from their source rows while any exist,
//! typed otherwise. The typed value waits in `sync_typed` while a computed one is shown, travels
//! on the wire, and comes back on every device when the last source row goes.

use super::*;
use crate::models::valuations::InsertAssetValuation;
use crate::services::sync::capture::Mode;
use crate::services::valuations::{self, ValuationKind};
use crate::services::{bank_accounts, crypto_investments, investments};

const DAY: i64 = 1_767_225_600; // 2026-01-01

/// Every `sync_typed` entry as `"tbl pk col typed -> assigned"`.
fn entries(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT tbl || ' ' || pk || ' ' || col || ' ' || quote(typed) || ' -> ' \
               || quote(assigned) FROM sync_typed ORDER BY tbl, pk, col",
        )
        .expect("prepare");
    stmt.query_map([], |r| r.get(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn ids(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect(sql);
    stmt.query_map([], |r| r.get(0))
        .expect(sql)
        .map(|r| r.expect("row"))
        .collect()
}

/// A shares `base` with B and C; B and C each delete one source row, `delete(b)` and
/// `delete(c)`; then everyone exchanges everything.
fn each_deletes_one(
    a: &Connection,
    b: &Connection,
    c: &Connection,
    delete_on_b: impl FnOnce(&Connection),
    delete_on_c: impl FnOnce(&Connection),
) {
    let base = send(a);
    apply(b, &base);
    apply(c, &base);
    delete_on_b(b);
    later(c, b);
    delete_on_c(c);
    let (from_b, from_c) = (send(b), send(c));
    apply(a, &from_b);
    apply(a, &from_c);
    apply(b, &from_c);
    apply(c, &from_b);
}

/// Repro A of the Task 18 report: AAPL has buys of 10 and 4; B deletes the 10, C the 4. Every
/// device ends with the typed quantity the buys overwrote (the position was created at 0).
#[test]
fn repro_a_two_devices_each_delete_one_of_the_last_two_buys() {
    let (a, b, c) = (device(A), device(B), device(C));
    for quantity in ["10", "4"] {
        investments::import_single_transaction(
            &a, "AAPL", "Apple", "buy", quantity, "100", "USD", DAY,
        )
        .expect("buy");
    }
    let position = natural_id("stock_investments", &["AAPL"]);
    assert_eq!(
        entries(&a),
        vec![format!("stock_investments {position} quantity '0' -> '14'")]
    );
    let buy = |quantity: &str| {
        ids(
            &a,
            &format!("SELECT id FROM investment_transactions WHERE quantity = '{quantity}'"),
        )
    };
    let (ten, four) = (buy("10"), buy("4"));

    each_deletes_one(
        &a,
        &b,
        &c,
        |conn| {
            investments::delete_transactions(conn, &ten).expect("delete 10");
        },
        |conn| {
            investments::delete_transactions(conn, &four).expect("delete 4");
        },
    );

    for conn in [&a, &b, &c] {
        assert_eq!(
            value(conn, "SELECT quantity FROM stock_investments").as_deref(),
            Some("0")
        );
        assert_eq!(
            count(conn, "SELECT COUNT(*) FROM investment_transactions"),
            0
        );
        assert!(entries(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// Repro B of the Task 18 report, deterministic. The account and its one record predate sync
/// (no clocks, no entries). C deletes the record: no typed value is held, so the balance falls to
/// 0 as before sync, captured. B relays C's balance with an edit of its own; A sees B's
/// changeset before C's and keeps its record a while: the computed balance hides the typed 0,
/// which comes back when the record goes.
#[test]
fn repro_b_a_relayed_balance_arrives_before_the_record_tombstone() {
    let (a, b, c) = (device(A), device(B), device(C));
    for conn in [&a, &b, &c] {
        capture::with_mode(conn, Mode::Repair, || {
            conn.execute_batch(
                "INSERT INTO bank_accounts (id, name, currency, balance, balance_as_of) \
                   VALUES ('acc', 'Main', 'CZK', '3259.46', 1767225600); \
                 INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of, \
                   recorded_at) VALUES ('rec', 'acc', '3259.46', 'CZK', 1767225600, 100);",
            )?;
            Ok(())
        })
        .expect("pre-sync rows");
    }
    bank_accounts::delete_balance_record(&c, "rec").expect("delete record");
    let relay_0 = send(&c);
    apply(&b, &relay_0);
    later(&b, &c);
    exec(
        &b,
        "UPDATE bank_accounts SET interest_rate = '1.5' WHERE id = 'acc'",
    );
    let relay_1 = send(&b);

    apply(&a, &relay_1);
    assert_eq!(
        entries(&a),
        vec![
            "bank_accounts acc balance '0' -> '3259.46'".to_string(),
            "bank_accounts acc balance_as_of NULL -> 1767225600".to_string(),
        ]
    );
    apply(&a, &relay_0);
    apply(&c, &relay_1);

    for conn in [&a, &b, &c] {
        assert_eq!(
            value(
                conn,
                "SELECT balance || ' ' || COALESCE(balance_as_of, '-') FROM bank_accounts"
            )
            .as_deref(),
            Some("0 -")
        );
        assert!(entries(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

fn add_valuation(
    conn: &Connection,
    kind: ValuationKind,
    asset: &str,
    value: &str,
    day: i64,
) -> String {
    valuations::add_valuation(
        conn,
        kind,
        &InsertAssetValuation {
            asset_id: asset.into(),
            value: value.into(),
            currency: Some("EUR".into()),
            valued_at: day,
            note: None,
        },
    )
    .expect("add valuation")
    .id
}

/// The `other_assets.market_price` mirror: the asset was typed at 2304.23, two valuations
/// followed, and B and C each delete one of them.
#[test]
fn other_asset_price_comes_back_typed_when_two_devices_delete_the_last_valuations() {
    let (a, b, c) = (device(A), device(B), device(C));
    exec(
        &a,
        "INSERT INTO other_assets (id, name, quantity, market_price, currency, created_at) \
         VALUES ('wine', 'Wine', '7', '2304.23', 'EUR', 100)",
    );
    let first = add_valuation(&a, ValuationKind::OtherAsset, "wine", "26000", DAY);
    let second = add_valuation(
        &a,
        ValuationKind::OtherAsset,
        "wine",
        "332000",
        DAY + 86_400,
    );
    assert_eq!(
        value(&a, "SELECT market_price FROM other_assets").as_deref(),
        Some("332000")
    );

    each_deletes_one(
        &a,
        &b,
        &c,
        |conn| {
            valuations::delete_valuation(conn, ValuationKind::OtherAsset, &first).expect("delete");
        },
        |conn| {
            valuations::delete_valuation(conn, ValuationKind::OtherAsset, &second).expect("delete");
        },
    );

    for conn in [&a, &b, &c] {
        assert_eq!(
            value(conn, "SELECT market_price FROM other_assets").as_deref(),
            Some("2304.23")
        );
        assert!(entries(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// `real_estate.market_price` and its currency come back as one typed pair.
#[test]
fn real_estate_price_and_currency_come_back_typed_when_two_devices_delete_the_last_valuations() {
    let (a, b, c) = (device(A), device(B), device(C));
    exec(
        &a,
        "INSERT INTO real_estate (id, name, address, type, created_at) \
         VALUES ('flat', 'Flat', 'Main street 1', 'personal', 100)",
    );
    let first = add_valuation(&a, ValuationKind::RealEstate, "flat", "518000", DAY);
    let second = add_valuation(
        &a,
        ValuationKind::RealEstate,
        "flat",
        "866000",
        DAY + 86_400,
    );
    assert_eq!(
        value(
            &a,
            "SELECT market_price || ' ' || market_price_currency FROM real_estate"
        )
        .as_deref(),
        Some("866000 EUR")
    );

    each_deletes_one(
        &a,
        &b,
        &c,
        |conn| {
            valuations::delete_valuation(conn, ValuationKind::RealEstate, &first).expect("delete");
        },
        |conn| {
            valuations::delete_valuation(conn, ValuationKind::RealEstate, &second).expect("delete");
        },
    );

    for conn in [&a, &b, &c] {
        assert_eq!(
            value(
                conn,
                "SELECT market_price || ' ' || market_price_currency FROM real_estate"
            )
            .as_deref(),
            Some("0 CZK")
        );
        assert!(entries(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// `crypto_investments.quantity` and `currency` come back as one typed pair.
#[test]
fn crypto_quantity_and_currency_come_back_typed_when_two_devices_delete_the_last_trades() {
    let (a, b, c) = (device(A), device(B), device(C));
    exec(
        &a,
        "INSERT INTO crypto_investments (id, ticker, name, quantity, currency) \
           VALUES ('btc', 'BTC', 'Bitcoin', '0.5', 'CZK'); \
         INSERT INTO crypto_transactions (id, investment_id, type, ticker, name, quantity, \
           price_per_unit, currency, transaction_date, created_at) VALUES \
           ('t1', 'btc', 'buy', 'BTC', 'Bitcoin', '1', '50000', 'EUR', 1767225600, 100), \
           ('t2', 'btc', 'buy', 'BTC', 'Bitcoin', '2', '60000', 'EUR', 1767312000, 100);",
    );
    crypto_investments::recalculate_crypto_metrics(&a, "btc").expect("recalculate");
    assert_eq!(
        value(
            &a,
            "SELECT quantity || ' ' || currency FROM crypto_investments"
        )
        .as_deref(),
        Some("3 EUR")
    );

    each_deletes_one(
        &a,
        &b,
        &c,
        |conn| {
            crypto_investments::delete_transaction(conn, "t1").expect("delete t1");
        },
        |conn| {
            crypto_investments::delete_transaction(conn, "t2").expect("delete t2");
        },
    );

    for conn in [&a, &b, &c] {
        assert_eq!(
            value(
                conn,
                "SELECT quantity || ' ' || currency FROM crypto_investments"
            )
            .as_deref(),
            Some("0.5 CZK")
        );
        assert!(entries(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// G5: without sync there are no shadow triggers and `sync_typed` stays empty, so the services
/// behave as before: the last balance record's deletion zeroes the balance, a position's
/// quantity follows its trades.
#[test]
fn without_sync_the_local_behaviour_is_unchanged() {
    let conn = crate::services::sync::test_support::migrated();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('acc', 'Main', 'CZK', '125.00');",
    )
    .expect("account");
    bank_accounts::record_balance(
        &conn,
        "acc",
        &crate::models::bank_accounts::InsertBalanceRecord {
            amount: "3000.00".into(),
            currency: "CZK".into(),
            as_of: DAY,
            confirm_unknown_as_of: true,
            source_batch_id: None,
        },
    )
    .expect("record");
    let record = bank_accounts::balance_record_id("acc", DAY);
    bank_accounts::delete_balance_record(&conn, &record).expect("delete record");
    for quantity in ["10", "4"] {
        investments::import_single_transaction(
            &conn, "AAPL", "Apple", "buy", quantity, "100", "USD", DAY,
        )
        .expect("buy");
    }
    let four = ids(
        &conn,
        "SELECT id FROM investment_transactions WHERE quantity = '4'",
    );
    investments::delete_transactions(&conn, &four).expect("delete buy");

    assert_eq!(
        value(
            &conn,
            "SELECT balance || ' ' || COALESCE(balance_as_of, '-') FROM bank_accounts"
        )
        .as_deref(),
        Some("0 -")
    );
    assert_eq!(
        value(&conn, "SELECT quantity FROM stock_investments").as_deref(),
        Some("10")
    );
    assert!(entries(&conn).is_empty());
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'trigger'"
        ),
        0
    );
}
