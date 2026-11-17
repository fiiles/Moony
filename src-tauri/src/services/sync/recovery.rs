//! The Sync Recovery Kit's recovery code and QR (spec §11.5, D24, ruling P4), and the relay
//! address a kit for a self-hosted relay carries (ruling SH-2).
//!
//! **Format v2** (phase 5 revision §2.2, the Kit as a paper device): the payload is the vault ID
//! (16 bytes), the Kit's seed (32 bytes, `identity::kit`) and the account number (7 bytes, a
//! big-endian integer below 10^16), 440 bits written big-endian as Crockford base32 (88
//! characters), then a 7-character checksum: 35 bits of `SHA-256("moony recovery code v2" ‖
//! payload)`; 95 characters, printed in 19 groups of five. The QR is `moony-recover:2:<code>` and,
//! for a server other than the default one, `:<base64url(server address)>` (`kit_qr_payload`).
//! A v1 code (80 characters) or QR is refused as `validation.syncRecoveryCodeInvalid`.
//!
//! Format v1 (the vault ID and one vault key, 80 characters, QR `moony-recover:1:`) is gone
//! (revision As built R-1): only development vaults ever had one.
//!
//! The checksum guards against typos and is NOT an authenticator: it proves nothing about who
//! made the code. Detection is probabilistic (about 1 typo in 34 billion passes); a code with
//! such a typo then derives a Kit the server does not know, which is a clear error. The code and
//! the QR payload are secrets (ruling P16): they live in `Zeroizing` buffers, and no error carries
//! the input. A `Zeroizing<String>` forwards `Debug` to its content, so callers must never format
//! the returned code or payload; the decoded seed comes back in the redacting `KitSecret`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::connection_code::server_address_from_input;
use super::identity::KitSecret;
use super::keys::VaultId;
use super::relay::http::{is_default_relay, normalize_base_url, DEFAULT_RELAY_URL};
use crate::error::{AppError, Result};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const GROUP: usize = 5;
const QR_MIN_SIDE: u32 = 240;

fn invalid() -> AppError {
    AppError::Validation("validation.syncRecoveryCodeInvalid".into())
}

/// The code in groups of five joined by `-`, for printing (19 groups for a v2 code).
pub fn format_recovery_code(code: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(code.len() + code.len() / GROUP));
    for (i, c) in code.chars().enumerate() {
        if i > 0 && i % GROUP == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

/// What a typed code or account may hold between its characters: whitespace, `-` and the dashes
/// U+2010 to U+2015 (an autocorrected en dash, say).
fn is_separator(c: char) -> bool {
    c == '-' || ('\u{2010}'..='\u{2015}').contains(&c) || c.is_whitespace()
}

/// The longest payload read. The longest one written is 471 bytes: the prefix, a 16-digit
/// account, a grouped 95-character code and the base64url of a 256-character relay address.
pub const MAX_QR_PAYLOAD_BYTES: usize = 512;

const QR_SCHEME: &str = "moony-recover:";
const ACCOUNT_DIGITS: usize = 16;

fn account_invalid() -> AppError {
    AppError::Validation("validation.syncRecoveryAccountInvalid".into())
}

// ── Format v2 (phase 5 revision §2.2) ────────────────────────────────────────────────────────

/// Crockford base32 characters of a v2 code, checksum included.
pub const KIT_CODE_LEN: usize = 95;

const KIT_PAYLOAD_BYTES: usize = 16 + 32 + 7;
const KIT_PAYLOAD_CHARS: usize = 88;
const KIT_CHECKSUM_CHARS: usize = 7;
const KIT_LABEL: &[u8] = b"moony recovery code v2";
const KIT_QR_PREFIX: &str = "moony-recover:2:";
/// Accounts are 16 decimal digits: below this.
const ACCOUNT_LIMIT: u64 = 10_000_000_000_000_000;

/// What a v2 code holds. `Debug` prints nothing of it.
pub struct KitCode {
    pub vault_id: VaultId,
    pub seed: KitSecret,
    /// The account number, 16 digits.
    pub account: String,
}

impl std::fmt::Debug for KitCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KitCode { .. }")
    }
}

/// The seven checksum characters (35 bits) of a v2 payload.
fn kit_checksum(payload: &[u8; KIT_PAYLOAD_BYTES]) -> [u8; KIT_CHECKSUM_CHARS] {
    let hash = Sha256::new()
        .chain_update(KIT_LABEL)
        .chain_update(payload)
        .finalize();
    let bits = u64::from_be_bytes([0, 0, 0, hash[0], hash[1], hash[2], hash[3], hash[4]]) >> 5;
    std::array::from_fn(|i| ALPHABET[((bits >> (5 * (KIT_CHECKSUM_CHARS - 1 - i))) & 31) as usize])
}

