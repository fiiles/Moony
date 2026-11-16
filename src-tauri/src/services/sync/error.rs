//! `SyncError`: what a sync operation can end in, as classes the engine acts on (ruling P2). A
//! relay answer becomes one of these through `From<RelayError>`; the engine backs off on the
//! transient classes (`backs_off`, ruling P8) and stops the round on the others; a command turns
//! any of them into a `validation.sync*` key with `to_app_error`.

use super::relay::RelayError;
use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("snapshot required")]
    SnapshotRequired,
    #[error("vault not found")]
    VaultNotFound,
    #[error("payment required")]
    Lapsed,
    #[error("unauthorized")]
    Unauthorized,
    #[error("read-only token")]
    ReadOnlyToken,
    #[error("too large: {0:?}")]
    TooLarge(TooLarge),
    #[error("rate limited")]
    RateLimited,
    #[error("transient: {0:?}")]
    Transient(Transient),
    /// A relay answer that is neither transient nor a sync state (ruling P22).
    #[error("refused: {0}")]
    Refused(RelayError),
    #[error(transparent)]
    Local(#[from] AppError),
}

/// What was too large: the vault's op quota, a snapshot upload, or a body this device read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooLarge {
    Quota,
    Snapshot,
    Body,
}

/// Why the relay could not be used right now: no connection, or an answer outside the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transient {
    Offline,
    Relay,
}

pub type SyncResult<T> = std::result::Result<T, SyncError>;

impl From<RelayError> for SyncError {
    fn from(error: RelayError) -> Self {
        match error {
            RelayError::SnapshotRequired => SyncError::SnapshotRequired,
            RelayError::VaultNotFound => SyncError::VaultNotFound,
            RelayError::PaymentRequired => SyncError::Lapsed,
            RelayError::Unauthorized => SyncError::Unauthorized,
            RelayError::TokenReadOnly => SyncError::ReadOnlyToken,
            RelayError::QuotaExceeded => SyncError::TooLarge(TooLarge::Quota),
            RelayError::SnapshotTooLarge => SyncError::TooLarge(TooLarge::Snapshot),
            RelayError::RateLimited => SyncError::RateLimited,
            RelayError::Network(_) => SyncError::Transient(Transient::Offline),
            // A full disk (`507`) backs off like any passing server error.
            RelayError::Protocol(_) | RelayError::Server(_) | RelayError::InsufficientStorage => {
                SyncError::Transient(Transient::Relay)
            }
            refused @ (RelayError::VaultLimitReached
            | RelayError::AccountNotFound
            | RelayError::NoSnapshot
            | RelayError::VaultExists
            | RelayError::AccountHasVaults
            | RelayError::SnapshotOutdated
            | RelayError::TooManyRotations
            | RelayError::CheckoutUnavailable
            | RelayError::SelfHosted
            | RelayError::RegistrationClosed
            | RelayError::SlotTaken
            | RelayError::SlotEmpty
            | RelayError::SlotConsumed
            | RelayError::MailboxGone
            | RelayError::BlobNotFound
            | RelayError::KitExists
            | RelayError::KitReplaced { .. }
            | RelayError::BadRequest(_)) => SyncError::Refused(refused),
        }
    }
}

impl SyncError {
    /// The classes the engine retries with a growing delay (ruling P8). Every other class stops
    /// the round with a problem; the periodic round tries again.
    pub fn backs_off(&self) -> bool {
        matches!(
            self,
            SyncError::RateLimited | SyncError::Transient(_) | SyncError::Local(_)
        )
    }

