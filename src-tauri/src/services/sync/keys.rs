//! The vault's credentials and this device's X25519 key pair, kept in the `sync_state` row
//! (spec §11.1, ruling P3): `vault_id` as base64url, `key_epoch` the current epoch, `vault_keys`
//! the JSON `{"<epoch>": "<base64url key>"}` of every epoch still needed, `device_secret_key` the
//! 32 secret bytes. None of the secret-bearing types derives `Debug`: each prints `<redacted>`
//! (ruling P16), and key JSON and tokens live in `Zeroizing` buffers.

use std::collections::BTreeMap;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::SysRng;
use rand::TryRng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

use super::envelope::vault_token;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::registry::keys::{barrier_number, schema_number};
use crate::error::{AppError, Result};

/// Fills `buf` from the operating system's random source (not a user-space generator): vault
/// keys, vault IDs and the device's X25519 secret come from here.
fn os_random(buf: &mut [u8]) -> Result<()> {
    SysRng
        .try_fill_bytes(buf)
        .map_err(|e| AppError::Internal(format!("sync: the system random source failed: {e}")))
}

/// The vault's routing ID: 16 random bytes, stored and sent as base64url without padding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VaultId(pub [u8; 16]);

impl VaultId {
    /// 16 bytes from the operating system's random source.
    pub fn random() -> Result<Self> {
        let mut bytes = [0u8; 16];
        os_random(&mut bytes)?;
        Ok(VaultId(bytes))
    }

    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_b64(s: &str) -> Result<Self> {
        let invalid = || AppError::Internal("sync: the vault ID is not 16 base64url bytes".into());
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| invalid())?;
        Ok(VaultId(bytes.try_into().map_err(|_| invalid())?))
    }
}

impl fmt::Debug for VaultId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultId(<redacted>)")
    }
}

/// Every vault key this device still needs, by epoch, and the current epoch, which always has a
/// key.
#[derive(Clone)]
pub struct VaultKeys {
    current: u32,
    keys: BTreeMap<u32, Zeroizing<[u8; 32]>>,
}

impl VaultKeys {
    /// A new vault's single key, epoch 0, from the operating system's random source.
    pub fn new_random() -> Result<Self> {
        let mut key = Zeroizing::new([0u8; 32]);
        os_random(&mut key[..])?;
        Ok(VaultKeys {
            current: 0,
            keys: BTreeMap::from([(0, key)]),
        })
    }

    /// Takes the keys over and wipes the caller's copies. The current epoch must have a key.
    pub fn from_parts(current: u32, mut keys: BTreeMap<u32, [u8; 32]>) -> Result<Self> {
        let owned = keys
            .iter()
            .map(|(epoch, key)| (*epoch, Zeroizing::new(*key)))
            .collect();
        keys.values_mut().for_each(Zeroize::zeroize);
        Self::checked(current, owned)
    }

    fn checked(current: u32, keys: BTreeMap<u32, Zeroizing<[u8; 32]>>) -> Result<Self> {
        if !keys.contains_key(&current) {
            return Err(AppError::Internal(format!(
                "sync: no vault key for the current epoch {current}"
            )));
        }
        Ok(VaultKeys { current, keys })
    }

    pub fn current_epoch(&self) -> u32 {
        self.current
    }

    pub fn current(&self) -> &[u8; 32] {
        // Every constructor goes through `checked` or inserts the current epoch itself.
        &self.keys[&self.current]
    }

    pub fn get(&self, epoch: u32) -> Option<Zeroizing<[u8; 32]>> {
        self.keys.get(&epoch).map(|key| Zeroizing::new(**key))
    }