/// A 16-digit account as its integer, refused otherwise.
fn account_number(account: &str) -> Result<u64> {
    if account.len() != ACCOUNT_DIGITS || !account.bytes().all(|b| b.is_ascii_digit()) {
        return Err(account_invalid());
    }
    account.parse::<u64>().map_err(|_| account_invalid())
}

/// The 95-character v2 code, no separators. `account` must be 16 digits
/// (`validation.syncRecoveryAccountInvalid`).
pub fn encode_kit_code(
    vault_id: &VaultId,
    seed: &KitSecret,
    account: &str,
) -> Result<Zeroizing<String>> {
    let number = account_number(account)?;
    let mut payload = Zeroizing::new([0u8; KIT_PAYLOAD_BYTES]);
    payload[..16].copy_from_slice(&vault_id.0);
    payload[16..48].copy_from_slice(seed.expose());
    payload[48..].copy_from_slice(&number.to_be_bytes()[1..]);
    let mut code = Zeroizing::new(String::with_capacity(KIT_CODE_LEN));
    for c in 0..KIT_PAYLOAD_CHARS {
        let value = (0..5).fold(0usize, |acc, k| {
            let i = c * 5 + k;
            (acc << 1) | usize::from((payload[i / 8] >> (7 - i % 8)) & 1)
        });
        code.push(char::from(ALPHABET[value]));
    }
    for c in kit_checksum(&payload) {
        code.push(char::from(c));
    }
    Ok(code)
}

/// Reads a typed or scanned v2 code: separators, case and the confusable letters as
/// the v1 reader treated them. Needs exactly 95 characters, a matching checksum and an
/// account below 10^16, else `validation.syncRecoveryCodeInvalid` (a v1 code included; the input
/// is never put into the error).
pub fn decode_kit_code(input: &str) -> Result<KitCode> {
    let mut values = Zeroizing::new(Vec::with_capacity(KIT_CODE_LEN));
    for c in input.chars() {
        if is_separator(c) {
            continue;
        }
        let c = match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        let value = u8::try_from(c)
            .ok()
            .and_then(|b| ALPHABET.iter().position(|a| *a == b))
            .ok_or_else(invalid)?;
        if values.len() == KIT_CODE_LEN {
            return Err(invalid());
        }
        values.push(value as u8);
    }
    if values.len() != KIT_CODE_LEN {
        return Err(invalid());
    }
    let mut payload = Zeroizing::new([0u8; KIT_PAYLOAD_BYTES]);
    for i in 0..KIT_PAYLOAD_BYTES * 8 {
        let bit = (values[i / 5] >> (4 - i % 5)) & 1;
        payload[i / 8] |= bit << (7 - i % 8);
    }
    let expected = kit_checksum(&payload);
    let given = &values[KIT_PAYLOAD_CHARS..];
    if !expected
        .iter()
        .zip(given)
        .all(|(e, g)| ALPHABET[usize::from(*g)] == *e)
    {
        return Err(invalid());
    }
    let mut number = [0u8; 8];
    number[1..].copy_from_slice(&payload[48..]);
    let number = u64::from_be_bytes(number);
    if number >= ACCOUNT_LIMIT {
        return Err(invalid());
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&payload[..16]);
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&payload[16..48]);
    Ok(KitCode {
        vault_id: VaultId(id),
        seed: KitSecret::from_bytes(seed),
        account: format!("{number:016}"),
    })
}

/// What the v2 Kit's QR encodes: `moony-recover:2:<code>`, and for a server that is not the
/// default one `:<base64url(server address)>` after it.
pub fn kit_qr_payload(code: &str, relay_url: Option<&str>) -> Zeroizing<String> {
    let relay_len = relay_url.map_or(0, |url| 1 + url.len().div_ceil(3) * 4);
    let mut out = Zeroizing::new(String::with_capacity(
        KIT_QR_PREFIX.len() + code.len() + relay_len,
    ));
    out.push_str(KIT_QR_PREFIX);
    out.push_str(code);
    if let Some(url) = relay_url {
        out.push(':');
        URL_SAFE_NO_PAD.encode_string(url.as_bytes(), &mut out);
    }
    out
}

/// A scanned v2 Kit QR. `Debug` prints nothing of it.
pub struct KitQr {
    pub code: KitCode,
    /// The server address the Kit names, normalized; `None` for the default server.
    pub relay_url: Option<String>,
}

impl std::fmt::Debug for KitQr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KitQr { .. }")
    }
}

