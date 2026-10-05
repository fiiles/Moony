//! Milestones gathered from every domain (spec 2026-10-05-milestones-design): contract
//! dates, payments, data upkeep and crossed watchlist targets. Each domain has a builder;
//! this module gives dated items their stage, drops the occurrences the user hid (done or
//! still snoozed) and the kinds the user muted, and orders the list for the dashboard card
//! and the top-bar indicator.

mod accounts;
mod bonds;
mod insurance;
mod loans;
#[cfg(test)]
pub(crate) mod test_support;
mod upkeep;
mod watchlist;

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::models::{Milestone, MILESTONE_STATE_DONE, MILESTONE_STATE_SNOOZED};

pub(crate) const DAY: i64 = 86_400;
/// `soon` covers events up to this many days ahead.
const SOON_HORIZON_DAYS: i64 = 90;
/// "Odložit" hides an occurrence for a week.
const SNOOZE_DAYS: i64 = 7;
/// Upkeep is snoozed for a month: a weekly reminder to revalue a flat is noise.
const UPKEEP_SNOOZE_DAYS: i64 = 30;
/// Undated data-upkeep kinds (their keys carry the date of the data they are about).
pub(crate) const UPKEEP_KINDS: [&str; 5] = [
    "backup_stale",
    "balances_stale",
    "valuation_stale",
    "loan_balance_check",
    "loan_fixation_expired",
];
/// `app_config` key holding the muted kinds as a JSON array.
const MUTED_KINDS_KEY: &str = "milestones.mutedKinds";

pub(crate) const STAGE_NOW: &str = "now";
pub(crate) const STAGE_SOON: &str = "soon";
pub(crate) const TONE_ACTION: &str = "action";
pub(crate) const TONE_INFO: &str = "info";

type Builder = fn(&Connection, i64) -> Result<Vec<Milestone>>;

/// Stage of a dated item: `now` from `remind_from` on, `soon` while the event is at most
/// 90 days away, otherwise None (not shown yet).
pub(crate) fn dated_stage(today: i64, due_day: i64, remind_from: i64) -> Option<&'static str> {
    if today >= remind_from {
        Some(STAGE_NOW)
    } else if due_day <= today + SOON_HORIZON_DAYS * DAY {
        Some(STAGE_SOON)
    } else {
        None
    }
}

/// A milestone with every optional field empty: `now`, `action`.
pub(crate) fn milestone(
    kind: &str,
    key: String,
    source_id: Option<&str>,
    title: &str,
) -> Milestone {
    Milestone {
        key,
        kind: kind.to_string(),
        stage: STAGE_NOW.to_string(),
        tone: TONE_ACTION.to_string(),
        source_id: source_id.map(str::to_string),
        title: title.to_string(),
        due_day: None,
        action_day: None,
        since_day: None,
        amount: None,
        currency: None,
        reference_amount: None,
        direction: None,
        count: None,
    }
}

/// A dated milestone keyed `<kind>:<source_id>:<due_day>`; None while it is too far ahead.
pub(crate) fn dated(
    kind: &str,
    source_id: &str,
    title: &str,
    due_day: i64,
    remind_from: i64,
    today: i64,
) -> Option<Milestone> {
    let stage = dated_stage(today, due_day, remind_from)?;
    let mut m = milestone(
        kind,
        format!("{kind}:{source_id}:{due_day}"),
        Some(source_id),
        title,
    );
    m.stage = stage.to_string();
    m.due_day = Some(due_day);
    Some(m)
}

/// Money as TEXT with two decimals (ADR 0001).
pub(crate) fn money_text(value: f64) -> String {
    format!("{:.2}", (value * 100.0).round() / 100.0)
}

