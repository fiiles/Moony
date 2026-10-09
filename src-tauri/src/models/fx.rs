//! Wire types for conversion quality and the FX quote snapshot.

use crate::services::fx::Quality;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

/// What a converted response could not (or only approximately) convert.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq)]
pub struct ConversionQuality {
    #[serde(rename = "missingCurrencies")]
    pub missing_currencies: Vec<String>,
    #[serde(rename = "missingCount")]
    pub missing_count: i32,
    #[serde(rename = "estimated")]
    pub estimated: bool,
}

impl From<&Quality> for ConversionQuality {
    fn from(quality: &Quality) -> Self {
        Self {
            missing_currencies: quality.missing_currencies(),
            missing_count: i32::try_from(quality.missing_count()).unwrap_or(i32::MAX),
            estimated: quality.is_estimated(),
        }
    }
}

/// The current quotes: units of each currency per 1 unit of `reference`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxSnapshot {
    #[serde(rename = "reference")]
    pub reference: String,
    /// Includes derived codes and the reference itself (1.0).
    #[serde(rename = "units")]
    pub units: HashMap<String, f64>,
    /// Unix seconds of the last fetch; 0 when no rates are loaded.
    #[serde(rename = "revision")]
    pub revision: i64,
}

/// Quotes stored for one UTC day (`day` is the UTC-midnight unix timestamp).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FxDayQuotes {
    #[serde(rename = "day")]
    pub day: i64,
    #[serde(rename = "units")]
    pub units: HashMap<String, f64>,
}
