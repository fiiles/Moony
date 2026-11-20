//! Audit findings M1, M2, L7 and L8 against the pairing session. Task 13 of the audit-fix plan
//! (protocol 2: the shower's acknowledgement in slot 3, the holder types the joiner's digits)
//! turned each proof of concept into a test of the fixed behaviour, except L7, an accepted
//! residual (ADR 0027). From protocol 4 the shower's slot 3 is its reveal (its key and the
//! acknowledgement tag); the tests keep their meaning in that form.

use std::future::Future;

use x25519_dalek::StaticSecret;

use super::tests::{
    assert_same_creds, creds, id, key_of, pair, public, to_compare, HOME, M_SECRET, S_SECRET, T0,
};
use super::*;
use crate::services::sync::pairing::{
    derive, seal_credentials, slot1, CANCEL_MARKER, REVEAL_SLOT_LEN, REVEAL_VERSION,
};
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::{RelayError, RelayResult};

/// Slot 2 holds nothing sealed: it is empty, or it holds the cancel marker.
fn nothing_sealed(relay: &FakeRelay, session: &PairingSession) -> bool {
    relay
        .peek_slot(&id(session), 2)
        .is_none_or(|bytes| bytes == CANCEL_MARKER)
}

/// The comparison code of the exchange between S and the impostor M.
fn impostors_code(session: &PairingSession) -> u32 {
    derive(
        &StaticSecret::from(M_SECRET),
        Side::Scanner,
        &public(S_SECRET),
        &session.mailbox,
    )
    .unwrap()
    .code()
}

// Audit M1 (an impostor answers the QR first; the holder confirms alone).
#[tokio::test]
async fn an_impostor_who_answers_first_gets_nothing_without_the_joiners_digits() {
    let relay = FakeRelay::new();
    let mut s =
        PairingSession::show_with_secret(Role::Holder, HOME, T0, StaticSecret::from(S_SECRET))
            .unwrap();
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(
        relay.peek_slot(&id(&s), 3).is_some(),
        "the holder acknowledged the impostor's key"
    );
    assert_eq!(s.comparison_code(), None, "the holder shows no digits");
    assert_eq!(s.attempts_left(), MAX_TYPED_ATTEMPTS);

    // No digits, or text that is not six digits, is answered apart and spends no attempt.
    for typed in [None, Some(""), Some("12345"), Some("12a456")] {
        let error = s
            .confirm(&relay, Some(&creds()), typed, T0)
            .await
            .unwrap_err();
        assert_eq!(key_of(&error), CODE_INCOMPLETE, "{typed:?}");
        assert!(keeps_session(&error));
        assert_eq!(s.attempts_left(), MAX_TYPED_ATTEMPTS);
    }

    // Three wrong six-digit codes: the third ends the pairing.
    let impostors = impostors_code(&s);
    for n in 1..=MAX_TYPED_ATTEMPTS {
        let typed = format!("{:06}", (impostors + u32::from(n)) % 1_000_000);
        let error = s
            .confirm(&relay, Some(&creds()), Some(&typed), T0)
            .await
            .unwrap_err();
        let last = n == MAX_TYPED_ATTEMPTS;
        let expected = if last {
            CODE_MISMATCH_FINAL
        } else {
            CODE_MISMATCH
        };
        assert_eq!(key_of(&error), expected);
        assert_eq!(keeps_session(&error), !last);
        assert_eq!(s.attempts_left(), MAX_TYPED_ATTEMPTS - n);
        assert!(nothing_sealed(&relay, &s));
    }
    assert_eq!(
        relay.peek_slot(&id(&s), 2),
        Some(CANCEL_MARKER.to_vec()),
        "the ended pairing closes slot 2"
    );

    // Afterwards even the right digits are refused, and nothing is sent.
    let calls = relay.calls().len();
    let typed = format!("{impostors:06}");
    let error = s
        .confirm(&relay, Some(&creds()), Some(&typed), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), CODE_MISMATCH_FINAL);
    assert_eq!(relay.calls().len(), calls);
    assert_ne!(s.stage(), Stage::Sent);
}

