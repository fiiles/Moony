//! Stock and crypto operations: positions and trades, price and dividend overrides, the
//! watchlist, tags, splits, crypto positions.

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{day, ids, money, no_target, one_of, pick, pick2, run, OpFn, Outcome};
use crate::models::crypto::InsertCryptoTransaction;
use crate::models::investments::InsertInvestmentTransaction;
use crate::models::stock_monitor::{
    validate_target_direction, validate_target_price, InsertWatchedStock,
};
use crate::models::stock_tags::{InsertStockTag, InsertStockTagGroup};
use crate::services::stock_splits::{self, DetectedSplit, StockSplitSource};
use crate::services::{
    crypto_investments, investments, price_overrides, stock_monitor, stock_tags,
};

pub(super) const OPS: &[(&str, OpFn)] = &[
    (
        "investments::create_investment_with_transaction",
        create_investment,
    ),
    ("investments::add_transaction_to_investment", add_trade),
    ("investments::update_transaction", update_trade),
    ("investments::delete_transactions", delete_trade),
    ("price_overrides::upsert_stock_price", stock_price),
    ("price_overrides::upsert_dividend", dividend),
    ("price_overrides::upsert_crypto_price", crypto_price),
    ("stock_monitor::follow_stock", follow),
    ("stock_monitor::set_target_price", target),
    ("stock_monitor::update_notes", notes),
    ("stock_monitor::unfollow_stock", unfollow),
    ("stock_tags::create_group", create_group),
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

fn create_investment(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
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

fn add_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((id, ticker)) = pick2(
        conn,
        rng,
        "SELECT id, ticker FROM stock_investments ORDER BY rowid",
    ) else {
        return no_target();
    };
    let side = one_of(rng, &["buy", "buy", "sell"]);
    let data = trade(rng, Some(id.clone()), &ticker, side);
    run(format!("{id} {side} {}", data.quantity), || {
        data.validate()?;
        investments::add_transaction_to_investment(conn, &id, &data).map(drop)
    })
}

fn update_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((tx, investment)) = pick2(
        conn,
        rng,
        "SELECT id, investment_id FROM investment_transactions ORDER BY rowid",
    ) else {
        return no_target();
    };
    let ticker = ids(
        conn,
        &format!("SELECT ticker FROM stock_investments WHERE id = '{investment}'"),
    )
    .remove(0);
    let side = one_of(rng, &["buy", "sell"]);
    let data = trade(rng, Some(investment), &ticker, side);
    run(
        format!("{tx} -> {} {}", data.tx_type, data.quantity),
        || {
            data.validate()?;
            investments::update_transaction(conn, &tx, &data).map(drop)
        },
    )
}

fn delete_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(tx) = pick(
        conn,
        rng,
        "SELECT id FROM investment_transactions ORDER BY rowid",
    ) else {
        return no_target();
    };
    run(tx.clone(), || {
        investments::delete_transactions(conn, std::slice::from_ref(&tx)).map(drop)
    })
}

fn stock_price(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let ticker = one_of(rng, &TICKERS);
    let price = money(rng);
    run(format!("{ticker} {price}"), || {
        price_overrides::upsert_stock_price(conn, ticker, &price, "USD", super::now()).map(drop)
    })
}

fn dividend(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let ticker = one_of(rng, &TICKERS);
    let amount = format!("0.{}", rng.random_range(10..99));
    run(format!("{ticker} {amount}"), || {
        price_overrides::upsert_dividend(conn, ticker, &amount, "USD", super::now()).map(drop)
    })
}

fn crypto_price(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let (coin, _) = one_of(rng, &COINS);
    let price = money(rng);
    run(format!("{coin} {price}"), || {
        price_overrides::upsert_crypto_price(conn, coin, &price, "USD", super::now())
    })
}

fn follow(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = InsertWatchedStock {
        ticker: one_of(rng, &TICKERS).to_string(),
    };
    run(data.ticker.clone(), || {
        data.validate()?;
        stock_monitor::follow_stock(conn, &data).map(drop)
    })
}

const WATCHED: &str = "SELECT ticker FROM watched_stocks ORDER BY ticker";

