# Encrypted attachments — design

Date: 2026-10-15. Status: **approved** by the owner in chat on 2026-10-15 (decisions D1–D13
below). ADR 0017 (`docs/architecture/decisions/0017-encrypted-attachments-store.md`) records the
storage decision and is Accepted. The owner chose variant C (a
second SQLCipher database) in chat on 2026-10-15, after an analysis of the plaintext data on
disk. This spec is also the hand-off brief for whoever implements it: every rule an implementer
needs is stated here. The plan is `docs/plans/2026-10-15-encrypted-attachments.md`.

## 1. Problem

Moony promises an encrypted local database, but only `moony.db` is encrypted. As of
`d25506eb`:

| Data | Where | How it is read |
|---|---|---|
| Real estate photos and 400 px thumbnails | `real_estate_photos/<property>/<batch>/<uuid>.jpg`, `<uuid>_thumb.jpg` | The webview loads them from disk through the `asset:` protocol (`assetProtocol` scope in `tauri.conf.json`); single-photo and batch downloads use the webview's `fs:allow-copy-file` permission. |
| Real estate documents | `real_estate_documents/<property>/<uuid>.<ext>` (older files: `<uuid>_<original name>`) | `open_real_estate_document` opens the file in the system app. |
| Insurance documents | `insurance_documents/<policy>/<uuid>.<ext>` (older: `<uuid>_<original name>`) | `open_insurance_document`, same way. |
| Backups | `.zip` chosen by the user | The DB inside is encrypted; the attachment folders are copied as plain files, and `manifest.json` lists their paths. |

Related facts:

- Deleting a property or a policy deletes its attachment rows (FK cascade and explicit
  `DELETE`s) but leaves the files on disk. That is a bug today.
- Stored document paths can point outside the current data dir (the identifier changes of
  0.9.0 and 0.11.0); `services/document_files.rs::resolve_document_path` re-roots them.
- PRIVACY.md, SECURITY.md and the README qualify the encryption claim; ROADMAP.md lists
  encrypted attachments as item 2.

Already fixed in `d25506eb`: photo uploads no longer pass through `$TMPDIR` (they arrive as a
raw IPC body), new documents no longer carry the original file name on disk, and onboarding no
longer sends the master key through the webview. `3d8ed6ee` removes the plaintext photo copies
that older versions left in the temp dir.

## 2. Goals and non-goals

**Goals**

- G1. Every attachment byte at rest is encrypted with the profile's master key: photos,
  thumbnails, documents, and their copies in backups.
- G2. The webview never reads attachment files from disk; it gets bytes only from the Rust side,
  only while the profile is unlocked.
- G3. Existing profiles are converted automatically and safely. Nothing is lost when the app is
  interrupted, and an original is not deleted until its encrypted copy has been read back and
  verified.
- G4. Backups are fully encrypted, and backups of earlier versions stay restorable.
- G5. The docs can say "everything you store in Moony is encrypted" and list only the honest
  exceptions (§5).

**Non-goals**

- Encrypting JSON or CSV exports (they are plaintext on purpose, chosen by the user).
- The greeting name in `localStorage` (the lock screen reads it before unlock, on purpose).
- Removing plaintext that is already in Time Machine, APFS snapshots, SSD blocks, or folders
  the app does not own. The app reports what it knows about (§4.8); the docs explain the rest.
- MCP access to attachment content. MCP keeps returning metadata only.
- Re-wrapping `key.enc` at the current scrypt cost (the follow-up ADR 0009 left open). It
  matters more once backups are fully encrypted, but it is a separate change (§7).
- Deduplication, video, and attachments for other domains.

## 3. Owner decisions (approved 2026-10-15)

