//! Engine tests: a vault on a `FakeRelay` with device `A` (sync turned on) and device `B` (joined
//! from `A`'s plaintext snapshot), sharing one set of credentials bound before either starts, each behind a `TestConn` the
//! engine locks one step at a time. Envelopes are sealed as the push seals them (`seal_from`) or
//! forged from hand-made changesets (`forge`), and reach the relay through `push`.

mod backoff;
mod binding_failure;
mod echo;
mod gap_skip;
mod gaps;
mod join;
mod join_new;
mod pull;
mod push;
mod relay_faults;
mod repair;
mod replace_safety;
mod runtime;
mod runtime_loop;
mod scan;
mod setup;
mod snapshots;
mod went_back;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::types::Value;
use rusqlite::{params, Connection};

use super::ConnAccess;
use crate::error::{AppError, Result};
use crate::services::sync::changeset::{to_json, Changeset, RowChange, RowKey, SqlValue};
use crate::services::sync::envelope::{envelope_hash, op_id, seal_op};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::journal::{build_next, record_sealed};
use crate::services::sync::keys::{self, Credentials, VaultId, VaultKeys};
use crate::services::sync::registry::keys::{barrier_number, schema_number};
use crate::services::sync::relay::fake::FakeRelay;
use crate::services::sync::relay::{OutgoingOp, Relay};
use crate::services::sync::snapshot;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

pub const A: DeviceId = DeviceId([1; 16]);
pub const B: DeviceId = DeviceId([2; 16]);
/// A third device that only ever appears through forged envelopes.
pub const C: DeviceId = DeviceId([3; 16]);
pub const NOW: i64 = 1_772_323_200; // 2026-03-01

/// The profile's connection as the engine sees it. `locked` makes `with_conn` fail with
/// `AppError::Auth`, as a locked `Database` does.
#[derive(Clone)]
pub struct TestConn {
    pub conn: Arc<Mutex<Connection>>,
    pub locked: Arc<AtomicBool>,
    /// This profile's own round lock, so tests on other profiles never wait for it.
    pub rounds: Arc<tokio::sync::Mutex<()>>,
}

impl TestConn {
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: Arc::new(Mutex::new(conn)),
            locked: Arc::new(AtomicBool::new(false)),
            rounds: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    /// The connection, for a test's own reads and writes.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("test connection")
    }
}

impl ConnAccess for TestConn {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(AppError::Auth("Database is locked".into()));
        }
        f(&self.lock())
    }

    fn round_lock(&self) -> &tokio::sync::Mutex<()> {
        &self.rounds
    }
}

pub struct Vault {
    pub relay: FakeRelay,
    pub creds: Credentials,
    pub a: TestConn,
    pub b: TestConn,
}

/// A paid vault on a fresh `FakeRelay`: `A` turns sync on (its rule-pack keys are dropped from
/// the outbox, so the relay starts empty), `B` joins from `A`'s snapshot at relay sequence 0.
pub async fn vault() -> Vault {
    let relay = FakeRelay::new();
    let account = relay.create_account().await.expect("account");
    relay.set_paid(&account, true);
    let creds = Credentials {
        relay_url: "https://relay.test".into(),
        account,
        vault_id: VaultId::random().expect("vault id"),
        keys: VaultKeys::new_random().expect("vault keys"),
    };
    assert!(relay
        .create_vault(&creds.account, &creds.auth())
        .await
        .expect("vault"));

    // The credentials are bound first, as setup and pairing do (either order leaves the device
    // marked at its barrier).
    let a = migrated();
    keys::save_credentials(&a, &creds).expect("credentials A");
    state::enable(&a, A).expect("enable A");
    exec(&a, "DELETE FROM sync_outbox");
    let mut stream = Vec::new();
    snapshot::dump(&a, 0, &mut stream).expect("dump A");

    let b = migrated();
    keys::save_credentials(&b, &creds).expect("credentials B");
    snapshot::join(&b, B, &mut stream.as_slice(), NOW).expect("join B");

    Vault {
        relay,
        creds,
        a: TestConn::new(a),
        b: TestConn::new(b),
    }
}

pub fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

pub fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

pub fn institution(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO institutions (id, name) VALUES (?1, 'Bank ' || ?1)",
        [id],
    )
    .expect("insert institution");
}

pub fn has_institution(conn: &Connection, id: &str) -> bool {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM institutions WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )
    .expect("institution")
}

