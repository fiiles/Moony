//! `FakeRelay`: an in-memory `Relay` for engine tests. It follows the real relay (Moony-relay
//! `src/vault.ts`, `src/account.ts`, `src/index.ts`) and answers with exactly the `RelayError`s
//! `HttpRelay` produces for the same situation, including the argument checks `HttpRelay` makes
//! before it sends anything. On top of that it has fault switches: a failure for the next call of
//! a route, a lost push answer, pruning, and dishonest-relay knobs (`drop_op`, `replay_op`,
//! `forget_op`, `set_swallow_pushes`). Its blob store and blob knobs live in `fake/blobs.rs`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use uuid::Uuid;

use super::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, KitInfo, KitRead,
    KitRotation, OutgoingOp, PullPage, PulledOp, PushReceipt, Rekey, RekeyPage, Relay, RelayError,
    RelayResult, Rotated, ServerInfo, ServerMode, SnapshotBlob, VaultInfo, MAX_KIT_ENVELOPE,
    MAX_REKEY_BUNDLE, MAX_REKEY_PAGE, MAX_REKEY_PAGE_BYTES,
};
use crate::services::sync::keys::VaultAuth;

const MAX_OP_BYTES: usize = 192 * 1024;
const MAX_OPS_PER_PUSH: usize = 500;
const MAX_PUSH_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_PULL_LIMIT: u32 = 1000;
const MAX_PULL_BYTES: usize = 4 * 1024 * 1024;
const MAX_VAULT_OP_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: usize = 100 * 1024 * 1024;
const MAX_VAULTS_PER_ACCOUNT: usize = 5;
const READ_GRACE_SECONDS: i64 = 30 * 86_400;
/// How long a retired token reads and a key-change bundle is kept (the server's 30 days).
const PREVIOUS_TOKEN_SECONDS: i64 = 30 * 86_400;
/// Bundles a vault keeps (younger than 30 days) before it refuses another rotation.
const MAX_REKEYS_KEPT: usize = 32;
/// Replaced Kit tokens a vault remembers (the newest), then a replaced Kit reads `Unauthorized`.
const MAX_KITS_RETIRED: usize = 256;
/// Failed Kit reads a vault takes inside `KIT_FAILURE_WINDOW_SECONDS` before more failures are
/// `RateLimited` (the right Kit token and a replaced one are never counted or refused).
const MAX_KIT_FAILURES: usize = 30;
const KIT_FAILURE_WINDOW_SECONDS: i64 = 3600;
/// How far `set_paid(.., true)` pushes `paid_until`.
const PAID_FOR_SECONDS: i64 = 365 * 86_400;
/// The `paid_until` a self-hosted server gives a person (year 9999).
const PERSON_PAID_UNTIL: i64 = 253_402_300_799;

/// The relay calls a test can make fail or look up in `calls()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    ServerInfo,
    CreateAccount,
    AccountStatus,
    Checkout,
    CreateVault,
    VaultInfo,
    Push,
    Pull,
    PutSnapshot,
    GetSnapshot,
    DeleteVault,
    RotateToken,
    Rekeys,
    PutKit,
    GetKit,
    MailboxPut,
    MailboxTake,
    MailboxBurn,
    PutBlob,
    HeadBlob,
    GetBlob,
    ListBlobs,
    DeleteBlob,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Default)]
struct Account {
    /// 0 = never paid.
    paid_until: i64,
    vaults: Vec<String>,
}

struct Vault {
    account: String,
    token: String,
    head: u64,
    floor: u64,
    ops: Vec<PulledOp>,
    /// Sequence numbers the relay silently leaves out of pull answers (`drop_op`).
    dropped: HashSet<u64>,
    bytes: u64,
    snapshot: Option<SnapshotBlob>,
    /// The token rotation counter `vault_info` answers (`set_rotation`).
    rotation: u32,
    /// Tokens rotated away, with when (the fake's clock): read-only for 30 days.
    prev_tokens: Vec<(String, i64)>,
    /// Key-change bundles by rotation, oldest first.
    rekeys: Vec<StoredRekey>,
    /// The Recovery Kit (`put_kit`, a rotation's `kit`).
    kit: Option<StoredKit>,
    /// Replaced Kit tokens with when, oldest first, at most `MAX_KITS_RETIRED`.
    kits_retired: Vec<(String, i64)>,
    /// When (the fake's clock) Kit reads with a wrong token failed, inside the last hour.
    kit_failures: Vec<i64>,
    blobs: blobs::VaultBlobs,
    /// Positions a lying relay kept free below a rotation head it raised
    /// (`lift_next_rotation_head`): the next free one and the last.
    gap: Option<(u64, u64)>,
}

struct StoredKit {
    token: String,
    envelope: Vec<u8>,
    since: i64,
    updated_at: i64,
    /// The minute (unix seconds, rounded down) of the last successful read.
    last_read: Option<i64>,
}

struct StoredRekey {
    rotation: u32,
    head: u64,
    at: i64,
    bundle: Vec<u8>,
}

