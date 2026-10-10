//! One ledger interpretation for actual reports and their drilldowns.
use crate::error::{AppError, Result};
use crate::models::{
    EffectiveTransactionFlow, FlowKind, FxQuality, ReportQuality, ReportTransaction,
};
use crate::services::financial_semantics::{
    resolve_structured_flow, signed_flow_amount, AccountIdentities,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

pub fn parse_money(text: &str) -> Result<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| AppError::Validation("validation.invalidAmount".into()))
}
pub fn money(amount: f64) -> String {
    format!("{:.2}", if amount.abs() < 0.005 { 0.0 } else { amount })
}
pub fn month_start(timestamp: i64) -> Option<i64> {
    let d = DateTime::<Utc>::from_timestamp(timestamp, 0)?;
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1)?
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc().timestamp())
}
/// Uses only recorded rates on or before booking (up to ten days for weekends).
/// DB failures propagate; absence is explicit and no global 1:1 fallback is used.
/// An unusable stored rate (not finite or not positive) counts as missing.
pub fn historical_czk(
    conn: &Connection,
    amount: f64,
    currency: &str,
    day: i64,
) -> Result<Option<f64>> {
    historical_amount(conn, amount, currency, "CZK", day)
}
/// Currency codes are normalized at the reporting boundary; unavailable quotes
/// remain missing even for syntactically valid currencies outside the provider.
pub fn normalize_currency(currency: &str) -> Result<String> {
    let currency = currency.trim().to_ascii_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|b| b.is_ascii_alphabetic()) {
        return Err(AppError::Validation("validation.currencyInvalid".into()));
    }
    Ok(currency)
}
/// ISO 4217 minor units of the money contract, shared with the frontend report
/// formatter. Codes without an ISO minor unit (metals, XDR, XXX) or outside the
/// list keep two decimals. This is wire precision, not the cash style of the UI.
static ISO_MINOR_UNITS: LazyLock<HashMap<String, usize>> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    struct Table {
        exceptions: HashMap<String, usize>,
    }
    serde_json::from_str::<Table>(include_str!("../../../shared/iso4217-minor-units.json"))
        .expect("shared/iso4217-minor-units.json is valid")
        .exceptions
});
pub fn minor_units(currency: &str) -> usize {
    ISO_MINOR_UNITS.get(currency).copied().unwrap_or(2)
}
pub fn money_in_currency(amount: f64, currency: &str) -> String {
    let decimals = minor_units(currency);
    let scale = 10_f64.powi(decimals as i32);
    let rounded = (amount * scale).round() / scale;
    format!(
        "{rounded:.decimals$}",
        rounded = if rounded == 0.0 { 0.0 } else { rounded }
    )
}
/// Cross quotes must share a valid day; never combine separate last-known dates.
/// Same-currency facts bypass database/rate access, including non-provider codes.
pub fn historical_amount(
    conn: &Connection,
    amount: f64,
    from: &str,
    target: &str,
    day: i64,
) -> Result<Option<f64>> {
    let from = normalize_currency(from)?;
    let target = normalize_currency(target)?;
    if from == target {
        return Ok(amount.is_finite().then_some(amount));
    }
    let day = day.div_euclid(86400) * 86400;
    let lower = day.saturating_sub(10 * 86400);
    let sql = if from == "CZK" {
        "SELECT 1.0,rate FROM exchange_rate_history WHERE UPPER(currency)=?2 AND date<=?3 AND date>=?4 AND rate>0 ORDER BY date DESC"
    } else if target == "CZK" {
        "SELECT rate,1.0 FROM exchange_rate_history WHERE UPPER(currency)=?1 AND date<=?3 AND date>=?4 AND rate>0 ORDER BY date DESC"
    } else {
        "SELECT s.rate,t.rate FROM exchange_rate_history s JOIN exchange_rate_history t ON s.date=t.date WHERE UPPER(s.currency)=?1 AND UPPER(t.currency)=?2 AND s.date<=?3 AND s.date>=?4 AND s.rate>0 AND t.rate>0 ORDER BY s.date DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params![from, target, day, lower])?;
    while let Some(row) = rows.next()? {
        let source: f64 = row.get(0)?;
        let target: f64 = row.get(1)?;
        if source.is_finite() && target.is_finite() && source > 0.0 && target > 0.0 {
            let converted = amount * source / target;
            return Ok(converted.is_finite().then_some(converted));
        }
    }
    Ok(None)
}

pub fn effective_flows(conn: &Connection, ids: &[String]) -> Result<Vec<EffectiveTransactionFlow>> {
    let own = AccountIdentities::load(conn)?;
    let mut out = vec![];
    let mut stmt=conn.prepare("SELECT bt.id,bt.flow_kind,bt.counterparty_iban,tc.kind,bt.tx_type,bt.counterparty_bank_code,bt.counterparty_country FROM bank_transactions bt LEFT JOIN transaction_categories tc ON tc.id=bt.category_id WHERE bt.id=?1")?;
    for id in ids {
        let row = stmt
            .query_row([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<FlowKind>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<FlowKind>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                ))
            })
            .optional()?;
        if let Some((id, explicit, identifier, category, direction, bank_code, country)) = row {
            let flow = resolve_structured_flow(
                explicit,
                identifier.as_deref(),
                bank_code.as_deref(),
                country.as_deref(),
                category,
                direction.eq_ignore_ascii_case("credit"),
                &own,
            );
            out.push(EffectiveTransactionFlow {
                id,
                effective_flow_kind: flow.kind,
                flow_source: flow.source,
            });
        }
    }
    Ok(out)
}
pub fn transactions(
    conn: &Connection,
    start: i64,
    end: i64,
    accounts: Option<&[String]>,
) -> Result<Vec<ReportTransaction>> {
    transactions_in_currency(conn, start, end, accounts, "CZK")
}
pub fn transactions_in_currency(
    conn: &Connection,
    start: i64,
    end: i64,
    accounts: Option<&[String]>,
    report_currency: &str,
) -> Result<Vec<ReportTransaction>> {
    let report_currency = normalize_currency(report_currency)?;
    let own = AccountIdentities::load(conn)?;
    let mut stmt=conn.prepare("SELECT bt.id,bt.booking_date,bt.amount,bt.currency,bt.description,bt.counterparty_name,bt.counterparty_iban,bt.category_id,bt.bank_account_id,ba.name,bt.tx_type,bt.flow_kind,tc.kind,bt.counterparty_bank_code,bt.counterparty_country,bt.remittance_info FROM bank_transactions bt JOIN bank_accounts ba ON ba.id=bt.bank_account_id LEFT JOIN transaction_categories tc ON tc.id=bt.category_id WHERE bt.booking_date>=?1 AND bt.booking_date<=?2 AND bt.status='booked' ORDER BY bt.booking_date DESC,bt.id")?;
    let rows = stmt.query_map(params![start, end], |r| {
        Ok((
            ReportTransaction {
                id: r.get(0)?,
                booking_date: r.get(1)?,
                amount: r.get(2)?,
                currency: r.get(3)?,
                description: r.get(4)?,
                remittance_info: r.get(15)?,
                counterparty_name: r.get(5)?,
                counterparty_iban: r.get(6)?,
                counterparty_bank_code: r.get(13)?,
                counterparty_country: r.get(14)?,
                category_id: r.get(7)?,
                bank_account_id: r.get(8)?,
                bank_account_name: r.get(9)?,
                tx_type: r.get(10)?,
                effective_flow_kind: FlowKind::Expense,
                flow_source: crate::models::reporting::FlowSource::Direction,
                signed_amount: None,
                report_currency: report_currency.clone(),
                fx_quality: FxQuality::Missing,
            },
            r.get::<_, Option<FlowKind>>(11)?,
            r.get::<_, Option<FlowKind>>(12)?,
        ))
    })?;
    let mut out = vec![];
    for row in rows {
        let (mut row, explicit, category) = row?;
        if accounts.is_some_and(|ids| !ids.contains(&row.bank_account_id)) {
            continue;
        }
        let credit = row.tx_type.eq_ignore_ascii_case("credit");
        let flow = resolve_structured_flow(
            explicit,
            row.counterparty_iban.as_deref(),
            row.counterparty_bank_code.as_deref(),
            row.counterparty_country.as_deref(),
            category,
            credit,
            &own,
        );
        row.effective_flow_kind = flow.kind;
        row.flow_source = flow.source;
        // A legacy non-finite amount is excluded and counted by `quality`,
        // so one bad row cannot make every report fail.
        let Ok(amount) = parse_money(&row.amount).map(f64::abs) else {
            out.push(row);
            continue;
        };
        if let Some(czk) = historical_amount(
            conn,
            amount,
            &row.currency,
            &report_currency,
            row.booking_date,
        )? {
            row.signed_amount = Some(money_in_currency(
                signed_flow_amount(flow.kind, credit, czk),
                &report_currency,
            ));
            row.fx_quality = FxQuality::Historical;
        }
        out.push(row);
    }
    Ok(out)
}
pub fn quality(rows: &[ReportTransaction], start: i64, end: i64, today: i64) -> ReportQuality {
    let mut q = ReportQuality::default();
    let mut observed = HashSet::new();
    for row in rows {
        if parse_money(&row.amount).is_err() {
            q.invalid_amount_count += 1;
        } else if row.fx_quality == FxQuality::Missing {
            q.missing_fx_count += 1;
            if !q.missing_fx_currencies.contains(&row.currency) {
                q.missing_fx_currencies.push(row.currency.clone());
            }
        }
        if let Some(m) = month_start(row.booking_date) {
            observed.insert(m);
        }
    }
    if let (Some(first), Some(last), Some(current)) =
        (month_start(start), month_start(end), month_start(today))
    {
        let mut month = first;
        while month <= last {
            let Some(d) = DateTime::<Utc>::from_timestamp(month, 0) else {
                break;
            };
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            let Some(next) = NaiveDate::from_ymd_opt(y, m, 1)
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|d| d.and_utc().timestamp())
            else {
                break;
            };
            if month < start || next - 1 > end || month >= current {
                q.partial_month_count += 1;
            } else if observed.contains(&month) {
                q.observed_completed_months.push(month);
            } else {
                q.unobserved_month_count += 1;
            }
            month = next;
        }
    }
    q.missing_fx_currencies.sort();
    q
}

