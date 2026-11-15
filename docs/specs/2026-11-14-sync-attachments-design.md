# Device sync: documents and photos — design

Date: 2026-11-14. Status: **approved by the owner on 2026-11-14 and built** (recorded in
[ADR 0030](../architecture/decisions/0030-device-sync-documents-and-photos.md); where the build
differs, see §13).

This is phase 4 of device sync (`2026-11-06-device-sync-client-design.md` §10, §12, §17), which
the owner put off on 2026-11-06 ("same for attachments") and asked for on 2026-11-14: "there is
no reason why everything should not sync" **(owner)**. It is designed for the Node sync server
(`../Moony-relay`, `DESIGN.md`), not for the Cloudflare Worker the old §12 was written for.

Decisions the owner made are marked **(owner)**. Decisions Claude made are marked *(my call)* and
listed again in §12.

The companion design `2026-11-14-sync-aware-app-design.md` covers the rest of the app (first run,
status, backups, wording). Its part C depends on this one.

## 1. Why

With sync on, a person's accounts, transactions and investments are on every device, but the
documents and photos attached to insurance policies and properties stay on the device that added
them. The rows are not synced either, so on the other device the policy simply has no documents.
That reads as data loss, it makes a local backup the only copy of those files, and it means the
app cannot honestly say "your data is on all your devices".

## 2. Goals, non-goals, anti-goals

**Goals**

- G1. A document or photo added on one device is listed on every device of the sync account, and
  its content can be opened there.
- G2. The sync server holds a complete encrypted copy of every attachment, so a person who loses
  every device gets them back with the Sync Recovery Kit.
- G3. A large or failing file never holds up the sync of ordinary data.
- G4. A person can always tell whether an attachment is on this device, on its way, or only
  elsewhere, and how much is still waiting to be sent.
- G5. The server learns nothing new except how many files a profile has and how big they are.

**Non-goals**

- Merging two profiles that each have their own data (D22 stays: a device joins by replace).
- Removing a lost device (phase 5), the Moony cloud, WebSocket push.
- Resumable uploads and downloads. A transfer that breaks starts again *(my call, §12)*.
- Freeing space on a device by dropping content it already has. Nothing is evicted in this work.
- Background transfer while the app is locked or closed. Transfers run while the app is unlocked,
  like the rest of sync.
- Compatibility with app builds that have sync on and do not know synced attachments. Sync is
  not released (`feature::RELEASED` is false), and this ships before it is.

**Anti-goals**

- An attachment that looks present on another device and silently cannot be opened. A row whose
  content is not on the server yet says so, and any device that holds the content uploads it.
- The server being able to read, swap or truncate a file without the app noticing.
- A device deleting, from the server, content that another device still references.
- Sync of ordinary data stopping because of one file.

## 3. What exists today

- Content lives in `attachments.db` (schema `files`, ADR 0017): `files.blobs(id, data, size,
  sha256, created_at)`, one row per document, photo and thumbnail, at most 100 MB each.
  `services/attachment_store.rs` reads and writes it.
- Four tables in the main schema describe attachments: `real_estate_photo_batches`,
  `real_estate_photos` (`blob_id`, `thumb_blob_id`), `real_estate_documents` and
  `insurance_documents` (`blob_id`). All four are `TableClass::Local` in the sync registry
  (`services/sync/registry/tables.rs`).
- Rows from before 0.13 may still carry `file_path` / `thumbnail_path` instead of a blob; a
  conversion after unlock turns them into blobs.
- A join, repair or re-sync replaces the main data and then re-attaches this device's attachment
  rows to items the vault also has (`AttachmentCarry { kept, orphaned }`,
  `services/sync/snapshot/`); the rest stay only in the backup.
- Snapshots already travel as an encrypted stream in 64 KiB chunks
  (`services/sync/envelope/stream.rs`, info string `moony sync snapshot v1`), up to 100 MiB,
  streamed to disk on the server with a temporary name and a rename.
- The server has no file storage for attachments ("Not built: Blob endpoints"). It answers
  `GET /v1/server` with `name`, `version`, `protocol`, `mode`.
- Both the desktop and the phone can add documents and photos.

## 4. Approaches

1. **Rows sync as rows; content travels as one encrypted file per blob (chosen).** The four
   tables become synced tables. Each blob is uploaded once, as a stream sealed like a snapshot,
   to a new per-vault file store on the server, addressed by its blob id. Small, fits the
   existing engine, and one file can fail without touching the rest.
