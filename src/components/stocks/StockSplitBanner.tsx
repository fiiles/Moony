import { useTranslation } from 'react-i18next';
import { Split } from 'lucide-react';
import type { StockSplit } from '@shared/schema';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { useStockSplitMutations } from '@/hooks/use-stock-split-mutations';
import { defaultSplitChoice, ratioText, splitActionOrder } from '@/utils/stock-splits';

interface StockSplitBannerProps {
  /** The ticker's splits; only the pending ones get a banner. */
  splits: readonly StockSplit[];
  /** Opens the splits dialog. */
  onManage: () => void;
}

/**
 * One inline alert per pending split of the position (spec D9): what Moony thinks of the
 * transactions before it and the two ways to resolve it. The verdict picks the primary button
 * (`unadjusted` → recalculate, `adjusted` → already accounted for); with `unknown` neither is
 * emphasised. Nothing when no split is pending.
 */
export function StockSplitBanner({ splits, onManage }: StockSplitBannerProps) {
  const { t } = useTranslation('stocks');
  const fmt = useFormat();
  const { apply, dismiss } = useStockSplitMutations();
  const pending = splits.filter((split) => split.status === 'pending');
  if (pending.length === 0) return null;

  const busy = apply.isPending || dismiss.isPending;

  return (
    <div className="mb-5 grid gap-2.5">
      {pending.map((split) => {
        const choice = defaultSplitChoice(split.verdict);
        const body =
          split.verdict === 'unknown'
            ? t('splits.banner.unknown')
            : t(`splits.banner.${split.verdict}`, { count: split.preSplitCount });
        const applyButton = (
          <Button
            key="apply"
            size="sm"
            variant={choice === 'apply' ? 'default' : 'outline'}
            disabled={busy}
            loading={apply.isPending && apply.variables === split.id}
            onClick={() => apply.mutate(split.id)}
          >
            {t('splits.actions.apply')}
          </Button>
        );
        const dismissButton = (
          <Button
            key="dismiss"
            size="sm"
            variant={choice === 'dismiss' ? 'default' : 'outline'}
            disabled={busy}
            loading={dismiss.isPending && dismiss.variables === split.id}
            onClick={() => dismiss.mutate(split.id)}
          >
            {t('splits.actions.dismiss')}
          </Button>
        );
        return (
          <Alert key={split.id} role="status">
            <Split />
            <AlertTitle>
              {t('splits.banner.title', {
                ratio: ratioText(split.numerator, split.denominator),
                date: fmt.day(split.effectiveDay),
              })}
            </AlertTitle>
            <AlertDescription>{body}</AlertDescription>
            <AlertActions className="mt-2.5 items-center">
              {/* The emphasised action leads; without a verdict the order stays apply, dismiss */}
              {splitActionOrder(split.verdict).map((kind) =>
                kind === 'apply' ? applyButton : dismissButton
              )}
              <Button variant="link" size="sm" className="ml-1.5" onClick={onManage}>
                {t('splits.actions.manage')}
              </Button>
            </AlertActions>
          </Alert>
        );
      })}
    </div>
  );
}
