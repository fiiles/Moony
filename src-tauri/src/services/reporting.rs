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
use std::collections::HashSet;

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
pub fn historical_czk(
    conn: &Connection,
    amount: f64,
    currency: &str,
    day: i64,
) -> Result<Option<f64>> {
    if currency.eq_ignore_ascii_case("CZK") {
        return Ok(Some(amount));
    }
    let day = day.div_euclid(86400) * 86400;
    let rate: Option<f64> = conn.query_row(
        "SELECT rate FROM exchange_rate_history WHERE UPPER(currency)=UPPER(?1) AND date <= ?2 AND date >= ?3 ORDER BY date DESC LIMIT 1",
        params![currency,day,day.saturating_sub(10*86400)], |r|r.get(0)).optional()?;
    match rate {
        Some(rate) if rate.is_finite() && rate > 0.0 && (amount * rate).is_finite() => {
            Ok(Some(amount * rate))
        }
        Some(_) => Err(AppError::Validation("validation.invalidAmount".into())),
        None => Ok(None),
    }
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
    let own = AccountIdentities::load(conn)?;
    let mut stmt=conn.prepare("SELECT bt.id,bt.booking_date,bt.amount,bt.currency,bt.description,bt.counterparty_name,bt.counterparty_iban,bt.category_id,bt.bank_account_id,ba.name,bt.tx_type,bt.flow_kind,tc.kind,bt.counterparty_bank_code,bt.counterparty_country FROM bank_transactions bt JOIN bank_accounts ba ON ba.id=bt.bank_account_id LEFT JOIN transaction_categories tc ON tc.id=bt.category_id WHERE bt.booking_date>=?1 AND bt.booking_date<=?2 AND bt.status='booked' ORDER BY bt.booking_date DESC,bt.id")?;
    let rows = stmt.query_map(params![start, end], |r| {
        Ok((
            ReportTransaction {
                id: r.get(0)?,
                booking_date: r.get(1)?,
                amount: r.get(2)?,
                currency: r.get(3)?,
                description: r.get(4)?,
                counterparty_name: r.get(5)?,
                counterparty_iban: r.get(6)?,
                category_id: r.get(7)?,
                bank_account_id: r.get(8)?,
                bank_account_name: r.get(9)?,
                tx_type: r.get(10)?,
                effective_flow_kind: FlowKind::Expense,
                flow_source: crate::models::reporting::FlowSource::Direction,
                signed_amount_czk: None,
                fx_quality: FxQuality::Missing,
            },
            r.get::<_, Option<FlowKind>>(11)?,
            r.get::<_, Option<FlowKind>>(12)?,
            r.get::<_, Option<String>>(13)?,
            r.get::<_, Option<String>>(14)?,
        ))
    })?;
    let mut out = vec![];
    for row in rows {
        let (mut row, explicit, category, bank_code, country) = row?;
        if accounts.is_some_and(|ids| !ids.contains(&row.bank_account_id)) {
            continue;
        }
        let credit = row.tx_type.eq_ignore_ascii_case("credit");
        let flow = resolve_structured_flow(
            explicit,
            row.counterparty_iban.as_deref(),
            bank_code.as_deref(),
            country.as_deref(),
            category,
            credit,
            &own,
        );
        row.effective_flow_kind = flow.kind;
        row.flow_source = flow.source;
        let amount = parse_money(&row.amount)?.abs();
        if let Some(czk) = historical_czk(conn, amount, &row.currency, row.booking_date)? {
            row.signed_amount_czk = Some(money(signed_flow_amount(flow.kind, credit, czk)));
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
        if row.fx_quality == FxQuality::Missing {
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
pub fn filtered_transactions(
    conn: &Connection,
    start: i64,
    end: i64,
    accounts: Option<&[String]>,
    kind: Option<FlowKind>,
    category: Option<&str>,
    source: Option<&str>,
) -> Result<Vec<ReportTransaction>> {
    Ok(transactions(conn, start, end, accounts)?
        .into_iter()
        .filter(|r| {
            kind.is_none_or(|kind| r.effective_flow_kind == kind)
                && category
                    .is_none_or(|id| r.category_id.as_deref().unwrap_or("uncategorized") == id)
                && source.is_none_or(|key| {
                    crate::services::cashflow_actuals::source_key(&r.counterparty_name).0 == key
                })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE bank_accounts(id TEXT,name TEXT,iban TEXT,bban TEXT,institution_id TEXT); CREATE TABLE institutions(id TEXT,country TEXT);
  CREATE TABLE transaction_categories(id TEXT,name TEXT,kind TEXT);
  CREATE TABLE bank_transactions(id TEXT,booking_date INTEGER,amount TEXT,currency TEXT,description TEXT,counterparty_name TEXT,counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT, import_line INTEGER, row_balance TEXT,category_id TEXT,bank_account_id TEXT,tx_type TEXT,flow_kind TEXT,status TEXT);
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
        conn.execute("UPDATE bank_transactions SET flow_kind='adjustment',counterparty_country='CZ' WHERE id='own'",[]).unwrap();
        let a = effective_flows(&conn, &["own".into()]).unwrap();
        assert_eq!(a[0].effective_flow_kind, FlowKind::Adjustment);
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
        assert!(foreign.signed_amount_czk.is_none());
        let q = quality(&rows, 1704067200, 1706745599, 1709251200);
        assert_eq!(q.missing_fx_count, 1);
        assert_eq!(q.observed_completed_months, vec![1704067200]);
        let flows = effective_flows(&conn, &["pending".into(), "own".into()]).unwrap();
        assert_eq!(flows.len(), 2);
        assert_eq!(flows[1].effective_flow_kind, FlowKind::Transfer);
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
