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
