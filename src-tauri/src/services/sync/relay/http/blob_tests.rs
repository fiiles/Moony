//! Tests of `HttpRelay`'s blob calls against one-shot local servers.

use super::tests::{answer, json_answer, serve_once};
use super::*;
use std::io::ErrorKind;
use std::net::TcpListener;
use uuid::Uuid;

use crate::services::sync::envelope::blob::MAX_BLOB_SEALED;
use crate::services::sync::relay::{BlobSink, Delivery, SinkFlow};

const VAULT_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
const TOKEN: &str = "dG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9";
const BLOB: &str = "0b1c2d3e-4f50-4617-8899-aabbccddeeff";

fn vault() -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(TOKEN.into()),
    }
}

fn blob() -> Uuid {
    Uuid::parse_str(BLOB).expect("uuid")
}

fn relay(base: &str) -> HttpRelay {
    HttpRelay::new(base).expect("relay")
}

fn pieces(parts: &[&[u8]]) -> BlobBody {
    let owned: Vec<std::io::Result<Vec<u8>>> = parts.iter().map(|p| Ok(p.to_vec())).collect();
    Box::new(owned.into_iter())
}

/// A sink that records what it was handed; it answers `Stop` from the `stop_at`-th piece on.
#[derive(Default)]
struct Recorder {
    begun: Vec<u64>,
    bytes: Vec<u8>,
    pieces: usize,
    stop_at: Option<usize>,
}

impl BlobSink for Recorder {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        self.begun.push(sealed_len);
        SinkFlow::Continue
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        self.pieces += 1;
        self.bytes.extend_from_slice(bytes);
        match self.stop_at {
            Some(at) if self.pieces >= at => SinkFlow::Stop,
            _ => SinkFlow::Continue,
        }
    }
}

/// A port nothing answers on, and a way to tell that nobody connected to it.
fn silent_port() -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    listener.set_nonblocking(true).expect("nonblocking");
    (format!("http://127.0.0.1:{port}"), listener)
}

fn assert_never_contacted(listener: &TcpListener) {
    match listener.accept() {
        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
        other => panic!("the server was contacted: {other:?}"),
    }
}

#[tokio::test]
async fn put_blob_streams_the_body_with_its_length_and_bearer() {
    let (base, server) = serve_once(json_answer("201 Created", r#"{"created":true}"#));
    relay(&base)
        .put_blob(&vault(), blob(), 9, pieces(&[b"abc", b"", b"defghi"]))
        .await
        .expect("put");
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("PUT /v1/vaults/{VAULT_ID}/blobs/{BLOB} HTTP/1.1")
    );
    assert_eq!(request.header("content-length").as_deref(), Some("9"));
    assert_eq!(request.header("transfer-encoding"), None);
    assert_eq!(
        request.header("content-type").as_deref(),
        Some("application/octet-stream")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );
    assert_eq!(request.body, b"abcdefghi");
}

#[tokio::test]
async fn an_existing_blob_answers_200_not_created() {
    let (base, server) = serve_once(json_answer("200 OK", r#"{"created":false}"#));
    relay(&base)
        .put_blob(&vault(), blob(), 3, pieces(&[b"abc"]))
        .await
        .expect("already there");
    server.join().expect("server");

    // A `created` that contradicts the status is outside the contract.
    let (base, server) = serve_once(json_answer("200 OK", r#"{"created":true}"#));
    let error = relay(&base)
        .put_blob(&vault(), blob(), 3, pieces(&[b"abc"]))
        .await
        .expect_err("contradiction");
    assert_eq!(error, RelayError::Protocol("unexpected answer".into()));
    server.join().expect("server");
}

#[tokio::test]
async fn a_body_over_the_blob_cap_is_refused_before_sending() {
    let (base, listener) = silent_port();
    let error = relay(&base)
        .put_blob(&vault(), blob(), MAX_BLOB_SEALED + 1, pieces(&[b"x"]))
        .await
        .expect_err("too large");
    assert_eq!(error, RelayError::Protocol("body too large".into()));
    assert_never_contacted(&listener);
}

#[tokio::test]
async fn a_body_that_does_not_match_its_length_is_refused() {
    // Shorter and longer than announced: the client stops the upload itself, whatever the
    // server would have answered.
    for (announced, parts) in [(10u64, &[&b"abc"[..]][..]), (2, &[&b"abc"[..]][..])] {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            // Accept and hold the connection; the answer never matters.
            let accepted = listener.accept();
            std::thread::sleep(Duration::from_millis(300));
            drop(accepted);
        });
        let error = relay(&format!("http://127.0.0.1:{port}"))
            .put_blob(&vault(), blob(), announced, pieces(parts))
            .await
            .expect_err("length mismatch");
        assert_eq!(
            error,
            RelayError::BadRequest("length_required".into()),
            "{announced}"
        );
        server.join().expect("server");
    }
}

#[tokio::test]
async fn a_body_that_fails_to_read_is_a_body_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let accepted = listener.accept();
        std::thread::sleep(Duration::from_millis(300));
        drop(accepted);
    });
    let failing: BlobBody =
        Box::new(vec![Ok(b"abc".to_vec()), Err(std::io::Error::other("disk gone"))].into_iter());
    let error = relay(&format!("http://127.0.0.1:{port}"))
        .put_blob(&vault(), blob(), 6, failing)
        .await
        .expect_err("read error");
    assert!(matches!(error, RelayError::Network(_)), "{error:?}");
    assert!(!format!("{error:?}").contains("disk gone"), "{error:?}");
    server.join().expect("server");
}

