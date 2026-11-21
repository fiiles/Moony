# Device sync: finishing a key change its maker never finished — design

Date: 2026-11-21. Status: **waiting for the owner**. Nothing here is built. It follows
`2026-11-20-sync-audit-2-fixes-design.md` ("the audit-2 design") and ADR 0034, whose first open
item it answers. Sync stays behind `feature::RELEASED = false`; this lands before the flag is
turned on. A reader who was asked to plan the build from this document alone raised nine
questions; §8 says how each was settled.

Words as in the audit-2 design: "the remover" is the device that made a key change (a removal or
a new Recovery Kit); "position" is an op's number in the server's log; "head" is the position at
which a key change happened; "base", "range", "watermark" and "older-key op" as defined in its
§2.3. New here: **the stretch** of a key change on a device is every position above that device's
cursor at the moment it took the new key, up to and including the head.

## 0. What the owner decides

| # | Decision | Recommendation |
|---|---|---|
| T1 | May another device finish a key change whose maker never signed it? | Yes: a **takeover**, a new key change made on purpose by a person, after 24 hours of waiting (§2.3) |
| T2 | What binds the stretch | The **first look** (§2.2): more to build, and every device then agrees on what it first saw. The alternative signs what is there at the takeover and builds no look |
| T3 | The signed head record | A version 3 that names the key changes it takes over; one new column (§2.4). No server or pairing change |
| T4 | "Remove the device that did not finish" in the takeover dialog | Offered, **unticked** (§3) |
| T5 | The remover itself, alone or unable to finish | No takeover from the remover in this version; its notice points to another device, and to setting sync up again as the last resort (§4) |

## 1. The problem

A device that holds a key-change head it has not verified hands no keys on (audit-2 design §2.3
step 5): it pairs nobody, uploads no snapshot, removes no device and makes no Recovery Kit. The
head is verified by the remover's signed record. If that record never comes, every device stays
in that state for good. Ordinary changes keep syncing, with one exception: a device whose stretch
holds an older-key op stops applying anything, because its pull ends at that op.

How the record can fail to come, on an honest server:

- the remover's device is lost, wiped or uninstalled between the call that changed the key and
  its next push (seconds on a good connection, longer on a bad one);
- the remover stays offline for weeks;
- the remover re-joined under a new ID before it signed, which leaves the change to an identity
  that never speaks again (ADR 0034, departure 7);
- the remover cannot read its range (the server pruned above its base).

Today's exits: the remover comes back, or the person turns sync off on every device and sets the
vault up again. The second loses what the other devices had not sent and replaces the Recovery
Kit. A device stolen during the wait cannot be removed.

Not this problem, and not answered here: a **refuted** head (the record came and does not match
what the server shows: the server is lying, and nothing should be signed over that); a server
that **went back** (its own state and message); the older-key op above a verified head (finding
B1 of the adversarial reading, a different stop with its own decision).

## 2. The design

### 2.1 Approaches

1. **A takeover (chosen).** Another device makes a new key change that covers the unfinished
   one and signs it. One signed statement, the same for every device; reuses the record, the
   range check and the gates of the audit-2 design.
2. *Each device stops waiting on its own.* No signature, so each device accepts what the server
   shows it and two devices can be shown different things without either noticing. Rejected.
3. *Only a guided new set-up.* Changes no protection; costs the person every device by hand, the
   unsent changes elsewhere and the Kit. Kept as the last resort (§4), not as the answer.

Rolling the key change back is not possible: the token is rotated and the removed device is cut
off, as it should be.

### 2.2 The first look (decision T2)

Fact the design rests on: on an honest server the stretch of a key change does not change after
the change. The server takes no write with a retired token, so every older-key op of the stretch
was there at the rotation; positions above the head hold only ops of devices that have the new
key. Positions at or below the last honest op are full, so a later write can only be **added**
above it (a server that reported a head higher than its real end leaves room there) or
**replace** something a device has not read yet.

**Every device looks once.** When a device records a head it has not verified and knows its
number (a key pickup; a replace, a join or a merge restore that leaves a head above its
watermark), it reads its stretch of that key change with the new token, applying and opening
nothing, and remembers the envelope hash at each position where it found an op, whatever key the
op is sealed under. The look runs in the round right after the pickup, before the heads are
verified and before the pull. A read that fails for the network is tried again next round. A
stretch that is longer than one range read, or that the server has pruned (`410`), gets **no
look**, for good: that device is not asked again and is treated as §2.5 says. An entry with no
known head (a Recovery Kit joiner without a bundle) has no look. Until the look is done or given
up, the existing rule already keeps the older-key ops of the stretch from being applied.

