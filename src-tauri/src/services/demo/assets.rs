//! Alex's property, loans, insurance and other assets: the flat (with the mortgage, the home
//! policy and the kitchen) and the parking space, the kitchen loan, five policies, the car and
//! two gold coins, plus a purchase contract and a policy document when attachments are on.
//!
//! Every past value is a dated fact (purchase, valuation, loan start, extra repayment), so the
//! app derives the history from it.

use super::clock;
use super::schedule::*;
use super::text::{t, T};
use super::{DemoOptions, DemoSummary};
use crate::error::Result;
use crate::models::{
    InsertAssetValuation, InsertInsurancePolicy, InsertLoan, InsertLoanEvent, InsertOtherAsset,
    InsertOtherAssetTransaction, InsertRealEstate, InsertRealEstateOneTimeCost, InsuranceLimit,
    RecurringCost,
};
use crate::services::attachments::{self, NewDocument};
use crate::services::valuations::{self, ValuationKind};
use crate::services::{insurance, loan_events, loans, milestones, other_assets, real_estate};
use chrono::{Datelike, NaiveDate};
use rusqlite::{params, Connection};

/// Address of the flat and the parking space.
const ADDRESS: &str = "Lindenstraße 14";
/// Insurer of every policy.
const INSURER: &str = "Aurora Insurance";
/// The car (a model name, not translated).
const CAR_NAME: &str = "Volkswagen Golf (2019)";

/// The flat is bought (and the mortgage paid out) at `M60` on this day.
pub const FLAT_BOUGHT_M: i32 = 60;
pub const FLAT_DAY: u32 = 15;
const FLAT_PRICE: &str = "248000";
/// Later flat valuations: (Mn, EUR).
const FLAT_VALUATIONS: [(i32, &str); 5] = [
    (48, "255000"),
    (36, "268000"),
    (24, "284000"),
    (12, "299000"),
    (2, "312000"),
];
/// Day of the month of a valuation.
const VALUATION_DAY: u32 = 10;
/// The parking space's later valuation. `M11` rather than `M12`: a valuation more than 365
/// days old raises the "valuation stale" reminder.
const PARKING_VALUATION: (i32, &str) = (11, "23000");
const KITCHEN_COST: &str = "10400";

const MORTGAGE_PRINCIPAL: &str = "165000";
const MORTGAGE_RATE: &str = "3.1";
const MORTGAGE_YEARS: i32 = 25;
/// The fixed rate ends this many months after today.
const MORTGAGE_FIXED_AHEAD_M: i32 = 14;
const KITCHEN_PRINCIPAL: &str = "9000";
const KITCHEN_RATE: &str = "6.4";
const KITCHEN_MONTHS: i32 = 48;

/// Other-asset valuations (price per unit): (Mn, EUR).
const CAR_VALUATIONS: [(i32, &str); 3] = [(28, "16200"), (16, "14800"), (3, "13600")];
const GOLD_VALUATIONS: [(i32, &str); 2] = [(12, "3050"), (2, "3480")];

/// Policy start months (same months and days the bank pays the premiums).
const HOME_PREMIUM: (u32, u32) = (2, 12);
const LIABILITY_PREMIUM: (u32, u32) = (3, 14);
const TRAVEL_PREMIUM: (u32, u32) = (6, 3);
const LIFE_START_M: i32 = 52;
const LIFE_DAY: u32 = 21;

fn money(value: f64) -> String {
    format!("{value:.2}")
}

/// UTC midnight of `(year, month, day)`, the day clamped to the month.
fn ymd(year: i32, month: u32, day: u32) -> i64 {
    let dom = day.min(clock::days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, dom).map_or(0, clock::day_of)
}

/// The first `(month, day)` on or after `day`.
fn first_on_or_after(day: i64, (month, dom): (u32, u32)) -> i64 {
    let year = clock::date_of(day).year();
    let this = ymd(year, month, dom);
    if this >= day {
        this
    } else {
        ymd(year + 1, month, dom)
    }
}

fn valuation(
    conn: &Connection,
    kind: ValuationKind,
    id: &str,
    value: &str,
    day: i64,
    note: Option<&str>,
) -> Result<()> {
    valuations::add_valuation(
        conn,
        kind,
        &InsertAssetValuation {
            asset_id: id.to_string(),
            value: value.to_string(),
            currency: Some("EUR".to_string()),
            valued_at: day,
            note: note.map(str::to_string),
        },
    )?;
    Ok(())
}

