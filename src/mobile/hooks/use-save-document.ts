import { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { AttachmentKind } from '@shared/schema';
import { attachmentsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { exportsOnThisDevice, stagedFileName } from '@/mobile/lib/export-file';
import { saveStagedExport } from '@/mobile/lib/picked-files';

/**
 * "Uložit do Souborů" in the document viewer: the one way a document leaves the phone, and the way
 * out when the phone cannot show it. Rust stages a decrypted copy under the document's own name
 * (`stage_attachment_export`; Documents on iOS, the private exports folder on Android), the
 * system's save picker exports it to the place the person picks (the Files app; Android's
 * document picker), and Rust removes the copy again whatever the person chose. No copy is left
 * behind: a removal that fails is told, and the startup sweep removes a copy an interrupted
 * export left. A saved copy is told together with the fact that it is not encrypted (the
 * desktop's "Uložit kopii" toast); a cancelled picker is not told; a failure is told with its
 * message. Nothing about the document is logged.
 */
export function useSaveDocument(): {
  save: (kind: AttachmentKind, id: string) => Promise<void>;
  isPending: boolean;
} {
  const { t } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [isPending, setPending] = useState(false);

  const save = useCallback(
    async (kind: AttachmentKind, id: string) => {
      if (!exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '')) {
        toast(t('backup.unavailableHere'));
        return;
      }
      setPending(true);
      let staged: string | null = null;
      try {
        staged = await attachmentsApi.stageExport(kind, id);
        const saved = await saveStagedExport(staged, { defaultPath: stagedFileName(staged) });
        // The copy goes before the toast: a removal that fails is the error to tell
        const discarding = staged;
        staged = null;
        await attachmentsApi.discardExport(discarding);
        if (saved) {
          toast(tc('attachments.viewer.saved'), {
            description: tc('attachments.viewer.savedDescription'),
          });
        }
      } catch (error) {
        toast.error(tc('status.error'), {
          description: translateApiError(error instanceof Error ? error : String(error), tc),
        });
      } finally {
        // A picker or a copy into Android's document that failed: the staged copy still goes
        if (staged !== null) await attachmentsApi.discardExport(staged).catch(() => undefined);
        setPending(false);
      }
    },
    [t, tc]
  );

  return { save, isPending };
}
