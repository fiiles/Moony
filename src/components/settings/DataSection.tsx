import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { CheckCircle2, CircleAlert } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { AttachmentsStatusRow } from '@/components/settings/AttachmentsStatusRow';
import { RestoreBackup } from '@/components/common/RestoreBackup';
import { queryClient } from '@/lib/queryClient';
import { dataApi } from '@/lib/tauri-api';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { formatBytes } from '@/utils/format-bytes';
import { todayIsoUtc } from '@/utils/period';
import { cn } from '@/lib/utils';
import type { IntegrityReport } from '@shared/schema';

type Busy = 'backup' | 'restore' | 'export' | 'verify' | null;

/**
 * Settings → "Data a zálohy" (prototype `settings.html` rows): where the data
 * lives, backup / restore, full JSON export and an integrity check
 */
export function DataSection() {
  const { t } = useTranslation('settings');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();

  const [busy, setBusy] = useState<Busy>(null);
  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null);

  const { data: location, refetch: refetchLocation } = useQuery({
    queryKey: ['data-location'],
    queryFn: () => dataApi.getDataLocation(),
  });

  const showError = (error: unknown) => {
    const message =
      error instanceof Error ? translateApiError(error, tc) : translateApiError(String(error), tc);
    toast.error(tc('status.error'), { description: message });
  };

  const handleOpenFolder = async () => {
    try {
      await dataApi.openDataFolder();
    } catch (error) {
      showError(error);
    }
  };

  const handleBackup = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({
        defaultPath: `moony-backup-${todayIsoUtc()}.zip`,
        filters: [{ name: 'Moony backup', extensions: ['zip'] }],
      });
      if (!path) return;
      setBusy('backup');
      const manifest = await dataApi.createBackup(path);
      void queryClient.invalidateQueries({ queryKey: ['milestones'] });
      toast(t('data.backupSaved'), {
        description: t('data.backupSavedDescription', { count: manifest.files.length, path }),
      });
      void refetchLocation();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  const handleExport = async () => {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({
        defaultPath: `moony-export-${todayIsoUtc()}.json`,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!path) return;
      setBusy('export');
      const summary = await dataApi.exportAllData(path);
      toast(t('data.exportSaved'), {
        description: t('data.exportSavedDescription', {
          tables: summary.tableCount,
          rows: summary.rowCount,
          path: summary.path,
        }),
      });
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  const handleVerify = async () => {
    try {
      setBusy('verify');
      const report = await dataApi.verifyDatabase();
      setIntegrity(report);
      if (report.ok) {
        toast(t('data.verifyOk'));
      } else {
        toast.error(t('data.verifyFailed'), { description: report.messages.join('\n') });
      }
    } catch (error) {
      showError(error);
    } finally {
      setBusy(null);
    }
  };

  return (
    <SettingsCard title={t('data.title')} description={t('data.description')}>
      <SettingsRow
        label={t('data.location')}
        hint={
          <>
            <code
              className="break-all font-mono text-[11px] text-ink-3"
              data-testid="data-folder-path"
            >
              {location?.dataDir ?? '…'}
            </code>
            {location && (
              <span className="mt-0.5 block">
                {t('data.size', { size: formatBytes(location.totalBytes, fmt.locale) })}
              </span>
            )}
          </>
        }
      >
        <Button variant="outline" size="sm" onClick={handleOpenFolder}>
          {t('data.openFolder')}
        </Button>
      </SettingsRow>

      <AttachmentsStatusRow />

      <SettingsRow label={t('data.backupRow')} hint={t('data.hint')}>
        <RestoreBackup onBusyChange={(b) => setBusy(b ? 'restore' : null)}>
          {(pick) => (
            <Button
              variant="outline"
              size="sm"
              onClick={pick}
              loading={busy === 'restore'}
              disabled={busy !== null}
            >
              {t('data.restore')}
            </Button>
          )}
        </RestoreBackup>
        <Button
          size="sm"
          onClick={handleBackup}
          loading={busy === 'backup'}
          disabled={busy !== null}
          data-testid="button-backup-now"
        >
          {t('data.backupNow')}
        </Button>
      </SettingsRow>

      <SettingsRow label={t('data.exportRow')} hint={t('data.exportHint')}>
        <Button
          variant="outline"
          size="sm"
          onClick={handleExport}
          loading={busy === 'export'}
          disabled={busy !== null}
        >
          {t('data.exportButton')}
        </Button>
      </SettingsRow>

      <SettingsRow
        label={t('data.verifyRow')}
        hint={
          integrity ? (
            <span
              className={cn(
                'inline-flex items-start gap-1.5 [&_svg]:mt-px [&_svg]:size-3.5 [&_svg]:shrink-0',
                integrity.ok ? 'text-gain' : 'text-loss'
              )}
              data-testid="integrity-result"
            >
              {integrity.ok ? <CheckCircle2 /> : <CircleAlert />}
              <span>
                {integrity.ok ? t('data.verifyOk') : t('data.verifyFailed')}
                {!integrity.ok && (
                  <ul className="mt-1 list-disc pl-4">
                    {integrity.messages.map((m, i) => (
                      <li key={i}>{m}</li>
                    ))}
                  </ul>
                )}
              </span>
            </span>
          ) : (
            t('data.verifyHint')
          )
        }
      >
        <Button
          variant="outline"
          size="sm"
          onClick={handleVerify}
          loading={busy === 'verify'}
          disabled={busy !== null}
        >
          {t('data.verify')}
        </Button>
      </SettingsRow>
    </SettingsCard>
  );
}
