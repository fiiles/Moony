//! Asset operations: loans and their events, real estate, valuations, other assets, bonds.

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{currency, day, money, no_target, now, one_of, pick, run, OpFn, Outcome};
use crate::models::bonds::InsertBond;
use crate::models::loan_events::InsertLoanEvent;
use crate::models::loans::InsertLoan;
use crate::models::other_assets::{InsertOtherAsset, InsertOtherAssetTransaction};
use crate::models::real_estate::InsertRealEstate;
use crate::models::valuations::InsertAssetValuation;
use crate::services::valuations::{self, ValuationKind};
use crate::services::{bonds, loan_events, loans, other_assets, real_estate};

pub(super) const OPS: &[(&str, OpFn)] = &[
    ("loans::create_loan", create_loan),
    ("loans::update_loan", update_loan),
    ("loan_events::add_event", add_event),
    ("loan_events::delete_event", delete_event),
    ("real_estate::create_property", create_property),
    ("real_estate::update_property", update_property),
    ("valuations::add_valuation", add_valuation),
    ("valuations::delete_valuation", delete_valuation),
    ("other_assets::create_asset", create_asset),
    ("other_assets::create_asset_transaction", asset_trade),
    ("bonds::create_bond", create_bond),
    ("bonds::update_bond", update_bond),
    ("bonds::delete_bond", delete_bond),
];

const LOANS: &str = "SELECT id FROM loans ORDER BY rowid";
const PROPERTIES: &str = "SELECT id FROM real_estate ORDER BY rowid";
const OTHER_ASSETS: &str = "SELECT id FROM other_assets ORDER BY rowid";
const BONDS: &str = "SELECT id FROM bonds ORDER BY rowid";

fn loan_data(rng: &mut StdRng) -> InsertLoan {
    InsertLoan {
        name: one_of(rng, &["Mortgage", "Car"]).to_string(),
        principal: format!("{}000", rng.random_range(100..900)),
        currency: Some(currency(rng).to_string()),
        interest_rate: Some(format!(
            "{}.{}",
            rng.random_range(1..7),
            rng.random_range(0..10)
        )),
        interest_rate_validity_date: Some(day(rng) + 365 * super::DAY),
        monthly_payment: Some(format!("{}00", rng.random_range(50..200))),
        start_date: Some(super::DAY0 - 400 * super::DAY),
        end_date: Some(super::DAY0 + 3_000 * super::DAY),
        balance_anchor_amount: None,
        balance_anchor_date: None,
    }
}

fn create_loan(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = loan_data(rng);
    run(format!("{:?} {}", data.name, data.principal), || {
        data.validate()?;
        loans::create_loan(conn, &data).map(drop)
    })
}

fn update_loan(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, LOANS) else {
        return no_target();
    };
    let data = loan_data(rng);
    run(
        format!("{id} -> {:?} {:?}", data.name, data.interest_rate),
        || {
            data.validate()?;
            loans::update_loan(conn, &id, &data).map(drop)
        },
    )
}

fn add_event(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(loan) = pick(conn, rng, LOANS) else {
        return no_target();
    };
    let kind = one_of(rng, &["extra_payment", "rate_change", "balance_check"]);
    let data = InsertLoanEvent {
        loan_id: loan.clone(),
        kind: kind.to_string(),
        event_day: day(rng),
        amount: (kind != "rate_change").then(|| format!("{}000", rng.random_range(1..90))),
        rate: (kind == "rate_change")
            .then(|| format!("{}.{}", rng.random_range(1..7), rng.random_range(0..10))),
        monthly_payment: (kind == "rate_change" && rng.random_bool(0.5))
            .then(|| format!("{}00", rng.random_range(50..200))),
        note: None,
    };
    run(format!("{loan} {kind} on {}", data.event_day), || {
        data.validate()?;
        loan_events::add_event(conn, &data, now() - now() % super::DAY).map(drop)
    })
}

fn delete_event(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, "SELECT id FROM loan_events ORDER BY rowid") else {
        return no_target();
    };
    run(id.clone(), || {
        loan_events::delete_event(conn, &id).map(drop)
    })
}

fn property_data(rng: &mut StdRng) -> InsertRealEstate {
    let market = rng.random_bool(0.5);
    InsertRealEstate {
        name: one_of(rng, &["Flat", "Cottage"]).to_string(),
        address: "Main street 1".into(),
        property_type: one_of(rng, &["personal", "investment"]).to_string(),
        purchase_price: Some(format!("{}00000", rng.random_range(10..90))),
        purchase_price_currency: Some("CZK".into()),
        purchase_date: Some(day(rng)),
        market_price: market.then(|| format!("{}00000", rng.random_range(10..90))),
        market_price_currency: market.then(|| currency(rng).to_string()),
        market_price_date: market.then(|| day(rng)),
        monthly_rent: rng.random_bool(0.3).then(|| money(rng)),
        monthly_rent_currency: Some("CZK".into()),
        recurring_costs: None,
        photos: None,
        notes: None,
    }
}