/// Reads a v2 Kit QR payload (`kit_qr_payload`), at most `MAX_QR_PAYLOAD_BYTES` (checked first):
/// a v2 code and, optionally, a base64url server address. Anything else, a v1 QR included, is
/// `validation.syncRecoveryCodeInvalid`; a server address the app does not use is
/// `validation.syncRelayUrlInvalid`. The default server reads as `None`. No error carries the
/// input.
pub fn parse_kit_qr_payload(input: &str) -> Result<KitQr> {
    if input.len() > MAX_QR_PAYLOAD_BYTES {
        return Err(invalid());
    }
    let rest = input.strip_prefix(KIT_QR_PREFIX).ok_or_else(invalid)?;
    let mut fields = rest.split(':');
    let code = fields.next().ok_or_else(invalid)?;
    let relay = fields.next();
    if fields.next().is_some() {
        return Err(invalid());
    }
    let code = decode_kit_code(code)?;
    let relay_url = match relay {
        None => None,
        Some(encoded) => {
            let bytes = URL_SAFE_NO_PAD
                .decode(encoded.as_bytes())
                .map_err(|_| invalid())?;
            let url = String::from_utf8(bytes).map_err(|_| invalid())?;
            if url.is_empty() {
                return Err(invalid());
            }
            Some(normalize_base_url(&url)?).filter(|url| !is_default_relay(url))
        }
    };
    Ok(KitQr { code, relay_url })
}

/// What a person gave to join with a v2 Kit (`read_kit_input`). `Debug` prints nothing of it.
pub struct KitInput {
    /// Normalized (`normalize_base_url`).
    pub relay_url: String,
    pub code: KitCode,
}

impl std::fmt::Debug for KitInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KitInput { .. }")
    }
}

/// Reads what the person scanned or typed from a v2 Kit: a QR (the trimmed `code` starts
/// `moony-recover:`) brings its own server (the default one when it names none) and ignores
/// `relay_url`; a typed code takes `relay_url` (a server address or connection code), or
/// `default_relay` when it is absent or blank. The account is inside the code. No error carries
/// the input.
pub fn read_kit_input(
    code: &str,
    relay_url: Option<&str>,
    default_relay: &str,
) -> Result<KitInput> {
    let code = code.trim();
    if code.starts_with(QR_SCHEME) {
        let qr = parse_kit_qr_payload(code)?;
        return Ok(KitInput {
            relay_url: qr
                .relay_url
                .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string()),
            code: qr.code,
        });
    }
    let code = decode_kit_code(code)?;
    let relay_url = match relay_url.filter(|url| !url.trim().is_empty()) {
        Some(url) => server_address_from_input(url)?,
        None => normalize_base_url(default_relay)?,
    };
    Ok(KitInput { relay_url, code })
}

