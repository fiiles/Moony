//! Device sync as the UI sees it (spec §9.4, §11.6; plan's call P17): the engine's phase, the
//! problem that stopped or paused it, and the counts Settings → Sync shows.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Where sync stands on this device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncPhase {
    /// Sync is not on for this profile (or the build does not run it).
    #[default]
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
    /// The key changed and this device got no new key and no verifiable removal notice (phase 5
    /// §7.5): paused, never turned off by a server's word alone; the person may turn sync off.
    #[serde(rename = "keyNotGiven")]
    KeyNotGiven,
    /// A key change opened for this device but cannot be verified: signed by a device this one
    /// does not trust, or contradicting what it holds (phase 5 §7.5). Paused.
    #[serde(rename = "keyChangeUnverified")]
    KeyChangeUnverified,
    /// This device picked up a key change and waits for the device that made it to send the
    /// signed start of the change (phase 5 As built 63) before it applies older changes past where
    /// it stood: paused on another device ("Čeká se, až se připojí zařízení, které změnu
    /// provedlo").
    #[serde(rename = "keyChangeWaiting")]
    KeyChangeWaiting,
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

/// Why documents and photos do not move while ordinary sync goes on (ADR 0030): uploads of their
/// content wait, everything else syncs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncAttachmentsProblem {
    /// The sync server has no room for more content (the vault's quota, or its disk at the
    /// free-space floor): uploads wait; downloads go on.
    #[serde(rename = "noRoom")]
    NoRoom,
    /// The sync server does not have the blob routes (no `blobs` feature): nothing moves until it
    /// is updated.
    #[serde(rename = "serverNeedsUpdate")]
    ServerNeedsUpdate,
}

/// Where this device's documents and photos stand with sync (attachments design §5.6), counted by
/// attachment row (a photo once): beside the ordinary phase, never part of it. Zeros while sync is
/// off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncAttachments {
    /// Rows whose content is held here and not yet confirmed by the sync server, and can still
    /// upload: while any waits, this device is not synchronised.
    #[serde(rename = "waitingUpload")]
    pub waiting_upload: u32,
    /// The plaintext bytes of the content waiting to upload.
    #[serde(rename = "waitingUploadBytes")]
    pub waiting_upload_bytes: i64,
    /// Rows whose content is on another device or the sync server, not fetched here yet.
    #[serde(rename = "notHere")]
    pub not_here: u32,
    /// Rows whose content the sync server does not have yet (not uploaded from the other device).
    #[serde(rename = "notOnServer")]
    pub not_on_server: u32,
    /// Rows whose content could not be fetched, or cannot upload by itself: failed items, which
    /// never keep this device "not synchronised".
    #[serde(rename = "failed")]
    pub failed: u32,
    /// Rows with any content not complete on this device, whatever the reason (the three counts
    /// above leave out of each other what they hold): the backup does not have these.
    #[serde(rename = "notOnDevice")]
    pub not_on_device: u32,
    /// Rows among `failed` whose content is held here and cannot upload by itself (refused by the
    /// sync server, a conflict with what it holds, an invalid ID): on this device only until tried
    /// again. An upload backing off (no room, the network) is in `waiting_upload` instead.
    #[serde(rename = "failedUpload")]
    pub failed_upload: u32,
    /// Why documents and photos do not move, while ordinary sync goes on.
    #[serde(rename = "problem")]
    pub problem: Option<SyncAttachmentsProblem>,
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