| # | Decision |
|---|---|
| D1 | Attachment content lives in `attachments.db`, attached to the main connection as schema `files` without a `KEY` clause (ADR 0017). |
| D2 | Everything goes into the store: photos, thumbnails, real estate documents, insurance documents. After conversion the attachment folders are gone. |
| D3 | Images and PDFs open **in the app**: a viewer dialog with `<img>` for images and pdf.js for PDFs. Every other type opens in its system app through a decrypted temporary copy in the app cache folder. The copy is deleted on lock, on exit and at the next start. Before the first such open, a dialog explains the temporary copy; it has a "don't ask again" option. |
| D4 | Conversion of plain files starts automatically in the background after the first unlock of the new version. Converted originals inside the data dir move to `attachments-converted/`, which is deleted at the next unlock once the store passes its checks. |
| D5 | Originals **outside** the current data dir (stale paths from old identifier folders) are converted but never deleted; they are listed as leftovers in Settings → Data with a "Show in Finder" action. The app also flags old identifier data folders that still hold attachment folders. It never touches any other folder (for example a hand-made copy). |
| D6 | Backup format 2 is fully encrypted: `moony.db`, `attachments.db`, key files, and plain legacy files only while some are still unconverted. Format 1 archives stay restorable and are converted after unlock. |
| D7 | Documents are capped at **100 MB**. Photos have no cap: they are re-encoded at no more than 2560 px. |
| D8 | Deleting a property, policy, batch, photo or document deletes its blobs in the same transaction. After unlock, a sweep deletes blobs no row references. |
| D9 | `files` uses `secure_delete = ON` and `auto_vacuum = INCREMENTAL`; `PRAGMA files.incremental_vacuum` runs after the sweep. |
| D10 | Losing both the password and the recovery key now loses the attachments too. The recovery key screen, the release notes and PRIVACY.md say so. |
| D11 | If `attachments.db` cannot be attached (damaged, or from another profile), the app still unlocks. Attachments are unavailable, the file is never overwritten, and Settings → Data shows the problem. Uploads and conversion are refused until it is resolved. |
| D12 | Downgrading after conversion is unsupported: an older build shows converted attachments as missing. The release notes say so. |
| D13 | The feature ships for everyone. It is not a paid add-on (a `Moony-launch/strategy.md` idea), because it closes a privacy gap in an AGPL app. |

## 4. Design

### 4.1 The store: `attachments.db`

- Path: `<app data dir>/attachments.db`, next to `moony.db`.
- `Database::open_with_key` and `Database::create_with_key` run
  `ATTACH DATABASE ?1 AS files` (no `KEY`) right after the main key is verified and before
  the post-open steps. A missing file is created encrypted (verified by the probe in ADR 0017).
- Failure (D11): if `ATTACH` fails, log a warning without the path, keep the main connection,
  and record `store_available = false` on the `Database` (cleared on close). Every store call
  then returns `AppError::Internal("attachments.store_unavailable")`, and the protocol answers
  503.
- Schema `files`, version 1, applied when `PRAGMA files.user_version` is 0:

  ```sql
  PRAGMA files.auto_vacuum = INCREMENTAL;   -- must run before the first table exists
  CREATE TABLE files.blobs (
      id         TEXT PRIMARY KEY,           -- UUID v4
      data       BLOB NOT NULL,
      size       INTEGER NOT NULL,
      sha256     TEXT NOT NULL,              -- lowercase hex of data
      created_at INTEGER NOT NULL DEFAULT (unixepoch())
  );
  PRAGMA files.user_version = 1;
  ```

  Then, on every open, `PRAGMA files.secure_delete = ON`.
- The `files` migrations are an append-only list in a new module next to `migrations.rs`
  (for example `db/files_schema.rs`), keyed by `user_version`. A pending entry first copies
  `attachments.db` to `attachments.db.bak-<ts>` with the same helper and pruning as
  `backup_database_file`. ADR 0017 records this as the one exception to "every schema change is
  a numbered migration in `migrations.rs`".
- Closing the connection (lock, logout, restore) detaches it. Nothing else holds the key.

### 4.2 Main schema: migration 021

`021_encrypted_attachments` rebuilds the three attachment tables (create new → copy → drop →
rename → recreate the index; see the migration playbook). No table references them, so dropping
them is safe with foreign keys on.

- `insurance_documents` and `real_estate_documents`: `file_path` becomes nullable (a legacy
  plain file, NULL once converted). New nullable columns: `blob_id`, `mime_type` and
  `original_name`. Add `CHECK (file_path IS NOT NULL OR blob_id IS NOT NULL)`.
- `real_estate_photos`: `file_path` and `thumbnail_path` become nullable. New `blob_id` and
  `thumb_blob_id`. Add `CHECK ((file_path IS NOT NULL AND thumbnail_path IS NOT NULL) OR
  (blob_id IS NOT NULL AND thumb_blob_id IS NOT NULL))`.
- New table `attachment_leftovers(id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE, found_at
  INTEGER NOT NULL DEFAULT (unixepoch()))`. It lives in the encrypted DB, so the paths it
  stores are protected.
- Existing rows are copied unchanged (all legacy). The schema snapshot is regenerated, and
  `database.md` documents the columns and the `files` schema.

