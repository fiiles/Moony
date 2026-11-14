//! The connection code of a self-hosted sync server (spec §9): `moony-sync1:<account>:<server>`,
//! the server as base64url-without-padding of its UTF-8 address (the encoding the pairing code and
//! the Recovery Kit QR already use). A person pastes it, or scans it, instead of typing an
//! address and an account number. Pure: no relay, no database.
//!
//! Nothing here echoes a code: every error is a fixed `validation.*` key, and `Debug` of a parsed
//! code shows its server's origin only (never the account).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use super::relay::http::normalize_base_url;
use crate::error::{AppError, Result};

/// The version-1 prefix of a connection code.
pub const CONNECTION_CODE_PREFIX: &str = "moony-sync1:";
/// The longest code read, after the characters a chat app adds are removed.
pub const MAX_CONNECTION_CODE_CHARS: usize = 512;

const WORD: &str = "moony-sync";
const ACCOUNT_DIGITS: usize = 16;
const INVALID: &str = "validation.syncConnectionCodeInvalid";
const NEWER: &str = "validation.syncConnectionCodeNewer";
const SERVER_INVALID: &str = "validation.syncRelayUrlInvalid";

/// A connection code read: the server's address in its normalized spelling (`normalize_base_url`)
/// and the 16-digit account number.
pub struct ConnectionCode {
    pub server_url: String,
    pub account: String,
}

impl std::fmt::Debug for ConnectionCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let origin = reqwest::Url::parse(&self.server_url)
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_default();
        f.debug_struct("ConnectionCode")
            .field("server", &origin)
            .finish()
    }
}

/// `text` as a code is read: trimmed, with every Unicode white space and the zero-width
/// characters removed (chat apps wrap long lines and insert them), and one trailing `.` or `,`
/// dropped (a sentence's punctuation; neither is in the base64url alphabet).
fn cleaned(text: &str) -> String {
    let mut code: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}'))
        .collect();
    if code.ends_with(['.', ',']) {
        code.pop();
    }
    code
}

/// The version digits of a `moony-sync<digits>:` prefix, and what follows it.
fn split_prefix(code: &str) -> Option<(&str, &str)> {
    let rest = code.strip_prefix(WORD)?;
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let (version, after) = rest.split_at(digits);
    let after = after.strip_prefix(':')?;
    (!version.is_empty()).then_some((version, after))
}

/// Whether `text` looks like a connection code of any version (`moony-sync<digits>:`).
pub fn is_connection_code(text: &str) -> bool {
    split_prefix(&cleaned(text)).is_some()
}

/// Reads a connection code. A version above 1 is `validation.syncConnectionCodeNewer`, any other
/// malformed shape `validation.syncConnectionCodeInvalid`, an address the app does not use
/// `validation.syncRelayUrlInvalid`.
pub fn parse_connection_code(text: &str) -> Result<ConnectionCode> {
    let code = cleaned(text);
    if code.chars().count() > MAX_CONNECTION_CODE_CHARS {
        return Err(AppError::Validation(INVALID.into()));
    }
    let (version, rest) =
        split_prefix(&code).ok_or_else(|| AppError::Validation(INVALID.into()))?;
    if version != "1" {
        // Longer than any counter, or above 1, is "newer"; zero and `01` are not versions.
        let newer = !version.starts_with('0');
        return Err(AppError::Validation(
            if newer { NEWER } else { INVALID }.into(),
        ));
    }
    let fields: Vec<&str> = rest.split(':').collect();
    let [account, server] = fields[..] else {
        return Err(AppError::Validation(INVALID.into()));
    };
    if account.len() != ACCOUNT_DIGITS || !account.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Validation(INVALID.into()));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(server.as_bytes())
        .ok()
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(|| AppError::Validation(INVALID.into()))?;
    let address = String::from_utf8(bytes).map_err(|_| AppError::Validation(INVALID.into()))?;
    Ok(ConnectionCode {
        server_url: normalize_base_url(&address)?,
        account: account.to_string(),
    })
}

/// The server address a person gave: a connection code yields its server (its account is
/// ignored), anything else is an address. Always normalized; blank, unreadable or unusable is
/// `validation.syncRelayUrlInvalid`.
pub fn server_address_from_input(input: &str) -> Result<String> {
    if is_connection_code(input) {
        return parse_connection_code(input).map(|code| code.server_url);
    }
    let input = input.trim();
    if input.is_empty() {
        return Err(AppError::Validation(SERVER_INVALID.into()));
    }
    normalize_base_url(input)
}