/// The next envelope of everything `conn` has queued, sealed as the push seals it: built,
/// sealed under the current epoch, recorded in the own log and kept in `sync_unsent`.
pub fn seal_from(conn: &Connection, creds: &Credentials) -> Vec<u8> {
    let tx = conn.unchecked_transaction().expect("transaction");
    let (built, refused) = build_next(&tx).expect("build_next");
    assert!(refused.is_empty(), "{refused:?}");
    let built = built.expect("something is queued");
    let json = to_json(&built.changeset).expect("json");
    let ct = forge_json(creds, &json);
    record_sealed(&tx, &built, envelope_hash(&ct)).expect("record_sealed");
    tx.execute(
        "INSERT INTO sync_unsent (seq, envelope) VALUES (?1, ?2)",
        params![built.changeset.seq as i64, ct],
    )
    .expect("unsent");
    tx.commit().expect("commit");
    ct
}

/// Pushes the envelopes in order (500 per call, the relay's limit).
pub async fn push(relay: &FakeRelay, creds: &Credentials, cts: &[Vec<u8>]) {
    for chunk in cts.chunks(500) {
        let ops: Vec<OutgoingOp> = chunk
            .iter()
            .map(|ct| OutgoingOp {
                id: op_id(ct),
                ct: ct.clone(),
            })
            .collect();
        let receipt = relay.push(&creds.auth(), &ops).await.expect("push");
        assert_eq!(receipt.accepted as usize, chunk.len());
    }
}

fn forge_json(creds: &Credentials, json: &[u8]) -> Vec<u8> {
    seal_op(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        json,
    )
    .expect("seal")
}

/// A hand-made changeset of `device` at this build's schema and barrier.
pub fn changeset(
    device: DeviceId,
    seq: u64,
    prev: Option<[u8; 32]>,
    rows: Vec<RowChange>,
) -> Changeset {
    Changeset {
        schema: schema_number(),
        barrier: barrier_number(),
        device,
        seq,
        prev,
        rows,
    }
}

/// `cs` sealed under the vault's current key.
pub fn forge(creds: &Credentials, cs: &Changeset) -> Vec<u8> {
    forge_json(creds, &to_json(cs).expect("json"))
}

/// A full-row insert of `table` with every column at `generation`.
pub fn row(table: &str, key: &str, generation: Clock, columns: &[(&str, SqlValue)]) -> RowChange {
    RowChange {
        table: table.to_string(),
        key: RowKey::Single(key.to_string()),
        generation,
        insert: true,
        deleted: None,
        columns: columns
            .iter()
            .map(|(name, value)| (name.to_string(), (value.clone(), generation)))
            .collect(),
        parents: BTreeMap::new(),
    }
}

pub fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

/// The consent of a confirmation that shows `device` as it stands now.
pub fn agreed(device: &TestConn) -> crate::services::sync::impact::ReplaceConsent {
    crate::services::sync::impact::replace_impact(&device.lock())
        .expect("impact")
        .consent()
}

pub fn cursor(conn: &Connection) -> u64 {
    state::load(conn).expect("state").expect("row").cursor
}

/// `(seq, hash, quarantined)` of `device`'s head.
pub fn head(conn: &Connection, device: DeviceId) -> Option<(i64, Vec<u8>, bool)> {
    conn.query_row(
        "SELECT seq, hash, quarantined FROM sync_heads WHERE device = ?1",
        [device.0.to_vec()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

/// Everything a pull may change: the `sync_state` row, the heads, the own log, the unsent
/// envelopes, the outbox and the synced row counts that the tests touch.
pub fn sync_rows(conn: &Connection) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT * FROM sync_state",
        "SELECT * FROM sync_heads ORDER BY device",
        "SELECT * FROM sync_own_log ORDER BY seq",
        "SELECT seq FROM sync_unsent ORDER BY seq",
        "SELECT tbl, pk FROM sync_outbox ORDER BY tbl, pk",
        "SELECT id FROM institutions ORDER BY id",
    ] {
        let mut stmt = conn.prepare(sql).expect(sql);
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                (0..columns)
                    .map(|i| r.get::<_, Value>(i).map(|v| format!("{v:?}")))
                    .collect::<std::result::Result<Vec<_>, _>>()
            })
            .expect(sql);
        for row in rows {
            out.push(format!("{sql}: {}", row.expect("row").join(" | ")));
        }
    }
    out
}
