import { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { RefreshCw } from 'lucide-react';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Progress } from '@/components/ui/progress';
import { useUpdater } from '@/hooks/use-updater';
import { releaseUrl } from '@/lib/app-links';
import { useFormat } from '@/lib/use-format';

const BYTE_UNITS = ['B', 'kB', 'MB', 'GB'];

/**
 * The one "update available" dialog: opens by itself when the automatic check
 * finds an update, and from the header badge. It is also where install
 * progress shows, whichever button started the install.
 *
 * It carries no release notes: the updater's notes are English and meant for
 * old clients only. "What's new" opens the version's GitHub release page,
 * whose body starts with that version's CHANGELOG section.
 */
export function UpdateNotification() {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const { status, update, progress, installFailed, promptOpen, install, dismiss } = useUpdater();

  // Focus the primary action on open, not the "What's new" link, which would show a focus box.
  const primaryRef = useRef<HTMLButtonElement>(null);

  const installing = status === 'installing';
  const open = promptOpen && update !== null && (status === 'available' || installing);

  const formatBytes = (bytes: number) => {
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return `${fmt.number(value, { maximumFractionDigits: unit === 0 ? 0 : 1 })} ${BYTE_UNITS[unit]}`;
  };

  const openWhatsNew = async (version: string) => {
    const url = releaseUrl(version);
    try {
      await openUrl(url);
    } catch (error) {
      console.error('Failed to open link, using fallback:', error);
      window.open(url, '_blank');
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // The install cannot be cancelled once it has started.
        if (!next && !installing) dismiss();
      }}
    >
      <DialogContent
        className="max-w-[440px]"
        hideCloseButton={installing}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          primaryRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle>
            {installFailed
              ? t('update.errorTitle')
              : t('update.dialogTitle', { version: update?.version ?? '' })}
          </DialogTitle>
          <DialogDescription>
            {installFailed
              ? t('update.installFailed')
              : t('update.currentVersion', { version: update?.currentVersion ?? '' })}
          </DialogDescription>
        </DialogHeader>

        {installing ? (
          progress && (
            <div className="space-y-2">
              <div className="flex items-center justify-between text-sm">
                <span>{t('update.downloading')}</span>
                <span>{progress.percentage}%</span>
              </div>
              <Progress value={progress.percentage} className="h-2" />
              {progress.contentLength && (
                <p className="text-xs text-ink-3">
                  {formatBytes(progress.downloaded)} / {formatBytes(progress.contentLength)}
                </p>
              )}
            </div>
          )
        ) : (
          <div className="space-y-2">
            <p className="m-0 text-table text-ink-2">{t('update.whatHappens')}</p>
            {update && (
              <p className="m-0">
                <a
                  href={releaseUrl(update.version)}
                  onClick={(event) => {
                    event.preventDefault();
                    void openWhatsNew(update.version);
                  }}
                  className="text-table font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline focus-visible:outline-none focus-visible:shadow-focus"
                >
                  {t('update.whatsNew')} ↗
                </a>
              </p>
            )}
          </div>
        )}

        <DialogFooter>
          {installing ? (
            <div className="flex w-full items-center justify-center gap-2 text-sm text-ink-3">
              <RefreshCw className="h-4 w-4 animate-spin" />
              {t('update.installing')}
            </div>
          ) : (
            <>
              <Button onClick={dismiss} variant="ghost">
                {t('update.later')}
              </Button>
              <Button ref={primaryRef} onClick={() => void install()}>
                {t('update.updateAndRestart')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
