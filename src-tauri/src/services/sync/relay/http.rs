//! `HttpRelay`: the relay contract over HTTPS (ruling P7). Every URL is built from one parsed
//! base, every request goes through `services::http::no_redirect_client()`, so the
//! `Moony-Account` header and the bearer token never reach another origin; every body is read
//! under a cap; every error is typed and carries no URL or body.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Method, RequestBuilder, Response, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{
    AccountStatus, CheckoutPlan, Mailbox, OutgoingOp, PullPage, PulledOp, PushReceipt, Relay,
    RelayError, RelayResult, ServerInfo, ServerMode, SnapshotBlob, VaultInfo, MAX_SLOT_BYTES,
};
use crate::error::{AppError, Result};
use crate::services::http::no_redirect_client;
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
/// The most a body buffer reserves up front, whatever length the answer announces.
const MAX_PREALLOCATION: usize = 1024 * 1024;

const ACCOUNT_HEADER: &str = "moony-account";
const SNAPSHOT_SEQ_HEADER: &str = "moony-snapshot-seq";

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
    // `sync.moonyfinance.com.` is the same host as `sync.moonyfinance.com`: one trailing dot of a
    // domain goes, so both spell (and compare as) one address.
    if let Some(bare) = parsed.domain().and_then(|d| d.strip_suffix('.')) {
        let bare = bare.to_string();
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
}

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

// These talk to one-shot servers on 127.0.0.1, an address only a debug build accepts.
#[cfg(all(test, debug_assertions))]
mod mailbox_tests;
#[cfg(test)]
mod scheme_tests;
#[cfg(all(test, debug_assertions))]
mod tests;
