//! The Kit envelope's bytes (phase 5 revision §2.4): a known-answer vector from fixed seeds and a
//! fixed random source, who opens it, and that no byte, key order or confirmed item can change
//! without the envelope failing.

use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::bundle::OldKey;
use super::envelope::{
    build, build_parts, envelope_json, open, peek, Confirmed, EnvelopeInput, HEADER_LEN,
    MAX_ENVELOPE_BYTES,
};
use crate::error::Result;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::cert::{self, CertFields, CertKind, Certificate};
use crate::services::sync::identity::{sign_pub, KitKeys, KitSecret, SignSeed};

const VAULT: [u8; 16] = [0x11; 16];
const AT: i64 = 1_794_816_000;

#[derive(Clone, Copy)]
struct Dev(u8);

impl Dev {
    fn id(self) -> DeviceId {
        DeviceId([self.0; 16])
    }
    fn seed(self) -> SignSeed {
        SignSeed::from_bytes(Zeroizing::new([self.0 ^ 0x40; 32]))
    }
    fn cert(self) -> Certificate {
        let fields = CertFields {
            vault_id: VAULT,
            device: self.id(),
            box_pub: PublicKey::from(&StaticSecret::from([self.0 ^ 0x80; 32])).to_bytes(),
            issued_at: AT,
        };
        cert::issue_self(CertKind::Genesis, &fields, &self.seed()).expect("certificate")
    }
    fn confirmed(self) -> Confirmed {
        Confirmed {
            device: self.id(),
            cert_hash: self.cert().hash(),
        }
    }
}

const A: Dev = Dev(0x0a);
const B: Dev = Dev(0x0b);

fn kit() -> KitKeys {
    KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&VAULT)
}

fn stranger_kit() -> KitKeys {
    KitSecret::from_bytes(Zeroizing::new([0x0d; 32])).derive(&VAULT)
}

/// A random source that fills the n-th request with the byte `start + n`.
fn counter(start: u8) -> impl FnMut(&mut [u8]) -> Result<()> {
    let mut next = start;
    move |buf: &mut [u8]| {
        buf.fill(next);
        next = next.wrapping_add(1);
        Ok(())
    }
}

/// Epochs 0 (head 0), 1 (head 50) and 2 (no head yet: the newest is never verified when sealed).
fn keys() -> Vec<OldKey> {
    vec![
        OldKey {
            epoch: 0,
            key: Zeroizing::new([0x50; 32]),
            head: Some(0),
        },
        OldKey {
            epoch: 1,
            key: Zeroizing::new([0x51; 32]),
            head: Some(50),
        },
        OldKey {
            epoch: 2,
            key: Zeroizing::new([0x52; 32]),
            head: None,
        },
    ]
}