**Inside a looked stretch only what was seen counts.** For as long as the stored cursor is below
the end of a looked stretch, an op read at a position of that stretch must be the one seen there:
another hash, or an op at a position where the look found none, ends the pull as `Invalid`. This
is rule 9a of the audit-2 design applied before the first handling instead of after it. It holds
whether or not anyone ever signs, with or without a takeover.

The look's notes are not "handled": they do not move the device's place or its watermark, and
rule 10 (snapshots) does not count them. They and the end of the looked stretch are kept until
the cursor passes them, and go on a replace of the profile and when sync is turned off; not on a
re-join under a new ID and not when the server is found to have gone back.

**What the look is worth, plainly.** With the remover gone, no honest device saw the stretch at
the moment of the change. A removed device working with a dishonest server can add or replace
writes in it until a device first looks, and the server decides when that is, by delaying when it
shows that device the key change. The look does not close that; nothing can, short of the
remover's own signature. What it does: (a) after a device has looked, nothing can be added or
replaced for it, so the wait before a takeover and the takeover itself are not a second window;
(b) the taker signs what it first saw, and a device that was shown something else at its own
first look stops instead of going along (§2.3), so the devices cannot be led apart silently
above the taker's cursor; (c) for a device that is behind the taker's cursor, the positions the
taker's signature does not cover are bound to that device's own first sight, where the audit-2
design left them unbound (its residual R4).

**The alternative (T2, not recommended).** Build no look; the taker signs what its range read
finds at the takeover. Less code. Then everything a removed device and a dishonest server place
during the whole wait is signed, the wait itself is a window, and (b) and (c) are lost.

### 2.3 The takeover

**Who and when.** A device may take over when all of these hold:

- it holds at least one unverified head, none of them refuted and none its own unsigned one;
- **every** unverified head it holds has waited 24 hours since this device recorded it (the
  existing `WAITING_AFTER_SECS`, counted from the epoch's row in `sync_epochs`; a replace of the
  profile records the rows again and so starts the count again). A takeover that is itself
  unsigned gets its own 24 hours before it can be taken over;
- its first look is done for every unverified head it holds, and a fresh read of the looked
  stretches shows what the look saw (below);
