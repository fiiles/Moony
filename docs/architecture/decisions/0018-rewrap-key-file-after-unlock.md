# 0018. Re-wrap the password key file at the current cost after unlock

Date: 2026-10-15

## Status

Accepted. Amends [0009](0009-key-derivation-parameters.md): takes up the follow-up it left open
("Existing users keep N=2^14 until they change their password").

## Context

ADR 0009 raised the password KDF to scrypt N=2^17, but only for key files written at setup,
password change or recovery. Profiles whose password was set earlier keep a 60-byte `key.enc` at
N=2^14 until the password changes, which most users never do. Re-wrapping at unlock was left
out to keep the unlock path free of writes.

Since ADR 0017 a backup carries both databases and the key files, so the main attack on a
stolen backup is guessing the password offline against `key.enc` and `salt`. At N=2^14 a guess
costs ~20 ms and 16 MiB; at N=2^17, ~170 ms and 128 MiB. The profiles still at N=2^14 are the
oldest ones.

Alternatives considered: wait for a password change (most never happen); ask users to change
their password (a nag for something the app can do itself); re-wrap on a background thread
after unlock (races a password change started right after unlock and could leave `key.enc`
wrapped under the old password).

## Decision

- After a successful password unlock (the master key is unwrapped, the database opens and the
  profile loads), `auth::unlock` re-wraps `key.enc` under the same password and master key with
  `KdfParams::CURRENT` when the file is headerless or CURRENT needs at least as much memory (r·N)
  and work (r·N·p) per guess as its header, and more of one. A file at CURRENT, stronger, or
  stronger on either axis is left alone, so a re-wrap never lowers an attacker's cost.
- It runs synchronously, before the session counts as unlocked. A failure is logged as a
  warning without paths or secrets and the unlock goes on with the old file, which still opens
  the profile; the next unlock tries again.
- Every write of `key.enc` and `recovery.enc` is atomic (`salt` is written once, at setup). The
  new content goes to `<name>.tmp` in the same folder (owner-only), is fsynced and renamed over
  the file, then the folder is fsynced on Unix (`std` has no directory handle on Windows, where
  the rename replaces the target in one step). A crash leaves the old or the new file, never a
  truncated one. Setup, password change and recovery share this write, and a process-wide lock
  serialises it so two writes of one file cannot share the staging file. A failed folder fsync
  after the rename is only logged: the new file is in place, and a password change must not then
  report failure.
- A leftover `.tmp` is overwritten by the next write of that file and removed by Delete Account.
  Backups list their files by name and never include it.
- `recovery.enc` stays headerless at the legacy cost, as 0009 decided: the recovery key has 120
  bits of entropy, so a slow KDF adds nothing, and older builds can still recover with it.

## Consequences

- The first unlock after the update spends one more N=2^17 derivation (~170 ms and 128 MiB on an
  M4 Pro) and one small write; later unlocks are unchanged.
- Unlock is no longer free of writes. The write is atomic and happens at most once per profile
  (again only if `CURRENT` is raised).
- Every public release since 0.9.0 reads the headered format, so an upgraded profile still opens
  after a downgrade to any of them.
- Backups made before the upgrade keep the N=2^14 wrapper. Only backups made afterwards carry
  the stronger one.
- A password change or recovery still rewrites `key.enc` and `recovery.enc` one after the other.
  A crash between the two can leave the new password with the old recovery key; both still open
  the profile, as before this change.
- On Windows, a key file held open without delete sharing (antivirus, a sync client) makes the
  rename fail. The write then reports an error and leaves the old file. There is no retry: an
  unlock tries again next time, and a password change can be repeated.
- Raising `CURRENT` later moves every profile to the new cost at its next unlock with no further
  code.
