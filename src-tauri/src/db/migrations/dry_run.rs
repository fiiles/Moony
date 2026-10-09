//! Migration dry run (plan Task 26): applies `011_fx_eur_reference` and `012_retire_czk_caches`
//! to a throwaway COPY of a profile and checks that the values survive.
//!
//! `#[ignore]`d; driven by environment variables so no path or password lives in the source:
//!
//! ```text
//! cd src-tauri && MOONY_DRYRUN_DIR=<copy of a data dir> MOONY_DRYRUN_PASSWORD=<password> \
//!     cargo test dry_run -- --ignored --nocapture
//! ```
//!
//! The directory must hold `moony.db`, `salt`, `key.enc` (and `recovery.enc`). The test modifies
//! it in place, so always point it at a fresh copy, never at a real profile.

use super::{all_migrations, run_chain};
use crate::services::currency::breakdown_from_json;
use crate::services::{crypto, fx, net_worth};
use rusqlite::types::Value;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

const DAY: i64 = 86_400;
const CZK_TOLERANCE: f64 = 0.01;
const RATE_TOLERANCE: f64 = 1e-9;
/// Codes migration 011 deletes outright (derived, never stored).
const DROPPED: [&str; 6] = ["AED", "SAR", "QAR", "BHD", "OMR", "BGN"];
/// Breakdown columns in `portfolio_metrics_history`, with the old CZK total column of each class.
const CLASSES: [(&str, &str, &str); 7] = [
    ("savings", "savings_by_currency", "total_savings"),
    (
        "investments",
        "investments_by_currency",
        "total_investments",
    ),
    ("crypto", "crypto_by_currency", "total_crypto"),
    ("bonds", "bonds_by_currency", "total_bonds"),
    (
        "real estate",
        "real_estate_by_currency",
        "total_real_estate_personal + total_real_estate_investment",
    ),
    ("loans", "loans_by_currency", "total_loans_principal"),
    (
        "other assets",
        "other_assets_by_currency",
        "total_other_assets",
    ),
];

fn number(value: Value) -> f64 {
    match value {
        Value::Integer(i) => i as f64,
        Value::Real(r) => r,
        Value::Text(t) => t.trim().parse().unwrap_or(f64::NAN),
        _ => 0.0,
    }
}

fn floor_day(ts: i64) -> i64 {
    ts.div_euclid(DAY) * DAY
}

fn rel_err(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs()).max(f64::MIN_POSITIVE)
}

fn open_copy(dir: &Path, password: &str) -> Connection {
    let salt = crypto::read_salt(&dir.join("salt")).expect("read salt");
    let master = crypto::decrypt_master_key_with_password(password, &salt, &dir.join("key.enc"))
        .expect("decrypt the master key (wrong password?)");
    let conn = Connection::open(dir.join("moony.db")).expect("open moony.db");
    conn.pragma_update(
        None,
        "key",
        format!("x{}", crypto::master_key_to_hex(&master)),
    )
    .expect("set key");
    conn.execute("PRAGMA foreign_keys = ON", []).expect("fk");
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .expect("the key must unlock the database");
    conn
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .expect("count")
}

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table_info");
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .expect("columns")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("columns");
    names
}

/// `exchange_rate_history` as stored: day -> currency -> rate.
fn rate_history(conn: &Connection) -> BTreeMap<i64, HashMap<String, f64>> {
    let mut stmt = conn
        .prepare("SELECT date, currency, rate FROM exchange_rate_history")
        .expect("rate history");
    let mut days: BTreeMap<i64, HashMap<String, f64>> = BTreeMap::new();
    for row in stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .expect("rate rows")
    {
        let (date, currency, rate) = row.expect("rate row");
        days.entry(date).or_default().insert(currency, rate);
    }
    days
}

