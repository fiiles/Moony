//! The Sync Recovery Kit's recovery code and QR (spec §11.5, D24, ruling P4).
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

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::keys::VaultId;
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

/// What the Recovery Kit's QR encodes: `moony-recover:1:{account}:{code}`.
pub fn recovery_qr_payload(account: &str, code: &str) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(16 + account.len() + code.len() + 1));
    out.push_str("moony-recover:1:");
    out.push_str(account);
    out.push(':');
    out.push_str(code);
    out
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
        let payload = recovery_qr_payload("1234-5678", V2);
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
}
