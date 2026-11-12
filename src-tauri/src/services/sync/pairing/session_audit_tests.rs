//! Audit proofs of concept for the pairing session: each asserts today's faulty behaviour until
//! the task named beside it fixes the finding and flips it.

use x25519_dalek::StaticSecret;

use super::tests::{assert_same_creds, creds, id, pair, public, HOME, M_SECRET, S_SECRET, T0};
use super::*;
use crate::services::sync::pairing::{derive, slot1, CANCEL_MARKER};
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::RelayError;

// Audit M1 (impostor first scanner): flipped by the pairing-identity task.
#[tokio::test]
async fn poc_m1_impostor_first_scanner_gets_keys_on_lone_holder_confirm() {
    use crate::services::sync::pairing::open_credentials;
    let relay = FakeRelay::new();
    let mut s =
        PairingSession::show_with_secret(Role::Holder, HOME, T0, StaticSecret::from(S_SECRET))
            .unwrap();
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(
        s.confirm(&relay, Some(&creds()), T0).await.unwrap(),
        Stage::Sent
    );
    let t = derive(
        &StaticSecret::from(M_SECRET),
        Side::Scanner,
        &public(S_SECRET),
        &s.mailbox,
    )
    .unwrap();
    let sealed = relay.peek_slot(&id(&s), 2).unwrap();
    assert_same_creds(&open_credentials(&t, &sealed).unwrap(), &creds());
}

// Audit L7 (impostor fills slot 2 before the cancel marker): flipped by the pairing-hardening task.
#[tokio::test]
async fn poc_l7_impostor_holder_fills_slot_2_before_the_cancel_marker() {
    use crate::services::sync::pairing::seal_credentials;
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
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.contested());
    assert_ne!(relay.peek_slot(&id(&s), 2), Some(CANCEL_MARKER.to_vec()));
    s.confirm(&relay, None, T0).await.unwrap();
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_eq!(s.received().unwrap().relay_url, "https://evil.example");
}

// Audit L8 (impostor after a failed announce): flipped by the pairing-hardening task.
#[tokio::test]
async fn poc_l8_impostor_after_a_failed_announce_is_taken_for_our_own_write() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxPut, RelayError::Server(503));
    assert!(c.announce(&relay, T0).await.is_err());
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &public(M_SECRET)).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(!c.contested());
    assert_eq!(relay.peek_slot(&id(&s), 2), None);
    assert_ne!(s.comparison_code(), c.comparison_code());
}
