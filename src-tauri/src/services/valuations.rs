//! Valuation log of manually priced assets (real estate, other assets).
//!
//! Each estimate is a dated row; the newest row is mirrored into the parent's
//! `market_price` so every existing reader (net worth, snapshots, lists) keeps
//! working unchanged. Editing the market price through the parent's update
//! command writes a row dated today (`record_price_change`), so the log is
//! complete whichever path the user takes.

use crate::error::{AppError, Result};
use crate::models::{AssetValuation, InsertAssetValuation};
use rusqlite::{Connection, OptionalExtension, Row};
use uuid::Uuid;

/// Which parent the valuations belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValuationKind {
    RealEstate,
    OtherAsset,
}

impl ValuationKind {
    fn table(self) -> &'static str {
        match self {
            ValuationKind::RealEstate => "real_estate_valuations",
            ValuationKind::OtherAsset => "other_asset_valuations",
        }
    }

    fn fk(self) -> &'static str {
        match self {
            ValuationKind::RealEstate => "real_estate_id",
            ValuationKind::OtherAsset => "asset_id",
        }
    }

    fn parent(self) -> &'static str {
        match self {
            ValuationKind::RealEstate => "real_estate",
            ValuationKind::OtherAsset => "other_assets",
        }
    }

    fn parent_currency_column(self) -> &'static str {
        match self {
            ValuationKind::RealEstate => "market_price_currency",
            ValuationKind::OtherAsset => "currency",
        }
    }
}

