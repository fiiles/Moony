# Device sync: fixes for the second security audit (2026-11-20) — design

Date: 2026-11-20. Status: **approved by the owner on 2026-11-20**: every recommendation of §0 and
§8 as written (D1 to D10; for D3 and D4 option 3 each; for D7 accept and document, `kit-history`
left for later). After the approval a fourth reading of §2.3 changed details of D1 (which honest
situations must not get stuck); no decision changed, and §11 lists every change. Nothing here is
built. The only code on the
branch `spec/sync-audit-2-fixes` is test code: tests that reproduce the two High findings and a
third one found while this design was reviewed, and a switch in the test server they need (§1). Sync stays behind `feature::RELEASED = false`; everything
the owner approves here lands before the flag is turned on and before the sync server's repository
is published.

Sources: the second audit's three reports of 2026-11-20 (removal and Recovery Kit, attachments,
server; kept outside this repository), the first
audit (`docs/audits/2026-11-11-device-sync-security-audit.md`), ADR 0027, 0029, 0030, 0032, the
phase-5 revision spec (`2026-11-16-device-sync-phase-5-revision-ux.md`, "the revision"), and the
server's contract (`Moony-relay/DESIGN.md`). App state: `release/1.4.0` at `8ab55947`. Server
state: `Moony-relay` main at `bde923c`.

Finding IDs are the audit's: `R2-` removal, key change, pairing, Recovery Kit; `A2-` attachments;
`S2-` the server. "The remover" is the device that removes another one. "Position" is an op's
number in the server's log. "Head" is the position at which a key change happened.

## 0. What the owner decides

The owner approved the recommendation of every row on 2026-11-20. The table stays as it was put to
him.

Each row is one decision. §8 repeats them with what "no" means. The recommendation is mine; where
a row is the owner's earlier ruling rather than a defect, it says so.

| # | About | Recommendation | Changes the wire? |
|---|---|---|---|
| D1 | R2-H1, a removed device writes after its removal; and R2-H3 (new, §1.3), the same through a snapshot | Fix in the app only: the remover signs the head together with the contents of what it read below it, every device checks those contents before applying them, and a snapshot must fit the key epoch of its position | No server change. The signed head record gets a second version, a format between devices |
| D2 | R2-H2, a pairing code rewritten on its way | Fix: the code carries a commitment to the shower's key, the key is revealed only after the other device answered | Pairing protocol 4 (no server change) |
| D3 | R2-M3 (ruling O-2), devices a removed device added | Tick "Odebrat také" by default for devices added in the last 30 days, show who added each and when | No |
| D4 | A2-M2 (ruling), what a retired token may read | A retired token reads only the vault's info and the key changes; ops, snapshots, documents and photos need the current token | Server protocol 3 |
| D5 | R2-M1, a device offline during a removal | Accept, state it in ADR 0032's threat table | No |
| D6 | R2-M2, a forged copy at a Recovery Kit join | Accept, correct the wording of residual R1 | No |
| D7 | R2-M4, a stolen token ruins the Kit's envelope | Accept for now and document; a 30-day envelope history is the fix if wanted | Optional feature |
| D8 | A2-M1, S2-M1 to S2-M4 (server) | Fix A2-M1, S2-M3, S2-M4 and the cheap halves of S2-M1 and S2-M2 now; the rest with the hosted service | No |
| D9 | The protocol numbers and the order of updates | Server protocol 3 (D4 and the vault-ID rule), pairing protocol 4; the home server is updated before the devices | — |
| D10 | First audit, M10: turning sync off skips the copy check (§6) | Fix with D1 | No |

## 1. The two High findings, tested

The audit only read code. Both findings are now reproduced by a test, and so is a third one of
the same class that the review of this design turned up (§1.3). Each test states what must hold,
so it **fails today**; they are `#[ignore]`d with the finding's ID so the suite stays green. The
first two have a control test next to them that passes.

Run them (from `src-tauri/`, by filter; the second grinds keys, so the extra flags keep it short):

```bash
cargo test --lib --config 'profile.dev.package.curve25519-dalek.opt-level=3' --config 'profile.dev.package.x25519-dalek.opt-level=3' audit2 -- --include-ignored --nocapture
```

### 1.1 R2-H1: confirmed

Test: `engine/tests/rekey_audit2.rs`,
`a_removed_devices_write_below_a_head_the_server_raised_is_applied_by_every_device`.
The test server got one switch, `FakeRelay::lift_next_rotation_head(n)`: the next key change is
answered and recorded `n` positions above the real end of the log, later writes of the current token
land above that, and a retired token's writes (with the existing `set_accept_retired_writes`) fill
the positions in between.

What the test does: three devices A, B, C, all fully synced (the test asserts every cursor is at
the end of the log). The server lifts the head by 8. A removes C. C, which still holds the old key
and token, then writes one row; the server stores it in a free position below the lifted head. A and
B sync three rounds each.

Result on `8ab55947`:

- A signs the server's number unchanged (`signed_head == end + 8`).
- B verifies the head against A's signature and finds nothing wrong.
- **Both A and B apply C's row.** No round reports a problem on either device.
- The control (`a_removed_devices_write_above_the_true_head_is_refused`): with the true head the
  same write is refused by both, and sync shows `InvalidChain`.

So the finding holds as written, and it is wider than accepted residual R4 of ADR 0032: R4 is
about a device that was behind; here every device is reached, the remover included, with no sign.
The server needs the removed device's help (it cannot seal anything itself), and it must deliver
the removed device's writes before the positions above them: a device whose cursor already passed
the lifted head is out of reach.

### 1.2 R2-H2: confirmed, and worse than reported

Test: `pairing/session_audit2_tests.rs`,
`a_pairing_code_rewritten_on_its_way_gives_the_vault_to_whoever_rewrote_it`. It runs the real
pairing sessions of the holder and of the joining computer against the test server's mailboxes. The
third party in it holds no secret and is not the server: it only replaces the text of the pairing
code on its way (its own mailbox and key) and then acts as an ordinary client of two mailboxes.

What the test does: the holder shows its code and the person sends it to the new computer. The
text arrives replaced. The new computer answers the replaced code. The third party reads that
answer from its own mailbox, so it knows the six digits the new computer is going to show before
they are shown; it tries keys until one gives the same digits with the holder's real code
(194,635 keys in 3.8 seconds in the recorded run on a twelve-core laptop; a million keys, about
twenty seconds, on average), answers the holder with that key, and only then lets the new computer
show its digits. The person types them into the holder, as the screen says.

Result on `8ab55947`:

- The holder accepts the digits (`Sent`) and seals the vault's credentials: every key and the
  account number. **The third party opens them.** (The test seals the credentials only. In the app
  the same sealed payload also carries the trust store and the one-time join ticket.)
