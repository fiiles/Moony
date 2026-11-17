//! The pairing session (phase 3, spec §20, ADR 0026; protocol 2 after audit M1, M2, L8): the
//! state machine that runs the bytes of `pairing` over the relay's three-slot mailbox. The shower
//! (S) puts a QR code on its screen; the scanner (C) reads it, writes slot 1 (its role and public
//! key) and derives the transcript. The shower reads slot 1, derives the same transcript and
//! writes its acknowledgement into slot 3; the scanner reads slot 3 and goes on only if it
//! verifies. Then the joiner shows its six digits, the holder's person types them, and the holder
//! seals the credentials into slot 2 for the joiner to open.
//!
//! The security rules this file keeps:
//! - Only the joiner ever shows digits, and a scanner shows them only after the acknowledgement
//!   verified: a relay that withheld the scanner's key cannot grind a matching one, because the
//!   digits are not on a screen before the shower proved it used the scanner's own key (M2).
//! - The holder seals slot 2 only after its person typed the joiner's digits (`code_matches`);
//!   `MAX_TYPED_ATTEMPTS` wrong codes end the pairing (M1). Typed text that is not six digits
//!   spends no attempt: it can never match.
//! - Any other bytes in slot 3, someone else's slot 3, or an acknowledgement read by someone else
//!   ends the pairing as interfered, with cancel markers (L8).
//! - The joiner reads slot 2 only after its own confirm. A scanner whose slot 1 was taken by
//!   someone else (contested) shows no digits, never reads slot 3, can never confirm, and writes
//!   cancel markers into slots 2 and 3 (best effort, and only where the impostor has not written
//!   first), so a shower that has not acknowledged yet ends as interfered and a holder-shower
//!   fails its slot-2 write.
//! - The ephemeral X25519 secret is taken out of its `Option` when it is used and dropped (and
//!   zeroized) right after. The session key is dropped once slot 2 is sealed or opened.
//! - Received credentials live until `expires_at + RECEIVED_GRACE_SECS`; any call after that
//!   wipes them, and the holder's join ticket (protocol 3) with them.
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
    check_roles, derive, is_cancel_marker, open_payload, parse_pairing_code, parse_slot1,
    seal_payload, slot1, typed_code_digits, MailboxId, PairingCode, PairingIdentity, Role, Side,
    SlotPayload, Transcript, CANCEL_MARKER,
};
use crate::error::{AppError, Result};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::keys::{os_random, Credentials};
use crate::services::sync::relay::http::{is_default_relay, normalize_base_url, DEFAULT_RELAY_URL};
use crate::services::sync::relay::{Mailbox, RelayError};

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "session_audit_tests.rs"]
mod audit_tests;

/// How long a pairing lives on this device (the relay's mailbox lives as long from its first
/// write).
pub const PAIRING_LIFETIME_SECS: i64 = 600;
/// How long received credentials outlive the pairing, so that a join that failed for a moment
/// can be tried again.
pub const RECEIVED_GRACE_SECS: i64 = 300;
/// The most `abandon` (and an interfered pairing) spends on its cancel markers and the delete.
pub const ABANDON_BUDGET: Duration = Duration::from_secs(3);
/// Slot 3: the shower's acknowledgement (`Transcript::ack`).
pub const ACK_SLOT: u8 = 3;
/// Wrong six-digit codes the holder's person may type before the pairing ends.
pub const MAX_TYPED_ATTEMPTS: u8 = 3;

/// Slot 1: the scanner's reply. Slot 2: the holder's sealed credentials.
const SCANNER_SLOT: u8 = 1;
const HOLDER_SLOT: u8 = 2;
const SLOTS: [u8; 3] = [SCANNER_SLOT, HOLDER_SLOT, ACK_SLOT];

