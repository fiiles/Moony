//! The relay contract (Moony-relay `DESIGN.md`, protocol v1.1): the calls the sync engine makes,
//! their wire types, and `RelayError`, every documented relay answer as a typed value. The relay is
//! untrusted: an answer outside the contract is a `Protocol` error, never a panic, and no error
//! carries a URL, a vault ID, an account number, a token or a response body.

use std::future::Future;

use serde::Deserialize;

use super::keys::VaultAuth;

#[cfg(test)]
pub mod fake;
pub mod http;

/// The subscription a checkout is for; `"monthly"` / `"yearly"` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutPlan {
    Monthly,
    Yearly,
}

impl CheckoutPlan {
    pub fn as_wire(self) -> &'static str {
        match self {
            CheckoutPlan::Monthly => "monthly",
            CheckoutPlan::Yearly => "yearly",
        }
    }
}

/// `GET /v1/accounts/me`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountStatus {
    pub paid_until: Option<i64>,
    pub active: bool,
    pub vaults: u32,
}

/// `GET /v1/vaults/:id`. `rotation` is the relay's token rotation counter (0 = never rotated).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultInfo {
    pub head: u64,
    pub floor: u64,
    pub snapshot_seq: Option<u64>,
    pub paid_until: i64,
    pub bytes: u64,
    pub rotation: u32,
}

/// One sealed op on its way to the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingOp {
    pub id: String,
    pub ct: Vec<u8>,
}

/// The relay's answer to a push: its head after the push and how many ops were new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PushReceipt {
    pub head: u64,
    pub accepted: u32,
}

/// One op of a pull page, with the sequence number the relay gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulledOp {
    pub seq: u64,
    pub id: String,
    pub ct: Vec<u8>,
}

/// One page of `GET /v1/vaults/:id/ops`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullPage {
    pub ops: Vec<PulledOp>,
    pub head: u64,
    pub more: bool,
}

/// The vault's newest snapshot: the op sequence it includes and its sealed bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotBlob {
    pub seq: u64,
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for SnapshotBlob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapshotBlob")
            .field("seq", &self.seq)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Every answer of the relay that is not the expected success, typed. `Network` and `Protocol`
/// carry a fixed reason (`"timeout"`, `"connect"`, `"body too large"`, `"not json"`, …) and
/// `BadRequest` one of the relay's documented error codes: nothing taken from a URL or a body.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    #[error("network: {0}")]
    Network(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("rate limited")]
    RateLimited,
    #[error("payment required")]
    PaymentRequired,
    #[error("unauthorized")]
    Unauthorized,
    #[error("token read-only")]
    TokenReadOnly,
    #[error("vault limit reached")]
    VaultLimitReached,
    #[error("account not found")]
    AccountNotFound,
    #[error("vault not found")]
    VaultNotFound,
    #[error("no snapshot")]
    NoSnapshot,
    #[error("vault exists")]
    VaultExists,
    #[error("account has vaults")]
    AccountHasVaults,
    #[error("snapshot outdated")]
    SnapshotOutdated,
    #[error("snapshot required")]
    SnapshotRequired,
    #[error("quota exceeded")]
    QuotaExceeded,
    #[error("snapshot too large")]
    SnapshotTooLarge,
    #[error("checkout unavailable")]
    CheckoutUnavailable,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("server error {0}")]
    Server(u16),
}

pub type RelayResult<T> = std::result::Result<T, RelayError>;

/// The `400` codes the relay documents (Moony-relay `src/index.ts`, `src/mailbox.ts`). Only these
/// become `BadRequest`; the code is copied from this list, never from the body.
const BAD_REQUEST_CODES: &[&str] = &[
    "missing_account",
    "invalid_json",
    "invalid_plan",
    "invalid_vault_id",
    "invalid_ops",
    "invalid_limit",
    "invalid_seq",
    "empty_body",
    "snapshot_ahead_of_head",
    "invalid_token",
    "invalid_rekey",
    "bad_mailbox",
    "bad_slot",
];

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(rename = "error")]
    error: String,
}

