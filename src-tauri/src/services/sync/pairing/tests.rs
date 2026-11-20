use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use std::collections::BTreeMap;

use sha2::Digest;

use super::*;
use crate::error::AppError;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::cert::{self, CertFields, CertKind, TicketSecret};
use crate::services::sync::identity::trust::{ExportEntry, TrustExport, TrustState};
use crate::services::sync::identity::{sign_pub, SignSeed};
use crate::services::sync::keys::{Credentials, VaultId, VaultKeys};
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;

const S_SECRET: [u8; 32] = [0x11; 32];
const C_SECRET: [u8; 32] = [0x22; 32];
const M_SECRET: [u8; 32] = [0x44; 32];
const MAILBOX: MailboxId = MailboxId([0x33; 16]);

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex32(s: &str) -> [u8; 32] {
    unhex(s).try_into().unwrap()
}

fn public(secret: [u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(secret)).to_bytes()
}

fn s_pub() -> [u8; 32] {
    public(S_SECRET)
}

fn c_pub() -> [u8; 32] {
    public(C_SECRET)
}

fn shower() -> Transcript {
    derive(
        &StaticSecret::from(S_SECRET),
        Side::Shower,
        &c_pub(),
        &MAILBOX,
    )
    .unwrap()
}

fn scanner() -> Transcript {
    derive(
        &StaticSecret::from(C_SECRET),
        Side::Scanner,
        &s_pub(),
        &MAILBOX,
    )
    .unwrap()
}