#[tokio::test]
async fn put_blob_answers_carry_their_codes() {
    async fn put(response: Vec<u8>) -> RelayError {
        let (base, server) = serve_once(response);
        let error = relay(&base)
            .put_blob(&vault(), blob(), 3, pieces(&[b"abc"]))
            .await
            .expect_err("refused");
        server.join().expect("server");
        error
    }
    let coded = |status: &str, code: &str| {
        answer(
            status,
            &[("Content-Type", "application/json"), ("Moony-Error", code)],
            format!(r#"{{"error":"{code}"}}"#).as_bytes(),
        )
    };
    assert_eq!(
        put(coded("507 Insufficient Storage", "server_full")).await,
        RelayError::InsufficientStorage
    );
    assert_eq!(
        put(coded("429 Too Many Requests", "too_many_uploads")).await,
        RelayError::RateLimited
    );
    assert_eq!(
        put(coded("413 Payload Too Large", "quota_exceeded")).await,
        RelayError::QuotaExceeded
    );
    assert_eq!(
        put(coded("413 Payload Too Large", "blob_too_large")).await,
        RelayError::BadRequest("blob_too_large".into())
    );
    assert_eq!(
        put(coded("403 Forbidden", "token_read_only")).await,
        RelayError::TokenReadOnly
    );
    // The header alone is enough (an edge may drop the body).
    assert_eq!(
        put(answer(
            "402 Payment Required",
            &[("Moony-Error", "payment_required")],
            b""
        ))
        .await,
        RelayError::PaymentRequired
    );
}

#[tokio::test]
async fn head_blob_reads_the_size_or_none() {
    let (base, server) = serve_once(answer(
        "200 OK",
        &[("Content-Length", "1234"), ("Moony-Blob-Size", "1234")],
        b"",
    ));
    assert_eq!(
        relay(&base).head_blob(&vault(), blob()).await,
        Ok(Some(1234))
    );
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("HEAD /v1/vaults/{VAULT_ID}/blobs/{BLOB} HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );

    // A HEAD answer has no body: the code is in the header.
    let (base, server) = serve_once(answer(
        "404 Not Found",
        &[("Moony-Error", "blob_not_found"), ("Content-Length", "26")],
        b"",
    ));
    assert_eq!(relay(&base).head_blob(&vault(), blob()).await, Ok(None));
    server.join().expect("server");

    // A 404 with no code at all is a missing blob too.
    let (base, server) = serve_once(answer("404 Not Found", &[], b""));
    assert_eq!(relay(&base).head_blob(&vault(), blob()).await, Ok(None));
    server.join().expect("server");

    // A missing vault is not a missing blob.
    let (base, server) = serve_once(answer(
        "404 Not Found",
        &[("Moony-Error", "vault_not_found")],
        b"",
    ));
    assert_eq!(
        relay(&base).head_blob(&vault(), blob()).await,
        Err(RelayError::VaultNotFound)
    );
    server.join().expect("server");

    let (base, server) = serve_once(answer(
        "402 Payment Required",
        &[("Moony-Error", "payment_required")],
        b"",
    ));
    assert_eq!(
        relay(&base).head_blob(&vault(), blob()).await,
        Err(RelayError::PaymentRequired)
    );
    server.join().expect("server");
}

#[tokio::test]
async fn get_blob_hands_every_byte_to_the_sink() {
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (base, server) = serve_once(answer(
        "200 OK",
        &[("Content-Type", "application/octet-stream")],
        &body,
    ));
    let mut sink = Recorder::default();
    let delivery = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect("get");
    assert_eq!(delivery, Delivery::Complete);
    assert_eq!(sink.begun, vec![body.len() as u64]);
    assert_eq!(sink.bytes, body);
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("GET /v1/vaults/{VAULT_ID}/blobs/{BLOB} HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );
}

#[tokio::test]
async fn get_blob_without_a_length_is_a_protocol_error() {
    let chunked =
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
            .to_vec();
    let (base, server) = serve_once(chunked);
    let mut sink = Recorder::default();
    let error = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect_err("no length");
    assert_eq!(error, RelayError::Protocol("length".into()));
    assert!(sink.begun.is_empty());
    assert!(sink.bytes.is_empty());
    server.join().expect("server");
}

#[tokio::test]
async fn get_blob_over_the_cap_is_refused_before_reading() {
    let length = (MAX_BLOB_SEALED + 1).to_string();
    let (base, server) = serve_once(answer("200 OK", &[("Content-Length", &length)], b""));
    let mut sink = Recorder::default();
    let error = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect_err("too large");
    assert_eq!(error, RelayError::Protocol("body too large".into()));
    assert!(sink.begun.is_empty());
    server.join().expect("server");
}

#[tokio::test]
async fn get_blob_stops_when_the_sink_stops() {
    let body = vec![7u8; 300_000];
    let (base, server) = serve_once(answer("200 OK", &[], &body));
    let mut sink = Recorder {
        stop_at: Some(1),
        ..Recorder::default()
    };
    let delivery = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect("get");
    assert_eq!(delivery, Delivery::Stopped);
    assert_eq!(sink.pieces, 1);
    assert!(sink.bytes.len() < body.len());
    server.join().expect("server");
}

/// A sink that refuses the length it is told (more than it can take, say).
struct Refuser {
    pieces: usize,
}

impl BlobSink for Refuser {
    fn begin(&mut self, _: u64) -> SinkFlow {
        SinkFlow::Stop
    }

    fn piece(&mut self, _: &[u8]) -> SinkFlow {
        self.pieces += 1;
        SinkFlow::Continue
    }
}

#[tokio::test]
async fn get_blob_stops_when_the_sink_refuses_to_begin() {
    let (base, server) = serve_once(answer("200 OK", &[], b"hello"));
    let mut sink = Refuser { pieces: 0 };
    let delivery = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect("get");
    assert_eq!(delivery, Delivery::Stopped);
    assert_eq!(sink.pieces, 0);
    server.join().expect("server");
}

#[tokio::test]
async fn a_missing_blob_is_blob_not_found() {
    let (base, server) = serve_once(answer(
        "404 Not Found",
        &[
            ("Content-Type", "application/json"),
            ("Moony-Error", "blob_not_found"),
        ],
        br#"{"error":"blob_not_found"}"#,
    ));
    let mut sink = Recorder::default();
    let error = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect_err("missing");
    assert_eq!(error, RelayError::BlobNotFound);
    assert!(sink.begun.is_empty());
    server.join().expect("server");

    // The body alone (no header) says the same.
    let (base, server) = serve_once(json_answer(
        "404 Not Found",
        r#"{"error":"blob_not_found"}"#,
    ));
    let error = relay(&base)
        .get_blob(&vault(), blob(), &mut sink)
        .await
        .expect_err("missing");
    assert_eq!(error, RelayError::BlobNotFound);
    server.join().expect("server");
}

#[tokio::test]
async fn list_blobs_parses_ids_sizes_creation_and_the_server_now() {
    let second = "ffffffff-0000-4000-8000-000000000001";
    let (base, server) = serve_once(json_answer(
        "200 OK",
        &format!(
            r#"{{"blobs":[{{"id":"{BLOB}","size":75,"createdAt":1794000000}},{{"id":"{second}","size":104883275,"createdAt":1794650000}}],"more":true,"now":1794650400}}"#
        ),
    ));
    let after = Uuid::parse_str("00000000-0000-4000-8000-000000000000").expect("uuid");
    let page = relay(&base)
        .list_blobs(&vault(), Some(after), 2)
        .await
        .expect("list");
    assert_eq!(
        page,
        BlobPage {
            blobs: vec![
                RemoteBlob {
                    id: blob(),
                    size: 75,
                    created_at: 1_794_000_000,
                },
                RemoteBlob {
                    id: Uuid::parse_str(second).expect("uuid"),
                    size: 104_883_275,
                    created_at: 1_794_650_000,
                },
            ],
            more: true,
            server_now: Some(1_794_650_400),
        }
    );
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!(
            "GET /v1/vaults/{VAULT_ID}/blobs?after=00000000-0000-4000-8000-000000000000&limit=2 HTTP/1.1"
        )
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );

    // No `after`, no `now`: the first page, and no server clock to measure ages on.
    let (base, server) = serve_once(json_answer("200 OK", r#"{"blobs":[],"more":false}"#));
    let page = relay(&base)
        .list_blobs(&vault(), None, MAX_BLOB_PAGE)
        .await
        .expect("list");
    assert_eq!(
        page,
        BlobPage {
            blobs: vec![],
            more: false,
            server_now: None,
        }
    );
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("GET /v1/vaults/{VAULT_ID}/blobs?limit=1000 HTTP/1.1")
    );

    // A `now` that is not a non-negative integer is no clock either.
    for now in ["-5", "1.5", "\"1794650400\"", "null"] {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            &format!(r#"{{"blobs":[],"more":false,"now":{now}}}"#),
        ));
        let page = relay(&base)
            .list_blobs(&vault(), None, 10)
            .await
            .expect("list");
        assert_eq!(page.server_now, None, "{now}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn a_listing_limit_outside_the_page_is_refused_before_sending() {
    let (base, listener) = silent_port();
    for limit in [0, MAX_BLOB_PAGE + 1] {
        let error = relay(&base)
            .list_blobs(&vault(), None, limit)
            .await
            .expect_err("limit");
        assert!(matches!(error, RelayError::Protocol(_)), "{error:?}");
    }
    assert_never_contacted(&listener);
}

#[tokio::test]
async fn a_listing_with_a_bad_id_is_a_protocol_error() {
    for id in [
        "../x",
        "0B1C2D3E-4F50-4617-8899-AABBCCDDEEFF",
        "0b1c2d3e4f5046178899aabbccddeeff",
        "{0b1c2d3e-4f50-4617-8899-aabbccddeeff}",
    ] {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            &format!(
                r#"{{"blobs":[{{"id":"{id}","size":1,"createdAt":1}}],"more":false,"now":2}}"#
            ),
        ));
        let error = relay(&base)
            .list_blobs(&vault(), None, 10)
            .await
            .expect_err("bad id");
        assert_eq!(
            error,
            RelayError::Protocol("unexpected answer".into()),
            "{id}"
        );
        assert!(!format!("{error:?}").contains(id), "{error:?}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn delete_blob_treats_404_as_gone() {
    let (base, server) = serve_once(answer("204 No Content", &[], b""));
    relay(&base)
        .delete_blob(&vault(), blob())
        .await
        .expect("deleted");
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("DELETE /v1/vaults/{VAULT_ID}/blobs/{BLOB} HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );

    let (base, server) = serve_once(answer(
        "404 Not Found",
        &[("Moony-Error", "blob_not_found")],
        br#"{"error":"blob_not_found"}"#,
    ));
    relay(&base)
        .delete_blob(&vault(), blob())
        .await
        .expect("already gone");
    server.join().expect("server");

    // A missing vault is still an error.
    let (base, server) = serve_once(answer(
        "404 Not Found",
        &[("Moony-Error", "vault_not_found")],
        br#"{"error":"vault_not_found"}"#,
    ));
    assert_eq!(
        relay(&base).delete_blob(&vault(), blob()).await,
        Err(RelayError::VaultNotFound)
    );
    server.join().expect("server");
}

#[tokio::test]
async fn server_info_reads_the_blobs_feature_and_ignores_others() {
    async fn blobs(features: &str) -> bool {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            &format!(
                r#"{{"name":"moony-sync","version":"1.0.0","protocol":2,"mode":"self_hosted"{features}}}"#
            ),
        ));
        let info = relay(&base).server_info().await.expect("server info");
        server.join().expect("server");
        info.blobs
    }
    assert!(blobs(r#","features":["x","blobs"]"#).await);
    assert!(blobs(r#","features":["blobs"]"#).await);
    assert!(!blobs("").await);
    assert!(!blobs(r#","features":[]"#).await);
    assert!(!blobs(r#","features":["BLOBS"]"#).await);
}

#[test]
fn the_blob_bounds_are_pinned() {
    assert_eq!(BLOB_TIMEOUT, Duration::from_secs(30 * 60));
    assert_eq!(MAX_BLOB_PAGE, 1000);
}