fn create_property(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = property_data(rng);
    run(format!("{:?} {:?}", data.name, data.market_price), || {
        data.validate()?;
        real_estate::create_property(conn, &data).map(drop)
    })
}

fn update_property(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, PROPERTIES) else {
        return no_target();
    };
    let data = property_data(rng);
    run(
        format!("{id} -> {:?} {:?}", data.name, data.market_price),
        || {
            data.validate()?;
            real_estate::update_property(conn, &id, &data, now())
        },
    )
}

fn valuation_target(conn: &Connection, rng: &mut StdRng) -> Option<(ValuationKind, String)> {
    if rng.random_bool(0.5) {
        pick(conn, rng, PROPERTIES).map(|id| (ValuationKind::RealEstate, id))
    } else {
        pick(conn, rng, OTHER_ASSETS).map(|id| (ValuationKind::OtherAsset, id))
    }
}

fn add_valuation(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some((kind, asset)) = valuation_target(conn, rng) else {
        return no_target();
    };
    let data = InsertAssetValuation {
        asset_id: asset.clone(),
        value: format!("{}000", rng.random_range(10..900)),
        currency: Some(currency(rng).to_string()),
        valued_at: day(rng),
        note: None,
    };
    run(
        format!("{kind:?} {asset} {} on {}", data.value, data.valued_at),
        || {
            data.validate()?;
            valuations::add_valuation(conn, kind, &data).map(drop)
        },
    )
}

fn delete_valuation(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let target = if rng.random_bool(0.5) {
        pick(
            conn,
            rng,
            "SELECT id FROM real_estate_valuations ORDER BY rowid",
        )
        .map(|id| (ValuationKind::RealEstate, id))
    } else {
        pick(
            conn,
            rng,
            "SELECT id FROM other_asset_valuations ORDER BY rowid",
        )
        .map(|id| (ValuationKind::OtherAsset, id))
    };
    let Some((kind, id)) = target else {
        return no_target();
    };
    run(format!("{kind:?} {id}"), || {
        valuations::delete_valuation(conn, kind, &id).map(drop)
    })
}

fn asset_trade_data(
    rng: &mut StdRng,
    asset: Option<String>,
    tx_type: &str,
) -> InsertOtherAssetTransaction {
    InsertOtherAssetTransaction {
        asset_id: asset,
        tx_type: tx_type.to_string(),
        quantity: rng.random_range(1..10).to_string(),
        price_per_unit: money(rng),
        currency: currency(rng).to_string(),
        transaction_date: day(rng),
    }
}

fn create_asset(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = InsertOtherAsset {
        name: one_of(rng, &["Gold", "Watch", "Wine"]).to_string(),
        quantity: Some(rng.random_range(1..5).to_string()),
        market_price: Some(money(rng)),
        currency: Some(currency(rng).to_string()),
        average_purchase_price: Some(money(rng)),
        yield_type: None,
        yield_value: None,
    };
    let initial = rng
        .random_bool(0.5)
        .then(|| asset_trade_data(rng, None, "buy"));
    run(
        format!("{:?} with trade {}", data.name, initial.is_some()),
        || {
            data.validate()?;
            if let Some(trade) = &initial {
                trade.validate()?;
            }
            other_assets::create_asset(conn, &data, initial.as_ref()).map(drop)
        },
    )
}

fn asset_trade(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(asset) = pick(conn, rng, OTHER_ASSETS) else {
        return no_target();
    };
    let side = one_of(rng, &["buy", "buy", "sell"]);
    let data = asset_trade_data(rng, Some(asset.clone()), side);
    run(
        format!("{asset} {side} {} {}", data.quantity, data.currency),
        || {
            data.validate()?;
            other_assets::create_asset_transaction(conn, &asset, &data).map(drop)
        },
    )
}

fn bond_data(rng: &mut StdRng) -> InsertBond {
    InsertBond {
        name: one_of(rng, &["Gov 2030", "Corp 2028"]).to_string(),
        isin: None,
        coupon_value: format!("{}000", rng.random_range(1..90)),
        quantity: Some(rng.random_range(1..10).to_string()),
        currency: Some(currency(rng).to_string()),
        interest_rate: Some(format!("{}.5", rng.random_range(1..6))),
        maturity_date: Some(super::DAY0 + 1_500 * super::DAY),
        purchase_date: Some(day(rng)),
    }
}

fn create_bond(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let data = bond_data(rng);
    run(format!("{:?} {}", data.name, data.coupon_value), || {
        data.validate()?;
        bonds::create_bond(conn, &data).map(drop)
    })
}

fn update_bond(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, BONDS) else {
        return no_target();
    };
    let data = bond_data(rng);
    run(
        format!("{id} -> {:?} {}", data.name, data.coupon_value),
        || {
            data.validate()?;
            bonds::update_bond(conn, &id, &data).map(drop)
        },
    )
}

fn delete_bond(conn: &Connection, rng: &mut StdRng) -> (String, Outcome) {
    let Some(id) = pick(conn, rng, BONDS) else {
        return no_target();
    };
    run(id.clone(), || bonds::delete_bond(conn, &id))
}
