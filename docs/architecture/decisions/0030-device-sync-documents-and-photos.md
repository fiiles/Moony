# 0030. Device sync: documents and photos

Date: 2026-11-15

## Status

Accepted. Records the decisions of `docs/specs/2026-11-14-sync-attachments-design.md` (app side)
and its plan `docs/plans/2026-11-14-sync-attachments-plan-2-app.md`, built on
`feat/sync-attachments`. Builds on [ADR 0017](0017-encrypted-attachments-store.md) (the store),
[ADR 0024](0024-device-sync-capture-clocks-merge.md) (capture and merge) and
[ADR 0027](0027-device-sync-audit-fixes.md), whose "Documents and photos survive a replace (H3)"
it replaces. The sync server side is the Moony-relay repository (`DESIGN.md` there is
authoritative). Sync is still hidden behind the runtime flag (`feature::RELEASED` is `false`).
Later tasks of the plan append "As built" notes below; where they differ, the code and those notes
win.

## Context

With sync on, a person's accounts and transactions were on every device, but the documents and
photos of policies and properties stayed on the device that added them: the four attachment tables
were Local, and a replace (join, repair, re-sync) kept them aside in `TEMP` tables and put them
back under the items the vault had. On another device the policy simply had no documents, a local
backup was the only copy of those files, and the app could not say "your data is on all your
devices". The owner asked on 2026-11-14 for everything to sync. Content is up to 100 MiB per file
(`attachment_store::MAX_DOCUMENT_BYTES`), far over the 192 KiB op limit and the 100 MiB snapshot,
so it cannot ride in envelopes or snapshots.

## Decision

**Rows**
- `real_estate_photo_batches`, `real_estate_photos`, `real_estate_documents` and
  `insurance_documents` are Synced: per-column clocks, tombstones, parents before children,
  cascaded deletes stamped with the parent's clock, exactly as every other synced table.
  `file_path` and `thumbnail_path` are Local columns.
- A row syncs only while it names its blobs: `TableSpec.requires` is `["blob_id"]` on the two
  document tables and `["blob_id", "thumb_blob_id"]` on `real_estate_photos`, whose CHECK on the
  receiving device takes both blobs or both paths (`registry::row_condition`). Capture, the
  journal reader and the snapshot dump all apply it, so a legacy row (a plain file from before
  0.13 the conversion has not turned into a blob) is never captured, read or dumped, and its edits
  and deletes record nothing.
- A legacy row that gets its blobs enrols as a new row: the `sync_<t>_enrol` trigger (the row
  condition false → true, local mode) gives it a fresh `'*'` with this device, drops its column
  clocks and queues it, so it travels as a complete insert. `sync_<t>_leave` (true → false)
  records a tombstone. The conversion leaves a row that already has a blob alone.
- No checksum column. The sealed stream is authenticated and bound to the blob ID, and blobs are
  immutable (new content gets a new ID); `files.blobs.sha256` is computed on the receiving device
  when the content is stored.

**Content on the wire**
- One blob is one sealed stream: the snapshot's STREAM construction with its own info string
  `moony sync blob v1`, keyed from the vault key, the blob ID (16 raw UUID bytes) as the object ID,
  64 KiB chunks, the last chunk flagged, the key epoch in the header. A blob is sealed once and
  never sealed again when the key rotates; older epochs reach a new device inside the snapshot.
- Sealing and opening work piece by piece (`envelope/blob.rs`) and stream between
  `attachments.db` (incremental blob I/O; an incoming blob stays invisible until its hash is
  written) and the HTTP body, so memory stays at a few chunks whatever the file size.
- One streamed `PUT` per blob, no parts and no resume: a broken transfer starts again. A `HEAD`
  comes before a `PUT`, so a blob the server already has is confirmed without a body.

**Rows are never held for their content.** An attachment row is captured, sealed and pushed in its
transaction's changeset like any row. Holding it would split changesets, and because a snapshot is
taken only from an empty outbox, one file that will not upload would stop this device's snapshots.
The price is a state other devices may see for a while: "not uploaded yet from the other device".

