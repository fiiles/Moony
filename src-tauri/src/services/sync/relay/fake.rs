//! `FakeRelay`: an in-memory `Relay` for engine tests. It follows the real relay (Moony-relay
//! `src/vault.ts`, `src/account.ts`, `src/index.ts`) and answers with exactly the `RelayError`s
//! `HttpRelay` produces for the same situation, including the argument checks `HttpRelay` makes
//! before it sends anything. On top of that it has fault switches: a failure for the next call of
//! a route, a lost push answer, pruning, and two dishonest-relay knobs (`drop_op`, `replay_op`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use super::{
    AccountStatus, CheckoutPlan, OutgoingOp, PullPage, PulledOp, PushReceipt, Relay, RelayError,
    RelayResult, SnapshotBlob, VaultInfo,
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
/// How far `set_paid(.., true)` pushes `paid_until`.
const PAID_FOR_SECONDS: i64 = 365 * 86_400;

/// The relay calls a test can make fail or look up in `calls()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
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
}

#[derive(Default)]
struct FakeState {
    accounts: HashMap<String, Account>,
    vaults: HashMap<String, Vault>,
    next_account: u64,
    failures: HashMap<usize, VecDeque<RelayError>>,
    lose_push_answer: bool,
    /// Test override of the per-vault op byte quota.
    op_quota: Option<u64>,
    calls: Vec<Route>,
    push_sizes: Vec<(usize, usize)>,
    /// What a checkout answers: this base followed by the plan (`set_checkout_base`).
    checkout_base: Option<String>,
    /// A self-hosted relay (`SELF_HOSTED=1`): checkout answers `SelfHosted`.
    self_hosted: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
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

    /// The payment page checkout answers: `base` followed by the plan's wire name (default
    /// `https://checkout.example.test/`).
    pub fn set_checkout_base(&self, base: &str) {
        self.lock().checkout_base = Some(base.to_string());
    }

    /// A self-hosted relay (Moony-relay `SELF_HOSTED=1`, ruling SH-1): checkout answers
    /// `409 self_hosted`, and an account is paid only once the test says so (the operator added it
    /// to `ALLOWED_ACCOUNTS`).
    pub fn set_self_hosted(&self, self_hosted: bool) {
        self.lock().self_hosted = self_hosted;
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
        match self
            .failures
            .get_mut(&(route as usize))
            .and_then(|queue| queue.pop_front())
        {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn paid_until(&self, account: &str) -> i64 {
        self.accounts.get(account).map_or(0, |a| a.paid_until)
    }

    /// `VaultDO.authorize`: the vault exists, the token is its current one, the subscription is
    /// live (write) or within the read grace (read); `Owner` needs neither.
    fn authorize(&self, vault: &VaultAuth, access: Access) -> RelayResult<&Vault> {
        let stored = self
            .vaults
            .get(&vault.vault_id)
            .ok_or(RelayError::VaultNotFound)?;
        if stored.token != *vault.token {
            return Err(RelayError::Unauthorized);
        }
        let paid_until = self.paid_until(&stored.account);
        let entitled = match access {
            Access::Owner => true,
            Access::Write => paid_until > now(),
            Access::Read => paid_until + READ_GRACE_SECONDS > now(),
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
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send {
        let result = (|| {
            let mut state = self.lock();
            state.enter(Route::CreateAccount)?;
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
            let stored = state.authorize(vault, Access::Read)?;
            Ok(VaultInfo {
                head: stored.head,
                floor: stored.floor,
                snapshot_seq: stored.snapshot.as_ref().map(|s| s.seq),
                paid_until: state.paid_until(&stored.account),
                bytes: stored.bytes,
                rotation: 0,
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
            let stored = state.vault_mut(&vault.vault_id);
            let mut accepted = 0u32;
            for op in ops {
                if stored.ops.iter().any(|known| known.id == op.id) {
                    continue;
                }
                stored.head += 1;
                stored.bytes += op.ct.len() as u64;
                stored.ops.push(PulledOp {
                    seq: stored.head,
                    id: op.id.clone(),
                    ct: op.ct.clone(),
                });
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
}

#[cfg(test)]
mod tests;