2. **The same, uploaded in parts.** The old §12 split uploads into 32 MiB parts because a
   Cloudflare Worker cannot take a 100 MiB request. The Node server streams any body to disk, so
   parts would only buy resumable uploads, at the cost of a second protocol and a sweep for
   half-finished parts. Not worth it for files that are mostly a few megabytes.
3. **Content inside envelopes or snapshots.** No new endpoints, but the op limit is 192 KiB and
   a snapshot is 100 MiB in all; photos would blow both, and every join would download every
   file before showing anything.

Approach 1 *(my call; the owner asked for the outcome, not the transport)*.

## 5. Design

### 5.1 Rows

- The four attachment tables move from `Local` to synced. Their rows merge like any other row
  (per-column last write wins, tombstones for deletes, parents before children).
- `file_path` and `thumbnail_path` stay local columns. A row that still has no blob (a file
  from before 0.13 that the conversion has not turned into a blob, usually because the file is
  missing) is neither captured nor put into a snapshot. If the conversion later gives it a blob,
  the row must reach the other devices as a complete new row, not as an update to a row they
  never saw; how capture is told so is the builder's. A row received from the vault has a blob
  and no `file_path`; the conversion must leave such rows alone.
- No checksum column is added *(my call)*. The sealed stream is authenticated and bound to the
  blob id (§5.2), and blobs are immutable (new content gets a new id), so the server cannot
  serve other bytes under an id. `files.blobs.sha256` is computed on the receiving device when
  the content is stored, as for a locally added file.
- A snapshot now carries the attachment rows (never content). A device that joins gets the rows
  with the rest and fetches content afterwards (§5.4).
- The re-attach step of a replace (`AttachmentCarry`) goes: after a replace the attachment rows
  are the vault's. Content this device already holds under the same blob id stays and is not
  downloaded again. This device's rows that the vault does not have are gone with the replace
  and remain in the backup, exactly like its other data; the confirmation and the outcome keep
  counting them ("documents and photos that stay only in the backup").

### 5.2 Content on the wire

- One blob is one sealed stream: the snapshot's STREAM construction with its own info string
  (`moony sync blob v1`), keyed from the vault key, with the blob id as associated data. A
  thumbnail is its own blob.
- Key rotation: a blob stays sealed under the key epoch it was uploaded with and names that
  epoch in its header, as envelopes and snapshots do. Blobs are not sealed again when the key
  changes. Older epochs reach a new or recovering device inside the snapshot (client spec D16),
  which is what lets a Recovery Kit open old blobs; nothing new is needed for that.
- Sealing and opening a blob must work piece by piece *(my call)*: today's `stream.rs` takes and
  returns whole buffers, which is fine for a snapshot and not for a 100 MiB file on a phone. The
  blob path reads from and writes to `attachments.db` with incremental blob I/O and streams the
  HTTP body, so memory use stays at a few chunks whatever the file size. Snapshots keep their
  current code.

### 5.3 Upload

