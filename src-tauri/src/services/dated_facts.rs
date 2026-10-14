//! Dated facts behind the steps of the net-worth line.
//!
//! The dashboard marks a big day-over-day change of net worth and says what stands behind it:
//! the purchase of a property, a new loan, a manual balance, a day of trades. Those are the
//! facts the history days are derived from (`history_days.rs` reads the same tables); this
//! module lists them for a range of days, per history class, without valuing them. The
//! amounts are native (as entered, in their own currency): the line itself is converted by
//! `get_portfolio_history`, a fact only names its cause.
//!
//! * Purchases of properties and bonds, new loans, manual balances and extra payments of a loan,
//!   balance records of an account counted in the balance, and every valuation of a property or
//!   an other asset are one fact each.
//! * Trades of stocks, crypto and other assets are summarised per day and direction: one fact
//!   with the number of distinct tickers (or assets) and up to three of them. Their legs are not
//!   summed (a mix of currencies and quote units has no honest single amount).
//! * The first estimate of a property that only repeats its purchase (same day, price and
//!   currency) is left out: the purchase already says it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::DatedFact;
use crate::services::loan_amortization::{day_floor, loan_balance_at, LoanLedger};
use crate::services::loans;
use crate::services::reporting::money_in_currency;

const DAY: i64 = 86_400;

/// Tickers or asset names a summarised day of trades names; the count tells how many there were.
const MAX_NAMED_TRADES: usize = 3;

/// The history classes a fact belongs to, in the order facts of one day are listed.
const CLASS_ORDER: [&str; 7] = [
    "savings",
    "investments",
    "crypto",
    "bonds",
    "realEstate",
    "loans",
    "otherAssets",
];

/// Raw-timestamp bounds of the days `[start, end]`: a stored timestamp falls in the range exactly
/// when its UTC day does, so the SQL can filter before the days are floored.
struct Bounds {
    from: i64,
    until: i64,
}

impl Bounds {
    fn new(start_date: Option<i64>, end_date: Option<i64>) -> Self {
        Self {
            from: start_date.map_or(i64::MIN, day_floor),
            until: end_date.map_or(i64::MAX, |end| day_floor(end).saturating_add(DAY)),
        }
    }
}

/// `raw` as it was entered when it is a finite number, else `None` (a corrupt amount is left
/// out, never replaced by 0).
fn entered_amount(raw: &str) -> Option<String> {
    let text = raw.trim();
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|_| text.to_string())
}

/// Like [`entered_amount`], for a price where 0 means "not entered".
fn entered_price(raw: &str) -> Option<String> {
    entered_amount(raw).filter(|text| text.parse::<f64>().is_ok_and(|value| value > 0.0))
}

fn fact(
    day: i64,
    kind: &str,
    class: &str,
    name: String,
    amount: Option<String>,
    currency: Option<String>,
) -> DatedFact {
    DatedFact {
        day: day_floor(day),
        kind: kind.to_string(),
        class: class.to_string(),
        name,
        amount,
        currency,
        previous_amount: None,
        count: None,
    }
}

/// Every fact dated in the UTC days `[start_date, end_date]` (an open end is unbounded), ordered
/// by day, class, kind and name.
pub fn dated_facts(
    conn: &Connection,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<DatedFact>> {
    let bounds = Bounds::new(start_date, end_date);
    let mut facts = Vec::new();
    account_balances(conn, &bounds, &mut facts)?;
    property_purchases(conn, &bounds, &mut facts)?;
    property_valuations(conn, &bounds, &mut facts)?;
    loan_facts(conn, &bounds, &mut facts)?;
    bond_purchases(conn, &bounds, &mut facts)?;
    ticker_trades(
        conn,
        "investment_transactions",
        "investments",
        &bounds,
        &mut facts,
    )?;
    ticker_trades(conn, "crypto_transactions", "crypto", &bounds, &mut facts)?;
    other_asset_trades(conn, &bounds, &mut facts)?;
    other_asset_valuations(conn, &bounds, &mut facts)?;

    let rank = |class: &str| {
        CLASS_ORDER
            .iter()
            .position(|known| *known == class)
            .unwrap_or(CLASS_ORDER.len())
    };
    facts.sort_by(|a, b| {
        (a.day, rank(&a.class), &a.kind, &a.name).cmp(&(b.day, rank(&b.class), &b.kind, &b.name))
    });
    Ok(facts)
}

/// Balance records of the accounts the history counts (`exclude_from_balance = 0`).
fn account_balances(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT r.as_of, a.name, r.amount, r.currency
         FROM bank_balance_records r JOIN bank_accounts a ON a.id = r.bank_account_id
         WHERE a.exclude_from_balance = 0 AND r.as_of >= ?1 AND r.as_of < ?2
         ORDER BY r.as_of, a.name",
    )?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (day, name, amount, currency) = row?;
        out.push(fact(
            day,
            "balance",
            "savings",
            name,
            entered_amount(&amount),
            Some(currency),
        ));
    }
    Ok(())
}

