//! Watchlist milestones: a target the price crossed in the direction it waits for.

use rusqlite::Connection;

use super::milestone;
use crate::error::Result;
use crate::models::{Milestone, TARGET_ABOVE, TARGET_BELOW};

pub(super) fn build(conn: &Connection, _today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT w.ticker, w.target_price, w.target_direction, sd.original_price, sd.currency
         FROM watched_stocks w LEFT JOIN stock_data sd ON sd.ticker = w.ticker
         WHERE w.target_price IS NOT NULL ORDER BY w.ticker",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (ticker, target_text, direction, price_text, currency) in rows {
        let target: f64 = target_text.trim().parse().unwrap_or(0.0);
        let Some(price_text) = price_text else {
            continue;
        };
        let price: f64 = price_text.trim().parse().unwrap_or(0.0);
        if target <= 0.0 || price <= 0.0 {
            continue;
        }
        // A target without a direction keeps the meaning of earlier versions.
        let direction = direction.unwrap_or_else(|| TARGET_ABOVE.to_string());
        let crossed = if direction == TARGET_BELOW {
            price <= target
        } else {
            price >= target
        };
        if !crossed {
            continue;
        }
        let key = format!("watch_target:{ticker}:{}:{direction}", target_text.trim());
        let mut m = milestone("watch_target", key, Some(&ticker), &ticker);
        m.amount = Some(target_text.trim().to_string());
        m.reference_amount = Some(price_text.trim().to_string());
        m.currency = currency;
        m.direction = Some(direction);
        out.push(m);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::setup;
    use super::*;

    fn watched(
        conn: &Connection,
        ticker: &str,
        target: &str,
        direction: &str,
        price: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO watched_stocks (id, ticker, target_price, target_direction) VALUES (?1, ?1, ?2, ?3)",
            [ticker, target, direction],
        )
        .unwrap();
        if let Some(p) = price {
            conn.execute(
                "INSERT INTO stock_data (id, ticker, original_price, currency) VALUES (?1, ?1, ?2, 'USD')",
                [ticker, p],
            )
            .unwrap();
        }
    }

    #[test]
    fn only_targets_crossed_in_their_direction_count() {
        let conn = setup();
        watched(&conn, "CAT", "600", "below", Some("845"));
        watched(&conn, "DIP", "600", "below", Some("598"));
        watched(&conn, "UP", "900", "above", Some("905"));
        watched(&conn, "WAIT", "900", "above", Some("845"));
        watched(&conn, "NOPRICE", "10", "above", None);
        let list = build(&conn, 0).unwrap();
        let tickers: Vec<_> = list.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(tickers, vec!["DIP", "UP"]);
        assert_eq!(list[0].key, "watch_target:DIP:600:below");
        assert_eq!(list[0].amount.as_deref(), Some("600"));
        assert_eq!(list[0].reference_amount.as_deref(), Some("598"));
        assert_eq!(list[0].direction.as_deref(), Some("below"));
        assert_eq!(list[0].currency.as_deref(), Some("USD"));
    }
}
