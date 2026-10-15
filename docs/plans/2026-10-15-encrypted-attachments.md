# Encrypted attachments — plan

Spec: `docs/specs/2026-10-15-encrypted-attachments-design.md`. Section ids (§4.1–§4.13) and
decisions (D1–D13) below refer to it; ADR 0017 records the storage decision. The owner approved
D1–D13 and accepted ADR 0017 on 2026-10-15.

Work happens in a sibling worktree, for example `../Moony-tauri-attachments` on the branch
`feat/encrypted-attachments`. Other sessions switch branches in the shared checkout
`/Users/filipkral/Programování/FinanceApp/Moony-tauri`, so never commit there. Demo runs from
the worktree use their own port (1421 or 1422), and nobody types the real profile's password.

Read `AGENTS.md` (the ten rules), `docs/standards/rust-backend.md`,
`docs/standards/type-contract.md`, `docs/standards/i18n.md`, `docs/standards/testing.md`,
`docs/playbooks/add-db-migration.md` and `docs/DEPRECATED.md` before coding. Use TDD: write
the failing test, then the code. Make one Conventional Commit per task, ending with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. New user-facing strings land in all
ten locales in the same commit.

Gate per task, before committing:

```bash
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
npm run lint && npm run typecheck && npm test
```

## Status

| Task | Status | Commit |
|---|---|---|
| 0. Spike: pdf.js and a custom scheme under the app CSP | done — works without widening the CSP; see "Task 0 outcome" | spike branch `spike/pdfjs-csp` 999635de (not merged) |
| 1. Store: `attachments.db`, `files` schema, store service | done — demo unlock created an encrypted `attachments.db` (store version 1) | 1c2f191a |
| 2. Migration 021, wire contract, `moony-file:` scheme serving legacy files, frontend switch | done — see note | 0154c748 |
| 3. New photos and documents go into the store; deletes remove blobs | done — live demo: a raw-body photo and a PDF/.docx landed as blobs (no new files, no plaintext bytes in `attachments.db`); deleting a document and a legacy and a new batch answered 404 and removed the legacy files and their emptied folders | this commit |
| 4. Viewer, save a copy, open elsewhere, temp copy wiping | pending | — |
| 5. Conversion of plain files, status and leftovers in Settings → Data | pending | — |
| 6. Backup format 2, restore, integrity, delete account | pending | — |
| 7. Tooling, recovery-key copy, living docs, ADR 0017 accepted | pending | — |
| 8. Verification and review | pending | — |

## Task 0 outcome (2026-10-15)

Setup: a `tauri build --debug --no-bundle` of the spike branch (`VITE_SPIKE=1`), run in the
native macOS webview (WKWebView, origin `tauri://localhost`) with `HOME` pointing at an empty
scratch folder. The page reported to stdout. Canaries proved the CSP was enforced: a fetch to
`https://example.com` and `new Function` were both blocked.

Results:
- **pdf.js renders under the CSP as planned.** `pdfjs-dist` **6.4.299** (modern build; the
  legacy build is not needed) rendered a text PDF with a non-embedded Helvetica and a PDF with a
  JPEG 2000 image. The JPX centre pixel came out exactly at the source colour, and pdf.js caused
  no CSP violation.
- **No `'wasm-unsafe-eval'`.** WebAssembly is blocked in the document but allowed in a worker
  (Tauri sends the CSP header with HTML only), and pdf.js runs its wasm decoders in its worker.
  pdf.js also ships JS fallbacks (`*_nowasm_fallback.js`) in case a platform blocks wasm in
  workers too.
- **`isEvalSupported` no longer exists in pdf.js 6** (there is no eval path), so §4.7's mention of
  it is moot.
- **`getTextContent` needs a polyfill on this WebKit**: `ReadableStream.prototype[Symbol.asyncIterator]`
  is missing. The canvas viewer does not need text; if a text layer or search is ever added,
  install the small polyfill from the spike page first.
- **pdf.js needs `wasmUrl` and `standardFontDataUrl` served from `'self'`.** Copy
  `node_modules/pdfjs-dist/{wasm,standard_fonts,cmaps,iccs}` into the build (about 4 MB) with a
  small Vite plugin or a script; never commit them. On `tauri://localhost` pdf.js fetches them
  on the main thread (its worker fetch needs http(s)); on Windows (`http://tauri.localhost`) the
  worker fetches them. Both paths are covered by `connect-src 'self'`.
