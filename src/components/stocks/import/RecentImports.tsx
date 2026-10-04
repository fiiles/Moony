import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import type { SavedStockImportFormat, StockImportBatch } from '@shared/schema';
import { undoableBatches } from './import-config';
import { useSourceLabel } from './use-source-label';

/** Imports listed before "Zobrazit všechny". */
const COLLAPSED_COUNT = 3;

interface RecentImportsProps {
  batches: readonly StockImportBatch[];
  formats: readonly SavedStockImportFormat[];
  onUndo: (batch: StockImportBatch) => void;
}

/**
 * "Poslední importy" under the file picker: when, which file, from which
 * source, how many trades — and "Vrátit". An import whose trades were all
 * deleted since is not listed: there is nothing left to undo.
 */
export function RecentImports({ batches, formats, onUndo }: RecentImportsProps) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const sourceLabel = useSourceLabel(formats);
  const [showAll, setShowAll] = useState(false);

  const undoable = undoableBatches(batches);
  if (undoable.length === 0) return null;
  const shown = showAll ? undoable : undoable.slice(0, COLLAPSED_COUNT);

  return (
    <div className="mt-6">
      <h4 className="mb-1 mt-0 text-caption font-700 text-ink-2">
        {t('importWizard.recent.title')}
      </h4>
      <ul className="m-0 list-none p-0">
        {shown.map((batch) => (
          <li
            key={batch.id}
            className="flex items-center gap-3 border-b border-line-soft py-2.5 text-table last:border-0"
          >
            <span className="min-w-0 flex-1">
              <b className="block truncate font-600 text-ink" title={batch.fileName}>
                {batch.fileName}
              </b>
              <small className="mt-px block text-micro text-ink-4">
                {fmt.dateTime(batch.createdAt)} · {sourceLabel(batch.source)} ·{' '}
                {batch.remainingCount < batch.tradeCount
                  ? t('importWizard.recent.tradesOf', {
                      remaining: batch.remainingCount,
                      total: batch.tradeCount,
                    })
                  : t('importWizard.recent.trades', { count: batch.tradeCount })}
              </small>
            </span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => onUndo(batch)}
              aria-label={t('importWizard.recent.undoFor', { file: batch.fileName })}
            >
              {t('importWizard.recent.undo')}
            </Button>
          </li>
        ))}
      </ul>
      {undoable.length > COLLAPSED_COUNT && (
        <button
          type="button"
          className="mt-1.5 text-micro font-600 text-ink-3 underline underline-offset-[3px] hover:text-ink"
          onClick={() => setShowAll((value) => !value)}
        >
          {showAll
            ? t('importWizard.recent.showLess')
            : t('importWizard.recent.showAll', { count: undoable.length })}
        </button>
      )}
    </div>
  );
}
