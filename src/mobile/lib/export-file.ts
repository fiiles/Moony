/**
 * Getting a file out of the phone and a backup back in (spec §8.9, Záloha a přenos): the pure
 * parts of the export through the system's Files picker and of the restore.
 *
 * On iOS the dialog plugin's `save()` takes the file name from `defaultPath`, creates an empty file
 * at `Documents/<name>` only when none exists, and exports that file to the place the person picks.
 * So the file is put into Documents first, exported, and removed again whatever the person chose.
 * Rust does the staging and the removing (`stage_backup_export` / `finish_backup_export` for a
 * backup, `stage_text_export` / `discard_text_export` for the recovery key's text): the webview has
 * no fs commands. Android stages the same way (in a private cache folder) and Rust copies the
 * staged file into the document its save picker created (`saveStagedExport` in `picked-files.ts`).
 */

/** The JSON of all data (Záloha a přenos → Export), the name Rust stages it under (`DATA_EXPORT_FILE_NAME`). */
export const DATA_EXPORT_FILE = 'moony-export.json';

/** The recovery key's text file, one of the names Rust allow-lists for staging (`TEXT_EXPORT_FILE_NAMES`; the others are the class pages' CSVs, `use-class-export.ts`). */
export const RECOVERY_KEY_FILE = 'moony-recovery-key.txt';

/**
 * The Sync Recovery Kit's two files (Settings → Sync → Show Recovery Kit, "Save as text" / "Save as
 * page"). Rust's `TEXT_EXPORT_FILE_NAMES` must allow both names before the phone can stage them.
 */
export const SYNC_KIT_TEXT_FILE = 'moony-sync-recovery-kit.txt';
export const SYNC_KIT_PAGE_FILE = 'moony-sync-recovery-kit.html';

/**
 * The new recovery key of a password change, `moony-recovery-key-2026-10-25-1742.txt`: the local day
 * and time of the attempt (the Rust allow-list accepts exactly this shape, and the day-only one of
 * older builds). A change may be abandoned, so its key must never replace the first run's
 * `RECOVERY_KEY_FILE`, which holds the key that then stays valid; the time keeps two attempts on
 * one day from sharing a name, so a later save never overwrites an earlier key in the Files app.
 */
export function changedRecoveryKeyFileName(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  return `moony-recovery-key-${day}-${pad(date.getHours())}${pad(date.getMinutes())}.txt`;
}

/** What an export came to: a file in a place the person chose, or nothing (the picker was cancelled). */
export type ExportOutcome = 'saved' | 'cancelled';

/**
 * Whether exporting through the system's save picker works on this platform
 * (`TAURI_ENV_PLATFORM`): the phones. iOS exports the staged file itself; on Android the picker
 * creates an empty document and Rust copies the staged file into it (`write_staged_export`).
 * Elsewhere (the phone UI in a desktop dev shell) nothing is staged.
 */
export function exportsOnThisDevice(platform: string): boolean {
  return platform === 'ios' || platform === 'android';
}

/** The backup archive's file name, `moony-backup-2026-10-17.zip` (the Rust command accepts only this form). */
export function backupFileName(isoDate: string): string {
  return `moony-backup-${isoDate}.zip`;
}

/**
 * The path a dialog result stands for. On iOS the dialog plugin hands over `file://` URLs
 * (percent-encoded), which this turns into the path they name: Rust needs the path, so the helper is
 * load-bearing there. Anything else is returned as it came, so a plain path with a `%` in it stays
 * intact. Android's `content://` URIs never come here: `pickedFilePath` has Rust copy them.
 */
export function dialogPathToFsPath(value: string): string {
  if (!value.startsWith('file://')) return value;
  const path = value.slice('file://'.length);
  try {
    return decodeURIComponent(path);
  } catch {
    // A broken escape: the path as it is names the file better than nothing
    return path;
  }
}

/** One step of a restore, in the order it is done. */
export type RestoreStep = 'disable-biometrics' | 'clear-asked' | 'restore' | 'reload';

/**
 * What restoring a backup does, in order. Biometric unlock goes off first when it is on: the key it
 * keeps behind Face ID / Touch ID opens the profile that is about to be replaced, not the restored
 * one (whose password the person types). The enrolment question is asked again afterwards, once the
 * restored profile has been unlocked with its password; the webview reloads to the lock screen.
 */
export function restoreBackupPlan(enrolled: boolean): RestoreStep[] {
  return [
    ...(enrolled ? (['disable-biometrics'] as const) : []),
    'clear-asked',
    'restore',
    'reload',
  ];
}