**The transfer queue.** `sync_blobs` (Local, migration 035) holds one row per referenced blob
(`up`, `down`, `done`), reconciled from the rows and the store, never from hooks in the services.
Turning sync off empties it; a replace empties it too (the rows are the vault's afterwards) and a
later step rebuilds it from the rows and the store.

**Uploads.** Before a round pushes it uploads waiting blobs of at most 8 MiB, oldest first, within a
20 s budget; everything else goes through two lanes (uploads, downloads) that run beside the rounds
with a backoff after failures. A failed upload never fails a round. A blob leaves the upload queue
only when the server has confirmed it. A device counts as synchronised only when nothing waits to
upload.

**Downloads.** Thumbnails are fetched at once on every device. A desktop fetches documents and full
photos in the background, oldest row first; a phone fetches them when the person opens them, or
all at once with "Download everything to this phone" (an action, not a mode). A stream that fails
to open is not stored; the attachment shows "Could not download" and the download is retried.

**The daily pass.** Nothing deletes a server blob at the moment of a delete. Once per 24 h, a device
that is fully caught up (pull done, outbox and unsent empty, nothing waiting to upload, no pause,
guard, went-back mark, miss or quarantine, and a server with the `blobs` feature) lists the vault's
blobs and deletes those no local row names that are older than 7 days by the server's clock (the
listing's `now`, never the device's; without it nothing is deleted), at most 1,000 per pass; it
re-queues for upload the blobs its rows name, the server lacks and this device holds.

