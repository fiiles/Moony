//! Loan amortization maths.
//!
//! Pure functions: no database, no clock, no `AppError`. Days are UTC-midnight
//! unix seconds (ADR 0008). The same rules live in
//! `shared/calculations/loan-amortization.ts`; both are pinned to the cent by
//! `shared/calculations/loan-amortization.fixture.json`.
//!
//! Rules (one place, so the UI can quote them):
//! - Monthly rate `i = annual% / 100 / 12`; a payment is due on the anchor's
//!   day-of-month, `k` calendar months after the anchor, clamped to the last
//!   day of a short month; a payment due on the valuation day counts as made.
//! - The anchor is the (amount, day) amortization runs from: the manual
//!   balance when both are set, else (principal, start day).
//! - No monthly payment: the balance stays at the anchor.

const SECONDS_PER_DAY: i64 = 86_400;

/// Longest schedule returned to the UI (50 years of monthly payments).
pub const MAX_SCHEDULE_ROWS: usize = 600;

/// Valuation never loops past 100 years of payments.
const MAX_VALUATION_MONTHS: u32 = 1_200;

/// Everything the maths needs from a loan, already parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct LoanTerms {
    /// Original amount; the anchor amount when no manual balance is set.
    pub principal: f64,
    /// Annual interest rate in percent (0 for an empty rate).
    pub annual_rate_pct: f64,
    /// Monthly payment; `<= 0` means "unknown": no amortization.
    pub monthly_payment: f64,
    /// UTC day the loan started; the anchor day when no manual balance is set.
    pub start_day: i64,
    pub anchor_amount: Option<f64>,
    pub anchor_day: Option<i64>,
    /// Schedule rows stop at the last payment due on or before this day.
    pub end_day: Option<i64>,
}

impl LoanTerms {
    /// True when the user entered a real balance as of a date.
    pub fn is_manual_anchor(&self) -> bool {
        self.anchor_amount.is_some() && self.anchor_day.is_some()
    }

    /// (amount, day) amortization starts from.
    pub fn anchor(&self) -> (f64, i64) {
        match (self.anchor_amount, self.anchor_day) {
            (Some(amount), Some(day)) => (amount.max(0.0), day_floor(day)),
            _ => (self.principal.max(0.0), day_floor(self.start_day)),
        }
    }

    fn monthly_rate(&self) -> f64 {
        self.annual_rate_pct / 100.0 / 12.0
    }
}

/// One payment of the annuity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub interest: f64,
    pub principal_part: f64,
    /// The payment actually made: smaller than the regular one on the last row.
    pub payment: f64,
    pub balance_after: f64,
}

/// The single amortization formula (also used by the annuity calculator via
/// its TypeScript mirror): `interest = b·i`, `principal = payment − interest`,
/// `b' = b − principal`; the final payment clears the balance and never
/// overshoots it. A payment below the interest yields a negative principal
/// part, i.e. the balance grows.
pub fn amortization_step(balance: f64, monthly_rate: f64, payment: f64) -> Step {
    let interest = balance * monthly_rate;
    let principal = payment - interest;
    if principal >= balance {
        Step {
            interest,
            principal_part: balance,
            payment: balance + interest,
            balance_after: 0.0,
        }
    } else {
        Step {
            interest,
            principal_part: principal,
            payment,
            balance_after: balance - principal,
        }
    }
}

/// Start of the UTC day containing `ts`.
pub fn day_floor(ts: i64) -> i64 {
    ts.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY
}

/// Today's UTC day (the valuation day of "now").
pub fn today_utc_day() -> i64 {
    day_floor(chrono::Utc::now().timestamp())
}

/// (year, month 1-12, day 1-31) of a day number (days since 1970-01-01).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Day number (days since 1970-01-01) of a civil date.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
    }
}

/// Day payment `k` falls due: `k` calendar months after the anchor, on the
/// anchor's day-of-month, clamped to the month's last day.
pub fn due_day(anchor_day: i64, k: u32) -> i64 {
    let (year, month, day) = civil_from_days(anchor_day.div_euclid(SECONDS_PER_DAY));
    let months = i64::from(month) - 1 + i64::from(k);
    let due_year = year + months.div_euclid(12);
    let due_month = (months.rem_euclid(12) + 1) as u32;
    let due_dom = day.min(days_in_month(due_year, due_month));
    days_from_civil(due_year, due_month, due_dom) * SECONDS_PER_DAY
}

