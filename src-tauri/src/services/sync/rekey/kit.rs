//! The Recovery Kit's part of a key change, and of the setup that makes the first Kit (phase 5
//! revision §2.3–§2.5, §2.7): a new Kit and its certificate, its row in `sync_devices`, where it
//! is kept (`kit_storage`), and the envelope sealed to it with every key the sealer holds.
//!
//! The Kit's seed lives only in a `KitSecret` (`NewKit`), handed to the person once; the row and
//! the trust store get the public keys and the kind-5 certificate.

use std::collections::BTreeMap;
use std::fmt;

use rusqlite::{params, Connection, OptionalExtension};

use super::bundle::{self, OldKey};
use super::envelope::{self, Confirmed, EnvelopeInput};
use super::head;
use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::trust::{TrustEntry, TrustState};
use crate::services::sync::identity::{cert, Certificate, KitKeys, KitSecret, SignSeed};
use crate::services::sync::keys::VaultKeys;

/// The `platform` of the Kit's row (revision §2.3). Never what tells a Kit's row: its kind-5
/// certificate does (`trust::is_kit_row`, As built R-26).
pub const KIT_PLATFORM: &str = "recoveryKit";

/// Where the person keeps the Kit (`sync_devices.kit_storage`, revision §2.7), weakest first:
/// the strongest value a person ever gave wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KitStorage {
    /// Printed ("Vytisknout").
    Paper,
    /// The code went to the clipboard ("Zkopírovat kód").
    Copied,
    /// Saved into a local folder ("Uložit jako soubor…").
    File,
    /// Saved into a folder that syncs to a cloud.
    Cloud,
}

impl KitStorage {
    pub fn as_str(self) -> &'static str {
        match self {
            KitStorage::Paper => "paper",
            KitStorage::Copied => "copied",
            KitStorage::File => "file",
            KitStorage::Cloud => "cloud",
        }
    }

    /// A stored value; anything else reads as nothing done (`None`).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "paper" => Some(KitStorage::Paper),
            "copied" => Some(KitStorage::Copied),
            "file" => Some(KitStorage::File),
            "cloud" => Some(KitStorage::Cloud),
            _ => None,
        }
    }
}

/// A Kit this device just made: its seed, for the person to see once (the command turns it into
/// a `RecoveryKit` and drops it), its ID and when it was made. `Debug` prints no secret; two are
/// equal when they are the same Kit (the ID is bound to the seed's signing key).
pub struct NewKit {
    pub secret: KitSecret,
    pub id: DeviceId,
    pub created_at: i64,
}

impl fmt::Debug for NewKit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewKit")
            .field("id", &self.id)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

impl PartialEq for NewKit {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.created_at == other.created_at
    }
}

impl Eq for NewKit {}

/// A new Kit for `vault`, made by `creator` (this device, signing key `creator_seed`) at `now`:
/// the seed, what it derives and the Kit's kind-5 certificate.
pub fn create(
    creator: DeviceId,
    creator_seed: &SignSeed,
    vault: &[u8; 16],
    now: i64,
) -> Result<(KitSecret, KitKeys, Certificate)> {
    let secret = KitSecret::generate()?;
    let keys = secret.derive(vault);
    let cert = cert::issue_kit(creator_seed, creator, &keys, now)?;
    Ok((secret, keys, cert))
}

/// Where the Kit `kit` is kept, from its row (`None`: nothing was done with it, or no row).
pub fn storage(conn: &Connection, kit: DeviceId) -> Result<Option<KitStorage>> {
    let stored: Option<Option<String>> = conn
        .query_row(
            "SELECT kit_storage FROM sync_devices WHERE id = ?1",
            [kit.to_hex()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(stored.flatten().as_deref().and_then(KitStorage::parse))
}

/// Writes the row of a Kit this device made (revision §2.3): its ID, its box key, no name, the
/// Kit platform, when it was made, its certificate and where it is kept. Captured, so it syncs.
/// A row already there keeps its values (write-once columns; a retried setup).
pub fn write_row(
    conn: &Connection,
    cert: &Certificate,
    storage: Option<KitStorage>,
    now: i64,
) -> Result<()> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    conn.execute(
        "INSERT INTO sync_devices \
           (id, public_key, name, platform, app_version, joined_at, cert, kit_storage) \
         VALUES (?1, ?2, '', ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO NOTHING",
        params![
            cert.body.device.to_hex(),
            URL_SAFE_NO_PAD.encode(cert.body.box_pub),
            KIT_PLATFORM,
            env!("CARGO_PKG_VERSION"),
            now,
            cert.to_b64(),
            storage.map(KitStorage::as_str),
        ],
    )?;
    Ok(())
}

/// The confirmed set a Kit envelope carries (revision §2.4): what this device's store will hold
/// `confirmed` once the key change commits. Every `trusted` entry confirmed now, minus the
/// devices removed and the Kit replaced, plus every recipient and the Kit sealed to; each with
/// its certificate's hash, by device.
pub fn confirmed_after(
    store: &[TrustEntry],
    recipients: &[TrustEntry],
    removing: &[DeviceId],
    replaced_kit: Option<DeviceId>,
    kit: &Certificate,
) -> Vec<Confirmed> {
    let mut set: BTreeMap<DeviceId, [u8; 32]> = store
        .iter()
        .filter(|e| {
            e.state == TrustState::Trusted
                && e.confirmed
                && !removing.contains(&e.device)
                && Some(e.device) != replaced_kit
        })
        .map(|e| (e.device, e.cert.hash()))
        .collect();
    for entry in recipients {
        set.insert(entry.device, entry.cert.hash());
    }
    set.insert(kit.body.device, kit.hash());
    set.into_iter()
        .map(|(device, cert_hash)| Confirmed { device, cert_hash })
        .collect()
}

/// Seals the envelope for the Kit `kit` (its certificate names the vault, its ID and its box
/// key): every key of `keys`, ascending, with the heads this device verified (and epoch 0's, which starts at 0), and
/// `confirmed`; signed by this device (`signer`, `signer_cert`) at `now`.
pub fn seal(
    conn: &Connection,
    keys: &VaultKeys,
    kit: &Certificate,
    signer: &SignSeed,
    signer_cert: &Certificate,
    confirmed: &[Confirmed],
    now: i64,
) -> Result<Vec<u8>> {
    if kit.body.kind != cert::CertKind::Kit {
        return Err(AppError::Internal(
            "sync: a Kit envelope is sealed only to a Kit".into(),
        ));
    }
    let heads = head::verified_epochs(conn)?;
    let held: Vec<OldKey> = keys
        .epochs()
        .map(|(epoch, key)| OldKey {
            epoch,
            key,
            head: heads.get(&epoch).copied().or((epoch == 0).then_some(0)),
        })
        .collect();
    envelope::build(
        &EnvelopeInput {
            vault_id: kit.body.vault_id,
            kit_id: kit.body.device,
            kit_box_pub: kit.body.box_pub,
            signer,
            signer_cert,
            at: now,
            keys: &held,
            confirmed,
        },
        &mut bundle::os_fill,
    )
}