#[tokio::test]
async fn the_holder_seals_only_after_the_joiners_digits_are_typed() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(c.comparison_code(), None);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(s.comparison_code(), None);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let digits = c.comparison_code().unwrap();
    assert_eq!(digits.len(), 6);
    assert!(digits.bytes().all(|b| b.is_ascii_digit()));
    assert_eq!(relay.peek_slot(&id(&s), 2), None);

    let sent = creds();
    let typed = format!("{} {}", &digits[..3], &digits[3..]);
    assert_eq!(
        s.confirm(&relay, Some(&sent), Some(&typed), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert_eq!(
        c.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_same_creds(c.received().unwrap(), &sent);
}

#[tokio::test]
async fn the_scanners_code_is_not_readable_before_the_ack() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(
        !c.has_transcript(),
        "the scanner derives nothing at its announce"
    );
    assert!(c.has_secret());
    assert_eq!(c.comparison_code(), None);
    // Polling before the shower acknowledged reads nothing and shows nothing.
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(c.comparison_code(), None);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(
        c.comparison_code(),
        None,
        "not before its own read of slot 3"
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());
}

/// Hands the shower the impostor's slot 1 in place of the scanner's (the relay withholds it).
struct SwapsSlot1 {
    relay: FakeRelay,
    record: Vec<u8>,
}

impl Mailbox for SwapsSlot1 {
    fn put_slot(
        &self,
        mailbox: &str,
        slot: u8,
        bytes: &[u8],
    ) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.put_slot(mailbox, slot, bytes)
    }

    fn take_slot(
        &self,
        mailbox: &str,
        slot: u8,
    ) -> impl Future<Output = RelayResult<Option<Vec<u8>>>> + Send {
        let taken = self.relay.take_slot(mailbox, slot);
        let swapped = (slot == 1).then(|| self.record.clone());
        async move {
            Ok(match (taken.await?, swapped) {
                (Some(_), Some(record)) => Some(record),
                (taken, _) => taken,
            })
        }
    }

    fn burn_mailbox(&self, mailbox: &str) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.burn_mailbox(mailbox)
    }
}

// Audit M2 (a relay that sees the QR and the phone's digits grinds a matching key): the digits
// are never on a screen before the acknowledgement, which a substituted key cannot produce.
#[tokio::test]
async fn a_scanner_shows_no_digits_before_a_valid_ack() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let swapping = SwapsSlot1 {
        relay: relay.clone(),
        record: slot1(Role::Joiner, &public(M_SECRET)).to_vec(),
    };
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(c.comparison_code(), None);
    assert_eq!(s.poll(&swapping, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.comparison_code(), None);
    assert_eq!(s.comparison_code(), None);

    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert!(!keeps_session(&error));
    assert_eq!(c.comparison_code(), None);
    assert_eq!(
        relay.peek_slot(&id(&s), 2),
        Some(CANCEL_MARKER.to_vec()),
        "the scanner closes slot 2"
    );
}

// Audit L8 (a lost announce answer, then an impostor's slot 1, taken for our own write).
#[tokio::test]
async fn an_impostor_after_a_lost_announce_answer_is_caught_by_the_ack() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxPut, RelayError::Server(503));
    assert!(c.announce(&relay, T0).await.is_err());
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(!c.contested(), "the impostor's record is taken for ours");
    assert_eq!(c.comparison_code(), None);

    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert!(!keeps_session(&error));
    assert_eq!(c.comparison_code(), None);
    assert!(nothing_sealed(&relay, &s));
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
}

