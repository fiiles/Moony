use std::collections::BTreeMap;
use std::io::Write;

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::blob::*;
use super::stream::{
    chunk_nonce, open_stream, open_stream_with, snapshot_object_id, StreamSealer, CHUNK, HEADER_LEN,
};
use super::{aead_seal, hkdf32, OpenError, BLOB_INFO};
use crate::services::sync::keys::VaultKeys;

const KEY: [u8; 32] = [0x11; 32];
const VID: [u8; 16] = [0x22; 16];
const SALT: [u8; 32] = [0x33; 32];
const PREFIX: [u8; 19] = [0x55; 19];
const TAG: usize = 16;
const SEALED_CHUNK: usize = CHUNK + TAG;

/// 00000000-0000-4000-8000-000000000001
fn blob_id() -> Uuid {
    Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0001)
}

fn other_id() -> Uuid {
    Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_0002)
}

fn keys(epoch: u32, key: [u8; 32]) -> VaultKeys {
    VaultKeys::single(epoch, Zeroizing::new(key))
}

fn only(epoch: u32, key: [u8; 32]) -> impl Fn(u32) -> Option<Zeroizing<[u8; 32]>> {
    move |e| (e == epoch).then(|| Zeroizing::new(key))
}

/// `len` bytes of a repeating 0..251 pattern, so no chunk equals another.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// Seals `plain` one chunk at a time with `sealer`: the header, the full chunks, then the last.
fn seal_with(mut sealer: BlobSealer, plain: &[u8]) -> Vec<u8> {
    let mut out = sealer.header().to_vec();
    let full = plain.len() / CHUNK;
    for i in 0..full {
        out.extend(
            sealer
                .seal_chunk(&plain[i * CHUNK..(i + 1) * CHUNK], false)
                .unwrap(),
        );
    }
    out.extend(sealer.seal_chunk(&plain[full * CHUNK..], true).unwrap());
    out
}

fn seal_fixed(epoch: u32, blob: &Uuid, plain: &[u8]) -> Vec<u8> {
    seal_with(
        BlobSealer::new_with(epoch, &KEY, &VID, blob, SALT, PREFIX),
        plain,
    )
}

fn seal(epoch: u32, blob: &Uuid, plain: &[u8]) -> Vec<u8> {
    seal_with(BlobSealer::new(epoch, &KEY, &VID, blob), plain)
}

/// Opens `sealed` announced as `announced` bytes, pushed `piece` bytes at a time.
fn open_announced(
    keys: VaultKeys,
    vault_id: [u8; 16],
    blob: Uuid,
    sealed: &[u8],
    announced: u64,
    piece: usize,
) -> Result<Vec<u8>, OpenError> {
    let mut opener = BlobOpener::new(keys, vault_id, blob, announced)?;
    let mut plain = Vec::new();
    for p in sealed.chunks(piece) {
        for chunk in opener.push(p)? {
            plain.extend_from_slice(&chunk);
        }
    }
    opener.finish()?;
    Ok(plain)
}

fn open(
    keys: VaultKeys,
    vault_id: [u8; 16],
    blob: Uuid,
    sealed: &[u8],
) -> Result<Vec<u8>, OpenError> {
    open_announced(
        keys,
        vault_id,
        blob,
        sealed,
        sealed.len() as u64,
        sealed.len().max(1),
    )
}

#[test]
fn sealed_and_plain_lengths_invert_each_other() {
    for n in [
        0,
        1,
        CHUNK as u64 - 1,
        CHUNK as u64,
        CHUNK as u64 + 1,
        3 * CHUNK as u64,
        MAX_BLOB_PLAIN,
    ] {
        assert_eq!(plain_len(sealed_len(n)), Some(n));
    }
    assert_eq!(sealed_len(0), 59 + 16);
    assert_eq!(plain_len(74), None);
    assert_eq!(plain_len(59 + 65_536 + 16 + 15), None); // a last chunk shorter than its tag
}

