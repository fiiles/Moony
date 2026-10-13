# 0015. App identifier not bound to a person

Date: 2026-10-13

## Status

Accepted. Supersedes the identifier rule of [0010](0010-new-identifier-and-single-schema-baseline.md)
("the identifier must never change again") for this one change.

## Context

The bundle identifier names the data folder, the log folder, the web view storage and the
installer product. 0.9.0 set it to `com.filipkral.moony`, which carried the author's name into
every user's file system and into the Windows publisher ("Filip Král"). The project now has its
own domain, and 0.10.1 is the last point at which the installed base is a handful of testers.

Three options were on the table: keep the identifier, change it and ask users to rename their
folder by hand, or change it and move the folder on start. A hand rename is silently skipped by
exactly the users who are least careful, and the result looks like lost data.

## Decision

- **The identifier is `com.moonyfinance.desktop` and the publisher is "Moony".** The copyright
  line keeps the author's name.
- **The first start moves the old data folder into place, once.** `services::data_dir` runs in
  `setup`, before the startup banner and before any command can open the database. It merges the
  legacy folder into the new one entry by entry: a name that already exists in the new folder
  stays in the old one (on Linux the log plugin has already created `logs/` there), every entry
  is renamed, or copied when the rename fails, and `moony.db` moves last, so an interrupted move
  is finished by the next start and a half-filled folder is never taken for a profile.
- **A new folder that holds `moony.db` is never touched.** After a copy the old folder is never
  deleted automatically; after a pure rename it is removed only when it is empty.
- **There is no second rename, ever.** The next change would make every user migrate twice.

## Consequences

- Interface preferences kept by the web view (lock-screen language, auto-lock) reset once. The
  log folder starts fresh; the old log and web view folders stay where they are and hold no
  financial data. Data and settings inside the database are unchanged.
- Attached documents keep the absolute path they had when they were added. Opening and deleting
  re-root such a path under the current data folder (`services::document_files`); photos are
  stored relative and need nothing.
- Going back to 0.10.0 after the move shows the first-run wizard, because 0.10.0 looks in the old
  folder. Downgrades are not supported.
- On Windows the NSIS installer keeps its install folder under `Software\<publisher>\Moony`. With
  the publisher changing, the first 0.10.1 installer cannot find a custom install folder: an
  install in a custom folder gets a second copy in the default folder, and running the new
  `setup.exe` by hand with "uninstall before installing" can report that the old version cannot
  be uninstalled ("Don't uninstall" works). Auto-updates into the default folder are unaffected.
  `setup.exe` had two downloads in total, so this is accepted.
- macOS code signing, when it comes, binds to this identifier.
- ADR 0010's rule to keep the identifier is superseded for this one change; the identifier is
  final from here on.
