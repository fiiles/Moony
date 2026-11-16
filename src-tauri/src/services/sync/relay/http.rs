//! `HttpRelay`: the relay contract over HTTPS (ruling P7). Every URL is built from one parsed
//! base, every request goes through `services::http::no_redirect_client()`, so the
//! `Moony-Account` header and the bearer token never reach another origin; every body is read
//! under a cap; every error is typed and carries no URL or body.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Body, Method, RequestBuilder, Response, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    check_blob_page, check_rekey_page, is_blob_size, AccountStatus, BlobBody, BlobPage, BlobSink,
    CheckoutPlan, Delivery, Mailbox, OutgoingOp, PullPage, PulledOp, PushReceipt, Rekey, RekeyPage,
    Relay, RelayError, RelayResult, RemoteBlob, Rotated, ServerInfo, ServerMode, SinkFlow,
    SnapshotBlob, VaultInfo, MAX_BLOB_PAGE, MAX_REKEY_BUNDLE, MAX_REKEY_PAGE, MAX_SLOT_BYTES,
};
use crate::error::{AppError, Result};
use crate::services::http::{no_redirect_client, no_redirect_download_client};
use crate::services::sync::envelope::blob::MAX_BLOB_SEALED;
use crate::services::sync::keys::VaultAuth;

/// The relay of release builds (a placeholder until phase 6).
pub const DEFAULT_RELAY_URL: &str = "https://sync.moonyfinance.com";
/// The host of `DEFAULT_RELAY_URL`: the Moony cloud, the one server plain http never reaches.
pub const DEFAULT_RELAY_HOST: &str = "sync.moonyfinance.com";
/// The longest relay address accepted, as typed and once normalized (it rides in the Recovery
/// Kit's QR, ruling SH-2).
pub const MAX_RELAY_URL_CHARS: usize = 256;
/// Account, vault and push answers, and every error answer.
pub const MAX_JSON_BODY: usize = 64 * 1024;
/// A pull page: the relay cuts a page at ~4 MiB of ciphertext, which base64 makes ~5.4 MiB.
pub const MAX_PULL_BODY: usize = 6 * 1024 * 1024;
/// A sealed snapshot: the relay's 100 MiB upload limit.
pub const MAX_SNAPSHOT_BODY: usize = 104_857_600;
/// One snapshot upload or download, end to end.
pub const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(600);
/// One push or pull, end to end: a full pull page (~5.4 MiB) or push (~3 MiB) must finish on a
/// slow link, which the shared 15 s bound would never allow. Other calls keep that bound.
pub const OPS_TIMEOUT: Duration = Duration::from_secs(120);
/// One blob upload or download, end to end: 100 MiB on a slow link.
pub const BLOB_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// A blob listing: at most `MAX_BLOB_PAGE` entries of ~80 bytes each.
const MAX_BLOB_LIST_BODY: usize = 256 * 1024;
/// The most a body buffer reserves up front, whatever length the answer announces.
const MAX_PREALLOCATION: usize = 1024 * 1024;

const ACCOUNT_HEADER: &str = "moony-account";
const SNAPSHOT_SEQ_HEADER: &str = "moony-snapshot-seq";
/// The error code of a blob route's answer, also on a `HEAD`, which has no body.
const ERROR_CODE_HEADER: &str = "moony-error";
/// A blob's stored size, sent with `HEAD` and `GET` besides `Content-Length`.
const BLOB_SIZE_HEADER: &str = "moony-blob-size";

/// The dev sync server of a debug build (`feature::dev_relay_base`: `MOONY_RELAY_URL` at run
/// time, else as the binary was built; an iPhone app has no environment), else
/// `DEFAULT_RELAY_URL`. Release builds never read the variable.
pub fn default_base_url() -> String {
    super::super::feature::dev_relay_base().unwrap_or_else(|| DEFAULT_RELAY_URL.to_string())
}

/// A relay base URL: `https`, or `http` for any host but the Moony cloud (a person's own server,
/// spec §9.2; see `is_insecure_address` for the warning); no user, password, query or fragment; no
/// path but `/`; at most `MAX_RELAY_URL_CHARS` characters. Parsed, never prefix-matched.
pub fn validate_base_url(url: &str) -> Result<()> {
    parse_base_url(url).map(|_| ())
}