#[test]
fn a_blob_opens_piece_by_piece_whatever_the_piece_size() {
    let plain = pattern(3 * CHUNK + 17);
    let sealed = seal(0, &blob_id(), &plain);
    assert_eq!(sealed.len() as u64, sealed_len(plain.len() as u64));
    for piece in [1, 7, 4096, CHUNK + 16 + 5, sealed.len()] {
        let mut opener =
            BlobOpener::new(keys(0, KEY), VID, blob_id(), sealed.len() as u64).unwrap();
        assert_eq!(opener.plain_len(), plain.len() as u64);
        let mut chunks = Vec::new();
        for p in sealed.chunks(piece) {
            chunks.extend(opener.push(p).unwrap());
        }
        opener.finish().unwrap();
        // Only whole, authenticated chunks come out.
        let lengths: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
        assert_eq!(lengths, [CHUNK, CHUNK, CHUNK, 17], "{piece}-byte pieces");
        let opened: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(opened, plain, "{piece}-byte pieces");
    }
    for len in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1] {
        let plain = pattern(len);
        let sealed = seal(0, &blob_id(), &plain);
        assert_eq!(sealed.len() as u64, sealed_len(len as u64), "{len} bytes");
        assert_eq!(
            open(keys(0, KEY), VID, blob_id(), &sealed).unwrap(),
            plain,
            "{len} bytes"
        );
    }
}

#[test]
fn the_blob_stream_is_the_snapshot_stream_with_its_own_info() {
    let plain = pattern(2 * CHUNK + 5);
    let object = blob_object_id(&blob_id());
    assert_eq!(object, *blob_id().as_bytes());

    let blob = seal_fixed(3, &blob_id(), &plain);
    let mut sealer = StreamSealer::new_with_info(BLOB_INFO, 3, &KEY, &VID, &object, SALT, PREFIX);
    sealer.write_all(&plain).unwrap();
    assert_eq!(blob, sealer.finish().unwrap());

    assert_eq!(
        open_stream_with(BLOB_INFO, only(3, KEY), &VID, &object, &blob).unwrap(),
        plain
    );
    assert_eq!(
        open_stream(only(3, KEY), &VID, &object, &blob),
        Err(OpenError::Authentication)
    );
}

#[test]
fn snapshots_seal_and_open_as_before_blobs() {
    // Pinned before blobs existed, from the unchanged stream.rs and independently in Python
    // (`cryptography` HKDF, hand-written HChaCha20 over its ChaCha20Poly1305).
    let plain = pattern(3 * CHUNK + 17);
    let mut sealer = StreamSealer::new_with(3, &KEY, &VID, &snapshot_object_id(42), SALT, PREFIX);
    sealer.write_all(&plain).unwrap();
    let snapshot = sealer.finish().unwrap();
    assert_eq!(snapshot.len(), 196_748);
    assert_eq!(
        hex::encode(Sha256::digest(&snapshot)),
        "8b3624997bb157fa3ea64263a669e34bf30e78fdf41421c9582a1ae2f3048482"
    );
    assert_eq!(
        open_stream(only(3, KEY), &VID, &snapshot_object_id(42), &snapshot).unwrap(),
        plain
    );

    // The empty snapshot vector pinned in tests.rs, opened by today's code.
    let empty = hex::decode(
        "4d4e593100000003333333333333333333333333333333333333333333333333333333333333333355\
         5555555555555555555555555555555555556b860b465dc022df32e2a67eb1e3c66b",
    )
    .unwrap();
    assert_eq!(
        open_stream(only(3, KEY), &VID, &snapshot_object_id(42), &empty).unwrap(),
        Vec::<u8>::new()
    );
    // A snapshot never opens as a blob.
    assert_eq!(
        open(keys(3, KEY), VID, blob_id(), &empty),
        Err(OpenError::Authentication)
    );
}

#[test]
fn the_fixed_blob_vector_holds() {
    let blob = Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
    let sealer = BlobSealer::new_with(0, &[7; 32], &[9; 16], &blob, [1; 32], [2; 19]);
    let sealed = seal_with(sealer, b"moony blob");
    assert_eq!(sealed.len(), 59 + 10 + 16);
    // Computed once in Python (`cryptography` HKDF, hand-written HChaCha20 over its
    // ChaCha20Poly1305) and pinned; never change it.
    assert_eq!(
        hex::encode(Sha256::digest(&sealed)),
        "a950f32fd9b665951b4e014f229bf10ca73591d9b02ec11b45ac381d605e080a"
    );
    assert_eq!(
        open(keys(0, [7; 32]), [9; 16], blob, &sealed).unwrap(),
        b"moony blob".to_vec()
    );
}

#[test]
fn a_tampered_chunk_fails_authentication() {
    let plain = pattern(2 * CHUNK + 100);
    let mut sealed = seal(0, &blob_id(), &plain);
    let (c1, c2) = (HEADER_LEN + SEALED_CHUNK, HEADER_LEN + 2 * SEALED_CHUNK);
    sealed[c1 + 10] ^= 0x01;

    let mut opener = BlobOpener::new(keys(0, KEY), VID, blob_id(), sealed.len() as u64).unwrap();
    let first = opener.push(&sealed[..c1]).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(&first[0][..], &plain[..CHUNK]);
    assert_eq!(opener.push(&sealed[c1..c2]), Err(OpenError::Authentication));
    // Nothing after a failure: the opener stays failed.
    assert_eq!(opener.push(&sealed[c2..]), Err(OpenError::Malformed));
    assert_eq!(opener.finish(), Err(OpenError::Malformed));

    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &sealed),
        Err(OpenError::Authentication)
    );
}

