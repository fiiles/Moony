# Sync attachments, plan 1 of 2: the sync server — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Moony sync server stores, serves, lists and deletes encrypted attachment files ("blobs") per vault, with a per-vault quota, a free-space floor, housekeeping, admin figures and backup, so the app (plan 2) can sync documents and photos.

**Architecture:** Blob files live in `DATA_DIR/blobs/<vault id>/<blob id>`, written like snapshots (temporary name, fsync, rename) and recorded in a new `blobs` table (migration 6). A new file store (`src/blob-files.ts`), a record/logic module (`src/blobs.ts`) and a route module (`src/blob-api.ts`) sit beside the snapshot code; `vaults.ts` only learns to remove blob directories and to count blobs in usage. The publish (rename) of an upload happens inside the same synchronous database transaction that inserts its record, so a file under a real name always has a record unless a later step failed, and housekeeping cleans up the rest.

**Tech Stack:** Node ≥ 22, TypeScript (ESM, `.js` import suffixes), Hono 4 + `@hono/node-server`, better-sqlite3, Vitest; admin UI in Preact.

**Spec:** `/Users/filipkral/Programování/FinanceApp/Moony-tauri-syncux/docs/specs/2026-11-14-sync-attachments-design.md` — this plan covers §6, the server side of §5.5, §8 as it concerns the server, and step 1 of §9. The app side is plan 2.

**Repository for every task:** `/Users/filipkral/Programování/FinanceApp/Moony-relay-blobs` (branch `feat/blobs`). Never touch the sibling `Moony-relay` checkout. All paths below are absolute or relative to this repository root (`R` = `/Users/filipkral/Programování/FinanceApp/Moony-relay-blobs`).

## Global Constraints

- Every task ends with `cd R && npm run typecheck && npm test` green, then one commit in `R`. Conventional Commits; the message ends with the line `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`. Single test file: `cd R && npx vitest run test/<file>.test.ts`; single test: add `-t "<name>"`.
- No new npm dependencies. Docker is not available: no task builds or runs the image. The `Dockerfile` does not change (`blobs/` is created at run time inside `DATA_DIR`, which the image already gives to user `node`).
- Schema changes are append-only: the next entry of `MIGRATIONS` in `src/db.ts` (this plan adds migration 6 only).
- Time only through the injected clock (unix seconds). Logs never contain vault IDs, blob IDs, tokens, file paths or client addresses.
- Errors are `{"error": "<code>"}` with the status. Every error answer of a blob route also carries the header `Moony-Error: <code>` (so a `HEAD`, which has no body, still says which 404 it is).
- `GET /v1/server` keeps `protocol: 2` and gains `features: ["blobs"]`.
- Blob ID: canonical lowercase UUID, `/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/` (the app makes them with `Uuid::new_v4().to_string()`). Anything else answers `400 invalid_blob_id` before any limiter, token check or file-system call. No path is ever built from anything but a validated vault ID and blob ID.
- Blob size limit `MAX_BLOB_BYTES = 104_883_275`: 100 MiB of content (`attachment_store::MAX_DOCUMENT_BYTES`) sealed by the app's stream (`envelope/stream.rs`: 59-byte header, 64 KiB chunks each with a 16-byte tag, then a last chunk of 0–65,535 bytes with its tag) = 104,857,600 + 59 + 16 × 1,601.
- Per-vault blob quota: default 10 GiB (10,737,418,240 bytes), one server-wide setting stored in `settings` under key `blob_quota_bytes`, valid range 0 to 2^50 bytes. A vault's use is the sum of its blob records' sizes; an upload is allowed while `used + size ≤ quota`.
- Free-space floor 1 GiB (1,073,741,824 bytes) for blob **and** snapshot uploads: refused when `free − declared length < floor`; an unknown free space (the stat failed) does not refuse. Free space comes from an injectable `freeBytes` function.
- At most 2 blob uploads per vault in flight. Blob uploads and downloads stream and are exempt from the buffered-body budget; uploads do have the 30-second idle-body timeout.
- `DESIGN.md` and `README.md` are updated in the task whose behaviour they describe. Admin text exists in English and Czech with identical keys and placeholders, and never says "relay".

## Review Focus

1. **Two devices upload the same blob ID at the same time** (both pass the pre-check): one answers 201, the other 200 `{"created": false}`, one record, one file, no `.tmp` left. → Task 3, test "two uploads of the same id at once".
2. **A record whose file is gone** (disk trouble, a partial restore): `GET` and `HEAD` answer `404 blob_not_found`, never an empty 200, and housekeeping drops the record so the devices' daily pass uploads it again. → Task 3 (answers), Task 6 (record dropped).
3. **The token is rotated while an upload streams**: the commit answers `403 token_read_only`, no file, no record, no temp. → Task 3, test "token rotated mid-upload".
4. **The admin lowers the quota below a vault's use**: existing blobs stay readable, re-uploading an existing ID still answers 200, new uploads `413 quota_exceeded`, deleting still works. → Task 4 and Task 5.
5. **A blocked person past the 30-day read grace**: `GET`, `HEAD` and the list answer `402 payment_required`; `DELETE` still works (current token, no entitlement), like vault deletion. → Task 5.

## The HTTP contract (fixed here; plan 2 relies on it)

All under `Authorization: Bearer <vault token>`; rate limits as for every vault call. Order of checks for every blob path: vault ID (`400 invalid_vault_id`), blob ID (`400 invalid_blob_id`), limiters (`429 rate_limited`), bearer present (`401 unauthorized`), then per call:

| Call | Access | Success | Errors (in check order) |
|---|---|---|---|
| `PUT /v1/vaults/:vaultId/blobs/:blobId`, raw body, `Content-Length` | write (current token + entitlement), judged before the body | `201 {"created": true}`; `200 {"created": false}` when the blob already exists (body not read, Content-Length not checked) | `404 vault_not_found`, `401 unauthorized`, `403 token_read_only`, `402 payment_required`; `411 length_required` (missing, non-integer or 0); `413 blob_too_large` (declared > 104,883,275); `400 empty_body`; `413 quota_exceeded`; `507 server_full`; `429 too_many_uploads` (2 in flight for this vault); while streaming: `413 blob_too_large` (more bytes than declared), `400 incomplete_body` (fewer, or the client stream failed), `408 body_timeout` (30 s without bytes), `404 vault_not_found` (vault deleted meanwhile); at commit: the auth errors again, `413 quota_exceeded`; disk failure `500 internal_error` |
| `HEAD /v1/vaults/:vaultId/blobs/:blobId` | read (current or previous token, entitlement + 30-day grace) | `200`, headers `Content-Length: <size>`, `Moony-Blob-Size: <size>`, `Content-Type: application/octet-stream`, `Cache-Control: no-store`, empty body | status + `Moony-Error`: `404 blob_not_found`, `404 vault_not_found`, `401 unauthorized`, `402 payment_required` |
| `GET /v1/vaults/:vaultId/blobs/:blobId` | read | `200` raw bytes, `Content-Type: application/octet-stream`, `Content-Length`, `Cache-Control: no-store`; no ranges | `404 blob_not_found` (no record, or record without file), `404 vault_not_found`, `401`, `402` |
| `GET /v1/vaults/:vaultId/blobs?after=<blobId>&limit=<n>` | read | `200 {"blobs": [{"id": "<uuid>", "size": n, "createdAt": <unix s>}], "more": bool}`, ascending by `id`, `after` exclusive (absent = from the start), `limit` default 500, max 1000 (larger values clamp) | `400 invalid_blob_id` (malformed `after`), `400 invalid_limit` (`limit=0`), `404 vault_not_found`, `401`, `402` |
| `DELETE /v1/vaults/:vaultId/blobs/:blobId` | owner (current token only, no entitlement) | `204`, also when no such blob exists (idempotent) | `404 vault_not_found`, `401 unauthorized`, `403 token_read_only` |

