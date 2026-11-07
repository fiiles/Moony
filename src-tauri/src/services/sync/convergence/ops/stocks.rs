//! Stock and crypto operations: positions, trades and imported trade batches, dividends, price
//! and dividend overrides, the watchlist, tags and tag groups, splits, crypto positions.

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{by_content, day, ids, money, no_target, one_of, pick, pick2, run, OpFn, Res};
use crate::error::AppError;
use crate::models::crypto::InsertCryptoTransaction;
use crate::models::investments::InsertInvestmentTransaction;
use crate::models::stock_dividends::InsertStockDividend;
use crate::models::stock_monitor::{
    validate_target_direction, validate_target_price, InsertWatchedStock,
};
use crate::models::stock_tags::{InsertStockTag, InsertStockTagGroup};
use crate::services::investments::{BulkStockRow, NewStockImportBatch};
use crate::services::stock_import::batches;
use crate::services::stock_splits::{self, DetectedSplit, StockSplitSource};
use crate::services::{
    crypto_investments, investments, price_overrides, stock_dividends, stock_monitor, stock_tags,
};

pub(super) const OPS: &[(&str, OpFn)] = &[
    (
        "investments::create_investment_with_transaction",
        create_investment,
    ),
    ("investments::add_transaction_to_investment", add_trade),
    ("investments::update_transaction", update_trade),
    ("investments::delete_transactions", delete_trade),
    ("investments::bulk_create_stock_transactions", import_trades),
    ("stock_import::batches::undo_batch", undo_import),
    ("stock_dividends::create", create_dividend),
    ("stock_dividends::update", update_dividend),
    ("stock_dividends::delete", delete_dividend),
    ("price_overrides::upsert_stock_price", stock_price),
    ("price_overrides::upsert_dividend", dividend_override),
    ("price_overrides::upsert_crypto_price", crypto_price),
    ("stock_monitor::follow_stock", follow),
    ("stock_monitor::set_target_price", target),
    ("stock_monitor::update_notes", notes),
    ("stock_monitor::unfollow_stock", unfollow),
    ("stock_tags::create_group", create_group),
    ("stock_tags::delete_group", delete_group),
    ("stock_tags::create_tag", create_tag),
    ("stock_tags::update_tag", update_tag),
    ("stock_tags::set_investment_tags", set_tags),
    ("stock_tags::delete_tag", delete_tag),
    ("stock_splits::record_detected", detect_split),
    ("stock_splits::apply", apply_split),
    ("stock_splits::revert", revert_split),
    ("stock_splits::dismiss", dismiss_split),
    (
        "crypto_investments::create_crypto_with_transaction",
        create_crypto,
    ),
    (
        "crypto_investments::add_transaction_to_crypto",
        add_crypto_trade,
    ),
    (
        "crypto_investments::delete_transaction",
        delete_crypto_trade,
    ),
];

const TICKERS: [&str; 4] = ["AAPL", "MSFT", "CEZ.PR", "VWCE.DE"];
const COINS: [(&str, &str); 2] = [("BTC", "BTC-USD"), ("ETH", "ETH-USD")];
const TAG_NAMES: [&str; 4] = ["Tech", "Dividend", "Europe", "Core"];
const GROUP_NAMES: [&str; 3] = ["Sector", "Region", "Style"];

fn positions(select: &str) -> String {
    by_content("stock_investments", select, "", "t.ticker")
}

fn trades(select: &str) -> String {
    by_content(
        "investment_transactions",
        select,
        "",
        "t.transaction_date, t.ticker, t.type, t.quantity, t.price_per_unit",
    )
}

fn trade(
    rng: &mut StdRng,
    investment: Option<String>,
    ticker: &str,
    tx_type: &str,
) -> InsertInvestmentTransaction {
    InsertInvestmentTransaction {
        investment_id: investment,
        tx_type: tx_type.to_string(),
        ticker: ticker.to_string(),
        company_name: format!("{ticker} Inc."),
        quantity: rng.random_range(1..20).to_string(),
        price_per_unit: money(rng),
        currency: "USD".into(),
        transaction_date: day(rng),
        fee: rng.random_bool(0.3).then(|| "1.50".to_string()),
    }
}

