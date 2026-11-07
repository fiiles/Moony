use std::io::Write;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::*;
use crate::services::sync::{changeset, journal};

const KEY: [u8; 32] = [0x11; 32];
const VID: [u8; 16] = [0x22; 16];
const JSON: &[u8] = br#"{"v":1,"device":"AAAAAAAAAAAAAAAAAAAAAA","seq":1,"rows":[]}"#;

fn only(epoch: u32, key: [u8; 32]) -> impl Fn(u32) -> Option<Zeroizing<[u8; 32]>> {
    move |e| (e == epoch).then(|| Zeroizing::new(key))
}

fn op_aad(epoch: u32, vault_id: &[u8; 16]) -> Vec<u8> {
    let mut aad = b"MNY1".to_vec();
    aad.extend_from_slice(&epoch.to_be_bytes());
    aad.extend_from_slice(vault_id);
    aad
}

/// `len` bytes that deflate does not shrink (a SHA-256 counter stream).
fn incompressible(len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 32);
    let mut counter = 0u64;
    while out.len() < len {
        out.extend_from_slice(&Sha256::digest(counter.to_be_bytes()));
        counter += 1;
    }
    out.truncate(len);
    out
}

fn raw_deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn padded(mut bytes: Vec<u8>) -> Vec<u8> {
    let len = bytes.len().div_ceil(PAD).max(1) * PAD;
    bytes.resize(len, 0);
    bytes
}

#[test]
fn derivations_match_frozen_vectors() {
    // Computed with Python's hmac/hashlib HKDF (RFC 5869), independent of the hkdf crate.
    assert_eq!(
        hex::encode(*hkdf32(&KEY, &VID, OPS_INFO)),
        "09f78d58938f72bc6f179f74097299ca56abab13c933c18f7f684e127d66ce03"
    );
    assert_eq!(
        vault_token(&KEY, &VID),
        "xEoN5KY965vOsERjjinYPzJYkS-KXnBEs1SCP_enuvQ"
    );
    assert_eq!(
        hex::encode(*hkdf32(&KEY, &[0x33; 32], SNAPSHOT_INFO)),
        "45f7a1d21bc1b4cf86eb4983adfe38ad5e638139b58c2152c674fce8ee0021e0"
    );
}

#[test]
fn op_id_is_the_first_32_base64url_characters_of_the_sha256() {
    assert_eq!(
        op_id(b"example envelope"),
        "cI0pGQzv4c-0fC9x6sZvDPiCRmqc-xG0"
    );
    assert_eq!(
        envelope_hash(b"example envelope").to_vec(),
        Sha256::digest(b"example envelope").to_vec()
    );
}

#[test]
fn xchacha_usage_matches_the_draft_vector() {
    // draft-irtf-cfrg-xchacha-03 §A.3.1, copied from the draft.
    let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip \
                      for the future, sunscreen would be it.";
    let aad = hex::decode("50515253c0c1c2c3c4c5c6c7").unwrap();
    let key: [u8; 32] =
        hex::decode("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")
            .unwrap()
            .try_into()
            .unwrap();
    let nonce: [u8; 24] = hex::decode("404142434445464748494a4b4c4d4e4f5051525354555657")
        .unwrap()
        .try_into()
        .unwrap();
    let ciphertext = "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb\
                      731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452\
                      2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9\
                      21f9664c97637da9768812f615c68b13b52e";
    let tag = "c0875924c1c7987947deafd8780acf49";

    let sealed = aead_seal(&key, &nonce, &aad, plaintext).unwrap();
    assert_eq!(hex::encode(&sealed), format!("{ciphertext}{tag}"));
    assert_eq!(
        aead_open(&key, &nonce, &aad, &sealed).unwrap(),
        plaintext.to_vec()
    );
}

#[test]
fn envelope_layout_is_magic_epoch_nonce_ciphertext() {
    let envelope = seal_op_with_nonce(7, &KEY, &VID, JSON, [0x44; 24]).unwrap();
    let plain = compress_padded(JSON).unwrap();

    assert_eq!(&envelope[0..4], b"MNY1");
    assert_eq!(&envelope[4..8], &7u32.to_be_bytes());
    assert_eq!(&envelope[8..32], &[0x44; 24]);
    assert_eq!(envelope.len(), ENVELOPE_OVERHEAD + plain.len());
    assert_eq!(envelope.len(), 48 + 1024);

    let op_key = hkdf32(&KEY, &VID, OPS_INFO);
    let cipher = XChaCha20Poly1305::new_from_slice(&op_key[..]).unwrap();
    let aad = op_aad(7, &VID);
    let decrypted = cipher
        .decrypt(
            (&[0x44u8; 24]).into(),
            Payload {
                msg: &envelope[32..],
                aad: &aad,
            },
        )
        .unwrap();
    assert_eq!(decrypted, plain);
}