/// The typed code is six digits but not the other device's; attempts are left.
pub const CODE_MISMATCH: &str = "validation.syncPairingCodeMismatch";
/// The last attempt was wrong: the pairing ends.
pub const CODE_MISMATCH_FINAL: &str = "validation.syncPairingCodeMismatchFinal";
/// Nothing typed, or not six digits: it can never match, so it spends no attempt.
pub const CODE_INCOMPLETE: &str = "validation.syncPairingCodeIncomplete";
/// Slot 3 held something that is not this exchange's acknowledgement, or was written or read by
/// someone else.
pub const INTERFERED: &str = "validation.syncPairingInterfered";
/// The sync server answered `bad_slot` for slot 3: it predates protocol 2.
pub const SERVER_NEEDS_UPDATE: &str = "validation.syncServerNeedsUpdate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Scanner: slot 1 is not written yet (or its answer was lost).
    Announcing,
    /// Shower: waiting for a scanner's slot 1.
    WaitingForScan,
    /// Scanner: slot 1 written, waiting for the shower's acknowledgement in slot 3. A contested
    /// scanner stays here.
    AwaitingAck,
    /// Shower: slot 1 read, its acknowledgement not stored yet.
    Acknowledging,
    /// Both: the joiner shows its digits, the holder waits for its person to type them.
    Compare,
    /// Joiner, after its confirm: waiting for slot 2.
    WaitingForOther,
    /// Holder, after its confirm: slot 2 is written.
    Sent,
    /// Joiner: slot 2 opened; the credentials (and the holder's identity) are held until
    /// `wiped_at`.
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
    /// From derive (the scanner's announce, the shower's read of slot 1) until slot 2 is sealed
    /// (holder) or opened (joiner).
    transcript: Option<Transcript>,
    /// The joiner's comparison code, set on entering Compare (after the acknowledgement), and
    /// kept after the transcript is dropped. Never set on the holder.
    comparison: Option<u32>,
    stage: Stage,
    contested: bool,
    /// A put to slot 1 / 2 / 3 whose answer was lost (network or server error): the relay may
    /// have stored it, so a later `SlotTaken` is our own write.
    ambiguous_put: [bool; 3],
    /// Slots 1 / 2 / 3 this side is known to have written.
    written: [bool; 3],
    /// Scanner: a take of slot 3 whose answer was lost, so the relay may have consumed the
    /// acknowledgement for us; a later "read by someone else" is then our own read.
    ambiguous_ack_take: bool,
    /// The sync server answered `bad_slot` for slot 3. The other device meets the same answer on
    /// its own call, so this side leaves the mailbox alone: a burn here could reach the other
    /// device first and read as "the code is too old".
    server_needs_update: bool,
    /// Holder: wrong six-digit codes typed so far.
    wrong_codes: u8,
    /// The joiner's opened slot 2: the credentials, the key heads and the holder's identity, its
    /// join ticket included; wiped together.
    received: Option<SlotPayload>,
    expires_at: i64,
    /// Set by the first call at or past `wiped_at`; every later call is refused, whatever its
    /// `now`.
    expired: bool,
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

fn interfered() -> SyncError {
    pairing_error(INTERFERED)
}

fn server_needs_update() -> SyncError {
    pairing_error(SERVER_NEEDS_UPDATE)
}

/// The client never makes `bad_slot` for slot 3 itself, so on slot 3 it is the server's own
/// route check: a sync server from before protocol 2.
fn predates_slot_3(error: &RelayError) -> bool {
    matches!(error, RelayError::BadRequest(code) if code == "bad_slot")
}

/// The cancel marker into every slot of `SLOTS` not in `written`, best effort; the slots it
/// managed to write are added to `written`.
async fn put_cancel_markers<M: Mailbox>(mailbox: &M, id: &str, written: &mut [bool; 3]) {
    for (index, slot) in SLOTS.into_iter().enumerate() {
        if !written[index] && mailbox.put_slot(id, slot, &CANCEL_MARKER).await.is_ok() {
            written[index] = true;
        }
    }
}

fn slot_index(slot: u8) -> usize {
    usize::from(slot) - 1
}

fn internal(what: &str) -> SyncError {
    SyncError::Local(AppError::Internal(format!("sync pairing: {what}")))
}