/// Number of payments due on or before `day` (0 before the first one).
pub fn payments_due_through(anchor_day: i64, day: i64) -> u32 {
    let anchor_day = day_floor(anchor_day);
    let day = day_floor(day);
    if day < anchor_day {
        return 0;
    }
    let (ay, am, _) = civil_from_days(anchor_day / SECONDS_PER_DAY);
    let (dy, dm, _) = civil_from_days(day / SECONDS_PER_DAY);
    let months = ((dy - ay) * 12 + i64::from(dm) - i64::from(am)).max(0) as u32;
    if months > 0 && due_day(anchor_day, months) > day {
        months - 1
    } else {
        months
    }
}

/// Balance and cumulative figures after every payment due on or before a day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elapsed {
    pub balance: f64,
    pub interest_paid: f64,
    pub payments_made: u32,
}

pub fn elapsed_to(terms: &LoanTerms, day: i64) -> Elapsed {
    let (anchor_amount, anchor_day) = terms.anchor();
    let mut elapsed = Elapsed {
        balance: anchor_amount,
        interest_paid: 0.0,
        payments_made: 0,
    };
    if terms.monthly_payment <= 0.0 || anchor_amount <= 0.0 || day_floor(day) < anchor_day {
        return elapsed;
    }
    let rate = terms.monthly_rate();
    let due = payments_due_through(anchor_day, day).min(MAX_VALUATION_MONTHS);
    for _ in 0..due {
        if elapsed.balance <= 0.0 {
            break;
        }
        let step = amortization_step(elapsed.balance, rate, terms.monthly_payment);
        elapsed.interest_paid += step.interest;
        elapsed.payments_made += 1;
        elapsed.balance = step.balance_after;
    }
    elapsed
}

/// Outstanding balance as of `day`: the anchor amount before the anchor day,
/// else the anchor after every payment due on or before `day`.
pub fn outstanding_balance_at(terms: &LoanTerms, day: i64) -> f64 {
    elapsed_to(terms, day).balance
}