/// The payload as an SVG QR code (error correction M, quiet zone, at least 240 by 240). The
/// SVG holds the secret: the caller treats it like the code.
pub fn qr_svg(payload: &str) -> Result<String> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|e| AppError::Internal(format!("sync: the recovery QR does not fit: {e}")))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(QR_MIN_SIDE, QR_MIN_SIDE)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str = "48H24-8H248-H248H-248H2-48H24-88H24-8H248-H248H-248H2-48H24-8H248-H248H-248H2-48H24-8H248-H2XSY";
    const V2: &str =
        "000G40R40M30E209185GR38E1W8124GK2GAHC5RR34D1P70X3RFJ08924CJ2A9H750MJMASC5MQ2YWPJ";

    const ACCOUNT: &str = "4417938205561273";
    const DEFAULT: &str = "https://default.relay.test";

    // ── Format v2 ────────────────────────────────────────────────────────────────────────

    fn kit_v2() -> (VaultId, [u8; 32]) {
        (
            VaultId(std::array::from_fn(|i| i as u8)),
            std::array::from_fn(|i| (i + 16) as u8),
        )
    }

    fn kit_code(account: &str) -> Zeroizing<String> {
        let (id, seed) = kit_v2();
        encode_kit_code(&id, &KitSecret::from_bytes(Zeroizing::new(seed)), account).unwrap()
    }

    /// Generated once and cross-checked with Python (`hashlib`, base32 by hand), then frozen.
    const KIT_V2: &str = "000G4-0R40M-30E20-9185G-R38E1-W8124-GK2GA-HC5RR-34D1P-70X3R-FJ089-24CJ2-A9H75-0MJMA-SC5MQ-2Y3XJ-2XJP3-MDSWX-01YQT";
    const KIT_V2_ZERO_ACCOUNT: &str =
        "000G40R40M30E209185GR38E1W8124GK2GAHC5RR34D1P70X3RFJ08924CJ2A9H750MJMASC5MQ2Y0000000000097XHY0T";

    fn assert_kit(code: &KitCode, account: &str) {
        let (id, seed) = kit_v2();
        assert_eq!(code.vault_id, id);
        assert_eq!(code.seed.expose(), &seed);
        assert_eq!(code.account, account);
    }

    #[test]
    fn the_v2_code_matches_its_frozen_vectors_and_round_trips() {
        let code = kit_code(ACCOUNT);
        assert_eq!(code.len(), KIT_CODE_LEN);
        let grouped = format_recovery_code(&code);
        assert_eq!(*grouped, KIT_V2);
        assert_eq!(grouped.split('-').count(), 19, "19 groups of five");
        assert_eq!(*kit_code("0000000000000000"), KIT_V2_ZERO_ACCOUNT);
        assert_kit(&decode_kit_code(KIT_V2).unwrap(), ACCOUNT);
        assert_kit(&decode_kit_code(&code).unwrap(), ACCOUNT);
        assert_kit(
            &decode_kit_code(KIT_V2_ZERO_ACCOUNT).unwrap(),
            "0000000000000000",
        );
        // The same reading rules as v1: case, spaces, dashes, confusable letters.
        let messy = format!(" {}\t{}\n", KIT_V2[..40].to_lowercase(), &KIT_V2[40..]);
        assert_kit(&decode_kit_code(&messy).unwrap(), ACCOUNT);
        let confusable = KIT_V2.replacen('0', "O", 3).replace('1', "l");
        assert_kit(&decode_kit_code(&confusable).unwrap(), ACCOUNT);
        let typed = KIT_V2.replace('-', "\u{2013}");
        assert_kit(&decode_kit_code(&typed).unwrap(), ACCOUNT);
    }

    #[test]
    fn every_single_substitution_and_transposition_of_the_v2_code_is_rejected() {
        let code: Vec<u8> = KIT_V2.bytes().filter(|b| *b != b'-').collect();
        let mut rejected = 0;
        for pos in 0..KIT_CODE_LEN {
            for replacement in ALPHABET.iter().filter(|c| **c != code[pos]) {
                let mut mutated = code.clone();
                mutated[pos] = *replacement;
                if decode_kit_code(std::str::from_utf8(&mutated).unwrap()).is_err() {
                    rejected += 1;
                }
            }
        }
        assert_eq!(rejected, KIT_CODE_LEN * 31);
        for pos in 0..KIT_CODE_LEN - 1 {
            if code[pos] == code[pos + 1] {
                continue;
            }
            let mut swapped = code.clone();
            swapped.swap(pos, pos + 1);
            assert!(
                decode_kit_code(std::str::from_utf8(&swapped).unwrap()).is_err(),
                "a transposition at {pos} passed"
            );
        }
    }

    #[test]
    fn a_v1_code_or_qr_is_refused_by_the_v2_reader() {
        let key = |r: Result<KitCode>| match r {
            Err(AppError::Validation(k)) => k,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            key(decode_kit_code(V2)),
            "validation.syncRecoveryCodeInvalid"
        );
        assert_eq!(
            key(decode_kit_code(V1)),
            "validation.syncRecoveryCodeInvalid"
        );
        let v1_qr = format!("moony-recover:1:{ACCOUNT}:{V2}");
        assert!(matches!(
            parse_kit_qr_payload(&v1_qr),
            Err(AppError::Validation(k)) if k == "validation.syncRecoveryCodeInvalid"
        ));
        assert!(matches!(
            read_kit_input(&v1_qr, None, DEFAULT),
            Err(AppError::Validation(k)) if k == "validation.syncRecoveryCodeInvalid"
        ));
        // Wrong lengths and foreign characters.
        let plain: String = KIT_V2.chars().filter(|c| *c != '-').collect();
        for bad in [
            String::new(),
            plain[..94].to_string(),
            format!("{plain}0"),
            format!("{}U", &plain[..94]),
        ] {
            assert_eq!(
                key(decode_kit_code(&bad)),
                "validation.syncRecoveryCodeInvalid"
            );
        }
    }

    #[test]
    fn an_account_of_seventeen_digits_never_reads() {
        // 10^16 fits the 7 bytes but is no account: the checksum is right, the account is not.
        let (id, seed) = kit_v2();
        let mut payload = [0u8; KIT_PAYLOAD_BYTES];
        payload[..16].copy_from_slice(&id.0);
        payload[16..48].copy_from_slice(&seed);
        payload[48..].copy_from_slice(&ACCOUNT_LIMIT.to_be_bytes()[1..]);
        let mut code = String::new();
        for c in 0..KIT_PAYLOAD_CHARS {
            let value = (0..5).fold(0usize, |acc, k| {
                let i = c * 5 + k;
                (acc << 1) | usize::from((payload[i / 8] >> (7 - i % 8)) & 1)
            });
            code.push(char::from(ALPHABET[value]));
        }
        for c in kit_checksum(&payload) {
            code.push(char::from(c));
        }
        assert!(decode_kit_code(&code).is_err());
        // And the writer refuses anything but 16 digits.
        let seed = KitSecret::from_bytes(Zeroizing::new(seed));
        for account in ["", "123", "44179382055612730", "441793820556127a"] {
            assert!(matches!(
                encode_kit_code(&id, &seed, account),
                Err(AppError::Validation(k)) if k == "validation.syncRecoveryAccountInvalid"
            ));
        }
    }

    #[test]
    fn the_v2_qr_carries_the_code_and_the_server() {
        let code = kit_code(ACCOUNT);
        let plain = kit_qr_payload(&code, None);
        assert_eq!(*plain, format!("moony-recover:2:{}", code.as_str()));
        let qr = parse_kit_qr_payload(&plain).expect("default server");
        assert_kit(&qr.code, ACCOUNT);
        assert_eq!(qr.relay_url, None);
        assert_eq!(format!("{qr:?}"), "KitQr { .. }");

        let custom = kit_qr_payload(&code, Some("https://sync.example.org"));
        assert!(custom.ends_with(":aHR0cHM6Ly9zeW5jLmV4YW1wbGUub3Jn"));
        let qr = parse_kit_qr_payload(&custom).expect("custom server");
        assert_kit(&qr.code, ACCOUNT);
        assert_eq!(qr.relay_url.as_deref(), Some("https://sync.example.org"));

        // The grouped code reads too; the longest payload fits the cap.
        let grouped = kit_qr_payload(&format_recovery_code(&code), Some(&"a".repeat(256)));
        assert!(grouped.len() <= MAX_QR_PAYLOAD_BYTES, "{}", grouped.len());

        for bad in [
            format!("moony-recover:2:{}:", code.as_str()),
            format!("moony-recover:2:{}:x:y", code.as_str()),
            format!("moony-recover:3:{}", code.as_str()),
            format!("moony-recover:2:{}", &code[..94]),
        ] {
            assert!(parse_kit_qr_payload(&bad).is_err(), "{bad}");
        }
        let long = format!("moony-recover:2:{}{}", code.as_str(), " ".repeat(512));
        assert!(parse_kit_qr_payload(&long).is_err());
    }

    #[test]
    fn a_v2_kit_input_reads_a_qr_or_a_typed_code_and_its_server() {
        let code = kit_code(ACCOUNT);
        let qr = kit_qr_payload(&code, Some("https://Relay.Example.org/"));
        let input = read_kit_input(
            &format!("  {}\n", qr.as_str()),
            Some("https://x.test"),
            DEFAULT,
        )
        .expect("QR");
        assert_eq!(input.relay_url, "https://relay.example.org");
        assert_kit(&input.code, ACCOUNT);
        let input = read_kit_input(&kit_qr_payload(&code, None), None, DEFAULT).expect("QR");
        assert_eq!(input.relay_url, DEFAULT_RELAY_URL);
        assert_eq!(format!("{input:?}"), "KitInput { .. }");

        let grouped = format_recovery_code(&code);
        for server in [None, Some(""), Some("  ")] {
            let input = read_kit_input(&grouped, server, DEFAULT).expect("typed");
            assert_eq!(input.relay_url, DEFAULT);
            assert_kit(&input.code, ACCOUNT);
        }
        let input = read_kit_input(&grouped, Some("http://192.168.1.20:8787"), DEFAULT)
            .expect("own server");
        assert_eq!(input.relay_url, "http://192.168.1.20:8787");
        assert!(matches!(
            read_kit_input(&grouped, Some("ftp://evil.example"), DEFAULT),
            Err(AppError::Validation(k)) if k == "validation.syncRelayUrlInvalid"
        ));
    }

    #[test]
    fn nothing_prints_the_v2_secrets() {
        let decoded = decode_kit_code(KIT_V2).unwrap();
        assert_eq!(format!("{decoded:?}"), "KitCode { .. }");
        let err = decode_kit_code(&KIT_V2[..60]).unwrap_err();
        assert!(!format!("{err:?}{err}").contains(&KIT_V2[..20]));
    }
}
