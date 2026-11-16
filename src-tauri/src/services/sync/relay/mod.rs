//! The relay contract (Moony-relay `DESIGN.md`, protocol v1.1): the calls the sync engine makes,
//! their wire types, and `RelayError`, every documented relay answer as a typed value. The relay is
//! untrusted: an answer outside the contract is a `Protocol` error, never a panic, and no error
//! carries a URL, a vault ID, an account number, a token or a response body.

use std::future::Future;

use serde::Deserialize;
use uuid::Uuid;

use super::envelope::blob::{sealed_len, MAX_BLOB_SEALED};
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
/// `kit`: the vault's Recovery Kit as the server knows it, `None` when the vault has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultInfo {
    pub head: u64,
    pub floor: u64,
    pub snapshot_seq: Option<u64>,
    pub paid_until: i64,
    pub bytes: u64,
    pub rotation: u32,
    pub kit: Option<KitInfo>,
}

/// The `kit` of the vault info: when the current Kit token was set (unix seconds on the server's
/// clock) and the minute of its last successful read (`None` = never read).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KitInfo {
    pub since: i64,
    pub last_read: Option<i64>,
}

/// `GET /v1/vaults/:id/kit`: the Kit's envelope (opaque bytes, 1 B to 256 KiB), when the current
/// Kit token was set (`since`) and when the envelope was last replaced (`updated_at`), both in
/// unix seconds on the server's clock.
#[derive(Clone, PartialEq, Eq)]
pub struct KitRead {
    pub envelope: Vec<u8>,
    pub since: i64,
    pub updated_at: i64,
}

impl std::fmt::Debug for KitRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitRead")
            .field("envelope", &self.envelope.len())
            .field("since", &self.since)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// The Kit part of a rotation (`POST /v1/vaults/:id/token`, `kit`): the new envelope, and a new
/// Kit token when the Kit is replaced (`None` keeps the Kit token and only replaces the envelope).
/// Required on a vault that has a Kit (`BadRequest("kit_required")` without it).
#[derive(Clone, Copy)]
pub struct KitRotation<'a> {
    pub envelope: &'a [u8],
    pub token: Option<&'a str>,
}

impl std::fmt::Debug for KitRotation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitRotation")
            .field("envelope", &self.envelope.len())
            .field("replaces_token", &self.token.is_some())
            .finish()
    }
}

/// The largest Kit envelope the server stores (decoded).
pub const MAX_KIT_ENVELOPE: usize = 256 * 1024;

/// The relay's answer to a token rotation (`POST /v1/vaults/:id/token`): the new rotation number
/// and the vault's head at the moment of the rotation (the position the new key starts after).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotated {
    pub rotation: u32,
    pub head: u64,
}

/// One stored key-change bundle (`GET /v1/vaults/:id/rekey`): the rotation it belongs to, the
/// vault head when it was made, when (unix seconds on the server's clock) and its opaque bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Rekey {
    pub rotation: u32,
    pub head: u64,
    pub at: i64,
    pub bundle: Vec<u8>,
}

impl std::fmt::Debug for Rekey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rekey")
            .field("rotation", &self.rotation)
            .field("head", &self.head)
            .field("at", &self.at)
            .field("bundle", &self.bundle.len())
            .finish()
    }
}

/// One page of key-change bundles, oldest first; `more` says to ask again after the last rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RekeyPage {
    pub rekeys: Vec<Rekey>,
    pub more: bool,
}

/// The largest bundle the server stores (decoded).
pub const MAX_REKEY_BUNDLE: usize = 256 * 1024;
/// The most bundles one page may ask for (the server's cap).
pub const MAX_REKEY_PAGE: u32 = 32;
/// The most bundle bytes the client accepts in one page (the server cuts a page at 4 MiB).
pub const MAX_REKEY_PAGE_BYTES: usize = 4 * 1024 * 1024;