    /// The error a command returns: a `validation.sync*` key, or the local error itself.
    /// `SnapshotRequired` is handled inside a round (the engine restores the snapshot); should it
    /// ever reach a command it reads as a relay error.
    pub fn to_app_error(&self) -> AppError {
        let key = match self {
            SyncError::Local(error) => return same_app_error(error),
            SyncError::Transient(Transient::Offline) => "validation.syncRelayUnreachable",
            SyncError::Transient(Transient::Relay) | SyncError::SnapshotRequired => {
                "validation.syncRelayError"
            }
            SyncError::RateLimited => "validation.syncRateLimited",
            SyncError::Lapsed => "validation.syncPaymentRequired",
            SyncError::VaultNotFound => "validation.syncVaultNotFound",
            SyncError::Unauthorized | SyncError::ReadOnlyToken => "validation.syncKeyRotated",
            SyncError::TooLarge(TooLarge::Quota) => "validation.syncQuotaExceeded",
            SyncError::TooLarge(TooLarge::Snapshot) => "validation.syncSnapshotTooLarge",
            SyncError::TooLarge(TooLarge::Body) => "validation.syncSnapshotInvalid",
            SyncError::Refused(RelayError::VaultLimitReached) => "validation.syncVaultLimitReached",
            SyncError::Refused(RelayError::AccountNotFound) => "validation.syncAccountNotFound",
            SyncError::Refused(RelayError::CheckoutUnavailable) => {
                "validation.syncCheckoutUnavailable"
            }
            SyncError::Refused(RelayError::SlotTaken) => "validation.syncPairingCodeUsed",
            SyncError::Refused(RelayError::MailboxGone) => "validation.syncPairingExpired",
            SyncError::Refused(RelayError::SlotConsumed) => "validation.syncPairingFailed",
            SyncError::Refused(_) => "validation.syncRelayError",
        };
        AppError::Validation(key.into())
    }

    /// `to_app_error` for the person's own server, which takes no payment: a blocked account is
    /// `syncAccountBlocked` (never "subscription"), and one the server does not know means the
    /// connection code was revoked.
    pub fn to_own_server_app_error(&self) -> AppError {
        match self {
            SyncError::Lapsed => AppError::Validation(ACCOUNT_BLOCKED.into()),
            SyncError::Refused(RelayError::AccountNotFound) => {
                AppError::Validation(CONNECTION_CODE_REVOKED.into())
            }
            other => other.to_app_error(),
        }
    }
}

/// The keys only the own-server mapping produces; setup shares them.
pub const ACCOUNT_BLOCKED: &str = "validation.syncAccountBlocked";
pub const CONNECTION_CODE_REVOKED: &str = "validation.syncConnectionCodeRevoked";