Any other method on these paths: `405 method_not_allowed`. `/v1/vaults/:id/blobs/<a>/<b>`: `404 not_found`. `GET /v1/server` → `{"name": "moony-sync", "version": "<semver>", "protocol": 2, "mode": "self_hosted", "features": ["blobs"]}`. Snapshot `PUT` gains `507 server_full`, judged after its authorisation and before its body.

## File structure

| File | Responsibility |
|---|---|
| `src/disk.ts` (new) | `FreeBytes` type, `diskFreeBytes(dir)`, `hasRoom(free, incoming)` |
| `src/files.ts` (new) | Shared file helpers moved out of `snapshots.ts`: `syncDir`, `streamToFile`, `listVaultDirs`, `BodyStreamError`, `BodyIdleError` |
| `src/blob-files.ts` (new) | `BlobFiles`: the blob files on disk |
| `src/blobs.ts` (new) | `Blobs`: blob records, access, quota, housekeeping; `blobQuotaBytes` / `setBlobQuotaBytes` |
| `src/blob-api.ts` (new) | The five blob calls, `blobError`, `UploadGate` |
| `src/sync-api.ts` | Path parsing, delegation to `blob-api.ts`, snapshot free-space check |
| `src/vaults.ts` | Public `authorize`, wider `check`, blob directory removal on wipe, blobs in `usage()` |
| `src/app.ts` | `Stores.blobFiles`, `Stores.blobs`, `AppDeps.freeBytes`, start-up sweep, `features` |
| `src/housekeeping.ts` | Calls `blobs.housekeep` and `blobs.sweep` |
| `src/admin/api.ts`, `src/admin/backup.ts` | Figures, quota setting, backup with `blobs/`, size estimate |
| `admin-ui/src/...` | Settings quota card, file counts, backup size |
| `test/blob-files.test.ts`, `test/blob-api.test.ts`, `test/blob-limits.test.ts`, `test/blob-housekeeping.test.ts`, `test/free-space.test.ts` (new) | Tests over HTTP in the existing style |

---

### Task 1: Free-space floor for uploads (snapshots first)

**Files:**
- Create: `R/src/disk.ts`, `R/test/free-space.test.ts`
- Modify: `R/src/app.ts` (Deps, createApp), `R/src/sync-api.ts:34-75,110-225` (route deps object, snapshot PUT), `R/src/admin/api.ts:207-230` (overview `bytesFree`), `R/src/limits.ts`, `R/test/helpers.ts:13-40` (`testApp`), `R/DESIGN.md`, `R/README.md`

**Interfaces:**
- Produces:
  - `src/disk.ts`: `export type FreeBytes = () => Promise<number | null>;` `export async function diskFreeBytes(dir: string): Promise<number | null>` (`statfs` → `bavail × bsize`; any error → `null`); `export function hasRoom(free: number | null, incoming: number): boolean` (`free === null || free - incoming >= FREE_SPACE_FLOOR_BYTES`).
  - `src/limits.ts`: `export const FREE_SPACE_FLOOR_BYTES = 1024 ** 3;`
  - `src/app.ts`: `Deps.freeBytes?: FreeBytes`; `export type AppDeps = Deps & { freeBytes: FreeBytes };` createApp resolves `freeBytes = deps.freeBytes ?? (() => diskFreeBytes(deps.config.dataDir))` and passes `{ ...deps, freeBytes }` (type `AppDeps`) to `syncRoutes` and `adminRoutes`.
  - `src/sync-api.ts`: `interface RouteDeps { stores: Stores; limiters: Limiters; budget: BodyBudget; freeBytes: FreeBytes }`; `vaultRoute(request: Request, rd: RouteDeps, caller: string): Promise<Response>` (pairingRoute unchanged). Later tasks add fields to `RouteDeps`.
  - `test/helpers.ts`: `testApp` returns `disk: { free: number | null }` (default `2 ** 40`) and injects `freeBytes: async () => disk.free` into `createApp`; `countingBody(chunks = 40)` moves here unchanged from `test/body-budget.test.ts` (which now imports it).

- [ ] **Step 1: Write the failing tests** in `test/free-space.test.ts`:

```ts
describe("free-space floor", () => {
  it("snapshot PUT is 507 server_full when free space minus the declared length is under 1 GiB, before the body", async () => {
    const t = testApp(); const v = await newVault(t);
    t.disk.free = FREE_SPACE_FLOOR_BYTES + 100;
    const { state, body } = countingBody(1);            // refused on the declared 101 bytes
    const r = await t.app.fetch(new Request(`http://localhost/v1/vaults/${v.id}/snapshot?seq=0`, {
      method: "PUT", headers: auth(v, { "content-length": String(101) }), body, duplex: "half" } as RequestInit));
    expect([r.status, await r.json()]).toEqual([507, { error: "server_full" }]);
    expect(state.pulled).toBeLessThanOrEqual(1);
    expect(snapshotFiles(t, v)).toEqual([]);
  });
  it("exactly the floor left after the upload is allowed", async () => {
    // free = FLOOR + 6, uploadSnapshot(t, v, 0, "snap-0") (6 bytes) → 201
  });
  it("an unknown free space (stat failed) does not refuse", async () => {
    // t.disk.free = null → uploadSnapshot → 201
  });
  it("auth comes first: a wrong token is 401, not 507", async () => { /* free = 0, wrong token → 401 */ });
  it("the overview reports the injected free space, 0 when unknown", async () => {
    // setupAdmin; t.disk.free = 12345 → overview.bytesFree === 12345; t.disk.free = null → 0
  });
  it("hasRoom", () => {
    expect(hasRoom(null, 10)).toBe(true);
    expect(hasRoom(FREE_SPACE_FLOOR_BYTES + 10, 10)).toBe(true);
    expect(hasRoom(FREE_SPACE_FLOOR_BYTES + 10, 11)).toBe(false);
  });
});
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/free-space.test.ts` — Expected: FAIL (`t.disk` undefined, no `src/disk.ts`).
- [ ] **Step 3: Implement** `src/disk.ts`, the limit, `AppDeps`, `RouteDeps`, `testApp.disk`. In the snapshot `PUT`, after `authorizeSnapshot` succeeds and before `snapshots.write`: `if (!hasRoom(await rd.freeBytes(), length)) return errorResponse(507, "server_full")`. The overview uses `(await deps.freeBytes()) ?? 0` instead of its inline `statfs`.
- [ ] **Step 4: Docs.** `DESIGN.md`: snapshot `PUT` row gains `507 server_full` ("the data volume would have less than 1 GiB free after it"); Limits table row "Free space kept on the data volume | 1 GiB; uploads that would go below it answer `507 server_full`". `README.md`, "What it stores": one sentence that uploads stop with "server full" when less than 1 GiB would remain free, and ordinary sync goes on.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS (the existing overview test's `bytesFree > 0` holds with the 1 TiB default).
- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "feat: refuse snapshot uploads that would leave under 1 GiB free

Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>"
```

### Task 2: Blob storage foundation (schema, limits, file store, wipe)