fn create_investment(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let ticker = one_of(rng, &TICKERS);
    let data = trade(rng, None, ticker, "buy");
    run(
        format!("{ticker} {} @ {}", data.quantity, data.price_per_unit),
        || {
            data.validate()?;
            investments::create_investment_with_transaction(
                conn,
                ticker,
                &data.company_name,
                Some(&data),
            )
            .map(drop)
        },
    )
}

fn add_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((id, ticker)) = pick2(conn, rng, &positions("t.id, t.ticker"))? else {
        return no_target();
    };
    let side = one_of(rng, &["buy", "buy", "sell"]);
    let data = trade(rng, Some(id.clone()), &ticker, side);
    run(format!("{id} {side} {}", data.quantity), || {
        data.validate()?;
        investments::add_transaction_to_investment(conn, &id, &data).map(drop)
    })
}

fn update_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((tx, ticker)) = pick2(conn, rng, &trades("t.id, t.ticker"))? else {
        return no_target();
    };
    let investment = ids(
        conn,
        &format!("SELECT investment_id FROM investment_transactions WHERE id = '{tx}'"),
    )?
    .pop()
    .ok_or_else(|| AppError::Internal(format!("trade {tx} vanished")))?;
    let side = one_of(rng, &["buy", "sell"]);
    let data = trade(rng, Some(investment), &ticker, side);
    run(format!("{tx} -> {side} {}", data.quantity), || {
        data.validate()?;
        investments::update_transaction(conn, &tx, &data).map(drop)
    })
}

fn delete_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(tx) = pick(conn, rng, &trades("t.id"))? else {
        return no_target();
    };
    run(tx.clone(), || {
        investments::delete_transactions(conn, std::slice::from_ref(&tx)).map(drop)
    })
}

/// A broker file of one to three trades written as one import batch (`stock_import_batches`).
fn import_trades(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let rows: Vec<BulkStockRow> = (0..rng.random_range(1..=3))
        .map(|_| {
            let ticker = one_of(rng, &TICKERS);
            BulkStockRow {
                ticker: ticker.to_string(),
                company_name: Some(format!("{ticker} Inc.")),
                tx_type: "buy".into(),
                quantity: rng.random_range(1..10).to_string(),
                price_per_unit: money(rng),
                currency: "USD".into(),
                transaction_date: day(rng),
                fee: None,
                external_id: None,
                allow_duplicate: false,
            }
        })
        .collect();
    let batch = NewStockImportBatch {
        file_name: format!("broker-{}.csv", rng.random_range(1..4)),
        source: "custom".into(),
    };
    let detail = format!(
        "{} {:?}",
        batch.file_name,
        rows.iter()
            .map(|r| format!("{} {}", r.ticker, r.quantity))
            .collect::<Vec<_>>()
    );
    run(detail, || {
        investments::bulk_create_stock_transactions(conn, &rows, Some(&batch)).map(drop)
    })
}

fn undo_import(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "stock_import_batches",
        "t.id",
        "",
        "t.file_name, t.source, t.trade_count",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(id.clone(), || batches::undo_batch(conn, &id).map(drop))
}

fn dividend_data(rng: &mut StdRng) -> InsertStockDividend {
    InsertStockDividend {
        ex_date: day(rng),
        gross_amount: format!(
            "{}.{:02}",
            rng.random_range(1..30),
            rng.random_range(0..100)
        ),
        currency: "USD".into(),
        note: None,
    }
}

fn dividends() -> String {
    by_content(
        "investment_dividends",
        "t.id",
        "",
        "t.ex_date, t.gross_amount, t.currency",
    )
}

fn create_dividend(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(investment) = pick(conn, rng, &positions("t.id"))? else {
        return no_target();
    };
    let data = dividend_data(rng);
    run(
        format!("{investment} {} on {}", data.gross_amount, data.ex_date),
        || stock_dividends::create(conn, &investment, &data).map(drop),
    )
}

fn update_dividend(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &dividends())? else {
        return no_target();
    };
    let data = dividend_data(rng);
    run(
        format!("{id} -> {} on {}", data.gross_amount, data.ex_date),
        || stock_dividends::update(conn, &id, &data).map(drop),
    )
}