fn current_rates(conn: &Connection) -> HashMap<String, f64> {
    let mut stmt = conn
        .prepare("SELECT currency, rate FROM exchange_rates")
        .expect("current rates");
    let rates = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
        .expect("rate rows")
        .collect::<rusqlite::Result<HashMap<_, _>>>()
        .expect("rate rows");
    rates
}

/// One `portfolio_metrics_history` row as it was before 011/012.
struct OldRow {
    recorded_at: i64,
    /// Per class: `(name, old CZK total, parsed native breakdown)`.
    classes: Vec<(&'static str, f64, HashMap<String, f64>)>,
    personal: f64,
    investment: f64,
}

impl OldRow {
    /// Net worth as the old CZK totals stated it.
    fn old_net_worth(&self) -> f64 {
        self.classes
            .iter()
            .map(
                |(name, total, _)| {
                    if *name == "loans" {
                        -total
                    } else {
                        *total
                    }
                },
            )
            .sum()
    }

    /// A class counts as incomplete when its CZK total is non-zero but no breakdown amount is
    /// readable (the exact condition 012 rewrites).
    fn incomplete_classes(&self) -> Vec<&'static str> {
        self.classes
            .iter()
            .filter(|(_, total, breakdown)| *total != 0.0 && breakdown.is_empty())
            .map(|(name, _, _)| *name)
            .collect()
    }
}

