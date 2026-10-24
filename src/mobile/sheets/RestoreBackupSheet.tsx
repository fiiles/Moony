import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import { Sheet } from '@/mobile/components/Sheet';
import { useRetained } from '@/mobile/hooks/use-retained';
import type { BackupInspection } from '@shared/schema';

interface RestoreBackupSheetProps {
  /** The archive the person picked, read; `null` while nothing is waiting for an answer. */
  inspection: BackupInspection | null;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
  isPending?: boolean;
}

/**
 * "Nahradit vaše data touto zálohou?" (spec §8.9, §10.5): the destructive confirmation of a
 * restore, with what the archive holds — its date, the Moony version that made it, the attachments
 * and the size. The sentence is the phone's own (`backup.restoreConfirmDescription`): the desktop's
 * promises a "pre-restore" folder with the old data, which the phone gives nobody access to, so it
 * says to back up first. An archive from a newer Moony says so in `text-loss` and cannot be
 * restored. The caller closes the sheet when the restore has succeeded; while it runs nothing
 * leaves the sheet.
 */
export function RestoreBackupSheet({
  inspection,
  onOpenChange,
  onConfirm,
  isPending = false,
}: RestoreBackupSheetProps) {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const fmt = useFormat();
  // The sheet keeps what it showed while it slides away
  const shown = useRetained(inspection);

  return (
    <Sheet
      open={inspection !== null}
      onOpenChange={(next) => {
        if (!next && isPending) return;
        onOpenChange(next);
      }}
      title={t('data.restoreConfirmTitle')}
      description={
        shown
          ? tm('backup.restoreConfirmDescription', {
              date: fmt.dateTime(shown.manifest.createdAt),
              version: shown.manifest.appVersion,
              attachments: shown.attachmentCount,
              size: formatBytes(shown.archiveBytes, fmt.locale),
            })
          : undefined
      }
      footer={
        <>
          <Button
            variant="ghost"
            className="h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] px-[18px] text-m-row"
            disabled={isPending}
            onClick={() => onOpenChange(false)}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            variant="destructive"
            className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
            loading={isPending}
            disabled={isPending || shown?.newerThanApp}
            onClick={onConfirm}
          >
            {isPending ? t('data.restoring') : t('data.restoreConfirm')}
          </Button>
        </>
      }
    >
      {shown?.newerThanApp ? (
        <p className="m-0 text-m-sub font-semibold text-loss">{t('data.restoreNewer')}</p>
      ) : null}
    </Sheet>
  );
}