    /// `{"<epoch>":"<base64url key>",…}` in ascending epoch order, built in one wiped buffer.
    fn to_json(&self) -> Zeroizing<String> {
        // `"4294967295":"<43 characters>",` is 59 bytes; reserving up front keeps the buffer
        // from reallocating and leaving an unwiped copy behind.
        let mut json = Zeroizing::new(String::with_capacity(2 + 64 * self.keys.len()));
        json.push('{');
        for (i, (epoch, key)) in self.keys.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json.push_str(&epoch.to_string());
            json.push_str("\":\"");
            URL_SAFE_NO_PAD.encode_string(key.as_slice(), &mut json);
            json.push('"');
        }
        json.push('}');
        json
    }

    fn from_json(current: u32, json: &str) -> Result<Self> {
        let corrupt = || AppError::Internal("sync_state.vault_keys is not valid".into());
        // The serde error may quote the value, so it is dropped.
        let entries: Entries = serde_json::from_str(json).map_err(|_| corrupt())?;
        let mut keys = BTreeMap::new();
        for (name, encoded) in entries.0 {
            // Only the spelling `to_json` writes: no "01", "+1" or " 1".
            let epoch: u32 = name.parse().map_err(|_| corrupt())?;
            if epoch.to_string() != name {
                return Err(corrupt());
            }
            let decoded = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(encoded.0.as_bytes())
                    .map_err(|_| corrupt())?,
            );
            if decoded.len() != 32 {
                return Err(corrupt());
            }
            let mut key = Zeroizing::new([0u8; 32]);
            key.copy_from_slice(&decoded);
            // A repeated member is refused, not resolved to the last one.
            if keys.insert(epoch, key).is_some() {
                return Err(corrupt());
            }
        }
        Self::checked(current, keys)
    }
}

impl fmt::Debug for VaultKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultKeys")
            .field("current", &self.current)
            .field("keys", &format_args!("<redacted>"))
            .finish()
    }
}

/// The members of the key JSON object in document order, repeated names included (a
/// `BTreeMap` would silently keep the last one).
struct Entries(Vec<(String, SecretString)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct EntriesVisitor;
        impl<'de> Visitor<'de> for EntriesVisitor {
            type Value = Entries;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of epochs")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Entries, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, SecretString>()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}

/// A JSON string read straight into a wiped buffer.
struct SecretString(Zeroizing<String>);

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct SecretVisitor;
        impl Visitor<'_> for SecretVisitor {
            type Value = SecretString;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<SecretString, E> {
                Ok(SecretString(Zeroizing::new(v.to_owned())))
            }
        }
        deserializer.deserialize_str(SecretVisitor)
    }
}

/// What the relay client authenticates a vault call with: the vault ID (base64url) and the bearer
/// token.
#[derive(Clone)]
pub struct VaultAuth {
    pub vault_id: String,
    pub token: Zeroizing<String>,
}

impl fmt::Debug for VaultAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultAuth(<redacted>)")
    }
}

/// Everything a device needs to reach and read the vault.
#[derive(Clone)]
pub struct Credentials {
    pub relay_url: String,
    pub account: String,
    pub vault_id: VaultId,
    pub keys: VaultKeys,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("relay_url", &format_args!("{}", origin_of(&self.relay_url)))
            .field("account", &format_args!("<redacted>"))
            .field("vault_id", &self.vault_id)
            .field("keys", &self.keys)
            .finish()
    }
}

/// `scheme://host` of `url`, without user info, port, path, query or fragment (ruling P16: a URL
/// in a log shows no more than its origin).
fn origin_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<redacted>".into();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(v6) => v6
            .split(']')
            .next()
            .map(|h| format!("[{h}]"))
            .unwrap_or_default(),
        None => host_port.split(':').next().unwrap_or_default().to_string(),
    };
    format!("{scheme}://{host}")
}

impl Credentials {
    /// The relay token of the current epoch (spec §11.1).
    pub fn token(&self) -> Zeroizing<String> {
        Zeroizing::new(vault_token(self.keys.current(), &self.vault_id.0))
    }

    /// The one place a [`VaultAuth`] is built.
    pub fn auth(&self) -> VaultAuth {
        VaultAuth {
            vault_id: self.vault_id.to_b64(),
            token: self.token(),
        }
    }
}

fn not_set_up() -> AppError {
    AppError::Internal("sync: sync is not set up on this device".into())
}

/// Creates the `sync_state` row, sync off, with a random device ID and today's schema and
/// barrier (as `state::ensure_row` would). A row that is still off is brought up to this build:
/// nothing crosses a barrier for it while it is off (`open_capture` skips it), so a build with a
/// higher barrier would otherwise leave it admitting snapshots from below that barrier. An
/// enabled row is left alone.
fn ensure_disabled_row(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (id, enabled, device_id, schema_seen, barrier) \
         VALUES (1, 0, ?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET schema_seen = excluded.schema_seen, \
           barrier = MAX(barrier, excluded.barrier) WHERE enabled = 0",
        params![
            DeviceId::random().0.to_vec(),
            schema_number(),
            barrier_number()
        ],
    )?;
    Ok(())
}