/// The purchase of every property with a purchase date.
fn property_purchases(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT purchase_date, name, purchase_price, purchase_price_currency
         FROM real_estate
         WHERE purchase_date IS NOT NULL AND purchase_date >= ?1 AND purchase_date < ?2
         ORDER BY purchase_date, name",
    )?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (day, name, price, currency) = row?;
        out.push(fact(
            day,
            "buy",
            "realEstate",
            name,
            entered_price(&price),
            Some(currency),
        ));
    }
    Ok(())
}

/// A valuation log row of a parent (a property or an other asset), in the order it was entered.
struct ValuationRow {
    parent: String,
    name: String,
    day: i64,
    value: String,
    currency: String,
}

/// One row per parent and day (the last entered), oldest first, per parent. The input is ordered
/// by parent, day, creation.
fn last_valuation_of_each_day(rows: Vec<ValuationRow>) -> Vec<ValuationRow> {
    let mut days: Vec<ValuationRow> = Vec::with_capacity(rows.len());
    for row in rows {
        match days.last_mut() {
            Some(last) if last.parent == row.parent && last.day == row.day => *last = row,
            _ => days.push(row),
        }
    }
    days
}

/// Every estimate of a property: the first one as `firstValuation`, later ones as `valuation`.
/// The first estimate that repeats the purchase (same day, price and currency) is left out, and
/// so is every estimate outside the range (the first of a property is the first of its whole
/// log, not of the range).
fn property_valuations(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT v.real_estate_id, r.name, v.valued_at, v.value, v.currency,
                r.purchase_price, r.purchase_price_currency, r.purchase_date
         FROM real_estate_valuations v JOIN real_estate r ON r.id = v.real_estate_id
         ORDER BY v.real_estate_id, v.valued_at, v.created_at, v.rowid",
    )?;
    struct Row {
        valuation: ValuationRow,
        purchase_price: String,
        purchase_currency: String,
        purchase_day: Option<i64>,
    }
    let rows = stmt
        .query_map([], |row| {
            Ok(Row {
                valuation: ValuationRow {
                    parent: row.get(0)?,
                    name: row.get(1)?,
                    day: day_floor(row.get::<_, i64>(2)?),
                    value: row.get(3)?,
                    currency: row.get(4)?,
                },
                purchase_price: row.get(5)?,
                purchase_currency: row.get(6)?,
                purchase_day: row.get::<_, Option<i64>>(7)?.map(day_floor),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // The purchase terms ride along with each row; keep them per property.
    let terms: BTreeMap<String, (String, String, Option<i64>)> = rows
        .iter()
        .map(|row| {
            (
                row.valuation.parent.clone(),
                (
                    row.purchase_price.clone(),
                    row.purchase_currency.clone(),
                    row.purchase_day,
                ),
            )
        })
        .collect();
    let days = last_valuation_of_each_day(rows.into_iter().map(|row| row.valuation).collect());

    let mut seen: BTreeSet<String> = BTreeSet::new();
    for valuation in days {
        let first = seen.insert(valuation.parent.clone());
        let initial = first
            && terms
                .get(&valuation.parent)
                .is_some_and(|(price, currency, purchase_day)| {
                    *purchase_day == Some(valuation.day)
                        && *currency == valuation.currency
                        && matches!(
                            (price.trim().parse::<f64>(), valuation.value.trim().parse::<f64>()),
                            (Ok(bought), Ok(valued)) if bought == valued
                        )
                });
        let in_range = valuation.day >= bounds.from && valuation.day < bounds.until;
        if initial || !in_range {
            continue;
        }
        out.push(fact(
            valuation.day,
            if first { "firstValuation" } else { "valuation" },
            "realEstate",
            valuation.name,
            entered_amount(&valuation.value),
            Some(valuation.currency),
        ));
    }
    Ok(())
}

/// The start of every loan, its manual balances and its extra payments.
fn loan_facts(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT start_date, name, principal, currency FROM loans
         WHERE start_date >= ?1 AND start_date < ?2 ORDER BY start_date, name",
    )?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (day, name, principal, currency) = row?;
        out.push(fact(
            day,
            "start",
            "loans",
            name,
            entered_price(&principal),
            Some(currency),
        ));
    }

    let mut stmt = conn.prepare(
        "SELECT e.kind, e.event_day, e.amount, e.loan_id, l.name, l.currency
         FROM loan_events e JOIN loans l ON l.id = e.loan_id
         WHERE e.kind IN ('balance_check', 'extra_payment')
           AND e.event_day >= ?1 AND e.event_day < ?2
         ORDER BY e.event_day, l.name, e.created_at, e.rowid",
    )?;
    let rows = stmt
        .query_map(params![bounds.from, bounds.until], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut ledgers: HashMap<String, Option<loans::LoanLedgerEntry>> = HashMap::new();
    for (kind, day, amount, loan_id, name, currency) in rows {
        let manual = kind == "balance_check";
        let mut entry = fact(
            day,
            if manual { "balance" } else { "extraPayment" },
            "loans",
            name,
            amount.as_deref().and_then(entered_amount),
            Some(currency.clone()),
        );
        if manual {
            let ledger = ledgers
                .entry(loan_id.clone())
                .or_insert_with(|| loans::load_ledger(conn, &loan_id).ok());
            entry.previous_amount = ledger
                .as_ref()
                .and_then(|loaded| balance_before(&loaded.ledger, entry.day))
                .map(|balance| money_in_currency(balance, &currency));
        }
        out.push(entry);
    }
    Ok(())
}

/// What the loan's ledger held the day before `day`, which a manual balance replaces; `None`
/// when the loan had not started yet (nothing was held to replace).
fn balance_before(ledger: &LoanLedger, day: i64) -> Option<f64> {
    let before = day_floor(day) - DAY;
    (before >= ledger.terms.start_day).then(|| loan_balance_at(ledger, before).balance)
}

/// The purchase of every bond with a purchase date, at its face value (`coupon_value × quantity`).
fn bond_purchases(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT purchase_date, name, coupon_value, quantity, currency FROM bonds
         WHERE purchase_date IS NOT NULL AND purchase_date >= ?1 AND purchase_date < ?2
         ORDER BY purchase_date, name",
    )?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (day, name, coupon_value, quantity, currency) = row?;
        let amount = match (
            coupon_value.trim().parse::<f64>(),
            quantity.trim().parse::<f64>(),
        ) {
            (Ok(value), Ok(quantity))
                if (value * quantity).is_finite() && value * quantity > 0.0 =>
            {
                Some(money_in_currency(value * quantity, &currency))
            }
            _ => None,
        };
        out.push(fact(day, "buy", "bonds", name, amount, Some(currency)));
    }
    Ok(())
}

