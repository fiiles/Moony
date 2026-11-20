# Sync audit 2, plan 3: pairing protocol 4 (commit, then reveal) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pairing code that is rewritten on its way no longer gives the vault away (finding R2-H2, decision D2).

**Architecture:** The pairing code carries a SHA-256 commitment to the shower's X25519 public key instead of the key. The shower reveals the key in slot 3, together with its acknowledgement, only after it read the other device's key from slot 1. The reader checks the revealed key against the commitment before it derives anything. The sync server is not touched: slots are opaque.

**Tech Stack:** Rust (`x25519-dalek`, `hkdf`, `hmac`, `sha2`, `subtle`, all already dependencies), the in-crate `FakeRelay` mailbox.

**Spec:** `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`, §1.2 and §3. Read both before starting.

## Global Constraints

- App only. Nothing in `Moony-relay` changes for this plan.
- Work in `src-tauri/src/services/sync/pairing/` (`mod.rs`, `session.rs` and their tests), the callers named in Task 3, and the locale files.
- `cargo test` runs **by filter only** (`cargo test --lib pairing`, `cargo test --lib sync_pairing`, `cargo test --lib relay_e2e_pairing`). The full suite takes about 27 minutes; do not run it per task.
- Secrets hygiene as in the module docs: keys and derived bytes in `Zeroizing`, `Debug` of every secret-bearing type prints `<redacted>`, no error or log line carries a code, a key, the digits or the mailbox ID.
- The six digits, `MAX_TYPED_ATTEMPTS = 3`, slot 2, its payload (`PAYLOAD_VERSION = 2`) and its AD are unchanged.
- No `.unwrap()` outside tests. Errors are `AppError`/`SyncError` as today; no new validation key is introduced.
- Every user-facing string lands in all ten locales (`en`, `cs`, `de`, `es`, `it`, `nb`, `pl`, `sv`, `ja`, `zh`); `src/i18n/locales.test.ts` fails otherwise. Czech is binding, English is the source for the other eight.
- Commit after every task with explicit paths; never `--no-verify`; never push.

## Review Focus

1. A shower whose slot 3 write is lost and retried: the scanner must still get the same 65 bytes (the reveal is deterministic from the transcript).
2. A scanner that polls slot 3 before it exists, many times: no state change, no digits.
3. A contested scanner (someone else wrote slot 1 first) must never read slot 3, so it never learns the shower's key from this device's calls.
4. A code text with white space or zero-width characters inside (a chat app wraps it): still parses, same commitment.
5. A QR of protocol 3 scanned by a protocol-4 phone and the reverse: the two existing "needs an update" texts, never `syncPairingCodeInvalid`.

---

### Task 1: The bytes (`pairing/mod.rs`)

**Files:**
- Modify: `src-tauri/src/services/sync/pairing/mod.rs`
- Test: `src-tauri/src/services/sync/pairing/tests.rs`

**Interfaces:**
- Produces (used by Task 2):
  - `pub const PROTOCOL: u32 = 4;` `pub const QR_PREFIX: &str = "moony-pair:4:";`
  - `pub const KDF_INFO: &[u8] = b"moony pair v2";`
  - `pub const SLOT1_VERSION: u8 = 0x03;`
  - `pub const COMMIT_DOMAIN: &[u8] = b"moony pair commit v1";`
  - `pub const REVEAL_VERSION: u8 = 0x02;` `pub const REVEAL_SLOT_LEN: usize = 1 + 32 + 32;`
  - `pub struct PairingCode { pub mailbox: MailboxId, pub commitment: [u8; 32], pub relay_url: Option<String> }` (the field `shower_public` is gone)
  - `pub fn commitment(mailbox: &MailboxId, shower_public: &[u8; 32]) -> [u8; 32]` = `SHA-256(COMMIT_DOMAIN ‖ mailbox.0 ‖ shower_public)`
  - `impl Transcript { pub fn reveal(&self) -> [u8; REVEAL_SLOT_LEN] }` = `REVEAL_VERSION ‖ S public ‖ HMAC tag` (the tag is what `ack()` computes today; the S public key is bytes 31..63 of the AD)
  - `pub fn open_reveal(slot3: &[u8], mailbox: &MailboxId, commitment: &[u8; 32]) -> Option<([u8; 32], [u8; 32])>`: `Some((shower_public, tag))` only for exactly `REVEAL_SLOT_LEN` bytes with `REVEAL_VERSION` whose key matches the commitment, compared with `subtle::ConstantTimeEq`; the cancel marker, a 33-byte protocol-3 slot and anything else is `None`
  - `impl Transcript { pub fn verify_tag(&self, tag: &[u8; 32]) -> bool }` (constant time). `ack()`, `verify_ack()`, `ACK_SLOT_LEN` are removed.
