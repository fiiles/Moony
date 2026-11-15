//! The fake's blob store (Moony-relay `src/blob-api.ts`, `DESIGN.md` "Blobs"): per vault, the
//! sealed bytes of each blob and when the server stored it, plus the knobs a test turns (quota, a
//! full disk, tampered, cut or lost files, the clock a listing reports).

use std::collections::BTreeMap;

use uuid::Uuid;

use super::{check_vault, now, Access, FakeRelay, Route};
use crate::services::sync::envelope::blob::MAX_BLOB_SEALED;
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::{
    BlobBody, BlobPage, BlobSink, Delivery, RelayError, RelayResult, RemoteBlob, SinkFlow,
    MAX_BLOB_PAGE,
};

/// The server's default blob quota per vault (admin Settings, 10 GiB).
pub(super) const DEFAULT_BLOB_QUOTA: u64 = 10 * 1024 * 1024 * 1024;
/// How much a download hands the sink at a time: deliberately not a multiple of the 64 KiB
/// stream chunk, as network pieces never are.
const PIECE: usize = 50_000;

/// One stored blob: its sealed bytes and when the server stored it (unix seconds).
pub(super) struct StoredBlob {
    pub(super) bytes: Vec<u8>,
    pub(super) created_at: i64,
}

/// A vault's blobs, ascending by ID (the listing's order).
pub(super) type VaultBlobs = BTreeMap<Uuid, StoredBlob>;

impl FakeRelay {
    /// The per-vault blob quota in bytes (default 10 GiB): an upload that would take the vault's
    /// blobs past it answers `QuotaExceeded`.
    pub fn set_blob_quota(&self, bytes: u64) {
        self.lock().blob_quota = bytes;
    }

    /// A full disk: every upload of a blob or a snapshot answers `InsufficientStorage` (an
    /// existing blob's upload still answers success first, as on the server).
    pub fn set_no_room(&self, on: bool) {
        self.lock().no_room = on;
    }

    /// What `list_blobs` reports as the server's clock: `Some(at)`, or `None` for a server that
    /// sends none. Until this is called, the fake's own clock.
    pub fn set_server_now(&self, at: Option<i64>) {
        self.lock().server_now = Some(at);
    }

    /// The server's copy of a blob changes: one byte in the middle is flipped.
    pub fn tamper_blob(&self, vault_id: &str, blob: Uuid) {
        self.with_blob(vault_id, blob, |stored| {
            let middle = stored.bytes.len() / 2;
            if let Some(byte) = stored.bytes.get_mut(middle) {
                *byte ^= 0x01;
            }
        });
    }

    /// The server's copy of a blob is cut to its first `keep` bytes.
    pub fn truncate_blob(&self, vault_id: &str, blob: Uuid, keep: usize) {
        self.with_blob(vault_id, blob, |stored| stored.bytes.truncate(keep));
    }

    /// The server lost a blob (its record and its file).
    pub fn lose_blob(&self, vault_id: &str, blob: Uuid) {
        if let Some(vault) = self.lock().vaults.get_mut(vault_id) {
            vault.blobs.remove(&blob);
        }
    }

    /// When the server says it stored a blob (unix seconds on its clock).
    pub fn set_blob_created_at(&self, vault_id: &str, blob: Uuid, at: i64) {
        self.with_blob(vault_id, blob, |stored| stored.created_at = at);
    }

    /// The IDs of the blobs the vault holds, ascending.
    pub fn blob_ids(&self, vault_id: &str) -> Vec<Uuid> {
        self.lock()
            .vaults
            .get(vault_id)
            .map(|vault| vault.blobs.keys().copied().collect())
            .unwrap_or_default()
    }

    /// How many blob uploads were asked for, whatever they answered (refused ones and ones of a
    /// blob the server already had included; the client's own argument checks excluded).
    pub fn blob_puts(&self) -> usize {
        self.lock()
            .calls
            .iter()
            .filter(|route| **route == Route::PutBlob)
            .count()
    }

    fn with_blob(&self, vault_id: &str, blob: Uuid, change: impl FnOnce(&mut StoredBlob)) {
        if let Some(stored) = self
            .lock()
            .vaults
            .get_mut(vault_id)
            .and_then(|vault| vault.blobs.get_mut(&blob))
        {
            change(stored);
        }
    }

