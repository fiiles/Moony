import { useTranslation } from 'react-i18next';
import { Segmented } from '@/components/ui/segmented';
import { cn } from '@/lib/utils';
import type { StockTypeValueAction } from '@shared/schema';
import type { TypeValueRow } from './import-config';

interface TypeValuesTableProps {
  /** One line per value of the type column (see `typeValueRows`). */
  rows: readonly TypeValueRow[];
  /** Some value is read as a buy or a sell; without one nothing would be imported. */
  hasTradeValue: boolean;
  /** Change what a mapping entry means (`TypeValueRow.entryValue`). */
  onChange: (entryValue: string, action: StockTypeValueAction) => void;
  /** XTB: the text of the comment decides buy or sell, the type only tells trades apart. */
  commentDecidesDirection?: boolean;
}

const ACTIONS: readonly StockTypeValueAction[] = ['buy', 'sell', 'skip'];

/**
 * What each value of the type column means: a buy, a sell, or not a trade
 * (deposits, dividends, fees … are skipped). Suggested from keywords in several
 * languages; where the file was counted, the count and the first line help to
 * judge a value like "Other".
 */
export function TypeValuesTable({
  rows,
  hasTradeValue,
  onChange,
  commentDecidesDirection = false,
}: TypeValuesTableProps) {
  const { t } = useTranslation('stocks');

  if (rows.length === 0) {
    return (
      <p className="m-0 rounded-r2 bg-well px-3 py-2.5 text-micro font-500 text-ink-3">
        {t('importWizard.mapping.typeTooMany')}
      </p>
    );
  }

  const counted = rows.some((row) => row.count != null);
  const options = ACTIONS.map((action) => ({
    value: action,
    label: t(`importWizard.mapping.action.${action}`),
  }));

  return (
    <div>
      <div className="overflow-hidden rounded-r2 border border-line">
        <div className="max-h-[248px] overflow-y-auto">
          <table className="w-full border-collapse text-table text-ink-2">
            <thead className="sticky top-0 bg-well">
              <tr className="border-b border-line text-left text-thead uppercase text-ink-3">
                <th className="h-8 px-3">{t('importWizard.mapping.typeValue')}</th>
                {counted && (
                  <>
                    <th className="h-8 px-2 text-right">{t('importWizard.mapping.typeCount')}</th>
                    <th className="h-8 px-2 text-right">{t('importWizard.mapping.typeLine')}</th>
                  </>
                )}
                <th className="h-8 px-3">{t('importWizard.mapping.typeMeaning')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.value} className="border-b border-line-soft last:border-0">
                  <td className="max-w-[170px] px-3 py-1.5">
                    <span
                      className={cn(
                        'block truncate font-600',
                        row.action === 'skip' ? 'text-ink-4' : 'text-ink'
                      )}
                      title={row.value}
                    >
                      {row.value}
                    </span>
                  </td>
                  {counted && (
                    <>
                      <td className="px-2 py-1.5 text-right text-caption num">
                        {row.count ?? '—'}
                      </td>
                      <td className="px-2 py-1.5 text-right text-caption text-ink-4 num">
                        {row.firstLine ?? '—'}
                      </td>
                    </>
                  )}
                  <td className="px-3 py-1.5">
                    <Segmented
                      value={row.action}
                      onValueChange={(next) => onChange(row.entryValue, next)}
                      options={options}
                      aria-label={row.value}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      {commentDecidesDirection && (
        <p className="mb-0 mt-2 text-micro font-500 text-ink-4">
          {t('importWizard.mapping.typeCommentNote')}
        </p>
      )}
      {!hasTradeValue && (
        <p className="mb-0 mt-2 text-micro font-600 text-loss">
          {t('importWizard.mapping.typeNoTrade')}
        </p>
      )}
    </div>
  );
}
