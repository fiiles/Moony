//! Payment frequency shared by insurance policies, cashflow and MCP.
//!
//! The database stores the frequency as TEXT. Historically three spellings
//! coexisted (`yearly`/`annually`, `one_time`/`one-time`, `semi_annually`/
//! `semi-annually`) and each consumer matched its own subset, so a policy
//! paid "yearly" was dropped from the annual premium while a one-time policy
//! was multiplied ×12 in cashflow. Migration 010
//! normalizes stored values to the canonical spellings below; `parse` stays
//! lenient for legacy JSON blobs (real-estate recurring costs use `yearly`).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentFrequency {
    Monthly,
    Quarterly,
    SemiAnnually,
    Annually,
    OneTime,
}

impl PaymentFrequency {
    /// Canonical wire/DB values, in UI order.
    pub const ALL: [PaymentFrequency; 5] = [
        PaymentFrequency::Monthly,
        PaymentFrequency::Quarterly,
        PaymentFrequency::SemiAnnually,
        PaymentFrequency::Annually,
        PaymentFrequency::OneTime,
    ];

    /// Canonical string stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentFrequency::Monthly => "monthly",
            PaymentFrequency::Quarterly => "quarterly",
            PaymentFrequency::SemiAnnually => "semi_annually",
            PaymentFrequency::Annually => "annually",
            PaymentFrequency::OneTime => "one_time",
        }
    }

    /// Lenient parse: canonical values plus every legacy spelling that has
    /// ever been written by the UI, MCP or CSV paths. Case-insensitive.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "monthly" => Some(PaymentFrequency::Monthly),
            "quarterly" => Some(PaymentFrequency::Quarterly),
            "semi_annually" | "semi-annually" | "semiannually" | "semi_annual" | "half_yearly"
            | "half-yearly" => Some(PaymentFrequency::SemiAnnually),
            "annually" | "annual" | "yearly" => Some(PaymentFrequency::Annually),
            "one_time" | "one-time" | "onetime" | "once" | "single" => {
                Some(PaymentFrequency::OneTime)
            }
            _ => None,
        }
    }

    /// Strict parse: only canonical values (what validation accepts).
    pub fn parse_canonical(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.as_str() == value)
    }

    /// Number of payments per year; 0 for one-time payments.
    pub fn payments_per_year(self) -> f64 {
        match self {
            PaymentFrequency::Monthly => 12.0,
            PaymentFrequency::Quarterly => 4.0,
            PaymentFrequency::SemiAnnually => 2.0,
            PaymentFrequency::Annually => 1.0,
            PaymentFrequency::OneTime => 0.0,
        }
    }
}

impl fmt::Display for PaymentFrequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Yearly total of a recurring `amount` paid with `frequency`.
/// Unknown frequencies contribute 0 and are logged — never silently ×12.
pub fn annualize(amount: f64, frequency: &str) -> f64 {
    match PaymentFrequency::parse(frequency) {
        Some(f) => amount * f.payments_per_year(),
        None => {
            log::warn!(
                "[FREQUENCY] Unknown payment frequency {frequency:?}; counting it as 0 per year"
            );
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_values_round_trip() {
        for f in PaymentFrequency::ALL {
            assert_eq!(PaymentFrequency::parse(f.as_str()), Some(f));
            assert_eq!(PaymentFrequency::parse_canonical(f.as_str()), Some(f));
        }
    }

    #[test]
    fn legacy_spellings_parse_but_are_not_canonical() {
        assert_eq!(
            PaymentFrequency::parse("yearly"),
            Some(PaymentFrequency::Annually)
        );
        assert_eq!(
            PaymentFrequency::parse("Annual"),
            Some(PaymentFrequency::Annually)
        );
        assert_eq!(
            PaymentFrequency::parse("one-time"),
            Some(PaymentFrequency::OneTime)
        );
        assert_eq!(
            PaymentFrequency::parse("semi-annually"),
            Some(PaymentFrequency::SemiAnnually)
        );
        assert_eq!(PaymentFrequency::parse_canonical("yearly"), None);
        assert_eq!(PaymentFrequency::parse("weekly"), None);
    }

    #[test]
    fn annualize_matches_each_frequency() {
        // a 'yearly' premium must count once, not be dropped.
        assert_eq!(annualize(3_219.0, "yearly"), 3_219.0);
        assert_eq!(annualize(100.0, "monthly"), 1_200.0);
        assert_eq!(annualize(100.0, "quarterly"), 400.0);
        // underscore spellings from the model must not fall to ×12.
        assert_eq!(annualize(10_000.0, "semi_annually"), 20_000.0);
        assert_eq!(annualize(50_000.0, "one_time"), 0.0);
        assert_eq!(annualize(50_000.0, "weekly"), 0.0);
    }
}
