//! The key-change bundle's bytes (phase 5 spec §6.2, §6.3): a known-answer vector built from
//! fixed seeds and a fixed random source, who opens what, and that no byte can change, move or be
//! reordered without the bundle failing.

use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::bundle::{
    body_json, build, build_with_json, open, open_body_with_key, BundleInput, Header, KitPart,
    Notice, OldKey, Opened, Reason, Recipient, HEADER_LEN, MAX_BUNDLE_BYTES,
};
use crate::error::Result;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::cert::{self, CertFields, CertKind, Certificate};
use crate::services::sync::identity::{sign_pub, Record, RecordType, SignSeed};

const VAULT: [u8; 16] = [0x11; 16];
const NEW_KEY: [u8; 32] = [0x5a; 32];
const OLD_KEY: [u8; 32] = [0x50; 32];
const AT: i64 = 1_794_816_000;

/// A device of the test vault from one byte: its ID, signing seed and box secret.
#[derive(Clone, Copy)]
struct Dev(u8);

impl Dev {
    fn id(self) -> DeviceId {
        DeviceId([self.0; 16])
    }
    fn seed(self) -> SignSeed {
        SignSeed::from_bytes(Zeroizing::new([self.0 ^ 0x40; 32]))
    }
    fn box_secret(self) -> StaticSecret {
        StaticSecret::from([self.0 ^ 0x80; 32])
    }
    fn box_pub(self) -> [u8; 32] {
        PublicKey::from(&self.box_secret()).to_bytes()
    }
    fn cert(self) -> Certificate {
        let fields = CertFields {
            vault_id: VAULT,
            device: self.id(),
            box_pub: self.box_pub(),
            issued_at: AT,
        };
        cert::issue_self(CertKind::Genesis, &fields, &self.seed()).expect("certificate")
    }
    fn recipient(self) -> Recipient {
        Recipient {
            box_pub: self.box_pub(),
            cert: self.cert(),
        }
    }
    /// `self` removes `subject` at epoch 0.
    fn removes(self, subject: Dev) -> Record {
        Record::sign(
            RecordType::Removed,
            VAULT,
            subject.id(),
            self.id(),
            AT,
            0,
            &self.seed(),
        )
        .expect("record")
    }
}

const A: Dev = Dev(0x0a);
const B: Dev = Dev(0x0b);
const C: Dev = Dev(0x0c);
const STRANGER: Dev = Dev(0x0d);

/// A random source that fills the n-th request with the byte `start + n`.
fn counter(start: u8) -> impl FnMut(&mut [u8]) -> Result<()> {
    let mut next = start;
    move |buf: &mut [u8]| {
        buf.fill(next);
        next = next.wrapping_add(1);
        Ok(())
    }
}

/// A removes C: A and B get the key from epoch 0 to 1, C its notice.
fn bundle_with(fill_start: u8, to_epoch: u32) -> Vec<u8> {
    let new_key = Zeroizing::new(NEW_KEY);
    let keys = [OldKey {
        epoch: 0,
        key: Zeroizing::new(OLD_KEY),
        head: Some(0),
    }];
    let removal = A.removes(C);
    let notices = [Notice {
        box_pub: C.box_pub(),
        record: removal.clone(),
    }];
    let recipients = [A.recipient(), B.recipient()];
    let signer_cert = A.cert();
    let signer = A.seed();
    let input = BundleInput {
        vault_id: VAULT,
        from_epoch: to_epoch - 1,
        to_epoch,
        new_key: &new_key,
        recipients: &recipients,
        notices: &notices,
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        reason: Reason::Remove,
        removed: std::slice::from_ref(&removal),
        keys: &keys,
        kit: None,
    };
    build(&input, &mut counter(fill_start)).expect("bundle")
}