**Replaces keep content by blob ID, and hold the rest until the next pull caught up.** After a
join, repair or re-sync the attachment rows are the vault's. Content this device holds under a
blob ID a vault row names stays and is not downloaded again. A replace also holds the content of
every blob this device's rows named before it (the Local table `sync_held_blobs`, migration 036,
written inside the replace's savepoint): `attachment_store::delete_unreferenced` keeps every held
blob (`attachment_store::HOLDS`), so neither the repair after each changeset, nor the sweep after
the next unlock, nor the maintenance command removes it. The first pull after the replace that
reaches the sync server's end with nothing stopped or held (`PullReport::done`) releases every
hold and sweeps; only then do the blobs no row names go. The reason is audit H3: a row this
device pushed after the vault's newest snapshot (the re-sync pushes before it replaces) is not in
the snapshot, so the replace drops it and the next pull brings it back; without the hold its
content was swept in between, and if its upload had not finished, only the backup had it.
Turning sync off drops the holds. `RestoreReport.attachments` counts this device's documents and
photos as before (`kept` if the replace brought them back, else `orphaned`, "only in the
backup"; batches are not counted).

**The server.** `GET /v1/server` announces `features: ["blobs"]` (an absent field means none);
`protocol` stays 2, so a server without the feature still syncs everything else and the app says
documents and photos need a newer server. The app assumes the server's error codes as built:
`invalid_blob_id`, `vault_not_found`, `unauthorized`, `token_read_only`, `payment_required`,
`length_required`, `blob_too_large`, `empty_body`, `quota_exceeded`, `server_full`,
`too_many_uploads`, `incomplete_body`, `body_timeout`, `blob_not_found`, `invalid_limit`,
`internal_error`, each also in the `Moony-Error` header.

## Consequences

- Every existing guarantee of ADR 0024 holds for the four tables; a new attachment table must get a
  class and, if its rows can exist without content, a `requires` column.
- `snapshot::attachments::{keep, reattach, discard}` and the `temp.sync_kept_*` tables are retired
  (`docs/DEPRECATED.md`); `mine` and `carry` only count.
- Known limits, accepted:
  - no resume: a broken transfer of a large file starts again from the first byte;
  - starting or abandoning the download of a very large file holds the database for as long as
    allocating or freeing it takes (the incoming blob is reserved whole, as incremental blob I/O
    needs); every other step of a transfer holds it for one chunk;
  - a row that stays unpushed for more than 7 days after its blob went up may have its blob
    deleted by another device's pass; the next pass on a device that holds it puts it back once
    the row is out;
  - a blob whose only holder is wiped before it uploads is lost; the row stays and shows "not
    uploaded yet from the other device" for good;
  - file sizes are visible to the server as they are (not padded);
  - the "only in the backup" count of a replace can overstate: a row pushed after the vault's
    newest snapshot is counted as orphaned, and the next pull brings it back with its held
    content (the backup is still marked kept, as before);
  - a hold lasts until a pull catches up; while a gap or a quarantine holds the pull, held blobs
    stay on the device (space only, never data);
  - while capture is broken, the guard (ADR 0027) refuses writes to the four tables as to any
    synced table, legacy rows included, so the conversion of legacy files cannot run there; a
    repair is the way out, and a repair replaces, so legacy rows that were never converted go
    with it (their files stay in the kept backup);
  - a vault created by a development build before documents and photos synced does not carry the
    attachments that existed then (nobody clocked or queued them), and an edit to such an old
    attachment row can reach devices that never received the row; such a vault must be set up
    again (sync off, set up, join the other devices again). No released build can be in that
    state: sync is unreleased and ships with this ADR's migrations.
- Release guard: if sync is ever released from a build without this ADR's migrations (035, 036),
  which it must not be, profiles that synced on it would need a one-time enrolment of their
  attachment rows, designed so it never overwrites changes other devices made after it.

## As built

- **The transfer queue (Task 7).** `services/sync/blobs/queue.rs` reconciles `sync_blobs` from the
  rows and the store with set-based SQL over `attachment_store::REFERENCES`, limited to rows that
  meet their row condition (`registry::row_condition`), so a row that never syncs never queues its
  blob. "Held here" has one definition, `attachment_store::complete_sql` (hash recorded, content at
  its recorded size), for the queue, the content reader and `rows_without_content`, so a truncated
  blob is neither uploaded nor shown as here. Each merged changeset that touched an attachment row
  table or lost rows of an owner runs `reconcile_rows` (adds and drops; a merge never adds or
  removes content of a blob a row still names); the whole `reconcile`, which checks every entry
  against the store, runs once at the end of a pull that merged anything and after every restore
  (`repair::full`), because reading a blob's `size` and `sha256` means walking past its content
  (they follow `data` in `files.blobs`). A blob held here is queued `up` unless the queue knows the
  server has it (`done`), so content the queue lost track of is confirmed with the server (a
  `HEAD`, no body when it is there) rather than assumed uploaded: after a replace every held blob
  is `up`, a held blob whose row the next pull brings back is `up`, and a `down` entry whose
  content is found here again becomes `up` (the plan had `done`: losing the copy forgot whether it
  was ever uploaded). The download lane marks what it stored `done` itself. `ContentReader` gives
  each blob an `AttachmentContent`; a row that does not meet its row condition reads as `Here`,
  never as waiting to upload. A `down` entry is not content: `rows_without_content` keeps counting
  it, so the quarantine of converted originals and the integrity check never rely on content that
  may exist nowhere; only the conversion status leaves out content expected by download
  (`rows_without_content_or_download`).
- **Moving one blob (Task 10).** `services/sync/blobs/transfer.rs` uploads (`HEAD` first, a body
  only when the server lacks the blob) and downloads one blob at a time; `policy.rs` holds what
  the round's upload step and the lanes share: the server's `blobs` feature (asked at most every
  six hours; a failed ask is no problem), the attachments problem (`SyncAttachmentsProblem`:
  `noRoom` pauses uploads for half an hour, downloads go on; `serverNeedsUpdate` only while content
  waits), one lane lock per direction and the wake-up. Blob answers are classified there, never by
  the round's `SyncError`, so no single file fails, pauses or slows a round: done; gone (no syncing
  row names the blob, or its content left the store under the upload: the entry is dropped and a
  download that completed after its row went is deleted again); failed with a backoff of that
  blob's own; blocked by what the round already shows (`402`, `401`, `403 token_read_only`, vault
  not found: nothing recorded); a locked profile ends the run as `AppError::Auth`. The HTTP client
  reports any body error as network trouble, so the upload keeps its source's error beside the
  call and a vanished or unreadable blob is never read as "offline". A blob ID that is not
  canonical never reaches a URL or the store. A run stops at the first network, server or no-room
  failure (the next blob would meet it too; the rest waits for the next run) and between blobs
  when its deadline passed.