/// A valid relay base URL (`validate_base_url`) in one spelling: `scheme://host[:port]` with the
/// host in lowercase, no default port and no trailing `/`. Setup stores this form, so equal
/// addresses compare equal.
pub fn normalize_base_url(url: &str) -> Result<String> {
    let parsed = parse_base_url(url)?;
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

/// Whether `url` is the default relay (`DEFAULT_RELAY_URL`), however it is spelled. An invalid
/// address is not.
pub fn is_default_relay(url: &str) -> bool {
    normalize_base_url(url).is_ok_and(|normal| normal == DEFAULT_RELAY_URL)
}

/// `https` for any host; `http` for any host but the Moony cloud (`DEFAULT_RELAY_HOST`), in every
/// build. The same rule in debug and release: a person's own server may sit on a home network
/// without a certificate, and `is_insecure_address` is what asks the person to be sure.
fn scheme_allowed(scheme: &str, host: Option<&str>) -> bool {
    let Some(host) = host.filter(|host| !host.is_empty()) else {
        return false;
    };
    match scheme {
        "https" => true,
        "http" => host != DEFAULT_RELAY_HOST,
        _ => false,
    }
}

/// Whether a normalized address is plain http to a host other than `localhost` / `127.0.0.1`,
/// the one condition the app warns about. An address that does not parse is not.
pub fn is_insecure_address(normalized: &str) -> bool {
    Url::parse(normalized).is_ok_and(|url| {
        url.scheme() == "http" && !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
    })
}

/// `host[:port]` of an address, for showing which server is meant; `None` when it does not parse.
pub fn display_host(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn parse_base_url(url: &str) -> Result<Url> {
    let invalid = || AppError::Validation("validation.syncRelayUrlInvalid".into());
    if url.chars().count() > MAX_RELAY_URL_CHARS {
        return Err(invalid());
    }
    let mut parsed = Url::parse(url).map_err(|_| invalid())?;
    // `sync.moonyfinance.com.` is the same host as `sync.moonyfinance.com`: every trailing dot of
    // a domain goes (`..` too, or the cloud host would slip past the http refusal), so all
    // spellings compare as one address.
    if let Some(domain) = parsed.domain().filter(|d| d.ends_with('.')) {
        let bare = domain.trim_end_matches('.').to_string();
        parsed.set_host(Some(&bare)).map_err(|_| invalid())?;
    }
    // Parsing can lengthen an address (an international domain becomes punycode): the stored and
    // QR-borne form stays within the cap too.
    if parsed.as_str().trim_end_matches('/').len() > MAX_RELAY_URL_CHARS {
        return Err(invalid());
    }
    let host_allowed = scheme_allowed(parsed.scheme(), parsed.host_str());
    let bare = parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path() == "/";
    if host_allowed && bare {
        Ok(parsed)
    } else {
        Err(invalid())
    }
}

#[derive(Clone)]
pub struct HttpRelay {
    base: Url,
}

impl std::fmt::Debug for HttpRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpRelay")
            .field("base", &self.base.origin().ascii_serialization())
            .finish()
    }
}

impl HttpRelay {
    pub fn new(base: &str) -> Result<Self> {
        Ok(HttpRelay {
            base: parse_base_url(base)?,
        })
    }

    /// `path` joined to the base; anything that would leave the base's origin is refused.
    fn url(&self, path: &str) -> RelayResult<Url> {
        let url = self
            .base
            .join(path)
            .map_err(|_| RelayError::Protocol("url".into()))?;
        if url.origin() != self.base.origin() {
            return Err(RelayError::Protocol("url".into()));
        }
        Ok(url)
    }

    fn account_request(
        &self,
        method: Method,
        path: &str,
        account: &str,
    ) -> RelayResult<RequestBuilder> {
        Ok(no_redirect_client()
            .request(method, self.url(path)?)
            .header(ACCOUNT_HEADER, account_header(account)?))
    }

    fn vault_url(&self, vault: &VaultAuth, suffix: &str) -> RelayResult<Url> {
        if !is_base64url(&vault.vault_id, 22) {
            return Err(RelayError::BadRequest("invalid_vault_id".into()));
        }
        self.url(&format!("v1/vaults/{}{suffix}", vault.vault_id))
    }

