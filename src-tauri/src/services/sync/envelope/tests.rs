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

#[test]
fn op_envelope_matches_the_frozen_vector() {
    // Python: zlib.compressobj(6, DEFLATED, -15) of JSON, zero-padded to 1 KiB, sealed with HKDF
    // from `cryptography` and XChaCha20-Poly1305 built from a hand-written HChaCha20 (checked
    // against draft-irtf-cfrg-xchacha-03 §2.2.1 and §A.3.1) over `cryptography`'s ChaCha20Poly1305.
    let deflated =
        hex::decode("ab562a53b232d4514a492dcb4c4e55b25272c40a9474948a530bc12a8bf2cb8b95aca2636b01")
            .unwrap();
    let envelope = seal_padded(7, &KEY, &VID, &padded(deflated), [0x44; 24]).unwrap();
    let expected = "4d4e59310000000744444444444444444444444444444444444444444444444411147f16229cf6c5\
        ba807303b1fecc9c4d23bcd06c0abe39e798208cbb77fc810944ba93138d804b7c78e0f75a28dd46a578bfe6e8dc\
        aa7006185d4e23cc1bb8536e76089b4a0b7876542993638711279cf6f800948d596426a26d15000b559fed08f24b\
        0f3557bbfa2456221dcc1879e6535447610dec049f11ac6bed3d530bc46fefc0c1b438c3107bfa4e84d8c666a049\
        ae1311d03098adc9a3bb363a3fce87874d9078bae13293b773e4203870ef2607e959aa5314920e11244852ec232c\
        4f236b1ddfb67808b3cc412c5531f188abf72eaf56c25bed174b3bb69b7aa91748284b61bcc2e67d42e2d3330d05\
        686b958a0bfb6129ff539421d54ae72545d8525fa9c1b5821a471cdc95f01d8fbeeb9fd99c8d30e2571314e7a316\
        f268cfda82684f4c18577d822c1aa05877eadecd7bf3f0e78d5c09c82c88521e7c7d454b9fdd7c3ca6a46ac2f20d\
        b33dea6aa1102ae5549de11072edeac937be346693e2e4d9d923ba1ca75b1f44a4d499dc40cf4b9491d8ed9b6774\
        65a78bf5a3f3b4a525dab4f53e1ce4a9b180aa7043ab0a6c3347a8756a4844d936dd9967335146ad9ee41ee6d647\
        0f16974d8d40c2075080a8e1a9affb00d4e99db865aa0b4552d03f0934b28af097ec7540c7ebe63f5da4c7902543\
        eca8d3452d636f06a7dbbd75b5e26b2b7b80853fa0694bbd37697bdd22da82a86d478a5c6658c75151709003\
        56ab5e790e50a30b1cbf9152bd57d09b1549d859901dd5cec5622730dd036e59fd16a6894af2c2e8958c5eed2ad6\
        68879af5b24cc9246efd532dae491346d038a14ab4fc79ad4bfcc2d1dee74f4c7ff093df42b85edfc41cd92a7df2\
        c72dc104913c9fa4f4463e88ce427f7f4244876e4a094a3597dcdb3c23cc4079de25125dbee1050e497ff2f5c78e\
        5252de92cd69dd4e29d240c5cc9e12dd2b66347b4263da948f9658d78f15d94d546d00513938b285f159dd0d7e1e\
        2a3e2c681ec9008d88da9c1b0a5be61438f91d45e9b40cd9a2a68df7c7023b33e48d74c2c4fb4eda21c213ffee0e\
        b9c9c17f9cc2a1c705f583d9023880824928bdb3e21f6fc548adf359fda0a5aaa16b4a1d2b980dfd911069f3800d\
        ac8466bf523a27eab8ef23a577115c4066275fb99f92fa91613ab69b07429f21a632ddca28992301ec670be0aeed\
        4f42ab9f808f0dfbfb21f1cf998ded8e2b7dc2bab8e62f947de32c011427daf6ea91a63a65e9754eca723e8f9927\
        75ddbb642a79ce3036feccd44909257d1b5bcbb68e9beeb05b4b5c0da11d11e3da9f5d8e2c374ce52a28a45d797f\
        2eddba7aea4640b8ff4938596374a1be2e0cbc627e99f98b200b94869c96e2668d64d5fcb44d760d509dcb1ccae5\
        fda024ad5f010c64fa30044174768865c436cc9bfe8de5ad97d8170bbebc166c8d4ae37103823d4bd6bfae0fad8c\
        7c995b22edcf26e157d00e41b244da2a2334beddd1c1";
    assert_eq!(envelope.len(), 1072);
    assert_eq!(hex::encode(&envelope), expected);
    assert_eq!(
        open_op(only(7, KEY), &VID, &envelope).unwrap(),
        JSON.to_vec()
    );
}