- It then hands the same credentials on to the new computer, which reaches `Received` with the
  right credentials. The audit expected a sign here ("the new computer never finishes"). There is
  none: both screens end as a normal pairing. By reading, not by this test: the third party needs
  no device of its own afterwards, since the vault key gives it the token that reads and writes the
  vault, so it would not appear in the device list either.
- The control (`a_pairing_code_that_arrives_unchanged_pairs_the_two_devices`) passes.

What it takes: being able to replace the text of one message on its way, and seconds of computing
inside the pairing's ten minutes. It does not take the server, any secret, or a guess (the three
typed attempts never come into play, because the digits are right). A QR code read from a screen in
front of the person is not affected.

### 1.3 R2-H3 (new): the same through a snapshot

Found by the second review of this document and then tested:
`engine/tests/rekey_audit2.rs`,
`a_removed_devices_snapshot_above_the_head_is_merged_by_an_up_to_date_device`.

The rule that ties a key to a place in the log covers ops only. A snapshot is opened with
whichever key its header names, wherever in the log it claims to be (`engine/snapshots.rs`), and a
restore merges it.

What the test does: A removes C on a server that reports the true head. A and B sync; B is on the
new key with its head verified and nothing wrong. C then writes a row and seals a snapshot of its
own profile under the **old** key, numbered at the end of the log, above the key change. The server
stores it and answers B's next pull with "your place was pruned, restore the snapshot".

Result on `8ab55947`: **B restores it and has C's row** (`restored: true`, no problem shown). No
lifted head is needed, and a snapshot carries a whole profile, not one change. It needs the same
two parties as R2-H1: the removed device and the server.

## 2. R2-H1: the head of a key change

### 2.1 How it works today

At a key change the server answers `{rotation, head}`; `head` is its log's end at that moment. The
remover records that number as the start of the new key epoch and signs it (`HeadRecord`, sent as
its first envelope under the new key). Every device applies an op sealed under an older key only at
or below that head (`engine::pull::epoch_fits_position`). Other devices take the server's head as
provisional until the remover's signed head confirms it (`rekey/head.rs`). The signature therefore
protects against a server that tells *another* device a different head. It does not protect against
a server that lies to the remover, because the remover signs whatever it is told. The remover does
not pull before it rotates, so it has no number of its own to compare with.

### 2.2 What a fix has to bind

Two reviews of this document shaped this section; §11 lists what they overturned. The lesson of
both: it is not enough to make the *number* trustworthy. As long as the signature covers only a
position, a server can show the remover one thing below that position and another device something
else, and whatever the removed device seals later under the old key still fits. The fix has to bind
the **contents** between the place the remover stood and the head, and the same for snapshots.

The rotation call itself is fine as it is. On an honest server the token changes and the head is
read in one transaction, so no write with the old token lands above the head, and from then on the
removed device's token writes nothing.

### 2.3 The design (recommended). App only.

Words. A device's **place** is the highest of: its stored cursor, the highest position it has
handled (applied, held back or passed over; that can be above the cursor when an op is held), and
the position of the last snapshot it restored. A device that is joining has no place yet. The **base** is the remover's
stored cursor when it sends the rotation (kept in the pending change, so the lost-answer path has
it); with a held op that is lower than its place, and the range then covers more, which is fine.
The **range** is every position above the base up to and including the head. An **older-key op**
of a key change is one sealed under an epoch below the one that change starts.

**The remover**

1. *The call is unchanged.* `POST /token` and its answer `{rotation, head}` stay as they are.
   Before the call the remover checks, from the vault info it already reads, that its cursor is not
   below what the server still keeps (its floor); if it is, the removal is refused with
   `validation.syncRemovalNeedsSync` (a normal round restores first). Nothing else is a
   precondition: in particular not the position of the vault's newest snapshot, which the device
   being removed could keep moving.
