/**
 * Getting a file out of the phone and a backup back in (spec §8.9, Záloha a přenos): the pure
 * parts of the export through the system's Files picker and of the restore.
 *
 * On iOS the dialog plugin's `save()` takes the file name from `defaultPath`, creates an empty file
 * at `Documents/<name>` only when none exists, and exports that file to the place the person picks.
 * So the file is put into Documents first (a backup by the Rust command `stage_backup_export`, the
 * recovery key's text by the fs plugin), exported, and removed again whatever the person chose.
 */

/** The one text file the webview may write (and remove) in Documents: the recovery key's text. */
export const RECOVERY_KEY_FILE = 'moony-recovery-key.txt';

/** What an export came to: a file in a place the person chose, or nothing (the picker was cancelled). */
export type ExportOutcome = 'saved' | 'cancelled';

/** The backup archive's file name, `moony-backup-2026-10-17.zip` (the Rust command accepts only this form). */
export function backupFileName(isoDate: string): string {
  return `moony-backup-${isoDate}.zip`;
}

/** `dir` and `name` with one slash between them, whether or not `dir` ends in one. */
export function stagedPath(dir: string, name: string): string {
  return `${dir.replace(/\/+$/, '')}/${name}`;
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
