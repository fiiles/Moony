//! Tests of `HttpRelay`'s pairing mailbox calls against one-shot local servers.

use super::tests::{answer, json_answer, serve_once};
use super::*;
use std::net::TcpListener;

const MAILBOX: &str = "AAECAwQFBgcICQoLDA0ODw";

fn relay(base: &str) -> HttpRelay {
    HttpRelay::new(base).expect("relay")
}

#[tokio::test]
async fn a_slot_is_put_as_raw_bytes_without_credentials() {
    let (base, server) = serve_once(answer("204 No Content", &[], b""));
    relay(&base)
        .put_slot(MAILBOX, 1, &[0, 1, 2, 255])
        .await
        .expect("put");
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("PUT /v1/pairing/{MAILBOX}/1 HTTP/1.1")
    );
    assert_eq!(
        request.header("content-type").as_deref(),
        Some("application/octet-stream")
    );
    assert_eq!(request.body, vec![0, 1, 2, 255]);
    assert_eq!(request.header("moony-account"), None);
    assert_eq!(request.header("authorization"), None);
}

#[tokio::test]
async fn slot_3_round_trips_over_http() {
    let (base, server) = serve_once(answer("204 No Content", &[], b""));
    relay(&base)
        .put_slot(MAILBOX, 3, b"ack")
        .await
        .expect("put");
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("PUT /v1/pairing/{MAILBOX}/3 HTTP/1.1")
    );
    assert_eq!(request.body, b"ack".to_vec());

    let (base, server) = serve_once(answer("200 OK", &[], &[4, 5]));
    assert_eq!(
        relay(&base).take_slot(MAILBOX, 3).await,
        Ok(Some(vec![4, 5]))
    );
    assert_eq!(
        server.join().expect("server").request_line(),
        format!("GET /v1/pairing/{MAILBOX}/3 HTTP/1.1")
    );
}

#[tokio::test]
async fn slot_4_is_refused_before_any_request() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let relay = relay(&base);
    let bad = RelayError::BadRequest("bad_slot".into());
    assert_eq!(relay.put_slot(MAILBOX, 4, b"x").await, Err(bad.clone()));
    assert_eq!(relay.take_slot(MAILBOX, 4).await, Err(bad));
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "no connection was made"
    );
}

#[tokio::test]
async fn an_empty_slot_reads_as_none_and_a_written_one_as_its_bytes() {
    let (base, server) = serve_once(json_answer("404 Not Found", r#"{"error":"empty"}"#));
    assert_eq!(relay(&base).take_slot(MAILBOX, 2).await, Ok(None));
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("GET /v1/pairing/{MAILBOX}/2 HTTP/1.1")
    );
    assert_eq!(request.header("moony-account"), None);
    assert_eq!(request.header("authorization"), None);

    let (base, server) = serve_once(answer(
        "200 OK",
        &[("Content-Type", "application/octet-stream")],
        &[9, 8, 7],
    ));
    assert_eq!(
        relay(&base).take_slot(MAILBOX, 1).await,
        Ok(Some(vec![9, 8, 7]))
    );
    server.join().expect("server");
}

#[tokio::test]
async fn a_mailbox_is_burnt_with_a_delete() {
    let (base, server) = serve_once(answer("204 No Content", &[], b""));
    relay(&base).burn_mailbox(MAILBOX).await.expect("burn");
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("DELETE /v1/pairing/{MAILBOX} HTTP/1.1")
    );
    assert_eq!(request.header("authorization"), None);
}

