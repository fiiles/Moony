//! End to end, phase 3: pairing over the sync server's mailbox and the new device's joins
//! (pairing in both directions, a Sync Recovery Kit), against a real Moony sync server
//! (Moony-relay-node, Plan 1). Every test is ignored; they run with the phase 2 ones
//! (`relay_e2e`, which has the run's recipe), one at a time:
//!
//! ```text
//! cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8787 \
//!   cargo test --lib relay_e2e -- --ignored --test-threads=1
//! ```
//!
//! The sessions are driven as the pairing commands drive them (`commands::sync_pairing`): the
//! shower's mailbox is on its stored account's server (a holder) or the typed one (a joiner), the
//! scanner's on the server its code names; a holder confirms with its stored credentials, and the
//! joiner joins with `join_new_device` on the relay of the credentials it received.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use super::binding::NoBinding;
use super::devices::list_devices;
use super::join::{join_new_device, recovery_credentials, JoinIdentity};
use super::keys::{self, Credentials};
use super::pairing::session::{
    keeps_session, PairingSession, Stage, CODE_MISMATCH, CODE_MISMATCH_FINAL,
};
use super::pairing::{pairing_code_text, parse_pairing_code, MailboxId, Role};
use super::recovery::read_recovery_input;
use super::relay::http::{normalize_base_url, HttpRelay, DEFAULT_RELAY_URL};
use super::relay::{Mailbox, RelayError, RelayResult};
use super::relay_e2e::{
    add_finances, command_key, converged, first_device, now, relay_at, relay_url, Device,
};
use super::setup::recovery_kit;

/// The address `device` stored with its account (the holder shows its mailbox there).
fn stored_relay(device: &Device) -> String {
    keys::account(&device.db.lock())
        .expect("account")
        .expect("stored")
        .0
}

/// A row of the new device's own, which the join replaces (the backup keeps it).
fn add_own_rows(device: &Device) {
    device.exec(
        "INSERT INTO bank_accounts (id, name, currency, balance) \
           VALUES ('acc-own', 'Only on this device', 'EUR', '5.00')",
    );
}

/// The joiner joins with `creds` on their own relay; then a round on each converges them, and
/// the holder lists the new device.
async fn join_and_converge(
    holder: &Device,
    joiner: &Device,
    creds: Credentials,
    name: &str,
    identity: JoinIdentity,
) {
    let relay = HttpRelay::new(&creds.relay_url).expect("the received relay address");
    let consent = super::engine::tests::agreed(&joiner.db);
    let backup: PathBuf = join_new_device(
        &relay,
        &joiner.db,
        creds,
        name,
        consent,
        identity,
        joiner.dir(),
        &NoBinding,
        now(),
    )
    .await
    .expect("join")
    .backup;
    assert!(backup.exists(), "the join's backup stays");
    assert!(joiner.enabled());
    assert_eq!(
        joiner.count("SELECT COUNT(*) FROM bank_accounts WHERE id = 'acc-own'"),
        0,
        "the vault's data replaced the device's own"
    );
    joiner.round(&relay).await;
    holder.round(&relay).await;
    converged(holder, joiner);
    assert_eq!(
        joiner.count("SELECT COUNT(*) FROM bank_transactions"),
        20,
        "the holder's finances reached the new device"
    );
    let listed = list_devices(&holder.db.lock()).expect("devices");
    assert!(
        listed
            .iter()
            .any(|device| device.name == name && !device.this_device),
        "the holder lists the new device: {listed:?}"
    );
}

/// A holder with sync on and finances pushed, and a new device with rows of its own.
async fn holder_and_new_device(url: &str) -> (Device, Device) {
    let relay = relay_at(url);
    let holder = first_device(&relay, url, "A").await;
    add_finances(&holder);
    holder.round(&relay).await;
    let joiner = Device::new();
    add_own_rows(&joiner);
    (holder, joiner)
}