/// Every milestone as of `today` (UTC day), without the occurrences marked done or still
/// snoozed and without the muted kinds, in display order.
pub fn list_milestones(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let builders: [(&str, Builder); 6] = [
        ("insurance", insurance::build),
        ("loans", loans::build),
        ("bonds", bonds::build),
        ("accounts", accounts::build),
        ("upkeep", upkeep::build),
        ("watchlist", watchlist::build),
    ];
    let mut all = Vec::new();
    for (name, build) in builders {
        match build(conn, today) {
            Ok(items) => all.extend(items),
            // One broken record must not hide every other reminder.
            Err(e) => log::warn!("[MILESTONES] {name} skipped: {e}"),
        }
    }
    let muted = muted_kinds(conn)?;
    all.retain(|m| !muted.contains(&m.kind));
    let states = load_states(conn)?;
    all.retain(|m| match states.get(&m.key) {
        Some((state, _)) if state == MILESTONE_STATE_DONE => false,
        Some((_, Some(until))) => *until <= today,
        _ => true,
    });
    all.sort_by(|a, b| {
        sort_key(a)
            .cmp(&sort_key(b))
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(all)
}

/// `now` first: dated items by deadline (else event day), then targets, then upkeep;
/// `soon` items by event day.
fn sort_key(m: &Milestone) -> (u8, u8, i64) {
    if m.stage == STAGE_SOON {
        return (1, 0, m.due_day.unwrap_or(i64::MAX));
    }
    let group = if m.due_day.is_some() {
        0
    } else if m.kind == "watch_target" {
        1
    } else {
        2
    };
    (0, group, m.action_day.or(m.due_day).unwrap_or(i64::MAX))
}

fn load_states(conn: &Connection) -> Result<HashMap<String, (String, Option<i64>)>> {
    let mut stmt = conn.prepare("SELECT key, state, until_day FROM milestone_states")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            (r.get::<_, String>(1)?, r.get::<_, Option<i64>>(2)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<HashMap<_, _>, _>>()?)
}

/// Mark one occurrence done (hidden for good) or snoozed (hidden for a week from `today`,
/// a month for upkeep kinds). Callers validate the arguments first
/// (`validate_milestone_state`).
pub fn set_milestone_state(conn: &Connection, key: &str, state: &str, today: i64) -> Result<()> {
    let key = key.trim();
    let kind = key.split(':').next().unwrap_or_default();
    let days = if UPKEEP_KINDS.contains(&kind) {
        UPKEEP_SNOOZE_DAYS
    } else {
        SNOOZE_DAYS
    };
    let until = (state == MILESTONE_STATE_SNOOZED).then_some(today + days * DAY);
    conn.execute(
        "INSERT INTO milestone_states (key, state, until_day, updated_at)
         VALUES (?1, ?2, ?3, unixepoch())
         ON CONFLICT(key) DO UPDATE SET state = excluded.state, until_day = excluded.until_day,
             updated_at = excluded.updated_at",
        params![key, state, until],
    )?;
    Ok(())
}

/// Forget the state of one occurrence (the toast's "Vrátit"); an unknown key is fine.
pub fn clear_milestone_state(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM milestone_states WHERE key = ?1", [key.trim()])?;
    Ok(())
}

/// Kinds the user turned off ("Nepřipomínat …"), sorted; empty when none or unreadable.
pub fn muted_kinds(conn: &Connection) -> Result<Vec<String>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [MUTED_KINDS_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw {
        None => Vec::new(),
        Some(json) => serde_json::from_str(&json).unwrap_or_else(|e| {
            log::warn!("[MILESTONES] muted kinds unreadable: {e}");
            Vec::new()
        }),
    })
}

/// Replace the muted kinds (callers validate them first). Stored trimmed, sorted and without
/// duplicates; an empty list removes the setting. Returns what was stored.
pub fn set_muted_kinds(conn: &Connection, kinds: &[String]) -> Result<Vec<String>> {
    let mut kinds: Vec<String> = kinds.iter().map(|k| k.trim().to_string()).collect();
    kinds.sort();
    kinds.dedup();
    if kinds.is_empty() {
        conn.execute("DELETE FROM app_config WHERE key = ?1", [MUTED_KINDS_KEY])?;
    } else {
        let json = serde_json::to_string(&kinds)
            .map_err(|e| AppError::Internal(format!("muted kinds: {e}")))?;
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![MUTED_KINDS_KEY, json],
        )?;
    }
    Ok(kinds)
}

#[cfg(test)]
mod tests {
    use super::test_support::{day, setup};
    use super::*;

