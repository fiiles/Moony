//! Tests of `sync_pairing.rs`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;
use crate::services::sync::error::Transient;
use crate::services::sync::keys::Credentials;
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::RelayError;
use crate::services::sync::test_support::migrated;

const T0: i64 = 1_800_000_000;
const HOME: &str = "https://sync.home.example.com";

fn owner(path: &str, session: u64) -> KitOwner {
    KitOwner {
        db_path: PathBuf::from(path),
        session,
    }
}

fn validation(key: &str) -> SyncError {
    SyncError::Local(AppError::Validation(key.into()))
}

fn key_of(result: Result<impl std::fmt::Debug>) -> String {
    match result.expect_err("refused") {
        AppError::Validation(key) | AppError::Auth(key) => key,
        other => panic!("not a key: {other:?}"),
    }
}

fn wrong_password() -> Result<()> {
    Err(AppError::Auth("auth.wrongPassword".into()))
}

#[tokio::test]
async fn pairing_needs_the_password_before_anything_is_shown_or_written() {
    // A joiner shows a code; this device scans it as the holder.
    let shower = PairingSession::show(Role::Joiner, HOME, T0).expect("shower");
    let text = pairing_code_text(&shower.code());
    let mailbox = mailbox_of(&shower).to_b64();

    for (password, verify, expected) in [
        ("", Ok(()), "validation.syncPasswordNeeded"),
        ("wrong", wrong_password(), "auth.wrongPassword"),
    ] {
        let relay = FakeRelay::new();
        let polled = AtomicBool::new(false);
        let scan = async {
            polled.store(true, Ordering::SeqCst);
            let scanned = PairingSession::scan(Role::Holder, &text, T0)?;
            announced(scanned, &relay, T0).await
        };
        let refused = begin(
            Zeroizing::new(password.to_string()),
            std::future::ready(verify),
            scan,
        )
        .await;
        assert_eq!(key_of(refused), expected);
        assert!(!polled.load(Ordering::SeqCst), "the scan never started");
        assert!(relay.calls().is_empty(), "nothing was sent: {password:?}");
        assert_eq!(relay.peek_slot(&mailbox, 1), None);

        // A holder's show: no pairing code is made.
        let shown = AtomicBool::new(false);
        let show = async {
            shown.store(true, Ordering::SeqCst);
            Ok(PairingSession::show(Role::Holder, HOME, T0)?)
        };
        let verify = if password.is_empty() {
            Ok(())
        } else {
            wrong_password()
        };
        let refused = begin(
            Zeroizing::new(password.to_string()),
            std::future::ready(verify),
            show,
        )
        .await;
        assert_eq!(key_of(refused), expected);
        assert!(!shown.load(Ordering::SeqCst), "no code was made");
    }

    // The right password: the scan answers, and slot 1 is written.
    let relay = FakeRelay::new();
    let scan = async {
        let scanned = PairingSession::scan(Role::Holder, &text, T0)?;
        announced(scanned, &relay, T0).await
    };
    let scanned = begin(
        Zeroizing::new("right".to_string()),
        std::future::ready(Ok(())),
        scan,
    )
    .await
    .expect("scanned");
    assert_eq!(scanned.stage(), Stage::AwaitingAck);
    assert!(relay.peek_slot(&mailbox, 1).is_some(), "slot 1 is written");
}

fn slot_of(owner: KitOwner) -> PairingSlot {
    PairingSlot {
        owner,
        session: PairingSession::show(Role::Holder, HOME, T0).expect("session"),
        device_name: None,
    }
}

#[test]
fn a_pairing_session_belongs_to_one_profile_session() {
    let a = owner("/profiles/a/moony.db", 1);
    let now = T0 + 10;

    let (kept, stale) = slot_for(&a, now, Some(slot_of(a.clone())));
    assert!(kept.is_some_and(|slot| slot.owner == a));
    assert!(stale.is_none());

    for other in [
        owner("/profiles/a/moony.db", 2),
        owner("/profiles/b/moony.db", 1),
    ] {
        let (kept, stale) = slot_for(&other, now, Some(slot_of(a.clone())));
        assert!(kept.is_none(), "{other:?} does not get it");
        assert!(stale.is_some_and(|slot| slot.owner == a), "it is abandoned");
    }

    // The right profile session, past `wiped_at`.
    let slot = slot_of(a.clone());
    let wiped_at = slot.session.wiped_at();
    assert_eq!(wiped_at, T0 + 600);
    let (kept, stale) = slot_for(&a, wiped_at - 1, Some(slot));
    assert!(kept.is_some() && stale.is_none(), "alive until wiped_at");
    let (kept, stale) = slot_for(&a, wiped_at, kept);
    assert!(kept.is_none());
    assert!(stale.is_some(), "abandoned at wiped_at");

    let (kept, stale) = slot_for(&a, now, None);
    assert!(kept.is_none() && stale.is_none());
}