/// What every page of bundles must be, whoever answered it (the server is untrusted): at most
/// `limit` bundles, rotations strictly ascending and above `after`, each bundle non-empty and within
/// the cap, the page within its byte cap, and no "more" on an empty page (it would never end).
/// `HttpRelay` checks the answer, the fake the page it built.
pub(crate) fn check_rekey_page(page: &RekeyPage, after: u32, limit: u32) -> RelayResult<()> {
    let refused = || Err(RelayError::Protocol("unexpected answer".into()));
    if (page.more && page.rekeys.is_empty()) || page.rekeys.len() > limit as usize {
        return refused();
    }
    let mut previous = after;
    let mut bytes = 0usize;
    for rekey in &page.rekeys {
        bytes += rekey.bundle.len();
        if rekey.rotation <= previous
            || rekey.at < 0
            || rekey.bundle.is_empty()
            || rekey.bundle.len() > MAX_REKEY_BUNDLE
            || bytes > MAX_REKEY_PAGE_BYTES
        {
            return refused();
        }
        previous = rekey.rotation;
    }
    Ok(())
}

/// What a server says it is (`GET /v1/server`, spec §9.3): `protocol` is the version of this
/// contract, `mode` how the server is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    /// A person's own server (`self_hosted`).
    SelfHosted,
    /// The Moony cloud (`cloud`).
    Cloud,
    /// Any other value: a mode this app does not know.
    Other,
}

/// `GET /v1/server`. `version` is 1–32 characters of `[0-9A-Za-z.+-]`. `blobs`: the server's
/// `features` list names `"blobs"` (the blob routes exist); `kit`: it names `"kit"` (the Recovery
/// Kit routes and the `kit` of a rotation exist). A server without the list has no features, and
/// features this app does not know are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub version: String,
    pub protocol: u32,
    pub mode: ServerMode,
    pub blobs: bool,
    pub kit: bool,
}

/// One blob of a listing (`GET /v1/vaults/:id/blobs`): its ID, its stored (sealed) size and when
/// the server stored it, in unix seconds on the server's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteBlob {
    pub id: Uuid,
    pub size: u64,
    pub created_at: i64,
}

/// One page of a blob listing, ascending by ID. `server_now` is the listing's `now` (unix seconds
/// on the server's clock, the clock `created_at` is on): what a blob's age is measured against.
/// `None` when the server sent no `now` or one that is not a non-negative integer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobPage {
    pub blobs: Vec<RemoteBlob>,
    pub more: bool,
    pub server_now: Option<i64>,
}

/// The most blobs one listing page may ask for (the server's cap).
pub const MAX_BLOB_PAGE: u32 = 1000;

/// The sealed size of an empty blob: the smallest a stored blob can be.
pub const MIN_BLOB_SEALED: u64 = sealed_len(0);

/// A size a stored (sealed) blob can have.
pub(crate) fn is_blob_size(size: u64) -> bool {
    (MIN_BLOB_SEALED..=MAX_BLOB_SEALED).contains(&size)
}

/// What every listing page must be, whoever answered it (the server is untrusted): sizes a sealed
/// blob can have, creation times and a clock from 1970 on, IDs strictly ascending and after
/// `after`, and no "more" on an empty page (it would never end). `HttpRelay` checks the answer,
/// the fake the page it built, so both refuse the same pages.
pub(crate) fn check_blob_page(page: &BlobPage, after: Option<Uuid>) -> RelayResult<()> {
    let refused = || Err(RelayError::Protocol("unexpected answer".into()));
    if (page.more && page.blobs.is_empty()) || page.server_now.is_some_and(|now| now < 0) {
        return refused();
    }
    let mut previous = after;
    for blob in &page.blobs {
        if !is_blob_size(blob.size)
            || blob.created_at < 0
            || previous.is_some_and(|previous| blob.id <= previous)
        {
            return refused();
        }
        previous = Some(blob.id);
    }
    Ok(())
}

/// A blob's sealed bytes on their way up, piece by piece: never one buffer for the whole blob.
pub type BlobBody = Box<dyn Iterator<Item = std::io::Result<Vec<u8>>> + Send + 'static>;

/// What a `BlobSink` wants after it was handed something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkFlow {
    Continue,
    Stop,
}

