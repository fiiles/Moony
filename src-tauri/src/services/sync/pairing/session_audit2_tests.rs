//! The second sync audit (2026-11-20), finding R2-H2: a pairing code that reaches the joining
//! device through a channel someone can rewrite (a message, an e-mail) is not protected by the
//! six digits. The rewriter needs no secret and no help from the sync server: it is an ordinary
//! client of two mailboxes.
//!
//! `…_gives_the_vault_to_whoever_rewrote_it` is the finding as a test of what must hold (the
//! holder seals nothing a stranger can open); it fails until the fix (spec
//! `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`), so it is ignored. It grinds about 10^6
//! X25519 keys on every core: seconds in an optimised build, minutes in a debug one
//! (`cargo test --lib session::audit2_tests -- --ignored`; `--config
//! 'profile.dev.package.curve25519-dalek.opt-level=3'` makes it quick).
//! The control next to it shows the same person, digits and typing with the code unchanged.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use x25519_dalek::{PublicKey, StaticSecret};

use super::tests::{assert_same_creds, creds, id, public, HOME, M_SECRET, S_SECRET, T0};
use super::*;
use crate::services::sync::pairing::pairing_code_text;
use crate::services::sync::relay::fake::FakeRelay;

const J_SECRET: [u8; 32] = [0x22; 32];
/// The rewriter's own mailbox.
const M_MAILBOX: MailboxId = MailboxId([0x5a; 16]);