/// A policy to create: (type, name, start, frequency, premium, limits, serial number).
type Policy = (
    &'static str,
    T,
    i64,
    &'static str,
    f64,
    Vec<InsuranceLimit>,
    u32,
);
/// An other asset: (name, quantity, price per unit, purchase day, later valuations).
type Holding = (&'static str, f64, f64, i64, &'static [(i32, &'static str)]);

/// What [`seed`] created that later steps and tests refer to.
#[derive(Debug, Clone, Default)]
pub struct Created {
    pub flat_id: String,
    pub parking_id: String,
    pub mortgage_id: String,
    pub kitchen_loan_id: String,
    pub home_policy_id: String,
}

/// Property, loans, insurance, other assets and (with `attachments`) documents.
pub fn seed(conn: &Connection, opts: &DemoOptions, summary: &mut DemoSummary) -> Result<()> {
    let lang = opts.language.as_str();
    let now = opts.now;
    let today = clock::today(now);
    let m = |n: i32, dom: u32| clock::months_ago(now, n, dom);
    let mut created = Created::default();

    // The flat: bought at M60, five later estimates, two recurring costs, the kitchen
    let flat_bought = m(FLAT_BOUGHT_M, FLAT_DAY);
    created.flat_id = real_estate::create_property(
        conn,
        &InsertRealEstate {
            name: t(lang, T::PropFlat).to_string(),
            address: ADDRESS.to_string(),
            property_type: "personal".to_string(),
            purchase_price: Some(FLAT_PRICE.to_string()),
            purchase_price_currency: Some("EUR".to_string()),
            purchase_date: Some(flat_bought),
            market_price: Some(FLAT_PRICE.to_string()),
            market_price_currency: Some("EUR".to_string()),
            market_price_date: Some(flat_bought),
            monthly_rent: None,
            monthly_rent_currency: None,
            recurring_costs: Some(vec![
                RecurringCost {
                    name: t(lang, T::BuildingFee).to_string(),
                    amount: 185.0,
                    frequency: "monthly".to_string(),
                    currency: Some("EUR".to_string()),
                },
                RecurringCost {
                    name: t(lang, T::PropertyTax).to_string(),
                    amount: 96.0,
                    frequency: "yearly".to_string(),
                    currency: Some("EUR".to_string()),
                },
            ]),
            photos: None,
            notes: Some(t(lang, T::FlatNote).to_string()),
        },
    )?;
    for (n, value) in FLAT_VALUATIONS {
        valuation(
            conn,
            ValuationKind::RealEstate,
            &created.flat_id,
            value,
            m(n, VALUATION_DAY),
            Some(t(lang, T::ValuationNote)),
        )?;
    }
    real_estate::create_one_time_cost(
        conn,
        InsertRealEstateOneTimeCost {
            real_estate_id: created.flat_id.clone(),
            name: t(lang, T::CostKitchen).to_string(),
            description: None,
            amount: KITCHEN_COST.to_string(),
            currency: Some("EUR".to_string()),
            date: m(KITCHEN_M, KITCHEN_DAY),
        },
    )?;

    // The parking space: an investment let to a neighbour
    let parking_bought = m(PARKING_BOUGHT_M, PARKING_DAY);
    created.parking_id = real_estate::create_property(
        conn,
        &InsertRealEstate {
            name: t(lang, T::PropParking).to_string(),
            address: ADDRESS.to_string(),
            property_type: "investment".to_string(),
            purchase_price: Some(money(PARKING_PRICE_EUR)),
            purchase_price_currency: Some("EUR".to_string()),
            purchase_date: Some(parking_bought),
            market_price: Some(money(PARKING_PRICE_EUR)),
            market_price_currency: Some("EUR".to_string()),
            market_price_date: Some(parking_bought),
            monthly_rent: Some(money(PARKING_RENT_EUR)),
            monthly_rent_currency: Some("EUR".to_string()),
            recurring_costs: Some(vec![RecurringCost {
                name: t(lang, T::BuildingFee).to_string(),
                amount: 12.0,
                frequency: "monthly".to_string(),
                currency: Some("EUR".to_string()),
            }]),
            photos: None,
            notes: Some(t(lang, T::ParkingNote).to_string()),
        },
    )?;
    let (n, value) = PARKING_VALUATION;
    valuation(
        conn,
        ValuationKind::RealEstate,
        &created.parking_id,
        value,
        m(n, 20),
        Some(t(lang, T::ValuationNote)),
    )?;

    seed_loans(conn, opts, &mut created)?;
    seed_insurance(conn, opts, &mut created)?;
    seed_other_assets(conn, opts)?;

    if opts.attachments {
        let doc = |name: T| NewDocument {
            name: t(lang, name),
            description: None,
            file_type: "contract",
        };
        attachments::add_real_estate_document_bytes(
            conn,
            &created.flat_id,
            "purchase-contract.pdf",
            &demo_pdf(),
            &doc(T::DocPurchaseContract),
            now,
        )?;
        attachments::add_insurance_document_bytes(
            conn,
            &created.home_policy_id,
            "home-insurance-policy.pdf",
            &demo_pdf(),
            &doc(T::DocPolicy),
            now,
        )?;
        summary.counts.insert("documents", 2);
    }

    // Neither loan has had a manual balance record (that would stop the amortization), so the
    // yearly "confirm the balance" reminder is marked done, as the user would.
    for item in milestones::list_milestones(conn, today, None)? {
        if item.kind == "loan_balance_check" {
            milestones::set_milestone_state(conn, &item.key, "done", today)?;
        }
    }

    summary.counts.insert("real_estate", 2);
    summary
        .counts
        .insert("real_estate_valuations", FLAT_VALUATIONS.len() + 3);
    summary.counts.insert("loans", 2);
    summary.counts.insert("insurance_policies", 5);
    summary.counts.insert("other_assets", 2);
    summary.counts.insert(
        "other_asset_valuations",
        CAR_VALUATIONS.len() + GOLD_VALUATIONS.len() + 2,
    );
    Ok(())
}

/// The mortgage (linked to the flat, one extra repayment) and the kitchen loan.
fn seed_loans(conn: &Connection, opts: &DemoOptions, created: &mut Created) -> Result<()> {
    let now = opts.now;
    let lang = opts.language.as_str();
    let today = clock::today(now);
    let today_dom = clock::date_of(today).day();
    let m = |n: i32, dom: u32| clock::months_ago(now, n, dom);
    let loan = |name: T,
                principal: &str,
                rate: &str,
                payment: f64,
                start: i64,
                end: i64,
                fixed_until: Option<i64>|
     -> InsertLoan {
        InsertLoan {
            name: t(lang, name).to_string(),
            principal: principal.to_string(),
            currency: Some("EUR".to_string()),
            interest_rate: Some(rate.to_string()),
            interest_rate_validity_date: fixed_until,
            monthly_payment: Some(money(payment)),
            start_date: Some(start),
            end_date: Some(end),
            balance_anchor_amount: None,
            balance_anchor_date: None,
        }
    };

    let mortgage = loans::create_loan(
        conn,
        &loan(
            T::LoanMortgage,
            MORTGAGE_PRINCIPAL,
            MORTGAGE_RATE,
            MORTGAGE_PAYMENT,
            m(FLAT_BOUGHT_M, FLAT_DAY),
            m(FLAT_BOUGHT_M - MORTGAGE_YEARS * 12, FLAT_DAY),
            Some(m(-MORTGAGE_FIXED_AHEAD_M, today_dom)),
        ),
    )?;
    loan_events::add_event(
        conn,
        &InsertLoanEvent {
            loan_id: mortgage.id.clone(),
            kind: "extra_payment".to_string(),
            event_day: m(EXTRA_MORTGAGE_M, EXTRA_MORTGAGE_DAY),
            amount: Some(money(EXTRA_MORTGAGE_EUR)),
            rate: None,
            monthly_payment: None,
            note: None,
        },
        today,
    )?;
    conn.execute(
        "INSERT INTO real_estate_loans (real_estate_id, loan_id) VALUES (?1, ?2)",
        params![created.flat_id, mortgage.id],
    )?;
    created.mortgage_id = mortgage.id;

    let kitchen = loans::create_loan(
        conn,
        &loan(
            T::LoanKitchen,
            KITCHEN_PRINCIPAL,
            KITCHEN_RATE,
            KITCHEN_PAYMENT,
            m(KITCHEN_M, KITCHEN_DAY),
            m(KITCHEN_M - KITCHEN_MONTHS, KITCHEN_DAY),
            None,
        ),
    )?;
    created.kitchen_loan_id = kitchen.id;
    Ok(())
}

/// Five policies with Aurora Insurance; the home policy is linked to the flat.
fn seed_insurance(conn: &Connection, opts: &DemoOptions, created: &mut Created) -> Result<()> {
    let now = opts.now;
    let lang = opts.language.as_str();
    let year = clock::date_of(clock::today(now)).year();
    let flat_bought = clock::months_ago(now, FLAT_BOUGHT_M, FLAT_DAY);
    let limit = |title: T, amount: f64| InsuranceLimit {
        title: t(lang, title).to_string(),
        amount,
        currency: "EUR".to_string(),
    };
    // (type, name, start, frequency, premium, limits, serial)
    let policies: [Policy; 5] = [
        (
            "property",
            T::PolHome,
            first_on_or_after(flat_bought, HOME_PREMIUM),
            "annually",
            168.0,
            vec![
                limit(T::LimContents, 60_000.0),
                limit(T::LimWater, 25_000.0),
            ],
            4_817,
        ),
        (
            "life",
            T::PolLife,
            clock::months_ago(now, LIFE_START_M, LIFE_DAY),
            "monthly",
            31.50,
            vec![limit(T::LimDeath, 150_000.0)],
            11_352,
        ),
        (
            "vehicle",
            T::PolCar,
            car_policy_start(now),
            "annually",
            612.0,
            vec![
                limit(T::LimLiability, 5_000_000.0),
                limit(T::LimOwnDamage, 14_000.0),
            ],
            20_471,
        ),
        (
            "liability",
            T::PolLiability,
            ymd(year - 4, LIABILITY_PREMIUM.0, LIABILITY_PREMIUM.1),
            "annually",
            62.0,
            vec![limit(T::LimLiability, 2_000_000.0)],
            7_709,
        ),
        (
            "travel",
            T::PolTravel,
            ymd(year - 3, TRAVEL_PREMIUM.0, TRAVEL_PREMIUM.1),
            "annually",
            54.0,
            vec![limit(T::LimMedical, 300_000.0)],
            15_236,
        ),
    ];
    for (kind, name, start, frequency, premium, limits, serial) in policies {
        let policy = insurance::create_policy(
            conn,
            &InsertInsurancePolicy {
                policy_type: kind.to_string(),
                provider: INSURER.to_string(),
                policy_name: t(lang, name).to_string(),
                policy_number: Some(format!("AUR-{}-{serial:06}", clock::date_of(start).year())),
                start_date: start,
                end_date: None,
                payment_frequency: frequency.to_string(),
                one_time_payment: None,
                one_time_payment_currency: None,
                regular_payment: Some(money(premium)),
                regular_payment_currency: Some("EUR".to_string()),
                limits: Some(limits),
                notes: None,
                status: None,
            },
        )?;
        if name == T::PolHome {
            conn.execute(
                "INSERT INTO real_estate_insurances (real_estate_id, insurance_id) VALUES (?1, ?2)",
                params![created.flat_id, policy.id],
            )?;
            created.home_policy_id = policy.id;
        }
    }
    Ok(())
}

/// The car (losing value) and two gold coins (gaining), each bought on its day with dated
/// valuations only: no valuation is dated today and the newest one is the market price.
fn seed_other_assets(conn: &Connection, opts: &DemoOptions) -> Result<()> {
    let now = opts.now;
    let lang = opts.language.as_str();
    let m = |n: i32, dom: u32| clock::months_ago(now, n, dom);
    let assets: [Holding; 2] = [
        (
            CAR_NAME,
            1.0,
            CAR_PRICE_EUR,
            m(CAR_BOUGHT_M, CAR_DAY),
            &CAR_VALUATIONS,
        ),
        (
            t(lang, T::AssetGold),
            2.0,
            GOLD_COIN_EUR,
            m(GOLD_BOUGHT_M, GOLD_DAY),
            &GOLD_VALUATIONS,
        ),
    ];
    for (name, quantity, unit_price, bought, later) in assets {
        // No market price on create: the initial valuation would be dated today
        let asset = other_assets::create_asset(
            conn,
            &InsertOtherAsset {
                name: name.to_string(),
                quantity: None,
                market_price: None,
                currency: Some("EUR".to_string()),
                average_purchase_price: None,
                yield_type: None,
                yield_value: None,
            },
            Some(&InsertOtherAssetTransaction {
                asset_id: None,
                tx_type: "buy".to_string(),
                quantity: format!("{quantity}"),
                price_per_unit: money(unit_price),
                currency: "EUR".to_string(),
                transaction_date: bought,
            }),
        )?;
        valuation(
            conn,
            ValuationKind::OtherAsset,
            &asset.id,
            &money(unit_price),
            bought,
            None,
        )?;
        for (n, value) in later {
            valuation(
                conn,
                ValuationKind::OtherAsset,
                &asset.id,
                value,
                m(*n, VALUATION_DAY),
                None,
            )?;
        }
    }
    Ok(())
}

/// A one-page PDF reading "Moony demo - sample document" (the approach of
/// `bin/seed_demo.rs::demo_pdf`).
pub fn demo_pdf() -> Vec<u8> {
    let content = "BT /F1 24 Tf 72 720 Td (Moony demo - sample document) Tj ET\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}
