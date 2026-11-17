//! The fake's pairing mailbox (Moony-relay `src/mailbox.ts`): three slots, each written once and
//! read once. Slots 1 and 2 are the key exchange; slot 3 is the shower's acknowledgement.

use std::future::Future;

use super::{is_base64url, FakeRelay, Route};
use crate::services::sync::relay::{Mailbox, RelayError, RelayResult, MAX_SLOT_BYTES};

/// A pairing mailbox the relay has stored something for (`src/mailbox.ts`).
#[derive(Default)]
pub(super) struct PairingBox {
    /// Burnt or expired: every call answers `MailboxGone`.
    pub(super) gone: bool,
    pub(super) slots: [Option<PairingSlot>; 3],
}

pub(super) struct PairingSlot {
    /// Emptied once read.
    pub(super) data: Vec<u8>,
    pub(super) consumed: bool,
}

/// The argument checks of `HttpRelay` before a mailbox call; the slot as an index.
pub(super) fn check_mailbox(mailbox: &str, slot: Option<u8>) -> RelayResult<usize> {
    if !is_base64url(mailbox, 22) {
        return Err(RelayError::BadRequest("bad_mailbox".into()));
    }
    match slot {
        None => Ok(0),
        Some(slot @ (1..=3)) => Ok(usize::from(slot) - 1),
        Some(_) => Err(RelayError::BadRequest("bad_slot".into())),
    }
}

impl FakeRelay {
    /// What slot `slot` of the mailbox holds, without consuming it.
    pub fn peek_slot(&self, mailbox: &str, slot: u8) -> Option<Vec<u8>> {
        let state = self.lock();
        let stored = state.mailboxes.get(mailbox)?;
        let slot = stored
            .slots
            .get(usize::from(slot).checked_sub(1)?)?
            .as_ref()?;
        (!slot.consumed).then(|| slot.data.clone())
    }

    /// An attacker's write to a slot: the relay takes no credentials, so it follows the same
    /// rules as `put_slot` (write once, burnt boxes refuse) but never fails by switch.
    pub fn write_slot_raw(&self, mailbox: &str, slot: u8, bytes: Vec<u8>) {
        let mut state = self.lock();
        let index = check_mailbox(mailbox, Some(slot)).expect("a valid mailbox and slot");
        let stored = state.mailboxes.entry(mailbox.to_string()).or_default();
        if !stored.gone && stored.slots[index].is_none() {
            stored.slots[index] = Some(PairingSlot {
                data: bytes,
                consumed: false,
            });
        }
    }

    /// As if the mailbox's 10 minutes were over: later calls answer `MailboxGone`. A mailbox
    /// nobody wrote to stays unknown.
    pub fn expire_mailbox(&self, mailbox: &str) {
        if let Some(stored) = self.lock().mailboxes.get_mut(mailbox) {
            stored.gone = true;
            stored.slots = [None, None, None];
        }
    }

    /// The next take of a written slot erases it, then answers `Network("timeout")`.
    pub fn lose_next_take_answer(&self) {
        self.lock().lose_take_answer = true;
    }

    /// The next put that succeeds stores the slot, then answers `Network("timeout")`.
    pub fn lose_next_put_answer(&self) {
        self.lock().lose_put_answer = true;
    }

    /// The next call of `route` that succeeds keeps its effect, then answers `Server(503)`: the
    /// answer was lost on the way back. Only `Route::MailboxPut` and `Route::RotateToken` have an
    /// effect to keep here.
    pub fn lose_next_answer(&self, route: Route) {
        self.lock().lose_answers.push(route);
    }

    /// From now on this fake is a sync server from before slot 3: it answers every put or take
    /// of slot 3 with 400 `bad_slot`, as the old relay's route check does.
    pub fn act_without_slot_3(&self) {
        self.lock().without_slot_3 = true;
    }
}

/// The old relay's answer to a slot it does not know.
fn refuses_slot(without_slot_3: bool, slot: u8) -> RelayResult<()> {
    if without_slot_3 && slot == 3 {
        return Err(RelayError::BadRequest("bad_slot".into()));
    }
    Ok(())
}

impl Mailbox for FakeRelay {
    fn put_slot(
        &self,
        mailbox: &str,
        slot: u8,
        bytes: &[u8],
    ) -> impl Future<Output = RelayResult<()>> + Send {
        let result = (|| {
            let index = check_mailbox(mailbox, Some(slot))?;
            if bytes.is_empty() {
                return Err(RelayError::BadRequest("empty_body".into()));
            }
            if bytes.len() > MAX_SLOT_BYTES {
                return Err(RelayError::BadRequest("body_too_large".into()));
            }
            let mut state = self.lock();
            state.enter(Route::MailboxPut)?;
            refuses_slot(state.without_slot_3, slot)?;
            let lose_answer = std::mem::take(&mut state.lose_put_answer);
            let lose_503 = state
                .lose_answers
                .iter()
                .position(|r| *r == Route::MailboxPut)
                .map(|at| state.lose_answers.remove(at))
                .is_some();
            // The first write opens the mailbox.
            let stored = state.mailboxes.entry(mailbox.to_string()).or_default();
            if stored.gone {
                return Err(RelayError::MailboxGone);
            }
            if stored.slots[index].is_some() {
                return Err(RelayError::SlotTaken);
            }
            stored.slots[index] = Some(PairingSlot {
                data: bytes.to_vec(),
                consumed: false,
            });
            if lose_answer {
                return Err(RelayError::Network("timeout".into()));
            }
            if lose_503 {
                return Err(RelayError::Server(503));
            }
            Ok(())
        })();
        async move { result }
    }

    fn take_slot(
        &self,
        mailbox: &str,
        slot: u8,
    ) -> impl Future<Output = RelayResult<Option<Vec<u8>>>> + Send {
        let result = (|| {
            let index = check_mailbox(mailbox, Some(slot))?;
            let mut state = self.lock();
            state.enter(Route::MailboxTake)?;
            refuses_slot(state.without_slot_3, slot)?;
            let lose_answer = std::mem::take(&mut state.lose_take_answer);
            let Some(stored) = state.mailboxes.get_mut(mailbox) else {
                return Ok(None);
            };
            if stored.gone {
                return Err(RelayError::MailboxGone);
            }
            let Some(slot) = stored.slots[index].as_mut() else {
                return Ok(None);
            };
            if slot.consumed {
                return Err(RelayError::SlotConsumed);
            }
            slot.consumed = true;
            let data = std::mem::take(&mut slot.data);
            if lose_answer {
                return Err(RelayError::Network("timeout".into()));
            }
            Ok(Some(data))
        })();
        async move { result }
    }

    fn burn_mailbox(&self, mailbox: &str) -> impl Future<Output = RelayResult<()>> + Send {
        let result = (|| {
            check_mailbox(mailbox, None)?;
            let mut state = self.lock();
            state.enter(Route::MailboxBurn)?;
            if let Some(stored) = state.mailboxes.get_mut(mailbox) {
                stored.gone = true;
                stored.slots = [None, None, None];
            }
            Ok(())
        })();
        async move { result }
    }
}