// ---- STREAM (snapshots) ----

use super::stream::{open_stream, snapshot_object_id, StreamSealer, CHUNK, HEADER_LEN};

const TAG: usize = 16;
const SEALED_CHUNK: usize = CHUNK + TAG;
const OBJECT: [u8; 8] = 42u64.to_be_bytes();

fn seal_stream(epoch: u32, object_id: &[u8], input: &[u8]) -> Vec<u8> {
    let mut sealer = StreamSealer::new(epoch, &KEY, &VID, object_id);
    sealer.write_all(input).unwrap();
    sealer.finish().unwrap()
}

fn fixed_stream(input: &[u8]) -> Vec<u8> {
    let mut sealer = StreamSealer::new_with(
        3,
        &KEY,
        &VID,
        &snapshot_object_id(42),
        [0x33; 32],
        [0x55; 19],
    );
    sealer.write_all(input).unwrap();
    sealer.finish().unwrap()
}

fn stream_nonce(prefix: [u8; 19], counter: u32, last: bool) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[..19].copy_from_slice(&prefix);
    nonce[19..23].copy_from_slice(&counter.to_be_bytes());
    nonce[23] = u8::from(last);
    nonce
}

fn stream_aad(epoch: u32, object_id: &[u8]) -> Vec<u8> {
    let mut aad = op_aad(epoch, &VID);
    aad.extend_from_slice(object_id);
    aad
}

#[test]
fn stream_round_trips_empty_one_byte_exact_chunk_and_multi_chunk_inputs() {
    for len in [0usize, 1, CHUNK - 1, CHUNK, CHUNK + 1, 200_001] {
        let input = incompressible(len);
        let sealed = seal_stream(1, &OBJECT, &input);
        // Every chunk but the last is full, so the last one carries 0..CHUNK bytes.
        let full = len / CHUNK;
        assert_eq!(
            sealed.len(),
            HEADER_LEN + full * SEALED_CHUNK + (len % CHUNK) + TAG,
            "{len} bytes"
        );
        assert_eq!(
            open_stream(only(1, KEY), &VID, &OBJECT, &sealed).unwrap(),
            input,
            "{len} bytes"
        );

        // Writing in odd pieces produces a stream of the same shape that opens the same.
        let mut sealer = StreamSealer::new(1, &KEY, &VID, &OBJECT);
        for piece in input.chunks(7_777) {
            sealer.write_all(piece).unwrap();
        }
        let pieced = sealer.finish().unwrap();
        assert_eq!(pieced.len(), sealed.len());
        assert_eq!(
            open_stream(only(1, KEY), &VID, &OBJECT, &pieced).unwrap(),
            input
        );
    }
    assert_eq!(seal_stream(1, &OBJECT, b"").len(), 59 + 16);
    assert_eq!(
        seal_stream(1, &OBJECT, &[0; 200_001]).len(),
        59 + 3 * 65_552 + 3_393 + 16
    );
}

#[test]
fn stream_header_layout_is_frozen() {
    assert_eq!(snapshot_object_id(42), [0, 0, 0, 0, 0, 0, 0, 42]);
    assert_eq!(
        snapshot_object_id(0x0102_0304_0506_0708),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );

    let sealed = fixed_stream(b"");
    assert_eq!(&sealed[0..4], b"MNY1");
    assert_eq!(&sealed[4..8], &3u32.to_be_bytes());
    assert_eq!(&sealed[8..40], &[0x33; 32]);
    assert_eq!(&sealed[40..59], &[0x55; 19]);
    assert_eq!(sealed.len(), 59 + 16);

    let key = hkdf32(&KEY, &[0x33; 32], SNAPSHOT_INFO);
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).unwrap();
    let aad = stream_aad(3, &42u64.to_be_bytes());
    let plain = cipher
        .decrypt(
            (&stream_nonce([0x55; 19], 0, true)).into(),
            Payload {
                msg: &sealed[59..],
                aad: &aad,
            },
        )
        .unwrap();
    assert!(plain.is_empty());

    // The whole stream, computed in Python the same way as the op envelope vector.
    assert_eq!(
        hex::encode(&sealed),
        "4d4e593100000003333333333333333333333333333333333333333333333333333333333333333355\
         5555555555555555555555555555555555556b860b465dc022df32e2a67eb1e3c66b"
    );
}