The legacy `real_estate.photos` JSON column is not touched.

### 4.3 Store service

A new service module (for example `services/attachment_store.rs`) takes `&Connection` (rule 4)
and knows nothing about owners:

- `put_bytes(conn, bytes) -> BlobRef { id, size, sha256 }`.
- `put_reader(conn, reader, len) -> BlobRef`: `INSERT … zeroblob(len)`, then `blob_open` on
  `files.blobs.data` and copy in 64 KiB chunks, hashing on the way. Used for documents and
  conversion, so a large file is never held twice in memory.
- `read_all(conn, id) -> Vec<u8>` (protocol), and `copy_to(conn, id, writer)` (exports,
  external open), both via `blob_open` read-only.
- `delete(conn, id)`, `delete_unreferenced(conn) -> usize` (D8; references are the `blob_id` /
  `thumb_blob_id` columns of the three tables), and `verify(conn, id, sha256) -> bool`.
- `sniff_mime(name, first_bytes) -> &'static str`: magic bytes for JPEG, PNG, GIF, WebP and
  PDF, then the extension map, falling back to `application/octet-stream`. No new crate.

Callers wrap a blob write and the row write in **one** transaction (`unchecked_transaction`
on the shared connection, as elsewhere). The probe showed a cross-schema transaction rolls back
in both schemas. The cap (D7) is checked before any write:
`AppError::Validation("validation.attachmentTooLarge")`.

### 4.4 Write paths

- **Photo upload** (`add_photo_to_batch`, raw body): `services/real_estate_photos.rs` decodes
  and resizes as today, but encodes the photo and the thumbnail to JPEG **in memory**. Inside one
  `with_conn` it puts both blobs and inserts the row with `blob_id` / `thumb_blob_id` and NULL
  paths. `StoredPhoto::discard` and the per-batch folder go away.
- **Document add** (`add_real_estate_document`, `add_insurance_document`): the path still comes
  from the native dialog. The service checks the size and streams the file with `put_reader`
  in the same transaction as the row (`mime_type`, `original_name` = the file name, `file_size`,
  `file_path` NULL). `Insert*::validate()` stays first.

### 4.5 Delete paths

- Photo, batch, document: one transaction deletes the rows and their blobs.
- Property (`delete_real_estate`) and policy (`delete_insurance`): collect the blob ids of
  every attachment row that will cascade (documents, and the photos of every batch), then
  delete the blobs and the owner in one transaction. Legacy files of those rows are deleted
  from disk after the commit. This also fixes today's orphaned files.
- The business logic moves from the commands into services (rule 4); the commands stay thin.
- The unlock sweep (`delete_unreferenced`, then `incremental_vacuum`) runs after the
  conversion step (§4.8).

### 4.6 Reading: the `moony-file:` URI scheme

- Registered with `register_asynchronous_uri_scheme_protocol("moony-file", …)`. The handler
  moves the work to a blocking task and takes the managed `Database`.
- URL path `/<kind>/<id>`, where kind is `photo`, `photo-thumb`, `real-estate-document` or
  `insurance-document` and id is the **row** id. The frontend builds the URL with
  `convertFileSrc(path, 'moony-file')`, so the platform form (`moony-file://localhost/…`
  vs `http://moony-file.localhost/…` on Windows) is Tauri's concern. Route parsing is a pure
  function with unit tests.
- Responses:
  - 200 with the body from the blob, or from the legacy file for a row not yet converted.
  - 403 while locked.
  - 404 for an unknown row or a missing blob or file.
  - 405 for anything but GET.
  - 503 when the store is unavailable.
- Headers on every 200:
  - `Cache-Control: no-store`
  - `X-Content-Type-Options: nosniff`
  - `Content-Security-Policy: sandbox; default-src 'none'`
  - `Content-Type`: the stored MIME type, but only for `image/*` and `application/pdf`.
    Everything else is `application/octet-stream`, so an attached HTML or SVG file can never
    run as a page.
- `tauri.conf.json`:
  - Drop `assetProtocol` and the `protocol-asset` feature of the `tauri` crate.
  - CSP: add `moony-file: http://moony-file.localhost` to `img-src` and `connect-src` (pdf.js
    fetches the bytes).
- Capabilities: drop the `fs:allow-copy-file` grant for `real_estate_photos`, and add
  `$APPDATA/attachments.db*` and `$APPDATA/attachments-converted/**` to every deny list that
  remains.

### 4.7 Viewing, exporting, opening elsewhere

