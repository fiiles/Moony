//! The queue is a function of the rows and the store: reconciling adds, drops and moves entries,
//! the lanes take them in their order, failures back off by kind, and the content state of a row
//! follows the queue and the store.

use super::*;
use crate::models::attachments::{AttachmentContent, AttachmentKind};
use crate::services::sync::blobs::canonical_id;
use crate::services::sync::blobs::content::{row_syncs, ContentReader};
use crate::services::sync::blobs::queue::{self, *};
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::hlc::Clock;
use crate::services::sync::repair::{self, Touched};

/// Stores `bytes` under the given ID, as a finished download would.
fn arrive(conn: &Connection, id: &str, bytes: &[u8]) {
    attachment_store::begin_incoming(conn, id, bytes.len() as u64).expect("begin");
    attachment_store::write_incoming(conn, id, 0, bytes).expect("write");
    attachment_store::finish_incoming(conn, id, "sha").expect("finish");
}

fn reconciled(queued_up: usize, queued_down: usize, dropped: usize, changed: usize) -> Reconciled {
    Reconciled {
        queued_up,
        queued_down,
        dropped,
        changed,
    }
}

fn next_try_at(conn: &Connection, blob: &str) -> i64 {
    conn.query_row(
        "SELECT next_try_at FROM sync_blobs WHERE blob_id = ?1",
        [blob],
        |r| r.get(0),
    )
    .expect("next_try_at")
}

/// Fetches everything offered, in the order offered, as the download lane would.
fn downloads(conn: &Connection, eager_full: bool) -> Vec<String> {
    let mut order = Vec::new();
    while let Some(t) = next_download(conn, NOW, eager_full).expect("next") {
        arrive(conn, &t.blob_id, t.blob_id.as_bytes());
        note_done(conn, &t.blob_id).expect("done");
        order.push(t.blob_id);
    }
    order
}

/// Deletes in merge mode, stamping the cascade with a clock as the merge does.
fn merge_delete(conn: &Connection, sql: &str) {
    capture::with_mode(conn, Mode::Merge, || {
        capture::set_cascade_clock(
            conn,
            Clock {
                hlc: 1 << 40,
                device: DeviceId([9; 16]),
            },
        )?;
        exec(conn, sql);
        Ok(())
    })
    .expect("merged delete");
}

#[test]
fn reconcile_queues_held_blobs_up_and_missing_ones_down() {
    let conn = profile();
    let here = held(&conn, b"%PDF-1.7 d1");
    document(&conn, "d1", &here, 100);
    document(&conn, "d2", &elsewhere(2), 200);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 300);

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(1, 3, 0, 0)
    );

    let mut expected = vec![
        format!("{here} up 0 0 100 {NOW} 0 -"),
        format!("{} down 0 0 200 {NOW} 0 -", elsewhere(2)),
        format!("{} down 0 0 300 {NOW} 0 -", elsewhere(3)),
        format!("{} down 1 0 300 {NOW} 0 -", elsewhere(4)),
    ];
    expected.sort();
    assert_eq!(queue_rows(&conn), expected);
    assert_eq!(
        queue::reconcile(&conn, NOW + 1).expect("again"),
        Reconciled::default(),
        "a second run finds nothing to do"
    );
    assert_eq!(queue_rows(&conn), expected);
}

#[test]
fn reconcile_drops_entries_no_row_references() {
    let conn = profile();
    let here = held(&conn, b"one");
    document(&conn, "d1", &here, 100);
    exec(
        &conn,
        &format!(
            "INSERT INTO sync_blobs (blob_id, state, queued_at) VALUES \
               ('{}', 'up', 1), ('{}', 'down', 1), ('{}', 'done', 1);",
            elsewhere(7),
            elsewhere(8),
            elsewhere(9)
        ),
    );

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(1, 0, 3, 0)
    );
    assert_eq!(queue_rows(&conn).len(), 1);

    // A row deleted here.
    exec(&conn, "DELETE FROM insurance_documents WHERE id = 'd1'");
    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(0, 0, 1, 0)
    );
    assert!(queue_rows(&conn).is_empty());

    // A policy deleted with its documents, and a property with its photos.
    document(&conn, "d2", &elsewhere(2), 100);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!(queue_rows(&conn).len(), 3);
    exec(
        &conn,
        "DELETE FROM insurance_policies WHERE id = 'p1'; DELETE FROM real_estate WHERE id = 'r1';",
    );
    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(0, 0, 3, 0)
    );
    assert!(queue_rows(&conn).is_empty());
}

