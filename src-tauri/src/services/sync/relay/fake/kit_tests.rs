//! Tests of `FakeRelay`'s Recovery Kit: `put_kit`, `get_kit`, the Kit of a rotation, the vault
//! info's `kit` and the failures-only read limit.

use super::*;
use zeroize::Zeroizing;

const VAULT_ID: &str = "AAECAwQFBgcICQoLDA0ODw";
const TOKEN: &str = "dG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9rZW4tdG9";
const NEW_TOKEN: &str = "bmV3LXRva2VuLW5ldy10b2tlbi1uZXctdG9rZW4tbmV";
const THIRD_TOKEN: &str = "dGhpcmQtdG9rZW4tdGhpcmQtdG9rZW4tdGhpcmQtdG9";
const DAY: i64 = 86_400;

fn tok(c: char) -> String {
    c.to_string().repeat(43)
}

/// A distinct valid token per `n`: `prefix` and `n`, padded to 43 base64url characters.
fn numbered(prefix: &str, n: u32) -> String {
    format!("{:_<43}", format!("{prefix}{n}"))
}

fn auth() -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(TOKEN.into()),
    }
}

fn auth_with(token: &str) -> VaultAuth {
    VaultAuth {
        vault_id: VAULT_ID.into(),
        token: Zeroizing::new(token.into()),
    }
}

async fn relay_with_vault() -> (FakeRelay, String) {
    let relay = FakeRelay::new();
    let account = relay.create_account().await.expect("account");
    relay.set_paid(&account, true);
    assert!(relay.create_vault(&account, &auth()).await.expect("vault"));
    (relay, account)
}

/// A vault with the Kit `K` and the envelope `[1, 1]`.
async fn relay_with_kit() -> (FakeRelay, String) {
    let (relay, account) = relay_with_vault().await;
    assert!(relay
        .put_kit(&auth(), &tok('K'), &[1, 1])
        .await
        .expect("kit"));
    (relay, account)
}

fn bad(code: &str) -> RelayError {
    RelayError::BadRequest(code.into())
}

fn kit<'a>(envelope: &'a [u8], token: Option<&'a str>) -> KitRotation<'a> {
    KitRotation { envelope, token }
}

#[tokio::test]
async fn a_kit_is_set_up_once_and_a_retried_setup_replaces_the_envelope() {
    let (relay, _) = relay_with_vault().await;
    assert_eq!(relay.vault_info(&auth()).await.expect("info").kit, None);
    assert!(relay.put_kit(&auth(), &tok('K'), &[1]).await.expect("put"));
    let info = relay.vault_info(&auth()).await.expect("info").kit;
    let info = info.expect("a Kit");
    assert_eq!(info.last_read, None);
    // The same Kit token again: the envelope is replaced, `since` stays.
    assert!(!relay
        .put_kit(&auth(), &tok('K'), &[2, 2])
        .await
        .expect("put"));
    let read = relay.get_kit(VAULT_ID, &tok('K')).await.expect("get");
    assert_eq!(read.envelope, vec![2, 2]);
    assert_eq!(read.since, info.since);
    // Another token while a Kit exists is `KitExists`; only a rotation replaces the Kit.
    assert_eq!(
        relay.put_kit(&auth(), &tok('L'), &[3]).await,
        Err(RelayError::KitExists)
    );
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('K'))
            .await
            .expect("get")
            .envelope,
        vec![2, 2]
    );
}

#[tokio::test]
async fn put_kit_is_judged_by_the_current_vault_token_and_needs_no_subscription() {
    let (relay, account) = relay_with_vault().await;
    relay.set_paid_until(&account, 1);
    // Unpaid: setting the Kit up still works (the owner's call).
    assert!(relay.put_kit(&auth(), &tok('K'), &[1]).await.expect("put"));
    assert_eq!(
        relay.put_kit(&auth_with(&tok('x')), &tok('L'), &[1]).await,
        Err(RelayError::Unauthorized)
    );
    relay
        .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[9], None))
        .await
        .expect("rotate");
    // The retired vault token only reads.
    assert_eq!(
        relay.put_kit(&auth(), &tok('K'), &[1]).await,
        Err(RelayError::TokenReadOnly)
    );
}