/// The `sync-status-changed` payload. `Default` is sync off with nothing to say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    /// The stored server is not the Moony cloud: a person's own sync server.
    #[serde(rename = "ownServer")]
    pub own_server: bool,
    /// The stored server's address, in every phase that has one.
    #[serde(rename = "serverAddress")]
    pub server_address: Option<String>,
    /// The stored address is plain http to a host other than this computer.
    #[serde(rename = "serverInsecure")]
    pub server_insecure: bool,
    /// When sync was turned on on this device: `joinedAt` of its own device row (unix seconds);
    /// `None` while off or before the row exists.
    #[serde(rename = "enabledAt")]
    pub enabled_at: Option<i64>,
    /// This device's documents and photos: content waiting to move, and their own problem.
    #[serde(rename = "attachments")]
    pub attachments: SyncAttachments,
    /// When a round last succeeded with nothing left to send here: no change, no document or photo
    /// waiting to upload or unable to (unix seconds; spec 2026-11-14 §7.2). `None` while off or
    /// before the first.
    #[serde(rename = "lastCompleteAt")]
    pub last_complete_at: Option<i64>,
    /// Sync keeps this device's data safe now (spec §7.3, `milestones::sync_protects`): on, nothing
    /// needs the person, a complete sync less than 7 whole UTC days old, and nothing held here that
    /// exists nowhere else (no unsent change, no document or photo left to upload).
    #[serde(rename = "protectsData")]
    pub protects_data: bool,
    /// Devices can be removed from this sync: this device's own certificate is pinned (a vault
    /// set up before phase 5 has none, revision §6.1). `false` while sync is off.
    #[serde(rename = "removalSupported")]
    pub removal_supported: bool,
    /// A key change (a removal, a new recovery kit) of this device waits for the sync server
    /// and finishes by itself: "Dokončuji odebrání zařízení…".
    #[serde(rename = "removalPending")]
    pub removal_pending: bool,
    /// With `keyChangeWaiting`: the head not verified yet is this device's own (a removal or a
    /// new recovery kit it made, not signed until it has read the changes before it): "Odebrání
    /// se dokončuje."
    #[serde(rename = "ownChangeFinishing")]
    pub own_change_finishing: bool,
    /// Another device removed devices or made a new recovery kit (Local, until dismissed).
    #[serde(rename = "removalNotice")]
    pub removal_notice: Option<SyncRemovalNotice>,
    /// A removal the person confirmed here that a background resolution found not done.
    #[serde(rename = "removalNotDone")]
    pub removal_not_done: Option<SyncRemovalNotDone>,
    /// This device applied changes the sync server placed past a key change it hid.
    #[serde(rename = "keyChangeOverrun")]
    pub key_change_overrun: Option<SyncKeyChangeOverrun>,
    /// Another device removed this one: sync is off here (readable while sync is off).
    #[serde(rename = "removedNotice")]
    pub removed_notice: Option<SyncRemovedNotice>,
    /// With `keyNotGiven` or `keyChangeUnverified`: the device that made the key change, which
    /// this device does not know ("Změnu provedlo zařízení {by}, které toto zařízení nezná.").
    #[serde(rename = "rejoinBy")]
    pub rejoin_by: Option<String>,
    /// With `keyRotated` for more than a day: the device whose signed confirmation of the last
    /// key change has not arrived, and since when.
    #[serde(rename = "waitingForRemover")]
    pub waiting_for_remover: Option<SyncWaitingForRemover>,
    /// Devices pinned outside a join (paired by another device, or joined with the Kit), newest
    /// last, until dismissed.
    #[serde(rename = "newDevices")]
    pub new_devices: Vec<SyncNewDevice>,
    /// The vault's usable recovery kit as this device sees it; `None`: sync has no kit this
    /// device can seal to.
    #[serde(rename = "recoveryKit")]
    pub recovery_kit: Option<SyncKitInfo>,
    /// The minute the sync server says the recovery kit was last read (unix seconds), when that is
    /// news the person has not seen and no device joined with the kit to explain it.
    #[serde(rename = "kitUsed")]
    pub kit_used: Option<i64>,
    /// A device that joined with the recovery kit holds back what it sends until the person
    /// decides about the devices it found on the list ("Data jsou zpět").
    #[serde(rename = "afterKitJoin")]
    pub after_kit_join: Option<SyncAfterKitJoin>,
    /// While a round runs: when it started (unix seconds), so the UI can say "taking longer than
    /// usual" after a minute. `None` otherwise.
    #[serde(rename = "syncingSince")]
    pub syncing_since: Option<i64>,
    /// The first snapshot of the data has not been uploaded (or restored) yet.
    #[serde(rename = "firstUpload")]
    pub first_upload: bool,
}

/// Another device's key change (`removalNotice`): who made it, when (its signed time), the
/// names of the devices it removed, and whether it made a new recovery kit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncRemovalNotice {
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "removed")]
    pub removed: Vec<String>,
    #[serde(rename = "kitReplaced")]
    pub kit_replaced: bool,
}

/// A removal the person confirmed here that did not happen (`removalNotDone`): `by` changed the
/// device list first (`None`: the sync server refused it), `devices` are the names meant to go,
/// `alreadyRemoved` that the other change had removed every one of them, `kitReplaced` that the
/// change was to make a new recovery kit, which then never started working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncRemovalNotDone {
    #[serde(rename = "by")]
    pub by: Option<String>,
    #[serde(rename = "devices")]
    pub devices: Vec<String>,
    #[serde(rename = "alreadyRemoved")]
    pub already_removed: bool,
    #[serde(rename = "kitReplaced")]
    pub kit_replaced: bool,
}

