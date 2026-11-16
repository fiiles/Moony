//! What a holder seals into slot 2 (protocol 3, phase 5 spec §5.3, §11.2), read from its profile:
//! the vault's credentials, the rotation head of each key epoch it knows one for, and, when its
//! own certificate is pinned, its identity: its whole trust store and a fresh one-time join
//! ticket. A vault without certificates (§5.5) sends `identity: null`. The ticket's secret lives
//! in the returned payload only; the caller drops it right after sealing.

use std::collections::BTreeMap;

use rusqlite::Connection;

use super::session::{PairingSession, Stage};
use super::{typed_code_digits, PairingIdentity, Role, SlotPayload};
use crate::error::{AppError, Result};
use crate::services::sync::engine::{self, ConnAccess};
use crate::services::sync::error::SyncResult;
use crate::services::sync::identity::{self, cert, trust};
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::relay::Mailbox;
use crate::services::sync::state;

const FAILED: &str = "validation.syncPairingFailed";

/// The person confirmed on this device (`PairingSession::confirm_with`). For a holder at Compare,
/// not contested, with typed text that can be a code, the slot-2 payload is read first holding
/// off every round of the profile (`engine::exclusive`, released before any relay call), so it is
/// one consistent state; sync off or missing secrets answer `validation.syncPairingFailed`. The
/// payload, and with it the join ticket's secret, is a local of this call: it is dropped (wiped)
/// on every path, sealed or not — a wrong code, a slot write that failed, an expired session.
pub async fn confirm<M: Mailbox, D: ConnAccess>(
    session: &mut PairingSession,
    mailbox: &M,
    db: &D,
    typed: Option<&str>,
    now: i64,
) -> SyncResult<Stage> {
    let seals = session.role() == Role::Holder
        && session.stage() == Stage::Compare
        && !session.contested()
        && typed.is_some_and(|typed| typed_code_digits(typed).is_some());
    let payload = if seals {
        let _no_round = engine::exclusive(db).await;
        match db.with_conn(|conn| holder_payload(conn, now))? {
            Some(payload) => Some(payload),
            None => return Err(AppError::Validation(FAILED.into()).into()),
        }
    } else {
        None
    };
    let confirmed = session
        .confirm_with(mailbox, payload.as_ref(), typed, now)
        .await;
    drop(payload);
    confirmed
}

/// The holder's slot-2 payload, `None` while sync is off or the vault's secrets are missing.
pub fn holder_payload(conn: &Connection, now: i64) -> Result<Option<SlotPayload>> {
    let Some(sync) = state::load(conn)?.filter(|sync| sync.enabled) else {
        return Ok(None);
    };
    let Some(creds) = keys::credentials(conn)? else {
        return Ok(None);
    };
    let heads = epoch_heads(conn, &creds)?;
    let identity = if trust::is_supported(conn)? {
        match identity::own_seed(conn)? {
            Some(seed) => {
                let (ticket_secret, ticket) =
                    cert::issue_ticket(&seed, creds.vault_id.0, sync.device, now)?;
                Some(PairingIdentity {
                    holder: sync.device,
                    export: trust::export(conn)?,
                    ticket_secret,
                    ticket,
                })
            }
            None => {
                log::warn!("[SYNC] This device's certificate is pinned but its key is missing");
                None
            }
        }
    } else {
        None
    };
    Ok(Some(SlotPayload {
        creds,
        heads,
        identity,
    }))
}

/// The rotation head of every epoch of `creds` that `sync_epochs` records; epoch 0 starts at 0
/// (spec §6.1) whether recorded or not.
fn epoch_heads(conn: &Connection, creds: &Credentials) -> Result<BTreeMap<u32, u64>> {
    let mut stmt = conn.prepare("SELECT epoch, rotation_head FROM sync_epochs")?;
    let recorded = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut heads = BTreeMap::new();
    for (epoch, head) in recorded {
        if let (Ok(epoch), Ok(head)) = (u32::try_from(epoch), u64::try_from(head)) {
            heads.insert(epoch, head);
        }
    }
    heads.entry(0).or_insert(0);
    heads.retain(|epoch, _| creds.keys.get(*epoch).is_some());
    Ok(heads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::identity::{create_own, sign_pub, CertKind, Issuer};
    use crate::services::sync::keys::{VaultId, VaultKeys};
    use crate::services::sync::test_support::migrated;

    const ME: DeviceId = DeviceId([0x01; 16]);
    const NOW: i64 = 1_794_816_000;

    fn holder() -> (Connection, Credentials) {
        let conn = migrated();
        let creds = Credentials {
            relay_url: "https://relay.test".into(),
            account: "1234567890123456".into(),
            vault_id: VaultId([0x33; 16]),
            keys: VaultKeys::new_random().expect("keys"),
        };
        keys::save_credentials(&conn, &creds).expect("credentials");
        state::enable(&conn, ME).expect("enable");
        (conn, creds)
    }

    #[test]
    fn a_holder_with_sync_off_has_nothing_to_send() {
        assert!(holder_payload(&migrated(), NOW).expect("payload").is_none());
    }

    #[test]
    fn a_holder_without_a_certificate_sends_no_identity() {
        let (conn, creds) = holder();
        identity::ensure_seed(&conn).expect("seed");
        let payload = holder_payload(&conn, NOW)
            .expect("payload")
            .expect("sync is on");
        assert!(payload.identity.is_none());
        assert_eq!(payload.creds.vault_id, creds.vault_id);
        assert_eq!(payload.heads, BTreeMap::from([(0, 0)]));
    }

    #[test]
    fn a_holder_vouches_with_its_store_and_a_fresh_ticket() {
        let (conn, creds) = holder();
        let own = create_own(&conn, Issuer::Genesis, NOW).expect("genesis");

        let first = holder_payload(&conn, NOW)
            .expect("payload")
            .expect("sync is on");
        let second = holder_payload(&conn, NOW + 1)
            .expect("payload")
            .expect("sync is on");

        let identity = first.identity.expect("an identity");
        assert_eq!(identity.holder, ME);
        assert_eq!(identity.export, trust::export(&conn).expect("export"));
        assert_eq!(identity.export.entries.len(), 1);
        identity
            .ticket
            .verify(&creds.vault_id.0, ME, &own.body.sign_pub)
            .expect("signed by the holder");
        assert_eq!(
            identity.ticket.ticket_pub,
            sign_pub(identity.ticket_secret.as_seed())
        );
        assert_ne!(
            second.identity.expect("an identity").ticket.ticket_pub,
            identity.ticket.ticket_pub,
            "a ticket is made for each pairing"
        );
        assert_eq!(own.body.kind, CertKind::Genesis);
    }

    #[test]
    fn the_heads_are_those_of_the_holders_epochs() {
        let (conn, creds) = holder();
        conn.execute_batch(
            "INSERT OR IGNORE INTO sync_epochs (epoch, rotation_head, adopted_at) VALUES (0, 0, 1);
             INSERT INTO sync_epochs (epoch, rotation_head, adopted_at) VALUES (7, 90, 1);",
        )
        .expect("epochs");
        let payload = holder_payload(&conn, NOW)
            .expect("payload")
            .expect("sync is on");
        assert_eq!(creds.keys.current_epoch(), 0);
        assert_eq!(
            payload.heads,
            BTreeMap::from([(0, 0)]),
            "only the epochs whose keys go along"
        );
    }
}