    /// `PUT /v1/vaults/:id/blobs/:blob`, in the server's order of checks: token and entitlement,
    /// an existing blob (success without reading the body), the length, the quota, the free
    /// space, then the body.
    pub(super) fn blob_put(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        check_vault(vault)?;
        // `HttpRelay` refuses this before sending anything.
        if sealed_len > MAX_BLOB_SEALED {
            return Err(RelayError::Protocol("body too large".into()));
        }
        let mut state = self.lock();
        state.enter(Route::PutBlob)?;
        let stored = state.authorize(vault, Access::Write)?;
        if stored.blobs.contains_key(&blob) {
            return Ok(());
        }
        if sealed_len == 0 {
            return Err(RelayError::BadRequest("length_required".into()));
        }
        let held: u64 = stored.blobs.values().map(|b| b.bytes.len() as u64).sum();
        if held + sealed_len > state.blob_quota {
            return Err(RelayError::QuotaExceeded);
        }
        if state.no_room {
            return Err(RelayError::InsufficientStorage);
        }
        // `HttpRelay` stops a body that is not its announced length before the server
        // answers; nothing is stored.
        let mut bytes = Vec::new();
        for piece in body {
            let piece = piece.map_err(|_| RelayError::Network("body".into()))?;
            if (bytes.len() + piece.len()) as u64 > sealed_len {
                return Err(RelayError::BadRequest("length_required".into()));
            }
            bytes.extend_from_slice(&piece);
        }
        if bytes.len() as u64 != sealed_len {
            return Err(RelayError::BadRequest("length_required".into()));
        }
        state.vault_mut(&vault.vault_id).blobs.insert(
            blob,
            StoredBlob {
                bytes,
                created_at: now(),
            },
        );
        Ok(())
    }

    /// `HEAD /v1/vaults/:id/blobs/:blob`.
    pub(super) fn blob_head(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<Option<u64>> {
        check_vault(vault)?;
        let mut state = self.lock();
        state.enter(Route::HeadBlob)?;
        let stored = state.authorize(vault, Access::Read)?;
        Ok(stored.blobs.get(&blob).map(|b| b.bytes.len() as u64))
    }

    /// `GET /v1/vaults/:id/blobs/:blob`, handed to the sink in pieces.
    pub(super) fn blob_get(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        check_vault(vault)?;
        let bytes = {
            let mut state = self.lock();
            state.enter(Route::GetBlob)?;
            let stored = state.authorize(vault, Access::Read)?;
            stored
                .blobs
                .get(&blob)
                .ok_or(RelayError::BlobNotFound)?
                .bytes
                .clone()
        };
        // The sink runs without the fake's lock, as it would without a connection.
        if sink.begin(bytes.len() as u64) == SinkFlow::Stop {
            return Ok(Delivery::Stopped);
        }
        for piece in bytes.chunks(PIECE) {
            if sink.piece(piece) == SinkFlow::Stop {
                return Ok(Delivery::Stopped);
            }
        }
        Ok(Delivery::Complete)
    }

    /// `GET /v1/vaults/:id/blobs?after=&limit=`.
    pub(super) fn blob_list(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        check_vault(vault)?;
        // `HttpRelay` refuses this before sending anything.
        if !(1..=MAX_BLOB_PAGE).contains(&limit) {
            return Err(RelayError::Protocol("limit".into()));
        }
        let mut state = self.lock();
        state.enter(Route::ListBlobs)?;
        let server_now = state.server_now.unwrap_or_else(|| Some(now()));
        let stored = state.authorize(vault, Access::Read)?;
        let mut listed = stored
            .blobs
            .iter()
            .filter(|(id, _)| after.is_none_or(|after| **id > after))
            .map(|(id, blob)| RemoteBlob {
                id: *id,
                size: blob.bytes.len() as u64,
                created_at: blob.created_at,
            })
            .peekable();
        let blobs: Vec<RemoteBlob> = listed.by_ref().take(limit as usize).collect();
        Ok(BlobPage {
            blobs,
            more: listed.peek().is_some(),
            server_now,
        })
    }

    /// `DELETE /v1/vaults/:id/blobs/:blob`: the current token, never refused for payment;
    /// success whether or not the blob existed.
    pub(super) fn blob_delete(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<()> {
        check_vault(vault)?;
        let mut state = self.lock();
        state.enter(Route::DeleteBlob)?;
        state.authorize(vault, Access::Owner)?;
        state.vault_mut(&vault.vault_id).blobs.remove(&blob);
        Ok(())
    }
}
