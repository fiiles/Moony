/**
 * Phone copy that names an iOS-only place (iCloud, iCloud Drive, the Files app) has an Android
 * sibling under the same key plus an `Android` suffix. `phoneCopyKey` picks the variant for the
 * platform the app was built for; every other platform keeps the key as written.
 */
export const ANDROID_COPY_KEYS: ReadonlySet<string> = new Set([
  // mobile namespace
  'backup.what',
  'backup.osBackupNote',
  'backup.osBackupNoteSync',
  'backup.syncLead',
  'backup.syncSaveCopy',
  'backup.syncCopySaved',
  'firstRun.points.localOnly.text',
  'firstRun.points.localOnly.textSync',
  'firstRun.recoveryLead',
  'firstRun.saveToFiles',
  'firstRun.backupLead',
  // deviceSync namespace
  'replace.saved.filesHint',
  'replace.saved.saveCopy',
  'setup.kit.saveShare',
]);

export function phoneCopyKey(
  key: string,
  platform: string = import.meta.env.TAURI_ENV_PLATFORM ?? ''
): string {
  return platform.startsWith('android') && ANDROID_COPY_KEYS.has(key) ? `${key}Android` : key;
}