fn key(t: &Transcript) -> String {
    t.key_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn validation_key(err: AppError) -> String {
    match err {
        AppError::Validation(k) => k,
        other => panic!("not a validation error: {other:?}"),
    }
}

fn creds() -> Credentials {
    let keys = VaultKeys::from_parts(
        1,
        std::collections::BTreeMap::from([(0, [0xA0; 32]), (1, [0xA1; 32])]),
    )
    .unwrap();
    Credentials {
        relay_url: "https://sync.home.example.com".into(),
        account: "1234567890123456".into(),
        vault_id: VaultId([0x77; 16]),
        keys,
    }
}

/// `creds()` from a holder without an identity, epoch 0's head known.
fn plain_payload() -> SlotPayload {
    SlotPayload {
        creds: creds(),
        heads: BTreeMap::from([(0, 0)]),
        identity: None,
    }
}

const HOLDER: DeviceId = DeviceId([0x06; 16]);
const ISSUED: i64 = 1_794_816_000;

fn holder_seed() -> SignSeed {
    SignSeed::from_bytes(Zeroizing::new([0x61; 32]))
}

/// The holder's genesis certificate in the vault of `creds()`.
fn holder_cert() -> cert::Certificate {
    let fields = CertFields {
        vault_id: [0x77; 16],
        device: HOLDER,
        box_pub: [0x63; 32],
        issued_at: ISSUED,
    };
    cert::issue_self(CertKind::Genesis, &fields, &holder_seed()).unwrap()
}

/// The holder vouches with the ticket whose seed is `0x62` repeated.
fn identity() -> PairingIdentity {
    let (secret, ticket) = cert::ticket_from_secret(
        TicketSecret::from_bytes(Zeroizing::new([0x62; 32])),
        &holder_seed(),
        [0x77; 16],
        HOLDER,
        ISSUED,
    );
    PairingIdentity {
        holder: HOLDER,
        export: TrustExport {
            entries: vec![ExportEntry {
                cert: holder_cert().to_b64(),
                state: TrustState::Trusted,
                record: None,
                cut_off_epoch: None,
                confirmed: false,
                cert_seen: None,
                left_seen: None,
                ticket_seen: None,
            }],
        },
        ticket_secret: secret,
        ticket,
    }
}

/// `creds()` with both heads and the holder's identity.
fn full_payload() -> SlotPayload {
    SlotPayload {
        creds: creds(),
        heads: BTreeMap::from([(0, 0), (1, 120)]),
        identity: Some(identity()),
    }
}

/// Seals `json` as slot 2 under `t`'s key and AD, whatever the json says.
fn seal_json(t: &Transcript, json: &str) -> Vec<u8> {
    super::seal_bytes(t, json.as_bytes(), [0x55; 24]).unwrap()
}

/// Protocol 4's frozen transcript of S_SECRET, C_SECRET and MAILBOX (KDF info `"moony pair v2"`).
const FROZEN_KEY: &str = "9f6a0f10a32ee07a98dd6265cd8c5d3a8b9c047df58613fedbb261c10141632c";
const FROZEN_CODE: u32 = 62532;
const FROZEN_ACK_KEY: &str = "ee56d61386cc345997b4bcbaab8a11834643be74df27b5bd8c436c5832918fdf";
/// The same exchange with M_SECRET answering in place of the scanner.
const FROZEN_KEY_M: &str = "755357eac5a189524a930327c7700ac7b23b8212724ee19d1948a075634e2550";
const FROZEN_CODE_M: u32 = 163133;

#[test]
fn the_transcript_matches_the_frozen_vector() {
    assert_eq!(
        s_pub(),
        hex32("7b4e909bbe7ffe44c465a220037d608ee35897d31ef972f07f74892cb0f73f13")
    );
    assert_eq!(
        c_pub(),
        hex32("0faa684ed28867b97f4a6a2dee5df8ce974e76b7018e3f22a1c4cf2678570f20")
    );
    for t in [shower(), scanner()] {
        assert_eq!(key(&t), FROZEN_KEY);
        assert_eq!(t.code(), FROZEN_CODE);
        assert_eq!(t.code_text(), format!("{FROZEN_CODE:06}"));
        assert_eq!(hex(t.ack_key().as_slice()), FROZEN_ACK_KEY);
        let mut ad = vec![0x01];
        ad.extend_from_slice(b"holder->joiner");
        ad.extend_from_slice(&[0x33; 16]);
        ad.extend_from_slice(&s_pub());
        ad.extend_from_slice(&c_pub());
        assert_eq!(t.ad().as_slice(), ad.as_slice());
        assert_eq!(ad.len(), 95);
    }
}

#[test]
fn a_substituted_scanner_key_shows_another_code() {
    let m_pub = public(M_SECRET);
    assert_eq!(
        m_pub,
        hex32("ff2ee45601ec1b67310c7790404585ae697331eee1c1f8cf2419731c1fff3e6b")
    );
    let t = derive(
        &StaticSecret::from(S_SECRET),
        Side::Shower,
        &m_pub,
        &MAILBOX,
    )
    .unwrap();
    assert_eq!(t.code(), FROZEN_CODE_M);
    assert_eq!(key(&t), FROZEN_KEY_M);
    assert_ne!(key(&t), key(&scanner()));
}

#[test]
fn the_wrong_side_gives_another_transcript() {
    let t = derive(
        &StaticSecret::from(C_SECRET),
        Side::Shower,
        &s_pub(),
        &MAILBOX,
    )
    .unwrap();
    assert_ne!(key(&t), key(&shower()));
}

#[test]
fn the_cancel_marker_is_two_bytes() {
    assert!(is_cancel_marker(&[1, 0]));
    assert!(!is_cancel_marker(&[1, 0, 0]));
    assert!(!is_cancel_marker(&[0, 0]));
    assert!(!is_cancel_marker(&slot1(Role::Joiner, &c_pub())));
    assert_eq!(CANCEL_MARKER, [1, 0]);
}

const FROZEN_COMMITMENT: &str = "24b4022a7fb1d63049b50725187304bc162d615197e952c07fe9741f7cf0e47f";
const FROZEN_TEXT: &str =
    "moony-pair:4:MzMzMzMzMzMzMzMzMzMzMw:JLQCKn-x1jBJtQclGHMEvBYtYVGX6VLAf-l0H3zw5H8";
/// What a version-1 build shows for the same exchange.
const FROZEN_TEXT_V1: &str =
    "moony-pair:1:MzMzMzMzMzMzMzMzMzMzMw:e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
/// What a version-2 build (before phase 5) shows for the same exchange.
const FROZEN_TEXT_V2: &str =
    "moony-pair:2:MzMzMzMzMzMzMzMzMzMzMw:e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
/// What a version-3 build shows: the shower's key itself in the code.
const FROZEN_TEXT_V3: &str =
    "moony-pair:3:MzMzMzMzMzMzMzMzMzMzMw:e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";

#[test]
fn the_commitment_matches_the_frozen_vector() {
    assert_eq!(hex(&commitment(&MAILBOX, &s_pub())), FROZEN_COMMITMENT);
    // Another mailbox or another key gives another commitment.
    assert_ne!(
        commitment(&MailboxId([0x34; 16]), &s_pub()),
        commitment(&MAILBOX, &s_pub())
    );
    assert_ne!(
        commitment(&MAILBOX, &c_pub()),
        commitment(&MAILBOX, &s_pub())
    );
    assert_eq!(COMMIT_DOMAIN, b"moony pair commit v1");
}

#[test]
fn the_code_text_carries_the_commitment_and_never_the_key() {
    let code = PairingCode {
        mailbox: MAILBOX,
        commitment: commitment(&MAILBOX, &s_pub()),
        relay_url: None,
    };
    let text = pairing_code_text(&code);
    assert_eq!(text, FROZEN_TEXT);
    assert!(text.starts_with("moony-pair:4:"));
    assert!(!text.contains(&URL_SAFE_NO_PAD.encode(s_pub())));
    let parsed = parse_pairing_code(&text).unwrap();
    assert_eq!(parsed.mailbox, MAILBOX);
    assert_eq!(parsed.commitment, code.commitment);
    assert_eq!(parsed.relay_url, None);
}

#[test]
fn a_code_names_a_sync_server_that_is_not_the_default() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let with = |url: &str| PairingCode {
        mailbox: MAILBOX,
        commitment: commitment(&MAILBOX, &s_pub()),
        relay_url: Some(url.into()),
    };
    let text = pairing_code_text(&with("https://sync.home.example.com"));
    let tail = URL_SAFE_NO_PAD.encode("https://sync.home.example.com");
    assert_eq!(text, format!("{FROZEN_TEXT}:{tail}"));
    assert_eq!(
        parse_pairing_code(&text).unwrap().relay_url.as_deref(),
        Some("https://sync.home.example.com")
    );
    // A sync server over plain http is allowed in every build (`relay::http::scheme_allowed`).
    let local = pairing_code_text(&with("http://127.0.0.1:8787"));
    assert_eq!(
        parse_pairing_code(&local).unwrap().relay_url.as_deref(),
        Some("http://127.0.0.1:8787")
    );
    // The default server never gets a fourth field, and a fourth field naming it reads as None.
    assert_eq!(pairing_code_text(&with(DEFAULT_RELAY_URL)), FROZEN_TEXT);
    let named = format!(
        "{FROZEN_TEXT}:{}",
        URL_SAFE_NO_PAD.encode(DEFAULT_RELAY_URL)
    );
    assert_eq!(parse_pairing_code(&named).unwrap().relay_url, None);
}