fn bundle() -> Vec<u8> {
    bundle_with(1, 1)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn opened_as_recipient(bytes: &[u8], dev: Dev) -> bool {
    matches!(
        open(bytes, &VAULT, &dev.box_secret()),
        Ok(Opened::Recipient(_))
    )
}

/// `(start, len)` of each entry's bytes after the count.
fn entry_spans(bytes: &[u8]) -> Vec<(usize, usize)> {
    let count = u16::from_be_bytes([bytes[HEADER_LEN], bytes[HEADER_LEN + 1]]) as usize;
    let mut at = HEADER_LEN + 2;
    let mut spans = Vec::new();
    for _ in 0..count {
        let len = u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize;
        let total = 2 + 24 + len;
        spans.push((at, total));
        at += total;
    }
    spans
}

#[test]
fn the_bundle_matches_the_frozen_vector() {
    let bytes = bundle();
    let eph_pub = PublicKey::from(&StaticSecret::from([0x01; 32])).to_bytes();
    let mut header = Vec::new();
    header.extend_from_slice(b"MNYK");
    header.push(0x02);
    header.extend_from_slice(&VAULT);
    header.extend_from_slice(&0u32.to_be_bytes());
    header.extend_from_slice(&1u32.to_be_bytes());
    header.extend_from_slice(&eph_pub);
    assert_eq!(&bytes[..HEADER_LEN], header.as_slice(), "the header layout");
    assert_eq!(&bytes[HEADER_LEN..HEADER_LEN + 2], &[0, 3], "three entries");
    let spans = entry_spans(&bytes);
    // A recipient's entry: len = 1 + 32 + 16; a notice's: 1 + 147 + 16.
    let lens: Vec<usize> = spans.iter().map(|(_, total)| total - 26).collect();
    assert_eq!(lens, vec![49, 49, 164]);
    // The nonces come from the random source in order: entries 0x02, 0x03, 0x04, the body 0x05.
    for (i, (start, _)) in spans.iter().enumerate() {
        assert_eq!(&bytes[start + 2..start + 26], &[0x02 + i as u8; 24]);
    }
    let body_at = spans.last().map(|(s, t)| s + t).expect("entries");
    assert_eq!(&bytes[body_at..body_at + 24], &[0x05; 24]);
    // Independently of `open`: A's entry and the body as spec §6.2 spells their keys and AADs.
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::XChaCha20Poly1305;
    use hkdf::Hkdf;
    let shared = A.box_secret().diffie_hellman(&PublicKey::from(eph_pub));
    let mut info = b"moony sync rekey seal v1".to_vec();
    info.extend_from_slice(&eph_pub);
    info.extend_from_slice(&A.box_pub());
    info.extend_from_slice(&1u32.to_be_bytes());
    let mut entry_key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&VAULT), shared.as_bytes())
        .expand(&info, &mut entry_key)
        .unwrap();
    let (start, total) = spans[0];
    let mut aad = header.clone();
    aad.extend_from_slice(&0u16.to_be_bytes());
    let plain = XChaCha20Poly1305::new((&entry_key).into())
        .decrypt(
            (&[0x02u8; 24]).into(),
            Payload {
                msg: &bytes[start + 26..start + total],
                aad: &aad,
            },
        )
        .expect("A's entry");
    assert_eq!(plain[0], 0x01);
    assert_eq!(&plain[1..], &NEW_KEY);
    let mut body_key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&VAULT), &NEW_KEY)
        .expand(b"moony sync rekey body v1", &mut body_key)
        .unwrap();
    let entries_hash = Sha256::digest(&bytes[HEADER_LEN..body_at]);
    let mut aad = header.clone();
    aad.extend_from_slice(&entries_hash);
    let plain = XChaCha20Poly1305::new((&body_key).into())
        .decrypt(
            (&[0x05u8; 24]).into(),
            Payload {
                msg: &bytes[body_at + 24..],
                aad: &aad,
            },
        )
        .expect("the body");
    let json = std::str::from_utf8(&plain[64..]).expect("JSON");
    assert!(json.starts_with(&format!(
        "{{\"v\":2,\"by\":\"{}\",\"at\":{AT},\"reason\":\"remove\",\"signer\":\"{}\",\
         \"recipients\":[{{\"device\":\"{}\",\"cert\":\"",
        A.id().to_hex(),
        A.cert().to_b64(),
        A.id().to_hex()
    )));
    assert!(json.ends_with(&format!(
        "\"removed\":[{{\"device\":\"{}\",\"record\":\"{}\"}}],\
         \"keys\":[{{\"epoch\":0,\"key\":\"UFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFA\",\"head\":0}}]}}",
        C.id().to_hex(),
        A.removes(C).to_b64()
    )));
    let mut signed = b"moony rekey v1".to_vec();
    signed.extend_from_slice(&header);
    signed.extend_from_slice(&entries_hash);
    signed.extend_from_slice(json.as_bytes());
    let sig: [u8; 64] = plain[..64].try_into().unwrap();
    crate::services::sync::identity::verify_sig(&sign_pub(&A.seed()), &signed, &sig)
        .expect("A's signature over the domain, header, entries hash and JSON");
    assert_eq!(
        hex(&Sha256::digest(&bytes)),
        "686e3c2b1c26b046257ae11d549fb8fb4ea6b0ecf6417643ae74497d1809b7c6"
    );
    assert_eq!(bundle(), bytes, "deterministic from its inputs");
}

