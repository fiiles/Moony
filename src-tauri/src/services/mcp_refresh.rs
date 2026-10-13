//! Durable MCP follow-up work. A write intent is saved before the domain mutation.
use crate::error::{AppError, Result};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRefresh {
    pub domain: String,
    pub earliest_date: Option<i64>,
    pub generation: String,
}

/// Persist before calling the domain writer: a crash between its commit and notification
/// cannot lose bookkeeping. Failed writes may leave a harmless refresh of unchanged data.
///
/// A domain coalesces to its earliest day. No date means "from the first fact", the earliest
/// day there is, so it wins over any dated write queued before or after it.
pub fn enqueue(conn: &Connection, domain: &str, earliest: Option<i64>) -> Result<()> {
    if domain == "stock-monitor" {
        return Ok(());
    }
    let earliest = earliest
        .map(|date| crate::models::validation::utc_day(date, "validation.dateRequired"))
        .transpose()?;
    conn.execute(
        "INSERT INTO mcp_pending_refresh (id, domain, earliest_date, generation) VALUES (?4, ?1, ?2, ?3)
         ON CONFLICT(domain) DO UPDATE SET
           earliest_date = CASE WHEN excluded.earliest_date IS NULL OR earliest_date IS NULL THEN NULL
             ELSE MIN(earliest_date, excluded.earliest_date) END,
           generation = excluded.generation",
        params![domain, earliest, uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string()],
    )?;
    Ok(())
}

