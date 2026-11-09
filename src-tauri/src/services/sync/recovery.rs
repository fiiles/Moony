//! The Sync Recovery Kit's recovery code and QR (spec §11.5, D24, ruling P4), and the relay
//! address a kit for a self-hosted relay carries (ruling SH-2).
//!
//! The code is the vault ID (16 bytes) followed by the current vault key (32 bytes), 384 bits
//! written big-endian as Crockford base32 with one zero padding bit (77 characters), then a
//! 3-character checksum: 15 bits of `SHA-256("moony recovery code v1" ‖ vault_id ‖ key)`.
//!
//! The checksum guards against typos and is NOT an authenticator: it proves nothing about who
//! made the code. Detection is probabilistic: about 1 typo in 32,768 passes the checksum (a code
//! with such a typo then fails to decrypt anything, which is a clear error). The code and the QR
//! payload are secrets (ruling P16): they live in `Zeroizing` buffers, and no error carries the
//! input. A `Zeroizing<String>` forwards `Debug` to its content, so callers must never format
//! the returned code or payload; the decoded key comes back in the redacting `RecoveryKey`.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::keys::VaultId;
use super::relay::http::{is_default_relay, normalize_base_url};
use crate::error::{AppError, Result};

/// Crockford base32 characters of the code, checksum included.
pub const RECOVERY_CODE_LEN: usize = 80;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const PAYLOAD_BYTES: usize = 48;
const PAYLOAD_CHARS: usize = 77;
const LABEL: &[u8] = b"moony recovery code v1";
const GROUP: usize = 5;
const QR_MIN_SIDE: u32 = 240;

/// The vault key decoded from a recovery code. Wiped on drop; `Debug` prints `<redacted>`.
pub struct RecoveryKey(Zeroizing<[u8; 32]>);

impl RecoveryKey {
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKey(<redacted>)")
    }
}

fn invalid() -> AppError {
    AppError::Validation("validation.syncRecoveryCodeInvalid".into())
}

/// The three checksum characters for a 48-byte payload (vault ID ‖ key).
fn checksum(payload: &[u8; PAYLOAD_BYTES]) -> [u8; 3] {
    let hash = Sha256::new()
        .chain_update(LABEL)
        .chain_update(payload)
        .finalize();
    let bits = u16::from_be_bytes([hash[0], hash[1]]) >> 1;
    [
        ALPHABET[usize::from((bits >> 10) & 31)],
        ALPHABET[usize::from((bits >> 5) & 31)],
        ALPHABET[usize::from(bits & 31)],
    ]
}

/// The 80-character code, no separators.
pub fn encode_recovery_code(vault_id: &VaultId, key: &[u8; 32]) -> Zeroizing<String> {
    let mut payload = Zeroizing::new([0u8; PAYLOAD_BYTES]);
    payload[..16].copy_from_slice(&vault_id.0);
    payload[16..].copy_from_slice(key);
    let bit = |i: usize| -> usize {
        // Bit 384 is the zero padding bit.
        if i < PAYLOAD_BYTES * 8 {
            usize::from((payload[i / 8] >> (7 - i % 8)) & 1)
        } else {
            0
        }
    };
    let mut code = Zeroizing::new(String::with_capacity(RECOVERY_CODE_LEN));
    for c in 0..PAYLOAD_CHARS {
        let value = (0..5).fold(0, |acc, k| (acc << 1) | bit(c * 5 + k));
        code.push(char::from(ALPHABET[value]));
    }
    for c in checksum(&payload) {
        code.push(char::from(c));
    }
    code
}

/// The code as 16 groups of five joined by `-`, for printing.
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

