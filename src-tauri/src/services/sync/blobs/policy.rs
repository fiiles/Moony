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

/// What the server said about its blob routes (its `blobs` feature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerBlobs {
    /// It has them.
    Yes,
    /// It said it has not: it needs an update.
    No,
    /// It could not be asked (offline, a passing error): try again later, nothing to show.
    Unknown,
}

impl From<bool> for ServerBlobs {
    fn from(blobs: bool) -> Self {
        if blobs {
            ServerBlobs::Yes
        } else {
            ServerBlobs::No
        }
    }
}

/// A transfer lane: uploads or downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Up,
    Down,
}

/// Shared by the round's upload step and the two lanes of one engine; clones share everything.
#[derive(Clone)]
pub struct BlobPolicy {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    up: tokio::sync::Mutex<()>,
    down: tokio::sync::Mutex<()>,
    /// One per lane (`Lane::Up`, `Lane::Down`).
    wakes: [Notify; 2],
    eager_full: bool,
}

#[derive(Default)]
struct State {
    /// The server's last answer: whether it has the blob routes, and when it gave it.
    server: Option<(bool, i64)>,
    problem: Option<SyncAttachmentsProblem>,
    /// Uploads wait until then (unix seconds).
    paused_until: i64,
    /// Per lane, a wake its `wait` has not taken yet.
    pending: [bool; 2],
}

fn index(lane: Lane) -> usize {
    match lane {
        Lane::Up => 0,
        Lane::Down => 1,
    }
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
                wakes: [Notify::new(), Notify::new()],
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

    /// Whether the server has the blob routes (its `blobs` feature): its last answer while that
    /// is younger than six hours, else asked again (`GET /v1/server`), and a fresh answer is kept
    /// from now. A server that could not be asked is `Unknown`: nothing is kept and no problem is
    /// set, so the next run asks again. A `Yes` ends the "needs an update" problem.
    pub async fn server_has_blobs<R: Relay>(&self, relay: &R, now: i64) -> ServerBlobs {
        if let Some((blobs, at)) = self.state().server {
            if (0..SERVER_TTL).contains(&now.saturating_sub(at)) {
                return ServerBlobs::from(blobs);
            }
        }
        match relay.server_info().await {
            Ok(info) => {
                let mut state = self.state();
                state.server = Some((info.blobs, now));
                if info.blobs && state.problem == Some(SyncAttachmentsProblem::ServerNeedsUpdate) {
                    state.problem = None;
                }
                ServerBlobs::from(info.blobs)
            }
            Err(e) => {
                log::debug!("[SYNC] The sync server's features could not be read: {e}");
                ServerBlobs::Unknown
            }
        }
    }

    /// Forgets the server's answer (a new server address): the next run asks again.
    pub fn forget_server(&self) {
        self.state().server = None;
    }

    /// The person's "Sync now" / "try again": the server is asked again and the pause for room
    /// ends (the problem stays until a body is accepted or nothing waits to upload).
    pub fn try_again(&self) {
        let mut state = self.state();
        state.server = None;
        state.paused_until = 0;
    }

    pub fn problem(&self) -> Option<SyncAttachmentsProblem> {
        self.state().problem
    }

    /// Whether uploads wait for room on the server. A pause that ends further ahead than its own
    /// length was dated by a clock since set back: it is over.
    pub fn uploads_paused(&self, now: i64) -> bool {
        let left = self.state().paused_until.saturating_sub(now);
        left > 0 && left <= NO_ROOM_PAUSE
    }

    /// The server has no room (its quota, or its disk): uploads wait half an hour, and the
    /// problem says so.
    pub fn note_no_room(&self, now: i64) {
        let mut state = self.state();
        state.problem = Some(SyncAttachmentsProblem::NoRoom);
        state.paused_until = now.saturating_add(NO_ROOM_PAUSE);
    }

    /// The server accepted an upload's body: there is room again. (A blob it already had stored
    /// nothing and says nothing about room.)
    pub fn note_uploaded(&self) {
        self.clear(SyncAttachmentsProblem::NoRoom);
    }

    /// The server's answer about its blob routes, given at `now`. Without them the problem says
    /// the server needs an update (callers note that only while content waits to move); with
    /// them that problem goes. The same answer as the one kept keeps its time, so noting it again
    /// never extends how long it is trusted.
    pub fn note_server(&self, blobs: bool, now: i64) {
        let mut state = self.state();
        if !matches!(state.server, Some((kept, _)) if kept == blobs) {
            state.server = Some((blobs, now));
        }
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

    /// Wakes both lanes.
    pub fn wake(&self) {
        self.wake_lane(Lane::Up);
        self.wake_lane(Lane::Down);
    }

    /// Wakes one lane: its current `wait` returns, or, while the lane runs, its next one returns
    /// at once. The wake is that lane's alone; the other lane never takes it.
    pub fn wake_lane(&self, lane: Lane) {
        self.state().pending[index(lane)] = true;
        self.inner.wakes[index(lane)].notify_waiters();
    }

    /// Returns after a wake of `lane` (one that came while it ran included), or after `at_most`.
    pub async fn wait(&self, lane: Lane, at_most: Duration) {
        let notified = self.inner.wakes[index(lane)].notified();
        tokio::pin!(notified);
        // Registered before the pending wake is read, so a wake in between is not missed.
        notified.as_mut().enable();
        if self.take_pending(lane) {
            return;
        }
        if tokio::time::timeout(at_most, notified).await.is_ok() {
            self.take_pending(lane);
        }
    }

    /// Takes `lane`'s pending wake; whether there was one.
    fn take_pending(&self, lane: Lane) -> bool {
        std::mem::take(&mut self.state().pending[index(lane)])
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
