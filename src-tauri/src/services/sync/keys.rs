//! The vault's credentials and this device's X25519 key pair, kept in the `sync_state` row
//! (spec §11.1, ruling P3): `vault_id` as base64url, `key_epoch` the current epoch, `vault_keys`
//! the JSON `{"<epoch>": "<base64url key>"}` of every epoch still needed, `device_secret_key` the
//! 32 secret bytes. None of the secret-bearing types derives `Debug`: each prints `<redacted>`
//! (ruling P16), and key JSON and tokens live in `Zeroizing` buffers.

use std::collections::BTreeMap;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::{self, Deserialize, Deserializer, Visitor};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize, Zeroizing};

use super::envelope::vault_token;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::registry::keys::{barrier_number, schema_number};
use crate::error::{AppError, Result};

/// The vault's routing ID: 16 random bytes, stored and sent as base64url without padding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VaultId(pub [u8; 16]);

impl VaultId {
    pub fn random() -> Self {
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        VaultId(bytes)
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
    /// A new vault's single key, epoch 0.
    pub fn new_random() -> Self {
        let mut key = Zeroizing::new([0u8; 32]);
        rand::rng().fill_bytes(&mut key[..]);
        VaultKeys {
            current: 0,
            keys: BTreeMap::from([(0, key)]),
        }
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
        let parsed: BTreeMap<String, SecretString> =
            serde_json::from_str(json).map_err(|_| corrupt())?;
        let mut keys = BTreeMap::new();
        for (epoch, encoded) in parsed {
            let epoch: u32 = epoch.parse().map_err(|_| corrupt())?;
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
            .field("relay_url", &self.relay_url)
            .field("account", &format_args!("<redacted>"))
            .field("vault_id", &self.vault_id)
            .field("keys", &self.keys)
            .finish()
    }
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
/// barrier (as `state::ensure_row` would), unless it exists.
fn ensure_disabled_row(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (id, enabled, device_id, schema_seen, barrier) \
         VALUES (1, 0, ?1, ?2, ?3) ON CONFLICT(id) DO NOTHING",
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
        let secrets = (VaultId::random(), VaultKeys::new_random());
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

fn write_vault_secrets(conn: &Connection, vault_id: &VaultId, keys: &VaultKeys) -> Result<()> {
    let json = keys.to_json();
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1, key_epoch = ?2, vault_keys = ?3 WHERE id = 1",
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

/// This device's X25519 public key, generating the key pair the first time.
pub fn ensure_device_keypair(conn: &Connection) -> Result<[u8; 32]> {
    let stored: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT device_secret_key FROM sync_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match stored {
        None => Err(not_set_up()),
        Some(None) => new_device_keypair(conn),
        Some(Some(bytes)) => {
            let bytes = Zeroizing::new(bytes);
            if bytes.len() != 32 {
                return Err(AppError::Internal(
                    "sync_state.device_secret_key is not 32 bytes".into(),
                ));
            }
            let mut secret = Zeroizing::new([0u8; 32]);
            secret.copy_from_slice(&bytes);
            Ok(public_key_of(&secret))
        }
    }
}

/// Replaces this device's key pair (a re-join) and returns the new public key.
pub fn new_device_keypair(conn: &Connection) -> Result<[u8; 32]> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(&mut bytes[..]);
    let secret = StaticSecret::from(*bytes);
    let changed = conn.execute(
        "UPDATE sync_state SET device_secret_key = ?1 WHERE id = 1",
        [secret.as_bytes().as_slice()],
    )?;
    if changed == 0 {
        return Err(not_set_up());
    }
    Ok(PublicKey::from(&secret).to_bytes())
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
        for broken in [
            r#"{"0":"AQEB"}"#,
            r#"{"x":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"}"#,
            r#"{"1":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"}"#,
            "not json",
        ] {
            conn.execute("UPDATE sync_state SET vault_keys = ?1", [broken])
                .unwrap();
            assert!(credentials(&conn).is_err(), "{broken}");
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
        let outputs = [
            format!("{creds:?}"),
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