/// Where a downloaded blob goes, piece by piece: `begin` once with the sealed length the server
/// announced, then `piece` for each part of the body in order. `Stop` ends the download there.
///
/// A download can fail after the sink was handed pieces (the connection drops, the body ends
/// short, a timeout): `get_blob` then answers an error, and the caller must discard whatever the
/// sink received, never keep it as the blob or part of one.
pub trait BlobSink: Send {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow;
    fn piece(&mut self, bytes: &[u8]) -> SinkFlow;
}

/// How a download ended: every byte handed to the sink, or the sink said `Stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Complete,
    Stopped,
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
    /// `409 too_many_rotations`: the vault already holds 32 key-change bundles younger than 30
    /// days.
    #[error("too many key changes")]
    TooManyRotations,
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
    /// A self-hosted relay (Moony-relay `SELF_HOSTED=1`, ruling SH-1) takes no payments: its
    /// checkout answers `409 self_hosted`.
    #[error("self-hosted relay")]
    SelfHosted,
    /// `403 registration_closed`: a self-hosted server takes no new accounts; its operator adds
    /// people.
    #[error("registration closed")]
    RegistrationClosed,
    /// `409 slot_taken`: the pairing slot was already written (by this device, or by someone
    /// who guessed the ID first).
    #[error("pairing slot taken")]
    SlotTaken,
    /// `404 empty`: nothing in the pairing slot yet.
    #[error("pairing slot empty")]
    SlotEmpty,
    /// `410 consumed`: the pairing slot was already read (and erased).
    #[error("pairing slot consumed")]
    SlotConsumed,
    /// `410 mailbox_gone`: the mailbox expired or was burnt.
    #[error("pairing mailbox gone")]
    MailboxGone,
    /// `404 blob_not_found`: the vault has no blob of that ID (never uploaded, deleted, or its
    /// file is gone on the server).
    #[error("blob not found")]
    BlobNotFound,
    /// `507` (`server_full`): the server's disk is at its free-space floor. By status alone.
    #[error("insufficient storage")]
    InsufficientStorage,
    /// `409 kit_exists`: the vault has a Kit with another token (a Kit is replaced only by a
    /// rotation).
    #[error("kit exists")]
    KitExists,
    /// `410 kit_replaced`: the Kit token was replaced by a newer Kit at `replaced_at` (unix
    /// seconds on the server's clock).
    #[error("kit replaced")]
    KitReplaced { replaced_at: i64 },
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
    "kit_required",
    "invalid_kit",
    "invalid_kit_token",
    "bad_mailbox",
    "bad_slot",
    "invalid_blob_id",
    "incomplete_body",
];

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(rename = "error")]
    error: String,
    /// Only a `410 kit_replaced` carries it.
    #[serde(rename = "replacedAt", default)]
    replaced_at: Option<i64>,
}

impl RelayError {
    /// The typed error of a relay answer that is not a success, by status and the `"error"` code
    /// of its `{"error": "<code>"}` body. Rate limits, redirects, `408`, `507` and 5xx go by
    /// status alone (an edge or proxy may answer them with HTML); everything else needs the
    /// documented code for its status, and an unknown code or a body that is not that JSON shape
    /// is `Protocol`. A `502` / `503` of the checkout route is `CheckoutUnavailable`, decided by
    /// the caller, which knows the route.
    pub fn from_response(status: u16, body: &[u8]) -> Self {
        if let Some(error) = Self::by_status(status) {
            return error;
        }
        let Ok(ErrorBody {
            error: code,
            replaced_at,
        }) = serde_json::from_slice::<ErrorBody>(body)
        else {
            return RelayError::Protocol("not json".into());
        };
        if (status, code.as_str()) == (410, "kit_replaced") {
            return match replaced_at.filter(|at| *at >= 0) {
                Some(replaced_at) => RelayError::KitReplaced { replaced_at },
                None => RelayError::Protocol("unexpected answer".into()),
            };
        }
        Self::by_code(status, &code)
    }

