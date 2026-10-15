# 0017. Attachments live in a second SQLCipher database

Date: 2026-10-15

## Status

Accepted (the owner approved decisions D1–D13 of
`docs/specs/2026-10-15-encrypted-attachments-design.md` on 2026-10-15)

## Context

Only `moony.db` is encrypted. Real estate photos and their thumbnails, real estate documents
and insurance documents are plain files in three folders of the data directory. The webview
reads photos straight from disk through the `asset:` protocol, documents open in the system
app from their stored path, and a backup archive carries the attachments as plain files next
to the encrypted database. PRIVACY.md, SECURITY.md and the README have to qualify the
"encrypted database" claim, and encrypted attachments are item 2 of the roadmap.

Three ways to encrypt the attachments were considered:

- **A. BLOBs in `moony.db`.** One file and one key, but the main database grows by every
  photo and PDF, and every pre-migration copy (`moony.db.bak-*`) and every backup snapshot
  copies all of it.
- **B. Encrypted files on disk.** Each file sealed with AES-256-GCM in chunks under a key
  derived from the master key. The main DB stays small and files stream naturally, but it is a
  new crypto format to design, version and audit, and the master key, which is dropped today
  once `PRAGMA key` has run, would have to stay in memory for the whole session and be wiped
  on lock.
- **C. A second SQLCipher database, `attachments.db`, attached to the main connection.**

A probe against the SQLCipher that rusqlite 0.40 bundles (SQLCipher 4.14.0 community)
confirmed what C relies on. `ATTACH` without a `KEY` clause encrypts the attached file with the
main database's key: the file has no plaintext header, opens with that key and fails without it
or with another key. `ATTACH` of a missing file creates it encrypted. `VACUUM files INTO`
writes an encrypted snapshot that opens with the same key. With the default `delete` journal
mode, a failed transaction that spans both schemas rolls back in both. Incremental blob I/O
(`blob_open` on the attached schema) round-trips a 3 MiB blob, and `PRAGMA files.quick_check`
and `PRAGMA files.cipher_integrity_check` work on the attached schema. If the main key is
wrong, `ATTACH` fails with "file is not a database" and leaves the file untouched.

## Decision

- Attachment content lives in `attachments.db` next to `moony.db`. It is attached as the
  schema `files`, without a `KEY` clause, right after the main key has been verified, both
  when a profile is created and when it is unlocked. The master key lifecycle does not change.
- `files` holds content only: one table `blobs(id, data, size, sha256, created_at)`, with
  `auto_vacuum = INCREMENTAL` and `secure_delete = ON`. Owners, names, MIME types and every
  relation stay in the main schema; attachment rows reference a blob by `blob_id`.
- The `files` schema has its own short, append-only migration list, versioned by
  `PRAGMA files.user_version`. It is the one deliberate exception to "every schema change is a
  numbered migration in `migrations.rs`", because that list belongs to the main schema. A
  pending `files` migration first copies `attachments.db`, the same way the main migrations
  first copy `moony.db`.
- The webview gets attachment bytes only through a custom `moony-file:` URI scheme. It serves
  data only while a session is unlocked, addresses an attachment by its owning row's id
  (never a path or a blob id) and sends `Cache-Control: no-store`. The `asset:` protocol and the
  webview's file-copy permission for attachments are removed.
- Writing a blob together with its row, and deleting a row together with its blob, happen in
  one transaction that spans both schemas. Foreign keys cannot cross schemas, so the service
  also sweeps unreferenced blobs after unlock.
- A backup becomes format 2: `VACUUM main INTO` and `VACUUM files INTO` snapshots plus the key
  files. Format 1 archives, which carry plain attachments, stay restorable.
- Plain files from earlier versions are converted into the store after unlock, as described in
  the spec. Only the conversion itself, and copies the user explicitly exports or opens in
  another app, ever write attachment content outside `attachments.db`.

## Consequences

- No new cryptography: one key and one cipher (SQLCipher) protect everything, and a password
  change or recovery leaves both databases as they are. Backups are fully encrypted, so the
  "everything is encrypted" claim becomes true. Plaintext stays only in explicit exports and in
  copies the user opens in another app.
- Losing both the password and the recovery key now loses the attachments too; today the
  plain files would survive. The app must say so where the recovery key is shown.
- The main database stays small. `attachments.db` is rewritten in place, so Time Machine and
  similar tools copy the whole file when an attachment changes (rarely).
- Bytes reach the webview through one handler that holds the connection mutex for the length
  of a read. A protocol response is a whole body in memory, so documents get a size cap
  (100 MB).
- Opening a document in another app needs a decrypted temporary copy. It lives in the app's
  cache folder, and the app deletes it on lock, on exit and at the next start.
- Downgrading after the conversion is not supported: an older build shows the converted
  attachments as missing. An older build rejects a format 2 backup (it rejects any format above
  1 and any unknown archive entry).
- A damaged or foreign `attachments.db` must not lock the user out of their finances: if the
  attach fails, the app unlocks without attachments, never overwrites the file, and reports
  the problem in Settings.
