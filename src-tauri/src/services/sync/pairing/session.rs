//! The pairing session (phase 3, spec §20, ADR 0026): the state machine that runs the bytes of
//! `pairing` over the relay's two-slot mailbox. The shower (S) puts a QR code on its screen; the
//! scanner (C) reads it, writes slot 1 (its role and public key), and both derive the same
//! six-digit comparison code. After the person has confirmed the codes match on each device,
//! the holder seals the credentials into slot 2 and the joiner opens them.
//!
//! The security rules this file keeps:
//! - The holder writes slot 2 only after its own confirm, and the joiner reads slot 2 only after
//!   its own confirm. A scanner whose slot 1 was taken by someone else (contested) shows its own
//!   code, which differs from the shower's, and can never confirm.
//! - The ephemeral X25519 secret is taken out of its `Option` when it is used and dropped (and
//!   zeroized) right after. The session key is dropped once slot 2 is sealed or opened.
//! - Received credentials live until `expires_at + RECEIVED_GRACE_SECS`; any call after that
//!   wipes them.
//! - No error or `Debug` output carries a secret, a pairing code, a comparison code or the
//!   mailbox ID.
//!
//! The session never retries on its own and has no clock: every call takes `now`, and the
//! caller decides with `keeps_session` whether an error ends the pairing.

use std::fmt;
use std::time::Duration;

use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::{
    check_roles, derive, is_cancel_marker, open_credentials, parse_pairing_code, parse_slot1,
    seal_credentials, slot1, MailboxId, PairingCode, Role, Side, Transcript, CANCEL_MARKER,
};
use crate::error::{AppError, Result};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::keys::{os_random, Credentials};
use crate::services::sync::relay::http::{is_default_relay, normalize_base_url, DEFAULT_RELAY_URL};
use crate::services::sync::relay::{Mailbox, RelayError};

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

/// How long a pairing lives on this device (the relay's mailbox lives as long from its first
/// write).
pub const PAIRING_LIFETIME_SECS: i64 = 600;
/// How long received credentials outlive the pairing, so that a join that failed for a moment
/// can be tried again.
pub const RECEIVED_GRACE_SECS: i64 = 300;
/// The most `abandon` spends on its cancel markers and the delete.
pub const ABANDON_BUDGET: Duration = Duration::from_secs(3);

/// Slot 1: the scanner's reply. Slot 2: the holder's sealed credentials.
const SCANNER_SLOT: u8 = 1;
const HOLDER_SLOT: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Scanner: slot 1 is not written yet (or its answer was lost).
    Announcing,
    /// Shower: waiting for a scanner's slot 1.
    WaitingForScan,
    /// Both: the comparison code is on screen, waiting for the person.
    Compare,
    /// Joiner, after its confirm: waiting for slot 2.
    WaitingForOther,
    /// Holder, after its confirm: slot 2 is written.
    Sent,
    /// Joiner: slot 2 opened; the credentials are held until `wiped_at`.
    Received,
}

pub struct PairingSession {
    side: Side,
    role: Role,
    mailbox: MailboxId,
    relay_url: String,
    /// Taken out at derive.
    secret: Option<StaticSecret>,
    own_public: [u8; 32],
    /// The scanner knows it from the code; the shower from slot 1.
    peer_public: Option<[u8; 32]>,
    /// From Compare until slot 2 is sealed (holder) or opened (joiner).
    transcript: Option<Transcript>,
    /// The comparison code, kept after the transcript is dropped.
    comparison: Option<u32>,
    stage: Stage,
    contested: bool,
    /// A put to slot 1 / slot 2 whose answer was lost (network or server error): the relay may
    /// have stored it, so a later `SlotTaken` is our own write.
    ambiguous_put: [bool; 2],
    /// Slots this side is known to have written.
    written: [bool; 2],
    received: Option<Credentials>,
    expires_at: i64,
}

fn pairing_error(key: &str) -> SyncError {
    SyncError::Local(AppError::Validation(key.into()))
}

fn expired() -> SyncError {
    pairing_error("validation.syncPairingExpired")
}

fn cancelled() -> SyncError {
    pairing_error("validation.syncPairingCancelled")
}

fn failed() -> SyncError {
    pairing_error("validation.syncPairingFailed")
}

fn code_used() -> SyncError {
    pairing_error("validation.syncPairingCodeUsed")
}

