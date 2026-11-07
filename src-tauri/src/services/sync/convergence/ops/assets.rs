//! Asset operations: loans and their events, real estate with its one-time costs and
//! valuations, insurance policies, other assets and their trades, bonds.

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{by_content, currency, day, money, no_target, now, one_of, pick, run, OpFn, Res};
use crate::error::Result;
use crate::models::bonds::InsertBond;
use crate::models::insurance::InsertInsurancePolicy;
use crate::models::loan_events::InsertLoanEvent;
use crate::models::loans::InsertLoan;
use crate::models::other_assets::{InsertOtherAsset, InsertOtherAssetTransaction};
use crate::models::real_estate::{InsertRealEstate, InsertRealEstateOneTimeCost};
use crate::models::valuations::InsertAssetValuation;
use crate::services::valuations::{self, ValuationKind};
use crate::services::{bonds, insurance, loan_events, loans, other_assets, real_estate};

pub(super) const OPS: &[(&str, OpFn)] = &[
    ("loans::create_loan", create_loan),
    ("loans::update_loan", update_loan),
    ("loan_events::add_event", add_event),
    ("loan_events::delete_event", delete_event),
    ("real_estate::create_property", create_property),
    ("real_estate::update_property", update_property),
    ("real_estate::delete_property", delete_property),
    ("real_estate::create_one_time_cost", create_cost),
    ("real_estate::update_one_time_cost", update_cost),
    ("valuations::add_valuation", add_valuation),
    ("valuations::delete_valuation", delete_valuation),
    ("insurance::create_policy", create_policy),
    ("insurance::update_policy", update_policy),
    ("insurance::delete_policy", delete_policy),
    ("other_assets::create_asset", create_asset),
    ("other_assets::create_asset_transaction", asset_trade),
    ("bonds::create_bond", create_bond),
    ("bonds::update_bond", update_bond),
    ("bonds::delete_bond", delete_bond),
];

fn loans() -> String {
    by_content("loans", "t.id", "", "t.name, t.principal, t.currency")
}

fn properties() -> String {
    by_content(
        "real_estate",
        "t.id",
        "",
        "t.name, t.purchase_date, t.purchase_price",
    )
}

fn other_assets() -> String {
    by_content("other_assets", "t.id", "", "t.name, t.currency")
}

fn bonds() -> String {
    by_content(
        "bonds",
        "t.id",
        "",
        "t.name, t.coupon_value, t.purchase_date",
    )
}

fn policies() -> String {
    by_content(
        "insurance_policies",
        "t.id",
        "",
        "t.policy_name, t.provider, t.start_date",
    )
}

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

fn create_loan(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = loan_data(rng);
    run(format!("{:?} {}", data.name, data.principal), || {
        data.validate()?;
        loans::create_loan(conn, &data).map(drop)
    })
}

