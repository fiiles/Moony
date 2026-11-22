# 0035. Device sync released, own servers only

Date: 2026-11-22

## Status

Accepted. Turns on the runtime flag that [ADR 0025](0025-device-sync-relay-client.md) put in front of
device sync (`services/sync/feature.rs`, `RELEASED`) and narrows the release phase of the client
spec (`docs/specs/2026-11-06-device-sync-client-design.md` §17, phase 6) to own servers. The
setup path is the one of [ADR 0029](0029-self-hosted-sync-server-in-the-app.md).

## Context

Sync was built in phases 1–5 behind `RELEASED = false`: only debug builds ran it. Phase 6 bundled
several things: deploying the Moony cloud with payments, the privacy policy and README text, dry
runs on copies of real profiles, and turning the flag on. The owner's decision (2026-11-22) is to
release sync now, in desktop and phone release builds alike, for own servers only. The Moony cloud
stays a choice in the setup that cannot be picked ("Coming soon"), as it already was.

## Decision

- **`RELEASED` is `true`.** Every build runs sync: the engine, the commands, Settings → Sync and the
  top bar's sync status. A release build still never reads `MOONY_DEV_SYNC` or `MOONY_RELAY_URL`,
  and the developer tools of sync (the measurements) stay debug-only (`feature::dev_tools`).
  `MOONY_DEV_SYNC=0` no longer turns sync off in a debug build.
- **Own servers only.** The cloud card stays disabled on the desktop (`ServerChoice`) and the phone
  (`ServerChoiceBlock`); `DEFAULT_RELAY_URL` stays the cloud's address and is reached by nothing
  a person can set up.
- **The texts follow.** The camera permission text names the scanning of pairing codes and the
  Recovery Kit's QR code again in all ten languages (`permission-strings.test.ts` ties it to the
  flag). `PRIVACY.md` gains a "Device sync" section and a row for the sync server; README and
  ROADMAP no longer say there is no sync.

## Consequences

- The next desktop release and the next phone build carry sync. Turning it on is a person's own
  step (Settings → Sync → Set up, with a connection code), so a profile without sync behaves as
  before; the change-capture triggers already ran in every build (ADR 0027, M12).
- **Before that release goes out:** the sync server's repository (`fiiles/moony-sync`, which
  `PRIVACY.md` links) must be public, or nobody but the owner can set sync up; and the real-profile
  dry runs and owner checks on real hardware owed from ADRs 0025–0027, 0030 and 0032 are still
  open. Both are the owner's.
- The Moony cloud and payments remain future work. When the cloud runs, enabling its card is a
  change of its own (with its text in `PRIVACY.md`, which now promises to describe it first).