#[test]
fn each_recipient_opens_the_new_key_and_the_removed_device_only_its_notice() {
    let bytes = bundle();
    for dev in [A, B] {
        let open_unverified = || match open(&bytes, &VAULT, &dev.box_secret()) {
            Ok(Opened::Recipient(unverified)) => unverified,
            other => panic!("a recipient opens the new key: {other:?}"),
        };
        let unverified = open_unverified();
        assert_eq!(unverified.signer(), &A.cert());
        assert_eq!(unverified.header().to_epoch, 1);
        assert!(
            !format!("{unverified:?}").contains("90, 90"),
            "no key bytes in Debug"
        );
        assert!(
            open_unverified().verify(&sign_pub(&B.seed())).is_err(),
            "not by B"
        );
        let verified = unverified
            .verify(&sign_pub(&A.seed()))
            .expect("signed by A");
        let (new_key, header, body) = (verified.new_key, verified.header, verified.body);
        assert_eq!(*new_key, NEW_KEY);
        assert_eq!(
            header,
            Header {
                vault_id: VAULT,
                from_epoch: 0,
                to_epoch: 1,
                eph_pub: PublicKey::from(&StaticSecret::from([0x01; 32])).to_bytes(),
            }
        );
        assert_eq!(body.by, A.id());
        assert_eq!(body.at, AT);
        assert_eq!(body.reason, Reason::Remove);
        assert_eq!(body.signer, A.cert());
        assert_eq!(
            body.recipients,
            vec![(A.id(), A.cert()), (B.id(), B.cert())]
        );
        assert_eq!(body.removed, vec![(C.id(), A.removes(C))]);
    }
    match open(&bytes, &VAULT, &C.box_secret()).expect("opens") {
        Opened::Notice { header, record } => {
            assert_eq!(header.to_epoch, 1);
            assert_eq!(record, A.removes(C));
        }
        other => panic!("the removed device reads only its notice: {other:?}"),
    }
    assert!(matches!(
        open(&bytes, &VAULT, &STRANGER.box_secret()).expect("opens"),
        Opened::Nothing
    ));
}

#[test]
fn the_body_opens_with_the_new_key_alone() {
    let bytes = bundle();
    let unverified = open_body_with_key(&bytes, &VAULT, &NEW_KEY).expect("the Kit's key");
    assert_eq!(
        (unverified.header().from_epoch, unverified.header().to_epoch),
        (0, 1)
    );
    let verified = unverified
        .verify(&sign_pub(&A.seed()))
        .expect("signed by A");
    assert_eq!(*verified.new_key, NEW_KEY);
    assert!(open_body_with_key(&bytes, &VAULT, &NEW_KEY)
        .expect("opens")
        .verify(&sign_pub(&B.seed()))
        .is_err());
    assert!(open_body_with_key(&bytes, &VAULT, &OLD_KEY).is_err());
    assert!(open_body_with_key(&bytes, &[0x12; 16], &NEW_KEY).is_err());
}

#[test]
fn the_older_keys_round_trip_with_their_heads() {
    let body = open_body_with_key(&bundle(), &VAULT, &NEW_KEY)
        .expect("opens")
        .verify(&sign_pub(&A.seed()))
        .expect("signed by A")
        .body;
    let keys: Vec<(u32, [u8; 32], Option<u64>)> = body
        .keys
        .iter()
        .map(|old| (old.epoch, *old.key, old.head))
        .collect();
    assert_eq!(keys, vec![(0, OLD_KEY, Some(0))]);
    let debug = format!("{body:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("80, 80"), "no key bytes");
}

#[test]
fn every_flipped_byte_fails() {
    let bytes = bundle();
    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 1;
        for dev in [A, B] {
            assert!(!opened_as_recipient(&bad, dev), "byte {i}");
        }
        assert!(
            open_body_with_key(&bad, &VAULT, &NEW_KEY).is_err(),
            "the body, byte {i}"
        );
    }
}

