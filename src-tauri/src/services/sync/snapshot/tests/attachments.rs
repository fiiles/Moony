//! Audit H3: a replace (a join, a repair, a re-sync) keeps this device's documents and photos,
//! which never travel in a snapshot, wherever their item is in the vault; the rest go with their
//! blobs and are counted.

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

fn kept_tables(conn: &Connection) -> Vec<String> {
    strings(
        conn,
        "SELECT name FROM sqlite_temp_master WHERE name LIKE 'sync_kept_%'",
    )
}

fn carry(kept: u32, orphaned: u32) -> AttachmentCarry {
    AttachmentCarry { kept, orphaned }
}

#[test]
fn a_resync_keeps_a_policys_document_and_its_blob() {
    let a = with_store(device(A));
    policy(&a, "p1");
    let b1 = policy_document(&a, "d1", "p1");
    let (bytes, _) = snapshot(&a, 1);

    let report = restore_replace(&a, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(
        count(
            &a,
            "SELECT COUNT(*) FROM insurance_documents WHERE id = 'd1'"
        ),
        1
    );
    assert_eq!(
        attachment_store::read_all(&a, &b1).expect("blob"),
        b"%PDF-1.7 d1".to_vec()
    );
    assert_eq!(report.attachments, carry(1, 0));
    assert!(kept_tables(&a).is_empty());
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
    assert_eq!(report.attachments, carry(1, 0));
}

#[test]
fn a_document_whose_parent_the_vault_deleted_goes_and_is_counted() {
    let a = device(A);
    policy(&a, "p1");
    policy(&a, "p2");
    let c = with_store(device(C));
    apply(&c, &send(&a));
    let b1 = policy_document(&c, "d1", "p1");
    let b2 = policy_document(&c, "d2", "p2");
    exec(&a, "DELETE FROM insurance_policies WHERE id = 'p2'");
    let (bytes, _) = snapshot(&a, 2);

    let report = restore_replace(&c, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(
        strings(&c, "SELECT id FROM insurance_documents ORDER BY id"),
        vec!["d1"]
    );
    assert!(readable(&c, &b1));
    assert!(!readable(&c, &b2), "the orphan's blob is purged");
    assert_eq!(report.attachments, carry(1, 1));
}

#[test]
fn a_property_that_the_vault_deleted_takes_its_photos_and_documents() {
    let a = device(A);
    property(&a, "r1");
    property(&a, "r2");
    let c = with_store(device(C));
    apply(&c, &send(&a));
    let (kept_full, kept_thumb) = photo(&c, "r1", "batch-1", "ph1");
    let (gone_full, gone_thumb) = photo(&c, "r2", "batch-2", "ph2");
    let deed = blob(&c, b"deed");
    c.execute(
        "INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id) \
         VALUES ('rd2', 'r2', 'Deed', ?1)",
        [&deed],
    )
    .expect("document");
    exec(&a, "DELETE FROM real_estate WHERE id = 'r2'");
    let (bytes, _) = snapshot(&a, 2);

    let report = restore_replace(&c, &mut &bytes[..], NOW).expect("replace");

    assert_eq!(
        report.attachments,
        carry(1, 2),
        "a photo and a document go; batches are not counted"
    );
    assert_eq!(
        strings(&c, "SELECT id FROM real_estate_photo_batches"),
        vec!["batch-1"]
    );
    assert_eq!(count(&c, "SELECT COUNT(*) FROM real_estate_documents"), 0);
    assert!(readable(&c, &kept_full) && readable(&c, &kept_thumb));
    assert!(!readable(&c, &gone_full) && !readable(&c, &gone_thumb) && !readable(&c, &deed));
}

#[test]
fn a_failed_replace_leaves_attachments_and_drops_the_temp_tables() {
    let a = device(A);
    policy(&a, "p1");
    let (bytes, _) = snapshot(&a, 1);
    let broken = with_orphan(&bytes);

    let c = with_store(device(C));
    policy(&c, "p1");
    let b1 = policy_document(&c, "d1", "p1");
    property(&c, "r1");
    let (full, thumb) = photo(&c, "r1", "batch-1", "ph1");
    let documents = "SELECT id || ' ' || insurance_id || ' ' || blob_id FROM insurance_documents";
    let photos = "SELECT id || ' ' || batch_id FROM real_estate_photos";
    let before = (strings(&c, documents), strings(&c, photos));

    restore_replace(&c, &mut &broken[..], NOW).expect_err("stopped");

    assert_eq!((strings(&c, documents), strings(&c, photos)), before);
    assert!(readable(&c, &b1) && readable(&c, &full) && readable(&c, &thumb));
    assert!(kept_tables(&c).is_empty());
    assert!(c.is_autocommit());

    let b = with_store(migrated());
    policy(&b, "p1");
    let own = policy_document(&b, "d1", "p1");
    join(&b, B, &mut &broken[..], NOW).expect_err("stopped");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM insurance_documents"), 1);
    assert!(readable(&b, &own));
    assert!(kept_tables(&b).is_empty());
}

#[test]
fn a_new_devices_own_documents_count_as_orphaned() {
    let a = device(A);
    policy(&a, "p1");
    let (bytes, _) = snapshot(&a, 1);

    let b = with_store(migrated());
    policy(&b, "own");
    let own = policy_document(&b, "d-own", "own");
    let report = join(&b, B, &mut &bytes[..], NOW).expect("join");

    assert_eq!(report.attachments, carry(0, 1));
    assert_eq!(count(&b, "SELECT COUNT(*) FROM insurance_documents"), 0);
    assert!(!readable(&b, &own));
    assert!(kept_tables(&b).is_empty());
}

#[test]
fn a_kit_rejoin_carries_documents_of_items_the_vault_has() {
    let a = device(A);
    policy(&a, "p1");
    let (bytes, _) = snapshot(&a, 1);

    let b = with_store(migrated());
    policy(&b, "p1");
    let b1 = policy_document(&b, "d1", "p1");
    let report = join(&b, B, &mut &bytes[..], NOW).expect("join");

    assert_eq!(report.attachments, carry(1, 0));
    assert_eq!(
        strings(
            &b,
            "SELECT id || ' ' || insurance_id FROM insurance_documents"
        ),
        vec!["d1 p1"]
    );
    assert!(readable(&b, &b1));
}
