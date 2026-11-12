use std::future::Future;

use x25519_dalek::{PublicKey, StaticSecret};

use super::*;
use crate::error::AppError;
use crate::services::sync::error::Transient;
use crate::services::sync::keys::{VaultId, VaultKeys};
use crate::services::sync::pairing::{pairing_code_text, slot1, CANCEL_MARKER};
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
use crate::services::sync::relay::{RelayError, RelayResult};

pub(super) const T0: i64 = 1_800_000_000;
pub(super) const HOME: &str = "https://sync.home.example.com";
pub(super) const S_SECRET: [u8; 32] = [0x11; 32];
const C_SECRET: [u8; 32] = [0x22; 32];
pub(super) const M_SECRET: [u8; 32] = [0x44; 32];

pub(super) fn public(secret: [u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(secret)).to_bytes()
}

pub(super) fn key_of(error: &SyncError) -> String {
    match error.to_app_error() {
        AppError::Validation(key) => key,
        other => panic!("not a validation key: {other:?}"),
    }
}

pub(super) fn creds() -> Credentials {
    let keys = VaultKeys::from_parts(
        1,
        std::collections::BTreeMap::from([(0, [0xA0; 32]), (1, [0xA1; 32])]),
    )
    .unwrap();
    Credentials {
        relay_url: HOME.into(),
        account: "1234567890123456".into(),
        vault_id: VaultId([0x77; 16]),
        keys,
    }
}

pub(super) fn assert_same_creds(got: &Credentials, want: &Credentials) {
    assert_eq!(got.relay_url, want.relay_url);
    assert_eq!(got.account, want.account);
    assert_eq!(got.vault_id.0, want.vault_id.0);
    assert_eq!(got.keys.current_epoch(), want.keys.current_epoch());
    let epochs = |c: &Credentials| -> Vec<(u32, [u8; 32])> {
        c.keys.epochs().map(|(e, k)| (e, *k)).collect()
    };
    assert_eq!(epochs(got), epochs(want));
}

pub(super) fn id(session: &PairingSession) -> String {
    session.mailbox.to_b64()
}

/// S shows (fixed secret), C scans S's code (fixed secret).
pub(super) fn pair(shower: Role, scanner: Role) -> (PairingSession, PairingSession) {
    let s =
        PairingSession::show_with_secret(shower, HOME, T0, StaticSecret::from(S_SECRET)).unwrap();
    let c = PairingSession::scan_with_secret(
        scanner,
        &pairing_code_text(&s.code()),
        T0,
        StaticSecret::from(C_SECRET),
    )
    .unwrap();
    (s, c)
}