#[test]
fn a_transient_join_failure_keeps_the_received_session() {
    for kept in [
        SyncError::Transient(Transient::Offline),
        SyncError::Transient(Transient::Relay),
        SyncError::RateLimited,
        validation("validation.syncChangeInProgress"),
        // The person can cancel the pending setup and try again within the grace period.
        validation("validation.syncJoinSetupPending"),
        // Nothing was replaced: the confirmation reads the new numbers and asks again.
        validation("validation.syncReplaceImpactChanged"),
    ] {
        assert!(after_join_error(&kept), "{kept:?} keeps the session");
    }
    for ended in [
        validation("validation.syncAlreadyEnabled"),
        validation("validation.syncVaultHasNoSnapshot"),
        SyncError::Refused(RelayError::AccountNotFound),
        SyncError::Refused(RelayError::SlotConsumed),
        SyncError::VaultNotFound,
        SyncError::Local(AppError::Internal("disk full".into())),
    ] {
        assert!(!after_join_error(&ended), "{ended:?} ends the pairing");
    }
}

#[test]
fn the_pairing_code_text_is_shown_with_the_qr() {
    let session = PairingSession::show(Role::Holder, HOME, T0).expect("session");
    let text = pairing_code_text(&session.code());
    assert!(text.starts_with("moony-pair:3:"));

    let view = view_of(&session).expect("view");
    assert_eq!(
        view.pairing_code.as_deref(),
        Some(text.as_str()),
        "the text goes with the QR in every build"
    );
    assert!(view
        .qr_svg
        .as_deref()
        .is_some_and(|svg| svg.contains("<svg")));
    assert_eq!(view.stage, PairingStage::WaitingForScan);
    assert_eq!(view.role, PairingRole::Holder);
    assert_eq!(view.comparison_code, None);
    assert!(!view.contested);
    assert_eq!(view.expires_at, T0 + 600);

    // A scanner shows no code of its own,
    let scanner = PairingSession::scan(Role::Joiner, &text, T0).expect("scanner");
    let view = view_of(&scanner).expect("view");
    assert_eq!(view.stage, PairingStage::Announcing);
    assert_eq!(view.role, PairingRole::Joiner);
    assert_eq!((view.qr_svg, view.pairing_code), (None, None));
}

#[test]
fn the_view_names_a_custom_server_and_the_holders_attempts() {
    let default = PairingSession::show(Role::Holder, &default_base_url(), T0).expect("shower");
    assert_eq!(view_of(&default).expect("view").server_host, None);
    let own = PairingSession::show(Role::Holder, "https://sync.example.org:8443", T0).expect("own");
    let view = view_of(&own).expect("view");
    assert_eq!(view.server_host.as_deref(), Some("sync.example.org:8443"));
    assert_eq!(view.attempts_left, None, "only in compare");
    assert_eq!(view.comparison_code, None);
    assert_eq!(
        server_host("https://sync.example.org"),
        Some("sync.example.org".into())
    );
}

#[tokio::test]
async fn a_holder_at_compare_has_no_digits_and_all_its_attempts() {
    use crate::services::sync::pairing::Role as R;
    let relay = FakeRelay::new();
    let shower = PairingSession::show(R::Joiner, HOME, T0).expect("shower");
    let text = pairing_code_text(&shower.code());
    let mut shower = shower;
    let mut holder = PairingSession::scan(R::Holder, &text, T0).expect("scanner");
    holder.announce(&relay, T0).await.expect("announce");
    shower.poll(&relay, T0).await.expect("acknowledge");
    holder.poll(&relay, T0).await.expect("verify");
    assert_eq!(holder.stage(), Stage::Compare);
    let view = view_of(&holder).expect("view");
    assert_eq!(view.stage, PairingStage::Compare);
    assert_eq!(view.role, PairingRole::Holder);
    assert_eq!(view.comparison_code, None);
    assert_eq!(view.attempts_left, Some(3));
    // The joiner shows its digits and has no attempts to count.
    let joiner = view_of(&shower).expect("view");
    assert!(joiner.comparison_code.is_some());
    assert_eq!(joiner.attempts_left, None);
}

