import { useId, useState, type ReactNode } from 'react';
import { ChevronDown, Lightbulb, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import { dismissTip, isTipDismissed } from '@/utils/tips';

interface TipBannerProps {
  /** localStorage key remembering the dismissal (see `src/utils/tips.ts`). */
  storageKey: string;
  /** Key of the older banner this tip replaced; a dismissal stored there still counts. */
  legacyStorageKey?: string;
  /** The single line shown next to "Tip". */
  summary: ReactNode;
  /** Longer explanation behind a "Learn more" toggle; no toggle when omitted. */
  details?: ReactNode;
  className?: string;
}

/**
 * Quiet one-line tip that can be dismissed for good. Used for hints that are
 * useful but secondary — e.g. the AI-assistant (MCP) suggestions — so they never compete with the
 * page's primary action.
 */
export function TipBanner({
  storageKey,
  legacyStorageKey,
  summary,
  details,
  className,
}: TipBannerProps) {
  const { t } = useTranslation('common');
  const [dismissed, setDismissed] = useState(() => isTipDismissed(storageKey, legacyStorageKey));
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();

  if (dismissed) return null;

  const dismiss = () => {
    dismissTip(storageKey);
    setDismissed(true);
  };

  return (
    <div
      role="note"
      className={cn('rounded-r2 border border-line bg-well px-3 py-2 text-table', className)}
    >
      <div className="flex items-center gap-2">
        <Lightbulb className="h-4 w-4 shrink-0 text-ink-3" aria-hidden="true" />
        <p className="min-w-0 flex-1 text-ink-3">
          <span className="font-medium text-ink">{t('tip.label')}</span>
          <span aria-hidden="true"> · </span>
          {summary}
        </p>
        {details && (
          <Button
            type="button"
            variant="link"
            size="sm"
            className="h-auto min-h-0 shrink-0 gap-1 p-0 text-xs text-ink"
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setExpanded((open) => !open)}
          >
            {expanded ? t('tip.showLess') : t('tip.learnMore')}
            <ChevronDown
              className={cn('h-3 w-3 transition-transform', expanded && 'rotate-180')}
              aria-hidden="true"
            />
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="shrink-0"
          aria-label={t('tip.dismiss')}
          onClick={dismiss}
        >
          <X />
        </Button>
      </div>
      {details && expanded && (
        <div id={detailsId} className="mt-2 space-y-1 pl-6 text-xs text-ink-3">
          {details}
        </div>
      )}
    </div>
  );
}