/// `keyChangeOverrun`: `at` is when this device adopted the key change the server hid (unix
/// seconds; `None` when this device no longer knows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncKeyChangeOverrun {
    #[serde(rename = "at")]
    pub at: Option<i64>,
}

/// Why sync is off on this device (`removedNotice`): `by` removed it at `at` (unix seconds,
/// the record's signed time) and `unsent` changes never left it. `left`: the record was this
/// device's own `left` record (from another copy of this profile), not another device's removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncRemovedNotice {
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "unsent")]
    pub unsent: u32,
    #[serde(rename = "left")]
    pub left: bool,
}

/// The device whose signed confirmation of a key change this device still waits for
/// (`waitingForRemover`), since when (unix seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncWaitingForRemover {
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "since")]
    pub since: i64,
}

/// A device that appeared on the list outside a join (`newDevices`): `by` added it (`viaKit`: it
/// joined with the recovery kit and `by` is empty), `at` is when this device noticed (unix
/// seconds), `lateVouch` that it is left out of the next key change until kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncNewDevice {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "viaKit")]
    pub via_kit: bool,
    #[serde(rename = "lateVouch")]
    pub late_vouch: bool,
}

/// Where the person keeps the recovery kit, as far as Moony saw (the strongest value wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncKitStorage {
    #[serde(rename = "paper")]
    Paper,
    #[serde(rename = "copied")]
    Copied,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "cloud")]
    Cloud,
}

/// The vault's recovery kit as the status shows it (`recoveryKit`): when it was made and by which
/// device (`None`: not on this device's list any more), where it was stored (`None`: the person
/// did nothing with it: "neuložená"), and when the sync server says it was last read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncKitInfo {
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "createdBy")]
    pub created_by: Option<String>,
    #[serde(rename = "storage")]
    pub storage: Option<SyncKitStorage>,
    #[serde(rename = "lastReadAt")]
    pub last_read_at: Option<i64>,
}

/// One device the Kit joiner's hold names: its ID (32 lowercase hex characters, as in
/// `SyncDevice.id`) and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncAfterKitDevice {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
}

/// `afterKitJoin`: the devices that were on the list when this device joined with the recovery
/// kit, and the newest change among them (unix seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncAfterKitJoin {
    #[serde(rename = "devices")]
    pub devices: Vec<SyncAfterKitDevice>,
    #[serde(rename = "newestChangeAt")]
    pub newest_change_at: Option<i64>,
}

/// Which notice `sync_dismiss_notice` dismisses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncNoticeKind {
    /// `removalNotice`.
    #[serde(rename = "removal")]
    Removal,
    /// `removedNotice`.
    #[serde(rename = "removed")]
    Removed,
    /// `removalNotDone`.
    #[serde(rename = "removalNotDone")]
    RemovalNotDone,
    /// `keyChangeOverrun`.
    #[serde(rename = "keyChangeOverrun")]
    KeyChangeOverrun,
    /// `newDevices`.
    #[serde(rename = "newDevices")]
    NewDevices,
    /// `kitUsed` ("Vím o tom").
    #[serde(rename = "kitUsed")]
    KitUsed,
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
    /// An own sync server takes no payments: its operator adds people in the server's admin
    /// interface and the account is active as long as the person is not blocked. Only the Moony
    /// cloud checkout (not reachable yet) answers `Open`.
    #[serde(rename = "selfHosted")]
    SelfHosted,
}