fn old_rows(conn: &Connection) -> Vec<OldRow> {
    let select = CLASSES
        .iter()
        .map(|(_, breakdown, total)| format!("{breakdown}, ({total})"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT recorded_at, {select}, total_real_estate_personal, total_real_estate_investment \
         FROM portfolio_metrics_history ORDER BY recorded_at, rowid"
    );
    let mut stmt = conn.prepare(&sql).expect("old snapshot rows");
    let rows = stmt
        .query_map([], |r| {
            let recorded_at: i64 = r.get(0)?;
            let mut classes = Vec::new();
            for (i, (name, _, _)) in CLASSES.iter().enumerate() {
                let breakdown: String = r.get(1 + 2 * i)?;
                let total = number(r.get::<_, Value>(2 + 2 * i)?);
                classes.push((*name, total, breakdown_from_json(&breakdown)));
            }
            let personal = number(r.get::<_, Value>(1 + 2 * CLASSES.len())?);
            let investment = number(r.get::<_, Value>(2 + 2 * CLASSES.len())?);
            Ok(OldRow {
                recorded_at,
                classes,
                personal,
                investment,
            })
        })
        .expect("old rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("old rows");
    rows
}

/// CZK value of `amount` of `code` at the OLD (CZK per unit) rates of `day`, looking back as the
/// converter does; `None` when the code has no old quote within the window.
fn old_czk(
    history: &BTreeMap<i64, HashMap<String, f64>>,
    amount: f64,
    code: &str,
    day: i64,
) -> Option<f64> {
    if code == "CZK" {
        return Some(amount);
    }
    let start = floor_day(day);
    (0..=fx::MAX_LOOKBACK_DAYS).find_map(|back| {
        history
            .get(&(start - back * DAY))
            .and_then(|quotes| quotes.get(code))
            .map(|rate| amount * rate)
    })
}

/// Legacy shapes a seeded profile does not contain, added before the capture when
/// `MOONY_DRYRUN_EDGE_ROWS=1`: ticker history rows, snapshot rows with empty or unreadable
/// breakdowns, a foreign personal property, derived-currency quotes and a day without EUR.
/// All of it is synthetic and goes into the throwaway copy only.
fn add_edge_rows(conn: &Connection, first_day: i64) {
    let day = |n: i64| floor_day(first_day) - n * DAY;
    let mut sql = String::new();
    for n in 1..=3 {
        let d = day(n);
        sql.push_str(&format!(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES ({d}, 'EUR', 25.0), ({d}, 'USD', 22.5), ({d}, 'AED', 6.1);"
        ));
    }
    // a day with a USD quote only: cannot be rebased, expected to be dropped
    sql.push_str(&format!(
        "INSERT INTO exchange_rate_history (date, currency, rate) VALUES ({}, 'USD', 22.5);",
        day(4)
    ));
    let snapshot = |id: &str, at: i64, totals: [&str; 8], breakdowns: [&str; 7]| {
        format!(
            "INSERT INTO portfolio_metrics_history (id, total_savings, total_loans_principal, total_investments, total_crypto, \
             total_bonds, total_real_estate_personal, total_real_estate_investment, total_other_assets, recorded_at, \
             savings_by_currency, loans_by_currency, investments_by_currency, crypto_by_currency, bonds_by_currency, \
             real_estate_by_currency, other_assets_by_currency) VALUES ('{id}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', {at}, \
             '{}', '{}', '{}', '{}', '{}', '{}', '{}');",
            totals[0], totals[1], totals[2], totals[3], totals[4], totals[5], totals[6], totals[7],
            breakdowns[0], breakdowns[1], breakdowns[2], breakdowns[3], breakdowns[4], breakdowns[5], breakdowns[6],
        )
    };
    // order of totals: savings, loans, investments, crypto, bonds, re personal, re investment, other
    // 1: legacy row, no breakdowns at all
    sql.push_str(&snapshot(
        "edge-legacy",
        day(1),
        [
            "100000", "40000", "250000", "30000", "20000", "1500000", "0", "5000",
        ],
        ["{}", "{}", "{}", "{}", "{}", "{}", "{}"],
    ));
    // 2: unreadable JSON and a non-object value
    sql.push_str(&snapshot(
        "edge-corrupt",
        day(2),
        ["80000", "0", "120000", "0", "0", "0", "0", "0"],
        ["not json", "{}", "[1,2]", "{}", "{}", "{}", "{}"],
    ));
    // 3: foreign personal property (USD 1000 + EUR 400 = 32500 CZK at 22.5 / 25; 20000 personal)
    sql.push_str(&snapshot(
        "edge-foreign-re",
        day(3),
        ["50000", "0", "0", "0", "0", "20000", "12500", "0"],
        [
            "{\"CZK\": 50000}",
            "{}",
            "{}",
            "{}",
            "{}",
            "{\"USD\": 1000, \"EUR\": 400}",
            "{}",
        ],
    ));
    for (i, n) in (1..=2).enumerate() {
        sql.push_str(&format!(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency) VALUES ('edge-s{i}', 'AAPL', {}, '{}', '10', '{}', 'USD');",
            day(n),
            225.0,
            22.5
        ));
        sql.push_str(&format!(
            "INSERT INTO crypto_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency) VALUES ('edge-c{i}', 'BTC', {}, '1000000', '1', '40000', 'USD');",
            day(n)
        ));
    }
    conn.execute_batch(&sql).expect("edge rows");
    println!("added synthetic edge rows (legacy, corrupt, foreign property, derived quotes, ticker history)");
}

#[derive(Default)]
struct Tally {
    ok: usize,
    expected_native_wins: usize,
    unexplained: Vec<String>,
}

#[test]
#[ignore = "needs MOONY_DRYRUN_DIR and MOONY_DRYRUN_PASSWORD (a COPY of a profile)"]
fn fx_and_cache_migrations_preserve_values() {
    let (Ok(dir), Ok(password)) = (
        std::env::var("MOONY_DRYRUN_DIR"),
        std::env::var("MOONY_DRYRUN_PASSWORD"),
    ) else {
        println!(
            "dry run skipped: set MOONY_DRYRUN_DIR (a profile COPY) and MOONY_DRYRUN_PASSWORD"
        );
        return;
    };
    let conn = open_copy(Path::new(&dir), &password);

    // 1. Bring the copy to 010 (a no-op when it is already there) and check where it stands.
    let before_011: Vec<(&str, &str)> = all_migrations()
        .into_iter()
        .filter(|(name, _)| *name < "011_fx_eur_reference")
        .collect();
    run_chain(&conn, &before_011).expect("migrations up to 010");
    let applied = super::applied_migration_names(&conn).expect("applied names");
    println!("applied before the dry run: {}", applied.join(", "));
    assert_eq!(
        applied.last().map(String::as_str),
        Some("010_income_categories"),
        "the copy must stop at 010 before the dry run"
    );

    if std::env::var("MOONY_DRYRUN_EDGE_ROWS").as_deref() == Ok("1") {
        let first: i64 = conn
            .query_row(
                "SELECT MIN(recorded_at) FROM portfolio_metrics_history",
                [],
                |r| r.get(0),
            )
            .expect("first snapshot day");
        add_edge_rows(&conn, first);
    }

    // 2. Capture the state before 011.
    let old_history = rate_history(&conn);
    let old_current = current_rates(&conn);
    let old_snapshots = old_rows(&conn);
    let stock_rows = count(&conn, "stock_value_history");
    let crypto_rows = count(&conn, "crypto_value_history");
    let snapshot_count = count(&conn, "portfolio_metrics_history");

    // 3. Apply 011 and 012 through the real runner.
    run_chain(&conn, &all_migrations()).expect("011 and 012 apply");

    // 4. Rates.
    let new_history = rate_history(&conn);
    let new_current = current_rates(&conn);
    let mut rate_failures: Vec<String> = Vec::new();
    let mut days_with_eur = 0usize;
    let mut days_without_eur = 0usize;
    let mut quotes_checked = 0usize;
    let mut quotes_dropped = 0usize;
    let mut max_rel = 0.0f64;
    for (day, old) in &old_history {
        let Some(&eur) = old.get("EUR").filter(|r| **r > 0.0) else {
            days_without_eur += 1;
            if new_history.contains_key(day) {
                rate_failures.push(format!("day {day} had no EUR row but survived"));
            }
            continue;
        };
        days_with_eur += 1;
        let Some(new) = new_history.get(day) else {
            rate_failures.push(format!("day {day} was lost"));
            continue;
        };
        match new.get("CZK") {
            Some(czk) if rel_err(*czk, eur) < RATE_TOLERANCE => {}
            other => rate_failures.push(format!("day {day}: CZK {other:?} != old EUR {eur}")),
        }
        if new.contains_key("EUR") {
            rate_failures.push(format!("day {day}: an EUR row survived"));
        }
        for (code, old_rate) in old {
            if code == "EUR" || code == "CZK" {
                continue;
            }
            if DROPPED.contains(&code.as_str()) {
                quotes_dropped += 1;
                if new.contains_key(code) {
                    rate_failures.push(format!("day {day}: dropped {code} survived"));
                }
                continue;
            }
            if *old_rate <= 0.0 {
                continue;
            }
            let expected = eur / old_rate;
            quotes_checked += 1;
            match new.get(code) {
                Some(q) => {
                    let err = rel_err(*q, expected);
                    max_rel = max_rel.max(err);
                    if err >= RATE_TOLERANCE {
                        rate_failures.push(format!("day {day}: {code} {q} != {expected}"));
                    }
                }
                None => rate_failures.push(format!("day {day}: {code} quote lost")),
            }
        }
    }
    for day in new_history.keys() {
        if !old_history.contains_key(day) {
            rate_failures.push(format!("day {day} appeared from nowhere"));
        }
    }
    let mut current_failures: Vec<String> = Vec::new();
    match (old_current.get("EUR"), new_current.get("CZK")) {
        (Some(eur), Some(czk)) if rel_err(*eur, *czk) < RATE_TOLERANCE => {}
        (eur, czk) => current_failures.push(format!("current CZK {czk:?} != old EUR {eur:?}")),
    }
    if new_current.contains_key("EUR") {
        current_failures.push("an EUR current row survived".into());
    }
    if let Some(eur) = old_current.get("EUR") {
        for (code, old_rate) in &old_current {
            if code == "EUR"
                || code == "CZK"
                || DROPPED.contains(&code.as_str())
                || *old_rate <= 0.0
            {
                continue;
            }
            let expected = eur / old_rate;
            match new_current.get(code) {
                Some(q) if rel_err(*q, expected) < RATE_TOLERANCE => {}
                other => current_failures.push(format!("current {code}: {other:?} != {expected}")),
            }
        }
    }

    // 5. Snapshots.
    let new_snapshot_count = count(&conn, "portfolio_metrics_history");
    let (points, quality) = net_worth::history_in(&conn, "CZK", None, None).expect("history_in");
    println!("history_in quality: {quality:?}");
    assert_eq!(points.len() as i64, new_snapshot_count);
    assert_eq!(points.len(), old_snapshots.len());

    // The stored breakdowns after the migration, for the incomplete-row check.
    let select_new = CLASSES
        .iter()
        .map(|(_, breakdown, _)| *breakdown)
        .chain(["real_estate_personal_by_currency"])
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT recorded_at, {select_new} FROM portfolio_metrics_history ORDER BY recorded_at, rowid"
        ))
        .expect("new rows");
    let new_breakdowns: Vec<(i64, Vec<HashMap<String, f64>>)> = stmt
        .query_map([], |r| {
            let mut all = Vec::new();
            for i in 0..=CLASSES.len() {
                let json: String = r.get(1 + i)?;
                all.push(breakdown_from_json(&json));
            }
            Ok((r.get::<_, i64>(0)?, all))
        })
        .expect("new rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("new rows");

    let mut net = Tally::default();
    let mut personal = Tally::default();
    let mut incomplete_rows = 0usize;
    let mut incomplete_failures: Vec<String> = Vec::new();
    let mut complete_rows = 0usize;
    let mut max_net_diff = 0.0f64;
    let mut max_personal_diff = 0.0f64;
    // Before the migration: sum of |native breakdown at the old rates - CZK total| per class.
    let mut class_gap: BTreeMap<&str, f64> = BTreeMap::new();
    let mut rows_with_gap = 0usize;
    for (i, (old, point)) in old_snapshots.iter().zip(&points).enumerate() {
        assert_eq!(old.recorded_at, point.recorded_at, "row {i} order");
        let (_, new_all) = &new_breakdowns[i];
        let incomplete = old.incomplete_classes();

        if !incomplete.is_empty() {
            incomplete_rows += 1;
            for (c, (name, total, breakdown)) in old.classes.iter().enumerate() {
                if breakdown.is_empty() && *total != 0.0 {
                    let new = &new_all[c];
                    let ok = new.len() == 1
                        && new
                            .get("CZK")
                            .is_some_and(|v| (v - total).abs() <= CZK_TOLERANCE);
                    if !ok {
                        incomplete_failures.push(format!(
                            "row {i} ({}): {name} should be {{CZK: {total}}}, is {new:?}",
                            old.recorded_at
                        ));
                    }
                }
            }
            let re_empty = incomplete.contains(&"real estate");
            if re_empty && old.personal > 0.0 && old.investment >= 0.0 {
                let new = &new_all[CLASSES.len()];
                let ok = new
                    .get("CZK")
                    .is_some_and(|v| (v - old.personal).abs() <= CZK_TOLERANCE);
                if !ok {
                    incomplete_failures.push(format!(
                        "row {i} ({}): personal real estate should be {{CZK: {}}}, is {new:?}",
                        old.recorded_at, old.personal
                    ));
                }
            }
        } else {
            complete_rows += 1;
        }

        if incomplete.is_empty() {
            let mut row_gap = false;
            for (name, total, breakdown) in &old.classes {
                let converted: Option<f64> = breakdown
                    .iter()
                    .map(|(code, amount)| old_czk(&old_history, *amount, code, old.recorded_at))
                    .sum();
                if let Some(c) = converted {
                    let gap = (c - total).abs();
                    if gap > CZK_TOLERANCE {
                        *class_gap.entry(name).or_default() += gap;
                        row_gap = true;
                    }
                }
            }
            rows_with_gap += usize::from(row_gap);
        }

        // Net worth in CZK from the migrated breakdowns at the migrated rates versus the old totals.
        let Some(new_nw) = point.net_worth else {
            net.unexplained.push(format!(
                "row {i} ({}): net worth is unknown",
                old.recorded_at
            ));
            continue;
        };
        let old_nw = old.old_net_worth();
        let diff = (new_nw - old_nw).abs();
        if incomplete.is_empty() {
            max_net_diff = max_net_diff.max(diff);
        }
        if diff <= CZK_TOLERANCE {
            net.ok += 1;
        } else if incomplete.is_empty() {
            // Did the old breakdowns, at the old rates, already disagree with the old totals?
            let from_breakdowns: Option<f64> = old
                .classes
                .iter()
                .map(|(name, _, breakdown)| {
                    let sum: Option<f64> = breakdown
                        .iter()
                        .map(|(code, amount)| old_czk(&old_history, *amount, code, old.recorded_at))
                        .sum();
                    sum.map(|s| if *name == "loans" { -s } else { s })
                })
                .sum();
            match from_breakdowns {
                Some(b) if (new_nw - b).abs() <= CZK_TOLERANCE => net.expected_native_wins += 1,
                other => net.unexplained.push(format!(
                    "row {i} ({}): new {new_nw:.2} vs old totals {old_nw:.2} (diff {diff:.2}); old breakdowns at old rates {other:?}",
                    old.recorded_at
                )),
            }
        } else {
            net.unexplained.push(format!(
                "row {i} ({}) incomplete: new {new_nw:.2} vs old totals {old_nw:.2} (diff {diff:.2})",
                old.recorded_at
            ));
        }

        // Personal real estate split.
        if old.personal > 0.0 && old.investment >= 0.0 {
            match point.classes.real_estate_personal {
                Some(p) => {
                    let d = (p - old.personal).abs();
                    if incomplete.is_empty() {
                        max_personal_diff = max_personal_diff.max(d);
                    }
                    if d <= CZK_TOLERANCE {
                        personal.ok += 1;
                    } else if diff > CZK_TOLERANCE && incomplete.is_empty() {
                        // the whole row moved with the breakdowns (native wins)
                        personal.expected_native_wins += 1;
                    } else {
                        personal.unexplained.push(format!(
                            "row {i} ({}): personal {p:.2} vs old {:.2} (diff {d:.2}, net worth diff {diff:.2})",
                            old.recorded_at, old.personal
                        ));
                    }
                }
                None => personal.unexplained.push(format!(
                    "row {i} ({}): personal real estate unknown",
                    old.recorded_at
                )),
            }
        }
    }

    // 6. Value-history tables and dropped columns.
    let new_stock_rows = count(&conn, "stock_value_history");
    let new_crypto_rows = count(&conn, "crypto_value_history");
    let mut column_failures: Vec<String> = Vec::new();
    for table in ["stock_value_history", "crypto_value_history"] {
        if columns(&conn, table).iter().any(|c| c == "value_czk") {
            column_failures.push(format!("{table}.value_czk still exists"));
        }
    }
    let snapshot_columns = columns(&conn, "portfolio_metrics_history");
    if snapshot_columns.iter().any(|c| c.starts_with("total_")) {
        column_failures.push(format!("total_* columns remain: {snapshot_columns:?}"));
    }
    let reference: String = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = 'fx_reference'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();

    // 7. Summary.
    println!();
    println!("================ MIGRATION DRY RUN (011 + 012) ================");
    println!("{:<46} {:>12}", "check", "result");
    println!("{}", "-".repeat(60));
    let row = |label: &str, value: String| println!("{label:<46} {value:>12}");
    row("rate history days (old)", old_history.len().to_string());
    row("  days with an EUR row", days_with_eur.to_string());
    row(
        "  days without an EUR row (dropped)",
        days_without_eur.to_string(),
    );
    row("  quotes verified q(c)=EUR/c", quotes_checked.to_string());
    row("  derived quotes dropped", quotes_dropped.to_string());
    row("  max relative error", format!("{max_rel:.2e}"));
    row("  rate failures", rate_failures.len().to_string());
    row(
        "current rates (old / new rows)",
        format!("{}/{}", old_current.len(), new_current.len()),
    );
    row(
        "  current-rate failures",
        current_failures.len().to_string(),
    );
    row(
        "snapshot rows (before / after)",
        format!("{snapshot_count}/{new_snapshot_count}"),
    );
    row("  rows with complete breakdowns", complete_rows.to_string());
    row(
        "  rows with an empty class breakdown",
        incomplete_rows.to_string(),
    );
    row(
        "  empty-breakdown failures",
        incomplete_failures.len().to_string(),
    );
    row(
        "net worth within 0.01 CZK of old totals",
        net.ok.to_string(),
    );
    row(
        "  differs, old breakdowns disagreed already",
        net.expected_native_wins.to_string(),
    );
    row("  differs, UNEXPLAINED", net.unexplained.len().to_string());
    row(
        "  max diff on complete rows (CZK)",
        format!("{max_net_diff:.6}"),
    );
    row("personal real estate within 0.01", personal.ok.to_string());
    row(
        "  differs, native wins",
        personal.expected_native_wins.to_string(),
    );
    row(
        "  differs, UNEXPLAINED",
        personal.unexplained.len().to_string(),
    );
    row(
        "  max diff on complete rows (CZK)",
        format!("{max_personal_diff:.6}"),
    );
    row(
        "stock_value_history rows (before/after)",
        format!("{stock_rows}/{new_stock_rows}"),
    );
    row(
        "crypto_value_history rows (before/after)",
        format!("{crypto_rows}/{new_crypto_rows}"),
    );
    row("column failures", column_failures.len().to_string());
    row("app_config fx_reference", reference.clone());
    println!("================================================================");
    println!(
        "before the migration {rows_with_gap} complete rows already had a breakdown that disagreed with its CZK total; sum of |gap| per class (CZK): {class_gap:?}"
    );
    let sections: [(&str, &Vec<String>); 6] = [
        ("RATE FAILURES", &rate_failures),
        ("CURRENT RATE FAILURES", &current_failures),
        ("EMPTY BREAKDOWN FAILURES", &incomplete_failures),
        ("NET WORTH UNEXPLAINED", &net.unexplained),
        ("PERSONAL REAL ESTATE UNEXPLAINED", &personal.unexplained),
        ("COLUMN FAILURES", &column_failures),
    ];
    for (title, lines) in sections {
        if !lines.is_empty() {
            println!("--- {title} (first 20 of {}) ---", lines.len());
            for line in lines.iter().take(20) {
                println!("{line}");
            }
        }
    }

    assert_eq!(reference, "EUR");
    assert!(rate_failures.is_empty(), "rate failures");
    assert!(current_failures.is_empty(), "current rate failures");
    assert_eq!(snapshot_count, new_snapshot_count);
    assert!(incomplete_failures.is_empty(), "empty-breakdown failures");
    assert!(
        net.unexplained.is_empty(),
        "unexplained net worth differences"
    );
    assert!(
        personal.unexplained.is_empty(),
        "unexplained personal real estate differences"
    );
    assert_eq!(stock_rows, new_stock_rows);
    assert_eq!(crypto_rows, new_crypto_rows);
    assert!(column_failures.is_empty(), "column failures");
}