- **`moony-file:` works for `<img>`.** A page `fetch` of a `moony-file:` URL needs
  `Access-Control-Allow-Origin`, because the page origin is `tauri://localhost`. **Amends §4.6
  for Task 2:** answer with `Access-Control-Allow-Origin` set to the request's `Origin` only
  when it is the app origin (`tauri://localhost`, `http://tauri.localhost`, or
  `https://tauri.localhost`), and never `*`.
- **The CSP must allow Tauri's IPC protocol. Amends §4.6 for Task 2:** add
  `ipc: http://ipc.localhost` to `connect-src`. Without it, every invoke falls back to
  postMessage: a raw `Uint8Array` body arrives as `InvokeBody::Json`, and a
  `tauri::ipc::Response` arrives in JS as a plain Array. That breaks the raw-body photo upload
  from `d25506eb` in built apps (`tauri dev` applies no CSP, so it hides the problem). Handed to
  the session that made that change, to fix on `main`; this branch picks the fix up by merging
  `main`.
- `pdfjs-dist` declares an optional dependency, `@napi-rs/canvas`, for Node only. It is not
  bundled into the app; keep it out of any runtime code path.

## Task 2 notes (2026-10-15)

- Documents carry `mimeType: string` (the stored type, or the type the legacy file's extension
  implies, so it is never null) and `extension: string | null` for the file tile, instead of §4.10's
  `mimeType: string | null`. The tile no longer needs a path.
- The protocol serves `image/svg+xml` as `application/octet-stream` too: SVG can carry scripts.
- CORS: the app origin in every platform form, plus `http://localhost:*` in debug builds only,
  because `tauri dev` serves the page from the dev server.
- `fs:allow-copy-file` is gone. A global `fs:scope` deny list now covers the databases, the store,
  the quarantine, the key files and restore copies for every fs command.
- Dev bridge: a browser tab loads `/__moony-file/<path>`, and the Tauri window fetches the real URL.
- Live demo, from the worktree on port 1422 with a `demo-home` copy:
  - an uploaded JPEG showed its 400×267 thumbnail and its 1200×800 photo through the scheme;
  - `export_attachment` and `export_photo_batch` wrote byte-identical copies;
  - `../` and unknown ids answered 404;
  - after `logout` the scheme answered 403 with no body.

## Task 0 — Spike (§4.6, §4.7, D3)

Throwaway branch; nothing from it is merged. Goal: learn whether pdf.js renders under the
app's CSP in the macOS webview before Tasks 2 and 4 depend on it.

- Register a minimal `moony-file` scheme that returns a bundled sample PDF and a JPEG, with the
  headers of §4.6.
- On a dev-only page, render the JPEG with `<img>` and the PDF with lazily imported
  `pdfjs-dist` (worker as a bundled asset, `isEvalSupported: false`), under the CSP of §4.6.
- Run `npm run tauri dev` in the worktree and read the console. Try a text PDF and a scanned
  PDF (JPX or JBIG2 images if you can find one).
- Record in the Status table: works as is / needs `'wasm-unsafe-eval'` (allowed, D3) / does
  not work (then D3 falls back to "open elsewhere" for PDFs; tell the owner before Task 4).

**Done when:** the outcome and the `pdfjs-dist` version are recorded here.

## Task 1 — Store (§4.1, §4.3)

- **`src-tauri/src/db/files_schema.rs` (new):**
  - `FILES_MIGRATIONS: &[(i64, &str)]` with version 1 from §4.1 (`auto_vacuum` first).
  - `attach_files(conn, attachments_path) -> Result<()>`: `ATTACH DATABASE ?1 AS files`, a
    pre-migration copy when a version is pending (reuse the copy-and-prune helper behind
    `backup_database_file`; generalise it rather than duplicate it), apply the pending
    versions, then `PRAGMA files.secure_delete = ON`.
- **`src-tauri/src/db/mod.rs`:**
  - `attachments_path(db_path)` → the sibling `attachments.db`.
  - Call `attach_files` in `open_with_key` (after the main migrations, before the post-open
    self-heals) and in `create_with_key`.
  - D11: on failure, `log::warn!` without the path and set a `store_available:
    Arc<AtomicBool>` to false. Cleared on `close`, read by `Database::store_available()`.
