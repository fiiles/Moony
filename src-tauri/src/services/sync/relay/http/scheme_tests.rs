//! The scheme rule of relay addresses (audit L9).

use super::*;

#[test]
fn plain_http_is_a_debug_build_convenience() {
    assert!(!scheme_allowed("http", Some("127.0.0.1"), false));
    assert!(!scheme_allowed("http", Some("localhost"), false));
    assert!(scheme_allowed("http", Some("127.0.0.1"), true));
    assert!(scheme_allowed("http", Some("localhost"), true));
    assert!(scheme_allowed("https", Some("sync.example.com"), false));
    assert!(scheme_allowed("https", Some("sync.example.com"), true));
    assert!(!scheme_allowed("http", Some("example.com"), true));
    assert!(!scheme_allowed("http", Some("example.com"), false));
    assert!(!scheme_allowed("https", Some(""), true));
    assert!(!scheme_allowed("https", None, true));
    assert!(!scheme_allowed("ftp", Some("localhost"), true));
}

#[test]
fn a_local_http_address_is_accepted_in_debug_builds_only() {
    for url in ["http://127.0.0.1:8787", "http://localhost:8787/"] {
        assert_eq!(validate_base_url(url).is_ok(), cfg!(debug_assertions), "{url}");
        assert_eq!(HttpRelay::new(url).is_ok(), cfg!(debug_assertions), "{url}");
    }
    assert!(validate_base_url("https://sync.moonyfinance.com").is_ok());
    assert!(validate_base_url("http://sync.moonyfinance.com").is_err());
}