/// A trade row to summarise: direction, the day and what was traded.
struct Trade {
    day: i64,
    kind: &'static str,
    subject: String,
}

/// `buy` / `sell` of a stored transaction type; anything else is not a trade.
fn trade_kind(kind: &str) -> Option<&'static str> {
    match kind.trim().to_lowercase().as_str() {
        "buy" => Some("buy"),
        "sell" => Some("sell"),
        _ => None,
    }
}

/// One fact per day and direction: how many distinct subjects were traded, and up to three of
/// them (alphabetically, so the text does not depend on the order of entry).
fn summarise_trades(class: &str, trades: Vec<Trade>, out: &mut Vec<DatedFact>) {
    let mut grouped: BTreeMap<(i64, &'static str), BTreeSet<String>> = BTreeMap::new();
    for trade in trades {
        grouped
            .entry((trade.day, trade.kind))
            .or_default()
            .insert(trade.subject);
    }
    for ((day, kind), subjects) in grouped {
        let named: Vec<&str> = subjects
            .iter()
            .take(MAX_NAMED_TRADES)
            .map(String::as_str)
            .collect();
        out.push(DatedFact {
            day,
            kind: kind.to_string(),
            class: class.to_string(),
            name: named.join(", "),
            amount: None,
            currency: None,
            previous_amount: None,
            count: Some(i32::try_from(subjects.len()).unwrap_or(i32::MAX)),
        });
    }
}

/// Stock or crypto trades (`table` has `type`, `ticker` and `transaction_date`).
fn ticker_trades(
    conn: &Connection,
    table: &str,
    class: &str,
    bounds: &Bounds,
    out: &mut Vec<DatedFact>,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!(
        "SELECT type, ticker, transaction_date FROM {table}
         WHERE transaction_date >= ?1 AND transaction_date < ?2"
    ))?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut trades = Vec::new();
    for row in rows {
        let (kind, ticker, day) = row?;
        if let Some(kind) = trade_kind(&kind) {
            trades.push(Trade {
                day: day_floor(day),
                kind,
                subject: ticker,
            });
        }
    }
    summarise_trades(class, trades, out);
    Ok(())
}