/// Drives both to Compare: C announces, S reads slot 1 and acknowledges, C reads the
/// acknowledgement. The joiner's digits, which the holder's person types (the holder shows none).
pub(super) async fn to_compare(
    relay: &FakeRelay,
    s: &mut PairingSession,
    c: &mut PairingSession,
) -> String {
    assert_eq!(c.announce(relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(s.poll(relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(relay, T0).await.unwrap(), Stage::Compare);
    let (holder, joiner) = if s.role() == Role::Holder {
        (&*s, &*c)
    } else {
        (&*c, &*s)
    };
    assert_eq!(holder.comparison_code(), None);
    joiner.comparison_code().expect("the joiner's digits")
}

fn puts(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .iter()
        .filter(|r| **r == Route::MailboxPut)
        .count()
}

#[tokio::test]
async fn a_joiner_scans_a_holders_code_and_receives_the_credentials() {
    let relay = FakeRelay::new();
    let mut s = PairingSession::show(Role::Holder, HOME, T0).unwrap();
    assert_eq!(s.side(), Side::Shower);
    assert_eq!(s.stage(), Stage::WaitingForScan);
    assert!(relay.calls().is_empty(), "showing sends nothing");
    let mut c = PairingSession::scan(Role::Joiner, &pairing_code_text(&s.code()), T0).unwrap();
    assert_eq!(c.side(), Side::Scanner);
    assert_eq!(c.role(), Role::Joiner);
    assert_eq!(c.stage(), Stage::Announcing);
    assert_eq!(c.comparison_code(), None);

    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert_eq!(c.comparison_code(), None);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(s.comparison_code(), None, "the holder shows no digits");
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let code = c.comparison_code().unwrap();
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|b| b.is_ascii_digit()));

    assert_eq!(
        c.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::WaitingForOther);
    let sent = creds();
    assert_eq!(
        s.confirm(&relay, Some(&sent), Some(&code), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_same_creds(c.received().unwrap(), &sent);
    assert!(s.received().is_none());
    assert_eq!(
        relay.put_slot(&id(&c), 1, b"x").await,
        Err(RelayError::MailboxGone),
        "the joiner burns the mailbox once it has the credentials"
    );
}

#[tokio::test]
async fn a_holder_scans_a_joiners_code_and_sends_the_credentials() {
    let relay = FakeRelay::new();
    let mut s = PairingSession::show(Role::Joiner, HOME, T0).unwrap();
    let mut c = PairingSession::scan(Role::Holder, &pairing_code_text(&s.code()), T0).unwrap();
    let code = to_compare(&relay, &mut s, &mut c).await;

    assert_eq!(
        s.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::WaitingForOther);
    let sent = creds();
    assert_eq!(
        c.confirm(&relay, Some(&sent), Some(&code), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_same_creds(s.received().unwrap(), &sent);
    assert_eq!(
        relay.put_slot(&id(&s), 2, b"x").await,
        Err(RelayError::MailboxGone)
    );
}

#[tokio::test]
async fn the_holder_writes_slot_2_only_after_its_own_confirm() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    assert_eq!(
        c.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::WaitingForOther);
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(relay.peek_slot(&id(&s), 2), None);

    // A holder confirm without credentials is a bug of the caller: nothing is written.
    let missing = s.confirm(&relay, None, Some(&code), T0).await.unwrap_err();
    assert!(matches!(missing, SyncError::Local(AppError::Internal(_))));
    assert_eq!(relay.peek_slot(&id(&s), 2), None);

    assert_eq!(
        s.confirm(&relay, Some(&creds()), Some(&code), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert!(relay.peek_slot(&id(&s), 2).is_some());
}

#[tokio::test]
async fn a_forged_slot_2_fails_and_nothing_is_joined() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    relay.write_slot_raw(&id(&s), 2, vec![0x01; 80]);

    let calls = relay.calls().len();
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(relay.calls().len(), calls, "no read before the confirm");

    assert_eq!(
        c.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert!(!keeps_session(&error));
    assert!(c.received().is_none());

    let error = s
        .confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert_ne!(s.stage(), Stage::Sent);
}

#[tokio::test]
async fn a_man_in_the_middle_who_writes_slot_1_first_is_caught() {
    let m_pub = public(M_SECRET);

    // S holds the vault; M answers S's code first as a joiner.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Joiner, &m_pub).to_vec());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(s.comparison_code(), None);

    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested());
    assert!(!s.contested());
    assert_eq!(c.comparison_code(), None);

    let error = c.confirm(&relay, None, None, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCodeUsed");
    assert_eq!(c.stage(), Stage::AwaitingAck);
    assert_eq!(
        relay.peek_slot(&id(&s), 2),
        Some(CANCEL_MARKER.to_vec()),
        "the contested scanner closes slot 2"
    );
    s.abandon(&relay).await;
    assert_eq!(
        relay.take_slot(&id(&c), 2).await,
        Err(RelayError::MailboxGone)
    );

    // Mirror: the scanner holds the vault; a contested holder never writes slot 2.
    let relay = FakeRelay::new();
    let (s, mut c) = pair(Role::Joiner, Role::Holder);
    relay.write_slot_raw(&id(&s), 1, slot1(Role::Holder, &m_pub).to_vec());
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested(), "an impostor's unread record");
    assert_eq!(c.comparison_code(), None);
    let error = c
        .confirm(&relay, Some(&creds()), Some("123456"), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCodeUsed");
    assert_eq!(
        relay.peek_slot(&id(&s), 2),
        Some(CANCEL_MARKER.to_vec()),
        "the contested scanner closes slot 2"
    );
}

#[tokio::test]
async fn a_scanner_whose_announce_failed_retries_on_poll() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxPut, RelayError::Server(503));
    let error = c.announce(&relay, T0).await.unwrap_err();
    assert!(keeps_session(&error));
    assert_eq!(c.stage(), Stage::Announcing);
    assert_eq!(relay.peek_slot(&id(&c), 1), None);

    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(!c.contested());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());
}

#[tokio::test]
async fn a_lost_slot_1_put_answer_still_reaches_compare() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.lose_next_put_answer();
    let error = c.announce(&relay, T0).await.unwrap_err();
    assert!(matches!(error, SyncError::Transient(Transient::Offline)));
    assert_eq!(c.stage(), Stage::Announcing);

    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(
        !c.contested(),
        "a slot taken after our own lost answer is ours"
    );
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());
}

#[tokio::test]
async fn a_lost_slot_2_put_answer_counts_as_sent() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();

    relay.lose_next_put_answer();
    let sent = creds();
    let error = s
        .confirm(&relay, Some(&sent), Some(&code), T0)
        .await
        .unwrap_err();
    assert!(keeps_session(&error));
    assert_eq!(s.stage(), Stage::Compare);
    assert_eq!(
        s.confirm(&relay, Some(&sent), Some(&code), T0)
            .await
            .unwrap(),
        Stage::Sent
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_same_creds(c.received().unwrap(), &sent);
}

#[tokio::test]
async fn abandoning_tells_a_waiting_peer_it_was_cancelled() {
    // The joiner waits on slot 2; the holder gives up and its delete fails.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();
    relay.fail_next(Route::MailboxBurn, RelayError::Network("timeout".into()));
    s.abandon(&relay).await;
    assert_eq!(relay.peek_slot(&id(&c), 2), Some(CANCEL_MARKER.to_vec()));
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
    assert!(!keeps_session(&error));

    // The shower gives up before anyone scanned, and its delete fails too.
    let relay = FakeRelay::new();
    let (s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxBurn, RelayError::Network("timeout".into()));
    s.abandon(&relay).await;
    let error = c.announce(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
    assert_eq!(c.stage(), Stage::Announcing);
    assert_eq!(c.comparison_code(), None);
}

/// A relay that never answers.
struct SilentRelay;

impl Mailbox for SilentRelay {
    fn put_slot(&self, _: &str, _: u8, _: &[u8]) -> impl Future<Output = RelayResult<()>> + Send {
        std::future::pending()
    }
    fn take_slot(
        &self,
        _: &str,
        _: u8,
    ) -> impl Future<Output = RelayResult<Option<Vec<u8>>>> + Send {
        std::future::pending()
    }
    fn burn_mailbox(&self, _: &str) -> impl Future<Output = RelayResult<()>> + Send {
        std::future::pending()
    }
}

#[tokio::test(start_paused = true)]
async fn abandoning_gives_up_after_its_budget() {
    let (s, _) = pair(Role::Holder, Role::Joiner);
    let started = tokio::time::Instant::now();
    s.abandon(&SilentRelay).await;
    assert_eq!(started.elapsed(), ABANDON_BUDGET);
}

#[tokio::test]
async fn the_secret_is_gone_after_derive() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert!(s.has_secret());
    assert!(c.has_secret());
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(!c.has_secret());
    assert!(s.has_secret());
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(!s.has_secret());
}

#[tokio::test]
async fn a_received_session_lives_through_the_grace_period() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();
    s.confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    assert_eq!(c.wiped_at(), c.expires_at());
    assert_eq!(c.poll(&relay, T0 + 10).await.unwrap(), Stage::Received);

    assert_eq!(c.expires_at(), T0 + PAIRING_LIFETIME_SECS);
    assert_eq!(c.wiped_at(), c.expires_at() + 300);
    assert_eq!(
        s.wiped_at(),
        s.expires_at(),
        "only a received session has grace"
    );
    let last = c.expires_at() + 299;
    assert_eq!(c.poll(&relay, last).await.unwrap(), Stage::Received);
    assert!(c.received().is_some());

    let end = c.expires_at() + 300;
    let error = c.poll(&relay, end).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");
    let error = c.confirm(&relay, None, None, end).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");
    assert!(
        c.received().is_none(),
        "the credentials are wiped at the end"
    );
}