// Audit L7, accepted residual (ADR 0027): an impostor that fills slot 1 and slot 2 before the
// real holder scans can still steer a joiner whose person confirms despite the holder's warning.
#[tokio::test]
async fn residual_l7_a_joiner_who_confirms_anyway_can_join_an_impostor() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Holder);
    let t = derive(
        &StaticSecret::from(M_SECRET),
        Side::Scanner,
        &public(S_SECRET),
        &s.mailbox,
    )
    .unwrap();
    let mut theirs = creds();
    theirs.relay_url = "https://evil.example".into();
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Holder, &public(M_SECRET)).to_vec());
    relay.write_slot_raw(&id(&s), 2, seal_credentials(&t, &theirs).unwrap());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(s.comparison_code(), Some(t.code_text()));
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    assert_eq!(c.comparison_code(), None);
    assert_ne!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
    s.confirm(&relay, None, None, T0).await.unwrap();
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_eq!(s.received().unwrap().relay_url, "https://evil.example");
}

#[tokio::test]
async fn a_lost_ack_answer_still_reaches_compare() {
    // The shower retries its acknowledgement and finds its own stored write.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.lose_next_answer(Route::MailboxPut);
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert!(keeps_session(&error));
    assert_eq!(s.stage(), Stage::Acknowledging);
    assert!(relay.peek_slot(&id(&s), 3).is_some(), "the relay stored it");
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());

    // The scanner read it before the retry: the retry's "taken" is still ours.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Holder);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.lose_next_answer(Route::MailboxPut);
    assert!(keeps_session(&s.poll(&relay, T0).await.unwrap_err()));
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let digits = s.comparison_code().unwrap();
    assert_eq!(
        c.confirm(&relay, Some(&creds()), Some(&digits), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
}

#[tokio::test]
async fn a_lost_ack_read_ends_the_pairing_clearly() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    relay.lose_next_take_answer();
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert!(keeps_session(&error));
    assert_eq!(c.stage(), Stage::AwaitingAck);

    // Our own lost read consumed it: nobody interfered, so no security alarm.
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert!(!keeps_session(&error));
    assert_eq!(c.comparison_code(), None);
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
}

#[tokio::test]
async fn someone_elses_slot_3_ends_the_shower_as_interfered() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Holder);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.write_slot_raw(&id(&s), 3, vec![REVEAL_VERSION; REVEAL_SLOT_LEN]);

    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert!(!keeps_session(&error));
    assert_ne!(s.stage(), Stage::Compare);
    assert_eq!(s.comparison_code(), None);
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));

    // The scanner reads the forged reveal and ends too.
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert_eq!(c.comparison_code(), None);
}

#[tokio::test]
async fn a_sync_server_without_slot_3_says_it_needs_an_update_on_both_sides() {
    let relay = FakeRelay::new();
    relay.act_without_slot_3();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);

    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), SERVER_NEEDS_UPDATE);
    assert!(!keeps_session(&error));
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), SERVER_NEEDS_UPDATE);
    assert!(!keeps_session(&error));
    assert_eq!(s.comparison_code(), None);
    assert_eq!(c.comparison_code(), None);
}

