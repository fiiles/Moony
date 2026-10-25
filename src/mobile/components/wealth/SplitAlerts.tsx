import { useTranslation } from 'react-i18next';
import { Split } from 'lucide-react';
import { useStockSplits } from '@/hooks/use-stock-splits';
import { useStockSplitMutations } from '@/hooks/use-stock-split-mutations';
import { useFormat } from '@/lib/use-format';
import { ratioText, splitActionOrder } from '@/utils/stock-splits';
import { InlineAlert } from '@/mobile/components/InlineAlert';

/**
 * A position's pending splits (spec D9, the desktop's `StockSplitBanner`): one inline alert per
 * split under the nav bar's sub-line, with the desktop's title, its verdict sentence and its two
 * decisions as text actions, the emphasised one first (`splitActionOrder`). Přehled's split row
 * opens this page, so the decision is here. The queries and mutations are the desktop's (the
 * apply runs the mutation contract and the forced price refresh, the toast offers the undo); the
 * splits dialog ("Spravovat splity", recording a split by hand) stays on the desktop. Nothing when
 * no split is pending. `live` false (the position is being deleted) keeps the alerts as they were:
 * the recalculation the delete emits would otherwise refetch them empty under the confirm sheet.
 */
export function SplitAlerts({ ticker, live = true }: { ticker: string; live?: boolean }) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const { data: splits = [] } = useStockSplits(ticker, live);
  const { apply, dismiss } = useStockSplitMutations();
  const pending = splits.filter((split) => split.status === 'pending');
  if (pending.length === 0) return null;

  const busy = apply.isPending || dismiss.isPending;

  return (
    <>
      {pending.map((split) => {
        const actions = splitActionOrder(split.verdict).map((kind) =>
          kind === 'apply'
            ? {
                label: t('splits.actions.apply'),
                onClick: () => apply.mutate(split.id),
                disabled: busy,
              }
            : {
                label: t('splits.actions.dismiss'),
                onClick: () => dismiss.mutate(split.id),
                disabled: busy,
              }
        );
        return (
          <InlineAlert
            key={split.id}
            icon={Split}
            title={t('splits.banner.title', {
              ratio: ratioText(split.numerator, split.denominator),
              date: fmt.day(split.effectiveDay),
            })}
            action={actions}
          >
            {split.verdict === 'unknown'
              ? t('splits.banner.unknown')
              : t(`splits.banner.${split.verdict}`, { count: split.preSplitCount })}
          </InlineAlert>
        );
      })}
    </>
  );
}