#[test]
fn a_notice_does_not_survive_a_flipped_byte_in_its_header_or_entry() {
    let bytes = bundle();
    let (start, total) = entry_spans(&bytes)[2];
    for i in (0..HEADER_LEN).chain(start..start + total) {
        let mut bad = bytes.clone();
        bad[i] ^= 1;
        assert!(
            !matches!(
                open(&bad, &VAULT, &C.box_secret()),
                Ok(Opened::Notice { .. })
            ),
            "byte {i}"
        );
    }
}

#[test]
fn an_entry_moved_into_another_bundle_does_not_open() {
    let ours = bundle();
    let theirs = bundle_with(0x41, 1);
    let (start, total) = entry_spans(&ours)[0];
    let (their_start, their_total) = entry_spans(&theirs)[0];
    assert_eq!(total, their_total);
    let mut spliced = theirs.clone();
    spliced[their_start..their_start + total].copy_from_slice(&ours[start..start + total]);
    assert!(!opened_as_recipient(&spliced, A));
    // Under the same ephemeral key but another epoch: the header differs, so it fails too.
    let later = bundle_with(1, 2);
    let mut spliced = later.clone();
    let (later_start, _) = entry_spans(&later)[0];
    spliced[later_start..later_start + total].copy_from_slice(&ours[start..start + total]);
    assert!(!opened_as_recipient(&spliced, A));
}

#[test]
fn reordered_entries_fail_the_body() {
    let bytes = bundle();
    let spans = entry_spans(&bytes);
    let (a_start, a_total) = spans[0];
    let (b_start, b_total) = spans[1];
    let mut swapped = bytes.clone();
    swapped[a_start..a_start + a_total].copy_from_slice(&bytes[b_start..b_start + b_total]);
    swapped[b_start..b_start + b_total].copy_from_slice(&bytes[a_start..a_start + a_total]);
    assert!(open_body_with_key(&swapped, &VAULT, &NEW_KEY).is_err());
    assert!(!opened_as_recipient(&swapped, A));
    assert!(!opened_as_recipient(&swapped, B));
}

#[test]
fn a_bundle_of_another_vault_or_junk_is_refused() {
    let bytes = bundle();
    assert!(open(&bytes, &[0x12; 16], &A.box_secret()).is_err());
    for junk in [&b""[..], b"MNYK", &[0u8; 200][..], &bytes[..HEADER_LEN + 1]] {
        assert!(open(junk, &VAULT, &A.box_secret()).is_err());
    }
    let mut too_big = bytes.clone();
    too_big.resize(MAX_BUNDLE_BYTES + 1, 0);
    assert!(open(&too_big, &VAULT, &A.box_secret()).is_err());
}

#[test]
fn a_bundle_out_of_bounds_refuses_to_build() {
    let new_key = Zeroizing::new(NEW_KEY);
    let signer = A.seed();
    let signer_cert = A.cert();
    let many: Vec<Recipient> = (0..65).map(|_| B.recipient()).collect();
    let notices: Vec<Notice> = (0..65)
        .map(|_| Notice {
            box_pub: C.box_pub(),
            record: A.removes(C),
        })
        .collect();
    let records: Vec<Record> = (0..1500).map(|_| A.removes(C)).collect();
    let low_order = [Recipient {
        box_pub: [0; 32],
        cert: B.cert(),
    }];
    let newer = [OldKey {
        epoch: 1,
        key: Zeroizing::new(OLD_KEY),
        head: None,
    }];
    let base = || BundleInput {
        vault_id: VAULT,
        from_epoch: 0,
        to_epoch: 1,
        new_key: &new_key,
        recipients: &[],
        notices: &[],
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        reason: Reason::Rotate,
        removed: &[],
        keys: &[],
        kit: None,
    };
    assert!(build(&base(), &mut counter(1)).is_ok());
    let b_cert = B.cert();
    let cases = [
        BundleInput {
            recipients: &many,
            ..base()
        },
        BundleInput {
            notices: &notices,
            ..base()
        },
        BundleInput {
            removed: &records,
            ..base()
        },
        BundleInput {
            recipients: &low_order,
            ..base()
        },
        BundleInput {
            keys: &newer,
            ..base()
        },
        BundleInput {
            to_epoch: 2,
            ..base()
        },
        BundleInput {
            signer_cert: &b_cert,
            ..base()
        },
    ];
    for (i, input) in cases.iter().enumerate() {
        assert!(build(input, &mut counter(1)).is_err(), "case {i}");
    }
}

