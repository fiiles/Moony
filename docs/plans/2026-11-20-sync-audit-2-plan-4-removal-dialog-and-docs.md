# Sync audit 2, plan 4: the removal dialog's defaults, two texts and the ADR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Devices that a removed device added recently start ticked in "Odebrat také" (decision D3), the plain-http warning says what an eavesdropper can do to the Recovery Kit (D7), and one ADR records every decision of the second audit (D1–D10).

**Architecture:** `rotate::plan` already lists the descendants of the removed devices; it additionally says which of them this device first verified within the last 30 days. The wire type carries that list and each row's date and issuer; the two dialogs (desktop, phone) start with those ticks. No protocol or format change.

**Tech Stack:** Rust (`rekey/rotate.rs`, `commands/sync_devices.rs`, `models/sync.rs`), TypeScript/React (`src/components/settings/sync/`, `src/mobile/components/sync/`, `src/utils/sync-removal.ts`), i18n JSON in ten locales.

**Spec:** `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`, §5.1 (option 3, approved), §4.4, §8. Plans 1–3 of the same date build the rest; Task 4 here is written after they are merged.

## Global Constraints

- The wire contract is `shared/schema.ts`: follow the seven steps of `docs/standards/type-contract.md` for every change to it (Rust type with per-field `#[serde(rename)]`, `bindings.rs`, regenerate `shared/generated-types.ts` with `cargo test generate_bindings -- --ignored`).
- Thin commands: the rule lives in `services/sync/rekey/rotate.rs`, the command only maps to the wire.
- Every user-facing string in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`); Czech is binding, English the source for the other eight. The glossary forbids "klíč synchronizace" and "výměna klíče" in the UI.
- "Added" means `TrustEntry::pinned_at`: when **this** device first verified the added device, by its own clock. Never the date inside the certificate (its issuer wrote it).
- The window is `RECENT_ADDED_SECS = 30 * 24 * 60 * 60`.
- The consent hash (`rotate::consent`) is unchanged: it already covers the devices actually removed.
- `cargo test` by filter only; `npm test`, `npm run typecheck`, `npm run lint` before each commit. Never push.

## Review Focus

1. The person unticks a pre-ticked device and the plan reloads (it reloads on every tick): the device must stay unticked. Default ticks are applied once per dialog opening, never again.
2. A removing device that itself joined yesterday pinned everything yesterday: every added device starts ticked. That is intended; the note above the list must make the reason readable.
3. "Vytvořit novou sadu" with a Kit that is replaced lists devices joined through that Kit: the same default applies there.
4. A device added 30 days ago to the second: `pinned_at >= now - RECENT_ADDED_SECS` is recent (inclusive).
5. A long device name or a missing issuer name (the issuer's row is gone): the row still renders, with the date alone.

---

### Task 1: The plan says which added devices are recent

**Files:**
- Modify: `src-tauri/src/services/sync/rekey/rotate.rs` (`Plan`, `plan`)
- Test: `src-tauri/src/services/sync/engine/tests/rekey_rotate.rs`

**Interfaces:**
- Produces: `pub const RECENT_ADDED_SECS: i64 = 30 * 24 * 60 * 60;` and in `Plan`: `pub recently_added: Vec<DeviceId>` — the members of `added_by_removing` whose trust entry has `pinned_at >= now - RECENT_ADDED_SECS`, ascending. `plan(conn, removing, also_remove, new_kit, now)` keeps its signature.

- [ ] **Step 1: Failing tests** (fixture `rekey_fixture::three`, where A paired B and C; pair a fourth device from C with `pair_with` and set its `pinned_at` on A by hand):

```rust
#[tokio::test] async fn a_device_added_within_30_days_by_the_removed_one_is_recent()   // pinned_at = NOW - 29 days → plan(A, &[C]).recently_added == [D]
#[tokio::test] async fn a_device_added_earlier_is_listed_but_not_recent()              // NOW - 31 days → added_by_removing == [D], recently_added == []
#[tokio::test] async fn exactly_30_days_is_recent()
#[tokio::test] async fn a_ticked_recent_device_stays_recent_in_the_reloaded_plan()     // plan(A, &[C], &[D]) still lists D in both
```

- [ ] **Step 2: `cargo test --lib rekey_rotate` — the four fail. Step 3: Implement. Step 4: pass. Step 5: Commit** (`feat(sync): a removal plan names the devices a removed one added recently`).

### Task 2: The wire and the two dialogs

**Files:**
- Modify: `src-tauri/src/models/sync.rs` (`RemovalPlan`, `RemovalPlanDevice`), `src-tauri/src/commands/sync_devices.rs` (the mapping), `src-tauri/src/bindings.rs` if a type is added, `shared/schema.ts`, `shared/generated-types.ts` (regenerated)
- Modify: `src/utils/sync-removal.ts`, `src/components/settings/sync/RemoveDeviceDialog.tsx`, `src/components/settings/sync/NewKitDialog.tsx`, `src/mobile/components/sync/RemoveDeviceSheet.tsx`, `src/mobile/components/sync/NewKitSheet.tsx`
- Modify: `src/i18n/locales/*/deviceSync.json`
- Test: `src/utils/sync-removal.test.ts`, `src-tauri/src/commands/sync_devices` tests

**Interfaces:**
- `RemovalPlanDevice` gains `addedAt: number | null` (`added_at: Option<i64>`, the entry's `pinned_at`; `null` for a row with no trust entry) and `addedBy: string | null` (`added_by: Option<String>`, the issuer's device name; `null` when its row is gone).
- `RemovalPlan` gains `tickedByDefault: string[]` (`ticked_by_default: Vec<String>`, the hex IDs of `Plan::recently_added`).
- `src/utils/sync-removal.ts`: `export function defaultAlsoRemove(plan: RemovalPlan): string[]` returns `plan.tickedByDefault` filtered to IDs present in `plan.addedByRemoving`.
- The dialogs: the first plan that arrives after the dialog opens sets `alsoRemove` to `defaultAlsoRemove(plan)` once (a ref guards it); every later plan goes through the existing `pruneAlsoRemove` only.
- Texts:

| Key (in `deviceSync.json`, next to the existing removal keys) | Czech (binding) | English |
|---|---|---|
| `removal.addedRow` | Přidáno {{date}} ze zařízení {{name}} | Added on {{date}} from {{name}} |
| `removal.addedRowNoIssuer` | Přidáno {{date}} | Added on {{date}} |
| `removal.recentNote` | Zařízení přidaná v posledních 30 dnech jsou zaškrtnutá předem. Pokud některé z nich znáte a chcete si ho nechat, zaškrtnutí zrušte. | Devices added in the last 30 days are ticked in advance. If you know one of them and want to keep it, untick it. |

`recentNote` shows above the list only when `tickedByDefault` is not empty. Dates use the app's existing short date formatter.

- [ ] **Step 1: Failing tests.** `sync-removal.test.ts`: `defaultAlsoRemove` returns the listed recent IDs, ignores an ID the plan does not list, and returns `[]` for none. A Rust test that the command's mapping fills `addedAt`, `addedBy`, `tickedByDefault` from a `Plan`.
- [ ] **Step 2: Run, fail. Step 3: Implement** (schema in seven steps; then the four components).
- [ ] **Step 4:** `npm test`, `npm run typecheck`, `npm run lint`, `cargo test --lib sync_devices`, `cargo test generate_bindings -- --ignored` leaves no diff after regeneration.
- [ ] **Step 5: Look at it.** Run the app on the demo profile with `MOONY_DEV_SYNC=1` per the repository's run instructions and open the removal dialog on desktop width and on the phone layout; the list must fit with a 40-character device name. (Without a second device the list is empty: say so in the task report rather than claiming it was seen.)
- [ ] **Step 6: Commit** (`feat(sync): recently added devices start ticked in the removal dialog`).

### Task 3: The plain-http warning names the Recovery Kit

**Files:**
- Modify: `src/i18n/locales/*/deviceSync.json` (`insecureBody` and `insecureAtJoin`, ten files)

- [ ] **Step 1:** In both texts the sentence about what someone on the same network can do becomes:
  - cs: `Data zůstávají šifrovaná, ale kdokoli ve stejné síti může zachytit přístup k synchronizaci, synchronizovaná data na serveru smazat nebo zablokovat a vyřadit i záchrannou sadu.`
  - en: `Your data stay encrypted, but anyone on the same network can capture the access to sync, delete or block the synced data on the server, and make the recovery kit useless too.`
  (`insecureAtJoin` keeps its first sentence with `{{host}}`.)
- [ ] **Step 2: `npm test -- locales` passes. Commit** (`docs(sync): the http warning says the recovery kit can be disabled too`).

### Task 4: ADR 0034 (after plans 1–3 are merged)

**Files:**
- Create: `docs/architecture/decisions/0034-device-sync-second-audit-fixes.md`
- Modify: `docs/architecture/decisions/README.md` (index), the status lines of ADR 0026, 0027, 0029, 0032 ("Amended by ADR 0034")

- [ ] **Step 1: Write the ADR** in the form of ADR 0027 (Status, Context, Decision, Consequences, Residuals), from the spec and from what plans 1–3 actually built (where the code differs from the spec, the code and the ADR win, and the ADR says so). It must contain:
  - the three High findings and how each was confirmed; the fixes as built (signed head record version 2 and the range check; the snapshot rule; rule "what was handled cannot be replaced"; pairing protocol 4; server protocol 3);
  - amendments: **0026** (pairing protocol 4; the revision's "a code sent through a messenger is safe" holds from protocol 4 on, not before); **0027** (Android has a Files copy since 2026-11-17; the leave path now runs the copy check, M10); **0029** (a holder of the token on plain http can also disable the Recovery Kit; accepted); **0032** (the threat table gets "Removed device plus dishonest server: can keep a device that was offline during the removal on a key the removed device holds" (R2-M1); R1 reads "an old copy, or one the removed device made up", also for a pairing join; R4 is restated: nothing below the remover's base is bound; O-2 becomes "recent ones ticked"; a retired token reads only vault info and key changes);
  - the residuals the owner accepted on 2026-11-20 (D5, D6, D7) and the two that follow from D1 (writes until the remover has read the range; everything below the base), with the pointer to the next design (attested chain heads or per-op signatures) and to `kit-history`;
  - what is still owed on devices: a Windows join (M9), the phone's backup copy (H4), pairing two computers through a messenger, a removal against the home server.
- [ ] **Step 2:** `rg -n "Android has no Files copy" docs src` finds only frozen specs and plans. **Commit** (`docs(adr): 0034, the second sync audit's fixes`).

## Not in this plan

- "Je moje" asking for the password (R2-L1) and the admin's backup download asking for it (S2-L3): recommended in the spec's §9, not among the approved decisions; ask the owner.
- A fingerprint of the device list (R2-M1's hardening) and the removal-reason question (option 4 of §5.1): not chosen.