- **Viewer dialog** (shared by both document lists and the photo lightbox):
  - Images: `<img src>` on the protocol URL.
  - PDF: pdf.js (`pdfjs-dist`, loaded lazily) renders pages to canvas from bytes fetched over
    the protocol, with `isEvalSupported: false`. Its worker is a bundled asset. Task 0 of the
    plan checks pdf.js under the app's CSP; if it needs `'wasm-unsafe-eval'` for some image
    codecs, that is the only CSP widening allowed. Without it, such PDFs fall back to
    "open elsewhere".
  - Actions: "Uložit kopii…" and "Otevřít v jiné aplikaci".
- **Save a copy:** a new command `export_attachment(kind, id, dest)` takes the path the user
  picked in a save dialog and streams the decrypted bytes there with `copy_to`. The default
  file name comes from `original_name` (sanitized) or the document name plus the extension of
  its MIME type.
  - Single-photo download uses it too.
  - Batch download becomes `export_photo_batch(batch_id, dir)`. It keeps today's naming:
    `batch_<date>_<id8>.jpg`.
- **Open elsewhere:** `open_real_estate_document` and `open_insurance_document` keep their
  names.
  - They write the decrypted copy to `<app cache dir>/open/<random>/<sanitized original
    name>`: folder 0700, file 0600 on Unix.
  - Then they open it with `document_files::open_with_system`.
  - `open/` is wiped on lock (`auth::logout` and auto-lock), on `RunEvent::Exit` and at
    startup. On Windows a file the other app still holds is skipped and wiped next time.
  - The first open shows the D3 dialog. Its "don't ask again" flag is a per-device
    `localStorage` key, like the other device settings.
- The frontend no longer needs `appDataDir()` for attachments.

### 4.8 Converting plain files from earlier versions

**When.** After every successful unlock, a background task pinned to the session
(`Database::current_session()`, so a lock or restore stops it) runs these steps in order:

1. Finish the quarantine.
2. Convert.
3. Sweep and vacuum (§4.5).
4. Refresh the leftovers.

It never blocks unlock. While it runs, the protocol serves legacy rows from disk, so the UI
works throughout.

**Finish the quarantine.** If `attachments-converted/` exists, `PRAGMA files.quick_check`
returns `ok`, and every converted row's blob exists with its recorded size, delete the folder.
Otherwise keep it and log a warning.

**Convert, one item at a time** (a document, or a photo with its thumbnail):

1. Resolve the legacy file: `resolve_document_path` for documents, the data-dir-relative path
   for photos. If it is missing, count it as missing and continue.
2. Read the file outside the connection lock (documents above the cap are converted anyway).
   For documents, compute `mime_type` and `original_name`; strip a legacy `<uuid>_` prefix.
3. In one `with_conn` call:
   - in one transaction, put the blob(s), update the row (`blob_id`, `mime_type`,
     `original_name`, `file_path` NULL), and read every blob back through `blob_open`,
     comparing its `sha256` with the source bytes. A file outside the current data dir also
     gets its path inserted into `attachment_leftovers` in this transaction. Commit only if
     every hash matches. A mismatch rolls the whole transaction back and counts the item as
     failed; the next unlock retries it.
   - after the commit, if the file is inside the current data dir, move it into
     `attachments-converted/`, keeping its relative path.

   Doing the commit and the move inside one `with_conn` keeps a concurrent backup consistent
   (§4.9).
4. When every row is converted, remove the attachment folders if they are empty.

**Leftovers** (D5):

- The paths in `attachment_leftovers` that still exist. Rows whose path is gone are dropped
  on refresh.
- Plus every sibling of the app data dir named after a former identifier (`com.filipkral.moony`,
  `com.filipkral.moony-tauri`; keep the list next to the identifier history) that contains one of
  the three attachment folders.
- The status shows the folder path, not the files in it.
- Nothing in either list is ever deleted by the app.

**Status**: a new command `get_attachment_status` returns
`{ storeAvailable, stored, legacyPending, missing, failed, converting, leftovers: string[] }`.
The conversion emits an event when it finishes, so the Data settings page refreshes.

### 4.9 Backup, restore, integrity, delete account

- **Create** (format 2), inside the single `with_conn` that `create_backup` already holds:
  - `VACUUM main INTO` and `VACUUM files INTO` to temp snapshots;
  - zip `moony.db`, `attachments.db`, `salt`, `key.enc`, `recovery.enc`;
  - plus the plain attachment folders **only if** legacy files remain (conversion pending or
    failed);
  - the manifest gains `attachmentCount` (the number of attachment rows), so inspection needs
    no key.

  Holding the lock keeps the two snapshots and the legacy folders consistent with each other.