pub fn pending(conn: &Connection) -> Result<Vec<PendingRefresh>> {
    let mut stmt = conn.prepare(
        "SELECT domain, earliest_date, generation FROM mcp_pending_refresh ORDER BY domain",
    )?;
    let jobs = stmt
        .query_map([], |r| {
            Ok(PendingRefresh {
                domain: r.get(0)?,
                earliest_date: r.get(1)?,
                generation: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(jobs)
}

/// A newer write replaces the generation, so completion of an older read cannot erase it.
pub fn acknowledge(conn: &Connection, job: &PendingRefresh) -> Result<()> {
    conn.execute(
        "DELETE FROM mcp_pending_refresh WHERE domain = ?1 AND generation = ?2",
        params![job.domain, job.generation],
    )?;
    Ok(())
}

/// The quote identities used by both event metadata and the durable worker are committed facts.
pub fn crypto_identity(conn: &Connection, ticker: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT coingecko_id FROM crypto_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn committed_crypto_tickers(
    conn: &Connection,
    tickers: &[String],
) -> Result<Vec<(String, Option<String>)>> {
    tickers
        .iter()
        .map(|ticker| Ok((ticker.clone(), crypto_identity(conn, ticker)?)))
        .collect()
}

pub fn stock_tickers(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT ticker FROM stock_investments ORDER BY ticker")?;
    let rows = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn crypto_identities(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT coingecko_id, ticker FROM crypto_investments ORDER BY ticker")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// A failed partial API response must not mark durable work complete. A manually overridden
/// price is already authoritative and does not depend on a quote provider being available.
pub fn require_current_stock_prices(conn: &Connection, tickers: &[String], now: i64) -> Result<()> {
    for ticker in tickers {
        let fresh: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM stock_data WHERE ticker = ?1 AND fetched_at >= ?2
               AND original_price IS NOT NULL AND CAST(original_price AS REAL) > 0)
             OR EXISTS(SELECT 1 FROM stock_price_overrides WHERE ticker = ?1)",
            params![ticker, now - 4 * 3600],
            |r| r.get(0),
        )?;
        if !fresh {
            return Err(AppError::Internal(format!(
                "Price refresh incomplete for {ticker}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch("CREATE TABLE mcp_pending_refresh (id TEXT PRIMARY KEY, domain TEXT NOT NULL UNIQUE, earliest_date INTEGER, generation TEXT NOT NULL);").expect("schema");
        conn
    }
    #[test]
    fn coalesces_to_the_earliest_date() {
        let conn = db();
        enqueue(&conn, "stocks", Some(172800)).expect("first");
        enqueue(&conn, "stocks", Some(86400)).expect("earlier");
        enqueue(&conn, "stocks", Some(259200)).expect("later");
        let jobs = pending(&conn).expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].earliest_date, Some(86400));
    }
    #[test]
    fn a_pending_refresh_from_the_first_fact_wins_over_a_dated_one_in_both_orders() {
        // No date means "from the first fact": the earliest day there is, so a dated write
        // queued next to it must not narrow it.
        let conn = db();
        enqueue(&conn, "bank-accounts", None).expect("undated first");
        enqueue(&conn, "bank-accounts", Some(86400)).expect("dated after");
        enqueue(&conn, "loans", Some(86400)).expect("dated first");
        enqueue(&conn, "loans", None).expect("undated after");
        enqueue(&conn, "real-estate", Some(172800)).expect("dated");
        enqueue(&conn, "real-estate", None).expect("undated");
        enqueue(&conn, "real-estate", Some(86400)).expect("dated again");

        let jobs = pending(&conn).expect("jobs");
        assert_eq!(jobs.len(), 3);
        for job in jobs {
            assert_eq!(job.earliest_date, None, "{}", job.domain);
        }
    }

    #[test]
    fn failed_financial_write_retains_its_refresh_intent() {
        let mut conn = db();
        conn.execute_batch("CREATE TABLE facts (id TEXT PRIMARY KEY);")
            .unwrap();
        enqueue(&conn, "bank", None).unwrap();
        {
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO facts VALUES ('uncommitted')", [])
                .unwrap();
            // A failed domain writer rolls back without rolling back the earlier intent.
        }
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM facts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(pending(&conn).unwrap().len(), 1);
    }

    #[test]
    fn following_a_stock_needs_no_financial_refresh() {
        let conn = db();
        enqueue(&conn, "stock-monitor", None).unwrap();
        assert!(pending(&conn).unwrap().is_empty());
    }

    #[test]
    fn invalid_dates_cannot_poison_the_retry_queue() {
        let conn = db();
        assert!(enqueue(&conn, "stocks", Some(i64::MAX)).is_err());
        assert!(pending(&conn).unwrap().is_empty());
    }

    #[test]
    fn intents_survive_closing_and_reopening_the_database() {
        let path = std::env::temp_dir().join(format!("moony-refresh-{}.db", uuid::Uuid::new_v4()));
        {
            let conn = Connection::open(&path).expect("open temp");
            conn.execute_batch("CREATE TABLE mcp_pending_refresh (id TEXT PRIMARY KEY, domain TEXT NOT NULL UNIQUE, earliest_date INTEGER, generation TEXT NOT NULL);").unwrap();
            enqueue(&conn, "crypto", Some(86400)).unwrap();
        }
        {
            let conn = Connection::open(&path).expect("reopen temp");
            assert_eq!(pending(&conn).unwrap()[0].earliest_date, Some(86400));
        }
        std::fs::remove_file(path).expect("remove temp");
    }

    #[test]
    fn crypto_event_identity_comes_from_committed_data() {
        let conn = db();
        conn.execute_batch("CREATE TABLE crypto_investments(ticker TEXT PRIMARY KEY, coingecko_id TEXT); INSERT INTO crypto_investments VALUES ('BTC','bitcoin');").unwrap();
        assert_eq!(
            committed_crypto_tickers(&conn, &["BTC".into()]).unwrap(),
            vec![("BTC".into(), Some("bitcoin".into()))]
        );
        assert_eq!(crypto_identity(&conn, "ETH").unwrap(), None);
    }

    #[test]
    fn incomplete_market_quotes_keep_refresh_pending_but_overrides_are_sufficient() {
        let conn = db();
        conn.execute_batch("CREATE TABLE stock_data(ticker TEXT PRIMARY KEY, original_price TEXT, fetched_at INTEGER); CREATE TABLE stock_price_overrides(ticker TEXT PRIMARY KEY, price TEXT); INSERT INTO stock_data VALUES ('AAPL',NULL,20000);").unwrap();
        let tickers = vec!["AAPL".into()];
        assert!(require_current_stock_prices(&conn, &tickers, 20000).is_err());
        conn.execute(
            "UPDATE stock_data SET original_price='10', fetched_at=1",
            [],
        )
        .unwrap();
        assert!(require_current_stock_prices(&conn, &tickers, 20000).is_err());
        conn.execute("INSERT INTO stock_price_overrides VALUES ('AAPL','12')", [])
            .unwrap();
        require_current_stock_prices(&conn, &tickers, 20000).unwrap();
    }

    #[test]
    fn completing_old_work_does_not_erase_a_concurrent_write() {
        let conn = db();
        enqueue(&conn, "bank", None).expect("first");
        let old = pending(&conn).expect("jobs").remove(0);
        enqueue(&conn, "bank", None).expect("next write");
        acknowledge(&conn, &old).expect("old completion");
        let jobs = pending(&conn).expect("new intent survives");
        assert_eq!(jobs.len(), 1);
        assert_ne!(jobs[0].generation, old.generation);
        acknowledge(&conn, &jobs[0]).expect("current completion");
        assert!(pending(&conn).expect("empty").is_empty());
    }
}
