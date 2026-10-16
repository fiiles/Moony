//! Biometric unlock on the phone (spec §8.1): what the lock screen needs to know before the
//! database is open.

use serde::Serialize;
use specta::Type;

/// Which biometry the phone offers, for the unlock button's icon and wording. Mirrors the
/// keychain plugin's `BiometryType` (the app depends on the plugin on phones only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
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

/// The `biometric_unlock_status` answer, readable while the database is still locked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BiometricStatus {
    /// Biometry-protected storage can be used right now (biometry enrolled, passcode set).
    #[serde(rename = "available")]
    pub available: bool,
    /// The person switched biometric unlock on for this profile on this phone.
    #[serde(rename = "enrolled")]
    pub enrolled: bool,
    #[serde(rename = "biometryType")]
    pub biometry_type: BiometryType,
    /// Why biometrics are not available, as an i18n key (`mobile:biometrics.unavailable`);
    /// `None` while they are.
    #[serde(rename = "reason")]
    pub reason: Option<String>,
}
