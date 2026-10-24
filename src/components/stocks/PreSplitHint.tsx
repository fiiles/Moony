import { useTranslation } from 'react-i18next';
import { FormDescription } from '@/components/ui/form';
import { useFormat } from '@/lib/use-format';
import { useStockSplits } from '@/hooks/use-stock-splits';
import { isoDayToUnix, preSplitHint, ratioText } from '@/utils/stock-splits';

interface PreSplitHintProps {
  ticker: string | undefined;
  /** The date field's yyyy-mm-dd value. */
  date: string | undefined;
}

/**
 * The hint under a trade's date field when the day falls before an applied split of the ticker
 * (spec D20): quantity and price go in today's units. No conversion and no block; the error of
 * the field replaces it like any field hint.
 */
export function PreSplitHint({ ticker, date }: PreSplitHintProps) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const { data: splits = [] } = useStockSplits(ticker);
  const day = isoDayToUnix(date);
  const split = day === null ? null : preSplitHint(splits, day);
  if (!split) return null;
  return (
    <FormDescription>
      {t('splits.hint', {
        date: fmt.day(split.effectiveDay),
        ratio: ratioText(split.numerator, split.denominator),
      })}
    </FormDescription>
  );
}