/// Stores the relay and the account number; creates the row with sync off and a random device ID
/// when absent. An existing row keeps its device and its enabled flag.
pub fn save_account(conn: &Connection, relay_url: &str, account: &str) -> Result<()> {
    in_savepoint(conn, "sync_save_account", || {
        ensure_disabled_row(conn)?;
        conn.execute(
            "UPDATE sync_state SET relay_url = ?1, account_number = ?2 WHERE id = 1",
            params![relay_url, account],
        )?;
        Ok(())
    })
}

/// `(relay_url, account)`, or `None` before an account is saved.
pub fn account(conn: &Connection) -> Result<Option<(String, String)>> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT relay_url, account_number FROM sync_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((Some(relay_url), Some(account))) => Some((relay_url, account)),
        _ => None,
    })
}

/// The vault ID and keys, generated once for the first device of a vault. Needs the account.
pub fn ensure_vault_secrets(conn: &Connection) -> Result<(VaultId, VaultKeys)> {
    in_savepoint(conn, "sync_vault_secrets", || {
        if account(conn)?.is_none() {
            return Err(AppError::Internal(
                "sync: a vault needs an account first".into(),
            ));
        }
        if let Some(secrets) = vault_secrets(conn)? {
            return Ok(secrets);
        }
        let secrets = (VaultId::random()?, VaultKeys::new_random()?);
        write_vault_secrets(conn, &secrets.0, &secrets.1)?;
        Ok(secrets)
    })
}

fn vault_secrets(conn: &Connection) -> Result<Option<(VaultId, VaultKeys)>> {
    let row: Option<(Option<String>, Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT vault_id, key_epoch, vault_keys FROM sync_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match row {
        None | Some((None, None, None)) => Ok(None),
        Some((Some(vault_id), Some(epoch), Some(json))) => {
            let json = Zeroizing::new(json);
            let epoch = u32::try_from(epoch).map_err(|_| {
                AppError::Internal(format!("sync_state.key_epoch is out of range: {epoch}"))
            })?;
            Ok(Some((
                VaultId::from_b64(&vault_id)?,
                VaultKeys::from_json(epoch, &json)?,
            )))
        }
        Some(_) => Err(AppError::Internal(
            "sync_state holds part of the vault secrets".into(),
        )),
    }
}

/// Binds the row to `vault_id` with `keys`. Binding another vault than the stored one forgets the
/// bookkeeping that belongs to the old vault: its relay positions (the barrier marker and a
/// below-barrier miss; a stale miss would otherwise force a restore in the new vault) and the
/// barrier a marker was sealed for (the new vault needs its own marker).
fn write_vault_secrets(conn: &Connection, vault_id: &VaultId, keys: &VaultKeys) -> Result<()> {
    let json = keys.to_json();
    conn.execute(
        "UPDATE sync_state SET \
           missed_below_barrier = CASE WHEN vault_id IS ?1 THEN missed_below_barrier END, \
           barrier_seq = CASE WHEN vault_id IS ?1 THEN barrier_seq END, \
           marker_barrier = CASE WHEN vault_id IS ?1 THEN marker_barrier ELSE 0 END, \
           vault_id = ?1, key_epoch = ?2, vault_keys = ?3 WHERE id = 1",
        params![vault_id.to_b64(), keys.current_epoch(), json.as_str()],
    )?;
    Ok(())
}

/// Stores credentials received when joining a vault; creates the row with sync off and a random
/// device ID when absent. An existing row keeps its device and its enabled flag.
pub fn save_credentials(conn: &Connection, creds: &Credentials) -> Result<()> {
    in_savepoint(conn, "sync_save_credentials", || {
        ensure_disabled_row(conn)?;
        conn.execute(
            "UPDATE sync_state SET relay_url = ?1, account_number = ?2 WHERE id = 1",
            params![creds.relay_url, creds.account],
        )?;
        write_vault_secrets(conn, &creds.vault_id, &creds.keys)
    })
}

/// The stored credentials, or `None` until both the account and the vault secrets are there.
pub fn credentials(conn: &Connection) -> Result<Option<Credentials>> {
    let Some((relay_url, account)) = account(conn)? else {
        return Ok(None);
    };
    Ok(vault_secrets(conn)?.map(|(vault_id, keys)| Credentials {
        relay_url,
        account,
        vault_id,
        keys,
    }))
}

/// This device's X25519 public key, generating the key pair the first time. The write only
/// fills an empty column and the answer is read back, so two callers agree on one pair.
pub fn ensure_device_keypair(conn: &Connection) -> Result<[u8; 32]> {
    if let Some(public) = stored_public_key(conn)? {
        return Ok(public);
    }
    let secret = random_secret()?;
    conn.execute(
        "UPDATE sync_state SET device_secret_key = ?1 WHERE id = 1 AND device_secret_key IS NULL",
        [secret.as_bytes().as_slice()],
    )?;
    stored_public_key(conn)?.ok_or_else(not_set_up)
}

/// Replaces this device's key pair (a re-join) and returns the new public key.
pub fn new_device_keypair(conn: &Connection) -> Result<[u8; 32]> {
    let secret = random_secret()?;
    let changed = conn.execute(
        "UPDATE sync_state SET device_secret_key = ?1 WHERE id = 1",
        [secret.as_bytes().as_slice()],
    )?;
    if changed == 0 {
        return Err(not_set_up());
    }
    Ok(PublicKey::from(&secret).to_bytes())
}

/// The public key of the stored secret: `None` while the column is empty, an error without the
/// row.
fn stored_public_key(conn: &Connection) -> Result<Option<[u8; 32]>> {
    let stored: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT device_secret_key FROM sync_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(stored) = stored else {
        return Err(not_set_up());
    };
    stored
        .map(|bytes| {
            let bytes = Zeroizing::new(bytes);
            if bytes.len() != 32 {
                return Err(AppError::Internal(
                    "sync_state.device_secret_key is not 32 bytes".into(),
                ));
            }
            let mut secret = Zeroizing::new([0u8; 32]);
            secret.copy_from_slice(&bytes);
            Ok(public_key_of(&secret))
        })
        .transpose()
}

/// A new X25519 secret from the operating system's random source (wiped when it drops).
fn random_secret() -> Result<StaticSecret> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    os_random(&mut bytes[..])?;
    Ok(StaticSecret::from(*bytes))
}