/// A code whose fourth field is `https://<host>.example.com` with `n` host characters.
fn code_with_host(n: usize) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let url = format!("https://{}.example.com", "a".repeat(n));
    format!("{FROZEN_TEXT}:{}", URL_SAFE_NO_PAD.encode(url))
}

#[test]
fn the_size_cap_comes_before_decoding() {
    // The longest address the app accepts (256 characters) gives a 422-byte code: under the cap.
    let longest = code_with_host(256 - "https://.example.com".len());
    assert!(longest.len() <= MAX_CODE_BYTES);
    assert!(parse_pairing_code(&longest).is_ok());
    // A code over the cap whose address is well formed but too long: without the cap it would
    // read as `syncRelayUrlInvalid`; with it, it is refused before anything is decoded.
    let over = code_with_host(400);
    assert!(over.len() > MAX_CODE_BYTES);
    assert_eq!(
        validation_key(parse_pairing_code(&over).unwrap_err()),
        "validation.syncPairingCodeInvalid"
    );
    assert_eq!(
        validation_key(parse_pairing_code(&code_with_host(250)).unwrap_err()),
        "validation.syncRelayUrlInvalid"
    );
}

#[test]
fn a_code_of_a_later_version_says_this_device_needs_an_update() {
    let mailbox = "MzMzMzMzMzMzMzMzMzMzMw";
    let key = "e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
    for input in [
        format!("moony-pair:5:{mailbox}:{key}"),
        "moony-pair:10:".to_string(),
        "moony-pair:5".to_string(),
        "moony-pair:99999999999999999999:x".to_string(),
    ] {
        let err = parse_pairing_code(&input).unwrap_err();
        assert!(!format!("{err} {err:?}").contains(key), "echoed the key");
        assert_eq!(validation_key(err), THIS_NEEDS_UPDATE, "{input}");
    }
    assert_eq!(THIS_NEEDS_UPDATE, "validation.syncPairingThisNeedsUpdate");
    // Not a version number: still just invalid.
    for input in ["moony-pair:x:abc", "moony-pair::abc", "moony-pair:-3:abc"] {
        assert_eq!(
            validation_key(parse_pairing_code(input).unwrap_err()),
            "validation.syncPairingCodeInvalid",
            "{input}"
        );
    }
}

#[test]
fn malformed_codes_are_refused_without_echoing_them() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mailbox = "MzMzMzMzMzMzMzMzMzMzMw";
    let key = "e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
    let evil = URL_SAFE_NO_PAD.encode("ftp://evil.example");
    let cases: Vec<(String, &str)> = vec![
        (
            format!("moony-recover:1:{mailbox}:{key}"),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:4:{}:{key}", &mailbox[..21]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:4:{mailbox}A:{key}"),
            "validation.syncPairingCodeInvalid",
        ),
        // 22 characters whose last character carries non-zero trailing bits: not canonical.
        (
            format!("moony-pair:4:{}x:{key}", &mailbox[..21]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:4:{mailbox}:{}", &key[..42]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:4:{mailbox}:{key}:{evil}:x"),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:4:{mailbox}:{key}:{evil}"),
            "validation.syncRelayUrlInvalid",
        ),
        (
            format!("moony-pair:4:{mailbox}:{key}:"),
            "validation.syncPairingCodeInvalid",
        ),
        (code_with_host(400), "validation.syncPairingCodeInvalid"),
    ];
    for (input, expected) in cases {
        let err = match parse_pairing_code(&input) {
            Ok(_) => panic!("accepted {input}"),
            Err(e) => e,
        };
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains(&input), "echoed {input}");
        assert!(!shown.contains(key), "echoed the key in {input}");
        assert_eq!(validation_key(err), expected, "{input}");
    }
}

#[test]
fn a_v1_v2_or_v3_code_is_refused_with_a_clear_message() {
    let mut v1_with_relay = FROZEN_TEXT_V1.to_string();
    v1_with_relay.push_str(":aHR0cDovL2V2aWwuZXhhbXBsZQ");
    for input in [
        FROZEN_TEXT_V1,
        FROZEN_TEXT_V2,
        FROZEN_TEXT_V3,
        v1_with_relay.as_str(),
        "moony-pair:1:",
        "moony-pair:1:junk",
        "moony-pair:2:",
        "moony-pair:2:junk",
        "moony-pair:3:",
        "moony-pair:3:junk",
        "moony-pair:0:junk",
    ] {
        let err = match parse_pairing_code(input) {
            Ok(_) => panic!("accepted {input}"),
            Err(e) => e,
        };
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains(input), "echoed {input}");
        assert!(
            !shown.contains("e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM"),
            "echoed the key"
        );
        assert_eq!(validation_key(err), OTHER_NEEDS_UPDATE, "{input}");
    }
    assert_eq!(OTHER_NEEDS_UPDATE, "validation.syncPairingOtherNeedsUpdate");
    assert_eq!(QR_PREFIX, "moony-pair:4:");
    assert_eq!(PROTOCOL, 4);
    // The size cap still comes first.
    let over = format!("{FROZEN_TEXT_V1}:{}", "A".repeat(MAX_CODE_BYTES));
    assert_eq!(
        validation_key(parse_pairing_code(&over).unwrap_err()),
        "validation.syncPairingCodeInvalid"
    );
}