/// Review requirement 1 (ADR 0030): a blob held here and named by a row waits for the server's
/// confirmation unless the queue knows it is done; a copy found again is confirmed again, since
/// losing it forgot whether the server ever had it (a `HEAD` answers that without a body).
#[test]
fn a_lost_local_copy_goes_back_to_down_and_a_found_one_waits_for_confirmation() {
    let conn = profile();
    let done = held(&conn, b"confirmed");
    let waiting = held(&conn, b"waiting");
    document(&conn, "d1", &done, 100);
    document(&conn, "d2", &waiting, 200);
    queue::reconcile(&conn, NOW).expect("reconcile");
    note_done(&conn, &done).expect("done");
    note_failed(&conn, &waiting, TransferError::Network, NOW).expect("failed");

    attachment_store::delete(&conn, &done).expect("lose");
    attachment_store::delete(&conn, &waiting).expect("lose");
    assert_eq!(
        queue::reconcile(&conn, NOW + 10).expect("reconcile"),
        reconciled(0, 0, 0, 2)
    );
    assert_eq!(state_of(&conn, &done).as_deref(), Some("down"));
    assert_eq!(state_of(&conn, &waiting).as_deref(), Some("down"));
    assert_eq!(
        next_try_at(&conn, &waiting),
        0,
        "the upload's backoff is forgotten"
    );

    arrive(&conn, &done, b"confirmed");
    assert_eq!(
        queue::reconcile(&conn, NOW + 20).expect("reconcile"),
        reconciled(0, 0, 0, 1)
    );
    assert_eq!(state_of(&conn, &done).as_deref(), Some("up"));
    assert_eq!(
        next_upload(&conn, NOW + 20, None)
            .expect("next")
            .map(|t| t.blob_id),
        Some(done.clone())
    );

    // Content that arrived through the download lane is done as soon as the lane says so.
    arrive(&conn, &waiting, b"waiting");
    note_done(&conn, &waiting).expect("done");
    assert_eq!(
        queue::reconcile(&conn, NOW + 30).expect("reconcile"),
        Reconciled::default()
    );
    assert_eq!(state_of(&conn, &waiting).as_deref(), Some("done"));
}

#[test]
fn a_row_without_a_blob_queues_nothing() {
    let conn = profile();
    exec(
        &conn,
        "INSERT INTO insurance_documents (id, insurance_id, name, file_path) \
           VALUES ('old', 'p1', 'Legacy', 'insurance/p1/old.pdf'); \
         INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
           VALUES ('batch-1', 'r1', 1767225600); \
         INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path) \
           VALUES ('ph-old', 'batch-1', 'photos/a.jpg', 'photos/a_thumb.jpg');",
    );

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        Reconciled::default()
    );
    assert!(queue_rows(&conn).is_empty());
}

/// Review requirement 2: a row that does not meet its row condition (`registry::row_condition`)
/// never syncs, so its blob is never queued. A photo with plain files and only a full blob is
/// such a row.
#[test]
fn a_row_that_does_not_meet_its_row_condition_queues_nothing() {
    let conn = profile();
    let full = held(&conn, b"full");
    exec(
        &conn,
        &format!(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
               VALUES ('batch-1', 'r1', 1767225600); \
             INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path, blob_id) \
               VALUES ('ph-half', 'batch-1', 'photos/a.jpg', 'photos/a_thumb.jpg', '{full}'); \
             INSERT INTO sync_blobs (blob_id, state, queued_at) VALUES ('{full}', 'up', 1);"
        ),
    );

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(0, 0, 1, 0),
        "an entry for such a row is dropped too"
    );
    assert!(queue_rows(&conn).is_empty());
    assert_eq!(queue::uploads_waiting(&conn).expect("waiting"), 0);
}

#[test]
fn canonical_id_accepts_only_lowercase_hyphenated_uuids() {
    let id = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";
    assert_eq!(
        canonical_id(id).map(|u| u.hyphenated().to_string()),
        Some(id.to_string())
    );
    for refused in [
        "0B9E6A52-3A4C-4E0F-9D7A-1C2B3D4E5F60",
        "{0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60}",
        "urn:uuid:0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60",
        "../x",
        "0b9e6a523a4c4e0f9d7a1c2b3d4e5f60",
        "",
    ] {
        assert_eq!(canonical_id(refused), None, "{refused}");
    }
}

#[test]
fn an_invalid_blob_id_is_queued_failed_and_never_offered() {
    let conn = profile();
    document(&conn, "d1", "../x", 100);
    document(&conn, "d2", "0B9E6A52-3A4C-4E0F-9D7A-1C2B3D4E5F60", 200);

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(0, 2, 0, 0)
    );
    for blob in ["../x", "0B9E6A52-3A4C-4E0F-9D7A-1C2B3D4E5F60"] {
        assert_eq!(state_of(&conn, blob).as_deref(), Some("down"));
        assert_eq!(next_try_at(&conn, blob), i64::MAX);
    }
    assert!(queue_rows(&conn).iter().all(|r| r.ends_with("invalidId")));

    want_row(&conn, AttachmentKind::InsuranceDocument, "d1").expect("want");
    retry_row(&conn, AttachmentKind::InsuranceDocument, "d2").expect("retry");
    want_all(&conn).expect("want all");
    retry_now(&conn).expect("retry now");
    assert_eq!(next_download(&conn, NOW, true).expect("next"), None);
    assert_eq!(next_download(&conn, i64::MAX, true).expect("next"), None);
    assert_eq!(
        ContentReader::new(&conn)
            .expect("reader")
            .of(Some("../x"), true)
            .expect("content"),
        AttachmentContent::Failed
    );
}