    fn policy(
        conn: &Connection,
        id: &str,
        start: i64,
        end: Option<i64>,
        frequency: &str,
        payment: &str,
    ) {
        conn.execute(
            "INSERT INTO insurance_policies (id, policy_name, start_date, end_date, payment_frequency, regular_payment)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Pojistka {id}"), start, end, frequency, payment],
        )
        .expect("policy");
    }

    fn kinds(list: &[Milestone]) -> Vec<&str> {
        list.iter().map(|m| m.kind.as_str()).collect()
    }

    #[test]
    fn dated_stage_boundaries() {
        let today = day(2026, 10, 5);
        assert_eq!(dated_stage(today, today + 10 * DAY, today), Some(STAGE_NOW));
        assert_eq!(
            dated_stage(today, today + 10 * DAY, today + DAY),
            Some(STAGE_SOON)
        );
        assert_eq!(
            dated_stage(today, today + 90 * DAY, today + 50 * DAY),
            Some(STAGE_SOON)
        );
        assert_eq!(dated_stage(today, today + 91 * DAY, today + 50 * DAY), None);
    }

    #[test]
    fn an_anniversary_reminds_ten_weeks_ahead_with_the_notice_deadline() {
        let conn = setup();
        // Anniversary 24 Dec 2026; the insurance period ends 23 Dec, so the last safe
        // delivery day of the notice is 11 Nov (43 days earlier); reminder from 14 Oct.
        policy(&conn, "p1", day(2020, 12, 24), None, "monthly", "300");
        let before = list_milestones(&conn, day(2026, 10, 13)).unwrap();
        assert_eq!(kinds(&before), vec!["insurance_anniversary"]);
        assert_eq!(before[0].stage, STAGE_SOON);
        let on = list_milestones(&conn, day(2026, 10, 14)).unwrap();
        assert_eq!(on[0].stage, STAGE_NOW);
        assert_eq!(on[0].tone, TONE_ACTION);
        assert_eq!(on[0].due_day, Some(day(2026, 12, 24)));
        assert_eq!(on[0].action_day, Some(day(2026, 11, 11)));
        assert_eq!(
            on[0].key,
            format!("insurance_anniversary:p1:{}", day(2026, 12, 24))
        );
        let last_day = list_milestones(&conn, day(2026, 11, 11)).unwrap();
        assert_eq!(
            last_day[0].tone, TONE_ACTION,
            "the deadline day itself is safe"
        );
        let late = list_milestones(&conn, day(2026, 11, 12)).unwrap();
        assert_eq!(late[0].tone, TONE_INFO, "the notice period has passed");
    }

    #[test]
    fn a_one_time_policy_has_no_anniversary_but_keeps_its_end() {
        let conn = setup();
        // The notice-to-period-end rule applies to regular premiums only.
        policy(&conn, "o", day(2020, 12, 24), None, "one_time", "0");
        assert!(list_milestones(&conn, day(2026, 10, 20))
            .unwrap()
            .is_empty());
        policy(
            &conn,
            "oe",
            day(2020, 12, 24),
            Some(day(2027, 1, 5)),
            "one_time",
            "0",
        );
        let list = list_milestones(&conn, day(2026, 10, 20)).unwrap();
        assert_eq!(kinds(&list), vec!["insurance_end"]);
    }

    #[test]
    fn monthly_payments_are_routine_but_yearly_ones_remind_two_weeks_ahead() {
        let conn = setup();
        policy(&conn, "m", day(2025, 3, 20), None, "monthly", "300");
        policy(&conn, "y", day(2025, 10, 20), None, "annually", "4200");
        let list = list_milestones(&conn, day(2026, 10, 6)).unwrap();
        let payment: Vec<_> = list
            .iter()
            .filter(|m| m.kind == "insurance_payment")
            .collect();
        assert_eq!(payment.len(), 1);
        assert_eq!(payment[0].source_id.as_deref(), Some("y"));
        assert_eq!(payment[0].stage, STAGE_NOW);
        assert_eq!(payment[0].amount.as_deref(), Some("4200"));
        assert_eq!(payment[0].currency.as_deref(), Some("CZK"));
    }

    #[test]
    fn an_ending_contract_reminds_sixty_days_ahead_and_has_no_anniversary_after_it() {
        let conn = setup();
        policy(
            &conn,
            "e",
            day(2024, 1, 15),
            Some(day(2026, 12, 1)),
            "one_time",
            "0",
        );
        let list = list_milestones(&conn, day(2026, 10, 5)).unwrap();
        assert_eq!(kinds(&list), vec!["insurance_end"]);
        assert_eq!(list[0].stage, STAGE_NOW);
    }

    #[test]
    fn ended_and_inactive_policies_are_skipped() {
        let conn = setup();
        policy(
            &conn,
            "old",
            day(2020, 1, 1),
            Some(day(2026, 1, 1)),
            "annually",
            "100",
        );
        policy(&conn, "off", day(2020, 11, 1), None, "annually", "100");
        conn.execute(
            "UPDATE insurance_policies SET status = 'cancelled' WHERE id = 'off'",
            [],
        )
        .unwrap();
        assert!(list_milestones(&conn, day(2026, 10, 5)).unwrap().is_empty());
    }

    #[test]
    fn anniversaries_clamp_to_the_month_end() {
        let conn = setup();
        // Started 29 Feb 2024: the 2027 anniversary is 28 Feb.
        policy(&conn, "leap", day(2024, 2, 29), None, "monthly", "100");
        let list = list_milestones(&conn, day(2026, 12, 20)).unwrap();
        assert_eq!(list[0].due_day, Some(day(2027, 2, 28)));
    }

    #[test]
    fn done_hides_for_good_and_snooze_for_a_week() {
        let conn = setup();
        policy(&conn, "p1", day(2020, 12, 24), None, "monthly", "300");
        let today = day(2026, 10, 20);
        let key = list_milestones(&conn, today).unwrap()[0].key.clone();

        set_milestone_state(&conn, &key, MILESTONE_STATE_SNOOZED, today).unwrap();
        assert!(list_milestones(&conn, today + 6 * DAY).unwrap().is_empty());
        assert_eq!(list_milestones(&conn, today + 7 * DAY).unwrap().len(), 1);

        set_milestone_state(&conn, &key, MILESTONE_STATE_DONE, today).unwrap();
        assert!(list_milestones(&conn, today + 30 * DAY).unwrap().is_empty());

        clear_milestone_state(&conn, &key).unwrap();
        assert_eq!(list_milestones(&conn, today).unwrap().len(), 1);
    }

    #[test]
    fn now_items_come_first_by_deadline_then_soon_items() {
        let conn = setup();
        policy(&conn, "b", day(2020, 12, 24), None, "monthly", "1"); // now, deadline 11 Nov
        policy(&conn, "a", day(2020, 12, 10), None, "monthly", "1"); // now, deadline 28 Oct
        policy(&conn, "c", day(2021, 1, 2), None, "monthly", "1"); // soon (remind from 23 Oct)
        let list = list_milestones(&conn, day(2026, 10, 20)).unwrap();
        let ids: Vec<_> = list.iter().map(|m| m.source_id.clone().unwrap()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn now_items_are_ordered_dated_then_watch_target_then_upkeep() {
        let conn = setup();
        let today = day(2026, 10, 20);
        // Anniversary 24 Dec 2026 is inside its reminder window (from 14 Oct).
        policy(&conn, "p1", day(2020, 12, 24), None, "monthly", "300");
        // A crossed watch target and a profile that never made a backup.
        conn.execute(
            "INSERT INTO watched_stocks (id, ticker, target_price, target_direction)
             VALUES ('w', 'DIP', '600', 'below')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency) VALUES ('w', 'DIP', '598', 'USD')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 1, 1)],
        )
        .unwrap();
        let list = list_milestones(&conn, today).unwrap();
        assert_eq!(
            kinds(&list),
            vec!["insurance_anniversary", "watch_target", "backup_stale"]
        );
        assert!(list.iter().all(|m| m.stage == STAGE_NOW));
    }

    #[test]
    fn hiding_an_upkeep_item_lasts_until_the_data_changes() {
        let conn = setup();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 1, 1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', ?1)",
            [day(2026, 8, 1).to_string()],
        )
        .unwrap();
        let today = day(2026, 10, 5);
        let first = list_milestones(&conn, today).unwrap();
        assert_eq!(first[0].key, format!("backup_stale:{}", day(2026, 8, 1)));
        set_milestone_state(&conn, &first[0].key, MILESTONE_STATE_DONE, today).unwrap();
        assert!(list_milestones(&conn, today).unwrap().is_empty());
        // A newer backup that grows old again is a new occurrence.
        conn.execute(
            "UPDATE app_config SET value = ?1 WHERE key = 'backup.lastCreatedAt'",
            [day(2026, 8, 20).to_string()],
        )
        .unwrap();
        let again = list_milestones(&conn, today).unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].key, format!("backup_stale:{}", day(2026, 8, 20)));
    }

    #[test]
    fn upkeep_snoozes_for_a_month_and_dates_for_a_week() {
        let conn = setup();
        let today = day(2026, 10, 5);
        let dated = format!("insurance_end:p1:{}", day(2026, 11, 1));
        set_milestone_state(&conn, "backup_stale:never", MILESTONE_STATE_SNOOZED, today).unwrap();
        set_milestone_state(&conn, &dated, MILESTONE_STATE_SNOOZED, today).unwrap();
        let until = |key: &str| -> i64 {
            conn.query_row(
                "SELECT until_day FROM milestone_states WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(until("backup_stale:never"), today + 30 * DAY);
        assert_eq!(until(&dated), today + 7 * DAY);
    }

    #[test]
    fn muted_kinds_are_left_out_and_round_trip() {
        let conn = setup();
        policy(&conn, "y", day(2025, 10, 20), None, "annually", "4200");
        let today = day(2026, 10, 6);
        assert!(list_milestones(&conn, today)
            .unwrap()
            .iter()
            .any(|m| m.kind == "insurance_payment"));
        let stored = set_muted_kinds(
            &conn,
            &[
                "insurance_payment".to_string(),
                " insurance_payment ".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(stored, vec!["insurance_payment".to_string()]);
        assert_eq!(muted_kinds(&conn).unwrap(), stored);
        assert!(list_milestones(&conn, today)
            .unwrap()
            .iter()
            .all(|m| m.kind != "insurance_payment"));
        assert!(set_muted_kinds(&conn, &[]).unwrap().is_empty());
        assert!(muted_kinds(&conn).unwrap().is_empty());
    }

    #[test]
    fn unreadable_muted_kinds_mute_nothing() {
        let conn = setup();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('milestones.mutedKinds', 'not json')",
            [],
        )
        .unwrap();
        assert!(muted_kinds(&conn).unwrap().is_empty());
    }
}