struct FakeState {
    accounts: HashMap<String, Account>,
    vaults: HashMap<String, Vault>,
    next_account: u64,
    failures: HashMap<usize, VecDeque<RelayError>>,
    /// Failures that hold for every call of a route until cleared (`set_failing`).
    failing: HashMap<usize, RelayError>,
    lose_push_answer: bool,
    /// Test override of the per-vault op byte quota.
    op_quota: Option<u64>,
    calls: Vec<Route>,
    push_sizes: Vec<(usize, usize)>,
    /// Per push call, the op IDs it carried, whatever the answer was (`pushed_ids`).
    pushed_ids: Vec<Vec<String>>,
    /// Every push answers success and stores nothing (`set_swallow_pushes`).
    swallow_pushes: bool,
    /// What a checkout answers: this base followed by the plan (`set_checkout_base`).
    checkout_base: Option<String>,
    /// A self-hosted relay (`SELF_HOSTED=1`): checkout answers `SelfHosted`.
    self_hosted: bool,
    /// Only mailboxes that were written to: reads, burns and expiry of other IDs store nothing.
    mailboxes: HashMap<String, mailbox::PairingBox>,
    lose_take_answer: bool,
    lose_put_answer: bool,
    /// Routes whose next successful call stores its effect, then answers `Server(503)`.
    lose_answers: Vec<Route>,
    /// A sync server from before slot 3: every call on slot 3 answers `BadRequest("bad_slot")`.
    without_slot_3: bool,
    /// What `server_info` answers; `None` is a relay from before `GET /v1/server`.
    server_info: Option<ServerInfo>,
    /// A self-hosted server with registration closed: `create_account` answers 403.
    registration_closed: bool,
    /// The per-vault blob quota in bytes (`set_blob_quota`).
    blob_quota: u64,
    /// A full disk: blob and snapshot uploads answer `InsufficientStorage` (`set_no_room`).
    no_room: bool,
    /// What a blob listing reports as the server's clock (`set_server_now`); `None` = `now()`.
    server_now: Option<Option<i64>>,
    /// Seconds the fake's clock is ahead of the real one (`advance_clock`): what the 30-day rules
    /// of a retired token and a key-change bundle are measured on.
    clock_offset: i64,
    /// A relay colluding with a removed device: a retired token still writes
    /// (`set_accept_retired_writes`) and reads (`set_accept_retired_reads`).
    accept_retired_writes: bool,
    accept_retired_reads: bool,
    /// What `rekeys` answers instead of the stored bundles (`set_rekeys_answer`): a lying relay's
    /// pages, built from `after`, never checked against the contract.
    rekeys_answer: Option<RekeysAnswer>,
    /// A relay colluding with a removed device: how far above the vault's real head the next
    /// rotation reports (and records) its head (`lift_next_rotation_head`).
    rotation_head_lift: u64,
}

/// A scripted `rekeys` answer: the page for `after`.
pub type RekeysAnswer = Arc<dyn Fn(u32) -> RekeyPage + Send + Sync>;

