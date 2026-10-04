import { useTranslation } from 'react-i18next';
import { AlertCircle, CheckCircle2, Download, Loader2 } from 'lucide-react';
import { useUpdater } from '@/hooks/use-updater';
import { cn } from '@/lib/utils';

/** One-line update status (icon + sentence) shared by the About modal and Settings → Updates. */
export function UpdateStatusLine({ className }: { className?: string }) {
  const { t } = useTranslation('common');
  const { status, update } = useUpdater();

  const base = cn('inline-flex items-center gap-1.5 [&_svg]:size-3.5 [&_svg]:shrink-0', className);

  switch (status) {
    case 'checking':
      return (
        <span className={cn(base, 'text-ink-4')} role="status">
          <Loader2 className="animate-spin" />
          <span>{t('update.checking')}</span>
        </span>
      );
    case 'installing':
      return (
        <span className={cn(base, 'text-ink-4')} role="status">
          <Loader2 className="animate-spin" />
          <span>{t('update.installing')}</span>
        </span>
      );
    case 'upToDate':
      return (
        <span className={cn(base, 'font-600 text-gain')} role="status">
          <CheckCircle2 />
          <span>{t('update.upToDate')}</span>
        </span>
      );
    case 'available':
      return (
        <span className={cn(base, 'font-650 text-ink')} role="status">
          <Download />
          <span>{t('update.newVersion', { version: update?.version ?? '' })}</span>
        </span>
      );
    case 'checkFailed':
      return (
        <span className={cn(base, 'text-ink-4')} role="status">
          <AlertCircle />
          <span>{t('update.checkFailed')}</span>
        </span>
      );
    default:
      return (
        <span className={cn(base, 'text-ink-4')} role="status">
          <span>{t('update.notChecked')}</span>
        </span>
      );
  }
}
