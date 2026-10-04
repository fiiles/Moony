# 0009. Versioned scrypt parameters for the password key file

Date: 2026-10-02

## Status

Accepted

## Context

The database is encrypted with a random 32-byte master key. That key is stored
twice, wrapped with AES-256-GCM: `key.enc` under a key derived from the user's
password, `recovery.enc` under a key derived from the recovery key. Both
derivations used scrypt with N=2^14, r=8, p=1 (the Node.js `crypto.scrypt`
defaults, kept from the original Node.js implementation of the format). A
security review flagged that as weak for 2026: OWASP's minimum for scrypt is N=2^17
with r=8, p=1 (equivalent trade-offs such as N=2^14 need p=5).

Only the password side needs the cost: a stolen `key.enc` + `salt` lets an
attacker guess passwords offline at the speed of one scrypt per guess, and
users choose short passwords (the minimum is 8 characters). The recovery key
is 24 random characters from a 32-symbol alphabet (120 bits); no KDF cost makes
that guessable or safer.

Measured on this machine (Apple M4 Pro, release build with LTO, scrypt 0.12,
median of 5 runs, r=8, p=1):

| N | memory | one derivation |
|---|--------|----------------|
| 2^14 (old) | 16 MiB | 20 ms |
| 2^15 | 32 MiB | 41 ms |
| 2^16 | 64 MiB | 83 ms |
| 2^17 | 128 MiB | 166 ms |
| 2^18 | 256 MiB | 335 ms |

A low-end laptop is several times slower, so 2^17 is roughly 0.5-1 s there;
unlock is a once-per-session action. Changing the cost needs a way for a key
file to say which cost it used, otherwise every existing profile would stop
unlocking.

Alternatives considered: Argon2id (the better algorithm, but a new dependency
and a bigger format change for the same practical gain at this threat level);
re-wrapping silently at every unlock (needs an atomic key-file rewrite on the
hot path; not worth the risk for this step); changing the cost with no header
(locks out every existing profile).

## Decision

- `key.enc` gains an 8-byte header in front of the existing 60-byte blob:
  `"MNYK"`, format version `1`, `log2(N)`, `r`, `p` (68 bytes in total). A file
  of exactly 60 bytes has no header and is read with the legacy parameters
  (N=2^14, r=8, p=1), so every existing profile keeps unlocking unchanged; a
  known-answer test pins a `key.enc` written by the old code.
- `KdfParams::CURRENT` is N=2^17, r=8, p=1. It is used whenever the password
  wrapper is (re)written: first-time setup, password change, and account
  recovery (which sets a new password). A profile therefore moves to the new
  cost the next time its password is set, not at unlock.
- `recovery.enc` stays headerless at the legacy cost. That keeps recovery fast,
  and an older build can still open a profile through its recovery key.
- Parameters read from a file are range-checked (N from 2^14 to 2^18, r and p
  from 1 to 16, at most 512 MiB) before scrypt runs, so a damaged or hostile
  file is reported as `auth.keyFilesCorrupted` instead of exhausting memory
  or time. The header is not authenticated separately: a tampered value can
  only make decryption fail, never weaken it.
- Debug builds compile the KDF crates (`scrypt`, `salsa20`, `pbkdf2`, `sha2`)
  with `opt-level = 3`, so unlock and the key-file tests stay fast.

## Consequences

- A profile whose password was set with this version cannot be unlocked by an
  older build (it sees a 68-byte `key.enc` and reports corrupted key files);
  `recovery.enc` still works there. The first release after this change is
  the one that carries the format.
- Unlock, password change and recovery take ~170 ms more on a fast machine and
  allocate 128 MiB transiently.
- Existing users keep N=2^14 until they change their password. A follow-up
  could re-wrap `key.enc` after a successful unlock using an atomic write
  (temporary file, fsync, rename); it was left out to keep the unlock path
  free of writes.
- Raising `CURRENT` later (or moving to Argon2id) only needs a new header
  version/parameters; readers of this version reject unknown versions as
  corrupted, so a format bump must ship in a release before it is written.