- the ordinary preconditions of a key change hold (cursor not below the floor, not more than
  `REMOVAL_MAX_BEHIND` behind the server's head, changes not paused, own identity trusted, the
  server's limit of kept rotations: a takeover is a rotation like any other).

A person starts it, from the waiting notice (§3). It is never automatic. It is the one key change
`rotate::check` allows while a head is unverified; every other refusal of that state stays
(pairing as the holder, snapshot upload, other removals and new Kits), until the takeover's own
head is verified.

**What it is.** An ordinary key change by this device (the next epoch, a rotation, a bundle to
every remaining device, the Kit's envelope sealed again as for any key change), which may remove
nobody. It may also remove the device that did not finish, and the devices that device added,
through the existing plan (§3).

**What the taker signs.** Its range is, as for any remover, everything above its stored cursor
up to the new head. That covers every position that was empty when the taker passed it, which is
the only place a write can be added after the fact; what lies below the taker's cursor is full
and is covered by each device's own look (§2.2, c).

Before the rotation is sent the taker reads its looked stretches again and compares: every op
must be the one seen, nothing may be added, nothing may be missing. If they differ it refuses
with `validation.syncTakeoverServerChanged` and sends nothing. After its provisional commit it
reads its range as any remover does and compares once more for the positions inside looked
stretches; if they differ it does not sign, keeps trying each round, and shows
`syncTakeoverServerChanged` as its problem. Otherwise it stores the notes, signs a version-3
record (§2.4) naming every epoch it holds unverified, and in the same step drops those entries
as taken over (§2.4). Until then it holds still like any remover with an unsigned head.

**What the other devices do.** A device that finds a version-3 record of the key change's maker
verifies it exactly as a version-2 one (the numbers, then its own read of the range from the
signed base, count and digest). When it verifies, and in the same step, every unverified head it
holds for an epoch the record names is dropped as taken over, its own unsigned one included. A
refuted head is not dropped: that device stays in `keyChangeUnverified` (§1). The device's own
look stays in force through its pull: where the taker signed something this device did not see
at its first look, its pull ends `Invalid` and nothing of it is applied.

**The device that did not finish.** If it was not removed and comes back, it picks the takeover
up like any other device. From the moment it holds a later unverified head it no longer signs
its own earlier one; when it verifies the takeover its own entry is dropped as taken over. If it
signed before it saw the takeover, devices that already dropped that head ignore the record
(§2.4), and devices that had not yet verified the takeover verify both; on an honest server the
two cannot disagree. If it was removed, it learns that from the key-change list as today.

**A takeover that is itself never signed** (the taker is lost too) is taken over the same way
after its own 24 hours; that record names every epoch still unverified.

**Two devices take over at once.** The server takes one rotation; the other device meets the
existing lost race, picks the winner's change up and commits nothing of its own.

### 2.4 The record, and what a taken-over key change leaves behind

**Record version 3**: the domain string `"moony rekey head v3"`, the fields of version 2 in
their order, then `over (u32)`, then the signature over everything before it. Fixed length (183
bytes). `over` is the lowest epoch this record takes over; it takes over every epoch from `over`
up to, not including, its own `to_epoch`. A verifier requires `1 <= over <= from_epoch`. The
digest is the version-2 digest, unchanged. A device accepts versions 2 and 3; a record is taken
only from the signer of the key change it belongs to, as today. The plan freezes a test vector.

Why a field and not an inference: a device further behind than the taker cannot tell a takeover
from an ordinary later key change by the numbers, and dropping an unverified head on the
strength of an ordinary later change would skip the check of its range.

**The mark.** A taken-over epoch is marked with the epoch that took it over (one new column of
`sync_epochs`, a migration). The mark is set where the entry is dropped, in the same step. It
means "this key change's range is the taker's", and every rule that asks for the range or base of
a marked epoch uses it so:

- rule 6 does not look for notes of a marked epoch; the taker's range judges the ops (it covers
  them wherever it reaches) and the look judges the rest;
- rule 9 (no skipped position) takes the taker's base for a marked epoch;
- rule 10 (snapshots): for the key that a marked epoch ended, "at or below the base" means at or
  below the lower of the taker's base and the marked epoch's own head;
- after a replace, a join or a merge restore, no unverified entry is made for a marked epoch.
  A device that learns of a taken-over epoch without the mark (a join, a replace: the heads come
  from a holder or a snapshot, which carry no mark) makes entries for it and for the takeover as
  for any two heads, finds the taker's record first or second, and sets the mark when it
  verifies it;
- a record of the original remover for a marked epoch is ignored;
- an epoch this device holds as verified, or does not hold at all, is not marked by a record
  that names it.

The mark goes with its `sync_epochs` row. Nothing else on the wire changes: no server change, no
pairing change, no snapshot format change. Devices hand on the head of a taken-over epoch like
any verified head; it is used only by the epoch rule.

### 2.5 What stays open after this

- **Until the first look.** Stated in §2.2: a removed device with a dishonest server can place
  writes in the stretch before a device first looks, and the server can delay that look. On an
  honest server there is nothing to place.
- **Devices without a look** (a stretch longer than one read; a pruned stretch; a Recovery Kit
  joiner without a bundle): they cannot take over, and below the taker's base they are where the
  audit-2 design left them (residual R4).
- **No device can take over**: one device left; every device without a look; every device more
  than `REMOVAL_MAX_BEHIND` behind because its pull is stopped in the stretch while the others
  keep writing; the server's limit of kept rotations reached. The last resort of §4 remains.
- **A refuted head, a server that went back, finding B1**: unchanged (§1). A device that refuted
  the original remover's record is not released by a takeover.
- **A member that takes over in bad faith** can drop a key change another member started. A
  member holds the key and is trusted throughout this design.
- **Later, not here:** the bundle could carry the remover's base, so that the stretch above it
  could be told apart from what the remover had already seen even when the remover never signs.
  That changes the bundle's format and what happens to honest writes that landed in between; it
  needs its own design.

## 3. What the person sees

The waiting notice ("waiting for device X for more than a day") names the maker of the **newest**
unverified head, the one being waited for now, and gains a button on every device that may take
over. A device whose own takeover is unsigned shows no button (it is the remover now). Where the
maker has no name on this device (its row is gone, or the key change was learned without a
bundle and its signer is not known) the notice and the dialog use the texts without a name, and
the checkbox is not shown.

Texts (Czech binding, English the source for the other eight locales; the glossary forbids
"klíč synchronizace" and "výměna klíče"):

| Key | Czech | English |
|---|---|---|
| notice without a name | Změna seznamu zařízení čeká na dokončení déle než den. | A change of the device list has been waiting to be finished for more than a day. |
| notice button | Dokončit z tohoto zařízení | Finish from this device |
| dialog title | Dokončit změnu z tohoto zařízení? | Finish the change from this device? |
| dialog body 1 | Zařízení {{name}} začalo {{date}} měnit seznam zařízení a změnu nedokončilo. Dokud není dokončená, nejde přidat ani odebrat zařízení a na serveru nevzniká nová kopie dat. | {{name}} started changing the device list on {{date}} and did not finish. Until the change is finished, no device can be added or removed and no new copy of your data is made on the server. |
| dialog body 1 without a name | Změna seznamu zařízení z {{date}} není dokončená. Dokud není dokončená, nejde přidat ani odebrat zařízení a na serveru nevzniká nová kopie dat. | The change of the device list from {{date}} is not finished. Until it is finished, no device can be added or removed and no new copy of your data is made on the server. |
| dialog body 2 | Dokončete ji odsud jen tehdy, když zařízení {{name}} nemáte po ruce. Pokud ho otevřít můžete, otevřete na něm Moony a nechte ho změnu dokončit. | Finish it from here only if you cannot get to {{name}}. If you can open it, open Moony on it and let it finish the change. |
| dialog body 2 without a name | Dokončete ji odsud jen tehdy, když zařízení, které ji začalo, nemáte po ruce. | Finish it from here only if you cannot get to the device that started it. |
| checkbox | Zařízení {{name}} už nemám. Odebrat ho ze synchronizace. | I no longer have {{name}}. Remove it from sync. |
| confirm | Dokončit | Finish |
| `validation.syncTakeoverNotYet` | Změnu ještě může dokončit zařízení, které ji začalo. Odjinud ji půjde dokončit po 24 hodinách čekání. | The device that started the change may still finish it. It can be finished from elsewhere after 24 hours of waiting. |
| `validation.syncTakeoverNotHere` | Toto zařízení změnu dokončit nemůže, protože nestihlo přečíst změny z doby před ní. Zkuste to z jiného zařízení. | This device cannot finish the change because it has not read the changes from before it. Try from another device. |
| `validation.syncTakeoverServerChanged` | Synchronizační server teď ukazuje jiné změny než dřív. Změnu nedokončujte a server zkontrolujte. | The sync server now shows other changes than before. Do not finish the change; check the server. |

`syncTakeoverNotHere` covers a device without a look and one too far behind; the other refusals
of a key change keep their existing texts.

The dialog is the removal dialog's shape: the password, the consent over the plan, and, when the
checkbox is ticked, the existing "also remove" list for the devices that device added, with the
defaults of ADR 0034 (recently added ones ticked). The checkbox starts unticked (T4): a device
that is only offline should come back to a vault it still belongs to.

While the takeover's own head is unsigned the taker shows the existing "finishing" status.

## 4. The remover itself, and the last resort

A remover whose own head is unsigned gets no takeover button in this version (T5): what keeps it
from signing (it cannot read its range) would keep a takeover of its own from signing too. Its
"finishing" status gains, after 24 hours, one sentence:

- cs: `Pokud to trvá déle než den, dokončete změnu z jiného zařízení v Nastavení → Synchronizace.`
- en: `If this takes more than a day, finish the change from another device in Settings → Sync.`

Where no device can take over (§2.5), the exit stays what it is today: turn sync off and set it
up again from the device with the best data. No new flow is built for it here; the help text of
the sync settings names it.

## 5. Tests

Each written first and seen failing for its reason:

- a remover that never signs: after 24 hours another device takes over, every device verifies,
  and pairing, snapshot upload and removal work again; a device whose pull was stopped in the
  stretch applies it and goes on;
- the takeover removes the unfinished device, and one that does not;
- a removed device's old-key write placed in the stretch **after** the taker's first look: the
  taker refuses before the rotation; placed after the rotation: the taker does not sign; an op
  the look saw that is **missing** later: the same;
- the same write placed before the taker's first look and before everyone else's: accepted (the
  stated residual), with a test that says so;
- another device that was shown something else at its first look than the taker signed: its pull
  ends `Invalid`, nothing of it is applied;
- an op at a position of a looked stretch where the look found none ends the pull, with and
  without a takeover (the first look alone); an op of any key there is noted by the look;
- a device behind the taker's cursor applies below it only what it first saw;
- the unfinished device comes back: before and after it has seen the takeover; it signed before
  it saw it; it holds the takeover unverified and signs nothing for its own;
- an ordinary second key change does **not** drop an earlier unverified head (a joiner below two
  heads waits for both records); a version-3 record with `over` out of bounds is refused;
- the mark: rules 6, 9 and 10 with a marked epoch (an old-key snapshot below the taker's base is
  restored by a device catching up); no entry is made for a marked epoch after a merge restore
  and after a replace; a joiner without the mark sets it; the original remover's late record for
  a marked epoch is ignored; a refuted head survives the takeover;
- takeover refused: before 24 hours for the newest head; with a refuted head; from the remover;
  with the look not done or given up; too far behind; two takeovers at once; a takeover of a
  takeover before its own 24 hours, and after;
- the look: its place in the round; a stretch longer than one read and a pruned one get no look
  and are not asked again; a network failure is retried; the notes do not move the place or the
  watermark, with a snapshot-rule test that shows it; the notes' lifecycle (cursor passing,
  replace, sync off, re-join, server went back);
