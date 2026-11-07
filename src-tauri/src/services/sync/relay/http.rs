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
    AccountStatus, CheckoutPlan, OutgoingOp, PullPage, PulledOp, PushReceipt, Relay, RelayError,
    RelayResult, SnapshotBlob, VaultInfo,
};
use crate::error::{AppError, Result};
use crate::services::http::no_redirect_client;
use crate::services::sync::keys::VaultAuth;

/// The relay of release builds (a placeholder until phase 6).
pub const DEFAULT_RELAY_URL: &str = "https://sync.moonyfinance.com";
/// Account, vault and push answers, and every error answer.
pub const MAX_JSON_BODY: usize = 64 * 1024;
/// A pull page: the relay cuts a page at ~4 MiB of ciphertext, which base64 makes ~5.4 MiB.
pub const MAX_PULL_BODY: usize = 6 * 1024 * 1024;
/// A sealed snapshot: the relay's 100 MiB upload limit.
pub const MAX_SNAPSHOT_BODY: usize = 104_857_600;
/// One snapshot upload or download, end to end.
pub const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(600);

const ACCOUNT_HEADER: &str = "moony-account";
const SNAPSHOT_SEQ_HEADER: &str = "moony-snapshot-seq";

/// `MOONY_RELAY_URL` in debug builds when it is set, else `DEFAULT_RELAY_URL`. Release builds
/// never read the variable.
pub fn default_base_url() -> String {
    #[cfg(debug_assertions)]
    if let Some(url) = std::env::var("MOONY_RELAY_URL")
        .ok()
        .filter(|url| !url.is_empty())
    {
        return url;
    }
    DEFAULT_RELAY_URL.to_string()
}

/// A relay base URL: `https`, or `http` only for the hosts `localhost` and `127.0.0.1`; no user,
/// password, query or fragment; no path but `/`. Parsed, never prefix-matched.
pub fn validate_base_url(url: &str) -> Result<()> {
    parse_base_url(url).map(|_| ())
}

