//! Device sync as the UI sees it (spec §9.4, §11.6; plan's call P17): the engine's phase, the
//! problem that stopped or paused it, and the counts Settings → Sync shows.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where sync stands on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncPhase {
    /// Sync is not on for this profile (or the build does not run it).
    #[serde(rename = "off")]
    Off,
    /// An account is saved but setup has not finished.
    #[serde(rename = "settingUp")]
    SettingUp,
    #[serde(rename = "idle")]
    Idle,
    /// A round is running.
    #[serde(rename = "syncing")]
    Syncing,
    /// Waiting on something outside this device: the vault, the subscription, the quota, an app
    /// update, the network or the relay's rate limit.
    #[serde(rename = "paused")]
    Paused,
    /// Stopped on something that needs a look.
    #[serde(rename = "error")]
    Error,
}

/// Why sync is paused or stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncProblem {
    /// Change capture could not be installed at unlock; the profile does not sync until repaired.
    #[serde(rename = "captureBroken")]
    CaptureBroken,
    /// The relay no longer knows the vault (`404`); paused until "Sync now" finds it again.
    #[serde(rename = "vaultGone")]
    VaultGone,
    /// The subscription ended (`402`): pulls go on while the relay allows, pushes wait.
    #[serde(rename = "lapsed")]
    Lapsed,
    #[serde(rename = "quotaExceeded")]
    QuotaExceeded,
    #[serde(rename = "snapshotTooLarge")]
    SnapshotTooLarge,
    /// Another device writes with a newer app version.
    #[serde(rename = "newerSchema")]
    NewerSchema,
    /// The relay withholds an envelope.
    #[serde(rename = "gap")]
    Gap,
    #[serde(rename = "undecryptable")]
    Undecryptable,
    #[serde(rename = "invalidChain")]
    InvalidChain,
    #[serde(rename = "missingParent")]
    MissingParent,
    /// A device's envelopes are held after its chain forked.
    #[serde(rename = "quarantined")]
    Quarantined,
    #[serde(rename = "offline")]
    Offline,
    #[serde(rename = "relayError")]
    RelayError,
    #[serde(rename = "rateLimited")]
    RateLimited,
    #[serde(rename = "unauthorized")]
    Unauthorized,
    /// The vault's token was rotated by another device (`403 token_read_only`).
    #[serde(rename = "keyRotated")]
    KeyRotated,
    #[serde(rename = "internal")]
    Internal,
}

/// The `sync-status-changed` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncStatus {
    #[serde(rename = "phase")]
    pub phase: SyncPhase,
    #[serde(rename = "problem")]
    pub problem: Option<SyncProblem>,
    /// When a round last finished without a problem (unix seconds).
    #[serde(rename = "lastSuccessAt")]
    pub last_success_at: Option<i64>,
    /// Changed rows waiting in the outbox plus sealed envelopes waiting for the relay.
    #[serde(rename = "pendingChanges")]
    pub pending_changes: u32,
    /// Devices whose envelopes are held (quarantined).
    #[serde(rename = "heldDevices")]
    pub held_devices: u32,
    /// Rows too large to send, from the last push.
    #[serde(rename = "refusedRows")]
    pub refused_rows: u32,
    /// The subscription's end, as the relay last reported it (unix seconds).
    #[serde(rename = "paidUntil")]
    pub paid_until: Option<i64>,
    /// When the relay deletes the vault after a lapse: 90 days after `paidUntil`.
    #[serde(rename = "deletesAt")]
    pub deletes_at: Option<i64>,
    /// The developer tools are offered (debug build with sync on).
    #[serde(rename = "devTools")]
    pub dev_tools: bool,
}

/// The subscription a checkout is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncPlan {
    #[serde(rename = "monthly")]
    Monthly,
    #[serde(rename = "yearly")]
    Yearly,
}

/// The sync account as setup shows it: its 16-digit number and whether the relay counts it as
/// paid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncAccount {
    #[serde(rename = "accountNumber")]
    pub account_number: String,
    #[serde(rename = "active")]
    pub active: bool,
    /// The subscription's end (unix seconds); `None` while the account has never been paid.
    #[serde(rename = "paidUntil")]
    pub paid_until: Option<i64>,
}

/// Where paying for the account happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind")]
pub enum CheckoutTarget {
    /// The payment page to open in the browser (https, on polar.sh).
    #[serde(rename = "open")]
    Open {
        #[serde(rename = "url")]
        url: String,
    },
    /// A self-hosted relay takes no payments: its operator adds the account number to the relay's
    /// `ALLOWED_ACCOUNTS`, and setup waits until the account is active.
    #[serde(rename = "selfHosted")]
    SelfHosted,
}

/// The Sync Recovery Kit (spec §11.5): what brings the synced data back when every device is
/// lost. It holds the vault key, so `Debug` prints nothing of it and the secret fields are wiped
/// when it drops (ruling P16).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecoveryKit {
    #[serde(rename = "accountNumber")]
    pub account_number: String,
    /// The recovery code as 16 groups of five joined by `-`.
    #[serde(rename = "recoveryCode")]
    pub recovery_code: String,
    /// The QR of `moony-recover:1:<account>:<code>[:<base64url relay address>]` as SVG.
    #[serde(rename = "qrSvg")]
    pub qr_svg: String,
    /// The relay address, only when it is not the default relay (a self-hosted one).
    #[serde(rename = "relayUrl")]
    pub relay_url: Option<String>,
}

impl std::fmt::Debug for RecoveryKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryKit { .. }")
    }
}

impl Drop for RecoveryKit {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.recovery_code.zeroize();
        self.qr_svg.zeroize();
    }
}

