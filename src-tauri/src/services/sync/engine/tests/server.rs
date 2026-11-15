//! The server check: an address answers as a Moony sync server of this person's own, or it is
//! refused with a reading of why, before any account call.

use super::join::validation_key;
use super::*;
use crate::models::sync::SyncServerCheck;
use crate::services::sync::error::SyncError;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{RelayError, ServerInfo, ServerMode};
use crate::services::sync::server::check_own_server;

#[tokio::test]
async fn an_own_server_passes_the_check() {
    let fake = FakeRelay::new();
    let check = check_own_server(&fake, "https://Relay.Example.org/")
        .await
        .expect("own server");
    assert_eq!(
        check,
        SyncServerCheck {
            address: "https://relay.example.org".into(),
            host: "relay.example.org".into(),
            version: "1.0.0".into(),
            insecure: false,
        }
    );

    let lan = check_own_server(&fake, "http://192.168.1.20:8787")
        .await
        .expect("lan server");
    assert_eq!(lan.address, "http://192.168.1.20:8787");
    assert_eq!(lan.host, "192.168.1.20:8787");
    assert!(lan.insecure);
    assert_eq!(fake.calls(), [Route::ServerInfo, Route::ServerInfo]);
}

#[tokio::test]
async fn an_address_that_is_not_one_is_refused_without_a_call() {
    let fake = FakeRelay::new();
    let error = check_own_server(&fake, "ftp://relay.example.org")
        .await
        .expect_err("refused");
    assert_eq!(
        validation_key(&error),
        Some("validation.syncRelayUrlInvalid")
    );
    assert!(fake.calls().is_empty());
}

#[tokio::test]
async fn what_is_not_an_own_moony_server_is_refused_before_any_account_call() {
    let info = |protocol, mode| {
        Some(ServerInfo {
            version: "1.0.0".into(),
            protocol,
            mode,
            blobs: true,
        })
    };
    type Prepare = Box<dyn Fn(&FakeRelay)>;
    let cases: Vec<(&str, Prepare, &str)> = vec![
        (
            "an old relay",
            Box::new(|f| f.set_server_info(None)),
            "validation.syncServerNotFound",
        ),
        (
            "protocol 1",
            Box::new(move |f| f.set_server_info(info(1, ServerMode::SelfHosted))),
            "validation.syncServerNeedsUpdate",
        ),
        (
            "the cloud",
            Box::new(move |f| f.set_server_info(info(2, ServerMode::Cloud))),
            "validation.syncServerNotOwn",
        ),
        (
            "another mode",
            Box::new(move |f| f.set_server_info(info(2, ServerMode::Other))),
            "validation.syncServerNotOwn",
        ),
        (
            "no network",
            Box::new(|f| f.fail_next(Route::ServerInfo, RelayError::Network("connect".into()))),
            "validation.syncServerNotFound",
        ),
        (
            "a server error",
            Box::new(|f| f.fail_next(Route::ServerInfo, RelayError::Server(502))),
            "validation.syncServerNotFound",
        ),
        (
            "not a moony server",
            Box::new(|f| f.fail_next(Route::ServerInfo, RelayError::Protocol("not json".into()))),
            "validation.syncServerNotFound",
        ),
    ];
    for (label, prepare, key) in cases {
        let fake = FakeRelay::new();
        prepare(&fake);
        let error = check_own_server(&fake, "https://relay.example.org")
            .await
            .expect_err(label);
        assert_eq!(validation_key(&error), Some(key), "{label}: {error:?}");
        assert_eq!(fake.calls(), [Route::ServerInfo], "{label}");
    }

    let fake = FakeRelay::new();
    fake.fail_next(Route::ServerInfo, RelayError::RateLimited);
    let error = check_own_server(&fake, "https://relay.example.org")
        .await
        .expect_err("rate limited");
    assert!(matches!(error, SyncError::RateLimited), "{error:?}");
    assert!(error.to_app_error().to_string().contains("syncRateLimited"));
    assert_eq!(fake.calls(), [Route::ServerInfo]);
}
