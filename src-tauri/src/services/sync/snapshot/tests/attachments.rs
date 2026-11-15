//! Documents and photos across a replace (a join, a repair, a re-sync): the attachment rows are
//! the vault's afterwards, like every other synced row. Content this device holds under a blob ID
//! a vault row names stays (no download later); this device's rows the vault lacks, legacy rows
//! without a blob included, are gone and counted as orphaned (they remain only in the backup), and
//! their content is held until the next pull caught up (`held_blobs`).

use super::*;
use crate::db::files_schema;
use crate::services::attachment_store;

const POLICY: &str = "INSERT INTO insurance_policies \
      (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
    VALUES (?1, 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')";
const PROPERTY: &str =
    "INSERT INTO real_estate (id, name, address, type) VALUES (?1, 'Flat', 'Street', 'flat')";

/// `conn` with an attachment store, as the app opens a profile.
fn with_store(conn: Connection) -> Connection {
    conn.execute("ATTACH DATABASE ':memory:' AS files", [])
        .expect("attach");
    files_schema::migrate(&conn).expect("files schema");
    conn
}

fn policy(conn: &Connection, id: &str) {
    conn.execute(POLICY, [id]).expect("policy");
}

fn property(conn: &Connection, id: &str) {
    conn.execute(PROPERTY, [id]).expect("property");
}

fn blob(conn: &Connection, bytes: &[u8]) -> String {
    attachment_store::put_bytes(conn, bytes).expect("blob").id
}

/// A policy document `id` under `policy` with a new blob; returns the blob's ID.
fn policy_document(conn: &Connection, id: &str, policy: &str) -> String {
    let blob = blob(conn, format!("%PDF-1.7 {id}").as_bytes());
    conn.execute(
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, mime_type) \
         VALUES (?1, ?2, 'Contract', ?3, 'application/pdf')",
        [id, policy, &blob],
    )
    .expect("document");
    blob
}

/// A property document `id` under `property` with a new blob; returns the blob's ID.
fn property_document(conn: &Connection, id: &str, property: &str) -> String {
    let blob = blob(conn, format!("deed {id}").as_bytes());
    conn.execute(
        "INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id) \
         VALUES (?1, ?2, 'Deed', ?3)",
        [id, property, &blob],
    )
    .expect("document");
    blob
}

/// A batch `batch` of `property` with one photo `photo`; returns its two blobs.
fn photo(conn: &Connection, property: &str, batch: &str, photo: &str) -> (String, String) {
    let full = blob(conn, format!("full {photo}").as_bytes());
    let thumb = blob(conn, format!("thumb {photo}").as_bytes());
    conn.execute(
        "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
         VALUES (?1, ?2, 1767225600)",
        [batch, property],
    )
    .expect("batch");
    conn.execute(
        "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) \
         VALUES (?1, ?2, ?3, ?4)",
        [photo, batch, &full, &thumb],
    )
    .expect("photo");
    (full, thumb)
}

fn readable(conn: &Connection, blob: &str) -> bool {
    attachment_store::size_of(conn, blob)
        .expect("size")
        .is_some()
}

fn carry(kept: u32, orphaned: u32) -> AttachmentCarry {
    AttachmentCarry { kept, orphaned }
}