#[test]
fn stream_chunk_nonces_count_big_endian_and_flag_only_the_last_chunk() {
    let mut input = vec![0xab; CHUNK];
    input.extend_from_slice(b"tail");
    let sealed = fixed_stream(&input);
    assert_eq!(sealed.len(), 59 + SEALED_CHUNK + 4 + TAG);

    let key = hkdf32(&KEY, &[0x33; 32], SNAPSHOT_INFO);
    let cipher = XChaCha20Poly1305::new_from_slice(&key[..]).unwrap();
    let aad = stream_aad(3, &OBJECT);
    let first = &sealed[59..59 + SEALED_CHUNK];
    let decrypted = cipher
        .decrypt(
            (&stream_nonce([0x55; 19], 0, false)).into(),
            Payload {
                msg: first,
                aad: &aad,
            },
        )
        .unwrap();
    assert_eq!(decrypted, vec![0xab; CHUNK]);
    // Python vector: nonce = prefix | 00000001 | 01.
    assert_eq!(
        hex::encode(&sealed[59 + SEALED_CHUNK..]),
        "14c79bb822ca352a92430db96ed92b5ee2889734"
    );
}

#[test]
fn two_streams_never_share_a_key() {
    let input = incompressible(CHUNK + 10);
    let a = seal_stream(0, &OBJECT, &input);
    let b = seal_stream(0, &OBJECT, &input);
    let (salt_a, salt_b) = (&a[8..40], &b[8..40]);
    assert_ne!(salt_a, salt_b, "fresh salts");
    assert_ne!(a[40..59], b[40..59], "fresh nonce prefixes");
    assert_ne!(
        *hkdf32(&KEY, salt_a, SNAPSHOT_INFO),
        *hkdf32(&KEY, salt_b, SNAPSHOT_INFO)
    );
    assert_ne!(a[59..], b[59..]);
    assert_eq!(open_stream(only(0, KEY), &VID, &OBJECT, &a).unwrap(), input);
    assert_eq!(open_stream(only(0, KEY), &VID, &OBJECT, &b).unwrap(), input);

    // Computed with Python's `cryptography` HKDF, independent of the hkdf crate.
    assert_eq!(
        hex::encode(*hkdf32(&KEY, &[0x34; 32], SNAPSHOT_INFO)),
        "ae037ce329d9c0b7aeb9ad37db5dcae61f3035a4b42dee2f9327411c8f805368"
    );
}

#[test]
fn truncated_stream_fails() {
    let sealed = seal_stream(0, &OBJECT, &incompressible(200_001));
    let open = |bytes: &[u8]| open_stream(only(0, KEY), &VID, &OBJECT, bytes);

    // Dropping the last chunk leaves only full chunks: no last chunk.
    let without_last = &sealed[..HEADER_LEN + 3 * SEALED_CHUNK];
    assert_eq!(open(without_last), Err(OpenError::Malformed));
    // Dropping the last two leaves a full chunk that was never flagged last.
    assert_eq!(
        open(&sealed[..HEADER_LEN + 2 * SEALED_CHUNK]),
        Err(OpenError::Malformed)
    );
    // Cut inside the last chunk, or inside a full one so a fragment poses as the last chunk.
    assert_eq!(
        open(&sealed[..sealed.len() - 1]),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open(&sealed[..HEADER_LEN + 2 * SEALED_CHUNK + 100]),
        Err(OpenError::Authentication)
    );
    // Shorter than a header plus one tag.
    assert_eq!(
        open(&sealed[..HEADER_LEN + TAG - 1]),
        Err(OpenError::Malformed)
    );
    assert_eq!(open(&sealed[..HEADER_LEN]), Err(OpenError::Malformed));
    assert_eq!(open(b""), Err(OpenError::Malformed));
    // A fragment shorter than a tag after the full chunks.
    assert_eq!(
        open(&sealed[..HEADER_LEN + SEALED_CHUNK + TAG - 1]),
        Err(OpenError::Malformed)
    );
}