    /// A blob's URL: its ID in lowercase hyphenated form, the only form the server takes.
    fn blob_url(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<Url> {
        self.vault_url(vault, &format!("/blobs/{}", blob.hyphenated()))
    }

    fn vault_request(
        &self,
        method: Method,
        url: Url,
        vault: &VaultAuth,
    ) -> RelayResult<RequestBuilder> {
        Ok(no_redirect_client()
            .request(method, url)
            .header(AUTHORIZATION, bearer(vault)?))
    }

    /// The pairing URL of `mailbox` (and `slot`), after the argument checks. The ID never leaves
    /// this URL: it is in no error and no log.
    fn pairing_url(&self, mailbox: &str, slot: Option<u8>) -> RelayResult<Url> {
        if !is_base64url(mailbox, 22) {
            return Err(RelayError::BadRequest("bad_mailbox".into()));
        }
        match slot {
            None => self.url(&format!("v1/pairing/{mailbox}")),
            Some(1..=3) => self.url(&format!("v1/pairing/{mailbox}/{}", slot.unwrap_or(0))),
            Some(_) => Err(RelayError::BadRequest("bad_slot".into())),
        }
    }

    /// `POST /v1/vaults/:id/ops` with the ops as standard padded base64.
    fn push_request(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<RequestBuilder> {
        let url = self.vault_url(vault, "/ops")?;
        let body = PushWire {
            ops: ops
                .iter()
                .map(|op| PushOpWire {
                    id: &op.id,
                    ct: STANDARD.encode(&op.ct),
                })
                .collect(),
        };
        Ok(self
            .vault_request(Method::POST, url, vault)?
            .json(&body)
            .timeout(OPS_TIMEOUT))
    }

    /// `GET /v1/vaults/:id/ops?after=&limit=`.
    fn pull_request(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> RelayResult<RequestBuilder> {
        let mut url = self.vault_url(vault, "/ops")?;
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limit.to_string());
        Ok(self
            .vault_request(Method::GET, url, vault)?
            .timeout(OPS_TIMEOUT))
    }
}

/// `n` characters of the base64url alphabet: a vault ID (22) or a vault token (43).
fn is_base64url(text: &str, n: usize) -> bool {
    text.len() == n
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_account_number(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| b.is_ascii_digit())
}

/// The `Moony-Account` value, marked sensitive so it never shows in a header dump.
fn account_header(account: &str) -> RelayResult<HeaderValue> {
    let invalid = || RelayError::BadRequest("missing_account".into());
    if !is_account_number(account) {
        return Err(invalid());
    }
    let mut value = HeaderValue::from_str(account).map_err(|_| invalid())?;
    value.set_sensitive(true);
    Ok(value)
}

/// `Bearer <token>`, built in a wiped buffer and marked sensitive.
fn bearer(vault: &VaultAuth) -> RelayResult<HeaderValue> {
    let invalid = || RelayError::BadRequest("invalid_token".into());
    if !is_base64url(&vault.token, 43) {
        return Err(invalid());
    }
    let mut text = Zeroizing::new(String::with_capacity(7 + vault.token.len()));
    text.push_str("Bearer ");
    text.push_str(&vault.token);
    let mut value = HeaderValue::from_str(&text).map_err(|_| invalid())?;
    value.set_sensitive(true);
    Ok(value)
}

/// A transport failure as a fixed reason; the error's URL and text are dropped.
fn network(error: reqwest::Error) -> RelayError {
    let error = error.without_url();
    let reason = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_body() || error.is_decode() {
        "body"
    } else if error.is_request() {
        "request"
    } else {
        "other"
    };
    RelayError::Network(reason.into())
}

async fn send(request: RequestBuilder) -> RelayResult<Response> {
    request.send().await.map_err(network)
}

/// The whole body, or `Protocol("body too large")` as soon as it passes `cap`: an announced
/// length over the cap is refused before a byte is read.
async fn read_capped(mut response: Response, cap: usize) -> RelayResult<Vec<u8>> {
    let too_large = || RelayError::Protocol("body too large".into());
    let announced = response.content_length().unwrap_or(0);
    if announced > cap as u64 {
        return Err(too_large());
    }
    // The announced length is the relay's word: reserve at most 1 MiB and let the rest grow.
    let mut body = Vec::with_capacity((announced as usize).min(MAX_PREALLOCATION));
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        if chunk.len() > cap - body.len() {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// The response when its status is one of `expected`, else the typed error of the answer.
async fn expect_status(response: Response, expected: &[u16]) -> RelayResult<Response> {
    let status = response.status().as_u16();
    if expected.contains(&status) {
        return Ok(response);
    }
    if response.status().is_success() {
        return Err(RelayError::Protocol("unexpected status".into()));
    }
    Err(match read_capped(response, MAX_JSON_BODY).await {
        Ok(body) => RelayError::from_response(status, &body),
        Err(RelayError::Network(reason)) => RelayError::Network(reason),
        // An error page over the cap is judged by its status alone.
        Err(_) => RelayError::from_response(status, b""),
    })
}

/// `expect_status` for a blob route: an error answer's code is read from its `Moony-Error`
/// header when there is one (a `HEAD` answer has no body), else from its body.
async fn expect_blob_status(response: Response, expected: &[u16]) -> RelayResult<Response> {
    let status = response.status().as_u16();
    if !expected.contains(&status) && !response.status().is_success() {
        if let Some(code) = error_code(&response) {
            return Err(RelayError::from_code(status, code));
        }
    }
    expect_status(response, expected).await
}

fn error_code(response: &Response) -> Option<&str> {
    response
        .headers()
        .get(ERROR_CODE_HEADER)
        .and_then(|value| value.to_str().ok())
}

/// The `Content-Length` header as sent (digits only). Read from the header, not the body: a
/// `HEAD` answer's body is empty whatever length it announces.
fn announced_length(response: &Response) -> Option<u64> {
    digits_header(response, CONTENT_LENGTH.as_str())
}

/// A header of digits only, as a number.
fn digits_header(response: &Response, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|text| text.parse::<u64>().ok())
}

/// A blob body on its way up, held to the length it announced: a piece that runs past it, or an
/// end before it, becomes an error that stops the upload, and `mismatch` records why.
struct Measured {
    body: BlobBody,
    left: u64,
    mismatch: Arc<AtomicBool>,
    done: bool,
}

impl Measured {
    fn length_error(&self) -> std::io::Error {
        self.mismatch.store(true, Ordering::SeqCst);
        std::io::Error::other("blob length")
    }

    /// What the source still holds once the announced length is reached: nothing (or only empty
    /// pieces) is fine; more bytes are a length error, a read error stays an error.
    fn check_rest(&mut self) -> std::io::Result<()> {
        loop {
            match self.body.next() {
                None => return Ok(()),
                Some(Ok(piece)) if piece.is_empty() => {}
                Some(Ok(_)) => return Err(self.length_error()),
                Some(Err(error)) => return Err(error),
            }
        }
    }
}

impl Iterator for Measured {
    type Item = std::io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let item = match self.body.next() {
            Some(Ok(piece)) if piece.len() as u64 <= self.left => {
                self.left -= piece.len() as u64;
                if self.left > 0 {
                    return Some(Ok(piece));
                }
                // The announced length is reached, and the transport never polls the body
                // again once it has written that much: the rest of the source is checked now,
                // before the last piece goes out, or extra bytes would be cut off unseen.
                Some(self.check_rest().map(|()| piece))
            }
            Some(Err(error)) => Some(Err(error)),
            None if self.left == 0 => None,
            // Longer than announced, or ended short.
            Some(Ok(_)) | None => Some(Err(self.length_error())),
        };
        self.done = true;
        item
    }
}

async fn read_json<T: DeserializeOwned>(response: Response, cap: usize) -> RelayResult<T> {
    let body = read_capped(response, cap).await?;
    serde_json::from_slice(&body).map_err(|_| RelayError::Protocol("not json".into()))
}

fn decode_ct(ct: &str) -> RelayResult<Vec<u8>> {
    STANDARD
        .decode(ct)
        .map_err(|_| RelayError::Protocol("bad base64".into()))
}

#[derive(Deserialize)]
struct NewAccountWire {
    #[serde(rename = "account")]
    account: String,
}

#[derive(Deserialize)]
struct AccountStatusWire {
    #[serde(rename = "paidUntil")]
    paid_until: Option<i64>,
    #[serde(rename = "active")]
    active: bool,
    #[serde(rename = "vaults")]
    vaults: u32,
}

#[derive(Deserialize)]
struct ServerInfoWire {
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "version")]
    version: String,
    #[serde(rename = "protocol")]
    protocol: u32,
    #[serde(rename = "mode")]
    mode: String,
    /// Optional capabilities, read loosely: a missing field, `null`, anything but an array, and
    /// any element that is not a string all mean no feature, never an unreadable answer.
    #[serde(rename = "features", default)]
    features: Option<serde_json::Value>,
}

