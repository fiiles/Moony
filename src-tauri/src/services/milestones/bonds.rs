//! Bond milestones: maturity (the principal comes back) and the yearly coupon, paid on the
//! maturity's anniversary (as on the bonds page). A coupon covers the year since the previous
//! anniversary, but only from the purchase day when the bond was bought inside that year.

use chrono::{DateTime, Datelike, NaiveDate};
use rusqlite::Connection;

use super::{dated, money_text, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;

const MATURITY_LEAD_DAYS: i64 = 30;
const COUPON_LEAD_DAYS: i64 = 7;

struct BondRow {
    id: String,
    name: String,
    coupon_value: String,
    quantity: String,
    currency: String,
    rate: String,
    maturity: i64,
    purchase: Option<i64>,
}

/// `day` moved to `year`; 29 February becomes 28 February in a common year.
fn in_year(day: i64, year: i32) -> Option<i64> {
    let date = DateTime::from_timestamp(day, 0)?.date_naive();
    let shifted = NaiveDate::from_ymd_opt(year, date.month(), date.day())
        .or_else(|| NaiveDate::from_ymd_opt(year, date.month(), date.day() - 1))?;
    Some(shifted.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

/// The share of a full yearly coupon paid on `due` (an anniversary of the maturity, or the
/// maturity itself): the days held of the coupon year that ends on `due`. A bond bought before
/// that year began, or with no purchase day, gets the full coupon (`1.0`).
pub(super) fn coupon_share(maturity: i64, due: i64, purchase: Option<i64>) -> f64 {
    let Some(purchase) = purchase.map(day_floor) else {
        return 1.0;
    };
    let previous =
        DateTime::from_timestamp(due, 0).and_then(|d| in_year(maturity, d.date_naive().year() - 1));
    let Some(previous) = previous else {
        return 1.0;
    };
    if purchase <= previous {
        1.0
    } else if purchase >= due {
        0.0
    } else {
        (due - purchase) as f64 / (due - previous) as f64
    }
}

/// Next yearly anniversary of the maturity strictly after `today` and strictly before the
/// maturity (the last coupon is paid with the principal).
pub(super) fn next_coupon_day(maturity: i64, today: i64) -> Option<i64> {
    let year = DateTime::from_timestamp(today, 0)?.date_naive().year();
    let this_year = in_year(maturity, year)?;
    let next = if this_year > today {
        this_year
    } else {
        in_year(maturity, year + 1)?
    };
    (next < maturity).then_some(next)
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, coupon_value, quantity, currency, interest_rate, maturity_date,
                purchase_date
         FROM bonds WHERE maturity_date IS NOT NULL",
    )?;
    let bonds = stmt
        .query_map([], |r| {
            Ok(BondRow {
                id: r.get(0)?,
                name: r.get(1)?,
                coupon_value: r.get(2)?,
                quantity: r.get(3)?,
                currency: r.get(4)?,
                rate: r.get(5)?,
                maturity: r.get(6)?,
                purchase: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for b in bonds {
        let maturity = day_floor(b.maturity);
        if maturity < today {
            continue;
        }
        let value: f64 = b.coupon_value.trim().parse().unwrap_or(0.0);
        let quantity: f64 = b.quantity.trim().parse().unwrap_or(1.0);
        let face = value * quantity;
        let rate: f64 = b.rate.trim().parse().unwrap_or(0.0);
        let yearly = if rate > 0.0 { face * rate / 100.0 } else { 0.0 };
        // The final coupon is paid with the principal (as in the bonds page ladder); a bond
        // bought within its last coupon year earns only the days held.
        let final_coupon = yearly * coupon_share(maturity, maturity, b.purchase);
        let remind_from = maturity - MATURITY_LEAD_DAYS * DAY;
        if let Some(mut m) = dated(
            "bond_maturity",
            &b.id,
            &b.name,
            maturity,
            remind_from,
            today,
        ) {
            m.amount = Some(money_text(face + final_coupon));
            m.currency = Some(b.currency.clone());
            out.push(m);
        }
        if rate > 0.0 && face > 0.0 {
            if let Some(coupon) = next_coupon_day(maturity, today) {
                let remind_from = coupon - COUPON_LEAD_DAYS * DAY;
                if let Some(mut m) =
                    dated("bond_coupon", &b.id, &b.name, coupon, remind_from, today)
                {
                    m.tone = TONE_INFO.to_string();
                    m.amount = Some(money_text(
                        yearly * coupon_share(maturity, coupon, b.purchase),
                    ));
                    m.currency = Some(b.currency.clone());
                    out.push(m);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::super::{STAGE_NOW, TONE_INFO};
    use super::*;
    use rusqlite::params;

    fn bond(
        conn: &Connection,
        id: &str,
        value: &str,
        qty: &str,
        rate: &str,
        maturity: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO bonds (id, name, coupon_value, quantity, interest_rate, maturity_date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Dluhopis {id}"), value, qty, rate, maturity],
        )
        .expect("bond");
    }

    #[test]
    fn a_maturity_reminds_a_month_ahead_with_the_face_value() {
        let conn = setup();
        bond(&conn, "b", "10000", "10", "0", Some(day(2026, 11, 1)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "bond_maturity");
        assert_eq!(list[0].stage, STAGE_NOW);
        assert_eq!(list[0].amount.as_deref(), Some("100000.00"));
        assert_eq!(list[0].currency.as_deref(), Some("CZK"));
    }

    #[test]
    fn a_maturity_amount_includes_the_final_coupon() {
        let conn = setup();
        // 100 000 face, 4.5 % -> 4 500 final coupon paid with the principal.
        bond(&conn, "b", "10000", "10", "4.5", Some(day(2026, 11, 1)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let maturity = list
            .iter()
            .find(|m| m.kind == "bond_maturity")
            .expect("maturity");
        assert_eq!(maturity.amount.as_deref(), Some("104500.00"));
    }

    fn bought(conn: &Connection, id: &str, purchase: i64) {
        conn.execute(
            "UPDATE bonds SET purchase_date = ?2 WHERE id = ?1",
            params![id, purchase],
        )
        .expect("purchase");
    }

    #[test]
    fn a_short_bond_returns_only_the_interest_for_the_days_held() {
        let conn = setup();
        // 1 700 000 at 3.75 % bought 2026-10-05, maturing 2027-01-15: 102 of 365 days.
        bond(&conn, "b", "1", "1700000", "3.75", Some(day(2027, 1, 15)));
        bought(&conn, "b", day(2026, 10, 5));
        let list = build(&conn, day(2026, 12, 20)).unwrap();
        let maturity = list
            .iter()
            .find(|m| m.kind == "bond_maturity")
            .expect("maturity");
        assert_eq!(maturity.amount.as_deref(), Some("1717815.07"));
    }

    #[test]
    fn a_bond_bought_before_the_coupon_year_pays_the_full_coupon() {
        let conn = setup();
        bond(&conn, "b", "10000", "10", "4.5", Some(day(2026, 11, 1)));
        bought(&conn, "b", day(2025, 6, 1));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(list[0].amount.as_deref(), Some("104500.00"));
    }

    #[test]
    fn the_first_coupon_after_a_purchase_pays_only_the_days_held() {
        let conn = setup();
        // Coupon year 2025-10-09 → 2026-10-09 (365 days), bought 2026-04-09: 183 days held.
        bond(&conn, "b", "10000", "10", "4.5", Some(day(2030, 10, 9)));
        bought(&conn, "b", day(2026, 4, 9));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let coupon = list
            .iter()
            .find(|m| m.kind == "bond_coupon")
            .expect("coupon");
        assert_eq!(coupon.amount.as_deref(), Some("2256.16"));
    }

    #[test]
    fn a_coupon_falls_on_the_maturity_anniversary_and_is_info() {
        let conn = setup();
        bond(&conn, "b", "10000", "10", "4.5", Some(day(2030, 10, 9)));
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let coupon = list
            .iter()
            .find(|m| m.kind == "bond_coupon")
            .expect("coupon");
        assert_eq!(coupon.due_day, Some(day(2026, 10, 9)));
        assert_eq!(coupon.tone, TONE_INFO);
        assert_eq!(coupon.amount.as_deref(), Some("4500.00"));
    }

    #[test]
    fn the_last_coupon_belongs_to_the_maturity() {
        assert_eq!(next_coupon_day(day(2026, 10, 9), day(2026, 10, 5)), None);
        assert_eq!(
            next_coupon_day(day(2027, 10, 9), day(2026, 10, 5)),
            Some(day(2026, 10, 9))
        );
        assert_eq!(next_coupon_day(day(2027, 10, 1), day(2026, 10, 5)), None);
    }

    #[test]
    fn a_29_february_maturity_pays_on_28_february_in_common_years() {
        assert_eq!(
            next_coupon_day(day(2028, 2, 29), day(2026, 10, 5)),
            Some(day(2027, 2, 28))
        );
    }

    #[test]
    fn matured_bonds_and_far_dates_are_skipped() {
        let conn = setup();
        bond(&conn, "old", "1000", "1", "3", Some(day(2026, 1, 1)));
        bond(&conn, "far", "1000", "1", "0", Some(day(2027, 6, 1)));
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }
}
