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
//!
//! Ledger (`loan_balance_at`, `plan_from`): the balance on a day is a walk over
//! the loan's events. A manual balance record holds exactly until the user
//! changes it (nothing is estimated over it); estimated segments amortize
//! monthly from their anchor day. See
//! `docs/specs/2026-10-13-history-from-dated-facts-design.md`, B2 and B3.

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

// ---------------------------------------------------------------------------
// Ledger walk (spec B2, B3)
// ---------------------------------------------------------------------------

/// The stored kind of a loan event. `BalanceCheck` is the manual balance
/// record (`balance_check` in the database).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerEventKind {
    ExtraPayment,
    RateChange,
    BalanceCheck,
}

/// One loan event, already parsed. Money is in the loan currency.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEvent {
    /// UTC day of the event.
    pub day: i64,
    /// Orders events that share a day (unix seconds).
    pub created_at: i64,
    pub kind: LedgerEventKind,
    /// Extra payment: the amount paid. Manual record: the balance.
    pub amount: Option<f64>,
    /// Rate change: the new annual rate in percent (`None` reads as 0, like an
    /// empty rate on the loan).
    pub rate: Option<f64>,
    /// Extra payment / rate change: the new regular payment when given.
    pub monthly_payment: Option<f64>,
    /// Rate change: the rate in force before it (`None` on legacy rows).
    pub previous_rate: Option<f64>,
    /// Rate change, or an extra payment that sets a new payment: the payment in
    /// force before it (`None` on legacy rows and on events that leave it as is).
    pub previous_monthly_payment: Option<f64>,
}

/// A loan and its events. `terms` holds the loan's *current* rate and payment,
/// the principal, the start day and the end day; its anchor fields are
/// ignored (the events are the source of every manual balance).
#[derive(Debug, Clone, PartialEq)]
pub struct LoanLedger {
    pub terms: LoanTerms,
    pub events: Vec<LedgerEvent>,
}

/// How the balance of a day came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerMode {
    /// Amortized monthly from the segment anchor.
    Estimated,
    /// Entered by the user; holds, nothing is amortized over it.
    Manual,
}

/// The walk's state on a day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LedgerState {
    /// Balance on the day, `>= 0`; 0 before the start day.
    pub balance: f64,
    pub mode: LedgerMode,
    /// Day whose day-of-month the monthly payments fall on: the start day, or
    /// the day of the last estimated extra payment / rate change. A manual
    /// record leaves it unchanged.
    pub segment_anchor_day: i64,
    /// Balance on `segment_anchor_day` (the amount the estimated segment
    /// amortizes from); equals `balance` in `Manual` mode.
    pub segment_anchor_amount: f64,
    /// Annual rate in force on the day, percent.
    pub annual_rate_pct: f64,
    /// Regular payment in force on the day.
    pub monthly_payment: f64,
}

impl LedgerState {
    pub fn monthly_rate(&self) -> f64 {
        self.annual_rate_pct / 100.0 / 12.0
    }
}

/// Events by (day, created_at), the order the walk applies them in.
fn ordered_events(ledger: &LoanLedger) -> Vec<&LedgerEvent> {
    let mut events: Vec<&LedgerEvent> = ledger.events.iter().collect();
    events.sort_by_key(|event| (day_floor(event.day), event.created_at));
    events
}

/// Rate and payment before any event: each is the previous value recorded by the
/// EARLIEST event that carries one (any kind: a rate change records both, an extra
/// payment that lowers the payment records the payment), else the loan's current
/// terms (legacy rows).
fn initial_terms(terms: &LoanTerms, events: &[&LedgerEvent]) -> (f64, f64) {
    let rate = events
        .iter()
        .find_map(|event| event.previous_rate)
        .unwrap_or(terms.annual_rate_pct);
    let payment = events
        .iter()
        .find_map(|event| event.previous_monthly_payment)
        .unwrap_or(terms.monthly_payment);
    (rate, payment)
}

/// Balance of the current estimated segment after every payment due on or
/// before `to_day`.
fn estimated_balance_through(state: &LedgerState, to_day: i64) -> f64 {
    let segment = LoanTerms {
        principal: state.segment_anchor_amount,
        annual_rate_pct: state.annual_rate_pct,
        monthly_payment: state.monthly_payment,
        start_day: state.segment_anchor_day,
        anchor_amount: None,
        anchor_day: None,
        end_day: None,
    };
    elapsed_to(&segment, to_day).balance
}

