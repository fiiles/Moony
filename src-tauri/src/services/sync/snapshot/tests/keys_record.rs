//! The snapshot's `Keys` record (phase 5 §8.4): every key epoch the dumper holds with the relay
//! position at which it started, restored by replace and by merge, and refused where it contradicts
//! this device.

use super::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zeroize::Zeroizing;

const VAULT: &str = "AAECAwQFBgcICQoLDA0ODw";

fn key(n: u8) -> Zeroizing<[u8; 32]> {
    Zeroizing::new([n; 32])
}

/// Binds `conn`'s sync row to a vault with `epochs` (epoch, key byte); the newest is current.
fn bind(conn: &Connection, epochs: &[(u32, u8)]) {
    let json = epochs
        .iter()
        .map(|(epoch, n)| format!("\"{epoch}\":\"{}\"", URL_SAFE_NO_PAD.encode([*n; 32])))
        .collect::<Vec<_>>()
        .join(",");
    let current = epochs
        .iter()
        .map(|(epoch, _)| *epoch)
        .max()
        .expect("epochs");
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1, key_epoch = ?2, vault_keys = ?3 WHERE id = 1",
        rusqlite::params![VAULT, current, format!("{{{json}}}")],
    )
    .expect("bind");
}

fn epoch_rows(conn: &Connection) -> Vec<(i64, i64)> {
    let mut stmt = conn
        .prepare("SELECT epoch, rotation_head FROM sync_epochs ORDER BY epoch")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn held(conn: &Connection) -> Vec<(u32, u8)> {
    crate::services::sync::keys::vault_keys(conn)
        .expect("keys")
        .map(|keys| keys.epochs().map(|(epoch, key)| (epoch, key[0])).collect())
        .unwrap_or_default()
}

/// A dumper at epoch 2: epoch 1 began at relay position 50, epoch 2's position is unknown.
fn dumper() -> Connection {
    let a = device(A);
    bind(&a, &[(0, 0x10), (1, 0x11), (2, 0x12)]);
    state::record_epoch(&a, 1, 50, None, NOW).expect("epoch 1");
    a
}

fn keys_of(records: &[Record]) -> Vec<&Vec<EpochKey>> {
    records
        .iter()
        .filter_map(|r| match r {
            Record::Keys(keys) => Some(keys),
            _ => None,
        })
        .collect()
}

#[test]
fn a_dump_carries_one_keys_record_with_every_epoch_and_its_head() {
    let (bytes, _) = snapshot(&dumper(), 3);
    let all = records(&bytes);

    assert!(
        matches!(all[1], Record::Header { .. }) && matches!(all[2], Record::Keys(_)),
        "the Keys record follows the header"
    );
    let listed = keys_of(&all);
    assert_eq!(listed.len(), 1);
    let summary: Vec<(u32, u8, Option<u64>)> = listed[0]
        .iter()
        .map(|k| (k.epoch, k.key[0], k.head))
        .collect();
    assert_eq!(
        summary,
        vec![
            (0, 0x10, Some(0)), // epoch 0 starts at 0 whether recorded or not
            (1, 0x11, Some(50)),
            (2, 0x12, None), // no recorded position: travels without one
        ]
    );
}

#[test]
fn a_profile_not_bound_to_a_vault_dumps_no_keys_record() {
    let (bytes, _) = snapshot(&device(A), 1);
    assert!(keys_of(&records(&bytes)).is_empty());
}

/// The bytes of the record are a contract: fixed keys, fixed shape.
#[test]
fn the_keys_record_has_a_fixed_json_shape() {
    let record = Record::Keys(vec![
        EpochKey {
            epoch: 0,
            key: key(0x11),
            head: Some(0),
        },
        EpochKey {
            epoch: 3,
            key: key(0x22),
            head: None,
        },
    ]);
    let mut out = Vec::new();
    write_record(&mut out, &encode(&record, &mut DevTable::new())).expect("write");

    let len = u32::from_be_bytes([out[0], out[1], out[2], out[3]]) as usize;
    assert_eq!(len, out.len() - 4);
    assert_eq!(
        std::str::from_utf8(&out[4..]).expect("utf8"),
        concat!(
            r#"{"keys":[{"epoch":0,"head":0,"key":"ERERERERERERERERERERERERERERERERERERERERERE"},"#,
            r#"{"epoch":3,"key":"IiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiI"}],"r":"keys"}"#,
        )
    );
    // And it reads back.
    assert_eq!(
        read_record(&mut &out[..], &[]).expect("read"),
        Some(record.clone())
    );
    // The key never shows in Debug.
    assert!(!format!("{record:?}").contains("ERERER"));
    assert!(!format!("{record:?}").contains("17"), "{record:?}");
}

#[test]
fn a_replace_adds_the_older_keys_and_their_heads() {
    let (bytes, _) = snapshot(&dumper(), 3);
    // A joiner that holds only the current key (a Recovery Kit's).
    let b = migrated();
    state::ensure_row(&b, B).expect("row");
    bind(&b, &[(2, 0x12)]);

    restore_replace(&b, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(held(&b), vec![(0, 0x10), (1, 0x11), (2, 0x12)]);
    assert_eq!(epoch_rows(&b), vec![(0, 0), (1, 50)]);
}

#[test]
fn a_merge_adds_only_what_is_missing() {
    let (bytes, _) = snapshot(&dumper(), 3);
    let b = device(B);
    bind(&b, &[(0, 0x10), (2, 0x12)]);
    state::record_epoch(&b, 0, 0, None, 1).expect("epoch 0");

    restore_merge(&b, &mut &bytes[..], NOW).expect("merge");

    assert_eq!(held(&b), vec![(0, 0x10), (1, 0x11), (2, 0x12)]);
    assert_eq!(epoch_rows(&b), vec![(0, 0), (1, 50)]);

    // Again: nothing changes.
    restore_merge(&b, &mut &bytes[..], NOW).expect("merge again");
    assert_eq!(held(&b), vec![(0, 0x10), (1, 0x11), (2, 0x12)]);
    assert_eq!(epoch_rows(&b), vec![(0, 0), (1, 50)]);
}

fn refused_with(b: &Connection, bytes: &[u8], merge: bool) {
    let before_keys = held(b);
    let before_epochs = epoch_rows(b);
    let before_rows = table_dump(b);
    let error = if merge {
        restore_merge(b, &mut &bytes[..], NOW)
    } else {
        restore_replace(b, &mut &bytes[..], NOW)
    }
    .expect_err("refused");
    assert!(
        matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotInvalid"),
        "{error:?}"
    );
    assert_eq!(held(b), before_keys, "no key was written");
    assert_eq!(epoch_rows(b), before_epochs, "no epoch was written");
    assert_eq!(table_dump(b), before_rows, "no row was written");
}

#[test]
fn a_key_that_contradicts_a_held_one_refuses_the_snapshot() {
    let (bytes, _) = snapshot(&dumper(), 3);
    for merge in [false, true] {
        let b = device(B);
        // Epoch 1 is another key here.
        bind(&b, &[(1, 0x99), (2, 0x12)]);
        refused_with(&b, &bytes, merge);
        // The current key is another one.
        let b = device(B);
        bind(&b, &[(2, 0x99)]);
        refused_with(&b, &bytes, merge);
    }
}

#[test]
fn a_head_that_contradicts_a_recorded_one_refuses_the_snapshot() {
    let (bytes, _) = snapshot(&dumper(), 3);
    let b = device(B);
    bind(&b, &[(1, 0x11), (2, 0x12)]);
    state::record_epoch(&b, 1, 49, None, 1).expect("epoch 1");
    refused_with(&b, &bytes, true);
}

#[test]
fn an_epoch_above_this_devices_current_one_refuses_the_snapshot() {
    let (bytes, _) = snapshot(&dumper(), 3);
    let b = device(B);
    bind(&b, &[(0, 0x10), (1, 0x11)]); // current 1; the record names epoch 2
    refused_with(&b, &bytes, true);
    // A device with no keys at all cannot judge a Keys record.
    let b = device(B);
    refused_with(&b, &bytes, true);
}

#[test]
fn a_snapshot_without_a_keys_record_restores_as_before() {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    let (bytes, _) = snapshot(&a, 1);
    assert!(keys_of(&records(&bytes)).is_empty());
    let b = joined(&bytes);
    assert_eq!(
        count(&b, "SELECT COUNT(*) FROM institutions WHERE id = 'i1'"),
        1
    );
}

#[test]
fn a_keys_record_out_of_place_or_malformed_is_refused() {
    let (bytes, _) = snapshot(&dumper(), 3);
    let all = records(&bytes);
    let keys = keys_of(&all)[0].clone();
    let invalid = |records: &[Record]| {
        let stream = stream_of(records);
        let error = restore_merge(&device(B), &mut &stream[..], NOW).expect_err("refused");
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotInvalid"),
            "{error:?}"
        );
    };
    // Twice.
    let mut twice = all.clone();
    twice.insert(3, Record::Keys(keys.clone()));
    invalid(&twice);
    // After the rows.
    let mut late = all.clone();
    late.retain(|r| !matches!(r, Record::Keys(_)));
    let end = late.len() - 1;
    late.insert(end, Record::Keys(keys.clone()));
    invalid(&late);
    // Epochs not ascending, or repeated.
    for broken in [
        vec![keys[1].clone(), keys[0].clone()],
        vec![keys[0].clone(), keys[0].clone()],
    ] {
        let mut stream = all.clone();
        stream[2] = Record::Keys(broken);
        invalid(&stream);
    }
    // An empty record names nothing.
    let mut empty = all.clone();
    empty[2] = Record::Keys(Vec::new());
    invalid(&empty);
}

#[test]
fn a_key_of_the_wrong_length_or_an_unknown_field_is_refused() {
    for text in [
        // 31 bytes.
        r#"{"keys":[{"epoch":0,"key":"ERERERERERERERERERERERERERERERERERERERERERE"}],"r":"keys"}"#
            .replace("ERERERERERERERERERERERERERERERERERERERERERE", "ERERERERERERERERERERERERERERERERERERERERE"),
        r#"{"keys":[{"epoch":0,"key":"ERERERERERERERERERERERERERERERERERERERERERE","extra":1}],"r":"keys"}"#.to_string(),
        r#"{"keys":[{"epoch":0,"key":"not base64!"}],"r":"keys"}"#.to_string(),
        r#"{"keys":[{"epoch":-1,"key":"ERERERERERERERERERERERERERERERERERERERERERE"}],"r":"keys"}"#.to_string(),
    ] {
        let mut out = (text.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(text.as_bytes());
        assert!(read_record(&mut &out[..], &[]).is_err(), "{text}");
    }
}

fn stream_with_keys(listed: Vec<EpochKey>) -> Vec<u8> {
    let (bytes, _) = snapshot(&dumper(), 3);
    let mut all = records(&bytes);
    all[2] = Record::Keys(listed);
    stream_of(&all)
}

fn entry(epoch: u32, n: u8, head: Option<u64>) -> EpochKey {
    EpochKey {
        epoch,
        key: key(n),
        head,
    }
}

#[test]
fn heads_that_decrease_across_epochs_are_refused() {
    for listed in [
        vec![
            entry(0, 0x10, Some(0)),
            entry(1, 0x11, Some(50)),
            entry(2, 0x12, Some(49)),
        ],
        // An epoch without a head does not hide the order of the others.
        vec![
            entry(0, 0x10, Some(0)),
            entry(1, 0x11, Some(50)),
            entry(2, 0x12, None),
            entry(3, 0x13, Some(10)),
        ],
    ] {
        let bytes = stream_with_keys(listed);
        let b = device(B);
        bind(&b, &[(3, 0x13)]);
        refused_with(&b, &bytes, true);
        refused_with(&b, &bytes, false);
    }
    // Equal heads are allowed (two key changes without an op between them).
    let bytes = stream_with_keys(vec![
        entry(0, 0x10, Some(0)),
        entry(1, 0x11, Some(50)),
        entry(2, 0x12, Some(50)),
    ]);
    let b = device(B);
    bind(&b, &[(2, 0x12)]);
    restore_merge(&b, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(epoch_rows(&b), vec![(0, 0), (1, 50), (2, 50)]);
}

#[test]
fn a_listed_head_below_a_recorded_earlier_one_is_refused() {
    // This device recorded epoch 2 at 40; the snapshot puts epoch 1 at 50.
    let bytes = stream_with_keys(vec![entry(0, 0x10, Some(0)), entry(1, 0x11, Some(50))]);
    let b = device(B);
    bind(&b, &[(2, 0x12)]);
    state::record_epoch(&b, 2, 40, None, 1).expect("epoch 2");
    refused_with(&b, &bytes, true);
}

#[test]
fn epoch_zero_starts_at_zero_or_has_no_head() {
    let bytes = stream_with_keys(vec![entry(0, 0x10, Some(5)), entry(1, 0x11, Some(50))]);
    let b = device(B);
    bind(&b, &[(1, 0x11)]);
    refused_with(&b, &bytes, true);
    // No head at all is fine.
    let bytes = stream_with_keys(vec![entry(0, 0x10, None), entry(1, 0x11, Some(50))]);
    let b = device(B);
    bind(&b, &[(1, 0x11)]);
    restore_merge(&b, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(epoch_rows(&b), vec![(0, 0), (1, 50)]);
}

#[test]
fn the_dumped_keys_record_is_written_without_a_plain_copy_of_the_keys() {
    // The dump and the fixed-shape record agree byte for byte.
    let (bytes, _) = snapshot(&dumper(), 3);
    let all = records(&bytes);
    let listed = keys_of(&all)[0].clone();
    let mut expected = Vec::new();
    write_record(
        &mut expected,
        &encode(&Record::Keys(listed.clone()), &mut DevTable::new()),
    )
    .expect("write");
    let mut direct = Vec::new();
    write_keys_record(&mut direct, &listed).expect("write");
    assert_eq!(direct, expected);
}
