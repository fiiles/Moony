//! Bond milestones: maturity (the principal comes back) and the yearly coupon, paid on the
//! maturity's anniversary (as on the bonds page).

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
}

/// `day` moved to `year`; 29 February becomes 28 February in a common year.
fn in_year(day: i64, year: i32) -> Option<i64> {
    let date = DateTime::from_timestamp(day, 0)?.date_naive();
    let shifted = NaiveDate::from_ymd_opt(year, date.month(), date.day())
        .or_else(|| NaiveDate::from_ymd_opt(year, date.month(), date.day() - 1))?;
    Some(shifted.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
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
        "SELECT id, name, coupon_value, quantity, currency, interest_rate, maturity_date
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
        let remind_from = maturity - MATURITY_LEAD_DAYS * DAY;
        if let Some(mut m) = dated(
            "bond_maturity",
            &b.id,
            &b.name,
            maturity,
            remind_from,
            today,
        ) {
            m.amount = Some(money_text(face));
            m.currency = Some(b.currency.clone());
            out.push(m);
        }
        let rate: f64 = b.rate.trim().parse().unwrap_or(0.0);
        if rate > 0.0 && face > 0.0 {
            if let Some(coupon) = next_coupon_day(maturity, today) {
                let remind_from = coupon - COUPON_LEAD_DAYS * DAY;
                if let Some(mut m) =
                    dated("bond_coupon", &b.id, &b.name, coupon, remind_from, today)
                {
                    m.tone = TONE_INFO.to_string();
                    m.amount = Some(money_text(face * rate / 100.0));
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