impl Default for FakeState {
    fn default() -> Self {
        FakeState {
            accounts: HashMap::new(),
            vaults: HashMap::new(),
            next_account: 0,
            failures: HashMap::new(),
            failing: HashMap::new(),
            lose_push_answer: false,
            op_quota: None,
            calls: Vec::new(),
            push_sizes: Vec::new(),
            pushed_ids: Vec::new(),
            swallow_pushes: false,
            checkout_base: None,
            self_hosted: false,
            mailboxes: HashMap::new(),
            lose_take_answer: false,
            lose_put_answer: false,
            lose_answers: Vec::new(),
            without_slot_3: false,
            server_info: Some(ServerInfo {
                version: "1.0.0".into(),
                protocol: 3,
                mode: ServerMode::SelfHosted,
                blobs: true,
                kit: true,
            }),
            registration_closed: false,
            blob_quota: blobs::DEFAULT_BLOB_QUOTA,
            no_room: false,
            server_now: None,
            clock_offset: 0,
            accept_retired_writes: false,
            accept_retired_reads: false,
            rekeys_answer: None,
            rotation_head_lift: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The vault's info and the key changes: all a retired token may still ask (protocol 3).
    Lookup,
    Read,
    Write,
    Owner,
}

fn is_base64url(text: &str, n: usize) -> bool {
    text.len() == n
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The argument checks of `HttpRelay` that come before a request is sent.
fn check_account(account: &str) -> RelayResult<()> {
    if account.len() == 16 && account.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(RelayError::BadRequest("missing_account".into()))
    }
}

fn check_vault(vault: &VaultAuth) -> RelayResult<()> {
    if !is_base64url(&vault.vault_id, 22) {
        return Err(RelayError::BadRequest("invalid_vault_id".into()));
    }
    if !is_base64url(&vault.token, 43) {
        return Err(RelayError::BadRequest("invalid_token".into()));
    }
    Ok(())
}

fn base64_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// The in-memory relay. Clones share one state, so a test keeps a handle while the engine owns
/// another.
#[derive(Clone, Default)]
pub struct FakeRelay {
    state: Arc<Mutex<FakeState>>,
}

impl FakeRelay {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().expect("fake relay state")
    }

    /// Makes the next call of `route` fail with `error` before it does anything (failures queue
    /// up, one per call).
    pub fn fail_next(&self, route: Route, error: RelayError) {
        self.lock()
            .failures
            .entry(route as usize)
            .or_default()
            .push_back(error);
    }

    /// Makes every call of `route` fail with `error` from now on (`None` clears it), unlike
    /// `fail_next`. A failure queued with `fail_next` comes first.
    pub fn set_failing(&self, route: Route, error: Option<RelayError>) {
        let mut state = self.lock();
        match error {
            Some(error) => state.failing.insert(route as usize, error),
            None => state.failing.remove(&(route as usize)),
        };
    }

    /// The payment page checkout answers: `base` followed by the plan's wire name (default
    /// `https://checkout.example.test/`).
    pub fn set_checkout_base(&self, base: &str) {
        self.lock().checkout_base = Some(base.to_string());
    }

    /// A self-hosted relay (Moony-relay `SELF_HOSTED=1`, ruling SH-1): checkout answers
    /// `409 self_hosted`, and an account is paid only once the test says so (`set_paid`, standing
    /// for an operator who added or unblocked the person).
    pub fn set_self_hosted(&self, self_hosted: bool) {
        self.lock().self_hosted = self_hosted;
    }

    /// What `server_info` answers from now on; `None` is an old relay that does not know the
    /// route (`BadRequest("not_found")`). Default: protocol 3, self-hosted, version 1.0.0, with
    /// blobs.
    pub fn set_server_info(&self, info: Option<ServerInfo>) {
        self.lock().server_info = info;
    }

    /// A person the server's operator added: an account that is active until year 9999.
    pub fn add_person(&self) -> String {
        let mut state = self.lock();
        state.next_account += 1;
        let number = format!("{:016}", 4_000_000_000_000_000u64 + state.next_account);
        state.accounts.insert(
            number.clone(),
            Account {
                paid_until: PERSON_PAID_UNTIL,
                vaults: Vec::new(),
            },
        );
        number
    }

    /// Blocks a person: `paid_until` is the moment of blocking (`at`, unix seconds).
    pub fn block(&self, account: &str, at: i64) {
        self.set_paid_until(account, at);
    }

    /// Unblocks a person: active until year 9999 again.
    pub fn unblock(&self, account: &str) {
        self.set_paid_until(account, PERSON_PAID_UNTIL);
    }

    /// The operator removes a person: the account and its vaults are gone.
    pub fn remove_account(&self, account: &str) {
        let mut state = self.lock();
        state.accounts.remove(account);
        state.vaults.retain(|_, vault| vault.account != account);
    }

    /// A self-hosted server with registration closed: `create_account` answers
    /// `RegistrationClosed`.
    pub fn set_registration_closed(&self, closed: bool) {
        self.lock().registration_closed = closed;
    }

    /// The next push that succeeds stores its ops, then answers `Network("timeout")`.
    pub fn lose_next_push_answer(&self) {
        self.lock().lose_push_answer = true;
    }

    /// Paid: `paid_until` a year ahead; unpaid: a day ago (writes are refused, reads are still in
    /// the 30-day grace). Creates the account if the test names one the relay has not seen.
    pub fn set_paid(&self, account: &str, paid: bool) {
        let paid_until = if paid {
            now() + PAID_FOR_SECONDS
        } else {
            now() - 86_400
        };
        self.set_paid_until(account, paid_until);
    }

    /// Lowers the per-vault op byte quota (the real one is 256 MiB), so a test can reach it.
    pub fn set_op_quota(&self, bytes: u64) {
        self.lock().op_quota = Some(bytes);
    }

    /// An exact `paid_until` (unix seconds), for tests of the read grace.
    pub fn set_paid_until(&self, account: &str, paid_until: i64) {
        self.lock()
            .accounts
            .entry(account.to_string())
            .or_default()
            .paid_until = paid_until;
    }

    /// Prunes the ops the relay's housekeeping would drop: the floor rises to `floor`, but never
    /// past the newest snapshot (nothing is pruned while there is none). Pruned op IDs are
    /// forgotten.
    ///
    /// Acts on every vault the fake holds (tests use one).
    pub fn prune_to(&self, floor: u64) {
        let mut state = self.lock();
        for vault in state.vaults.values_mut() {
            let Some(snapshot) = &vault.snapshot else {
                continue;
            };
            let floor = floor.min(snapshot.seq).max(vault.floor);
            vault.floor = floor;
            vault.ops.retain(|op| op.seq > floor);
            vault.dropped.retain(|seq| *seq > floor);
            vault.bytes = vault.ops.iter().map(|op| op.ct.len() as u64).sum();
        }
    }

    /// A dishonest relay: the op stays in the log (and counts towards `head`), but pulls never
    /// return it.
    ///
    /// Single-vault helper: `seq` is looked up in every vault the fake holds (tests use one).
    pub fn drop_op(&self, seq: u64) {
        let mut state = self.lock();
        for vault in state.vaults.values_mut() {
            if vault.ops.iter().any(|op| op.seq == seq) {
                vault.dropped.insert(seq);
            }
        }
    }

    /// A relay that lost op `seq` after acknowledging it (it never stored it durably): the op
    /// leaves the log and its ID is forgotten, so a push of the same op stores it again under a new
    /// sequence number. The head stays. This is not a relay restored from an older backup: that
    /// one's head goes back too (below a device's cursor), which no knob here models.
    ///
    /// Single-vault helper: `seq` is looked up in every vault the fake holds (tests use one).
    pub fn forget_op(&self, seq: u64) {
        let mut state = self.lock();
        for vault in state.vaults.values_mut() {
            vault.ops.retain(|op| op.seq != seq);
            vault.dropped.remove(&seq);
            vault.bytes = vault.ops.iter().map(|op| op.ct.len() as u64).sum();
        }
    }

    /// A dishonest relay: from now on (until switched off) every push that passes its checks
    /// answers that it accepted all its ops, and stores none of them.
    pub fn set_swallow_pushes(&self, swallow: bool) {
        self.lock().swallow_pushes = swallow;
    }

    /// A dishonest relay: serves op `seq` again under a new sequence number (the same ID and
    /// ciphertext at `head + 1`).
    ///
    /// Single-vault helper: `seq` is looked up in every vault the fake holds (tests use one).
    pub fn replay_op(&self, seq: u64) {
        let mut state = self.lock();
        for vault in state.vaults.values_mut() {
            let Some(original) = vault.ops.iter().find(|op| op.seq == seq).cloned() else {
                continue;
            };
            vault.head += 1;
            vault.bytes += original.ct.len() as u64;
            vault.ops.push(PulledOp {
                seq: vault.head,
                ..original
            });
        }
    }

    /// The ops the relay holds, in sequence order, except the ones it was told to drop.
    ///
    /// Single-vault helper: the ops of every vault the fake holds, merged by `seq` (tests use one).
    pub fn ops(&self) -> Vec<PulledOp> {
        let state = self.lock();
        let mut ops: Vec<PulledOp> = state
            .vaults
            .values()
            .flat_map(|v| v.ops.iter().filter(|op| !v.dropped.contains(&op.seq)))
            .cloned()
            .collect();
        ops.sort_by_key(|op| op.seq);
        ops
    }

    /// Per push call, the op IDs it carried, in order (including the calls a fault switch
    /// failed).
    pub fn pushed_ids(&self) -> Vec<Vec<String>> {
        self.lock().pushed_ids.clone()
    }

    /// Every call made, in order (including the ones a fault switch failed).
    pub fn calls(&self) -> Vec<Route> {
        self.lock().calls.clone()
    }

    /// Per push: how many ops it carried and their base64 size, whatever the answer was.
    pub fn push_sizes(&self) -> Vec<(usize, usize)> {
        self.lock().push_sizes.clone()
    }

    /// The newest snapshot's sequence number, if any.
    ///
    /// Single-vault helper: the first snapshot found in any vault (tests use one).
    pub fn snapshot_seq(&self) -> Option<u64> {
        self.lock()
            .vaults
            .values()
            .find_map(|v| v.snapshot.as_ref().map(|s| s.seq))
    }

    /// The token rotation counter `vault_info` answers for `vault_id` from now on (a re-keyed
    /// vault; the token stays the one the vault was created with).
    pub fn set_rotation(&self, vault_id: &str, rotation: u32) {
        if let Some(vault) = self.lock().vaults.get_mut(vault_id) {
            vault.rotation = rotation;
        }
    }

    /// Moves the fake's clock `seconds` ahead: retired tokens and key-change bundles expire 30 days
    /// after their rotation on this clock.
    pub fn advance_clock(&self, seconds: i64) {
        self.lock().clock_offset += seconds;
    }

    /// The bundle of rotation `rotation`, as stored (a test reads what the server holds).
    pub fn stored_rekey(&self, vault_id: &str, rotation: u32) -> Option<Vec<u8>> {
        let state = self.lock();
        let now = state.now();
        state
            .vaults
            .get(vault_id)?
            .rekeys
            .iter()
            .find(|r| r.rotation == rotation && r.at + PREVIOUS_TOKEN_SECONDS > now)
            .map(|r| r.bundle.clone())
    }

    /// A relay colluding with a removed device: from now on (until switched off) a retired token
    /// pushes (and uploads snapshots) as if it were current. Only rotations still need the
    /// current token.
    pub fn set_accept_retired_writes(&self, accept: bool) {
        self.lock().accept_retired_writes = accept;
    }

    /// A relay that also serves a retired token's reads (pull, snapshot, blobs), which an honest
    /// protocol-3 server refuses (`token_read_only`): for a server that hides a key change from a
    /// lagging device. The vault's info and the key changes need no help.
    pub fn set_accept_retired_reads(&self, accept: bool) {
        self.lock().accept_retired_reads = accept;
    }

    /// A relay colluding with a removed device (audit 2026-11-20, R2-H1): the next rotation
    /// answers, and records with its bundle, a head `by` above the vault's real one, and the
    /// vault's head moves there, so every later write of the current token lands above it. The
    /// positions in between stay free: with `set_accept_retired_writes`, a retired token's
    /// writes fill them in order.
    pub fn lift_next_rotation_head(&self, by: u64) {
        self.lock().rotation_head_lift = by;
    }

    /// A lying relay: `rekeys` reports `head` as the vault's head at rotation `rotation`.
    pub fn set_rekey_head(&self, vault_id: &str, rotation: u32, head: u64) {
        if let Some(vault) = self.lock().vaults.get_mut(vault_id) {
            for rekey in vault.rekeys.iter_mut().filter(|r| r.rotation == rotation) {
                rekey.head = head;
            }
        }
    }

    /// A lying relay: stores `bundle` as the next key change's (the rotation counter moves on,
    /// the head is the vault's) without rotating the token, so no key the bundle carries has a
    /// token here.
    pub fn insert_rekey(&self, vault_id: &str, bundle: &[u8]) {
        let mut state = self.lock();
        let now = state.now();
        if let Some(vault) = state.vaults.get_mut(vault_id) {
            vault.rotation += 1;
            vault.rekeys.push(StoredRekey {
                rotation: vault.rotation,
                head: vault.head,
                at: now,
                bundle: bundle.to_vec(),
            });
        }
    }

    /// A lying relay: `rekeys` answers `answer(after)` (any page, unchecked) until cleared.
    pub fn set_rekeys_answer(&self, answer: Option<RekeysAnswer>) {
        self.lock().rekeys_answer = answer;
    }

    /// Serves an op `drop_op` hid again.
    pub fn undrop_op(&self, seq: u64) {
        for vault in self.lock().vaults.values_mut() {
            vault.dropped.remove(&seq);
        }
    }

    /// A server restored from an older backup: the vault's head (and the ops past it) go back to
    /// `head`. Never raises a head.
    pub fn roll_back_head(&self, vault_id: &str, head: u64) {
        if let Some(vault) = self.lock().vaults.get_mut(vault_id) {
            if head < vault.head {
                vault.head = head;
                vault.ops.retain(|op| op.seq <= head);
                vault.dropped.retain(|seq| *seq <= head);
                vault.bytes = vault.ops.iter().map(|op| op.ct.len() as u64).sum();
            }
        }
    }

    /// The relay forgets the vault (as after the lapsed-vault wipe): later calls get
    /// `VaultNotFound`.
    pub fn wipe_vault(&self, vault_id: &str) {
        let mut state = self.lock();
        if let Some(vault) = state.vaults.remove(vault_id) {
            if let Some(account) = state.accounts.get_mut(&vault.account) {
                account.vaults.retain(|id| id != vault_id);
            }
        }
    }
}

impl FakeState {
    /// Records the call and takes the failure queued for it, if any.
    fn enter(&mut self, route: Route) -> RelayResult<()> {
        self.calls.push(route);
        let queued = self
            .failures
            .get_mut(&(route as usize))
            .and_then(|queue| queue.pop_front());
        match queued.or_else(|| self.failing.get(&(route as usize)).cloned()) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// The fake's clock: the real one plus `advance_clock`.
    fn now(&self) -> i64 {
        now() + self.clock_offset
    }

    fn paid_until(&self, account: &str) -> i64 {
        self.accounts.get(account).map_or(0, |a| a.paid_until)
    }

    /// `Vaults.verdict`: the vault exists; the token is its current one, or one rotated away less
    /// than 30 days ago (which may only look up the vault's info and the key changes, unless the
    /// relay colludes: `set_accept_retired_writes`, `set_accept_retired_reads`); the subscription
    /// is live (write) or within the read grace (read, lookup); `Owner` needs no subscription.
    fn authorize(&self, vault: &VaultAuth, access: Access) -> RelayResult<&Vault> {
        let stored = self
            .vaults
            .get(&vault.vault_id)
            .ok_or(RelayError::VaultNotFound)?;
        let now = self.now();
        if stored.token != *vault.token {
            let previous = stored
                .prev_tokens
                .iter()
                .any(|(token, at)| *token == *vault.token && at + PREVIOUS_TOKEN_SECONDS > now);
            if !previous {
                return Err(RelayError::Unauthorized);
            }
            let colluding = match access {
                Access::Write => self.accept_retired_writes,
                Access::Read => self.accept_retired_reads,
                Access::Lookup | Access::Owner => false,
            };
            if access != Access::Lookup && !colluding {
                return Err(RelayError::TokenReadOnly);
            }
        }
        let paid_until = self.paid_until(&stored.account);
        let entitled = match access {
            Access::Owner => true,
            Access::Write => paid_until > now,
            Access::Lookup | Access::Read => paid_until + READ_GRACE_SECONDS > now,
        };
        if entitled {
            Ok(stored)
        } else {
            Err(RelayError::PaymentRequired)
        }
    }

    fn vault_mut(&mut self, vault_id: &str) -> &mut Vault {
        self.vaults.get_mut(vault_id).expect("authorized vault")
    }
}

impl Relay for FakeRelay {
    fn server_info(&self) -> impl Future<Output = RelayResult<ServerInfo>> + Send {
        let result = (|| {
            let mut state = self.lock();
            state.enter(Route::ServerInfo)?;
            state
                .server_info
                .clone()
                .ok_or_else(|| RelayError::BadRequest("not_found".into()))
        })();
        async move { result }
    }

    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send {
        let result = (|| {
            let mut state = self.lock();
            state.enter(Route::CreateAccount)?;
            if state.registration_closed {
                return Err(RelayError::RegistrationClosed);
            }
            state.next_account += 1;
            let number = format!("{:016}", 4_000_000_000_000_000u64 + state.next_account);
            state.accounts.insert(number.clone(), Account::default());
            Ok(number)
        })();
        async move { result }
    }