#[tokio::test]
async fn put_kit_checks_its_arguments_and_the_token_conflicts() {
    let (relay, _) = relay_with_vault().await;
    assert_eq!(
        relay.put_kit(&auth(), "short", &[1]).await,
        Err(bad("invalid_kit_token"))
    );
    assert_eq!(
        relay.put_kit(&auth(), &tok('K'), &[]).await,
        Err(bad("invalid_kit"))
    );
    assert_eq!(
        relay
            .put_kit(&auth(), &tok('K'), &vec![0; MAX_KIT_ENVELOPE + 1])
            .await,
        Err(bad("kit_too_large"))
    );
    assert!(relay
        .put_kit(&auth(), &tok('K'), &vec![0; MAX_KIT_ENVELOPE])
        .await
        .is_ok());
    // The Kit token may not be the vault's, current or previous (whatever its age).
    let (relay, _) = relay_with_vault().await;
    assert_eq!(
        relay.put_kit(&auth(), TOKEN, &[1]).await,
        Err(bad("invalid_kit_token"))
    );
    relay
        .rotate_token(&auth(), NEW_TOKEN, &[1])
        .await
        .expect("rotate");
    relay.advance_clock(90 * DAY);
    assert_eq!(
        relay.put_kit(&auth_with(NEW_TOKEN), TOKEN, &[1]).await,
        Err(bad("invalid_kit_token"))
    );
    assert_eq!(
        relay.put_kit(&auth_with(NEW_TOKEN), NEW_TOKEN, &[1]).await,
        Err(bad("invalid_kit_token"))
    );
}

#[tokio::test]
async fn the_kit_token_reads_the_envelope_and_the_minute_is_recorded() {
    let (relay, _) = relay_with_kit().await;
    let read = relay.get_kit(VAULT_ID, &tok('K')).await.expect("get");
    assert_eq!(read.envelope, vec![1, 1]);
    assert_eq!(read.updated_at, read.since);
    let kit = relay
        .vault_info(&auth())
        .await
        .expect("info")
        .kit
        .expect("kit");
    let minute = kit.last_read.expect("read");
    assert_eq!(minute % 60, 0);
    assert!((now() - minute) < 120);
    assert_eq!(kit.since, read.since);
}

#[tokio::test]
async fn nothing_but_the_kit_token_reads_the_kit() {
    let (relay, _) = relay_with_kit().await;
    // A vault token, a wrong token, an unknown vault.
    for token in [TOKEN.to_string(), tok('Z')] {
        assert_eq!(
            relay.get_kit(VAULT_ID, &token).await,
            Err(RelayError::Unauthorized)
        );
    }
    assert_eq!(
        relay.get_kit("EBESExQVFhcYGRobHB0eHw", &tok('K')).await,
        Err(RelayError::VaultNotFound)
    );
    // A vault without a Kit.
    let (without, _) = relay_with_vault().await;
    assert_eq!(
        without.get_kit(VAULT_ID, &tok('K')).await,
        Err(RelayError::Unauthorized)
    );
    // Malformed arguments never reach the server.
    assert_eq!(
        relay.get_kit("short", &tok('K')).await,
        Err(bad("invalid_vault_id"))
    );
    assert_eq!(
        relay.get_kit(VAULT_ID, "short").await,
        Err(bad("invalid_kit_token"))
    );
    // A failed read changed nothing: the Kit was never read.
    assert_eq!(
        relay
            .vault_info(&auth())
            .await
            .expect("info")
            .kit
            .expect("kit")
            .last_read,
        None
    );
}