/// A scanner key whose comparison code with the shower `s_pub` on `mailbox` is `code`, and how
/// many keys were tried: the six digits are 20 bits, so about 10^6.
fn grind(s_pub: &[u8; 32], mailbox: &MailboxId, code: u32) -> (StaticSecret, u64) {
    let found: Mutex<Option<StaticSecret>> = Mutex::new(None);
    let done = AtomicBool::new(false);
    let trials = AtomicU64::new(0);
    let threads = std::thread::available_parallelism().map_or(4, usize::from) as u64;
    std::thread::scope(|scope| {
        for thread in 0..threads {
            let (found, done, trials) = (&found, &done, &trials);
            scope.spawn(move || {
                let mut seed = [0x4d; 32];
                let mut n = thread;
                while !done.load(Ordering::Relaxed) {
                    // Bytes 8..16: X25519 clamps bits of the first and last byte, so a counter
                    // there would try each key eight times.
                    seed[8..16].copy_from_slice(&n.to_le_bytes());
                    n += threads;
                    trials.fetch_add(1, Ordering::Relaxed);
                    let candidate = StaticSecret::from(seed);
                    let Ok(transcript) = derive(&candidate, Side::Scanner, s_pub, mailbox) else {
                        continue;
                    };
                    if transcript.code() == code {
                        *found.lock().expect("found") = Some(candidate);
                        done.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    let key = found.into_inner().expect("found").expect("a key");
    (key, trials.load(Ordering::Relaxed))
}

/// R2-H2. The holder shows a pairing code and its person sends it to the new computer in a
/// message. Someone on the way replaces it with a code of their own. What must hold: whatever the
/// person then does as the screens tell them, the holder seals nothing that anyone but the new
/// computer can open. Today the rewriter opens the vault's credentials (every key, the account,
/// the join ticket), and hands them on, so both screens end as a normal pairing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "R2-H2 (audit 2026-11-20): fails until the fix; grinds about 10^6 X25519 keys"]
async fn a_pairing_code_rewritten_on_its_way_gives_the_vault_to_whoever_rewrote_it() {
    let relay = FakeRelay::new();
    let vault = creds();

    // The holder shows its code; "Zkopírovat kód" puts this text into a message.
    let mut holder =
        PairingSession::show_with_secret(Role::Holder, HOME, T0, StaticSecret::from(S_SECRET))
            .unwrap();
    let holder_pub = public(S_SECRET);
    let sent = pairing_code_text(&holder.code());

    // The message arrives rewritten: the rewriter's mailbox and public key.
    let m_shower = StaticSecret::from(M_SECRET);
    let arrived = pairing_code_text(&PairingCode {
        mailbox: M_MAILBOX,
        shower_public: public(M_SECRET),
        relay_url: Some(HOME.to_string()),
    });
    assert_ne!(sent, arrived);

    // The new computer pastes what arrived and answers it.
    let mut joiner =
        PairingSession::scan_with_secret(Role::Joiner, &arrived, T0, StaticSecret::from(J_SECRET))
            .unwrap();
    assert_eq!(
        joiner.announce(&relay, T0).await.unwrap(),
        Stage::AwaitingAck
    );
    assert_eq!(joiner.comparison_code(), None, "no digits before the ack");

    // The rewriter reads the joiner's key from its own mailbox and knows the digits the joiner
    // will show before the joiner shows them.
    let slot = relay
        .take_slot(&M_MAILBOX.to_b64(), 1)
        .await
        .unwrap()
        .expect("the joiner's slot 1");
    let (_, joiner_pub) = parse_slot1(&slot).unwrap();
    let toward_joiner = derive(&m_shower, Side::Shower, &joiner_pub, &M_MAILBOX).unwrap();
    let digits = toward_joiner.code();

    // It finds a key of its own that gives the same digits with the holder's real code, and
    // answers the holder with it. The holder cannot tell: it acknowledges and waits for digits.
    let started = std::time::Instant::now();
    let (m_scanner, trials) = grind(&holder_pub, &holder.mailbox, digits);
    eprintln!("R2-H2: {trials} keys in {:?}", started.elapsed());
    let m_scanner_pub = PublicKey::from(&m_scanner).to_bytes();
    relay
        .put_slot(&id(&holder), 1, &slot1(Role::Joiner, &m_scanner_pub))
        .await
        .unwrap();
    assert_eq!(holder.poll(&relay, T0).await.unwrap(), Stage::Compare);

    // Only now does it acknowledge the joiner, whose digits appear.
    relay
        .put_slot(&M_MAILBOX.to_b64(), 3, &toward_joiner.ack())
        .await
        .unwrap();
    assert_eq!(joiner.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let shown = joiner.comparison_code().expect("the joiner's digits");
    assert_eq!(shown, format!("{digits:06}"));
    assert_eq!(
        joiner.confirm(&relay, None, None, T0).await.unwrap(),
        Stage::WaitingForOther
    );

    // The person types the digits the new computer shows into the holder, as the screen says.
    let typed = holder.confirm(&relay, Some(&vault), Some(&shown), T0).await;

    // What the rewriter can open of what the holder sealed.
    let toward_holder = derive(&m_scanner, Side::Scanner, &holder_pub, &holder.mailbox).unwrap();
    let stolen = match relay.take_slot(&id(&holder), 2).await {
        Ok(Some(sealed)) => open_payload(&toward_holder, &sealed).ok(),
        _ => None,
    };
    // And it hands the same credentials on, so the new computer joins as if nothing happened.
    let mut joined_normally = false;
    if let Some(payload) = &stolen {
        let resealed = seal_payload(&toward_joiner, payload).unwrap();
        relay
            .put_slot(&M_MAILBOX.to_b64(), 2, &resealed)
            .await
            .unwrap();
        joined_normally = joiner.poll(&relay, T0).await.ok() == Some(Stage::Received)
            && joiner.received().is_some();
        if joined_normally {
            assert_same_creds(joiner.received().unwrap(), &vault);
        }
    }

    assert!(
        stolen.is_none(),
        "whoever rewrote the pairing code opened the vault's credentials after {trials} keys \
         (the holder's confirm: {:?}; the new computer joined as usual: {joined_normally})",
        typed.as_ref().map_err(|e| e.to_string())
    );
}

/// The control: the same code arriving unchanged. The person types the joiner's digits, the
/// joiner receives the credentials, and a stranger who answered nothing holds nothing.
#[tokio::test]
async fn a_pairing_code_that_arrives_unchanged_pairs_the_two_devices() {
    let relay = FakeRelay::new();
    let vault = creds();
    let mut holder =
        PairingSession::show_with_secret(Role::Holder, HOME, T0, StaticSecret::from(S_SECRET))
            .unwrap();
    let arrived = pairing_code_text(&holder.code());
    let mut joiner =
        PairingSession::scan_with_secret(Role::Joiner, &arrived, T0, StaticSecret::from(J_SECRET))
            .unwrap();

    assert_eq!(
        joiner.announce(&relay, T0).await.unwrap(),
        Stage::AwaitingAck
    );
    assert_eq!(holder.poll(&relay, T0).await.unwrap(), Stage::Compare);
    assert_eq!(joiner.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let shown = joiner.comparison_code().expect("the joiner's digits");
    joiner.confirm(&relay, None, None, T0).await.unwrap();
    assert_eq!(
        holder
            .confirm(&relay, Some(&vault), Some(&shown), T0)
            .await
            .unwrap(),
        Stage::Sent
    );

    assert_eq!(joiner.poll(&relay, T0).await.unwrap(), Stage::Received);
    assert_same_creds(joiner.received().unwrap(), &vault);
}