#[test]
fn a_bundle_of_64_recipients_fits() {
    let new_key = Zeroizing::new(NEW_KEY);
    let signer = A.seed();
    let signer_cert = A.cert();
    let recipients: Vec<Recipient> = (0..64u8).map(|i| Dev(0x10 + i).recipient()).collect();
    let input = BundleInput {
        vault_id: VAULT,
        from_epoch: 0,
        to_epoch: 1,
        new_key: &new_key,
        recipients: &recipients,
        notices: &[],
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        reason: Reason::Rotate,
        removed: &[],
        keys: &[],
        kit: None,
    };
    let bytes = build(&input, &mut counter(1)).expect("64 recipients");
    assert!(bytes.len() < 64 * 1024, "{} bytes", bytes.len());
    assert!(opened_as_recipient(&bytes, Dev(0x10 + 63)));
}

// ── Review of Task 6: what `open` refuses ───────────────────────────────────────────────────

#[test]
fn a_truncated_bundle_or_one_with_a_trailing_byte_is_refused() {
    let bytes = bundle();
    for cut in [HEADER_LEN + 2, HEADER_LEN + 40, bytes.len() - 1] {
        assert!(
            open(&bytes[..cut], &VAULT, &A.box_secret()).is_err(),
            "cut at {cut}"
        );
        assert!(open_body_with_key(&bytes[..cut], &VAULT, &NEW_KEY).is_err());
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(!opened_as_recipient(&longer, A));
    assert!(open_body_with_key(&longer, &VAULT, &NEW_KEY).is_err());
}

#[test]
fn a_low_order_ephemeral_key_opens_nothing() {
    let mut bytes = bundle();
    // The all-zero point is of low order: no contributory exchange, no entry for anyone.
    bytes[29..HEADER_LEN].fill(0);
    assert!(matches!(
        open(&bytes, &VAULT, &A.box_secret()).expect("parses"),
        Opened::Nothing
    ));
}

/// The test bundle with its body JSON passed through `edit`, signed by A.
fn bundle_with_json(edit: impl Fn(&str) -> String) -> Vec<u8> {
    let new_key = Zeroizing::new(NEW_KEY);
    let keys = [OldKey {
        epoch: 0,
        key: Zeroizing::new(OLD_KEY),
        head: Some(0),
    }];
    let removal = A.removes(C);
    let recipients = [A.recipient(), B.recipient()];
    let signer_cert = A.cert();
    let signer = A.seed();
    let input = BundleInput {
        vault_id: VAULT,
        from_epoch: 0,
        to_epoch: 1,
        new_key: &new_key,
        recipients: &recipients,
        notices: &[],
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        reason: Reason::Remove,
        removed: std::slice::from_ref(&removal),
        keys: &keys,
        kit: None,
    };
    let json = Zeroizing::new(edit(&body_json(&input)));
    build_with_json(&input, json, &mut counter(1)).expect("bundle")
}

#[test]
fn a_body_that_breaks_its_own_rules_is_refused() {
    // The untouched JSON opens: the edits below are what fails.
    assert!(open_body_with_key(&bundle_with_json(|j| j.to_string()), &VAULT, &NEW_KEY).is_ok());
    let a = A.id().to_hex();
    let b = B.id().to_hex();
    let c = C.id().to_hex();
    let old = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, OLD_KEY);
    type Edit = Box<dyn Fn(&str) -> String>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a duplicate epoch in keys",
            Box::new(move |j: &str| {
                j.replace(
                    "\"keys\":[",
                    &format!("\"keys\":[{{\"epoch\":0,\"key\":\"{old}\"}},"),
                )
            }),
        ),
        (
            "by is not the signer certificate's device",
            Box::new({
                let a = a.clone();
                let b = b.clone();
                move |j: &str| j.replacen(&format!("\"by\":\"{a}\""), &format!("\"by\":\"{b}\""), 1)
            }),
        ),
        (
            "a certificate listed under another device",
            Box::new({
                let b = b.clone();
                let c = c.clone();
                move |j: &str| {
                    j.replacen(
                        &format!("{{\"device\":\"{b}\",\"cert\""),
                        &format!("{{\"device\":\"{c}\",\"cert\""),
                        1,
                    )
                }
            }),
        ),
        (
            "a record listed under another device",
            Box::new(move |j: &str| {
                j.replacen(
                    &format!("{{\"device\":\"{c}\",\"record\""),
                    &format!("{{\"device\":\"{b}\",\"record\""),
                    1,
                )
            }),
        ),
    ];
    for (what, edit) in cases {
        let bytes = bundle_with_json(|j| {
            let edited = edit(j);
            assert_ne!(edited, j, "{what}: the edit applies");
            edited
        });
        assert!(
            open_body_with_key(&bytes, &VAULT, &NEW_KEY).is_err(),
            "{what}"
        );
        assert!(open(&bytes, &VAULT, &A.box_secret()).is_err(), "{what}");
    }
}