**Files:**
- Create: `R/src/files.ts`, `R/src/blob-files.ts`, `R/test/blob-files.test.ts`
- Modify: `R/src/db.ts:7-106` (migration 6), `R/src/limits.ts`, `R/src/encoding.ts` (`isBlobId`), `R/src/snapshots.ts` (use `files.ts`), `R/src/vaults.ts:124-129,262-269` (constructor, `finishWipe`), `R/src/app.ts` (`Stores.blobFiles`, start-up `sweepTemp`), `R/test/vault-housekeeping.test.ts:37-49` (`flakyVaults` passes `t.stores.blobFiles`), `R/test/helpers.ts`, `R/DESIGN.md` (Storage layout), `R/README.md` (What it stores)

**Interfaces:**
- Consumes: nothing from Task 1 beyond a green suite.
- Produces:
  - Migration 6 (append to `MIGRATIONS`, comment `// 6: blob (attachment) records; contents in blobs/<vault id>/<blob id>.`):

```sql
CREATE TABLE blobs (
  vault_id TEXT NOT NULL REFERENCES vaults (vault_id) ON DELETE CASCADE,
  blob_id TEXT NOT NULL,
  size INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (vault_id, blob_id)
)
```

  - `src/limits.ts`: `MAX_BLOB_CONTENT_BYTES = 100 * 1024 * 1024`; `MAX_BLOB_BYTES = MAX_BLOB_CONTENT_BYTES + 59 + 16 * (Math.floor(MAX_BLOB_CONTENT_BYTES / 65536) + 1)` with a comment naming the app's stream format (= 104,883,275); `DEFAULT_BLOB_QUOTA_BYTES = 10 * 1024 ** 3`; `MAX_BLOB_QUOTA_BYTES = 2 ** 50`; `MAX_BLOB_UPLOADS_PER_VAULT = 2`; `DEFAULT_BLOB_LIST_LIMIT = 500`; `MAX_BLOB_LIST_LIMIT = 1000`. `ORPHAN_SNAPSHOT_AGE_SECONDS` is reused for blobs; its comment becomes "A snapshot or blob file with no record this old is an abandoned upload."
  - `src/encoding.ts`: `export const isBlobId = (s: unknown): s is string` (the lowercase UUID regex of Global Constraints).
  - `src/files.ts` (moved from `snapshots.ts`, behaviour unchanged for snapshots): `export function syncDir(dir: string): void`; `export class BodyStreamError extends Error` (message `"body_stream_error"`); `export class BodyIdleError extends Error` (message `"body_idle"`); `export async function streamToFile(handle: FileHandle, body: ReadableStream<Uint8Array>, maxBytes: number, idleMs?: number): Promise<number | "too_large">` (returns bytes written; a read error throws `BodyStreamError`; no chunk for `idleMs` cancels the reader and throws `BodyIdleError`; past `maxBytes` cancels and returns `"too_large"`; it does not sync, close or remove); `export function listVaultDirs(root: string): { vaultId: string; mtime: number }[]`. `snapshots.ts` keeps exporting `SnapshotBodyError` as `export { BodyStreamError as SnapshotBodyError } from "./files.js"` so `sync-api.ts` and the snapshot tests stay as they are; `snapshots-fsync.test.ts` must keep passing (it mocks `fsyncSync` of `node:fs`, which `files.ts` uses).
  - `src/blob-files.ts`:

```ts
export interface BlobFiles {
  /** Streams `body` to "<blobId>.<random uuid>.tmp" (mode 0600, created with "wx") in the vault's
   *  directory (created if missing), fsyncs and closes it. The temp stays for publish/discard.
   *  "too_large" past maxBytes; BodyStreamError / BodyIdleError / disk errors are rethrown.
   *  In every non-success case the temp is removed. */
  writeTemp(vaultId: string, blobId: string, body: ReadableStream<Uint8Array>, maxBytes: number, idleMs?: number):
    Promise<{ tmp: string; size: number } | "too_large">;
  /** Synchronous: renames the temp to <blobId> (replacing nothing: callers publish only when no
   *  record exists) and syncDir()s; if syncDir throws, removes <blobId> and rethrows. */
  publish(vaultId: string, blobId: string, tmp: string): void;
  /** Removes a temp from writeTemp; no-op when it is gone or the name is not a temp name. */
  discard(vaultId: string, tmp: string): void;
  /** null when an ID is malformed or the file is missing; size and stream from one descriptor. */
  open(vaultId: string, blobId: string): { stream: ReadableStream<Uint8Array>; size: number } | null;
  /** Size on disk, null when missing or an ID is malformed. */
  size(vaultId: string, blobId: string): number | null;
  remove(vaultId: string, blobId: string): void;
  removeVault(vaultId: string): void;
  /** Deletes every "*.tmp" under the root; run once at start. */
  sweepTemp(): void;
  /** Files of the vault whose name isBlobId (temps and other names are ignored); mtime unix s. */
  listFiles(vaultId: string): { blobId: string; mtime: number }[];
  listVaultDirs(): { vaultId: string; mtime: number }[];
}
export function createBlobFiles(root: string): BlobFiles;   // root = join(dataDir, "blobs")
```

  Every method checks `isVaultId` / `isBlobId` (and the temp name `/^[0-9a-f-]{36}\.[0-9a-f-]{36}\.tmp$/`) before touching the file system; `writeTemp` and `publish` throw `Error("invalid_id")` on a malformed ID, the others return null / do nothing. `open` copies `Snapshots.open` (one descriptor, errors never name a path).
  - `src/vaults.ts`: `createVaults(db, clock, people, snapshots, blobFiles: BlobFiles)`; `finishWipe` removes `snapshots.removeVault(id)` **and** `blobFiles.removeVault(id)` before deleting the `pending_wipes` row (either failing leaves it pending).
  - `src/app.ts`: `Stores.blobFiles: BlobFiles`; createApp builds it on `join(dataDir, "blobs")` and calls `blobFiles.sweepTemp()` next to `snapshots.sweepTemp()`.
  - `test/helpers.ts`: `newBlobId = (): string => randomUUID()`; `blobFiles(t, v: { id: string }): string[]` (sorted directory listing of `blobs/<id>`, temps included, `[]` when missing — the blob twin of `snapshotFiles`); `stream(bytes: Uint8Array): ReadableStream<Uint8Array>` (one chunk, closed).

- [ ] **Step 1: Write the failing tests** in `test/blob-files.test.ts`:

