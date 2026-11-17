import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Sheet } from '@/mobile/components/Sheet';
import { useRetained } from '@/mobile/hooks/use-retained';
import { useSyncBackupExport } from '@/mobile/hooks/use-sync-backup-export';
import { outcomeSentences, type ReplaceSaved } from '@/mobile/lib/replace-saved';
import { phoneCopyKey } from '@/mobile/lib/platform-copy';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

interface SyncBackupSavedSheetProps {
  /** The replace that just ended with a kept backup; `null` while nothing is to be said. */
  saved: ReplaceSaved | null;
  onClose: () => void;
}

/**
 * After a join, repair or re-sync whose backup is kept: says a backup was saved on this phone, what
 * only it holds and where it is listed (Záloha a přenos), and on iOS offers "Save a copy to Files"
 * (audit H4: nothing else puts it where the person can reach it). Elsewhere it only says where the
 * backup is listed.
 */
export function SyncBackupSavedSheet({ saved, onClose }: SyncBackupSavedSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { t: tm } = useTranslation('mobile');
  const { canExport, exporting, exportCopy } = useSyncBackupExport();
  const shown = useRetained(saved);
  const busy = exporting !== null;

  return (
    <Sheet
      open={saved !== null}
      locked={busy}
      onOpenChange={(next) => {
        if (!next && !busy) onClose();
      }}
      title={t('replace.saved.title')}
      description={shown?.headline}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            disabled={busy}
            onClick={onClose}
          >
            {tc('buttons.close')}
          </Button>
          {canExport && shown && (
            <Button
              className={`${BUTTON} flex-1`}
              loading={busy}
              onClick={() => void exportCopy(shown.outcome.backupPath)}
            >
              {t(phoneCopyKey('replace.saved.saveCopy'))}
            </Button>
          )}
        </>
      }
    >
      {shown && (
        <div className="grid gap-2 text-m-lead text-ink-2" data-testid="sync-backup-saved">
          {outcomeSentences(shown.outcome).map((line) => (
            <p key={line.key} className="m-0">
              {t(line.key, { count: line.count })}
            </p>
          ))}
          <p className="m-0">{t('replace.saved.where')}</p>
          <p className="m-0 text-ink-3">
            {canExport ? t(phoneCopyKey('replace.saved.filesHint')) : tm('backup.syncNoCopy')}
          </p>
        </div>
      )}
    </Sheet>
  );
}
