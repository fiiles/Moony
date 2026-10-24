import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TargetDirection } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useStockMonitorMutations } from '@/hooks/use-stock-monitor-mutations';
import { useFormat } from '@/lib/use-format';
import { targetReached } from '@/utils/stock-monitor';
import { AmountField } from '@/mobile/components/AmountField';
import { FOOTER_GHOST, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { Segment } from '@/mobile/components/Segment';
import { Sheet } from '@/mobile/components/Sheet';
import { limitAmountText } from '@/mobile/lib/budget-limit';
import {
  priceOf,
  watchPriceText,
  watchTargetDirection,
  watchTargetForm,
} from '@/mobile/lib/watch-detail';

interface WatchTargetSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  ticker: string;
  /** The stock's own currency: the unit of the field and of every price the sheet writes. */
  currency: string | null;
  /** Today's price; `null` when there is none (the direction then cannot be inferred). */
  currentPrice: number | null;
  /** The 52-week high, a hint for how far a target may reach. */
  high52: number | null;
  /** The target the stock has now, decimal text; `null` when there is none. */
  targetPrice: string | null;
  targetDirection: TargetDirection | null;
}

/**
 * "Cílová cena" (spec §8.8, the desktop's `TargetPriceDialog` as a sheet): one field with the
 * stock's currency inside it and today's price and the 52-week high as the hint, the direction —
 * Nad · Pod, chosen for the person from the typed price until they pick one — with a sentence on
 * what Moony will remind them of, and Uložit. When a target exists, "Smazat cíl" removes it. The
 * prices are Yahoo's public numbers and stay visible while amounts are hidden; the target is a
 * price of that stock, not an amount of the person's. A save needs a positive number; the error
 * stands under the field once a save has been tried. Every opening starts over from the stored
 * target.
 */
export function WatchTargetSheet({
  open,
  onOpenChange,
  ticker,
  currency,
  currentPrice,
  high52,
  targetPrice,
  targetDirection,
}: WatchTargetSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tm } = useTranslation('stockMonitor');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const { targetPriceMutation } = useStockMonitorMutations();
  const formId = useId();

  const [draft, setDraft] = useState(() => (targetPrice ? limitAmountText(targetPrice) : ''));
  const [chosen, setChosen] = useState<TargetDirection | null>(null);
  const [attempted, setAttempted] = useState(false);

  // Every opening starts over
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) {
      setDraft(targetPrice ? limitAmountText(targetPrice) : '');
      setChosen(null);
      setAttempted(false);
    }
  }

  const form = watchTargetForm(draft);
  const current = currentPrice !== null && currentPrice > 0 ? currentPrice : null;
  const storedPrice = priceOf(targetPrice);
  const direction = watchTargetDirection({
    chosen,
    draft: form.price,
    current,
    stored:
      storedPrice !== null && targetDirection !== null
        ? { price: storedPrice, direction: targetDirection }
        : null,
  });
  const hasTarget = storedPrice !== null;
  const price = (value: number) => watchPriceText(fmt, value, currency);

  const explain =
    form.price === null
      ? null
      : current === null
        ? tm(direction === 'below' ? 'target.explainBelowNoPrice' : 'target.explainAboveNoPrice', {
            price: price(form.price),
          })
        : targetReached(current, form.price, direction)
          ? tm('target.explainReached')
          : tm(direction === 'below' ? 'target.explainBelow' : 'target.explainAbove', {
              price: price(form.price),
              pct: fmt.percent(form.price / current - 1, 1, { signed: true }),
            });
  const hint = [
    current !== null ? tm('target.hintCurrent', { price: price(current) }) : null,
    high52 !== null ? tm('target.hintHigh', { price: price(high52) }) : null,
  ].filter(Boolean);

  const errorText =
    attempted && form.error !== undefined
      ? {
          required: tc('validation.priceRequired'),
          invalid: tc('validation.invalidPrice'),
          positive: tc('validation.pricePositive'),
        }[form.error]
      : undefined;

  const close = () => onOpenChange(false);
  const submit = () => {
    // A second Enter while the save runs must not save twice (`loading` only blocks taps)
    if (targetPriceMutation.isPending) return;
    setAttempted(true);
    if (form.text === null) return;
    targetPriceMutation.mutate(
      { ticker, targetPrice: form.text, targetDirection: direction },
      { onSuccess: close }
    );
  };
  const remove = () =>
    targetPriceMutation.mutate(
      { ticker, targetPrice: null, targetDirection: null },
      { onSuccess: close }
    );

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={tm('target.title')}
      description={tm('target.description')}
      footer={
        <>
          <Button type="button" variant="ghost" className={FOOTER_GHOST} onClick={close}>
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={formId}
            className={FOOTER_PRIMARY}
            loading={targetPriceMutation.isPending}
          >
            {tm('target.save')}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <AmountField
          label={tm('target.field')}
          aria-label={tm('target.field')}
          unit={currency ?? ''}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="0"
          hint={hint.length > 0 ? `${hint.join(' · ')}.` : undefined}
          error={errorText}
        />
        <span className="mx-0.5 mb-1.5 mt-4 block text-m-sub font-semibold text-ink-2">
          {tm('target.directionLabel')}
        </span>
        <Segment<TargetDirection>
          value={direction}
          options={[
            { value: 'above', label: t('wealth.watch.above') },
            { value: 'below', label: t('wealth.watch.below') },
          ]}
          onChange={setChosen}
          label={tm('target.directionLabel')}
        />
        {explain && <p className="mx-0.5 mb-0 mt-2 text-m-caption text-ink-3">{explain}</p>}
        {hasTarget && (
          <Button
            type="button"
            variant="ghost"
            className="mt-3 h-11 w-full rounded-[10px] text-m-row text-loss"
            disabled={targetPriceMutation.isPending}
            onClick={remove}
          >
            {tm('target.clear')}
          </Button>
        )}
      </form>
    </Sheet>
  );
}