/// Reads a typed or scanned code. Case, whitespace, `-` and the dashes U+2010 to U+2015 are ignored; `I` and `L` read as `1`,
/// `O` as `0`. Needs exactly 80 characters, a zero padding bit and a matching checksum, else
/// `validation.syncRecoveryCodeInvalid` (the input is never put into the error).
pub fn decode_recovery_code(input: &str) -> Result<(VaultId, RecoveryKey)> {
    let mut values = Zeroizing::new(Vec::with_capacity(RECOVERY_CODE_LEN));
    for c in input.chars() {
        if c == '-' || ('\u{2010}'..='\u{2015}').contains(&c) || c.is_whitespace() {
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
        if values.len() == RECOVERY_CODE_LEN {
            return Err(invalid());
        }
        values.push(value as u8);
    }
    if values.len() != RECOVERY_CODE_LEN {
        return Err(invalid());
    }
    // The 77th character carries the padding bit as its lowest bit.
    if values[PAYLOAD_CHARS - 1] & 1 != 0 {
        return Err(invalid());
    }
    let mut payload = Zeroizing::new([0u8; PAYLOAD_BYTES]);
    for i in 0..PAYLOAD_BYTES * 8 {
        let bit = (values[i / 5] >> (4 - i % 5)) & 1;
        payload[i / 8] |= bit << (7 - i % 8);
    }
    let expected = checksum(&payload);
    let given = &values[PAYLOAD_CHARS..];
    if !expected
        .iter()
        .zip(given)
        .all(|(e, g)| ALPHABET[usize::from(*g)] == *e)
    {
        return Err(invalid());
    }
    let mut id = [0u8; 16];
    id.copy_from_slice(&payload[..16]);
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&payload[16..]);
    Ok((VaultId(id), RecoveryKey(key)))
}

const QR_PREFIX: &str = "moony-recover:1:";
/// The longest payload read. The longest one written is 471 bytes: the prefix, a 16-digit
/// account, a grouped 95-character code and the base64url of a 256-character relay address.
pub const MAX_QR_PAYLOAD_BYTES: usize = 512;

/// What the Recovery Kit's QR encodes: `moony-recover:1:<account>:<code>`, and for a relay that is
/// not the default one `:<base64url(relay address)>` after it (ruling SH-2).
pub fn recovery_qr_payload(
    account: &str,
    code: &str,
    relay_url: Option<&str>,
) -> Zeroizing<String> {
    let relay_len = relay_url.map_or(0, |url| 1 + url.len().div_ceil(3) * 4);
    let mut out = Zeroizing::new(String::with_capacity(
        QR_PREFIX.len() + account.len() + 1 + code.len() + relay_len,
    ));
    out.push_str(QR_PREFIX);
    out.push_str(account);
    out.push(':');
    out.push_str(code);
    if let Some(url) = relay_url {
        out.push(':');
        URL_SAFE_NO_PAD.encode_string(url.as_bytes(), &mut out);
    }
    out
}

/// A scanned Recovery Kit QR. `Debug` prints nothing of it.
pub struct RecoveryQr {
    pub account: String,
    pub vault_id: VaultId,
    pub key: RecoveryKey,
    /// The relay address the kit names, normalized; `None` for the default relay.
    pub relay_url: Option<String>,
}

impl std::fmt::Debug for RecoveryQr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryQr { .. }")
    }
}