- **Rows are never held back for their content** *(my call; this replaces D18's "a row waits for
  its blob" of the client spec)*. An attachment row is captured, sealed and pushed like any other
  row, in its transaction's changeset. Holding it would split changesets and, because a snapshot
  is taken only from an empty outbox, one file that will not upload would stop this device's
  snapshots. The price is a state other devices may see for a while: "not uploaded yet from the
  other device" (§5.6).
- To keep that state rare, a round uploads waiting blobs **before** it pushes ops, oldest first,
  one at a time, each with a time budget so a large file on a slow line does not starve the
  push; what did not fit continues after the push and on later rounds, with a backoff after
  failures. A failed upload never turns the round into a failure. Sync counts as "synchronised"
  only when nothing waits to upload (§5.6).
- A blob leaves the upload queue only when the server has confirmed it.
- Any device that holds the content of a blob the server lacks uploads it (§5.5's daily pass
  finds these), so a file is not lost because the device that added it was wiped after its row
  went out, as long as one device still has it.
- Turning sync on for a profile that already has attachments queues all of them. Setup does not
  wait: sync is on at once and the status shows the upload progress.
- The phone uploads on any connection *(my call)*: a file the person just added is small enough
  compared with the harm of it existing on one device only.

### 5.4 Download

- When a merged or restored row names a blob this device does not hold, the blob is queued.
- Thumbnails are fetched right away on every device.
- Desktop: documents and full photos are fetched in the background, oldest row first, so a
  desktop ends up holding everything.
- Phone: documents and full photos are fetched **when the person opens them** *(my call)*, not
  ahead of time. Reasons: no dependable way to tell Wi-Fi from mobile data in the app today, a
  property with a few hundred photos is hundreds of megabytes, and the phone is rarely where
  they are all looked at. The settings page gets "Download everything to this phone" for a
  person who wants a full copy (it fetches the queue once; it is an action, not a mode).
- A downloaded stream that fails to open (wrong key epoch missing, broken, truncated) is not
  stored; the attachment shows the error state of §5.6 and the download is retried.

### 5.5 Deleting and cleaning up

- Deleting an attachment deletes its row (a synced tombstone) and its local content in one
  transaction, as today. A device that merges the tombstone deletes its own copy. Deleting a
  policy, a property or a photo batch cascades to the attachment rows; the capture triggers
  record cascaded children as tombstones (client spec D4, D13), so they are covered the same
  way.
- **Nothing deletes a blob from the server at the moment of the delete** *(my call)*. The server
  cannot tell which blobs are referenced (it sees only ciphertext), and a row can come back (a
  newer whole row beats a tombstone, D13). So the only thing that removes server blobs is one
  daily pass run by the devices: a device that is fully caught up (no pending pull, nothing
  held, nothing waiting to upload) lists the vault's blobs and
  - deletes those that no row on this device names **and** that are older than 7 days, and
  - uploads those its rows name, the server lacks and this device holds (§5.3).
- Seven days covers a device that uploaded a file and has not pushed its row yet. A device that
  stays unable to push for longer may have its blob removed; the second half of the pass puts
  it back once the row is out. That gap is accepted.
- Deleting the synced data (the app) or a profile or person (the admin) removes the blobs with
  the rest of the vault.

### 5.6 What the person sees

Attachment states, on the item where the attachment is listed (desktop and phone):

| State | Shown as |
|---|---|
| On this device | as today |
| Waiting to upload (added here) | the attachment works as today, with a quiet "waiting to sync" mark |
| Thumbnail here, full content not yet | the thumbnail; opening it downloads with a progress indication |
| Document not here yet | name, type and size as usual, with "Download" on the phone or "Downloading…" on the desktop |
| Not on the server yet (the device that added it has not uploaded it) | "Not uploaded yet from the other device"; fetched by itself once it is there |
| Download failed | "Could not download" with "Try again" |
| Server too old or no room (§6, §7) | the sync status says why; attachments added here work locally |

Sync status (Settings → Sync on both platforms, the top-bar popover and the phone's Overview row
of the companion design):

- "Documents and photos: N waiting to upload" while there are any, with the size.
- On the phone: "N documents and photos are on your other devices" with the "Download
  everything to this phone" action.
- A device is "synchronised" only when nothing waits to upload. Downloads that have not been
  fetched do not count against it (the content is safe on the server).

New problems (existing problem mechanism, own-server wording):

- **No room on the server** (`quota_exceeded` for blobs, or the server's free-space floor): sync
  of ordinary data continues; uploads wait. Text: ask whoever runs the server to give more room.
- **Server needs an update to sync documents and photos** (§6): ordinary sync continues.

### 5.7 Backups

- A local backup still contains what this device holds. On a phone that has not downloaded
  everything, the backup screen says so ("N documents and photos are not on this phone and are
  not in this backup").
- The backups made before a replace keep working as today.

## 6. Server (`../Moony-relay`)

Storage: `blobs/<vault id>/<blob id>` under `DATA_DIR`, written like a snapshot (temporary name,
fsync, rename), with a record in the database (vault, blob id, size, created at).

| Method | Path | Notes |
|---|---|---|
| `PUT` | `/v1/vaults/:vaultId/blobs/:blobId` | Raw body with `Content-Length`; current token and write entitlement, judged before the body is read; streams to disk. Idempotent: a blob that exists answers success without reading the body. Size limit 100 MiB plus the stream's overhead. |
| `HEAD` | `/v1/vaults/:vaultId/blobs/:blobId` | Exists or not, and its size. Read entitlement. |
| `GET` | `/v1/vaults/:vaultId/blobs/:blobId` | Raw bytes. Read entitlement (so a blocked person can still restore for 30 days, and a previous token can read). |
| `GET` | `/v1/vaults/:vaultId/blobs?after=<id>&limit=<n>` | Ids, sizes and creation times, paged. Read entitlement. |
| `DELETE` | `/v1/vaults/:vaultId/blobs/:blobId` | Current token only. Used by the daily pass of §5.5 and by nothing else. |

- `blobId` must be a UUID; anything else is refused before the file system is touched.
- `GET /v1/server` gains `"features": ["blobs"]` *(my call)*. `protocol` stays 2: a server
  without the feature still syncs everything else, and the app says documents and photos need a
  newer server instead of refusing it. A server that sends no `features` field has none. The
  app's parser already ignores fields it does not know (`ServerInfoWire`, `relay/http.rs`), so
  builds from before this work keep working against the new server.
- Quota: bytes of blobs per vault, separate from the op quota. Default 10 GiB, changed in the
  admin's settings for the whole server *(my call; the old D25 number)*. It is the one new admin
  setting: stored in the `settings` table, with an admin API call and a field on the Settings
  page. A vault's use is the sum of its blob records. Past the quota, `PUT` answers the quota
  error.
- Free-space floor *(my call)*: a `PUT` (blob or snapshot) is refused when the data volume has
  less than 1 GiB free, so attachments can never fill the disk that the database lives on.
- Blob uploads and downloads stream and are exempt from the buffered-body budget, like
  snapshots; at most two blob uploads per vault run at once.
- Vault deletion, person deletion and the `pending_wipes` sweep remove the vault's blob
  directory. Housekeeping removes blob files without a record that are over an hour old and
  `*.tmp` leftovers at start, as it does for snapshots.
- Admin: the space figures (overview, person, profile) include blobs; the profile detail shows
  the number of files. The backup archive includes `blobs/`; the backup page says the archive
  can now be large and shows its estimated size before the download.
- "What the server knows" gains: per vault, the number of blobs, each blob's id, size and upload
  time. Sizes are not padded *(my call)*.
- `DESIGN.md` and `README.md` are updated; the ported protocol test suite gets the blob cases.

## 7. App (`Moony-tauri`)

- Registry: the four tables' class and their local columns; snapshot dump and restore; merge
  order (batches before photos, items before their documents).
- A local table for transfer state (which blobs wait to upload or download, attempts, last
  error), added by the next append-only migration. Layout is the builder's.
- `Relay` trait, HTTP client and the fake: the five blob calls and the `features` field.
- Engine: upload (before and after the push) and download steps in the round, the daily pass of
  §5.5, the two new problems. No change to capture, sealing or the snapshot gate beyond the
  tables becoming synced.
- `envelope/`: a piece-by-piece seal and open for blobs (§5.2).
- `attachment_store`: store a downloaded stream as a blob under a given id (today ids are made
  on insert).
- The `moony-file:` handler and the document opener: a row whose content is not local answers
  "not here yet" (a defined state, not the current "missing" error), and opening it starts the
  download.
- Commands and `shared/schema.ts`: attachment state per row, transfer counts in the sync status,
  "download everything", "try again". The seven-step procedure of
  `docs/standards/type-contract.md` applies.
- UI on desktop and phone as in §5.6; every string in all ten locales.
- A new ADR records: synced attachment rows, the blob stream and its binding to the blob id, why
  rows are not held for their content, the device-run daily pass and its seven-day rule,
  on-demand download on phones.

## 8. Security notes for the builder

- The threat model of ADR 0025–0027 holds: the server may drop, delay, replay or refuse. A
  missing blob must show as "could not download", never as an empty or wrong file.
- The blob id in the path is attacker-controlled input on the server and a file name on disk:
  validate it as a UUID and never build a path from anything else.
- The stream must be bound to the blob id and to its vault key epoch, and its last chunk marked,
  so a swap or a truncation fails to open.
- The clean-up deletes server data on a device's say-so. It must run only on a device that is
  fully caught up, and never on a profile whose sync is paused, stopped, held or went back.
- A decrypted download is written only into `attachments.db`; no plaintext temporary file.

## 9. Order of work

1. Server: storage, the five calls, quota, free-space floor, housekeeping, admin figures,
   backup, tests, docs.
2. App engine: registry, migration, relay client, piece-by-piece blob stream, upload, download,
   the daily pass, replace and snapshot changes, problems; convergence and e2e tests against the
   server.
3. App UI: attachment states, status lines, "download everything", strings.
4. With the owner's go-ahead: update the home server and check two real devices (a document
   added on the Mac opens on the iPhone and the other way round; a photo set; a delete; a join
   by a third device).

## 10. Left to the builder

- Table layout for transfer state, backoff numbers, the upload time budget before a push, how
  progress is reported to the UI.
- Whether upload progress inside one file is shown or only the count.
- How capture learns that a converted legacy row is a new row (§5.1).
- Test layout. Required coverage: convergence with attachments on two devices, an upload that
  keeps failing while other data and snapshots go on, a row that arrives before its content, a
  join that fetches content, a delete on one device and by cascade, the daily pass refusing to
  run on a device that is behind and re-uploading a blob the server lost, a tampered and a
  truncated blob, a 100 MiB file within a fixed memory bound.

## 11. Verification

- Unit and convergence tests in `services/sync/` with the in-memory relay fake.
- The e2e suites against a running server (`relay_e2e*.rs`) gain blob cases.
- Server: the protocol suite over HTTP, including quota, the free-space floor (injected),
  path-safety cases and vault deletion.
- Live check on two devices (step 4 of §9), owed by the owner as with earlier phases.

## 12. For the owner to check

Calls Claude made:

1. One streamed upload per file, no parts and no resume; a broken transfer starts again.
2. No checksum column on the rows; the sealed stream bound to the blob id is the integrity check.
3. The phone uploads on any connection.
4. The phone downloads full photos and documents when they are opened, plus a "Download
   everything to this phone" action; the desktop downloads everything in the background.
5. Devices clean up server leftovers in a daily pass: unreferenced and older than 7 days, only
   when fully caught up. Nothing else deletes a file from the server, so a deleted document's
   space is freed about a week later.
11. A row is never held back for its file. Another device can therefore see "not uploaded yet
    from the other device" for a while; in exchange one stubborn file cannot stop sync or
    snapshots.
12. Large files are sealed and opened piece by piece, so a 100 MB document does not need
    hundreds of megabytes of memory on a phone.
6. 10 GiB of documents and photos per profile by default, one setting for the whole server.
7. Uploads are refused when the server's disk has less than 1 GiB free.
8. A server without the feature keeps syncing the rest; the app says documents and photos need a
   newer server.
9. File sizes are visible to the server as they are (not padded).
10. This ships before sync is released, so there is no handling of older sync-enabled builds.

What "everything" does not include, and stays per device unless the owner says otherwise: the
language, the theme, which menu items are shown, notification settings, biometric unlock, the
device password and recovery key, the AI assistant (MCP) settings, and local backups. Prices,
exchange rates and the portfolio history are not synced because each device downloads or
recomputes them.

## 13. As built (2026-11-15)

The sections above are the design as approved and are not rewritten. Where the build differs
(details in ADR 0030):

- §5.1: profiles that had sync on before this build get no one-time enrolment of their
  attachment rows; it was built and then removed. A development vault that synced before this
  build does not carry the documents and photos that existed then and must be set up again.
- §5.1: a replace does not drop this device's content when the vault lacks its row. It holds the
  content of every blob this device's rows named (`sync_held_blobs`) until the first pull after
  the replace has caught up to the server's head, because a row pushed after the vault's newest
  snapshot comes back with that pull. Only then are the blobs no row names swept.
- §5.3: the round's upload step before the push sends only content that never failed an attempt,
  each blob at most 8 MiB, 20 seconds in all; everything else goes through two transfer lanes
  beside the rounds.
- §5.3: two upload outcomes the design did not name: `refused` (the server refuses the blob for
  good: too large, empty, a length it cannot take) and `conflict` (the server holds other content
  under the blob's ID and never overwrites it). Both wait for "Try again" and count as failed,
  never as waiting to upload.
- §5.4: on the phone, a file type it cannot show is never downloaded when it is opened and its
  row offers no download; it says the file is on the other devices. "Download everything to this
  phone" still fetches it.
- §5.5: the daily pass is not held back by uploads that wait for room on the server or that are
  stuck for good, since the pass may be what frees the room and never deletes a blob a row here
  names. A pass that started and did not complete is tried again after an hour.
- §5.5: blob ages are measured on the server's clock (the listing's `now`); a blob dated before
  2020-01-01 or in the server's future is never old enough to delete.
- §5.5: the pass has a budget: 60 seconds, at most 100 pages of the listing and at most 1,000
  deletes. A vault of more than 100,000 blobs therefore never completes a pass; it still deletes
  among the pages it lists, but never re-queues uploads.
- §5.6: the status and the listings answer where content is from the transfer queue alone, never
  from the store, and the store gets no index. Content lost from the store outside a merge shows
  as here until the next start of the sync engine.
- §5.6, §5.7: the sync status carries one more count, `notOnDevice` (rows with any content not on
  this device, for any reason), which the backup screen uses for "not in this backup".
- §7: the file handler answers `425 Too Early` for content on its way (including content that
  could not be opened yet, which is retried) and `404` with `attachmentUnavailable` only for
  content that cannot arrive as things stand (refused, a conflict, an invalid ID).