    /// The typed error of an answer whose code came from its `Moony-Error` header (blob routes;
    /// a `HEAD` answer has no body): the same status rules and code table as `from_response`.
    pub fn from_code(status: u16, code: &str) -> Self {
        Self::by_status(status).unwrap_or_else(|| Self::by_code(status, code))
    }

    /// The statuses decided without a code; `None` for a 4xx, which needs its code.
    fn by_status(status: u16) -> Option<Self> {
        Some(match status {
            300..=399 => RelayError::Protocol("redirect".into()),
            // `429 too_many_uploads` (two blob uploads of a vault running) passes the same way.
            429 => RelayError::RateLimited,
            // `507 server_full`: the server's disk is at its free-space floor.
            507 => RelayError::InsufficientStorage,
            // A slow upload (`408 body_timeout`) passes like a busy server (`503 busy`).
            408 | 500..=599 => RelayError::Server(status),
            400..=499 => return None,
            _ => RelayError::Protocol("unexpected status".into()),
        })
    }

    fn by_code(status: u16, code: &str) -> Self {
        match (status, code) {
            (400, code) => match BAD_REQUEST_CODES.iter().find(|known| **known == code) {
                Some(known) => RelayError::BadRequest((*known).to_string()),
                None => RelayError::Protocol("unknown error code".into()),
            },
            (401, "unauthorized") => RelayError::Unauthorized,
            (402, "payment_required") => RelayError::PaymentRequired,
            (403, "token_read_only") => RelayError::TokenReadOnly,
            (409, "too_many_rotations") => RelayError::TooManyRotations,
            (403, "vault_limit_reached") => RelayError::VaultLimitReached,
            (403, "registration_closed") => RelayError::RegistrationClosed,
            (403, "managed_by_operator") => RelayError::BadRequest("managed_by_operator".into()),
            (404, "account_not_found") => RelayError::AccountNotFound,
            (404, "vault_not_found") => RelayError::VaultNotFound,
            (404, "no_snapshot") => RelayError::NoSnapshot,
            (404, "blob_not_found") => RelayError::BlobNotFound,
            (404, "not_found") => RelayError::BadRequest("not_found".into()),
            (405, "method_not_allowed") => RelayError::BadRequest("method_not_allowed".into()),
            (409, "vault_exists") => RelayError::VaultExists,
            (409, "account_has_vaults") => RelayError::AccountHasVaults,
            (409, "snapshot_outdated") => RelayError::SnapshotOutdated,
            (409, "kit_exists") => RelayError::KitExists,
            // The vault is still being deleted (a re-created vault always has a new ID, so this
            // is near unreachable): the relay is busy, a passing server error that backs off.
            (409, "vault_deleting") => RelayError::Server(409),
            (409, "self_hosted") => RelayError::SelfHosted,
            (409, "slot_taken") => RelayError::SlotTaken,
            (404, "empty") => RelayError::SlotEmpty,
            (410, "consumed") => RelayError::SlotConsumed,
            (410, "mailbox_gone") => RelayError::MailboxGone,
            (410, "snapshot_required") => RelayError::SnapshotRequired,
            (411, "length_required") => RelayError::BadRequest("length_required".into()),
            (413, "quota_exceeded") => RelayError::QuotaExceeded,
            (413, "snapshot_too_large") => RelayError::SnapshotTooLarge,
            (413, "body_too_large") => RelayError::BadRequest("body_too_large".into()),
            (413, "rekey_too_large") => RelayError::BadRequest("rekey_too_large".into()),
            (413, "kit_too_large") => RelayError::BadRequest("kit_too_large".into()),
            (413, "blob_too_large") => RelayError::BadRequest("blob_too_large".into()),
            _ => RelayError::Protocol("unknown error code".into()),
        }
    }
}