#[test]
fn reordered_chunks_fail() {
    let input = incompressible(2 * CHUNK + 100);
    let sealed = seal_stream(0, &OBJECT, &input);
    assert_eq!(sealed.len(), HEADER_LEN + 2 * SEALED_CHUNK + 100 + TAG);
    let (c0, c1) = (HEADER_LEN, HEADER_LEN + SEALED_CHUNK);

    let mut swapped = sealed[..HEADER_LEN].to_vec();
    swapped.extend_from_slice(&sealed[c1..c1 + SEALED_CHUNK]);
    swapped.extend_from_slice(&sealed[c0..c0 + SEALED_CHUNK]);
    swapped.extend_from_slice(&sealed[c1 + SEALED_CHUNK..]);
    assert_eq!(swapped.len(), sealed.len());
    assert_eq!(
        open_stream(only(0, KEY), &VID, &OBJECT, &swapped),
        Err(OpenError::Authentication)
    );

    // A chunk repeated in place of its neighbour fails as well.
    let mut repeated = sealed.clone();
    repeated.copy_within(c0..c0 + SEALED_CHUNK, c1);
    assert_eq!(
        open_stream(only(0, KEY), &VID, &OBJECT, &repeated),
        Err(OpenError::Authentication)
    );
}

#[test]
fn appended_bytes_fail() {
    let input = incompressible(CHUNK + 10);
    let sealed = seal_stream(0, &OBJECT, &input);
    let open = |bytes: &[u8]| open_stream(only(0, KEY), &VID, &OBJECT, bytes);
    assert_eq!(open(&sealed).unwrap(), input);

    for extra in [1usize, TAG, SEALED_CHUNK] {
        let mut longer = sealed.clone();
        longer.resize(sealed.len() + extra, 0);
        assert_eq!(
            open(&longer),
            Err(OpenError::Authentication),
            "{extra} zero bytes"
        );
    }

    // A real last chunk sealed with the same key and prefix, placed after this stream's last
    // chunk: the old last chunk now sits where a full chunk belongs.
    let mut fixed = fixed_stream(&input);
    let mut longer_input = input.clone();
    longer_input.extend_from_slice(&incompressible(CHUNK));
    let longer = fixed_stream(&longer_input);
    fixed.extend_from_slice(&longer[HEADER_LEN + 2 * SEALED_CHUNK..]);
    assert_eq!(
        open_stream(only(3, KEY), &VID, &OBJECT, &fixed),
        Err(OpenError::Authentication)
    );
}

#[test]
fn a_stream_opened_under_another_object_id_fails() {
    let sealed = seal_stream(0, &snapshot_object_id(42), b"profile");
    assert_eq!(
        open_stream(only(0, KEY), &VID, &snapshot_object_id(43), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open_stream(only(0, KEY), &[0x23; 16], &snapshot_object_id(42), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open_stream(only(0, [0x12; 32]), &VID, &snapshot_object_id(42), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open_stream(only(1, KEY), &VID, &snapshot_object_id(42), &sealed),
        Err(OpenError::UnknownEpoch(0))
    );
    assert_eq!(
        open_stream(only(0, KEY), &VID, &snapshot_object_id(42), &sealed).unwrap(),
        b"profile".to_vec()
    );
}

#[test]
fn every_flipped_header_byte_fails() {
    let sealed = fixed_stream(b"a short profile");
    assert_eq!(sealed.len(), HEADER_LEN + 15 + TAG);
    for i in 0..sealed.len() {
        let mut tampered = sealed.clone();
        tampered[i] ^= 0x01;
        let any_epoch = |_| Some(Zeroizing::new(KEY));
        let expected = match i {
            0..4 => OpenError::Malformed,
            4..8 => {
                let epoch = u32::from_be_bytes(tampered[4..8].try_into().unwrap());
                assert_eq!(
                    open_stream(any_epoch, &VID, &OBJECT, &tampered),
                    Err(OpenError::Authentication),
                    "byte {i} under the same key"
                );
                OpenError::UnknownEpoch(epoch)
            }
            _ => OpenError::Authentication,
        };
        assert_eq!(
            open_stream(only(3, KEY), &VID, &OBJECT, &tampered),
            Err(expected),
            "byte {i}"
        );
    }
}