/// Seals with A as the signer through `seal`.
fn with_input<T>(
    keys: &[OldKey],
    confirmed: &[Confirmed],
    seal: impl FnOnce(&EnvelopeInput<'_>) -> T,
) -> T {
    let kit = kit();
    let signer = A.seed();
    let signer_cert = A.cert();
    let input = EnvelopeInput {
        vault_id: VAULT,
        kit_id: kit.id(),
        kit_box_pub: *kit.box_pub(),
        signer: &signer,
        signer_cert: &signer_cert,
        at: AT,
        keys,
        confirmed,
    };
    seal(&input)
}

fn envelope() -> Vec<u8> {
    with_input(&keys(), &[A.confirmed(), B.confirmed()], |input| {
        build(input, &mut counter(1)).expect("envelope")
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_envelope_matches_the_frozen_vector() {
    let bytes = envelope();
    let kit = kit();
    let eph_pub = PublicKey::from(&StaticSecret::from([0x01; 32])).to_bytes();
    let mut header = Vec::new();
    header.extend_from_slice(b"MNYE");
    header.push(0x01);
    header.extend_from_slice(&VAULT);
    header.extend_from_slice(&kit.id().0);
    header.extend_from_slice(&2u32.to_be_bytes());
    header.extend_from_slice(&eph_pub);
    assert_eq!(HEADER_LEN, 73);
    assert_eq!(&bytes[..HEADER_LEN], header.as_slice(), "the header layout");
    assert_eq!(
        &bytes[HEADER_LEN..HEADER_LEN + 24],
        &[0x02; 24],
        "the nonce"
    );

    // Independently of `open`: the key and AAD as revision §2.4 spells them.
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::XChaCha20Poly1305;
    use hkdf::Hkdf;
    let shared = kit.box_secret().diffie_hellman(&PublicKey::from(eph_pub));
    let mut info = b"moony kit envelope v1".to_vec();
    info.extend_from_slice(&eph_pub);
    info.extend_from_slice(kit.box_pub());
    info.extend_from_slice(&2u32.to_be_bytes());
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&VAULT), shared.as_bytes())
        .expand(&info, &mut key)
        .unwrap();
    let plain = XChaCha20Poly1305::new((&key).into())
        .decrypt(
            (&[0x02u8; 24]).into(),
            Payload {
                msg: &bytes[HEADER_LEN + 24..],
                aad: &header,
            },
        )
        .expect("the body");
    let json = std::str::from_utf8(&plain[64..]).expect("JSON");
    assert_eq!(
        json,
        format!(
            "{{\"v\":1,\"by\":\"{}\",\"at\":{AT},\"signer\":\"{}\",\"keys\":[\
             {{\"epoch\":0,\"key\":\"UFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFA\",\"head\":0}},\
             {{\"epoch\":1,\"key\":\"UVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVE\",\"head\":50}},\
             {{\"epoch\":2,\"key\":\"UlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlI\"}}],\
             \"confirmed\":[{{\"device\":\"{}\",\"cert\":\"{}\"}},{{\"device\":\"{}\",\"cert\":\"{}\"}}]}}",
            A.id().to_hex(),
            A.cert().to_b64(),
            A.id().to_hex(),
            hex(&A.cert().hash()),
            B.id().to_hex(),
            hex(&B.cert().hash()),
        )
    );
    let mut signed = b"moony kit envelope v1".to_vec();
    signed.extend_from_slice(&header);
    signed.extend_from_slice(json.as_bytes());
    let sig: [u8; 64] = plain[..64].try_into().unwrap();
    crate::services::sync::identity::verify_sig(&sign_pub(&A.seed()), &signed, &sig)
        .expect("A's signature over the domain, header and JSON");
    assert_eq!(
        hex(&Sha256::digest(&bytes)),
        "d1c125b2f455f3ba6a47ce4291359316dbf3d0401d8d82e0e3dcbe0f73cdaf4c"
    );
    assert_eq!(envelope(), bytes, "deterministic from its inputs");
}

#[test]
fn the_kit_opens_every_key_and_the_confirmed_set() {
    let opened = open(&envelope(), &kit()).expect("opens");
    assert_eq!(opened.header.epoch, 2);
    assert_eq!(opened.header.kit_id, kit().id());
    assert_eq!(opened.by, A.id());
    assert_eq!(opened.at, AT);
    assert_eq!(opened.signer, A.cert());
    let got: Vec<(u32, [u8; 32], Option<u64>)> = opened
        .keys
        .iter()
        .map(|k| (k.epoch, *k.key, k.head))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, [0x50; 32], Some(0)),
            (1, [0x51; 32], Some(50)),
            (2, [0x52; 32], None)
        ]
    );
    assert_eq!(opened.confirmed, vec![A.confirmed(), B.confirmed()]);
    let debug = format!("{opened:?}");
    assert!(
        debug.contains("<redacted>") && !debug.contains("80, 80"),
        "{debug}"
    );
    assert_eq!(peek(&envelope()).expect("peek").epoch, 2);
}

#[test]
fn a_stranger_opens_nothing() {
    let bytes = envelope();
    assert!(open(&bytes, &stranger_kit()).is_err(), "another Kit");
    let other_vault = KitSecret::from_bytes(Zeroizing::new([0x0c; 32])).derive(&[0x12; 16]);
    assert!(
        open(&bytes, &other_vault).is_err(),
        "the same seed, another vault"
    );
    // A stranger's envelope relabelled with this Kit's ID still does not open.
    let stranger = stranger_kit();
    let mut theirs = with_input(&keys(), &[], |input| {
        let input = EnvelopeInput {
            kit_box_pub: *stranger.box_pub(),
            ..*input
        };
        build(&input, &mut counter(1)).expect("envelope")
    });
    theirs[21..37].copy_from_slice(&kit().id().0);
    assert!(open(&theirs, &kit()).is_err());
}

#[test]
fn a_flipped_byte_anywhere_fails() {
    let bytes = envelope();
    let kit = kit();
    for index in 0..bytes.len() {
        for mask in [0x01u8, 0x80] {
            let mut flipped = bytes.clone();
            flipped[index] ^= mask;
            assert!(
                open(&flipped, &kit).is_err(),
                "byte {index} ^ {mask:#x} was accepted"
            );
        }
    }
    assert!(open(&bytes[..bytes.len() - 1], &kit).is_err(), "cut");
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(open(&longer, &kit).is_err(), "extended");
}