/// The relay calls of the sync engine. `HttpRelay` talks to the real relay; tests use a fake.
pub trait Relay: Send + Sync {
    /// `GET /v1/server`: what the server is and which protocol it speaks. No credentials.
    fn server_info(&self) -> impl Future<Output = RelayResult<ServerInfo>> + Send;
    /// `POST /v1/accounts` → the new 16-digit account number.
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send;
    /// `GET /v1/accounts/me`.
    fn account_status(
        &self,
        account: &str,
    ) -> impl Future<Output = RelayResult<AccountStatus>> + Send;
    /// `POST /v1/accounts/me/checkout` → the payment page URL (`SelfHosted` from a relay that
    /// takes no payments).
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
    /// `POST /v1/vaults/:id/token {token, rekey}`: the current token hands the vault to
    /// `new_token` and stores `bundle` (1 B to 256 KiB, opaque to the server). The retired token
    /// reads for 30 days and writes nothing: a second rotation with it is `TokenReadOnly`
    /// (the other of two concurrent rotations lost), the 33rd inside 30 days `TooManyRotations`.
    fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> impl Future<Output = RelayResult<Rotated>> + Send;
    /// `POST /v1/vaults/:id/token {token, rekey, kit}`: `rotate_token` with the Kit's part, atomic
    /// with it (a refused rotation leaves the Kit as it was). `kit` is required on a vault that
    /// has a Kit (`BadRequest("kit_required")`); with a `token` the Kit is replaced (the old Kit
    /// token then reads `KitReplaced`), and that is also how a vault without a Kit gets one by
    /// rotation. Errors besides `rotate_token`'s: `BadRequest("invalid_kit" | "invalid_kit_token"
    /// | "kit_too_large")`.
    fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: KitRotation<'_>,
    ) -> impl Future<Output = RelayResult<Rotated>> + Send;
    /// `PUT /v1/vaults/:id/kit {token, envelope}` (the current vault token, no subscription):
    /// sets the Kit up; `true` when it was created (201), `false` when the same Kit token was
    /// already stored and only the envelope was replaced (200, a retried setup). Another token
    /// while a Kit exists is `KitExists`; a token that is not 43 base64url characters or equals a
    /// vault token (or a replaced Kit token) is `BadRequest("invalid_kit_token")`, an empty or
    /// malformed envelope `BadRequest("invalid_kit")`, one over `MAX_KIT_ENVELOPE`
    /// `BadRequest("kit_too_large")`.
    fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> impl Future<Output = RelayResult<bool>> + Send;
    /// `GET /v1/vaults/:id/kit` with the Kit token as the bearer (never a vault token): the
    /// envelope. `KitReplaced` for a Kit token a newer Kit replaced, `Unauthorized` for any other
    /// token or a vault without a Kit, `VaultNotFound`, `PaymentRequired` after the read grace.
    fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> impl Future<Output = RelayResult<KitRead>> + Send;
    /// `GET /v1/vaults/:id/rekey?after=&limit=` (current or previous token): the bundles of
    /// rotations above `after`, oldest first, `limit` 1..=`MAX_REKEY_PAGE`. Bundles older than 30
    /// days are gone.
    fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> impl Future<Output = RelayResult<RekeyPage>> + Send;
    /// `PUT /v1/vaults/:id/blobs/:blob`: `body` streamed with `Content-Length: sealed_len`. A
    /// blob the server already holds succeeds without being stored again. A body that is not
    /// `sealed_len` bytes is `BadRequest("length_required")`; one over the blob cap is refused
    /// before anything is sent.
    fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> impl Future<Output = RelayResult<()>> + Send;
    /// `HEAD /v1/vaults/:id/blobs/:blob` → the stored size, or `None` when the blob is missing.
    fn head_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<Option<u64>>> + Send;
    /// `GET /v1/vaults/:id/blobs/:blob`, handed to `sink` piece by piece as it arrives.
    fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> impl Future<Output = RelayResult<Delivery>> + Send;
    /// `GET /v1/vaults/:id/blobs?after=&limit=` (`after` exclusive; `limit` 1..=`MAX_BLOB_PAGE`).
    fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> impl Future<Output = RelayResult<BlobPage>> + Send;
    /// `DELETE /v1/vaults/:id/blobs/:blob`; a blob that is already gone is success.
    fn delete_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<()>> + Send;
}