#[tokio::test]
async fn equal_roles_are_refused_by_the_shower() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Holder);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingBothOn");
    assert!(!keeps_session(&error));
    s.abandon(&relay).await;
    // The scanner's read of slot 3 meets the burnt mailbox.
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");

    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Joiner, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingBothOff");
    s.abandon(&relay).await;
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
    assert_eq!(c.comparison_code(), None);
}

#[tokio::test]
async fn an_expired_pairing_is_refused() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let calls = relay.calls().len();
    let error = s.poll(&relay, s.expires_at()).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");
    let error = c.announce(&relay, c.expires_at()).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");
    assert_eq!(
        relay.calls().len(),
        calls,
        "an expired session sends nothing"
    );

    // The relay's ten minutes ran out first (its clock started with the first write).
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.lose_next_put_answer();
    c.announce(&relay, T0).await.unwrap_err();
    relay.expire_mailbox(&id(&c));
    let error = c.announce(&relay, T0 + 1).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");
    let error = s.poll(&relay, T0 + 1).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingExpired");

    // After Compare a gone mailbox means the other device gave up.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    relay.expire_mailbox(&id(&s));
    let error = s
        .confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingCancelled");
}

#[tokio::test]
async fn a_lost_read_ends_the_pairing_with_a_clear_error() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    relay.lose_next_take_answer();
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert!(matches!(error, SyncError::Transient(_)));
    assert!(keeps_session(&error));
    assert_eq!(s.stage(), Stage::WaitingForScan);
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert!(!keeps_session(&error));
}