    fn account_status(
        &self,
        account: &str,
    ) -> impl Future<Output = RelayResult<AccountStatus>> + Send {
        let result = (|| {
            check_account(account)?;
            let mut state = self.lock();
            state.enter(Route::AccountStatus)?;
            let stored = state
                .accounts
                .get(account)
                .ok_or(RelayError::AccountNotFound)?;
            Ok(AccountStatus {
                paid_until: (stored.paid_until != 0).then_some(stored.paid_until),
                active: stored.paid_until > now(),
                vaults: stored.vaults.len() as u32,
            })
        })();
        async move { result }
    }

    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl Future<Output = RelayResult<String>> + Send {
        let result = (|| {
            check_account(account)?;
            let mut state = self.lock();
            state.enter(Route::Checkout)?;
            if state.self_hosted {
                return Err(RelayError::SelfHosted);
            }
            if !state.accounts.contains_key(account) {
                return Err(RelayError::AccountNotFound);
            }
            let base = state
                .checkout_base
                .as_deref()
                .unwrap_or("https://checkout.example.test/");
            Ok(format!("{base}{}", plan.as_wire()))
        })();
        async move { result }
    }

    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<bool>> + Send {
        let result = (|| {
            check_vault(vault)?;
            check_account(account)?;
            let mut state = self.lock();
            state.enter(Route::CreateVault)?;
            if let Some(existing) = state.vaults.get(&vault.vault_id) {
                let same = existing.account == account && existing.token == *vault.token;
                return if same {
                    Ok(false)
                } else {
                    Err(RelayError::VaultExists)
                };
            }
            let stored = state
                .accounts
                .get_mut(account)
                .ok_or(RelayError::AccountNotFound)?;
            if stored.paid_until <= now() {
                return Err(RelayError::PaymentRequired);
            }
            if stored.vaults.len() >= MAX_VAULTS_PER_ACCOUNT {
                return Err(RelayError::VaultLimitReached);
            }
            stored.vaults.push(vault.vault_id.clone());
            state.vaults.insert(
                vault.vault_id.clone(),
                Vault {
                    account: account.to_string(),
                    token: (*vault.token).clone(),
                    head: 0,
                    floor: 0,
                    ops: Vec::new(),
                    dropped: HashSet::new(),
                    bytes: 0,
                    snapshot: None,
                    rotation: 0,
                    prev_tokens: Vec::new(),
                    rekeys: Vec::new(),
                    kit: None,
                    kits_retired: Vec::new(),
                    kit_failures: Vec::new(),
                    blobs: blobs::VaultBlobs::new(),
                    gap: None,
                },
            );
            Ok(true)
        })();
        async move { result }
    }