fn target(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(ticker) = pick(conn, rng, WATCHED) else {
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

fn notes(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(ticker) = pick(conn, rng, WATCHED) else {
        return no_target();
    };
    let notes = format!("note {}", rng.random_range(0..100));
    run(format!("{ticker} {notes:?}"), || {
        stock_monitor::update_notes(conn, &ticker, notes.clone()).map(drop)
    })
}

fn unfollow(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(ticker) = pick(conn, rng, WATCHED) else {
        return no_target();
    };
    run(ticker.clone(), || {
        stock_monitor::unfollow_stock(conn, &ticker)
    })
}

fn create_group(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = InsertStockTagGroup {
        name: one_of(rng, &GROUP_NAMES).to_string(),
        description: None,
    };
    run(data.name.clone(), || {
        data.validate()?;
        stock_tags::create_group(conn, data.clone()).map(drop)
    })
}

fn tag_data(conn: &Connection, rng: &mut StdRng) -> InsertStockTag {
    InsertStockTag {
        name: one_of(rng, &TAG_NAMES).to_string(),
        color: Some(one_of(rng, &["#336699", "#669933"]).to_string()),
        group_id: if rng.random_bool(0.5) {
            pick(conn, rng, "SELECT id FROM stock_tag_groups ORDER BY rowid")
        } else {
            None
        },
    }
}

fn create_tag(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = tag_data(conn, rng);
    run(format!("{:?} in {:?}", data.name, data.group_id), || {
        data.validate()?;
        stock_tags::create_tag(conn, data.clone()).map(drop)
    })
}

const TAGS: &str = "SELECT id FROM stock_tags ORDER BY rowid";

fn update_tag(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, TAGS) else {
        return no_target();
    };
    let data = tag_data(conn, rng);
    run(
        format!("{id} -> {:?} in {:?}", data.name, data.group_id),
        || {
            data.validate()?;
            stock_tags::update_tag(conn, &id, data.clone()).map(drop)
        },
    )
}

fn set_tags(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(investment) = pick(conn, rng, "SELECT id FROM stock_investments ORDER BY rowid")
    else {
        return no_target();
    };
    let tags: Vec<String> = super::ids(conn, TAGS)
        .into_iter()
        .filter(|_| rng.random_bool(0.4))
        .collect();
    run(format!("{investment} {tags:?}"), || {
        stock_tags::set_investment_tags(conn, &investment, &tags)
    })
}

fn delete_tag(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, TAGS) else {
        return no_target();
    };
    run(id.clone(), || stock_tags::delete_tag(conn, &id))
}

fn detect_split(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(ticker) = pick(
        conn,
        rng,
        "SELECT ticker FROM stock_investments ORDER BY ticker",
    ) else {
        return no_target();
    };
    let split = DetectedSplit {
        effective_day: day(rng),
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

fn split_with(conn: &Connection, rng: &mut StdRng, status: &str) -> Option<String> {
    pick(
        conn,
        rng,
        &format!("SELECT id FROM stock_splits WHERE status = '{status}' ORDER BY rowid"),
    )
}

fn apply_split(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = split_with(conn, rng, "pending") else {
        return no_target();
    };
    run(id.clone(), || stock_splits::apply(conn, &id).map(drop))
}

fn revert_split(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = split_with(conn, rng, "applied") else {
        return no_target();
    };
    run(id.clone(), || stock_splits::revert(conn, &id).map(drop))
}

fn dismiss_split(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = split_with(conn, rng, "pending") else {
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

fn create_crypto(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
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

fn add_crypto_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((id, ticker)) = pick2(
        conn,
        rng,
        "SELECT id, ticker FROM crypto_investments ORDER BY rowid",
    ) else {
        return no_target();
    };
    let side = one_of(rng, &["buy", "buy", "sell"]);
    let data = crypto_trade(rng, Some(id.clone()), &ticker, side);
    run(format!("{id} {side} {}", data.quantity), || {
        data.validate()?;
        crypto_investments::add_transaction_to_crypto(conn, &id, &data).map(drop)
    })
}

fn delete_crypto_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(tx) = pick(
        conn,
        rng,
        "SELECT id FROM crypto_transactions ORDER BY rowid",
    ) else {
        return no_target();
    };
    run(tx.clone(), || {
        crypto_investments::delete_transaction(conn, &tx).map(drop)
    })
}
