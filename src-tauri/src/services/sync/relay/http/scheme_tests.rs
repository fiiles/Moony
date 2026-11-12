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