#[tokio::test]
async fn mailbox_answers_map_to_their_errors() {
    let cases: Vec<(&str, Vec<u8>, RelayError)> = vec![
        (
            "put",
            json_answer("409 Conflict", r#"{"error":"slot_taken"}"#),
            RelayError::SlotTaken,
        ),
        (
            "take",
            json_answer("410 Gone", r#"{"error":"consumed"}"#),
            RelayError::SlotConsumed,
        ),
        (
            "take",
            json_answer("410 Gone", r#"{"error":"mailbox_gone"}"#),
            RelayError::MailboxGone,
        ),
        (
            "put",
            json_answer("410 Gone", r#"{"error":"mailbox_gone"}"#),
            RelayError::MailboxGone,
        ),
        (
            "take",
            answer("429 Too Many Requests", &[], b"slow"),
            RelayError::RateLimited,
        ),
        (
            "burn",
            answer("429 Too Many Requests", &[], b"slow"),
            RelayError::RateLimited,
        ),
        (
            "put",
            json_answer("413 Payload Too Large", r#"{"error":"body_too_large"}"#),
            RelayError::BadRequest("body_too_large".into()),
        ),
        (
            "take",
            json_answer("503 Service Unavailable", r#"{"error":"try_again"}"#),
            RelayError::Server(503),
        ),
    ];
    for (call, response, expected) in cases {
        let (base, server) = serve_once(response);
        let relay = relay(&base);
        let result = match call {
            "put" => relay.put_slot(MAILBOX, 1, b"x").await,
            "take" => relay.take_slot(MAILBOX, 1).await.map(|_| ()),
            _ => relay.burn_mailbox(MAILBOX).await,
        };
        assert_eq!(result, Err(expected.clone()), "{call} {expected:?}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn bad_mailbox_ids_and_slots_are_refused_before_sending() {
    // Nothing connects: the listener would answer, but no request is made.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let relay = relay(&base);
    let bad_mailbox = RelayError::BadRequest("bad_mailbox".into());
    let bad_slot = RelayError::BadRequest("bad_slot".into());
    for id in [
        "",
        "short",
        "AAECAwQFBgcICQoLDA0ODwX",
        "AAECAwQFBgcICQoLDA0OD+",
        "../../v1/accounts/x",
        "AAECAwQFBgcICQoLDA0OD/",
    ] {
        assert_eq!(
            relay.put_slot(id, 1, b"x").await,
            Err(bad_mailbox.clone()),
            "{id}"
        );
        assert_eq!(
            relay.take_slot(id, 1).await,
            Err(bad_mailbox.clone()),
            "{id}"
        );
        assert_eq!(
            relay.burn_mailbox(id).await,
            Err(bad_mailbox.clone()),
            "{id}"
        );
    }
    for slot in [0u8, 4, 255] {
        assert_eq!(
            relay.put_slot(MAILBOX, slot, b"x").await,
            Err(bad_slot.clone())
        );
        assert_eq!(relay.take_slot(MAILBOX, slot).await, Err(bad_slot.clone()));
    }
    assert_eq!(
        relay.put_slot(MAILBOX, 1, b"").await,
        Err(RelayError::BadRequest("empty_body".into()))
    );
    let too_large = vec![0u8; MAX_SLOT_BYTES + 1];
    assert_eq!(
        relay.put_slot(MAILBOX, 1, &too_large).await,
        Err(RelayError::BadRequest("body_too_large".into()))
    );
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "no connection was made"
    );
}

#[tokio::test]
async fn a_slot_of_exactly_the_cap_is_put_and_taken() {
    let (base, server) = serve_once(answer("204 No Content", &[], b""));
    relay(&base)
        .put_slot(MAILBOX, 2, &vec![5u8; MAX_SLOT_BYTES])
        .await
        .expect("put");
    assert_eq!(server.join().expect("server").body.len(), MAX_SLOT_BYTES);

    let (base, server) = serve_once(answer("200 OK", &[], &vec![6u8; MAX_SLOT_BYTES]));
    let bytes = relay(&base).take_slot(MAILBOX, 2).await.expect("take");
    assert_eq!(bytes.map(|b| b.len()), Some(MAX_SLOT_BYTES));
    server.join().expect("server");
}

#[tokio::test]
async fn an_oversized_slot_answer_is_a_protocol_error() {
    let (base, server) = serve_once(answer("200 OK", &[], &vec![1u8; MAX_SLOT_BYTES + 1]));
    assert_eq!(
        relay(&base).take_slot(MAILBOX, 1).await,
        Err(RelayError::Protocol("body too large".into()))
    );
    server.join().expect("server");
}

#[tokio::test]
async fn a_redirect_is_not_followed_and_errors_carry_no_mailbox_id() {
    let (base, server) = serve_once(answer(
        "302 Found",
        &[("Location", "http://127.0.0.1:1/elsewhere")],
        b"",
    ));
    let error = relay(&base)
        .take_slot(MAILBOX, 1)
        .await
        .expect_err("redirect");
    assert_eq!(error, RelayError::Protocol("redirect".into()));
    assert!(!format!("{error} {error:?}").contains(MAILBOX));
    server.join().expect("server");

    // A refused connection names neither the URL nor the mailbox.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let error = relay(&format!("http://127.0.0.1:{port}"))
        .put_slot(MAILBOX, 1, b"x")
        .await
        .expect_err("connect");
    assert_eq!(error, RelayError::Network("connect".into()));
}
