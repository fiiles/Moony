//! Attachment operations: documents of policies and properties, photos in batches, and the
//! deletes of all three. Adds and deletes go through `services::attachments` and
//! `services::real_estate_photos` (the content goes into the device's in-memory `files` store),
//! batches are created as `create_photo_batch` does (a plain insert: the command has no service).
//! There is no document rename service, so no operation edits a document; the scenarios do.

use std::path::Path;

use rand::rngs::StdRng;
use rand::RngExt;
use rusqlite::Connection;

use super::{by_content, day, no_target, now, one_of, pick, run, OpFn, Res};
use crate::models::AttachmentKind;
use crate::services::attachments::{self, NewDocument};
use crate::services::real_estate_photos::{self, prepared_for_test};

pub(super) const OPS: &[(&str, OpFn)] = &[
    ("attachments::add_policy_document", add_policy_document),
    ("attachments::add_property_document", add_property_document),
    ("attachments::add_photo", add_photo),
    ("attachments::delete_document", delete_document),
    ("attachments::delete_photo", delete_photo),
    ("attachments::delete_photo_batch", delete_photo_batch),
];

fn policies() -> String {
    by_content(
        "insurance_policies",
        "t.id",
        "",
        "t.policy_name, t.provider, t.start_date",
    )
}

fn properties() -> String {
    by_content(
        "real_estate",
        "t.id",
        "",
        "t.name, t.purchase_date, t.purchase_price",
    )
}

/// A small distinct "PDF": the number makes the content (and its hash) differ per call.
fn pdf(rng: &mut StdRng) -> Vec<u8> {
    format!("%PDF-1.4 sample {}", rng.random_range(0..1_000_000)).into_bytes()
}

fn document_facts(rng: &mut StdRng) -> (&'static str, &'static str, &'static str) {
    (
        one_of(rng, &["Contract", "Invoice", "Deed"]),
        one_of(rng, &["contract", "invoice", "other"]),
        one_of(rng, &["a.pdf", "b.pdf", "scan.pdf"]),
    )
}

fn add_policy_document(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(policy) = pick(conn, rng, &policies())? else {
        return no_target();
    };
    let (name, file_type, file) = document_facts(rng);
    let bytes = pdf(rng);
    let doc = NewDocument {
        name,
        description: rng.random_bool(0.5).then_some("A note"),
        file_type,
    };
    run(format!("{policy} {name:?} {file}"), || {
        attachments::add_insurance_document_bytes(conn, &policy, file, &bytes, &doc, now())
            .map(drop)
    })
}

fn add_property_document(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(property) = pick(conn, rng, &properties())? else {
        return no_target();
    };
    let (name, file_type, file) = document_facts(rng);
    let bytes = pdf(rng);
    let doc = NewDocument {
        name,
        description: None,
        file_type,
    };
    run(format!("{property} {name:?} {file}"), || {
        attachments::add_real_estate_document_bytes(conn, &property, file, &bytes, &doc, now())
            .map(drop)
    })
}

fn batches_of(property: &str) -> String {
    by_content(
        "real_estate_photo_batches",
        "t.id",
        &format!("WHERE t.real_estate_id = '{property}'"),
        "t.photo_date, t.description",
    )
}

/// A photo into the property's newest batch, or into a new one (also when it has none).
fn add_photo(conn: &mut Connection, rng: &mut StdRng, _: &Path) -> Res {
    let Some(property) = pick(conn, rng, &properties())? else {
        return no_target();
    };
    let newest = super::ids(conn, &batches_of(&property))?.pop();
    let new_batch = newest.is_none() || rng.random_bool(0.3);
    let (date, description) = (day(rng), one_of(rng, &["Kitchen", "Garden"]));
    let (full, thumb) = (pdf(rng), pdf(rng));
    run(
        format!(
            "{property} {}",
            if new_batch {
                "new batch"
            } else {
                "newest batch"
            }
        ),
        || {
            let batch = match newest {
                Some(batch) if !new_batch => batch,
                _ => {
                    let id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO real_estate_photo_batches \
                           (id, real_estate_id, photo_date, description, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![id, property, date, description, now()],
                    )?;
                    id
                }
            };
            real_estate_photos::record_photo(conn, &batch, &prepared_for_test(full, thumb), now())
                .map(drop)
        },
    )
}