#[test]
fn padded_plaintext_length_matches_the_journal_estimate() {
    let repetitive = format!(
        r#"{{"v":1,"rows":[{}]}}"#,
        vec![r#"{"t":"bank_transactions","pk":"x","cols":{"amount":"12.50"}}"#; 400].join(",")
    );
    let random = STANDARD.encode(incompressible(52_500));
    assert_eq!(random.len(), 70_000);
    let jsons: [&[u8]; 5] = [
        b"",
        br#"{"rows":[]}"#,
        JSON,
        repetitive.as_bytes(),
        random.as_bytes(),
    ];
    for json in jsons {
        let plain = compress_padded(json).unwrap();
        assert_eq!(
            plain.len(),
            changeset::padded_deflated_len(json).unwrap(),
            "{} bytes of JSON",
            json.len()
        );
        assert_eq!(plain.len() % PAD, 0);
        assert!(plain.len() >= PAD);
        let deflated = raw_deflate(json);
        assert_eq!(&plain[..deflated.len()], &deflated[..]);
        assert!(plain[deflated.len()..].iter().all(|&b| b == 0));
    }
    assert!(compress_padded(random.as_bytes()).unwrap().len() > 50 * 1024);
}

#[test]
fn seal_then_open_round_trips_and_nonces_differ() {
    let big = STANDARD.encode(incompressible(30_000));
    for json in [&b""[..], JSON, big.as_bytes()] {
        let a = seal_op(3, &KEY, &VID, json).unwrap();
        let b = seal_op(3, &KEY, &VID, json).unwrap();
        assert_ne!(a[8..32], b[8..32], "fresh nonces");
        assert_ne!(op_id(&a), op_id(&b));
        assert_eq!(open_op(only(3, KEY), &VID, &a).unwrap(), json.to_vec());
        assert_eq!(open_op(only(3, KEY), &VID, &b).unwrap(), json.to_vec());
    }
}

#[test]
fn every_flipped_byte_fails_to_open() {
    let envelope = seal_op_with_nonce(0, &KEY, &VID, JSON, [0x44; 24]).unwrap();
    assert_eq!(envelope.len(), 48 + 1024);
    for i in 0..envelope.len() {
        let mut tampered = envelope.clone();
        tampered[i] ^= 0x01;
        let error = open_op(only(0, KEY), &VID, &tampered).expect_err("tampered");
        let expected = match i {
            0..4 => OpenError::Malformed,
            4..8 => OpenError::UnknownEpoch(u32::from_be_bytes(tampered[4..8].try_into().unwrap())),
            _ => OpenError::Authentication,
        };
        assert_eq!(error, expected, "byte {i}");
    }
}

#[test]
fn wrong_vault_id_or_epoch_fails_authentication() {
    let envelope = seal_op(5, &KEY, &VID, JSON).unwrap();

    assert_eq!(
        open_op(only(5, KEY), &[0x23; 16], &envelope),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open_op(only(5, [0x12; 32]), &VID, &envelope),
        Err(OpenError::Authentication)
    );
    let mut relabelled = envelope.clone();
    relabelled[4..8].copy_from_slice(&6u32.to_be_bytes());
    let any_epoch = |_| Some(Zeroizing::new(KEY));
    assert_eq!(
        open_op(any_epoch, &VID, &relabelled),
        Err(OpenError::Authentication)
    );
}

#[test]
fn unknown_epoch_is_reported() {
    let envelope = seal_op(7, &KEY, &VID, JSON).unwrap();
    assert_eq!(
        open_op(only(6, KEY), &VID, &envelope),
        Err(OpenError::UnknownEpoch(7))
    );
}

#[test]
fn lengths_outside_the_envelope_bounds_are_refused() {
    let envelope = seal_op(0, &KEY, &VID, JSON).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope[..48 + 1023]),
        Err(OpenError::Malformed)
    );
    assert_eq!(open_op(only(0, KEY), &VID, b""), Err(OpenError::Malformed));

    let mut huge = envelope.clone();
    huge.resize(MAX_OP_BYTES + 1, 0);
    assert_eq!(open_op(only(0, KEY), &VID, &huge), Err(OpenError::TooLarge));

    let mut wrong_magic = envelope.clone();
    wrong_magic[..4].copy_from_slice(b"MNY2");
    assert_eq!(
        open_op(only(0, KEY), &VID, &wrong_magic),
        Err(OpenError::Malformed)
    );
}

#[test]
fn non_zero_padding_is_refused() {
    let mut plain = compress_padded(JSON).unwrap();
    let last = plain.len() - 1;
    plain[last] = 0x01;
    let envelope = seal_padded(0, &KEY, &VID, &plain, [0x44; 24]).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope),
        Err(OpenError::Padding)
    );

    let clean = seal_padded(0, &KEY, &VID, &compress_padded(JSON).unwrap(), [0x44; 24]).unwrap();
    assert_eq!(open_op(only(0, KEY), &VID, &clean).unwrap(), JSON.to_vec());
}

#[test]
fn a_broken_deflate_stream_is_malformed() {
    // Authenticated but not a deflate stream: block type 0b11 is reserved.
    let mut garbage = vec![0u8; PAD];
    garbage[0] = 0xff;
    let envelope = seal_padded(0, &KEY, &VID, &garbage, [0x44; 24]).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope),
        Err(OpenError::Malformed)
    );

    // A stream cut short never reaches its end.
    let deflated = raw_deflate(incompressible(4_000).as_slice());
    let envelope = seal_padded(0, &KEY, &VID, &deflated[..2 * PAD], [0x44; 24]).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope),
        Err(OpenError::Malformed)
    );
}

#[test]
fn oversize_inflation_is_refused() {
    let plain = padded(raw_deflate(&vec![0u8; 5 * 1024 * 1024]));
    assert!(plain.len() + ENVELOPE_OVERHEAD <= MAX_OP_BYTES);
    let envelope = seal_padded(0, &KEY, &VID, &plain, [0x44; 24]).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope),
        Err(OpenError::TooLarge)
    );

    let at_limit = padded(raw_deflate(&vec![b' '; MAX_CHANGESET_JSON]));
    let envelope = seal_padded(0, &KEY, &VID, &at_limit, [0x44; 24]).unwrap();
    assert_eq!(
        open_op(only(0, KEY), &VID, &envelope).unwrap().len(),
        MAX_CHANGESET_JSON
    );
}

#[test]
fn the_largest_single_row_envelope_fits_the_relay_limit() {
    const { assert!(journal::SINGLE_ROW_LIMIT + ENVELOPE_OVERHEAD <= MAX_OP_BYTES) };
}