#[test]
fn the_acknowledgement_stages_read_as_connecting() {
    assert_eq!(stage_of(Stage::AwaitingAck), PairingStage::Connecting);
    assert_eq!(stage_of(Stage::Acknowledging), PairingStage::Connecting);
    assert_eq!(stage_of(Stage::Announcing), PairingStage::Announcing);
    assert_eq!(
        stage_of(Stage::WaitingForScan),
        PairingStage::WaitingForScan
    );
}

#[test]
fn the_shown_mailbox_is_on_the_server_of_the_role() {
    let conn = migrated();
    let default = normalize_base_url(&default_base_url()).expect("default");
    // Without an account: a holder cannot show, a joiner uses the typed address or the
    // default.
    assert_eq!(
        key_of(shower_relay(&conn, Role::Holder, Some(HOME))),
        "validation.syncSetupIncomplete"
    );
    assert_eq!(
        shower_relay(&conn, Role::Joiner, None).expect("joiner"),
        default
    );
    assert_eq!(
        shower_relay(&conn, Role::Joiner, Some("  ")).expect("joiner"),
        default
    );
    assert_eq!(
        shower_relay(&conn, Role::Joiner, Some(" https://sync.example.com/ ")).expect("typed"),
        normalize_base_url("https://sync.example.com").expect("typed")
    );
    assert_eq!(
        key_of(shower_relay(&conn, Role::Joiner, Some("ftp://nope"))),
        "validation.syncRelayUrlInvalid"
    );

    // With an account: a holder always its own; a joiner the typed one first.
    keys::save_account(&conn, HOME, "1234567890123456").expect("account");
    assert_eq!(
        shower_relay(&conn, Role::Holder, Some("https://sync.example.com")).expect("holder"),
        HOME
    );
    assert_eq!(
        shower_relay(&conn, Role::Joiner, None).expect("joiner"),
        HOME
    );
    assert_eq!(
        shower_relay(&conn, Role::Joiner, Some("https://sync.example.com")).expect("typed"),
        normalize_base_url("https://sync.example.com").expect("typed")
    );
}

#[test]
fn a_joiner_may_type_a_connection_code() {
    use crate::services::sync::connection_code::encode_connection_code;
    let conn = migrated();
    let code = encode_connection_code("https://sync.example.com", "1111222233334444");
    assert_eq!(
        shower_relay(&conn, Role::Joiner, Some(&code)).expect("code"),
        "https://sync.example.com"
    );
}

/// A joiner whose credentials (sent by a holder on `server`) arrived: its session at `Received`.
async fn received_joiner(server: &str) -> PairingSession {
    use crate::services::sync::keys::{VaultId, VaultKeys};
    let relay = FakeRelay::new();
    let mut shower = PairingSession::show(Role::Holder, HOME, T0).expect("shower");
    let text = pairing_code_text(&shower.code());
    let mut joiner = PairingSession::scan(Role::Joiner, &text, T0).expect("scanner");
    joiner.announce(&relay, T0).await.expect("announce");
    shower.poll(&relay, T0).await.expect("acknowledge");
    joiner.poll(&relay, T0).await.expect("verify");
    let digits = joiner.comparison_code().expect("digits");
    joiner
        .confirm(&relay, None, None, T0)
        .await
        .expect("confirm");
    let creds = Credentials {
        relay_url: server.into(),
        account: "1234567890123456".into(),
        vault_id: VaultId([0x77; 16]),
        keys: VaultKeys::from_parts(0, std::collections::BTreeMap::from([(0, [0xA0; 32])]))
            .expect("keys"),
    };
    shower
        .confirm(&relay, Some(&creds), Some(&digits), T0)
        .await
        .expect("send");
    assert_eq!(
        joiner.poll(&relay, T0).await.expect("read"),
        Stage::Received
    );
    (joiner, shower).0
}

#[tokio::test]
async fn the_view_names_an_insecure_join_server() {
    let plain = received_joiner("http://192.168.1.20:8787").await;
    let view = view_of(&plain).expect("view");
    assert_eq!(view.stage, PairingStage::Received);
    assert_eq!(view.join_server.as_deref(), Some("192.168.1.20:8787"));
    assert!(view.join_server_insecure);

    let secure = received_joiner("https://sync.example.com").await;
    let view = view_of(&secure).expect("view");
    assert_eq!(view.join_server.as_deref(), Some("sync.example.com"));
    assert!(!view.join_server_insecure);

    // A holder's view, and a joiner before the credentials, name nothing.
    let holder = PairingSession::show(Role::Holder, HOME, T0).expect("holder");
    let view = view_of(&holder).expect("view");
    assert_eq!((view.join_server, view.join_server_insecure), (None, false));
}

