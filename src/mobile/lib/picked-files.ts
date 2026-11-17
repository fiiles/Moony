import { save, type SaveDialogOptions } from '@tauri-apps/plugin-dialog';
import { dataApi, receivedFilesApi } from '@/lib/tauri-api';
import { dialogPathToFsPath } from '@/mobile/lib/export-file';

/**
 * Files in and out of the phone through the system pickers, the part that differs between iOS and
 * Android (`TAURI_ENV_PLATFORM`, read when called).
 *
 * iOS works with files: the save picker exports the file Rust staged in Documents under
 * `defaultPath`, and the open picker hands over a `file://` copy inside the app. Android works with
 * `content://` URIs that only its ContentResolver reads: the save picker creates an empty document
 * and returns its URI, and the open picker returns the URI of the person's file. Rust bridges both
 * (`write_staged_export`, `copy_picked_file`), so the callers keep one flow: stage, save, finish;
 * pick, read the copy, discard it.
 */
function isAndroid(): boolean {
  return (import.meta.env.TAURI_ENV_PLATFORM ?? '').startsWith('android');
}

/**
 * Exports a staged file (`dataApi.stage*Export`, `deviceSyncApi.stageSyncBackupExport`) through the
 * system's save picker; `true` once it reached a place the person chose, `false` for a cancelled
 * picker. On Android Rust copies the staged file into the document the picker created; a failed
 * copy rejects. The caller finishes or discards the staged copy either way.
 */
export async function saveStagedExport(
  staged: string,
  options: SaveDialogOptions
): Promise<boolean> {
  const target = await save(options);
  if (target === null) return false;
  if (isAndroid()) await dataApi.writeStagedExport(staged, target);
  return true;
}

/**
 * The path a file picked in the open picker is read from. iOS: the `file://` URL of the picker's
 * copy, as a path. Android: Rust copies the `content://` document into the app's inbox folder and
 * answers the copy's path. Either way the copy is the app's to discard
 * (`receivedFilesApi.discard`), and the startup sweep removes one an interruption left behind.
 */
export async function pickedFilePath(selected: string): Promise<string> {
  if (isAndroid()) return receivedFilesApi.copyPicked(selected);
  return dialogPathToFsPath(selected);
}