fn delete_document(conn: &mut Connection, rng: &mut StdRng, dir: &Path) -> Res {
    let (kind, table) = if rng.random_bool(0.5) {
        (AttachmentKind::InsuranceDocument, "insurance_documents")
    } else {
        (AttachmentKind::RealEstateDocument, "real_estate_documents")
    };
    let sql = by_content(table, "t.id", "", "t.name, t.file_type, t.original_name");
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(format!("{table} {id}"), || {
        attachments::delete_document(conn, dir, kind, &id)
    })
}

fn delete_photo(conn: &mut Connection, rng: &mut StdRng, dir: &Path) -> Res {
    let sql = by_content(
        "real_estate_photos",
        "t.id",
        "",
        "(SELECT b.photo_date FROM real_estate_photo_batches b WHERE b.id = t.batch_id)",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(id.clone(), || attachments::delete_photo(conn, dir, &id))
}

fn delete_photo_batch(conn: &mut Connection, rng: &mut StdRng, dir: &Path) -> Res {
    let sql = by_content(
        "real_estate_photo_batches",
        "t.id",
        "",
        "t.photo_date, t.description",
    );
    let Some(id) = pick(conn, rng, &sql)? else {
        return no_target();
    };
    run(id.clone(), || {
        attachments::delete_photo_batch(conn, dir, &id)
    })
}

/// The queue invariant of a device (ADR 0030): after `reconcile`, every blob a syncing row names
/// is held here or queued `down` (and an entry's state agrees with the store), and no entry names
/// a blob no syncing row names.
pub(in crate::services::sync::convergence) fn queue_consistent(conn: &Connection) {
    crate::services::sync::blobs::queue::reconcile(conn, now()).expect("reconcile");
    let referenced = "SELECT blob_id AS b FROM insurance_documents WHERE blob_id IS NOT NULL \
        UNION SELECT blob_id FROM real_estate_documents WHERE blob_id IS NOT NULL \
        UNION SELECT blob_id FROM real_estate_photos WHERE blob_id IS NOT NULL \
          AND thumb_blob_id IS NOT NULL \
        UNION SELECT thumb_blob_id FROM real_estate_photos WHERE blob_id IS NOT NULL \
          AND thumb_blob_id IS NOT NULL";
    let held = |id: &str| crate::services::attachment_store::complete_sql(id);
    let list = |sql: String| -> Vec<String> {
        let mut stmt = conn.prepare(&sql).expect("prepare");
        stmt.query_map([], |r| r.get(0))
            .expect("query")
            .collect::<std::result::Result<_, _>>()
            .expect("rows")
    };
    let missing = list(format!(
        "SELECT b FROM ({referenced}) WHERE NOT {held} \
         AND NOT EXISTS (SELECT 1 FROM sync_blobs q WHERE q.blob_id = b AND q.state = 'down')",
        held = held("b")
    ));
    assert!(
        missing.is_empty(),
        "blobs neither held nor queued down: {missing:?}"
    );
    let unreferenced = list(format!(
        "SELECT blob_id FROM sync_blobs WHERE blob_id NOT IN ({referenced})"
    ));
    assert!(
        unreferenced.is_empty(),
        "entries naming no row: {unreferenced:?}"
    );
    let wrong = list(format!(
        "SELECT blob_id FROM sync_blobs WHERE (state IN ('up', 'done') AND NOT {held}) \
         OR (state = 'down' AND {held} AND last_error IS NOT 'invalidId')",
        held = held("blob_id")
    ));
    assert!(
        wrong.is_empty(),
        "entries disagreeing with the store: {wrong:?}"
    );
}
