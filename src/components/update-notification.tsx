import { useTranslation } from 'react-i18next';
import { AlertCircle, Check, Download, RefreshCw, X } from 'lucide-react';
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
import { useFormat } from '@/lib/use-format';

const BYTE_UNITS = ['B', 'kB', 'MB', 'GB'];

/**
 * The one "update available" dialog: opens by itself when the automatic check
 * finds an update, and from the header badge. It is also where install
 * progress shows, whichever button started the install.
 */
export function UpdateNotification() {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const { status, update, progress, installFailed, promptOpen, install, dismiss } = useUpdater();

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

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // The install cannot be cancelled once it has started.
        if (!next && !installing) dismiss();
      }}
    >
      <DialogContent className="sm:max-w-md" hideCloseButton={installing}>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {installFailed ? (
              <>
                <AlertCircle className="h-5 w-5 text-loss" />
                {t('update.errorTitle')}
              </>
            ) : (
              <>
                <Download className="h-5 w-5 text-ink" />
                {t('update.available')}
              </>
            )}
          </DialogTitle>
          <DialogDescription>
            {installFailed
              ? t('update.installFailed')
              : t('update.newVersion', { version: update?.version ?? '' })}
          </DialogDescription>
        </DialogHeader>

        {update?.body && !installing && (
          <div className="max-h-48 overflow-y-auto rounded-r1 border bg-well p-3">
            <h4 className="mb-2 text-sm font-semibold">{t('update.releaseNotes')}</h4>
            <div className="prose prose-sm">
              <pre className="whitespace-pre-wrap text-xs">{update.body}</pre>
            </div>
          </div>
        )}

        {installing && progress && (
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
        )}

        <DialogFooter className="flex-col gap-2 sm:flex-row">
          {installing ? (
            <div className="flex w-full items-center justify-center gap-2 text-sm text-ink-3">
              <RefreshCw className="h-4 w-4 animate-spin" />
              {t('update.installing')}
            </div>
          ) : (
            <>
              <Button onClick={dismiss} variant="outline" className="flex-1">
                <X className="mr-2 h-4 w-4" />
                {t('update.later')}
              </Button>
              <Button onClick={() => void install()} className="flex-1">
                <Check className="mr-2 h-4 w-4" />
                {t('update.updateNow')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