fn internal(what: &str) -> SyncError {
    SyncError::Local(AppError::Internal(format!("sync pairing: {what}")))
}

/// A put whose answer was lost may have been stored.
fn put_was_ambiguous(error: &RelayError) -> bool {
    matches!(error, RelayError::Network(_) | RelayError::Server(_))
}

fn random_secret() -> Result<StaticSecret> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    os_random(&mut bytes[..])?;
    Ok(StaticSecret::from(*bytes))
}

impl PairingSession {
    /// A new pairing this device shows as a QR code, on the mailbox server `relay_url`. Nothing
    /// is sent: the shower's first call is a `poll`.
    pub fn show(role: Role, relay_url: &str, now: i64) -> Result<Self> {
        Self::new_shower(role, relay_url, now, random_secret()?)
    }

    /// A pairing started by scanning `code_text`; the mailbox server is the one the code names,
    /// or the default. The scanner's first call is `announce`.
    pub fn scan(role: Role, code_text: &str, now: i64) -> Result<Self> {
        Self::new_scanner(role, code_text, now, random_secret()?)
    }

    fn new_shower(role: Role, relay_url: &str, now: i64, secret: StaticSecret) -> Result<Self> {
        let relay_url = normalize_base_url(relay_url)?;
        Ok(Self::new(
            Side::Shower,
            role,
            MailboxId::random()?,
            relay_url,
            secret,
            None,
            now,
        ))
    }