```ts
it("MAX_BLOB_BYTES is 100 MiB sealed by the app's stream", () => expect(MAX_BLOB_BYTES).toBe(104_883_275));
it("isBlobId takes only the canonical lowercase UUID", () => {
  expect(isBlobId("0f8fad5b-d9cb-469f-a165-70867728950e")).toBe(true);
  for (const bad of ["0F8FAD5B-D9CB-469F-A165-70867728950E", "0f8fad5bd9cb469fa16570867728950e", "../x", "", "0f8fad5b-d9cb-469f-a165-70867728950e/", 5])
    expect([bad, isBlobId(bad)]).toEqual([bad, false]);
});
it("writeTemp + publish stores the bytes under the blob id, mode 0600, no temp left", async () => {
  const t = testApp(); const vid = newVaultId(); const id = newBlobId();
  const w = await t.stores.blobFiles.writeTemp(vid, id, stream(bytes(70_000)), 1e6);
  expect(blobFiles(t, { id: vid })).toEqual([(w as { tmp: string }).tmp]);
  t.stores.blobFiles.publish(vid, id, (w as { tmp: string }).tmp);
  expect(blobFiles(t, { id: vid })).toEqual([id]);
  expect(statSync(join(t.dataDir, "blobs", vid, id)).mode & 0o777).toBe(0o600);
  expect(t.stores.blobFiles.size(vid, id)).toBe(70_000);
});
it("writeTemp past maxBytes is too_large and leaves nothing", async () => { /* 40 bytes, max 39 → "too_large", blobFiles(...) === [] */ });
it("a body stream error rethrows BodyStreamError and leaves nothing", async () => { /* ReadableStream that errors after one chunk */ });
it("never touches the disk for malformed ids", () => {
  const f = createBlobFiles(join(t.dataDir, "blobs"));
  expect(f.open(newVaultId(), "../../moony-sync.db")).toBeNull();
  expect(f.open("../x", newBlobId())).toBeNull();
  expect(() => f.publish(newVaultId(), "../evil", "x.tmp")).toThrow("invalid_id");
  expect(existsSync(join(t.dataDir, "blobs"))).toBe(false);
});
it("sweepTemp removes *.tmp only; listFiles lists blob ids only", () => { /* write a uuid.uuid.tmp, a uuid, a "notes.txt" */ });
it("a vault wipe removes its blob directory", async () => {
  const t = testApp(); const v = await newVault(t);
  /* writeTemp + publish one blob under v.id */
  t.stores.vaults.wipe(v.id);
  expect(existsSync(join(t.dataDir, "blobs", v.id))).toBe(false);
  expect(rowCount(t, "pending_wipes")).toBe(0);
});
it("a leftover blob .tmp is gone when the app starts", () => { /* testApp({}, prepare writes blobs/<vid>/<uuid>.<uuid>.tmp) → blobFiles === [] */ });
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/blob-files.test.ts` — Expected: FAIL (modules missing).
- [ ] **Step 3: Implement** migration 6, limits, `isBlobId`, `files.ts` (moving `syncDir`, the read loop and `listVaultDirs` out of `snapshots.ts`), `blob-files.ts`, the `createVaults` parameter and `finishWipe`, `Stores.blobFiles`, start-up sweep, helpers, the `flakyVaults` call.
- [ ] **Step 4: Docs.** `DESIGN.md` Storage layout: add `blobs/<vault id>/<blob id>  attachment files (documents, photos, thumbnails), sealed by the apps, up to 104,883,275 bytes each`; the database list gains "blob records"; the paragraph on snapshot uploads says blob uploads are written the same way (temporary name, fsync, rename, directory fsync). `README.md` "What it stores": `DATA_DIR` also has a `blobs/` folder with the encrypted documents and photos.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS, including `snapshots-fsync.test.ts` and `vault-housekeeping.test.ts`.
- [ ] **Step 6: Commit** `feat: blob file store and schema (migration 6)` with the trailer.

### Task 3: Upload, download and HEAD

**Files:**
- Create: `R/src/blobs.ts`, `R/src/blob-api.ts`, `R/test/blob-api.test.ts`
- Modify: `R/src/vaults.ts` (public `authorize`, wider `check`), `R/src/sync-api.ts` (`VAULT_PATH`, delegation), `R/src/app.ts` (`Stores.blobs`), `R/test/helpers.ts`, `R/DESIGN.md`

**Interfaces:**
- Consumes: Task 2 `BlobFiles`, `isBlobId`, `MAX_BLOB_BYTES`, `BodyStreamError`, `BodyIdleError`; Task 1 `RouteDeps`, `hasRoom`.
- Produces:
  - `src/vaults.ts`: `export type Access = "read" | "write" | "owner";` `Vaults.check(vaultId, token, need: Access): Result` (side-effect free, now also "read"); new `Vaults.authorize(vaultId: string, token: string, access: Access): Result` (the existing private `authorize`: verdict plus the activity mark; joins an outer transaction).
  - `src/blobs.ts`:

```ts
export type BlobRecord = { id: string; size: number; createdAt: number };
export interface Blobs {
  /** Side-effect free, before any body byte: vaults.check(write), then whether the record exists. */
  check(vaultId: string, token: string, blobId: string): Result<{ exists: boolean }>;
  /** One transaction: vaults.authorize(write); record exists → { created: false } (publish NOT
   *  called); else publish() (the rename) and insert (vault_id, blob_id, size, clock()). A throw
   *  from publish rolls back and propagates. */
  commit(vaultId: string, token: string, blobId: string, size: number, publish: () => void): Result<{ created: boolean }>;
  /** vaults.authorize(read), then the record; 404 blob_not_found without one. */
  find(vaultId: string, token: string, blobId: string): Result<{ size: number; createdAt: number }>;
}
export function createBlobs(db: Db, clock: Clock, vaults: Vaults, files: BlobFiles): Blobs;
```

  - `src/blob-api.ts`: `export function blobError(status: number, code: string): Response` (`errorResponse` plus header `moony-error: <code>`); `export async function blobRoute(rd: RouteDeps, request: Request, url: URL, vaultId: string, blobId: string | null, token: string): Promise<Response>` (`blobId` null = the collection path; in this task only the item path answers, the collection answers `405 method_not_allowed`).
  - `src/sync-api.ts`: `VAULT_PATH = /^\/v1\/vaults\/([^/]+)(\/ops|\/snapshot|\/token|\/rekey|\/blobs(?:\/([^/]+))?)?$/`; `const isBlob = sub.startsWith("/blobs")`; for a blob path every early error uses `blobError`; the blob segment is checked with `isBlobId` right after the vault ID (before the limiters) → `400 invalid_blob_id`; after the token and the per-vault limiter, `return blobRoute(rd, request, url, vaultId, match[3] ?? null, token)`.
  - `Stores.blobs: Blobs` (createApp builds it after `vaults`).
  - `test/helpers.ts`: `uploadBlob(t, v: Vault, blobId: string, content: Uint8Array | string, headers?: HeadersInit): Promise<Response>` (PUT with `content-length`); `heldBlobUpload(t, v, blobId, content)` → `{ response, started(), finish(), fail() }` (the `heldUpload` pattern of `vault-housekeeping.test.ts`: sends 2 bytes, `started()` waits until a `.tmp` is in `blobFiles(t, v)`, `fail()` errors the stream). `countingBody` is already in helpers (Task 1).
- Route order for `PUT` (exact; Task 4 inserts its two checks where marked):
  1. `blobs.check` → error as is; `exists` → `200 {"created": false}` (body untouched).
  2. `Content-Length`: not a positive integer → `411 length_required`; `> MAX_BLOB_BYTES` → `413 blob_too_large`; no `request.body` → `400 empty_body`.
  3. *(Task 4: quota pre-check.)*
  4. `hasRoom(await rd.freeBytes(), length)` false → `507 server_full`.
  5. *(Task 4: upload gate.)*
  6. `files.writeTemp(vaultId, blobId, request.body, length, IDLE_BODY_SECONDS * 1000)`: `BodyStreamError` → `400 incomplete_body`; `BodyIdleError` → `408 body_timeout`; `ENOENT` → `404 vault_not_found`; other errors rethrow (500); `"too_large"` → `413 blob_too_large`; `size !== length` → discard, `400 incomplete_body`.
  7. `blobs.commit(..., () => files.publish(vaultId, blobId, tmp))` inside `try`; in `finally` `files.discard(vaultId, tmp)` (a no-op after a publish). Failure → its error; `created` → `201 {"created": true}`, else `200 {"created": false}`.
- `HEAD` (check `request.method === "HEAD"` **before** the GET branch: Hono dispatches HEAD as GET and drops the body, so it must not open a file): `blobs.find`, then `files.size(...) === null` → `404 blob_not_found`; else 200 with the headers of the contract and a null body. `GET`: `blobs.find`, then `files.open(...) ?? 404 blob_not_found`, stream it with the contract's headers. The body budget is never used by blob routes.

- [ ] **Step 1: Write the failing tests** in `test/blob-api.test.ts` (each `it` on its own `testApp()`; `bytes(n)` = random bytes):

