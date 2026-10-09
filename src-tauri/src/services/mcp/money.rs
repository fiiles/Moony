//! Money context for the MCP tools (ADR 0001, ADR 0006).
//!
//! Aggregates are computed in CZK (the internal base currency) and converted
//! into the user's main currency once, at serialisation time. A `MoneyContext`
//! is loaded once per tool call so every amount in one response uses the same
//! quote snapshot (the app's current quotes, as the UI commands) and the same
//! main currency. An amount that cannot be converted is
//! excluded from aggregates or emitted as `null`, and its currency is listed
//! in the response's `missingCurrencies` (never relabelled or valued 1:1).

use std::cell::RefCell;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;
use crate::services::fx::{self, History, Quality, Quotes};

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

pub struct MoneyContext {
    main: String,
    /// The request's quote snapshot: the app's current quotes (`fx::current()`), the same
    /// source the UI commands use (C-A).
    quotes: Quotes,
    /// Currencies that could not be converted while building this response.
    quality: RefCell<Quality>,
}

impl MoneyContext {
    /// Captures the current quote snapshot and reads the user's main currency
    /// (`user_profile.currency`, falling back to CZK when there is no profile).
    pub fn load(conn: &Connection) -> Result<MoneyContext> {
        Self::with_quotes(conn, fx::current())
    }

    /// Like `load`, with a snapshot the caller already captured for this request.
    pub fn with_quotes(conn: &Connection, quotes: Quotes) -> Result<MoneyContext> {
        Ok(MoneyContext {
            main: load_main_currency(conn)?,
            quotes,
            quality: RefCell::new(Quality::default()),
        })
    }

    /// The request's quote snapshot.
    pub fn quotes(&self) -> &Quotes {
        &self.quotes
    }

    /// The user's main currency (3-letter uppercase code).
    pub fn main_currency(&self) -> &str {
        &self.main
    }

    /// Converts an amount in `currency` to CZK for aggregation. An amount that cannot be
    /// converted is excluded (0.0) and its currency is listed in `missing_currencies`.
    pub fn to_czk(&self, amount: f64, currency: &str) -> f64 {
        self.try_to_czk(amount, currency).unwrap_or(0.0)
    }

    /// Like `to_czk`, but `None` for an unconvertible amount, so a caller can null the row it
    /// belongs to. The currency is listed in `missing_currencies` either way.
    pub fn try_to_czk(&self, amount: f64, currency: &str) -> Option<f64> {
        let converted = self.quotes.convert(amount, currency, "CZK");
        self.quality.borrow_mut().track(currency, converted)
    }

    /// Converts a CZK amount into the main currency; `None` (and the main currency listed as
    /// missing) when it has no quote.
    pub fn czk_to_main(&self, czk: f64) -> Option<f64> {
        let converted = self.quotes.convert(czk, "CZK", &self.main);
        self.quality.borrow_mut().track(&self.main, converted)
    }

    /// A CZK amount in the main currency as a two-decimal string like the other money fields,
    /// or JSON `null` when it cannot be converted. Never the CZK number relabelled.
    pub fn money(&self, czk: f64) -> serde_json::Value {
        match self.czk_to_main(czk) {
            Some(value) => serde_json::json!(format!("{value:.2}")),
            None => serde_json::Value::Null,
        }
    }

    /// Merges conversions another service could not make (e.g. cost basis) into this response.
    pub fn absorb(&self, quality: &Quality) {
        self.quality.borrow_mut().merge(quality);
    }

    /// Currencies excluded or nulled in this response, sorted.
    pub fn missing_currencies(&self) -> Vec<String> {
        self.quality.borrow().missing_currencies()
    }

    /// Loads the rate history needed to convert rows recorded on `days`. Empty when nothing
    /// needs it (a CZK main currency converts nothing, and no rows means no days).
    pub fn day_rates_for(&self, conn: &Connection, days: &[i64]) -> Result<History> {
        if self.main == "CZK" {
            return Ok(History::default());
        }
        match (days.iter().min(), days.iter().max()) {
            (Some(first), Some(last)) => History::load(conn, *first, *last),
            _ => Ok(History::default()),
        }
    }