#[tokio::test]
async fn transient_errors_keep_the_session() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    assert_eq!(c.announce(&relay, T0).await.unwrap(), Stage::AwaitingAck);

    relay.fail_next(Route::MailboxTake, RelayError::RateLimited);
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert!(matches!(error, SyncError::RateLimited));
    assert!(keeps_session(&error));
    assert_eq!(s.stage(), Stage::WaitingForScan);

    relay.fail_next(Route::MailboxTake, RelayError::Network("timeout".into()));
    let error = s.poll(&relay, T0).await.unwrap_err();
    assert!(keeps_session(&error));
    assert_eq!(s.stage(), Stage::WaitingForScan);

    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());

    assert!(!keeps_session(&SyncError::Refused(RelayError::SlotTaken)));
    assert!(!keeps_session(&SyncError::Local(AppError::Validation(
        "validation.syncPairingFailed".into()
    ))));
}

#[tokio::test]
async fn the_scanner_uses_the_sync_server_the_code_names() {
    let s = PairingSession::show(Role::Holder, HOME, T0).unwrap();
    assert_eq!(s.relay_url(), HOME);
    let c = PairingSession::scan(Role::Joiner, &pairing_code_text(&s.code()), T0).unwrap();
    assert_eq!(c.relay_url(), HOME);

    let s = PairingSession::show(Role::Holder, DEFAULT_RELAY_URL, T0).unwrap();
    let text = pairing_code_text(&s.code());
    assert_eq!(text.matches(':').count(), 3, "no fourth field: {text}");
    let c = PairingSession::scan(Role::Joiner, &text, T0).unwrap();
    assert_eq!(c.relay_url(), DEFAULT_RELAY_URL);

    // A spelling of the address that is not normalized still makes a valid code.
    let s = PairingSession::show(Role::Holder, "https://SYNC.home.example.com/", T0).unwrap();
    assert_eq!(s.relay_url(), HOME);
    let error = PairingSession::show(Role::Holder, "ftp://example.com", T0).unwrap_err();
    assert!(matches!(error, AppError::Validation(k) if k == "validation.syncRelayUrlInvalid"));
}