/// A code as the server writes it, for the tests.
#[cfg(test)]
pub fn encode_connection_code(server_url: &str, account: &str) -> String {
    format!(
        "{CONNECTION_CODE_PREFIX}{account}:{}",
        URL_SAFE_NO_PAD.encode(server_url)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "4417938205561273";
    const CODE: &str = "moony-sync1:4417938205561273:aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn";

    fn key_of<T: std::fmt::Debug>(result: Result<T>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn a_connection_code_reads_its_server_and_account() {
        let code = parse_connection_code(CODE).expect("code");
        assert_eq!(code.server_url, "https://sync.example.org");
        assert_eq!(code.account, ACCOUNT);
        assert_eq!(
            encode_connection_code("https://sync.example.org", ACCOUNT),
            CODE
        );
        let again = parse_connection_code(&encode_connection_code(&code.server_url, &code.account))
            .expect("round trip");
        assert_eq!(
            (again.server_url, again.account),
            (code.server_url, code.account)
        );
    }

    #[test]
    fn a_code_mangled_by_a_chat_app_still_reads() {
        let mut mangled = String::from("  ");
        for (i, c) in CODE.chars().enumerate() {
            if i == 20 {
                mangled.push('\n');
            }
            if i > 0 && i % 8 == 0 {
                mangled.push(' ');
            }
            if i == 30 {
                mangled.push('\u{200B}');
            }
            mangled.push(c);
        }
        mangled.push_str(".  ");
        let code = parse_connection_code(&mangled).expect("mangled");
        assert_eq!(code.server_url, "https://sync.example.org");
        assert_eq!(code.account, ACCOUNT);
        let comma = parse_connection_code(&format!("{CODE},")).expect("comma");
        assert_eq!(comma.account, ACCOUNT);
    }

    #[test]
    fn http_codes_and_uppercase_hosts_are_normalised() {
        let upper = parse_connection_code(&format!(
            "moony-sync1:{ACCOUNT}:SFRUUDovL0hvc3QuRXhhbXBsZTo4MC8"
        ))
        .expect("upper");
        assert_eq!(upper.server_url, "http://host.example");
        let lan = parse_connection_code(&format!(
            "moony-sync1:{ACCOUNT}:aHR0cDovLzE5Mi4xNjguMS4yMDo4Nzg3"
        ))
        .expect("lan");
        assert_eq!(lan.server_url, "http://192.168.1.20:8787");
    }

    #[test]
    fn a_code_of_the_longest_length_passes_the_length_gate() {
        // 512 characters get past the length check (and fail on the address, which a code of
        // that size can never carry); 513 stop at the gate.
        let prefix = format!("moony-sync1:{ACCOUNT}:");
        let at_limit = format!(
            "{prefix}{}",
            "A".repeat(MAX_CONNECTION_CODE_CHARS - prefix.len())
        );
        assert_eq!(at_limit.chars().count(), MAX_CONNECTION_CODE_CHARS);
        assert_eq!(
            key_of(parse_connection_code(&at_limit)),
            "validation.syncRelayUrlInvalid"
        );
        let over = format!("{at_limit}A");
        assert_eq!(
            key_of(parse_connection_code(&over)),
            "validation.syncConnectionCodeInvalid"
        );
    }

    #[test]
    fn malformed_codes_are_refused_without_echoing_them() {
        let good_server = "aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn";
        let invalid = "validation.syncConnectionCodeInvalid";
        let long = format!("moony-sync1:{ACCOUNT}:{}", "A".repeat(513));
        let cases = [
            format!("moony-sync1:441793820556127:{good_server}"),
            format!("moony-sync1:441793820556127a:{good_server}"),
            format!("moony-sync1:{ACCOUNT}:!!"),
            format!("moony-sync1:{ACCOUNT}:"),
            format!("moony-sync1:{ACCOUNT}:{good_server}:extra"),
            format!("moony-sync1:{ACCOUNT}"),
            format!("moony-sync0:{ACCOUNT}:{good_server}"),
            format!("moony-sync01:{ACCOUNT}:{good_server}"),
            format!("moony-sync:{ACCOUNT}:{good_server}"),
            format!("moony-sync1:{ACCOUNT}:/w"), // not UTF-8
            long,
            String::new(),
        ];
        for case in &cases {
            let error = parse_connection_code(case).expect_err(case);
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains(ACCOUNT), "{case}");
            assert_eq!(key_of(parse_connection_code(case)), invalid, "{case}");
        }
        for newer in [
            format!("moony-sync2:{ACCOUNT}:{good_server}"),
            "moony-sync2:abc".to_string(),
            format!("moony-sync99999999999999999999:{ACCOUNT}:{good_server}"),
        ] {
            assert_eq!(
                key_of(parse_connection_code(&newer)),
                "validation.syncConnectionCodeNewer",
                "{newer}"
            );
        }
        for server in ["ftp://x.example", "https://x.example/p"] {
            let code = encode_connection_code(server, ACCOUNT);
            let error = parse_connection_code(&code).expect_err(server);
            assert!(!format!("{error:?}").contains(ACCOUNT));
            assert_eq!(
                key_of(parse_connection_code(&code)),
                "validation.syncRelayUrlInvalid"
            );
        }
        let code = parse_connection_code(CODE).expect("code");
        let shown = format!("{code:?}");
        assert!(!shown.contains("4417"), "{shown}");
        assert!(shown.contains("https://sync.example.org"), "{shown}");
    }

    #[test]
    fn server_address_from_input_takes_an_address_or_a_code() {
        assert_eq!(
            server_address_from_input(" https://Sync.Example.org/ ").expect("address"),
            "https://sync.example.org"
        );
        assert_eq!(
            server_address_from_input(CODE).expect("code"),
            "https://sync.example.org"
        );
        for refused in ["", "   ", "moony-pair:2:x:y"] {
            assert_eq!(
                key_of(server_address_from_input(refused)),
                "validation.syncRelayUrlInvalid",
                "{refused:?}"
            );
        }
        // A code that is a code but broken says so, not "address invalid".
        assert_eq!(
            key_of(server_address_from_input("moony-sync2:abc")),
            "validation.syncConnectionCodeNewer"
        );
    }

    #[test]
    fn a_code_of_any_version_is_recognised() {
        assert!(is_connection_code(CODE));
        assert!(is_connection_code("  moony-sync2:abc"));
        assert!(!is_connection_code("moony-pair:2:x:y"));
        assert!(!is_connection_code("https://sync.example.org"));
        assert!(!is_connection_code("moony-sync:1"));
    }
}
