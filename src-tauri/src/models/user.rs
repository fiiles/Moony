//! User profile and app configuration models

use crate::error::{AppError, Result};
use crate::services::fx;
use serde::{Deserialize, Serialize};
use specta::Type;

fn default_true() -> bool {
    true
}

/// Menu preferences for sidebar visibility
#[derive(Debug, Clone, Serialize, Deserialize, Default, Type)]
pub struct MenuPreferences {
    pub loans: bool,
    pub insurance: bool,
    pub investments: bool,
    pub bonds: bool,
    #[serde(rename = "realEstate")]
    pub real_estate: bool,
    #[serde(default = "default_true")]
    pub crypto: bool,
    #[serde(rename = "otherAssets", default = "default_true")]
    pub other_assets: bool,
}

impl MenuPreferences {
    pub fn all_enabled() -> Self {
        Self {
            loans: true,
            insurance: true,
            investments: true,
            bonds: true,
            real_estate: true,
            crypto: true,
            other_assets: true,
        }
    }
}

/// User profile stored in database
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UserProfile {
    pub id: i64,
    pub name: String,
    pub surname: String,
    pub email: String,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: MenuPreferences,
    pub currency: String,
    pub language: String,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: bool,
    #[serde(rename = "coingeckoModalDismissed", default)]
    pub coingecko_modal_dismissed: bool,
    #[serde(rename = "mcpServerEnabled", default)]
    pub mcp_server_enabled: bool,
    /// ISO 3166-1 alpha-2 country of the profile (`CZ`), None until set.
    pub country: Option<String>,
    /// The cancellation notice of insurance policies in days that the milestones use: the stored
    /// value, else 43 for Czechia and Slovakia, else None (no anniversary reminder).
    #[serde(rename = "insuranceNoticeDays")]
    pub insurance_notice_days: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating/updating user profile
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertUserProfile {
    pub name: String,
    pub surname: String,
    pub email: String,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: Option<MenuPreferences>,
    pub currency: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: Option<bool>,
}

/// Default main currency when a new profile does not specify one.
pub const DEFAULT_MAIN_CURRENCY: &str = "CZK";

/// Validate a main (profile) currency and return its normalised code.
///
/// The main currency must have an exchange-rate source (quoted or pegged),
/// otherwise mixed-currency totals could not be converted into it.
pub fn validate_main_currency(code: &str) -> Result<String> {
    match fx::normalize(code) {
        Some(normalized) if fx::is_supported(&normalized) => Ok(normalized),
        _ => Err(AppError::Validation(
            "validation.currencyUnsupported".into(),
        )),
    }
}

/// Normalise a profile country: an empty string clears it, otherwise two ASCII letters in upper case.
pub fn normalize_country(code: &str) -> Result<Option<String>> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Ok(Some(trimmed.to_ascii_uppercase()));
    }
    Err(AppError::Validation("validation.countryInvalid".into()))
}

impl InsertUserProfile {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        if let Some(currency) = &self.currency {
            validate_main_currency(currency)?;
        }
        Ok(())
    }
}

impl UpdateUserProfile {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        if let Some(currency) = &self.currency {
            validate_main_currency(currency)?;
        }
        if let Some(country) = &self.country {
            normalize_country(country)?;
        }
        if let Some(days) = self.insurance_notice_days {
            if !(0..=365).contains(&days) {
                return Err(AppError::Validation(
                    "validation.insuranceNoticeInvalid".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Data for updating user profile (partial updates)
#[derive(Debug, Clone, Deserialize, Type)]
pub struct UpdateUserProfile {
    pub name: Option<String>,
    pub surname: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: Option<MenuPreferences>,
    pub currency: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: Option<bool>,
    #[serde(rename = "coingeckoModalDismissed")]
    pub coingecko_modal_dismissed: Option<bool>,
    #[serde(rename = "mcpServerEnabled")]
    pub mcp_server_enabled: Option<bool>,
    /// `Some("")` clears the country; otherwise two ASCII letters (stored upper case).
    pub country: Option<String>,
    /// `0` turns the anniversary reminder off; 1..=365 sets the notice period.
    #[serde(rename = "insuranceNoticeDays")]
    pub insurance_notice_days: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_preferences_ignore_the_retired_savings_flag() {
        let stored = r#"{"savings":true,"loans":true,"insurance":false,"investments":true,
            "bonds":true,"realEstate":true,"crypto":false,"otherAssets":true}"#;
        let prefs: MenuPreferences = serde_json::from_str(stored).expect("legacy JSON parses");
        assert!(prefs.loans);
        assert!(!prefs.insurance);
        assert!(!prefs.crypto);

        let written = serde_json::to_string(&prefs).expect("serializes");
        assert!(!written.contains("savings"));
    }

    #[test]
    fn menu_preferences_default_new_classes_to_visible() {
        let stored =
            r#"{"loans":true,"insurance":true,"investments":true,"bonds":true,"realEstate":true}"#;
        let prefs: MenuPreferences = serde_json::from_str(stored).expect("parses");
        assert!(prefs.crypto);
        assert!(prefs.other_assets);
    }

    #[test]
    fn a_country_is_two_letters_in_upper_case_and_an_empty_one_clears_it() {
        assert_eq!(normalize_country(" gb ").unwrap(), Some("GB".to_string()));
        assert_eq!(normalize_country("").unwrap(), None);
        assert_eq!(normalize_country("   ").unwrap(), None);
        for bad in ["G", "GBR", "G1", "České"] {
            match normalize_country(bad) {
                Err(AppError::Validation(key)) => assert_eq!(key, "validation.countryInvalid"),
                other => panic!("{bad}: {other:?}"),
            }
        }
    }

    #[test]
    fn the_insurance_notice_is_0_to_365_days() {
        let update = |days: i64| -> UpdateUserProfile {
            serde_json::from_value(serde_json::json!({ "insuranceNoticeDays": days }))
                .expect("update")
        };
        for ok in [0, 1, 43, 365] {
            assert!(update(ok).validate().is_ok(), "{ok}");
        }
        for bad in [-1, 366, 10_000] {
            assert!(
                matches!(
                    update(bad).validate(),
                    Err(AppError::Validation(key)) if key == "validation.insuranceNoticeInvalid"
                ),
                "{bad}"
            );
        }
    }
}