#[test]
fn a_resync_keeps_documents_the_vault_has_and_their_blobs() {
    let a = with_store(device(A));
    policy(&a, "p1");
    let b1 = policy_document(&a, "d1", "p1");
    let (bytes, _) = snapshot(&a, 1);

    let report = restore_replace(&a, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(report.attachments, carry(1, 0));
    assert_eq!(
        strings(
            &a,
            "SELECT id || ' ' || insurance_id || ' ' || blob_id FROM insurance_documents"
        ),
        vec![format!("d1 p1 {b1}")]
    );
    assert!(readable(&a, &b1));
    assert_eq!(
        attachment_store::read_all(&a, &b1).expect("blob"),
        b"%PDF-1.7 d1".to_vec()
    );
}

#[test]
fn photos_keep_their_batch_and_both_blobs() {
    let a = with_store(device(A));
    property(&a, "r1");
    let (full, thumb) = photo(&a, "r1", "batch-1", "ph1");
    let (bytes, _) = snapshot(&a, 1);

    let report = restore_replace(&a, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(
        strings(
            &a,
            "SELECT p.id || ' ' || b.id || ' ' || b.real_estate_id FROM real_estate_photos p \
             JOIN real_estate_photo_batches b ON b.id = p.batch_id"
        ),
        vec!["ph1 batch-1 r1"]
    );
    assert!(readable(&a, &full));
    assert!(readable(&a, &thumb));
    assert_eq!(report.attachments, carry(1, 0), "batches are not counted");
}

#[test]
fn a_document_only_this_device_had_is_gone_and_counted() {
    let a = device(A);
    policy(&a, "p1");
    let c = with_store(device(C));
    apply(&c, &send(&a));
    let own = policy_document(&c, "d1", "p1");
    let (bytes, _) = snapshot(&a, 1);

    let report = restore_replace(&c, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(report.attachments, carry(0, 1));
    assert_eq!(count(&c, "SELECT COUNT(*) FROM insurance_documents"), 0);
    assert!(
        readable(&c, &own),
        "held until the next pull caught up: the row may come back with it"
    );
    crate::services::sync::held_blobs::release(&c).expect("release");
    assert!(!readable(&c, &own), "then the orphan's blob is purged");
}

#[test]
fn a_joiner_gets_the_vaults_attachment_rows_without_content() {
    let a = with_store(device(A));
    policy(&a, "p1");
    property(&a, "r1");
    let doc = policy_document(&a, "d1", "p1");
    let (full, thumb) = photo(&a, "r1", "batch-1", "ph1");
    let (bytes, _) = snapshot(&a, 1);

    let b = with_store(migrated());
    let report = join(&b, B, &mut &bytes[..], NOW).expect("join");

    assert_eq!(report.attachments, carry(0, 0));
    assert_eq!(
        strings(&b, "SELECT id || ' ' || blob_id FROM insurance_documents"),
        vec![format!("d1 {doc}")]
    );
    assert_eq!(
        strings(
            &b,
            "SELECT id || ' ' || batch_id || ' ' || blob_id || ' ' || thumb_blob_id \
               || ' ' || COALESCE(file_path, '-') || ' ' || COALESCE(thumbnail_path, '-') \
             FROM real_estate_photos"
        ),
        vec![format!("ph1 batch-1 {full} {thumb} - -")]
    );
    assert!(
        !readable(&b, &doc),
        "content is fetched later, never in a snapshot"
    );
    assert!(!readable(&b, &full) && !readable(&b, &thumb));
}

#[test]
fn a_legacy_row_is_counted_as_orphaned() {
    let a = device(A);
    policy(&a, "p1");
    let c = with_store(device(C));
    apply(&c, &send(&a));
    exec(
        &c,
        "INSERT INTO insurance_documents (id, insurance_id, name, file_path) \
         VALUES ('old', 'p1', 'Old contract', 'insurance/p1/old.pdf')",
    );
    let (bytes, _) = snapshot(&a, 1);

    let report = restore_replace(&c, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(report.attachments, carry(0, 1));
    assert_eq!(count(&c, "SELECT COUNT(*) FROM insurance_documents"), 0);
}

/// A second replace right after the first (a re-sync tried again) leaves the same rows once and
/// reports the same counts.
#[test]
fn a_second_replace_back_to_back_adds_no_duplicate_attachments() {
    let a = device(A);
    policy(&a, "p1");
    property(&a, "r1");
    let c = with_store(device(C));
    apply(&c, &send(&a));
    let doc = policy_document(&c, "d1", "p1");
    let deed = property_document(&c, "rd1", "r1");
    let (full, thumb) = photo(&c, "r1", "batch-1", "ph1");
    apply(&a, &send(&c));
    let (bytes, _) = snapshot(&a, 1);
    let rows = "SELECT 'i ' || id FROM insurance_documents \
                UNION ALL SELECT 'r ' || id FROM real_estate_documents \
                UNION ALL SELECT 'b ' || id FROM real_estate_photo_batches \
                UNION ALL SELECT 'p ' || id FROM real_estate_photos ORDER BY 1";

    let first = restore_replace(&c, &mut &bytes[..], NOW).expect("first replace");
    let after_first = strings(&c, rows);
    let second = restore_replace(&c, &mut &bytes[..], NOW).expect("second replace");

    assert_eq!(first.attachments, carry(3, 0));
    assert_eq!(second.attachments, first.attachments);
    assert_eq!(strings(&c, rows), after_first);
    assert_eq!(after_first, vec!["b batch-1", "i d1", "p ph1", "r rd1"]);
    assert!(
        readable(&c, &doc) && readable(&c, &deed) && readable(&c, &full) && readable(&c, &thumb)
    );
}

#[test]
fn a_failed_replace_leaves_attachments_as_they_were() {
    let a = device(A);
    policy(&a, "p1");
    let (bytes, _) = snapshot(&a, 1);
    let broken = with_orphan(&bytes);

    let c = with_store(device(C));
    policy(&c, "p1");
    let b1 = policy_document(&c, "d1", "p1");
    property(&c, "r1");
    let (full, thumb) = photo(&c, "r1", "batch-1", "ph1");
    exec(
        &c,
        "INSERT INTO insurance_documents (id, insurance_id, name, file_path) \
         VALUES ('old', 'p1', 'Old contract', 'insurance/p1/old.pdf'); \
         INSERT INTO sync_blobs (blob_id, state, queued_at) VALUES ('queued', 'up', 1);",
    );
    let documents = "SELECT id || ' ' || insurance_id || ' ' || COALESCE(blob_id, file_path) \
                     FROM insurance_documents ORDER BY id";
    let photos = "SELECT id || ' ' || batch_id FROM real_estate_photos";
    let before = (strings(&c, documents), strings(&c, photos));

    restore_replace(&c, &mut &broken[..], NOW).expect_err("stopped");

    assert_eq!((strings(&c, documents), strings(&c, photos)), before);
    assert!(readable(&c, &b1) && readable(&c, &full) && readable(&c, &thumb));
    assert_eq!(count(&c, "SELECT COUNT(*) FROM sync_blobs"), 1);
    assert!(c.is_autocommit());

    let b = with_store(migrated());
    policy(&b, "p1");
    let own = policy_document(&b, "d1", "p1");
    join(&b, B, &mut &broken[..], NOW).expect_err("stopped");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM insurance_documents"), 1);
    assert!(readable(&b, &own));
}

/// The transfer queue names blobs of the rows this device had; the rows are the vault's after a
/// replace, so a queue left behind would be stale. It is emptied with the rest of the bookkeeping
/// (a later step rebuilds it from the rows and the store).
#[test]
fn a_replace_leaves_no_transfer_queue_behind() {
    let a = device(A);
    policy(&a, "p1");
    let (bytes, _) = snapshot(&a, 1);
    let c = with_store(device(C));
    exec(
        &c,
        "INSERT INTO sync_blobs (blob_id, state, queued_at) VALUES \
           ('gone-up', 'up', 1), ('gone-down', 'down', 1);",
    );

    restore_replace(&c, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(count(&c, "SELECT COUNT(*) FROM sync_blobs"), 0);
}