/// The most a pairing slot holds (Moony-relay `MAX_MAILBOX_SLOT_BYTES`).
pub const MAX_SLOT_BYTES: usize = 64 * 1024;

/// The relay's pairing mailbox (Moony-relay `src/mailbox.ts`): a three-slot drop box for the key
/// exchange that pairs a device (slots 1 and 2) and the shower's acknowledgement (slot 3). No credentials; each slot is written once and read once, and the
/// mailbox lives ten minutes. `mailbox` is the 22-character base64url ID, `slot` 1, 2 or 3.
pub trait Mailbox: Send + Sync {
    /// `PUT /v1/pairing/:id/:slot` (raw body, 1 B to 64 KiB) → 204.
    fn put_slot(
        &self,
        mailbox: &str,
        slot: u8,
        bytes: &[u8],
    ) -> impl Future<Output = RelayResult<()>> + Send;
    /// `GET /v1/pairing/:id/:slot` → `Some(bytes)` (the relay erases them) or `None` (404 empty).
    fn take_slot(
        &self,
        mailbox: &str,
        slot: u8,
    ) -> impl Future<Output = RelayResult<Option<Vec<u8>>>> + Send;
    /// `DELETE /v1/pairing/:id` → 204, idempotent.
    fn burn_mailbox(&self, mailbox: &str) -> impl Future<Output = RelayResult<()>> + Send;
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
            (409, "self_hosted", E::SelfHosted),
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
            // A self-hosted server (spec §5.3, §5.5): no open registration, no self-deletion.
            (403, "registration_closed", E::RegistrationClosed),
            (403, "managed_by_operator", bad("managed_by_operator")),
            (404, "vault_not_found", E::VaultNotFound),
            (404, "no_snapshot", E::NoSnapshot),
            (409, "vault_exists", E::VaultExists),
            (409, "snapshot_outdated", E::SnapshotOutdated),
            // A vault still being deleted (feat/pairing-ack): the relay is busy, try later.
            (409, "vault_deleting", E::Server(409)),
            (410, "snapshot_required", E::SnapshotRequired),
            (411, "length_required", bad("length_required")),
            (413, "quota_exceeded", E::QuotaExceeded),
            (413, "snapshot_too_large", E::SnapshotTooLarge),
            // Routes the relay serves but no phase 2 call reaches.
            (404, "not_found", bad("not_found")),
            (405, "method_not_allowed", bad("method_not_allowed")),
            // Token rotation (`rotate_token`) and the pairing mailbox.
            (400, "invalid_token", bad("invalid_token")),
            (400, "invalid_rekey", bad("invalid_rekey")),
            (400, "bad_mailbox", bad("bad_mailbox")),
            (400, "bad_slot", bad("bad_slot")),
            (413, "rekey_too_large", bad("rekey_too_large")),
            (409, "too_many_rotations", E::TooManyRotations),
            (409, "slot_taken", E::SlotTaken),
            (404, "empty", E::SlotEmpty),
            (410, "consumed", E::SlotConsumed),
            (410, "mailbox_gone", E::MailboxGone),
            // Blobs (Moony-relay DESIGN.md "Blobs", src/blob-api.ts).
            (404, "blob_not_found", E::BlobNotFound),
            (400, "invalid_blob_id", bad("invalid_blob_id")),
            (400, "incomplete_body", bad("incomplete_body")),
            (413, "blob_too_large", bad("blob_too_large")),
            (429, "too_many_uploads", E::RateLimited),
            (507, "anything", E::InsufficientStorage),
            (507, "server_full", E::InsufficientStorage),
            (500, "internal_error", E::Server(500)),
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

    /// Final review B, minor 5: a vault the relay is still deleting is a passing relay error (the
    /// round backs off and tries again), not an unknown answer.
    #[test]
    fn a_vault_being_deleted_is_a_transient_relay_error() {
        use crate::services::sync::error::{SyncError, Transient};
        let error = RelayError::from_response(409, &body("vault_deleting"));
        assert_eq!(error, E::Server(409));
        let sync = SyncError::from(error);
        assert!(
            matches!(sync, SyncError::Transient(Transient::Relay)),
            "{sync:?}"
        );
        assert!(sync.backs_off());
    }