#[tokio::test]
async fn a_kit_read_follows_the_read_rule_of_the_subscription() {
    let (relay, account) = relay_with_kit().await;
    let at = now();
    // Inside the 30-day grace after `paid_until` it reads; past it, `PaymentRequired`.
    relay.set_paid_until(&account, at - 10 * DAY);
    assert!(relay.get_kit(VAULT_ID, &tok('K')).await.is_ok());
    relay.set_paid_until(&account, at - 40 * DAY);
    assert_eq!(
        relay.get_kit(VAULT_ID, &tok('K')).await,
        Err(RelayError::PaymentRequired)
    );
    // The token is judged first: a wrong one is `Unauthorized`, not `PaymentRequired`.
    assert_eq!(
        relay.get_kit(VAULT_ID, &tok('Z')).await,
        Err(RelayError::Unauthorized)
    );
}

#[tokio::test]
async fn a_rotation_with_a_kit_replaces_the_envelope_and_keeps_the_kit_token() {
    let (relay, _) = relay_with_kit().await;
    let before = relay.get_kit(VAULT_ID, &tok('K')).await.expect("get");
    relay.advance_clock(100);
    let rotated = relay
        .rotate_token_with_kit(&auth(), NEW_TOKEN, &[5], kit(&[7, 7, 7], None))
        .await
        .expect("rotate");
    assert_eq!(rotated.rotation, 1);
    assert_eq!(relay.stored_rekey(VAULT_ID, 1), Some(vec![5]));
    let after = relay.get_kit(VAULT_ID, &tok('K')).await.expect("get");
    assert_eq!(after.envelope, vec![7, 7, 7]);
    assert_eq!(after.since, before.since);
    assert!(after.updated_at >= before.updated_at + 100);
    let info = relay.vault_info(&auth_with(NEW_TOKEN)).await.expect("info");
    // `lastRead` stays: the read just made is the one recorded.
    assert_eq!(info.kit.expect("kit").since, before.since);
}

#[tokio::test]
async fn a_vault_with_a_kit_needs_the_kit_in_every_rotation() {
    let (relay, _) = relay_with_kit().await;
    assert_eq!(
        relay.rotate_token(&auth(), NEW_TOKEN, &[1]).await,
        Err(bad("kit_required"))
    );
    // Refused: nothing changed (the vault token, the bundle, the Kit).
    assert_eq!(relay.vault_info(&auth()).await.expect("info").rotation, 0);
    assert_eq!(relay.stored_rekey(VAULT_ID, 1), None);
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('K'))
            .await
            .expect("get")
            .envelope,
        vec![1, 1]
    );
    // A vault without a Kit rotates as before.
    let (without, _) = relay_with_vault().await;
    assert!(without.rotate_token(&auth(), NEW_TOKEN, &[1]).await.is_ok());
}

#[tokio::test]
async fn a_replaced_kit_reads_gone_with_the_time_and_the_new_kit_opens() {
    let (relay, _) = relay_with_kit().await;
    relay.advance_clock(1000);
    let replaced_at = now() + 1000;
    relay
        .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[8, 8], Some(&tok('N'))))
        .await
        .expect("rotate");
    let gone = relay.get_kit(VAULT_ID, &tok('K')).await;
    match gone {
        Err(RelayError::KitReplaced { replaced_at: at }) => {
            assert!((at - replaced_at).abs() < 5, "{at} vs {replaced_at}");
        }
        other => panic!("{other:?}"),
    }
    let read = relay.get_kit(VAULT_ID, &tok('N')).await.expect("new kit");
    assert_eq!(read.envelope, vec![8, 8]);
    // A new Kit: `since` is now, nothing has read it, and it is the one the info reports.
    let info = relay
        .vault_info(&auth_with(NEW_TOKEN))
        .await
        .expect("info")
        .kit
        .expect("kit");
    assert_eq!(info.since, read.since);
    assert!(read.since >= replaced_at - 5);
    // The old Kit token may not come back by rotation or by `put_kit`.
    assert_eq!(
        relay
            .rotate_token_with_kit(
                &auth_with(NEW_TOKEN),
                THIRD_TOKEN,
                &[1],
                kit(&[1], Some(&tok('K')))
            )
            .await,
        Err(bad("invalid_kit_token"))
    );
    assert_eq!(
        relay.put_kit(&auth_with(NEW_TOKEN), &tok('K'), &[1]).await,
        Err(bad("invalid_kit_token"))
    );
    // And a replaced Kit token is not a vault token to come back as either.
    assert_eq!(
        relay
            .rotate_token_with_kit(&auth_with(NEW_TOKEN), &tok('K'), &[1], kit(&[1], None))
            .await,
        Err(bad("invalid_token"))
    );
}