- **`src-tauri/src/services/attachment_store.rs` (new):** the API of §4.3 (`put_bytes`,
  `put_reader`, `read_all`, `copy_to`, `delete`, `delete_unreferenced`, `verify`,
  `sniff_mime`), plus `MAX_DOCUMENT_BYTES = 100 * 1024 * 1024`. Add `sha2` as a direct
  dependency at the version already in `Cargo.lock`.
  - `delete_unreferenced` reads the reference columns that Task 2 adds. Until then, give it the
    column list as a constant that Task 2 extends; there is no dead code in between.
- **Tests:**
  - Service tests on `Connection::open_in_memory()` + `ATTACH ':memory:' AS files` + the
    version 1 SQL: put/read round trips (bytes and a 3 MiB reader), sha256, delete, the sweep,
    the cap, sniffing, and a rollback across both schemas.
  - `files_schema` tests on keyed temp files (`tempfile`, a raw `Connection` with `PRAGMA
    key`, never `Database`):
    - the file has no plaintext header and holds no `FF D8 FF` or `%PDF` byte run after storing
      such content;
    - `VACUUM files INTO` output is encrypted;
    - re-attaching reads `user_version`;
    - attaching under a wrong key fails and leaves the file byte-identical.

**Done when:** tests pass and an unlocked dev profile has `attachments.db` next to `moony.db`.
Commit `feat(attachments): encrypted attachments.db store attached with the main key`.

## Task 2 — Migration, wire contract, scheme, frontend switch (§4.2, §4.6, §4.10)

After this task the app still stores new files on disk, but the webview reads every attachment
through `moony-file:`.

- **Migration 021** (`021_encrypted_attachments`) per §4.2, including `attachment_leftovers`.
  - Add a migration test: legacy rows survive with their values, and the CHECKs reject a row
    with neither a file nor a blob.
  - Regenerate the snapshot with `cargo test regenerate_schema_snapshot -- --ignored`.
  - Add the new reference columns to the sweep constant from Task 1.
- **Models and contract**, per the 7-step procedure:
  - `RealEstatePhoto` loses `filePath` / `thumbnailPath`; the internal row type keeps them.
  - Documents lose `filePath` and gain `mimeType`.
  - Add `AttachmentKind`.
  - Update `shared/schema.ts`, `bindings.rs` and `generated-types.ts`, then fix every TS user.
