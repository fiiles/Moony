//! The property test of removal and key changes (plan Task 14, revision Task 22): four devices
//! (A set up, B and C paired by A, D paired by B, a Kit made by A) and a seeded schedule of edits,
//! rounds, offline periods, removals, new Kits, devices that leave, and late vouches from the
//! copies of devices that left (a profile backup with the signing key, revision §3.1, R2). At the
//! end, with everyone online again:
//!
//! - the remaining devices' synced tables are equal (`test_support::divergence`), each holds
//!   every edit a remaining device made, and every envelope of every remaining device was applied
//!   once and in order on every other (its head there is the author's own last envelope);
//! - no removed device ever holds a key of an epoch at or after its removal (checked after every
//!   step on its profile, and at the end on every bundle the server stored), and none that left
//!   gets a key after it left;
//! - no excluded chain member (a late vouch, or a device it vouched for) is ever given a key, and
//!   every remaining device that pinned it leaves it out;
//! - the Kit opens the newest envelope with every key, and a newcomer joins with it after all the
//!   key changes and converges.
//!
//! The default runs eight fixed seeds in two tests of 40 steps each (seconds); the ignored
//! `wide` run takes 64 seeds of 80 steps, or the range `REKEY_PROPERTY_SEEDS=a..b`.

use std::collections::{BTreeMap, BTreeSet};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::*;
use crate::services::sync::identity::KitSecret;
use crate::services::sync::test_support::divergence;

const D: DeviceId = DeviceId([4; 16]);
const STEPS: usize = 40;
const WIDE_STEPS: usize = 80;

/// A device that left: what a profile backup made while sync was on still holds.
struct LeftCopy {
    seed: SignSeed,
    creds: Credentials,
    /// The epoch it left at (its copy's key is that epoch's until the next change).
    epoch: u32,
}

/// A device a copy vouched for (or one such a device vouched for), known only by its row.
struct Phantom {
    id: DeviceId,
    seed: SignSeed,
    secret: StaticSecret,
    /// The device that left whose copy started the chain.
    root: DeviceId,
}

struct World {
    three: Three,
    seed: u64,
    devices: BTreeMap<DeviceId, TestConn>,
    secrets: BTreeMap<DeviceId, StaticSecret>,
    members: BTreeSet<DeviceId>,
    offline: BTreeSet<DeviceId>,
    /// Removed devices with the epoch of the change that removed them.
    removed: BTreeMap<DeviceId, u32>,
    /// Removed devices that have turned sync off (they learnt it).
    gone: BTreeSet<DeviceId>,
    copies: BTreeMap<DeviceId, LeftCopy>,
    phantoms: Vec<Phantom>,
    edits: Vec<(DeviceId, String)>,
    epoch: u32,
    kit: KitSecret,
    log: Vec<String>,
}

impl World {
    fn conn(&self, id: DeviceId) -> &TestConn {
        &self.devices[&id]
    }

    fn note(&mut self, what: String) {
        self.log.push(what);
    }

    fn fail(&self, what: &str) -> ! {
        panic!(
            "seed {}: {what}\nschedule:\n  {}",
            self.seed,
            self.log.join("\n  ")
        );
    }

    fn online_members(&self) -> Vec<DeviceId> {
        self.members
            .iter()
            .filter(|id| !self.offline.contains(id))
            .copied()
            .collect()
    }
}

fn name(id: DeviceId) -> String {
    format!("{:02x}", id.0[0])
}