/// Reads a Recovery Kit QR payload of either form (`recovery_qr_payload`), at most
/// `MAX_QR_PAYLOAD_BYTES` (checked before anything is decoded): a 16-digit account, a
/// recovery code (`decode_recovery_code`) and, optionally, a base64url relay address. Anything
/// else is `validation.syncRecoveryCodeInvalid`; a relay address the app does not use
/// (`validate_base_url`) is `validation.syncRelayUrlInvalid`. A relay address equal to the default
/// relay reads as `None`. No error carries the input.
pub fn parse_recovery_qr_payload(input: &str) -> Result<RecoveryQr> {
    if input.len() > MAX_QR_PAYLOAD_BYTES {
        return Err(invalid());
    }
    let rest = input.strip_prefix(QR_PREFIX).ok_or_else(invalid)?;
    let mut fields = rest.split(':');
    let account = fields.next().ok_or_else(invalid)?;
    let code = fields.next().ok_or_else(invalid)?;
    let relay = fields.next();
    if fields.next().is_some() {
        return Err(invalid());
    }
    if account.len() != 16 || !account.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    let (vault_id, key) = decode_recovery_code(code)?;
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
    Ok(RecoveryQr {
        account: account.to_string(),
        vault_id,
        key,
        relay_url,
    })
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

    fn v1() -> (VaultId, [u8; 32]) {
        (VaultId([0x22; 16]), [0x11; 32])
    }

    fn v2() -> (VaultId, [u8; 32]) {
        (
            VaultId(std::array::from_fn(|i| i as u8)),
            std::array::from_fn(|i| (i + 16) as u8),
        )
    }

    #[test]
    fn recovery_code_matches_frozen_vectors() {
        let (id, key) = v1();
        let code = encode_recovery_code(&id, &key);
        assert_eq!(code.len(), RECOVERY_CODE_LEN);
        assert_eq!(*format_recovery_code(&code), V1);
        let (id, key) = v2();
        assert_eq!(*encode_recovery_code(&id, &key), V2);
    }

    #[test]
    fn the_vectors_round_trip() {
        for (v, (id, key)) in [(V1, v1()), (V2, v2())] {
            let (got_id, got_key) = decode_recovery_code(v).unwrap();
            assert_eq!(got_id, id);
            assert_eq!(*got_key.expose(), key);
        }
    }

    #[test]
    fn decoding_accepts_lowercase_dashes_spaces_and_confusable_letters() {
        let (id, key) = v2();
        // Lowercase, spaces and dashes in odd places, tabs and newlines.
        let messy = format!(
            " {}-\t{}\n",
            V2[..30].to_lowercase(),
            V2[30..].to_lowercase()
        );
        let (got_id, got_key) = decode_recovery_code(&messy).unwrap();
        assert_eq!((got_id, *got_key.expose()), (id, key));
        // V2 starts with "000G40R40M30E2091…": spell its zeros as O and o, its ones as I and l.
        let confusable = V2
            .replacen('0', "O", 2)
            .replacen('0', "o", 1)
            .replace('1', "I");
        assert_ne!(confusable, V2);
        assert!(decode_recovery_code(&confusable).is_ok());
        let confusable = V2.replace('1', "l");
        assert!(decode_recovery_code(&confusable).is_ok());
    }

    #[test]
    fn every_single_substitution_of_the_two_frozen_vectors_is_rejected() {
        for vector in [V1, V2] {
            let code: Vec<u8> = vector.bytes().filter(|b| *b != b'-').collect();
            assert_eq!(code.len(), RECOVERY_CODE_LEN);
            let mut rejected = 0;
            for pos in 0..RECOVERY_CODE_LEN {
                for replacement in ALPHABET.iter().filter(|c| **c != code[pos]) {
                    let mut mutated = code.clone();
                    mutated[pos] = *replacement;
                    let text = String::from_utf8(mutated).unwrap();
                    if decode_recovery_code(&text).is_err() {
                        rejected += 1;
                    }
                }
            }
            assert_eq!(rejected, 2_480);
        }
    }

    #[test]
    fn every_adjacent_transposition_of_the_frozen_vectors_is_rejected() {
        for vector in [V1, V2] {
            let code: Vec<u8> = vector.bytes().filter(|b| *b != b'-').collect();
            let mut tried = 0;
            for pos in 0..RECOVERY_CODE_LEN - 1 {
                if code[pos] == code[pos + 1] {
                    continue;
                }
                let mut swapped = code.clone();
                swapped.swap(pos, pos + 1);
                tried += 1;
                let text = String::from_utf8(swapped).unwrap();
                assert!(
                    decode_recovery_code(&text).is_err(),
                    "a transposition at {pos} passed the checksum"
                );
            }
            assert!(tried > 0);
        }
    }

    #[test]
    fn typographic_dashes_are_skipped_like_a_hyphen() {
        for dash in [
            '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}',
        ] {
            let typed = format_recovery_code(V2).replace('-', &dash.to_string());
            assert!(decode_recovery_code(&typed).is_ok(), "{dash:?}");
        }
        let (id, _) = v2();
        let typed = format!("{}\u{2013}{}", &V2[..40], &V2[40..]);
        assert_eq!(decode_recovery_code(&typed).unwrap().0, id);
    }

    #[test]
    fn wrong_lengths_and_foreign_characters_are_refused() {
        let plain: String = V2.to_string();
        for bad in [
            String::new(),
            plain[..79].to_string(),
            format!("{plain}0"),
            format!("{}U", &plain[..79]),
            format!("{}!", &plain[..79]),
        ] {
            assert!(
                matches!(decode_recovery_code(&bad), Err(AppError::Validation(k)) if k == "validation.syncRecoveryCodeInvalid")
            );
        }
    }

    #[test]
    fn a_set_padding_bit_is_refused() {
        // Character 77 (index 76) ends in the padding bit: flip it and fix the checksum, so only
        // the padding rule can reject the code.
        let (id, key) = v2();
        let mut code = encode_recovery_code(&id, &key).as_bytes().to_vec();
        let last = ALPHABET.iter().position(|c| *c == code[76]).unwrap();
        assert_eq!(last & 1, 0);
        code[76] = ALPHABET[last | 1];
        let text = String::from_utf8(code).unwrap();
        assert!(decode_recovery_code(&text).is_err());
    }

    #[test]
    fn the_qr_payload_and_svg_are_well_formed() {
        let payload = recovery_qr_payload("1234-5678", V2, None);
        assert_eq!(*payload, format!("moony-recover:1:1234-5678:{V2}"));
        let svg = qr_svg(&payload).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("</svg>"));
        // The code is drawn as modules, never written out as text.
        assert!(!svg.contains("moony-recover"));
        assert!(!svg.contains("1234-5678"));
        for window in V2.as_bytes().windows(8) {
            assert!(!svg.contains(std::str::from_utf8(window).unwrap()));
        }
        let side = |attr: &str| -> u32 {
            let tail = svg.split(&format!("{attr}=\"")).nth(1).unwrap();
            tail.split('"').next().unwrap().parse().unwrap()
        };
        assert!(side("width") >= 240 && side("height") >= 240);
    }

    #[test]
    fn nothing_prints_the_secret() {
        let (id, _) = v2();
        assert_eq!(format!("{id:?}"), "VaultId(<redacted>)");
        let err = decode_recovery_code(&format!("{}!", &V2[..79])).unwrap_err();
        assert!(!format!("{err:?}{err}").contains(&V2[..20]));
        let (_, key) = decode_recovery_code(V2).unwrap();
        assert_eq!(format!("{key:?}"), "RecoveryKey(<redacted>)");
    }

    const ACCOUNT: &str = "4417938205561273";

    fn qr_key(qr: &RecoveryQr) -> [u8; 32] {
        *qr.key.expose()
    }

    #[test]
    fn a_custom_relay_rides_in_the_qr_payload_as_base64url() {
        let plain = V2;
        let payload = recovery_qr_payload(ACCOUNT, plain, Some("https://relay.example.org"));
        assert_eq!(
            *payload,
            format!("moony-recover:1:{ACCOUNT}:{plain}:aHR0cHM6Ly9yZWxheS5leGFtcGxlLm9yZw")
        );
        assert_eq!(
            *recovery_qr_payload(ACCOUNT, plain, None),
            format!("moony-recover:1:{ACCOUNT}:{plain}"),
            "the default relay leaves the fourth field out"
        );
    }

    #[test]
    fn qr_payloads_of_both_forms_parse_back() {
        let (id, key) = v2();
        let default = recovery_qr_payload(ACCOUNT, V2, None);
        let qr = parse_recovery_qr_payload(&default).expect("default relay form");
        assert_eq!(qr.account, ACCOUNT);
        assert_eq!((qr.vault_id, qr_key(&qr)), (id, key));
        assert_eq!(qr.relay_url, None);

        let custom = recovery_qr_payload(ACCOUNT, V2, Some("http://localhost:8787"));
        assert!(custom.ends_with(":aHR0cDovL2xvY2FsaG9zdDo4Nzg3"));
        let qr = parse_recovery_qr_payload(&custom).expect("custom relay form");
        assert_eq!(qr.account, ACCOUNT);
        assert_eq!((qr.vault_id, qr_key(&qr)), (id, key));
        assert_eq!(qr.relay_url.as_deref(), Some("http://localhost:8787"));

        // A grouped code and the default relay written out read as well.
        let grouped = format!(
            "moony-recover:1:{ACCOUNT}:{}:aHR0cHM6Ly9zeW5jLm1vb255ZmluYW5jZS5jb20",
            format_recovery_code(V2).as_str()
        );
        let qr = parse_recovery_qr_payload(&grouped).expect("grouped code");
        assert_eq!(qr.relay_url, None, "the default relay is the default");
        assert_eq!(format!("{qr:?}"), "RecoveryQr { .. }");
    }

    #[test]
    fn garbage_qr_payloads_are_refused() {
        let invalid = |input: &str| {
            let error = parse_recovery_qr_payload(input).expect_err(input);
            let key = match &error {
                AppError::Validation(key) => key.clone(),
                other => panic!("{input}: {other:?}"),
            };
            assert!(!format!("{error:?}").contains(&V2[..20]), "{input}");
            key
        };
        let code = "validation.syncRecoveryCodeInvalid";
        for input in [
            String::new(),
            "moony-recover".to_string(),
            format!("moony-recover:2:{ACCOUNT}:{V2}"),
            format!("other:1:{ACCOUNT}:{V2}"),
            format!("moony-recover:1:{ACCOUNT}"),
            format!("moony-recover:1:123:{V2}"),
            format!("moony-recover:1:44179382055612730:{V2}"),
            format!("moony-recover:1:{ACCOUNT}:{}", &V2[..79]),
            format!("moony-recover:1:{ACCOUNT}:{V2}:"),
            format!("moony-recover:1:{ACCOUNT}:{V2}:not base64!"),
            format!("moony-recover:1:{ACCOUNT}:{V2}:aHR0cHM6Ly9yZWxheS5leGFtcGxlLm9yZw:x"),
            // Not UTF-8 once decoded.
            format!("moony-recover:1:{ACCOUNT}:{V2}:__8"),
        ] {
            assert_eq!(invalid(&input), code, "{input}");
        }
        // A relay address the app would never use (plain http to another host).
        let http = format!("moony-recover:1:{ACCOUNT}:{V2}:aHR0cDovL3JlbGF5LmV4YW1wbGUub3Jn");
        assert_eq!(invalid(&http), "validation.syncRelayUrlInvalid");
    }

    #[test]
    fn a_payload_over_512_bytes_is_refused_before_it_is_read() {
        let valid = recovery_qr_payload(ACCOUNT, V2, Some("https://relay.example.org"));
        assert!(parse_recovery_qr_payload(&valid).is_ok());
        // Valid in every field, only too long: padded with spaces the code reader would skip.
        let padded_code = format!("{V2}{}", " ".repeat(512));
        let long = format!("moony-recover:1:{ACCOUNT}:{padded_code}");
        assert!(decode_recovery_code(&padded_code).is_ok());
        assert!(long.len() > 512);
        let error = parse_recovery_qr_payload(&long).expect_err("too long");
        assert!(
            matches!(&error, AppError::Validation(k) if k == "validation.syncRecoveryCodeInvalid")
        );
        let at_cap = format!("moony-recover:1:{ACCOUNT}:{V2}{}", " ".repeat(512 - 113));
        assert_eq!(at_cap.len(), 512);
        assert!(
            parse_recovery_qr_payload(&at_cap).is_ok(),
            "512 bytes is fine"
        );
    }

    #[test]
    fn a_relay_written_with_a_trailing_dot_is_the_same_relay() {
        let dotted = recovery_qr_payload(ACCOUNT, V2, Some("https://sync.moonyfinance.com."));
        let qr = parse_recovery_qr_payload(&dotted).expect("reads");
        assert_eq!(qr.relay_url, None, "the default relay");
    }
}