/// The X25519 public key of `secret` (the secret is wiped when the `StaticSecret` drops).
fn public_key_of(secret: &[u8; 32]) -> [u8; 32] {
    PublicKey::from(&StaticSecret::from(*secret)).to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::envelope::vault_token;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::registry::keys::{barrier_number, schema_number};
    use crate::services::sync::test_support::migrated;
    use crate::services::sync::{capture, on_open, state};

    const RELAY: &str = "https://relay.example.org";
    const ACCOUNT: &str = "4417938205561273";

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    fn two_epochs() -> Credentials {
        Credentials {
            relay_url: RELAY.into(),
            account: ACCOUNT.into(),
            vault_id: VaultId([0x22; 16]),
            keys: VaultKeys::from_parts(1, BTreeMap::from([(0, [0x01; 32]), (1, [0x11; 32])]))
                .expect("keys"),
        }
    }

    #[test]
    fn save_account_creates_a_disabled_row_and_never_changes_the_device() {
        let conn = migrated();
        assert_eq!(account(&conn).unwrap(), None);

        save_account(&conn, RELAY, ACCOUNT).unwrap();

        let row = state::load(&conn).unwrap().expect("row");
        assert!(!row.enabled);
        assert_ne!(row.device, DeviceId::ZERO);
        assert_eq!(row.schema_seen, schema_number());
        assert_eq!(row.barrier, barrier_number());
        assert_eq!(
            account(&conn).unwrap(),
            Some((RELAY.to_string(), ACCOUNT.to_string()))
        );

        save_account(&conn, "https://other.example.org", "9000000000000001").unwrap();
        let again = state::load(&conn).unwrap().expect("row");
        assert_eq!(again.device, row.device);
        assert!(!again.enabled);
        assert_eq!(
            account(&conn).unwrap(),
            Some((
                "https://other.example.org".to_string(),
                "9000000000000001".to_string()
            ))
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_state"), 1);
        assert_eq!(credentials(&conn).unwrap().map(|c| c.account), None);
    }

    #[test]
    fn a_disabled_row_is_brought_up_to_this_builds_barrier() {
        let conn = migrated();
        save_account(&conn, RELAY, ACCOUNT).unwrap();
        // The account was saved by an older build.
        conn.execute(
            "UPDATE sync_state SET barrier = ?1, schema_seen = 1",
            [barrier_number() - 1],
        )
        .unwrap();

        save_account(&conn, RELAY, ACCOUNT).unwrap();

        let row = state::load(&conn).unwrap().expect("row");
        assert_eq!(row.barrier, barrier_number());
        assert_eq!(row.schema_seen, schema_number());
        assert!(!row.enabled);

        conn.execute("UPDATE sync_state SET barrier = 1", [])
            .unwrap();
        save_credentials(&conn, &two_epochs()).unwrap();
        assert_eq!(
            state::load(&conn).unwrap().expect("row").barrier,
            barrier_number(),
            "the join path refreshes it too"
        );

        state::ensure_row(&conn, row.device).unwrap();
        conn.execute("UPDATE sync_state SET barrier = 1, schema_seen = 2", [])
            .unwrap();
        save_account(&conn, RELAY, ACCOUNT).unwrap();
        let enabled = state::load(&conn).unwrap().expect("row");
        assert_eq!(
            (enabled.barrier, enabled.schema_seen),
            (1, 2),
            "an enabled row is left to cross_barrier"
        );
    }

    #[test]
    fn a_disabled_row_installs_no_capture_and_queues_nothing() {
        let conn = migrated();
        save_account(&conn, RELAY, ACCOUNT).unwrap();

        on_open(&conn);

        assert!(!capture::is_installed(&conn).unwrap());
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'trigger'"
            ),
            0
        );
        conn.execute_batch("INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')")
            .unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_clock"), 0);
        let marks = state::marks(&conn).unwrap().expect("row");
        assert_eq!(marks.capture_broken_at, None);
        assert_eq!(marks.last_error, None);
    }

    #[test]
    fn random_secrets_come_from_the_os_and_differ() {
        let a = VaultKeys::new_random().unwrap();
        let b = VaultKeys::new_random().unwrap();
        assert_ne!(a.current(), b.current());
        assert_ne!(a.current(), &[0u8; 32]);
        assert_ne!(VaultId::random().unwrap(), VaultId::random().unwrap());
    }

    #[test]
    fn ensure_vault_secrets_is_generated_once() {
        let conn = migrated();
        assert!(ensure_vault_secrets(&conn).is_err(), "no account, no vault");
        save_account(&conn, RELAY, ACCOUNT).unwrap();

        let (id, keys) = ensure_vault_secrets(&conn).unwrap();
        let (id_again, keys_again) = ensure_vault_secrets(&conn).unwrap();

        assert_eq!(id, id_again);
        assert_eq!(keys.current_epoch(), 0);
        assert_eq!(keys_again.current_epoch(), 0);
        assert_eq!(keys.current(), keys_again.current());
        assert_ne!(keys.current(), &[0u8; 32]);
        let (stored_id, epoch): (String, i64) = conn
            .query_row("SELECT vault_id, key_epoch FROM sync_state", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(stored_id, id.to_b64());
        assert_eq!(stored_id.len(), 22);
        assert_eq!(epoch, 0);
        let creds = credentials(&conn).unwrap().expect("credentials");
        assert_eq!(creds.vault_id, id);
        assert_eq!(creds.keys.current(), keys.current());
        assert_eq!(creds.account, ACCOUNT);
    }

    #[test]
    fn credentials_round_trip_through_sync_state() {
        let conn = migrated();
        let creds = two_epochs();

        save_credentials(&conn, &creds).unwrap();

        let row = state::load(&conn).unwrap().expect("row");
        assert!(
            !row.enabled,
            "joining stores credentials; enabling is separate"
        );
        let back = credentials(&conn).unwrap().expect("credentials");
        assert_eq!(back.relay_url, RELAY);
        assert_eq!(back.account, ACCOUNT);
        assert_eq!(back.vault_id, VaultId([0x22; 16]));
        assert_eq!(back.keys.current_epoch(), 1);
        assert_eq!(back.keys.current(), &[0x11; 32]);
        assert_eq!(back.keys.get(0).as_deref(), Some(&[0x01; 32]));
        assert_eq!(back.keys.get(2), None);

        state::ensure_row(&conn, DeviceId([7; 16])).unwrap();
        save_credentials(&conn, &creds).unwrap();
        let kept = state::load(&conn).unwrap().expect("row");
        assert_eq!(kept.device, DeviceId([7; 16]), "the device stays");
        assert!(kept.enabled, "the enabled flag stays");
    }

    #[test]
    fn binding_another_vault_forgets_the_old_vaults_relay_positions() {
        let marks = |conn: &Connection| {
            let row = state::load(conn).unwrap().expect("row");
            let sealed = state::marks(conn).unwrap().expect("marks").marker_barrier;
            (row.missed_below_barrier, row.barrier_seq, sealed)
        };
        let conn = migrated();
        save_credentials(&conn, &two_epochs()).unwrap();
        state::note_missed_below_barrier(&conn, 7).unwrap();
        state::set_barrier_seq(&conn, Some(5)).unwrap();
        state::note_sealed_at_barrier(&conn, 25).unwrap();

        // The same vault again keeps them.
        save_credentials(&conn, &two_epochs()).unwrap();
        assert_eq!(marks(&conn), (Some(7), Some(5), 25));

        // Joining another vault forgets them.
        let mut other = two_epochs();
        other.vault_id = VaultId([0x33; 16]);
        save_credentials(&conn, &other).unwrap();
        assert_eq!(marks(&conn), (None, None, 0));

        // So does a first device's new vault.
        let first = migrated();
        save_account(&first, RELAY, ACCOUNT).unwrap();
        state::note_missed_below_barrier(&first, 9).unwrap();
        state::note_sealed_at_barrier(&first, 25).unwrap();
        ensure_vault_secrets(&first).unwrap();
        assert_eq!(marks(&first), (None, None, 0));
    }

    #[test]
    fn vault_keys_json_keeps_every_epoch() {
        let conn = migrated();
        let keys = VaultKeys::from_parts(
            2,
            BTreeMap::from([(0, [0x01; 32]), (1, [0x02; 32]), (2, [0x03; 32])]),
        )
        .unwrap();
        let creds = Credentials {
            keys,
            ..two_epochs()
        };
        save_credentials(&conn, &creds).unwrap();

        let (json, epoch): (String, i64) = conn
            .query_row("SELECT vault_keys, key_epoch FROM sync_state", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(
            json,
            r#"{"0":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE","1":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI","2":"AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM"}"#
        );
        assert_eq!(epoch, 2);
        let back = credentials(&conn).unwrap().expect("credentials").keys;
        assert_eq!(back.current_epoch(), 2);
        assert_eq!(back.get(0).as_deref(), Some(&[0x01; 32]));
        assert_eq!(back.get(1).as_deref(), Some(&[0x02; 32]));
        assert_eq!(back.get(2).as_deref(), Some(&[0x03; 32]));

        assert!(
            VaultKeys::from_parts(3, BTreeMap::from([(0, [0x01; 32])])).is_err(),
            "the current epoch needs a key"
        );
        const K: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE";
        conn.execute(
            "UPDATE sync_state SET vault_keys = ?1",
            [format!(r#"{{"2":"{K}"}}"#)],
        )
        .unwrap();
        assert!(credentials(&conn).is_ok(), "the canonical spelling reads");
        for broken in [
            r#"{"0":"AQEB"}"#.to_string(),
            format!(r#"{{"x":"{K}"}}"#),
            format!(r#"{{"1":"{K}"}}"#),
            format!(r#"{{"02":"{K}"}}"#),
            format!(r#"{{"+2":"{K}"}}"#),
            format!(r#"{{" 2":"{K}"}}"#),
            format!(r#"{{"2":"{K}","2":"{K}"}}"#),
            format!(r#"{{"2":"{K}","1":"{K}","2":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI"}}"#),
            "not json".to_string(),
        ] {
            conn.execute("UPDATE sync_state SET vault_keys = ?1", [&broken])
                .unwrap();
            let error = credentials(&conn).expect_err(&broken).to_string();
            assert!(!error.contains(K), "the error never quotes a key: {error}");
        }
    }

    #[test]
    fn the_token_is_43_base64url_characters_and_matches_the_envelope_derivation() {
        let creds = two_epochs();

        let token = creds.token();

        // The frozen vector of envelope::tests (key 0x11.., vault 0x22..).
        assert_eq!(
            token.as_str(),
            "xEoN5KY965vOsERjjinYPzJYkS-KXnBEs1SCP_enuvQ"
        );
        assert_eq!(token.len(), 43);
        assert!(token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(*token, vault_token(&[0x11; 32], &[0x22; 16]));
        let auth = creds.auth();
        assert_eq!(auth.vault_id, "IiIiIiIiIiIiIiIiIiIiIg");
        assert_eq!(*auth.token, *token);
        assert_eq!(
            VaultId::from_b64("IiIiIiIiIiIiIiIiIiIiIg").unwrap(),
            creds.vault_id
        );
        assert!(VaultId::from_b64("IiIi").is_err());
        assert!(VaultId::from_b64("IiIiIiIiIiIiIiIiIiIiIg==").is_err());
    }

    #[test]
    fn debug_output_of_secrets_is_redacted() {
        let creds = Credentials {
            keys: VaultKeys::from_parts(0, BTreeMap::from([(0, [0x5a; 32])])).unwrap(),
            ..two_epochs()
        };
        let token = creds.token();
        let auth = creds.auth();
        let creds = Credentials {
            relay_url: "https://user:pw@relay.example.org:8443/v1/hidden-path?q=1#f".into(),
            ..creds
        };
        let shown = format!("{creds:?}");
        assert!(
            shown.contains(r#"relay_url: https://relay.example.org,"#),
            "{shown}"
        );
        for part in ["user", "pw", "8443", "hidden-path", "q=1"] {
            assert!(!shown.contains(part), "{shown} shows {part}");
        }
        let outputs = [
            shown,
            format!("{:?}", creds.vault_id),
            format!("{:?}", creds.keys),
            format!("{auth:?}"),
        ];
        let forbidden = [
            ACCOUNT.to_string(),
            hex::encode([0x5a; 32]),
            "5a5a".to_string(),
            "90, 90".to_string(),
            "WlpaWlpa".to_string(),
            token.to_string(),
            creds.vault_id.to_b64(),
            hex::encode(creds.vault_id.0),
            "34, 34".to_string(),
        ];
        for output in &outputs {
            assert!(output.contains("<redacted>"), "{output}");
            for secret in &forbidden {
                assert!(!output.contains(secret.as_str()), "{output} shows {secret}");
            }
        }
    }

    #[test]
    fn the_relay_origin_keeps_only_scheme_and_host() {
        assert_eq!(
            origin_of("https://relay.example.org"),
            "https://relay.example.org"
        );
        assert_eq!(
            origin_of("https://relay.example.org/"),
            "https://relay.example.org"
        );
        assert_eq!(origin_of("http://[::1]:8787/v1"), "http://[::1]");
        assert_eq!(origin_of("https://a@b.example?x"), "https://b.example");
        assert_eq!(origin_of("relay.example.org/path"), "<redacted>");
    }

    #[test]
    fn device_keypair_is_stable_until_renewed() {
        let conn = migrated();
        assert!(ensure_device_keypair(&conn).is_err(), "no row, no key pair");
        save_account(&conn, RELAY, ACCOUNT).unwrap();

        let first = ensure_device_keypair(&conn).unwrap();
        assert_eq!(ensure_device_keypair(&conn).unwrap(), first);
        let stored: Vec<u8> = conn
            .query_row("SELECT device_secret_key FROM sync_state", [], |r| r.get(0))
            .unwrap();
        let stored: [u8; 32] = stored.try_into().expect("32 bytes");
        assert_eq!(public_key_of(&stored), first);

        let renewed = new_device_keypair(&conn).unwrap();
        assert_ne!(renewed, first);
        assert_eq!(ensure_device_keypair(&conn).unwrap(), renewed);

        // A pair another caller stored first is the one returned (RFC 7748 §6.1, Alice).
        conn.execute(
            "UPDATE sync_state SET device_secret_key = ?1",
            [
                hex::decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
                    .unwrap(),
            ],
        )
        .unwrap();
        assert_eq!(
            hex::encode(ensure_device_keypair(&conn).unwrap()),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
    }

    #[test]
    fn the_public_key_is_the_x25519_base_point_multiple() {
        // RFC 7748 §6.1, Alice.
        let secret: [u8; 32] =
            hex::decode("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a")
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(
            hex::encode(public_key_of(&secret)),
            "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
        );
    }

    #[test]
    fn disable_forgets_the_account_and_every_secret() {
        let conn = migrated();
        save_account(&conn, RELAY, ACCOUNT).unwrap();
        ensure_vault_secrets(&conn).unwrap();
        ensure_device_keypair(&conn).unwrap();

        state::disable(&conn).unwrap();

        assert_eq!(account(&conn).unwrap(), None);
        assert!(credentials(&conn).unwrap().is_none());
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_state"), 0);
    }
}
