/**
 * Settings → Data: the attachments row (encrypted-attachments spec §4.8, §4.11). It shows how
 * many attachments are stored encrypted, the background conversion of plain files from earlier
 * versions, files missing on disk, an unavailable store (D11), and plain originals the app found
 * outside its folder and never deletes (D5), each with a "show" action.
 */
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { SettingsRow } from '@/components/settings/SettingsCard';
import { attachmentsApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';

const ATTACHMENT_STATUS_KEY = ['attachment-status'] as const;

export function AttachmentsStatusRow() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const { data: status } = useQuery({
    queryKey: ATTACHMENT_STATUS_KEY,
    queryFn: () => attachmentsApi.status(),
    // While a conversion runs, follow its progress; the finish event refreshes it anyway.
    refetchInterval: (query) => (query.state.data?.converting ? 2000 : false),
  });

  const reveal = async (path: string) => {
    try {
      await attachmentsApi.revealLeftover(path);
    } catch (error) {
      toast.error(tc('status.error'), {
        description: translateApiError(error instanceof Error ? error : String(error), tc),
      });
    }
  };

  if (!status) return null;

  const lines: { text: string; tone?: 'loss' }[] = [];
  if (!status.storeAvailable) {
    lines.push({ text: t('data.attachments.unavailable'), tone: 'loss' });
  } else if (status.converting) {
    lines.push({
      text: t('data.attachments.converting', { done: status.done, total: status.total }),
    });
  } else if (status.stored > 0) {
    lines.push({ text: t('data.attachments.stored', { n: status.stored }) });
  } else if (status.legacyPending === 0) {
    lines.push({ text: t('data.attachments.none') });
  }
  if (!status.converting && status.legacyPending > 0) {
    lines.push({ text: t('data.attachments.pending', { n: status.legacyPending }) });
  }
  if (status.failed > 0) {
    lines.push({ text: t('data.attachments.failed', { n: status.failed }), tone: 'loss' });
  }
  if (status.missing > 0) {
    lines.push({ text: t('data.attachments.missing', { n: status.missing }) });
  }

  return (
    <>
      <SettingsRow
        label={t('data.attachments.label')}
        hint={
          <span className="block space-y-0.5" data-testid="attachment-status">
            {lines.map((line) => (
              <span key={line.text} className={line.tone === 'loss' ? 'block text-loss' : 'block'}>
                {line.text}
              </span>
            ))}
          </span>
        }
      />
      {status.leftovers.length > 0 && (
        <SettingsRow
          label={t('data.attachments.leftoversTitle')}
          hint={
            <>
              <span className="block">{t('data.attachments.leftoversBody')}</span>
              <span className="mt-1.5 block space-y-1.5">
                {status.leftovers.map((path) => (
                  <span key={path} className="flex items-center gap-2">
                    <code className="min-w-0 flex-1 break-all font-mono text-[11px] text-ink-3">
                      {path}
                    </code>
                    <Button variant="outline" size="sm" onClick={() => reveal(path)}>
                      {t('data.attachments.reveal')}
                    </Button>
                  </span>
                ))}
              </span>
            </>
          }
        />
      )}
    </>
  );
}