#[tokio::test]
async fn a_vault_without_a_kit_gets_one_by_rotation() {
    let (relay, _) = relay_with_vault().await;
    // An envelope without a Kit token would belong to no Kit.
    assert_eq!(
        relay
            .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[9], None))
            .await,
        Err(bad("invalid_kit"))
    );
    assert_eq!(relay.vault_info(&auth()).await.expect("info").rotation, 0);
    relay
        .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[9, 9], Some(&tok('K'))))
        .await
        .expect("rotate");
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('K'))
            .await
            .expect("get")
            .envelope,
        vec![9, 9]
    );
}

#[tokio::test]
async fn the_kit_token_of_a_rotation_may_not_be_a_vault_token_or_a_kit_token() {
    let (relay, _) = relay_with_kit().await;
    for token in [
        TOKEN.to_string(),     // the current vault token
        NEW_TOKEN.to_string(), // the new vault token
        tok('K'),              // the current Kit token
    ] {
        assert_eq!(
            relay
                .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[1], Some(&token)))
                .await,
            Err(bad("invalid_kit_token")),
            "{token}"
        );
    }
    // Nothing of the refused rotations stuck.
    assert_eq!(relay.vault_info(&auth()).await.expect("info").rotation, 0);
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('K'))
            .await
            .expect("get")
            .envelope,
        vec![1, 1]
    );
    // The new vault token may not be the current Kit token.
    assert_eq!(
        relay
            .rotate_token_with_kit(&auth(), &tok('K'), &[1], kit(&[1], None))
            .await,
        Err(bad("invalid_token"))
    );
}

#[tokio::test]
async fn a_rotation_with_a_kit_checks_its_arguments_and_the_cap_leaves_the_kit_alone() {
    let (relay, _) = relay_with_kit().await;
    for (envelope, token, code) in [
        (vec![], None, "invalid_kit"),
        (vec![0u8; MAX_KIT_ENVELOPE + 1], None, "kit_too_large"),
        (vec![1u8], Some("short".to_string()), "invalid_kit_token"),
    ] {
        assert_eq!(
            relay
                .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&envelope, token.as_deref()))
                .await,
            Err(bad(code)),
            "{code}"
        );
    }
    // Fill the 32 rotations of 30 days: the 33rd is refused whole, the Kit as it was.
    let mut current = TOKEN.to_string();
    for n in 0..32u32 {
        let next = numbered("v", n);
        relay
            .rotate_token_with_kit(&auth_with(&current), &next, &[1], kit(&[2, n as u8], None))
            .await
            .expect("rotate");
        current = next;
    }
    assert_eq!(
        relay
            .rotate_token_with_kit(
                &auth_with(&current),
                &tok('q'),
                &[1],
                kit(&[0xAA], Some(&tok('R')))
            )
            .await,
        Err(RelayError::TooManyRotations)
    );
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('K'))
            .await
            .expect("get")
            .envelope,
        vec![2, 31]
    );
}

#[tokio::test]
async fn a_lost_rotation_answer_stores_the_kit_with_the_rotation() {
    let (relay, _) = relay_with_kit().await;
    relay.lose_next_answer(Route::RotateToken);
    assert_eq!(
        relay
            .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[6], Some(&tok('N'))))
            .await,
        Err(RelayError::Server(503))
    );
    // Both happened or neither: the new Kit is in, and the old one is replaced.
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &tok('N'))
            .await
            .expect("get")
            .envelope,
        vec![6]
    );
    assert!(matches!(
        relay.get_kit(VAULT_ID, &tok('K')).await,
        Err(RelayError::KitReplaced { .. })
    ));
}