fn parse_base_url(url: &str) -> Result<Url> {
    let invalid = || AppError::Validation("validation.syncRelayUrlInvalid".into());
    let parsed = Url::parse(url).map_err(|_| invalid())?;
    let host_allowed = match parsed.scheme() {
        "https" => parsed.host_str().is_some_and(|host| !host.is_empty()),
        "http" => matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")),
        _ => false,
    };
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
    let mut body = Vec::with_capacity(announced as usize);
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
        Ok(answer.url)
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
        let request = self.vault_request(Method::POST, url, vault)?.json(&body);
        let response = expect_status(send(request).await?, &[200]).await?;
        let answer: PushAnswerWire = read_json(response, MAX_JSON_BODY).await?;
        Ok(PushReceipt {
            head: answer.head,
            accepted: answer.accepted,
        })
    }

    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        let mut url = self.vault_url(vault, "/ops")?;
        url.query_pairs_mut()
            .append_pair("after", &after.to_string())
            .append_pair("limit", &limit.to_string());
        let request = self.vault_request(Method::GET, url, vault)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{ErrorKind, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread::JoinHandle;

    const VAULT_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
    const TOKEN: &str = "dG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9";
    const ACCOUNT: &str = "1234567890123456";

    fn vault() -> VaultAuth {
        VaultAuth {
            vault_id: VAULT_ID.into(),
            token: Zeroizing::new(TOKEN.into()),
        }
    }

    /// What the one-shot server received.
    struct Request {
        head: String,
        body: Vec<u8>,
    }

    impl Request {
        fn request_line(&self) -> &str {
            self.head.lines().next().unwrap_or_default()
        }

        fn header(&self, name: &str) -> Option<String> {
            let wanted = format!("{}:", name.to_ascii_lowercase());
            self.head.lines().find_map(|line| {
                line.to_ascii_lowercase()
                    .starts_with(&wanted)
                    .then(|| line[wanted.len()..].trim().to_string())
            })
        }
    }

    fn read_request(stream: &mut TcpStream) -> Request {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            let n = stream.read(&mut chunk).expect("read request");
            assert!(n > 0, "the client closed before the request head ended");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut request = Request {
            head,
            body: buf[head_end..].to_vec(),
        };
        let length: usize = request
            .header("content-length")
            .map_or(0, |v| v.parse().expect("content length"));
        while request.body.len() < length {
            let n = stream.read(&mut chunk).expect("read body");
            assert!(n > 0, "the client closed before the body ended");
            request.body.extend_from_slice(&chunk[..n]);
        }
        request
    }

    /// Accepts one connection, answers `response` and hands back what it received.
    fn serve_once(response: Vec<u8>) -> (String, JoinHandle<Request>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            // A client that stops reading a body over the cap keeps the connection open; the
            // server thread gives up writing instead of blocking forever.
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(1)))
                .expect("write timeout");
            let request = read_request(&mut stream);
            // The client may hang up mid-answer (a cut-off body); that is the point of the test.
            let _ = stream.write_all(&response);
            let _ = stream.flush();
            request
        });
        (format!("http://127.0.0.1:{port}"), server)
    }

    fn answer(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
        for (name, value) in headers {
            out.push_str(&format!("{name}: {value}\r\n"));
        }
        if !headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        {
            out.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn json_answer(status: &str, body: &str) -> Vec<u8> {
        answer(
            status,
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        )
    }

    fn relay(base: &str) -> HttpRelay {
        HttpRelay::new(base).expect("relay")
    }

    #[test]
    fn base_urls_are_parsed_not_prefix_matched() {
        for ok in [
            "https://sync.moonyfinance.com",
            "https://sync.moonyfinance.com/",
            "http://127.0.0.1:8787",
            "http://localhost:8787/",
        ] {
            assert!(validate_base_url(ok).is_ok(), "{ok} is accepted");
            assert!(HttpRelay::new(ok).is_ok(), "{ok} builds a relay");
        }
        for refused in [
            "http://sync.moonyfinance.com",
            "http://localhost.evil.com",
            "http://localhost@evil.com",
            "http://127.0.0.1.evil.com",
            "https://user:pw@sync.moonyfinance.com",
            "https://user@sync.moonyfinance.com",
            "https://x/?q=1",
            "https://x/?",
            "https://x/#top",
            "https://x/relay",
            "ftp://x",
            "sync.moonyfinance.com",
            "",
        ] {
            let error = validate_base_url(refused).expect_err(refused);
            assert!(
                matches!(&error, AppError::Validation(k) if k == "validation.syncRelayUrlInvalid"),
                "{refused}: {error:?}"
            );
            assert!(
                HttpRelay::new(refused).is_err(),
                "{refused} builds no relay"
            );
        }
    }

    #[test]
    fn the_release_relay_is_the_default() {
        assert_eq!(DEFAULT_RELAY_URL, "https://sync.moonyfinance.com");
        if std::env::var_os("MOONY_RELAY_URL").is_none() {
            assert_eq!(default_base_url(), DEFAULT_RELAY_URL);
        }
        assert_eq!(MAX_JSON_BODY, 65_536);
        assert_eq!(MAX_PULL_BODY, 6_291_456);
        assert_eq!(MAX_SNAPSHOT_BODY, 104_857_600);
        assert_eq!(SNAPSHOT_TIMEOUT, Duration::from_secs(600));
    }

    #[test]
    fn a_relay_prints_only_its_origin() {
        let relay = relay("http://127.0.0.1:8787/");
        assert_eq!(
            format!("{relay:?}"),
            r#"HttpRelay { base: "http://127.0.0.1:8787" }"#
        );
    }

    #[tokio::test]
    async fn an_html_answer_is_a_protocol_error() {
        let (base, server) = serve_once(answer(
            "200 OK",
            &[("Content-Type", "text/html")],
            b"<html><body>Sign in to the hotel Wi-Fi</body></html>",
        ));
        let error = relay(&base).pull(&vault(), 0, 500).await.expect_err("html");
        assert_eq!(error, RelayError::Protocol("not json".into()));
        server.join().expect("server");

        let (base, server) = serve_once(answer(
            "502 Bad Gateway",
            &[("Content-Type", "text/html")],
            b"<html>bad gateway</html>",
        ));
        let error = relay(&base).pull(&vault(), 0, 500).await.expect_err("502");
        assert_eq!(error, RelayError::Server(502));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let elsewhere = TcpListener::bind("127.0.0.1:0").expect("bind");
        let elsewhere_port = elsewhere.local_addr().expect("addr").port();
        elsewhere.set_nonblocking(true).expect("nonblocking");
        let location = format!("http://127.0.0.1:{elsewhere_port}/v1/accounts/me");
        let (base, server) = serve_once(answer("302 Found", &[("Location", &location)], b""));

        let error = relay(&base)
            .account_status(ACCOUNT)
            .await
            .expect_err("a redirect is an error");
        assert_eq!(error, RelayError::Protocol("redirect".into()));
        let request = server.join().expect("server");
        assert_eq!(request.header("moony-account").as_deref(), Some(ACCOUNT));

        match elsewhere.accept() {
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            other => panic!("the redirect target was contacted: {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_oversized_body_is_cut_off() {
        let too_large = RelayError::Protocol("body too large".into());
        let page = vec![b' '; MAX_PULL_BODY + 1];

        // Announced by Content-Length: refused before reading.
        let (base, server) = serve_once(answer("200 OK", &[], &page));
        let error = relay(&base)
            .pull(&vault(), 0, 500)
            .await
            .expect_err("too large");
        assert_eq!(error, too_large);
        server.join().expect("server");

        // No Content-Length (the body runs until the connection closes): cut off while reading.
        let mut unannounced = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
        unannounced.extend_from_slice(&page);
        let (base, server) = serve_once(unannounced);
        let error = relay(&base)
            .pull(&vault(), 0, 500)
            .await
            .expect_err("too large");
        assert_eq!(error, too_large);
        server.join().expect("server");
    }

    #[tokio::test]
    async fn a_snapshot_over_the_cap_is_refused_before_it_is_read() {
        // The relay announces one byte more than a snapshot may have and sends none of it: the
        // cap holds before a single byte reaches the caller (and `open_stream`).
        let length = (MAX_SNAPSHOT_BODY + 1).to_string();
        let (base, server) = serve_once(answer(
            "200 OK",
            &[("Content-Length", &length), ("Moony-Snapshot-Seq", "7")],
            b"",
        ));
        let error = relay(&base)
            .get_snapshot(&vault())
            .await
            .expect_err("too large");
        assert_eq!(error, RelayError::Protocol("body too large".into()));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn network_errors_carry_no_url() {
        // A port nobody listens on: bind, note the port, close.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let relay = relay(&format!("http://127.0.0.1:{port}"));
        let errors = [
            relay.vault_info(&vault()).await.expect_err("closed port"),
            relay.pull(&vault(), 3, 500).await.expect_err("closed port"),
            relay
                .account_status(ACCOUNT)
                .await
                .expect_err("closed port"),
        ];
        for error in errors {
            assert_eq!(error, RelayError::Network("connect".into()));
            let sync = crate::services::sync::error::SyncError::from(error.clone());
            for text in [
                error.to_string(),
                format!("{error:?}"),
                sync.to_string(),
                format!("{sync:?}"),
            ] {
                for secret in [VAULT_ID, "/v1/", ACCOUNT, TOKEN, "127.0.0.1"] {
                    assert!(!text.contains(secret), "{text:?} contains {secret:?}");
                }
            }
        }
    }

    #[tokio::test]
    async fn push_sends_bearer_json_and_base64_ops() {
        let (base, server) = serve_once(json_answer("200 OK", r#"{"head":12,"accepted":2}"#));
        let ops = [
            OutgoingOp {
                id: "op-1".into(),
                ct: vec![0, 1, 2, 250, 251, 252],
            },
            OutgoingOp {
                id: "op_2".into(),
                ct: vec![0xff; 4],
            },
        ];
        let receipt = relay(&base).push(&vault(), &ops).await.expect("push");
        assert_eq!(
            receipt,
            PushReceipt {
                head: 12,
                accepted: 2
            }
        );

        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("POST /v1/vaults/{VAULT_ID}/ops HTTP/1.1")
        );
        assert_eq!(
            request.header("authorization"),
            Some(format!("Bearer {TOKEN}"))
        );
        assert_eq!(request.header("moony-account"), None);
        assert_eq!(
            request.header("content-type").as_deref(),
            Some("application/json")
        );
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
        assert_eq!(
            body,
            serde_json::json!({"ops": [
                {"id": "op-1", "ct": "AAEC+vv8"},
                {"id": "op_2", "ct": "/////w=="},
            ]})
        );
        assert_eq!(STANDARD.encode([0, 1, 2, 250, 251, 252]), "AAEC+vv8");
    }

    #[tokio::test]
    async fn pull_sends_after_and_limit_and_decodes_ops() {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"ops":[{"seq":4,"id":"a","ct":"AAEC+vv8"},{"seq":5,"id":"b","ct":"/////w=="}],"head":9,"more":true}"#,
        ));
        let page = relay(&base).pull(&vault(), 3, 2).await.expect("pull");
        assert_eq!(
            page,
            PullPage {
                ops: vec![
                    PulledOp {
                        seq: 4,
                        id: "a".into(),
                        ct: vec![0, 1, 2, 250, 251, 252],
                    },
                    PulledOp {
                        seq: 5,
                        id: "b".into(),
                        ct: vec![0xff; 4],
                    },
                ],
                head: 9,
                more: true,
            }
        );
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("GET /v1/vaults/{VAULT_ID}/ops?after=3&limit=2 HTTP/1.1")
        );
        assert_eq!(
            request.header("authorization"),
            Some(format!("Bearer {TOKEN}"))
        );

        // Ciphertext that is not standard padded base64 is the relay's fault.
        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"ops":[{"seq":4,"id":"a","ct":"AAEC-vv8"}],"head":4,"more":false}"#,
        ));
        let error = relay(&base)
            .pull(&vault(), 3, 2)
            .await
            .expect_err("base64url");
        assert_eq!(error, RelayError::Protocol("bad base64".into()));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn create_vault_sends_the_account_header_and_reports_creation() {
        for (status, created) in [("201 Created", true), ("200 OK", false)] {
            let body = format!(r#"{{"created":{created}}}"#);
            let (base, server) = serve_once(json_answer(status, &body));
            let answer = relay(&base)
                .create_vault(ACCOUNT, &vault())
                .await
                .expect("create");
            assert_eq!(answer, created, "{status}");
            let request = server.join().expect("server");
            assert_eq!(
                request.request_line(),
                format!("PUT /v1/vaults/{VAULT_ID} HTTP/1.1")
            );
            assert_eq!(request.header("moony-account").as_deref(), Some(ACCOUNT));
            assert_eq!(
                request.header("authorization"),
                Some(format!("Bearer {TOKEN}"))
            );
        }

        let (base, server) = serve_once(json_answer(
            "403 Forbidden",
            r#"{"error":"vault_limit_reached"}"#,
        ));
        let error = relay(&base)
            .create_vault(ACCOUNT, &vault())
            .await
            .expect_err("limit");
        assert_eq!(error, RelayError::VaultLimitReached);
        server.join().expect("server");
    }

    #[tokio::test]
    async fn get_snapshot_reads_the_sequence_header() {
        let sealed = b"sealed snapshot bytes".to_vec();
        let (base, server) = serve_once(answer(
            "200 OK",
            &[
                ("Content-Type", "application/octet-stream"),
                ("Moony-Snapshot-Seq", "42"),
            ],
            &sealed,
        ));
        let blob = relay(&base).get_snapshot(&vault()).await.expect("snapshot");
        assert_eq!(blob.seq, 42);
        assert_eq!(blob.bytes, sealed);
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("GET /v1/vaults/{VAULT_ID}/snapshot HTTP/1.1")
        );

        // Without the header the bytes cannot be placed: the relay broke the contract.
        let (base, server) = serve_once(answer("200 OK", &[], &sealed));
        let error = relay(&base)
            .get_snapshot(&vault())
            .await
            .expect_err("no seq");
        assert_eq!(error, RelayError::Protocol("snapshot seq".into()));
        server.join().expect("server");

        let (base, server) = serve_once(json_answer("404 Not Found", r#"{"error":"no_snapshot"}"#));
        let error = relay(&base).get_snapshot(&vault()).await.expect_err("none");
        assert_eq!(error, RelayError::NoSnapshot);
        server.join().expect("server");
    }

    #[tokio::test]
    async fn put_snapshot_sends_raw_bytes_with_the_sequence() {
        let (base, server) = serve_once(json_answer("201 Created", r#"{"seq":42}"#));
        relay(&base)
            .put_snapshot(&vault(), 42, vec![9, 8, 7])
            .await
            .expect("upload");
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("PUT /v1/vaults/{VAULT_ID}/snapshot?seq=42 HTTP/1.1")
        );
        assert_eq!(
            request.header("content-type").as_deref(),
            Some("application/octet-stream")
        );
        assert_eq!(request.header("content-length").as_deref(), Some("3"));
        assert_eq!(request.body, vec![9, 8, 7]);
    }

    #[tokio::test]
    async fn checkout_posts_the_plan_and_returns_the_url() {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"url":"https://buy.polar.sh/checkout/abc"}"#,
        ));
        let url = relay(&base)
            .checkout(ACCOUNT, CheckoutPlan::Yearly)
            .await
            .expect("checkout");
        assert_eq!(url, "https://buy.polar.sh/checkout/abc");
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            "POST /v1/accounts/me/checkout HTTP/1.1"
        );
        assert_eq!(request.header("moony-account").as_deref(), Some(ACCOUNT));
        assert_eq!(request.header("authorization"), None);
        let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json body");
        assert_eq!(body, serde_json::json!({"plan": "yearly"}));

        // 502 / 503 of the checkout route: the payment provider is unavailable.
        for status in ["502 Bad Gateway", "503 Service Unavailable"] {
            let (base, server) = serve_once(json_answer(status, r#"{"error":"checkout_failed"}"#));
            let error = relay(&base)
                .checkout(ACCOUNT, CheckoutPlan::Monthly)
                .await
                .expect_err(status);
            assert_eq!(error, RelayError::CheckoutUnavailable, "{status}");
            server.join().expect("server");
        }
    }

    #[tokio::test]
    async fn account_calls_send_and_read_the_documented_shapes() {
        let (base, server) = serve_once(json_answer(
            "201 Created",
            r#"{"account":"1234567890123456"}"#,
        ));
        assert_eq!(
            relay(&base).create_account().await.expect("create"),
            ACCOUNT
        );
        let request = server.join().expect("server");
        assert_eq!(request.request_line(), "POST /v1/accounts HTTP/1.1");
        assert_eq!(request.header("moony-account"), None);

        // An "account number" that is not 16 digits is not stored.
        let (base, server) = serve_once(json_answer("201 Created", r#"{"account":"../evil"}"#));
        let error = relay(&base).create_account().await.expect_err("bad number");
        assert_eq!(error, RelayError::Protocol("unexpected answer".into()));
        server.join().expect("server");

        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"paidUntil":1800000000,"active":true,"vaults":1}"#,
        ));
        let status = relay(&base).account_status(ACCOUNT).await.expect("status");
        assert_eq!(
            status,
            AccountStatus {
                paid_until: Some(1_800_000_000),
                active: true,
                vaults: 1,
            }
        );
        let request = server.join().expect("server");
        assert_eq!(request.request_line(), "GET /v1/accounts/me HTTP/1.1");
        assert_eq!(request.header("moony-account").as_deref(), Some(ACCOUNT));

        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"paidUntil":null,"active":false,"vaults":0}"#,
        ));
        let status = relay(&base).account_status(ACCOUNT).await.expect("status");
        assert_eq!(status.paid_until, None);
        server.join().expect("server");
    }

    #[tokio::test]
    async fn vault_info_and_delete_use_the_bearer_token() {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            r#"{"head":10,"floor":2,"snapshotSeq":8,"paidUntil":1800000000,"bytes":4096,"rotation":0}"#,
        ));
        let info = relay(&base).vault_info(&vault()).await.expect("info");
        assert_eq!(
            info,
            VaultInfo {
                head: 10,
                floor: 2,
                snapshot_seq: Some(8),
                paid_until: 1_800_000_000,
                bytes: 4096,
                rotation: 0,
            }
        );
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("GET /v1/vaults/{VAULT_ID} HTTP/1.1")
        );
        assert_eq!(
            request.header("authorization"),
            Some(format!("Bearer {TOKEN}"))
        );

        let (base, server) = serve_once(answer("204 No Content", &[], b""));
        relay(&base).delete_vault(&vault()).await.expect("delete");
        let request = server.join().expect("server");
        assert_eq!(
            request.request_line(),
            format!("DELETE /v1/vaults/{VAULT_ID} HTTP/1.1")
        );
        assert_eq!(
            request.header("authorization"),
            Some(format!("Bearer {TOKEN}"))
        );

        // A captive portal's 200 page is not a deletion.
        let (base, server) = serve_once(answer("200 OK", &[], b"<html>welcome</html>"));
        let error = relay(&base)
            .delete_vault(&vault())
            .await
            .expect_err("not 204");
        assert_eq!(error, RelayError::Protocol("unexpected status".into()));
        server.join().expect("server");
    }

    #[tokio::test]
    async fn malformed_credentials_never_reach_the_wire() {
        // Nobody listens: a request that went out would be a Network error.
        let relay = relay("http://127.0.0.1:9");
        let traversal = VaultAuth {
            vault_id: "../accounts/me".into(),
            token: Zeroizing::new(TOKEN.into()),
        };
        assert_eq!(
            relay.vault_info(&traversal).await.expect_err("bad id"),
            RelayError::BadRequest("invalid_vault_id".into())
        );
        let newline = VaultAuth {
            vault_id: VAULT_ID.into(),
            token: Zeroizing::new("x\r\nMoony-Account: 1".into()),
        };
        assert_eq!(
            relay.vault_info(&newline).await.expect_err("bad token"),
            RelayError::BadRequest("invalid_token".into())
        );
        assert_eq!(
            relay
                .account_status("12345")
                .await
                .expect_err("bad account"),
            RelayError::BadRequest("missing_account".into())
        );
    }
}