/// The four devices and the Kit (module doc).
async fn world(seed: u64) -> World {
    let three = three().await;
    let d = {
        let a = three.vault.a.lock();
        let mut stream = Vec::new();
        crate::services::sync::snapshot::dump(&a, 0, &mut stream).expect("dump A");
        let d = migrated();
        keys::save_credentials(&d, &three.vault.creds).expect("credentials D");
        crate::services::sync::snapshot::join(&d, D, &mut stream.as_slice(), NOW).expect("join D");
        TestConn::new(d)
    };
    pair_with(&three.vault.b.lock(), &d.lock(), "Tablet");
    let kit = with_kit(&three, A, Some(KitStorage::Paper)).await;
    let mut devices = BTreeMap::new();
    for id in [A, B, C] {
        devices.insert(id, three.device(id).clone());
    }
    devices.insert(D, d);
    for _ in 0..2 {
        for (id, device) in &devices {
            let report = round_of(&three.vault, device).await;
            assert_eq!(report.problem, None, "{id:?}: {report:?}");
        }
    }
    let mut secrets = BTreeMap::new();
    for (id, device) in &devices {
        let conn = device.lock();
        trust::refresh(&conn, NOW).expect("refresh");
        let plan = trust::recipients(&conn, &[]).expect("plan");
        assert_eq!(plan.included.len(), 4, "{id:?}: {plan:?}");
        assert!(plan.kit.is_some(), "{id:?} pinned the Kit");
        secrets.insert(*id, box_secret(&conn));
    }
    World {
        three,
        seed,
        devices,
        secrets,
        members: [A, B, C, D].into_iter().collect(),
        offline: BTreeSet::new(),
        removed: BTreeMap::new(),
        gone: BTreeSet::new(),
        copies: BTreeMap::new(),
        phantoms: Vec::new(),
        edits: Vec::new(),
        epoch: 0,
        kit,
        log: Vec::new(),
    }
}