    /// The Node sync server answers `503 {"error":"busy"}` (body budget, mailbox caps) and
    /// `408 {"error":"body_timeout"}` (a slow upload): both pass, so a round backs off and tries
    /// again instead of stopping on a permanent refusal.
    #[test]
    fn a_busy_or_slow_server_is_a_transient_relay_error() {
        use crate::services::sync::error::{SyncError, Transient};
        for (status, code) in [(503, "busy"), (408, "body_timeout")] {
            let error = RelayError::from_response(status, &body(code));
            assert_eq!(error, E::Server(status), "{status} {code}");
            let sync = SyncError::from(error);
            assert!(
                matches!(sync, SyncError::Transient(Transient::Relay)),
                "{status}: {sync:?}"
            );
            assert!(sync.backs_off(), "{status}");
        }
        // Whatever the body (an edge may answer a timeout with HTML), the status decides.
        assert_eq!(
            RelayError::from_response(408, b"<html>timeout</html>"),
            E::Server(408)
        );
    }

    /// A blob route's code may come from its `Moony-Error` header (a `HEAD` has no body): the
    /// same status rules and the same table as a body's code.
    #[test]
    fn a_header_code_maps_like_a_body_code() {
        assert_eq!(
            RelayError::from_code(404, "blob_not_found"),
            E::BlobNotFound
        );
        assert_eq!(
            RelayError::from_code(404, "vault_not_found"),
            E::VaultNotFound
        );
        assert_eq!(
            RelayError::from_code(507, "server_full"),
            E::InsufficientStorage
        );
        assert_eq!(
            RelayError::from_code(429, "too_many_uploads"),
            E::RateLimited
        );
        assert_eq!(RelayError::from_code(404, "../x"), unknown_code());
    }

    /// A full server backs off like the `507` it was before it had a variant; a missing blob is a
    /// refusal the caller handles.
    #[test]
    fn blob_answers_take_their_sync_classes() {
        use crate::services::sync::error::{SyncError, Transient};
        let full = SyncError::from(E::InsufficientStorage);
        assert!(
            matches!(full, SyncError::Transient(Transient::Relay)),
            "{full:?}"
        );
        assert!(full.backs_off());
        let missing = SyncError::from(E::BlobNotFound);
        assert!(
            matches!(missing, SyncError::Refused(E::BlobNotFound)),
            "{missing:?}"
        );
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
            RelayError::from_response(507, b"<html>"),
            E::InsufficientStorage
        );
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
    fn the_kit_answers_map_to_their_variants() {
        let replaced = br#"{"error":"kit_replaced","replacedAt":1700000000}"#;
        assert_eq!(
            E::from_response(410, replaced),
            E::KitReplaced {
                replaced_at: 1_700_000_000
            }
        );
        // The time is the server's word, but it must be there and make sense.
        for body in [
            &br#"{"error":"kit_replaced"}"#[..],
            br#"{"error":"kit_replaced","replacedAt":-5}"#,
            br#"{"error":"kit_replaced","replacedAt":"soon"}"#,
        ] {
            assert!(matches!(E::from_response(410, body), E::Protocol(_)));
        }
        // Only a 410 is a replaced Kit; a 429 stays a rate limit whatever the body says.
        assert_eq!(E::from_response(429, replaced), E::RateLimited);
        assert_eq!(E::from_response(409, &body("kit_exists")), E::KitExists);
        for code in ["kit_required", "invalid_kit", "invalid_kit_token"] {
            assert_eq!(E::from_response(400, &body(code)), bad(code));
        }
        assert_eq!(
            E::from_response(413, &body("kit_too_large")),
            bad("kit_too_large")
        );
    }

    #[test]
    fn checkout_plans_have_their_wire_names() {
        assert_eq!(CheckoutPlan::Monthly.as_wire(), "monthly");
        assert_eq!(CheckoutPlan::Yearly.as_wire(), "yearly");
    }
}