fn delete_dividend(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &dividends())? else {
        return no_target();
    };
    run(id.clone(), || stock_dividends::delete(conn, &id))
}

fn stock_price(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let ticker = one_of(rng, &TICKERS);
    let price = money(rng);
    run(format!("{ticker} {price}"), || {
        price_overrides::upsert_stock_price(conn, ticker, &price, "USD", super::now()).map(drop)
    })
}

fn dividend_override(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let ticker = one_of(rng, &TICKERS);
    let amount = format!("0.{}", rng.random_range(10..99));
    run(format!("{ticker} {amount}"), || {
        price_overrides::upsert_dividend(conn, ticker, &amount, "USD", super::now()).map(drop)
    })
}

fn crypto_price(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let (coin, _) = one_of(rng, &COINS);
    let price = money(rng);
    run(format!("{coin} {price}"), || {
        price_overrides::upsert_crypto_price(conn, coin, &price, "USD", super::now())
    })
}

fn follow(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = InsertWatchedStock {
        ticker: one_of(rng, &TICKERS).to_string(),
    };
    run(data.ticker.clone(), || {
        data.validate()?;
        stock_monitor::follow_stock(conn, &data).map(drop)
    })
}

fn watched() -> String {
    by_content("watched_stocks", "t.ticker", "", "t.ticker")
}

fn target(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(ticker) = pick(conn, rng, &watched())? else {
        return no_target();
    };
    let price = rng.random_bool(0.8).then(|| money(rng));
    let direction = Some(one_of(rng, &["above", "below"]).to_string());
    run(format!("{ticker} {price:?} {direction:?}"), || {
        validate_target_price(&price)?;
        validate_target_direction(&direction)?;
        stock_monitor::set_target_price(conn, &ticker, price.clone(), direction.clone()).map(drop)
    })
}

fn notes(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(ticker) = pick(conn, rng, &watched())? else {
        return no_target();
    };
    let notes = format!("note {}", rng.random_range(0..100));
    run(format!("{ticker} {notes:?}"), || {
        stock_monitor::update_notes(conn, &ticker, notes.clone()).map(drop)
    })
}

fn unfollow(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(ticker) = pick(conn, rng, &watched())? else {
        return no_target();
    };
    run(ticker.clone(), || {
        stock_monitor::unfollow_stock(conn, &ticker)
    })
}

fn groups() -> String {
    by_content("stock_tag_groups", "t.id", "", "t.name")
}

fn create_group(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = InsertStockTagGroup {
        name: one_of(rng, &GROUP_NAMES).to_string(),
        description: None,
    };
    run(data.name.clone(), || {
        data.validate()?;
        stock_tags::create_group(conn, data.clone()).map(drop)
    })
}

/// Deleting a group takes its tags and their assignments with it.
fn delete_group(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &groups())? else {
        return no_target();
    };
    run(id.clone(), || stock_tags::delete_group(conn, &id))
}

fn tag_data(conn: &Connection, rng: &mut StdRng) -> crate::error::Result<InsertStockTag> {
    Ok(InsertStockTag {
        name: one_of(rng, &TAG_NAMES).to_string(),
        color: Some(one_of(rng, &["#336699", "#669933"]).to_string()),
        group_id: if rng.random_bool(0.5) {
            pick(conn, rng, &groups())?
        } else {
            None
        },
    })
}

fn create_tag(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = tag_data(conn, rng)?;
    run(format!("{:?} in {:?}", data.name, data.group_id), || {
        data.validate()?;
        stock_tags::create_tag(conn, data.clone()).map(drop)
    })
}

fn tags() -> String {
    by_content("stock_tags", "t.id", "", "t.name, t.color")
}

fn update_tag(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &tags())? else {
        return no_target();
    };
    let data = tag_data(conn, rng)?;
    run(
        format!("{id} -> {:?} in {:?}", data.name, data.group_id),
        || {
            data.validate()?;
            stock_tags::update_tag(conn, &id, data.clone()).map(drop)
        },
    )
}

