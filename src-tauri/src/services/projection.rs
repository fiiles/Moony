//! Portfolio projection in a report currency (ADR 0011).
//!
//! Today's values come from the net-worth service (native breakdowns converted with ONE quote
//! snapshot per request); the monthly contributions are stored in the currency the user typed
//! and converted here, at projection time. A holding or contribution in a currency the
//! snapshot cannot convert is left out and named in the response's `fxQuality`. The formulas
//! live in `projection_math` (pure) and in the per-class helpers below.

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::Result;
use crate::models::fx::ConversionQuality;
use crate::models::{
    CalculatedDefaults, PortfolioProjection, ProjectionInput, ProjectionSettings,
    ProjectionTimelinePoint,
};
use crate::services::fx::{self, Quality, Quotes};
use crate::services::net_worth;
use crate::services::projection_math::{
    contribution_future_value, parse_rate_setting, projection_date,
};
use crate::services::reporting::{money_in_currency, normalize_currency};

/// All stored projection settings.
pub fn list_settings(conn: &Connection) -> Result<Vec<ProjectionSettings>> {
    let mut stmt = conn.prepare(
        "SELECT id, asset_type, yearly_growth_rate, monthly_contribution,
                contribution_currency, enabled, created_at, updated_at
         FROM projection_settings",
    )?;
    let settings = stmt
        .query_map([], |row| {
            Ok(ProjectionSettings {
                id: row.get(0)?,
                asset_type: row.get(1)?,
                yearly_growth_rate: row.get(2)?,
                monthly_contribution: row.get(3)?,
                contribution_currency: row.get(4)?,
                enabled: row.get::<_, i32>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(settings)
}

/// Upserts the settings by asset type. Every entry is validated before anything is written,
/// and the batch is saved together or not at all.
pub fn save_settings(conn: &Connection, settings: Vec<ProjectionSettings>) -> Result<()> {
    for setting in &settings {
        setting.validate()?;
    }
    let now = Utc::now().timestamp();
    let tx = conn.unchecked_transaction()?;
    for s in settings {
        let id = if s.id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            s.id
        };
        // validate() accepted it, so normalisation always yields a code
        let currency = fx::normalize(&s.contribution_currency).unwrap_or(s.contribution_currency);
        tx.execute(
            "INSERT INTO projection_settings
                (id, asset_type, yearly_growth_rate, monthly_contribution,
                 contribution_currency, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(asset_type) DO UPDATE SET
                yearly_growth_rate = excluded.yearly_growth_rate,
                monthly_contribution = excluded.monthly_contribution,
                contribution_currency = excluded.contribution_currency,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            rusqlite::params![
                id,
                s.asset_type,
                s.yearly_growth_rate,
                s.monthly_contribution,
                currency,
                if s.enabled { 1 } else { 0 },
                now,
                now,
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Projection in `report_currency` at the current quote snapshot.
pub fn calculate_projection(
    conn: &Connection,
    report_currency: &str,
    input: &ProjectionInput,
) -> Result<PortfolioProjection> {
    calculate_projection_at(conn, &fx::current(), report_currency, input, Utc::now())
}

/// Projection in `report_currency` with the caller's quote snapshot and start instant.
pub fn calculate_projection_at(
    conn: &Connection,
    quotes: &Quotes,
    report_currency: &str,
    input: &ProjectionInput,
    now: DateTime<Utc>,
) -> Result<PortfolioProjection> {
    let currency = normalize_currency(report_currency)?;
    let mut quality = Quality::default();

    // 1. Today's values and the weighted rates, in the report currency
    let current = current_values(
        conn,
        quotes,
        &currency,
        input.exclude_personal_real_estate,
        &mut quality,
    )?;

    // 2. Settings (defaults where not set); contributions converted into the report currency
    let settings = get_settings_map(conn, input.rate_shift, quotes, &currency, &mut quality)?;

    // 3. Timeline points
    let is_monthly = input.view_type == "monthly";
    let total_periods = if is_monthly {
        input.horizon_years * 12
    } else {
        input.horizon_years
    };

    let text = |value: f64| money_in_currency(value, &currency);
    let point = |date: i64, assets: f64, liabilities: f64, classes: [f64; 6]| {
        let [savings, investments, crypto, bonds, real_estate, other_assets] = classes;
        ProjectionTimelinePoint {
            date,
            total_assets: text(assets),
            total_liabilities: text(liabilities),
            net_worth: text(assets - liabilities),
            savings: text(savings),
            investments: text(investments),
            crypto: text(crypto),
            bonds: text(bonds),
            real_estate: text(real_estate),
            other_assets: text(other_assets),
            loans: text(liabilities),
        }
    };

    let mut timeline = Vec::with_capacity(total_periods.max(0) as usize + 1);
    timeline.push(point(
        now.timestamp(),
        current.total_assets,
        current.total_liabilities,
        [
            current.savings,
            current.investments,
            current.crypto,
            current.bonds,
            current.real_estate,
            current.other_assets,
        ],
    ));

    let mut total_contributions = 0.0;
    let mut projected_net_worth = current.net_worth;

    // Project forward
    for period in 1..=total_periods {
        // Calendar months/years in UTC, not 30/365-day steps
        let period_date = projection_date(now, period, is_monthly);

        let years_elapsed = if is_monthly {
            period as f64 / 12.0
        } else {
            period as f64
        };

        let months_elapsed = if is_monthly { period } else { period * 12 };

        // Contributions are reported for the final period only
        let is_final_period = period == total_periods;
        let mut period_contributions = 0.0;

        let savings = project_savings(
            &current,
            &settings,
            years_elapsed,
            months_elapsed,
            &mut period_contributions,
        );
        let investments = project_investments(
            &current,
            &settings,
            years_elapsed,
            months_elapsed,
            &mut period_contributions,
        );
        let crypto = project_crypto(
            &current,
            &settings,
            years_elapsed,
            months_elapsed,
            &mut period_contributions,
        );
        let bonds = project_bonds(
            &current,
            &settings,
            years_elapsed,
            months_elapsed,
            &mut period_contributions,
        );
        let real_estate = project_real_estate(&current, &settings, years_elapsed);
        let other = project_other_assets(
            &current,
            &settings,
            years_elapsed,
            months_elapsed,
            &mut period_contributions,
        );
        let loans = project_loans(&current, months_elapsed).max(0.0);

        if is_final_period {
            total_contributions = period_contributions;
        }

        let total_assets = savings + investments + crypto + bonds + real_estate + other;
        projected_net_worth = total_assets - loans;

        timeline.push(point(
            period_date.timestamp(),
            total_assets,
            loans,
            [savings, investments, crypto, bonds, real_estate, other],
        ));
    }

    let total_growth = projected_net_worth - current.net_worth - total_contributions;

    Ok(PortfolioProjection {
        currency: currency.clone(),
        fx_quality: ConversionQuality::from(&quality),
        horizon_years: input.horizon_years,
        view_type: input.view_type.clone(),
        timeline,
        projected_net_worth: text(projected_net_worth),
        total_contributions: text(total_contributions),
        total_growth: text(total_growth),
        calculated_defaults: CalculatedDefaults {
            savings_rate: current.savings_weighted_interest,
            bonds_rate: current.bonds_weighted_yield,
        },
    })
}

/// Current portfolio values for projection starting point
struct CurrentValues {
    savings: f64,
    investments: f64,
    crypto: f64,
    bonds: f64,
    real_estate: f64,
    other_assets: f64,
    total_assets: f64,
    total_liabilities: f64,
    net_worth: f64,
    // For loan amortization
    loan_monthly_payment: f64,
    loan_interest_rate: f64,
    // For savings interest
    savings_weighted_interest: f64,
    // For bonds yield
    bonds_weighted_yield: f64,
}

/// Settings map with defaults
///
/// `savings_growth` and `bonds_growth` are `Option`: `Some(rate)` is the user's
/// explicit choice (an explicit `Some(0.0)` is honoured), `None` means "use the
/// weighted rate calculated from the user's own accounts / bonds".
/// The other classes have a constant default applied when the setting is
/// missing, so they stay plain numbers.
///
/// A class whose setting row is disabled is *frozen*: it stays at today's
/// value, with no growth and no contributions (design system §7 Outlook).
#[derive(Default)]
struct SettingsMap {
    savings_growth: Option<f64>,
    savings_contribution: f64,
    savings_frozen: bool,
    investments_growth: f64,
    investments_contribution: f64,
    investments_frozen: bool,
    crypto_growth: f64,
    crypto_contribution: f64,
    crypto_frozen: bool,
    bonds_growth: Option<f64>,
    bonds_contribution: f64,
    bonds_frozen: bool,
    real_estate_growth: f64,
    real_estate_frozen: bool,
    other_growth: f64,
    other_contribution: f64,
    other_frozen: bool,
    /// Percentage points added to every active class's annual rate.
    rate_shift: f64,
}

impl SettingsMap {
    /// Effective annual rate (as a fraction) of a class: the configured or
    /// derived percentage plus the scenario shift; a frozen class has none.
    fn rate(&self, percent: f64, frozen: bool) -> f64 {
        if frozen {
            0.0
        } else {
            (percent + self.rate_shift) / 100.0
        }
    }

    /// Monthly contribution of a class; a frozen class contributes nothing.
    fn contribution(&self, monthly: f64, frozen: bool) -> f64 {
        if frozen {
            0.0
        } else {
            monthly
        }
    }
}

/// Today's values in `currency` for the projection's starting point. The class totals come
/// from the net-worth service (their misses land in `quality`); the rate weights and loan
/// payments convert the same amounts again without recording, so a currency is never counted
/// twice.
fn current_values(
    conn: &Connection,
    quotes: &Quotes,
    currency: &str,
    exclude_personal_real_estate: bool,
    quality: &mut Quality,
) -> Result<CurrentValues> {
    let worth = net_worth::current_net_worth_with_quotes(
        conn,
        quotes,
        currency,
        exclude_personal_real_estate,
    )?;
    quality.merge(&worth.quality);
    let classes = worth.classes;
    let convert = |amount: f64, from: &str| quotes.convert(amount, from, currency);

    // Weighted average interest rate of the accounts (excluding 0 % accounts)
    let mut stmt = conn.prepare(
        "SELECT balance, currency, interest_rate FROM bank_accounts WHERE exclude_from_balance = 0",
    )?;
    let savings_data: Vec<(f64, f64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(balance, account_currency, rate)| {
            let balance: f64 = balance.parse().unwrap_or(0.0);
            let rate: f64 = rate.parse().unwrap_or(0.0);
            Some((convert(balance, &account_currency)?, rate))
        })
        .collect();
    let weighted_interest = weighted_rate(&savings_data);

    // Weighted average bond yield (excluding 0 % bonds)
    let mut stmt =
        conn.prepare("SELECT coupon_value, quantity, currency, interest_rate FROM bonds")?;
    let bonds_data: Vec<(f64, f64)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(value, quantity, bond_currency, rate)| {
            let value: f64 = value.parse().unwrap_or(0.0);
            let quantity: f64 = quantity.parse().unwrap_or(1.0);
            let rate: f64 = rate.parse().unwrap_or(0.0);
            Some((convert(value * quantity, &bond_currency)?, rate))
        })
        .collect();
    let weighted_bond_yield = weighted_rate(&bonds_data);

    // Loans: the amortized balance as of today, the payments and the weighted rate
    let today = crate::services::loan_amortization::today_utc_day();
    let loans_data: Vec<(f64, f64, f64)> = crate::services::loans::load_ledgers(conn)?
        .iter()
        .filter_map(|entry| {
            let outstanding = convert(
                crate::services::loan_amortization::loan_balance_at(&entry.ledger, today).balance,
                &entry.currency,
            )?;
            let payment = convert(entry.ledger.terms.monthly_payment, &entry.currency)?;
            Some((outstanding, entry.ledger.terms.annual_rate_pct, payment))
        })
        .collect();
    let total_liabilities = classes.loans.unwrap_or(0.0);
    let loans_total: f64 = loans_data.iter().map(|(p, _, _)| p).sum();
    let total_monthly_payment: f64 = loans_data.iter().map(|(_, _, m)| m).sum();
    // Weighted avg interest on loans for simplicity
    let weighted_loan_interest = if loans_total > 0.0 {
        loans_data
            .iter()
            .map(|(p, r, _)| p * r / 100.0)
            .sum::<f64>()
            / loans_total
    } else {
        0.0
    };

    let real_estate = classes
        .real_estate(exclude_personal_real_estate)
        .unwrap_or(0.0);
    Ok(CurrentValues {
        savings: classes.savings.unwrap_or(0.0),
        investments: classes.investments.unwrap_or(0.0),
        crypto: classes.crypto.unwrap_or(0.0),
        bonds: classes.bonds.unwrap_or(0.0),
        real_estate,
        other_assets: classes.other_assets.unwrap_or(0.0),
        // Unconvertible classes already count as zero above, so unknown totals do too
        total_assets: worth.total_assets.unwrap_or(0.0),
        total_liabilities,
        net_worth: worth.net_worth.unwrap_or(0.0),
        loan_monthly_payment: total_monthly_payment,
        loan_interest_rate: weighted_loan_interest,
        savings_weighted_interest: weighted_interest,
        bonds_weighted_yield: weighted_bond_yield,
    })
}

/// Balance-weighted average of the positive rates over the positive balances (0 when none).
fn weighted_rate(weighted: &[(f64, f64)]) -> f64 {
    let positive: Vec<&(f64, f64)> = weighted
        .iter()
        .filter(|(weight, rate)| *rate > 0.0 && *weight > 0.0)
        .collect();
    let total: f64 = positive.iter().map(|(weight, _)| weight).sum();
    if total > 0.0 {
        positive
            .iter()
            .map(|(weight, rate)| weight * rate)
            .sum::<f64>()
            / total
    } else {
        0.0
    }
}

/// Reads the settings rows. A contribution is stored in its own `contribution_currency` and
/// converted into `currency` here; one that cannot be converted counts as 0 and is recorded in
/// `quality` (a frozen class contributes nothing, so it records nothing).
fn get_settings_map(
    conn: &Connection,
    rate_shift: f64,
    quotes: &Quotes,
    currency: &str,
    quality: &mut Quality,
) -> Result<SettingsMap> {
    let mut stmt = conn.prepare(
        "SELECT asset_type, yearly_growth_rate, monthly_contribution, contribution_currency, enabled
         FROM projection_settings",
    )?;

    let mut settings = SettingsMap {
        savings_growth: None,    // Will use weighted avg from accounts
        investments_growth: 7.0, // Default 7% S&P average
        crypto_growth: 7.0,      // Same as investments
        bonds_growth: None,      // Will use bond interest rates
        real_estate_growth: 3.0, // Conservative appreciation
        other_growth: 0.0,
        rate_shift,
        ..SettingsMap::default()
    };

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i32>(4)? != 0,
        ))
    })?;

    for (asset_type, rate_text, contrib_text, contrib_currency, enabled) in
        rows.filter_map(|r| r.ok())
    {
        // Empty / unusable text = "not set" -> class default; "0" = explicit 0 %
        let rate = parse_rate_setting(&rate_text);
        // A disabled row freezes the class at today's value
        let frozen = !enabled;
        let typed: f64 = contrib_text.parse().unwrap_or(0.0);
        let contrib = if typed == 0.0 || frozen {
            0.0
        } else {
            let from = contrib_currency.trim().to_uppercase();
            quality
                .track_conversion(
                    quotes,
                    &from,
                    currency,
                    quotes.convert(typed, &from, currency),
                )
                .unwrap_or(0.0)
        };

        match asset_type.as_str() {
            "savings" => {
                settings.savings_growth = rate;
                settings.savings_contribution = contrib;
                settings.savings_frozen = frozen;
            }
            "investments" => {
                settings.investments_growth = rate.unwrap_or(settings.investments_growth);
                settings.investments_contribution = contrib;
                settings.investments_frozen = frozen;
            }
            "crypto" => {
                settings.crypto_growth = rate.unwrap_or(settings.crypto_growth);
                settings.crypto_contribution = contrib;
                settings.crypto_frozen = frozen;
            }
            "bonds" => {
                settings.bonds_growth = rate;
                settings.bonds_contribution = contrib;
                settings.bonds_frozen = frozen;
            }
            "real_estate" => {
                settings.real_estate_growth = rate.unwrap_or(settings.real_estate_growth);
                settings.real_estate_frozen = frozen;
            }
            "other_assets" => {
                settings.other_growth = rate.unwrap_or(settings.other_growth);
                settings.other_contribution = contrib;
                settings.other_frozen = frozen;
            }
            _ => {}
        }
    }

    Ok(settings)
}

// Projection helper functions

fn project_savings(
    current: &CurrentValues,
    settings: &SettingsMap,
    years: f64,
    months: i32,
    contributions: &mut f64,
) -> f64 {
    // Use weighted avg interest from accounts, or override if set
    let percent = settings
        .savings_growth
        .unwrap_or(current.savings_weighted_interest);
    let rate = settings.rate(percent, settings.savings_frozen);

    // Compound interest: FV = PV * (1 + r)^n
    let growth_factor = (1.0 + rate).powf(years);
    let base = current.savings * growth_factor;

    // Monthly contributions with compound growth
    let monthly_contrib =
        settings.contribution(settings.savings_contribution, settings.savings_frozen);
    let total_contrib = monthly_contrib * months as f64;
    *contributions += total_contrib;

    // Monthly contributions compound monthly
    let contrib_value = contribution_future_value(monthly_contrib, rate, months);

    base + contrib_value
}

fn project_investments(
    current: &CurrentValues,
    settings: &SettingsMap,
    years: f64,
    months: i32,
    contributions: &mut f64,
) -> f64 {
    let rate = settings.rate(settings.investments_growth, settings.investments_frozen);
    let growth_factor = (1.0 + rate).powf(years);
    let base = current.investments * growth_factor;

    let monthly_contrib = settings.contribution(
        settings.investments_contribution,
        settings.investments_frozen,
    );
    let total_contrib = monthly_contrib * months as f64;
    *contributions += total_contrib;

    let contrib_value = contribution_future_value(monthly_contrib, rate, months);

    base + contrib_value
}

fn project_crypto(
    current: &CurrentValues,
    settings: &SettingsMap,
    years: f64,
    months: i32,
    contributions: &mut f64,
) -> f64 {
    let rate = settings.rate(settings.crypto_growth, settings.crypto_frozen);
    let growth_factor = (1.0 + rate).powf(years);
    let base = current.crypto * growth_factor;

    let monthly_contrib =
        settings.contribution(settings.crypto_contribution, settings.crypto_frozen);
    let total_contrib = monthly_contrib * months as f64;
    *contributions += total_contrib;

    let contrib_value = contribution_future_value(monthly_contrib, rate, months);

    base + contrib_value
}

fn project_bonds(
    current: &CurrentValues,
    settings: &SettingsMap,
    years: f64,
    months: i32,
    contributions: &mut f64,
) -> f64 {
    // Use weighted avg yield from bonds, or override if set
    let percent = settings
        .bonds_growth
        .unwrap_or(current.bonds_weighted_yield);
    let rate = settings.rate(percent, settings.bonds_frozen);

    let growth_factor = (1.0 + rate).powf(years);
    let base = current.bonds * growth_factor;

    let monthly_contrib = settings.contribution(settings.bonds_contribution, settings.bonds_frozen);
    let total_contrib = monthly_contrib * months as f64;
    *contributions += total_contrib;

    let contrib_value = contribution_future_value(monthly_contrib, rate, months);

    base + contrib_value
}

fn project_real_estate(current: &CurrentValues, settings: &SettingsMap, years: f64) -> f64 {
    let rate = settings.rate(settings.real_estate_growth, settings.real_estate_frozen);
    current.real_estate * (1.0 + rate).powf(years)
}

fn project_other_assets(
    current: &CurrentValues,
    settings: &SettingsMap,
    years: f64,
    months: i32,
    contributions: &mut f64,
) -> f64 {
    let rate = settings.rate(settings.other_growth, settings.other_frozen);
    let growth_factor = (1.0 + rate).powf(years);
    let base = current.other_assets * growth_factor;

    let monthly_contrib = settings.contribution(settings.other_contribution, settings.other_frozen);
    let total_contrib = monthly_contrib * months as f64;
    *contributions += total_contrib;

    let contrib_value = contribution_future_value(monthly_contrib, rate, months);

    base + contrib_value
}

fn project_loans(current: &CurrentValues, months: i32) -> f64 {
    // Simple amortization: reduce principal by monthly payments
    // This is a simplified model - actual amortization depends on specific loan terms
    if current.total_liabilities <= 0.0 || current.loan_monthly_payment <= 0.0 {
        return current.total_liabilities;
    }

    let monthly_rate = current.loan_interest_rate / 12.0;
    let mut balance = current.total_liabilities;

    for _ in 0..months {
        if balance <= 0.0 {
            break;
        }
        let interest = balance * monthly_rate;
        let principal_payment = (current.loan_monthly_payment - interest).max(0.0);
        balance -= principal_payment;
    }

    balance.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create test CurrentValues
    fn make_test_current() -> CurrentValues {
        CurrentValues {
            savings: 100_000.0,
            investments: 200_000.0,
            crypto: 50_000.0,
            bonds: 100_000.0,
            real_estate: 500_000.0,
            other_assets: 25_000.0,
            total_assets: 975_000.0,
            total_liabilities: 300_000.0,
            net_worth: 675_000.0,
            loan_monthly_payment: 5_000.0,
            loan_interest_rate: 0.05, // 5% annual
            savings_weighted_interest: 3.0,
            bonds_weighted_yield: 5.0,
        }
    }

    /// Helper to create test SettingsMap
    fn make_test_settings() -> SettingsMap {
        SettingsMap {
            savings_growth: Some(3.0),
            savings_contribution: 1000.0,
            investments_growth: 7.0,
            investments_contribution: 2000.0,
            crypto_growth: 7.0,
            crypto_contribution: 500.0,
            bonds_growth: Some(5.0),
            bonds_contribution: 0.0,
            real_estate_growth: 3.0,
            other_growth: 0.0,
            other_contribution: 0.0,
            ..SettingsMap::default()
        }
    }

    // =====================================================================
    // SCENARIO SHIFT AND FROZEN CLASSES (design system §7 Outlook)
    // =====================================================================

    #[test]
    fn test_rate_shift_moves_the_compound_factor() {
        let current = make_test_current();
        let mut settings = make_test_settings();
        settings.investments_contribution = 0.0;
        settings.rate_shift = 2.0;
        let mut c = 0.0;
        // 7 % + 2 p.p. = 9 % over 10 years
        let value = project_investments(&current, &settings, 10.0, 120, &mut c);
        let expected = 200_000.0 * 1.09f64.powf(10.0);
        assert!((value - expected).abs() < 1.0, "{value} vs {expected}");
    }

    #[test]
    fn test_negative_shift_applies_to_derived_rates_too() {
        let current = make_test_current(); // bonds weighted yield 5 %
        let mut settings = make_test_settings();
        settings.bonds_growth = None; // no override: the weighted yield is used
        settings.bonds_contribution = 0.0;
        settings.rate_shift = -2.0;
        let mut c = 0.0;
        let value = project_bonds(&current, &settings, 1.0, 12, &mut c);
        assert!((value - 103_000.0).abs() < 0.01, "{value}");
    }

    #[test]
    fn test_frozen_class_keeps_todays_value_and_contributes_nothing() {
        let current = make_test_current();
        let mut settings = make_test_settings();
        settings.savings_frozen = true;
        settings.rate_shift = 2.0;
        let mut c = 0.0;
        let value = project_savings(&current, &settings, 10.0, 120, &mut c);
        assert_eq!(value, 100_000.0);
        assert_eq!(c, 0.0);
    }

    #[test]
    fn test_settings_map_reads_disabled_rows_as_frozen() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE projection_settings (
                id TEXT PRIMARY KEY,
                asset_type TEXT NOT NULL,
                yearly_growth_rate TEXT NOT NULL,
                monthly_contribution TEXT NOT NULL,
                contribution_currency TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            INSERT INTO projection_settings VALUES ('a', 'investments', '8', '1500', 'CZK', 1, 0, 0);
            INSERT INTO projection_settings VALUES ('b', 'other_assets', '4', '200', 'CZK', 0, 0, 0);",
        )
        .expect("schema");
        let settings = settings_for(&conn, 1.5);
        assert_eq!(settings.investments_growth, 8.0);
        assert_eq!(settings.investments_contribution, 1500.0);
        assert!(!settings.investments_frozen);
        assert!(settings.other_frozen);
        assert_eq!(settings.rate_shift, 1.5);
        // Missing rows keep their defaults and stay active
        assert_eq!(settings.real_estate_growth, 3.0);
        assert!(!settings.real_estate_frozen);
    }

    // =====================================================================
    // COMPOUND GROWTH TESTS
    // =====================================================================

    #[test]
    fn test_compound_growth_1_year_10_percent() {
        // 100,000 at 10% for 1 year should = 110,000
        let base: f64 = 100_000.0;
        let rate: f64 = 0.10;
        let years: f64 = 1.0;
        let result = base * (1.0_f64 + rate).powf(years);
        assert!(
            (result - 110_000.0).abs() < 0.01,
            "Expected 110,000, got {}",
            result
        );
    }

    #[test]
    fn test_compound_growth_5_years_10_percent() {
        // 100,000 at 10% for 5 years = 100,000 * 1.1^5 = 161,051.00
        let base: f64 = 100_000.0;
        let rate: f64 = 0.10;
        let years: f64 = 5.0;
        let result = base * (1.0_f64 + rate).powf(years);
        assert!(
            (result - 161_051.0).abs() < 1.0,
            "Expected ~161,051, got {}",
            result
        );
    }

    #[test]
    fn test_compound_growth_zero_rate() {
        // 100,000 at 0% for any years should stay 100,000
        let base: f64 = 100_000.0;
        let rate: f64 = 0.0;
        let years: f64 = 10.0;
        let result = base * (1.0_f64 + rate).powf(years);
        assert!(
            (result - 100_000.0).abs() < 0.01,
            "Expected 100,000, got {}",
            result
        );
    }

    // =====================================================================
    // MONTHLY CONTRIBUTIONS - through the real project_* functions
    // =====================================================================

    #[test]
    fn test_contributions_1000_monthly_7_percent_12_months() {
        // Monthly compounding: 1 000/month at 7 % for a year = 12 392.59.
        // (The annual-annuity form gave 12 000.00 and passed a loose
        // `> 12_000` assertion only through floating-point noise.)
        let mut current = make_test_current();
        current.investments = 0.0;
        let mut settings = make_test_settings();
        settings.investments_growth = 7.0;
        settings.investments_contribution = 1000.0;

        let mut contributions = 0.0;
        let result = project_investments(&current, &settings, 1.0, 12, &mut contributions);

        assert!((result - 12_392.59).abs() < 0.01, "got {result}");
        assert!((contributions - 12_000.0).abs() < 0.01);
    }

    #[test]
    fn test_contributions_1000_monthly_7_percent_10_years() {
        let mut current = make_test_current();
        current.investments = 0.0;
        let mut settings = make_test_settings();
        settings.investments_growth = 7.0;
        settings.investments_contribution = 1000.0;

        let mut contributions = 0.0;
        let result = project_investments(&current, &settings, 10.0, 120, &mut contributions);

        assert!((result - 173_085.0).abs() < 1.0, "got {result}");
    }

    #[test]
    fn test_contributions_at_zero_rate_are_the_plain_sum() {
        let mut current = make_test_current();
        current.investments = 0.0;
        let mut settings = make_test_settings();
        settings.investments_growth = 0.0;
        settings.investments_contribution = 1000.0;

        let mut contributions = 0.0;
        let result = project_investments(&current, &settings, 1.0, 12, &mut contributions);

        assert!((result - 12_000.0).abs() < 0.01, "got {result}");
    }

    #[test]
    fn test_every_contributing_class_uses_the_monthly_formula() {
        let mut current = make_test_current();
        current.savings = 0.0;
        current.investments = 0.0;
        current.crypto = 0.0;
        current.bonds = 0.0;
        current.other_assets = 0.0;
        let mut settings = make_test_settings();
        settings.savings_growth = Some(7.0);
        settings.investments_growth = 7.0;
        settings.crypto_growth = 7.0;
        settings.bonds_growth = Some(7.0);
        settings.other_growth = 7.0;
        settings.savings_contribution = 1000.0;
        settings.investments_contribution = 1000.0;
        settings.crypto_contribution = 1000.0;
        settings.bonds_contribution = 1000.0;
        settings.other_contribution = 1000.0;

        let mut c = 0.0;
        let values = [
            project_savings(&current, &settings, 1.0, 12, &mut c),
            project_investments(&current, &settings, 1.0, 12, &mut c),
            project_crypto(&current, &settings, 1.0, 12, &mut c),
            project_bonds(&current, &settings, 1.0, 12, &mut c),
            project_other_assets(&current, &settings, 1.0, 12, &mut c),
        ];
        for v in values {
            assert!((v - 12_392.59).abs() < 0.01, "got {v}");
        }
    }

    // =====================================================================
    // PROJECT_SAVINGS TESTS
    // =====================================================================

    #[test]
    fn test_project_savings_growth_only() {
        let mut current = make_test_current();
        current.savings = 100_000.0;

        let mut settings = make_test_settings();
        settings.savings_growth = Some(5.0);
        settings.savings_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_savings(&current, &settings, 1.0, 12, &mut contributions);

        // 100,000 * 1.05 = 105,000
        assert!(
            (result - 105_000.0).abs() < 1.0,
            "Expected ~105,000, got {}",
            result
        );
        assert!(contributions == 0.0, "No contributions expected");
    }

    #[test]
    fn test_project_savings_with_contributions() {
        let mut current = make_test_current();
        current.savings = 100_000.0;

        let mut settings = make_test_settings();
        settings.savings_growth = Some(5.0);
        settings.savings_contribution = 1000.0;

        let mut contributions = 0.0;
        let result = project_savings(&current, &settings, 1.0, 12, &mut contributions);

        // Base growth: 100,000 * 1.05 = 105,000
        // Plus contributions with growth, result should be > 105,000 + 12,000 = 117,000
        assert!(result > 116_000.0, "Expected > 116,000, got {}", result);
        assert!(
            (contributions - 12_000.0).abs() < 0.01,
            "Contributions should be 12,000"
        );
    }

    #[test]
    fn test_project_savings_uses_weighted_interest() {
        let mut current = make_test_current();
        current.savings = 100_000.0;
        current.savings_weighted_interest = 4.0; // 4% weighted average

        let mut settings = make_test_settings();
        settings.savings_growth = None; // Should use current.savings_weighted_interest
        settings.savings_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_savings(&current, &settings, 1.0, 12, &mut contributions);

        // 100,000 * 1.04 = 104,000
        assert!(
            (result - 104_000.0).abs() < 1.0,
            "Expected ~104,000, got {}",
            result
        );
    }

    #[test]
    fn test_project_savings_explicit_zero_is_honoured() {
        // an explicit 0 % must not fall back to the weighted rate.
        let mut current = make_test_current();
        current.savings = 100_000.0;
        current.savings_weighted_interest = 3.0;

        let mut settings = make_test_settings();
        settings.savings_growth = Some(0.0);
        settings.savings_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_savings(&current, &settings, 1.0, 12, &mut contributions);

        assert!(
            (result - 100_000.0).abs() < 0.01,
            "explicit 0 % should keep 100,000, got {result}"
        );
    }

    #[test]
    fn test_project_bonds_explicit_zero_is_honoured() {
        let mut current = make_test_current();
        current.bonds = 100_000.0;
        current.bonds_weighted_yield = 5.0;

        let mut settings = make_test_settings();
        settings.bonds_growth = Some(0.0);
        settings.bonds_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_bonds(&current, &settings, 1.0, 12, &mut contributions);

        assert!(
            (result - 100_000.0).abs() < 0.01,
            "explicit 0 % should keep 100,000, got {result}"
        );
    }

    // =====================================================================
    // PROJECT_INVESTMENTS TESTS
    // =====================================================================

    #[test]
    fn test_project_investments_7_percent_10_years() {
        let mut current = make_test_current();
        current.investments = 100_000.0;

        let mut settings = make_test_settings();
        settings.investments_growth = 7.0;
        settings.investments_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_investments(&current, &settings, 10.0, 120, &mut contributions);

        // 100,000 * 1.07^10 = 196,715
        assert!(
            (result - 196_715.0).abs() < 10.0,
            "Expected ~196,715, got {}",
            result
        );
    }

    #[test]
    fn test_project_investments_with_dca() {
        let mut current = make_test_current();
        current.investments = 0.0; // Start from zero

        let mut settings = make_test_settings();
        settings.investments_growth = 7.0;
        settings.investments_contribution = 1000.0;

        let mut contributions = 0.0;
        let result = project_investments(&current, &settings, 10.0, 120, &mut contributions);

        // Should have significant growth from DCA
        assert!(
            result > 120_000.0,
            "DCA should grow beyond 120,000, got {}",
            result
        );
        assert!(
            (contributions - 120_000.0).abs() < 0.01,
            "Contributions should be 120,000"
        );
    }

    // =====================================================================
    // PROJECT_BONDS TESTS
    // =====================================================================

    #[test]
    fn test_project_bonds_uses_settings_override() {
        let mut current = make_test_current();
        current.bonds = 100_000.0;
        current.bonds_weighted_yield = 3.0; // This should be ignored

        let mut settings = make_test_settings();
        settings.bonds_growth = Some(6.0); // Override with 6%
        settings.bonds_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_bonds(&current, &settings, 1.0, 12, &mut contributions);

        // 100,000 * 1.06 = 106,000
        assert!(
            (result - 106_000.0).abs() < 1.0,
            "Expected ~106,000, got {}",
            result
        );
    }

    #[test]
    fn test_project_bonds_uses_weighted_yield_when_no_override() {
        let mut current = make_test_current();
        current.bonds = 100_000.0;
        current.bonds_weighted_yield = 4.0;

        let mut settings = make_test_settings();
        settings.bonds_growth = None; // No override
        settings.bonds_contribution = 0.0;

        let mut contributions = 0.0;
        let result = project_bonds(&current, &settings, 1.0, 12, &mut contributions);

        // 100,000 * 1.04 = 104,000
        assert!(
            (result - 104_000.0).abs() < 1.0,
            "Expected ~104,000, got {}",
            result
        );
    }

    // =====================================================================
    // PROJECT_REAL_ESTATE TESTS
    // =====================================================================

    #[test]
    fn test_project_real_estate_3_percent_appreciation() {
        let mut current = make_test_current();
        current.real_estate = 500_000.0;

        let mut settings = make_test_settings();
        settings.real_estate_growth = 3.0;

        let result = project_real_estate(&current, &settings, 10.0);

        // 500,000 * 1.03^10 = 671,958
        assert!(
            (result - 671_958.0).abs() < 10.0,
            "Expected ~671,958, got {}",
            result
        );
    }

    #[test]
    fn test_project_real_estate_zero_growth() {
        let mut current = make_test_current();
        current.real_estate = 500_000.0;

        let mut settings = make_test_settings();
        settings.real_estate_growth = 0.0;

        let result = project_real_estate(&current, &settings, 10.0);

        assert!(
            (result - 500_000.0).abs() < 0.01,
            "Expected 500,000, got {}",
            result
        );
    }

    // =====================================================================
    // PROJECT_OTHER_ASSETS TESTS
    // =====================================================================

    #[test]
    fn test_project_other_assets_with_contributions() {
        let mut current = make_test_current();
        current.other_assets = 10_000.0;

        let mut settings = make_test_settings();
        settings.other_growth = 0.0;
        settings.other_contribution = 500.0;

        let mut contributions = 0.0;
        let result = project_other_assets(&current, &settings, 1.0, 12, &mut contributions);

        // 10,000 + (500 * 12) = 16,000
        assert!(
            (result - 16_000.0).abs() < 0.01,
            "Expected 16,000, got {}",
            result
        );
        assert!(
            (contributions - 6_000.0).abs() < 0.01,
            "Contributions should be 6,000"
        );
    }

    // =====================================================================
    // PROJECT_LOANS (AMORTIZATION) TESTS
    // =====================================================================

    #[test]
    fn test_project_loans_basic_amortization() {
        let mut current = make_test_current();
        current.total_liabilities = 100_000.0;
        current.loan_monthly_payment = 2_000.0;
        current.loan_interest_rate = 0.05; // 5% annual

        // After 12 months, balance should be reduced
        let result = project_loans(&current, 12);

        // With 5% interest and 2,000/month payments, balance should decrease
        assert!(result < 100_000.0, "Balance should decrease");
        assert!(result > 70_000.0, "Balance shouldn't decrease too much");
    }

    #[test]
    fn test_project_loans_fully_paid() {
        let mut current = make_test_current();
        current.total_liabilities = 10_000.0;
        current.loan_monthly_payment = 5_000.0;
        current.loan_interest_rate = 0.0; // 0% interest

        // With 5,000/month and 0% interest, 10,000 should be paid off in 2 months
        let result = project_loans(&current, 12);

        assert!((result - 0.0).abs() < 0.01, "Loan should be fully paid");
    }

    #[test]
    fn test_project_loans_zero_payment() {
        let mut current = make_test_current();
        current.total_liabilities = 100_000.0;
        current.loan_monthly_payment = 0.0;
        current.loan_interest_rate = 0.05;

        // With no payments, balance stays the same (simplified model)
        let result = project_loans(&current, 12);

        assert!(
            (result - 100_000.0).abs() < 0.01,
            "Balance should stay at 100,000"
        );
    }

    // =====================================================================
    // INTEGRATION TESTS
    // =====================================================================

    #[test]
    fn test_total_assets_calculation() {
        let current = make_test_current();
        let settings = make_test_settings();
        let mut contributions = 0.0;

        let savings = project_savings(&current, &settings, 1.0, 12, &mut contributions);
        contributions = 0.0;
        let investments = project_investments(&current, &settings, 1.0, 12, &mut contributions);
        contributions = 0.0;
        let crypto = project_crypto(&current, &settings, 1.0, 12, &mut contributions);
        contributions = 0.0;
        let bonds = project_bonds(&current, &settings, 1.0, 12, &mut contributions);
        let real_estate = project_real_estate(&current, &settings, 1.0);
        contributions = 0.0;
        let other = project_other_assets(&current, &settings, 1.0, 12, &mut contributions);

        let total = savings + investments + crypto + bonds + real_estate + other;

        // Verify total is greater than initial (due to growth)
        assert!(
            total > current.total_assets,
            "Total after 1 year ({}) should exceed initial ({})",
            total,
            current.total_assets
        );
    }

    #[test]
    fn test_net_worth_calculation() {
        let current = make_test_current();
        let settings = make_test_settings();
        let mut contributions = 0.0;

        let savings = project_savings(&current, &settings, 1.0, 12, &mut contributions);
        let investments = project_investments(&current, &settings, 1.0, 12, &mut contributions);
        let crypto = project_crypto(&current, &settings, 1.0, 12, &mut contributions);
        let bonds = project_bonds(&current, &settings, 1.0, 12, &mut contributions);
        let real_estate = project_real_estate(&current, &settings, 1.0);
        let other = project_other_assets(&current, &settings, 1.0, 12, &mut contributions);
        let loans = project_loans(&current, 12);

        let total_assets = savings + investments + crypto + bonds + real_estate + other;
        let net_worth = total_assets - loans;

        // Net worth should increase (assets grow faster than loans decrease)
        assert!(
            net_worth > current.net_worth,
            "Net worth after 1 year ({}) should exceed initial ({})",
            net_worth,
            current.net_worth
        );
    }

    // =====================================================================
    // SETTINGS LOADING (Some(0.0) vs None)
    // =====================================================================

    fn settings_db(rows: &[(&str, &str, &str)]) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE projection_settings (
                id TEXT PRIMARY KEY,
                asset_type TEXT NOT NULL UNIQUE,
                yearly_growth_rate TEXT NOT NULL DEFAULT '0',
                monthly_contribution TEXT NOT NULL DEFAULT '0',
                contribution_currency TEXT NOT NULL DEFAULT 'CZK',
                enabled INTEGER NOT NULL DEFAULT 1
            );",
        )
        .expect("schema");
        for (asset, rate, contribution) in rows {
            conn.execute(
                "INSERT INTO projection_settings (id, asset_type, yearly_growth_rate, monthly_contribution)
                 VALUES (?1, ?1, ?2, ?3)",
                rusqlite::params![asset, rate, contribution],
            )
            .expect("insert");
        }
        conn
    }

    /// Quotes with EUR as reference: CZK 25, USD 1.1 per EUR.
    fn eur_quotes() -> Quotes {
        let mut quotes = Quotes::new("EUR");
        quotes.insert("CZK", 25.0);
        quotes.insert("USD", 1.1);
        quotes
    }

    /// The settings as read in CZK (the stored rows here are CZK, so nothing converts).
    fn settings_for(conn: &Connection, rate_shift: f64) -> SettingsMap {
        get_settings_map(
            conn,
            rate_shift,
            &eur_quotes(),
            "CZK",
            &mut Quality::default(),
        )
        .expect("settings")
    }

    /// Rows `(asset_type, monthly_contribution, contribution_currency, enabled)`.
    fn contribution_db(rows: &[(&str, &str, &str, i32)]) -> Connection {
        let conn = settings_db(&[]);
        for (asset, contribution, currency, enabled) in rows {
            conn.execute(
                "INSERT INTO projection_settings
                    (id, asset_type, monthly_contribution, contribution_currency, enabled)
                 VALUES (?1, ?1, ?2, ?3, ?4)",
                rusqlite::params![asset, contribution, currency, enabled],
            )
            .expect("insert");
        }
        conn
    }

    fn savings_contribution(
        conn: &Connection,
        report_currency: &str,
        quality: &mut Quality,
    ) -> f64 {
        get_settings_map(conn, 0.0, &eur_quotes(), report_currency, quality)
            .expect("settings")
            .savings_contribution
    }

    #[test]
    fn a_contribution_in_the_report_currency_ignores_the_rate() {
        let conn = contribution_db(&[("savings", "500", "EUR", 1)]);
        let mut quality = Quality::default();
        assert_eq!(savings_contribution(&conn, "EUR", &mut quality), 500.0);
        assert!(quality.is_complete());
    }

    #[test]
    fn a_contribution_is_converted_into_another_report_currency() {
        let conn = contribution_db(&[("savings", "500", "EUR", 1)]);
        let mut quality = Quality::default();
        let czk = savings_contribution(&conn, "CZK", &mut quality);
        assert!((czk - 12_500.0).abs() < 1e-6, "{czk}");
        assert!(quality.is_complete());
    }

    #[test]
    fn a_legacy_czk_contribution_is_converted_into_the_report_currency() {
        let conn = contribution_db(&[("savings", "12500", "CZK", 1)]);
        let mut quality = Quality::default();
        let eur = savings_contribution(&conn, "EUR", &mut quality);
        assert!((eur - 500.0).abs() < 1e-6, "{eur}");
        assert!(quality.is_complete());
    }

    #[test]
    fn an_unconvertible_contribution_is_excluded_and_recorded() {
        let conn = contribution_db(&[("savings", "100", "KWD", 1)]);
        let mut quality = Quality::default();
        assert_eq!(savings_contribution(&conn, "EUR", &mut quality), 0.0);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
        assert_eq!(quality.missing_count(), 1);
    }

    #[test]
    fn a_frozen_or_zero_contribution_needs_no_quote() {
        let conn = contribution_db(&[("savings", "100", "KWD", 0), ("bonds", "0", "KWD", 1)]);
        let mut quality = Quality::default();
        assert_eq!(savings_contribution(&conn, "EUR", &mut quality), 0.0);
        assert!(quality.is_complete());
    }

    #[test]
    fn test_settings_without_rows_use_the_class_defaults() {
        let conn = settings_db(&[]);
        let settings = settings_for(&conn, 0.0);
        assert_eq!(settings.savings_growth, None);
        assert_eq!(settings.bonds_growth, None);
        assert_eq!(settings.investments_growth, 7.0);
        assert_eq!(settings.crypto_growth, 7.0);
        assert_eq!(settings.real_estate_growth, 3.0);
        assert_eq!(settings.other_growth, 0.0);
    }

    #[test]
    fn test_stored_zero_is_an_explicit_zero_for_savings_and_bonds() {
        let conn = settings_db(&[("savings", "0", "500"), ("bonds", "0.00", "0")]);
        let settings = settings_for(&conn, 0.0);
        assert_eq!(settings.savings_growth, Some(0.0));
        assert_eq!(settings.bonds_growth, Some(0.0));
        assert_eq!(settings.savings_contribution, 500.0);
    }

    #[test]
    fn test_stored_empty_rate_means_use_the_calculated_default() {
        let conn = settings_db(&[("savings", "", "0"), ("bonds", "  ", "0")]);
        let settings = settings_for(&conn, 0.0);
        assert_eq!(settings.savings_growth, None);
        assert_eq!(settings.bonds_growth, None);
    }

    #[test]
    fn test_stored_rates_are_read_for_every_class() {
        let conn = settings_db(&[
            ("savings", "2.5", "100"),
            ("investments", "0", "2000"),
            ("crypto", "10", "0"),
            ("bonds", "4", "0"),
            ("real_estate", "0", "0"),
            ("other_assets", "1.5", "50"),
        ]);
        let settings = settings_for(&conn, 0.0);
        assert_eq!(settings.savings_growth, Some(2.5));
        assert_eq!(settings.investments_growth, 0.0, "explicit 0 % for stocks");
        assert_eq!(settings.crypto_growth, 10.0);
        assert_eq!(settings.bonds_growth, Some(4.0));
        assert_eq!(
            settings.real_estate_growth, 0.0,
            "explicit 0 % for real estate"
        );
        assert_eq!(settings.other_growth, 1.5);
        assert_eq!(settings.other_contribution, 50.0);
    }

    #[test]
    fn test_disabled_rows_freeze_the_class() {
        // A disabled row no longer falls back to the default: the class is frozen
        // at today's value (design system §7 Outlook, "mimo projekci").
        let conn = settings_db(&[("savings", "0", "0")]);
        conn.execute("UPDATE projection_settings SET enabled = 0", [])
            .expect("disable");
        let settings = settings_for(&conn, 0.0);
        assert!(settings.savings_frozen);
        let current = make_test_current();
        let mut c = 0.0;
        assert_eq!(
            project_savings(&current, &settings, 5.0, 60, &mut c),
            current.savings
        );
    }

    // =====================================================================
    // CURRENT VALUES: the projection starts from the net-worth numbers
    // =====================================================================

    fn portfolio_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE bank_accounts (balance TEXT, currency TEXT, interest_rate TEXT, exclude_from_balance INTEGER);
             CREATE TABLE bonds (coupon_value TEXT, quantity TEXT, currency TEXT, interest_rate TEXT);
             CREATE TABLE loans (id TEXT, currency TEXT, principal TEXT, interest_rate TEXT, monthly_payment TEXT,
                                 start_date INTEGER, end_date INTEGER,
                                 balance_anchor_amount TEXT, balance_anchor_date INTEGER);
             CREATE TABLE loan_events(id TEXT PRIMARY KEY,loan_id TEXT,kind TEXT,event_day INTEGER,amount TEXT,rate TEXT,monthly_payment TEXT,previous_rate TEXT,previous_monthly_payment TEXT,note TEXT,created_at INTEGER);
             CREATE TABLE real_estate (type TEXT, market_price TEXT, market_price_currency TEXT);
             CREATE TABLE other_assets (id TEXT, quantity TEXT, market_price TEXT, currency TEXT);
             CREATE TABLE other_asset_valuations (id TEXT, asset_id TEXT, value TEXT, currency TEXT,
                                                  valued_at INTEGER, created_at INTEGER);
             CREATE TABLE stock_investments (id TEXT, ticker TEXT, quantity TEXT, currency TEXT);
             CREATE TABLE investment_transactions (investment_id TEXT, type TEXT, quantity TEXT, price_per_unit TEXT,
                                                   currency TEXT, transaction_date INTEGER, created_at INTEGER);
             CREATE TABLE stock_data (ticker TEXT PRIMARY KEY, original_price TEXT, currency TEXT, fetched_at INTEGER);
             CREATE TABLE stock_price_overrides (ticker TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
             CREATE TABLE crypto_investments (id TEXT, ticker TEXT, quantity TEXT, currency TEXT);
             CREATE TABLE crypto_transactions (investment_id TEXT, type TEXT, quantity TEXT, price_per_unit TEXT,
                                               currency TEXT, transaction_date INTEGER, created_at INTEGER);
             CREATE TABLE crypto_prices (symbol TEXT PRIMARY KEY, price TEXT, currency TEXT, fetched_at INTEGER);
             CREATE TABLE crypto_price_overrides (symbol TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
             CREATE TABLE projection_settings (
                id TEXT PRIMARY KEY,
                asset_type TEXT NOT NULL UNIQUE,
                yearly_growth_rate TEXT NOT NULL DEFAULT '0',
                monthly_contribution TEXT NOT NULL DEFAULT '0',
                contribution_currency TEXT NOT NULL DEFAULT 'CZK',
                enabled INTEGER NOT NULL DEFAULT 1
             );",
        )
        .expect("schema");
        conn
    }

    fn values_in(conn: &Connection, currency: &str) -> (CurrentValues, Quality) {
        let mut quality = Quality::default();
        let values = current_values(conn, &eur_quotes(), currency, false, &mut quality)
            .expect("current values");
        (values, quality)
    }

    #[test]
    fn test_investments_use_the_api_price_when_it_is_newer_than_the_override() {
        // Override 150 entered Monday, API refresh 180 on Tuesday: the dashboard
        // says 180, the projection used to start from 150.
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO stock_investments VALUES ('AAPL', 'AAPL', '10', 'USD');
             INSERT INTO stock_data VALUES ('AAPL', '180', 'EUR', 2000);
             INSERT INTO stock_price_overrides VALUES ('AAPL', '150', 'EUR', 1000);",
        )
        .unwrap();
        let (values, quality) = values_in(&conn, "EUR");
        assert!(
            (values.investments - 1800.0).abs() < 0.01,
            "got {}",
            values.investments
        );
        assert!(quality.is_complete());
    }

    #[test]
    fn test_investments_use_the_override_when_it_is_newer_than_the_api() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO stock_investments VALUES ('AAPL', 'AAPL', '10', 'USD');
             INSERT INTO stock_data VALUES ('AAPL', '180', 'EUR', 1000);
             INSERT INTO stock_price_overrides VALUES ('AAPL', '150', 'EUR', 2000);",
        )
        .unwrap();
        let (values, _) = values_in(&conn, "EUR");
        assert!(
            (values.investments - 1500.0).abs() < 0.01,
            "got {}",
            values.investments
        );
    }

    #[test]
    fn test_crypto_honours_a_newer_override() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO crypto_investments VALUES ('BTC', 'BTC', '2', 'USD');
             INSERT INTO crypto_prices VALUES ('BTC', '60000', 'EUR', 1000);
             INSERT INTO crypto_price_overrides VALUES ('BTC', '50000', 'EUR', 2000);",
        )
        .unwrap();
        let (values, _) = values_in(&conn, "EUR");
        assert!(
            (values.crypto - 100_000.0).abs() < 0.01,
            "got {}",
            values.crypto
        );
    }

    #[test]
    fn test_crypto_uses_the_api_price_when_it_is_newer_than_the_override() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO crypto_investments VALUES ('BTC', 'BTC', '2', 'USD');
             INSERT INTO crypto_prices VALUES ('BTC', '60000', 'EUR', 2000);
             INSERT INTO crypto_price_overrides VALUES ('BTC', '50000', 'EUR', 1000);",
        )
        .unwrap();
        let (values, _) = values_in(&conn, "EUR");
        assert!(
            (values.crypto - 120_000.0).abs() < 0.01,
            "got {}",
            values.crypto
        );
    }

    #[test]
    fn test_positions_without_any_price_are_not_valued() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO stock_investments VALUES ('XYZ', 'XYZ', '10', 'USD');
             INSERT INTO crypto_investments VALUES ('ETH', 'ETH', '1', 'USD');",
        )
        .unwrap();
        let (values, _) = values_in(&conn, "EUR");
        assert_eq!(values.investments, 0.0);
        assert_eq!(values.crypto, 0.0);
    }

    #[test]
    fn holdings_are_valued_in_the_report_currency_at_one_snapshot() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts VALUES ('1000', 'EUR', '2', 0);
             INSERT INTO bank_accounts VALUES ('25000', 'CZK', '0', 0);
             INSERT INTO bank_accounts VALUES ('50', 'KWD', '9', 0);
             INSERT INTO real_estate VALUES ('personal', '100000', 'CZK');",
        )
        .unwrap();
        let (values, quality) = values_in(&conn, "CZK");
        // 1000 EUR = 25000 CZK, plus 25000 CZK; the KWD account is left out
        assert!(
            (values.savings - 50_000.0).abs() < 1e-6,
            "{}",
            values.savings
        );
        assert!((values.real_estate - 100_000.0).abs() < 1e-6);
        assert_eq!(quality.missing_currencies(), vec!["KWD".to_string()]);
        // Counted once although the account also takes part in the rate weights
        assert_eq!(quality.missing_count(), 1);
        // 2 % on 25000 CZK and 0 % on the rest: the weighted rate ignores the 0 % account
        assert!((values.savings_weighted_interest - 2.0).abs() < 1e-9);
    }

    #[test]
    fn excluding_personal_real_estate_leaves_it_out_of_the_start() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO real_estate VALUES ('personal', '100000', 'EUR');
             INSERT INTO real_estate VALUES ('investment', '50000', 'EUR');",
        )
        .unwrap();
        let mut quality = Quality::default();
        let values =
            current_values(&conn, &eur_quotes(), "EUR", true, &mut quality).expect("values");
        assert_eq!(values.real_estate, 50_000.0);
        assert_eq!(values.total_assets, 50_000.0);
    }

    #[test]
    fn the_projection_is_text_in_the_report_currency() {
        let conn = portfolio_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts VALUES ('1000', 'EUR', '0', 0);
             INSERT INTO projection_settings (id, asset_type, yearly_growth_rate, monthly_contribution, contribution_currency)
             VALUES ('a', 'savings', '0', '10', 'EUR');
             INSERT INTO projection_settings (id, asset_type, yearly_growth_rate, monthly_contribution, contribution_currency)
             VALUES ('b', 'other_assets', '0', '250', 'CZK');",
        )
        .unwrap();
        let input = ProjectionInput {
            horizon_years: 1,
            view_type: "yearly".to_string(),
            exclude_personal_real_estate: false,
            rate_shift: 0.0,
        };
        let now = Utc::now();

        let in_czk = calculate_projection_at(&conn, &eur_quotes(), "czk", &input, now)
            .expect("projection in CZK");
        assert_eq!(in_czk.currency, "CZK");
        assert_eq!(in_czk.timeline.len(), 2);
        assert_eq!(in_czk.timeline[0].savings, "25000.00");
        // 12 x (10 EUR = 250 CZK) + 12 x 250 CZK
        assert_eq!(in_czk.total_contributions, "6000.00");
        assert_eq!(in_czk.projected_net_worth, "31000.00");
        assert_eq!(in_czk.total_growth, "0.00");
        assert!(in_czk.fx_quality.missing_currencies.is_empty());

        let in_eur = calculate_projection_at(&conn, &eur_quotes(), "EUR", &input, now)
            .expect("projection in EUR");
        assert_eq!(in_eur.timeline[0].savings, "1000.00");
        assert_eq!(in_eur.total_contributions, "240.00");
        assert_eq!(in_eur.projected_net_worth, "1240.00");
    }

    #[test]
    fn an_unquoted_report_currency_is_reported_not_faked() {
        let conn = portfolio_db();
        conn.execute_batch("INSERT INTO bank_accounts VALUES ('1000', 'EUR', '0', 0);")
            .unwrap();
        let input = ProjectionInput {
            horizon_years: 1,
            view_type: "yearly".to_string(),
            exclude_personal_real_estate: false,
            rate_shift: 0.0,
        };
        let projection =
            calculate_projection_at(&conn, &eur_quotes(), "KWD", &input, Utc::now()).expect("ok");
        assert_eq!(projection.currency, "KWD");
        assert_eq!(projection.timeline[0].savings, "0.000");
        assert_eq!(
            projection.fx_quality.missing_currencies,
            vec!["KWD".to_string()]
        );
    }

    // =====================================================================
    // SAVING SETTINGS
    // =====================================================================

    fn setting(asset: &str, amount: &str, currency: &str) -> ProjectionSettings {
        ProjectionSettings {
            id: String::new(),
            asset_type: asset.into(),
            yearly_growth_rate: String::new(),
            monthly_contribution: amount.into(),
            contribution_currency: currency.into(),
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn save_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE projection_settings (
                id TEXT PRIMARY KEY,
                asset_type TEXT NOT NULL UNIQUE,
                yearly_growth_rate TEXT NOT NULL DEFAULT '0',
                monthly_contribution TEXT NOT NULL DEFAULT '0',
                contribution_currency TEXT NOT NULL DEFAULT 'CZK',
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );",
        )
        .expect("schema");
        conn
    }

    #[test]
    fn saving_normalises_the_currency_and_upserts_by_asset_type() {
        let conn = save_db();
        save_settings(&conn, vec![setting("savings", "100", " eur ")]).expect("save");
        save_settings(&conn, vec![setting("savings", "250.5", "USD")]).expect("upsert");
        let rows = list_settings(&conn).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].monthly_contribution, "250.5");
        assert_eq!(rows[0].contribution_currency, "USD");
        save_settings(&conn, vec![setting("bonds", "1", "eur")]).expect("save");
        let bonds = list_settings(&conn)
            .expect("list")
            .into_iter()
            .find(|s| s.asset_type == "bonds")
            .expect("bonds");
        assert_eq!(bonds.contribution_currency, "EUR");
    }

    #[test]
    fn an_invalid_entry_rejects_the_whole_batch_before_any_write() {
        let conn = save_db();
        for bad in [
            setting("bonds", "10", "EURO"),
            setting("bonds", "10", ""),
            setting("bonds", "10", "E1R"),
            setting("bonds", "abc", "EUR"),
            setting("bonds", "", "EUR"),
            setting("bonds", "NaN", "EUR"),
            setting("bonds", "-5", "EUR"),
        ] {
            let result = save_settings(&conn, vec![setting("savings", "1", "EUR"), bad]);
            assert!(matches!(result, Err(crate::error::AppError::Validation(_))));
        }
        assert!(list_settings(&conn).expect("list").is_empty());
    }
}