#[test]
fn a_v3_slot1_is_refused() {
    // Versions 1 (protocol 1) and 2 (protocols 2 and 3) are refused as well.
    for version in [0x01, 0x02] {
        let mut old = vec![version, 0x02];
        old.extend_from_slice(&c_pub());
        assert_eq!(old.len(), SLOT1_LEN);
        assert_eq!(
            validation_key(parse_slot1(&old).unwrap_err()),
            "validation.syncPairingFailed"
        );
    }
    assert_eq!(SLOT1_VERSION, 0x03);
    // Slot 2, slot 3 and the cancel marker keep version 1.
    assert_eq!(SLOT_VERSION, 0x01);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const TAG_S_C: &str = "6f69ab37a02b8f223ca5591fe65ea20257233206d9ca7b2b4ed86d571a9d1e41";
const TAG_S_M: &str = "9cb1feeb960519dabbfe6e2400a98ee70f7a8e75469d2a0bbdb1a79e04a07c5a";

fn tag_of(hex_text: &str) -> [u8; 32] {
    hex32(hex_text)
}

#[test]
fn the_reveal_matches_the_frozen_vector() {
    assert_eq!(ACK_DIRECTION, b"shower->scanner");
    assert_eq!(ACK_DIRECTION.len(), 15);
    assert_eq!(REVEAL_VERSION, 0x02);
    assert_eq!(REVEAL_SLOT_LEN, 65);
    assert_eq!(OKM_LEN, 72);
    for t in [shower(), scanner()] {
        let slot = t.reveal();
        let mut expected = vec![REVEAL_VERSION];
        expected.extend_from_slice(&s_pub());
        expected.extend_from_slice(&tag_of(TAG_S_C));
        assert_eq!(slot.to_vec(), expected);
        assert!(scanner().verify_tag(&tag_of(TAG_S_C)));
    }
}

#[test]
fn the_reveal_opens_only_against_its_own_commitment() {
    let c = commitment(&MAILBOX, &s_pub());
    let slot = shower().reveal();
    assert_eq!(slot.len(), REVEAL_SLOT_LEN);
    let (key, tag) = open_reveal(&slot, &MAILBOX, &c).expect("opens");
    assert_eq!(key, s_pub());
    assert!(scanner().verify_tag(&tag));
    assert!(open_reveal(&slot, &MAILBOX, &commitment(&MAILBOX, &c_pub())).is_none());
    assert!(open_reveal(&slot, &MailboxId([0x34; 16]), &c).is_none());
    assert!(
        open_reveal(&slot[..33], &MAILBOX, &c).is_none(),
        "a protocol-3 slot 3"
    );
    assert!(open_reveal(&CANCEL_MARKER, &MAILBOX, &c).is_none());
    assert!(open_reveal(&[], &MAILBOX, &c).is_none());
    let mut other_version = slot;
    other_version[0] = 0x01;
    assert!(open_reveal(&other_version, &MAILBOX, &c).is_none());
    let mut long = slot.to_vec();
    long.push(0);
    assert!(open_reveal(&long, &MAILBOX, &c).is_none());
    // A flipped bit in the key breaks the commitment; one in the tag is for `verify_tag`.
    for bit in 8..8 + 256 {
        let mut bad = slot;
        bad[bit / 8] ^= 1 << (bit % 8);
        assert!(open_reveal(&bad, &MAILBOX, &c).is_none(), "bit {bit}");
    }
}

#[test]
fn a_tag_for_a_substituted_key_does_not_verify() {
    let m_pub = public(M_SECRET);
    let fooled = derive(
        &StaticSecret::from(S_SECRET),
        Side::Shower,
        &m_pub,
        &MAILBOX,
    )
    .unwrap();
    let (_, forged) = open_reveal(&fooled.reveal(), &MAILBOX, &commitment(&MAILBOX, &s_pub()))
        .expect("the same shower key");
    assert_eq!(forged, tag_of(TAG_S_M));
    assert!(!scanner().verify_tag(&forged));
    assert!(scanner().verify_tag(&tag_of(TAG_S_C)));
}

#[test]
fn a_tag_for_the_same_keys_under_another_mailbox_does_not_verify() {
    let other = MailboxId([0x34; 16]);
    let elsewhere = derive(
        &StaticSecret::from(S_SECRET),
        Side::Shower,
        &c_pub(),
        &other,
    )
    .unwrap();
    let (_, tag) =
        open_reveal(&elsewhere.reveal(), &other, &commitment(&other, &s_pub())).expect("opens");
    assert!(!scanner().verify_tag(&tag));
}

#[test]
fn a_tampered_tag_is_refused() {
    let c = scanner();
    let good = tag_of(TAG_S_C);
    assert!(c.verify_tag(&good));
    for bit in 0..256 {
        let mut bad = good;
        bad[bit / 8] ^= 1 << (bit % 8);
        assert!(!c.verify_tag(&bad), "bit {bit}");
    }
}

#[test]
fn the_typed_code_is_checked_exactly() {
    let t = shower();
    assert_eq!(t.code_text(), format!("{FROZEN_CODE:06}"));
    // Full-width digits (U+FF10–U+FF19) count as their ASCII digit.
    for typed in [
        "062532",
        "062 532",
        "062-532",
        "０６２５３２",
        "０６２ ５３２",
        "06２5３2",
    ] {
        assert!(t.code_matches(typed), "{typed:?}");
        assert!(typed_code_digits(typed).is_some(), "{typed:?}");
    }
    for typed in [
        "062533",
        "06253",
        "0625320",
        "06253a",
        " 062532",
        "",
        "062532 ",
        "06 2532",
        "062  532",
        "062_532",
        "062--532",
        "062 -532",
        "０６２５３３",
        "０６２　５３２",
        "٤٨٢٦٨٦",
    ] {
        assert!(!t.code_matches(typed), "{typed:?}");
    }
    // Only input of another shape is not six digits; a wrong six-digit code is.
    for typed in [
        "06253",
        "062532 ",
        "06 2532",
        "062_532",
        "０６２　５３２",
        "",
    ] {
        assert!(typed_code_digits(typed).is_none(), "{typed:?}");
    }
    assert!(typed_code_digits("062533").is_some());
    let seven = Transcript {
        code: 7,
        ..shower()
    };
    assert_eq!(seven.code_text(), "000007");
    assert!(seven.code_matches("000007"));
    assert!(seven.code_matches("000 007"));
    assert!(!seven.code_matches("7"));
    assert!(!seven.code_matches("062532"));
}

#[test]
fn mailbox_ids_are_canonical_base64url() {
    assert_eq!(MAILBOX.to_b64(), "MzMzMzMzMzMzMzMzMzMzMw");
    assert_eq!(
        MailboxId::from_b64("MzMzMzMzMzMzMzMzMzMzMw").unwrap(),
        MAILBOX
    );
    assert!(MailboxId::from_b64("MzMzMzMzMzMzMzMzMzMzMx").is_err());
    assert_ne!(MailboxId::random().unwrap(), MailboxId::random().unwrap());
}

#[test]
fn slot_1_layout_and_roles() {
    let slot = slot1(Role::Joiner, &c_pub());
    assert_eq!(slot.len(), SLOT1_LEN);
    assert_eq!(&slot[..2], &[0x03, 0x02]);
    assert_eq!(&slot[2..], &c_pub());
    assert_eq!(parse_slot1(&slot).unwrap(), (Role::Joiner, c_pub()));
    let failed = |bytes: &[u8]| validation_key(parse_slot1(bytes).unwrap_err());
    assert_eq!(failed(&slot[..33]), "validation.syncPairingFailed");
    let mut long = slot.to_vec();
    long.push(0);
    assert_eq!(failed(&long), "validation.syncPairingFailed");
    let mut v2 = slot;
    v2[0] = 2;
    assert_eq!(failed(&v2), "validation.syncPairingFailed");
    let mut r3 = slot;
    r3[1] = 3;
    assert_eq!(failed(&r3), "validation.syncPairingFailed");

    assert_eq!(Role::of(true), Role::Holder);
    assert_eq!(Role::of(false), Role::Joiner);
    assert_eq!(Role::from_byte(Role::Holder.byte()), Some(Role::Holder));
    assert_eq!(Role::from_byte(0), None);
    assert_eq!(
        validation_key(check_roles(Role::Holder, Role::Holder).unwrap_err()),
        "validation.syncPairingBothOn"
    );
    assert_eq!(
        validation_key(check_roles(Role::Joiner, Role::Joiner).unwrap_err()),
        "validation.syncPairingBothOff"
    );
    assert!(check_roles(Role::Holder, Role::Joiner).is_ok());
    assert!(check_roles(Role::Joiner, Role::Holder).is_ok());
}

#[test]
fn a_low_order_public_key_is_refused() {
    let mut one = [0u8; 32];
    one[0] = 1;
    let order_8 = hex32("e0eb7a7c3b41b8ae1656e3faf19fc46ada098deb9c32b1fd866205165f49b800");
    for peer in [[0u8; 32], one, order_8] {
        let err = derive(&StaticSecret::from(S_SECRET), Side::Shower, &peer, &MAILBOX).unwrap_err();
        assert_eq!(validation_key(err), "validation.syncPairingFailed");
    }
}

#[test]
fn credentials_round_trip_through_slot_2() {
    let t = shower();
    let sealed = seal_payload_with_nonce(&t, &plain_payload(), [0x55; 24]).unwrap();
    assert_eq!(sealed[0], 0x01);
    assert_eq!(&sealed[1..25], &[0x55; 24]);
    let opened = open_payload(&scanner(), &sealed).unwrap();
    assert!(opened.identity.is_none(), "a holder without an identity");
    assert_eq!(opened.heads, BTreeMap::from([(0, 0)]));
    let opened = opened.creds;
    let want = creds();
    assert_eq!(opened.relay_url, "https://sync.home.example.com");
    assert_eq!(opened.account, want.account);
    assert_eq!(opened.vault_id, want.vault_id);
    assert_eq!(opened.keys.current_epoch(), 1);
    let got: Vec<_> = opened.keys.epochs().map(|(e, k)| (e, *k)).collect();
    assert_eq!(got, vec![(0, [0xA0; 32]), (1, [0xA1; 32])]);
    // The random-nonce entry point opens too.
    let sealed = seal_payload(&t, &plain_payload()).unwrap();
    assert!(open_payload(&scanner(), &sealed).is_ok());
}

#[test]
fn slot_2_is_bound_to_this_exchange() {
    let sealed = seal_payload(&shower(), &plain_payload()).unwrap();
    let failed = |t: &Transcript, bytes: &[u8]| validation_key(open_payload(t, bytes).unwrap_err());
    let other_mailbox = derive(
        &StaticSecret::from(C_SECRET),
        Side::Scanner,
        &s_pub(),
        &MailboxId([0x34; 16]),
    )
    .unwrap();
    assert_eq!(
        failed(&other_mailbox, &sealed),
        "validation.syncPairingFailed"
    );
    let swapped = derive(
        &StaticSecret::from(C_SECRET),
        Side::Shower,
        &s_pub(),
        &MAILBOX,
    )
    .unwrap();
    assert_eq!(failed(&swapped, &sealed), "validation.syncPairingFailed");
    for i in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[i] ^= 1;
        assert_eq!(
            failed(&scanner(), &bad),
            "validation.syncPairingFailed",
            "byte {i}"
        );
    }
    // Sealed under the session key but with an AD lacking the version and direction label.
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    let t = shower();
    let mut bare = Vec::new();
    bare.extend_from_slice(&MAILBOX.0);
    bare.extend_from_slice(&s_pub());
    bare.extend_from_slice(&c_pub());
    let ct = chacha20poly1305::XChaCha20Poly1305::new(t.key_bytes().into())
        .encrypt(
            (&[0x55u8; 24]).into(),
            Payload {
                msg: b"{}",
                aad: &bare,
            },
        )
        .unwrap();
    let mut forged = vec![0x01];
    forged.extend_from_slice(&[0x55; 24]);
    forged.extend_from_slice(&ct);
    assert_eq!(failed(&scanner(), &forged), "validation.syncPairingFailed");
    // Larger than any legal slot 2: refused before the AEAD runs.
    let mut huge = sealed.clone();
    huge.resize(1 + 24 + MAX_PAYLOAD_JSON + 16 + 1, 0);
    assert_eq!(failed(&scanner(), &huge), "validation.syncPairingFailed");
    // Too short for version + nonce + tag.
    assert_eq!(
        failed(&scanner(), &sealed[..40]),
        "validation.syncPairingFailed"
    );
}