/// One device of the sync account as Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncDevice {
    /// The device ID, 32 lowercase hex characters.
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    /// `macos`, `windows`, `linux`, `ios` or `android`.
    #[serde(rename = "platform")]
    pub platform: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    #[serde(rename = "joinedAt")]
    pub joined_at: i64,
    /// The last successful sync of this device, or the newest change another device wrote
    /// (unix seconds); `None` before either.
    #[serde(rename = "lastActiveAt")]
    pub last_active_at: Option<i64>,
    #[serde(rename = "thisDevice")]
    pub this_device: bool,
    /// The device's messages are held back (quarantined) here.
    #[serde(rename = "held")]
    pub held: bool,
}

/// Where a pairing stands on this device (`services::sync::pairing::session::Stage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum PairingStage {
    /// Scanner: its answer is not written yet; polling retries it.
    #[serde(rename = "announcing")]
    Announcing,
    /// Shower: the QR code is on screen, waiting for the other device to scan it.
    #[serde(rename = "waitingForScan")]
    WaitingForScan,
    /// Both: the six-digit comparison code is on screen, waiting for the person.
    #[serde(rename = "compare")]
    Compare,
    /// Joiner, after its confirm: waiting for the other device to send the sync details.
    #[serde(rename = "waitingForOther")]
    WaitingForOther,
    /// Holder, after its confirm: the sync details are sent.
    #[serde(rename = "sent")]
    Sent,
    /// Joiner: the sync details arrived; joining can start.
    #[serde(rename = "received")]
    Received,
}

/// Whether this device holds the synced data (sync on) or joins it (sync off).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum PairingRole {
    #[serde(rename = "holder")]
    Holder,
    #[serde(rename = "joiner")]
    Joiner,
}

/// A pairing as the UI shows it. Nothing secret: the QR code and the comparison code only
/// protect the exchange while they are on screens the person is looking at; the sync details
/// themselves never leave Rust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PairingView {
    #[serde(rename = "stage")]
    pub stage: PairingStage,
    #[serde(rename = "role")]
    pub role: PairingRole,
    /// The pairing code as an SVG QR code, while this device shows it (`waitingForScan`).
    #[serde(rename = "qrSvg")]
    pub qr_svg: Option<String>,
    /// The same code as text, only with the developer tools (a simulator cannot scan).
    #[serde(rename = "pairingCode")]
    pub pairing_code: Option<String>,
    /// The six digits both devices show, from `compare` on.
    #[serde(rename = "comparisonCode")]
    pub comparison_code: Option<String>,
    /// Scanner: another device answered this code first; this device's digits differ from the
    /// other device's, and only cancelling is offered.
    #[serde(rename = "contested")]
    pub contested: bool,
    /// When the pairing ends on this device (unix seconds).
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
}

/// What a replace (a join, a repair, a re-sync) puts at stake on this device, for the confirmation
/// that asks for it (`services::sync::impact::ReplaceImpact`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReplaceImpactView {
    /// Changed items not sealed yet (`sync_outbox` rows).
    #[serde(rename = "pendingItems")]
    pub pending_items: u32,
    /// Sealed batches the sync server has not confirmed (`sync_unsent` rows).
    #[serde(rename = "pendingBatches")]
    pub pending_batches: u32,
    /// Documents of properties and insurance policies.
    #[serde(rename = "documents")]
    pub documents: u32,
    /// Photos of properties.
    #[serde(rename = "photos")]
    pub photos: u32,
    /// Accounts and assets (bonds and insurance policies included).
    #[serde(rename = "accounts")]
    pub accounts: u32,
    /// Bank transactions.
    #[serde(rename = "transactions")]
    pub transactions: u32,
    /// The newest clock this device knows, as a decimal string (it exceeds 2^53): the watermark a
    /// confirmation sends back in its consent.
    #[serde(rename = "changesThrough")]
    pub changes_through: String,
}

/// What the person agreed to in a replace's confirmation: the watermark of the impact it showed,
/// and the documents and photos it showed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReplaceConsentView {
    /// `ReplaceImpactView::changes_through`, as the confirmation received it (decimal string).
    #[serde(rename = "changesThrough")]
    pub changes_through: String,
    /// Documents plus photos the confirmation showed.
    #[serde(rename = "attachments")]
    pub attachments: u32,
}

/// What a join, repair or re-sync did: its backup and what stayed only in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ReplaceOutcomeView {
    /// The backup made before the replace (listed in Restore).
    #[serde(rename = "backupPath")]
    pub backup_path: String,
    /// This device's documents and photos re-attached to items the synced data also has.
    #[serde(rename = "attachmentsKept")]
    pub attachments_kept: u32,
    /// This device's documents and photos found only in the backup now.
    #[serde(rename = "attachmentsInBackupOnly")]
    pub attachments_in_backup_only: u32,
    /// Changed items that reached no sync server and are only in the backup.
    #[serde(rename = "unsentItemsInBackupOnly")]
    pub unsent_items_in_backup_only: u32,
    /// Sealed batches that reached no sync server and are only in the backup.
    #[serde(rename = "unsentBatchesInBackupOnly")]
    pub unsent_batches_in_backup_only: u32,
    /// The backup is kept: never removed by the app, only by the person.
    #[serde(rename = "kept")]
    pub kept: bool,
}

/// A backup a join, repair or re-sync made, as Restore lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncBackupEntry {
    #[serde(rename = "path")]
    pub path: String,
    /// When it was made (unix seconds).
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "bytes")]
    pub bytes: u64,
    /// It holds something found nowhere else: the app never removes it, only the person does.
    #[serde(rename = "kept")]
    pub kept: bool,
}
