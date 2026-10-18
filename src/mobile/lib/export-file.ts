/**
 * Getting a file out of the phone and a backup back in (spec §8.9, Záloha a přenos): the pure
 * parts of the export through the system's Files picker and of the restore.
 *
 * On iOS the dialog plugin's `save()` takes the file name from `defaultPath`, creates an empty file
 * at `Documents/<name>` only when none exists, and exports that file to the place the person picks.
 * So the file is put into Documents first, exported, and removed again whatever the person chose.
 * Rust does the staging and the removing (`stage_backup_export` / `finish_backup_export` for a
 * backup, `stage_text_export` / `discard_text_export` for the recovery key's text): the webview has
 * no fs commands. Only iOS exports this way (`exportsOnThisDevice`).
 */

/** The one text file the phone stages in Documents (an allow-listed name in Rust): the recovery key's text. */
export const RECOVERY_KEY_FILE = 'moony-recovery-key.txt';

/** What an export came to: a file in a place the person chose, or nothing (the picker was cancelled). */
export type ExportOutcome = 'saved' | 'cancelled';

/**
 * Whether exporting through the Files picker works on this platform (`TAURI_ENV_PLATFORM`): iOS
 * only. On Android the picker creates an empty document and returns its URI, nothing copies the
 * staged file into it, so an "export" would be an empty file recorded as a backup. Android needs
 * its own way to write the file to the chosen URI; until then it does not export. Restoring reads
 * a picked file through Rust and works everywhere.
 */
export function exportsOnThisDevice(platform: string): boolean {
  return platform === 'ios';
}

/** The backup archive's file name, `moony-backup-2026-10-17.zip` (the Rust command accepts only this form). */
export function backupFileName(isoDate: string): string {
  return `moony-backup-${isoDate}.zip`;
}

/**
 * The path a dialog result stands for. The dialog crate hands over plain paths, but a `file://` URL
 * (percent-encoded) is turned into the path it names; anything else is returned as it came, so a
 * plain path with a `%` in it stays intact.
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