#[test]
fn a_hostile_payload_is_refused() {
    let t = shower();
    let vid = VaultId([0x77; 16]).to_b64();
    let k = "oKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKA"; // 32 x 0xA0
    let good = |relay: &str, account: &str, v: u32, keys: &str, extra: &str| {
        format!(
            "{{\"v\":{v},\"relayUrl\":\"{relay}\",\"accountNumber\":\"{account}\",\"vaultId\":\"{vid}\",\"keys\":[{keys}],\"identity\":null{extra}}}"
        )
    };
    let entry = |e: u32| format!("{{\"epoch\":{e},\"key\":\"{k}\",\"head\":0}}");
    let ok_keys = entry(0);
    let acct = "1234567890123456";
    let relay = "https://sync.home.example.com";
    // Sanity: the builder's good payload opens.
    assert!(open_payload(&t, &seal_json(&t, &good(relay, acct, 2, &ok_keys, ""))).is_ok());

    let addr = "validation.syncRelayUrlInvalid";
    let fail = "validation.syncPairingFailed";
    let many = (0..65).map(entry).collect::<Vec<_>>().join(",");
    let dup = format!("{},{}", entry(3), entry(3));
    let short = format!("{{\"epoch\":0,\"key\":\"{}\",\"head\":0}}", &k[..42]);
    let big = format!(
        "{}{}",
        good(relay, acct, 2, &ok_keys, ""),
        " ".repeat(49 * 1024)
    );
    let key_extra = format!("{{\"epoch\":0,\"key\":\"{k}\",\"head\":0,\"x\":1}}");
    let cases: Vec<(String, &str)> = vec![
        (good("ftp://evil.example", acct, 2, &ok_keys, ""), addr),
        (good("https://a.b/path", acct, 2, &ok_keys, ""), addr),
        (good(relay, "123", 2, &ok_keys, ""), fail),
        (good(relay, acct, 2, &ok_keys, ",\"extra\":1"), fail),
        // A version-1 payload (before phase 5) and a later one.
        (good(relay, acct, 1, &ok_keys, ""), fail),
        (good(relay, acct, 3, &ok_keys, ""), fail),
        (good(relay, acct, 2, &many, ""), fail),
        (good(relay, acct, 2, "", ""), fail),
        (good(relay, acct, 2, &dup, ""), fail),
        (good(relay, acct, 2, &short, ""), fail),
        (good(relay, acct, 2, &key_extra, ""), fail),
        (big, fail),
        ("not json".into(), fail),
    ];
    for (json, expected) in cases {
        let err = open_payload(&t, &seal_json(&t, &json)).unwrap_err();
        assert_eq!(
            validation_key(err),
            expected,
            "{}",
            &json[..json.len().min(120)]
        );
    }
}