#[test]
fn a_truncated_blob_never_finishes() {
    let plain = pattern(2 * CHUNK + 100);
    let sealed = seal(0, &blob_id(), &plain);
    let announced = sealed.len() as u64;
    let last = HEADER_LEN + 2 * SEALED_CHUNK;
    let cut = |bytes: &[u8]| {
        let mut opener = BlobOpener::new(keys(0, KEY), VID, blob_id(), announced).unwrap();
        let chunks = opener.push(bytes).unwrap();
        (chunks.len(), opener.finish())
    };

    // (a) The last sealed chunk dropped: the full chunks open, the stream never finishes.
    assert_eq!(cut(&sealed[..last]), (2, Err(OpenError::Malformed)));
    // (b) The last byte dropped.
    assert_eq!(
        cut(&sealed[..sealed.len() - 1]),
        (2, Err(OpenError::Malformed))
    );
    // Nothing at all.
    assert_eq!(cut(b""), (0, Err(OpenError::Malformed)));

    // Announced as the shorter length: without its last chunk the length has no room for one,
    // and a stream cut mid-chunk presents a fragment as the last chunk.
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &sealed[..last]),
        Err(OpenError::Malformed)
    );
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &sealed[..sealed.len() - 1]),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open(
            keys(0, KEY),
            VID,
            blob_id(),
            &sealed[..HEADER_LEN + SEALED_CHUNK + 100]
        ),
        Err(OpenError::Authentication)
    );
}

#[test]
fn a_whole_chunk_presented_as_last_fails() {
    let plain = pattern(CHUNK);
    let sealed = seal_fixed(0, &blob_id(), &plain);
    assert_eq!(sealed.len(), HEADER_LEN + SEALED_CHUNK + TAG);
    let full = &sealed[..HEADER_LEN + SEALED_CHUNK];

    // Without the empty last chunk, the shorter length leaves no room for a last chunk.
    assert_eq!(plain_len(full.len() as u64), None);
    assert_eq!(
        BlobOpener::new(keys(0, KEY), VID, blob_id(), full.len() as u64).err(),
        Some(OpenError::Malformed)
    );

    // The same empty final chunk sealed without the last flag: only the flag differs.
    let key = hkdf32(&KEY, &SALT, BLOB_INFO);
    let mut aad = b"MNY1".to_vec();
    aad.extend_from_slice(&0u32.to_be_bytes());
    aad.extend_from_slice(&VID);
    aad.extend_from_slice(blob_id().as_bytes());
    let flagged = aead_seal(&key, &chunk_nonce(&PREFIX, 1, true), &aad, b"").unwrap();
    assert_eq!(flagged, sealed[HEADER_LEN + SEALED_CHUNK..].to_vec());
    let unflagged = aead_seal(&key, &chunk_nonce(&PREFIX, 1, false), &aad, b"").unwrap();
    let mut forged = full.to_vec();
    forged.extend_from_slice(&unflagged);
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &forged),
        Err(OpenError::Authentication)
    );

    // The sealer never seals a whole chunk as the last.
    let mut sealer = BlobSealer::new(0, &KEY, &VID, &blob_id());
    assert!(sealer.seal_chunk(&plain, true).is_err());
}

#[test]
fn reordered_duplicated_or_swapped_chunks_fail() {
    let plain = pattern(2 * CHUNK + 100);
    let sealed = seal(0, &blob_id(), &plain);
    let (c0, c1) = (HEADER_LEN, HEADER_LEN + SEALED_CHUNK);

    let mut reordered = sealed[..c0].to_vec();
    reordered.extend_from_slice(&sealed[c1..c1 + SEALED_CHUNK]);
    reordered.extend_from_slice(&sealed[c0..c1]);
    reordered.extend_from_slice(&sealed[c1 + SEALED_CHUNK..]);
    assert_eq!(reordered.len(), sealed.len());
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &reordered),
        Err(OpenError::Authentication)
    );

    let mut duplicated = sealed.clone();
    duplicated.copy_within(c0..c1, c1);
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &duplicated),
        Err(OpenError::Authentication)
    );

    // A chunk, or the header, of another blob's stream under the same vault key.
    let other = seal(0, &other_id(), &plain);
    let mut swapped = sealed.clone();
    swapped[c1..c1 + SEALED_CHUNK].copy_from_slice(&other[c1..c1 + SEALED_CHUNK]);
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &swapped),
        Err(OpenError::Authentication)
    );
    let mut other_header = sealed.clone();
    other_header[..HEADER_LEN].copy_from_slice(&other[..HEADER_LEN]);
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &other_header),
        Err(OpenError::Authentication)
    );
}