    fn new_scanner(role: Role, code_text: &str, now: i64, secret: StaticSecret) -> Result<Self> {
        let code = parse_pairing_code(code_text)?;
        let relay_url = code
            .relay_url
            .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string());
        Ok(Self::new(
            Side::Scanner,
            role,
            code.mailbox,
            relay_url,
            secret,
            Some(code.shower_public),
            now,
        ))
    }

    fn new(
        side: Side,
        role: Role,
        mailbox: MailboxId,
        relay_url: String,
        secret: StaticSecret,
        peer_public: Option<[u8; 32]>,
        now: i64,
    ) -> Self {
        let own_public = PublicKey::from(&secret).to_bytes();
        PairingSession {
            side,
            role,
            mailbox,
            relay_url,
            secret: Some(secret),
            own_public,
            peer_public,
            transcript: None,
            comparison: None,
            stage: match side {
                Side::Shower => Stage::WaitingForScan,
                Side::Scanner => Stage::Announcing,
            },
            contested: false,
            ambiguous_put: [false; 2],
            written: [false; 2],
            received: None,
            expires_at: now + PAIRING_LIFETIME_SECS,
        }
    }

    #[cfg(test)]
    fn show_with_secret(
        role: Role,
        relay_url: &str,
        now: i64,
        secret: StaticSecret,
    ) -> Result<Self> {
        Self::new_shower(role, relay_url, now, secret)
    }

    #[cfg(test)]
    fn scan_with_secret(
        role: Role,
        code_text: &str,
        now: i64,
        secret: StaticSecret,
    ) -> Result<Self> {
        Self::new_scanner(role, code_text, now, secret)
    }

    #[cfg(test)]
    fn has_secret(&self) -> bool {
        self.secret.is_some()
    }

    /// The pairing code of this session: the one the shower shows, or the one the scanner read.
    pub fn code(&self) -> PairingCode {
        let shower_public = match self.side {
            Side::Shower => self.own_public,
            Side::Scanner => self.peer_public.unwrap_or(self.own_public),
        };
        PairingCode {
            mailbox: self.mailbox,
            shower_public,
            relay_url: Some(self.relay_url.clone()).filter(|url| !is_default_relay(url)),
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn role(&self) -> Role {
        self.role
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn relay_url(&self) -> &str {
        &self.relay_url
    }

    pub fn expires_at(&self) -> i64 {
        self.expires_at
    }

    /// Six digits, from Compare on.
    pub fn comparison_code(&self) -> Option<String> {
        self.comparison.map(|code| format!("{code:06}"))
    }

    /// Scanner: someone else wrote slot 1 first; this device's code differs from the shower's.
    pub fn contested(&self) -> bool {
        self.contested
    }

    pub fn received(&self) -> Option<&Credentials> {
        self.received.as_ref()
    }

    /// When every call starts answering `syncPairingExpired`: `expires_at`, or, once the
    /// credentials are received, `expires_at + RECEIVED_GRACE_SECS`.
    pub fn wiped_at(&self) -> i64 {
        match self.stage {
            Stage::Received => self.expires_at + RECEIVED_GRACE_SECS,
            _ => self.expires_at,
        }
    }

    /// Scanner, in Announcing: writes slot 1 and derives the comparison code → Compare. Any
    /// other stage is returned untouched.
    pub async fn announce<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<Stage> {
        self.check_alive(now)?;
        if self.stage != Stage::Announcing {
            return Ok(self.stage);
        }
        let id = self.mailbox.to_b64();
        let record = slot1(self.role, &self.own_public);
        match mailbox.put_slot(&id, SCANNER_SLOT, &record).await {
            Ok(()) => {
                self.written[0] = true;
                self.enter_compare()
            }
            // Our own earlier write, whose answer was lost.
            Err(RelayError::SlotTaken) if self.ambiguous_put[0] => {
                self.written[0] = true;
                self.enter_compare()
            }
            Err(RelayError::SlotTaken) => self.contest(mailbox, &id).await,
            Err(error) => {
                if put_was_ambiguous(&error) {
                    self.ambiguous_put[0] = true;
                }
                Err(self.relay_error(error))
            }
        }
    }

    /// Someone else wrote slot 1 first: read it once. A cancel marker means the shower gave up;
    /// anything else (an impostor's record, or nothing left to read) leaves this device showing
    /// its own code, which cannot match the shower's, flagged as contested.
    async fn contest<M: Mailbox>(&mut self, mailbox: &M, id: &str) -> SyncResult<Stage> {
        match mailbox.take_slot(id, SCANNER_SLOT).await {
            Ok(Some(bytes)) if is_cancel_marker(&bytes) => Err(cancelled()),
            Ok(_) | Err(RelayError::SlotConsumed) => {
                self.contested = true;
                self.enter_compare()
            }
            Err(error) => Err(self.relay_error(error)),
        }
    }

    /// One step of waiting: the scanner retries its announce, the shower reads slot 1, the
    /// confirmed joiner reads slot 2. Every other stage is returned untouched and sends nothing.
    pub async fn poll<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<Stage> {
        self.check_alive(now)?;
        match self.stage {
            Stage::Announcing => self.announce(mailbox, now).await,
            Stage::WaitingForScan => self.read_scan(mailbox).await,
            Stage::WaitingForOther => self.read_credentials(mailbox).await,
            stage @ (Stage::Compare | Stage::Sent | Stage::Received) => Ok(stage),
        }
    }

    async fn read_scan<M: Mailbox>(&mut self, mailbox: &M) -> SyncResult<Stage> {
        let id = self.mailbox.to_b64();
        let bytes = match mailbox.take_slot(&id, SCANNER_SLOT).await {
            Ok(None) => return Ok(self.stage),
            Ok(Some(bytes)) => bytes,
            Err(error) => return Err(self.relay_error(error)),
        };
        if is_cancel_marker(&bytes) {
            return Err(cancelled());
        }
        let (theirs, scanner_public) = parse_slot1(&bytes)?;
        check_roles(self.role, theirs)?;
        self.peer_public = Some(scanner_public);
        self.enter_compare()
    }

    async fn read_credentials<M: Mailbox>(&mut self, mailbox: &M) -> SyncResult<Stage> {
        let id = self.mailbox.to_b64();
        let sealed = match mailbox.take_slot(&id, HOLDER_SLOT).await {
            Ok(None) => return Ok(self.stage),
            Ok(Some(bytes)) => bytes,
            Err(error) => return Err(self.relay_error(error)),
        };
        if is_cancel_marker(&sealed) {
            return Err(cancelled());
        }
        let transcript = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?;
        let credentials = open_credentials(transcript, &sealed)?;
        self.received = Some(credentials);
        self.transcript = None;
        self.stage = Stage::Received;
        // Nothing is left to read; a mailbox that stays expires on its own.
        let _ = mailbox.burn_mailbox(&id).await;
        Ok(self.stage)
    }

    /// The person confirmed the codes match. Only from Compare: the holder seals the
    /// credentials into slot 2 → Sent; the joiner starts waiting for them → WaitingForOther,
    /// sending nothing. A contested scanner cannot confirm.
    pub async fn confirm<M: Mailbox>(
        &mut self,
        mailbox: &M,
        holder_creds: Option<&Credentials>,
        now: i64,
    ) -> SyncResult<Stage> {
        self.check_alive(now)?;
        if self.stage != Stage::Compare {
            return Err(internal("there is nothing to confirm"));
        }
        if self.contested {
            return Err(code_used());
        }
        match self.role {
            Role::Joiner => {
                self.stage = Stage::WaitingForOther;
                Ok(self.stage)
            }
            Role::Holder => self.send(mailbox, holder_creds).await,
        }
    }

    async fn send<M: Mailbox>(
        &mut self,
        mailbox: &M,
        holder_creds: Option<&Credentials>,
    ) -> SyncResult<Stage> {
        let creds = holder_creds.ok_or_else(|| internal("the holder has no credentials"))?;
        let transcript = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?;
        let sealed = seal_credentials(transcript, creds)?;
        let id = self.mailbox.to_b64();
        match mailbox.put_slot(&id, HOLDER_SLOT, &sealed).await {
            Ok(()) => self.enter_sent(),
            Err(RelayError::SlotTaken) if self.ambiguous_put[1] => self.enter_sent(),
            Err(error) => {
                if put_was_ambiguous(&error) {
                    self.ambiguous_put[1] = true;
                }
                Err(self.relay_error(error))
            }
        }
    }

    /// Gives the pairing up: within `ABANDON_BUDGET`, a cancel marker into each slot this side
    /// has not written (so a waiting peer reads "cancelled" even if the delete fails), then the
    /// delete. Both best effort; the secret, the session key and the credentials are wiped.
    pub async fn abandon<M: Mailbox>(mut self, mailbox: &M) {
        let id = self.mailbox.to_b64();
        let written = self.written;
        let _ = tokio::time::timeout(ABANDON_BUDGET, async {
            for (index, slot) in [SCANNER_SLOT, HOLDER_SLOT].into_iter().enumerate() {
                if !written[index] {
                    let _ = mailbox.put_slot(&id, slot, &CANCEL_MARKER).await;
                }
            }
            let _ = mailbox.burn_mailbox(&id).await;
        })
        .await;
        self.wipe();
    }

    fn check_alive(&mut self, now: i64) -> SyncResult<()> {
        if now >= self.wiped_at() {
            self.wipe();
            return Err(expired());
        }
        Ok(())
    }

    fn wipe(&mut self) {
        self.secret = None;
        self.transcript = None;
        self.received = None;
    }

    /// Takes the secret out, derives the transcript (the secret is dropped and zeroized here)
    /// → Compare.
    fn enter_compare(&mut self) -> SyncResult<Stage> {
        let peer = self
            .peer_public
            .ok_or_else(|| internal("no peer public key"))?;
        let secret = self.secret.take().ok_or_else(|| internal("no secret"))?;
        let transcript = derive(&secret, self.side, &peer, &self.mailbox)?;
        drop(secret);
        self.comparison = Some(transcript.code());
        self.transcript = Some(transcript);
        self.stage = Stage::Compare;
        Ok(self.stage)
    }

    fn enter_sent(&mut self) -> SyncResult<Stage> {
        self.written[1] = true;
        self.transcript = None;
        self.stage = Stage::Sent;
        Ok(self.stage)
    }

    /// A mailbox answer as the error this session reports. A gone mailbox before Compare means
    /// the code is too old; from Compare on, the other device gave up. A slot that was read or
    /// written by someone else ends the pairing.
    fn relay_error(&self, error: RelayError) -> SyncError {
        match error {
            RelayError::MailboxGone => match self.stage {
                Stage::Announcing | Stage::WaitingForScan => expired(),
                _ => cancelled(),
            },
            RelayError::SlotConsumed | RelayError::SlotTaken => failed(),
            other => SyncError::from(other),
        }
    }
}

impl fmt::Debug for PairingSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingSession")
            .field("side", &self.side)
            .field("role", &self.role)
            .field("stage", &self.stage)
            .field("contested", &self.contested)
            .finish_non_exhaustive()
    }
}

/// Whether the pairing may go on after `error`: a rate limit or a momentary relay failure.
/// Everything else ends it.
pub fn keeps_session(error: &SyncError) -> bool {
    matches!(error, SyncError::RateLimited | SyncError::Transient(_))
}