/// `AppError` is not `Clone`; every variant is one message, so a copy is the same variant.
fn same_app_error(error: &AppError) -> AppError {
    match error {
        AppError::Database(m) => AppError::Database(m.clone()),
        AppError::Auth(m) => AppError::Auth(m.clone()),
        AppError::NotFound(m) => AppError::NotFound(m.clone()),
        AppError::Validation(m) => AppError::Validation(m.clone()),
        AppError::ExternalApi(m) => AppError::ExternalApi(m.clone()),
        AppError::Encryption(m) => AppError::Encryption(m.clone()),
        AppError::Internal(m) => AppError::Internal(m.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use RelayError as E;

    fn key(error: &SyncError) -> String {
        match error.to_app_error() {
            AppError::Validation(key) => key,
            other => panic!("not a validation key: {other:?}"),
        }
    }

    #[test]
    fn relay_errors_fall_into_the_sync_error_classes() {
        // (relay error, class, backs off, key)
        let table: Vec<(RelayError, &str, bool, &str)> = vec![
            (
                E::Network("timeout".into()),
                "Transient(Offline)",
                true,
                "validation.syncRelayUnreachable",
            ),
            (
                E::Protocol("not json".into()),
                "Transient(Relay)",
                true,
                "validation.syncRelayError",
            ),
            (
                E::Server(502),
                "Transient(Relay)",
                true,
                "validation.syncRelayError",
            ),
            (
                E::RateLimited,
                "RateLimited",
                true,
                "validation.syncRateLimited",
            ),
            (
                E::PaymentRequired,
                "Lapsed",
                false,
                "validation.syncPaymentRequired",
            ),
            (
                E::Unauthorized,
                "Unauthorized",
                false,
                "validation.syncKeyRotated",
            ),
            (
                E::TokenReadOnly,
                "ReadOnlyToken",
                false,
                "validation.syncKeyRotated",
            ),
            (
                E::VaultNotFound,
                "VaultNotFound",
                false,
                "validation.syncVaultNotFound",
            ),
            (
                E::SnapshotRequired,
                "SnapshotRequired",
                false,
                "validation.syncRelayError",
            ),
            (
                E::QuotaExceeded,
                "TooLarge(Quota)",
                false,
                "validation.syncQuotaExceeded",
            ),
            (
                E::SnapshotTooLarge,
                "TooLarge(Snapshot)",
                false,
                "validation.syncSnapshotTooLarge",
            ),
            (
                E::VaultLimitReached,
                "Refused(VaultLimitReached)",
                false,
                "validation.syncVaultLimitReached",
            ),
            (
                E::AccountNotFound,
                "Refused(AccountNotFound)",
                false,
                "validation.syncAccountNotFound",
            ),
            (
                E::CheckoutUnavailable,
                "Refused(CheckoutUnavailable)",
                false,
                "validation.syncCheckoutUnavailable",
            ),
            (
                // Setup turns it into `CheckoutTarget::SelfHosted` before it is an error.
                E::SelfHosted,
                "Refused(SelfHosted)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::RegistrationClosed,
                "Refused(RegistrationClosed)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::NoSnapshot,
                "Refused(NoSnapshot)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::VaultExists,
                "Refused(VaultExists)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::AccountHasVaults,
                "Refused(AccountHasVaults)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::SnapshotOutdated,
                "Refused(SnapshotOutdated)",
                false,
                "validation.syncRelayError",
            ),
            (
                E::BadRequest("invalid_ops".into()),
                "Refused(BadRequest(\"invalid_ops\"))",
                false,
                "validation.syncRelayError",
            ),
        ];
        for (relay, class, backs_off, expected_key) in table {
            let label = format!("{relay:?}");
            let error = SyncError::from(relay);
            assert_eq!(format!("{error:?}"), class, "{label}");
            assert_eq!(error.backs_off(), backs_off, "{label}");
            assert_eq!(key(&error), expected_key, "{label}");
        }
    }

    #[test]
    fn local_and_body_errors_keep_their_own_meaning() {
        let local = SyncError::from(AppError::Database("disk I/O error".into()));
        assert!(local.backs_off());
        assert!(matches!(local.to_app_error(), AppError::Database(m) if m == "disk I/O error"));

        let keyed = SyncError::Local(AppError::Validation(
            "validation.syncRelayUrlInvalid".into(),
        ));
        assert_eq!(key(&keyed), "validation.syncRelayUrlInvalid");

        let body = SyncError::TooLarge(TooLarge::Body);
        assert!(!body.backs_off());
        assert_eq!(key(&body), "validation.syncSnapshotInvalid");
    }

    #[test]
    fn mailbox_errors_read_as_pairing_keys() {
        for (relay, class, expected_key) in [
            (
                E::SlotTaken,
                "Refused(SlotTaken)",
                "validation.syncPairingCodeUsed",
            ),
            (
                E::MailboxGone,
                "Refused(MailboxGone)",
                "validation.syncPairingExpired",
            ),
            (
                E::SlotConsumed,
                "Refused(SlotConsumed)",
                "validation.syncPairingFailed",
            ),
            (
                E::SlotEmpty,
                "Refused(SlotEmpty)",
                "validation.syncRelayError",
            ),
        ] {
            let error = SyncError::from(relay);
            assert_eq!(format!("{error:?}"), class);
            assert!(!error.backs_off(), "{class}");
            assert_eq!(key(&error), expected_key, "{class}");
        }
    }

    #[test]
    fn own_server_errors_never_ask_for_payment() {
        assert_eq!(
            SyncError::Lapsed.to_own_server_app_error().to_string(),
            AppError::Validation("validation.syncAccountBlocked".into()).to_string()
        );
        for (error, expected) in [
            (SyncError::Lapsed, "validation.syncAccountBlocked"),
            (
                SyncError::Refused(RelayError::AccountNotFound),
                "validation.syncConnectionCodeRevoked",
            ),
            (
                SyncError::Transient(Transient::Offline),
                "validation.syncRelayUnreachable",
            ),
        ] {
            match error.to_own_server_app_error() {
                AppError::Validation(key) => assert_eq!(key, expected),
                other => panic!("not a validation key: {other:?}"),
            }
        }
    }
}