#[test]
fn downloads_come_wanted_first_then_thumbnails_then_oldest_rows() {
    let conn = profile();
    document(&conn, "old", &elsewhere(1), 100);
    document(&conn, "new", &elsewhere(2), 200);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 50);
    document(&conn, "newer", &elsewhere(5), 300);
    queue::reconcile(&conn, NOW).expect("reconcile");
    want_row(&conn, AttachmentKind::InsuranceDocument, "new").expect("want");

    let first = next_download(&conn, NOW, true).expect("next").expect("one");
    assert_eq!(
        first,
        Transfer {
            blob_id: elsewhere(2),
            state: BlobState::Down,
            thumbnail: false,
            wanted: true,
            attempts: 0,
            size: None,
        }
    );
    assert_eq!(
        downloads(&conn, true),
        vec![
            elsewhere(2),
            elsewhere(4),
            elsewhere(3),
            elsewhere(1),
            elsewhere(5)
        ]
    );
}

#[test]
fn a_phone_is_offered_only_thumbnails_and_wanted_content() {
    let conn = profile();
    document(&conn, "d1", &elsewhere(1), 100);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 50);
    queue::reconcile(&conn, NOW).expect("reconcile");

    assert_eq!(downloads(&conn, false), vec![elsewhere(4)]);

    want_row(&conn, AttachmentKind::Photo, "ph1").expect("want");
    assert_eq!(downloads(&conn, false), vec![elsewhere(3)]);
    assert!(want_blob(&conn, &elsewhere(1)).expect("want"));
    assert!(
        !want_blob(&conn, &elsewhere(9)).expect("want"),
        "no entry, nothing to want"
    );
    assert_eq!(downloads(&conn, false), vec![elsewhere(1)]);

    // A failed download waits for its backoff, an active one is not offered twice.
    document(&conn, "d2", &elsewhere(2), 100);
    document(&conn, "d3", &elsewhere(5), 200);
    queue::reconcile(&conn, NOW).expect("reconcile");
    want_blob(&conn, &elsewhere(2)).expect("want");
    want_blob(&conn, &elsewhere(5)).expect("want");
    note_failed(&conn, &elsewhere(2), TransferError::Network, NOW).expect("failed");
    set_active(&conn, &elsewhere(5), true).expect("active");
    assert_eq!(next_download(&conn, NOW, false).expect("next"), None);
    clear_active(&conn).expect("clear");
    assert_eq!(
        next_download(&conn, NOW, false)
            .expect("next")
            .map(|t| t.blob_id),
        Some(elsewhere(5))
    );
    set_active(&conn, &elsewhere(5), true).expect("active");
    assert_eq!(
        next_download(&conn, NOW + 30, false)
            .expect("next")
            .map(|t| (t.blob_id, t.attempts)),
        Some((elsewhere(2), 1))
    );
}

#[test]
fn uploads_come_oldest_first_and_respect_the_size_limit() {
    let conn = profile();
    let big = held(&conn, &[7u8; 100]);
    document(&conn, "big", &big, 900);
    queue::reconcile(&conn, NOW).expect("reconcile");
    let small = held(&conn, &[1u8; 10]);
    document(&conn, "small", &small, 100);
    queue::reconcile(&conn, NOW + 60).expect("reconcile");

    assert_eq!(
        next_upload(&conn, NOW + 60, None).expect("next"),
        Some(Transfer {
            blob_id: big.clone(),
            state: BlobState::Up,
            thumbnail: false,
            wanted: false,
            attempts: 0,
            size: Some(100),
        })
    );
    assert_eq!(
        next_upload(&conn, NOW + 60, Some(50))
            .expect("next")
            .map(|t| (t.blob_id, t.size)),
        Some((small.clone(), Some(10)))
    );
    assert_eq!(
        next_upload(&conn, NOW + 60, Some(100))
            .expect("next")
            .map(|t| t.blob_id),
        Some(big.clone()),
        "the limit is inclusive"
    );

    note_failed(&conn, &big, TransferError::Server, NOW + 60).expect("failed");
    assert_eq!(
        next_upload(&conn, NOW + 60, None)
            .expect("next")
            .map(|t| t.blob_id),
        Some(small.clone()),
        "a failed upload waits for its backoff"
    );
    note_done(&conn, &small).expect("done");
    assert_eq!(next_upload(&conn, NOW + 60, None).expect("next"), None);
    assert_eq!(
        next_upload(&conn, NOW + 90, None)
            .expect("next")
            .map(|t| t.blob_id),
        Some(big.clone())
    );

    // The daily pass puts a confirmed blob the server lost back in the queue.
    assert!(requeue_upload(&conn, &small, NOW + 120).expect("requeue"));
    assert!(
        !requeue_upload(&conn, &small, NOW + 120).expect("requeue"),
        "only a done entry"
    );
    assert_eq!(state_of(&conn, &small).as_deref(), Some("up"));
    note_gone(&conn, &small).expect("gone");
    assert_eq!(state_of(&conn, &small), None);
}