/// The Sync Recovery Kit (revision §2.1, §2.2): what brings the synced data back when every
/// device is lost. It holds the Kit's seed, so `Debug` prints nothing of it and the secret fields
/// are wiped when it drops (ruling P16).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RecoveryKit {
    /// The recovery code (format v2, the account inside) as 19 groups of five joined by `-`.
    #[serde(rename = "recoveryCode")]
    pub recovery_code: String,
    /// The QR of `moony-recover:2:<code>[:<base64url server address>]` as SVG.
    #[serde(rename = "qrSvg")]
    pub qr_svg: String,
    /// The sync server's address, only when it is not the default one (a self-hosted one).
    #[serde(rename = "relayUrl")]
    pub relay_url: Option<String>,
    /// When the kit was made (unix seconds): its certificate's date.
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    /// The name of the device that made it (this device), `None` while setup has not recorded
    /// this device's name yet.
    #[serde(rename = "createdBy")]
    pub created_by: Option<String>,
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
    /// Where the device stands (revision §6.5): `active`, `check` ("Zkontrolujte": it gets no
    /// key at the next change until kept or removed) or `former` (left or removed, listed for
    /// 30 days).
    #[serde(rename = "status")]
    pub status: SyncDeviceStatus,
    /// With `status = check`: why.
    #[serde(rename = "checkReason")]
    pub check_reason: Option<SyncCheckReason>,
    /// With `status = former`: how it went.
    #[serde(rename = "formerReason")]
    pub former_reason: Option<SyncFormerReason>,
    /// How the device came to the list.
    #[serde(rename = "joinedVia")]
    pub joined_via: SyncJoinedVia,
    /// The name of the device that paired it (`joinedVia = paired`), when this device knows one.
    #[serde(rename = "addedBy")]
    pub added_by: Option<String>,
    /// When a former device left or was removed (unix seconds), when known.
    #[serde(rename = "stateAt")]
    pub state_at: Option<i64>,
    /// The name of the device that removed it (`formerReason = removed`), when known.
    #[serde(rename = "stateBy")]
    pub state_by: Option<String>,
    /// A key change of this device that removes it waits for the sync server ("Odebírá se…").
    #[serde(rename = "removalPending")]
    pub removal_pending: bool,
    /// Whole days since an active other device was last seen, once that is 30 or more.
    #[serde(rename = "inactiveDays")]
    pub inactive_days: Option<u32>,
}

/// `SyncDevice::status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncDeviceStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "check")]
    Check,
    #[serde(rename = "former")]
    Former,
}

/// `SyncDevice::check_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncCheckReason {
    /// This device could not verify the device (no pinned certificate, or its issuer is unknown).
    #[serde(rename = "unverified")]
    Unverified,
    /// An older ID of a device that connected again, still writing changes: a copy.
    #[serde(rename = "copy")]
    Copy,
    /// A device a removed device added.
    #[serde(rename = "issuerRemoved")]
    IssuerRemoved,
    /// Added by a device that left (or was superseded) with nothing showing it happened in time,
    /// or by a device left out for that reason.
    #[serde(rename = "lateVouch")]
    LateVouch,
}

/// `SyncDevice::former_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncFormerReason {
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "removed")]
    Removed,
}

/// `SyncDevice::joined_via`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SyncJoinedVia {
    /// The device that set the sync up.
    #[serde(rename = "created")]
    Created,
    /// Paired by another device.
    #[serde(rename = "paired")]
    Paired,
    /// Joined with the recovery kit.
    #[serde(rename = "recoveryKit")]
    RecoveryKit,
    /// Connected again after turning sync off.
    #[serde(rename = "rejoined")]
    Rejoined,
    /// This device has no certificate for it (a vault from before phase 5, or not verified).
    #[serde(rename = "unknown")]
    Unknown,
}

/// What removing devices would do (`sync_removal_plan`), as the dialog shows it (revision §4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RemovalPlan {
    /// The devices that go (those asked for and the ticked ones).
    #[serde(rename = "removing")]
    pub removing: Vec<RemovalPlanDevice>,
    /// The devices that keep syncing (this device included).
    #[serde(rename = "staying")]
    pub staying: Vec<RemovalPlanDevice>,
    /// Devices that drop out of sync with them, each with the reason ("Synchronizovat se přestanou
    /// i:"); only rows that are active or "Zkontrolujte".
    #[serde(rename = "droppingOut")]
    pub dropping_out: Vec<RemovalDroppingOut>,
    /// Devices a removed device added, at any depth (to tick: "Odebrat také"), the ones asked
    /// for left out.
    #[serde(rename = "addedByRemoving")]
    pub added_by_removing: Vec<RemovalPlanDevice>,
    /// The hex IDs of the members of `added_by_removing` this device first verified within the
    /// last 30 days: the dialog starts with them ticked.
    #[serde(rename = "tickedByDefault")]
    pub ticked_by_default: Vec<String>,
    /// Whether, and why, the recovery kit is replaced; a new kit then comes with the outcome.
    #[serde(rename = "replacesKit")]
    pub replaces_kit: ReplacesKit,
    /// The change touches no recovery kit at all (none known): "Tímto odebráním se záchranná sada
    /// neaktualizuje.".
    #[serde(rename = "kitNotUpdated")]
    pub kit_not_updated: bool,
    /// Pass back to `sync_remove_devices`: a hash over what the person was shown.
    #[serde(rename = "consent")]
    pub consent: String,
}