- **Inspect**:
  - `classify` treats root `attachments.db` as a database entry. A format 2 archive without it
    is invalid.
  - `attachmentCount` comes from the manifest for format 2, and from counting files for
    format 1.
  - Older builds reject format 2 on both the format number and the unknown entry.
- **Restore**: `managed_names` gains `attachments.db`, `attachments.db-journal` and
  `attachments-converted`, so they move aside into `pre-restore-<ts>/` and roll back like the
  rest. A format 1 archive restores legacy folders, and the next unlock converts them.
- **Integrity** (`verify_database`): also runs `PRAGMA files.quick_check` and
  `PRAGMA files.cipher_integrity_check`, with messages prefixed "attachments:". When the store
  is unavailable, it reports that.
- **Delete account**: also removes `attachments.db`, its journal and `.bak-*` copies,
  `attachments-converted/` and the cache `open/` folder.
- **Data location**: unchanged (it already sums the data folder).
- **JSON export**: unchanged. `files` is a separate schema, so `blobs` is not exported; the
  new columns are.

### 4.10 Wire contract (7-step procedure, `type-contract.md`)

- `RealEstatePhoto`: remove `filePath` and `thumbnailPath`.
- `RealEstateDocument`, `InsuranceDocument`: remove `filePath`; add `mimeType: string | null`
  (null only for legacy rows not converted yet; the viewer then sniffs nothing and offers
  "open elsewhere").
- `BackupManifest`: add optional `attachmentCount?: number`.
- New `AttachmentStatus` (§4.8) and `AttachmentKind = 'photo' | 'real-estate-document' |
  'insurance-document'`.
- New commands, each wrapped in `src/lib/tauri-api.ts`: `get_attachment_status`,
  `export_attachment`, `export_photo_batch`.

### 4.11 UI and copy

Czech wording is the reference. All ten locales get every key (rule 8).

| Where | Copy (cs) |
|---|---|
| Viewer actions | „Uložit kopii…“, „Otevřít v jiné aplikaci“ |
| D3 dialog | Title „Otevřít mimo Moony?“. Body „Moony vytvoří dočasnou nešifrovanou kopii souboru, aby ji mohla otevřít jiná aplikace. Kopii smaže při zamčení nebo zavření Moony. Co si ta aplikace uloží sama, smazat neumí.“ Checkbox „Příště se neptat“. |
| Recovery key screen (D10) | „Bez hesla i tohoto klíče nejde otevřít nic, ani přiložené fotky a dokumenty.“ |
| Settings → Data, status | „Přílohy jsou uložené šifrovaně ({{count}}).“ / „Šifruji přílohy z dřívější verze… ({{done}} z {{total}})“ / „{{count}} příloh chybí na disku.“ / „{{count}} příloh se nepodařilo zašifrovat. Zkusím to znovu při dalším odemčení.“ |
| Settings → Data, leftovers | Title „Nešifrované kopie mimo Moony“. Body „Moony je při šifrování našla. Mimo svou složku nic nemaže; zkontrolujte je a smažte ručně.“ Action „Zobrazit“. |
| Settings → Data, D11 | „Přílohy teď nejde otevřít: soubor attachments.db je poškozený nebo nepatří k tomuto profilu. Obnovte zálohu, nebo kontaktujte podporu.“ |
| Validation | „Soubor je větší než 100 MB.“ |

The photo gallery and the document lists look the same as today. Only the open action changes:
the in-app viewer for images and PDFs, the D3 dialog for other types.

### 4.12 Tooling

- `seed_demo`: seeds a property with one photo batch (two generated JPEGs) and a PDF document,
  plus a policy with a PDF, all through the store. A `--legacy-attachments` flag instead writes
  plain files and legacy rows the way 0.12 did, to exercise the conversion.
- `local_anonymize_profile`: opens `moony.db` without attaching `files` and keeps deleting the
  attachment rows. It must never copy `attachments.db` (the header comment says so).
- Dev browser bridge: a dev-only HTTP route answers `moony-file` URLs by relaying to the
  running app, and its `convertFileSrc` mock maps to that route.

### 4.13 Living docs to update

- PRIVACY.md: the "what is not encrypted" table shrinks to exports, open-elsewhere copies,
  the greeting name and OS-level history; add D10.