```ts
describe("upload and download", () => {
  it("round trip: 201 created, GET returns the exact bytes, file is blobs/<vault>/<id>", async () => {
    const t = testApp(); const v = await newVault(t); const id = newBlobId(); const data = bytes(200_000);
    const put = await uploadBlob(t, v, id, data);
    expect([put.status, await put.json()]).toEqual([201, { created: true }]);
    const get = await t.call("GET", `/v1/vaults/${v.id}/blobs/${id}`, { headers: auth(v) });
    expect(get.status).toBe(200);
    expect(get.headers.get("content-type")).toBe("application/octet-stream");
    expect(get.headers.get("content-length")).toBe("200000");
    expect(Buffer.from(await get.arrayBuffer()).equals(Buffer.from(data))).toBe(true);
    expect(blobFiles(t, v)).toEqual([id]);
  });
  it("re-upload of an existing id is 200 created:false without reading the body or checking the length", async () => {
    // after a first upload: countingBody(40) with no content-length → 200 {created:false}, state.pulled <= 1, GET still the first bytes
  });
  it("two uploads of the same id at once: 201 and 200, one record, one file, no temp", async () => {
    // two heldBlobUpload with different content of equal length; await both started(); finish both;
    // statuses sorted → [200, 201]; blobFiles(t, v) → [id]; COUNT(*) FROM blobs → 1; GET equals one of the two
  });
  it("HEAD: 200 with Content-Length and Moony-Blob-Size and no body; 404 says which", async () => {
    // existing: status 200, content-length & moony-blob-size === String(size), await text() === ""
    // missing blob: 404, moony-error "blob_not_found"; unknown vault: 404, moony-error "vault_not_found"
  });
  it("GET of a missing blob is 404 blob_not_found; a record whose file is gone is 404 too, never an empty 200", async () => {
    // upload, rmSync the file; GET → [404, {error:"blob_not_found"}], moony-error header; HEAD → 404
  });
});
describe("access", () => {
  it("a previous (rotated) token reads with GET and HEAD, and gets 403 token_read_only for PUT", async () => {});
  it("a blocked person reads inside the 30-day grace and is 402 after it", async () => {
    // block, clock += 1 day → GET 200; clock += 30 days → GET 402 payment_required
  });
  it("token before body: unknown vault 404, wrong token 401, read-only 403, blocked 402, none reads the body", async () => {
    // countingBody(40) with content-length 40*65536 for each; state.pulled <= 1; nothing under blobs/
  });
});
describe("path safety", () => {
  it("a non-UUID or traversal blob id is 400 invalid_blob_id for PUT, GET and HEAD, and nothing is created", async () => {
    for (const bad of ["not-a-uuid", "0F8FAD5B-D9CB-469F-A165-70867728950E", "..%2F..%2Fmoony-sync.db", "0f8fad5b-d9cb-469f-a165-70867728950", "0f8fad5b-d9cb-469f-a165-70867728950eX"])
      for (const m of ["PUT", "GET"]) { /* status 400, json {error:"invalid_blob_id"}, moony-error header */ }
    // HEAD: 400 + moony-error; existsSync(join(t.dataDir, "blobs")) === false
  });
  it("an extra path segment is 404 not_found", async () => { /* GET /v1/vaults/<id>/blobs/<uuid>/x → 404 */ });
});
describe("bodies", () => {
  it("411 length_required without or with a zero Content-Length", async () => {});
  it("declared over 104,883,275 bytes is 413 blob_too_large before the body", async () => {});
  it("declared exactly MAX_BLOB_BYTES passes the size gate (a short body then is 400 incomplete_body)", async () => {});
  it("an upload that ends early is 400 incomplete_body and leaves no file and no record", async () => {
    // content-length 50, body "0123456789"; and a stream that errors after 2 bytes (heldBlobUpload().fail())
  });
  it("a body longer than declared is 413 blob_too_large, nothing left", async () => {});
  it("a body idle for 30 s is 408 body_timeout and leaves no temp", async () => { /* vi.useFakeTimers as in body-budget.test.ts */ });
  it("a 9 MiB upload succeeds while the same client already holds its whole 8 MiB body share (exempt from the budget)", async () => {
    // 512 held admin logins × 16 KiB from clientAddress "203.0.113.9" (as in body-budget.test.ts); a login from it → 503;
    // uploadBlob(t, v, id, bytes(9 * 1024 * 1024)) with clientAddress "203.0.113.9" → 201; then end the held bodies
  });
  it("507 server_full when the upload would leave under 1 GiB, before the body", async () => {
    // t.disk.free = FREE_SPACE_FLOOR_BYTES + 100: length 101 → 507 (countingBody pulled <= 1); length 100 → 201
  });
});
describe("races", () => {
  it("vault deleted while the body streams: 404 vault_not_found, no file, no record", async () => {
    // heldBlobUpload; started(); DELETE /v1/vaults/:id → 204; finish(); → [404, vault_not_found]; blobFiles === []; no blobs rows
  });
  it("token rotated mid-upload: 403 token_read_only, no file, no record, no temp", async () => {});
});
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/blob-api.test.ts` — Expected: FAIL (404 not_found for every blob path).
- [ ] **Step 3: Implement** `vaults.authorize`/`check`, `blobs.ts`, `blob-api.ts`, the path parsing and delegation, `Stores.blobs`, helpers.
- [ ] **Step 4: Docs.** `DESIGN.md` HTTP API → new subsection "Blobs (attachments)" with the PUT, HEAD and GET rows of the contract table above (errors as listed), the `Moony-Error` header rule, and the order "token and entitlement before the body; an existing blob answers 200 without reading it". "What the server knows" → per vault: blob IDs, sizes and upload times. Abuse limits paragraph: blob uploads stream to disk, are exempt from the buffered-body budget, but are aborted after 30 s without bytes.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS.
- [ ] **Step 6: Commit** `feat: blob upload, download and HEAD` with the trailer.

### Task 4: Quota and the two-upload limit

**Files:**
- Create: `R/test/blob-limits.test.ts`
- Modify: `R/src/blobs.ts`, `R/src/blob-api.ts`, `R/src/sync-api.ts` (`RouteDeps.uploads`), `R/DESIGN.md` (Limits table), `R/README.md` (Known limits)