/// Optional filters used by cashflow drilldowns. FX-missing rows remain visible
/// with a null contribution; they are not silently counted in numeric totals.
pub fn filtered_transactions_in_currency(
    conn: &Connection,
    start: i64,
    end: i64,
    accounts: Option<&[String]>,
    kind: Option<FlowKind>,
    category: Option<&str>,
    report_currency: &str,
) -> Result<Vec<ReportTransaction>> {
    Ok(
        transactions_in_currency(conn, start, end, accounts, report_currency)?
            .into_iter()
            .filter(|r| {
                kind.is_none_or(|kind| r.effective_flow_kind == kind)
                    && category
                        .is_none_or(|id| r.category_id.as_deref().unwrap_or("uncategorized") == id)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE bank_accounts(id TEXT,name TEXT,iban TEXT,bban TEXT,institution_id TEXT); CREATE TABLE institutions(id TEXT,country TEXT);
  CREATE TABLE transaction_categories(id TEXT,name TEXT,kind TEXT);
  CREATE TABLE bank_transactions(id TEXT,booking_date INTEGER,amount TEXT,currency TEXT,description TEXT,remittance_info TEXT,counterparty_name TEXT,counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT, import_line INTEGER, row_balance TEXT,category_id TEXT,bank_account_id TEXT,tx_type TEXT,flow_kind TEXT,status TEXT);
  CREATE TABLE exchange_rate_history(date INTEGER,currency TEXT,rate REAL);
  INSERT INTO bank_accounts(id,name,iban,bban) VALUES ('a','Main',NULL,NULL),('b','Excluded savings','CZ6508000000192000145399',NULL);
  INSERT INTO transaction_categories VALUES ('custom','Salary','income');
  INSERT INTO bank_transactions(id,booking_date,amount,currency,description,counterparty_name,counterparty_iban,category_id,bank_account_id,tx_type,flow_kind,status) VALUES ('own',1704067200,'100','CZK',NULL,NULL,'19-2000145399/0800','custom','a','credit',NULL,'booked'),
   ('manual',1704067200,'100','CZK',NULL,NULL,'19-2000145399/0800','custom','a','credit','income','booked'),
   ('pending',1704067200,'1000','CZK',NULL,NULL,NULL,'custom','a','credit',NULL,'pending'),
   ('foreign',1704067200,'100','ZZZ',NULL,NULL,NULL,'custom','a','credit',NULL,'booked');").unwrap();
        conn
    }
    #[test]
    fn target_precision_rows_remain_visible_and_sum_at_minor_units() {
        let conn = db();
        conn.execute("DELETE FROM bank_transactions", []).unwrap();
        for (id, amount, currency) in [
            ("a", "0.1234", "KWD"),
            ("b", "0.1234", "KWD"),
            ("missing", "7", "USD"),
        ] {
            conn.execute("INSERT INTO bank_transactions(id,booking_date,amount,currency,bank_account_id,tx_type,status) VALUES (?1,1704067200,?2,?3,'a','debit','booked')", params![id,amount,currency]).unwrap();
        }
        let rows = transactions_in_currency(&conn, 0, i64::MAX, None, "kwd").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .filter_map(|r| r.signed_amount.as_deref())
                .map(|a| parse_money(a).unwrap())
                .sum::<f64>(),
            0.246
        );
        assert!(rows.iter().all(|r| r.report_currency == "KWD"));
        assert!(rows
            .iter()
            .find(|r| r.id == "missing")
            .unwrap()
            .signed_amount
            .is_none());
        assert_eq!(money_in_currency(123.6, "JPY"), "124");
        assert_eq!(money_in_currency(-0.0001, "KWD"), "0.000");
        assert_eq!(money_in_currency(0.123, "KWD"), "0.123");
        assert_eq!(money_in_currency(1.234, "EUR"), "1.23");
        assert_eq!(normalize_currency(" eur ").unwrap(), "EUR");
        assert!(normalize_currency("EURO").is_err());
    }
    #[test]
    fn minor_units_follow_the_iso_list_not_the_ui_cash_style() {
        // ISO exceptions beyond the currency picker (SIX List One, 2026-09-17).
        for (code, units) in [("JOD", 3), ("TND", 3), ("UGX", 0), ("XOF", 0), ("CLF", 4)] {
            assert_eq!(minor_units(code), units, "{code}");
        }
        // CZK/HUF keep ISO two decimals even though the UI hides cash subunits.
        assert_eq!(money_in_currency(200.8, "CZK"), "200.80");
        assert_eq!(money_in_currency(1.2345, "JOD"), "1.235");
        assert_eq!(money_in_currency(1500.4, "UGX"), "1500");
        // No ISO minor unit (gold) or an unlisted code: two decimals.
        assert_eq!(minor_units("XAU"), 2);
        assert_eq!(minor_units("ZZZ"), 2);
    }
    #[test]
    fn target_historical_cross_rates_ignore_live_changes_and_old_quotes() {
        let conn = db();
        conn.execute_batch("INSERT INTO exchange_rate_history VALUES (1704067200,'XHU',25),(1704067200,'XHE',27.5);").unwrap();
        let before = historical_amount(&conn, 110.0, "XHU", "XHE", 1704067200).unwrap();
        crate::services::currency::update_exchange_rates(std::collections::HashMap::from([(
            "XHL".into(),
            99.0,
        )]));
        assert_eq!(
            historical_amount(&conn, 110.0, "XHU", "XHE", 1704067200).unwrap(),
            before
        );
        assert_eq!(
            historical_amount(&conn, 110.0, "XHU", "XHE", 1704067200 + 11 * 86400).unwrap(),
            None
        );
        assert_eq!(
            historical_amount(&conn, 110.0, "XHU", "AED", 1704067200).unwrap(),
            None
        );
        conn.execute("DROP TABLE exchange_rate_history", [])
            .unwrap();
        assert!(historical_amount(&conn, 110.0, "XHU", "XHE", 1704067200).is_err());
    }
    #[test]
    fn target_native_currency_bypasses_fx_even_without_rate_schema() {
        let conn = Connection::open_in_memory().unwrap();
        for currency in ["EUR", "AED", "KWD", "JPY"] {
            assert_eq!(
                historical_amount(&conn, 100.0, currency, currency, 1704067200).unwrap(),
                Some(100.0)
            );
        }
    }
    #[test]
    fn target_cross_rates_use_one_common_historical_day() {
        let conn = db();
        conn.execute_batch("INSERT INTO exchange_rate_history VALUES (1704067200,'USD',25),(1704067200,'EUR',27.5);").unwrap();
        assert_eq!(
            historical_amount(&conn, 110.0, "USD", "EUR", 1704067200).unwrap(),
            Some(100.0)
        );
        conn.execute_batch("DELETE FROM exchange_rate_history; INSERT INTO exchange_rate_history VALUES (1704067200,'USD',25),(1703980800,'EUR',27.5);").unwrap();
        assert_eq!(
            historical_amount(&conn, 110.0, "USD", "EUR", 1704067200).unwrap(),
            None
        );
    }
    #[test]
    fn integrity_structured_country_is_shared_by_report_and_effective_reads() {
        let conn = db();
        conn.execute("UPDATE bank_transactions SET counterparty_iban='19-2000145399',counterparty_bank_code='0800',counterparty_country='CZ' WHERE id='own'",[]).unwrap();
        let a = effective_flows(&conn, &["own".into()]).unwrap();
        assert_eq!(a[0].effective_flow_kind, FlowKind::Transfer);
        conn.execute(
            "UPDATE bank_transactions SET counterparty_country='DE' WHERE id='own'",
            [],
        )
        .unwrap();
        let a = effective_flows(&conn, &["own".into()]).unwrap();
        assert_eq!(a[0].effective_flow_kind, FlowKind::Income);
        let a = transactions(&conn, 0, i64::MAX, None).unwrap();
        assert_eq!(
            a.iter()
                .find(|r| r.id == "own")
                .unwrap()
                .effective_flow_kind,
            FlowKind::Income
        );
        conn.execute("UPDATE bank_transactions SET flow_kind='expense',counterparty_country='CZ' WHERE id='own'",[]).unwrap();
        let a = effective_flows(&conn, &["own".into()]).unwrap();
        assert_eq!(a[0].effective_flow_kind, FlowKind::Expense);
    }

    #[test]
    fn rows_carry_the_remittance_info_for_drilldowns() {
        let conn = db();
        conn.execute(
            "UPDATE bank_transactions SET remittance_info='Faktura 42' WHERE id='own'",
            [],
        )
        .unwrap();
        let rows = transactions(&conn, 0, i64::MAX, None).unwrap();
        let own = rows.iter().find(|r| r.id == "own").unwrap();
        assert_eq!(own.remittance_info.as_deref(), Some("Faktura 42"));
    }

    #[test]
    fn booked_scoped_rows_share_effective_meaning_and_flag_missing_currency() {
        let conn = db();
        let rows = transactions(&conn, 1704067200, 1706745599, Some(&["a".into()])).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(transactions(&conn, 0, i64::MAX, Some(&[]))
            .unwrap()
            .is_empty());
        let own = rows.iter().find(|r| r.id == "own").unwrap();
        assert_eq!(own.effective_flow_kind, FlowKind::Transfer);
        assert_eq!(own.flow_source, crate::models::FlowSource::OwnAccount);
        let explicit = rows.iter().find(|r| r.id == "manual").unwrap();
        assert_eq!(explicit.effective_flow_kind, FlowKind::Income);
        let foreign = rows.iter().find(|r| r.id == "foreign").unwrap();
        assert_eq!(foreign.fx_quality, FxQuality::Missing);
        assert!(foreign.signed_amount.is_none());
        let q = quality(&rows, 1704067200, 1706745599, 1709251200);
        assert_eq!(q.missing_fx_count, 1);
        assert_eq!(q.observed_completed_months, vec![1704067200]);
        let flows = effective_flows(&conn, &["pending".into(), "own".into()]).unwrap();
        assert_eq!(flows.len(), 2);
        assert_eq!(flows[1].effective_flow_kind, FlowKind::Transfer);
    }
    /// Final review M-6: one legacy non-finite amount or unusable stored rate
    /// must not make every report fail; it is excluded and reported.
    #[test]
    fn legacy_invalid_amounts_and_rates_are_excluded_not_fatal() {
        let conn = db();
        conn.execute_batch(
            "INSERT INTO bank_transactions(id,booking_date,amount,currency,bank_account_id,tx_type,status) VALUES
               ('inf',1704067200,'inf','CZK','a','debit','booked'),
               ('nan',1704067200,'NaN','CZK','a','debit','booked'),
               ('badrate',1704067200,'10','YYY','a','debit','booked');
             INSERT INTO exchange_rate_history VALUES (1704067200,'YYY',0.0);",
        )
        .unwrap();
        let rows = transactions(&conn, 0, i64::MAX, None).expect("report stays usable");
        for id in ["inf", "nan", "badrate"] {
            let row = rows.iter().find(|r| r.id == id).unwrap();
            assert!(row.signed_amount.is_none(), "{id}");
        }
        let q = quality(&rows, 1704067200, 1706745599, 1709251200);
        assert_eq!(q.invalid_amount_count, 2);
        assert_eq!(
            q.missing_fx_count, 2,
            "ZZZ without a rate and YYY with an unusable one"
        );
        assert_eq!(q.missing_fx_currencies, vec!["YYY", "ZZZ"]);
    }

    #[test]
    fn strict_fx_uses_recorded_rates_and_propagates_schema_errors() {
        let conn = db();
        assert_eq!(
            historical_czk(&conn, 10.0, "ZZZ", 1704067200).unwrap(),
            None
        );
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (1704067200,'ZZZ',25.0)",
            [],
        )
        .unwrap();
        assert_eq!(
            historical_czk(&conn, 10.0, "ZZZ", 1704067200 + 86400).unwrap(),
            Some(250.0)
        );
        assert_eq!(
            historical_czk(&conn, 10.0, "ZZZ", 1704067200 + 11 * 86400).unwrap(),
            None
        );
        conn.execute("DROP TABLE exchange_rate_history", [])
            .unwrap();
        assert!(historical_czk(&conn, 10.0, "ZZZ", 1704067200).is_err());
    }
}
