# 0034. Device sync: the second security audit's fixes

Date: 2026-11-20

## Status

Accepted. Amends [ADR 0026](0026-device-sync-pairing.md) (pairing protocol 4),
[ADR 0027](0027-device-sync-audit-fixes.md) (Android has a Files copy; the leave path runs the copy
check), [ADR 0029](0029-self-hosted-sync-server-in-the-app.md) (a holder of the token on plain `http`
can also disable the Recovery Kit) and [ADR 0032](0032-device-sync-device-removal-and-the-recovery-kit.md)
(the signed head binds the range below it; a new row of the threat table; residuals R1 and R4 and
ruling O-2 restated; a retired token reads less). Records the fixes of the second sync security audit
(2026-11-20) as built on `fix/sync-audit-2`, from the design
`docs/specs/2026-11-20-sync-audit-2-fixes-design.md` ("the design"), whose decisions D1–D10 the owner
approved on 2026-11-20 as recommended. The server half is the Moony-relay repository, branch
`feat/protocol-3` (`DESIGN.md` there is authoritative for the server); it is not merged into that
repository's main. Sync is still hidden behind the runtime flag (`feature::RELEASED` is `false`); all
of this lands before the flag is turned on and before the server's repository is published. Where the
design differs from the code, the code and this ADR win; the section "Where the build departed from
the design" lists each difference.

## Context

The second audit (three reports of 2026-11-20: removal, key change, pairing and the Recovery Kit;
attachments; the server) read the code built after the first audit
([ADR 0027](0027-device-sync-audit-fixes.md)) and phase 5
([ADR 0032](0032-device-sync-device-removal-and-the-recovery-kit.md)). Finding IDs are the audit's:
`R2-` removal, key change, pairing, Recovery Kit; `A2-` attachments; `S2-` the server. Terms:
the **remover** is the device that removes another; a **position** is an op's number in the server's
log; the **head** is the position at which a key change happened; the **base** is the remover's stored
cursor when it sends the key change; the **range** is every position above the base up to and
including the head; an **older-key op** of a key change is one sealed under an epoch below the one
that change starts.

The audit's two High findings (R2-H1, R2-H2) were reproduced by tests before any fix was designed. A
third (R2-H3) turned up while the design was reviewed and was reproduced the same way. Each test
states what must hold, so it failed on the code of the audit and passes now.

- **R2-H1: a removed device writes after its removal.** At a key change the server answers the head
  and the remover signed whatever number it was told. A server that works with the removed device
  lifts the head, stores the removed device's later writes (old key, retired token) in the positions
  below it, and every device applies them as changes made before the removal. Test
  `a_removed_devices_write_below_a_head_the_server_raised_is_not_applied_by_the_remover` and
  `..._by_a_device_that_was_up_to_date` (`engine/tests/rekey_audit2.rs`), with a control that passes
  (`a_removed_devices_write_above_the_true_head_is_refused`). On the audited code both the remover and
  another device applied the removed device's row and no round reported a problem. This is wider than
  residual R4 of ADR 0032: every device is reached, the remover included. The server needs the removed
  device's help and has to deliver those writes before the positions above them.
- **R2-H2: a pairing code rewritten on its way.** Someone who can replace the text of the code (a
  messenger, a clipboard) and holds no secret learns the six digits the joining device will show
  before it shows them, tries keys until one gives the same digits with the holder's real key (194,635
  keys in 3.8 seconds in the recorded run on a laptop), and then holds the vault's credentials. Both
  screens end as a normal pairing. Test
  `a_pairing_code_rewritten_on_its_way_gives_nothing_to_whoever_rewrote_it`
  (`pairing/session_audit2_tests.rs`), with a control for a code that arrives unchanged. A QR code read
  from a screen in front of the person is not affected.
- **R2-H3: the same through a snapshot.** The rule that ties a key to a place in the log covered ops
  only. A removed device seals a snapshot of its own profile under its old key, numbered above the key
  change; the server stores it and answers an up-to-date device's next pull with "your place was
  pruned, restore the snapshot", and the device restored it. No lifted head is needed and a snapshot
  carries a whole profile. Test `a_removed_devices_snapshot_above_the_head_is_refused_by_an_up_to_date_device`.

The build confirmed two more holes by a test that failed on the code before the fix, and fixed both:

- **A handled position refilled behind a held cursor (rule 9a below).** When an op is held (a gap, a
  quarantined author, a date too far ahead) the cursor stays before it while later ops are handled,
  so those positions are read again in every pull. Nothing tied a position to what was found there
  the first time, so an op of the next sequence number at such a position was applied: the removed
  device's later write, under the old key, below the head. Test
  `a_position_already_handled_cannot_be_refilled_behind_a_held_cursor`. It needs no key change.
- **A lying low head wiped the notes.** The first form of rule 9a cleared the notes when the server
  was found to have gone back. That verdict rests on a head the server sends unauthenticated, so one
  lying page wiped the notes and reopened the hole. Test
  `a_lying_low_head_does_not_let_a_handled_position_be_refilled`.

The owner's decisions (2026-11-20), as taken: D1 the fix of R2-H1 and R2-H3 in the app only; D2 pairing
protocol 4; D3 option 3 for ruling O-2; D4 option 3 for A2-M2; D5, D6 and D7 accepted and documented
(`kit-history` left for later); D8 the server fixes named below; D9 the protocol numbers; D10 the leave
path's copy check.

## Decision

**The signed head binds the range below it (D1, R2-H1).** The remover no longer signs the server's
number. The call (`POST /token`) and its answer `{rotation, head}` are unchanged. On `200` the remover
makes the new key current and records the head as not verified, with its base (`rotate::commit`), and
queues nothing. It then reads every op of the range with the new token (`range::read_range`), applying
and opening nothing: it notes each position and the SHA-256 of the sealed envelope found there, in the
Local table `sync_range_notes`. A position with nothing in it is not noted and the remover never waits
for one to fill, since waiting would let the server fill it with the removed device's writes. In one
savepoint it then stores the notes, queues the signed head record as its first envelope under the new
key, and counts the head as verified (`head::verify_own`). On an honest server this happens inside the
removal call. The read ends when a page says there is no more or serves a position above the head; a
read that cannot finish (a refusal, a network error, `410`, more pages than one pull reads) stores
nothing and starts again from the base in the next round.

The signed head record has a second version. Fixed length, integers big-endian:
`"moony rekey head v2"` (19 bytes) ‖ vault ID (16) ‖ signer device ID (16) ‖ from epoch (u32) ‖ to
epoch (u32) ‖ rotation (u32) ‖ head (u64) ‖ base (u64) ‖ count (u32) ‖ digest (32) ‖ Ed25519
signature by the signer over every byte before it (64); 179 bytes. The digest is
`SHA-256("moony rekey range v1" ‖ vault ID ‖ to epoch (u32) ‖ base (u64) ‖ head (u64) ‖ count (u32) ‖
for each noted op in ascending position: position (u64) ‖ envelope hash (32))`, where the envelope hash
is the existing `envelope_hash` over the whole sealed bytes. An empty range has `count = 0` and the
digest of the prefix alone; notes that are not strictly ascending or lie outside `(base, head]` are
refused. A version-1 record no longer confirms a key change. The digest and the record are frozen by
test vectors (`rekey/head.rs`, `rekey/range.rs`); a change to either is a new version with new
vectors.

Every device checks the range before it applies anything of it:

- **Rule 6.** An older-key op at a position of the range is applied only if its hash equals the one
  noted for that position. Any other op there, and any op at a position of the range with no note, ends
  the pull as `Invalid`. Where two ranges overlap an op must match in both.
- **Rule 7.** A device takes a head as verified when the remover's signed record names the same head
  as the server and the device has read the range itself, from the signed record's base and never the
  server's, noted positions and hashes, and found the same count and digest. A different digest
  refutes the head (`keyChangeUnverified`). A device that can no longer read the range (the server
  pruned it) settles the head as before, by restoring a snapshot under the new key that names it. A
  device with nothing below the head left to check makes no notes; that is decided from the
  watermark below which the device has handled every position, the larger of its stored cursor and
  the position of its last restored snapshot, not from its place, so a device holding an op above its
  cursor reads the range (`seen::watermark`, `head::plan_of`).