    fn vault_info(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<VaultInfo>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::VaultInfo)?;
            let stored = state.authorize(vault, Access::Lookup)?;
            Ok(VaultInfo {
                head: stored.head,
                floor: stored.floor,
                snapshot_seq: stored.snapshot.as_ref().map(|s| s.seq),
                paid_until: state.paid_until(&stored.account),
                bytes: stored.bytes,
                rotation: stored.rotation,
                kit: stored.kit.as_ref().map(|kit| KitInfo {
                    since: kit.since,
                    last_read: kit.last_read,
                }),
            })
        })();
        async move { result }
    }

    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl Future<Output = RelayResult<PushReceipt>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            let wire_bytes: usize = ops.iter().map(|op| base64_len(op.ct.len())).sum();
            state.push_sizes.push((ops.len(), wire_bytes));
            state
                .pushed_ids
                .push(ops.iter().map(|op| op.id.clone()).collect());
            state.enter(Route::Push)?;
            if wire_bytes > MAX_PUSH_BODY_BYTES {
                return Err(RelayError::BadRequest("body_too_large".into()));
            }
            let valid_id = |id: &str| {
                (1..=64).contains(&id.len())
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            };
            if ops.is_empty()
                || ops.len() > MAX_OPS_PER_PUSH
                || ops
                    .iter()
                    .any(|op| !valid_id(&op.id) || op.ct.is_empty() || op.ct.len() > MAX_OP_BYTES)
            {
                return Err(RelayError::BadRequest("invalid_ops".into()));
            }
            let stored = state.authorize(vault, Access::Write)?;
            let incoming: u64 = ops.iter().map(|op| op.ct.len() as u64).sum();
            if stored.bytes + incoming > state.op_quota.unwrap_or(MAX_VAULT_OP_BYTES) {
                return Err(RelayError::QuotaExceeded);
            }
            let lose_answer = std::mem::take(&mut state.lose_push_answer);
            if state.swallow_pushes {
                let head = state.vault_mut(&vault.vault_id).head;
                return Ok(PushReceipt {
                    head,
                    accepted: ops.len() as u32,
                });
            }
            let stored = state.vault_mut(&vault.vault_id);
            // A colluding relay places a retired token's writes below the head it raised.
            let retired = stored.token != *vault.token;
            let mut accepted = 0u32;
            for op in ops {
                if stored.ops.iter().any(|known| known.id == op.id) {
                    continue;
                }
                let free = stored.gap.filter(|(next, last)| retired && next <= last);
                let seq = match free {
                    Some((next, last)) => {
                        stored.gap = Some((next + 1, last));
                        next
                    }
                    None => {
                        stored.head += 1;
                        stored.head
                    }
                };
                stored.bytes += op.ct.len() as u64;
                let at = stored.ops.partition_point(|known| known.seq < seq);
                stored.ops.insert(
                    at,
                    PulledOp {
                        seq,
                        id: op.id.clone(),
                        ct: op.ct.clone(),
                    },
                );
                accepted += 1;
            }
            if lose_answer {
                return Err(RelayError::Network("timeout".into()));
            }
            Ok(PushReceipt {
                head: stored.head,
                accepted,
            })
        })();
        async move { result }
    }

    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl Future<Output = RelayResult<PullPage>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::Pull)?;
            if limit == 0 {
                return Err(RelayError::BadRequest("invalid_limit".into()));
            }
            let limit = limit.min(MAX_PULL_LIMIT) as usize;
            let stored = state.authorize(vault, Access::Read)?;
            if after < stored.floor {
                return Err(RelayError::SnapshotRequired);
            }
            // The page is cut like the relay's (limit, then ~4 MiB of ciphertext, always at
            // least one op) but counts only the ops the relay still serves: a dropped op leaves
            // a gap in the sequence numbers, never an empty page with `more` set.
            let mut page = Vec::new();
            let mut bytes = 0usize;
            let mut served = stored
                .ops
                .iter()
                .filter(|op| op.seq > after && !stored.dropped.contains(&op.seq))
                .peekable();
            while let Some(op) = served.peek() {
                if page.len() >= limit || (!page.is_empty() && bytes + op.ct.len() > MAX_PULL_BYTES)
                {
                    break;
                }
                bytes += op.ct.len();
                page.push((*op).clone());
                served.next();
            }
            Ok(PullPage {
                ops: page,
                head: stored.head,
                more: served.peek().is_some(),
            })
        })();
        async move { result }
    }

    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::PutSnapshot)?;
            if bytes.is_empty() {
                return Err(RelayError::BadRequest("length_required".into()));
            }
            if bytes.len() > MAX_SNAPSHOT_BYTES {
                return Err(RelayError::SnapshotTooLarge);
            }
            let stored = state.authorize(vault, Access::Write)?;
            if seq > stored.head {
                return Err(RelayError::BadRequest("snapshot_ahead_of_head".into()));
            }
            if stored.snapshot.as_ref().is_some_and(|s| seq < s.seq) {
                return Err(RelayError::SnapshotOutdated);
            }
            if state.no_room {
                return Err(RelayError::InsufficientStorage);
            }
            state.vault_mut(&vault.vault_id).snapshot = Some(SnapshotBlob { seq, bytes });
            Ok(())
        })();
        async move { result }
    }

    fn get_snapshot(
        &self,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<SnapshotBlob>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::GetSnapshot)?;
            let stored = state.authorize(vault, Access::Read)?;
            stored.snapshot.clone().ok_or(RelayError::NoSnapshot)
        })();
        async move { result }
    }

    fn delete_vault(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<()>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::DeleteVault)?;
            let account = state.authorize(vault, Access::Owner)?.account.clone();
            state.vaults.remove(&vault.vault_id);
            if let Some(stored) = state.accounts.get_mut(&account) {
                stored.vaults.retain(|id| id != &vault.vault_id);
            }
            Ok(())
        })();
        async move { result }
    }

    fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> impl Future<Output = RelayResult<Rotated>> + Send {
        let result = self.rotate(vault, new_token, bundle, None);
        async move { result }
    }

    fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: KitRotation<'_>,
    ) -> impl Future<Output = RelayResult<Rotated>> + Send {
        let result = self.rotate(vault, new_token, bundle, Some(kit));
        async move { result }
    }

    fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> impl Future<Output = RelayResult<bool>> + Send {
        let result = (|| {
            check_vault(vault)?;
            if !is_base64url(kit_token, 43) {
                return Err(RelayError::BadRequest("invalid_kit_token".into()));
            }
            check_kit_envelope(envelope)?;
            let mut state = self.lock();
            state.enter(Route::PutKit)?;
            let stored = state.authorize(vault, Access::Owner)?;
            let now = state.now();
            if stored
                .kit
                .as_ref()
                .is_some_and(|kit| kit.token == kit_token)
            {
                // A retried setup: the envelope is replaced, the Kit stays.
                let stored = state.vault_mut(&vault.vault_id);
                if let Some(kit) = stored.kit.as_mut() {
                    kit.envelope = envelope.to_vec();
                    kit.updated_at = now;
                }
                return Ok(false);
            }
            if stored.kit_token_conflicts(kit_token, None) {
                return Err(RelayError::BadRequest("invalid_kit_token".into()));
            }
            if stored.kit.is_some() {
                return Err(RelayError::KitExists);
            }
            state.vault_mut(&vault.vault_id).kit = Some(StoredKit {
                token: kit_token.to_string(),
                envelope: envelope.to_vec(),
                since: now,
                updated_at: now,
                last_read: None,
            });
            Ok(true)
        })();
        async move { result }
    }

    fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> impl Future<Output = RelayResult<KitRead>> + Send {
        let result = (|| {
            if !is_base64url(vault_id, 22) {
                return Err(RelayError::BadRequest("invalid_vault_id".into()));
            }
            if !is_base64url(kit_token, 43) {
                return Err(RelayError::BadRequest("invalid_kit_token".into()));
            }
            let mut state = self.lock();
            state.enter(Route::GetKit)?;
            let now = state.now();
            let stored = state
                .vaults
                .get(vault_id)
                .ok_or(RelayError::VaultNotFound)?;
            // A replaced Kit token always hears so: never counted, never `RateLimited`.
            if let Some((_, at)) = stored.kits_retired.iter().find(|(t, _)| t == kit_token) {
                return Err(RelayError::KitReplaced { replaced_at: *at });
            }
            let right = stored
                .kit
                .as_ref()
                .is_some_and(|kit| kit.token == kit_token);
            if !right {
                // Failures only: past the limit more failures are `RateLimited`, while the right
                // token still reads.
                let stored = state.vault_mut(vault_id);
                stored
                    .kit_failures
                    .retain(|at| at + KIT_FAILURE_WINDOW_SECONDS > now);
                stored.kit_failures.push(now);
                return Err(if stored.kit_failures.len() > MAX_KIT_FAILURES {
                    RelayError::RateLimited
                } else {
                    RelayError::Unauthorized
                });
            }
            // The read rule: until 30 days after `paid_until`.
            if state.paid_until(&stored.account) + READ_GRACE_SECONDS <= now {
                return Err(RelayError::PaymentRequired);
            }
            let kit = state
                .vault_mut(vault_id)
                .kit
                .as_mut()
                .ok_or(RelayError::Unauthorized)?;
            kit.last_read = Some(now - now.rem_euclid(60));
            Ok(KitRead {
                envelope: kit.envelope.clone(),
                since: kit.since,
                updated_at: kit.updated_at,
            })
        })();
        async move { result }
    }

    fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> impl Future<Output = RelayResult<RekeyPage>> + Send {
        let result = (|| {
            check_vault(vault)?;
            let mut state = self.lock();
            state.enter(Route::Rekeys)?;
            if limit == 0 {
                return Err(RelayError::BadRequest("invalid_limit".into()));
            }
            let limit = limit.min(MAX_REKEY_PAGE) as usize;
            let stored = state.authorize(vault, Access::Lookup)?;
            if let Some(answer) = &state.rekeys_answer {
                return Ok(answer(after));
            }
            let now = state.now();
            let mut live = stored
                .rekeys
                .iter()
                .filter(|r| r.rotation > after && r.at + PREVIOUS_TOKEN_SECONDS > now)
                .peekable();
            let mut page = Vec::new();
            let mut bytes = 0usize;
            while let Some(next) = live.peek() {
                if page.len() >= limit
                    || (!page.is_empty() && bytes + next.bundle.len() > MAX_REKEY_PAGE_BYTES)
                {
                    break;
                }
                bytes += next.bundle.len();
                page.push(Rekey {
                    rotation: next.rotation,
                    head: next.head,
                    at: next.at,
                    bundle: next.bundle.clone(),
                });
                live.next();
            }
            Ok(RekeyPage {
                rekeys: page,
                more: live.peek().is_some(),
            })
        })();
        async move { result }
    }

    fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        let result = self.blob_put(vault, blob, sealed_len, body);
        async move { result }
    }

    fn head_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<Option<u64>>> + Send {
        let result = self.blob_head(vault, blob);
        async move { result }
    }

    fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> impl Future<Output = RelayResult<Delivery>> + Send {
        let result = self.blob_get(vault, blob, sink);
        async move { result }
    }

    fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> impl Future<Output = RelayResult<BlobPage>> + Send {
        let result = self.blob_list(vault, after, limit);
        async move { result }
    }

    fn delete_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        let result = self.blob_delete(vault, blob);
        async move { result }
    }
}