- **`src-tauri/src/services/attachments.rs` (new, owner-aware):**
  - `source(conn, data_dir, kind, id) -> Option<Source>`, where `Source` is
    `Blob { blob_id, mime }` or `Legacy { path, mime }`. Legacy documents resolve through
    `document_files::resolve_document_path`; legacy photos are relative to the data dir; the
    legacy MIME type is sniffed.
  - `export(conn, data_dir, kind, id, dest)` and `export_batch(conn, data_dir, batch_id, dir)`
    (today's `batch_<date>_<id8>.jpg` names).
- **`src-tauri/src/services/attachment_protocol.rs` (new):**
  - pure `parse_route(path) -> Option<(AttachmentKind | PhotoThumb, id)>` and
    `headers_for(mime)` per §4.6;
  - `respond(db, data_dir, request) -> http::Response<Vec<u8>>` (403, 404, 405, 503).
  - Unit tests for routing, the header choice, and the status codes on an in-memory DB.
- **`src-tauri/src/lib.rs`:** register the scheme with
  `register_asynchronous_uri_scheme_protocol("moony-file", …)` and move the work to
  `tauri::async_runtime::spawn_blocking`. Register `export_attachment` and
  `export_photo_batch` (thin commands).
- **Config:**
  - `tauri.conf.json`: remove `assetProtocol`; add `moony-file: http://moony-file.localhost`
    to `img-src` and `connect-src`.
  - `Cargo.toml`: drop the `protocol-asset` feature.
  - `capabilities/default.json`: remove the `fs:allow-copy-file` grant, add the `attachments.db*`
    and `attachments-converted/**` denies, and update the description.
- **Frontend:**
  - `src/utils/attachment-url.ts` (+ test) builds the `/<kind>/<id>` path; callers wrap it in
    `convertFileSrc(…, 'moony-file')`.
  - `PhotoTimelineGallery.tsx` drops `appDataDir` and `copyFile` and uses the URL helper and
    the new export wrappers in `src/lib/tauri-api.ts`.
  - The dev browser bridge maps `moony-file` URLs to a dev relay route (§4.12).

**Done when:** in a demo run, the gallery, the lightbox and both downloads work with
`assetProtocol` gone, and documents still open. Commit
`feat(attachments): serve attachments through the moony-file scheme`.

## Task 3 — Writes and deletes through the store (§4.4, §4.5, D7, D8)

- **`services/real_estate_photos.rs`:** encode the photo and the thumbnail to JPEG in memory.
  In one `with_conn` and one transaction, put both blobs and insert the row with blob ids and
  NULL paths. Remove `StoredPhoto::discard` and the folder creation, and update the tests.
- **Documents:** move the add and delete logic out of `commands/insurance.rs` and
  `commands/real_estate.rs` into `services/attachments.rs`:
  - `add_document(conn, owner, source_path, data)`: size check
    (`validation.attachmentTooLarge`), `put_reader`, insert with `mime_type`, `original_name`
    and `file_size` in one transaction;
  - `delete_document(conn, data_dir, kind, id)`: delete the row and blob in one transaction,
    then delete the legacy file, if any, after the commit.
- **Owners:** `delete_real_estate` and `delete_insurance` call new service functions. They
  collect the blob ids and legacy paths of every cascading attachment, delete the blobs and the
  owner in one transaction, then delete the legacy files. Photo and batch deletes work the
  same way. Commands stay thin.
- `validation.attachmentTooLarge` in all ten locales.
- **Tests:**
  - a new photo creates two blobs and no file;
  - a document over the cap is refused before any write;
  - deleting a property removes the blobs of its batches and documents (the orphan-file bug);
  - legacy rows still delete their files.

**Done when:** a demo run uploads JPEG and HEIC photos and adds a PDF, and the data dir gains
no new files. Commit
`feat(attachments): new photos and documents go into the encrypted store`.

## Task 4 — Viewer, save a copy, open elsewhere (§4.7, D3)

- **Backend (`services/attachments.rs`):**
  - `open_elsewhere(conn, data_dir, cache_dir, kind, id) -> PathBuf` writes to
    `<cache>/open/<uuid>/<sanitized name>` (0700 folder, 0600 file on Unix);
  - `wipe_open_copies(cache_dir)`;
  - `sanitize_file_name` (pure, tested: separators, control characters, reserved Windows names,
    120-character limit, a fallback name).
- `open_real_estate_document` and `open_insurance_document` use `open_elsewhere` and then
  `document_files::open_with_system`.
- **Wiping:**
  - in the `logout` command, which auto-lock also uses (check; if auto-lock takes another path,
    wipe there too);
  - at startup in `setup`;
  - on `RunEvent::Exit`: change `.run(generate_context!())` in `lib.rs` to
    `.build(…)?.run(|app, event| …)`.
- **Frontend:**
  - `src/utils/attachment-viewer.ts` (+ test): MIME type → `'image' | 'pdf' | 'external'`.
  - A shared `AttachmentViewerDialog` with a lazily loaded `PdfPages` component (pdf.js per
    the Task 0 outcome) and the actions „Uložit kopii…“ and „Otevřít v jiné aplikaci“.
  - `InsuranceDocuments.tsx` and `RealEstateDocuments.tsx` open the viewer for image/PDF and
    the D3 dialog for the rest. The D3 flag lives in `localStorage` through
    `src/utils/device-storage.ts`.
  - Add the `pdfjs-dist` version recorded in Task 0 from npmjs, and run `npm audit`.
- All copy from §4.11 for the viewer and D3 goes into all ten locales.

**Done when:** in a demo run, a PDF and a JPEG document open in the viewer; a .docx shows the
D3 dialog and opens; after lock and after quit, `<cache>/open` is empty; a saved copy
byte-matches the original. Commit
`feat(attachments): in-app viewer, save a copy, open elsewhere with wiped temp copies`.

## Task 5 — Conversion, status, leftovers (§4.8, D4, D5)

- **`src-tauri/src/services/attachment_conversion.rs` (new):**
  - `finish_quarantine(conn, data_dir)`;
  - `pending(conn) -> Vec<LegacyItem>`;
  - `convert(conn, data_dir, item, bytes) -> Outcome` (Converted / Outside / Missing / Failed,
    with the single-transaction verify of §4.8, step 3);
  - `refresh_leftovers(conn, data_dir)`, using a new `FORMER_IDENTIFIERS` constant in
    `constants.rs` (`com.filipkral.moony`, `com.filipkral.moony-tauri`);
  - `status(conn, data_dir, progress) -> AttachmentStatus`.
- **Runner:** managed state `AttachmentConversion` (an `AtomicBool` "running" plus progress
  counters).
  - Started after a successful unlock, next to where the MCP server starts on unlock in
    `commands/auth.rs`.
  - Pinned with `db.current_session()`. Files are read outside the lock; each item's commit
    and move happen in one `with_conn`.
  - Then the sweep and `incremental_vacuum`.
  - Emits `attachments-conversion-finished`.
- **Commands:** `get_attachment_status`, and `reveal_attachment_leftover(path)`, which only
  accepts a path present in the current status and reveals it with the opener plugin.
- **Frontend:** a status block and leftovers list in `components/settings/DataSection.tsx`
  (§4.11 copy). The finish event invalidates `["attachment-status"]` and the photo-batch and
  document query keys.
- **Tests** (temp dirs, in-memory DB):
  - an inside file → quarantine and a blob;
  - an outside file → stays put plus a leftover row;
  - missing is counted;
  - a forced hash mismatch rolls back;
  - an interrupted run resumes;
  - `finish_quarantine` deletes only when the checks pass;
  - a leftover whose path vanished is dropped;
  - a former-identifier folder with attachments is reported.

**Done when:** a `seed_demo --legacy-attachments` profile converts in a demo run (Task 7 adds
the flag; do that part of Task 7 first if needed), the Data page shows the counts, and the next
unlock removes `attachments-converted/`. Commit
`feat(attachments): convert plain files from earlier versions after unlock`.

## Task 6 — Backup format 2 (§4.9, D6)

- **`services/backup.rs`:**
  - `BACKUP_FORMAT = 2`; `create_backup` per §4.9 (two `VACUUM … INTO` snapshots, legacy
    folders only when legacy files remain, `attachmentCount`);
  - `classify` knows `attachments.db`; `inspect_backup` requires it for format 2 and still
    accepts format 1;
  - `managed_names` gains `attachments.db`, `attachments.db-journal` and
    `attachments-converted`.
- **`verify_database`:** add the `files` checks (§4.9).
- **`services/auth.rs::delete_account`:** remove the store, its journal and `.bak-*` copies,
  the quarantine and the cache `open/`. The commands pass the cache dir.
- **Contract:** `BackupManifest.attachmentCount?` per the 7-step procedure.
- **Tests:**
  - a format 2 round trip (create → inspect → restore into an empty dir → open with the key →
    blobs intact);
  - a format 1 archive still restores;
  - a format 2 archive without `attachments.db` is invalid;
  - a backup taken mid-conversion holds every attachment exactly once (blob or legacy file);
  - delete account leaves no store files.

**Done when:** a demo backup zip lists exactly `manifest.json`, `moony.db`, `attachments.db`,
`salt`, `key.enc`, `recovery.enc`; restoring it and a 0.12-era backup both work. Commit
`feat(backup): format 2 carries the encrypted attachments store`.

## Task 7 — Tooling, copy, docs (§4.11 D10, §4.12, §4.13)

- `seed_demo`: attachments through the store; the `--legacy-attachments` flag (§4.12).
  `local_anonymize_profile`: confirm it never attaches `files` or copies `attachments.db`, and
  say so in its header comment.
- `RecoveryKeyBox.tsx` / onboarding: the D10 sentence in all ten locales.
- Living docs per §4.13: PRIVACY.md, SECURITY.md, README, ROADMAP,
  `docs/architecture/database.md`, `docs/architecture/overview.md`, `docs/DEPRECATED.md`.
  (ADR 0017 was already accepted at approval.)
- Draft the release-notes paragraph (D10, D12, "first unlock converts your attachments in the
  background") in the plan's Status notes for the owner.

**Done when:** the docs no longer qualify the encryption claim except for §5's honest
exceptions. Commits `chore(seed): …` and `docs(attachments): …`.

## Task 8 — Verification and review (§6)

- All gates green, including `cargo audit` and the generated-types drift check.
- The live checklist of §6 in a demo run from the worktree. Attach evidence to the Status row:
  the `unzip -l` listing and the `find` output.
- Code review (`superpowers:requesting-code-review`) on the whole branch, focused on:
  - transactions spanning both schemas;
  - the protocol's lock and 403 behaviour;
  - path handling in `open_elsewhere` and `reveal_attachment_leftover`;
  - the conversion's delete and move order.

  Fix the findings in follow-up commits.
- Merge into local `main` (no push). Before the release, ask the owner for the dry run on a
  copy of the real profile, and add the Windows/Linux checklist of §6 to the release pre-flight.