/// The feature a server lists when it has the blob routes.
const BLOBS_FEATURE: &str = "blobs";

/// What `GET /v1/server` answers as `name`.
const SERVER_NAME: &str = "moony-sync";

/// 1–32 characters of `[0-9A-Za-z.+-]`.
fn is_server_version(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

#[derive(Serialize)]
struct CheckoutWire {
    #[serde(rename = "plan")]
    plan: &'static str,
}

#[derive(Deserialize)]
struct CheckoutAnswerWire {
    #[serde(rename = "url")]
    url: String,
}

#[derive(Deserialize)]
struct CreatedWire {
    #[serde(rename = "created")]
    created: bool,
}

#[derive(Deserialize)]
struct VaultInfoWire {
    #[serde(rename = "head")]
    head: u64,
    #[serde(rename = "floor")]
    floor: u64,
    #[serde(rename = "snapshotSeq")]
    snapshot_seq: Option<u64>,
    #[serde(rename = "paidUntil")]
    paid_until: i64,
    #[serde(rename = "bytes")]
    bytes: u64,
    #[serde(rename = "rotation")]
    rotation: u32,
}

#[derive(Serialize)]
struct PushWire<'a> {
    #[serde(rename = "ops")]
    ops: Vec<PushOpWire<'a>>,
}