/// Protocol 3's slot 2 (spec §11.2): every key with its head and the holder's identity, sealed
/// under the frozen transcript with a fixed nonce. The certificate and ticket bytes are frozen by
/// `identity/tests.rs`; this freezes the payload's layout and the sealed bytes.
#[test]
fn slot_2_v2_matches_the_frozen_vector() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let payload = full_payload();
    let json = payload_json(&payload).unwrap();
    let ticket = &payload.identity.as_ref().unwrap().ticket;
    let expected = format!(
        "{{\"v\":2,\"relayUrl\":\"https://sync.home.example.com\",\
         \"accountNumber\":\"1234567890123456\",\"vaultId\":\"{vid}\",\
         \"keys\":[{{\"epoch\":0,\"key\":\"{k0}\",\"head\":0}},\
         {{\"epoch\":1,\"key\":\"{k1}\",\"head\":120}}],\
         \"identity\":{{\"holder\":\"06060606060606060606060606060606\",\
         \"export\":{{\"entries\":[{{\"cert\":\"{cert}\",\"state\":\"trusted\"}}]}},\
         \"ticket\":{{\"secret\":\"{secret}\",\"holderSig\":\"{sig}\",\
         \"issuedAt\":1794816000}}}}}}",
        vid = VaultId([0x77; 16]).to_b64(),
        k0 = URL_SAFE_NO_PAD.encode([0xA0; 32]),
        k1 = URL_SAFE_NO_PAD.encode([0xA1; 32]),
        cert = holder_cert().to_b64(),
        secret = URL_SAFE_NO_PAD.encode([0x62; 32]),
        sig = URL_SAFE_NO_PAD.encode(ticket.holder_sig),
    );
    assert_eq!(json.as_str(), expected);
    assert_eq!(
        hex(&ticket.holder_sig),
        "bc1c2fe4fe3f04958418e44a57b257454ab777d34db0ef5422ec4787b166cadaf580190e7076e4c942b207e9d84d28ddb6e13616a525083f2c4ebb6262958e06",
        "the holder's signature over the ticket"
    );
    let sealed = seal_payload_with_nonce(&shower(), &payload, [0x55; 24]).unwrap();
    assert_eq!(
        hex(&sha2::Sha256::digest(&sealed)),
        "222f2e2f1cacfcf0a2cb4678b38fe1ff6a18b07837e89da749fc6ef625c4f366"
    );
    assert_eq!(sealed.len(), 1 + 24 + json.len() + 16);
}