- Unchanged: `derive`, `slot1`, `parse_slot1`, `check_roles`, `seal_payload`, `open_payload`, `typed_code_digits`, `code_matches`.

- [ ] **Step 1: Rewrite the frozen-vector tests so they fail.** In `tests.rs` keep `S_SECRET`, `C_SECRET`, `M_SECRET`, `MAILBOX`. Replace `FROZEN_TEXT` by the protocol-4 text and add tests with these names and assertions (the vector values are computed in Step 3 and pasted in; write the tests first with `todo!()`-free placeholders that cannot pass, e.g. an all-zero expected value):

```rust
#[test]
fn the_commitment_matches_the_frozen_vector() {
    assert_eq!(hex(&commitment(&MAILBOX, &s_pub())), FROZEN_COMMITMENT);
    // Another mailbox or another key gives another commitment.
    assert_ne!(commitment(&MailboxId([0x34; 16]), &s_pub()), commitment(&MAILBOX, &s_pub()));
    assert_ne!(commitment(&MAILBOX, &c_pub()), commitment(&MAILBOX, &s_pub()));
}

#[test]
fn the_code_text_carries_the_commitment_and_never_the_key() {
    let code = PairingCode { mailbox: MAILBOX, commitment: commitment(&MAILBOX, &s_pub()), relay_url: None };
    let text = pairing_code_text(&code);
    assert_eq!(text, FROZEN_TEXT);
    assert!(text.starts_with("moony-pair:4:"));
    assert!(!text.contains(&URL_SAFE_NO_PAD.encode(s_pub())));
    let parsed = parse_pairing_code(&text).unwrap();
    assert_eq!(parsed.commitment, code.commitment);
}

#[test]
fn the_reveal_opens_only_against_its_own_commitment() {
    let c = commitment(&MAILBOX, &s_pub());
    let slot = shower().reveal();
    assert_eq!(slot.len(), REVEAL_SLOT_LEN);
    let (key, tag) = open_reveal(&slot, &MAILBOX, &c).expect("opens");
    assert_eq!(key, s_pub());
    assert!(scanner().verify_tag(&tag));
    assert!(open_reveal(&slot, &MAILBOX, &commitment(&MAILBOX, &c_pub())).is_none());
    assert!(open_reveal(&slot, &MailboxId([0x34; 16]), &c).is_none());
    assert!(open_reveal(&slot[..33], &MAILBOX, &c).is_none(), "a protocol-3 slot 3");
    assert!(open_reveal(&CANCEL_MARKER, &MAILBOX, &c).is_none());
    let mut other_version = slot; other_version[0] = 0x01;
    assert!(open_reveal(&other_version, &MAILBOX, &c).is_none());
}

#[test]
fn a_v1_v2_or_v3_code_is_refused_with_a_clear_message() { /* each of the three frozen old texts → OTHER_NEEDS_UPDATE */ }

#[test]
fn a_code_of_a_later_version_says_this_device_needs_an_update() { /* "moony-pair:5:…" → THIS_NEEDS_UPDATE */ }
```