fn read_row(row: &Row) -> rusqlite::Result<AssetValuation> {
    Ok(AssetValuation {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        value: row.get(2)?,
        currency: row.get(3)?,
        valued_at: row.get(4)?,
        note: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn columns(kind: ValuationKind) -> String {
    format!(
        "id, {fk}, value, currency, valued_at, note, created_at",
        fk = kind.fk()
    )
}

/// Does the parent row exist?
fn parent_exists(conn: &Connection, kind: ValuationKind, asset_id: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {} WHERE id = ?1", kind.parent());
    Ok(conn
        .query_row(&sql, [asset_id], |_| Ok(()))
        .optional()?
        .is_some())
}

/// All valuations of one asset, oldest first.
pub fn list_valuations(
    conn: &Connection,
    kind: ValuationKind,
    asset_id: &str,
) -> Result<Vec<AssetValuation>> {
    let sql = format!(
        "SELECT {cols} FROM {table} WHERE {fk} = ?1 ORDER BY valued_at ASC, created_at ASC",
        cols = columns(kind),
        table = kind.table(),
        fk = kind.fk()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([asset_id], read_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The newest valuation of an asset, if any.
pub fn latest_valuation(
    conn: &Connection,
    kind: ValuationKind,
    asset_id: &str,
) -> Result<Option<AssetValuation>> {
    let sql = format!(
        "SELECT {cols} FROM {table} WHERE {fk} = ?1 ORDER BY valued_at DESC, created_at DESC LIMIT 1",
        cols = columns(kind),
        table = kind.table(),
        fk = kind.fk()
    );
    Ok(conn.query_row(&sql, [asset_id], read_row).optional()?)
}

/// The value that applied on `day`: the newest row dated on or before it, or
/// the earliest row when the day precedes every estimate (history rebuilds).
pub fn value_at_day(
    conn: &Connection,
    kind: ValuationKind,
    asset_id: &str,
    day: i64,
) -> Result<Option<f64>> {
    let day = day.div_euclid(86_400) * 86_400;
    let on_or_before = format!(
        "SELECT value FROM {table} WHERE {fk} = ?1 AND valued_at <= ?2 ORDER BY valued_at DESC, created_at DESC LIMIT 1",
        table = kind.table(),
        fk = kind.fk()
    );
    let value: Option<String> = conn
        .query_row(&on_or_before, rusqlite::params![asset_id, day], |row| {
            row.get(0)
        })
        .optional()?;
    let value = match value {
        Some(v) => Some(v),
        None => {
            let earliest = format!(
                "SELECT value FROM {table} WHERE {fk} = ?1 ORDER BY valued_at ASC, created_at ASC LIMIT 1",
                table = kind.table(),
                fk = kind.fk()
            );
            conn.query_row(&earliest, [asset_id], |row| row.get(0))
                .optional()?
        }
    };
    Ok(value.and_then(|v| v.trim().parse::<f64>().ok()))
}

/// Mirror the newest valuation into the parent's market price. For other
/// assets only the price moves: their single currency column also prices the
/// purchase average, so a valuation in another currency does not change it.
fn sync_parent_price(conn: &Connection, kind: ValuationKind, asset_id: &str) -> Result<()> {
    let Some(latest) = latest_valuation(conn, kind, asset_id)? else {
        return Ok(());
    };
    let now = chrono::Utc::now().timestamp();
    let sql = match kind {
        ValuationKind::RealEstate => format!(
            "UPDATE {parent} SET market_price = ?1, {cur} = ?2, updated_at = ?3 WHERE id = ?4",
            parent = kind.parent(),
            cur = kind.parent_currency_column()
        ),
        ValuationKind::OtherAsset => format!(
            "UPDATE {parent} SET market_price = ?1, updated_at = ?3 WHERE id = ?4 AND ?2 = ?2",
            parent = kind.parent()
        ),
    };
    conn.execute(
        &sql,
        rusqlite::params![latest.value, latest.currency, now, asset_id],
    )?;
    Ok(())
}

/// Record a valuation and mirror it into the parent when it is the newest.
pub fn add_valuation(
    conn: &Connection,
    kind: ValuationKind,
    data: &InsertAssetValuation,
) -> Result<AssetValuation> {
    data.validate()?;
    if !parent_exists(conn, kind, &data.asset_id)? {
        return Err(AppError::NotFound("Asset not found".into()));
    }
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let currency = data
        .currency
        .clone()
        .unwrap_or_else(|| "CZK".to_string())
        .to_uppercase();
    let day = data.valued_at.div_euclid(86_400) * 86_400;
    let note = data
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let sql = format!(
        "INSERT INTO {table} (id, {fk}, value, currency, valued_at, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        table = kind.table(),
        fk = kind.fk()
    );
    conn.execute(
        &sql,
        rusqlite::params![
            id,
            data.asset_id,
            data.value.trim(),
            currency,
            day,
            note,
            now
        ],
    )?;
    sync_parent_price(conn, kind, &data.asset_id)?;
    Ok(AssetValuation {
        id,
        asset_id: data.asset_id.clone(),
        value: data.value.trim().to_string(),
        currency,
        valued_at: day,
        note,
        created_at: now,
    })
}

/// Delete a valuation; the parent's price follows the newest remaining row.
pub fn delete_valuation(conn: &Connection, kind: ValuationKind, id: &str) -> Result<()> {
    let sql = format!(
        "SELECT {fk} FROM {table} WHERE id = ?1",
        fk = kind.fk(),
        table = kind.table()
    );
    let asset_id: String = conn
        .query_row(&sql, [id], |row| row.get(0))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Valuation not found".into()))?;
    conn.execute(&format!("DELETE FROM {} WHERE id = ?1", kind.table()), [id])?;
    sync_parent_price(conn, kind, &asset_id)?;
    Ok(())
}

/// The parent's market price was edited directly: keep the log complete by
/// writing today's row when the price or currency differs from the newest
/// valuation (today's row is updated in place when it already exists).
/// Returns true when the log changed.
pub fn record_price_change(
    conn: &Connection,
    kind: ValuationKind,
    asset_id: &str,
    value: &str,
    currency: &str,
    today: i64,
) -> Result<bool> {
    let value = value.trim();
    let currency = currency.trim().to_uppercase();
    let latest = latest_valuation(conn, kind, asset_id)?;
    let same = |v: &AssetValuation| {
        v.value.trim().parse::<f64>().ok() == value.parse::<f64>().ok()
            && v.currency.eq_ignore_ascii_case(&currency)
    };
    if latest.as_ref().is_some_and(same) {
        return Ok(false);
    }
    let today = today.div_euclid(86_400) * 86_400;
    let now = chrono::Utc::now().timestamp();
    if let Some(row) = latest.filter(|v| v.valued_at == today) {
        conn.execute(
            &format!(
                "UPDATE {table} SET value = ?1, currency = ?2, created_at = ?3 WHERE id = ?4",
                table = kind.table()
            ),
            rusqlite::params![value, currency, now, row.id],
        )?;
    } else {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, {fk}, value, currency, valued_at, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                table = kind.table(),
                fk = kind.fk()
            ),
            rusqlite::params![
                Uuid::new_v4().to_string(),
                asset_id,
                value,
                currency,
                today,
                now
            ],
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL, type TEXT NOT NULL,
                purchase_price TEXT NOT NULL DEFAULT '0', purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
                market_price TEXT NOT NULL DEFAULT '0', market_price_currency TEXT NOT NULL DEFAULT 'CZK',
                monthly_rent TEXT, monthly_rent_currency TEXT DEFAULT 'CZK',
                recurring_costs TEXT DEFAULT '[]', photos TEXT DEFAULT '[]', notes TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()), updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0', currency TEXT NOT NULL DEFAULT 'CZK',
                average_purchase_price TEXT NOT NULL DEFAULT '0', yield_type TEXT NOT NULL DEFAULT 'none', yield_value TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()), updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY,
                real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
                value TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'CZK', valued_at INTEGER NOT NULL, note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
                value TEXT NOT NULL, currency TEXT NOT NULL DEFAULT 'CZK', valued_at INTEGER NOT NULL, note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            INSERT INTO real_estate (id, name, address, type, purchase_price, market_price) VALUES ('re1', 'Flat', 'Brno', 'investment', '2900000', '3480000');
            INSERT INTO other_assets (id, name, quantity, market_price, currency) VALUES ('oa1', 'Gold', '3', '62000', 'CZK');
            "#,
        )
        .expect("schema");
        conn
    }

    fn insert(asset_id: &str, value: &str, day: i64) -> InsertAssetValuation {
        InsertAssetValuation {
            asset_id: asset_id.into(),
            value: value.into(),
            currency: Some("CZK".into()),
            valued_at: day,
            note: None,
        }
    }

    fn market_price(conn: &Connection, table: &str, id: &str) -> String {
        conn.query_row(
            &format!("SELECT market_price FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn newest_valuation_moves_the_parent_price() {
        let conn = setup_test_db();
        add_valuation(
            &conn,
            ValuationKind::RealEstate,
            &insert("re1", "3650000", 20_000 * DAY),
        )
        .unwrap();
        assert_eq!(market_price(&conn, "real_estate", "re1"), "3650000");

        // A backdated estimate does not override the newer one
        add_valuation(
            &conn,
            ValuationKind::RealEstate,
            &insert("re1", "3150000", 19_000 * DAY),
        )
        .unwrap();
        assert_eq!(market_price(&conn, "real_estate", "re1"), "3650000");
        let rows = list_valuations(&conn, ValuationKind::RealEstate, "re1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, "3150000"); // oldest first
        assert_eq!(rows[1].valued_at, 20_000 * DAY);
    }

    #[test]
    fn valued_at_is_floored_to_the_utc_day_and_note_trimmed() {
        let conn = setup_test_db();
        let mut data = insert("oa1", "68500", 20_000 * DAY + 3_600);
        data.note = Some("  dealer quote ".into());
        let row = add_valuation(&conn, ValuationKind::OtherAsset, &data).unwrap();
        assert_eq!(row.valued_at, 20_000 * DAY);
        assert_eq!(row.note.as_deref(), Some("dealer quote"));
        assert_eq!(market_price(&conn, "other_assets", "oa1"), "68500");
    }

    #[test]
    fn deleting_the_newest_row_reverts_the_parent_price() {
        let conn = setup_test_db();
        let first = add_valuation(
            &conn,
            ValuationKind::OtherAsset,
            &insert("oa1", "59000", 19_500 * DAY),
        )
        .unwrap();
        let second = add_valuation(
            &conn,
            ValuationKind::OtherAsset,
            &insert("oa1", "68500", 20_000 * DAY),
        )
        .unwrap();
        assert_eq!(market_price(&conn, "other_assets", "oa1"), "68500");
        delete_valuation(&conn, ValuationKind::OtherAsset, &second.id).unwrap();
        assert_eq!(market_price(&conn, "other_assets", "oa1"), "59000");
        delete_valuation(&conn, ValuationKind::OtherAsset, &first.id).unwrap();
        // Nothing left: the parent keeps its last price
        assert_eq!(market_price(&conn, "other_assets", "oa1"), "59000");
        assert!(matches!(
            delete_valuation(&conn, ValuationKind::OtherAsset, &first.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn record_price_change_writes_todays_row_only_when_the_price_moved() {
        let conn = setup_test_db();
        let today = 20_100 * DAY;
        assert!(record_price_change(
            &conn,
            ValuationKind::RealEstate,
            "re1",
            "3480000",
            "CZK",
            today
        )
        .unwrap());
        assert!(!record_price_change(
            &conn,
            ValuationKind::RealEstate,
            "re1",
            "3480000.0",
            "czk",
            today
        )
        .unwrap());
        assert!(record_price_change(
            &conn,
            ValuationKind::RealEstate,
            "re1",
            "3650000",
            "CZK",
            today + 3_600
        )
        .unwrap());
        let rows = list_valuations(&conn, ValuationKind::RealEstate, "re1").unwrap();
        // Same day: updated in place, not duplicated
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].value, "3650000");
        assert_eq!(rows[0].valued_at, today);
    }

    #[test]
    fn validation_and_missing_parent() {
        let conn = setup_test_db();
        let mut bad = insert("re1", "-1", 20_000 * DAY);
        assert!(matches!(
            add_valuation(&conn, ValuationKind::RealEstate, &bad),
            Err(AppError::Validation(msg)) if msg == "validation.invalidAmount"
        ));
        bad = insert("re1", "100", 0);
        assert!(matches!(
            add_valuation(&conn, ValuationKind::RealEstate, &bad),
            Err(AppError::Validation(msg)) if msg == "validation.invalidDate"
        ));
        bad = insert("re1", "100", 20_000 * DAY);
        bad.currency = Some("CZKK".into());
        assert!(matches!(
            add_valuation(&conn, ValuationKind::RealEstate, &bad),
            Err(AppError::Validation(msg)) if msg == "validation.currencyInvalid"
        ));
        assert!(matches!(
            add_valuation(
                &conn,
                ValuationKind::RealEstate,
                &insert("missing", "100", 20_000 * DAY)
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn value_at_day_picks_the_estimate_that_applied() {
        let conn = setup_test_db();
        add_valuation(
            &conn,
            ValuationKind::OtherAsset,
            &insert("oa1", "59000", 19_500 * DAY),
        )
        .unwrap();
        add_valuation(
            &conn,
            ValuationKind::OtherAsset,
            &insert("oa1", "68500", 20_000 * DAY),
        )
        .unwrap();
        let at = |day: i64| value_at_day(&conn, ValuationKind::OtherAsset, "oa1", day).unwrap();
        assert_eq!(at(19_000 * DAY), Some(59000.0)); // before the first estimate: the earliest one
        assert_eq!(at(19_700 * DAY + 7_200), Some(59000.0));
        assert_eq!(at(20_000 * DAY), Some(68500.0));
        assert_eq!(at(21_000 * DAY), Some(68500.0));
        assert_eq!(
            value_at_day(&conn, ValuationKind::OtherAsset, "none", 20_000 * DAY).unwrap(),
            None
        );
    }

    #[test]
    fn deleting_the_parent_cascades() {
        let conn = setup_test_db();
        add_valuation(
            &conn,
            ValuationKind::RealEstate,
            &insert("re1", "3650000", 20_000 * DAY),
        )
        .unwrap();
        conn.execute("DELETE FROM real_estate WHERE id = 're1'", [])
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM real_estate_valuations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
