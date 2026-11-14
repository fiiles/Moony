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
    /// Syncing goes on, but a change from another device has not arrived (a gap in its chain, or
    /// a parent it references): the change waits, this device keeps pulling and pushing.
    #[serde(rename = "waiting")]
    Waiting,
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
    /// A change from another device is dated more than ten years past this device's time: it is
    /// held (skippable after 14 days, `HeldGapKind::Ahead`), and this device keeps syncing
    /// meanwhile (audit M6).
    #[serde(rename = "clockAhead")]
    ClockAhead,
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
    /// This device's secure storage could not be reached (the device binding could not be
    /// checked); paused, and tried again at the next trigger.
    #[serde(rename = "secureStorage")]
    SecureStorage,
    /// The sync server went back to an older state (its head below this device's place, e.g.
    /// restored from an older backup); changes may be missing on other devices. Sticky until a
    /// replace or turning sync off; syncing goes on.
    #[serde(rename = "serverWentBack")]
    ServerWentBack,
    #[serde(rename = "internal")]
    Internal,
}

/// What a build without the sync flag can still say about the open profile (`sync_profile_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncProfileState {
    /// This profile has sync turned on (a development build did it); its changes are recorded.
    #[serde(rename = "enabled")]
    pub enabled: bool,
    /// This build runs sync (the flag).
    #[serde(rename = "featureOn")]
    pub feature_on: bool,
    /// Changes recorded but never sent (turning sync off means they will not be sent).
    #[serde(rename = "pendingChanges")]
    pub pending_changes: u32,
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
    /// While this device's clock (its hybrid logical clock) is more than a day past its wall
    /// clock: the date its changes are stamped with until then (unix seconds). A note, never a
    /// problem: sync goes on, and until that date this device's edits win over edits made
    /// elsewhere before it. `None` otherwise, and for a date past year 9999 (audit M6).
    #[serde(rename = "clockAheadUntil")]
    pub clock_ahead_until: Option<i64>,
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

/// A sync server that passed the check (`GET /v1/server`, spec §9.3): the address in its stored
/// spelling, how to show it, its version, and whether it is plain http beyond this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncServerCheck {
    #[serde(rename = "address")]
    pub address: String,
    #[serde(rename = "host")]
    pub host: String,
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "insecure")]
    pub insecure: bool,
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

/// Which file of the Recovery Kit the desktop saves (`sync_save_recovery_kit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KitFileKind {
    /// The self-contained page (`.html`), to print or keep.
    #[serde(rename = "page")]
    Page,
    /// The same facts as plain text (`.txt`).
    #[serde(rename = "text")]
    Text,
}

/// What became of a Recovery Kit save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KitSaveOutcome {
    /// `false`: the person closed the save dialog; nothing was written.
    #[serde(rename = "saved")]
    pub saved: bool,
    /// The chosen folder looks synced to a cloud service (iCloud Drive, OneDrive, Dropbox, …).
    #[serde(rename = "syncedFolder")]
    pub synced_folder: bool,
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
    /// Both: the other device is answered (scanner: slot 1 written, the shower's acknowledgement
    /// awaited; shower: the acknowledgement is being stored); nothing is shown yet.
    #[serde(rename = "connecting")]
    Connecting,
    /// The joiner shows its six digits; the holder asks its person to type them.
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
    /// The six digits the joiner shows, from `compare` on; never set on the holder, whose person
    /// types them.
    #[serde(rename = "comparisonCode")]
    pub comparison_code: Option<String>,
    /// Scanner: another device answered this code first; this device's digits differ from the
    /// other device's, and only cancelling is offered.
    #[serde(rename = "contested")]
    pub contested: bool,
    /// When the pairing ends on this device (unix seconds).
    #[serde(rename = "expiresAt")]
    pub expires_at: i64,
    /// The sync server of this pairing as `host[:port]`, only when it is not the default one, so
    /// the person sees which server a self-hosted pairing goes through.
    #[serde(rename = "serverHost")]
    pub server_host: Option<String>,
    /// Holder, in `compare`: wrong codes the person may still type before the pairing ends.
    #[serde(rename = "attemptsLeft")]
    pub attempts_left: Option<u8>,
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

/// What a held change waits for (`HeldGapView::kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum HeldGapKind {
    /// An earlier change of the device never arrived; skipping applies the ones after it.
    #[serde(rename = "missing")]
    Missing,
    /// The device's change refers to something no change brought; skipping skips that change.
    #[serde(rename = "parent")]
    Parent,
    /// The device's change is dated more than ten years ahead (audit M6); skipping skips that
    /// change, and the device's later changes then apply.
    #[serde(rename = "ahead")]
    Ahead,
}

/// A change from another device that this device holds because something it needs has not arrived
/// (audit M3) or because it is dated more than ten years ahead (audit M6): Settings offers to skip
/// it once `skippableAt` has passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HeldGapView {
    /// The author's device ID, 32 lowercase hex characters (`sync_skip_missing_change` takes it).
    #[serde(rename = "deviceId")]
    pub device_id: String,
    /// The author's name from the device list; `None` for a device that has no row there.
    #[serde(rename = "deviceName")]
    pub device_name: Option<String>,
    #[serde(rename = "kind")]
    pub kind: HeldGapKind,
    /// How many changes a skip gives up: the device's changes that never arrived (`missing`), or
    /// 1 (`parent` and `ahead`, the held change itself).
    #[serde(rename = "missingChanges")]
    pub missing_changes: u64,
    /// When this device first held it (unix seconds).
    #[serde(rename = "since")]
    pub since: i64,
    /// From when it may be skipped (unix seconds): `since` plus 14 days.
    #[serde(rename = "skippableAt")]
    pub skippable_at: i64,
    /// The date an `ahead` change carries (unix seconds, its newest clock); `None` for the other
    /// kinds and for a date past year 9999.
    #[serde(rename = "datedAt")]
    pub dated_at: Option<i64>,
}
