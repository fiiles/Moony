use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::*;
use crate::error::AppError;
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

/// Seals `json` as slot 2 under `t`'s key and AD, whatever the json says.
fn seal_json(t: &Transcript, json: &str) -> Vec<u8> {
    super::seal_bytes(t, json.as_bytes(), [0x55; 24]).unwrap()
}

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
    let expected = "01b8d60583d5b13dc2ac3426e3bb2550bc24e241f7fea9f2787597d0c03d527b";
    for t in [shower(), scanner()] {
        assert_eq!(key(&t), expected);
        assert_eq!(t.code(), 482686);
        assert_eq!(t.code_text(), "482686");
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
    assert_eq!(t.code(), 763075);
    assert_eq!(
        key(&t),
        "0efa12d5cc75e952aa8a22e2676ec387480c9f8124a440153a79019911756f36"
    );
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

const FROZEN_TEXT: &str =
    "moony-pair:1:MzMzMzMzMzMzMzMzMzMzMw:e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";

#[test]
fn the_code_text_is_frozen() {
    let code = PairingCode {
        mailbox: MAILBOX,
        shower_public: s_pub(),
        relay_url: None,
    };
    assert_eq!(pairing_code_text(&code), FROZEN_TEXT);
    let parsed = parse_pairing_code(FROZEN_TEXT).unwrap();
    assert_eq!(parsed.mailbox, MAILBOX);
    assert_eq!(parsed.shower_public, s_pub());
    assert_eq!(parsed.relay_url, None);
}

#[test]
fn a_code_names_a_sync_server_that_is_not_the_default() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let with = |url: &str| PairingCode {
        mailbox: MAILBOX,
        shower_public: s_pub(),
        relay_url: Some(url.into()),
    };
    let text = pairing_code_text(&with("https://sync.home.example.com"));
    let tail = URL_SAFE_NO_PAD.encode("https://sync.home.example.com");
    assert_eq!(text, format!("{FROZEN_TEXT}:{tail}"));
    assert_eq!(
        parse_pairing_code(&text).unwrap().relay_url.as_deref(),
        Some("https://sync.home.example.com")
    );
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
fn malformed_codes_are_refused_without_echoing_them() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let mailbox = "MzMzMzMzMzMzMzMzMzMzMw";
    let key = "e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
    let evil = URL_SAFE_NO_PAD.encode("http://evil.example");
    let cases: Vec<(String, &str)> = vec![
        (
            format!("moony-recover:1:{mailbox}:{key}"),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:2:{mailbox}:{key}"),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:1:{}:{key}", &mailbox[..21]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:1:{mailbox}A:{key}"),
            "validation.syncPairingCodeInvalid",
        ),
        // 22 characters whose last character carries non-zero trailing bits: not canonical.
        (
            format!("moony-pair:1:{}x:{key}", &mailbox[..21]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:1:{mailbox}:{}", &key[..42]),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:1:{mailbox}:{key}:{evil}:x"),
            "validation.syncPairingCodeInvalid",
        ),
        (
            format!("moony-pair:1:{mailbox}:{key}:{evil}"),
            "validation.syncRelayUrlInvalid",
        ),
        (
            format!("moony-pair:1:{mailbox}:{key}:"),
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
    assert_eq!(&slot[..2], &[0x01, 0x02]);
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
    let sealed = seal_credentials_with_nonce(&t, &creds(), [0x55; 24]).unwrap();
    assert_eq!(sealed[0], 0x01);
    assert_eq!(&sealed[1..25], &[0x55; 24]);
    let opened = open_credentials(&scanner(), &sealed).unwrap();
    let want = creds();
    assert_eq!(opened.relay_url, "https://sync.home.example.com");
    assert_eq!(opened.account, want.account);
    assert_eq!(opened.vault_id, want.vault_id);
    assert_eq!(opened.keys.current_epoch(), 1);
    let got: Vec<_> = opened.keys.epochs().map(|(e, k)| (e, *k)).collect();
    assert_eq!(got, vec![(0, [0xA0; 32]), (1, [0xA1; 32])]);
    // The random-nonce entry point opens too.
    let sealed = seal_credentials(&t, &creds()).unwrap();
    assert!(open_credentials(&scanner(), &sealed).is_ok());
}

#[test]
fn slot_2_is_bound_to_this_exchange() {
    let sealed = seal_credentials(&shower(), &creds()).unwrap();
    let failed =
        |t: &Transcript, bytes: &[u8]| validation_key(open_credentials(t, bytes).unwrap_err());
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
            "{{\"v\":{v},\"relayUrl\":\"{relay}\",\"accountNumber\":\"{account}\",\"vaultId\":\"{vid}\",\"keys\":[{keys}]{extra}}}"
        )
    };
    let entry = |e: u32| format!("{{\"epoch\":{e},\"key\":\"{k}\"}}");
    let ok_keys = entry(0);
    let acct = "1234567890123456";
    let relay = "https://sync.home.example.com";
    // Sanity: the builder's good payload opens.
    assert!(open_credentials(&t, &seal_json(&t, &good(relay, acct, 1, &ok_keys, ""))).is_ok());

    let addr = "validation.syncRelayUrlInvalid";
    let fail = "validation.syncPairingFailed";
    let many = (0..65).map(entry).collect::<Vec<_>>().join(",");
    let dup = format!("{},{}", entry(3), entry(3));
    let short = format!("{{\"epoch\":0,\"key\":\"{}\"}}", &k[..42]);
    let big = format!(
        "{}{}",
        good(relay, acct, 1, &ok_keys, ""),
        " ".repeat(17 * 1024)
    );
    let cases: Vec<(String, &str)> = vec![
        (good("http://evil.example", acct, 1, &ok_keys, ""), addr),
        (good("https://a.b/path", acct, 1, &ok_keys, ""), addr),
        (good(relay, "123", 1, &ok_keys, ""), fail),
        (good(relay, acct, 1, &ok_keys, ",\"extra\":1"), fail),
        (good(relay, acct, 2, &ok_keys, ""), fail),
        (good(relay, acct, 1, &many, ""), fail),
        (good(relay, acct, 1, "", ""), fail),
        (good(relay, acct, 1, &dup, ""), fail),
        (good(relay, acct, 1, &short, ""), fail),
        (big, fail),
        ("not json".into(), fail),
    ];
    for (json, expected) in cases {
        let err = open_credentials(&t, &seal_json(&t, &json)).unwrap_err();
        assert_eq!(
            validation_key(err),
            expected,
            "{}",
            &json[..json.len().min(120)]
        );
    }
}

#[test]
fn secrets_print_redacted() {
    let id = format!("{:?}", MAILBOX);
    assert_eq!(id, "MailboxId(<redacted>)");
    let t = format!("{:?} {:?}", shower(), shower());
    assert!(!t.contains(&key(&shower())));
    assert!(!t.contains("482686"));
    assert!(!t.contains("33"));
}

#[test]
fn secrets_are_wiped_on_drop() {
    fn wiped<T: zeroize::ZeroizeOnDrop>() {}
    wiped::<x25519_dalek::StaticSecret>();
    wiped::<Zeroizing<[u8; 32]>>();
}
