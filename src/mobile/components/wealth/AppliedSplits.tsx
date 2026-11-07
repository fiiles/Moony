import { useTranslation } from 'react-i18next';
import { Split } from 'lucide-react';
import type { StockSplit } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { ratioText } from '@/utils/stock-splits';
import { InlineAlert } from '@/mobile/components/InlineAlert';

/**
 * The splits already applied to a position (the desktop's line under the ledger header): one note
 * per split, newest first, "2:1 split on 3 Jun · 4 transactions recalculated", with the
 * "Spravovat splity" action that opens the splits dialog (undo, record a missing one). Nothing
 * without an applied split.
 */
export function AppliedSplits({
  splits,
  onManage,
}: {
  splits: readonly StockSplit[];
  onManage: () => void;
}) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const applied = splits
    .filter((split) => split.status === 'applied')
    .sort((a, b) => b.effectiveDay - a.effectiveDay);
  if (applied.length === 0) return null;

  return (
    <InlineAlert
      icon={Split}
      muted
      action={{ label: t('splits.actions.manage'), onClick: onManage }}
    >
      {applied.map((split, index) => (
        <span key={split.id}>
          {index > 0 && <br />}
          {t('splits.appliedLine', {
            ratio: ratioText(split.numerator, split.denominator),
            date: fmt.day(split.effectiveDay),
            count: split.appliedCount ?? split.preSplitCount,
          })}
        </span>
      ))}
    </InlineAlert>
  );
}