Also: `the_transcript_matches_the_frozen_vector` (new key, code and tag under `"moony pair v2"`), `a_v3_slot1_is_refused` (version byte `0x02` → `syncPairingFailed`), and keep every existing test that does not name the removed items, adapted to `commitment`.

- [ ] **Step 2: Run `cargo test --lib pairing::tests` and see them fail** (compile errors for the new names count as failing).

- [ ] **Step 3: Implement the interface above in `mod.rs`.** Update the module doc: add a "Protocol 4" paragraph (the code commits to the shower's key; slot 3 reveals it with the tag; why: spec §3.2) and correct the sentence that says the code only protects when the key reaches the reader unmodified. Then print the vectors once (a temporary `eprintln!` in a test), paste them into `FROZEN_TEXT`, `FROZEN_COMMITMENT`, the transcript vector and the tag constants, and delete the print.

- [ ] **Step 4: Run `cargo test --lib pairing::tests`: all pass.** The ignored `the_six_digits_alone_can_be_ground` stays ignored; put its counter into bytes 8..16 of the seed (X25519 clamps bits of byte 0, so the present loop tries each key eight times).

- [ ] **Step 5: Commit** (`feat(sync): pairing protocol 4 bytes, the code commits to the shower's key`).

### Task 2: The session (`pairing/session.rs`)

**Files:**
- Modify: `src-tauri/src/services/sync/pairing/session.rs`
- Test: `src-tauri/src/services/sync/pairing/session_tests.rs`, `session_audit_tests.rs`, `session_audit2_tests.rs`

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces: the public surface of `PairingSession` is unchanged (`show`, `scan`, `announce`, `poll`, `confirm_with`, `abandon`, `code`, `comparison_code`, `contested`, `received*`, `stage`). `code()` now returns a `PairingCode` with the commitment: the shower's from its own key, the scanner's as read.

Behaviour that changes:

- **Scanner.** `scanner_of` no longer has a key to test for low order; it stores the code's commitment. `announce` writes slot 1 and goes to `AwaitingAck` **without deriving**: the secret stays in its `Option`. `read_ack` takes slot 3; `open_reveal(.., &self.mailbox, &commitment)` → on `None`: `interfere` (as any wrong slot 3 today). On `Some((key, tag))`: set `peer_public`, `derive_transcript()` (a low-order key fails there → end the pairing through `interfere`, the error stays `INTERFERED`), `verify_tag(&tag)` false → `interfere`; true → `enter_compare`. The digits still appear only in `enter_compare`.
- **Shower.** `acknowledge` puts `transcript.reveal()` into slot 3 instead of the 33-byte ack. Nothing else changes; a shower that is the joiner still shows its digits only after its slot 3 is stored.
- **Contested scanner.** Unchanged: it never reads slot 3. It now also never derived a transcript; `contest` must not expect one.

- [ ] **Step 1: Failing tests.** In `session_audit2_tests.rs` remove `#[ignore]` from the R2-H2 test and reshape it to protocol 4 (the rewriter cannot compute the holder's digits before it answers, so it answers the holder with an arbitrary key):

```rust
#[tokio::test]
async fn a_pairing_code_rewritten_on_its_way_gives_nothing_to_whoever_rewrote_it() {
    // holder shows; the text arrives rewritten (the rewriter's mailbox and its own commitment);
    // the joiner answers the rewritten code; the rewriter answers the holder with a key of its own.
    // …
    assert_eq!(holder.poll(&relay, T0).await.unwrap(), Stage::Compare);
    // The rewriter reveals its key to the joiner, who shows digits.
    let shown = joiner.comparison_code().expect("digits");
    // The person types them into the holder: they are not the holder's.
    let error = holder.confirm(&relay, Some(&vault), Some(&shown), T0).await.unwrap_err();
    assert_eq!(key_of(&error), CODE_MISMATCH);
    assert!(relay.peek_slot(&id(&holder), 2).is_none(), "nothing sealed");
}
```

(For the fixed test secrets the two codes differ; assert that explicitly before the confirm so a one-in-a-million coincidence would fail loudly rather than silently.) Add to `session_tests.rs`:

```rust
#[tokio::test] async fn a_scanner_derives_nothing_before_the_reveal()            // after announce: has_transcript() == false, has_secret() == true, comparison_code() == None
#[tokio::test] async fn a_reveal_that_does_not_match_the_code_is_interference()  // slot 3 = another shower's reveal → INTERFERED, cancel markers, secrets wiped
#[tokio::test] async fn a_low_order_revealed_key_ends_the_pairing()              // a commitment to the all-zero key, revealed → INTERFERED, no digits
#[tokio::test] async fn a_protocol_3_acknowledgement_is_interference()           // 33 bytes in slot 3 → INTERFERED
#[tokio::test] async fn the_showers_key_is_nowhere_before_slot_1_was_read()      // code text, format!("{session:?}"), and every slot of the mailbox hold no S public key until poll() read slot 1
#[tokio::test] async fn a_lost_reveal_answer_is_retried_with_the_same_bytes()    // lose_next_put_answer on slot 3; the retry enters Compare and slot 3 holds transcript.reveal()
```

Adapt the existing tests that wrote or checked a 33-byte slot 3 (`session_audit_tests.rs` M1, M2, L8 tests keep their meaning in protocol-4 form).

- [ ] **Step 2: Run `cargo test --lib pairing::session` and see the new tests fail.**
- [ ] **Step 3: Implement the changes above.** Update the module doc of `session.rs` (the flow paragraph and the security-rules list: add "the scanner derives nothing and shows nothing before the shower's revealed key matched the commitment in the code it read").
- [ ] **Step 4: Run `cargo test --lib pairing`: all pass**, including the control `a_pairing_code_that_arrives_unchanged_pairs_the_two_devices`.
- [ ] **Step 5: Commit** (`feat(sync): pairing protocol 4 session, the shower reveals its key after slot 1`).

### Task 3: Callers, fixtures and the hint text

**Files:**
- Modify: `src-tauri/src/commands/sync_pairing.rs`, `src-tauri/src/commands/sync_pairing_tests.rs`, `src-tauri/src/services/sync/relay_e2e_pairing.rs`, `src-tauri/src/services/sync/pairing/holder.rs` (only if it names removed items)
- Modify: `src/utils/sync-server.test.ts` (add a protocol-4 case; the parser is version-agnostic and stays)
- Modify: `src/i18n/locales/*/deviceSync.json` (the holder dialog's `codeHint`, ten files)

**Interfaces:**
- Consumes: Tasks 1 and 2. No new command, no wire type change (`shared/schema.ts` untouched).

- [ ] **Step 1: Compile the callers** (`cargo check --tests`) and fix every use of the removed names; `sync_pairing.rs` compares two codes by their text, which keeps working.
- [ ] **Step 2: Run `cargo test --lib sync_pairing` and `cargo test --lib relay_e2e_pairing`: all pass.**
- [ ] **Step 3: The hint.** Replace the holder dialog's `codeHint` (today "…Můžete ho poslat i zprávou: bez šesti číslic, které potom zadáte tady, nic neprozradí.") with:
  - cs: `Bez fotoaparátu? Zadejte nebo vložte tento kód na druhém zařízení. Můžete ho poslat i zprávou. Šest číslic, které potom zadáte tady, potvrdí, že ho cestou nikdo nezměnil.`
  - en: `No camera? Type or paste this code on the other device. You can send it in a message. The six digits you type here next confirm that nobody changed it on the way.`
  - the other eight locales translated from the English.
- [ ] **Step 4: `npm test -- sync-server locales` and `npm run typecheck`: pass.**
- [ ] **Step 5: Commit** (`feat(sync): pairing protocol 4 in the callers; the code hint says what the digits confirm`).

## Not in this plan

- The ADR that records protocol 4 and corrects the revision's §5 sentence (plan 4).
- A real-device check (two computers, one code sent through a messenger; a phone scanning a desktop QR): the owner's, after all four plans.