#[derive(Serialize)]
struct PushOpWire<'a> {
    #[serde(rename = "id")]
    id: &'a str,
    #[serde(rename = "ct")]
    ct: String,
}

#[derive(Deserialize)]
struct PushAnswerWire {
    #[serde(rename = "head")]
    head: u64,
    #[serde(rename = "accepted")]
    accepted: u32,
}

#[derive(Deserialize)]
struct PullWire {
    #[serde(rename = "ops")]
    ops: Vec<PullOpWire>,
    #[serde(rename = "head")]
    head: u64,
    #[serde(rename = "more")]
    more: bool,
}

#[derive(Deserialize)]
struct BlobListWire {
    #[serde(rename = "blobs")]
    blobs: Vec<BlobWire>,
    #[serde(rename = "more")]
    more: bool,
    /// The server's clock; read loosely, since an answer without a usable one is still a page.
    #[serde(rename = "now", default)]
    now: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct BlobWire {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "size")]
    size: u64,
    #[serde(rename = "createdAt")]
    created_at: i64,
}

/// A blob ID as the server must write it: a UUID in lowercase hyphenated form, nothing else.
fn canonical_blob_id(text: &str) -> Option<Uuid> {
    Uuid::try_parse(text)
        .ok()
        .filter(|id| id.hyphenated().to_string() == text)
}

#[derive(Serialize)]
struct RotateWire<'a> {
    #[serde(rename = "token")]
    token: &'a str,
    #[serde(rename = "rekey")]
    rekey: String,
}

#[derive(Deserialize)]
struct RotatedWire {
    #[serde(rename = "rotation")]
    rotation: u32,
    #[serde(rename = "head")]
    head: u64,
}

#[derive(Deserialize)]
struct RekeyListWire {
    #[serde(rename = "rekeys")]
    rekeys: Vec<RekeyWire>,
    #[serde(rename = "more")]
    more: bool,
}

#[derive(Deserialize)]
struct RekeyWire {
    #[serde(rename = "rotation")]
    rotation: u32,
    #[serde(rename = "head")]
    head: u64,
    #[serde(rename = "at")]
    at: i64,
    #[serde(rename = "rekey")]
    rekey: String,
}

#[derive(Deserialize)]
struct PullOpWire {
    #[serde(rename = "seq")]
    seq: u64,
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "ct")]
    ct: String,
}

impl Relay for HttpRelay {
    async fn server_info(&self) -> RelayResult<ServerInfo> {
        // No account header: the question is asked before there is an account.
        let request = no_redirect_client().get(self.url("v1/server")?);
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: ServerInfoWire = read_json(response, MAX_JSON_BODY).await?;
        if answer.name != SERVER_NAME {
            return Err(RelayError::Protocol("not a sync server".into()));
        }
        if !is_server_version(&answer.version) {
            return Err(RelayError::Protocol("version".into()));
        }
        let mode = match answer.mode.as_str() {
            "self_hosted" => ServerMode::SelfHosted,
            "cloud" => ServerMode::Cloud,
            _ => ServerMode::Other,
        };
        Ok(ServerInfo {
            version: answer.version,
            protocol: answer.protocol,
            mode,
            blobs: answer
                .features
                .as_ref()
                .and_then(serde_json::Value::as_array)
                .is_some_and(|features| {
                    features
                        .iter()
                        .any(|feature| feature.as_str() == Some(BLOBS_FEATURE))
                }),
        })
    }

