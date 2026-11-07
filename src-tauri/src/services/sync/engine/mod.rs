//! The sync engine (spec §9.4): the steps of a sync round against the relay. Every step reads or
//! writes the profile under the connection lock and releases it before any network call
//! (`ConnAccess`). `pull` brings other devices' envelopes in.

pub mod pull;

use rusqlite::Connection;

use crate::error::Result;

/// Short access to the profile's connection: the engine takes it for one step at a time and never
/// holds it across a network call. `Database` locks its session mutex; a locked profile answers
/// `AppError::Auth`, which ends the round.
pub trait ConnAccess: Clone + Send + Sync + 'static {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>;
}

impl ConnAccess for crate::db::Database {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        crate::db::Database::with_conn(self, f)
    }
}

/// Why a pull stopped before the relay's end. The cursor stays before the envelope that stopped
/// it, so the next pull meets it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStop {
    /// An envelope of an author is missing (the relay dropped it).
    Gap,
    /// A malformed chain link, a changeset that does not parse, or a pull page outside the
    /// relay contract (a sequence not above `after` or not increasing, a head below the page's
    /// last sequence, an empty page that says there is more).
    Invalid,
    /// An envelope that does not open: a failed authentication, or an epoch this device has no
    /// key for (never read as a rekey: the epoch is not authenticated).
    Undecryptable,
    /// Written by a newer schema than this build knows.
    NewerSchema,
    /// A changeset references a parent this device has never seen; the merge rolled back.
    MissingParent,
}

/// How a pull ended (ruling P1), from `PullReport::end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullEnd {
    /// Every envelope up to the relay's end was dealt with.
    Done,
    /// Every envelope was dealt with except a quarantined author's, which are held.
    Held,
    /// The pull stopped on an envelope (or a page) and went no further.
    Stopped(PullStop),
}

/// What one pull did.
#[derive(Debug, Default)]
pub struct PullReport {
    /// Changesets merged.
    pub applied: usize,
    pub stop: Option<PullStop>,
    /// This device re-joined under a new device ID (a clone or a rollback).
    pub rejoined: bool,
    /// Envelopes held for a quarantined author.
    pub held: usize,
    /// The profile was restored from a snapshot (the engine's `410` handling, Task 11).
    pub restored: bool,
    /// The relay's head is past the last envelope it served: it withholds a trailing envelope (or
    /// served none), so this device is behind. Shown in diagnostics; not an error, and the cursor
    /// never moves to the head.
    pub behind: bool,
}

impl PullReport {
    /// The pull reached the relay's end with nothing stopped or held (P23).
    pub fn done(&self) -> bool {
        self.stop.is_none() && self.held == 0
    }

    pub fn end(&self) -> PullEnd {
        match &self.stop {
            Some(stop) => PullEnd::Stopped(stop.clone()),
            None if self.held > 0 => PullEnd::Held,
            None => PullEnd::Done,
        }
    }

    /// Whether the round may push after this pull (ruling P1): the pull ended done, with only a
    /// quarantined author's envelopes held, or stopped only on a newer schema (an older device
    /// keeps pushing, spec §14).
    pub fn may_push(&self) -> bool {
        matches!(
            self.end(),
            PullEnd::Done | PullEnd::Held | PullEnd::Stopped(PullStop::NewerSchema)
        )
    }
}

#[cfg(test)]
pub(crate) mod tests;
