//! Money helpers for the MCP tools (ADR 0006, ADR 0011).
//!
//! Aggregates come from the app's services in the user's main currency, with the app's
//! current quote snapshot (C-A), so the assistant and the app agree. An amount that cannot be
//! converted is excluded from aggregates or emitted as `null`, and its currency is listed in
//! the response's `missingCurrencies` (never relabelled or valued 1:1).

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// The user's main currency (`user_profile.currency`, trimmed and uppercased),
/// CZK when there is no profile or its currency is blank. The MCP write tools
/// use this to default a missing currency; the shared services keep their own
/// CZK default for the UI path (ADR 0007).
pub fn load_main_currency(conn: &Connection) -> Result<String> {
    let stored: Option<String> = conn
        .query_row("SELECT currency FROM user_profile LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(stored
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "CZK".to_string()))
}

/// A currency argument of an MCP write tool: the given code, or the user's
/// main currency when the argument is missing or blank (an empty or
/// whitespace-only string counts as missing). Applied in the MCP layer only;
/// the shared services keep their own CZK default for the UI path (ADR 0007).
pub fn currency_or_main(conn: &Connection, currency: Option<&str>) -> Result<String> {
    match currency.filter(|code| !code.trim().is_empty()) {
        Some(code) => Ok(code.to_string()),
        None => load_main_currency(conn),
    }
}

/// A converted amount as decimal text at the currency's ISO minor unit
/// (`reporting::money_in_currency`), or JSON `null` when it is not known.
pub fn amount_json(value: Option<f64>, currency: &str) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |v| {
        serde_json::json!(crate::services::reporting::money_in_currency(v, currency))
    })
}

/// A ratio in percent with two decimals, or JSON `null` when it is not known.
pub fn percent_json(value: Option<f64>) -> serde_json::Value {
    value.map_or(serde_json::Value::Null, |p| {
        serde_json::json!(format!("{p:.2}"))
    })
}

/// Every position of `kind` in the user's main currency: the app's positions service with
/// the app's current quote snapshot (C-A), so the assistant and the app agree.
pub fn main_currency_positions(
    conn: &Connection,
    kind: crate::services::positions::AssetKind,
) -> Result<crate::services::positions::Positions> {
    let main = load_main_currency(conn)?;
    crate::services::positions::value_positions(conn, kind, &main, chrono::Utc::now().timestamp())
}

/// Adds a position's native price and average cost and its main-currency figures to a list
/// row.
/// Unknown amounts are `null`; `missingCurrencies` names what this position could not convert.
pub fn add_position_fields(
    row: &mut serde_json::Value,
    item: Option<&crate::services::positions::PositionValue>,
    currency: &str,
) {
    let field = |f: fn(&crate::services::positions::PositionValue) -> Option<f64>| {
        amount_json(item.and_then(f), currency)
    };
    // The resolver's price (override vs API), so `currentPrice × quantity` matches `value`;
    // `null` when unpriced.
    row["currentPrice"] = serde_json::json!(item.and_then(|i| i.price).map(|p| p.to_string()));
    row["priceCurrency"] = serde_json::json!(item.and_then(|i| i.price_currency.clone()));
    row["averagePrice"] = serde_json::json!(
        item.map_or_else(|| "0".to_string(), |item| item.average_price.to_string())
    );
    row["value"] = field(|i| i.value);
    row["costBasis"] = field(|i| i.cost_basis);
    row["unrealizedGain"] = field(|i| i.unrealized_gain);
    row["unrealizedGainPct"] = percent_json(item.and_then(|i| i.unrealized_gain_percent));
    row["realizedGain"] = field(|i| i.realized_gain);
    if item.is_some_and(|i| i.dividend_per_share.is_some() || i.yearly_dividend.is_some()) {
        row["annualDividend"] = field(|i| i.yearly_dividend);
    }
    row["missingCurrencies"] = serde_json::json!(item
        .map(|i| i.quality.missing_currencies())
        .unwrap_or_default());
}

/// A list response: `mainCurrency`, the rows under `key`, the totals of the positions whose
/// value and cost are known, and every currency that could not be converted.
pub fn positions_response(
    positions: &crate::services::positions::Positions,
    key: &str,
    rows: Vec<serde_json::Value>,
) -> serde_json::Value {
    let currency = positions.currency.as_str();
    let totals = crate::services::positions::totals(&positions.items);
    let money = |v: f64| crate::services::reporting::money_in_currency(v, currency);
    let mut summary = serde_json::json!({
        "totalValue": money(totals.value),
        "totalCostBasis": money(totals.cost_basis),
        "totalUnrealizedGain": money(totals.unrealized_gain),
        "totalRealizedGain": money(totals.realized_gain),
    });
    // Stocks pay dividends; crypto has none.
    if positions.items.iter().any(|i| i.yearly_dividend.is_some()) {
        summary["annualDividends"] = serde_json::json!(money(totals.yearly_dividend));
    }
    serde_json::json!({
        "mainCurrency": currency,
        key: rows,
        "summary": summary,
        "missingCurrencies": positions.quality.missing_currencies(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db(profile_currency: Option<&str>) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE exchange_rates (
                currency TEXT PRIMARY KEY,
                rate REAL NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES
                ('EUR', 25.0, 0), ('USD', 20.0, 0), ('ZMN1', 0.0, 0);
            "#,
        )
        .expect("schema");
        if let Some(c) = profile_currency {
            conn.execute("INSERT INTO user_profile (currency) VALUES (?1)", [c])
                .expect("profile");
        }
        // The current snapshot, not the table, is the source (C-A); unusable quotes are
        // ignored by the snapshot.
        crate::services::fx::set_current_for_test(&[("EUR", 25.0), ("USD", 20.0), ("ZMN1", 0.0)]);
        conn
    }

    #[test]
    fn currency_or_main_keeps_a_given_code_and_fills_a_missing_one() {
        let conn = setup_db(Some("EUR"));

        assert_eq!(currency_or_main(&conn, Some("USD")).expect("given"), "USD");
        assert_eq!(currency_or_main(&conn, None).expect("missing"), "EUR");
    }

    #[test]
    fn a_blank_currency_counts_as_missing() {
        let conn = setup_db(Some("EUR"));

        assert_eq!(currency_or_main(&conn, Some("")).expect("empty"), "EUR");
        assert_eq!(currency_or_main(&conn, Some("  ")).expect("blank"), "EUR");
    }

    #[test]
    fn load_main_currency_reads_the_profile_or_falls_back_to_czk() {
        assert_eq!(
            load_main_currency(&setup_db(Some(" eur "))).expect("main"),
            "EUR"
        );
        assert_eq!(load_main_currency(&setup_db(None)).expect("main"), "CZK");
        assert_eq!(
            load_main_currency(&setup_db(Some("  "))).expect("main"),
            "CZK"
        );
    }
}