/// True when the regular payment does not even cover the first month's
/// interest, so the balance stays or grows.
pub fn payment_below_interest(terms: &LoanTerms) -> bool {
    let (anchor_amount, _) = terms.anchor();
    terms.monthly_payment > 0.0
        && anchor_amount > 0.0
        && terms.monthly_payment <= anchor_amount * terms.monthly_rate()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScheduleRow {
    /// Day the payment falls due; the interest covers the month ending then.
    pub due_day: i64,
    pub payment: f64,
    pub interest: f64,
    pub principal_part: f64,
    pub balance_after: f64,
}

/// Why the schedule stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleEnd {
    /// The balance reached 0.
    Paid,
    /// The next payment would fall after `end_day` and a balance is left.
    EndDate,
    /// `max_rows` reached with a balance left.
    Capped,
    /// No monthly payment: nothing to schedule.
    NoPayment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schedule {
    pub rows: Vec<ScheduleRow>,
    pub end: ScheduleEnd,
}

/// Payments from the anchor until the balance is 0, the last payment due on
/// or before `end_day`, or `max_rows`.
pub fn schedule(terms: &LoanTerms, max_rows: usize) -> Schedule {
    let (mut balance, anchor_day) = terms.anchor();
    if terms.monthly_payment <= 0.0 {
        return Schedule {
            rows: Vec::new(),
            end: ScheduleEnd::NoPayment,
        };
    }
    let mut rows = Vec::new();
    if balance <= 0.0 {
        return Schedule {
            rows,
            end: ScheduleEnd::Paid,
        };
    }
    let rate = terms.monthly_rate();
    let end_day = terms.end_day.map(day_floor);
    let mut k: u32 = 0;
    loop {
        k += 1;
        let due = due_day(anchor_day, k);
        if end_day.is_some_and(|end| due > end) {
            return Schedule {
                rows,
                end: ScheduleEnd::EndDate,
            };
        }
        if rows.len() >= max_rows {
            return Schedule {
                rows,
                end: ScheduleEnd::Capped,
            };
        }
        let step = amortization_step(balance, rate, terms.monthly_payment);
        balance = step.balance_after;
        rows.push(ScheduleRow {
            due_day: due,
            payment: step.payment,
            interest: step.interest,
            principal_part: step.principal_part,
            balance_after: step.balance_after,
        });
        if balance <= 0.0 {
            return Schedule {
                rows,
                end: ScheduleEnd::Paid,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, NaiveDate};
    use serde_json::Value;

    const FIXTURE: &str =
        include_str!("../../../shared/calculations/loan-amortization.fixture.json");

    fn day(iso: &str) -> i64 {
        let date = NaiveDate::parse_from_str(iso, "%Y-%m-%d").unwrap();
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        date.signed_duration_since(epoch).num_days() * SECONDS_PER_DAY
    }

    fn cents(value: f64) -> i64 {
        (value * 100.0).round() as i64
    }

    fn terms(iso_start: &str) -> LoanTerms {
        LoanTerms {
            principal: 100_000.0,
            annual_rate_pct: 6.0,
            monthly_payment: 2_000.0,
            start_day: day(iso_start),
            anchor_amount: None,
            anchor_day: None,
            end_day: None,
        }
    }

    fn terms_from_fixture(t: &Value) -> LoanTerms {
        let opt_f = |key: &str| t[key].as_f64();
        let opt_day = |key: &str| t[key].as_str().map(day);
        LoanTerms {
            principal: t["principal"].as_f64().unwrap(),
            annual_rate_pct: t["annualRatePct"].as_f64().unwrap(),
            monthly_payment: t["monthlyPayment"].as_f64().unwrap(),
            start_day: day(t["startDay"].as_str().unwrap()),
            anchor_amount: opt_f("anchorAmount"),
            anchor_day: opt_day("anchorDay"),
            end_day: opt_day("endDay"),
        }
    }

    #[test]
    fn civil_conversion_matches_chrono_for_every_day_in_a_century() {
        let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        for days in -3_000..40_000 {
            let date = epoch + chrono::Duration::days(days);
            let (y, m, d) = civil_from_days(days);
            assert_eq!(
                (y, m, d),
                (i64::from(date.year()), date.month(), date.day())
            );
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn due_day_clamps_to_the_last_day_of_short_months() {
        let anchor = day("2024-01-31");
        assert_eq!(due_day(anchor, 0), anchor);
        assert_eq!(due_day(anchor, 1), day("2024-02-29")); // leap year
        assert_eq!(due_day(anchor, 2), day("2024-03-31"));
        assert_eq!(due_day(anchor, 3), day("2024-04-30"));
        assert_eq!(due_day(anchor, 13), day("2025-02-28"));
        assert_eq!(due_day(day("2023-12-15"), 2), day("2024-02-15"));
    }

    #[test]
    fn payments_due_through_counts_whole_months_inclusive_of_the_due_day() {
        let anchor = day("2020-01-15");
        assert_eq!(payments_due_through(anchor, day("2020-01-14")), 0);
        assert_eq!(payments_due_through(anchor, day("2020-01-15")), 0);
        assert_eq!(payments_due_through(anchor, day("2020-02-14")), 0);
        assert_eq!(payments_due_through(anchor, day("2020-02-15")), 1);
        assert_eq!(payments_due_through(anchor, day("2021-01-14")), 11);
        assert_eq!(payments_due_through(anchor, day("2021-01-15")), 12);
        // Anchor on the 31st: February's payment falls due on its last day.
        let month_end = day("2024-01-31");
        assert_eq!(payments_due_through(month_end, day("2024-02-28")), 0);
        assert_eq!(payments_due_through(month_end, day("2024-02-29")), 1);
        assert_eq!(payments_due_through(month_end, day("2025-02-27")), 12);
        assert_eq!(payments_due_through(month_end, day("2025-02-28")), 13);
    }

    #[test]
    fn a_timestamp_inside_a_day_counts_as_that_day() {
        let anchor = day("2020-01-15");
        let noon = day("2020-02-15") + 12 * 3_600;
        assert_eq!(payments_due_through(anchor, noon), 1);
    }

    #[test]
    fn step_splits_a_payment_into_interest_and_principal() {
        let step = amortization_step(100_000.0, 0.005, 2_000.0);
        assert!((step.interest - 500.0).abs() < 1e-9);
        assert!((step.principal_part - 1_500.0).abs() < 1e-9);
        assert!((step.balance_after - 98_500.0).abs() < 1e-9);
        assert_eq!(step.payment, 2_000.0);
    }

    #[test]
    fn step_never_overshoots_and_shrinks_the_final_payment() {
        let step = amortization_step(1_000.0, 0.01, 5_000.0);
        assert_eq!(step.balance_after, 0.0);
        assert_eq!(step.principal_part, 1_000.0);
        assert!((step.payment - 1_010.0).abs() < 1e-9);
    }

    #[test]
    fn step_with_payment_below_interest_grows_the_balance() {
        let step = amortization_step(100_000.0, 0.01, 500.0);
        assert!(step.principal_part < 0.0);
        assert!((step.balance_after - 100_500.0).abs() < 1e-9);
    }

    #[test]
    fn valuation_before_the_anchor_is_the_anchor_amount() {
        let mut t = terms("2024-06-15");
        t.anchor_amount = Some(70_000.0);
        t.anchor_day = Some(day("2025-01-10"));
        assert_eq!(outstanding_balance_at(&t, day("2024-12-31")), 70_000.0);
        assert_eq!(outstanding_balance_at(&t, day("2025-01-10")), 70_000.0);
        assert_eq!(
            outstanding_balance_at(&terms("2024-06-15"), day("2020-01-01")),
            100_000.0
        );
    }

    #[test]
    fn a_manual_anchor_replaces_principal_and_start() {
        let mut t = terms("2020-01-01");
        t.anchor_amount = Some(50_000.0);
        t.anchor_day = Some(day("2025-03-20"));
        assert!(t.is_manual_anchor());
        let after_one = outstanding_balance_at(&t, day("2025-04-20"));
        // 50 000 * 1.005 - 2 000
        assert_eq!(cents(after_one), cents(48_250.0));
    }

    #[test]
    fn an_empty_payment_never_amortizes() {
        let mut t = terms("2020-01-01");
        t.monthly_payment = 0.0;
        assert_eq!(outstanding_balance_at(&t, day("2040-01-01")), 100_000.0);
        let s = schedule(&t, MAX_SCHEDULE_ROWS);
        assert!(s.rows.is_empty());
        assert_eq!(s.end, ScheduleEnd::NoPayment);
    }

    #[test]
    fn payment_below_interest_is_detected_and_the_balance_grows() {
        let mut t = terms("2020-01-01");
        t.monthly_payment = 400.0; // interest is 500
        assert!(payment_below_interest(&t));
        assert!(outstanding_balance_at(&t, day("2021-01-01")) > 100_000.0);
        t.monthly_payment = 500.0; // interest-only: never pays off
        assert!(payment_below_interest(&t));
        t.monthly_payment = 501.0;
        assert!(!payment_below_interest(&t));
        t.monthly_payment = 0.0;
        assert!(!payment_below_interest(&t));
    }

    #[test]
    fn elapsed_tracks_interest_paid_and_payments_made() {
        let t = LoanTerms {
            principal: 10_000.0,
            annual_rate_pct: 12.0,
            monthly_payment: 3_000.0,
            start_day: day("2024-01-31"),
            anchor_amount: None,
            anchor_day: None,
            end_day: None,
        };
        let after_two = elapsed_to(&t, day("2024-03-31"));
        assert_eq!(after_two.payments_made, 2);
        assert_eq!(cents(after_two.interest_paid), cents(171.0));
        let paid_off = elapsed_to(&t, day("2030-01-01"));
        assert_eq!(paid_off.payments_made, 4);
        assert_eq!(paid_off.balance, 0.0);
    }

    #[test]
    fn schedule_rows_chain_and_end_when_paid() {
        let t = terms("2024-01-31");
        let s = schedule(&t, MAX_SCHEDULE_ROWS);
        assert_eq!(s.end, ScheduleEnd::Paid);
        let mut previous = t.principal;
        for row in &s.rows {
            assert!((previous + row.interest - row.payment - row.balance_after).abs() < 1e-6);
            assert!((row.payment - row.interest - row.principal_part).abs() < 1e-6);
            previous = row.balance_after;
        }
        assert_eq!(s.rows.last().unwrap().balance_after, 0.0);
        assert!(s.rows.last().unwrap().payment <= t.monthly_payment + 1e-9);
    }

    #[test]
    fn schedule_agrees_with_valuation_after_every_payment() {
        let t = terms("2022-08-31");
        let s = schedule(&t, MAX_SCHEDULE_ROWS);
        for row in &s.rows {
            let valued = outstanding_balance_at(&t, row.due_day);
            assert!((valued - row.balance_after).abs() < 1e-6);
        }
    }

    #[test]
    fn schedule_stops_at_the_end_date_with_a_balance_left() {
        let mut t = terms("2024-01-15");
        t.end_day = Some(day("2024-06-15"));
        let s = schedule(&t, MAX_SCHEDULE_ROWS);
        assert_eq!(s.end, ScheduleEnd::EndDate);
        assert_eq!(s.rows.len(), 5);
        assert_eq!(s.rows.last().unwrap().due_day, day("2024-06-15"));
        // end_date never changes the valuation
        assert_eq!(
            cents(outstanding_balance_at(&t, day("2025-01-15"))),
            cents(outstanding_balance_at(
                &terms("2024-01-15"),
                day("2025-01-15")
            ))
        );
    }

    #[test]
    fn schedule_is_capped_at_max_rows() {
        let mut t = terms("2024-01-15");
        t.monthly_payment = 500.0; // interest-only, never ends
        let s = schedule(&t, 600);
        assert_eq!(s.rows.len(), 600);
        assert_eq!(s.end, ScheduleEnd::Capped);
    }

    #[test]
    fn a_zero_balance_anchor_has_an_empty_paid_schedule() {
        let mut t = terms("2020-01-01");
        t.anchor_amount = Some(0.0);
        t.anchor_day = Some(day("2024-01-01"));
        let s = schedule(&t, MAX_SCHEDULE_ROWS);
        assert!(s.rows.is_empty());
        assert_eq!(s.end, ScheduleEnd::Paid);
        assert_eq!(outstanding_balance_at(&t, day("2030-01-01")), 0.0);
    }

    #[test]
    fn fixture_balances_and_schedules_match_to_the_cent() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 8);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let t = terms_from_fixture(&case["terms"]);
            for b in case["balances"].as_array().unwrap() {
                let on = b["on"].as_str().unwrap();
                let expected = b["balance"].as_f64().unwrap();
                let actual = outstanding_balance_at(&t, day(on));
                assert_eq!(
                    cents(actual),
                    cents(expected),
                    "{name} balance on {on}: got {actual}"
                );
            }
            if let Some(sched) = case.get("schedule") {
                let max_rows = sched["maxRows"].as_u64().unwrap() as usize;
                let s = schedule(&t, max_rows);
                let end = match s.end {
                    ScheduleEnd::Paid => "paid",
                    ScheduleEnd::EndDate => "endDate",
                    ScheduleEnd::Capped => "capped",
                    ScheduleEnd::NoPayment => "noPayment",
                };
                assert_eq!(end, sched["end"].as_str().unwrap(), "{name}: end");
                assert_eq!(
                    s.rows.len() as u64,
                    sched["rowCount"].as_u64().unwrap(),
                    "{name}: rows"
                );
                match sched["lastBalance"].as_f64() {
                    Some(expected) => {
                        let last = s.rows.last().expect("rows");
                        assert_eq!(
                            cents(last.balance_after),
                            cents(expected),
                            "{name}: last balance"
                        );
                    }
                    None => assert!(s.rows.is_empty(), "{name}: expected no rows"),
                }
                let check_row = |row: &ScheduleRow, expected: &Value| {
                    assert_eq!(
                        row.due_day,
                        day(expected["dueDay"].as_str().unwrap()),
                        "{name}: due"
                    );
                    assert_eq!(
                        cents(row.payment),
                        cents(expected["payment"].as_f64().unwrap()),
                        "{name}: payment"
                    );
                    assert_eq!(
                        cents(row.interest),
                        cents(expected["interest"].as_f64().unwrap()),
                        "{name}: interest"
                    );
                    assert_eq!(
                        cents(row.principal_part),
                        cents(expected["principalPart"].as_f64().unwrap()),
                        "{name}: principal"
                    );
                    assert_eq!(
                        cents(row.balance_after),
                        cents(expected["balanceAfter"].as_f64().unwrap()),
                        "{name}: balance after"
                    );
                };
                if let Some(first) = sched.get("firstRows").and_then(Value::as_array) {
                    for (row, expected) in s.rows.iter().zip(first) {
                        check_row(row, expected);
                    }
                }
                if let Some(last) = sched.get("lastRow") {
                    check_row(s.rows.last().unwrap(), last);
                }
            }
        }
    }

    #[test]
    fn the_thirty_year_annuity_pins_the_documented_figures() {
        // 1 000 000 at 5 % for 360 months: payment 5 368.22 (rounded up from
        // 5 368.2162…); after 12 payments 985 246.30 (NOT 985 252.93).
        let t = LoanTerms {
            principal: 1_000_000.0,
            annual_rate_pct: 5.0,
            monthly_payment: 5_368.22,
            start_day: day("2020-01-15"),
            anchor_amount: None,
            anchor_day: None,
            end_day: None,
        };
        assert_eq!(
            cents(outstanding_balance_at(&t, day("2021-01-15"))),
            98_524_630
        );
        assert_eq!(schedule(&t, MAX_SCHEDULE_ROWS).rows.len(), 360);
    }
}