#[test]
fn the_holders_identity_and_heads_round_trip_through_slot_2() {
    let sealed = seal_payload(&shower(), &full_payload()).unwrap();
    let opened = open_payload(&scanner(), &sealed).unwrap();
    assert_eq!(opened.heads, BTreeMap::from([(0, 0), (1, 120)]));
    assert_eq!(opened.creds.keys.current_epoch(), 1);
    let got = opened.identity.expect("the holder's identity");
    let want = identity();
    assert_eq!(got.holder, HOLDER);
    assert_eq!(got.export, want.export);
    assert_eq!(got.ticket, want.ticket);
    assert_eq!(got.ticket_secret.expose(), want.ticket_secret.expose());
    // The joiner can sign its certificate with what it received.
    let joiner = SignSeed::from_bytes(Zeroizing::new([0x64; 32]));
    let fields = CertFields {
        vault_id: [0x77; 16],
        device: DeviceId([0x07; 16]),
        box_pub: [0x65; 32],
        issued_at: ISSUED,
    };
    let paired = cert::issue_paired(
        &got.ticket_secret,
        &got.ticket,
        got.holder,
        &sign_pub(&holder_seed()),
        &fields,
        &joiner,
    )
    .expect("a paired certificate");
    assert_eq!(paired.body.issuer_device, HOLDER);
}

/// A key the holder knows no head for travels without one; `identity: null` (a holder whose
/// vault has no certificates) opens.
#[test]
fn a_key_without_a_head_and_a_null_identity_open() {
    let t = shower();
    let k = "oKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKCgoKA";
    let json = format!(
        "{{\"v\":2,\"relayUrl\":\"https://sync.home.example.com\",\
         \"accountNumber\":\"1234567890123456\",\"vaultId\":\"{}\",\
         \"keys\":[{{\"epoch\":0,\"key\":\"{k}\",\"head\":0}},{{\"epoch\":1,\"key\":\"{k}\"}}],\
         \"identity\":null}}",
        VaultId([0x77; 16]).to_b64()
    );
    let opened = open_payload(&t, &seal_json(&t, &json)).unwrap();
    assert_eq!(opened.heads, BTreeMap::from([(0, 0)]));
    assert!(opened.identity.is_none());
    // And a holder that seals without a head writes none.
    let mut payload = full_payload();
    payload.heads.remove(&1);
    payload.identity = None;
    let json = payload_json(&payload).unwrap();
    assert!(json.contains("{\"epoch\":1,\"key\":\"oaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaGhoaE\"}"));
    assert!(json.ends_with(",\"identity\":null}"));
}

#[test]
fn a_hostile_identity_is_refused() {
    let t = shower();
    let payload = full_payload();
    let json = payload_json(&payload).unwrap();
    let fail = "validation.syncPairingFailed";
    let swap = |from: &str, to: &str| {
        assert!(json.contains(from), "{from}");
        json.replacen(from, to, 1)
    };
    let secret = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode([0x62; 32])
    };
    let cases = [
        swap("\"holder\":\"0606", "\"holder\":\"0A06"),
        swap(
            "\"holder\":\"06060606060606060606060606060606\"",
            "\"holder\":\"060606\"",
        ),
        swap("\"holder\":", "\"extra\":1,\"holder\":"),
        swap("\"secret\":", "\"extra\":1,\"secret\":"),
        swap("\"entries\":", "\"extra\":1,\"entries\":"),
        swap(&secret, &secret[..42]),
        swap("\"holderSig\":\"", "\"holderSig\":\"AAAA"),
        swap("\"issuedAt\":1794816000", "\"issuedAt\":\"x\""),
        swap("\"identity\":{", "\"identity\":5,\"x\":{"),
    ];
    for json in cases {
        let err = open_payload(&t, &seal_json(&t, &json)).unwrap_err();
        assert_eq!(validation_key(err), fail, "{}", &json[json.len() - 200..]);
    }
}