#[test]
fn a_blob_opened_under_another_id_or_vault_fails() {
    let plain = pattern(CHUNK + 3);
    let sealed = seal(0, &blob_id(), &plain);
    assert_eq!(
        open(keys(0, KEY), VID, other_id(), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open(keys(0, KEY), [0x23; 16], blob_id(), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open(keys(0, [0x12; 32]), VID, blob_id(), &sealed),
        Err(OpenError::Authentication)
    );
    assert_eq!(open(keys(0, KEY), VID, blob_id(), &sealed).unwrap(), plain);
}

#[test]
fn an_unknown_epoch_is_named() {
    let sealer = BlobSealer::new(5, &KEY, &VID, &blob_id());
    assert_eq!(&sealer.header()[0..4], b"MNY1");
    assert_eq!(&sealer.header()[4..8], &5u32.to_be_bytes());
    let sealed = seal_with(sealer, b"a receipt");
    assert_eq!(
        open(keys(0, KEY), VID, blob_id(), &sealed),
        Err(OpenError::UnknownEpoch(5))
    );

    // Relabelled to another epoch that has the very same key: the epoch is bound in.
    let both = VaultKeys::from_parts(6, BTreeMap::from([(5, KEY), (6, KEY)])).unwrap();
    let mut relabelled = sealed.clone();
    relabelled[4..8].copy_from_slice(&6u32.to_be_bytes());
    assert_eq!(
        open(both.clone(), VID, blob_id(), &relabelled),
        Err(OpenError::Authentication)
    );
    assert_eq!(
        open(both, VID, blob_id(), &sealed).unwrap(),
        b"a receipt".to_vec()
    );

    let mut wrong_magic = sealed.clone();
    wrong_magic[..4].copy_from_slice(b"MNY2");
    assert_eq!(
        open(keys(5, KEY), VID, blob_id(), &wrong_magic),
        Err(OpenError::Malformed)
    );
}

#[test]
fn more_bytes_than_announced_are_refused() {
    let plain = pattern(CHUNK + 3);
    let sealed = seal(0, &blob_id(), &plain);
    let announced = sealed.len() as u64;

    let mut longer = sealed.clone();
    longer.push(0);
    assert_eq!(
        open_announced(
            keys(0, KEY),
            VID,
            blob_id(),
            &longer,
            announced,
            longer.len()
        ),
        Err(OpenError::Malformed)
    );

    // The extra byte after a complete stream, pushed on its own.
    let mut opener = BlobOpener::new(keys(0, KEY), VID, blob_id(), announced).unwrap();
    assert_eq!(opener.push(&sealed).unwrap().len(), 2);
    assert_eq!(opener.push(&[0]), Err(OpenError::Malformed));
    assert_eq!(opener.finish(), Err(OpenError::Malformed));
}

#[test]
fn the_sealer_refuses_chunks_out_of_shape() {
    let mut sealer = BlobSealer::new(0, &KEY, &VID, &blob_id());
    assert!(sealer.seal_chunk(&[0; CHUNK - 1], false).is_err());
    assert!(sealer.seal_chunk(&[0; CHUNK + 1], false).is_err());
    assert!(sealer.seal_chunk(&[0; CHUNK + 1], true).is_err());
    assert!(sealer.seal_chunk(&[0; CHUNK], false).is_ok());
    assert!(sealer.seal_chunk(b"end", true).is_ok());
    assert!(sealer.seal_chunk(b"", true).is_err());
    assert!(sealer.seal_chunk(&[0; CHUNK], false).is_err());
}

#[test]
fn the_blob_limit_is_the_document_limit() {
    assert_eq!(
        MAX_BLOB_PLAIN,
        crate::services::attachment_store::MAX_DOCUMENT_BYTES
    );
    // The sync server's per-blob limit.
    assert_eq!(MAX_BLOB_SEALED, 104_883_275);
    assert!(BlobOpener::new(keys(0, KEY), VID, blob_id(), MAX_BLOB_SEALED).is_ok());
    assert_eq!(
        BlobOpener::new(keys(0, KEY), VID, blob_id(), sealed_len(MAX_BLOB_PLAIN + 1)).err(),
        Some(OpenError::Malformed)
    );
}