#[test]
fn only_a_joiner_needs_a_device_name() {
    assert_eq!(joiner_name(Role::Holder, None).expect("holder"), None);
    assert_eq!(
        joiner_name(Role::Holder, Some(String::new())).expect("ignored"),
        None
    );
    assert_eq!(
        joiner_name(Role::Joiner, Some("Work laptop".into())).expect("joiner"),
        Some("Work laptop".to_string())
    );
    for invalid in [None, Some("   ".to_string()), Some("x".repeat(61))] {
        assert_eq!(
            key_of(joiner_name(Role::Joiner, invalid)),
            "validation.syncDeviceNameInvalid"
        );
    }
}

/// A `Pairings` whose disposals a test counts (each test its own counter).
macro_rules! counted_pairings {
    ($counter:ident) => {{
        static $counter: AtomicUsize = AtomicUsize::new(0);
        fn dispose(slot: PairingSlot) {
            $counter.fetch_add(1, Ordering::SeqCst);
            drop(slot);
        }
        (Pairings::new(dispose), &$counter)
    }};
}

fn put_count(relay: &FakeRelay) -> usize {
    relay
        .calls()
        .iter()
        .filter(|route| matches!(route, Route::MailboxPut))
        .count()
}

#[tokio::test]
async fn forgetting_never_waits_for_a_running_join() {
    let (pairings, disposed) = counted_pairings!(FORGET_BUSY);
    let a = owner("/profiles/a/moony.db", 1);
    *pairings.lock().await = Some(slot_of(a.clone()));

    // A join holds the slot; a lock forgets the pairing meanwhile, without waiting.
    let join = pairings.lock().await;
    pairings.forget();
    assert!(join.is_some(), "the running join keeps its slot");
    assert_eq!(disposed.load(Ordering::SeqCst), 0);
    // The join ends: the pairing goes with it.
    drop(join);
    assert_eq!(disposed.load(Ordering::SeqCst), 1);
    assert!(pairings.lock().await.is_none());

    // A free slot is given up at once.
    *pairings.lock().await = Some(slot_of(a.clone()));
    pairings.forget();
    assert_eq!(disposed.load(Ordering::SeqCst), 2);
    assert!(pairings.lock().await.is_none());
    assert_eq!(
        disposed.load(Ordering::SeqCst),
        2,
        "nothing is forgotten twice"
    );
}

#[tokio::test]
async fn a_forget_is_honoured_by_whoever_takes_the_slot_next() {
    let (pairings, disposed) = counted_pairings!(FORGET_NEXT);
    let a = owner("/profiles/a/moony.db", 1);
    *pairings.lock().await = Some(slot_of(a.clone()));
    let join = pairings.lock().await;
    pairings.forget();
    // Whatever holds the lock next (here a new start waiting behind the join) sees the old
    // pairing gone before it installs its own, and its own pairing survives.
    let next = async {
        let mut pairing = pairings.lock().await;
        assert!(pairing.is_none(), "the forgotten pairing is gone first");
        *pairing = Some(slot_of(owner("/profiles/a/moony.db", 2)));
    };
    let release = async move {
        tokio::task::yield_now().await;
        drop(join);
    };
    tokio::join!(next, release);
    assert_eq!(disposed.load(Ordering::SeqCst), 1);
    assert!(pairings
        .lock()
        .await
        .as_ref()
        .is_some_and(|slot| slot.owner.session == 2));
}

