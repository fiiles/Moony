//! Tests of `HttpRelay`'s Recovery Kit calls (`PUT` / `GET /v1/vaults/:id/kit`, the `kit` of a
//! rotation, the `kit` of the vault info and of the server's features) against one-shot local
//! servers.

use super::tests::{json_answer, serve_once};
use super::*;

const VAULT_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
const TOKEN: &str = "dG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9";
const NEW_TOKEN: &str = "bmV3LXRva2VuLW5ldy10b2tlbi1uZXctdG9rZW4tbmV";

fn kit_token(c: char) -> String {
    c.to_string().repeat(43)
}

fn vault() -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(TOKEN.into()),
    }
}

fn relay(base: &str) -> HttpRelay {
    HttpRelay::new(base).expect("relay")
}

#[tokio::test]
async fn put_kit_sends_the_kit_token_and_the_padded_base64_envelope_under_the_vault_token() {
    let (base, server) = serve_once(json_answer("201 Created", r#"{"created":true}"#));
    // 5 bytes: base64 needs padding.
    let envelope = [1u8, 2, 3, 4, 5];
    let created = relay(&base)
        .put_kit(&vault(), &kit_token('K'), &envelope)
        .await
        .expect("put");
    assert!(created);
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("PUT /v1/vaults/{VAULT_ID}/kit HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );
    assert_eq!(request.header("moony-account"), None);
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json");
    assert_eq!(body["token"], kit_token('K'));
    assert_eq!(body["envelope"], STANDARD.encode(envelope));
    assert!(body["envelope"].as_str().expect("text").ends_with('='));
    assert_eq!(body.as_object().expect("object").len(), 2);
}

#[tokio::test]
async fn put_kit_tells_a_created_kit_from_a_retried_setup_and_checks_the_answer() {
    let (base, server) = serve_once(json_answer("200 OK", r#"{"created":false}"#));
    let created = relay(&base)
        .put_kit(&vault(), &kit_token('K'), &[1])
        .await
        .expect("put");
    assert!(!created);
    server.join().expect("server");
    // A status and a body that disagree are not the contract.
    for (status, body) in [
        ("201 Created", r#"{"created":false}"#),
        ("200 OK", r#"{"created":true}"#),
    ] {
        let (base, server) = serve_once(json_answer(status, body));
        let error = relay(&base)
            .put_kit(&vault(), &kit_token('K'), &[1])
            .await
            .expect_err("mismatch");
        assert_eq!(error, RelayError::Protocol("unexpected answer".into()));
        server.join().expect("server");
    }
}

#[tokio::test]
async fn put_kit_maps_the_servers_answers() {
    for (status, code, expected) in [
        ("409 Conflict", "kit_exists", RelayError::KitExists),
        (
            "400 Bad Request",
            "invalid_kit_token",
            RelayError::BadRequest("invalid_kit_token".into()),
        ),
        (
            "400 Bad Request",
            "invalid_kit",
            RelayError::BadRequest("invalid_kit".into()),
        ),
        (
            "413 Payload Too Large",
            "kit_too_large",
            RelayError::BadRequest("kit_too_large".into()),
        ),
        ("401 Unauthorized", "unauthorized", RelayError::Unauthorized),
        (
            "403 Forbidden",
            "token_read_only",
            RelayError::TokenReadOnly,
        ),
        (
            "404 Not Found",
            "vault_not_found",
            RelayError::VaultNotFound,
        ),
    ] {
        let (base, server) = serve_once(json_answer(status, &format!(r#"{{"error":"{code}"}}"#)));
        let error = relay(&base)
            .put_kit(&vault(), &kit_token('K'), &[1])
            .await
            .expect_err(code);
        assert_eq!(error, expected, "{code}");
        server.join().expect("server");
    }
    let (base, server) = serve_once(json_answer(
        "503 Service Unavailable",
        r#"{"error":"busy"}"#,
    ));
    let error = relay(&base)
        .put_kit(&vault(), &kit_token('K'), &[1])
        .await
        .expect_err("busy");
    assert_eq!(error, RelayError::Server(503));
    server.join().expect("server");
}

#[tokio::test]
async fn put_kit_refuses_bad_arguments_before_sending() {
    // Nothing listens: a request would be a `Network` error, not the argument error.
    let relay = relay("http://127.0.0.1:9");
    for (token, envelope, code) in [
        ("short".to_string(), vec![1u8], "invalid_kit_token"),
        (kit_token('K'), vec![], "invalid_kit"),
        (
            kit_token('K'),
            vec![0u8; MAX_KIT_ENVELOPE + 1],
            "kit_too_large",
        ),
    ] {
        let error = relay
            .put_kit(&vault(), &token, &envelope)
            .await
            .expect_err(code);
        assert_eq!(error, RelayError::BadRequest(code.into()), "{code}");
    }
    let error = relay
        .put_kit(&vault(), &kit_token('K'), &vec![0u8; MAX_KIT_ENVELOPE])
        .await
        .expect_err("connect");
    assert!(matches!(error, RelayError::Network(_)), "{error:?}");
}

#[tokio::test]
async fn get_kit_reads_with_the_kit_token_as_the_only_credential() {
    let (base, server) = serve_once(json_answer(
        "200 OK",
        &format!(
            r#"{{"envelope":"{}","since":1700000000,"updatedAt":1700000500}}"#,
            STANDARD.encode([9u8, 8, 7])
        ),
    ));
    let read = relay(&base)
        .get_kit(VAULT_ID, &kit_token('K'))
        .await
        .expect("get");
    assert_eq!(
        read,
        KitRead {
            envelope: vec![9, 8, 7],
            since: 1_700_000_000,
            updated_at: 1_700_000_500,
        }
    );
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("GET /v1/vaults/{VAULT_ID}/kit HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {}", kit_token('K')))
    );
    assert_eq!(request.header("moony-account"), None);
}

#[tokio::test]
async fn get_kit_maps_the_servers_answers_and_a_replaced_kit_carries_its_time() {
    for (status, body, expected) in [
        (
            "410 Gone",
            r#"{"error":"kit_replaced","replacedAt":1700000123}"#,
            RelayError::KitReplaced {
                replaced_at: 1_700_000_123,
            },
        ),
        (
            "401 Unauthorized",
            r#"{"error":"unauthorized"}"#,
            RelayError::Unauthorized,
        ),
        (
            "404 Not Found",
            r#"{"error":"vault_not_found"}"#,
            RelayError::VaultNotFound,
        ),
        (
            "402 Payment Required",
            r#"{"error":"payment_required"}"#,
            RelayError::PaymentRequired,
        ),
        (
            "429 Too Many Requests",
            r#"{"error":"rate_limited"}"#,
            RelayError::RateLimited,
        ),
        // A replaced Kit without its time is not the contract; it is never read as a rate limit.
        (
            "410 Gone",
            r#"{"error":"kit_replaced"}"#,
            RelayError::Protocol("unexpected answer".into()),
        ),
        (
            "410 Gone",
            r#"{"error":"kit_replaced","replacedAt":-1}"#,
            RelayError::Protocol("unexpected answer".into()),
        ),
    ] {
        let (base, server) = serve_once(json_answer(status, body));
        let error = relay(&base)
            .get_kit(VAULT_ID, &kit_token('K'))
            .await
            .expect_err(body);
        assert_eq!(error, expected, "{body}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn get_kit_refuses_answers_outside_the_contract() {
    for body in [
        // An empty envelope, bad base64, a negative time, and not JSON.
        r#"{"envelope":"","since":1,"updatedAt":1}"#,
        r#"{"envelope":"!!!","since":1,"updatedAt":1}"#,
        r#"{"envelope":"AQ==","since":-1,"updatedAt":1}"#,
        r#"{"envelope":"AQ==","since":1}"#,
        "<html>hello</html>",
    ] {
        let (base, server) = serve_once(json_answer("200 OK", body));
        let error = relay(&base)
            .get_kit(VAULT_ID, &kit_token('K'))
            .await
            .expect_err(body);
        assert!(
            matches!(error, RelayError::Protocol(_)),
            "{body}: {error:?}"
        );
        server.join().expect("server");
    }
}

#[tokio::test]
async fn get_kit_refuses_bad_arguments_before_sending() {
    let relay = relay("http://127.0.0.1:9");
    assert_eq!(
        relay.get_kit("short", &kit_token('K')).await,
        Err(RelayError::BadRequest("invalid_vault_id".into()))
    );
    assert_eq!(
        relay.get_kit(VAULT_ID, "short").await,
        Err(RelayError::BadRequest("invalid_kit_token".into()))
    );
}

#[tokio::test]
async fn a_rotation_with_a_kit_sends_the_envelope_and_a_new_kit_token() {
    let (base, server) = serve_once(json_answer("200 OK", r#"{"rotation":4,"head":9}"#));
    let kit = KitRotation {
        envelope: &[1, 2, 3, 4, 5],
        token: Some(&kit_token('N')),
    };
    let rotated = relay(&base)
        .rotate_token_with_kit(&vault(), NEW_TOKEN, &[7, 7], kit)
        .await
        .expect("rotate");
    assert_eq!(
        rotated,
        Rotated {
            rotation: 4,
            head: 9
        }
    );
    let request = server.join().expect("server");
    assert_eq!(
        request.request_line(),
        format!("POST /v1/vaults/{VAULT_ID}/token HTTP/1.1")
    );
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {TOKEN}"))
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("json");
    assert_eq!(body["token"], NEW_TOKEN);
    assert_eq!(body["rekey"], STANDARD.encode([7u8, 7]));
    assert_eq!(body["kit"]["envelope"], STANDARD.encode([1u8, 2, 3, 4, 5]));
    assert_eq!(body["kit"]["token"], kit_token('N'));
    assert_eq!(body.as_object().expect("object").len(), 3);
}

#[tokio::test]
async fn a_rotation_that_keeps_the_kit_token_sends_no_kit_token_and_one_without_a_kit_no_kit() {
    let (base, server) = serve_once(json_answer("200 OK", r#"{"rotation":1,"head":0}"#));
    let kit = KitRotation {
        envelope: &[1],
        token: None,
    };
    relay(&base)
        .rotate_token_with_kit(&vault(), NEW_TOKEN, &[7], kit)
        .await
        .expect("rotate");
    let body: serde_json::Value =
        serde_json::from_slice(&server.join().expect("server").body).expect("json");
    assert_eq!(body["kit"].as_object().expect("kit").len(), 1);
    assert!(body["kit"].get("token").is_none());

    let (base, server) = serve_once(json_answer("200 OK", r#"{"rotation":1,"head":0}"#));
    relay(&base)
        .rotate_token(&vault(), NEW_TOKEN, &[7])
        .await
        .expect("rotate");
    let body: serde_json::Value =
        serde_json::from_slice(&server.join().expect("server").body).expect("json");
    assert!(body.get("kit").is_none());
}

#[tokio::test]
async fn a_rotation_with_a_kit_maps_the_kit_errors() {
    for (status, code, expected) in [
        (
            "400 Bad Request",
            "kit_required",
            RelayError::BadRequest("kit_required".into()),
        ),
        (
            "400 Bad Request",
            "invalid_kit",
            RelayError::BadRequest("invalid_kit".into()),
        ),
        (
            "400 Bad Request",
            "invalid_kit_token",
            RelayError::BadRequest("invalid_kit_token".into()),
        ),
        (
            "413 Payload Too Large",
            "kit_too_large",
            RelayError::BadRequest("kit_too_large".into()),
        ),
        (
            "403 Forbidden",
            "token_read_only",
            RelayError::TokenReadOnly,
        ),
    ] {
        let (base, server) = serve_once(json_answer(status, &format!(r#"{{"error":"{code}"}}"#)));
        let kit = KitRotation {
            envelope: &[1],
            token: None,
        };
        let error = relay(&base)
            .rotate_token_with_kit(&vault(), NEW_TOKEN, &[7], kit)
            .await
            .expect_err(code);
        assert_eq!(error, expected, "{code}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn a_rotation_with_a_kit_refuses_bad_arguments_before_sending() {
    let relay = relay("http://127.0.0.1:9");
    let long = vec![0u8; MAX_KIT_ENVELOPE + 1];
    for (envelope, token, code) in [
        (&[][..], None, "invalid_kit"),
        (&long[..], None, "kit_too_large"),
        (&[1u8][..], Some("short"), "invalid_kit_token"),
    ] {
        let kit = KitRotation { envelope, token };
        let error = relay
            .rotate_token_with_kit(&vault(), NEW_TOKEN, &[7], kit)
            .await
            .expect_err(code);
        assert_eq!(error, RelayError::BadRequest(code.into()), "{code}");
    }
}

#[tokio::test]
async fn vault_info_carries_the_kit_or_none() {
    for (body, expected) in [
        (
            r#","kit":{"since":1700000000,"lastRead":1700000040}"#,
            Some(KitInfo {
                since: 1_700_000_000,
                last_read: Some(1_700_000_040),
            }),
        ),
        (
            r#","kit":{"since":1700000000,"lastRead":null}"#,
            Some(KitInfo {
                since: 1_700_000_000,
                last_read: None,
            }),
        ),
        (r#","kit":null"#, None),
        // A server from before the Kit sends no field.
        ("", None),
    ] {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            &format!(
                r#"{{"head":1,"floor":0,"snapshotSeq":null,"paidUntil":1800000000,"bytes":0,"rotation":0{body}}}"#
            ),
        ));
        let info = relay(&base).vault_info(&vault()).await.expect("info");
        assert_eq!(info.kit, expected, "{body}");
        server.join().expect("server");
    }
}

#[tokio::test]
async fn server_info_reads_the_kit_feature() {
    for (features, kit, blobs) in [
        (r#","features":["blobs","kit"]"#, true, true),
        (r#","features":["kit"]"#, true, false),
        (r#","features":["blobs"]"#, false, true),
        (r#","features":"kit""#, false, false),
        ("", false, false),
    ] {
        let (base, server) = serve_once(json_answer(
            "200 OK",
            &format!(
                r#"{{"name":"moony-sync","version":"1.0.0","protocol":2,"mode":"self_hosted"{features}}}"#
            ),
        ));
        let info = relay(&base).server_info().await.expect("info");
        assert_eq!((info.kit, info.blobs), (kit, blobs), "{features}");
        server.join().expect("server");
    }
}