    /// Converts a CZK amount into the main currency at the rates of `day` (ADR 0001:
    /// historical values use the rates of their day): the closest earlier day, at most ten
    /// days back, that quotes both currencies. `None` (main currency listed as missing) when
    /// there is none; current rates are never substituted.
    pub fn czk_to_main_on(&self, czk: f64, day: i64, history: &History) -> Option<f64> {
        if self.main == "CZK" {
            return Some(czk);
        }
        let converted = history.convert_on(czk, "CZK", &self.main, day);
        self.quality.borrow_mut().track(&self.main, converted)
    }

    /// Converts a stored CZK money cell (TEXT in production) into the main
    /// currency at the rates of `day`, as a two-decimal string like the other
    /// money fields. A CZK main currency returns the stored value untouched;
    /// NULL stays NULL; an unparseable cell counts as 0 (the convention of the
    /// other MCP readers); an amount without usable rates becomes `null`.
    pub fn stored_czk_to_main_on(
        &self,
        cell: rusqlite::types::Value,
        day: i64,
        history: &History,
    ) -> serde_json::Value {
        if self.main == "CZK" {
            return super::sql_to_json(cell);
        }
        let czk = match cell {
            rusqlite::types::Value::Null | rusqlite::types::Value::Blob(_) => {
                return serde_json::Value::Null
            }
            rusqlite::types::Value::Integer(i) => i as f64,
            rusqlite::types::Value::Real(f) => f,
            rusqlite::types::Value::Text(s) => s.trim().parse::<f64>().unwrap_or(0.0),
        };
        match self.czk_to_main_on(czk, day, history) {
            Some(value) => serde_json::json!(format!("{value:.2}")),
            None => serde_json::Value::Null,
        }
    }
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

    fn approx(value: Option<f64>, expected: f64) {
        let value = value.expect("converted");
        assert!((value - expected).abs() < 1e-9, "{value} != {expected}");
    }

    #[test]
    fn eur_main_currency_converts_czk_by_the_current_snapshot() {
        let ctx = MoneyContext::load(&setup_db(Some("EUR"))).expect("load");
        assert_eq!(ctx.main_currency(), "EUR");
        approx(ctx.czk_to_main(2500.0), 100.0);
        assert!(ctx.missing_currencies().is_empty());
    }

    #[test]
    fn missing_profile_row_means_czk_and_identity() {
        let ctx = MoneyContext::load(&setup_db(None)).expect("load");
        assert_eq!(ctx.main_currency(), "CZK");
        assert_eq!(ctx.czk_to_main(2500.0), Some(2500.0));
    }

    #[test]
    fn empty_profile_currency_falls_back_to_czk() {
        let ctx = MoneyContext::load(&setup_db(Some("  "))).expect("load");
        assert_eq!(ctx.main_currency(), "CZK");
        assert_eq!(ctx.czk_to_main(10.0), Some(10.0));
    }

    #[test]
    fn profile_currency_is_trimmed_and_uppercased() {
        let ctx = MoneyContext::load(&setup_db(Some(" eur "))).expect("load");
        assert_eq!(ctx.main_currency(), "EUR");
        approx(ctx.czk_to_main(2500.0), 100.0);
    }

    #[test]
    fn to_czk_uses_the_current_snapshot() {
        let ctx = MoneyContext::load(&setup_db(Some("EUR"))).expect("load");
        assert!((ctx.to_czk(10.0, "USD") - 200.0).abs() < 1e-9);
        assert!((ctx.to_czk(10.0, "EUR") - 250.0).abs() < 1e-9);
        assert_eq!(ctx.to_czk(10.0, "CZK"), 10.0);
        assert!(ctx.missing_currencies().is_empty());
    }

    #[test]
    fn an_unquoted_amount_is_excluded_and_reported() {
        let ctx = MoneyContext::load(&setup_db(Some("EUR"))).expect("load");
        assert_eq!(ctx.to_czk(10.0, "KWD"), 0.0);
        assert_eq!(ctx.missing_currencies(), vec!["KWD".to_string()]);
    }

