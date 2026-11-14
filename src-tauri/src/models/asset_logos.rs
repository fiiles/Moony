//! A cached stock logo (`services::asset_logos`), sent to the webview as a data URL so the
//! webview never fetches an image from the network (spec 2026-11-14-asset-logos).

use serde::{Deserialize, Serialize};
use specta::Type;

/// One stored logo: the upper-case ticker and the normalized PNG as
/// `data:image/png;base64,…`. Tickers without a logo are not listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct AssetLogo {
    pub ticker: String,
    #[serde(rename = "dataUrl")]
    pub data_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `shared/schema.ts` mirrors these exact wire names.
    #[test]
    fn serializes_with_camel_case_field_names() {
        let logo = AssetLogo {
            ticker: "AAPL".to_string(),
            data_url: "data:image/png;base64,AA==".to_string(),
        };
        let json = serde_json::to_value(&logo).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "ticker": "AAPL", "dataUrl": "data:image/png;base64,AA==" })
        );
    }
}
