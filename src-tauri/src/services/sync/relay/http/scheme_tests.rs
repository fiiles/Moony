//! The scheme rule of sync server addresses (audit L9, spec §9.2).

use super::*;

#[test]
fn plain_http_is_allowed_except_to_the_moony_cloud() {
    assert!(scheme_allowed("http", Some("192.168.1.20")));
    assert!(scheme_allowed("http", Some("nas.local")));
    assert!(scheme_allowed("http", Some("localhost")));
    assert!(scheme_allowed("https", Some("sync.example.com")));
    assert!(!scheme_allowed("http", Some("sync.moonyfinance.com")));
    assert!(!scheme_allowed("http", Some("")));
    assert!(!scheme_allowed("http", None));
    assert!(!scheme_allowed("https", Some("")));
    assert!(!scheme_allowed("https", None));
    assert!(!scheme_allowed("ftp", Some("localhost")));
    // The same in debug and release builds: no build flag is read.
    for url in [
        "http://sync.moonyfinance.com",
        "http://sync.moonyfinance.com.:80/",
        "http://sync.moonyfinance.com..",
        "http://sync.moonyfinance.com...:80/",
    ] {
        assert!(validate_base_url(url).is_err(), "{url}");
        assert!(HttpRelay::new(url).is_err(), "{url}");
    }
    for url in [
        "http://127.0.0.1:8787",
        "http://localhost:8787/",
        "http://192.168.1.20:8787",
    ] {
        assert!(validate_base_url(url).is_ok(), "{url}");
        assert!(HttpRelay::new(url).is_ok(), "{url}");
    }
    assert!(validate_base_url("https://sync.moonyfinance.com").is_ok());
    assert_eq!(
        Url::parse(DEFAULT_RELAY_URL).expect("default").host_str(),
        Some(DEFAULT_RELAY_HOST)
    );
}

#[test]
fn an_http_address_is_insecure_unless_loopback() {
    assert!(is_insecure_address("http://192.168.1.20:8787"));
    assert!(is_insecure_address("http://nas.local"));
    assert!(!is_insecure_address("http://localhost:8787"));
    assert!(!is_insecure_address("http://127.0.0.1:8787"));
    // By design: the IPv6 loopback is not on the short list, so it asks for the warning too.
    assert!(is_insecure_address("http://[::1]"));
    assert!(!is_insecure_address("https://x.example"));
    assert!(!is_insecure_address("not a url"));
}

#[test]
fn a_display_host_is_host_and_port() {
    assert_eq!(
        display_host("https://sync.example.org:8443"),
        Some("sync.example.org:8443".into())
    );
    assert_eq!(
        display_host("https://sync.example.org"),
        Some("sync.example.org".into())
    );
    assert_eq!(display_host("nope"), None);
}

#[test]
fn every_trailing_dot_of_the_cloud_host_is_stripped() {
    assert_eq!(
        normalize_base_url("https://sync.moonyfinance.com..").expect("https"),
        DEFAULT_RELAY_URL
    );
    assert!(is_default_relay("https://sync.moonyfinance.com..."));
}