/// A put whose answer was lost may have been stored: every network failure and every server
/// error, except a connection that was never made (`HttpRelay` reports it as `"connect"`), which
/// sent nothing. `"request"` stays ambiguous: reqwest also reports a reset or EOF after the
/// request was written that way, and a stored put taken for unsent would flag our own retry as
/// contested.
fn put_was_ambiguous(error: &RelayError) -> bool {
    match error {
        RelayError::Network(reason) => reason != "connect",
        RelayError::Server(_) => true,
        _ => false,
    }
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
    /// or the default. The scanner's first call is `announce`. The code is parsed before any
    /// secret is made.
    pub fn scan(role: Role, code_text: &str, now: i64) -> Result<Self> {
        let code = parse_pairing_code(code_text)?;
        Self::scanner_of(role, code, now, random_secret()?)
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

    /// A low-order shower key in the code is refused here, before anything is written.
    fn scanner_of(role: Role, code: PairingCode, now: i64, secret: StaticSecret) -> Result<Self> {
        if !secret
            .diffie_hellman(&PublicKey::from(code.shower_public))
            .was_contributory()
        {
            return Err(AppError::Validation("validation.syncPairingFailed".into()));
        }
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
            ambiguous_put: [false; 3],
            written: [false; 3],
            ambiguous_ack_take: false,
            server_needs_update: false,
            wrong_codes: 0,
            received: None,
            expires_at: now + PAIRING_LIFETIME_SECS,
            expired: false,
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
        let code = parse_pairing_code(code_text)?;
        Self::scanner_of(role, code, now, secret)
    }

    #[cfg(test)]
    fn has_secret(&self) -> bool {
        self.secret.is_some()
    }

    #[cfg(test)]
    fn has_transcript(&self) -> bool {
        self.transcript.is_some()
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

    /// The joiner's six digits from Compare on; always `None` on the holder and on a contested
    /// scanner. A scanner derives its transcript at its announce, but its digits stay hidden
    /// until the shower's acknowledgement verified.
    pub fn comparison_code(&self) -> Option<String> {
        self.comparison.map(|code| format!("{code:06}"))
    }

    /// Holder: how many wrong six-digit codes its person may still type.
    pub fn attempts_left(&self) -> u8 {
        MAX_TYPED_ATTEMPTS.saturating_sub(self.wrong_codes)
    }

    /// Scanner: someone else wrote slot 1 first; this device's code differs from the shower's.
    pub fn contested(&self) -> bool {
        self.contested
    }

    pub fn received(&self) -> Option<&Credentials> {
        self.received.as_ref().map(|payload| &payload.creds)
    }

    /// The holder's identity received with the credentials (protocol 3), `None` from a holder
    /// whose vault has no certificates.
    pub fn received_identity(&self) -> Option<&PairingIdentity> {
        self.received
            .as_ref()
            .and_then(|payload| payload.identity.as_ref())
    }

    /// The rotation head of each key epoch the holder knew one for.
    pub fn received_heads(&self) -> Option<&std::collections::BTreeMap<u32, u64>> {
        self.received.as_ref().map(|payload| &payload.heads)
    }

    /// When every call starts answering `syncPairingExpired`: `expires_at`, or, once the
    /// credentials are received, `expires_at + RECEIVED_GRACE_SECS`.
    pub fn wiped_at(&self) -> i64 {
        match self.stage {
            Stage::Received => self.expires_at + RECEIVED_GRACE_SECS,
            _ => self.expires_at,
        }
    }

    /// Scanner, in Announcing: writes slot 1 and derives the transcript → AwaitingAck (no digits
    /// yet). Any other stage is returned untouched.
    pub async fn announce<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<Stage> {
        self.check_alive(now)?;
        if self.stage != Stage::Announcing {
            return Ok(self.stage);
        }
        let id = self.mailbox.to_b64();
        let record = slot1(self.role, &self.own_public);
        match mailbox.put_slot(&id, SCANNER_SLOT, &record).await {
            Ok(()) => self.enter_awaiting_ack(),
            // Our own earlier write, whose answer was lost. If it was not ours after all, the
            // shower acknowledges someone else's key and the acknowledgement does not verify.
            Err(RelayError::SlotTaken) if self.ambiguous_put[0] => self.enter_awaiting_ack(),
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
    /// anything else (an impostor's record, or nothing left to read) leaves this device in
    /// AwaitingAck, contested: no digits, no read of slot 3, only a cancel.
    async fn contest<M: Mailbox>(&mut self, mailbox: &M, id: &str) -> SyncResult<Stage> {
        match mailbox.take_slot(id, SCANNER_SLOT).await {
            Ok(Some(bytes)) if is_cancel_marker(&bytes) => Err(cancelled()),
            Ok(_) | Err(RelayError::SlotConsumed) => {
                self.contested = true;
                self.secret = None;
                self.close_slots(mailbox, id).await;
                self.stage = Stage::AwaitingAck;
                Ok(self.stage)
            }
            Err(error) => Err(self.relay_error(error)),
        }
    }

    /// A contested scan: the shower is paired with someone else. A cancel marker in slot 2 (once,
    /// best effort) means that a holder-shower fails its own slot-2 write and a joiner-shower
    /// reads "cancelled" instead of anything sealed; one in slot 3 ends a shower that has not
    /// acknowledged yet as interfered.
    async fn close_slots<M: Mailbox>(&mut self, mailbox: &M, id: &str) {
        for slot in [HOLDER_SLOT, ACK_SLOT] {
            if mailbox.put_slot(id, slot, &CANCEL_MARKER).await.is_ok() {
                self.written[slot_index(slot)] = true;
            }
        }
    }

    /// One step of waiting: the scanner retries its announce or reads slot 3, the shower reads
    /// slot 1 or retries its acknowledgement, the confirmed joiner reads slot 2. Every other
    /// stage, and a contested scanner, is returned untouched and sends nothing.
    pub async fn poll<M: Mailbox>(&mut self, mailbox: &M, now: i64) -> SyncResult<Stage> {
        self.check_alive(now)?;
        match self.stage {
            Stage::Announcing => self.announce(mailbox, now).await,
            Stage::WaitingForScan => self.read_scan(mailbox).await,
            Stage::AwaitingAck if self.contested => Ok(self.stage),
            Stage::AwaitingAck => self.read_ack(mailbox).await,
            Stage::Acknowledging => self.acknowledge(mailbox).await,
            Stage::WaitingForOther => self.read_credentials(mailbox).await,
            stage @ (Stage::Compare | Stage::Sent | Stage::Received) => Ok(stage),
        }
    }

    /// Shower: slot 1 → derive → Acknowledging, then the acknowledgement in the same call.
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
        self.derive_transcript()?;
        self.stage = Stage::Acknowledging;
        self.acknowledge(mailbox).await
    }

    /// Shower: puts the acknowledgement into slot 3 → Compare. A slot taken after our own lost
    /// answer is ours; taken otherwise, someone else wrote it (interfered).
    async fn acknowledge<M: Mailbox>(&mut self, mailbox: &M) -> SyncResult<Stage> {
        let id = self.mailbox.to_b64();
        let ack = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?
            .ack();
        let ours = self.ambiguous_put[slot_index(ACK_SLOT)];
        match mailbox.put_slot(&id, ACK_SLOT, &ack).await {
            Ok(()) => self.enter_compare(),
            // Stored, and maybe read by the scanner meanwhile.
            Err(RelayError::SlotTaken | RelayError::SlotConsumed) if ours => self.enter_compare(),
            Err(RelayError::SlotTaken | RelayError::SlotConsumed) => {
                Err(self.interfere(mailbox, &id).await)
            }
            Err(error) if predates_slot_3(&error) => Err(self.needs_update()),
            Err(error) => {
                if put_was_ambiguous(&error) {
                    self.ambiguous_put[slot_index(ACK_SLOT)] = true;
                }
                Err(self.relay_error(error))
            }
        }
    }

    /// Scanner: reads slot 3. Nothing yet stays; the cancel marker is "cancelled"; this
    /// exchange's acknowledgement → Compare (the joiner's digits appear now); anything else, or
    /// a slot someone else read, is interfered.
    async fn read_ack<M: Mailbox>(&mut self, mailbox: &M) -> SyncResult<Stage> {
        let id = self.mailbox.to_b64();
        let bytes = match mailbox.take_slot(&id, ACK_SLOT).await {
            Ok(None) => return Ok(self.stage),
            Ok(Some(bytes)) => bytes,
            // Our own earlier take, whose answer was lost, consumed it: nobody else interfered.
            Err(RelayError::SlotConsumed) if self.ambiguous_ack_take => {
                self.close(mailbox, &id).await;
                return Err(failed());
            }
            Err(RelayError::SlotConsumed) => return Err(self.interfere(mailbox, &id).await),
            Err(error) if predates_slot_3(&error) => return Err(self.needs_update()),
            Err(error) => {
                if put_was_ambiguous(&error) {
                    self.ambiguous_ack_take = true;
                }
                return Err(self.relay_error(error));
            }
        };
        if is_cancel_marker(&bytes) {
            return Err(cancelled());
        }
        let verified = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?
            .verify_ack(&bytes);
        if !verified {
            return Err(self.interfere(mailbox, &id).await);
        }
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
        let payload = open_payload(transcript, &sealed)?;
        self.received = Some(payload);
        self.transcript = None;
        self.stage = Stage::Received;
        // Nothing is left to read; a mailbox that stays expires on its own.
        let _ = mailbox.burn_mailbox(&id).await;
        Ok(self.stage)
    }

    /// The person confirmed. From Compare: the holder checks `typed`, the digits shown on the
    /// joiner, and only then seals the credentials into slot 2 → Sent; the joiner starts waiting
    /// for them → WaitingForOther, sending nothing (`typed` is ignored). A contested scanner
    /// cannot confirm. After Compare a repeated confirm returns the stage and does nothing;
    /// before it, there is nothing to confirm (`syncPairingFailed`).
    ///
    /// The holder: no digits, or text that is not six digits, is `CODE_INCOMPLETE` (no attempt
    /// spent); a wrong code is `CODE_MISMATCH` (the session is kept) until the
    /// `MAX_TYPED_ATTEMPTS`th, which ends the pairing with `CODE_MISMATCH_FINAL` and writes the
    /// cancel markers.
    ///
    /// `holder` is what the holder seals (its credentials, key heads and identity), read once
    /// the typed text can be a code; the joiner passes `None`.
    pub async fn confirm_with<M: Mailbox>(
        &mut self,
        mailbox: &M,
        holder: Option<&SlotPayload>,
        typed: Option<&str>,
        now: i64,
    ) -> SyncResult<Stage> {
        self.check_alive(now)?;
        if self.contested {
            return Err(code_used());
        }
        match self.stage {
            Stage::Compare => {}
            Stage::WaitingForOther | Stage::Sent | Stage::Received => return Ok(self.stage),
            Stage::Announcing
            | Stage::WaitingForScan
            | Stage::AwaitingAck
            | Stage::Acknowledging => return Err(failed()),
        }
        match self.role {
            Role::Joiner => {
                self.stage = Stage::WaitingForOther;
                Ok(self.stage)
            }
            Role::Holder => {
                self.check_typed(mailbox, typed).await?;
                self.send(mailbox, holder).await
            }
        }
    }

    /// `confirm_with` for a holder without an identity, epoch 0's head 0: the tests' shorthand.
    #[cfg(test)]
    pub async fn confirm<M: Mailbox>(
        &mut self,
        mailbox: &M,
        holder_creds: Option<&Credentials>,
        typed: Option<&str>,
        now: i64,
    ) -> SyncResult<Stage> {
        let payload = holder_creds.map(|creds| SlotPayload {
            creds: creds.clone(),
            heads: std::collections::BTreeMap::from([(0, 0)]),
            identity: None,
        });
        self.confirm_with(mailbox, payload.as_ref(), typed, now)
            .await
    }

    /// Holder: whether `typed` is the joiner's code, with a limited number of wrong attempts.
    async fn check_typed<M: Mailbox>(
        &mut self,
        mailbox: &M,
        typed: Option<&str>,
    ) -> SyncResult<()> {
        if self.attempts_left() == 0 {
            return Err(pairing_error(CODE_MISMATCH_FINAL));
        }
        let Some(typed) = typed.filter(|typed| typed_code_digits(typed).is_some()) else {
            return Err(pairing_error(CODE_INCOMPLETE));
        };
        let matches = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?
            .code_matches(typed);
        if matches {
            return Ok(());
        }
        self.wrong_codes = self.wrong_codes.saturating_add(1);
        if self.attempts_left() > 0 {
            return Err(pairing_error(CODE_MISMATCH));
        }
        let id = self.mailbox.to_b64();
        let mut written = self.written;
        let _ = tokio::time::timeout(
            ABANDON_BUDGET,
            put_cancel_markers(mailbox, &id, &mut written),
        )
        .await;
        self.written = written;
        self.wipe();
        Err(pairing_error(CODE_MISMATCH_FINAL))
    }

    async fn send<M: Mailbox>(
        &mut self,
        mailbox: &M,
        holder: Option<&SlotPayload>,
    ) -> SyncResult<Stage> {
        let payload = holder.ok_or_else(|| internal("the holder has no credentials"))?;
        let transcript = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?;
        let sealed = seal_payload(transcript, payload)?;
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

    /// Gives the pairing up: within `ABANDON_BUDGET`, a cancel marker into each of slots 1–3
    /// this side has not written (so a waiting peer reads "cancelled" even if the delete fails),
    /// then the delete. Both best effort; the secret, the session key and the credentials are
    /// wiped.
    ///
    /// A holder in Sent is simply dropped, sending nothing: the joiner may not have read slot 2
    /// yet, and the mailbox expires on its own.
    pub async fn abandon<M: Mailbox>(mut self, mailbox: &M) {
        if self.stage == Stage::Sent || self.server_needs_update {
            self.wipe();
            return;
        }
        let id = self.mailbox.to_b64();
        let mut written = self.written;
        let _ = tokio::time::timeout(ABANDON_BUDGET, async {
            put_cancel_markers(mailbox, &id, &mut written).await;
            let _ = mailbox.burn_mailbox(&id).await;
        })
        .await;
        self.wipe();
    }

    /// Someone else wrote or read slot 3, or it held another exchange's acknowledgement: the
    /// cancel markers (within `ABANDON_BUDGET`, best effort), the secrets wiped, and the error
    /// that ends the pairing.
    async fn interfere<M: Mailbox>(&mut self, mailbox: &M, id: &str) -> SyncError {
        self.close(mailbox, id).await;
        interfered()
    }

    /// The cancel markers (within `ABANDON_BUDGET`, best effort) and the secrets wiped.
    async fn close<M: Mailbox>(&mut self, mailbox: &M, id: &str) {
        let mut written = self.written;
        let _ = tokio::time::timeout(
            ABANDON_BUDGET,
            put_cancel_markers(mailbox, id, &mut written),
        )
        .await;
        self.written = written;
        self.wipe();
    }

    /// The sync server predates slot 3: the error that ends the pairing, and no mailbox writes
    /// from this side afterwards.
    fn needs_update(&mut self) -> SyncError {
        self.server_needs_update = true;
        self.wipe();
        server_needs_update()
    }

    fn check_alive(&mut self, now: i64) -> SyncResult<()> {
        if self.expired || now >= self.wiped_at() {
            self.expired = true;
            self.wipe();
            return Err(expired());
        }
        Ok(())
    }

    fn wipe(&mut self) {
        self.secret = None;
        self.transcript = None;
        self.received = None;
        self.comparison = None;
    }

    /// Takes the secret out and derives the transcript (the secret is dropped and zeroized here).
    /// No digits are exposed yet.
    fn derive_transcript(&mut self) -> SyncResult<()> {
        let peer = self
            .peer_public
            .ok_or_else(|| internal("no peer public key"))?;
        let secret = self.secret.take().ok_or_else(|| internal("no secret"))?;
        let transcript = derive(&secret, self.side, &peer, &self.mailbox)?;
        drop(secret);
        self.transcript = Some(transcript);
        Ok(())
    }

    /// Scanner: slot 1 is ours → derive → AwaitingAck.
    fn enter_awaiting_ack(&mut self) -> SyncResult<Stage> {
        self.written[slot_index(SCANNER_SLOT)] = true;
        self.derive_transcript()?;
        self.stage = Stage::AwaitingAck;
        Ok(self.stage)
    }

    /// The acknowledgement is stored (shower) or verified (scanner) → Compare; a joiner's digits
    /// become readable here and only here.
    fn enter_compare(&mut self) -> SyncResult<Stage> {
        if self.side == Side::Shower {
            self.written[slot_index(ACK_SLOT)] = true;
        }
        let code = self
            .transcript
            .as_ref()
            .ok_or_else(|| internal("no transcript"))?
            .code();
        self.comparison = (self.role == Role::Joiner).then_some(code);
        self.stage = Stage::Compare;
        Ok(self.stage)
    }

    fn enter_sent(&mut self) -> SyncResult<Stage> {
        self.written[slot_index(HOLDER_SLOT)] = true;
        self.transcript = None;
        self.stage = Stage::Sent;
        Ok(self.stage)
    }

    /// A mailbox answer as the error this session reports. A gone mailbox before the scan is
    /// answered means the code is too old; after it, the other device gave up. A slot that was
    /// read or written by someone else ends the pairing.
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

/// Whether the pairing may go on after `error`: a rate limit, a momentary relay failure, or a
/// typed code that was wrong with attempts left or was not six digits. Everything else ends it.
pub fn keeps_session(error: &SyncError) -> bool {
    match error {
        SyncError::RateLimited | SyncError::Transient(_) => true,
        SyncError::Local(AppError::Validation(key)) => {
            key == CODE_MISMATCH || key == CODE_INCOMPLETE
        }
        _ => false,
    }
}