fn set_tags(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(investment) = pick(conn, rng, &positions("t.id"))? else {
        return no_target();
    };
    let tags: Vec<String> = ids(conn, &tags())?
        .into_iter()
        .filter(|_| rng.random_bool(0.4))
        .collect();
    run(format!("{investment} {tags:?}"), || {
        stock_tags::set_investment_tags(conn, &investment, &tags)
    })
}

fn delete_tag(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &tags())? else {
        return no_target();
    };
    run(id.clone(), || stock_tags::delete_tag(conn, &id))
}

fn detect_split(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(ticker) = pick(conn, rng, &positions("t.ticker"))? else {
        return no_target();
    };
    // Yahoo reports a split inside the position's history: after its first trade.
    let first = ids(
        conn,
        &format!(
            "SELECT CAST(MIN(transaction_date) AS TEXT) FROM investment_transactions \
             WHERE ticker = '{ticker}'"
        ),
    )?
    .pop()
    .and_then(|day| day.parse::<i64>().ok())
    .unwrap_or(super::DAY0);
    let split = DetectedSplit {
        effective_day: first - first.rem_euclid(super::DAY) + rng.random_range(1..90) * super::DAY,
        numerator: one_of(rng, &[2, 3, 4]),
        denominator: 1,
    };
    run(
        format!("{ticker} {}:1 on {}", split.numerator, split.effective_day),
        || {
            stock_splits::record_detected(conn, &ticker, &[split], StockSplitSource::Yahoo)
                .map(drop)
        },
    )
}

fn split_with(
    conn: &Connection,
    rng: &mut StdRng,
    status: &str,
) -> crate::error::Result<Option<String>> {
    let sql = by_content(
        "stock_splits",
        "t.id",
        &format!("WHERE t.status = '{status}'"),
        "t.ticker, t.effective_day",
    );
    pick(conn, rng, &sql)
}

fn apply_split(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = split_with(conn, rng, "pending")? else {
        return no_target();
    };
    run(id.clone(), || stock_splits::apply(conn, &id).map(drop))
}

fn revert_split(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = split_with(conn, rng, "applied")? else {
        return no_target();
    };
    run(id.clone(), || stock_splits::revert(conn, &id).map(drop))
}

fn dismiss_split(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = split_with(conn, rng, "pending")? else {
        return no_target();
    };
    run(id.clone(), || stock_splits::dismiss(conn, &id).map(drop))
}

fn crypto_trade(
    rng: &mut StdRng,
    investment: Option<String>,
    ticker: &str,
    tx_type: &str,
) -> InsertCryptoTransaction {
    InsertCryptoTransaction {
        investment_id: investment,
        tx_type: tx_type.to_string(),
        ticker: ticker.to_string(),
        name: format!("{ticker} coin"),
        quantity: format!("0.{}", rng.random_range(1..99)),
        price_per_unit: money(rng),
        currency: "USD".into(),
        transaction_date: day(rng),
    }
}

fn create_crypto(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let (coin, symbol) = one_of(rng, &COINS);
    let data = crypto_trade(rng, None, coin, "buy");
    run(format!("{coin} {}", data.quantity), || {
        data.validate()?;
        crypto_investments::create_crypto_with_transaction(
            conn,
            coin,
            Some(symbol),
            &data.name,
            Some(&data),
        )
        .map(drop)
    })
}

fn add_crypto_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content("crypto_investments", "t.id, t.ticker", "", "t.ticker");
    let Some((id, ticker)) = pick2(conn, rng, &sql)? else {
        return no_target();
    };
    let side = one_of(rng, &["buy", "buy", "sell"]);
    let data = crypto_trade(rng, Some(id.clone()), &ticker, side);
    run(format!("{id} {side} {}", data.quantity), || {
        data.validate()?;
        crypto_investments::add_transaction_to_crypto(conn, &id, &data).map(drop)
    })
}

fn delete_crypto_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "crypto_transactions",
        "t.id",
        "",
        "t.transaction_date, t.ticker, t.type, t.quantity, t.price_per_unit",
    );
    let Some(tx) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(tx.clone(), || {
        crypto_investments::delete_transaction(conn, &tx).map(drop)
    })
}