fn apply_event(state: &mut LedgerState, event: &LedgerEvent) {
    let estimated = state.mode == LedgerMode::Estimated;
    match event.kind {
        LedgerEventKind::BalanceCheck => {
            state.balance = event.amount.unwrap_or(0.0).max(0.0);
            state.mode = LedgerMode::Manual;
        }
        LedgerEventKind::ExtraPayment => {
            state.balance = (state.balance - event.amount.unwrap_or(0.0)).max(0.0);
            if let Some(payment) = event.monthly_payment {
                state.monthly_payment = payment;
            }
        }
        LedgerEventKind::RateChange => {
            state.annual_rate_pct = event.rate.unwrap_or(0.0);
            if let Some(payment) = event.monthly_payment {
                state.monthly_payment = payment;
            }
        }
    }
    if estimated && event.kind != LedgerEventKind::BalanceCheck {
        // The event day starts a new estimated segment.
        state.segment_anchor_day = day_floor(event.day);
    }
    state.segment_anchor_amount = state.balance;
}

/// Balance of a loan on `day`: the ledger walk (spec B2).
///
/// Before the start day the balance is 0. Otherwise the walk starts as
/// `Estimated` from (principal, start day), with the terms in force before the
/// first event (`initial_terms`), and applies the events dated on or before `day`
/// in (day, created_at) order:
/// - estimated segments amortize monthly between events, on the segment
///   anchor's day of month;
/// - a manual record (`BalanceCheck`) sets the balance and switches to
///   `Manual`: nothing is amortized after it;
/// - an extra payment subtracts its amount (a new payment applies when given);
///   estimated: the event day becomes the segment anchor; manual: stays manual;
/// - a rate change swaps the terms; estimated: the event day becomes the
///   segment anchor; manual: the balance is unchanged.
pub fn loan_balance_at(ledger: &LoanLedger, day: i64) -> LedgerState {
    let day = day_floor(day);
    let start = day_floor(ledger.terms.start_day);
    let events = ordered_events(ledger);
    let (annual_rate_pct, monthly_payment) = initial_terms(&ledger.terms, &events);
    let principal = ledger.terms.principal.max(0.0);
    let mut state = LedgerState {
        balance: principal,
        mode: LedgerMode::Estimated,
        segment_anchor_day: start,
        segment_anchor_amount: principal,
        annual_rate_pct,
        monthly_payment,
    };
    if day < start {
        state.balance = 0.0;
        return state;
    }
    for event in events
        .into_iter()
        .take_while(|event| day_floor(event.day) <= day)
    {
        if state.mode == LedgerMode::Estimated {
            state.balance = estimated_balance_through(&state, day_floor(event.day));
        }
        apply_event(&mut state, event);
    }
    if state.mode == LedgerMode::Estimated {
        state.balance = estimated_balance_through(&state, day);
    }
    state.balance = state.balance.max(0.0);
    state
}