    #[test]
    fn main_without_a_table_rate_yields_null_and_is_reported() {
        // JPY is not in the snapshot: no invented rate, no relabelled CZK.
        let ctx = MoneyContext::load(&setup_db(Some("JPY"))).expect("load");
        assert_eq!(ctx.czk_to_main(1500.0), None);
        assert_eq!(ctx.money(1500.0), serde_json::Value::Null);
        assert_eq!(ctx.missing_currencies(), vec!["JPY".to_string()]);
    }

    #[test]
    fn main_with_a_non_positive_rate_is_unusable_not_a_division_by_zero() {
        let ctx = MoneyContext::load(&setup_db(Some("ZMN"))).expect("load");
        assert_eq!(ctx.czk_to_main(1500.0), None);
        assert_eq!(ctx.missing_currencies(), vec!["ZMN".to_string()]);
    }

    // ---- day-rate conversion (history rows, ADR 0001) ----

    const DAY: i64 = 86_400;

    fn add_history(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )
        .expect("history row");
    }

    fn setup_history_db(profile_currency: Option<&str>) -> Connection {
        let conn = setup_db(profile_currency);
        conn.execute_batch(
            "CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );",
        )
        .expect("history schema");
        conn
    }

    fn load_day_rates(conn: &Connection, ctx: &MoneyContext, first: i64, last: i64) -> History {
        ctx.day_rates_for(conn, &[first, last]).expect("history")
    }

    #[test]
    fn day_rates_convert_each_day_at_its_own_rate() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        add_history(&conn, 101 * DAY, "EUR", 26.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 101 * DAY);

        approx(ctx.czk_to_main_on(2400.0, 100 * DAY, &rates), 100.0);
        approx(ctx.czk_to_main_on(2600.0, 101 * DAY, &rates), 100.0);
        // The current rate (25) would give a different answer for both days.
        approx(ctx.czk_to_main(2400.0), 96.0);
    }

    #[test]
    fn a_day_without_a_snapshot_walks_back_to_the_closest_earlier_day() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        // The row sits three days after the snapshot; the range query has to
        // reach back that far.
        let rates = load_day_rates(&conn, &ctx, 103 * DAY, 103 * DAY);

        approx(ctx.czk_to_main_on(2400.0, 103 * DAY, &rates), 100.0);
    }

    #[test]
    fn a_day_with_no_history_is_null_and_reported_not_the_current_rate() {
        let conn = setup_history_db(Some("EUR"));
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2500.0, 100 * DAY, &rates), None);
        assert_eq!(ctx.missing_currencies(), vec!["EUR".to_string()]);
    }

    #[test]
    fn a_snapshot_without_the_main_currency_is_null_not_the_current_rate() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "USD", 21.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2500.0, 100 * DAY, &rates), None);
    }

    #[test]
    fn czk_main_currency_ignores_day_rates() {
        let conn = setup_history_db(Some("CZK"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 100 * DAY);

        assert_eq!(ctx.czk_to_main_on(2400.0, 100 * DAY, &rates), Some(2400.0));
    }

    #[test]
    fn stored_czk_text_is_converted_to_a_two_decimal_string() {
        let conn = setup_history_db(Some("EUR"));
        add_history(&conn, 100 * DAY, "EUR", 24.0);
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 100 * DAY);
        let text = |s: &str| rusqlite::types::Value::Text(s.to_string());

        assert_eq!(
            ctx.stored_czk_to_main_on(text("2500"), 100 * DAY, &rates),
            serde_json::json!("104.17")
        );
        assert_eq!(
            ctx.stored_czk_to_main_on(rusqlite::types::Value::Null, 100 * DAY, &rates),
            serde_json::Value::Null
        );
    }

    #[test]
    fn stored_czk_text_stays_untouched_for_a_czk_main_currency() {
        let conn = setup_history_db(Some("CZK"));
        let ctx = MoneyContext::load(&conn).expect("load");
        let rates = load_day_rates(&conn, &ctx, 100 * DAY, 100 * DAY);

        assert_eq!(
            ctx.stored_czk_to_main_on(
                rusqlite::types::Value::Text("1234.5678".to_string()),
                100 * DAY,
                &rates
            ),
            serde_json::json!("1234.5678")
        );
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
    }
}