#[tokio::test]
async fn failed_kit_reads_are_limited_per_vault_but_the_kit_and_a_replaced_kit_never_are() {
    let (relay, _) = relay_with_kit().await;
    relay
        .rotate_token_with_kit(&auth(), NEW_TOKEN, &[1], kit(&[3], Some(&tok('N'))))
        .await
        .expect("rotate");
    for _ in 0..30 {
        assert_eq!(
            relay.get_kit(VAULT_ID, &tok('Z')).await,
            Err(RelayError::Unauthorized)
        );
    }
    // Past 30 failures an hour, more failures are `RateLimited`...
    assert_eq!(
        relay.get_kit(VAULT_ID, &tok('Z')).await,
        Err(RelayError::RateLimited)
    );
    assert_eq!(
        relay.get_kit(VAULT_ID, TOKEN).await,
        Err(RelayError::RateLimited)
    );
    // ...while the right Kit token still reads and a replaced one still hears `410`.
    assert!(relay.get_kit(VAULT_ID, &tok('N')).await.is_ok());
    assert!(matches!(
        relay.get_kit(VAULT_ID, &tok('K')).await,
        Err(RelayError::KitReplaced { .. })
    ));
    // An hour later the failures are forgotten.
    relay.advance_clock(3601);
    assert_eq!(
        relay.get_kit(VAULT_ID, &tok('Z')).await,
        Err(RelayError::Unauthorized)
    );
}

#[tokio::test]
async fn only_the_newest_256_replaced_kit_tokens_are_remembered() {
    let (relay, account) = relay_with_kit().await;
    // The clock runs for twenty years: the subscription must outlast it.
    relay.set_paid_until(&account, 253_402_300_799);
    // Each rotation replaces the Kit; the clock moves on so the 32-per-30-days cap never bites.
    let mut vault_token = TOKEN.to_string();
    let mut kit_token = tok('K');
    let mut first_replaced = None;
    for n in 0..258u32 {
        relay.advance_clock(31 * DAY);
        let next_vault = numbered("v", n);
        let next_kit = numbered("k", n);
        relay
            .rotate_token_with_kit(
                &auth_with(&vault_token),
                &next_vault,
                &[1],
                kit(&[1], Some(&next_kit)),
            )
            .await
            .expect("rotate");
        first_replaced.get_or_insert_with(|| kit_token.clone());
        vault_token = next_vault;
        kit_token = next_kit;
    }
    // 258 replacements: the first two are forgotten (`401`), the third still reads `410`.
    assert_eq!(
        relay
            .get_kit(VAULT_ID, &first_replaced.expect("first"))
            .await,
        Err(RelayError::Unauthorized)
    );
    assert!(relay.get_kit(VAULT_ID, &kit_token).await.is_ok());
    assert!(matches!(
        relay.get_kit(VAULT_ID, &numbered("k", 255)).await,
        Err(RelayError::KitReplaced { .. })
    ));
}

#[tokio::test]
async fn the_server_says_whether_it_has_the_kit() {
    let relay = FakeRelay::new();
    assert!(relay.server_info().await.expect("info").kit);
    relay.set_server_info(Some(ServerInfo {
        version: "1.0.0".into(),
        protocol: 3,
        mode: ServerMode::SelfHosted,
        blobs: true,
        kit: false,
    }));
    assert!(!relay.server_info().await.expect("info").kit);
}

#[tokio::test]
async fn deleting_the_vault_deletes_the_kit() {
    let (relay, _) = relay_with_kit().await;
    relay.delete_vault(&auth()).await.expect("delete");
    assert_eq!(
        relay.get_kit(VAULT_ID, &tok('K')).await,
        Err(RelayError::VaultNotFound)
    );
}