#[tokio::test]
async fn confirm_outside_compare_sends_nothing() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let error = s
        .confirm(&relay, Some(&creds()), Some("123456"), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert_eq!(s.stage(), Stage::WaitingForScan);
    let error = c.confirm(&relay, None, None, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert_eq!(c.stage(), Stage::Announcing);
    assert!(relay.calls().is_empty());

    // After Compare a repeated confirm returns the stage and does nothing.
    let code = to_compare(&relay, &mut s, &mut c).await;
    s.confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    c.confirm(&relay, None, None, T0).await.unwrap();
    let calls = relay.calls().len();
    let again = s
        .confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    assert_eq!(again, Stage::Sent);
    let again = c.confirm(&relay, None, None, T0).await.unwrap();
    assert_eq!(again, Stage::WaitingForOther);
    assert_eq!(
        relay.calls().len(),
        calls,
        "a repeated confirm sends nothing"
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
    let calls = relay.calls().len();
    assert_eq!(
        c.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::Received
    );
    assert_eq!(relay.calls().len(), calls);
}

#[test]
fn a_session_prints_nothing_secret() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let text = pairing_code_text(&s.code());
    let code = runtime.block_on(to_compare(&relay, &mut s, &mut c));
    let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
    let b64 = |b: &[u8]| -> String {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
    };
    for session in [&s, &c] {
        let printed = format!("{session:?}");
        for secret in [
            id(session),
            text.clone(),
            code.clone(),
            b64(&public(S_SECRET)),
            b64(&public(C_SECRET)),
            hex(&public(S_SECRET)),
            hex(&S_SECRET),
            hex(&C_SECRET),
            HOME.to_string(),
        ] {
            assert!(!printed.contains(&secret), "{printed}");
        }
        assert!(printed.contains("Compare"), "{printed}");
    }
}

// Fix round 1.

#[test]
fn only_a_lost_answer_makes_a_put_ambiguous() {
    let net = |r: &str| RelayError::Network(r.into());
    for (error, ambiguous) in [
        (net("timeout"), true),
        (net("body"), true),
        (net("other"), true),
        (net("request"), true),
        (RelayError::Server(502), true),
        (net("connect"), false),
        (RelayError::RateLimited, false),
        (RelayError::Protocol("redirect".into()), false),
    ] {
        assert_eq!(put_was_ambiguous(&error), ambiguous, "{error:?}");
    }
}

#[tokio::test]
async fn a_put_that_never_left_is_not_taken_for_our_own_write() {
    let relay = FakeRelay::new();
    let (_s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxPut, RelayError::Network("connect".into()));
    let error = c.announce(&relay, T0).await.unwrap_err();
    assert!(keeps_session(&error));
    let m_pub = public(M_SECRET);
    relay.write_slot_raw(&id(&c), 1, slot1(Role::Joiner, &m_pub).to_vec());
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(c.contested(), "the impostor's record is not ours");
}

#[tokio::test]
async fn a_lost_request_answer_still_reaches_compare_as_ours() {
    // reqwest reports a reset after the request was written as "request": the relay stored it.
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    relay.fail_next(Route::MailboxPut, RelayError::Network("request".into()));
    assert!(keeps_session(&c.announce(&relay, T0).await.unwrap_err()));
    relay.write_slot_raw(&id(&c), 1, slot1(Role::Joiner, &public(C_SECRET)).to_vec());
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::AwaitingAck);
    assert!(!c.contested());
    assert_eq!(relay.peek_slot(&id(&c), 2), None, "slot 2 is left open");
    assert_eq!(s.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert!(c.comparison_code().is_some());
}

#[tokio::test]
async fn abandoning_a_sent_holder_leaves_the_mailbox_to_the_joiner() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();
    s.confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    let calls = relay.calls().len();
    s.abandon(&relay).await;
    assert_eq!(
        relay.calls().len(),
        calls,
        "a Sent holder is simply dropped"
    );
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
}