/// The mailbox ID of `session`, as the relay names it.
fn mailbox_of(session: &PairingSession) -> String {
    session.code().mailbox.to_b64()
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn the_mailbox_answers_as_documented() {
    let relay = relay_at(&relay_url());
    let id = MailboxId::random().expect("id").to_b64();

    relay.put_slot(&id, 1, b"scanner").await.expect("put 1");
    assert_eq!(
        relay.take_slot(&id, 2).await.expect("take 2"),
        None,
        "an unwritten slot is empty"
    );
    assert_eq!(
        relay.take_slot(&id, 1).await.expect("take 1"),
        Some(b"scanner".to_vec())
    );
    let again = relay.take_slot(&id, 1).await;
    assert!(
        matches!(again, Err(RelayError::SlotConsumed)),
        "read once: {again:?}"
    );
    let rewrite = relay.put_slot(&id, 1, b"impostor").await;
    assert!(
        matches!(rewrite, Err(RelayError::SlotTaken)),
        "written once: {rewrite:?}"
    );

    relay.burn_mailbox(&id).await.expect("burn");
    let put = relay.put_slot(&id, 2, b"late").await;
    assert!(matches!(put, Err(RelayError::MailboxGone)), "{put:?}");
    let take = relay.take_slot(&id, 1).await;
    assert!(matches!(take, Err(RelayError::MailboxGone)), "{take:?}");
    relay
        .burn_mailbox(&id)
        .await
        .expect("a second burn is fine");

    let unknown = MailboxId::random().expect("id").to_b64();
    assert_eq!(
        relay.take_slot(&unknown, 1).await.expect("take unknown"),
        None
    );
    relay
        .burn_mailbox(&unknown)
        .await
        .expect("burning an unknown ID");
    assert_eq!(
        relay.take_slot(&unknown, 2).await.expect("still unknown"),
        None,
        "burning an ID nobody wrote stores nothing"
    );
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn the_mailbox_has_three_slots() {
    let url = relay_url();
    let relay = relay_at(&url);
    let id = MailboxId::random().expect("id").to_b64();

    relay.put_slot(&id, 3, b"ack").await.expect("put 3");
    let rewrite = relay.put_slot(&id, 3, b"again").await;
    assert!(
        matches!(rewrite, Err(RelayError::SlotTaken)),
        "written once: {rewrite:?}"
    );
    assert_eq!(
        relay.take_slot(&id, 3).await.expect("take 3"),
        Some(b"ack".to_vec())
    );
    let again = relay.take_slot(&id, 3).await;
    assert!(
        matches!(again, Err(RelayError::SlotConsumed)),
        "read once: {again:?}"
    );

    // Slot 4: the client refuses it before sending, and the relay itself answers 400 bad_slot.
    let client = relay.put_slot(&id, 4, b"x").await;
    assert!(
        matches!(&client, Err(RelayError::BadRequest(code)) if code == "bad_slot"),
        "{client:?}"
    );
    let base = normalize_base_url(&url).expect("relay address");
    let raw = reqwest::Client::new()
        .put(format!("{}/v1/pairing/{id}/4", base.trim_end_matches('/')))
        .header("content-type", "application/octet-stream")
        .body(b"x".to_vec())
        .send()
        .await
        .expect("raw put");
    assert_eq!(raw.status().as_u16(), 400);
    let body = raw.text().await.expect("body");
    assert!(body.contains("bad_slot"), "{body}");
    relay.burn_mailbox(&id).await.expect("burn");
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn three_wrong_codes_end_the_pairing_and_nothing_is_sealed() {
    let url = relay_url();
    let a = first_device(&relay_at(&url), &url, "A").await;
    let mut shower = PairingSession::show(Role::Holder, &stored_relay(&a), now()).expect("show");
    let relay = relay_at(shower.relay_url());
    let text = pairing_code_text(&shower.code());
    let mut scanner = PairingSession::scan(Role::Joiner, &text, now()).expect("scan");
    scanner.announce(&relay, now()).await.expect("announce");
    shower.poll(&relay, now()).await.expect("poll");
    scanner.poll(&relay, now()).await.expect("read the ack");
    let right = scanner.comparison_code().expect("the joiner's digits");
    scanner
        .confirm(&relay, None, None, now())
        .await
        .expect("joiner confirms");

    let wrong = if right == "000000" {
        "111111"
    } else {
        "000000"
    };
    let held = a.credentials();
    assert_eq!(
        command_key(
            shower
                .confirm(&relay, Some(&held), Some(wrong), now())
                .await
        ),
        CODE_MISMATCH
    );
    assert_eq!(
        command_key(
            shower
                .confirm(&relay, Some(&held), Some(wrong), now())
                .await
        ),
        CODE_MISMATCH
    );
    assert_eq!(
        command_key(
            shower
                .confirm(&relay, Some(&held), Some(wrong), now())
                .await
        ),
        CODE_MISMATCH_FINAL
    );
    // The right code afterwards no longer pairs: the session is over and nothing was sealed.
    let after = shower
        .confirm(&relay, Some(&held), Some(&right), now())
        .await;
    assert!(after.is_err(), "{after:?}");
    assert_ne!(shower.stage(), Stage::Sent);
    assert_eq!(
        command_key(scanner.poll(&relay, now()).await),
        "validation.syncPairingCancelled",
        "the joiner reads the cancel marker, not credentials"
    );
    assert!(scanner.received().is_none());
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_phone_joins_a_desktop_that_types_its_code() {
    let url = relay_url();
    let (a, b) = holder_and_new_device(&url).await;
    let local = normalize_base_url(&url).expect("relay address");

    // A (the desktop, sync on) shows its code on its own account's server.
    let mut shower = PairingSession::show(Role::Holder, &stored_relay(&a), now()).expect("show");
    let text = pairing_code_text(&shower.code());
    let code = parse_pairing_code(&text).expect("the shown code reads back");
    assert_eq!(code.relay_url.as_deref(), Some(local.as_str()));
    assert!(local.contains("127.0.0.1"), "{local}");
    let a_relay = relay_at(shower.relay_url());
    assert_eq!(
        shower.poll(&a_relay, now()).await.expect("poll"),
        Stage::WaitingForScan,
        "nobody scanned yet"
    );

    // B (the phone, sync off) scans it: its mailbox is on the server the code names.
    let mut scanner = PairingSession::scan(Role::Joiner, &text, now()).expect("scan");
    assert_eq!(scanner.relay_url(), local);
    let b_relay = relay_at(scanner.relay_url());
    assert_eq!(
        scanner.announce(&b_relay, now()).await.expect("announce"),
        Stage::AwaitingAck
    );
    assert_eq!(
        shower.poll(&a_relay, now()).await.expect("poll"),
        Stage::Compare,
        "A acknowledged B's key in slot 3"
    );
    assert_eq!(shower.comparison_code(), None, "the holder shows no digits");
    assert_eq!(
        scanner.poll(&b_relay, now()).await.expect("read the ack"),
        Stage::Compare
    );
    let digits = scanner.comparison_code().expect("code on B");
    assert!(!scanner.contested());

    // The joiner confirms first and finds nothing yet; then the holder seals its details once
    // its person typed B's digits.
    assert_eq!(
        scanner
            .confirm(&b_relay, None, None, now())
            .await
            .expect("B confirms"),
        Stage::WaitingForOther
    );
    assert_eq!(
        scanner.poll(&b_relay, now()).await.expect("B waits"),
        Stage::WaitingForOther
    );
    let held = a.credentials();
    assert_eq!(
        shower
            .confirm(&a_relay, Some(&held), Some(&digits), now())
            .await
            .expect("A confirms"),
        Stage::Sent
    );
    assert_eq!(
        scanner.poll(&b_relay, now()).await.expect("B reads"),
        Stage::Received
    );
    let creds = scanner.received().cloned().expect("credentials");
    assert_eq!(creds.relay_url, local);
    assert_eq!(creds.account, held.account);
    assert_eq!(creds.vault_id, held.vault_id);
    let burnt = b_relay.take_slot(&mailbox_of(&scanner), 2).await;
    assert!(
        matches!(burnt, Err(RelayError::MailboxGone)),
        "the joiner burns the mailbox once it has read: {burnt:?}"
    );

    join_and_converge(
        &a,
        &b,
        creds,
        "B",
        JoinIdentity::Unvouched(Default::default()),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_desktop_joins_from_a_phone_and_the_phone_types_the_code() {
    let url = relay_url();
    let (a, b) = holder_and_new_device(&url).await;
    let local = normalize_base_url(&url).expect("relay address");

    // B (the desktop, sync off) shows a code on the typed server.
    let mut shower = PairingSession::show(Role::Joiner, &local, now()).expect("show");
    let text = pairing_code_text(&shower.code());
    assert_eq!(
        parse_pairing_code(&text)
            .expect("the shown code reads back")
            .relay_url
            .as_deref(),
        Some(local.as_str())
    );
    let b_relay = relay_at(shower.relay_url());

    // A (the phone, sync on) scans it.
    let mut scanner = PairingSession::scan(Role::Holder, &text, now()).expect("scan");
    let a_relay = relay_at(scanner.relay_url());
    assert_eq!(
        scanner.announce(&a_relay, now()).await.expect("announce"),
        Stage::AwaitingAck
    );
    assert_eq!(
        shower.poll(&b_relay, now()).await.expect("poll"),
        Stage::Compare
    );
    assert_eq!(
        scanner.poll(&a_relay, now()).await.expect("read the ack"),
        Stage::Compare
    );
    assert_eq!(
        scanner.comparison_code(),
        None,
        "the holder shows no digits"
    );
    let digits = shower.comparison_code().expect("code on B");

    // The holder confirms first this time: the sealed details wait in slot 2.
    let held = a.credentials();
    assert_eq!(
        scanner
            .confirm(&a_relay, Some(&held), Some(&digits), now())
            .await
            .expect("A confirms"),
        Stage::Sent
    );
    assert_eq!(
        shower
            .confirm(&b_relay, None, None, now())
            .await
            .expect("B confirms"),
        Stage::WaitingForOther
    );
    assert_eq!(
        shower.poll(&b_relay, now()).await.expect("B reads"),
        Stage::Received
    );
    let creds = shower.received().cloned().expect("credentials");
    assert_eq!(creds.account, held.account);
    assert_eq!(creds.vault_id, held.vault_id);

    join_and_converge(
        &a,
        &b,
        creds,
        "B",
        JoinIdentity::Unvouched(Default::default()),
    )
    .await;
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_device_joins_with_the_recovery_kit() {
    let url = relay_url();
    let (a, b) = holder_and_new_device(&url).await;
    let kit = recovery_kit(&a.db.lock()).expect("kit");

    // Typed: the grouped code, the account with spaces, the server's address as written.
    let account = format!(
        "{} {} {} {}",
        &kit.account_number[..4],
        &kit.account_number[4..8],
        &kit.account_number[8..12],
        &kit.account_number[12..]
    );
    let input = read_recovery_input(
        &kit.recovery_code,
        Some(&account),
        Some(&format!("{url}/")),
        DEFAULT_RELAY_URL,
    )
    .expect("kit input");
    let relay = HttpRelay::new(&input.relay_url).expect("relay");
    let creds = recovery_credentials(&relay, &b.db, input)
        .await
        .expect("kit credentials");
    let held = a.credentials();
    assert_eq!(creds.relay_url, stored_relay(&a));
    assert_eq!(creds.account, held.account);
    assert_eq!(creds.vault_id, held.vault_id);

    join_and_converge(&a, &b, creds, "B", JoinIdentity::RecoveryKit).await;
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_mismatch_burns_the_mailbox_and_sends_nothing() {
    let url = relay_url();
    let a = first_device(&relay_at(&url), &url, "A").await;
    let mut shower = PairingSession::show(Role::Holder, &stored_relay(&a), now()).expect("show");
    let relay = relay_at(shower.relay_url());
    let text = pairing_code_text(&shower.code());

    // Someone who photographed the QR answers it first, and the holder pairs with them.
    let mut attacker = PairingSession::scan(Role::Joiner, &text, now()).expect("attacker");
    assert_eq!(
        attacker
            .announce(&relay, now())
            .await
            .expect("attacker announces"),
        Stage::AwaitingAck
    );
    assert_eq!(
        shower.poll(&relay, now()).await.expect("poll"),
        Stage::Compare
    );
    assert_eq!(
        attacker
            .poll(&relay, now())
            .await
            .expect("attacker reads the ack"),
        Stage::Compare
    );
    let attackers = attacker.comparison_code().expect("the attacker's digits");
    assert_eq!(shower.comparison_code(), None);

    // The person's own phone scans: slot 1 is taken, so it shows no digits at all.
    let mut scanner = PairingSession::scan(Role::Joiner, &text, now()).expect("scan");
    assert_eq!(
        scanner.announce(&relay, now()).await.expect("announce"),
        Stage::AwaitingAck
    );
    assert!(scanner.contested());
    assert_eq!(scanner.comparison_code(), None);

    // The person gives up on the phone.
    scanner.abandon(&relay).await;

    // The holder's person types the attacker's digits anyway (told them, say): nothing is sealed
    // into the mailbox.
    let held = a.credentials();
    let confirmed = shower
        .confirm(&relay, Some(&held), Some(&attackers), now())
        .await;
    assert!(
        !confirmed.as_ref().is_err_and(keeps_session),
        "{confirmed:?}"
    );
    let key = command_key(confirmed);
    assert!(
        key == "validation.syncPairingCancelled" || key == "validation.syncPairingExpired",
        "{key}"
    );
    assert_eq!(shower.stage(), Stage::Compare, "the holder sent nothing");
    let peek = relay.take_slot(&mailbox_of(&shower), 2).await;
    assert!(
        matches!(peek, Err(RelayError::MailboxGone)),
        "the mailbox is burnt: {peek:?}"
    );

    // The attacker, confirming and waiting, gets nothing.
    assert_eq!(
        attacker
            .confirm(&relay, None, None, now())
            .await
            .expect("attacker confirms"),
        Stage::WaitingForOther
    );
    let read = attacker.poll(&relay, now()).await;
    assert_eq!(
        command_key(read),
        "validation.syncPairingCancelled",
        "the attacker reads a burnt mailbox"
    );
    assert!(attacker.received().is_none());
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_holder_confirming_an_attackers_code_seals_nothing() {
    let url = relay_url();
    let a = first_device(&relay_at(&url), &url, "A").await;
    let mut shower = PairingSession::show(Role::Holder, &stored_relay(&a), now()).expect("show");
    let relay = relay_at(shower.relay_url());
    let text = pairing_code_text(&shower.code());
    let mut attacker = PairingSession::scan(Role::Joiner, &text, now()).expect("attacker");
    attacker
        .announce(&relay, now())
        .await
        .expect("attacker announces");
    shower.poll(&relay, now()).await.expect("poll");
    attacker
        .poll(&relay, now())
        .await
        .expect("attacker reads the ack");
    let attackers = attacker.comparison_code().expect("the attacker's digits");

    // The contested phone closes slot 2 with a cancel marker before anyone confirms.
    let mut scanner = PairingSession::scan(Role::Joiner, &text, now()).expect("scan");
    scanner.announce(&relay, now()).await.expect("announce");
    assert!(scanner.contested());
    assert_eq!(
        command_key(scanner.confirm(&relay, None, None, now()).await),
        "validation.syncPairingCodeUsed",
        "a contested scan cannot confirm"
    );

    // The holder's person types the attacker's digits before the phone gives up: the slot-2
    // write meets the marker, and the attacker reads "cancelled".
    let held = a.credentials();
    assert_eq!(
        command_key(
            shower
                .confirm(&relay, Some(&held), Some(&attackers), now())
                .await
        ),
        "validation.syncPairingFailed"
    );
    assert_eq!(shower.stage(), Stage::Compare, "the holder sent nothing");
    attacker
        .confirm(&relay, None, None, now())
        .await
        .expect("attacker confirms");
    assert_eq!(
        command_key(attacker.poll(&relay, now()).await),
        "validation.syncPairingCancelled"
    );
    assert!(attacker.received().is_none());
    scanner.abandon(&relay).await;
}

/// The real relay, losing the answer to its first slot write after the relay stored it (a reset
/// after the request was written, as `HttpRelay` reports it).
struct LosesFirstPutAnswer<'a> {
    relay: &'a HttpRelay,
    lost: AtomicBool,
}

impl Mailbox for LosesFirstPutAnswer<'_> {
    async fn put_slot(&self, mailbox: &str, slot: u8, bytes: &[u8]) -> RelayResult<()> {
        let stored = self.relay.put_slot(mailbox, slot, bytes).await;
        if !self.lost.swap(true, Ordering::SeqCst) {
            stored.expect("the relay stored the first write");
            return Err(RelayError::Network("request".into()));
        }
        stored
    }

    async fn take_slot(&self, mailbox: &str, slot: u8) -> RelayResult<Option<Vec<u8>>> {
        self.relay.take_slot(mailbox, slot).await
    }

    async fn burn_mailbox(&self, mailbox: &str) -> RelayResult<()> {
        self.relay.burn_mailbox(mailbox).await
    }
}

#[tokio::test]
#[ignore = "needs a Moony sync server (Plan 1) with SETUP_CODE: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_lost_slot_1_answer_is_taken_as_ours() {
    let url = relay_url();
    let a = first_device(&relay_at(&url), &url, "A").await;
    let mut shower = PairingSession::show(Role::Holder, &stored_relay(&a), now()).expect("show");
    let relay = relay_at(shower.relay_url());
    let text = pairing_code_text(&shower.code());
    let mut scanner = PairingSession::scan(Role::Joiner, &text, now()).expect("scan");
    let lossy = LosesFirstPutAnswer {
        relay: &relay,
        lost: AtomicBool::new(false),
    };

    // The first announce stored slot 1 but lost the answer: the pairing goes on, announcing.
    let first = scanner.announce(&lossy, now()).await;
    assert!(first.as_ref().is_err_and(keeps_session), "{first:?}");
    assert_eq!(scanner.stage(), Stage::Announcing);

    // The retry (the next poll) meets its own write: the relay answers 409, read as ours.
    assert_eq!(
        scanner.poll(&lossy, now()).await.expect("retry"),
        Stage::AwaitingAck
    );
    assert!(!scanner.contested(), "our own write is no contest");
    assert_eq!(
        shower.poll(&relay, now()).await.expect("poll"),
        Stage::Compare
    );
    assert_eq!(
        scanner.poll(&relay, now()).await.expect("read the ack"),
        Stage::Compare
    );
    let digits = scanner.comparison_code().expect("the joiner's digits");

    // The holder's slot-2 write loses its answer too: its second confirm meets its own sealed
    // details (409) and counts them as sent; the joiner opens the first copy.
    let held = a.credentials();
    scanner
        .confirm(&relay, None, None, now())
        .await
        .expect("joiner confirms");
    let lossy = LosesFirstPutAnswer {
        relay: &relay,
        lost: AtomicBool::new(false),
    };
    let first = shower
        .confirm(&lossy, Some(&held), Some(&digits), now())
        .await;
    assert!(first.as_ref().is_err_and(keeps_session), "{first:?}");
    assert_eq!(shower.stage(), Stage::Compare);
    assert_eq!(
        shower
            .confirm(&lossy, Some(&held), Some(&digits), now())
            .await
            .expect("holder confirms again"),
        Stage::Sent
    );
    assert_eq!(
        scanner.poll(&relay, now()).await.expect("read"),
        Stage::Received
    );
    assert_eq!(
        scanner.received().map(|creds| creds.vault_id),
        Some(held.vault_id)
    );
}
