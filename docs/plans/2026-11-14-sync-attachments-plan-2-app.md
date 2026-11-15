# Synced Documents and Photos (App Side) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Documents and photos attached to insurance policies and properties sync between a person's devices: their rows travel as synced rows, their content as one sealed stream per blob through the sync server's blob store, uploaded and downloaded beside the ordinary sync rounds without ever holding them up.

**Architecture:** The four attachment tables become `Synced` in the registry; a row syncs only while it names a blob (`TableSpec.requires`), so legacy rows stay home until the conversion gives them a blob, which capture records as a complete new row. A Local table `sync_blobs` (one row per referenced blob: `up`, `down` or `done`) is reconciled from the rows and the store, never from hooks in the services. Content is sealed and opened chunk by chunk (`envelope/blob.rs`, the snapshot STREAM with `moony sync blob v1` and the blob ID as object ID) and streamed between `attachments.db` (incremental blob I/O, incomplete blobs invisible until their hash is written) and the server. A round uploads small waiting blobs before it pushes (20 s budget); two lanes (uploads, downloads) run beside the rounds for everything else; a daily pass, run only by a fully caught-up device, deletes old unreferenced blobs from the server and re-uploads blobs the server lost.

**Tech Stack:** Rust (Tauri 2, rusqlite/SQLCipher with the attached `files` schema, tokio, reqwest 0.13 with the `stream` feature, chacha20poly1305 + hkdf), React 19 + TypeScript (React Query, react-i18next), Vitest.

**Spec:** `docs/specs/2026-11-14-sync-attachments-design.md` (app side: §5, §7, the app points of §8, §9 steps 2–3, §10, §11). Background: `docs/specs/2026-11-06-device-sync-client-design.md` §20 (as built), ADRs 0017, 0024–0029.

**Working tree:** `/Users/filipkral/Programování/FinanceApp/Moony-tauri-syncux` (never the sibling `Moony-tauri`). Every path below is relative to it.

## Global Constraints

- Gates after every task: `npm run lint && npm run typecheck && npm test` and `cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test`. Each task ends with one Conventional Commit whose message ends with the `Co-Authored-By:` attribution line the implementer's own session instructions give for commits.
- `AGENTS.md` rules hold: `invoke()` only in `src/lib/tauri-api.ts`; every wire change follows the 7 steps of `docs/standards/type-contract.md` (models with per-field `#[serde(rename = "camelCase")]`, `bindings.rs`, `cd src-tauri && cargo test generate_bindings -- --ignored`, `shared/schema.ts`, `tauri-api.ts`); thin commands, logic in `services/`, `AppError` (never `Result<_, String>`), no `.unwrap()` outside tests; append-only migrations; every user-facing string in all ten locales (`en cs de es it nb pl sv ja zh`) in the task that adds it, plural forms per `docs/standards/i18n.md`, "sync server" never "relay" (cs "synchronizační server").
- Sync rules (`docs/standards/rust-backend.md` §12): network only through `services/sync/relay/http.rs` (tests use `relay/fake.rs`); never hold the session mutex across a network call (every DB touch is one `ConnAccess::with_conn`); sync errors stay `SyncError`/typed until the command boundary; no `INSERT OR REPLACE`; log levels per §8 (document names, file names and amounts only at `debug`).
- Values pinned by the spec: info string `moony sync blob v1`; 64 KiB chunks; the blob ID (16 raw UUID bytes) as the STREAM object ID; per-blob size limit 100 MiB of plaintext (`attachment_store::MAX_DOCUMENT_BYTES`, 104,857,600) plus stream overhead; daily pass at most once per 24 h; server blobs deleted only when unreferenced **and** older than 7 days, only by a fully caught-up device; rows never held back for their content; thumbnails fetched at once on every device; the phone fetches full content on open or "Download everything to this phone", the desktop fetches everything in the background; no checksum column; no resume (a broken transfer starts again); `features: ["blobs"]` on `GET /v1/server` (absent field = no features); `protocol` stays 2.
- Server contract **as built** (Moony-relay local `main`, merge f77d8dd; its `DESIGN.md` is authoritative, read the blob rows of its HTTP table before Task 3). All calls take `Authorization: Bearer <vault token>`; every error answer of a blob route carries the header `Moony-Error: <code>` besides the JSON body `{"error":"<code>"}` (a `HEAD` has no body, so read the header there). `PUT /v1/vaults/:vaultId/blobs/:blobId` raw body with `Content-Length` (digits only) → `201 {"created":true}`, or `200 {"created":false}` when the blob already exists (the body is not read); errors `400 invalid_blob_id` (the ID must be a lowercase canonical UUID), `404 vault_not_found`, `401 unauthorized`, `403 token_read_only`, `402 payment_required`, `411 length_required`, `413 blob_too_large` (over 104,883,275 bytes), `400 empty_body`, `413 quota_exceeded`, `507 server_full` (free space, uploads in progress counted; a snapshot `PUT` answers it too), `429 too_many_uploads` (two uploads per vault at once: retry later), `400 incomplete_body`, `408 body_timeout`, `500 internal_error`. `HEAD …/blobs/:blobId` → `200` with `Content-Length` and `Moony-Blob-Size` = stored size; `404` with `Moony-Error: blob_not_found` or `vault_not_found`. `GET …/blobs/:blobId` → `200` raw bytes with `Content-Length`; `404 blob_not_found`. `GET …/blobs?after=<uuid>&limit=<n>` (default 500, max 1000, `after` exclusive, ascending by id) → `{"blobs":[{"id","size","createdAt"}],"more":bool,"now":<unix seconds>}`; **`now` is the server's clock and is what blob ages are measured against** (`now − createdAt`), not the HTTP `Date` header; `400 invalid_limit` for `limit=0`. `DELETE …/blobs/:blobId` → `204`, also when the blob does not exist; current token only, no entitlement. Read calls work with a previous (rotated) token and for a blocked person inside the 30-day read grace; after it they answer `402`.
- Sync is unreleased (`feature::RELEASED` false); there are no older sync-enabled builds to stay compatible with. The new migration raises the schema number, so a build without this work judges this build's envelopes `NewerSchema` and refuses its snapshots, which is the intended behaviour.
- Status surfaces that the sync-aware-app plan (parts A and B) adds — the top-bar sync popover, the shared status mapping under `src/utils/`, the phone's Údržba sync row, the "Co se synchronizuje" text — exist on `main` before this plan starts; find them in the tree by role (grep for `SyncStatusIndicator`, the mapping imported by both it and `SyncStatusBlock.tsx`) and extend them, never re-create them.

## Review Focus