#[tokio::test]
async fn expiry_is_sticky() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    s.poll(&relay, s.expires_at()).await.unwrap_err();
    c.announce(&relay, c.expires_at() + 5).await.unwrap_err();
    for error in [
        s.poll(&relay, T0).await.unwrap_err(),
        s.confirm(&relay, Some(&creds()), None, T0)
            .await
            .unwrap_err(),
        c.announce(&relay, T0).await.unwrap_err(),
        c.poll(&relay, T0).await.unwrap_err(),
    ] {
        assert_eq!(key_of(&error), "validation.syncPairingExpired");
    }
    assert!(relay.calls().is_empty(), "an expired session sends nothing");
}

#[tokio::test]
async fn a_slot_2_sealed_in_another_pairing_is_refused() {
    // Same device keys, another mailbox: a genuine blob of the first pairing.
    let first = FakeRelay::new();
    let (mut s1, mut c1) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&first, &mut s1, &mut c1).await;
    s1.confirm(&first, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    let blob = first.peek_slot(&id(&s1), 2).unwrap();

    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();
    relay.write_slot_raw(&id(&s), 2, blob);
    let error = c.poll(&relay, T0).await.unwrap_err();
    assert_eq!(key_of(&error), "validation.syncPairingFailed");
    assert!(c.received().is_none());
}

#[tokio::test]
async fn a_bad_scanner_key_keeps_the_shower_out_of_compare() {
    let mut malformed = slot1(Role::Joiner, &public(C_SECRET)).to_vec();
    malformed.pop();
    for record in [slot1(Role::Joiner, &[0u8; 32]).to_vec(), malformed] {
        let relay = FakeRelay::new();
        let (mut s, _c) = pair(Role::Holder, Role::Joiner);
        relay.write_slot_raw(&id(&s), 1, record);
        let error = s.poll(&relay, T0).await.unwrap_err();
        assert_eq!(key_of(&error), "validation.syncPairingFailed");
        assert_eq!(s.stage(), Stage::WaitingForScan);
        assert_eq!(s.comparison_code(), None);
        assert!(s
            .confirm(&relay, Some(&creds()), Some("123456"), T0)
            .await
            .is_err());
        assert_eq!(puts(&relay), 0);
        assert_eq!(relay.peek_slot(&id(&s), 2), None);
    }
}

#[tokio::test]
async fn a_received_joiner_prints_nothing_secret() {
    let relay = FakeRelay::new();
    let (mut s, mut c) = pair(Role::Holder, Role::Joiner);
    let code = to_compare(&relay, &mut s, &mut c).await;
    c.confirm(&relay, None, None, T0).await.unwrap();
    s.confirm(&relay, Some(&creds()), Some(&code), T0)
        .await
        .unwrap();
    assert_eq!(c.poll(&relay, T0).await.unwrap(), Stage::Received);
    let printed = format!("{c:?}");
    let sent = creds();
    let mut secrets = vec![
        id(&c),
        code,
        sent.account.clone(),
        sent.vault_id.to_b64(),
        HOME.to_string(),
    ];
    for (_, key) in sent.keys.epochs() {
        secrets.push(key.iter().map(|b| format!("{b:02x}")).collect());
        use base64::Engine;
        secrets.push(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(*key));
    }
    for secret in secrets {
        assert!(!printed.contains(&secret), "{printed}");
    }
    assert!(printed.contains("Received"), "{printed}");
}
