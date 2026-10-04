import { useTranslation } from 'react-i18next';
import { Download, Loader2 } from 'lucide-react';
import { useUpdater } from '@/hooks/use-updater';

/**
 * Top-bar note for an available update. It only opens the shared dialog
 * (UpdateNotification); there is no second dismiss or install flow here.
 */
export function UpdateStatus() {
  const { t } = useTranslation('common');
  const { status, update, showPrompt } = useUpdater();

  if (status === 'installing') {
    return (
      <span className="inline-flex items-center gap-1.5 text-caption font-600 text-ink-3">
        <Loader2 className="size-3.5 animate-spin" />
        {t('update.installing')}
      </span>
    );
  }

  if (status === 'available' && update) {
    return (
      <button
        type="button"
        onClick={showPrompt}
        className="inline-flex items-center gap-1.5 rounded-r1 px-1 py-0.5 text-caption font-600 text-ink-2 hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
      >
        <Download className="size-3.5" />
        {t('update.available')}
        <span className="rounded-r1 bg-dark px-1.5 text-micro font-700 text-ink-inverse num">
          {update.version}
        </span>
      </button>
    );
  }

  return null;
}