- SECURITY.md: remove "attachments are stored unencrypted" from out of scope.
- README: the encryption claim.
- ROADMAP: remove item 2.
- `docs/architecture/database.md` (the `files` schema, the new columns,
  `attachment_leftovers`) and `docs/architecture/overview.md` (the backup row,
  `attachments.db`).
- `docs/DEPRECATED.md`: the `asset:` protocol, reading attachments by path from the webview,
  and `file_path` columns, which are legacy-only.
- ADR 0017 → Accepted. The site copies of PRIVACY/SECURITY sync at release as usual.

## 5. Security properties after the change

| Threat | Before | After |
|---|---|---|
| Laptop or disk read without the password (no FileVault) | Attachments readable | Encrypted |
| Backup zip on cloud storage or USB | Attachments readable | Encrypted; an offline password guess against `key.enc` is the only way in |
| Another OS account, a copy of the data folder | Attachments readable | Encrypted |
| Unlocked session, malware running as the user | Exposed | Still exposed (out of scope, SECURITY.md) |
| Document opened in another app | Plaintext file | Temporary plaintext copy, wiped on lock, exit and next start; whatever that app caches is beyond our reach |
| Exports (JSON, CSV, "save a copy") | Plaintext | Plaintext, chosen by the user |
| Time Machine and APFS history, SSD remnants of old plain files | Plaintext | Old versions stay until the OS expires them; documented |

## 6. Verification

**Automated** (per `docs/standards/testing.md`):

- Store service on an in-memory main connection with `ATTACH ':memory:' AS files`: put, read,
  stream, delete, sweep, cap, MIME sniffing, and a rollback that spans both schemas.
- An encryption test on keyed temp files: `attachments.db` has no plaintext header and contains
  no JPEG (`FF D8 FF`) or `%PDF` byte runs; the `VACUUM files INTO` output is encrypted too.
- Migration 021 keeps every row and passes the CHECKs. The snapshot is regenerated.
- Conversion with temp dirs: inside-dir originals go to quarantine; outside-dir files stay and
  are recorded as leftovers; a missing file is counted; a hash mismatch rolls back; the
  quarantine is finished on the next run; the process resumes after an interruption.
- Backup: format 2 create, inspect and restore; format 1 restore; a format 2 archive missing
  `attachments.db` is rejected.
- Protocol route parsing and the header choice by MIME type, as pure functions.
- TS: the attachment URL path builder, and the MIME type → viewer kind mapping.

**Live** (a worktree demo run, never the real profile):

- Upload JPEG and HEIC photos, open the lightbox, download a photo and a batch.
- Add, view, save and delete a PDF; add a .docx and open it elsewhere (D3 dialog), then lock:
  `open/` is empty.
- Delete a property: its blobs are gone (`stored` count).
- `--legacy-attachments` profile: after unlock, conversion finishes, the attachment folders are
  gone, and the next unlock removes `attachments-converted/`.
- A backup zip lists only six root files; restoring a format 1 backup converts it.
- `find <data dir> -name '*.jpg' -o -name '*.pdf'` finds nothing.

**Owner:** a dry run on a copy of the real profile, where the owner types the password.

**At release:** a Windows and Linux checklist: the protocol URL form, CSP, pdf.js, and wiping
the temp copy while another app holds the file.

## 7. Risks and open points

- **pdf.js under the strict CSP** is the main unknown. Task 0 checks it before the rest
  depends on it, and D3's fallback ("open elsewhere") bounds the damage.
- **Mutex contention:** the protocol shares the one connection. A gallery of thumbnails
  (~50 kB each) is a few ms per image. If it is ever visible, a second read connection would
  need the key kept in memory, which ADR 0017 avoids. Measure before considering it.
- **Large legacy volumes:** conversion is incremental and in the background. A huge profile
  takes minutes but never blocks unlock.
- **Fully encrypted backups make `key.enc` the target.** Profiles whose password was set before
  ADR 0009 still use scrypt N=2^14. Recommended follow-up: re-wrap `key.enc` at the current cost
  after a successful unlock, with an atomic write.

## 8. Alternatives considered

Variants A (BLOBs in `moony.db`) and B (a custom per-file AES-GCM format) are compared in
ADR 0017. Options for opening documents:

- Only through a temporary copy, with no in-app viewer: simpler, but leaves plaintext for the
  most common case, PDFs.
- A native PDF iframe: works in WKWebView and WebView2 but not in WebKitGTK, and serving
  documents as frames weakens the sandbox in §4.6.