fn check_kit_envelope(envelope: &[u8]) -> RelayResult<()> {
    if envelope.is_empty() {
        return Err(RelayError::BadRequest("invalid_kit".into()));
    }
    if envelope.len() > MAX_KIT_ENVELOPE {
        return Err(RelayError::BadRequest("kit_too_large".into()));
    }
    Ok(())
}

impl Vault {
    /// Whether `kit_token` may not become the Kit's: it equals the current vault token or any
    /// previous one (whatever its age), `new_vault_token` (a rotation's), the current Kit token
    /// or a replaced one.
    fn kit_token_conflicts(&self, kit_token: &str, new_vault_token: Option<&str>) -> bool {
        self.token == kit_token
            || self.prev_tokens.iter().any(|(token, _)| token == kit_token)
            || new_vault_token == Some(kit_token)
            || self.kit.as_ref().is_some_and(|kit| kit.token == kit_token)
            || self
                .kits_retired
                .iter()
                .any(|(token, _)| token == kit_token)
    }
}

impl FakeRelay {
    /// `rotate_token` and `rotate_token_with_kit`: the checks in the server's order, then the
    /// vault token, the bundle, the envelope and the Kit token change together.
    fn rotate(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: Option<KitRotation<'_>>,
    ) -> RelayResult<Rotated> {
        check_vault(vault)?;
        // The argument checks of `HttpRelay`, before anything is sent.
        if !is_base64url(new_token, 43) {
            return Err(RelayError::BadRequest("invalid_token".into()));
        }
        if bundle.is_empty() {
            return Err(RelayError::BadRequest("invalid_rekey".into()));
        }
        if bundle.len() > MAX_REKEY_BUNDLE {
            return Err(RelayError::BadRequest("rekey_too_large".into()));
        }
        if let Some(kit) = &kit {
            check_kit_envelope(kit.envelope)?;
            if kit.token.is_some_and(|token| !is_base64url(token, 43)) {
                return Err(RelayError::BadRequest("invalid_kit_token".into()));
            }
        }
        let mut state = self.lock();
        state.enter(Route::RotateToken)?;
        // Only the current token rotates (a retired one is `TokenReadOnly`, so of two
        // concurrent rotations the second fails); no subscription is needed.
        let stored = state.authorize(vault, Access::Owner)?;
        let now = state.now();
        // A token that was ever the vault's (and is still remembered) cannot come back, nor can
        // a Kit token be the vault's.
        let known = stored.token == new_token
            || stored
                .prev_tokens
                .iter()
                .any(|(token, at)| token == new_token && at + PREVIOUS_TOKEN_SECONDS > now)
            || stored.kit.as_ref().is_some_and(|k| k.token == new_token)
            || stored
                .kits_retired
                .iter()
                .any(|(token, _)| token == new_token);
        if known {
            return Err(RelayError::BadRequest("invalid_token".into()));
        }
        if stored.kit.is_some() && kit.is_none() {
            return Err(RelayError::BadRequest("kit_required".into()));
        }
        if let Some(kit) = &kit {
            match kit.token {
                // An envelope with no Kit token on a vault without a Kit belongs to no Kit.
                None if stored.kit.is_none() => {
                    return Err(RelayError::BadRequest("invalid_kit".into()));
                }
                Some(token) if stored.kit_token_conflicts(token, Some(new_token)) => {
                    return Err(RelayError::BadRequest("invalid_kit_token".into()));
                }
                _ => {}
            }
        }
        let kept = stored
            .rekeys
            .iter()
            .filter(|r| r.at + PREVIOUS_TOKEN_SECONDS > now)
            .count();
        if kept >= MAX_REKEYS_KEPT {
            return Err(RelayError::TooManyRotations);
        }
        let lift = std::mem::take(&mut state.rotation_head_lift);
        let stored = state.vault_mut(&vault.vault_id);
        if lift > 0 {
            stored.gap = Some((stored.head + 1, stored.head + lift));
            stored.head += lift;
        }
        // A token retired over 30 days ago may still be remembered: it is current again.
        stored.prev_tokens.retain(|(token, _)| token != new_token);
        let retired = std::mem::replace(&mut stored.token, new_token.to_string());
        stored.prev_tokens.push((retired, now));
        stored.rotation += 1;
        let rotation = stored.rotation;
        let head = stored.head;
        stored.rekeys.push(StoredRekey {
            rotation,
            head,
            at: now,
            bundle: bundle.to_vec(),
        });
        if let Some(kit) = kit {
            match kit.token {
                Some(token) => {
                    if let Some(old) = stored.kit.take() {
                        stored.kits_retired.push((old.token, now));
                        let excess = stored.kits_retired.len().saturating_sub(MAX_KITS_RETIRED);
                        stored.kits_retired.drain(..excess);
                    }
                    stored.kit = Some(StoredKit {
                        token: token.to_string(),
                        envelope: kit.envelope.to_vec(),
                        since: now,
                        updated_at: now,
                        last_read: None,
                    });
                }
                None => {
                    if let Some(current) = stored.kit.as_mut() {
                        current.envelope = kit.envelope.to_vec();
                        current.updated_at = now;
                    }
                }
            }
        }
        let lose_answer = state
            .lose_answers
            .iter()
            .position(|r| *r == Route::RotateToken);
        if let Some(index) = lose_answer {
            state.lose_answers.remove(index);
            return Err(RelayError::Server(503));
        }
        Ok(Rotated { rotation, head })
    }
}

mod blobs;
#[cfg(test)]
mod kit_tests;
mod mailbox;
#[cfg(test)]
mod tests;
