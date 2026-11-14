//! Tests of `HttpRelay` against one-shot local servers.

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
pub(super) struct Request {
    pub(super) head: String,
    pub(super) body: Vec<u8>,
}

impl Request {
    pub(super) fn request_line(&self) -> &str {
        self.head.lines().next().unwrap_or_default()
    }

    pub(super) fn header(&self, name: &str) -> Option<String> {
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
pub(super) fn serve_once(response: Vec<u8>) -> (String, JoinHandle<Request>) {
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

pub(super) fn answer(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
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

pub(super) fn json_answer(status: &str, body: &str) -> Vec<u8> {
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
        "http://localhost.evil.com",
        "http://127.0.0.1.evil.com",
    ] {
        assert!(validate_base_url(ok).is_ok(), "{ok} is accepted");
        assert!(HttpRelay::new(ok).is_ok(), "{ok} builds a relay");
    }
    for refused in [
        "http://sync.moonyfinance.com",
        "http://localhost@evil.com",
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
    // Unless a debug build points at a dev sync server, at run time or baked in at build time.
    if crate::services::sync::feature::dev_relay_base().is_none() {
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

#[test]
fn push_and_pull_get_the_longer_ops_timeout() {
    assert_eq!(OPS_TIMEOUT, Duration::from_secs(120));
    let relay = relay("https://sync.moonyfinance.com");
    let ops = [OutgoingOp {
        id: "op-1".into(),
        ct: vec![1, 2, 3],
    }];
    let push = relay
        .push_request(&vault(), &ops)
        .expect("push request")
        .build()
        .expect("build push");
    assert_eq!(push.timeout(), Some(&OPS_TIMEOUT));
    let pull = relay
        .pull_request(&vault(), 3, 500)
        .expect("pull request")
        .build()
        .expect("build pull");
    assert_eq!(pull.timeout(), Some(&OPS_TIMEOUT));

    // Every other vault call keeps the shared client's 15 s bound (no per-request override).
    let url = relay.vault_url(&vault(), "").expect("url");
    let info = relay
        .vault_request(Method::GET, url, &vault())
        .expect("info request")
        .build()
        .expect("build info");
    assert_eq!(info.timeout(), None);
}

#[tokio::test]
async fn checkout_accepts_only_an_https_url() {
    for url in [
        "http://buy.polar.sh/checkout/abc",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "moony://open",
        "not a url",
        "",
    ] {
        let body = serde_json::json!({ "url": url }).to_string();
        let (base, server) = serve_once(json_answer("200 OK", &body));
        let error = relay(&base)
            .checkout(ACCOUNT, CheckoutPlan::Monthly)
            .await
            .expect_err(url);
        assert_eq!(
            error,
            RelayError::Protocol("unexpected answer".into()),
            "{url}"
        );
        assert!(!error.to_string().contains(url) || url.is_empty(), "{url}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn a_self_hosted_relay_answers_checkout_without_a_url() {
    let (base, server) = serve_once(json_answer("409 Conflict", r#"{"error":"self_hosted"}"#));
    let error = relay(&base)
        .checkout(ACCOUNT, CheckoutPlan::Monthly)
        .await
        .expect_err("a self-hosted relay takes no payments");
    assert_eq!(error, RelayError::SelfHosted);
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        "POST /v1/accounts/me/checkout HTTP/1.1"
    );
}

#[test]
fn base_urls_are_normalized_to_their_origin() {
    for (given, normal) in [
        (
            "https://sync.moonyfinance.com",
            "https://sync.moonyfinance.com",
        ),
        (
            "https://sync.moonyfinance.com/",
            "https://sync.moonyfinance.com",
        ),
        (
            "https://Sync.MoonyFinance.com:443/",
            "https://sync.moonyfinance.com",
        ),
        (
            "https://relay.example.org:8443",
            "https://relay.example.org:8443",
        ),
        ("http://localhost:8787/", "http://localhost:8787"),
        ("http://127.0.0.1:8787", "http://127.0.0.1:8787"),
    ] {
        assert_eq!(normalize_base_url(given).expect(given), normal, "{given}");
    }
    assert_eq!(
        normalize_base_url("http://relay.example.org").expect("plain http"),
        "http://relay.example.org"
    );
    assert!(is_default_relay("https://sync.moonyfinance.com/"));
    assert!(is_default_relay(DEFAULT_RELAY_URL));
    assert!(!is_default_relay("https://relay.example.org"));
    assert!(!is_default_relay("not a url"));
}

#[test]
fn a_trailing_dot_on_the_host_is_dropped() {
    assert_eq!(
        normalize_base_url("https://sync.moonyfinance.com.").expect("dotted"),
        DEFAULT_RELAY_URL
    );
    assert_eq!(
        normalize_base_url("https://relay.example.org.:8443/").expect("dotted with port"),
        "https://relay.example.org:8443"
    );
    assert!(is_default_relay("https://sync.moonyfinance.com."));
    assert!(is_default_relay("https://SYNC.moonyfinance.com./"));
}

#[test]
fn a_relay_address_over_256_characters_is_refused() {
    let host = |len: usize| {
        let mut host = String::new();
        while host.len() < len {
            if !host.is_empty() {
                host.push('.');
            }
            let label = (len - host.len()).min(50);
            host.push_str(&"r".repeat(label));
        }
        host
    };
    let longest = format!("https://{}", host(248));
    assert_eq!(longest.len(), 256);
    assert_eq!(normalize_base_url(&longest).expect("256"), longest);
    assert!(validate_base_url(&longest).is_ok());

    let over = format!("https://{}", host(249));
    assert_eq!(over.len(), 257);
    // The cap counts what was typed: a trailing `/` past 256 is refused too.
    for refused in [over, format!("{longest}/")] {
        let error = normalize_base_url(&refused).expect_err("over 256");
        assert!(
            matches!(&error, AppError::Validation(k) if k == "validation.syncRelayUrlInvalid"),
            "{error:?}"
        );
        assert!(validate_base_url(&refused).is_err());
        assert!(HttpRelay::new(&refused).is_err());
    }
}

#[tokio::test]
async fn server_info_reads_the_documented_answer() {
    let (base, server) = serve_once(json_answer(
        "200 OK",
        r#"{"name":"moony-sync","version":"1.0.0","protocol":2,"mode":"self_hosted"}"#,
    ));
    let info = relay(&base).server_info().await.expect("server info");
    assert_eq!(
        info,
        ServerInfo {
            version: "1.0.0".into(),
            protocol: 2,
            mode: ServerMode::SelfHosted,
        }
    );
    let request = server.join().expect("server");
    assert_eq!(request.request_line(), "GET /v1/server HTTP/1.1");
    assert_eq!(request.header("moony-account"), None);
    assert_eq!(request.header("authorization"), None);
}

#[tokio::test]
async fn server_info_refuses_what_is_not_a_moony_server() {
    async fn ask(response: Vec<u8>) -> RelayResult<ServerInfo> {
        let (base, server) = serve_once(response);
        let result = relay(&base).server_info().await;
        server.join().expect("server");
        result
    }
    let good = |name: &str, version: &str, mode: &str| {
        json_answer(
            "200 OK",
            &format!(r#"{{"name":"{name}","version":"{version}","protocol":2,"mode":"{mode}"}}"#),
        )
    };

    assert_eq!(
        ask(json_answer("404 Not Found", r#"{"error":"not_found"}"#)).await,
        Err(RelayError::BadRequest("not_found".into()))
    );
    assert_eq!(
        ask(answer("200 OK", &[], b"<html>hello</html>")).await,
        Err(RelayError::Protocol("not json".into()))
    );
    assert_eq!(
        ask(good("other", "1.0.0", "self_hosted")).await,
        Err(RelayError::Protocol("not a sync server".into()))
    );
    for version in ["", &"1".repeat(40), "1.0 0", "1.0/0"] {
        assert_eq!(
            ask(good("moony-sync", version, "self_hosted")).await,
            Err(RelayError::Protocol("version".into())),
            "{version:?}"
        );
    }
    assert_eq!(
        ask(good("moony-sync", "1.2.3-rc.1+b5", "cloud")).await,
        Ok(ServerInfo {
            version: "1.2.3-rc.1+b5".into(),
            protocol: 2,
            mode: ServerMode::Cloud,
        })
    );
    assert_eq!(
        ask(good("moony-sync", "1.0.0", "weird"))
            .await
            .map(|i| i.mode),
        Ok(ServerMode::Other)
    );
}