2. *The commit is provisional.* On `200` (and on the lost-answer path that finds its own bundle in
   the server's list) the remover makes the new key current and writes the removal as today, in the
   same savepoint records the head as **not verified** with its base, and does not queue the signed
   head (today's commit queues it at once; that call moves to step 4).
3. *It reads the range.* With the new token it reads every op of the range, applying nothing and
   opening nothing: it only notes each position and the hash of the envelope found there, in a local
   table. The read ends when a page says there is no more, or serves a position above the head. It
   uses the pull's own page limits, not the short look-ahead's. The remover signs **what it found**:
   a position with nothing in it is simply not noted, and the remover never waits for one to fill
   (waiting would let the server fill it with the removed device's writes). An op of the range
   sealed under the new key (only a misbehaving server puts one there) is noted like any other; the
   pull stops on it later, as today. If the read itself is answered `410` (the server pruned above
   the base since step 1), the remover stays provisional and restores first, like any device.
4. *It signs head and contents.* The signed head record gets a second version: today's fields plus
   the base, the number of ops in the range and a digest over their positions and hashes. It is
   queued as the remover's first envelope under the new key, as today, and the head counts as
   verified on the remover from that moment (in the same savepoint), not when the record comes back
   from the server. On an honest server steps 2 to 4 happen inside the removal call.
5. *Until then the remover holds still.* While its own head is not verified it pushes nothing,
   seals nothing again, uploads no snapshot, hands no keys to a joining device, and starts no other
   removal or Kit; it applies no older-key op above its base. On an honest server this state lasts
   no longer than the removal call; it is visible only when step 3 could not finish, and then the
   device shows the state of §2.5 and tries again every round. The other devices wait for the signed head as they do today (R5).

   The last part holds for every device, not only the remover: **while a device holds a head it
   has not verified, it hands no keys on** (no pairing as the holder, no snapshot upload, no
   removal or new Kit of its own). Today
   such a device hands the new key on without its head, and the receiver then checks nothing for
   that key.

**Every device, the remover included**

6. *An older-key op in the range is applied only if it is the one that was signed.* When the pull
   meets an op sealed under an older key at a position in the range, its hash must equal the one
   noted for that position. Any other op there, and any op at a position of the range that has no
   note, ends the pull as `Invalid`. Notes are kept per key change and position for as long as the
   device could read the range again, and where two ranges overlap an op must match in both. After
   a replace of the profile (a repair, a re-sync, a join) the device's place is the snapshot's
   position: for every key change whose head is above it, step 7 runs again, and until it has, the
   older-key ops of that range wait, exactly as they wait for a signed head today.
7. *How the other devices get the notes.* A device takes the head as verified when, as today, the
   remover's signed record names the same head as the server, **and** the device has read the range
   itself (from the base, again if its own place is already past it), noted positions and hashes,
   and found the same number and digest. A different digest is a refuted head: today's
   `keyChangeUnverified`. A device whose place was already at or past the head has nothing to
   check at that moment (it makes its notes if it is ever put back below the head, see rule 6). A
   device that can no longer read the range (the server pruned it) settles the head as today, by
   restoring a snapshot under the new key that names it. A device that has to find the signed
   record starts reading at the server's floor, not at zero.
8. *A device that got the head some other way* (from the device that paired it, from a snapshot's
   key list, from the Kit's envelope) and whose place is below that head finds the signed record in
   the log first, as a Kit joiner without a bundle does today, and then does step 7. Until then
   older-key ops above its place wait.
9. *Below the base, no skipped position.* A device that is behind the base still reads older-key
   ops there unchecked (that is residual R4). There, each op must directly follow the one before it,
   the first one being the stored cursor plus one; a skipped position ends the pull as `Invalid`.
   While the base is not known yet, the rule holds up to the head. Inside the range a missing
   position is not an error; rule 6 decides there. An honest
   server never skips. The rule is not applied in the scan that follows a newer-schema stop, which
   jumps by design.
9a. *What a device has handled cannot be replaced.* When an op is held (a gap in its author's
   chain, a quarantined author, a date too far ahead) the stored cursor stays before it while
   later ops are handled, so those positions are read again in every pull. Today nothing ties a
   position to what was found there the first time, and an op of an author's next sequence number
   at such a position is applied: the removed device's later write, under the old key, below the
   head. So a device notes the hash of every op it handles above its stored cursor, and an op read
   again at a noted position must have the same hash, else the pull ends as `Invalid`. A note is
   written in the same transaction as the handling it records, and a position above the cursor
   that has no note (a crash came between) is noted when it is next read. The notes go as the
   cursor passes them, and all of them go when the profile is replaced, when the device re-joins
   under a new ID, and when the server is found to have gone back (a restored backup reuses
   positions honestly; that case keeps its own message). This rule does not depend on a key
   change.

**Snapshots (R2-H3)**

10. *A snapshot must fit the key of its position.* A snapshot sealed under the newest key a
    device holds is restored as today. One sealed under an **older** key is restored only by a
    device that is catching up from behind: its position must be above the device's place and at or
    below the base of the key change that ended that key. Everything else is refused: an old-key
    snapshot above the base, and an old-key snapshot offered to a device that is already past it
    (by a `410` the server made up, or by the heal of a missed change). The seal epoch is read from
    the stream the open authenticated, as the Kit join already does; a device that does not know
    the base yet finds the signed record first, which needs only the keys.
    - **A joining device** (pairing or the Recovery Kit) has no place and keeps today's rule: an
      older-key snapshot at or below the head is taken, with the head checked afterwards. What a
      join restores is not bound by anything in this design (R1, R2-M2, §4.2), and the Kit must keep
      working when no device is left to upload a newer snapshot.
    - **So that an honest refusal ends:** every device that has verified a key change and sees, in
      the vault's info, that the newest snapshot is not above that head owes the vault a snapshot
      under the new key and uploads it as soon as it may (today only the remover owes one). An
      honest old-key snapshot can sit above the base when another device was ahead of the remover;
      until a new-key one exists, a device that needs a restore waits and says so
      (`syncSnapshotInvalid` is the wrong message for it: the existing "waiting" state is used).

**The record and the digest, so that two builders build the same thing**

- The hash of an op is the existing envelope hash over its whole sealed bytes (`envelope_hash`).
- `digest = SHA-256("moony rekey range v1" ‖ vault_id ‖ to_epoch (u32) ‖ base (u64) ‖ head (u64) ‖
  count (u32) ‖ for each noted op in ascending position: position (u64) ‖ hash (32))`, integers
  big-endian. An empty range has `count = 0` and the digest of the prefix alone.
- Record version 2: the domain string `"moony rekey head v2"`, then today's fields in today's
  order, then `base (u64) ‖ count (u32) ‖ digest (32)`, then the signature over everything before
  it. Fixed length. A version-1 record no longer confirms a key change (no released vault has one;
  the owner's development vaults are set up again, as after the revision).
- The plan freezes a test vector for the digest and the record.

### 2.4 Why this, and what it leaves

What it closes:

- A lifted head (§1.1): the remover finds nothing in the range, signs "empty", and whatever the
  removed device has stored there later has no note. Neither the remover nor B applies it.
- The server showing the remover one thing and others another: the others' digest differs, the head
  is refuted, nothing of the range is applied.
- A later re-read of the range with other contents (a device whose cursor is held back reads the
  same positions again): each op is compared with its note.
- A snapshot under the old key above the key change (§1.3).
- The device being removed cannot delay its own removal: nothing waits for the log to stand still.

What stays, said plainly:

- **Until the remover has read the range, the removed device can still write**, with the server's
  help: the server lifts the head, stores those writes below it and only then lets the remover
  read. They are signed and applied like any change made just before the removal. On an honest
  server that window is the instant of the rotation. A server can stretch it only by stalling, and
  then the removal visibly hangs on the remover ("Odebrání se dokončuje…").
- **Below the base nothing is signed.** A device whose place was below the base when the key
  changed (it was offline, or the server kept changes from it) can still be served the removed
  device's later writes between its place and the base, as ops or as an old-key snapshot; so can a
  device joining from such a snapshot. That is residual R4, accepted on 2026-11-16. Rule 9 keeps
  the op variant to replacing genuine ops the device has not seen, which breaks their author's
  chain and shows; rule 9a closes the variant that needed no lag at all. §9 says what would close
  the rest.
- **A replace puts a device behind again.** After a repair, a re-sync or a join the device reads
  from its snapshot's position; what lies between that and the base is unbound for it like for any
  device that was behind.
- **Two states this design can leave a vault waiting in, both needing a server that misbehaves or
  a rare race, both with a way out the app names** (turn sync off on the device and connect it
  again, or the Recovery Kit): the remover cannot read its range because the server pruned above
  its base after step 1; and no device can upload a snapshot because every cursor is held behind an
  author whose envelopes are quarantined. The second exists today and is not caused by this
  design, but it matters more with rule 10; §9 lists it.
- **A device that has not been told of the key change is not protected by it.** A server that
  hides the rotation from a device keeps feeding it older-key ops, the removed device's later ones
  included. The device says so when it learns the truth (`keyChangeOverrun`), and nothing is
  undone. This is R2-M1's class (§4.1) and reaches online devices too.

Approaches considered and dropped:

- **Compare-and-swap on the server** (the rotation is refused unless the head equals a position the
  remover sends). My first draft. Until the rotation lands the device being removed holds the
  current token, so a stolen device that writes once a second would stall its own removal for ever.
  It also needed a protocol change and a new error code that old builds mishandle.
- **Verify the number only** (the remover reads up to the head before it signs, no digest). My
  second draft. It protects the remover and nobody else: the server fills the lifted range for the
  remover and serves other contents to everyone whose place is below the head, which after a lift
  is everyone.
- **The remover signs the lower of the server's number and its own cursor**, before rotating. On an
  honest server a push that lands between its last pull and the rotation would sit above the signed
  head, and every device would stop on an op its author saw acknowledged.
- **A signature on every op, or the signed head attesting every author's chain.** Closes more (R4
  too), costs a new op format or chain bookkeeping at every key change. Not instead of this design:
  see §9.
- **Accept for a self-hosted beta.** Possible, since "dishonest server" then means the owner's own
  server is compromised. With no server change needed there is little reason to.

### 2.5 What honest devices meet, and the texts

- A device that pushed before the rotation: its op is in the log at or below the head, as today.
- A device that pushes after it with the old token: `403`, picks the new key up, seals again and
  pushes, as today.
- The remover: one more read after the rotation, normally of nothing or a handful of ops.
- Every other device: one more read of the same small range before it accepts the head.
- A device paired or joined right after a removal: finds the signed record in the log before it
  applies the last older-key ops.
- Every device of a vault has to run a build that knows the second version of the signed head
  before one of them removes a device: an older build does not recognise the record and waits for
  it for good (`keyChangeWaiting`). Today that means the owner's own devices.

| Where | Czech (binding) | English |
|---|---|---|
| The remover's status while its own head is not verified (problem `keyChangeWaiting`, a variant for "by this device") | Odebrání se dokončuje. Synchronizační server zatím neposlal všechny změny z doby před odebráním. Moony to zkouší dál; data na tomto zařízení jsou v pořádku. | The removal is finishing. The sync server has not yet sent every change from before the removal. Moony keeps trying; the data on this device are fine. |
| `validation.syncRemovalNeedsSync` | Než zařízení odeberete, musí se dokončit synchronizace. Zkuste to za chvíli znovu. | Sync has to finish before a device can be removed. Try again in a moment. |

Pairing as the holder, another removal and a new Kit started while the remover's head is not
verified answer the existing `validation.syncChangeInProgress`. The same texts serve "Vytvořit
novou sadu".

### 2.6 Tests

- The three ignored tests of §1.1 and §1.3 lose their `#[ignore]` and pass: with a lifted head the
  remover signs an empty range and neither it nor B applies C's write; B refuses C's snapshot.
- An honest server: the head is verified and signed inside the removal call; the signed record is
  the remover's first envelope under the new key; B's digest matches and it applies the range.
- The server shows B other contents in the range than it showed the remover: B's head is refuted.
- A re-read of the range with one op replaced, on the remover and on B: `Invalid`.
- The remover while not verified: no push, no re-seal, no snapshot upload, no pairing as holder, no
  second removal; a crash between commit and signing resumes; the lost-answer path.
- A device paired right after a removal, and a Kit joiner, find the record and check the range.
- An old-key snapshot above the base, a snapshot at or below the device's own place, and an honest
  old-key snapshot below the base (still restored).
- Rule 9: a hole below the base stops the pull; a hole with no key change pending does not; the
  scan after a newer-schema stop is exempt.
- Rule 9a: a device with a held op is served another op at a position it already handled:
  `Invalid`; the same op again: passed over as today. This one is written as a failing test first,
  like those of §1: it is a finding of the third review, read from the code, not yet run.
- A heal of a missed change with a new-key snapshot at or below the device's place still works; an
  old-key one is refused and the heal waits.
- The accepted residual, pinned so that nobody mistakes it for a regression: a lifted range the
  server fills before the remover reads it is signed and applied by all.

## 3. R2-H2: the pairing code

### 3.1 How it works today

The pairing code (QR or text) is `moony-pair:3:<mailbox>:<S public key>[:<server>]`. The device that
reads it posts its own public key, the shower acknowledges, the joining device shows six digits
derived from both keys, and the holder's person types them. The digits are 20 bits. That is enough
when the code reaches the other device unmodified (a QR on a screen in front of the person): the
first audit's fix (the acknowledgement) stops a server that only relays. It is not enough when
someone can replace the code on its way: they learn the digits the joining device will show before
it shows them, and then try keys until one gives the same digits with the holder's real key. The
revision added exactly that way of use: "Zkopírovat kód", a code field on the desktop, and the hint
"Můžete ho poslat i zprávou".

### 3.2 Approaches

**A. Commit, then reveal (recommended): pairing protocol 4.**

- The code becomes `moony-pair:4:<mailbox>:<commitment>[:<server>]` with
  `commitment = SHA-256("moony pair commit v1" ‖ mailbox ‖ S public key)`. Same length as today.
  The shower's public key is no longer in the code.
- The reader posts its key (slot 1) as today. It cannot derive anything yet.
- The shower answers with slot 3 = version ‖ **S public key** ‖ acknowledgement tag (65 bytes
  instead of 33).
- The reader checks that the key matches the commitment in the code it read (constant time), refuses
  a low-order key, derives the transcript, checks the tag, and only then shows digits (when it is
  the joiner). Anything else is today's "interfered".
- The key derivation gets a new label (`"moony pair v2"`), slot 1 a new version byte. The six
  digits, the three typed attempts, slot 2 and its payload stay as they are.

Why it holds: whoever rewrites the code must fix their own key in the rewritten code before the
joining device answers, and must answer the holder before the holder's key is revealed. At no point
do they know both sets of digits while they can still choose a key. A rewritten code then matches
with probability one in a million per pairing, and a failed try shows as digits that do not match.
This is the same structure as Bluetooth's numeric comparison.

Cost: one pairing protocol version. A device with protocol 3 that reads a version-4 code says "Tento
párovací kód pochází z novější verze Moony…", a version-4 device that reads a version-3 code says
"Druhé zařízení potřebuje aktualizovat Moony…" (both texts exist). Devices already paired keep
syncing; only pairing needs both sides on the new build. The server does not change: slots are
opaque, 1 byte to 64 KiB. No public build has sync switched on, so the only old builds are the
owner's own.

**B. Text only.** Remove "Zkopírovat kód" and the sentence that invites sending the code, tell
people to pass the code only by a way nobody can alter. No protocol version. Rejected as the fix:
two computers have no camera between them, the code is about 70 characters, so people will send it
whatever the text says. It would also mean withdrawing decision D4 of the revision in practice.

**C. A longer comparison for a pasted code** (the audit's first suggestion: 20 digits or 8 words).
Still a protocol version (both sides must agree on the length), worse to type, and it only raises
the cost of the same attack instead of removing it.

**D. A short code and a password-authenticated exchange** (the joining device types a few words
shown by the holder; no long code at all). Better to use for two computers, but a new cryptographic
dependency and a new flow. Not for now; A keeps every screen as it is.

### 3.3 Text

The hint under the holder's code stays true with A and can stay. A more exact wording, if the owner
wants it:

| Key | Czech (binding) | English |
|---|---|---|
| `deviceSync.….codeHint` (holder's dialog) | Bez fotoaparátu? Zadejte nebo vložte tento kód na druhém zařízení. Můžete ho poslat i zprávou. Šest číslic, které potom zadáte tady, potvrdí, že ho cestou nikdo nezměnil. | No camera? Type or paste this code on the other device. You can send it in a message. The six digits you type here next confirm that nobody changed it on the way. |

The sentence in the revision's §5 ("a code sent through a messenger is safe for that reason") is
wrong for protocol 3 and right for protocol 4; the ADR that records this work says so.

### 3.4 Tests

- The ignored test of §1.2 loses its `#[ignore]` and changes shape: the rewriter cannot compute the
  holder's digits before it has answered, so it answers with a key it cannot tune; the digits differ,
  the holder's confirm is `CODE_MISMATCH`, slot 2 holds nothing.
- New frozen vectors for the commitment, the transcript and slot 3; a revealed key that does not
  match the commitment, a low-order revealed key, a version-3 slot 3 are all "interfered".
- The shower's public key appears nowhere before slot 1 was read (code text, QR, logs, `Debug`).
- The first audit's M1 and M2 tests keep passing in their protocol-4 form.

## 4. The Medium findings

"Confirmed" means I read the cited code at the audited commits. None of these has a test of its
own yet; each fix below names the test it gets.

### 4.1 R2-M1: a device that is offline during a removal can be led onto the removed device's key

Confirmed by reading (`rekey/pickup.rs`: a bundle is taken from any device that may still speak on
*this* device's store, and the offline device has not seen the removal). It needs a dishonest
server and the removed device together, and it is inherent: the offline device has nothing but the
server to tell it that a removal happened.

- **Recommendation: accept, and say so.** ADR 0032's threat table gets a row for it: "Removed device
  plus dishonest server: can keep a device that was offline during the removal on a key the removed
  device holds, and read what that device writes from then on."
- Not proposed: a fingerprint of the device list for people to compare between devices. It works,
  but nobody compares fingerprints unprompted; it can be added later without touching the protocol.
- Wire: none.

### 4.2 R2-M2: a Recovery Kit join can be handed forged data, not only stale data

Confirmed by reading (`join.rs`: the Kit joiner restores whatever snapshot the server serves; a
snapshot is authenticated only by a vault key, and a removed device holds every old one). The
audit's fix (refuse a snapshot sealed under an older key than the envelope's newest) does not close
it: the same server can replay the Kit's *older envelope*, from before the removal, and then the
removed device's key is the newest one the joiner knows. A sheet of paper cannot learn that a
removal happened.

- **Recommendation: accept, and correct residual R1** in ADR 0032: "an old copy" becomes "an old
  copy, or one the removed device made up". The push hold ("Data jsou zpět") stays the mitigation.
- The same holds, without a push hold, for a device joining by pairing: it restores the newest
  snapshot the server serves, and an old-key one at or below the base passes rule 10 (§2.3). It is
  R4's class and closes with it (§9); ADR 0032's R4 says so.
- Wire: none.

### 4.3 R2-M3: devices added by the device being removed keep getting the key

This is the owner's ruling O-2, not a defect; see §5.1.

### 4.4 R2-M4: a holder of the vault token can overwrite the Kit's envelope with junk

Confirmed by reading (the server's rotation replaces the stored envelope with whatever bytes come
with a valid current token). The token is exposed only on a plain-http server to someone on the same
network (ADR 0029 accepts that such a person can lock the devices out by rotating). The new part is
that the paper Kit stops working too.

- **Recommendation: accept for now and document** the interaction in ADR 0029 and in the plain-http
  warning ("…a kdo je ve stejné síti, může synchronizaci i záchrannou sadu vyřadit").
- The fix, if the owner wants it: the server keeps replaced envelopes for 30 days, `GET /kit` lists
  them after the current one, and a Kit join that cannot open the current envelope tries the older
  ones (it then restores the data as they were before the attack). An optional server feature
  (`kit-history`), so it can be added after the freeze without a protocol number.
- Not proposed: a server-side check of the envelope's header. Whoever has the token can write a
  header that passes, and the server would start depending on a format it is meant not to read.
- Wire: none now; one optional feature later.

### 4.5 A2-M1: vault folders on a disk that ignores letter case

Confirmed by reading (`blob-files.ts` and `snapshots.ts` name a folder by the vault ID as sent;
vault IDs differ by case; `doInit` treats only an exact match as existing). On macOS, Windows or a
share from them, a second vault of the same account whose ID differs only by case shares the
folder: it can overwrite or delete the other vault's files. Linux and the Docker volume on Linux are
not affected.

- **Recommendation: fix now**, in the server: vault IDs become unique ignoring letter case
  (migration 9: a unique index on the lower-cased ID; `doInit` and the pending-deletion check use
  the same comparison; a colliding create answers the existing `409 vault_exists`). No folder is
  renamed and no data moves. Two random 128-bit IDs never collide this way by chance.
- Rejected: naming folders by a hash or hex of the ID. It also works, but it renames every
  existing folder and makes the data directory harder to read for the person running the server.
- Test: on any file system, creating `…AbC…` then `…abc…` under one account is `409`; snapshots and
  blobs of the first are untouched.
- Wire: none (an existing error code for a request no honest app sends).

### 4.6 A2-M2: a retired token reads documents and photos for 30 days

The owner's ruling, not a defect; see §5.2.

### 4.7 S2-M1: behind a proxy without `TRUST_PROXY`, everyone shares one limit

Confirmed by reading (`ratelimit.ts`, `sync-api.ts`: the per-client vault limit is counted before the
token is looked at).

- **Recommendation: fix the cheap half now.** The server notices a request that carries
  `X-Forwarded-For` or `Forwarded` while `TRUST_PROXY` is off, logs one line, and the admin overview
  shows a standing warning ("This server seems to be behind a proxy, but TRUST_PROXY is off: all
  devices share one limit and anyone can use it up"). The README's first-run section gets the proxy
  step.
- Defer to the hosted service: `TRUST_PROXY` as a number of hops or a list of proxy addresses.
  Compatible later, because `1` keeps its meaning.
- Wire: none.

### 4.8 S2-M2: the shared lockout of the admin login

Confirmed by reading (`admin/auth.ts`: 50 wrong passwords from anywhere in 15 minutes refuse
everyone; `reset-admin` does not clear it; only a restart does).

- **Recommendation: fix the cheap half now.** A password re-check that comes with a valid session
  counts only against that client, not against the shared count. A new command,
  `moony-sync unlock-admin`, clears the counters of a running server (through the settings table the
  server already polls for the setup code); `reset-admin` does the same.
- Defer to the hosted service: replace the shared refusal with a per-browser cookie that marks a
  browser which logged in before, so strangers' failures never lock the operator out.
- For a server at home this leaves: a neighbour on the network can keep the login refused until
  the operator runs one command. Stated in the README.
- Wire: none.

### 4.9 S2-M3: the published port and `TRUST_PROXY=1`

Confirmed by reading (`compose.yml` publishes 8787 on every interface; with `TRUST_PROXY=1` a client
that reaches the port directly chooses its own rate-limit key).

- **Recommendation: fix now.** `compose.yml` and the README's proxy section bind the port to
  loopback in the proxy recipe (`127.0.0.1:8787:8787`). With `TRUST_PROXY=1` the server logs one
  line at start, and the admin overview warns when a request arrives **without** any forwarding
  header (a client reached the server around the proxy).
- Defer: an allow-list of proxy addresses (with S2-M1's deferred half).
- Wire: none.

### 4.10 S2-M4: a pull reads up to about 190 MiB before it cuts the page to 4 MiB

Confirmed by reading (`vaults.ts`: the query fetches up to `limit` full rows, the cut comes after).

- **Recommendation: fix now.** Two steps: read positions and sizes first, choose the rows that fit
  4 MiB (always at least one), then read only those ciphertexts. The same for the list of key
  changes. About ten lines and a test that counts what the query returned.
- Wire: none (the page a client receives is the same).

### 4.11 Which fixes change the wire

| Fix | Change |
|---|---|
| R2-H1 and R2-H3 (D1) | None on the server. Between devices: the signed head record, version 2 |
| A2-M2 (D4) | A retired token: only vault info and `/rekey`; `403 token_read_only` elsewhere (an existing code) |
| A2-M1 (D8) | A vault ID equal to an existing one ignoring letter case: `409 vault_exists` (an existing code) |
| R2-H2 (D2) | Pairing only: code prefix `moony-pair:4:`, slot 3 carries the shower's key. Nothing on the server |
| R2-M4 (D7), if wanted | Optional feature `kit-history` |
| Everything else | None |

## 5. The owner's rulings, looked at again

These two are decisions the owner made knowingly. The audit asks for them to be made again with
what it found; neither is a bug.

### 5.1 O-2 (R2-M3): "Odebrat také" starts unticked

Today the removal dialog lists every device the removed one added, unticked; unticked ones get the
new key and are confirmed for good. The reason for the ruling was the phone that would drop out of
sync when the tablet it was paired from is retired. The audit's case is the opposite one: a stolen
device, from which the thief paired a device of their own before the owner noticed. That device is
listed, under a name the thief chose, unticked.

Options:

1. **Keep** (today).
2. **Tick them all by default.** Safe for the stolen case; every retired tablet then takes the
   devices it paired with it unless the person unticks them, which is what O-2 was made to avoid.
3. **Tick the recent ones (recommended).** A device added in the 30 days before the removal starts
   ticked; older ones start unticked. Every row shows "Přidáno {date} ze zařízení {name}" (the text
   exists in the device details). A thief's device is recent by nature; the family phone paired last
   year is not.
4. Ask why the device is removed ("Už ho nepoužívám" / "Ztratilo se nebo mi ho ukradli") and tick
   all for the second answer. Clearest, but one more question in a dialog the revision made short.

"Added" here is when the removing device itself first verified the added device (`pinned_at` in its
trust store, its own clock), not the date inside the certificate, which the removed device wrote
and could backdate. The row shows the same date. A remover that joined only recently sees every
device as recent and starts with more ticks, which errs on the safe side. The consent hash already
covers the ticks. Wire: none.

### 5.2 A2-M2: what a retired token may still read

Today a token retired by a key change reads for 30 days: vault info, ops, the snapshot, key changes,
and (since attachments) lists and downloads every document and photo. A removed device holds the old
keys, so on an honest server it can still fetch, for 30 days: every document uploaded before its
removal, including ones it never downloaded (phones fetch on open), and every op and snapshot up to
its removal, including weeks it missed while it lay lost in a drawer.

Options:

1. **Keep and say so** in ADR 0032 and the removal dialog's help.
2. **Documents and photos need the current token** (the audit's fix).
3. **Everything but the key change needs the current token (recommended).** A retired token reads
   the vault's info and `/rekey`; ops, the snapshot and blobs answer `403 token_read_only`. A device
   that lagged meets the `403` (or the rotation number in the vault's info), picks the new key up
   and reads with the new token; a removed device finds its removal notice in `/rekey` as today.

What option 3 asks of the app (the review of this document found the first two):

- **Every read treats `403 token_read_only` as "pick the new key up".** The round's pull already
  does. The look-ahead for a signed head (`rekey::head::verify_heads`) does not: today its error
  ends the round as `keyRotated` with no pickup, and the device would wait for the next round that
  happens to read the vault's info. It must trigger the pickup like the pull does. The same for
  the blob lanes (today they only stop) and for a snapshot restore.
- **A join that overlaps a key change starts again.** A paired joiner or a Kit joiner that got its
  keys just before a key change reads with a token that was retired meanwhile. Today it can still
  read and then finds no bundle that gives it the new key; with option 3 it is refused at the first
  read. Both end the same way (pair again, or read the Kit again); the refusal must say "zkuste to
  znovu", not "sada nejde přečíst".
- Nothing else: every other reader either gets the new key from the bundle or is not meant to read.

Limits, whichever is chosen: what a removed device already downloaded stays with it, and a
dishonest server can hand it anything. The promise the app can make with option 3 is: "Od odebrání
se zařízení k ničemu dalšímu na serveru nedostane."

## 6. The first audit's fixes the second round did not reach

Checked on `8ab55947` by reading the code and tracing each original scenario, then by running the
tests that pin each fix (by filter: 99 tests, all passed). Nothing was tried on Windows, an iPhone
or an Android phone.

| Finding | Result | Why |
|---|---|---|
| H3, documents and photos deleted by a join, repair or re-sync | Holds (the design changed, ADR 0030) | Attachments now sync. Every replace goes through one function that holds the blobs of replaced rows until a pull has caught up; rows the vault does not have are counted, shown before and after, and kept in a backup that is marked to stay. |
| H4, the safety backup cannot be reached on a phone | Holds by reading; no test on a device | Restore lists these backups; iOS and Android can both save a copy to Files (Android since 2026-11-17, so ADR 0027's "Android has no Files copy" is out of date); deleting one asks first. Only the Rust helpers have tests. |
| M7, the unlock sweep deletes ordinary rows | Holds | The sweep visits only tables seeded by migrations newer than the profile last saw, skips while capture is marked broken, and the mark never goes down. |
| M8, a write between the backup and the replace is lost | Holds | Backup, consent check and replace run in one hold of the only connection; the `&mut Connection` change of `02be8bc2` kept that. A repair's backup is made before its drain, by design. |
| M9, the backup's flush fails on Windows | Fix present; never run on Windows | The flush uses a handle opened for writing. The test is a Unix stand-in; ADR 0027 still lists one full Windows join as owed. |
| M10, the device binding fails open | Holds where it was fixed; **one new path** | The engine start, repair, re-sync and resumed setup refuse to run on a binding that could not be checked. Accepted residuals unchanged (the blob seals a hash, Windows DPAPI is machine-wide). Android has no binding (the native call answers "unsupported"). New: see below. |
| M12, a release build keeps capturing with no way out | Holds (the ADR's fix, not the audit's) | Capture or the write guard is installed in every build; a build without sync can still turn it off. |

**The new path under M10.** Turning sync off, and Delete Account, call `setup::leave_best_effort`
(added in phase 5): it signs this device's `left` record and pushes it with everything queued. It
does not run the copy check that every other pushing path runs. So a profile copy whose binding
could not be checked (the state the M10 fix stops the engine in) can still be told to turn sync
off, and then pushes a `left` record under the original device's ID; the other devices would treat
the original as having left, and the original ends sync when it sees its own record. Read and
traced, not tested. It needs a copied profile on a machine where the binding call fails rather
than answers. The fix is one check before the push (refuse to say "left" while the binding is
unchecked or says "not this device"; turning sync off locally still happens), with a test next to
`engine/tests/binding_failure.rs`.

Smaller things the re-check turned up, none a regression: no test opens a kept backup and finds a
dropped attachment's content in it (H3); the phone's backup rows and "Save a copy" have no UI test
(H4); ADR 0027 and three comments still call the Files copy iOS-only.

## 7. The protocol plan

### 7.1 Numbers

- **The server's `protocol` becomes 3**, and 3 is the number the server is published with. Protocol
  3 is protocol 2 plus: the narrower rights of a retired token (D4) and vault IDs unique ignoring
  case (A2-M1). `features` keeps listing `blobs` and `kit`; from protocol 3 on both are part of the
  protocol and every protocol-3 server has them. Protocol 2 was never public and needs no
  compatibility promise.
- **The pairing protocol becomes 4** (D2). It lives in the apps only.
- **The app's oldest accepted server protocol becomes 3**, checked where it is checked today (at
  setup and when a server address is entered). Nothing in the app *needs* a protocol-3 server to
  work: D1 is app-only. The minimum is raised so that the promise of D4 ("a removed device reads
  nothing more") is true for every server a new setup accepts.
- **One format between devices changes: the signed head record, version 2** (D1). Every device of
  a vault needs a build that knows it before one of them removes a device.
- Formats between devices that do not change: the op envelope, the key-change bundle, the Kit
  envelope, the slot-2 pairing payload, the pending key change.
- No new error code is introduced on the server. That matters for old builds: an error code they
  do not know during a key change would be kept as "no answer" and tried again every round.

### 7.2 Who meets what

| | New server (protocol 3) | Old server (protocol 2) |
|---|---|---|
| **New app** | Everything works. | Setup and entering the address: "Tento synchronizační server potřebuje aktualizaci…". A device already joined keeps working in full, removal included; only D4's promise does not hold there. |
| **Old app** (a development build of today) | Ordinary sync, removal and the Kit work. A device that lagged over a key change gets `403` on its first pull, which the round already treats as "the key changed"; its look-ahead and blob lanes may show `keyRotated` or pause until the next round that reads the vault's info. It still signs the server's head unverified, and it does not recognise a version-2 signed head, so after a removal made by a new build it waits (`keyChangeWaiting`) until it is updated. | As today. |

Pairing: an old and a new build refuse each other with the two existing "needs an update" texts,
in both directions. Paired devices of mixed builds keep syncing with each other, and an old build
picks up the key of a key change made by a new one but cannot confirm its head (see the table).

There are no released builds with sync on. The builds that exist are the owner's own, and the only
running server is the one at home. Order: the home server first (protocol 3 serves today's builds),
then all the devices, and no removal until the last one is updated.

### 7.3 To decide before the protocol is frozen for publication

1. D4: it is the one change of meaning in protocol 3. After the freeze it would be a protocol 4
   with users on protocol 3.
2. Whether `kit-history` (D7) is wanted. It can come later as a feature; deciding now only saves a
   second server release.
3. The rule for growth, written into `DESIGN.md` as the compatibility statement the server audit
   asked for: a server ignores request fields it does not know; an app ignores answer fields and
   features it does not know; a change an old app would mishandle gets a new protocol number; error
   codes are never reused with another meaning, and a new error code on an existing route counts as
   a change old apps may mishandle; which app versions speak which protocol.
4. Strict ID spelling: the server accepts any 22 or 43 characters of the alphabet for vault IDs,
   mailbox IDs and tokens, the apps only the one canonical spelling. Make the server refuse the
   others (`400`, existing codes) before anyone else writes a client. No honest app is affected.
5. `TRUST_PROXY`'s grammar (S2-M1, S2-M3): stays a switch now; hops or addresses later must keep `1`
   meaning what it means today.
6. The title and numbers in `DESIGN.md` ("protocol v1.1", `limits.ts` "protocol v1") against the `3`
   the server will answer.
7. Not a server matter, but the same moment: whether the signed head attests every author's chain,
   or ops get per-device signatures (§9), before sync is switched on. It is a format between
   devices; changing it later needs a barrier that every device of a vault has to cross.

## 8. Decisions for the owner

For each: what I recommend, and what follows from "no".

- **D1 (R2-H1, R2-H3).** Yes to the design of §2.3. It costs no server change and one new version
  of a record between devices. *No* means: sync is switched on only for servers the person runs and
  trusts, and ADR 0032 says "a removed device with the server's help writes to every device".
- **D2 (R2-H2).** Yes to pairing protocol 4. *No* means: "Zkopírovat kód" and the hint that the
  code may be sent in a message are removed, the desktop-to-desktop join of the revision's D4 is in
  practice the Recovery Kit, and the revision's sentence is corrected.
- **D3 (O-2).** Option 3 (recent ones ticked, each row dated). *Keep* is a legitimate answer; the
  rows should then at least show who added the device and when.
- **D4 (A2-M2).** Option 3 (a retired token reads only vault info and key changes). *Keep* means
  ADR 0032 and the removal dialog say that a removed device can fetch documents and data from before
  its removal for 30 more days.
- **D5 (R2-M1).** Accept, with the new row in ADR 0032's threat table.
- **D6 (R2-M2).** Accept, with R1 reworded.
- **D7 (R2-M4).** Accept and document now; say whether `kit-history` should be built with protocol 3
  or left for later.
- **D8 (server).** Fix A2-M1, S2-M3, S2-M4 and the cheap halves of S2-M1 and S2-M2 now; the rest
  with the hosted service. *No to any of them* leaves a line in the server's README under "Known
  limits".
- **D9 (protocol).** Server protocol 3 (D4 and the vault-ID rule) as the number the server is
  published with, pairing protocol 4, the app's minimum 3 at setup, the home server first. The owner
  updates the home server himself. If D4 is answered "keep", protocol 3 is only the vault-ID rule
  and the folding-in of `blobs` and `kit`.
- **D10 (first audit, M10).** Yes to the one-line fix of the leave path (§6), built with D1. The
  Windows join (M9) and the phone checks (H4) stay on the owner's list of device checks.

## 9. Not in this design

- **Closing R4.** After D1, older-key data above the remover's place is exactly what the remover
  signed. Below that place nothing is bound: a device that had not pulled as far as the remover can
  still be served the removed device's later writes there, and a server that plans for it can keep
  devices behind on purpose. Two ways to close it: the signed head also attests every author's last
  sequence number and hash at the key change (devices that lag check their catch-up against it), or
  every op carries its author's signature (64 bytes per op; also ends writing in another device's
  name and helps R2-M2). Both are formats between devices and independent of the server. Snapshots
  need the same thought: they are authenticated by a key only. I recommend designing this before
  sync is switched on for everyone (§7.3 item 7).
- **A held author holds every cursor for good.** A device that forks its own chain is quarantined
  and nothing lifts that, so no device's pull ends clean and none uploads a snapshot. A removed
  device can do it before its removal. It is today's behaviour and outside the audit, but the
  natural fix belongs next to D1: envelopes of a device whose removal is verified no longer hold
  the cursor. It needs its own small design (what is passed over, what the person sees).
- **A short pairing code** with a password-authenticated exchange (§3.2 D).
- **The Low findings** of the three reports. Two are worth doing with this work because they are
  one line each: R2-L1 ("Je moje" asks for the password, like removal does) and S2-L3 (the admin's
  backup download asks for the password). The others get their own pass.
- The hosted service's needs listed in the server report (account-wide quotas, self-service
  deletion, monitoring).
- The repository's publication checklist (licence, history, the address of the home server in the
  1.4.0 tree). Those are in the audit digest and are the owner's separate decisions.

## 10. How it would be built

For the plan that follows approval; nothing here is a commitment to an order inside a task.

1. Server (`Moony-relay`, its own branch): protocol 3 (retired-token rights, the vault-ID rule,
   strict ID spelling if approved), S2-M3, S2-M4, the cheap halves of S2-M1 and S2-M2, `DESIGN.md`
   and README. The test server in the app (`relay/fake.rs`) follows the same contract.
2. App, key change (D1): the provisional commit and what it holds back, the read of the range and
   its local table (one migration, a Local table), the signed head record version 2, the check on
   every device, the path for devices that got the head elsewhere, rule 9, the snapshot rule;
   `403` on every read as a pickup trigger (D4); the leave path's copy check (D10).
3. App, pairing protocol 4: code, slot 3, vectors, the session's reader side (a shower that is the
   joiner shows its digits after its slot 3 is stored, as today); the native scanners and the code
   field only pass text through.
4. App, the removal dialog (D3), texts in ten locales, one new ADR that amends 0026, 0029 and 0032
   (and corrects 0027's line about Android), the two ignored tests switched on.
5. The owner: updates the home server, then the devices; pairs two devices and removes one against
   the real server.

Each fix lands with the tests named in its section. The ignored tests of §1 are the acceptance
tests for D1 and D2.

## 11. What the reviews of this document changed

Two readers were asked, one after the other, to plan from this document alone and to break it.
Neither could break the pairing design (§3.2). They overturned the fix for R2-H1 twice and found a
new finding:

- **First draft: a compare-and-swap on the server.** The device being removed could have stalled
  its own removal by writing. It also needed a new error code; an old build keeps an unknown error
  during a key change as "no answer" and tries again every round, pushing nothing meanwhile. No new
  code is introduced any more, and §7.3 item 3 makes that a rule.
- **Second draft: the remover verifies the number before signing.** It protected only the remover:
  the signature bound a position, not what is below it, so the server could serve other devices
  different contents there. The design now signs the contents (§2.3).
- **R2-H3** (§1.3): snapshots had no rule tying their key to their position at all. Tested.
- While its own head is unverified nothing in the round held the remover back, and other flows
  would have handed its keys on without the head; §2.3 step 5 lists what is held.
- D4's "no honest flow needs more" was wrong for the signed-head look-ahead and for a join that
  overlaps a key change; §5.2 lists both.
- D3's date was described as the certificate's first sighting, which is stored as a position, not
  a time. It is now the time this device verified the added device, which the trust store has.

A third reading then went through §2.3. It could not get past the core: a lifted head, different
contents for different devices, a replayed op and an old-key snapshot above the base were all
refused, and no honest op was lost. It added:

- A device with a held op reads positions again that it already handled, and nothing tied them to
  what it found the first time; the server can arrange a held op cheaply. That is rule 9a, and the
  definition of "place".
- The snapshot rule as first written would have refused honest heals and missed forged ones; rule
  10 now turns on the snapshot's key, not only its position.
- Several things that two builders would have decided differently: when the remover's head counts
  as verified, where the base is stored, what exactly is hashed, holes in the range, how long the
  notes live. They are stated in §2.3 now.

A fourth reading, after the owner's approval, again found no way past the protection: a forged
old-key snapshot was refused on every path, rule 9a closed the attack it was written for, and no
honest re-read holds different bytes. What it changed is where honest devices could have got stuck:

- A precondition added after the third reading (the remover must be past the vault's newest
  snapshot) would have let the device being removed block its removal by uploading snapshots. It is
  gone; step 1 checks the floor only.
- Rule 10 could have refused honest restores for good: the Kit joiner (now exempt, like any join),
  and everyone while only the remover owed the new-key snapshot (now every verified device owes
  it).
- A device that was past the head at the key change makes no notes; after a re-sync it would have
  met the range without them. Step 7 now runs again after a replace.
- "Place" now counts restored snapshots; rule 9a says when notes are written and when they go.
- Two waiting states and one weakness that exists today (a quarantined author holds every cursor)
  are named in §2.4 and §9 instead of being left for the builder to discover.

Four readings each found something. D1 is the part of this work that needs the most care: plan 2
starts with the failing tests of §2.6 and ends with one more adversarial reading of the code, and
its liveness cases are tests, not prose.

