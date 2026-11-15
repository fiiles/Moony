//! What the blob transfers of one running engine share (ADR 0030): whether the sync server has the
//! blob routes (asked at most every six hours), the attachments problem it shows
//! (`SyncAttachmentsProblem`), the pause of uploads while the server has no room, the two lane
//! locks (one upload and one download at a time, whoever runs them: the round's upload step or
//! the lanes), the wake-up of the lanes, and whether this device fetches full content eagerly.
//!
//! None of it touches the round's own state: a blob problem never pauses or fails ordinary sync.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;

use crate::models::sync::SyncAttachmentsProblem;
use crate::services::sync::relay::Relay;

/// How long the server's answer to `GET /v1/server` is trusted.
const SERVER_TTL: i64 = 6 * 3_600;
/// How long uploads wait after the server said it has no room.
const NO_ROOM_PAUSE: i64 = 1_800;
/// The lanes that share a policy (uploads, downloads): how many waits one wake ends.
const LANES: u8 = 2;

/// Shared by the round's upload step and the two lanes of one engine; clones share everything.
#[derive(Clone)]
pub struct BlobPolicy {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    up: tokio::sync::Mutex<()>,
    down: tokio::sync::Mutex<()>,
    wake: Notify,
    eager_full: bool,
}

#[derive(Default)]
struct State {
    /// The server's last answer: whether it has the blob routes, and when it was asked.
    server: Option<(bool, i64)>,
    problem: Option<SyncAttachmentsProblem>,
    /// Uploads wait until then (unix seconds).
    paused_until: i64,
    /// Wakes not yet taken by a `wait`, at most one per lane.
    pending: u8,
}

impl BlobPolicy {
    /// `eager_full`: fetch documents and full photos without being asked (a desktop); a phone
    /// fetches thumbnails and what the person opens.
    pub fn new(eager_full: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                up: tokio::sync::Mutex::new(()),
                down: tokio::sync::Mutex::new(()),
                wake: Notify::new(),
                eager_full,
            }),
        }
    }

    /// A desktop fetches everything in the background; a phone only on demand.
    pub fn for_this_platform() -> Self {
        Self::new(!cfg!(any(target_os = "ios", target_os = "android")))
    }

    pub fn eager_full(&self) -> bool {
        self.inner.eager_full
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether the server has the blob routes (its `blobs` feature): its last answer while it is
    /// younger than six hours, else asked again (`GET /v1/server`). A server that cannot be asked
    /// counts as without them this time, remembered for nothing and with no problem set: the
    /// next run asks again.
    pub async fn server_has_blobs<R: Relay>(&self, relay: &R, now: i64) -> bool {
        if let Some((blobs, at)) = self.state().server {
            if now.saturating_sub(at) < SERVER_TTL && now >= at {
                return blobs;
            }
        }
        match relay.server_info().await {
            Ok(info) => {
                let mut state = self.state();
                state.server = Some((info.blobs, now));
                if info.blobs && state.problem == Some(SyncAttachmentsProblem::ServerNeedsUpdate) {
                    state.problem = None;
                }
                info.blobs
            }
            Err(e) => {
                log::debug!("[SYNC] The sync server's features could not be read: {e}");
                false
            }
        }
    }

    /// Forgets the server's answer ("Sync now", a new server address): the next run asks again.
    pub fn forget_server(&self) {
        self.state().server = None;
    }

    pub fn problem(&self) -> Option<SyncAttachmentsProblem> {
        self.state().problem
    }

    /// Whether uploads wait for room on the server.
    pub fn uploads_paused(&self, now: i64) -> bool {
        self.state().paused_until > now
    }

    /// The server has no room (its quota, or its disk): uploads wait half an hour, and the
    /// problem says so.
    pub fn note_no_room(&self, now: i64) {
        let mut state = self.state();
        state.problem = Some(SyncAttachmentsProblem::NoRoom);
        state.paused_until = now.saturating_add(NO_ROOM_PAUSE);
    }

    /// An upload went through: there is room again.
    pub fn note_uploaded(&self) {
        self.clear(SyncAttachmentsProblem::NoRoom);
    }

    /// The server's answer about its blob routes, as of `now`. Without them the problem says the
    /// server needs an update (the caller notes that only while content waits to move); with
    /// them that problem goes.
    pub fn note_server(&self, blobs: bool, now: i64) {
        let mut state = self.state();
        state.server = Some((blobs, now));
        if !blobs {
            state.problem = Some(SyncAttachmentsProblem::ServerNeedsUpdate);
        } else if state.problem == Some(SyncAttachmentsProblem::ServerNeedsUpdate) {
            state.problem = None;
        }
    }

    /// `problem` no longer holds (nothing waits that it would stop); room also ends the pause.
    pub(super) fn clear(&self, problem: SyncAttachmentsProblem) {
        let mut state = self.state();
        if state.problem == Some(problem) {
            state.problem = None;
        }
        if problem == SyncAttachmentsProblem::NoRoom {
            state.paused_until = 0;
        }
    }

    /// Wakes the lanes: each lane's current or next `wait` returns at once (a wake while a lane
    /// runs is kept for its next wait, so it is never lost).
    pub fn wake(&self) {
        self.state().pending = LANES;
        self.inner.wake.notify_waiters();
    }

    /// Returns after a wake, or after `at_most`.
    pub async fn wait(&self, at_most: Duration) {
        let notified = self.inner.wake.notified();
        tokio::pin!(notified);
        // Registered before the pending wakes are read, so a wake in between is not missed.
        notified.as_mut().enable();
        if self.take_pending() {
            return;
        }
        if tokio::time::timeout(at_most, notified).await.is_ok() {
            self.take_pending();
        }
    }

    /// Takes one pending wake; whether there was one.
    fn take_pending(&self) -> bool {
        let mut state = self.state();
        if state.pending == 0 {
            return false;
        }
        state.pending -= 1;
        true
    }

    /// One upload at a time.
    pub fn up_lane(&self) -> &tokio::sync::Mutex<()> {
        &self.inner.up
    }

    /// One download at a time.
    pub fn down_lane(&self) -> &tokio::sync::Mutex<()> {
        &self.inner.down
    }
}

impl Default for BlobPolicy {
    fn default() -> Self {
        Self::new(true)
    }
}
