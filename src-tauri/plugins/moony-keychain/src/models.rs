use serde::{Deserialize, Serialize};

/// Which biometry the device offers, for the unlock button's icon and wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiometryType {
    #[serde(rename = "faceId")]
    FaceId,
    /// Touch ID on iOS; a fingerprint-only device on Android.
    #[serde(rename = "touchId")]
    TouchId,
    /// Any other biometry (Optic ID, or an Android device with several sensors).
    #[serde(rename = "biometric")]
    Biometric,
    #[serde(rename = "none")]
    None,
}

/// The `status` answer: whether a biometry-protected secret can be stored and read right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    #[serde(rename = "available")]
    pub available: bool,
    #[serde(rename = "biometryType")]
    pub biometry_type: BiometryType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parses_the_native_answer() {
        let status: Status =
            serde_json::from_str(r#"{"available":true,"biometryType":"faceId"}"#).expect("json");
        assert_eq!(
            status,
            Status {
                available: true,
                biometry_type: BiometryType::FaceId
            }
        );
    }

    #[test]
    fn every_biometry_type_round_trips_by_its_wire_name() {
        for (wire, kind) in [
            ("faceId", BiometryType::FaceId),
            ("touchId", BiometryType::TouchId),
            ("biometric", BiometryType::Biometric),
            ("none", BiometryType::None),
        ] {
            let json = format!("\"{wire}\"");
            assert_eq!(
                serde_json::from_str::<BiometryType>(&json).expect("json"),
                kind
            );
            assert_eq!(serde_json::to_string(&kind).expect("json"), json);
        }
    }
}