/// Buys and sells of other assets, named by the asset.
fn other_asset_trades(conn: &Connection, bounds: &Bounds, out: &mut Vec<DatedFact>) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT t.type, a.name, t.transaction_date
         FROM other_asset_transactions t JOIN other_assets a ON a.id = t.asset_id
         WHERE t.transaction_date >= ?1 AND t.transaction_date < ?2",
    )?;
    let rows = stmt.query_map(params![bounds.from, bounds.until], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut trades = Vec::new();
    for row in rows {
        let (kind, name, day) = row?;
        if let Some(kind) = trade_kind(&kind) {
            trades.push(Trade {
                day: day_floor(day),
                kind,
                subject: name,
            });
        }
    }
    summarise_trades("otherAssets", trades, out);
    Ok(())
}

/// Every valuation of an other asset (the last of a day when there are several).
fn other_asset_valuations(
    conn: &Connection,
    bounds: &Bounds,
    out: &mut Vec<DatedFact>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT v.asset_id, a.name, v.valued_at, v.value, v.currency
         FROM other_asset_valuations v JOIN other_assets a ON a.id = v.asset_id
         WHERE v.valued_at >= ?1 AND v.valued_at < ?2
         ORDER BY v.asset_id, v.valued_at, v.created_at, v.rowid",
    )?;
    let rows = stmt
        .query_map(params![bounds.from, bounds.until], |row| {
            Ok(ValuationRow {
                parent: row.get(0)?,
                name: row.get(1)?,
                day: day_floor(row.get::<_, i64>(2)?),
                value: row.get(3)?,
                currency: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for valuation in last_valuation_of_each_day(rows) {
        out.push(fact(
            valuation.day,
            "valuation",
            "otherAssets",
            valuation.name,
            entered_amount(&valuation.value),
            Some(valuation.currency),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-01 UTC.
    const D0: i64 = 1_767_225_600;

    fn d(n: i64) -> i64 {
        D0 + n * DAY
    }

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                exclude_from_balance INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE bank_balance_records (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL,
                amount TEXT NOT NULL, currency TEXT NOT NULL, as_of INTEGER NOT NULL
            );
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                purchase_price TEXT NOT NULL DEFAULT '0',
                purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
                purchase_date INTEGER
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY, real_estate_id TEXT NOT NULL,
                value TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'CZK',
                valued_at INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE loans (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                interest_rate TEXT NOT NULL DEFAULT '0',
                monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL, end_date INTEGER
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY, loan_id TEXT NOT NULL, kind TEXT NOT NULL,
                event_day INTEGER NOT NULL, amount TEXT, rate TEXT, monthly_payment TEXT,
                note TEXT, created_at INTEGER NOT NULL DEFAULT 0,
                previous_rate TEXT, previous_monthly_payment TEXT
            );
            CREATE TABLE bonds (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, coupon_value TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '1', currency TEXT NOT NULL DEFAULT 'CZK',
                purchase_date INTEGER
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, ticker TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, ticker TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
            CREATE TABLE other_assets (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, type TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
            );
            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY, asset_id TEXT NOT NULL, value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', valued_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn all(conn: &Connection) -> Vec<DatedFact> {
        dated_facts(conn, None, None).expect("facts")
    }

    fn property(conn: &Connection, id: &str, name: &str, price: &str, bought: Option<i64>) {
        conn.execute(
            "INSERT INTO real_estate (id, name, purchase_price, purchase_date)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, name, price, bought],
        )
        .expect("property");
    }

    fn valuation(conn: &Connection, id: &str, property: &str, value: &str, day: i64, created: i64) {
        conn.execute(
            "INSERT INTO real_estate_valuations (id, real_estate_id, value, valued_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, property, value, day, created],
        )
        .expect("valuation");
    }

    fn trade(conn: &Connection, table: &str, id: &str, kind: &str, ticker: &str, at: i64) {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, type, ticker, transaction_date) VALUES (?1, ?2, ?3, ?4)"
            ),
            params![id, kind, ticker, at],
        )
        .expect("trade");
    }

    #[test]
    fn a_property_purchase_and_its_later_estimates() {
        let conn = setup();
        property(&conn, "p1", "Chlumec", "5000000", Some(d(10)));
        // The initial estimate repeats the purchase: left out. The next ones are estimates.
        valuation(&conn, "v1", "p1", "5000000", d(10), 1);
        valuation(&conn, "v2", "p1", "5400000", d(40), 2);
        // Two estimates on one day: the last entered counts.
        valuation(&conn, "v3", "p1", "5500000", d(70), 3);
        valuation(&conn, "v4", "p1", "5600000", d(70), 4);

        let facts = all(&conn);
        let summary: Vec<_> = facts
            .iter()
            .map(|f| {
                (
                    f.day,
                    f.kind.as_str(),
                    f.class.as_str(),
                    f.amount.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (d(10), "buy", "realEstate", Some("5000000")),
                (d(40), "valuation", "realEstate", Some("5400000")),
                (d(70), "valuation", "realEstate", Some("5600000")),
            ]
        );
        assert!(facts.iter().all(|f| f.name == "Chlumec"));
        assert!(facts.iter().all(|f| f.currency.as_deref() == Some("CZK")));
    }

    #[test]
    fn the_first_estimate_of_a_property_with_another_price_is_marked_first() {
        let conn = setup();
        property(&conn, "p1", "Byt", "4000000", Some(d(0)));
        valuation(&conn, "v1", "p1", "4300000", d(0), 1);
        // A property without a purchase date: its first estimate is where it starts.
        property(&conn, "p2", "Chata", "0", None);
        valuation(&conn, "v2", "p2", "1200000", d(5), 2);
        valuation(&conn, "v3", "p2", "1300000", d(25), 3);

        let kinds: Vec<_> = all(&conn)
            .into_iter()
            .map(|f| (f.day, f.kind, f.name))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (d(0), "buy".to_string(), "Byt".to_string()),
                (d(0), "firstValuation".to_string(), "Byt".to_string()),
                (d(5), "firstValuation".to_string(), "Chata".to_string()),
                (d(25), "valuation".to_string(), "Chata".to_string()),
            ]
        );
        // A purchase price of 0 is "not entered"
        let chata = all(&conn);
        assert!(chata
            .iter()
            .all(|f| f.name != "Chata" || f.amount.is_some()));
    }

    #[test]
    fn the_first_estimate_is_the_first_of_the_log_not_of_the_range() {
        let conn = setup();
        property(&conn, "p1", "Byt", "4000000", Some(d(0)));
        valuation(&conn, "v1", "p1", "4000000", d(0), 1);
        valuation(&conn, "v2", "p1", "4200000", d(30), 2);
        let facts = dated_facts(&conn, Some(d(20)), Some(d(40))).expect("facts");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].kind, "valuation");
    }

    #[test]
    fn loans_start_and_their_events() {
        let conn = setup();
        conn.execute(
            "INSERT INTO loans (id, name, principal, currency, start_date)
             VALUES ('l1', 'Chlumec - byt', '2000000', 'CZK', ?1)",
            [d(10)],
        )
        .expect("loan");
        for (id, kind, day, amount) in [
            ("e1", "balance_check", d(30), Some("1800000")),
            ("e2", "extra_payment", d(40), Some("100000")),
            ("e3", "rate_change", d(50), None),
        ] {
            conn.execute(
                "INSERT INTO loan_events (id, loan_id, kind, event_day, amount)
                 VALUES (?1, 'l1', ?2, ?3, ?4)",
                params![id, kind, day, amount],
            )
            .expect("event");
        }

        let summary: Vec<_> = all(&conn)
            .into_iter()
            .map(|f| (f.day, f.kind, f.class, f.amount, f.previous_amount))
            .collect();
        let text = |value: &str| Some(value.to_string());
        assert_eq!(
            summary,
            vec![
                (d(10), "start".into(), "loans".into(), text("2000000"), None),
                // A manual balance carries what the ledger held the day before it
                (
                    d(30),
                    "balance".into(),
                    "loans".into(),
                    text("1800000"),
                    text("2000000.00")
                ),
                (
                    d(40),
                    "extraPayment".into(),
                    "loans".into(),
                    text("100000"),
                    None
                ),
            ]
        );
    }

    #[test]
    fn a_manual_balance_on_the_start_day_replaces_nothing() {
        let conn = setup();
        conn.execute(
            "INSERT INTO loans (id, name, principal, currency, start_date)
             VALUES ('l1', 'Auto', '500000', 'EUR', ?1)",
            [d(10)],
        )
        .expect("loan");
        conn.execute(
            "INSERT INTO loan_events (id, loan_id, kind, event_day, amount)
             VALUES ('e1', 'l1', 'balance_check', ?1, '480000')",
            [d(10)],
        )
        .expect("event");
        let balance = all(&conn)
            .into_iter()
            .find(|f| f.kind == "balance")
            .expect("balance fact");
        assert_eq!(balance.previous_amount, None);
        assert_eq!(balance.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn balance_records_count_only_for_accounts_in_the_balance() {
        let conn = setup();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, exclude_from_balance) VALUES
                ('a1', 'Partners', 0), ('a2', 'Firma', 1);",
        )
        .expect("accounts");
        for (id, account, amount, day) in
            [("r1", "a1", "-600000", d(3)), ("r2", "a2", "900000", d(3))]
        {
            conn.execute(
                "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of)
                 VALUES (?1, ?2, ?3, 'CZK', ?4)",
                params![id, account, amount, day],
            )
            .expect("record");
        }
        let facts = all(&conn);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].kind, "balance");
        assert_eq!(facts[0].class, "savings");
        assert_eq!(facts[0].name, "Partners");
        // A negative balance is an amount, not corruption
        assert_eq!(facts[0].amount.as_deref(), Some("-600000"));
    }

    #[test]
    fn trades_are_summarised_per_day_and_direction() {
        let conn = setup();
        // Different times of one UTC day, three tickers bought, one sold, one bought twice
        for (id, kind, ticker, at) in [
            ("t1", "buy", "MSFT", d(5) + 3_600),
            ("t2", "buy", "AAPL", d(5) + 40_000),
            ("t3", "buy", "NVDA", d(5)),
            ("t4", "buy", "NVDA", d(5) + 100),
            ("t5", "buy", "TSLA", d(5) + 200),
            ("t6", "sell", "AMZN", d(5)),
            ("t7", "buy", "AAPL", d(6)),
            ("t8", "dividend", "AAPL", d(6)),
        ] {
            trade(&conn, "investment_transactions", id, kind, ticker, at);
        }
        trade(&conn, "crypto_transactions", "c1", "buy", "BTC", d(5));

        let facts = all(&conn);
        let summary: Vec<_> = facts
            .iter()
            .map(|f| {
                (
                    f.day,
                    f.class.as_str(),
                    f.kind.as_str(),
                    f.name.as_str(),
                    f.count,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (d(5), "investments", "buy", "AAPL, MSFT, NVDA", Some(4)),
                (d(5), "investments", "sell", "AMZN", Some(1)),
                (d(5), "crypto", "buy", "BTC", Some(1)),
                (d(6), "investments", "buy", "AAPL", Some(1)),
            ]
        );
        assert!(facts
            .iter()
            .all(|f| f.amount.is_none() && f.currency.is_none()));
    }

    #[test]
    fn bonds_and_other_assets() {
        let conn = setup();
        conn.execute(
            "INSERT INTO bonds (id, name, coupon_value, quantity, currency, purchase_date)
             VALUES ('b1', 'Rohlik', '10000', '40', 'CZK', ?1),
                    ('b2', 'Undated', '10000', '1', 'CZK', NULL)",
            [d(8)],
        )
        .expect("bonds");
        conn.execute_batch(
            "INSERT INTO other_assets (id, name) VALUES ('o1', 'Gold'), ('o2', 'Car');",
        )
        .expect("assets");
        for (id, asset, kind, day) in [("x1", "o1", "buy", d(9)), ("x2", "o2", "buy", d(9))] {
            conn.execute(
                "INSERT INTO other_asset_transactions (id, asset_id, type, transaction_date)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, asset, kind, day],
            )
            .expect("tx");
        }
        conn.execute(
            "INSERT INTO other_asset_valuations (id, asset_id, value, currency, valued_at)
             VALUES ('ov1', 'o1', '2500000', 'CZK', ?1)",
            [d(12)],
        )
        .expect("valuation");

        let summary: Vec<_> = all(&conn)
            .into_iter()
            .map(|f| (f.day, f.class, f.kind, f.name, f.amount, f.count))
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    d(8),
                    "bonds".to_string(),
                    "buy".to_string(),
                    "Rohlik".to_string(),
                    Some("400000.00".to_string()),
                    None
                ),
                (
                    d(9),
                    "otherAssets".to_string(),
                    "buy".to_string(),
                    "Car, Gold".to_string(),
                    None,
                    Some(2)
                ),
                (
                    d(12),
                    "otherAssets".to_string(),
                    "valuation".to_string(),
                    "Gold".to_string(),
                    Some("2500000".to_string()),
                    None
                ),
            ]
        );
    }

    #[test]
    fn the_range_is_whole_utc_days_inclusive() {
        let conn = setup();
        trade(
            &conn,
            "investment_transactions",
            "t1",
            "buy",
            "AAA",
            d(9) + 86_399,
        );
        trade(&conn, "investment_transactions", "t2", "buy", "BBB", d(10));
        trade(
            &conn,
            "investment_transactions",
            "t3",
            "buy",
            "CCC",
            d(20) + 80_000,
        );
        trade(&conn, "investment_transactions", "t4", "buy", "DDD", d(21));

        // Start inside a day still takes that whole day; the end day is included
        let names: Vec<_> = dated_facts(&conn, Some(d(10) + 5_000), Some(d(20) + 10))
            .expect("facts")
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, vec!["BBB", "CCC"]);
        assert_eq!(all(&conn).len(), 4);
    }

    #[test]
    fn a_corrupt_amount_is_left_out_not_zeroed() {
        let conn = setup();
        property(&conn, "p1", "Byt", "n/a", Some(d(1)));
        let facts = all(&conn);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].amount, None);
    }
}