1. **The app is locked, quit or crashes in the middle of a download.** The half-written blob must never be served, exported or counted as present, and must be cleaned up. Pinned by `an_incomplete_blob_is_invisible_to_every_reader` (Task 2) and `a_lane_start_sweeps_incomplete_blobs` (Task 12).
2. **The person deletes a document (or its policy) while its upload or download is running.** No error loop, no resurrected blob, the queue entry disappears. Pinned by `a_row_deleted_during_a_transfer_drops_its_entry` (Task 10).
3. **A device whose wall clock is wrong runs the daily pass.** Ages are measured on the server's clock only; without one nothing is deleted. Pinned by `the_pass_ages_blobs_by_the_server_clock_only` (Task 13).
4. **Two devices hold the same blob** (a join kept this device's copy, or a re-upload after the pass): both "upload" it. A blob the server has must be confirmed without a body. Pinned by `a_blob_the_server_has_is_confirmed_without_a_put` (Task 10).
5. **A row names a blob ID that is not a canonical UUID** (a buggy or hostile key holder). It must never reach a URL or a file name, and the item shows "could not download". Pinned by `an_invalid_blob_id_is_never_requested` (Task 10) and `canonical_id_accepts_only_lowercase_hyphenated_uuids` (Task 7).

---

## File map

| Path | Responsibility |
|---|---|
| `src-tauri/src/services/sync/envelope/blob.rs` (new) | Piece-by-piece seal and open of one blob stream; length arithmetic |
| `src-tauri/src/services/attachment_store.rs` | Incomplete (incoming) blobs, offset reads, sweep |
| `src-tauri/src/services/sync/relay/{mod,http,fake}.rs`, `relay/fake/blobs.rs` (new), `relay/http/blob_tests.rs` (new) | The five blob calls, `features`, the blob errors |
| `src-tauri/src/db/migrations.rs`, `db/schema.snapshot` | Migration `0NN_sync_blobs` |
| `src-tauri/src/services/sync/registry/{mod,tables,keys}.rs` | Attachment tables `Synced`, `TableSpec.requires`, the enrolment migration name |
| `src-tauri/src/services/sync/capture/{triggers,enrol}.rs` | Row filter, enrol/leave triggers, one-time enrolment of unclocked rows |
| `src-tauri/src/services/sync/journal/read.rs`, `snapshot/{mod,attachments}.rs` | Rows without a blob read as absent; replace counts what it carries |
| `src-tauri/src/services/sync/blobs/` (new: `mod.rs`, `queue.rs`, `content.rs`, `stream.rs`, `policy.rs`, `transfer.rs`, `pass.rs`, `tests/`) | Queue, per-row content state, store↔wire streams, transfers, daily pass |
| `src-tauri/src/services/sync/engine/{mod,runtime,status}.rs` | Pre-push uploads, the two lanes, status counts and problem |
| `src-tauri/src/services/attachments.rs`, `real_estate_photos.rs`, `attachment_protocol.rs` | Listings with content state (moved out of commands), "not here yet" |
| `src-tauri/src/commands/sync_attachments.rs` (new) | Download, retry, download everything |
| `src-tauri/tests/sync_blob_memory.rs` (new) | 100 MiB within a fixed memory bound |
| `src/utils/attachment-content.ts`, `src/utils/sync-attachments.ts`, `src/utils/file-size.ts` (new, with tests) | Pure UI mappings |
| `docs/architecture/decisions/0030-device-sync-documents-and-photos.md` (new; next free number) | The ADR |

---

### Task 1: Blob stream sealed and opened piece by piece

**Files:**
- Create: `src-tauri/src/services/sync/envelope/blob.rs`, `src-tauri/src/services/sync/envelope/blob_tests.rs`
- Modify: `src-tauri/src/services/sync/envelope/mod.rs` (`pub mod blob;`, `pub const BLOB_INFO`, `#[cfg(test)] mod blob_tests;`), `src-tauri/src/services/sync/envelope/stream.rs` (share the chunk nonce, the AAD and an info-parametrised sealer/opener; snapshot bytes unchanged), `src-tauri/Cargo.toml` (`[profile.dev.package.chacha20]`, `[profile.dev.package.poly1305]`, `[profile.dev.package.chacha20poly1305]` with `opt-level = 3`, commented like the KDF entries)

**Interfaces:**
- Consumes: `stream::{CHUNK, HEADER_LEN}`, `hkdf32`, `aead_seal`, `aead_open`, `OpenError`, `MAGIC`.
- Produces (in `envelope::blob` unless noted):
  - `pub const BLOB_INFO: &[u8] = b"moony sync blob v1";` (`envelope/mod.rs`)
  - `pub const MAX_BLOB_PLAIN: u64 = 104_857_600;`
  - `pub const fn sealed_len(plain_len: u64) -> u64` = `HEADER_LEN + (plain/CHUNK)·(CHUNK+16) + plain%CHUNK + 16`
  - `pub const fn plain_len(sealed_len: u64) -> Option<u64>` (`None` below `HEADER_LEN + 16` or when the last chunk is shorter than its tag)
  - `pub const MAX_BLOB_SEALED: u64 = sealed_len(MAX_BLOB_PLAIN);`
  - `pub fn blob_object_id(blob: &uuid::Uuid) -> [u8; 16]` (the raw bytes)
  - `pub struct BlobSealer` — `pub fn new(epoch: u32, vault_key: &[u8; 32], vault_id: &[u8; 16], blob: &Uuid) -> Self`; `pub fn header(&self) -> [u8; HEADER_LEN]`; `pub fn seal_chunk(&mut self, plain: &[u8], last: bool) -> crate::error::Result<Vec<u8>>` (exactly `CHUNK` bytes unless `last`, then `0..CHUNK`; any call after `last` is an error)
  - `pub struct BlobOpener` — `pub fn new(keys: VaultKeys, vault_id: [u8; 16], blob: Uuid, sealed_len: u64) -> Result<Self, OpenError>` (`Malformed` when `plain_len` is `None` or the plaintext is over `MAX_BLOB_PLAIN`); `pub fn plain_len(&self) -> u64`; `pub fn push(&mut self, sealed: &[u8]) -> Result<Vec<Zeroizing<Vec<u8>>>, OpenError>` (the plaintext chunks that completed and authenticated, in order; the header is parsed once its 59 bytes are in, and its epoch looked up with `VaultKeys::get`); `pub fn finish(self) -> Result<(), OpenError>` (`Malformed` unless exactly `sealed_len` bytes came and the flagged last chunk opened)
  - `stream.rs`: `pub(super) fn chunk_nonce`, `pub(super) fn stream_aad`, `pub(super) fn open_stream_with(info: &[u8], …)`; `StreamSealer::new_with` gains a `pub(super)` sibling taking `info`. `open_stream` and the snapshot sealer keep their signatures and bytes.

The opener buffers at most one sealed chunk (`CHUNK + 16`) plus the piece being pushed; it never holds the whole stream.

- [ ] **Step 1: Write the failing tests** in `envelope/blob_tests.rs`:

```rust
#[test] fn sealed_and_plain_lengths_invert_each_other() {
    for n in [0, 1, CHUNK as u64 - 1, CHUNK as u64, CHUNK as u64 + 1, 3 * CHUNK as u64, MAX_BLOB_PLAIN] {
        assert_eq!(plain_len(sealed_len(n)), Some(n));
    }
    assert_eq!(sealed_len(0), 59 + 16);
    assert_eq!(plain_len(74), None);
    assert_eq!(plain_len(59 + 65_536 + 16 + 15), None); // a last chunk shorter than its tag
}
#[test] fn a_blob_opens_piece_by_piece_whatever_the_piece_size()  // plaintext 3*CHUNK+17 bytes; piece sizes 1, 7, 4096, CHUNK+16+5 and whole; concatenated output == input; finish() Ok
#[test] fn the_blob_stream_is_the_snapshot_stream_with_its_own_info() // same key/salt/prefix: BlobSealer bytes == StreamSealer(info = BLOB_INFO) bytes; open_stream_with(BLOB_INFO) opens them; open_stream (snapshot info) answers Err(OpenError::Authentication)
#[test] fn the_fixed_blob_vector_holds() // key [7;32], vault [9;16], blob 00000000-0000-4000-8000-000000000001, epoch 0, salt [1;32], prefix [2;19], plaintext b"moony blob": hex(SHA-256(sealed)) == the pinned value (compute once, pin, never change)
#[test] fn a_tampered_chunk_fails_authentication() // flip one byte of chunk 1 of a 3-chunk stream: push answers Err(Authentication); chunk 0's plaintext was returned, nothing after
#[test] fn a_truncated_blob_never_finishes() // (a) drop the last sealed chunk, (b) drop the last byte: finish() == Err(Malformed) for the announced length
#[test] fn a_whole_chunk_presented_as_last_fails() // plaintext of exactly CHUNK bytes; remove the empty last chunk and announce the shorter length: Err(Authentication) (the flag differs)
#[test] fn a_blob_opened_under_another_id_or_vault_fails() // Err(Authentication) for another Uuid, and for another vault id
#[test] fn an_unknown_epoch_is_named() // sealed under epoch 5, keys only for 0: Err(UnknownEpoch(5))
#[test] fn more_bytes_than_announced_are_refused() // one extra byte: Err(Malformed)
#[test] fn the_blob_limit_is_the_document_limit() { assert_eq!(MAX_BLOB_PLAIN, crate::services::attachment_store::MAX_DOCUMENT_BYTES); }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri && cargo test --lib services::sync::envelope::blob_tests`
Expected: compile errors (`blob` module missing).

- [ ] **Step 3: Implement `envelope/blob.rs` and the `stream.rs` sharing.** The sealer and opener reuse the snapshot construction exactly (header, per-stream HKDF key with `BLOB_INFO`, nonce = prefix | counter | last flag, AAD = op AAD | blob ID). Existing snapshot vectors in `envelope/tests.rs` must still pass untouched.

- [ ] **Step 4: Run to verify they pass**

Run: `cd src-tauri && cargo test --lib services::sync::envelope`
Expected: PASS (the snapshot vectors included).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/services/sync/envelope src-tauri/Cargo.toml
git commit -m "feat(sync): seal and open one blob stream piece by piece

Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>"
```

---

### Task 2: Incomplete blobs in the attachment store

**Files:**
- Modify: `src-tauri/src/services/attachment_store.rs` (functions + its `#[cfg(test)]` module)

**Interfaces:**
- Produces:
  - Rule: a `files.blobs` row whose `sha256 = ''` is **incomplete**. `rowid_of`, `size_of`, `copy_to`, `read_all`, `verify`, `delete_unreferenced` (unchanged: it still deletes unreferenced incomplete rows) and `rows_without_content` treat it as absent. `put_reader` keeps writing `''` until `fill` sets the hash.
  - `pub fn begin_incoming(conn: &Connection, id: &str, len: u64) -> Result<()>` — `validation.attachmentTooLarge` over `MAX_DOCUMENT_BYTES`; deletes an incomplete row of the same `id` first; `AppError::Internal("attachments.blobExists")` when a complete one exists; inserts `zeroblob(len)` with `sha256 = ''`.
  - `pub fn write_incoming(conn: &Connection, id: &str, offset: u64, bytes: &[u8]) -> Result<()>` — incomplete rows only (`NotFound` otherwise); past the end is `Internal`.
  - `pub fn finish_incoming(conn: &Connection, id: &str, sha256: &str) -> Result<()>` — sets the hex hash on the incomplete row; `NotFound` when it is gone.
  - `pub fn abandon_incoming(conn: &Connection, id: &str) -> Result<()>` — deletes an incomplete row, never a complete one.
  - `pub fn is_complete(conn: &Connection, id: &str) -> Result<bool>`
  - `pub fn read_at(conn: &Connection, id: &str, offset: u64, buf: &mut [u8]) -> Result<usize>` — complete blobs only; returns bytes read (0 at the end).
  - `pub fn sweep_incomplete(conn: &Connection) -> Result<usize>`

- [ ] **Step 1: Write the failing tests** (in the module's `#[cfg(test)]`, `setup_test_db()` as today):

```rust
#[test] fn an_incoming_blob_is_written_in_pieces_and_completes_with_its_hash() // begin 5 bytes, write "mo" at 0 and "ony" at 2, finish(sha("moony")); read_all == b"moony"; verify(..) true
#[test] fn an_incomplete_blob_is_invisible_to_every_reader() // after begin+partial write: size_of None, read_all Err(NotFound), is_complete false, copy_to Err, read_at Err; a row referencing it counts in rows_without_content
#[test] fn beginning_again_replaces_an_incomplete_blob_but_never_a_complete_one() // begin twice Ok; finish; begin again → Err(Internal("attachments.blobExists"))
#[test] fn abandon_and_sweep_remove_only_incomplete_blobs() // one complete, two incomplete: sweep_incomplete == 2; the complete one is readable
#[test] fn writing_past_the_end_or_into_a_complete_blob_is_refused()
#[test] fn read_at_reads_a_window_of_a_complete_blob() // 3 MiB payload: read_at(offset 65_536, buf 65_536) equals the slice; at len returns 0
#[test] fn an_oversized_incoming_blob_is_refused_before_anything_is_written() // begin(MAX_DOCUMENT_BYTES + 1) → Validation("validation.attachmentTooLarge"); no row
```

- [ ] **Step 2: Run to verify they fail** — `cd src-tauri && cargo test --lib services::attachment_store` → FAIL (functions missing).
- [ ] **Step 3: Implement** with `blob_open(files_schema::SCHEMA, "blobs", "data", rowid, …)` and `Blob::write_at` / `read_at`; every lookup of a complete blob adds `AND sha256 <> ''`.
- [ ] **Step 4: Run** `cd src-tauri && cargo test --lib services::attachment` → PASS (the conversion and protocol tests included).
- [ ] **Step 5: Commit** — `feat(attachments): incoming blobs written in pieces stay invisible until complete`.

---

### Task 3: The relay's blob calls: contract, HTTP client and fake

**Files:**
- Modify: `src-tauri/src/services/sync/relay/mod.rs`, `relay/http.rs`, `relay/fake.rs`, `relay/fake/tests.rs`, `src-tauri/src/services/sync/error.rs` (exhaustive `From<RelayError>`), `src-tauri/src/services/sync/relay_e2e.rs` (the `Counting` wrapper passes the five calls through), `src-tauri/Cargo.toml` (`reqwest` features `["json", "stream"]`; `futures-util = "0.3"`, already in `Cargo.lock`)
- Create: `relay/fake/blobs.rs` (the fake's per-vault blob store and knobs), `relay/http/blob_tests.rs`

**Interfaces:**
- Produces (in `relay/mod.rs`):
  - `ServerInfo` gains `pub blobs: bool` (`features` contains `"blobs"`; unknown features ignored; a missing field is no features). `ServerInfoWire` gains `#[serde(rename = "features", default)] features: Vec<String>`.
  - `RelayError::BlobNotFound` (`404 blob_not_found`; for a `HEAD`, which has no body, the code is read from the `Moony-Error` header: `blob_not_found` → `BlobNotFound`, `vault_not_found` → the existing vault-not-found variant, a `404` with neither → `BlobNotFound`); `429 too_many_uploads` maps like any `429` (transient, retried), `RelayError::InsufficientStorage` (status `507`, by status alone like `429`); `"invalid_blob_id"` joins `BAD_REQUEST_CODES`. In `error.rs`: `BlobNotFound` → `SyncError::Refused`, `InsufficientStorage` → `SyncError::Transient(Transient::Relay)` (a snapshot `PUT` on the free-space floor backs off as a `507` did before).
  - `pub struct RemoteBlob { pub id: Uuid, pub size: u64, pub created_at: i64 }`, `pub struct BlobPage { pub blobs: Vec<RemoteBlob>, pub more: bool, pub server_now: Option<i64> }` (`server_now` from the list answer's `now` field, unix seconds on the server's clock; `None` when the field is absent or not a non-negative integer), `pub const MAX_BLOB_PAGE: u32 = 1000;`
  - `pub type BlobBody = Box<dyn Iterator<Item = std::io::Result<Vec<u8>>> + Send + 'static>;`
  - `pub enum SinkFlow { Continue, Stop }`, `pub trait BlobSink: Send { fn begin(&mut self, sealed_len: u64) -> SinkFlow; fn piece(&mut self, bytes: &[u8]) -> SinkFlow; }`, `pub enum Delivery { Complete, Stopped }`
  - `Relay` gains:
    ```rust
    fn put_blob(&self, vault: &VaultAuth, blob: Uuid, sealed_len: u64, body: BlobBody) -> impl Future<Output = RelayResult<()>> + Send;
    fn head_blob(&self, vault: &VaultAuth, blob: Uuid) -> impl Future<Output = RelayResult<Option<u64>>> + Send;
    fn get_blob(&self, vault: &VaultAuth, blob: Uuid, sink: &mut (dyn BlobSink + Send)) -> impl Future<Output = RelayResult<Delivery>> + Send;
    fn list_blobs(&self, vault: &VaultAuth, after: Option<Uuid>, limit: u32) -> impl Future<Output = RelayResult<BlobPage>> + Send;
    fn delete_blob(&self, vault: &VaultAuth, blob: Uuid) -> impl Future<Output = RelayResult<()>> + Send;
    ```
  - `HttpRelay` derives `Clone` (Task 12 clones it into the lanes). URLs use `blob.hyphenated()` lowercase only. `put_blob`: `Body::wrap_stream(futures_util::stream::iter(body))` with an explicit `CONTENT_LENGTH = sealed_len`, `application/octet-stream`, timeout `BLOB_TIMEOUT = 30 min`; `sealed_len > MAX_BLOB_SEALED` is refused before sending (`Protocol("body too large")`). `get_blob`: requires `Content-Length` (`Protocol("length")` otherwise), refuses one over `MAX_BLOB_SEALED` before reading, calls `sink.begin`, feeds every `response.chunk()` to `sink.piece`, returns `Stopped` as soon as the sink says so; timeout `BLOB_TIMEOUT`. `head_blob`: `200` → `Some(content_length)`, `404` → `None`. `delete_blob`: `204` and `404` are success. `list_blobs`: `limit` in `1..=MAX_BLOB_PAGE` else `Protocol`; every `id` must parse as a canonical UUID, else `Protocol("unexpected answer")`.
  - Fake (`FakeRelay`), routes `PutBlob, HeadBlob, GetBlob, ListBlobs, DeleteBlob`; read entitlement for `HEAD`/`GET`/list (a lapsed account still reads, as for ops), write entitlement and current token for `PUT`/`DELETE`; `PUT` drains the body, refuses a length mismatch (`BadRequest("length_required")`), answers success without storing again for an existing blob; knobs: `set_blob_quota(bytes: u64)` (`QuotaExceeded` past it), `set_no_room(on: bool)` (`InsufficientStorage` on every `PUT`), `set_failing(route: Route, error: Option<RelayError>)` (a sticky failure, unlike `fail_next`), `tamper_blob(vault_id: &str, blob: Uuid)` (flips one byte), `truncate_blob(vault_id, blob, keep: usize)`, `lose_blob(vault_id, blob)`, `set_blob_created_at(vault_id, blob, at: i64)`, `set_server_now(at: Option<i64>)` (what `list_blobs` reports; default the fake's `now()`), `blob_ids(vault_id) -> Vec<Uuid>`, `blob_puts() -> usize`. `FakeState::server_info` defaults to `blobs: true`.

- [ ] **Step 1: Write the failing tests.** Add to `every_documented_status_and_code_maps_to_its_variant`: `(404, "blob_not_found", E::BlobNotFound)`, `(400, "invalid_blob_id", bad("invalid_blob_id"))`, `(507, "anything", E::InsufficientStorage)`; and `RelayError::from_response(507, b"<html>") == E::InsufficientStorage`. In `relay/http/blob_tests.rs` (one-shot servers, as `http/tests.rs`):

```rust
#[tokio::test] async fn put_blob_streams_the_body_with_its_length_and_bearer() // request line "PUT /v1/vaults/<id>/blobs/<uuid> HTTP/1.1"; content-length == sealed_len; body == concatenated pieces; bearer header present
#[tokio::test] async fn a_body_over_the_blob_cap_is_refused_before_sending() // sealed_len = MAX_BLOB_SEALED + 1 → Err(Protocol("body too large")), the server saw no request
#[tokio::test] async fn head_blob_reads_the_size_or_none()                // 200 + content-length 1234 → Some(1234); 404 → None
#[tokio::test] async fn get_blob_hands_every_byte_to_the_sink()          // sink.begin(len) once; pieces concatenate to the body; Ok(Delivery::Complete)
#[tokio::test] async fn get_blob_without_a_length_is_a_protocol_error()  // chunked answer → Err(Protocol("length")), sink never begun
#[tokio::test] async fn get_blob_over_the_cap_is_refused_before_reading()
#[tokio::test] async fn get_blob_stops_when_the_sink_stops()             // sink answers Stop on the first piece → Ok(Delivery::Stopped)
#[tokio::test] async fn a_missing_blob_is_blob_not_found()               // 404 {"error":"blob_not_found"} → Err(BlobNotFound)
#[tokio::test] async fn list_blobs_parses_ids_sizes_creation_and_the_server_now() // body "now": 1794650400 → server_now == Some(1_794_650_400); no "now" field → None; after= and limit= in the query
#[tokio::test] async fn a_listing_with_a_bad_id_is_a_protocol_error()    // id "../x" → Err(Protocol(_))
#[tokio::test] async fn delete_blob_treats_404_as_gone()
#[tokio::test] async fn server_info_reads_the_blobs_feature_and_ignores_others() // features ["x","blobs"] → blobs true; field missing → false; ["BLOBS"] → false
```

In `relay/fake/tests.rs`: `the_fake_keeps_blobs_per_vault_and_checks_entitlement`, `the_fake_answers_the_quota_and_no_room`, `a_put_of_an_existing_blob_succeeds_without_storing_again` (`blob_puts()` counts both calls, the stored bytes are the first ones), `a_tampered_or_truncated_blob_is_served_as_changed`.

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::relay` → FAIL (compile).
- [ ] **Step 3: Implement** the contract, the HTTP calls, the fake (`relay/fake/blobs.rs` holds `BTreeMap<Uuid, StoredBlob { bytes, created_at }>` per vault) and the `Counting` pass-through in `relay_e2e.rs` (it is `#[cfg(test)]` and must compile).
- [ ] **Step 4: Run** `cd src-tauri && cargo test --lib services::sync` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): blob calls in the relay contract, the HTTP client and the fake`.

---

### Task 4: Migration for the blob queue

**Files:**
- Modify: `src-tauri/src/db/migrations.rs` (const + `all_migrations()` entry), `src-tauri/src/db/schema.snapshot` (regenerate), `src-tauri/src/services/sync/registry/tables.rs` (Local entry), `src-tauri/src/services/sync/registry/keys.rs` (`MIGRATION_CLASSES` entry `A`; rename `schema_number_is_34_and_the_newest_barrier_is_025` to the new number), `src-tauri/src/services/sync/registry/tests.rs` (the class list), `src-tauri/src/services/sync/state.rs` (`disable` deletes from `sync_blobs`), `docs/architecture/database.md` (Device sync catalog row, the migration list)

**Interfaces:**
- Produces: migration `0NN_sync_blobs` where `0NN` is the next free number at execution time (read `all_migrations()`; on this tree it would be `035`). Everywhere below, `0NN_sync_blobs` means that name. SQL:

```sql
CREATE TABLE sync_blobs (
    blob_id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('up', 'down', 'done')),
    thumbnail INTEGER NOT NULL DEFAULT 0 CHECK (thumbnail IN (0, 1)),
    wanted INTEGER NOT NULL DEFAULT 0 CHECK (wanted IN (0, 1)),
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    row_at INTEGER NOT NULL DEFAULT 0,
    queued_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_try_at INTEGER NOT NULL DEFAULT 0,
    last_error TEXT CHECK (last_error IN
        ('network', 'server', 'notOnServer', 'unreadable', 'noRoom', 'invalidId', 'local')),
    last_error_at INTEGER
) WITHOUT ROWID;
CREATE INDEX idx_sync_blobs_state ON sync_blobs (state, next_try_at);
```

  `state`: `up` = held here, the server has not confirmed it; `down` = referenced, not held here; `done` = held here and known to the server. `row_at` = the owning row's `uploaded_at` (documents) or `created_at` (photos).

- [ ] **Step 1: Write the failing test** in `state.rs` tests: `turning_sync_off_forgets_the_blob_queue` (insert one `sync_blobs` row, `disable`, count 0). The registry tests (`every_table_of_the_migrated_schema_has_a_class`, the class list, `every_migration_has_a_class_and_no_class_is_stale`, the renamed schema-number test) fail once the migration exists.
- [ ] **Step 2: Add the migration, run** `cd src-tauri && cargo test --lib services::sync::registry` → FAIL until classified; then classify.
- [ ] **Step 3: Regenerate the snapshot** `cd src-tauri && cargo test regenerate_schema_snapshot -- --ignored`; the diff shows only `sync_blobs` and its index.
- [ ] **Step 4: Run** `cd src-tauri && cargo test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): a local queue table for document and photo content`.

---

### Task 5: Attachment rows sync while they name a blob

**Files:**
- Modify: `src-tauri/src/services/sync/registry/{mod,tables}.rs`, `registry/tests.rs`, `capture/triggers.rs`, `capture/tests.rs`, `journal/read.rs`, `journal/tests.rs`, `snapshot/mod.rs`, `snapshot/attachments.rs`, `snapshot/tests/{attachments,stream}.rs`, `test_support.rs` (`dump` compares only rows that sync), `engine/tests/repair.rs` and `impact.rs` tests where they insert attachment rows (adjust to capture)
- Create: `docs/architecture/decisions/0030-device-sync-documents-and-photos.md` (next free number; add its row to `docs/architecture/decisions/README.md`)
- Docs: `docs/DEPRECATED.md` (new section "Retired with synced documents and photos": `snapshot::attachments::{keep, reattach, discard}` and the `temp.sync_kept_*` tables → the carry count of this task), `docs/architecture/database.md` (the four tables are Synced; `file_path`, `thumbnail_path` Local), `docs/architecture/overview.md` (the "Replaces and their backups" bullet: rows come from the vault, content kept by blob ID)

**Interfaces:**
- Produces:
  - `TableSpec` gains `pub requires: Option<&'static str>` (BASE `None`): "a row syncs only while this column is not NULL". `real_estate_photos` (`local: &["file_path", "thumbnail_path"]`, `requires: Some("blob_id")`), `real_estate_documents` and `insurance_documents` (`local: &["file_path"]`, `requires: Some("blob_id")`), `real_estate_photo_batches` (`requires: None`), all `TableClass::Synced`. `pub fn registry::row_condition(spec: &TableSpec, alias: &str) -> Option<String>` → `{alias}."blob_id" IS NOT NULL`.
  - Capture (`triggers.rs`): `row_filter` takes the spec and combines the `app_config` allowlist with `requires`; the `_upd` trigger adds `AND {old_ok}`; for a table with `requires = Some(c)` two more triggers: `sync_{t}_enrol` (`AFTER UPDATE OF "c"`, local mode, `OLD.c IS NULL AND NEW.c IS NOT NULL`: tick, fresh `'*'` with this device, clear column clocks, queue) and `sync_{t}_leave` (`OLD.c IS NOT NULL AND NEW.c IS NULL`: tick, `'-'`, clear column clocks, queue). This is how capture learns that a converted legacy row is new: it travels as a complete insert with a fresh generation.
  - Journal (`Reader::shape`): the filter adds the row condition, so a row without its blob reads as absent (its tombstone if any, else `None`).
  - Snapshot: `live_keys` adds the row condition; content never enters a snapshot. `snapshot::attachments` becomes: `pub(super) struct Mine` (this device's document and photo `(table, id)` pairs, legacy rows included), `pub(super) fn mine(conn: &Connection) -> Result<Mine>` before `clear`, `pub(super) fn carry(conn: &Connection, mine: Mine) -> Result<AttachmentCarry>` after the refill (`kept` = ids present again, `orphaned` = the rest). `replace()` drops `keep`/`reattach`/`discard`. Blobs of rows the vault has stay (no download later); the others are purged by the existing `repair::full` sweep and remain only in the backup.

- [ ] **Step 1: Write the failing tests.**

`registry/tests.rs` — replace `the_attachment_tables_are_local_in_v1` with `the_attachment_tables_sync_only_with_their_blob` (class `Synced` for all four; `requires == Some("blob_id")` for the three row tables, `None` for batches; `column_class("real_estate_photos", "thumbnail_path") == ColumnClass::Local`); move the four names from `local` to `synced` in the class-list test.

`capture/tests.rs`:

```rust
#[test] fn a_document_with_a_blob_is_captured_like_any_row()        // outbox has ('insurance_documents', id); '*' clock with this device
#[test] fn a_legacy_row_without_a_blob_is_not_captured()             // insert with file_path, blob_id NULL: no outbox row, no sync_clock row; deleting it records nothing
#[test] fn a_converted_legacy_row_travels_as_a_complete_new_row()    // legacy row, then UPDATE blob_id: outbox has it; '*' newer than every earlier clock; read_row(..).insert and every carried column present; applied on a second device the row equals the first's (test_support::assert_converged)
#[test] fn clearing_a_blob_records_a_tombstone()                     // UPDATE blob_id = NULL on a docs row with file_path: '-' clock, read_row is a tombstone-only change
#[test] fn deleting_a_policy_tombstones_its_documents_at_the_policy_clock()
#[test] fn an_edit_of_a_legacy_row_records_nothing()                 // UPDATE name on a blob-less row: no outbox
```

`journal/tests.rs`: `the_journal_reads_a_row_without_its_blob_as_absent`. `snapshot/tests/stream.rs` (or `mod.rs`): `a_snapshot_carries_attachment_rows_but_no_legacy_ones_and_no_content` (record rows include the blob row, exclude the legacy row; no record holds the blob's bytes).

`snapshot/tests/attachments.rs`, rewritten (keep its helpers):

```rust
#[test] fn a_resync_keeps_documents_the_vault_has_and_their_blobs()   // report.attachments == carry(1, 0); readable(&c, &blob)
#[test] fn photos_keep_their_batch_and_both_blobs()
#[test] fn a_document_only_this_device_had_is_gone_and_counted()      // carry(0, 1); row gone; blob purged
#[test] fn a_joiner_gets_the_vaults_attachment_rows_without_content() // rows present with blob ids; readable(..) false for both blobs
#[test] fn a_legacy_row_is_counted_as_orphaned()
#[test] fn a_second_replace_back_to_back_adds_no_duplicate_attachments()
#[test] fn a_failed_replace_leaves_attachments_as_they_were()
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync` → FAIL.
- [ ] **Step 3: Implement** registry, triggers, reader, dump, carry, `test_support::dump` filter. Fix the tests that inserted attachment rows assuming they were Local (impact, repair).
- [ ] **Step 4: Write ADR 0030** ("Device sync: documents and photos"), Status Accepted, recording every decision of this plan, each one line: rows sync while they name a blob (`requires`), legacy rows enrol as new rows through `_enrol`; the blob stream (STREAM, `moony sync blob v1`, blob ID as object ID, epoch in the header, sealed once, never re-sealed on rotation); no checksum column; rows never held for content and why (snapshots need an empty outbox); `sync_blobs` reconciled from rows and store; pre-push uploads (20 s, blobs up to 8 MiB) and two lanes beside the rounds; HEAD before PUT; on-demand full content on phones, eager on desktops, thumbnails at once; the daily device pass (24 h, 7 days by the server clock, fully caught up only, at most 1,000 deletes); the replace keeps content by blob ID; the assumed server error codes; known limits (no resume; a row unpushed for over 7 days may lose its blob until it goes out; a blob whose only holder is wiped before upload is lost).
- [ ] **Step 5: Run** `cd src-tauri && cargo test` → PASS. **Commit** — `feat(sync): documents and photos sync as rows while they name a blob`.

---

### Task 6: Enrol the attachment rows of a profile that synced before this build

**Files:**
- Create: `src-tauri/src/services/sync/capture/enrol.rs`
- Modify: `capture/mod.rs` (`pub mod enrol;`), `registry/keys.rs`, `src-tauri/src/services/sync/mod.rs` (`open_capture`, its tests)

**Interfaces:**
- Consumes: Task 4's migration name, Task 5's `requires`.
- Produces:
  - `registry::keys::ATTACHMENTS_SYNCED_MIGRATION: &str = "0NN_sync_blobs";` and `pub fn migration_position(name: &str) -> Option<u32>` (1-based in `known_migration_names()`).
  - `capture::enrol::enrol_unclocked(conn: &Connection, device: DeviceId, tables: &[&str]) -> Result<usize>` — needs capture installed; for each table in order, every row that passes the row condition and has neither `'*'` nor `'-'` in `sync_clock` gets `'*'` = `sync_hlc_tick()` with `device` and an outbox entry. Tables: `["real_estate_photo_batches", "real_estate_photos", "real_estate_documents", "insurance_documents"]` (every unclocked batch, so a later converted photo never waits for a parent the vault lacks).
  - `open_capture`: `let enrol = sync.schema_seen < migration_position(ATTACHMENTS_SYNCED_MIGRATION)`; after `capture::install`, in a savepoint, `enrol_unclocked(..)` when `enrol`; `state::note_schema_seen` moves after it (still only when `capture_ok`), so a failed enrolment is tried again at the next unlock.

- [ ] **Step 1: Write the failing tests** in `sync/mod.rs` tests:

```rust
#[test] fn a_profile_that_synced_before_attachments_enrols_them_once() // enable; set schema_seen = position - 1; uninstall capture; insert a policy document with a blob, a batch and a photo without clocks; on_open_with(&conn, true): all three in sync_outbox with '*' of this device; schema_seen == schema_number(); a second on_open queues nothing new (outbox emptied first)
#[test] fn a_profile_set_up_after_attachments_enrols_nothing()       // schema_seen already current: outbox unchanged
#[test] fn enrolment_skips_legacy_rows_and_rows_with_a_clock()
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::tests` → FAIL.
- [ ] **Step 3: Implement.** One `INSERT … SELECT` per table into `sync_clock` (with `CLOCK_UPSERT`) and one into `sync_outbox` (`ON CONFLICT DO NOTHING`).
- [ ] **Step 4: Run** `cd src-tauri && cargo test --lib services::sync` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): attachment rows of an already synced profile are sent once`.

---

### Task 7: The blob queue and the content state of a row

**Files:**
- Create: `src-tauri/src/services/sync/blobs/mod.rs`, `blobs/queue.rs`, `blobs/content.rs`, `blobs/tests/mod.rs`, `blobs/tests/queue.rs`
- Modify: `src-tauri/src/services/sync/mod.rs` (`pub mod blobs;`), `src-tauri/src/models/attachments.rs` (`AttachmentContent`; registered for the drift detector in Task 11, carried by a model from Task 14), `src-tauri/src/services/sync/repair/invariants.rs`, `src-tauri/src/services/attachment_store.rs` (`rows_without_content`), `src-tauri/src/services/attachment_conversion.rs` (status uses it; no logic change), `docs/architecture/database.md` (`sync_blobs` semantics)

**Interfaces:**
- Produces:
  - `models::attachments::AttachmentContent` — `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]`, variants with per-variant `#[serde(rename = …)]`: `Here "here"`, `WaitingUpload "waitingUpload"`, `NotHere "notHere"`, `Downloading "downloading"`, `NotOnServer "notOnServer"`, `Failed "failed"`, `Missing "missing"`.
  - `blobs::canonical_id(text: &str) -> Option<uuid::Uuid>` (parses, and the text equals `uuid.hyphenated().to_string()`).
  - `blobs::queue`:
    ```rust
    pub enum BlobState { Up, Down, Done }
    pub enum TransferError { Network, Server, NotOnServer, Unreadable, NoRoom, InvalidId, Local } // as_str() == the last_error codes
    pub struct Transfer { pub blob_id: String, pub state: BlobState, pub thumbnail: bool, pub wanted: bool, pub attempts: u32, pub size: Option<u64> }
    pub struct Reconciled { pub queued_up: usize, pub queued_down: usize, pub dropped: usize, pub changed: usize }
    pub fn reconcile(conn: &Connection, now: i64) -> Result<Reconciled>;          // Ok(default) without the files store or with sync off
    pub fn next_upload(conn: &Connection, now: i64, max_plain: Option<u64>) -> Result<Option<Transfer>>;
    pub fn next_download(conn: &Connection, now: i64, eager_full: bool) -> Result<Option<Transfer>>;
    pub fn set_active(conn: &Connection, blob_id: &str, active: bool) -> Result<()>;
    pub fn clear_active(conn: &Connection) -> Result<()>;
    pub fn note_done(conn: &Connection, blob_id: &str) -> Result<()>;
    pub fn note_failed(conn: &Connection, blob_id: &str, error: TransferError, now: i64) -> Result<()>;
    pub fn note_gone(conn: &Connection, blob_id: &str) -> Result<()>;
    pub fn requeue_upload(conn: &Connection, blob_id: &str, now: i64) -> Result<bool>; // 'done' and held → 'up'
    pub fn want_row(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<()>;   // the row's full blob: wanted = 1, next_try_at = 0
    pub fn retry_row(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<()>;  // + attempts = 0, last_error = NULL
    pub fn want_blob(conn: &Connection, blob_id: &str) -> Result<bool>;
    pub fn want_all(conn: &Connection) -> Result<u32>;   // rows whose full content is now wanted
    pub fn retry_now(conn: &Connection) -> Result<()>;   // next_try_at = 0 for every 'up'/'down' except noRoom and invalidId
    pub fn uploads_waiting(conn: &Connection) -> Result<u32>; // rows; part C of the sync-aware-app design reads this
    pub struct AttachmentSummary { pub waiting_upload: u32, pub waiting_upload_bytes: i64, pub not_here: u32, pub not_on_server: u32, pub failed: u32 }
    pub fn summary(conn: &Connection) -> Result<AttachmentSummary>; // counts rows (a photo once), not blobs
    ```
  - `blobs::content::ContentReader<'c>` — `pub fn new(conn: &'c Connection) -> Result<Self>` (reads once whether sync is on and the store attached); `pub fn of(&self, blob_id: Option<&str>) -> Result<AttachmentContent>`.

  Reconcile semantics (set-based SQL over the three row tables, rows with a blob only; `held` = complete in `files.blobs`): drop entries no row references; add each new referenced blob as `up` when held, `down` otherwise (`thumbnail` from `thumb_blob_id`, `row_at` from the row, `queued_at = now`); `done`/`up` not held → `down`; `down` held → `done`. A blob ID that `canonical_id` rejects is added as `down` with `last_error = 'invalidId'`, `next_try_at = i64::MAX`.

  Ordering: `next_upload` — `up`, due, size ≤ `max_plain` when given, by `queued_at, blob_id`. `next_download` — `down`, due, and (`wanted` or `thumbnail` or `eager_full`), by `wanted DESC, thumbnail DESC, row_at, blob_id`.

  Backoff after the n-th failure: `Network | Server | Local` 30 s·2^(n−1) ≤ 1,800 s; `NotOnServer` 300 s·2^(n−1) ≤ 3,600 s; `Unreadable` 600 s·2^(n−1) ≤ 21,600 s; `NoRoom` 1,800 s; `InvalidId` never (`i64::MAX`).

  Content: no blob ID (legacy row) → `Here`; store unavailable → `Here` (the existing unavailable path reports it); held: entry `up` (or none while sync is on) → `WaitingUpload`, else `Here`; not held: no entry → `Missing`; `active` → `Downloading`; `last_error` `notOnServer` → `NotOnServer`; `unreadable` or `invalidId` → `Failed`; otherwise `NotHere`.

  Wiring: `repair::invariants::run` calls `blobs::queue::reconcile(conn, now)` whenever an attachment row table was touched or an `ATTACHMENT_OWNERS` table lost rows (after `delete_unreferenced`), so a merge or a restore queues the content of new rows. `attachment_store::rows_without_content` leaves out rows whose missing blob has a `down` entry (with sync on, content on its way is not "missing").

- [ ] **Step 1: Write the failing tests** in `blobs/tests/queue.rs` (in-memory `migrated()` + `ATTACH ':memory:' AS files` + `files_schema::migrate`, sync enabled with `state::enable`):

```rust
#[test] fn reconcile_queues_held_blobs_up_and_missing_ones_down()
#[test] fn reconcile_drops_entries_no_row_references()
#[test] fn a_lost_local_copy_goes_back_to_down_and_a_found_one_to_done()
#[test] fn a_row_without_a_blob_queues_nothing()
#[test] fn canonical_id_accepts_only_lowercase_hyphenated_uuids() // "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60" ok; uppercase, braces, "../x", 32-hex form → None
#[test] fn an_invalid_blob_id_is_queued_failed_and_never_offered()
#[test] fn downloads_come_wanted_first_then_thumbnails_then_oldest_rows()
#[test] fn a_phone_is_offered_only_thumbnails_and_wanted_content() // eager_full = false
#[test] fn uploads_come_oldest_first_and_respect_the_size_limit()
#[test] fn failures_back_off_by_their_kind()                         // exact next_try_at for n = 1, 2 and at the cap, per kind
#[test] fn want_all_marks_every_full_download_and_counts_rows()
#[test] fn summary_counts_rows_not_blobs()                           // one photo with both blobs 'up' counts once; bytes = sum of both sizes
#[test] fn content_follows_the_queue_and_the_store()                 // every AttachmentContent variant, and Here with sync off
#[test] fn a_merge_queues_the_content_of_the_rows_it_brought()       // repair::after_merge with touched insurance_documents → 'down' entry
#[test] fn content_on_its_way_is_not_missing_content()               // rows_without_content == 0 with a 'down' entry; == 1 without
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::blobs` → FAIL.
- [ ] **Step 3: Implement** `queue.rs`, `content.rs`, the hooks.
- [ ] **Step 4: Run** `cd src-tauri && cargo test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): a queue of document and photo content reconciled from the rows`.

---

### Task 8: Convergence with documents and photos

**Files:**
- Create: `src-tauri/src/services/sync/convergence/ops/attachments.rs`
- Modify: `convergence/ops/mod.rs` (catalog), `convergence/mod.rs` (every device and every joiner gets an in-memory `files` store attached and migrated), `convergence/tests.rs`

**Interfaces:**
- Consumes: `attachments::{add_insurance_document_bytes, add_real_estate_document_bytes, delete_document, delete_photo, delete_photo_batch}`, `real_estate_photos::record_photo` (with a `PreparedPhoto` built from two tiny byte strings — add a `#[cfg(test)] pub(crate) fn prepared_for_test(full: Vec<u8>, thumb: Vec<u8>) -> PreparedPhoto` in `real_estate_photos.rs`), an existing document rename service if one exists (grep `UPDATE insurance_documents SET name`; skip the op if none).
- Produces: `ops::attachments::OPS` with `add_policy_document`, `add_property_document`, `add_photo` (into a new or the property's newest batch), `delete_document`, `delete_photo`, `delete_photo_batch`; targets picked by content (name, then `'*'` clock) like the other ops. Device invariant `fn queue_consistent(conn: &Connection)`: after `blobs::queue::reconcile`, every referenced blob is held or has a `down` entry, and no entry is unreferenced.

- [ ] **Step 1: Write the failing test** `attachments_converge_with_cascades_on_two_devices` in `convergence/tests.rs`: a two-device vault over fixed seeds where at least a third of the operations are attachment ops plus `delete_policy`/`delete_property`; at the end `assert_converged` (the attachment tables are compared through `test_support::dump`) and `queue_consistent` on each device. The existing random suites pick the new ops up through the catalog.
- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::convergence` → FAIL (ops missing).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same → PASS (all seeds).
- [ ] **Step 5: Commit** — `test(sync): documents, photos and their cascades converge on every device`.

---

### Task 9: Streams between the store and the wire

**Files:**
- Create: `src-tauri/src/services/sync/blobs/stream.rs`, `blobs/tests/stream.rs`, `src-tauri/tests/sync_blob_memory.rs`

**Interfaces:**
- Consumes: Task 1 (`BlobSealer`, `BlobOpener`, `sealed_len`), Task 2 (`read_at`, `begin_incoming`, `write_incoming`, `finish_incoming`, `abandon_incoming`), Task 3 (`BlobBody`, `BlobSink`, `SinkFlow`), `engine::ConnAccess`.
- Produces:
  ```rust
  pub struct UploadSource<D: ConnAccess> { … }
  impl<D: ConnAccess> UploadSource<D> {
      pub fn new(db: D, blob_id: String, blob: Uuid, plain_len: u64, sealer: BlobSealer) -> Self;
      pub fn sealed_len(&self) -> u64;
      pub fn into_body(self) -> BlobBody;            // yields the header, then one sealed chunk per item
  }
  impl<D: ConnAccess> Iterator for UploadSource<D> { type Item = std::io::Result<Vec<u8>>; } // each item reads ≤ CHUNK under one with_conn, never holding the lock between items
  pub struct StoreSink<D: ConnAccess> { … }
  impl<D: ConnAccess> StoreSink<D> {
      pub fn new(db: D, blob_id: String, blob: Uuid, keys: VaultKeys, vault_id: [u8; 16]) -> Self;
      pub fn finish(self) -> SinkOutcome;            // completes or abandons the incoming blob
  }
  impl<D: ConnAccess> BlobSink for StoreSink<D> { … } // begin: BlobOpener::new + begin_incoming(plain_len); piece: open, write_incoming at the running offset, hash; any failure → Stop and remember why
  pub enum SinkOutcome { Stored, Unreadable, Incomplete, Local(AppError) }
  ```
  Decrypted bytes go only into `attachments.db` (no temporary file); the SHA-256 written is that of the plaintext.

- [ ] **Step 1: Write the failing tests.** `blobs/tests/stream.rs` (a `TestConn`-like `ConnAccess` over an in-memory connection with the store attached):

```rust
#[test] fn a_blob_goes_through_the_wire_pieces_and_lands_under_its_id() // 3*CHUNK+5 bytes: UploadSource → pieces → StoreSink (another connection) → Stored; read_all equal; files.blobs.sha256 == hex(sha256(plain))
#[test] fn an_empty_blob_and_a_whole_chunk_blob_round_trip()
#[test] fn a_tampered_stream_stores_nothing()                          // flip a byte: SinkOutcome::Unreadable; is_complete false; no incomplete row left
#[test] fn a_truncated_stream_stores_nothing()                         // deliver one piece less than announced: Incomplete; nothing left
#[test] fn a_locked_profile_stops_the_source()                         // with_conn answers Auth: the iterator yields Err and stops
#[test] fn the_source_never_holds_the_connection_between_pieces()      // the test's ConnAccess counts open with_conn calls: never more than one at a time, zero between next() calls
```

`src-tauri/tests/sync_blob_memory.rs`: a counting `#[global_allocator]` (live bytes and peak, `AtomicUsize`), an in-memory main connection with `files` attached (SQLite's own allocations do not go through Rust's allocator), a 100 MiB blob stored with `attachment_store::put_reader` from a pseudo-random reader, then measurement starts:

```rust
#[test] fn a_100_mib_blob_seals_and_opens_within_a_fixed_memory_bound() {
    // reset the peak; drive UploadSource for the 100 MiB blob into StoreSink (another id, same store)
    assert!(peak_growth < 2 * 1024 * 1024, "peak growth {peak_growth} bytes");
    // sha256 of the copy equals the original's
}
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::blobs && cargo test --test sync_blob_memory` → FAIL.
- [ ] **Step 3: Implement.** `StoreSink` keeps the plaintext chunk only while it writes it; `UploadSource` reads into one reused `CHUNK` buffer.
- [ ] **Step 4: Run** the same → PASS.
- [ ] **Step 5: Commit** — `feat(sync): stream blobs between the encrypted store and the wire in chunks`.

---

### Task 10: Uploading and downloading one blob

**Files:**
- Create: `src-tauri/src/services/sync/blobs/policy.rs`, `blobs/transfer.rs`, `blobs/tests/transfer.rs`
- Modify: `src-tauri/src/models/sync.rs` (`SyncAttachmentsProblem`)

**Interfaces:**
- Consumes: Tasks 3, 7, 9.
- Produces:
  ```rust
  // policy.rs
  #[derive(Clone)] pub struct BlobPolicy { … }   // Arc inside: state mutex, the two lane locks (tokio Mutex), a Notify, eager_full
  impl BlobPolicy {
      pub fn new(eager_full: bool) -> Self;
      pub fn for_this_platform() -> Self;          // eager_full = !cfg!(any(target_os = "ios", target_os = "android"))
      pub fn eager_full(&self) -> bool;
      pub async fn server_has_blobs<R: Relay>(&self, relay: &R, now: i64) -> bool; // GET /v1/server cached 6 h; a failed ask → false, no problem set
      pub fn forget_server(&self);
      pub fn problem(&self) -> Option<SyncAttachmentsProblem>;
      pub fn uploads_paused(&self, now: i64) -> bool;
      pub fn note_no_room(&self, now: i64);         // problem NoRoom, uploads paused 1,800 s
      pub fn note_uploaded(&self);                  // clears NoRoom
      pub fn note_server(&self, blobs: bool, now: i64);
      pub fn wake(&self);
      pub async fn wait(&self, at_most: Duration);
      pub fn up_lane(&self) -> &tokio::sync::Mutex<()>;
      pub fn down_lane(&self) -> &tokio::sync::Mutex<()>;
  }
  impl Default for BlobPolicy { fn default() -> Self { Self::new(true) } }
  // transfer.rs
  pub enum Outcome { Done, Gone, Failed(TransferError), Blocked, Locked }
  pub struct TransferReport { pub uploaded: usize, pub stored: usize, pub failed: usize, pub changed: bool }
  pub async fn upload_one<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, transfer: &Transfer, now: i64) -> Outcome;
  pub async fn download_one<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, transfer: &Transfer, now: i64) -> Outcome;
  pub async fn run_uploads<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, policy: &BlobPolicy, until: Option<tokio::time::Instant>, max_plain: Option<u64>) -> Result<TransferReport>;
  pub async fn run_downloads<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, policy: &BlobPolicy, until: Option<tokio::time::Instant>) -> Result<TransferReport>;
  ```
  This task creates `models::sync::SyncAttachmentsProblem` (`#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]`, `NoRoom "noRoom"`, `ServerNeedsUpdate "serverNeedsUpdate"`); Task 11 puts it on the wire.

  `upload_one`: the blob must still be referenced and complete (else `Gone`, entry dropped); `canonical_id` (else `Failed(InvalidId)`); `head_blob` — `Some(_)` → `Done` without a `PUT`; else `put_blob` with an `UploadSource` sealed under the current epoch. `download_one`: `get_blob` into a `StoreSink`; `Delivery::Complete` + `Stored` → `Done`; `Unreadable` / `Incomplete` → `Failed(Unreadable)`; `Stopped` with `Local` → `Failed(Local)`. Error mapping: `Network` → `Network`; `Server | Protocol | RateLimited` → `Server`; `BlobNotFound` → `NotOnServer`; `QuotaExceeded | InsufficientStorage` → `NoRoom` (and `policy.note_no_room`); `BadRequest("invalid_blob_id")` → `InvalidId`; `PaymentRequired | Unauthorized | TokenReadOnly | VaultNotFound` → `Blocked` (the round shows those; nothing is noted); a locked profile → `Locked`. `run_*`: take the lane lock, `reconcile` first, then loop `next_*` → `set_active` → transfer → note → `set_active(false)` until none is due, `until` passes, or `Blocked`/`Locked`; nothing runs when `server_has_blobs` is false (then `note_server(false)` sets `ServerNeedsUpdate` if any entry waits) or, for uploads, while paused. A transfer error never becomes an `Err`; only a local SQL error does.

- [ ] **Step 1: Write the failing tests** in `blobs/tests/transfer.rs` (FakeRelay vault as in `engine/tests/mod.rs::vault()`, both devices with the store attached):

```rust
#[tokio::test] async fn an_upload_reaches_the_server_and_is_done()          // relay.blob_ids(vault) contains it; entry 'done'; content Here
#[tokio::test] async fn a_blob_the_server_has_is_confirmed_without_a_put() // second device with the same blob: blob_puts() unchanged
#[tokio::test] async fn a_download_stores_the_content_under_its_id()       // bytes and sha256 equal the uploader's
#[tokio::test] async fn content_the_server_lacks_is_not_on_server()        // Failed(NotOnServer); content NotOnServer; next_try_at = now + 300
#[tokio::test] async fn a_tampered_or_truncated_blob_is_not_stored_and_shows_failed() // tamper_blob / truncate_blob: nothing stored, content Failed, retried later
#[tokio::test] async fn no_room_pauses_uploads_and_says_so()                // set_no_room(true) and set_blob_quota(1): policy.problem() == Some(NoRoom); downloads still run
#[tokio::test] async fn a_server_without_blobs_transfers_nothing()         // set_server_info(.. blobs false ..): no blob route called; problem ServerNeedsUpdate while something waits
#[tokio::test] async fn a_row_deleted_during_a_transfer_drops_its_entry()  // delete the document between next_upload and upload_one: Gone; entry gone; no PUT
#[tokio::test] async fn an_invalid_blob_id_is_never_requested()            // a row with blob_id "../../x": no relay call names it; content Failed
#[tokio::test] async fn a_lapsed_account_still_downloads_but_does_not_upload()
#[tokio::test] async fn a_phone_fetches_only_thumbnails_until_asked()      // BlobPolicy::new(false): thumbnail stored, full not; after want_row, full stored
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::blobs` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** — `feat(sync): upload and download documents and photos one blob at a time`.

---

### Task 11: Uploads in the round and the sync status

**Files:**
- Modify: `src-tauri/src/services/sync/engine/mod.rs` (`round_steps`), `engine/snapshots.rs` (`SnapshotPolicy` gains `pub blobs: BlobPolicy`, `Default`), `engine/status.rs` (`Runtime.attachments_problem`, `SyncStatus.attachments`), `src-tauri/src/models/sync.rs` (`SyncAttachments`; `SyncStatus.attachments`), `src-tauri/src/bindings.rs`, `shared/generated-types.ts` (regenerate), `shared/schema.ts`, every TS fixture that builds a `SyncStatus` (typecheck finds them)
- Create: `src-tauri/src/services/sync/engine/tests/blobs.rs` (and `mod blobs;` in `engine/tests/mod.rs`, plus a `with_store(&TestConn)` helper there)

**Interfaces:**
- Consumes: Tasks 7 and 10.
- Produces:
  - In `round_steps`, between the pull and `push::push_all`, when the round may push and `mode != RoundMode::Drain`: `blobs::transfer::run_uploads(relay, db, creds, &policy.blobs, Some(now_instant + PRE_PUSH_BUDGET), Some(PRE_PUSH_MAX_PLAIN))` with `pub const PRE_PUSH_BUDGET: Duration = Duration::from_secs(20)` and `pub const PRE_PUSH_MAX_PLAIN: u64 = 8 * 1024 * 1024` (in `blobs/transfer.rs`). The lane lock is taken with `tokio::time::timeout(PRE_PUSH_BUDGET, …)`: a lane busy with a long transfer means the round skips this step. Its result never fails the round (a local error is logged at warn and dropped).
  - `models::sync::SyncAttachments { #[serde(rename = "waitingUpload")] waiting_upload: u32, #[serde(rename = "waitingUploadBytes")] waiting_upload_bytes: i64, #[serde(rename = "notHere")] not_here: u32, #[serde(rename = "notOnServer")] not_on_server: u32, #[serde(rename = "failed")] failed: u32, #[serde(rename = "problem")] problem: Option<SyncAttachmentsProblem> }` (`Default`), `SyncStatus.attachments: SyncAttachments` (`#[serde(rename = "attachments")]`), zeros while sync is off; filled from `blobs::queue::summary` and `Runtime.attachments_problem`.
  - `shared/schema.ts`: `export type AttachmentContent = …` (Task 14 uses it), `export type SyncAttachmentsProblem = 'noRoom' | 'serverNeedsUpdate'`, `export interface SyncAttachments { … }`, `SyncStatus.attachments: SyncAttachments`.

- [ ] **Step 1: Write the failing tests** in `engine/tests/blobs.rs`:

```rust
#[tokio::test] async fn an_upload_that_keeps_failing_holds_neither_data_nor_snapshots() {
    // relay.set_failing(Route::PutBlob, Some(RelayError::Server(503))); A adds a policy, a document and an institution
    // run_round(A, RoundMode::FirstSnapshot): Ok; report.push pushed > 0; report.snapshot_uploaded == true; report.problem == None
    // B pulls: the institution and the document row are there; B's content of it is NotOnServer after its download try
    // A's entry is 'up' with attempts >= 1; status(A).attachments.waiting_upload == 1
}
#[tokio::test] async fn small_blobs_go_up_before_the_push()               // the PUT precedes the ops push in relay.calls()
#[tokio::test] async fn a_large_blob_waits_for_the_lane()                 // 9 MiB document: no PutBlob in the round's calls; entry still 'up'
#[tokio::test] async fn a_drain_round_uploads_nothing()
#[test] fn the_status_counts_waiting_uploads_and_content_elsewhere()      // status::status(..).attachments with one 'up' doc (bytes = size) and one 'down' photo
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::engine` → FAIL.
- [ ] **Step 3: Implement**, then the 7-step wire procedure for `SyncStatus` (bindings register `SyncAttachments`, `SyncAttachmentsProblem`, `AttachmentContent`; regenerate; mirror in `schema.ts`; fix TS fixtures).
- [ ] **Step 4: Run** `cd src-tauri && cargo test && cargo test generate_bindings -- --ignored && git diff --stat shared/generated-types.ts` then `npm run typecheck && npm test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): a round sends small documents and photos before its changes`.

---

### Task 12: The transfer lanes beside the rounds

**Files:**
- Modify: `src-tauri/src/services/sync/engine/runtime.rs`, `engine/mod.rs` (`EngineEvents::attachments_changed`), `engine/tests/{runtime,runtime_loop,blobs}.rs` (test `EngineEvents` impls gain the method), `src-tauri/src/commands/sync.rs` (`TauriEvents` emits `SYNC_ATTACHMENTS_EVENT = "sync-attachments-changed"`; `spawn_engine` unchanged otherwise), `docs/architecture/overview.md` (Device sync: a "Documents and photos" bullet: rows, `sync_blobs`, pre-push step, lanes, events, phone vs desktop)

**Interfaces:**
- Consumes: Tasks 10 and 11.
- Produces:
  - `pub fn spawn<R: Relay + Clone + 'static, D: ConnAccess, E: EngineEvents>(relay: R, db: D, events: E) -> EngineHandle` — builds `BlobPolicy::for_this_platform()` into the round policy and spawns two lane tasks (`Lane::Up`, `Lane::Down`) sharing it, the relay clone, `db` and `Arc<E>`. Each lane: at start `queue::clear_active` and (down lane) `attachment_store::sweep_incomplete`; then loop: `policy.wait(LANE_IDLE = 60 s)` → load credentials (none → continue) → `run_uploads(.., None, None)` / `run_downloads(.., None)` → when `report.changed`, `events.attachments_changed()` and mark the status dirty (a flag in `Shared` the main loop checks to `publish()` without a round); `Locked` or an `AppError::Auth` ends the lane.
  - `EngineHandle::wake_transfers(&self)`; `stop()` aborts the lanes too. The main loop wakes the lanes after every round. On `Trigger::SyncNow`: `policy.blobs.forget_server()`, `queue::retry_now`, wake.
  - `EngineEvents::attachments_changed(&self)` (the Tauri adapter emits `sync-attachments-changed` with no payload).
  - `Runtime.attachments_problem` is copied from `policy.blobs.problem()` before every publish.

- [ ] **Step 1: Write the failing tests** in `engine/tests/blobs.rs` (two devices; a test helper `settle(vault)` runs rounds on A and B and the lane functions `run_uploads`/`run_downloads` directly until nothing changes, so the tests stay deterministic; one test drives the spawned runtime):

```rust
#[tokio::test] async fn attachments_converge_with_their_content_on_two_devices() // A: policy + document + property + batch + photo; settle; B: rows equal (assert_converged), every blob read_all-equal to A's, content Here
#[tokio::test] async fn a_row_that_arrives_before_its_content_is_fetched_once_it_is_there() // fail_next(PutBlob) on A; B: NotOnServer; A uploads; B after retry_now: Here
#[tokio::test] async fn a_join_fetches_content()                          // C joins from A's snapshot (snapshot::join + creds): 'down' entries; run_downloads(C): every blob stored, sha256 equal
#[tokio::test] async fn a_delete_and_a_cascade_remove_content_everywhere() // A deletes a document; B merges: row, blob and entry gone. B deletes the property; A merges: batch, photos, blobs gone. relay.blob_ids still has all of them (nothing deleted at delete time)
#[tokio::test] async fn the_running_engine_tells_the_app_when_content_arrives() // runtime::spawn with a recording EngineEvents: attachments_changed called after B's download
#[tokio::test] async fn a_lane_start_sweeps_incomplete_blobs()            // an incomplete row before spawn: gone after the down lane started
#[tokio::test] async fn stopping_the_engine_stops_the_lanes()
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::engine` → FAIL.
- [ ] **Step 3: Implement** (`HttpRelay: Clone` from Task 3; the test `FakeRelay` is `Clone`).
- [ ] **Step 4: Run** `cd src-tauri && cargo test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): documents and photos transfer beside the rounds`.

---

### Task 13: The daily pass

**Files:**
- Create: `src-tauri/src/services/sync/blobs/pass.rs`, `blobs/tests/pass.rs`
- Modify: `engine/mod.rs` (`run_round_into` calls it after `after_push`), ADR 0030 (as-built note only if anything differed)

**Interfaces:**
- Consumes: Tasks 3, 7, 10; `state::{marks, vault_went_back_at, skipped_miss}`, `SyncState.missed_below_barrier`, `PullReport::done`.
- Produces:
  ```rust
  pub const PASS_INTERVAL: i64 = 86_400;
  pub const UNREFERENCED_GRACE: i64 = 7 * 86_400;
  pub const PASS_MAX_PAGES: usize = 100;
  pub const PASS_MAX_DELETES: usize = 1_000;
  pub const PASS_AT_KEY: &str = "sync.blob_pass_at";      // a Local app_config key (not allowlisted)
  pub enum Skip { NotDue, Behind, Pending, UploadsWaiting, Paused, WentBack, Missed, Quarantined, NoServerFeature }
  pub fn pass_allowed(conn: &Connection, pull: &PullReport, now: i64) -> Result<Option<Skip>>;
  pub struct PassReport { pub deleted: usize, pub requeued: usize, pub skipped: Option<Skip> }
  pub async fn run_if_due<R: Relay, D: ConnAccess>(relay: &R, db: &D, creds: &Credentials, policy: &BlobPolicy, pull: &PullReport, now: i64) -> Result<PassReport>;
  ```
  Allowed only when: due (`now − sync.blob_pass_at ≥ PASS_INTERVAL`), the pull `done()`, `sync_outbox` and `sync_unsent` empty, no `up` entry, no capture-broken or vault-gone mark and no guard, no went-back mark, no `missed_below_barrier` and no skipped miss, no quarantined head, and the server has the feature. It lists every page (`MAX_BLOB_PAGE`, at most `PASS_MAX_PAGES`), then: deletes each listed blob that no local row references and with `server_now − created_at > UNREFERENCED_GRACE` (no deletion at all when `server_now` is `None`), at most `PASS_MAX_DELETES`; for each locally referenced, held blob not listed, `requeue_upload` (the up lane sends it). `PASS_AT_KEY` is written only after a pass that listed every page. Called in `run_round_into` after `after_push` for `Normal` and `FirstSnapshot` rounds; its errors are logged and never fail the round.

- [ ] **Step 1: Write the failing tests** in `blobs/tests/pass.rs`:

```rust
#[test] fn the_pass_refuses_on_a_device_that_is_behind() // one case per Skip: an unfinished or held pull, an outbox row, an unsent envelope, an 'up' entry, vault_gone_at, the went-back mark, missed_below_barrier, a skipped miss, a quarantined head, run within 24 h
#[tokio::test] async fn the_pass_deletes_only_unreferenced_blobs_older_than_seven_days() // three server blobs: referenced (old), unreferenced 8 days, unreferenced 6 days → only the 8-day one deleted
#[tokio::test] async fn the_pass_reuploads_a_blob_the_server_lost()      // lose_blob; pass: requeued 1; run_uploads: relay has it again; entry 'done'
#[tokio::test] async fn the_pass_ages_blobs_by_the_server_clock_only()   // set_server_now(None): nothing deleted; server_now − created_at = 6 days while the device clock is 30 days ahead: nothing deleted
#[tokio::test] async fn the_pass_runs_at_most_once_a_day()
#[tokio::test] async fn a_failing_pass_does_not_fail_the_round()         // set_failing(ListBlobs, Server(503)): run_round Ok, problem None
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::sync::blobs::tests::pass` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cd src-tauri && cargo test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): a daily pass cleans the server's blobs and puts lost ones back`.

---

### Task 14: Content state in the attachment lists, "not here yet" when opened

**Files:**
- Modify: `src-tauri/src/models/{insurance,real_estate}.rs` (`InsuranceDocument.content`, `RealEstateDocument.content`, `RealEstatePhoto.content` and `RealEstatePhoto.thumb_content`, `#[serde(rename = "content")]`, `#[serde(rename = "thumbContent")]`), `src-tauri/src/services/attachments.rs`, `services/real_estate_photos.rs`, `services/attachment_protocol.rs`, `src-tauri/src/commands/{insurance,real_estate}.rs` (the listing commands become thin), `bindings.rs` / `shared/generated-types.ts` / `shared/schema.ts`, TS fixtures that build these models, `src/i18n/locales/*/common.json` (`validation.attachmentNotHereYet` in all ten)

**Interfaces:**
- Consumes: Task 7 (`ContentReader`, `want_blob`).
- Produces:
  - `attachments::list_insurance_documents(conn: &Connection, insurance_id: &str) -> Result<Vec<InsuranceDocument>>`, `attachments::list_real_estate_documents(conn: &Connection, real_estate_id: &str) -> Result<Vec<RealEstateDocument>>`, `real_estate_photos::list_batches(conn: &Connection, real_estate_id: &str) -> Result<Vec<RealEstatePhotoBatch>>` — the queries moved from `commands/insurance.rs` (`get_insurance_documents`), `commands/real_estate.rs` (`get_real_estate_documents`, `get_real_estate_photo_batches`), same order and fields, plus content from one `ContentReader` per call. Rows created locally (`add_*_document*`, `record_photo`) report their content through the same reader.
  - `pub const NOT_HERE_YET: &str = "validation.attachmentNotHereYet";` in `attachments.rs`. `read_bytes`, `write_to`, `export`, `export_photo_batch`, `open_elsewhere`: a `Source::Blob` that is not complete → `blobs::queue::want_blob` then `Err(AppError::Validation(NOT_HERE_YET))`.
  - `attachment_protocol::response_for` maps `AppError::Validation(NOT_HERE_YET)` to `425 Too Early` with `Cache-Control: no-store` (and CORS for the app origin).
  - Strings: en "This document or photo is still on its way to this device." / cs "Tento dokument nebo fotka ještě nedorazila do tohoto zařízení."

- [ ] **Step 1: Write the failing tests:**

```rust
// services/attachments.rs tests
#[test] fn the_lists_report_each_rows_content()          // documents: Here, WaitingUpload (sync on, no entry), NotHere ('down'); photo: content NotHere, thumb_content Here
#[test] fn opening_content_that_is_not_here_asks_for_it() // open_elsewhere / export / read_bytes → Validation(NOT_HERE_YET); the 'down' entry is wanted
// services/attachment_protocol.rs tests
#[test] fn content_that_is_not_here_yet_answers_425()
// services/real_estate_photos.rs tests
#[test] fn photo_batches_list_with_both_contents_newest_first() // same order as the command returned before the move
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib services::attachment services::real_estate_photos` → FAIL.
- [ ] **Step 3: Implement**, then the 7 steps for the three models (`schema.ts`: `content: AttachmentContent` on the two document types and `RealEstatePhoto`, `thumbContent: AttachmentContent` on the photo).
- [ ] **Step 4: Run** `cd src-tauri && cargo test && cargo test generate_bindings -- --ignored`; `npm run typecheck && npm test` → PASS.
- [ ] **Step 5: Commit** — `feat(attachments): lists say where each document and photo is; opening one not here asks for it`.

---

### Task 15: Commands, API wrappers and the attachments event

**Files:**
- Create: `src-tauri/src/commands/sync_attachments.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (`generate_handler!` under the sync comment), `src-tauri/src/commands/sync.rs` (`pub(crate) fn wake_transfers(app: &AppHandle)`; the scan test `commands_and_their_first_statements` and `every_sync_command_checks_the_flag_first` include `sync_attachments.rs` and the prefix `commands::sync_attachments::`, count 35 → 38), `src/lib/tauri-api.ts` (`deviceSyncApi`), `src/hooks/use-device-sync-events.ts`

**Interfaces:**
- Consumes: Task 7 (`want_row`, `retry_row`, `want_all`, `ContentReader`), Task 12 (`EngineHandle::wake_transfers`).
- Produces:
  ```rust
  #[tauri::command] pub async fn sync_attachment_download(app: AppHandle, db: State<'_, Database>, kind: AttachmentKind, id: String) -> Result<AttachmentContent>
  #[tauri::command] pub async fn sync_attachment_retry(app: AppHandle, db: State<'_, Database>, kind: AttachmentKind, id: String) -> Result<AttachmentContent>
  #[tauri::command] pub async fn sync_download_everything(app: AppHandle, db: State<'_, Database>) -> Result<u32>
  ```
  Each starts with `require_feature()?`; `id` is checked like `attachment_protocol::parse_route` (≤ 64, `[A-Za-z0-9-]`), an unknown row is `AppError::NotFound`; then the queue call, `wake_transfers(&app)`, and the row's content now.
  `tauri-api.ts`: `deviceSyncApi.downloadAttachment(kind: AttachmentKind, id: string): Promise<AttachmentContent>` (`'sync_attachment_download'`, `{ kind, id }`), `retryAttachment(kind, id)`, `downloadEverything(): Promise<number>`.
  `use-device-sync-events.ts`: `export const SYNC_ATTACHMENTS_EVENT = 'sync-attachments-changed';` its listener invalidates `['insurance-documents']`, `['real-estate-documents']`, `['real-estate-photo-batches']` (prefixes) and `['device-sync-status']`.

- [ ] **Step 1: Write the failing test:** the scan test with the new file and count 38 (it fails until the commands exist and are registered).
- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib commands::sync::tests` → FAIL.
- [ ] **Step 3: Implement** (commands thin; no logic beyond validation, delegation, wake).
- [ ] **Step 4: Run** `cd src-tauri && cargo test`; `npm run lint && npm run typecheck && npm test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): commands to fetch, retry and fetch every document and photo`.

---

### Task 16: Desktop: documents and photos show where they are

**Files:**
- Create: `src/utils/attachment-content.ts`, `src/utils/attachment-content.test.ts`, `src/utils/file-size.ts`, `src/utils/file-size.test.ts` (the `formatFileSize` both document tables copy today, moved here)
- Modify: `src/components/insurance/InsuranceDocuments.tsx`, `src/components/real-estate/RealEstateDocuments.tsx`, `src/components/real-estate/PhotoTimelineGallery.tsx`, `src/components/attachments/use-document-opener.tsx`, `src/components/attachments/AttachmentViewerDialog.tsx`, `src/i18n/locales/*/common.json` (all ten)

**Interfaces:**
- Consumes: `AttachmentContent` (schema), `deviceSyncApi.downloadAttachment` / `retryAttachment`.
- Produces:
  ```ts
  export type AttachmentPlatform = 'desktop' | 'phone';
  export type AttachmentNote = 'waitingUpload' | 'downloading' | 'notOnServer' | 'failed' | 'missing';
  export interface AttachmentView { openable: boolean; note: AttachmentNote | null; action: 'download' | 'retry' | null; busy: boolean }
  export function attachmentView(content: AttachmentContent, platform: AttachmentPlatform): AttachmentView;
  export function thumbnailShown(content: AttachmentContent): boolean; // here | waitingUpload
  export function formatFileSize(bytes: number | null): string;      // file-size.ts, unchanged output
  ```
  The table `attachmentView` implements:

  | content | desktop | phone |
  |---|---|---|
  | `here` | openable | openable |
  | `waitingUpload` | openable, note `waitingUpload` | same |
  | `notHere` | note `downloading`, busy | openable (opening downloads), action `download` |
  | `downloading` | note `downloading`, busy | same |
  | `notOnServer` | note `notOnServer` | same |
  | `failed` | note `failed`, action `retry` | same |
  | `missing` | note `missing` | same |

  Components: a document row shows its note as quiet secondary text (the `waitingUpload` mark a small muted badge) and, for `retry`, a "Try again" button calling `retryAttachment`; a non-openable row's open action is disabled. The photo grid renders a thumbnail only when `thumbnailShown(photo.thumbContent)`, else a skeleton tile. Opening a photo whose full content is not here shows the thumbnail with a spinner overlay, calls `downloadAttachment('photo', id)` once, and swaps to the full image when the list refetch (the attachments event) reports `here`. `use-document-opener` never builds a `moony-file` URL for a document that is not openable; an `ApiError` with key `validation.attachmentNotHereYet` shows its translated toast.
  Strings (`common.attachments.content.*`), en / cs: `waitingUpload` "Waiting to sync" / "Čeká na synchronizaci"; `downloading` "Downloading…" / "Stahuji…"; `notOnServer` "Not uploaded yet from the other device" / "Druhé zařízení ho zatím nenahrálo"; `failed` "Could not download" / "Nepodařilo se stáhnout"; `retry` "Try again" / "Zkusit znovu"; `missing` "The content is missing" / "Obsah chybí"; `download` "Download" / "Stáhnout"; `fetching` "Downloading from the sync server…" / "Stahuji ze synchronizačního serveru…".

- [ ] **Step 1: Write the failing tests** `attachment-content.test.ts` (every row of the table for both platforms; `thumbnailShown` for all seven values) and `file-size.test.ts` (`null` → `''`, 512 → `'512 B'`, 2048 → `'2 kB'`, 3·1024·1024 → `'3.0 MB'`).
- [ ] **Step 2: Run** `npx vitest run src/utils/attachment-content.test.ts src/utils/file-size.test.ts` → FAIL.
- [ ] **Step 3: Implement** the utils, the components and the strings (all ten locales).
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → PASS (the locale parity gate included).
- [ ] **Step 5: Commit** — `feat(attachments): the desktop shows waiting, downloading and missing documents and photos`.

---

### Task 17: Phone: open to download, and the same states

**Files:**
- Modify: `src/mobile/components/documents/DocumentRow.tsx`, `src/mobile/components/documents/DocumentsSection.tsx`, `src/mobile/sheets/DocumentViewerSheet.tsx`, `src/mobile/sheets/PhotoViewerSheet.tsx`, `src/mobile/components/property/PropertyGallery.tsx`, `src/mobile/lib/property-detail.ts` (`GalleryPhoto` gains `content` and `thumbContent`) and its test, `src/mobile/lib/documents.ts` (the row's sub-line names the note) and its test, `src/i18n/locales/*/mobile.json` (all ten)

**Interfaces:**
- Consumes: Task 16 (`attachmentView(…, 'phone')`, `thumbnailShown`), `deviceSyncApi.downloadAttachment` / `retryAttachment`.
- Produces: `galleryPhotos` carries `content` and `thumbContent`; `documentSub(…)` (the existing sub-line builder in `documents.ts`; keep its name) appends the note text key when there is one. `AddDocumentSheet.tsx` and `photo-upload.ts` stay as they are (a new attachment is `waitingUpload` from the list). Opening a `notHere` document opens the viewer in a "downloading" status (a fourth `Status` value `'fetching'`, spinner and `common.attachments.content.fetching`), calls `downloadAttachment` once, and loads the URL when the refetched row is `here`; a `failed` row opens to "Could not download" with "Try again". A gallery tile without its thumbnail is a neutral placeholder; the photo viewer shows the thumbnail with a spinner while the full photo downloads.
  Strings (`mobile.documents.*` next to the existing keys), en / cs: `notHereSub` "On your other devices · tap to download" / "Na vašich dalších zařízeních · klepnutím stáhnete".

- [ ] **Step 1: Write the failing tests** in `src/mobile/lib/property-detail.test.ts` (`galleryPhotos` carries both contents) and `src/mobile/lib/documents.test.ts` (the sub-line for `notHere`, `notOnServer`, `failed`, and unchanged for `here`).
- [ ] **Step 2: Run** `npx vitest run src/mobile/lib` → FAIL.
- [ ] **Step 3: Implement** with the strings in all ten locales.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → PASS.
- [ ] **Step 5: Commit** — `feat(mobile): documents and photos download when opened`.

---

### Task 18: Status lines, "Download everything to this phone", backups

**Files:**
- Create: `src/utils/sync-attachments.ts`, `src/utils/sync-attachments.test.ts`
- Modify: `src/components/settings/sync/SyncStatusCard.tsx`, the top-bar sync popover (`src/components/shell/SyncStatusIndicator.tsx` as parts A/B left it), the shared status mapping under `src/utils/` that parts A/B added (find it by its import in both `SyncStatusIndicator.tsx` and `SyncStatusBlock.tsx`) and its test, `src/mobile/components/sync/SyncStatusBlock.tsx`, the phone's Údržba sync row (parts A/B), `src/mobile/pages/settings/BackupSection.tsx`, the "Co se synchronizuje" text (both platforms; drop the sentence that documents and photos stay on the device that added them), `src/i18n/locales/*/deviceSync.json` and `*/mobile.json` (all ten)

**Interfaces:**
- Consumes: `SyncStatus.attachments` (Task 11), `deviceSyncApi.downloadEverything` (Task 15), `formatFileSize` (Task 16).
- Produces:
  ```ts
  export type AttachmentsLine =
    | { kind: 'allUploaded' }
    | { kind: 'waiting'; count: number; bytes: number }
    | { kind: 'problem'; problem: SyncAttachmentsProblem };
  export function attachmentsLine(a: SyncAttachments): AttachmentsLine;        // a problem wins while anything waits; else waiting; else allUploaded
  export function uploadsKeepUnsynced(status: SyncStatus): boolean;           // phase idle and attachments.waitingUpload > 0
  export function offerDownloadEverything(a: SyncAttachments): boolean;       // a.notHere - a.notOnServer > 0 (phone only; callers decide the platform)
  export function notInBackup(a: SyncAttachments | undefined): number;        // a?.notHere ?? 0
  ```
  The shared mapping: `uploadsKeepUnsynced` turns the "Synchronizováno" state into the neutral "Uploading documents and photos" (a device is synchronised only when nothing waits to upload); `attachments.problem` adds its line but never changes the phase's label. Desktop Settings → Sync and the popover show the "Documents and photos" row from `attachmentsLine`; the phone's Settings → Sync shows it and, when `offerDownloadEverything`, the line "N documents and photos are on your other devices" with the "Download everything to this phone" button (toast `downloadEverythingStarted`); the Údržba row appears for `attachments.problem`. The phone's backup screen shows `backupNotHere` when `notInBackup(status.attachments) > 0` and sync is on.
  Strings, en / cs (plurals per locale rules):
  - `deviceSync.attachments.title` "Documents and photos" / "Dokumenty a fotky"; `allUploaded` "All uploaded" / "Vše nahráno"; `waiting_one|_other` "{{count}} waiting to upload · {{size}}" / cs `_one` "{{count}} čeká na nahrání · {{size}}", `_few` "{{count}} čekají na nahrání · {{size}}", `_other` "{{count}} čeká na nahrání · {{size}}"; `uploading` "Uploading documents and photos" / "Nahrávám dokumenty a fotky"; `onOtherDevices_one|_other` "{{count}} document or photo is on your other devices" / "{{count}} documents and photos are on your other devices"; cs `_one` "{{count}} dokument nebo fotka je na vašich dalších zařízeních", `_few` "{{count}} dokumenty a fotky jsou na vašich dalších zařízeních", `_other` "{{count}} dokumentů a fotek je na vašich dalších zařízeních"; `downloadEverything` "Download everything to this phone" / "Stáhnout vše do tohoto telefonu"; `downloadEverythingStarted` "Downloading while Moony is open" / "Stahuji, dokud je Moony otevřené"; `problem.noRoom` "Documents and photos are waiting: the sync server has no room left. Ask whoever runs it for more space." / "Dokumenty a fotky čekají: na synchronizačním serveru došlo místo. Požádejte jeho správce o víc místa."; `problem.serverNeedsUpdate` "Documents and photos need a newer sync server. Everything else syncs." / "Dokumenty a fotky potřebují novější synchronizační server. Vše ostatní se synchronizuje."
  - `mobile.backup.notHere_one|_other` "{{count}} document or photo is not on this phone and is not in this backup." / "{{count}} documents and photos are not on this phone and are not in this backup."; cs `_one` "{{count}} dokument nebo fotka není v tomto telefonu ani v této záloze.", `_few` "{{count}} dokumenty a fotky nejsou v tomto telefonu ani v této záloze.", `_other` "{{count}} dokumentů a fotek není v tomto telefonu ani v této záloze."

- [ ] **Step 1: Write the failing tests** `sync-attachments.test.ts` (each function on zeros, waiting, a problem with and without waiting entries, notHere vs notOnServer) and, in the shared mapping's test, `idle` with `waitingUpload > 0` maps to the uploading state, with `0` to "Synchronizováno".
- [ ] **Step 2: Run** `npx vitest run src/utils` → FAIL.
- [ ] **Step 3: Implement** with the strings in all ten locales.
- [ ] **Step 4: Run** `npm run lint && npm run typecheck && npm test` → PASS.
- [ ] **Step 5: Commit** — `feat(sync): status shows documents and photos; the phone can fetch them all`.

---

### Task 19: End-to-end blob cases against a real sync server

Run by the controller once the server branch (`../Moony-relay-blobs`) is built; ignored by default like the other suites.

**Files:**
- Create: `src-tauri/src/services/sync/relay_e2e_blobs.rs`
- Modify: `src-tauri/src/services/sync/mod.rs` (`#[cfg(test)] mod relay_e2e_blobs;`), `docs/architecture/overview.md` (the end-to-end bullet names the new file)

**Interfaces:**
- Consumes: `relay_e2e::relay_url`, `relay_e2e_admin::{admin, Person}`, `sync_until_allowed`, `TestConn`, `blobs::transfer::{run_uploads, run_downloads}`, `blobs::pass::run_if_due`, `HttpRelay`.

- [ ] **Step 1: Write the ignored tests** (`#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]`):

```rust
#[tokio::test] async fn the_blob_calls_round_trip_and_refuse_a_bad_id()   // put/head/get/list/delete one sealed blob; a GET of an unknown uuid → BlobNotFound; server_info().blobs == true
#[tokio::test] async fn a_document_added_on_one_device_opens_on_the_other()
#[tokio::test] async fn a_photo_set_travels_with_its_thumbnails()
#[tokio::test] async fn a_join_fetches_content_from_the_server()
#[tokio::test] async fn a_deleted_document_stays_on_the_server_until_the_pass_and_a_lost_blob_comes_back() // delete: still listed; DELETE through the pass helper (grace forced with a test hook) removes it; a blob deleted by hand is re-uploaded after run_if_due
```

- [ ] **Step 2: Run** `cd src-tauri && cargo test --lib relay_e2e_blobs` → the tests are listed as ignored; `cargo test` stays green.
- [ ] **Step 3: Controller only:** start the server as the header of `relay_e2e.rs` says and run `MOONY_RELAY_E2E=http://127.0.0.1:8787 cargo test --lib relay_e2e_blobs -- --ignored --test-threads=1` → PASS.
- [ ] **Step 4: Commit** — `test(sync): documents and photos against a real sync server`.

---

## Spec coverage

| Spec | Task |
|---|---|
| §5.1 rows synced, local columns, no blob → not captured/dumped, converted row as a new row, received rows left alone by the conversion (it selects `blob_id IS NULL` only) | 5, 6 |
| §5.1 replace keeps content by blob ID, counts the rest | 5 |
| §5.2 stream, epoch, piece by piece, bound to blob ID | 1, 9 |
| §5.3 rows not held, uploads before the push with a budget, after it in the lane, backoff, leaves only when confirmed, any holder uploads, setup queues all (reconcile), phone on any connection | 7, 10, 11, 12 |
| §5.4 queue on merge/restore, thumbnails at once, desktop background, phone on open + download everything, failed opens not stored and retried | 7, 10, 12, 15, 17, 18 |
| §5.5 delete with row, cascade, nothing deleted at delete time, daily pass rules | 5, 12, 13 |
| §5.6 item states and status lines, problems | 11, 14, 16, 17, 18 |
| §5.7 backups | 18 |
| §7 registry, migration, relay, engine, envelope, store, handler and opener, commands, UI, ADR | 1–18 |
| §8 threat model, UUID only, bound stream, pass only when caught up, no plaintext temp file | 1, 3, 7, 9, 13 |
| §10 required coverage: convergence on two devices | 8, 12 (`attachments_converge_with_their_content_on_two_devices`) |
| — an upload that keeps failing while data and snapshots go on | 11 |
| — a row before its content | 12 |
| — a join that fetches content | 12 |
| — a delete and a cascade | 12, 5 |
| — the pass refusing when behind; re-uploading a lost blob | 13 |
| — a tampered and a truncated blob | 1, 9, 10 |
| — 100 MiB within a fixed memory bound | 9 |
| §11 e2e blob cases | 19 |