impl RelayError {
    /// The typed error of a relay answer that is not a success, by status and the `"error"` code
    /// of its `{"error": "<code>"}` body. Rate limits, redirects and 5xx go by status alone (an
    /// edge or proxy may answer them with HTML); everything else needs the documented code for its
    /// status, and an unknown code or a body that is not that JSON shape is `Protocol`. A `502` /
    /// `503` of the checkout route is `CheckoutUnavailable`, decided by the caller, which knows the
    /// route.
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        match status {
            300..=399 => return RelayError::Protocol("redirect".into()),
            429 => return RelayError::RateLimited,
            500..=599 => return RelayError::Server(status),
            400..=499 => {}
            _ => return RelayError::Protocol("unexpected status".into()),
        }
        let Ok(ErrorBody { error: code }) = serde_json::from_slice::<ErrorBody>(body) else {
            return RelayError::Protocol("not json".into());
        };
        match (status, code.as_str()) {
            (400, code) => match BAD_REQUEST_CODES.iter().find(|known| **known == code) {
                Some(known) => RelayError::BadRequest((*known).to_string()),
                None => RelayError::Protocol("unknown error code".into()),
            },
            (401, "unauthorized") => RelayError::Unauthorized,
            (402, "payment_required") => RelayError::PaymentRequired,
            (403, "token_read_only") => RelayError::TokenReadOnly,
            (403, "vault_limit_reached") => RelayError::VaultLimitReached,
            (404, "account_not_found") => RelayError::AccountNotFound,
            (404, "vault_not_found") => RelayError::VaultNotFound,
            (404, "no_snapshot") => RelayError::NoSnapshot,
            (404, "not_found") => RelayError::BadRequest("not_found".into()),
            (405, "method_not_allowed") => RelayError::BadRequest("method_not_allowed".into()),
            (409, "vault_exists") => RelayError::VaultExists,
            (409, "account_has_vaults") => RelayError::AccountHasVaults,
            (409, "snapshot_outdated") => RelayError::SnapshotOutdated,
            (410, "snapshot_required") => RelayError::SnapshotRequired,
            (411, "length_required") => RelayError::BadRequest("length_required".into()),
            (413, "quota_exceeded") => RelayError::QuotaExceeded,
            (413, "snapshot_too_large") => RelayError::SnapshotTooLarge,
            (413, "body_too_large") => RelayError::BadRequest("body_too_large".into()),
            (413, "rekey_too_large") => RelayError::BadRequest("rekey_too_large".into()),
            _ => RelayError::Protocol("unknown error code".into()),
        }
    }
}