#[test]
fn failures_back_off_by_their_kind() {
    // (kind, delay after the 1st, the 2nd failure, the cap, failures to reach the cap)
    let cases = [
        (TransferError::Network, 30, 60, 1_800, 7),
        (TransferError::Server, 30, 60, 1_800, 7),
        (TransferError::Local, 30, 60, 1_800, 7),
        (TransferError::NotOnServer, 300, 600, 3_600, 5),
        (TransferError::Unreadable, 600, 1_200, 21_600, 7),
        (TransferError::NoRoom, 1_800, 1_800, 1_800, 1),
    ];
    let conn = profile();
    for (n, (kind, first, second, cap, at_cap)) in cases.into_iter().enumerate() {
        let blob = elsewhere(n as u8);
        document(&conn, &format!("d{n}"), &blob, 100);
        queue::reconcile(&conn, NOW).expect("reconcile");

        note_failed(&conn, &blob, kind, NOW).expect("1st");
        assert_eq!(next_try_at(&conn, &blob), NOW + first, "{kind:?} 1st");
        note_failed(&conn, &blob, kind, NOW).expect("2nd");
        assert_eq!(next_try_at(&conn, &blob), NOW + second, "{kind:?} 2nd");
        for _ in 2..at_cap.max(2) {
            note_failed(&conn, &blob, kind, NOW).expect("nth");
        }
        assert_eq!(next_try_at(&conn, &blob), NOW + cap, "{kind:?} at the cap");
        if at_cap > 2 {
            exec(
                &conn,
                &format!("UPDATE sync_blobs SET attempts = attempts - 2 WHERE blob_id = '{blob}'"),
            );
            note_failed(&conn, &blob, kind, NOW).expect("below the cap");
            assert!(
                next_try_at(&conn, &blob) < NOW + cap,
                "{kind:?} below the cap"
            );
        }
        for _ in 0..80 {
            note_failed(&conn, &blob, kind, NOW).expect("many");
        }
        assert_eq!(
            next_try_at(&conn, &blob),
            NOW + cap,
            "{kind:?} stays at the cap"
        );
        assert_eq!(
            conn.query_row(
                "SELECT last_error || ' ' || last_error_at || ' ' || active FROM sync_blobs \
                 WHERE blob_id = ?1",
                [&blob],
                |r| r.get::<_, String>(0)
            )
            .expect("bookkeeping"),
            format!("{} {NOW} 0", kind.as_str())
        );
    }

    let blob = elsewhere(99);
    document(&conn, "invalid", &blob, 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    note_failed(&conn, &blob, TransferError::InvalidId, NOW).expect("invalid");
    assert_eq!(next_try_at(&conn, &blob), i64::MAX);

    assert_eq!(
        [
            TransferError::Network,
            TransferError::Server,
            TransferError::NotOnServer,
            TransferError::Unreadable,
            TransferError::NoRoom,
            TransferError::InvalidId,
            TransferError::Local,
            TransferError::Refused,
            TransferError::Conflict,
        ]
        .map(TransferError::as_str),
        [
            "network",
            "server",
            "notOnServer",
            "unreadable",
            "noRoom",
            "invalidId",
            "local",
            "refused",
            "conflict"
        ]
    );

    // A retry now makes every waiting transfer due, a full server's included, except an invalid
    // ID (and a refusal or a conflict, which never move by themselves either).
    retry_now(&conn).expect("retry now");
    let waiting: Vec<(String, i64)> = conn
        .prepare("SELECT COALESCE(last_error, '-'), next_try_at FROM sync_blobs ORDER BY 1")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    for (error, next) in waiting {
        let expected = match error.as_str() {
            "invalidId" => i64::MAX,
            _ => 0,
        };
        assert_eq!(next, expected, "{error}");
    }

    // A retry of one row starts its count again.
    retry_row(&conn, AttachmentKind::InsuranceDocument, "d5").expect("retry row");
    assert_eq!(
        conn.query_row(
            "SELECT attempts || ' ' || next_try_at || ' ' || COALESCE(last_error, '-') \
             FROM sync_blobs WHERE blob_id = ?1",
            [elsewhere(5)],
            |r| r.get::<_, String>(0)
        )
        .expect("retried"),
        "0 0 -"
    );
}

#[test]
fn want_all_marks_every_full_download_and_counts_rows() {
    let conn = profile();
    let here = held(&conn, b"here");
    document(&conn, "d1", &elsewhere(1), 100);
    document(&conn, "d2", &elsewhere(2), 100);
    document(&conn, "d3", &here, 100);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    note_failed(&conn, &elsewhere(1), TransferError::NotOnServer, NOW).expect("failed");

    assert_eq!(want_all(&conn).expect("want all"), 3);

    let wanted: Vec<String> = conn
        .prepare("SELECT blob_id FROM sync_blobs WHERE wanted = 1 ORDER BY blob_id")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect();
    assert_eq!(wanted, vec![elsewhere(1), elsewhere(2), elsewhere(3)]);
    assert_eq!(next_try_at(&conn, &elsewhere(1)), 0, "asked for: due now");
    assert_eq!(want_all(&conn).expect("again"), 3, "still wanted");
    assert_eq!(
        downloads(&conn, false).len(),
        4,
        "the phone fetches it all, the thumbnail included"
    );
    assert_eq!(want_all(&conn).expect("after"), 0);
}

#[test]
fn summary_counts_rows_not_blobs() {
    let conn = profile();
    let full = held(&conn, &[1u8; 700]);
    let thumb = held(&conn, &[2u8; 30]);
    photo(&conn, "ph1", &full, &thumb, 100);
    document(&conn, "d1", &elsewhere(1), 100);
    document(&conn, "d2", &elsewhere(2), 100);
    document(&conn, "d3", &elsewhere(3), 100);
    photo(&conn, "ph2", &elsewhere(4), &elsewhere(5), 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    note_failed(&conn, &elsewhere(2), TransferError::NotOnServer, NOW).expect("failed");
    note_failed(&conn, &elsewhere(3), TransferError::Unreadable, NOW).expect("failed");
    note_failed(&conn, &elsewhere(4), TransferError::Unreadable, NOW).expect("failed");

    assert_eq!(
        summary(&conn).expect("summary"),
        AttachmentSummary {
            waiting_upload: 1,
            waiting_upload_bytes: 730,
            not_here: 1,
            not_on_server: 1,
            failed: 2,
        }
    );
    assert_eq!(uploads_waiting(&conn).expect("waiting"), 1);

    note_done(&conn, &thumb).expect("done");
    assert_eq!(
        uploads_waiting(&conn).expect("waiting"),
        1,
        "the full photo still waits"
    );
    assert_eq!(summary(&conn).expect("summary").waiting_upload_bytes, 700);
    note_done(&conn, &full).expect("done");
    assert_eq!(uploads_waiting(&conn).expect("waiting"), 0);
}

#[test]
fn content_follows_the_queue_and_the_store() {
    let conn = profile();
    let up = held(&conn, b"up");
    let unqueued = held(&conn, b"not queued yet");
    let done = held(&conn, b"done");
    for (n, blob) in [&up, &done].into_iter().enumerate() {
        document(&conn, &format!("h{n}"), blob, 100);
    }
    for n in 1..=6 {
        document(&conn, &format!("e{n}"), &elsewhere(n), 100);
    }
    document(&conn, "bad", "../x", 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    note_done(&conn, &done).expect("done");
    set_active(&conn, &elsewhere(2), true).expect("active");
    note_failed(&conn, &elsewhere(3), TransferError::NotOnServer, NOW).expect("failed");
    note_failed(&conn, &elsewhere(4), TransferError::Unreadable, NOW).expect("failed");
    note_failed(&conn, &elsewhere(5), TransferError::Network, NOW).expect("failed");
    note_gone(&conn, &elsewhere(6)).expect("gone");

    let reader = ContentReader::new(&conn).expect("reader");
    let of = |blob: Option<&str>| reader.of(blob, true).expect("content");
    assert_eq!(of(None), AttachmentContent::Here, "a legacy row");
    assert_eq!(of(Some(&up)), AttachmentContent::WaitingUpload);
    assert_eq!(of(Some(&unqueued)), AttachmentContent::WaitingUpload);
    assert_eq!(of(Some(&done)), AttachmentContent::Here);
    assert_eq!(of(Some(&elsewhere(1))), AttachmentContent::NotHere);
    assert_eq!(of(Some(&elsewhere(2))), AttachmentContent::Downloading);
    assert_eq!(of(Some(&elsewhere(3))), AttachmentContent::NotOnServer);
    assert_eq!(of(Some(&elsewhere(4))), AttachmentContent::Failed);
    assert_eq!(of(Some("../x")), AttachmentContent::Failed);
    assert_eq!(
        of(Some(&elsewhere(5))),
        AttachmentContent::NotHere,
        "a transient failure is retried"
    );
    assert_eq!(of(Some(&elsewhere(6))), AttachmentContent::Missing);

    state::disable(&conn).expect("sync off");
    let off = ContentReader::new(&conn).expect("reader");
    assert_eq!(
        off.of(Some(&unqueued), true).expect("content"),
        AttachmentContent::Here
    );
    assert_eq!(
        off.of(Some(&elsewhere(1)), true).expect("content"),
        AttachmentContent::Missing
    );
    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        Reconciled::default(),
        "no queue with sync off"
    );
    assert!(queue_rows(&conn).is_empty());

    exec(&conn, "DETACH DATABASE files");
    let unavailable = ContentReader::new(&conn).expect("reader");
    assert_eq!(
        unavailable.of(Some(&elsewhere(1)), true).expect("content"),
        AttachmentContent::Here,
        "the unavailable store is reported elsewhere"
    );
}

#[test]
fn without_the_store_nothing_is_queued() {
    let conn = profile();
    document(&conn, "d1", &elsewhere(1), 100);
    exec(&conn, "DETACH DATABASE files");

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        Reconciled::default()
    );
    assert!(queue_rows(&conn).is_empty());
}

#[test]
fn a_merge_queues_the_content_of_the_rows_it_brought() {
    let conn = profile();
    capture::with_mode(&conn, Mode::Merge, || {
        document(&conn, "d1", &elsewhere(1), 100);
        Ok(())
    })
    .expect("merged row");

    repair::after_merge(&conn, &Touched::default(), NOW).expect("repair");
    assert!(
        queue_rows(&conn).is_empty(),
        "nothing about attachments touched"
    );

    let mut touched = Touched::default();
    touched.row("insurance_documents", "d1");
    repair::after_merge(&conn, &touched, NOW).expect("repair");
    assert_eq!(state_of(&conn, &elsewhere(1)).as_deref(), Some("down"));
}

/// Review requirement 2: a merge that deletes a row, or the policy or property that owns it,
/// drops its entry.
#[test]
fn a_merge_that_deletes_a_row_or_its_owner_drops_its_entry() {
    let conn = profile();
    document(&conn, "d1", &elsewhere(1), 100);
    photo(&conn, "ph1", &elsewhere(3), &elsewhere(4), 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!(queue_rows(&conn).len(), 3);

    merge_delete(&conn, "DELETE FROM insurance_documents WHERE id = 'd1'");
    let mut touched = Touched::default();
    touched.row("insurance_documents", "d1");
    touched.mark_deleted("insurance_documents");
    repair::after_merge(&conn, &touched, NOW).expect("repair");
    assert_eq!(state_of(&conn, &elsewhere(1)), None);
    assert_eq!(queue_rows(&conn).len(), 2);

    merge_delete(&conn, "DELETE FROM real_estate WHERE id = 'r1'");
    let mut touched = Touched::default();
    touched.row("real_estate", "r1");
    touched.mark_deleted("real_estate");
    repair::after_merge(&conn, &touched, NOW).expect("repair");
    assert!(queue_rows(&conn).is_empty());
}

/// Content on its way is not a failed conversion, but it is not content here either: the strict
/// count (`rows_without_content`, the quarantine and the integrity check) still counts it.
#[test]
fn content_on_its_way_is_not_missing_content() {
    let conn = profile();
    let coming =
        |c: &Connection| attachment_store::rows_without_content_or_download(c).expect("count");
    let strict = |c: &Connection| attachment_store::rows_without_content(c).expect("count");
    document(&conn, "d1", &elsewhere(1), 100);
    assert_eq!((coming(&conn), strict(&conn)), (1, 1));

    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!((coming(&conn), strict(&conn)), (0, 1));

    // A photo is missing content while either blob is neither here nor on its way.
    let thumb = held(&conn, b"thumb");
    photo(&conn, "ph1", &elsewhere(2), &thumb, 100);
    assert_eq!((coming(&conn), strict(&conn)), (1, 2));
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!((coming(&conn), strict(&conn)), (0, 2));

    note_gone(&conn, &elsewhere(1)).expect("gone");
    assert_eq!((coming(&conn), strict(&conn)), (1, 2));

    // An ID that can never be fetched is not on its way (the reconcile queues `d1` again).
    document(&conn, "bad", "../x", 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!((coming(&conn), strict(&conn)), (1, 3));
}

/// Review fix 2: a row that does not meet its row condition never syncs, so content held for it
/// is plain "here", never "waiting to upload" (nothing would ever clear that mark). A row that
/// syncs and has no entry yet (before the next reconcile) waits.
#[test]
fn a_row_that_does_not_sync_reads_here_never_waiting() {
    let conn = profile();
    let full = held(&conn, b"full");
    exec(
        &conn,
        &format!(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
               VALUES ('batch-1', 'r1', 1767225600); \
             INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path, blob_id) \
               VALUES ('ph-half', 'batch-1', 'photos/a.jpg', 'photos/a_thumb.jpg', '{full}');"
        ),
    );
    queue::reconcile(&conn, NOW).expect("reconcile");
    let reader = ContentReader::new(&conn).expect("reader");

    let half = row_syncs(AttachmentKind::Photo, Some(&full), None).expect("syncs");
    assert!(!half);
    assert_eq!(
        reader.of(Some(&full), half).expect("content"),
        AttachmentContent::Here
    );
    assert_eq!(queue::uploads_waiting(&conn).expect("waiting"), 0);

    let fresh = held(&conn, b"fresh");
    document(&conn, "fresh", &fresh, 100);
    let syncs = row_syncs(AttachmentKind::InsuranceDocument, Some(&fresh), None).expect("syncs");
    assert!(syncs);
    assert_eq!(
        reader.of(Some(&fresh), syncs).expect("content"),
        AttachmentContent::WaitingUpload
    );
    assert!(row_syncs(AttachmentKind::Photo, Some(&full), Some(&fresh)).expect("syncs"));
    assert!(!row_syncs(AttachmentKind::RealEstateDocument, None, None).expect("syncs"));
}

/// Review fix 3: one definition of content complete on this device (hash recorded, content at its
/// recorded size) for the queue, the reader and `rows_without_content`.
#[test]
fn a_truncated_blob_is_neither_queued_for_upload_nor_here() {
    let conn = profile();
    let blob = held(&conn, b"full content");
    exec(
        &conn,
        &format!("UPDATE files.blobs SET data = x'00' WHERE id = '{blob}'"),
    );
    document(&conn, "d1", &blob, 100);

    assert_eq!(
        queue::reconcile(&conn, NOW).expect("reconcile"),
        reconciled(0, 1, 0, 0)
    );
    assert_eq!(next_upload(&conn, NOW, None).expect("next"), None);
    assert_eq!(
        ContentReader::new(&conn)
            .expect("reader")
            .of(Some(&blob), true)
            .expect("content"),
        AttachmentContent::NotHere
    );
    assert_eq!(
        attachment_store::rows_without_content(&conn).expect("count"),
        1
    );
}

/// Review fix 5, Task 11 fix round 2: content complete here under an ID that can never be
/// transferred is "here" in the reader and a failed item in the summary, never waiting to upload
/// and never taken as on the server: `up` with `invalidId`, never picked. Losing its content keeps
/// the failure, and so does "try again".
#[test]
fn a_held_blob_with_an_invalid_id_reads_here_and_counts_as_failed() {
    let conn = profile();
    arrive(&conn, "../x", b"content");
    document(&conn, "d1", "../x", 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    let entry = |conn: &Connection| -> (String, Option<String>, i64) {
        conn.query_row(
            "SELECT state, last_error, next_try_at FROM sync_blobs WHERE blob_id = '../x'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("entry")
    };
    let read = |conn: &Connection| {
        ContentReader::new(conn)
            .expect("reader")
            .of(Some("../x"), true)
            .expect("content")
    };
    let failed_only = AttachmentSummary {
        failed: 1,
        ..AttachmentSummary::default()
    };

    assert_eq!(
        entry(&conn),
        ("up".into(), Some("invalidId".into()), i64::MAX)
    );
    assert_eq!(read(&conn), AttachmentContent::Here);
    assert_eq!(summary(&conn).expect("summary"), failed_only);
    assert_eq!(queue::next_upload(&conn, NOW, None).expect("next"), None);

    queue::retry_row(&conn, AttachmentKind::InsuranceDocument, "d1").expect("try again");
    assert_eq!(
        entry(&conn),
        ("up".into(), Some("invalidId".into()), i64::MAX)
    );

    attachment_store::delete(&conn, "../x").expect("lose");
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!(
        entry(&conn),
        ("down".into(), Some("invalidId".into()), i64::MAX)
    );
    assert_eq!(read(&conn), AttachmentContent::Failed);
    assert_eq!(summary(&conn).expect("summary"), failed_only);

    arrive(&conn, "../x", b"content");
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!(
        entry(&conn),
        ("up".into(), Some("invalidId".into()), i64::MAX)
    );
}

/// Review fix 4: a merged changeset reconciles only what rows can change (a merge never removes
/// content a row still names); a restore, like the end of a pull, reconciles the whole queue, so
/// content this store lost is fetched again.
#[test]
fn a_restore_reconciles_the_whole_queue() {
    let conn = profile();
    let blob = held(&conn, b"mine");
    document(&conn, "d1", &blob, 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    attachment_store::delete(&conn, &blob).expect("lose");

    let mut touched = Touched::default();
    touched.row("insurance_documents", "d1");
    repair::after_merge(&conn, &touched, NOW).expect("merge repair");
    assert_eq!(
        state_of(&conn, &blob).as_deref(),
        Some("up"),
        "left to the full reconcile"
    );

    repair::full(&conn, NOW).expect("restore repair");
    assert_eq!(state_of(&conn, &blob).as_deref(), Some("down"));
}

fn content_of(conn: &Connection, blob: &str) -> AttachmentContent {
    ContentReader::new(conn)
        .expect("reader")
        .of(Some(blob), true)
        .expect("content")
}

/// Fix round 2, item 1: content held here that the server refused for good, that conflicts with
/// what the server holds, or whose ID is invalid can never upload by itself: it counts as failed
/// and never as waiting (so the device can count as synchronised), and reads as failed (an invalid
/// ID reads as here: the content is here and nothing will ever move it, Task 11 fix round 2),
/// until "try again" on its row makes a refused or conflicting one wait again. An entry merely
/// backing off still waits.
#[test]
fn an_upload_that_cannot_move_by_itself_reads_failed_until_tried_again() {
    for error in [
        TransferError::Refused,
        TransferError::Conflict,
        TransferError::InvalidId,
    ] {
        let conn = profile();
        let stuck = held(&conn, b"stuck");
        let backing_off = held(&conn, b"waits!");
        document(&conn, "d1", &stuck, 100);
        document(&conn, "d2", &backing_off, 200);
        queue::reconcile(&conn, NOW).expect("reconcile");
        note_failed(&conn, &stuck, error, NOW).expect("stuck");
        note_failed(&conn, &backing_off, TransferError::Network, NOW).expect("backing off");
        let reads_failed = if error == TransferError::InvalidId {
            AttachmentContent::Here
        } else {
            AttachmentContent::Failed
        };

        assert_eq!(content_of(&conn, &stuck), reads_failed, "{error:?}");
        assert_eq!(
            content_of(&conn, &backing_off),
            AttachmentContent::WaitingUpload,
            "{error:?}"
        );
        let counts = summary(&conn).expect("summary");
        assert_eq!(
            (
                counts.waiting_upload,
                counts.waiting_upload_bytes,
                counts.failed
            ),
            (1, 6, 1),
            "{error:?}"
        );
        assert_eq!(uploads_waiting(&conn).expect("waiting"), 1, "{error:?}");
        assert!(waiting(&conn, true).expect("waiting"), "{error:?}");

        // Only the stuck one left: nothing waits.
        conn.execute("DELETE FROM insurance_documents WHERE id = 'd2'", [])
            .expect("delete");
        queue::reconcile(&conn, NOW).expect("reconcile");
        assert_eq!(uploads_waiting(&conn).expect("waiting"), 0, "{error:?}");
        assert!(!waiting(&conn, true).expect("waiting"), "{error:?}");
        assert!(!waiting(&conn, false).expect("waiting"), "{error:?}");
        assert_eq!(summary(&conn).expect("summary").waiting_upload, 0);

        // "Try again" on its row: a refusal or a conflict waits again; an invalid ID stays
        // failed (no try makes it valid, `retry_row` never touches one).
        retry_row(&conn, AttachmentKind::InsuranceDocument, "d1").expect("try again");
        let again = error != TransferError::InvalidId;
        let expected = if again {
            AttachmentContent::WaitingUpload
        } else {
            reads_failed
        };
        assert_eq!(content_of(&conn, &stuck), expected, "{error:?}");
        let counts = summary(&conn).expect("summary");
        assert_eq!(
            (counts.waiting_upload, counts.failed),
            (u32::from(again), u32::from(!again)),
            "{error:?}"
        );
        assert_eq!(
            uploads_waiting(&conn).expect("waiting"),
            u32::from(again),
            "{error:?}"
        );
        assert_eq!(waiting(&conn, true).expect("waiting"), again, "{error:?}");
    }
}

/// Fix round 2, item 3: a `found` note that finds nothing to move (the content left again) still
/// ends the transfer, so the entry never reads as downloading until the lanes restart.
#[test]
fn a_found_note_that_finds_nothing_still_ends_the_transfer() {
    let conn = profile();
    let blob = elsewhere(1);
    document(&conn, "d1", &blob, 100);
    queue::reconcile(&conn, NOW).expect("reconcile");
    set_active(&conn, &blob, true).expect("active");

    assert!(!note_found(&conn, &blob, NOW).expect("found"));

    assert_eq!(state_of(&conn, &blob).as_deref(), Some("down"));
    assert_eq!(content_of(&conn, &blob), AttachmentContent::NotHere);
}

/// The queue keeps an `up` entry's plaintext size, so the status never reads the store for it: a
/// new entry records it, and the full reconcile fills it for an entry that lacks it (one from
/// before migration 038).
#[test]
fn an_up_entry_keeps_its_size() {
    let conn = profile();
    let blob = held(&conn, b"twelve bytes");
    document(&conn, "d1", &blob, 100);
    let size = |conn: &Connection| -> Option<i64> {
        conn.query_row(
            "SELECT size FROM sync_blobs WHERE blob_id = ?1",
            [&blob],
            |r| r.get(0),
        )
        .expect("entry")
    };

    queue::reconcile_rows(&conn, NOW).expect("reconcile rows");
    assert_eq!(size(&conn), Some(12));

    exec(&conn, "UPDATE sync_blobs SET size = NULL");
    queue::reconcile(&conn, NOW).expect("reconcile");
    assert_eq!(size(&conn), Some(12));
    assert_eq!(summary(&conn).expect("summary").waiting_upload_bytes, 12);
}
