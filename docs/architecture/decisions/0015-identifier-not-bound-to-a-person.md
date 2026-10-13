# 0015. App identifier not bound to a person

Date: 2026-10-13

## Status

Accepted. Amends the identifier rule of [0010](0010-new-identifier-and-single-schema-baseline.md)
("the identifier must never change again") for this one change.

## Context

The bundle identifier names the data folder, the log folder, the web view storage and the
installer product. 0.9.0 set it to `com.filipkral.moony`, which carried the author's name into
every user's file system and into the Windows publisher ("Filip Král"). The project now has its
own domain.

Three options were on the table: keep the identifier, change it and move the data folder
automatically on the first start, or change it and let users rename the folder by hand.

A startup move was designed, implemented and reviewed. It touches every existing user's
encrypted data folder at the one moment a mistake is expensive, and the review found a path that
loses the key files (a failed move left them stranded and the first-run wizard then overwrote
them) and several ways to strand part of a profile. There are almost no installations, and the
releases before 0.11.0 are withdrawn, so the move would be code that runs once for almost nobody
and stays in the code base for good.

## Decision

- **The identifier is `com.moonyfinance.desktop` and the publisher is "Moony".** The copyright
  line keeps the author's name.
- **Moony does not move the old data folder.** `PRIVACY.md` (Where your data lives) tells anyone
  who ran 0.10.0 or earlier to quit Moony and rename the old folder to the new name before the
  first start of 0.11.0, and what to do if 0.11.0 has already created a new folder.
- **There is no further identifier change, ever.** The name is final; a later change would make
  every user rename the folder again.

## Consequences

- Existing users rename the data folder by hand. Someone who does not sees the first-run wizard,
  with the old data untouched in the old folder.
- Web view storage (lock-screen language, auto-lock, the automatic update check, which is on by
  default) and the logs start fresh; the old log and web view folders stay where they are. Data
  and settings inside the database are unchanged.
- Attached documents keep the absolute path they had when they were added. Opening and deleting
  look under the current data folder first and fall back to the stored path
  (`services::document_files`); photos are stored relative and need nothing.
- On Windows the NSIS installer keeps its install folder under `Software\<publisher>\Moony`. With
  the publisher changing, the first 0.11.0 installer cannot find a custom install folder: an
  install in a custom folder gets a second copy in the default folder, and running the new
  `setup.exe` by hand with "uninstall before installing" can report that the old version cannot
  be uninstalled ("Don't uninstall" works). Auto-updates into the default folder are unaffected.
- macOS code signing, when it comes, binds to this identifier.
- ADR 0010's rule to keep the identifier applies again from here on.
