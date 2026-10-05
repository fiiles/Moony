//! Insurance milestones: anniversaries (with the notice deadline), contract ends and
//! non-monthly payments. The date rules mirror `src/utils/insurance.ts`: payments and
//! anniversaries fall on the start's day of month, clamped to the month's end. Unlike the
//! insurance page, the first anniversary is never the start day itself.

use chrono::{DateTime, Datelike};
use rusqlite::Connection;

use super::{dated, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::{Milestone, PaymentFrequency};
use crate::services::loan_amortization::{day_floor, due_day};

/// A policy can be cancelled to the end of its insurance period with at least six weeks'
/// notice; the period ends the day before the anniversary, so the last safe delivery day is
/// the anniversary minus 43 days (the earlier date is the safe one).
const NOTICE_DAYS: i64 = 43;
/// Remind four weeks before the notice deadline (about ten weeks before the anniversary).
const ANNIVERSARY_LEAD_DAYS: i64 = 28;
const END_LEAD_DAYS: i64 = 60;
const PAYMENT_LEAD_DAYS: i64 = 14;
/// Never walk more than this many payment periods.
const MAX_PERIODS: u32 = 1_200;

struct Policy {
    id: String,
    name: String,
    start: i64,
    end: Option<i64>,
    frequency: String,
    payment: String,
    currency: String,
}

/// Whole calendar months from `from` to `to`; negative when `to` is earlier.
fn months_diff(from: i64, to: i64) -> Option<i64> {
    let a = DateTime::from_timestamp(from, 0)?.date_naive();
    let b = DateTime::from_timestamp(to, 0)?.date_naive();
    Some(
        (i64::from(b.year()) - i64::from(a.year())) * 12 + i64::from(b.month())
            - i64::from(a.month()),
    )
}

/// Next yearly anniversary of `start` after `today`, at least one year after the start;
/// None when the contract ends on or before it.
fn next_anniversary(start: i64, end: Option<i64>, today: i64) -> Option<i64> {
    let start = day_floor(start);
    let years = (months_diff(start, today)? / 12).max(1);
    for y in years..=years + 2 {
        let due = due_day(start, u32::try_from(y * 12).ok()?);
        if due > today {
            return match end {
                Some(e) if due >= e => None,
                _ => Some(due),
            };
        }
    }
    None
}

/// Next payment after `today` of a policy paid every `period` months; None past the end.
fn next_payment_day(start: i64, end: Option<i64>, period: u32, today: i64) -> Option<i64> {
    let start = day_floor(start);
    let first = u32::try_from((months_diff(start, today)? / i64::from(period) - 1).max(0)).ok()?;
    for k in first..=first.checked_add(MAX_PERIODS)? {
        let due = due_day(start, k.checked_mul(period)?);
        if due > today {
            return match end {
                Some(e) if due > e => None,
                _ => Some(due),
            };
        }
    }
    None
}

/// Months between payments worth a reminder; monthly and one-time policies have none.
fn reminder_period(frequency: &str) -> Option<u32> {
    match PaymentFrequency::parse(frequency)? {
        PaymentFrequency::Quarterly => Some(3),
        PaymentFrequency::SemiAnnually => Some(6),
        PaymentFrequency::Annually => Some(12),
        PaymentFrequency::Monthly | PaymentFrequency::OneTime => None,
    }
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, policy_name, start_date, end_date, payment_frequency, regular_payment,
                regular_payment_currency
         FROM insurance_policies WHERE status = 'active'",
    )?;
    let policies = stmt
        .query_map([], |r| {
            Ok(Policy {
                id: r.get(0)?,
                name: r.get(1)?,
                start: r.get(2)?,
                end: r.get(3)?,
                frequency: r.get(4)?,
                payment: r.get(5)?,
                currency: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    for p in policies {
        let end = p.end.map(day_floor);
        if end.is_some_and(|e| e < today) {
            continue;
        }
        // The notice-to-period-end rule applies to regular premiums, not one-time policies.
        let one_time = PaymentFrequency::parse(&p.frequency) == Some(PaymentFrequency::OneTime);
        let anniversary = next_anniversary(p.start, end, today).filter(|_| !one_time);
        if let Some(anniversary) = anniversary {
            let action_day = anniversary - NOTICE_DAYS * DAY;
            let remind_from = action_day - ANNIVERSARY_LEAD_DAYS * DAY;
            if let Some(mut m) = dated(
                "insurance_anniversary",
                &p.id,
                &p.name,
                anniversary,
                remind_from,
                today,
            ) {
                m.action_day = Some(action_day);
                if today > action_day {
                    m.tone = TONE_INFO.to_string();
                }
                out.push(m);
            }
        }
        if let Some(e) = end {
            if let Some(m) = dated(
                "insurance_end",
                &p.id,
                &p.name,
                e,
                e - END_LEAD_DAYS * DAY,
                today,
            ) {
                out.push(m);
            }
        }
        // Only to decide whether a payment reminder is worth showing.
        let amount: f64 = p.payment.trim().parse().unwrap_or(0.0);
        if let Some(period) = reminder_period(&p.frequency).filter(|_| amount > 0.0) {
            if let Some(due) = next_payment_day(p.start, end, period, today) {
                let remind_from = due - PAYMENT_LEAD_DAYS * DAY;
                if let Some(mut m) =
                    dated("insurance_payment", &p.id, &p.name, due, remind_from, today)
                {
                    m.amount = Some(p.payment.trim().to_string());
                    m.currency = Some(p.currency.clone());
                    out.push(m);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::day;
    use super::*;

    // Mirrors of `src/utils/insurance.test.ts` cases for nextPaymentDay / nextAnniversary.
    #[test]
    fn a_payment_on_the_31st_clamps_to_the_month_end() {
        let start = day(2026, 1, 31);
        assert_eq!(
            next_payment_day(start, None, 3, day(2026, 2, 1)),
            Some(day(2026, 4, 30))
        );
    }

    #[test]
    fn no_payment_after_the_end() {
        let start = day(2025, 1, 10);
        assert_eq!(
            next_payment_day(start, Some(day(2026, 12, 31)), 12, day(2026, 1, 11)),
            None
        );
    }

    #[test]
    fn the_first_anniversary_is_a_year_after_the_start() {
        assert_eq!(
            next_anniversary(day(2026, 9, 1), None, day(2026, 8, 1)),
            Some(day(2027, 9, 1))
        );
    }

    // `payment days`: quarterly from 31 Jan, as the TS case, asked on 3 Oct 2026.
    #[test]
    fn a_quarterly_payment_falls_on_the_start_day_of_month_every_period() {
        let start = day(2026, 1, 31);
        let today = day(2026, 10, 3);
        assert_eq!(
            next_payment_day(start, None, 3, today),
            Some(day(2026, 10, 31))
        );
        assert_eq!(
            next_payment_day(start, None, 3, day(2026, 10, 31)),
            Some(day(2027, 1, 31))
        );
        assert_eq!(
            next_payment_day(start, None, 3, day(2027, 1, 31)),
            Some(day(2027, 4, 30))
        );
    }

    #[test]
    fn the_next_monthly_payment_is_on_the_start_day_of_the_next_month() {
        let start = day(2019, 4, 1);
        assert_eq!(
            next_payment_day(start, None, 1, day(2026, 10, 3)),
            Some(day(2026, 11, 1))
        );
    }

    #[test]
    fn a_payment_after_the_end_date_is_none() {
        let start = day(2019, 4, 1);
        assert_eq!(
            next_payment_day(start, Some(day(2026, 10, 15)), 1, day(2026, 10, 3)),
            None
        );
        // On the end day itself the payment still counts.
        assert_eq!(
            next_payment_day(start, Some(day(2026, 11, 1)), 1, day(2026, 10, 3)),
            Some(day(2026, 11, 1))
        );
    }

    #[test]
    fn monthly_and_one_time_policies_have_no_payment_reminder_period() {
        assert_eq!(reminder_period("monthly"), None);
        assert_eq!(reminder_period("one_time"), None);
        assert_eq!(reminder_period("quarterly"), Some(3));
        assert_eq!(reminder_period("semi_annually"), Some(6));
        assert_eq!(reminder_period("annually"), Some(12));
        assert_eq!(reminder_period("yearly"), Some(12), "legacy spelling");
    }

    #[test]
    fn the_next_anniversary_is_on_the_start_day_of_month_a_whole_year_on() {
        let today = day(2026, 10, 3);
        assert_eq!(
            next_anniversary(day(2019, 4, 1), None, today),
            Some(day(2027, 4, 1))
        );
        assert_eq!(
            next_anniversary(day(2024, 1, 15), None, today),
            Some(day(2027, 1, 15))
        );
    }

    #[test]
    fn no_anniversary_when_the_contract_ends_before_it() {
        let start = day(2019, 4, 1);
        let today = day(2026, 10, 3);
        assert_eq!(next_anniversary(start, Some(day(2027, 3, 1)), today), None);
        // Ending on the anniversary itself leaves nothing to renew.
        assert_eq!(next_anniversary(start, Some(day(2027, 4, 1)), today), None);
        assert_eq!(
            next_anniversary(start, Some(day(2027, 4, 2)), today),
            Some(day(2027, 4, 1))
        );
    }
}