/// Terms whose `anchor()` starts the schedule ahead, so `schedule` and
/// `elapsed_to` run on them unchanged (spec B3). The terms are the ones in
/// force on `today`:
/// - `Estimated`: the schedule continues from the segment anchor, as before;
/// - `Manual`: it starts from today's balance. Payments stay on the segment
///   anchor's day of month, so the anchor day is the last payment day on or
///   before `today` of that cycle, and the first planned payment is the next
///   cycle day strictly after `today` (the balance already includes today's).
///   A cycle day above 28 that the anchor month clamps (e.g. the 31st in a
///   30-day month) moves the later due days to the clamped day.
pub fn plan_from(ledger: &LoanLedger, today: i64) -> LoanTerms {
    let state = loan_balance_at(ledger, today);
    let (anchor_amount, anchor_day) = match state.mode {
        LedgerMode::Estimated => (state.segment_anchor_amount, state.segment_anchor_day),
        LedgerMode::Manual => {
            let paid = payments_due_through(state.segment_anchor_day, today);
            (state.balance, due_day(state.segment_anchor_day, paid))
        }
    };
    LoanTerms {
        principal: ledger.terms.principal,
        annual_rate_pct: state.annual_rate_pct,
        monthly_payment: state.monthly_payment,
        start_day: ledger.terms.start_day,
        anchor_amount: Some(anchor_amount),
        anchor_day: Some(anchor_day),
        end_day: ledger.terms.end_day,
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

    /// Asserts a schedule against a fixture `schedule` block (shared by the plain
    /// and the ledger cases).
    fn assert_schedule_matches(name: &str, t: &LoanTerms, sched: &Value) {
        let max_rows = sched["maxRows"].as_u64().unwrap() as usize;
        let s = schedule(t, max_rows);
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
                assert_schedule_matches(name, &t, sched);
            }
        }
    }

    fn ledger_event_from_fixture(e: &Value) -> LedgerEvent {
        LedgerEvent {
            day: day(e["day"].as_str().unwrap()),
            created_at: e["createdAt"].as_i64().unwrap(),
            kind: match e["kind"].as_str().unwrap() {
                "extra_payment" => LedgerEventKind::ExtraPayment,
                "rate_change" => LedgerEventKind::RateChange,
                "balance_check" => LedgerEventKind::BalanceCheck,
                other => panic!("unknown event kind {other}"),
            },
            amount: e["amount"].as_f64(),
            rate: e["rate"].as_f64(),
            monthly_payment: e["monthlyPayment"].as_f64(),
            previous_rate: e["previousRate"].as_f64(),
            previous_monthly_payment: e["previousMonthlyPayment"].as_f64(),
        }
    }

    fn ledger_from_fixture(l: &Value) -> LoanLedger {
        let t = &l["terms"];
        LoanLedger {
            terms: LoanTerms {
                principal: t["principal"].as_f64().unwrap(),
                annual_rate_pct: t["annualRatePct"].as_f64().unwrap(),
                monthly_payment: t["monthlyPayment"].as_f64().unwrap(),
                start_day: day(t["startDay"].as_str().unwrap()),
                anchor_amount: None,
                anchor_day: None,
                end_day: t["endDay"].as_str().map(day),
            },
            events: l["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(ledger_event_from_fixture)
                .collect(),
        }
    }

    fn mode_name(mode: LedgerMode) -> &'static str {
        match mode {
            LedgerMode::Estimated => "estimated",
            LedgerMode::Manual => "manual",
        }
    }

    #[test]
    fn fixture_ledger_balances_and_plans_match_to_the_cent() {
        let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
        let cases = fixture["ledgerCases"].as_array().unwrap();
        assert!(cases.len() >= 12);
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let ledger = ledger_from_fixture(&case["ledger"]);
            for b in case["balances"].as_array().unwrap() {
                let on = b["on"].as_str().unwrap();
                let state = loan_balance_at(&ledger, day(on));
                assert_eq!(
                    cents(state.balance),
                    cents(b["balance"].as_f64().unwrap()),
                    "{name} balance on {on}: got {}",
                    state.balance
                );
                assert_eq!(
                    mode_name(state.mode),
                    b["mode"].as_str().unwrap(),
                    "{name} mode on {on}"
                );
                assert_eq!(
                    state.segment_anchor_day,
                    day(b["segmentAnchorDay"].as_str().unwrap()),
                    "{name} segment anchor on {on}"
                );
            }
            if let Some(plan) = case.get("plan") {
                let today = day(plan["today"].as_str().unwrap());
                assert_eq!(
                    mode_name(loan_balance_at(&ledger, today).mode),
                    plan["mode"].as_str().unwrap(),
                    "{name}: plan mode"
                );
                let terms = plan_from(&ledger, today);
                let (amount, anchor_day) = terms.anchor();
                assert_eq!(
                    cents(amount),
                    cents(plan["anchorAmount"].as_f64().unwrap()),
                    "{name}: plan anchor amount"
                );
                assert_eq!(
                    anchor_day,
                    day(plan["anchorDay"].as_str().unwrap()),
                    "{name}: plan anchor day"
                );
                assert!(
                    (terms.annual_rate_pct - plan["annualRatePct"].as_f64().unwrap()).abs() < 1e-9,
                    "{name}: plan rate"
                );
                assert!(
                    (terms.monthly_payment - plan["monthlyPayment"].as_f64().unwrap()).abs() < 1e-9,
                    "{name}: plan payment"
                );
                assert_schedule_matches(name, &terms, &plan["schedule"]);
            }
        }
    }

    fn event(day_iso: &str, created_at: i64, kind: LedgerEventKind) -> LedgerEvent {
        LedgerEvent {
            day: day(day_iso),
            created_at,
            kind,
            amount: None,
            rate: None,
            monthly_payment: None,
            previous_rate: None,
            previous_monthly_payment: None,
        }
    }

    fn ledger(start: &str, events: Vec<LedgerEvent>) -> LoanLedger {
        LoanLedger {
            terms: terms(start),
            events,
        }
    }

    #[test]
    fn a_ledger_without_events_is_the_legacy_annuity() {
        let l = ledger("2023-01-15", Vec::new());
        for iso in ["2023-02-14", "2023-02-15", "2024-01-15", "2031-05-20"] {
            assert_eq!(
                cents(loan_balance_at(&l, day(iso)).balance),
                cents(outstanding_balance_at(&l.terms, day(iso))),
                "{iso}"
            );
        }
    }

    #[test]
    fn the_ledger_ignores_the_terms_anchor_fields() {
        let mut l = ledger("2023-01-15", Vec::new());
        l.terms.anchor_amount = Some(1.0);
        l.terms.anchor_day = Some(day("2023-06-01"));
        assert_eq!(loan_balance_at(&l, day("2023-02-14")).balance, 100_000.0);
    }

    #[test]
    fn event_order_is_day_then_created_at_whatever_the_slice_order() {
        let mut record = event("2024-05-10", 10, LedgerEventKind::BalanceCheck);
        record.amount = Some(80_000.0);
        let mut extra = event("2024-05-10", 20, LedgerEventKind::ExtraPayment);
        extra.amount = Some(5_000.0);
        let forward = ledger("2023-01-15", vec![record.clone(), extra.clone()]);
        let reversed = ledger("2023-01-15", vec![extra.clone(), record.clone()]);
        assert_eq!(
            loan_balance_at(&forward, day("2024-06-01")).balance,
            75_000.0
        );
        assert_eq!(
            loan_balance_at(&reversed, day("2024-06-01")).balance,
            75_000.0
        );
        // created in the other order: the payment first, then the record wins.
        extra.created_at = 5;
        let payment_first = ledger("2023-01-15", vec![record, extra]);
        assert_eq!(
            loan_balance_at(&payment_first, day("2024-06-01")).balance,
            80_000.0
        );
    }

    #[test]
    fn the_first_rate_changes_previous_terms_are_the_initial_terms() {
        let mut change = event("2024-06-10", 1, LedgerEventKind::RateChange);
        change.rate = Some(4.5);
        change.monthly_payment = Some(1_900.0);
        change.previous_rate = Some(6.0);
        change.previous_monthly_payment = Some(2_000.0);
        let mut l = ledger("2023-01-15", vec![change]);
        l.terms.annual_rate_pct = 4.5;
        l.terms.monthly_payment = 1_900.0;
        // One payment at 6 % / 2 000, not at the loan's current 4.5 % / 1 900.
        assert_eq!(
            cents(loan_balance_at(&l, day("2023-02-15")).balance),
            cents(98_500.0)
        );
    }

    #[test]
    fn an_extra_payment_that_lowers_the_payment_records_the_one_it_replaced() {
        let mut extra = event("2024-03-20", 1, LedgerEventKind::ExtraPayment);
        extra.amount = Some(10_000.0);
        extra.monthly_payment = Some(1_500.0);
        extra.previous_monthly_payment = Some(2_000.0);
        let mut l = ledger("2023-01-15", vec![extra]);
        l.terms.monthly_payment = 1_500.0;
        // One payment at the replaced 2 000, not at the loan's current 1 500.
        let first = loan_balance_at(&l, day("2023-02-15"));
        assert_eq!(cents(first.balance), cents(98_500.0));
        assert_eq!(
            loan_balance_at(&l, day("2024-03-20")).monthly_payment,
            1_500.0
        );
    }

    #[test]
    fn each_initial_term_comes_from_the_earliest_event_that_carries_it() {
        let mut extra = event("2023-09-05", 1, LedgerEventKind::ExtraPayment);
        extra.amount = Some(5_000.0);
        extra.monthly_payment = Some(1_700.0);
        extra.previous_monthly_payment = Some(2_000.0);
        let mut change = event("2024-06-10", 2, LedgerEventKind::RateChange);
        change.rate = Some(4.5);
        change.previous_rate = Some(6.0);
        change.previous_monthly_payment = Some(1_700.0);
        // Slice order does not matter: the walk orders by (day, created_at).
        let mut l = ledger("2023-01-15", vec![change, extra]);
        l.terms.annual_rate_pct = 4.5;
        l.terms.monthly_payment = 1_700.0;
        let first = loan_balance_at(&l, day("2023-02-15"));
        assert_eq!(first.annual_rate_pct, 6.0);
        assert_eq!(first.monthly_payment, 2_000.0);
        assert_eq!(cents(first.balance), cents(98_500.0));
    }

    #[test]
    fn a_manual_balance_survives_a_zero_balance_and_a_missing_payment() {
        let mut record = event("2024-01-10", 1, LedgerEventKind::BalanceCheck);
        record.amount = Some(0.0);
        let l = ledger("2023-01-15", vec![record]);
        let state = loan_balance_at(&l, day("2026-01-01"));
        assert_eq!(state.balance, 0.0);
        assert_eq!(state.mode, LedgerMode::Manual);
        let plan = plan_from(&l, day("2026-01-01"));
        assert_eq!(schedule(&plan, MAX_SCHEDULE_ROWS).end, ScheduleEnd::Paid);
    }

    #[test]
    fn plan_from_keeps_the_start_and_end_day() {
        let mut l = ledger("2023-01-15", Vec::new());
        l.terms.end_day = Some(day("2030-01-15"));
        let plan = plan_from(&l, day("2024-01-01"));
        assert_eq!(plan.start_day, day("2023-01-15"));
        assert_eq!(plan.end_day, Some(day("2030-01-15")));
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