#[test]
fn a_payload_of_up_to_48_kib_opens() {
    assert_eq!(MAX_PAYLOAD_JSON, 48 * 1024);
    let t = shower();
    let json = payload_json(&full_payload()).unwrap();
    let fill = |n: usize| format!("{}{}", &json[..json.len() - 1], " ".repeat(n)) + "}";
    let at_cap = fill(MAX_PAYLOAD_JSON - json.len());
    assert_eq!(at_cap.len(), MAX_PAYLOAD_JSON);
    assert!(open_payload(&t, &seal_json(&t, &at_cap)).is_ok());
    let over = fill(MAX_PAYLOAD_JSON - json.len() + 1);
    assert_eq!(
        validation_key(open_payload(&t, &seal_json(&t, &over)).unwrap_err()),
        "validation.syncPairingFailed"
    );
}

/// A holder whose store would not fit the slot refuses before anything is sealed.
#[test]
fn a_store_too_large_for_slot_2_is_refused_when_sealing() {
    let mut payload = full_payload();
    let entry = payload.identity.as_ref().unwrap().export.entries[0].clone();
    payload.identity.as_mut().unwrap().export.entries = vec![entry; 120];
    let err = seal_payload(&shower(), &payload).unwrap_err();
    assert_eq!(validation_key(err), "validation.syncTooManyDevices");
}

#[test]
fn secrets_print_redacted() {
    let id = format!("{:?}", MAILBOX);
    assert_eq!(id, "MailboxId(<redacted>)");
    let payload = format!("{:?} {:?}", full_payload(), identity());
    assert!(payload.contains("<redacted>"));
    assert!(!payload.contains("1234567890123456"));
    assert!(!payload.contains("98, 98"), "the ticket secret");
    let t = format!("{:?} {:?}", shower(), shower());
    assert!(!t.contains(&key(&shower())));
    assert!(!t.contains(&format!("{FROZEN_CODE:06}")));
    assert!(!t.contains("33"));
    assert!(!t.contains(&hex(shower().ack_key().as_slice())));
    assert!(!t.contains(TAG_S_C));
}

#[test]
fn secrets_are_wiped_on_drop() {
    fn wiped<T: zeroize::ZeroizeOnDrop>() {}
    wiped::<x25519_dalek::StaticSecret>();
    wiped::<Zeroizing<[u8; 32]>>();
    // The session key and the ack key are held in wiped buffers.
    let t = shower();
    let _: &Zeroizing<[u8; 32]> = &t.key;
    let _: &Zeroizing<[u8; 32]> = &t.ack_key;
}

// Audit M2, after Task 13 of the audit-fix plan: this stays true. Six digits can always be ground
// (about 10^6 X25519 keys); nothing in the digits themselves stops it. The defences are the order
// of the messages: the scanner shows its digits only after the shower proved it derived its
// transcript from the scanner's own key (`session_audit_tests::
// a_scanner_shows_no_digits_before_a_valid_ack`), and, from protocol 4, the code commits to the
// shower's key, which is revealed only after the scanner's key was read, so there is no key left
// to grind (`session_audit2_tests::
// a_pairing_code_rewritten_on_its_way_gives_nothing_to_whoever_rewrote_it`).
#[test]
#[ignore = "grinds about 10^6 X25519 keys"]
fn the_six_digits_alone_can_be_ground() {
    let s_pub = public(S_SECRET);
    let phone = derive(
        &StaticSecret::from(C_SECRET),
        Side::Scanner,
        &s_pub,
        &MAILBOX,
    )
    .unwrap()
    .code();
    let started = std::time::Instant::now();
    let mut seed = [0u8; 32];
    for i in 1u64.. {
        // Bytes 8..16: X25519 clamps bits of the first byte, so a counter there would try each
        // key eight times.
        seed[8..16].copy_from_slice(&i.to_le_bytes());
        let m = StaticSecret::from(seed);
        let Ok(t) = derive(&m, Side::Scanner, &s_pub, &MAILBOX) else {
            continue;
        };
        if t.code() == phone {
            let m_pub = PublicKey::from(&m).to_bytes();
            let shower = derive(
                &StaticSecret::from(S_SECRET),
                Side::Shower,
                &m_pub,
                &MAILBOX,
            )
            .unwrap();
            assert_eq!(shower.code(), phone);
            assert_ne!(m_pub, public(C_SECRET));
            eprintln!("AUDIT grind: {i} trials in {:?}", started.elapsed());
            return;
        }
    }
}

/// Owner decision O-3 (As built R-114): a pasted or typed pairing code reads with any white space
/// or zero-width character inside it (a chat app wraps it, a person types it in groups), as a
/// connection code does.
#[test]
fn a_pairing_code_reads_with_white_space_inside() {
    let split = |at: usize, gap: &str| format!("{}{gap}{}", &FROZEN_TEXT[..at], &FROZEN_TEXT[at..]);
    for input in [
        format!("  {FROZEN_TEXT}\n"),
        split(6, " "),
        split(20, "\n"),
        split(30, "\u{00A0}\t"),
        split(40, "\u{200B}"),
        format!("\u{FEFF}{}", split(12, "\r\n ")),
    ] {
        let parsed = parse_pairing_code(&input).expect("reads");
        assert_eq!(parsed.mailbox, MAILBOX, "{input:?}");
        assert_eq!(parsed.commitment, hex32(FROZEN_COMMITMENT), "{input:?}");
    }
    // White space does not count toward the size cap, but a flood of it is still refused.
    let flood = format!("{FROZEN_TEXT}{}", " ".repeat(4 * MAX_CODE_BYTES));
    assert_eq!(
        validation_key(parse_pairing_code(&flood).unwrap_err()),
        "validation.syncPairingCodeInvalid"
    );
}