    async fn create_account(&self) -> RelayResult<String> {
        let request = no_redirect_client().post(self.url("v1/accounts")?);
        let response = expect_status(send(request).await?, &[201]).await?;
        let answer: NewAccountWire = read_json(response, MAX_JSON_BODY).await?;
        if !is_account_number(&answer.account) {
            return Err(RelayError::Protocol("unexpected answer".into()));
        }
        Ok(answer.account)
    }

    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        let request = self.account_request(Method::GET, "v1/accounts/me", account)?;
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: AccountStatusWire = read_json(response, MAX_JSON_BODY).await?;
        Ok(AccountStatus {
            paid_until: answer.paid_until,
            active: answer.active,
            vaults: answer.vaults,
        })
    }

    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        let request = self
            .account_request(Method::POST, "v1/accounts/me/checkout", account)?
            .json(&CheckoutWire {
                plan: plan.as_wire(),
            });
        let response = send(request).await?;
        // The relay answers 503 when checkout is not configured and 502 when the payment
        // provider failed; on this route alone both mean "try paying later".
        if matches!(response.status().as_u16(), 502 | 503) {
            return Err(RelayError::CheckoutUnavailable);
        }
        let response = expect_status(response, &[200]).await?;
        let answer: CheckoutAnswerWire = read_json(response, MAX_JSON_BODY).await?;
        // The URL is handed to the system opener: only an https page, never another scheme.
        match Url::parse(&answer.url) {
            Ok(url) if url.scheme() == "https" => Ok(answer.url),
            _ => Err(RelayError::Protocol("unexpected answer".into())),
        }
    }

    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        let url = self.vault_url(vault, "")?;
        let request = self
            .vault_request(Method::PUT, url, vault)?
            .header(ACCOUNT_HEADER, account_header(account)?);
        let response = expect_status(send(request).await?, &[200, 201]).await?;
        let created = response.status().as_u16() == 201;
        let answer: CreatedWire = read_json(response, MAX_JSON_BODY).await?;
        if answer.created != created {
            return Err(RelayError::Protocol("unexpected answer".into()));
        }
        Ok(created)
    }

    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        let url = self.vault_url(vault, "")?;
        let request = self.vault_request(Method::GET, url, vault)?;
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: VaultInfoWire = read_json(response, MAX_JSON_BODY).await?;
        Ok(VaultInfo {
            head: answer.head,
            floor: answer.floor,
            snapshot_seq: answer.snapshot_seq,
            paid_until: answer.paid_until,
            bytes: answer.bytes,
            rotation: answer.rotation,
        })
    }

    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        let request = self.push_request(vault, ops)?;
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: PushAnswerWire = read_json(response, MAX_JSON_BODY).await?;
        Ok(PushReceipt {
            head: answer.head,
            accepted: answer.accepted,
        })
    }

    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        let request = self.pull_request(vault, after, limit)?;
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: PullWire = read_json(response, MAX_PULL_BODY).await?;
        let ops = answer
            .ops
            .into_iter()
            .map(|op| {
                Ok(PulledOp {
                    seq: op.seq,
                    id: op.id,
                    ct: decode_ct(&op.ct)?,
                })
            })
            .collect::<RelayResult<Vec<_>>>()?;
        Ok(PullPage {
            ops,
            head: answer.head,
            more: answer.more,
        })
    }

    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        let mut url = self.vault_url(vault, "/snapshot")?;
        url.query_pairs_mut().append_pair("seq", &seq.to_string());
        let request = self
            .vault_request(Method::PUT, url, vault)?
            .header(CONTENT_TYPE, "application/octet-stream")
            .timeout(SNAPSHOT_TIMEOUT)
            .body(bytes);
        expect_status(send(request).await?, &[201]).await?;
        Ok(())
    }

    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        let url = self.vault_url(vault, "/snapshot")?;
        let request = self
            .vault_request(Method::GET, url, vault)?
            .timeout(SNAPSHOT_TIMEOUT);
        let response = expect_status(send(request).await?, &[200]).await?;
        let seq = response
            .headers()
            .get(SNAPSHOT_SEQ_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or_else(|| RelayError::Protocol("snapshot seq".into()))?;
        let bytes = read_capped(response, MAX_SNAPSHOT_BODY).await?;
        Ok(SnapshotBlob { seq, bytes })
    }

    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        let url = self.vault_url(vault, "")?;
        let request = self.vault_request(Method::DELETE, url, vault)?;
        expect_status(send(request).await?, &[204]).await?;
        Ok(())
    }

    async fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> RelayResult<Rotated> {
        let url = self.vault_url(vault, "/token")?;
        if !is_base64url(new_token, 43) {
            return Err(RelayError::BadRequest("invalid_token".into()));
        }
        if bundle.is_empty() {
            return Err(RelayError::BadRequest("invalid_rekey".into()));
        }
        if bundle.len() > MAX_REKEY_BUNDLE {
            return Err(RelayError::BadRequest("rekey_too_large".into()));
        }
        // Standard base64 with padding: the server checks `length % 4`.
        let body = RotateWire {
            token: new_token,
            rekey: STANDARD.encode(bundle),
        };
        let request = self.vault_request(Method::POST, url, vault)?.json(&body);
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: RotatedWire = read_json(response, MAX_JSON_BODY).await?;
        Ok(Rotated {
            rotation: answer.rotation,
            head: answer.head,
        })
    }

    async fn rekeys(&self, vault: &VaultAuth, after: u32, limit: u32) -> RelayResult<RekeyPage> {
        if limit == 0 || limit > MAX_REKEY_PAGE {
            return Err(RelayError::BadRequest("invalid_limit".into()));
        }
        let mut url = self.vault_url(vault, "/rekey")?;
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limit.to_string());
        let request = self.vault_request(Method::GET, url, vault)?;
        let response = expect_status(send(request).await?, &[200]).await?;
        // Up to 4 MiB of bundles, which base64 makes ~5.4 MiB, plus the JSON around them.
        let answer: RekeyListWire = read_json(response, MAX_PULL_BODY).await?;
        let rekeys = answer
            .rekeys
            .into_iter()
            .map(|rekey| {
                Ok(Rekey {
                    rotation: rekey.rotation,
                    head: rekey.head,
                    at: rekey.at,
                    bundle: decode_ct(&rekey.rekey)?,
                })
            })
            .collect::<RelayResult<Vec<_>>>()?;
        let page = RekeyPage {
            rekeys,
            more: answer.more,
        };
        check_rekey_page(&page, after, limit)?;
        Ok(page)
    }

    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        let url = self.blob_url(vault, blob)?;
        if sealed_len > MAX_BLOB_SEALED {
            return Err(RelayError::Protocol("body too large".into()));
        }
        // Streamed piece by piece under the announced length; never one buffer for the blob.
        let mismatch = Arc::new(AtomicBool::new(false));
        let measured = Measured {
            body,
            left: sealed_len,
            mismatch: Arc::clone(&mismatch),
            done: false,
        };
        let request = self
            .vault_request(Method::PUT, url, vault)?
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, sealed_len)
            .timeout(BLOB_TIMEOUT)
            .body(Body::wrap_stream(futures_util::stream::iter(measured)));
        let sent = send(request).await;
        if mismatch.load(Ordering::SeqCst) {
            return Err(RelayError::BadRequest("length_required".into()));
        }
        let response = expect_blob_status(sent?, &[200, 201]).await?;
        // `201 {"created":true}`, or `200 {"created":false}` for a blob the server already has.
        let created = response.status().as_u16() == 201;
        let answer: CreatedWire = read_json(response, MAX_JSON_BODY).await?;
        if answer.created != created {
            return Err(RelayError::Protocol("unexpected answer".into()));
        }
        Ok(())
    }

    async fn head_blob(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<Option<u64>> {
        let url = self.blob_url(vault, blob)?;
        let response = send(self.vault_request(Method::HEAD, url, vault)?).await?;
        // A HEAD answer has no body: a 404 without a code is a missing blob.
        if response.status().as_u16() == 404 && error_code(&response).is_none() {
            return Ok(None);
        }
        let response = match expect_blob_status(response, &[200]).await {
            Ok(response) => response,
            Err(RelayError::BlobNotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        // A proxy may drop `Content-Length` from a HEAD answer; the server's own header stays.
        let size = announced_length(&response)
            .or_else(|| digits_header(&response, BLOB_SIZE_HEADER))
            .ok_or_else(|| RelayError::Protocol("length".into()))?;
        if !is_blob_size(size) {
            return Err(RelayError::Protocol("unexpected answer".into()));
        }
        Ok(Some(size))
    }

    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        let url = self.blob_url(vault, blob)?;
        // The download client: a stalled body fails after a minute without a byte, not at the
        // end of `BLOB_TIMEOUT`.
        let request = no_redirect_download_client()
            .get(url)
            .header(AUTHORIZATION, bearer(vault)?)
            .timeout(BLOB_TIMEOUT);
        let mut response = expect_blob_status(send(request).await?, &[200]).await?;
        let sealed_len =
            announced_length(&response).ok_or_else(|| RelayError::Protocol("length".into()))?;
        if sealed_len > MAX_BLOB_SEALED {
            return Err(RelayError::Protocol("body too large".into()));
        }
        if sink.begin(sealed_len) == SinkFlow::Stop {
            return Ok(Delivery::Stopped);
        }
        // Each piece goes to the sink as it arrives; the transport holds the body to its
        // announced length (a short body is a `Network` error).
        while let Some(piece) = response.chunk().await.map_err(network)? {
            if sink.piece(&piece) == SinkFlow::Stop {
                return Ok(Delivery::Stopped);
            }
        }
        Ok(Delivery::Complete)
    }

    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        if !(1..=MAX_BLOB_PAGE).contains(&limit) {
            return Err(RelayError::Protocol("limit".into()));
        }
        let mut url = self.vault_url(vault, "/blobs")?;
        {
            let mut query = url.query_pairs_mut();
            if let Some(after) = after {
                query.append_pair("after", &after.hyphenated().to_string());
            }
            query.append_pair("limit", &limit.to_string());
        }
        let request = self.vault_request(Method::GET, url, vault)?;
        let response = expect_blob_status(send(request).await?, &[200]).await?;
        let answer: BlobListWire = read_json(response, MAX_BLOB_LIST_BODY).await?;
        let blobs = answer
            .blobs
            .into_iter()
            .map(|blob| {
                Ok(RemoteBlob {
                    id: canonical_blob_id(&blob.id)
                        .ok_or_else(|| RelayError::Protocol("unexpected answer".into()))?,
                    size: blob.size,
                    created_at: blob.created_at,
                })
            })
            .collect::<RelayResult<Vec<_>>>()?;
        // An integer is the server's clock (a negative one is refused below); anything else, or
        // no field, is no clock.
        let server_now = answer.now.as_ref().and_then(serde_json::Value::as_i64);
        let page = BlobPage {
            blobs,
            more: answer.more,
            server_now,
        };
        check_blob_page(&page, after)?;
        Ok(page)
    }

    async fn delete_blob(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<()> {
        let url = self.blob_url(vault, blob)?;
        let request = self.vault_request(Method::DELETE, url, vault)?;
        match expect_blob_status(send(request).await?, &[204]).await {
            // The server answers 204 for a missing blob too; a 404 for it means the same.
            Ok(_) | Err(RelayError::BlobNotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

impl Mailbox for HttpRelay {
    async fn put_slot(&self, mailbox: &str, slot: u8, bytes: &[u8]) -> RelayResult<()> {
        let url = self.pairing_url(mailbox, Some(slot))?;
        if bytes.is_empty() {
            return Err(RelayError::BadRequest("empty_body".into()));
        }
        if bytes.len() > MAX_SLOT_BYTES {
            return Err(RelayError::BadRequest("body_too_large".into()));
        }
        // No account header and no token: the mailbox takes no credentials.
        let request = no_redirect_client()
            .put(url)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(bytes.to_vec());
        expect_status(send(request).await?, &[204]).await?;
        Ok(())
    }

    async fn take_slot(&self, mailbox: &str, slot: u8) -> RelayResult<Option<Vec<u8>>> {
        let url = self.pairing_url(mailbox, Some(slot))?;
        let response = match expect_status(send(no_redirect_client().get(url)).await?, &[200]).await
        {
            Ok(response) => response,
            Err(RelayError::SlotEmpty) => return Ok(None),
            Err(error) => return Err(error),
        };
        let bytes = read_capped(response, MAX_SLOT_BYTES).await?;
        if bytes.is_empty() {
            return Err(RelayError::Protocol("unexpected answer".into()));
        }
        Ok(Some(bytes))
    }

    async fn burn_mailbox(&self, mailbox: &str) -> RelayResult<()> {
        let url = self.pairing_url(mailbox, None)?;
        expect_status(send(no_redirect_client().delete(url)).await?, &[204]).await?;
        Ok(())
    }
}

// These talk to one-shot servers on 127.0.0.1 over plain http, which every build accepts for a
// server that is not the Moony cloud (`scheme_allowed`). They stay in debug test builds, the ones
// `cargo test` makes, so a release test run needs no loopback port.
#[cfg(all(test, debug_assertions))]
mod blob_tests;
#[cfg(all(test, debug_assertions))]
mod mailbox_tests;
#[cfg(test)]
mod scheme_tests;
#[cfg(all(test, debug_assertions))]
mod tests;