**Interfaces:**
- Consumes: Task 3 `Blobs`, `blobRoute`, `RouteDeps`.
- Produces:
  - `src/blobs.ts`: `export function blobQuotaBytes(db: Db): number` (setting `blob_quota_bytes` parsed as an integer; missing or unparsable → `DEFAULT_BLOB_QUOTA_BYTES`); `export function setBlobQuotaBytes(db: Db, bytes: number): void` (uses `getSetting`/`setSetting` from `./admin/auth.js`; callers validate); `Blobs.fits(vaultId: string, size: number): boolean` (`SUM(size)` of the vault's records `+ size <= blobQuotaBytes(db)`); `commit` returns `fail(413, "quota_exceeded")` after the exists check and before `publish` when `fits` is false.
  - `src/blob-api.ts`: `export interface UploadGate { tryEnter(vaultId: string): (() => void) | null }`; `export function createUploadGate(perVault: number = MAX_BLOB_UPLOADS_PER_VAULT): UploadGate` (in-memory count per vault ID; the returned release is idempotent; the map entry is dropped at 0).
  - `RouteDeps.uploads: UploadGate`, created once in `syncRoutes`.
  - PUT order: step 3 = `!blobs.fits(vaultId, length)` → `413 quota_exceeded`; step 5 = `rd.uploads.tryEnter(vaultId)` null → `429 too_many_uploads`; the release runs in a `finally` around steps 6–7.

- [ ] **Step 1: Write the failing tests** in `test/blob-limits.test.ts`:

```ts
describe("quota", () => {
  it("defaults to 10 GiB", () => expect(blobQuotaBytes(testApp().db)).toBe(10_737_418_240));
  it("refuses an upload past the quota before its body; exactly at the quota is allowed", async () => {
    // setBlobQuotaBytes(t.db, 1000); 600 → 201; 401 via countingBody → [413, quota_exceeded], pulled <= 1; 400 → 201
  });
  it("the commit re-checks: two uploads that each fit alone end as one 201 and one 413", async () => {
    // quota 1000, two heldBlobUpload of 600 bytes, different ids; finish both → statuses sorted [201, 413]; one record; no temp
  });
  it("is per vault", async () => { /* vault A full, vault B (other person) still 201 */ });
  it("lowered below use: reads and re-uploads of existing ids work, new uploads are 413", async () => {
    // upload 600, setBlobQuotaBytes(t.db, 100); GET → 200; re-PUT same id → 200 created:false; new id 10 bytes → 413
  });
});
describe("two uploads per vault", () => {
  it("a third concurrent upload to the same vault is 429 too_many_uploads before its body; other vaults are not affected", async () => {
    // two heldBlobUpload to A (started); third to A with countingBody → [429, too_many_uploads], pulled <= 1;
    // uploadBlob to vault B → 201; finish one of A → a new upload to A → 201; finish the other
  });
  it("a failed upload gives its slot back", async () => { /* two held, fail() both → two new uploads → 201 each */ });
  it("re-uploading an existing id is answered while two uploads run", async () => { /* → 200 created:false */ });
});
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/blob-limits.test.ts` — Expected: FAIL.
- [ ] **Step 3: Implement** the quota functions, `fits`, the commit check, `UploadGate`, the two route steps.
- [ ] **Step 4: Docs.** `DESIGN.md` Limits table: "Blob | 104,883,275 bytes (100 MiB of content sealed by the app)", "Blob bytes per vault | 10 GiB by default, one setting for the whole server (admin Settings)", "Blob uploads per vault at once | 2, then `429 too_many_uploads`". PUT row: `413 quota_exceeded`, `429 too_many_uploads`. `README.md` Known limits: replace "Snapshot uploads … are not limited in parallel" with the same fact plus "a profile may upload at most two documents or photos at once"; the nginx `client_max_body_size 128m` advice already covers a 100 MiB document (say so in one clause).
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS.
- [ ] **Step 6: Commit** `feat: blob quota and two uploads per vault` with the trailer.

### Task 5: List and delete; advertise the feature

**Files:**
- Modify: `R/src/blobs.ts`, `R/src/blob-api.ts`, `R/src/app.ts:64-74` (`/v1/server`), `R/test/blob-api.test.ts` (new describes), `R/test/app.test.ts:17-27`, `R/DESIGN.md`, `R/README.md`

**Interfaces:**
- Consumes: Task 3/4 `Blobs`, `blobRoute`.
- Produces:
  - `Blobs.list(vaultId: string, token: string, after: string | null, limit: number): Result<{ blobs: BlobRecord[]; more: boolean }>` — `vaults.authorize(read)`; `SELECT blob_id, size, created_at FROM blobs WHERE vault_id = ? AND blob_id > ? ORDER BY blob_id LIMIT ?` with `limit + 1` rows to compute `more`; `after` null → `""`.
  - `Blobs.remove(vaultId: string, token: string, blobId: string): Result` — `vaults.authorize(owner)`; delete the record; then `files.remove` in a `try` (a failure leaves an orphan for housekeeping); ok whether or not a record existed.
  - Routes: collection `GET` parses `after` (present and not `isBlobId` → `400 invalid_blob_id`) and `limit` (`intParam` semantics: absent/non-numeric → 500, clamp to 1000, `< 1` → `400 invalid_limit`), answers `json({ blobs, more })`; item `DELETE` → 204.
  - `src/app.ts`: `export const FEATURES = ["blobs"] as const;` added to the `/v1/server` answer as `features`.

- [ ] **Step 1: Write the failing tests** (in `test/blob-api.test.ts` and `test/app.test.ts`):

```ts
describe("list", () => {
  it("pages by id: limit, after (exclusive), more; entries are id, size, createdAt", async () => {
    // 5 uploads with ids sorted ascending (ids[0..4]), sizes 1..5, t.clock.now moves between them;
    // ?limit=2 → ids[0..1], more true, entries toEqual {id, size, createdAt: <clock at that upload>}
    // ?after=ids[1]&limit=2 → ids[2..3], more true; ?after=ids[3] → [ids[4]], more false
  });
  it("defaults and bounds: no params lists all; limit=0 is 400 invalid_limit; limit=5000 clamps; bad after is 400 invalid_blob_id", async () => {});
  it("an empty vault lists nothing; another vault's blobs are never listed; a previous token can list", async () => {
    // → {blobs: [], more: false}
  });
});
describe("delete", () => {
  it("current token: 204, then GET 404 blob_not_found, file and record gone; a second DELETE is 204 too", async () => {});
  it("a previous token is 403 token_read_only; a wrong token 401; an unknown vault 404 vault_not_found; a bad id 400", async () => {});
  it("delete then upload of the same id is 201 created:true", async () => {});
  it("a blocked person past the grace: GET, HEAD and list are 402, DELETE is 204", async () => {
    // block, clock += 31 days
  });
  it("frees quota: with the quota lowered below use, deleting makes room again", async () => {
    // quota 1000, upload 600, setBlobQuotaBytes 500; new 100 → 413; DELETE the 600 → 204; new 100 → 201
  });
});
// app.test.ts "GET /v1/server": toEqual({ name: "moony-sync", version: "9.9.9", protocol: 2, mode: "self_hosted", features: ["blobs"] })
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/blob-api.test.ts test/app.test.ts` — Expected: FAIL.
- [ ] **Step 3: Implement** `list`, `remove`, the two routes, `FEATURES`.
- [ ] **Step 4: Docs.** `DESIGN.md`: the blob subsection gains the list and DELETE rows of the contract (DELETE: "current token only; used by the devices' daily clean-up and by nothing else"); "Changes to the v1.1 API" `GET /v1/server` row shows `features: ["blobs"]` and says protocol stays 2 and a server without the field has no features; "Not built" drops "Blob (attachment) endpoints". `README.md` intro: the server also stores the apps' encrypted documents and photos.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS.
- [ ] **Step 6: Commit** `feat: blob list and delete, advertise the blobs feature` with the trailer.

### Task 6: Housekeeping and deletion cover blobs

**Files:**
- Create: `R/test/blob-housekeeping.test.ts`
- Modify: `R/src/blobs.ts`, `R/src/housekeeping.ts:7-29`, `R/test/housekeeping.test.ts:85-120` (partial stores gain `blobs: t.stores.blobs`), `R/test/vault-housekeeping.test.ts:29` (`VAULT_TABLES` gains `"blobs"`), `R/DESIGN.md` (Housekeeping, deletion), `R/README.md`

**Interfaces:**
- Consumes: Task 2 `BlobFiles.listFiles/listVaultDirs/removeVault/size`, Task 3 `Blobs`.
- Produces:
  - `Blobs.housekeep(vaultId: string): void` — (a) files of the vault without a record whose mtime is older than `clock() - ORPHAN_SNAPSHOT_AGE_SECONDS` are removed; (b) records whose file is missing (`files.size(...) === null`) are deleted. Does nothing for a vault without a `vaults` row.
  - `Blobs.sweep(): void` — blob directories (`files.listVaultDirs()`) of vault IDs with no `vaults` row and no `pending_wipes` row, whose newest mtime is older than the orphan age, are removed (errors swallowed, retried next run).
  - `startHousekeeping(deps, stores: Pick<Stores, "vaults" | "mailboxes" | "blobs">, ...)`: per vault after `vaults.housekeep(id)`: `attempt("blobs", () => stores.blobs.housekeep(id))`; after the vault sweep: `attempt("blob sweep", () => stores.blobs.sweep())`.

- [ ] **Step 1: Write the failing tests** in `test/blob-housekeeping.test.ts`:

```ts
it("removes blob files without a record once they are an hour old, and only those", async () => {
  // recorded blob (uploaded), old orphan (uuid name, utimes now-3600-10), fresh orphan, "notes.txt";
  // t.stores.blobs.housekeep(v.id) → recorded + fresh orphan + notes.txt remain
});
it("drops a record whose file is gone, so the blob can be uploaded again", async () => {
  // upload, rmSync file; housekeep; list → []; uploadBlob same id → 201 created:true
});
it("sweep() removes old blob directories of unknown vaults, keeps fresh ones", () => { /* as the snapshot sweep test, under blobs/ */ });
it("runOnce() of startHousekeeping runs both", async () => { /* orphan file and ghost dir gone after hk.runOnce() */ });
it("deleting a vault removes its blob files", async () => { /* upload; DELETE /v1/vaults/:id → 204; blobs/<id> gone; no blobs rows */ });
it("deleting a person (admin) removes every profile's blob files", async () => {
  // setupAdmin, address, create person, two vaults with a blob each; DELETE people/:id → 204; both blobs/<id> gone
});
it("deleting one profile (admin) removes its blob files and leaves the other's", async () => {});
it("a wipe whose blob directory removal fails stays pending and finishes on the next housekeep", async () => {
  // createVaults(t.db, clock, people, t.stores.snapshots, { ...t.stores.blobFiles, removeVault: fails once }) .destroy(...)
  // → pending_wipes 1, blobs/<id> still there; t.stores.vaults.housekeep(id) → gone, pending_wipes 0
});
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/blob-housekeeping.test.ts` — Expected: FAIL (`housekeep`/`sweep` missing).
- [ ] **Step 3: Implement** `housekeep`, `sweep`, the housekeeping loop, the two test updates.
- [ ] **Step 4: Docs.** `DESIGN.md` Housekeeping: per vault also removes blob files without a record that are over an hour old and drops blob records whose file is gone (so the devices upload it again); the sweep also removes blob directories of vaults without a record; at start also `*.tmp` under `blobs/`. Deletion text ("Deleting a person removes … snapshot files", the vault `DELETE` row, the `pending_wipes` paragraph): "snapshot and blob files". `README.md` "What it stores": housekeeping also cleans up abandoned document uploads.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS.
- [ ] **Step 6: Commit** `feat: housekeeping and deletion cover blob files` with the trailer.

### Task 7: Admin API: figures, quota setting, backup

**Files:**
- Modify: `R/src/vaults.ts:189-193,537-540` (usage), `R/src/admin/api.ts` (person detail, settings, backup, backup size), `R/src/admin/backup.ts`, `R/test/admin-people.test.ts`, `R/test/backup.test.ts`, `R/DESIGN.md` (Admin API, Backup, What the server knows), `R/README.md` (Backup and restore)

**Interfaces:**
- Consumes: `blobQuotaBytes`, `setBlobQuotaBytes`, `MAX_BLOB_QUOTA_BYTES` (Task 4), `BlobFiles` (Task 2).
- Produces:
  - `Vaults.usage(): { vaultId: string; account: string; bytes: number; files: number; lastActivity: number | null }[]` — `bytes` = op charge + snapshot sizes + blob sizes; `files` = number of blob records. Person list, overview and person detail sums follow automatically.
  - `GET /admin/api/people/:id` → `vaults: [{ id, shortId, bytes, files, lastActivity }]`.
  - `GET /admin/api/settings` → `{ "publicUrl": string | null, "blobQuotaBytes": number }`.
  - `PUT /admin/api/settings`: exactly one key. `{publicUrl}` behaves as today and answers `{publicUrl}`; `{blobQuotaBytes}` must be an integer with `0 <= n <= 2 ** 50` (else `400 invalid_quota`), is stored, answers `{blobQuotaBytes: n}`; no key, both keys, or any other key → `400 invalid_request`.
  - `GET /admin/api/backup/size` → `{ "estimatedBytes": number }` from `export function estimateBackupBytes(db: Db, dataDir: string): number` in `backup.ts`: sizes on disk of `moony-sync.db` and `moony-sync.db-wal` (missing → 0) + 512 (its header) + for every snapshot and blob record `512 + ceil(size / 512) * 512` + 512 per directory entry (`snapshots/`, `blobs/`, one per vault directory that has records) + 1024 (end blocks). An estimate, not exact (the database copy differs from the files).
  - `createBackupStream(opts: { db; dataDir; snapshots; blobFiles: BlobFiles; now })`: after the snapshot entries, a `blobs/` directory entry (always, like `snapshots/`), `blobs/<vaultId>/` per directory from `blobFiles.listVaultDirs()`, and `blobs/<vaultId>/<blobId>` per `blobFiles.listFiles(vaultId)` (temps never listed).

- [ ] **Step 1: Write the failing tests:**

```ts
// admin-people.test.ts
it("space figures include blobs; the profile shows its file count", async () => {
  // person with vault v; push op "12345"; uploadBlob 1000 bytes; people[0].bytes === 5 + OP_OVERHEAD_BYTES + 1000;
  // overview.bytesUsed same; GET people/:id → vaults[0] toEqual {id, shortId, bytes: 5 + 128 + 1000, files: 1, lastActivity: expect.any(Number)}
});
it("blob quota setting: default, save, validation, effect", async () => {
  // GET settings → {publicUrl: null, blobQuotaBytes: 10737418240}
  // PUT {blobQuotaBytes: 5 * 1024 ** 3} → [200, {blobQuotaBytes: 5368709120}]; blobQuotaBytes(t.db) === 5368709120
  // each of -1, 1.5, "10", null, true, 2 ** 50 + 1 → [400, {error: "invalid_quota"}]; 0 and 2 ** 50 → 200
  // {publicUrl: ADDRESS, blobQuotaBytes: 1} → [400, invalid_request]; {} and [] → 400 invalid_request
  // PUT {blobQuotaBytes: 10}; uploadBlob 11 bytes → 413 quota_exceeded
});
// existing "saves a normalised address…": GET settings expectations gain blobQuotaBytes: 10737418240
// existing guard test: routes gain ["GET", "backup/size"]

// backup.test.ts
it("the archive holds blobs/<vault>/<id> with the exact bytes, never a temp", async () => {});
it("backup/size: a number that grows by at least the rounded size plus a header per blob", async () => {
  // before = estimatedBytes; uploadBlob 1000 bytes; after >= before + 1024 + 512
});
// existing header test: dirs toEqual ["blobs/", "snapshots/", `snapshots/${v.id}/`] (sorted)
```

- [ ] **Step 2: Run** `cd R && npx vitest run test/admin-people.test.ts test/backup.test.ts` — Expected: FAIL.
- [ ] **Step 3: Implement** usage, person detail `files`, settings GET/PUT, `estimateBackupBytes`, `GET /backup/size` (register it so `GET /backup` keeps its HEAD behaviour), the backup stream.
- [ ] **Step 4: Docs.** `DESIGN.md` Admin API table: settings rows with `blobQuotaBytes` and `400 invalid_quota`, the new `GET /admin/api/backup/size` row, person detail `files`; Backup section: the archive also holds `blobs/<vault id>/<file>` and can be large; "What the server knows": per vault the number of blobs, each blob's ID, size and upload time, sizes not padded. `README.md` Backup and restore: the archive includes documents and photos and can be large; the Backup page shows an estimate first.
- [ ] **Step 5: Run** `cd R && npm run typecheck && npm test` — Expected: all PASS.
- [ ] **Step 6: Commit** `feat(admin): blob figures, quota setting and blobs in the backup` with the trailer.

### Task 8: Admin interface: quota field, file counts, backup size

**Files:**
- Modify: `R/admin-ui/src/api.ts`, `R/admin-ui/src/screens/Settings.tsx`, `R/admin-ui/src/screens/PersonDetail.tsx`, `R/admin-ui/src/screens/Backup.tsx`, `R/admin-ui/src/i18n/en.ts`, `R/admin-ui/src/i18n/cs.ts`, `R/admin-ui/test/screens.test.tsx`, `R/README.md` (First setup / admin wording)

**Interfaces:**
- Consumes: Task 7 admin API shapes.
- Produces:
  - `api.ts`: `Vault.files: number`; `settings: () => request<{ publicUrl: string | null; blobQuotaBytes: number }>("GET", "settings")`; `saveBlobQuota: (blobQuotaBytes: number) => request<{ blobQuotaBytes: number }>("PUT", "settings", { blobQuotaBytes })`; `backupSize: () => request<{ estimatedBytes: number }>("GET", "backup/size")`.
  - `Settings.tsx`: `QuotaCard` between `AddressCard` and `PasswordCard`, built like `AddressCard` (useResource on `api.settings()`, `LoadError` on failure). The field is in whole GiB, pre-filled with `String(blobQuotaBytes / 1024 ** 3)`; on submit the text must match `/^\d{1,7}$/` and be ≤ 1,048,576, otherwise `t("err.invalid_quota")` shows (ErrorNote `extra`) and no request is sent; else `api.saveBlobQuota(n * 1024 ** 3)` and the saved note.
  - `PersonDetail.tsx`: a column `person.colFiles` after `person.colSpace`, cell `cell--num` showing `v.files`.
  - `Backup.tsx`: under the download card text, a paragraph: while `api.backupSize()` has loaded, `t("backup.sizeText", { size: formatBytes(estimatedBytes, lang) })`; before or on failure `t("backup.sizeUnknown")`.
- Exact copy (add to both dictionaries; change the three existing keys as given):

| Key | en | cs |
|---|---|---|
| `settings.lead` (changed) | Address, space for documents and photos, administrator account, second factor and language. | Adresa, místo na dokumenty a fotky, účet správce, druhý faktor a jazyk. |
| `settings.quotaTitle` | Space for documents and photos | Místo na dokumenty a fotky |
| `settings.quotaText` | The most each profile may store in documents and photos. It applies to every profile on this server. When you lower it, files already stored stay; new ones wait until there is room. | Kolik smí každý profil uložit v dokumentech a fotkách. Platí pro všechny profily na tomto serveru. Když limit snížíte, už uložené soubory zůstanou; nové počkají, až bude místo. |
| `settings.quotaLabel` | Per profile (GiB) | Na profil (GiB) |
| `settings.quotaSave` | Save limit | Uložit limit |
| `settings.quotaSaved` | The limit was saved. | Limit byl uložen. |
| `err.invalid_quota` | Enter a whole number of GiB from 0 to 1048576. | Zadejte celý počet GiB od 0 do 1048576. |
| `person.colFiles` | Files | Soubory |
| `backup.lead` (changed) | One archive with a consistent copy of the database, all snapshot files and all documents and photos. | Jeden archiv s konzistentní kopií databáze, všech souborů se snímky a všech dokumentů a fotek. |
| `backup.sizeText` | Documents and photos are in the archive too, so it can be large. Estimated size: {size}. | Archiv obsahuje i dokumenty a fotky, takže může být velký. Odhadovaná velikost: {size}. |
| `backup.sizeUnknown` | Documents and photos are in the archive too, so it can be large. | Archiv obsahuje i dokumenty a fotky, takže může být velký. |

- [ ] **Step 1: Write the failing tests** in `admin-ui/test/screens.test.tsx`:

```ts
it("settings: the quota field shows GiB and saves bytes", async () => {
  // mocks: GET state, GET overview, GET settings → {publicUrl: null, blobQuotaBytes: 10 * 1024 ** 3}, PUT settings → echo
  // open /settings; field "Per profile (GiB)" value "10"; type "25"; click "Save limit";
  // PUT body toEqual {blobQuotaBytes: 25 * 1024 ** 3}; "The limit was saved." shown
});
it("settings: a quota that is not a whole number is refused before any request", async () => {
  // type "2.5" → alert contains "whole number of GiB"; no PUT call
});
it("person detail shows each profile's file count", async () => { /* vault {..., files: 3} → cell "3" under "Files" */ });
it("backup shows the estimated size, in Czech too", async () => {
  // GET backup/size → {estimatedBytes: 1.2 * 1024 ** 3}: text contains "Estimated size: 1.2 GB"; cs: "Odhadovaná velikost: 1,2 GB"
});
// existing tests: person detail mock vault gains files: 0; settings and backup describes add
// "GET settings" → {publicUrl: null, blobQuotaBytes: 10737418240} and "GET backup/size" → {estimatedBytes: 41943040} mocks
```

- [ ] **Step 2: Run** `cd R && npx vitest run admin-ui/test` — Expected: FAIL.
- [ ] **Step 3: Implement** the api functions, the three screens and the dictionaries.
- [ ] **Step 4: Docs.** `README.md`: under "Add a person" (or a short new paragraph after it) say that each profile may store 10 GiB of documents and photos by default and that Settings changes it for the whole server.
- [ ] **Step 5: Run the whole branch** `cd R && npm run typecheck && npm test && npm run build` — Expected: all PASS, build writes `admin-ui/dist` and `dist/`. Then check by eye that the `DESIGN.md` Limits table matches `src/limits.ts` and that `DESIGN.md` "Not built" no longer lists blob endpoints.
- [ ] **Step 6: Commit** `feat(admin-ui): quota field, file counts and backup size` with the trailer.

---

## Self-review notes (done while writing)

- Spec §6 coverage: storage and record (T2, T3), five calls (T3, T5), UUID check (T2, T3), `features` (T5), quota + admin setting + Settings field (T4, T7, T8), floor for blob and snapshot (T1, T3), budget exemption and two uploads (T3, T4), vault/person/pending-wipe deletion (T2, T6), orphan and `*.tmp` housekeeping (T2, T6), admin figures and file count (T7, T8), backup with `blobs/` and size estimate (T7, T8), "what the server knows" and docs (T3, T5, T7), protocol suite cases (T3–T6). §5.5 server side: list and delete (T5). §8: path safety (T2, T3), server holds only ciphertext (no parsing anywhere).
- Names used across tasks: `BlobFiles.writeTemp/publish/discard/open/size/remove/removeVault/sweepTemp/listFiles/listVaultDirs`, `Blobs.check/commit/find/fits/list/remove/housekeep/sweep`, `blobQuotaBytes/setBlobQuotaBytes`, `UploadGate.tryEnter`, `RouteDeps.{stores, limiters, budget, freeBytes, uploads}`, `blobError`, `hasRoom`, `FreeBytes`, `AppDeps`, `Stores.{blobFiles, blobs}`, helpers `newBlobId/blobFiles/stream/uploadBlob/heldBlobUpload/countingBody`, `t.disk.free`.