/// One round on `id`; a removed device that learns of its removal is `gone`.
async fn round(world: &mut World, id: DeviceId) -> Option<Option<SyncProblem>> {
    let result = run_round(
        &world.three.vault.relay,
        world.conn(id),
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;
    match result {
        Ok(report) => {
            world.note(format!(
                "round {}: {:?}{}",
                name(id),
                report.problem,
                if report.removed_here { " removed" } else { "" }
            ));
            if world.members.contains(&id) && report.removed_here {
                world.fail(&format!("{} is a member but was removed", name(id)));
            }
            if report.removed_here {
                world.gone.insert(id);
            }
            Some(report.problem)
        }
        Err(error) => {
            if world.members.contains(&id) {
                world.fail(&format!("round of {}: {error:?}", name(id)));
            }
            world.note(format!("round {}: {error:?}", name(id)));
            None
        }
    }
}

/// The remover's preconditions (the app's plan runs a round first): online, its round clean, at
/// the vault's epoch.
async fn ready_to_change(world: &mut World, id: DeviceId) -> bool {
    let problem = round(world, id).await;
    problem == Some(None) && key_epoch(&world.conn(id).lock()) == world.epoch
}

/// A key change made by `remover`: removing `removing`, or a new Kit when `None`.
async fn change(world: &mut World, remover: DeviceId, removing: Option<DeviceId>) {
    let list: Vec<DeviceId> = removing.into_iter().collect();
    let outcome = remove_in(
        &world.three.vault,
        world.conn(remover),
        world.three.dir.path(),
        &list,
        removing.is_none(),
    )
    .await;
    match outcome {
        Ok(Outcome::Changed { epoch, kit, .. }) if epoch == world.epoch + 1 => {
            world.note(format!(
                "{} {} -> epoch {epoch}{}",
                name(remover),
                removing.map_or("makes a new Kit".to_string(), |t| format!(
                    "removes {}",
                    name(t)
                )),
                if kit.is_some() { " (new Kit)" } else { "" }
            ));
            world.epoch = epoch;
            if let Some(kit) = kit {
                world.kit = kit.secret;
            }
            if let Some(target) = removing {
                world.members.remove(&target);
                world.removed.insert(target, epoch);
            }
        }
        other => world.fail(&format!(
            "{} {:?}: {other:?}",
            name(remover),
            removing.map(name)
        )),
    }
    // The command runs a round after a change, so the signed head and the records go out.
    round(world, remover).await;
}

/// `id` turns sync off; its copy is kept (module doc).
async fn leave(world: &mut World, id: DeviceId) {
    let copy = {
        let conn = world.conn(id).lock();
        LeftCopy {
            seed: seed_of(&conn),
            creds: keys::credentials(&conn).expect("creds").expect("bound"),
            epoch: world.epoch,
        }
    };
    turn_off_and_leave(&world.three.vault, world.conn(id), world.three.dir.path()).await;
    world.note(format!("{} leaves at epoch {}", name(id), world.epoch));
    world.members.remove(&id);
    world.copies.insert(id, copy);
}

/// The copy of `root` (or a device of its chain) vouches for a new device with a certificate
/// dated before `root` left, written while the copy still holds the vault's current key.
async fn late_vouch(world: &mut World, root: DeviceId, rng: &mut StdRng) {
    let n = world.phantoms.len() as u8;
    let id = DeviceId([0x80 + n; 16]);
    let (secret, box_pub) = box_key(0x80 + n);
    let seed = SignSeed::from_bytes(Zeroizing::new([0x80 + n; 32]));
    let chain: Vec<usize> = world
        .phantoms
        .iter()
        .enumerate()
        .filter(|(_, p)| p.root == root)
        .map(|(i, _)| i)
        .collect();
    let vault = world.three.vault.creds.vault_id.0;
    let copy = &world.copies[&root];
    let (issuer, issuer_seed) = if !chain.is_empty() && rng.random_bool(0.5) {
        let parent = &world.phantoms[chain[rng.random_range(0..chain.len())]];
        (parent.id, &parent.seed)
    } else {
        (root, &copy.seed)
    };
    let cert = paired_cert(vault, issuer, issuer_seed, id, &seed, box_pub, NOW - 60);
    let ct = own_row(&copy.creds, id, &cert, NOW);
    push(&world.three.vault.relay, &copy.creds, &[ct]).await;
    world.note(format!(
        "the copy of {} vouches for {} (issuer {})",
        name(root),
        name(id),
        name(issuer)
    ));
    world.phantoms.push(Phantom {
        id,
        seed,
        secret,
        root,
    });
}

/// After every step: no removed device holds a key of the epoch that removed it or later.
fn check_removed_keys(world: &World) {
    for (id, removed_at) in &world.removed {
        let conn = world.conn(*id).lock();
        if let Some(keys) = keys::vault_keys(&conn).expect("keys") {
            if keys.current_epoch() >= *removed_at {
                world.fail(&format!(
                    "{} removed at epoch {removed_at} holds epoch {}",
                    name(*id),
                    keys.current_epoch()
                ));
            }
        }
    }
}

async fn step(world: &mut World, rng: &mut StdRng, at: usize) {
    let roll = rng.random_range(0..100);
    let online = world.online_members();
    let pick = |rng: &mut StdRng, from: &[DeviceId]| from[rng.random_range(0..from.len())];
    match roll {
        0..=29 => {
            let members: Vec<DeviceId> = world.members.iter().copied().collect();
            let id = pick(rng, &members);
            let edit = format!("e{}-{at}-{}", world.seed, name(id));
            institution(&world.conn(id).lock(), &edit);
            world.note(format!("{} edits {edit}", name(id)));
            world.edits.push((id, edit));
        }
        30..=64 => {
            let mut candidates = online.clone();
            candidates.extend(
                world
                    .removed
                    .keys()
                    .filter(|id| !world.gone.contains(id) && !world.offline.contains(id)),
            );
            let id = pick(rng, &candidates);
            round(world, id).await;
        }
        65..=74 => {
            let mut all: Vec<DeviceId> = world.members.iter().copied().collect();
            all.extend(world.removed.keys().filter(|id| !world.gone.contains(id)));
            let id = pick(rng, &all);
            if world.offline.remove(&id) {
                world.note(format!("{} comes online", name(id)));
            } else if online.len() > 1 || !world.members.contains(&id) {
                world.offline.insert(id);
                world.note(format!("{} goes offline", name(id)));
            }
        }
        75..=84 if world.members.len() > 2 => {
            let remover = pick(rng, &online);
            let others: Vec<DeviceId> = world
                .members
                .iter()
                .filter(|id| **id != remover)
                .copied()
                .collect();
            let target = pick(rng, &others);
            if ready_to_change(world, remover).await {
                change(world, remover, Some(target)).await;
            }
        }
        85..=89 => {
            let maker = pick(rng, &online);
            if ready_to_change(world, maker).await {
                change(world, maker, None).await;
            }
        }
        90..=93 if world.members.len() > 2 && online.len() > 1 => {
            let id = pick(rng, &online);
            if ready_to_change(world, id).await {
                leave(world, id).await;
            }
        }
        94..=99 => {
            // A copy can write only while it holds the vault's current key.
            let current: Vec<DeviceId> = world
                .copies
                .iter()
                .filter(|(_, copy)| copy.epoch == world.epoch)
                .map(|(id, _)| *id)
                .collect();
            if !current.is_empty() && world.phantoms.len() < 32 {
                let root = pick(rng, &current);
                late_vouch(world, root, rng).await;
            }
        }
        _ => {}
    }
    check_removed_keys(world);
}

/// Everyone online; rounds until every remaining device is clean and has sent everything.
async fn settle_world(world: &mut World) {
    world.offline.clear();
    world.note("everyone online".into());
    let stragglers: Vec<DeviceId> = world
        .removed
        .keys()
        .filter(|id| !world.gone.contains(id))
        .copied()
        .collect();
    for id in stragglers {
        round(world, id).await;
    }
    for pass in 0..8 {
        let mut clean = true;
        let members: Vec<DeviceId> = world.members.iter().copied().collect();
        for id in members {
            if round(world, id).await != Some(None) {
                clean = false;
            }
        }
        let sent = world.members.iter().all(|id| {
            let conn = world.conn(*id).lock();
            count(&conn, "SELECT COUNT(*) FROM sync_unsent") == 0
                && count(&conn, "SELECT COUNT(*) FROM sync_outbox") == 0
        });
        if clean && sent && pass > 0 {
            return;
        }
    }
    world.fail("the remaining devices did not settle");
}

fn check_end(world: &World) {
    let members: Vec<DeviceId> = world.members.iter().copied().collect();
    let conns: Vec<_> = members.iter().map(|id| world.conn(*id).lock()).collect();
    let refs: Vec<&Connection> = conns.iter().map(|c| &**c).collect();
    if let Some(diff) = divergence(&refs) {
        world.fail(&format!("not converged: {diff}"));
    }
    for (author, edit) in &world.edits {
        if world.members.contains(author) && !refs.iter().all(|c| has_institution(c, edit)) {
            world.fail(&format!("{edit} of {} is missing somewhere", name(*author)));
        }
    }
    for (i, reader) in refs.iter().enumerate() {
        assert_eq!(key_epoch(reader), world.epoch, "seed {}", world.seed);
        for (j, author) in refs.iter().enumerate() {
            if i != j {
                let tip = own_tip(author).map(|(seq, hash)| (seq, hash, false));
                if head(reader, members[j]) != tip {
                    world.fail(&format!(
                        "{} did not apply {}'s chain once and in order",
                        name(members[i]),
                        name(members[j])
                    ));
                }
            }
        }
        let plan = trust::recipients(reader, &[]).expect("plan");
        for phantom in &world.phantoms {
            if trust::entry(reader, phantom.id).expect("entry").is_some()
                && included(&plan, phantom.id)
            {
                world.fail(&format!(
                    "{} is a recipient on {}",
                    name(phantom.id),
                    name(members[i])
                ));
            }
        }
    }
    // Every bundle the server holds: nothing for a removed device at or after its removal, for a
    // device that left after it left, or for any excluded chain member.
    let vault = &world.three.vault;
    for rotation in 1..=world.epoch {
        let bytes = vault
            .relay
            .stored_rekey(&vault_b64(vault), rotation)
            .expect("bundle");
        let to = bundle::peek(&bytes, &vault.creds.vault_id.0)
            .expect("header")
            .to_epoch;
        for (id, removed_at) in &world.removed {
            if to >= *removed_at && bundle_gives_key(vault, rotation, &world.secrets[id]) {
                world.fail(&format!("{} got epoch {to}", name(*id)));
            }
        }
        for (id, copy) in &world.copies {
            if to > copy.epoch && bundle_gives_key(vault, rotation, &world.secrets[id]) {
                world.fail(&format!("{} left and got epoch {to}", name(*id)));
            }
        }
        for phantom in &world.phantoms {
            if bundle_gives_key(vault, rotation, &phantom.secret) {
                world.fail(&format!("{} got epoch {to}", name(phantom.id)));
            }
        }
    }
}

/// The Kit opens the newest envelope with every key, and a newcomer joins with it.
async fn check_kit(world: &mut World) {
    let three = &world.three;
    let kit = kit_keys(three, &world.kit);
    let read = three
        .vault
        .relay
        .get_kit(&vault_b64(&three.vault), kit.token())
        .await
        .expect("the Kit reads");
    let opened = envelope::open(&read.envelope, &kit).expect("the Kit opens its envelope");
    let first = *world.members.iter().next().expect("a member");
    let keys = keys::vault_keys(&world.conn(first).lock())
        .expect("keys")
        .expect("keys");
    let epochs: Vec<u32> = opened.keys.iter().map(|k| k.epoch).collect();
    if epochs != (0..=world.epoch).collect::<Vec<_>>() {
        world.fail(&format!("the envelope holds epochs {epochs:?}"));
    }
    for key in &opened.keys {
        if Some(&key.key) != keys.get(key.epoch).as_ref() {
            world.fail(&format!(
                "the envelope's key of epoch {} differs",
                key.epoch
            ));
        }
    }

    snapshot_by(three, world.conn(first)).await;
    let joiner = newcomer();
    let dir = tempfile::tempdir().expect("dir");
    if let Err(error) = kit_join(three, &joiner, dir.path(), &world.kit).await {
        world.fail(&format!("the Kit join: {error:?}"));
    }
    join::lift_kit_join_hold(&joiner.lock()).expect("Mám je");
    let id = me(&joiner.lock());
    world.note(format!("{} joins with the Kit", name(id)));
    world.devices.insert(id, joiner);
    world.members.insert(id);
    settle_world(world).await;
    let members: Vec<DeviceId> = world.members.iter().copied().collect();
    let conns: Vec<_> = members.iter().map(|id| world.conn(*id).lock()).collect();
    let refs: Vec<&Connection> = conns.iter().map(|c| &**c).collect();
    if let Some(diff) = divergence(&refs) {
        world.fail(&format!("not converged after the Kit join: {diff}"));
    }
    let joined_at = refs[members.iter().position(|m| *m == id).expect("a member")];
    assert_eq!(key_epoch(joined_at), world.epoch, "seed {}", world.seed);
}

async fn run(seed: u64, steps: usize) {
    let mut world = world(seed).await;
    let mut rng = StdRng::seed_from_u64(seed);
    for at in 0..steps {
        step(&mut world, &mut rng, at).await;
    }
    settle_world(&mut world).await;
    check_end(&world);
    check_kit(&mut world).await;
}

#[tokio::test]
async fn removals_and_key_changes_keep_the_remaining_devices_together() {
    for seed in [1, 2, 3, 5] {
        run(seed, STEPS).await;
    }
}

#[tokio::test]
async fn removals_and_key_changes_keep_the_remaining_devices_together_more_seeds() {
    for seed in [8, 13, 21, 34] {
        run(seed, STEPS).await;
    }
}

/// The wide run (module doc): `cargo test --lib rekey_scenarios::property::wide -- --ignored`.
#[tokio::test]
#[ignore = "the wide property run: 64 seeds of 80 steps (minutes)"]
async fn wide() {
    let range = std::env::var("REKEY_PROPERTY_SEEDS")
        .ok()
        .and_then(|value| {
            let (from, to) = value.split_once("..")?;
            Some(from.parse::<u64>().ok()?..to.parse::<u64>().ok()?)
        })
        .unwrap_or(100..164);
    for seed in range {
        run(seed, WIDE_STEPS).await;
    }
}