- the waiting notice names the newest head's maker, hides the button on the taker, and uses the
  texts without a name when there is none;
- the frozen version-3 record; a version-2 record still verifies;
- a quit at every await point of the takeover.

## 6. Left to the builder

Where the look's notes are stored (the table of rule 9a's notes with a mark, or a table of their
own: either way they are told apart from handled positions) and where the end of a looked
stretch is kept after its entry is gone; the shape of the plan and command (`takeover` as a mode
of the existing plan and removal call, or calls of their own); the layout of the notice and the
dialog on desktop and phone; the texts in the other eight locales.

## 7. Decisions for the owner

- **T1** A takeover after 24 hours, started by a person: approve / another waiting time / no
  takeover.
- **T2** The first look (recommended; §2.2 says exactly what it does and does not buy) / sign
  what is there at the takeover and build no look.
- **T3** Record version 3 with `over`, and the mark: approve.
- **T4** The "remove it" checkbox unticked / ticked / no checkbox (a takeover never removes).
- **T5** No takeover from the remover itself in this version: approve / design one.

## 8. What the planning reader's questions changed

1. *Which head's age gates a takeover, and whom the notice names.* Every unverified head must
   have waited its 24 hours, so a fresh takeover cannot be overridden at once; the notice names
   the newest one's maker; a replace restarts the count (§2.3, §3).