#[tokio::test]
async fn a_repeated_scan_of_the_same_code_keeps_its_session() {
    let (pairings, disposed) = counted_pairings!(REPEATED_SCAN);
    let a = owner("/profiles/a/moony.db", 1);
    let shower = PairingSession::show(Role::Joiner, HOME, T0).expect("shower");
    let text = pairing_code_text(&shower.code());
    let relay = FakeRelay::new();
    let scan = || PairingSession::scan(Role::Holder, &text, T0).expect("scan");

    let first = scan_under_lock(&pairings, &a, || Some(a.clone()), scan(), &relay, None, T0)
        .await
        .expect("first scan");
    // Answered, waiting for the shower's acknowledgement: nothing to show yet.
    assert_eq!(first.stage, PairingStage::Connecting);
    assert!(!first.contested);

    // The camera reports the same QR again: the same session answers, nothing is written.
    let again = scan_under_lock(&pairings, &a, || Some(a.clone()), scan(), &relay, None, T0)
        .await
        .expect("repeated scan");
    assert_eq!(again, first);
    assert_eq!(put_count(&relay), 1, "slot 1 is written once");
    assert_eq!(disposed.load(Ordering::SeqCst), 0);

    // Another code replaces the session (the older one is abandoned).
    let other = PairingSession::show(Role::Joiner, HOME, T0).expect("other");
    let other_text = pairing_code_text(&other.code());
    let replaced = scan_under_lock(
        &pairings,
        &a,
        || Some(a.clone()),
        PairingSession::scan(Role::Holder, &other_text, T0).expect("scan"),
        &relay,
        None,
        T0,
    )
    .await
    .expect("another code");
    assert_eq!(
        replaced.comparison_code, None,
        "no digits before the acknowledgement"
    );
    assert_eq!(put_count(&relay), 2, "the other code's slot 1 is written");
    assert_eq!(disposed.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_scans_of_one_code_answer_it_once() {
    let (pairings, disposed) = counted_pairings!(CONCURRENT_SCANS);
    let a = owner("/profiles/a/moony.db", 1);
    let shower = PairingSession::show(Role::Joiner, HOME, T0).expect("shower");
    let text = pairing_code_text(&shower.code());
    let relay = FakeRelay::new();
    let scan = || {
        scan_under_lock(
            &pairings,
            &a,
            || Some(a.clone()),
            PairingSession::scan(Role::Holder, &text, T0).expect("scan"),
            &relay,
            None,
            T0,
        )
    };
    let (one, two) = tokio::join!(scan(), scan());
    let (one, two) = (one.expect("one"), two.expect("two"));
    assert_eq!(one, two);
    assert!(!one.contested && one.stage == PairingStage::Connecting);
    assert_eq!(put_count(&relay), 1);
    assert_eq!(disposed.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn concurrent_starts_leave_one_pairing() {
    let (pairings, disposed) = counted_pairings!(CONCURRENT_STARTS);
    let a = owner("/profiles/a/moony.db", 1);
    let start = || {
        start_under_lock(
            &pairings,
            &a,
            || Some(a.clone()),
            PairingSession::show(Role::Holder, HOME, T0).expect("show"),
            None,
        )
    };
    let (one, two) = tokio::join!(start(), start());
    let (one, two) = (one.expect("one"), two.expect("two"));
    assert_ne!(one.qr_svg, two.qr_svg, "two codes were made");
    assert_eq!(disposed.load(Ordering::SeqCst), 1, "the first is abandoned");
    let installed = pairings.lock().await;
    let installed = installed.as_ref().expect("one pairing");
    assert_eq!(view_of(&installed.session).expect("view"), two);
}

#[tokio::test]
async fn a_pairing_started_by_a_session_that_ended_is_not_installed() {
    let (pairings, disposed) = counted_pairings!(ENDED_OWNER);
    let a = owner("/profiles/a/moony.db", 1);
    let refused = start_under_lock(
        &pairings,
        &a,
        || Some(owner("/profiles/a/moony.db", 2)),
        PairingSession::show(Role::Holder, HOME, T0).expect("show"),
        None,
    )
    .await;
    assert_eq!(
        key_of(refused.map_err(|e| e.to_app_error())),
        "validation.syncPairingNotStarted"
    );
    assert_eq!(disposed.load(Ordering::SeqCst), 1);
    assert!(pairings.lock().await.is_none());
}

#[test]
fn the_wiper_rechecks_the_clock_at_least_every_30_seconds() {
    let wiped_at = T0 + 900;
    assert_eq!(wiper_wait(T0, wiped_at), Some(Duration::from_secs(30)));
    assert_eq!(
        wiper_wait(wiped_at - 30, wiped_at),
        Some(Duration::from_secs(30))
    );
    assert_eq!(
        wiper_wait(wiped_at - 7, wiped_at),
        Some(Duration::from_secs(7))
    );
    // After a machine sleep the wall clock may be far past `wiped_at`.
    assert_eq!(wiper_wait(wiped_at, wiped_at), None);
    assert_eq!(wiper_wait(wiped_at + 3_600, wiped_at), None);
}
