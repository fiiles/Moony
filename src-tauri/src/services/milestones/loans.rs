//! Loan milestones: the end of a fixed rate, an expired fixation without a new rate, the
//! last payment and a yearly balance check against a statement.

use rusqlite::Connection;

use super::{dated, milestone, DAY, TONE_INFO};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;
use crate::services::loans as loans_service;

const FIXATION_LEAD_DAYS: i64 = 180;
const PAYOFF_LEAD_DAYS: i64 = 30;
const BALANCE_CHECK_DAYS: i64 = 365;

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    for loan in loans_service::list_loans(conn, today)? {
        let outstanding: f64 = loan.outstanding_balance.trim().parse().unwrap_or(0.0);
        let matured = outstanding <= 0.0 || loan.end_date.is_some_and(|e| day_floor(e) < today);
        if matured {
            continue;
        }

        if let Some(validity) = loan.interest_rate_validity_date.map(day_floor) {
            if validity > today {
                let remind_from = validity - FIXATION_LEAD_DAYS * DAY;
                if let Some(m) = dated(
                    "loan_fixation_end",
                    &loan.id,
                    &loan.name,
                    validity,
                    remind_from,
                    today,
                ) {
                    out.push(m);
                }
            } else {
                // A rate change clears the validity date, which resolves this item.
                let mut m = milestone(
                    "loan_fixation_expired",
                    format!("loan_fixation_expired:{}:{validity}", loan.id),
                    Some(&loan.id),
                    &loan.name,
                );
                m.due_day = Some(validity);
                out.push(m);
            }
        }

        match loans_service::loan_schedule(conn, &loan.id, today) {
            Ok(schedule) => {
                if let Some(payoff) = schedule.payoff_day.filter(|d| *d > today) {
                    let remind_from = payoff - PAYOFF_LEAD_DAYS * DAY;
                    if let Some(mut m) = dated(
                        "loan_payoff",
                        &loan.id,
                        &loan.name,
                        payoff,
                        remind_from,
                        today,
                    ) {
                        m.tone = TONE_INFO.to_string();
                        out.push(m);
                    }
                }
            }
            Err(e) => log::warn!("[MILESTONES] loan schedule skipped: {e}"),
        }

        // Every extra payment, rate change and balance check moves the anchor.
        let checked = day_floor(loan.balance_anchor_date.unwrap_or(loan.start_date));
        if checked < today - BALANCE_CHECK_DAYS * DAY {
            let mut m = milestone(
                "loan_balance_check",
                format!("loan_balance_check:{}:{checked}", loan.id),
                Some(&loan.id),
                &loan.name,
            );
            m.since_day = Some(checked);
            out.push(m);
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

    fn loan(
        conn: &Connection,
        id: &str,
        start: i64,
        validity: Option<i64>,
        payment: &str,
        anchor: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, interest_rate_validity_date,
                                monthly_payment, start_date, balance_anchor_amount, balance_anchor_date)
             VALUES (?1, ?2, '1000000', '5', ?3, ?4, ?5, ?6, ?7)",
            params![id, format!("Úvěr {id}"), validity, payment, start,
                    anchor.map(|_| "900000"), anchor],
        )
        .expect("loan");
    }

    #[test]
    fn the_fixation_end_reminds_six_months_ahead() {
        let conn = setup();
        let today = day(2026, 10, 5);
        loan(
            &conn,
            "h",
            day(2026, 1, 1),
            Some(day(2027, 3, 1)),
            "8000",
            Some(day(2026, 6, 1)),
        );
        let list = build(&conn, today).unwrap();
        let fix = list
            .iter()
            .find(|m| m.kind == "loan_fixation_end")
            .expect("fixation");
        assert_eq!(fix.stage, STAGE_NOW);
        assert_eq!(fix.due_day, Some(day(2027, 3, 1)));
        // Two months later than six months ahead: nothing yet.
        let conn2 = setup();
        loan(
            &conn2,
            "h",
            day(2026, 1, 1),
            Some(day(2027, 6, 1)),
            "8000",
            Some(day(2026, 6, 1)),
        );
        assert!(build(&conn2, today)
            .unwrap()
            .iter()
            .all(|m| m.kind != "loan_fixation_end"));
    }

    #[test]
    fn an_expired_fixation_asks_for_the_new_rate() {
        let conn = setup();
        loan(
            &conn,
            "h",
            day(2026, 1, 1),
            Some(day(2026, 9, 1)),
            "8000",
            Some(day(2026, 6, 1)),
        );
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let expired = list
            .iter()
            .find(|m| m.kind == "loan_fixation_expired")
            .expect("expired");
        assert_eq!(expired.due_day, Some(day(2026, 9, 1)));
    }

    #[test]
    fn a_payoff_within_a_month_is_info() {
        let conn = setup();
        // 5 000 on 1 Aug, 2 000 a month: 1 000 left after 1 Oct, paid off on 1 Nov.
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date,
                                balance_anchor_amount, balance_anchor_date)
             VALUES ('car', 'Auto', '20000', '0', '2000', ?1, '5000', ?2)",
            params![day(2026, 1, 1), day(2026, 8, 1)],
        )
        .unwrap();
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let payoff = list
            .iter()
            .find(|m| m.kind == "loan_payoff")
            .expect("payoff");
        assert_eq!(payoff.tone, TONE_INFO);
        assert_eq!(payoff.stage, STAGE_NOW);
    }

    #[test]
    fn a_balance_unchecked_for_a_year_asks_for_a_statement() {
        let conn = setup();
        loan(&conn, "old", day(2020, 1, 1), None, "8000", None);
        loan(
            &conn,
            "fresh",
            day(2020, 1, 1),
            None,
            "8000",
            Some(day(2026, 3, 1)),
        );
        let list = build(&conn, day(2026, 10, 5)).unwrap();
        let checks: Vec<_> = list
            .iter()
            .filter(|m| m.kind == "loan_balance_check")
            .collect();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].source_id.as_deref(), Some("old"));
        assert_eq!(checks[0].since_day, Some(day(2020, 1, 1)));
        assert_eq!(
            checks[0].key,
            format!("loan_balance_check:old:{}", day(2020, 1, 1))
        );
    }

    #[test]
    fn matured_loans_are_skipped() {
        let conn = setup();
        conn.execute(
            "INSERT INTO loans (id, name, principal, monthly_payment, start_date, end_date, interest_rate_validity_date)
             VALUES ('done', 'Hotovo', '1000', '100', ?1, ?2, ?3)",
            params![day(2019, 1, 1), day(2025, 1, 1), day(2024, 1, 1)],
        )
        .unwrap();
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }

    #[test]
    fn a_fixation_inside_the_window_is_now_even_when_near() {
        let conn = setup();
        let today = day(2026, 10, 5);
        // Ends in 80 days; the reminder window is 180 days, so it is "now", not "soon".
        loan(
            &conn,
            "h",
            day(2026, 1, 1),
            Some(today + 80 * 86_400),
            "8000",
            Some(day(2026, 6, 1)),
        );
        let fix = build(&conn, today)
            .unwrap()
            .into_iter()
            .find(|m| m.kind == "loan_fixation_end")
            .unwrap();
        assert_eq!(fix.stage, STAGE_NOW);
    }
}