2. *What a taken-over epoch leaves behind.* The mark, and what rules 6, 9, 10 and the
   after-restore step do with it (§2.4). Without it the existing code would wait for the
   original remover again after a merge restore and would never admit an old-key snapshot.
3. *The taker's and the returning remover's bookkeeping.* The taker drops the named entries
   where it signs; an own unsigned entry is dropped when the takeover is verified; a device that
   holds a later unverified head no longer signs its own earlier one; the exact exception in
   `rotate::check`; no button on the taker (§2.3, §3).
4. *The look's window is longer than first written.* It runs until a device first looks, and a
   dishonest server can delay that. §2.2 now says what the look buys and what it does not, and
   T2 is put to the owner on that footing.
5. *What the takeover's range covers when the taker is up to date.* Everything above its cursor,
   which is where a write can be added; below it each device's own look holds; a missing op
   counts as a difference (§2.3).
6. *When a look ends or is given up, its place in the round, what it notes.* §2.2.
7. *A head whose maker has no name.* Texts without a name, no checkbox (§3).
8. *A refuted head and the bounds of `over`.* A refuted head survives a takeover; `over` is
   bounded; verified and unknown epochs are not marked; a late record for a marked epoch is
   ignored (§2.3, §2.4).
9. *A taker whose pull is stopped may fall too far behind; the server's rotation limit.* Both
   are named among the cases where no device can take over (§2.5).
