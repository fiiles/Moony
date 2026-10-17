/// Result alias for the keychain API.
pub type Result<T> = std::result::Result<T, KeychainError>;

/// What can go wrong with a keychain call. The native sides reject with a `code` that
/// [`KeychainError::from_rejection`] turns into one of these; the app maps them to its own
/// `AppError` (`commands/biometric.rs`), since this crate must not depend on the app.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeychainError {
    /// The biometric enrolment (or the device passcode) changed since the secret was stored,
    /// so the secret can never be read again. Remove it and fall back to the password.
    #[error("the biometric enrolment changed, so the stored secret can no longer be read")]
    Invalidated,
    /// The person (or the system) dismissed the biometric prompt.
    #[error("biometric authentication was cancelled")]
    Cancelled,
    /// No usable biometry right now: none enrolled, no passcode, locked out, or no hardware.
    #[error("biometric authentication is unavailable on this device")]
    Unavailable,
    /// Anything else, with the native side's message.
    #[error("keychain operation failed: {0}")]
    Failed(String),
    /// The keychain exists on iOS and Android only.
    #[error("the keychain is not supported on this platform")]
    Unsupported,
}

impl KeychainError {
    /// Wire codes the Swift and Kotlin sides reject with.
    pub const CODE_INVALIDATED: &'static str = "invalidated";
    pub const CODE_CANCELLED: &'static str = "cancelled";
    pub const CODE_UNAVAILABLE: &'static str = "unavailable";
    pub const CODE_FAILED: &'static str = "failed";

    /// Parses a rejection `code` alone (`KeychainError::from_code("invalidated")`).
    pub fn from_code(code: &str) -> Self {
        Self::from_rejection(Some(code), None)
    }

    /// Parses a native rejection `{ code, message }`. Unknown or missing codes become
    /// [`KeychainError::Failed`] carrying the message (or the code when there is no message).
    pub fn from_rejection(code: Option<&str>, message: Option<&str>) -> Self {
        match code {
            Some(Self::CODE_INVALIDATED) => Self::Invalidated,
            Some(Self::CODE_CANCELLED) => Self::Cancelled,
            Some(Self::CODE_UNAVAILABLE) => Self::Unavailable,
            _ => Self::Failed(
                message
                    .filter(|m| !m.is_empty())
                    .or(code)
                    .unwrap_or("unknown keychain error")
                    .to_string(),
            ),
        }
    }
}

#[cfg(mobile)]
impl From<tauri::plugin::mobile::PluginInvokeError> for KeychainError {
    fn from(error: tauri::plugin::mobile::PluginInvokeError) -> Self {
        use tauri::plugin::mobile::PluginInvokeError;
        match error {
            PluginInvokeError::InvokeRejected(rejection) => {
                Self::from_rejection(rejection.code.as_deref(), rejection.message.as_deref())
            }
            other => Self::Failed(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rejection payload both native sides send: `{ "message": …, "code": … }`.
    #[derive(serde::Deserialize)]
    struct Rejection {
        code: Option<String>,
        message: Option<String>,
    }

    fn parse_rejection(json: &str) -> KeychainError {
        let rejection: Rejection = serde_json::from_str(json).expect("rejection json");
        KeychainError::from_rejection(rejection.code.as_deref(), rejection.message.as_deref())
    }

    #[test]
    fn invalidated_code_parses_to_invalidated() {
        assert_eq!(
            KeychainError::from_code("invalidated"),
            KeychainError::Invalidated
        );
    }

    #[test]
    fn cancelled_code_parses_to_cancelled() {
        assert_eq!(
            KeychainError::from_code("cancelled"),
            KeychainError::Cancelled
        );
    }

    #[test]
    fn unavailable_code_parses_to_unavailable() {
        assert_eq!(
            KeychainError::from_code("unavailable"),
            KeychainError::Unavailable
        );
    }

    #[test]
    fn failed_code_keeps_the_native_message() {
        assert_eq!(
            parse_rejection(r#"{"message":"SecItemAdd failed: -34018","code":"failed"}"#),
            KeychainError::Failed("SecItemAdd failed: -34018".to_string())
        );
    }

    #[test]
    fn swift_and_kotlin_rejections_parse_by_code_not_message() {
        assert_eq!(
            parse_rejection(r#"{"message":"Biometry changed","code":"invalidated"}"#),
            KeychainError::Invalidated
        );
        assert_eq!(
            parse_rejection(r#"{"message":"User canceled","code":"cancelled"}"#),
            KeychainError::Cancelled
        );
    }

    #[test]
    fn unknown_code_is_failed_with_the_message() {
        assert_eq!(
            KeychainError::from_rejection(Some("userCancel"), Some("Canceled by user")),
            KeychainError::Failed("Canceled by user".to_string())
        );
    }

    #[test]
    fn codes_are_case_sensitive() {
        assert_eq!(
            KeychainError::from_code("Invalidated"),
            KeychainError::Failed("Invalidated".to_string())
        );
    }

    #[test]
    fn missing_code_is_failed_with_the_message() {
        // Tauri's own rejections (bad arguments, unknown command) carry no code.
        assert_eq!(
            parse_rejection(r#"{"message":"No command foo found for plugin moony-keychain"}"#),
            KeychainError::Failed("No command foo found for plugin moony-keychain".to_string())
        );
    }

    #[test]
    fn missing_code_and_message_is_failed_with_a_fallback() {
        assert_eq!(
            KeychainError::from_rejection(None, None),
            KeychainError::Failed("unknown keychain error".to_string())
        );
        assert_eq!(
            KeychainError::from_rejection(None, Some("")),
            KeychainError::Failed("unknown keychain error".to_string())
        );
    }

    #[test]
    fn display_names_each_case() {
        assert!(KeychainError::Invalidated.to_string().contains("enrolment"));
        assert!(KeychainError::Cancelled.to_string().contains("cancelled"));
        assert!(KeychainError::Unavailable
            .to_string()
            .contains("unavailable"));
        assert_eq!(
            KeychainError::Failed("boom".to_string()).to_string(),
            "keychain operation failed: boom"
        );
        assert!(KeychainError::Unsupported
            .to_string()
            .contains("not supported"));
    }
}