// ── Revision §2.5: the bundle names the vault's Kit ─────────────────────────────────────────

/// The test Kit (from a fixed seed), created by A.
fn kit_keys(byte: u8) -> crate::services::sync::identity::KitKeys {
    crate::services::sync::identity::KitSecret::from_bytes(Zeroizing::new([byte; 32]))
        .derive(&VAULT)
}

fn kit_cert(byte: u8) -> Certificate {
    cert::issue_kit(&A.seed(), A.id(), &kit_keys(byte), AT).expect("Kit certificate")
}

/// A replaces the Kit of seed 0x31 with the Kit of seed 0x32, nobody removed: what `with` makes
/// of that input.
fn with_kit_input<T>(kit: &KitPart, with: impl FnOnce(&BundleInput<'_>) -> T) -> T {
    let new_key = Zeroizing::new(NEW_KEY);
    let recipients = [A.recipient(), B.recipient()];
    let signer_cert = A.cert();
    let signer = A.seed();
    let input = BundleInput {
        vault_id: VAULT,
        from_epoch: 0,
        to_epoch: 1,
        new_key: &new_key,
        recipients: &recipients,
        notices: &[],
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        reason: Reason::Rotate,
        removed: &[],
        keys: &[],
        kit: Some(kit),
    };
    with(&input)
}

fn kit_input_bundle(kit: &KitPart) -> Vec<u8> {
    with_kit_input(kit, |input| build(input, &mut counter(1)).expect("bundle"))
}

fn replacing_kit() -> KitPart {
    let old = kit_keys(0x31).id();
    KitPart {
        cert: kit_cert(0x32),
        replaces: Some(
            Record::sign(RecordType::Removed, VAULT, old, A.id(), AT, 0, &A.seed())
                .expect("record"),
        ),
    }
}

#[test]
fn the_kit_and_what_it_replaces_round_trip_and_match_their_vector() {
    let kit = replacing_kit();
    let bytes = kit_input_bundle(&kit);

    let verified = open_body_with_key(&bytes, &VAULT, &NEW_KEY)
        .expect("opens")
        .verify(&sign_pub(&A.seed()))
        .expect("verifies");

    assert_eq!(verified.body.kit.as_ref(), Some(&kit));
    // The JSON ends with the Kit, its replaced Kit's removal last.
    let json = with_kit_input(&kit, |input| body_json(input).to_string());
    let old = kit.replaces.as_ref().expect("replaces");
    assert!(
        json.ends_with(&format!(
            "\"keys\":[],\"kit\":{{\"device\":\"{}\",\"cert\":\"{}\",\
             \"replaces\":{{\"device\":\"{}\",\"record\":\"{}\"}}}}}}",
            kit.cert.body.device.to_hex(),
            kit.cert.to_b64(),
            old.subject.to_hex(),
            old.to_b64()
        )),
        "{json}"
    );
    assert_eq!(
        hex(&Sha256::digest(&bytes)),
        "2929ddc14ec502c2fd4d86e4553c7ff470cb0e963ede370bb4c797455d033190"
    );
    assert_eq!(
        kit_input_bundle(&kit),
        bytes,
        "deterministic from its inputs"
    );
    // A Kit that stays: no `replaces`.
    let staying = KitPart {
        cert: kit_cert(0x32),
        replaces: None,
    };
    let opened = open_body_with_key(&kit_input_bundle(&staying), &VAULT, &NEW_KEY)
        .expect("opens")
        .verify(&sign_pub(&A.seed()))
        .expect("verifies");
    assert_eq!(opened.body.kit, Some(staying));
    // No Kit: no field at all.
    let plain = open_body_with_key(&bundle(), &VAULT, &NEW_KEY)
        .expect("opens")
        .verify(&sign_pub(&A.seed()))
        .expect("verifies");
    assert_eq!(plain.body.kit, None);
}

#[test]
fn a_kit_that_is_not_one_refuses_to_build_or_open() {
    // Built: a device certificate as the Kit, or a Kit replacing itself, or a left record.
    let as_kit = KitPart {
        cert: B.cert(),
        replaces: None,
    };
    let new_key = Zeroizing::new(NEW_KEY);
    let signer_cert = A.cert();
    let signer = A.seed();
    let itself = KitPart {
        cert: kit_cert(0x32),
        replaces: Some(
            Record::sign(
                RecordType::Removed,
                VAULT,
                kit_keys(0x32).id(),
                A.id(),
                AT,
                0,
                &A.seed(),
            )
            .expect("record"),
        ),
    };
    let left = KitPart {
        cert: kit_cert(0x32),
        replaces: Some(
            Record::sign(
                RecordType::Left,
                VAULT,
                kit_keys(0x31).id(),
                kit_keys(0x31).id(),
                AT,
                0,
                kit_keys(0x31).sign_seed(),
            )
            .expect("record"),
        ),
    };
    for kit in [&as_kit, &itself, &left] {
        let input = BundleInput {
            vault_id: VAULT,
            from_epoch: 0,
            to_epoch: 1,
            new_key: &new_key,
            recipients: &[],
            notices: &[],
            signer: &signer,
            signer_cert: &signer_cert,
            at: AT,
            reason: Reason::Rotate,
            removed: &[],
            keys: &[],
            kit: Some(kit),
        };
        assert!(build(&input, &mut counter(1)).is_err(), "{kit:?}");
    }
    // Opened: a JSON edited the same ways (signed by A all the same) is refused.
    let kit = replacing_kit();
    let new_device = kit.cert.body.device.to_hex();
    let old_device = kit.replaces.as_ref().expect("old").subject.to_hex();
    let b_cert = B.cert().to_b64();
    let kit_b64 = kit.cert.to_b64();
    type Edit = Box<dyn Fn(&str) -> String>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a device certificate as the Kit",
            Box::new({
                let new_device = new_device.clone();
                move |j: &str| {
                    j.replace(&kit_b64, &b_cert)
                        .replace(&new_device, &B.id().to_hex())
                }
            }),
        ),
        (
            "the Kit listed under another device",
            Box::new({
                let new_device = new_device.clone();
                move |j: &str| {
                    j.replace(
                        &format!("\"kit\":{{\"device\":\"{new_device}\""),
                        &format!("\"kit\":{{\"device\":\"{}\"", C.id().to_hex()),
                    )
                }
            }),
        ),
        (
            "the removal listed under another device",
            Box::new(move |j: &str| {
                j.replace(
                    &format!("\"replaces\":{{\"device\":\"{old_device}\""),
                    &format!("\"replaces\":{{\"device\":\"{}\"", C.id().to_hex()),
                )
            }),
        ),
    ];
    for (what, edit) in cases {
        let bytes = with_kit_input(&kit, |input| {
            let original = body_json(input);
            let edited = edit(&original);
            assert_ne!(
                edited.as_str(),
                original.as_str(),
                "{what}: the edit applies"
            );
            build_with_json(input, Zeroizing::new(edited), &mut counter(1)).expect("bundle")
        });
        assert!(
            open_body_with_key(&bytes, &VAULT, &NEW_KEY).is_err(),
            "{what}"
        );
    }
}