fn update_loan(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &loans())? else {
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

fn add_event(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(loan) = pick(conn, rng, &loans())? else {
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

fn delete_event(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "loan_events",
        "t.id",
        "",
        "t.event_day, t.kind, t.amount, t.rate",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
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

fn create_property(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = property_data(rng);
    run(format!("{:?} {:?}", data.name, data.market_price), || {
        data.validate()?;
        real_estate::create_property(conn, &data).map(drop)
    })
}

fn update_property(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &properties())? else {
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

/// Deleting a property takes its costs, valuations and links with it.
fn delete_property(conn: &mut Connection, rng: &mut StdRng, dir: &Path) -> Res {
    let Some(id) = pick(conn, rng, &properties())? else {
        return no_target();
    };
    run(id.clone(), || real_estate::delete_property(conn, dir, &id))
}

fn cost_data(rng: &mut StdRng, property: &str) -> InsertRealEstateOneTimeCost {
    InsertRealEstateOneTimeCost {
        real_estate_id: property.to_string(),
        name: one_of(rng, &["Roof", "Kitchen"]).to_string(),
        description: None,
        amount: format!("{}000", rng.random_range(5..90)),
        currency: Some(currency(rng).to_string()),
        date: day(rng),
    }
}

fn create_cost(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(property) = pick(conn, rng, &properties())? else {
        return no_target();
    };
    let data = cost_data(rng, &property);
    run(
        format!("{property} {:?} {}", data.name, data.amount),
        || real_estate::create_one_time_cost(conn, data.clone()).map(drop),
    )
}

fn update_cost(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let sql = by_content(
        "real_estate_one_time_costs",
        "t.id, t.real_estate_id",
        "",
        "t.date, t.name, t.amount",
    );
    let Some((id, property)) = super::pick2(conn, rng, &sql)? else {
        return no_target();
    };
    let data = cost_data(rng, &property);
    run(format!("{id} -> {:?} {}", data.name, data.amount), || {
        real_estate::update_one_time_cost(conn, &id, data.clone()).map(drop)
    })
}

fn valuation_target(
    conn: &Connection,
    rng: &mut StdRng,
) -> Result<Option<(ValuationKind, String)>> {
    Ok(if rng.random_bool(0.5) {
        pick(conn, rng, &properties())?.map(|id| (ValuationKind::RealEstate, id))
    } else {
        pick(conn, rng, &other_assets())?.map(|id| (ValuationKind::OtherAsset, id))
    })
}

fn add_valuation(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some((kind, asset)) = valuation_target(conn, rng)? else {
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

fn delete_valuation(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let content = "t.valued_at, t.value, t.currency";
    let target = if rng.random_bool(0.5) {
        pick(
            conn,
            rng,
            &by_content("real_estate_valuations", "t.id", "", content),
        )?
        .map(|id| (ValuationKind::RealEstate, id))
    } else {
        pick(
            conn,
            rng,
            &by_content("other_asset_valuations", "t.id", "", content),
        )?
        .map(|id| (ValuationKind::OtherAsset, id))
    };
    let Some((kind, id)) = target else {
        return no_target();
    };
    run(format!("{kind:?} {id}"), || {
        valuations::delete_valuation(conn, kind, &id).map(drop)
    })
}

fn policy_data(rng: &mut StdRng) -> InsertInsurancePolicy {
    InsertInsurancePolicy {
        policy_type: one_of(rng, &["life", "property", "travel"]).to_string(),
        provider: one_of(rng, &["Allianz", "Kooperativa"]).to_string(),
        policy_name: one_of(rng, &["Home", "Life", "Trip"]).to_string(),
        policy_number: None,
        start_date: day(rng),
        end_date: rng.random_bool(0.5).then(|| day(rng) + 400 * super::DAY),
        payment_frequency: one_of(rng, &["monthly", "annually"]).to_string(),
        one_time_payment: None,
        one_time_payment_currency: None,
        regular_payment: Some(format!("{}00", rng.random_range(1..30))),
        regular_payment_currency: Some(currency(rng).to_string()),
        limits: None,
        notes: None,
        status: None,
    }
}

fn create_policy(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = policy_data(rng);
    run(
        format!(
            "{:?} {:?} {}",
            data.policy_name, data.provider, data.start_date
        ),
        || insurance::create_policy(conn, &data).map(drop),
    )
}

fn update_policy(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &policies())? else {
        return no_target();
    };
    let data = policy_data(rng);
    run(
        format!("{id} -> {:?} {:?}", data.policy_name, data.regular_payment),
        || insurance::update_policy(conn, &id, &data).map(drop),
    )
}

fn delete_policy(conn: &mut Connection, rng: &mut StdRng, dir: &Path) -> Res {
    let Some(id) = pick(conn, rng, &policies())? else {
        return no_target();
    };
    run(id.clone(), || insurance::delete_policy(conn, dir, &id))
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

fn create_asset(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
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

fn asset_trade(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(asset) = pick(conn, rng, &other_assets())? else {
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

fn create_bond(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let data = bond_data(rng);
    run(format!("{:?} {}", data.name, data.coupon_value), || {
        data.validate()?;
        bonds::create_bond(conn, &data).map(drop)
    })
}

fn update_bond(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &bonds())? else {
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

fn delete_bond(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(id) = pick(conn, rng, &bonds())? else {
        return no_target();
    };
    run(id.clone(), || bonds::delete_bond(conn, &id))
}
