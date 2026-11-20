//! The second sync audit (2026-11-20), finding R2-H2: a pairing code that reaches the joining
//! device through a channel someone can rewrite (a message, an e-mail) was not protected by the
//! six digits (protocol 3). The rewriter needs no secret and no help from the sync server: it is
//! an ordinary client of two mailboxes. It read the joiner's key from its own mailbox, so it knew
//! the digits the joiner would show before they were shown, ground a key of its own that gave the
//! same digits with the holder's real key (about 10^6 X25519 keys), and answered the holder with
//! it.
//!
//! Fixed by protocol 4 (design `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`, §3): the code
//! carries a commitment to the holder's key, and the holder reveals the key only after it read the
//! other device's key. `…_gives_nothing_to_whoever_rewrote_it` is the finding as a test of what
//! must hold: the rewriter has to answer the holder with a key before it can learn the holder's,
//! so there is nothing to grind; the digits the joiner shows are not the holder's, the holder's
//! confirm is `CODE_MISMATCH` and slot 2 holds nothing. The control next to it shows the same
//! person, digits and typing with the code unchanged.

use x25519_dalek::{PublicKey, StaticSecret};

use super::tests::{assert_same_creds, creds, id, key_of, public, HOME, M_SECRET, S_SECRET, T0};
use super::*;
use crate::services::sync::pairing::pairing_code_text;
use crate::services::sync::relay::fake::FakeRelay;

const J_SECRET: [u8; 32] = [0x22; 32];
/// A key the rewriter answers the holder with: it cannot tune it, the holder's key is still unknown.
const R_SECRET: [u8; 32] = [0x4d; 32];
/// The rewriter's own mailbox.
const M_MAILBOX: MailboxId = MailboxId([0x5a; 16]);

/// R2-H2. The holder shows a pairing code and its person sends it to the new computer in a
/// message. Someone on the way replaces it with a code of their own. What must hold: whatever the
/// person then does as the screens tell them, the holder seals nothing, and in particular nothing
/// the rewriter can open.
#[tokio::test]
async fn a_pairing_code_rewritten_on_its_way_gives_nothing_to_whoever_rewrote_it() {
    let relay = FakeRelay::new();
    let vault = creds();

    // The holder shows its code; "Zkopírovat kód" puts this text into a message.
    let mut holder =
        PairingSession::show_with_secret(Role::Holder, HOME, T0, StaticSecret::from(S_SECRET))
            .unwrap();
    let holder_pub = public(S_SECRET);
    let sent = pairing_code_text(&holder.code());

    // The message arrives rewritten: the rewriter's mailbox and a commitment to its own key.
    let m_shower = StaticSecret::from(M_SECRET);
    let arrived = pairing_code_text(&PairingCode {
        mailbox: M_MAILBOX,
        commitment: commitment(&M_MAILBOX, &public(M_SECRET)),
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
    assert_eq!(
        joiner.comparison_code(),
        None,
        "no digits before the reveal"
    );

    // The rewriter reads the joiner's key from its own mailbox. Whatever it does next, it has to
    // answer the holder before it learns the holder's key (revealed only after slot 1): it
    // answers with a key it could not tune.
    let slot = relay
        .take_slot(&M_MAILBOX.to_b64(), 1)
        .await
        .unwrap()
        .expect("the joiner's slot 1");
    let (_, joiner_pub) = parse_slot1(&slot).unwrap();
    let r_scanner = StaticSecret::from(R_SECRET);
    relay
        .put_slot(
            &id(&holder),
            1,
            &slot1(Role::Joiner, &PublicKey::from(&r_scanner).to_bytes()),
        )
        .await
        .unwrap();
    assert_eq!(holder.poll(&relay, T0).await.unwrap(), Stage::Compare);

    // Only now can it reveal its key to the joiner, whose digits appear.
    let toward_joiner = derive(&m_shower, Side::Shower, &joiner_pub, &M_MAILBOX).unwrap();
    relay
        .put_slot(&M_MAILBOX.to_b64(), 3, &toward_joiner.reveal())
        .await
        .unwrap();
    assert_eq!(joiner.poll(&relay, T0).await.unwrap(), Stage::Compare);
    let shown = joiner.comparison_code().expect("the joiner's digits");
    assert_eq!(shown, toward_joiner.code_text());

    // The holder's digits are not the joiner's (they would be, by chance, one time in a million:
    // that would fail here, not silently).
    let toward_holder = derive(&r_scanner, Side::Scanner, &holder_pub, &holder.mailbox).unwrap();
    assert_ne!(shown, toward_holder.code_text());

    // The person types the digits the new computer shows into the holder, as the screen says.
    let error = holder
        .confirm(&relay, Some(&vault), Some(&shown), T0)
        .await
        .unwrap_err();
    assert_eq!(key_of(&error), CODE_MISMATCH);
    assert!(
        relay.peek_slot(&id(&holder), 2).is_none(),
        "the holder sealed nothing"
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
