import { useCallback } from 'react';
import { documentDir } from '@tauri-apps/api/path';
import { save } from '@tauri-apps/plugin-dialog';
import { remove, writeTextFile } from '@tauri-apps/plugin-fs';
import { stagedPath, type ExportOutcome } from '@/mobile/lib/export-file';

/**
 * Hands a short text to the system's Files picker (the recovery key's `.txt`): the text is written
 * to `Documents/<fileName>`, which the iOS save picker then exports to the place the person chooses,
 * and the staged file is removed again, whatever the person did. The capability allows exactly
 * this one file (`RECOVERY_KEY_FILE`, `mobile.json`), so `fileName` must be that name.
 *
 * A removal that fails rejects the whole export, even after the picker saved the file: the text
 * would still be lying in Documents, and the caller must say so rather than report a clean save.
 */
export function useExportTextFile(): {
  exportText: (fileName: string, contents: string) => Promise<ExportOutcome>;
} {
  const exportText = useCallback(
    async (fileName: string, contents: string): Promise<ExportOutcome> => {
      const staged = stagedPath(await documentDir(), fileName);
      await writeTextFile(staged, contents);
      try {
        // `null` is a cancelled picker
        const target = await save({ defaultPath: fileName });
        return target === null ? 'cancelled' : 'saved';
      } finally {
        await remove(staged);
      }
    },
    []
  );

  return { exportText };
}