#[tokio::test]
async fn a_contested_scanner_shows_no_digits_and_only_cancels() {
    // The shower acknowledged the impostor first.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    assert!(!c.has_secret());
    assert_eq!(c.comparison_code(), None);

    let calls = relay.calls().len();
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(
        relay.calls().len(),
        calls,
        "a contested scanner ignores slot 3"
    );
    assert!(
        relay.peek_slot(&id(&s), 3).is_some(),
        "the ack is left unread"
    );
    let error = c.confirm(&relay, None, None, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCodeUsed");
    assert_eq!(c.comparison_code(), None);
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
    c.abandon(&relay).await;
    assert_eq!(
        relay.take_slot(&id(&s), 3).await,
        Err(RelayError::MailboxGone)
    );

    // The contested scanner read the impostor's slot 1 before the shower did: the shower finds it
    // read by someone else and ends; slots 2 and 3 are closed too.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
    assert_eq!(relay.peek_slot(&id(&s), 3), Some(CANCEL_MARKER.to_vec()));
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert!(!keeps_session(&error));
    assert_ne!(s.stage(), Stage::Compare);
}

#[tokio::test]
async fn a_contested_scanners_marker_in_slot_3_ends_a_shower_that_has_not_acknowledged() {
    // The shower read the impostor's slot 1, but its acknowledgement's put never left; the
    // person's own phone, contested, closes slot 3 meanwhile.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    // The shower's take of slot 1 goes through; its put of slot 3 is never sent.
    relay.fail_next(Route::MailboxPut, RelayError::Network("connect".into()));
    assert!(keeps_session(&s.poll(&relay, T0).await.unwrap_err()));
    assert_eq!(s.stage(), Stage::Acknowledging);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert!(nothing_sealed(&relay, &s));
    assert_eq!(s.comparison_code(), None);
}

#[tokio::test]
async fn abandoning_marks_slot_3_too() {
    // The shower gives up while the scanner waits for its acknowledgement; the delete fails.
    let relay = FakeRelay::new();
    let (s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.fail_next(Route::MailboxBurn, RelayError::Network("timeout".into()));
    s.abandon(&relay).await;
    assert_eq!(relay.peek_slot(&id(&c), 2), Some(CANCEL_MARKER.to_vec()));
    assert_eq!(relay.peek_slot(&id(&c), 3), Some(CANCEL_MARKER.to_vec()));
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
    assert_eq!(c.comparison_code(), None);

    // The scanner gives up after its announce; the delete fails: slots 2 and 3 are closed.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Holder);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.fail_next(Route::MailboxBurn, RelayError::Network("timeout".into()));
    c.abandon(&relay).await;
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
    assert_eq!(relay.peek_slot(&id(&s), 3), Some(CANCEL_MARKER.to_vec()));
    // The scanner's marker in slot 3 is the contested scanner's marker too, so the shower cannot
    // tell them apart and reads it as someone else's answer.
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), INTERFERED);
    assert_ne!(s.stage(), Stage::Compare);
    assert_eq!(s.comparison_code(), None);
}

#[tokio::test]
async fn a_contested_scan_closes_slot_2_for_the_shower() {
    let m_pub = public(M_SECRET);

    // The shower holds the vault and the person wrongly confirms on it: nothing sealed is written.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &m_pub).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    // Even the impostor's own digits, typed on the holder, seal nothing.
    let impostors = derive(
        &StaticSecret::from(S_SECRET),
        Side::Shower,
        &m_pub,
        &s.mailbox,
    )
    .unwrap()
    .code_text();
    let error = s
        .confirm(&relay, Some(&creds()), Some(&impostors), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert_ne!(s.stage(), Stage::Sent);
    assert_eq!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));

    // The shower wants to join: it reads "cancelled" instead of anything sealed.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Holder);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Holder, &m_pub).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    s.confirm(&relay, None, None, T0).await.unwrap();
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
    assert!(s.received().is_none());
}

#[tokio::test]
async fn a_scanner_that_met_an_old_sync_server_leaves_the_mailbox_alone() {
    // Its burn could reach the shower before its next read and read as "too old".
    let relay = FakeRelay::new();
    relay.act_without_slot_3();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), SERVER_NEEDS_UPDATE);
    c.abandon(&relay).await;
    assert_eq!(relay.peek_slot(&id(&s), 2), None, "no cancel marker either");
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), SERVER_NEEDS_UPDATE);
}

#[tokio::test]
async fn a_kept_holder_session_still_works_after_a_wrong_code() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let digits = to_compare(&relay, &mut s, &mut c).await;
    let wrong = if digits == "000000" {
        "000001"
    } else {
        "000000"
    };
    let error = s
        .confirm(&relay, Some(&creds()), Some(wrong), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), CODE_MISMATCH);
    assert!(keeps_session(&error));
    assert_eq!(s.attempts_left(), 2);
    assert_eq!(
        s.confirm(&relay, Some(&creds()), Some(&digits), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert_eq!(s.attempts_left(), 2);
}
