import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { CheckCircle2, CircleAlert } from 'lucide-react';
import type { IntegrityReport } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { attachmentsApi, dataApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { cn } from '@/lib/utils';
import { ATTACHMENT_STATUS_KEY, attachmentStatusLines } from '@/utils/attachment-status';
import { SectionHead } from '@/mobile/components/SectionHead';
import { DATA_EXPORT_FILE, exportsOnThisDevice } from '@/mobile/lib/export-file';
import { saveStagedExport } from '@/mobile/lib/picked-files';

/** The 48 pt buttons of the page (`BackupSection`'s). */
const BUTTON =
  'h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row';
/** The sentence under a section head. */
const LEAD = 'mx-0.5 mb-3 mt-0 text-m-sub text-ink-3';

type Busy = 'export' | 'verify' | null;

/**
 * The rest of the desktop's "Data a zálohy" card (`DataSection`) on Záloha a přenos (spec §8.9):
 * Export (every table as JSON, through the Files picker), Kontrola databáze (`verify_database`) and
 * the encrypted attachments' status (`AttachmentsStatusRow`'s lines). The data folder and its "Open
 * folder" stay on the desktop: a phone has no folder to open. The export is staged by Rust in
 * Documents, handed to the picker and removed again whatever the person chose; like the other
 * exports it works on iOS only for now (elsewhere the person is told and nothing is staged).
 */
export function DataTools() {
  const { t } = useTranslation('settings');
  const { t: tm } = useTranslation('mobile');
  const { t: tc } = useTranslation('common');
  const [busy, setBusy] = useState<Busy>(null);
  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null);
  const { data: status } = useQuery({
    queryKey: ATTACHMENT_STATUS_KEY,
    queryFn: () => attachmentsApi.status(),
    // While a conversion runs, follow its progress (as the desktop row does)
    refetchInterval: (query) => (query.state.data?.converting ? 2000 : false),
  });

  const showError = (error: unknown) =>
    toast.error(tc('status.error'), {
      description: translateApiError(error instanceof Error ? error : String(error), tc),
    });

  const exportAll = async () => {
    if (!exportsOnThisDevice(import.meta.env.TAURI_ENV_PLATFORM ?? '')) {
      toast(tm('backup.unavailableHere'));
      return;
    }
    setBusy('export');
    let staged: string | null = null;
    let saved: { tables: number; rows: number } | null = null;
    try {
      const summary = await dataApi.stageDataExport();
      staged = summary.path;
      // A cancelled picker: nothing to say
      if (await saveStagedExport(staged, { defaultPath: DATA_EXPORT_FILE }))
        saved = { tables: summary.tableCount, rows: summary.rowCount };
    } catch (error) {
      showError(error);
    }
    // The JSON is plain: it never stays in Documents. The export counts as saved only once the
    // staged copy is gone; a failed removal is said as the error instead.
    try {
      if (staged !== null) await dataApi.discardTextExport(staged);
      if (saved) toast(t('data.exportSaved'), { description: tm('backup.exportSummary', saved) });
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  const verify = async () => {
    setBusy('verify');
    try {
      const report = await dataApi.verifyDatabase();
      setIntegrity(report);
      if (report.ok) toast(t('data.verifyOk'));
      else toast.error(t('data.verifyFailed'), { description: report.messages.join('\n') });
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  const lines = status ? attachmentStatusLines(status) : [];

  return (
    <>
      <SectionHead title={t('data.exportRow')} />
      <p className={LEAD}>{t('data.exportHint')}</p>
      <Button
        variant="secondary"
        className={BUTTON}
        loading={busy === 'export'}
        disabled={busy !== null}
        onClick={() => void exportAll()}
      >
        {t('data.exportAll')}
      </Button>

      <SectionHead title={t('data.verifyRow')} />
      <p className={LEAD}>{t('data.verifyHint')}</p>
      <Button
        variant="secondary"
        className={BUTTON}
        loading={busy === 'verify'}
        disabled={busy !== null}
        onClick={() => void verify()}
      >
        {t('data.verify')}
      </Button>
      {integrity && (
        <div
          role="status"
          className={cn(
            'mx-0.5 mt-3 flex items-start gap-2 text-m-sub [&_svg]:mt-px [&_svg]:size-4 [&_svg]:shrink-0',
            integrity.ok ? 'text-gain' : 'text-loss'
          )}
        >
          {integrity.ok ? <CheckCircle2 aria-hidden /> : <CircleAlert aria-hidden />}
          <div className="min-w-0">
            {integrity.ok ? t('data.verifyOk') : t('data.verifyFailed')}
            {!integrity.ok && (
              <ul className="m-0 mt-1 list-disc break-words pl-4">
                {integrity.messages.map((message, i) => (
                  <li key={i}>{message}</li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {lines.length > 0 && (
        <>
          <SectionHead title={t('data.attachments.label')} />
          <div className="mx-0.5 grid gap-1 text-m-sub text-ink-3">
            {lines.map((line) => (
              <p key={line.key} className={cn('m-0', line.tone === 'loss' && 'text-loss')}>
                {t(line.key, line.params)}
              </p>
            ))}
            {status && status.leftovers.length > 0 && (
              <p className="m-0">
                {/* One key for "label: value" (B9): its colon is the language's */}
                <span className="block">
                  {t('mobile:labelValue', {
                    label: t('data.attachments.leftoversTitle'),
                    value: status.leftovers.length,
                  })}
                </span>
                {t('data.attachments.leftoversBody')}
              </p>
            )}
          </div>
        </>
      )}
    </>
  );
}