/// The sealer signs one JSON and seals another: what a holder of the Kit's box key (never the
/// server) could do, and the signature catches.
#[test]
fn a_changed_confirmed_item_or_key_fails_the_signature() {
    let keys = keys();
    let confirmed = [A.confirmed(), B.confirmed()];
    let changed = |edit: &dyn Fn(&str) -> String| {
        with_input(&keys, &confirmed, |input| {
            let json = envelope_json(input);
            let sealed = edit(&json);
            assert_ne!(sealed.as_str(), json.as_str());
            build_parts(input, json.as_bytes(), sealed.as_bytes(), &mut counter(1))
                .expect("envelope")
        })
    };
    let other_hash = hex(&Dev(0x0e).cert().hash());
    for bytes in [
        changed(&|json| json.replace(&hex(&B.cert().hash()), &other_hash)),
        changed(&|json| json.replace(&B.id().to_hex(), &Dev(0x0e).id().to_hex())),
        changed(&|json| json.replace("\"head\":50", "\"head\":51")),
        changed(&|json| json.replace(&format!("\"at\":{AT}"), "\"at\":1")),
    ] {
        assert!(open(&bytes, &kit()).is_err());
    }
}

#[test]
fn keys_out_of_order_or_not_ending_at_the_header_epoch_fail() {
    let keys = keys();
    // Signed correctly, but the keys run backwards.
    let backwards = with_input(&keys, &[], |input| {
        let json = envelope_json(input);
        let first =
            "{\"epoch\":0,\"key\":\"UFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFA\",\"head\":0}";
        let second =
            "{\"epoch\":1,\"key\":\"UVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVFRUVE\",\"head\":50}";
        let swapped = json.replace(&format!("{first},{second}"), &format!("{second},{first}"));
        assert_ne!(swapped, json.as_str());
        build_parts(
            input,
            swapped.as_bytes(),
            swapped.as_bytes(),
            &mut counter(1),
        )
        .expect("envelope")
    });
    assert!(open(&backwards, &kit()).is_err());
    // The header says epoch 2, the signed JSON holds keys up to 1 only.
    let fewer = with_input(&keys[..2], &[], envelope_json);
    let short = with_input(&keys, &[], |input| {
        build_parts(input, fewer.as_bytes(), fewer.as_bytes(), &mut counter(1)).expect("envelope")
    });
    assert!(open(&short, &kit()).is_err());
    // An item named twice, signed correctly.
    let twice = with_input(&keys, &[A.confirmed()], |input| {
        let json = envelope_json(input);
        let item = format!(
            "{{\"device\":\"{}\",\"cert\":\"{}\"}}",
            A.id().to_hex(),
            hex(&A.cert().hash())
        );
        let doubled = json.replace(&item, &format!("{item},{item}"));
        build_parts(
            input,
            doubled.as_bytes(),
            doubled.as_bytes(),
            &mut counter(1),
        )
        .expect("envelope")
    });
    assert!(open(&twice, &kit()).is_err());
}

#[test]
fn the_build_refuses_what_it_must_never_write() {
    let keys = keys();
    assert!(with_input(&[], &[], |input| build(input, &mut counter(1))).is_err());
    let backwards = [
        OldKey {
            epoch: 1,
            key: Zeroizing::new([1; 32]),
            head: None,
        },
        OldKey {
            epoch: 0,
            key: Zeroizing::new([0; 32]),
            head: None,
        },
    ];
    assert!(with_input(&backwards, &[], |input| build(input, &mut counter(1))).is_err());
    assert!(with_input(&keys, &[A.confirmed(), A.confirmed()], |input| {
        build(input, &mut counter(1))
    })
    .is_err());
    let b_cert = B.cert();
    assert!(with_input(&keys, &[], |input| {
        let input = EnvelopeInput {
            signer_cert: &b_cert,
            ..*input
        };
        build(&input, &mut counter(1))
    })
    .is_err());
    assert!(with_input(&keys, &[], |input| {
        let input = EnvelopeInput {
            kit_box_pub: [0; 32],
            ..*input
        };
        build(&input, &mut counter(1))
    })
    .is_err());
}

#[test]
fn an_envelope_is_at_most_256_kib() {
    let many: Vec<Confirmed> = (0..2400u32)
        .map(|i| {
            let mut id = [0u8; 16];
            id[..4].copy_from_slice(&i.to_be_bytes());
            Confirmed {
                device: DeviceId(id),
                cert_hash: [0x33; 32],
            }
        })
        .collect();
    let keys = keys();
    assert!(with_input(&keys, &many, |input| build(input, &mut counter(1))).is_err());
    let fits = with_input(&keys, &many[..1800], |input| build(input, &mut counter(1)))
        .expect("under the cap");
    assert!(fits.len() <= MAX_ENVELOPE_BYTES);
    assert_eq!(open(&fits, &kit()).expect("opens").confirmed.len(), 1800);
    let mut over = fits.clone();
    over.resize(MAX_ENVELOPE_BYTES + 1, 0);
    assert!(open(&over, &kit()).is_err());
    assert!(peek(&over).is_err());
}