/// A device in a removal plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RemovalPlanDevice {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "platform")]
    pub platform: String,
    #[serde(rename = "joinedAt")]
    pub joined_at: i64,
    #[serde(rename = "thisDevice")]
    pub this_device: bool,
    /// When this device first verified it (`pinned_at`, its own clock); `None` with no trust entry.
    #[serde(rename = "addedAt")]
    pub added_at: Option<i64>,
    /// The name of the device that added it (a pairing's issuer); `None` for any other way of
    /// joining or when that device's row is gone.
    #[serde(rename = "addedBy")]
    pub added_by: Option<String>,
}

/// A device that drops out of sync with the removed ones, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RemovalDroppingOut {
    #[serde(rename = "device")]
    pub device: RemovalPlanDevice,
    #[serde(rename = "reason")]
    pub reason: SyncCheckReason,
}

/// Whether, and why, a removal replaces the recovery kit (revision §2.7, §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ReplacesKit {
    /// The kit stays.
    #[serde(rename = "no")]
    No,
    /// The kit was saved on a device being removed.
    #[serde(rename = "creatorRemoved")]
    CreatorRemoved,
    /// The kit is in a folder that syncs to a cloud, and a device is being removed.
    #[serde(rename = "cloud")]
    Cloud,
    /// The person asked for a new kit.
    #[serde(rename = "asked")]
    Asked,
    /// This device has no kit it can seal to but the sync server holds one: a new one replaces it.
    #[serde(rename = "missing")]
    Missing,
}

/// How `sync_remove_devices` ended (revision §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum RemovalOutcomeKind {
    /// The key changed.
    #[serde(rename = "done")]
    Done,
    /// The sync server did not answer: the change finishes by itself ("Odebrání se dokončí
    /// samo"); a new kit starts working then.
    #[serde(rename = "pending")]
    Pending,
    /// Another device removed every device asked for first (`by`).
    #[serde(rename = "alreadyRemoved")]
    AlreadyRemoved,
    /// Another device's change had removed this device: sync is off here (the status carries the
    /// removed notice).
    #[serde(rename = "removedHere")]
    RemovedHere,
}

/// What a removal did. A new recovery kit (`kit`) comes only here, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RemovalOutcome {
    #[serde(rename = "kind")]
    pub kind: RemovalOutcomeKind,
    /// `alreadyRemoved`: the name of the device that removed them.
    #[serde(rename = "by")]
    pub by: Option<String>,
    /// The new recovery kit to show once, when the change made one (`done` or `pending`).
    #[serde(rename = "kit")]
    pub kit: Option<RecoveryKit>,
}

/// What `sync_check_recovery_kit` found (revision §2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KitCheckResult {
    /// The kit is this sync's current one.
    #[serde(rename = "valid")]
    Valid,
    /// A newer kit replaced it.
    #[serde(rename = "replaced")]
    Replaced,
    /// The code belongs to another sync.
    #[serde(rename = "otherSync")]
    OtherSync,
    /// This sync has no such kit.
    #[serde(rename = "unknown")]
    Unknown,
}

/// The answer of "Ověřit sadu".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KitCheck {
    #[serde(rename = "result")]
    pub result: KitCheckResult,
    /// `replaced`: when this device learnt of the replacement (unix seconds), if known.
    #[serde(rename = "replacedAt")]
    pub replaced_at: Option<i64>,
    /// `replaced`: when the valid kit was made (unix seconds), if known.
    #[serde(rename = "currentCreatedAt")]
    pub current_created_at: Option<i64>,
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
    /// The same code as text, to copy and type on a device that cannot scan.
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
    /// Joiner, at `received`: `host[:port]` of the server the received sync details name, so the
    /// person sees where the account will be used before joining.
    #[serde(rename = "joinServer")]
    pub join_server: Option<String>,
    /// Whether that server is reached over plain http beyond this device (`is_insecure_address`).
    #[serde(rename = "joinServerInsecure")]
    pub join_server_insecure: bool,
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
