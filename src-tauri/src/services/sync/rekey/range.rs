//! The digest of a range of the server's log that a key change's signed head binds (version 2
//! of `head::HeadRecord`): every op above `base` up to and including `head`, as the remover
//! read it, noted by position and envelope hash.

use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};

const RANGE_DOMAIN: &[u8] = b"moony rekey range v1";

/// One noted op of the range: where it stands and the envelope hash over its whole sealed bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeNote {
    pub pos: u64,
    pub hash: [u8; 32],
}

/// `SHA-256("moony rekey range v1" ‖ vault_id ‖ to_epoch (u32) ‖ base (u64) ‖ head (u64) ‖
/// count (u32) ‖ for each note in ascending position: position (u64) ‖ hash (32))`, integers
/// big-endian. Refuses notes that are not strictly ascending or lie outside `(base, head]`.
pub fn range_digest(
    vault_id: &[u8; 16],
    to_epoch: u32,
    base: u64,
    head: u64,
    notes: &[RangeNote],
) -> Result<[u8; 32]> {
    let bad = || AppError::Internal("sync: the range of a key change is not valid".into());
    let count = u32::try_from(notes.len()).map_err(|_| bad())?;
    let mut previous = base;
    for note in notes {
        if note.pos <= previous || note.pos > head {
            return Err(bad());
        }
        previous = note.pos;
    }
    let mut hasher = Sha256::new();
    hasher.update(RANGE_DOMAIN);
    hasher.update(vault_id);
    hasher.update(to_epoch.to_be_bytes());
    hasher.update(base.to_be_bytes());
    hasher.update(head.to_be_bytes());
    hasher.update(count.to_be_bytes());
    for note in notes {
        hasher.update(note.pos.to_be_bytes());
        hasher.update(note.hash);
    }
    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VAULT: [u8; 16] = [0x22; 16];

    fn notes() -> Vec<RangeNote> {
        vec![
            RangeNote {
                pos: 11,
                hash: [0xaa; 32],
            },
            RangeNote {
                pos: 13,
                hash: [0xbb; 32],
            },
        ]
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_range_digest_matches_the_frozen_vector() {
        let two = range_digest(&VAULT, 4, 10, 13, &notes()).expect("digest");
        let empty = range_digest(&VAULT, 4, 13, 13, &[]).expect("digest");
        assert_eq!(
            hex(&two),
            "0c4975dbc30e2f9866ff89c16bdd9d42330faf65f2e5e009a7b2ddd98b08e870"
        );
        assert_eq!(
            hex(&empty),
            "eb3fb1176f17d5054b36bc1a56ec7cc143283c12f9295db531502e5fa38e8a11"
        );
    }

    #[test]
    fn a_digest_changes_with_every_field_and_with_the_order_of_notes_refused() {
        let base = range_digest(&VAULT, 4, 10, 13, &notes()).expect("digest");
        let mut other = notes();
        other[1].hash[0] ^= 1;
        let mut moved = notes();
        moved[0].pos = 12;
        let variants = [
            range_digest(&[0x23; 16], 4, 10, 13, &notes()),
            range_digest(&VAULT, 5, 10, 13, &notes()),
            range_digest(&VAULT, 4, 9, 13, &notes()),
            range_digest(&VAULT, 4, 10, 14, &notes()),
            range_digest(&VAULT, 4, 10, 13, &notes()[..1]),
            range_digest(&VAULT, 4, 10, 13, &other),
            range_digest(&VAULT, 4, 10, 13, &moved),
        ];
        for (i, v) in variants.iter().enumerate() {
            assert_ne!(v.as_ref().expect("digest"), &base, "variant {i}");
        }
        let swapped = vec![notes()[1], notes()[0]];
        assert!(range_digest(&VAULT, 4, 10, 13, &swapped).is_err());
        let twice = vec![notes()[0], notes()[0]];
        assert!(range_digest(&VAULT, 4, 10, 13, &twice).is_err());
        assert!(
            range_digest(&VAULT, 4, 11, 13, &notes()).is_err(),
            "at the base"
        );
        assert!(
            range_digest(&VAULT, 4, 10, 12, &notes()).is_err(),
            "above the head"
        );
    }
}