/// The relay calls of the sync engine. `HttpRelay` talks to the real relay; tests use a fake.
pub trait Relay: Send + Sync {
    /// `POST /v1/accounts` → the new 16-digit account number.
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send;
    /// `GET /v1/accounts/me`.
    fn account_status(
        &self,
        account: &str,
    ) -> impl Future<Output = RelayResult<AccountStatus>> + Send;
    /// `POST /v1/accounts/me/checkout` → the payment page URL.
    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl Future<Output = RelayResult<String>> + Send;
    /// `PUT /v1/vaults/:id`; `true` when the relay created the vault (201), `false` when it
    /// already existed for this account and token (200).
    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<bool>> + Send;
    /// `GET /v1/vaults/:id`.
    fn vault_info(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<VaultInfo>> + Send;
    /// `POST /v1/vaults/:id/ops`.
    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl Future<Output = RelayResult<PushReceipt>> + Send;
    /// `GET /v1/vaults/:id/ops?after=&limit=`.
    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl Future<Output = RelayResult<PullPage>> + Send;
    /// `PUT /v1/vaults/:id/snapshot?seq=`.
    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl Future<Output = RelayResult<()>> + Send;
    /// `GET /v1/vaults/:id/snapshot`.
    fn get_snapshot(
        &self,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<SnapshotBlob>> + Send;
    /// `DELETE /v1/vaults/:id`.
    fn delete_vault(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<()>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use RelayError as E;

    fn body(code: &str) -> Vec<u8> {
        format!(r#"{{"error":"{code}"}}"#).into_bytes()
    }

    fn bad(code: &str) -> RelayError {
        E::BadRequest(code.to_string())
    }

    fn unknown_code() -> RelayError {
        E::Protocol("unknown error code".into())
    }

    #[test]
    fn every_documented_status_and_code_maps_to_its_variant() {
        let table: Vec<(u16, &str, RelayError)> = vec![
            // Accounts (DESIGN.md "Accounts", src/index.ts, src/account.ts).
            (400, "missing_account", bad("missing_account")),
            (400, "invalid_json", bad("invalid_json")),
            (400, "invalid_plan", bad("invalid_plan")),
            (404, "account_not_found", E::AccountNotFound),
            (409, "account_has_vaults", E::AccountHasVaults),
            (413, "body_too_large", bad("body_too_large")),
            (429, "rate_limited", E::RateLimited),
            (503, "try_again", E::Server(503)),
            (503, "checkout_not_configured", E::Server(503)),
            (502, "checkout_failed", E::Server(502)),
            // Vaults (src/vault.ts).
            (400, "invalid_vault_id", bad("invalid_vault_id")),
            (400, "invalid_ops", bad("invalid_ops")),
            (400, "invalid_limit", bad("invalid_limit")),
            (400, "invalid_seq", bad("invalid_seq")),
            (400, "empty_body", bad("empty_body")),
            (400, "snapshot_ahead_of_head", bad("snapshot_ahead_of_head")),
            (401, "unauthorized", E::Unauthorized),
            (402, "payment_required", E::PaymentRequired),
            (403, "token_read_only", E::TokenReadOnly),
            (403, "vault_limit_reached", E::VaultLimitReached),
            (404, "vault_not_found", E::VaultNotFound),
            (404, "no_snapshot", E::NoSnapshot),
            (409, "vault_exists", E::VaultExists),
            (409, "snapshot_outdated", E::SnapshotOutdated),
            (410, "snapshot_required", E::SnapshotRequired),
            (411, "length_required", bad("length_required")),
            (413, "quota_exceeded", E::QuotaExceeded),
            (413, "snapshot_too_large", E::SnapshotTooLarge),
            // Routes the relay serves but no phase 2 call reaches.
            (404, "not_found", bad("not_found")),
            (405, "method_not_allowed", bad("method_not_allowed")),
            // Token rotation and the pairing mailbox: no call of this client sends them, so the
            // 400s are bad requests and the other codes are outside this client's contract.
            (400, "invalid_token", bad("invalid_token")),
            (400, "invalid_rekey", bad("invalid_rekey")),
            (400, "bad_mailbox", bad("bad_mailbox")),
            (400, "bad_slot", bad("bad_slot")),
            (413, "rekey_too_large", bad("rekey_too_large")),
            (409, "too_many_rotations", unknown_code()),
            (409, "slot_taken", unknown_code()),
            (404, "empty", unknown_code()),
            (410, "consumed", unknown_code()),
            (410, "mailbox_gone", unknown_code()),
            // A documented code under the wrong status, or an unknown code.
            (404, "snapshot_required", unknown_code()),
            (410, "vault_not_found", unknown_code()),
            (403, "unauthorized", unknown_code()),
            (400, "something_new", unknown_code()),
            (418, "teapot", unknown_code()),
        ];
        for (status, code, expected) in table {
            assert_eq!(
                RelayError::from_response(status, &body(code)),
                expected,
                "{status} {code}"
            );
        }
    }

    #[test]
    fn answers_without_the_json_shape_are_protocol_errors() {
        let not_json = E::Protocol("not json".into());
        assert_eq!(
            RelayError::from_response(404, b"<html>nope</html>"),
            not_json
        );
        assert_eq!(RelayError::from_response(400, b""), not_json);
        assert_eq!(RelayError::from_response(401, b"Unauthorized"), not_json);
        assert_eq!(RelayError::from_response(402, br#"{"error":7}"#), not_json);
        assert_eq!(RelayError::from_response(410, br#"{"code":"x"}"#), not_json);
        // Status alone: rate limits, redirects and server errors.
        assert_eq!(
            RelayError::from_response(429, b"<html>slow down</html>"),
            E::RateLimited
        );
        assert_eq!(
            RelayError::from_response(500, b"<html>oops</html>"),
            E::Server(500)
        );
        assert_eq!(RelayError::from_response(599, b""), E::Server(599));
        assert_eq!(
            RelayError::from_response(302, b""),
            E::Protocol("redirect".into())
        );
        assert_eq!(
            RelayError::from_response(200, br#"{"error":"vault_not_found"}"#),
            E::Protocol("unexpected status".into())
        );
    }

    #[test]
    fn a_hostile_code_is_never_copied_into_the_error() {
        // An "error" value carrying a vault ID or an account number stays out of the text.
        for code in ["AAAAAAAAAAAAAAAAAAAAAA", "1234567890123456", "/v1/vaults/x"] {
            for status in [400, 403, 404, 409, 410, 413] {
                let error = RelayError::from_response(status, &body(code));
                assert!(!error.to_string().contains(code), "{error}");
                assert!(!format!("{error:?}").contains(code), "{error:?}");
            }
        }
    }

    #[test]
    fn checkout_plans_have_their_wire_names() {
        assert_eq!(CheckoutPlan::Monthly.as_wire(), "monthly");
        assert_eq!(CheckoutPlan::Yearly.as_wire(), "yearly");
    }
}