- **Rule 8.** A device that got a head some other way (from the pairing holder, a snapshot's key list,
  the Kit's envelope, an earlier epoch kept across a replace) and whose watermark (rule 7) is below
  it finds the signed record in the log and does rule 7 (`head::require_ranges`). After a replace of
  the profile every key change whose head lies above the snapshot's position starts again this way.
  Until then older-key ops above the device's place wait.
- **Rule 9.** Below the base nothing is signed, but no position may be skipped there: each op must
  directly follow the one before it, the first being the stored cursor plus one, else the pull ends as
  `Invalid` (`range::must_follow_directly`); while the base is not known yet the rule holds up to the
  head. Inside the range a missing position is not an error;
  rule 6 decides there. The rule is not applied in the scan after a newer-schema stop.
- **Rule 9a: what a device has handled cannot be replaced.** A device notes the hash of every op it
  handles above its stored cursor in `sync_seen_ops` (Local), in the same transaction as the handling.
  An op read again at a noted position must have the same hash, else the pull ends as `Invalid`. Notes
  go as the cursor passes them, and all of them go when the profile is replaced and when sync is
  turned off. They stay when the server is found to have gone back, and while the went-back mark is
  set the round shows that state's own message. They also stay when the device re-joins under a new
  ID.

A device that holds a head it has not verified hands no keys on: no pairing as the holder (shower,
scan or seal), no snapshot upload, no removal, no new Kit (`validation.syncChangeInProgress`). The
remover whose own head is unsigned also pushes nothing and seals nothing again (`engine::round_pass`,
`reseal::may_reseal`, `snapshots::ready_to_dump`, `rotate::check`, `pairing::holder`). A stored list of unverified heads that does not read is an error, not "none": every gate reads
it, and an unreadable value keeps them closed. The pull refuses an op position beyond the store's
range. Its status shows
a variant of `keyChangeWaiting`: "The removal is finishing. The sync server has not yet sent every
change from before the removal. Moony keeps trying; the data on this device are fine."

**A snapshot must fit the key of its position (D1, R2-H3, `engine/snapshots.rs`).** A snapshot sealed
under the newest key a device holds is restored as before. One sealed under an older key is judged
against the key change that ended that key, with the seal epoch read from the stream the open
authenticated. Three kinds of taker: a device catching up from behind (a restore after `410`, the heal
of a missed change) takes it only when its position is above the device's place and at or below the
base; it waits between the base and the head or while the base is unknown; it refuses it at or below
its own place and above the head. A device that already holds the heads and replaces its profile
(repair, re-sync) takes it only at or below the base, waits between base and head or with no base,
and refuses it above the head. A device joining (pairing, the Recovery Kit, the copy re-join) has no
place and keeps the earlier rule: an older-key snapshot at or below the head is taken, one above it is
refused, and with no head known it is taken; what a join restores is not bound by this design (R1,
R2-M2). Every device that has verified a key change and sees that the vault's newest snapshot is not
above that head owes the vault a snapshot under the new key and uploads it as soon as it may, so that
an honest wait ends. A wait is the existing waiting state (`keyChangeWaiting`); a repair or re-sync
that must wait answers `validation.syncSnapshotWaiting`, and a join that overlapped a key change
answers `validation.syncJoinOvertaken` ("The device list changed while you were connecting. Try
again.").

**A retired token reads only vault info and key changes (D4, A2-M2, server protocol 3).** The server
answers a retired token on the vault's info and on the key-change list and nothing else: ops,
snapshots, documents and photos are `403 token_read_only` (an existing code). Every read in the app
treats that answer as "pick the new key up": the round's pull as before, the look-ahead for a signed
head, the blob lanes (a lane asks the engine for a round once per run, paced by its backoff) and a
snapshot restore. A join that overlaps a key change starts again with the text above.

**Pairing protocol 4 (D2, R2-H2, `pairing/`).** The code is `moony-pair:4:<mailbox>:<commitment>[:<server>]`
with `commitment = SHA-256("moony pair commit v1" ‖ mailbox ‖ S public key)`, the same length as
before; the shower's key is no longer in the code. The reader posts its key as slot 1 (34 bytes,
version `0x03`) and derives nothing yet. The shower answers with slot 3 = `0x02` ‖ S public key ‖
acknowledgement tag (65 bytes). The reader checks the revealed key against the commitment in constant
time, refuses a low-order key, derives the transcript with the label `"moony pair v2"`, checks the
tag, and only then shows its six digits when it is the joiner. Anything else is the existing
"interfered". The digits, the three typed attempts, slot 2 and its payload are unchanged. Whoever
rewrites the code must fix their own key in it before the joining device answers and must answer the
holder before the holder's key is revealed, so at no point do they know both sets of digits while they
can still choose a key (the structure of Bluetooth's numeric comparison); a rewritten code matches
with probability one in a million. The test of R2-H2 changed shape: the rewriter answers with a key it
cannot tune, the holder's confirm is `CODE_MISMATCH` and slot 2 holds nothing. The commitment, the
transcript and slot 3 are frozen by vectors in `pairing/tests.rs`. A device on protocol 3 that reads a
version-4 code says this one needs an update, a version-4 device that reads an older code says the
other device does; both texts existed. The server does not change.

**The removal dialog (D3, ruling O-2).** A device the remover itself first verified within the 30 days
before the removal starts ticked in "Odebrat také"; older ones start unticked (`RECENT_ADDED_SECS`,
`pinned_at` in the remover's trust store on its own clock, never the certificate's date, which the
removed device wrote and could backdate). Every row shows who added the device and when. A remover
that joined recently sees every device as recent and starts with more ticks, which errs on the safe
side. The consent hash already covers the ticks. The dialogs of a new Recovery Kit pre-tick recently added
devices the same way; the plan asked for it, while the design's §5.1 speaks of removal only.

**The leave path (D10, first audit M10).** `setup::leave_best_effort`, used by turning sync off and
by Delete Account, now runs the binding check before it pushes: a profile copy whose binding is
unchecked or says "not this device" announces nothing, and turning sync off locally still happens
(`binding::check_without_counting`).

**The server (D8, D9, branch `feat/protocol-3`).** The server's `protocol` is 3: protocol 2 plus the
narrower retired token and vault IDs unique ignoring letter case; `blobs` and `kit` are part of the
protocol. The app's oldest accepted server protocol is 3 (`MIN_PROTOCOL` in `services/sync/server.rs`),
checked at setup and when an address is entered; a device already joined to an older server keeps
working in full. No new error code was introduced, because an app that does not know one keeps it as
"no answer" and tries again every round. Also built there: vault IDs are unique ignoring case by a
unique index on the lower-cased ID (A2-M1; a colliding create is `409 vault_exists` or
`vault_deleting`; a start that finds existing collisions refuses with a line that says how to resolve
it); a pull and the key-change list read positions and sizes first and then only the ciphertexts that
fit the page (S2-M4); a request carrying `X-Forwarded-For` or `Forwarded` while `TRUST_PROXY` is off,
and one without any forwarding header while it is on, raise a log line and a warning in the admin
overview, and the compose recipe binds the port to loopback (S2-M1 and S2-M3, the cheap halves);
`moony-sync unlock-admin` clears the admin login's counters, `reset-admin` does too, and a password
re-check with a valid session counts only against its own client (S2-M2, the cheap half). The image's
health check calls `/health` so that a correct proxy setup does not warn.

**The Kit and plain `http` (D7, R2-M4).** A holder of the vault token on a plain-`http` server, for
example someone on the same network, can overwrite the Kit's stored envelope with junk during a
rotation, which stops the paper Kit working too, in addition to the lock-out of the devices that
ADR 0029 accepts. Accepted for now. The plain-`http` warning says the Recovery Kit can be disabled
too.

## Where the build departed from the design

The design is written from reading the code; the build met cases it did not cover, and each was
ruled on while the build ran. Where a ruling changed the design or its plan, the code and this ADR win.
The design's own text was amended for rules 7 and 9a.

1. **Signer of a key change (step 8).** The design does not say whose signature a device looks for
   when it holds a head from somewhere else than a bundle. Built: the signer recorded in
   `sync_epochs.by_device`, else the maker of the server's bundle for that epoch whose header verifies
   under a certificate pinned here that speaks and is not known gone; with neither, the device waits
   until a snapshot under the new key names the head. Never "any pinned device". No wire or snapshot
   format change. Cost: a device that was a member at that epoch and was removed later can, with a
   dishonest server, pose as that change's signer to a joiner (open item 4).
2. **Who owes the signature.** An unverified entry is "own, awaiting signature"
   only when `rotate::commit` made it, not whenever the signer is this device. After a replace that
   keeps the device ID, an entry for an epoch this device signed is checked against the record in the
   log, not signed again.
3. **A second key change before the remover signs.** The design is silent. Built: when a later key is
   current at signing time the remover seals its head record under the current key (a record is
   searched for in envelopes sealed under its epoch or later), and a local error from `verify_own` is
   logged and treated as waiting; it never ends the round. Without this a second key change adopted
   before the signature wedged the remover for good. Cost: a record sealed under a later key is
   unreadable to a device that later change removed, which no longer matters to it.
4. **Repair and re-sync while the own head is unsigned.** Both are refused with
   `validation.syncChangeInProgress`: both give the device a new ID, and the other devices would wait
   for a record signed by the old one. Cost: during a removal that cannot finish the person can only
   turn sync off and join again.
5. **Notes stay when the server went back (rule 9a).** The design's rule 9a cleared them; built: they
   do not go (see the second hole above), only as the cursor passes them, on a replace and when sync is
   turned off. Cost: after an honest server restore, a device holding ops above its cursor stops with
   `Invalid` at a reused position instead of applying it; it is told to re-sync either way, and the
   replace clears the notes. The design's text was amended.
6. **Repair and re-sync are not "joining" (rule 10).** The design gave the joining rule to pairing
   and the Kit only, but the first build put repair and re-sync on it, and a re-sync of an up-to-date
   device then took a removed device's forged old-key snapshot anywhere up to the head. Built: they
   are a taker of their own (above). Cost: an up-to-date device that verified a head without a stored
   base cannot re-sync from an old-key snapshot until some device uploads the new-key one; one new
   text in ten locales.
7. **Re-join under a new ID with an unsigned head of its own.** An entry the device owed becomes an
   ordinary waiting entry, signed-by the old ID: the new identity never signs a change the old one
   made. Refusing the re-join instead would let two devices share an ID. Cost: if the original is gone
   for good, nobody signs that change, and the vault is in the state of open item 1.
8. **A device holding a key-change overrun uploads no snapshots (the build's reading B2).** The design
   makes every verified device owe the new-key snapshot. Built: a device that applied the removed
   device's write because the server hid the rotation from it (`keyChangeOverrun`) does not upload and
   does not count as owing one (`ready_to_dump`, `state::overrun_tainted`). A persistent Local mark
   is written with the notice and lifted only by a replace of the profile or by turning sync off;
   dismissing the notice does not lift it. Otherwise one deceived device became the vault's source of
   truth for every device that later restores. On an honest server an overrun cannot happen.
9. **Nothing sealed under an older key leaves in a direct push.** The design's gates covered the
   round. Built: `push_all` cuts its batch at the first envelope sealed under an epoch below the
   current key's and sends nothing behind it, so the sender's chain stays in order; one place covers
   the leave, the re-sync and any other direct caller, and such envelopes wait for the re-seal. Without it a leave or re-sync right after adopting a key change put an old-key op above
   the head, which stops every device (open item 2). Cost: a leave or re-sync right after a key change
   may leave unsent changes unsent; the re-sync's backup keeps them.
10. **A second precondition for a removal.** The design says "nothing else is a precondition" beyond
    the floor. Built: a removal is also refused with `validation.syncRemovalNeedsSync` when the
    server's head is more than half of one range read above the device's cursor
    (`REMOVAL_MAX_BEHIND`, 50,000 positions of the 100,000 one read returns): a range nobody can read
    wedges the vault for good. The head is the
    server's number, so a lie only refuses. Cost: a device being removed that floods the log within its
    quota can delay its own removal for as long as it keeps the remover that far behind.
11. **"Nothing to read" is judged from a watermark, not the place (rule 7).** The design says a device
    whose place is at or past the head has nothing to check. The place counts a held op above the head,
    so a server that withheld the record and the range and served one later op made a device skip the
    check and apply the removed device's write inside a range signed as empty (reading A-1). Built:
    the watermark of rule 7 above. Cost: a device with a held cursor reads a range it may not have
    needed.
12. **A re-join carries the signed head over.** A member can seal an envelope that claims another
    device's ID; the device reads it as a clone, re-joins under a new ID, and the re-join deleted its
    unsent envelopes, among them the remover's queued signed head: nobody ever verified the key change
    (reading A-2, with no dishonest server). Built: a re-join queues every unsent signed head record
    again as the marker envelope of the new ID; the record's signature is the old ID's and is what the
    others check.
13. **The same re-join keeps the notes of rule 9a.** The re-join wiped them, so a handled position
    could be refilled with the removed device's write (reading A-3). Built: they are positional
    hashes, the cursor does not move, so they stay; they go as the cursor passes them, on a replace and
    when sync is turned off.
14. **The look-ahead stores its position only over an unbroken run.** The design is silent. A page
    that skipped the signed record made the look-ahead step over it for good, and an honest server was
    never asked for it again (reading A-4). Built: the stored position passes freely up to the entry's
    head, where an honest log has holes (the server lifts the head for a removal and nothing sits in
    the free range), and advances above it only over an unbroken run of positions, stopping at the
    first skipped one; the jump over a pruned floor after a `410` stays. Cost: a log with a real hole
    above a head, which only a misbehaving server makes, stops the look-ahead there and the device
    waits.
15. **The remover's read is one call, not resumable.** The design lets the read use the pull's page
    limits and says nothing of a partial read. Built: one call up to `MAX_PULL_PAGES` pages; if it
    cannot finish, nothing is stored and the next round starts from the base, so a half-read range is
    never signed. Cost: a re-read each round for a remover whose range exceeds one call, which departure 10
    keeps out of reach.
16. **Every verified device force-uploads the snapshot it owes.** Even when the vault's info already
    shows a snapshot above the head, the device uploads the new-key snapshot it owes: the info carries
    no seal epoch, so a forged old-key snapshot above the head would otherwise clear the duty and no
    honest device would ever replace it (the control test
    `a_forged_old_key_snapshot_at_the_end_of_the_log_is_replaced_by_the_new_key_one` fails with the
    saving). Cost: after an honest removal each verified device uploads one snapshot, N−1 of them
    redundant.

After the build, two readers (A and B) went through the finished code adversarially, one for the
remover, the other devices and the pull, one for snapshots, joins, gates and the leave path. They
tried 48 ways through and found ten points (A-1 to A-5 and B1 to B5, of which A-5 and B4 are the
same). Departures 8, 9, 10, 11, 12, 13 and 14 are the fixes of B2, B3, A-5 and B4, A-1, A-2, A-3 and
A-4; B1 is open item 2 below and B5 is under "Smaller, found and left". Their tests are in
`engine/tests/rekey_audit2.rs` and `rekey_reading_b.rs`; those of open findings are `#[ignore]`d with
the finding's name.

## Amendments to earlier ADRs

- **ADR 0026.** The pairing protocol is 4, described above. The sentence of the phase-5 revision that
  a code sent through a messenger is safe because of the typed digits is wrong for protocol 3 and
  right from protocol 4 on. The hint under the holder's code can say that the six digits typed next
  confirm that nobody changed the code on the way.
- **ADR 0027.** Android has had a Files copy of a sync backup since 2026-11-17; the sentence "Android
  has no Files copy and its text says so" under "Replaces: one hold, the person's consent, kept
  backups" is out of date. The leave path (turning sync off, Delete Account) runs the copy check that
  every other pushing path runs (M10).
- **ADR 0029.** A holder of the vault token on a plain-`http` server can also disable the Recovery Kit
  by overwriting its envelope (R2-M4). Accepted; the warning says so.
- **ADR 0032.**
  - The threat table gets a row: *Removed device plus dishonest server: can keep a device that was
    offline during the removal on a key the removed device holds, and read what that device writes
    from then on* (R2-M1; D5, accepted). The offline device has nothing but the server to tell it
    that a removal happened.
  - The existing row "Removed device plus dishonest server" keeps R1 and R4, and R4 now reads as
    below.
  - Residual R1 reads "an old copy, or one the removed device made up" (R2-M2; D6, accepted), also for
    a device that joins by pairing: it restores the newest snapshot the server serves, and an old-key
    one at or below the base passes the snapshot rule. The push hold ("Data jsou zpět") stays the
    mitigation for a Kit join.
  - Residual R4 is restated: after this ADR the older-key data above the remover's base is exactly
    what the remover signed, and nothing below the base is bound. A device that was behind the
    remover, or put behind again by a replace, can still be served the removed device's later writes
    between its place and the base, as ops or as an old-key snapshot; rule 9 limits the op variant to
    replacing genuine ops it has not seen, which breaks their author's chain and shows, and rule 9a
    closes the variant that needed no lag. Closing it for good is open item 3.
  - Ruling O-2 now reads "recent ones ticked" (D3, option 3), each row dated and naming who added
    the device.
  - A retired token reads only the vault's info and the key changes (D4, server protocol 3). The
    promise the app can make is that from the removal on the device reaches nothing more on the
    server. What it already downloaded stays with it, and a dishonest server can hand it anything.

## Consequences

- Invariants to keep:
  - the signed head record and the range digest change only as a new version with new vectors;
  - an older-key op at a position of a key change's range is applied only if it is the one signed, and
    a position a device has handled is never filled with another op;
  - a device that holds a head it has not verified hands no keys on;
  - a snapshot under an older key is restored only where the snapshot rule says;
  - a change to the pairing bytes is a new protocol number with new vectors;
  - a new error code on an existing server route counts as a change an old app may mishandle and gets
    a new protocol number; error codes are never reused with another meaning.
- New Local state (never captured; migration 042): tables `sync_range_notes` (per key epoch, the
  envelope hash of each op of the range, by position) and `sync_seen_ops` (hashes of ops handled above
  the stored cursor, by position); columns `sync_epochs.range_base` and `range_count`,
  `sync_state.restored_seq` (the position of the last snapshot restored here). Notes are forgotten
  per epoch together with the epoch's base and count. New problems and texts: the removal's own
  variant of `keyChangeWaiting`, `validation.syncRemovalNeedsSync`, `validation.syncSnapshotWaiting`,
  `validation.syncJoinOvertaken`; the code hint and the plain-`http` warning changed; texts are in all
  ten locales.
- Order of updates: the server the owner runs first (protocol 3 serves today's builds), then every
  device, and no removal until the last one is updated. A build that does not know the second version
  of the signed head does not recognise the record and waits for it for good (`keyChangeWaiting`); a
  device of an older build also cannot pair with a new one (the two "needs an update" texts, both
  directions), while devices already paired keep syncing.
- A removal now takes one more read after the rotation, normally of nothing or a handful of ops, and
  every other device reads the same small range before it accepts the head.
- The app's oldest accepted server is protocol 3; a protocol-2 server was never public.

## Residuals and open items

**Accepted by the owner on 2026-11-20.**

- **D5, R2-M1.** A device that is offline during a removal can be led onto the removed device's key by
  a dishonest server working with the removed device (row of the threat table above). It is inherent;
  a fingerprint of the device list that people compare is not built.
- **D6, R2-M2.** A Kit join, and a join by pairing, can be handed forged data and not only stale data,
  since a sheet of paper cannot learn that a removal happened. Residual R1 of ADR 0032 is reworded.
  Three behaviours were decided to stay as built: (a) a Kit join
  right after a removal has no head for the Kit's newest epoch, since the envelope is sealed inside
  the removal before the new epoch's head exists, so it takes an old-key snapshot at any position;
  the joiner notes `keyChangeOverrun` afterwards; (b) a server that serves nothing between a device's
  cursor and a head makes the device pass over that range, and it loses those ops by withholding,
  never applying anything of the removed device; (c) no overrun notice is raised when a device's place
  lay inside the range and what it had applied differs from what was signed, since it has no hashes
  below its cursor to tell.
- **D7, R2-M4.** A holder of the vault token can ruin the Kit's envelope. A 30-day history of replaced
  envelopes (`kit-history`, an optional server feature that a Kit join would try after the current
  one) is the fix if wanted; left for later.

**Two that follow from D1.**

- **Until the remover has read the range, the removed device can still write**, with the server's
  help: the server lifts the head, stores those writes below it and only then lets the remover read.
  They are signed and applied like any change made just before the removal. On an honest server the
  window is the instant of the rotation; a server can stretch it only by stalling, and the removal
  then visibly hangs on the remover.
- **Everything below the base is unbound** (R4 above), for a device that was behind and for a device
  put behind again by a repair, a re-sync or a join. A device that was not told of the key change at
  all, because the server hid it, is not protected by it either; it says so when it learns the truth
  (`keyChangeOverrun`) and nothing is undone.

**Open items the build found that are not fixed and wait for the owner.**

1. **A head that is never verified keeps every device from handing keys on for good.** Built as the
   design says: while a device holds a head it has not verified it does not pair, upload snapshots,
   remove devices or make a Recovery Kit, and with them a re-sync and a Kit join are out of reach. If
   the remover is lost between its removal call and its next push, or its queued record never reaches
   the server, there is no exit but the remover coming back or turning sync off everywhere and setting the vault up again.
   Options: a consented "stop waiting" after some days, or letting a device that read the range itself
   hand keys on. Either weakens the rule D1 approved, so it is the owner's decision. Unsent signed
   head records are carried over a re-join under a new ID (departure 12), but a device whose own head
   is unsigned and that re-joins under a new ID leaves that change to an identity that may never sign
   it (departure 7): if the original is gone for good, nobody does. A server whose head is below the
   stored base, and the quarantined author below, lead to the same state or one like it.
2. **One older-key op above a verified head, stored once by a server that worked with the removed
   device, stops every device's pull for good (reading B1).** Every round ends `InvalidChain`
   (`epoch_fits_position`); no pull ends clean, so nobody uploads the snapshot under the new key that
   every verified device owes; a re-sync or a Kit join reads a snapshot below the op and meets it
   again. Nothing is applied, so integrity holds; the harm outlives the server's dishonesty and no
   honest action clears it. Test
   `an_old_key_op_above_the_head_stored_once_does_not_wedge_the_vault_for_good`, `#[ignore]`d. Proposed
   fix: in `handle_op`, advance past an older-key op above a head that is verified, without applying
   it, with a warning and the overrun notice; every other misfit keeps stopping. Cost: it changes the
   epoch rule of the first audit, and it would silently drop an honest device's change and break its
   author's chain if any build ever sent an older-key envelope above a head; with the fix of departure 9
   none does.
3. **The root cause behind readings A-2 and A-3: envelopes are not signed by their author.** Any
   member of the vault holds the vault key and can seal an envelope claiming another device's ID, so
   it can make that device re-join under a new ID. Departures 12 and 13 remove what the readers could do with it (losing the queued signed head,
   wiping the notes); the class is not removed. Closing residual R4 with a signature on every op, or with a signed head
   that attests every author's last sequence number and hash at the key change, removes it, and also
   ends writing in another device's name, helps R2-M2 and makes `sync_devices.public_key` trustworthy
   (ADR 0027). Both are formats between devices; changing them after sync is on for everyone needs a
   barrier every device of a vault has to cross, so the design belongs before the flag is turned on.
4. **The signer of a key change that a device learned without a bundle comes from the server's list
   (departure 1).** The clean fix is for the pairing holder and the snapshot's key list to name each
   epoch's signer, which is a format change.
5. **A refuted head stays refuted across a re-sync**, although the design says rule 7 runs again after
   a replace. The way out is to turn sync off on the device and join again, which a lying server can
   refute again; it is a denial of service and nothing is applied.
6. **A server that held a quarantined author keeps every cursor held**, so no device's pull ends clean
   and none uploads a snapshot. It exists since the first audit and is outside this one, but it
   matters more with the snapshot rule. A removed device can cause it before its removal. A fix
   belongs next to this work: envelopes of a device whose removal is verified no longer hold the
   cursor; it needs its own small design.
7. **The remover cannot read its range** when the server pruned above its base after the removal call,
   and the device waits; a restore first, like any device, is the way out.

**Smaller, found and left.** A copy of a profile on Linux, where no device binding exists, still
announces that the original left (documented since M10). The owed new-key snapshot duty also ends on a `409` of a forced upload, which a server that
stores an old-key snapshot above its head and later turns honest can arrange, leaving a vault with no
new-key snapshot due for a while (reading B5). An honest
`410` for a device held behind a gap past the snapshot reads `syncSnapshotInvalid`, which is the wrong
text for the honest case. Two recommended one-line hardenings are not among the approved decisions and
wait for the owner: "Je moje" asking for the password (R2-L1) and the admin's backup download asking
for it (S2-L3). Not built and not chosen: a fingerprint of the device list, the removal-reason
question, `kit-history`, `TRUST_PROXY` as a number of hops or a list of addresses and a per-browser
marker for the admin lock-out (the last two with the hosted service).

## Checks owed on real devices

Nothing here was tried on Windows, an iPhone or an Android phone, or against a real server over the
network.

- One full join on Windows (the flush of the backup, M9).
- The phone's copy of a sync backup to Files, on iOS and on Android, and a restore from the list (H4).
- Pairing two computers through a messenger with the typed code, in both directions, wrong codes,
  three strikes, a rewritten code.
- A removal against the real server, with the Kit join and a second device that was offline during it,
  and a server of protocol 3 refusing a retired token's reads.
- The removal dialog's layout with a long device name, on the desktop and on the phone (the rows with
  the added date and the tick).
- The translations of the new texts in the eight locales other than Czech and English (German,
  Spanish, Italian, Norwegian Bokmål, Japanese, Swedish, Chinese, Polish): the tick wording of the
  dialog, the removal's own waiting text, the three new `validation` texts and the code hint are
  machine-made and not checked by a native speaker.
- The end-to-end tests against a real server (`relay_e2e.rs`, `relay_e2e_blobs.rs`,
  `relay_e2e_rekey.rs`): they were updated to protocol 3 and compile, but they need a server and were
  not run.
