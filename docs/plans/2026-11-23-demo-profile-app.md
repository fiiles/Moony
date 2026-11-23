# Demo profile inside the app: build plan

Spec: `docs/specs/2026-11-23-demo-profile-design.md` (approved by the owner on 2026-11-23:
"Ano, postav to"). Look and copy: `docs/specs/2026-11-23-demo-profile-prototype.html` (Czech,
six scenes; open it in a browser, the notes drawer explains each scene). The data generator
exists: `services::demo::seed` (see `docs/plans/2026-11-23-demo-profile-data.md`).

Desktop only in this round. The phone keeps today's first run (no choice screen, no demo).

## Behaviour to build

1. **No real profile yet** (`check_setup` false): after the brand intro the app shows the
   choice screen instead of going straight to the wizard. "Založit profil" opens today's
   wizard, unchanged. "Otevřít ukázku" generates the demo in the language selected on that
   screen and enters the app with no password. The two "already use Moony" links are repeated
   on the choice screen and lead where the wizard's own links lead.
2. **In the demo** the app works as usual, with: the dashed sidebar card and "Ukázkový profil"
   account row, the "Ukázková data" top-bar badge with its popover, a one-time dismissable
   welcome card on the Overview, a profile menu with "Založit vlastní profil", "Obnovit
   původní data" and "Ukončit ukázku" (no "Zamknout aplikaci"), a Settings section "Ukázka",
   and Security, Data a zálohy, Synchronizace, AI asistent shown but locked with one sentence
   and a button to create a profile. No fingerprint offer, no backup reminder, no auto-lock.
3. **Reset** ("Obnovit původní data", confirmed in a dialog) throws the demo away and
   generates it again, in the profile's current language.
4. **Exit** ("Ukončit ukázku") closes the demo and returns to the choice screen; the demo
   stays on disk. The choice screen then offers "Pokračovat v ukázce" with when it was last
   open, and "Začít znovu s původními daty" (deletes the stored demo).
5. **To an own profile** ("Založit vlastní profil" anywhere in the demo): closes the demo and
   opens the wizard at step 1, which is exactly today's step plus a back button "Zpět do
   ukázky" (reopens the demo) and one sentence that demo data does not carry over. Entered
   from the choice screen, the back button reads "Zpět" and returns there.
6. **The demo is deleted** the moment a real profile exists: account created in the wizard,
   backup restored, or sync joined. A leftover demo folder next to a real profile is removed
   at startup.
7. **Every later launch** without a real profile starts on the choice screen. With a real
   profile nothing changes from today.

## Backend shape

- The demo lives in `<app data dir>/demo/` (`moony.db`, `attachments.db`, a small state file
  with the last-opened time). It is encrypted with a fixed key that is a constant in the code:
  it protects nothing, it only keeps one storage format. No `salt`, `key.enc`, `recovery.enc`.
- One process-wide "demo is open" flag and ONE resolver for "the folder of the open profile"
  that every `app_data_dir()` call site goes through or is consciously excluded from
  (`check_setup` and everything about the real profile's key files must keep using the real
  folder). The audit of call sites is part of the task and is listed in the ADR.
- `services/demo/profile.rs` (logic, `&Connection`/paths, tested with temp dirs) and thin
  commands: status, open (create when missing), reset, close, discard.
- Gates while the demo is open, enforced in Rust and reflected in the UI: no sync engine and
  no sync setup, no MCP server, no biometric unlock, no backup/restore, no password change or
  recovery, no account deletion, no auto-lock; the `backup_stale` reminder is not produced.
- Creating the demo is not atomic (the generator's services open their own transactions): a
  failed or interrupted generation deletes the folder; opening a folder that fails its
  completeness check regenerates it.
- Wire contract per `docs/standards/type-contract.md` (7 steps); new ADR in
  `docs/architecture/decisions/` + index; `docs/architecture/overview.md` gets a short part.

## Tasks (one writer at a time, each ends with a commit; gates before every commit)

1. **Backend demo mode.** Resolver + flag, `profile.rs`, commands and registration, gates,
   discard on the three ways a real profile comes to exist and at startup, wire types
   (`shared/schema.ts`, bindings, regenerated `generated-types.ts`), `demoApi` in
   `src/lib/tauri-api.ts`, Rust tests, ADR, overview.
2. **First run.** `useAuth` learns the demo (status on start incl. a webview reload while the
   demo is open, open/close/reset), the choice screen, the wizard's back button and sentence,
   the returning state, desktop-only switch, texts in ten locales.
3. **In the demo.** Sidebar card, account row and menu, top-bar badge and popover, Overview
   welcome card, Settings "Ukázka" + locked sections, reset dialog, no fingerprint offer,
   design-system note for the dashed "sample" motif, texts in ten locales.
4. **Review, live walk-through, full gates** (controller).

Gates: `npm run lint`, `npm run typecheck`, `npm test`, `cargo fmt`, `cargo clippy --lib
--bins --tests -- -D warnings`, the touched Rust test modules; the full Rust suite once at
the end.
