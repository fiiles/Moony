import { useTranslation } from 'react-i18next';
import type { ConversionQuality } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';

/**
 * A converted plan amount (decimal TEXT) in `currency`, with the app's compact missing-rate
 * marker (loss colour, the reason in the tooltip): "—" when nothing could be converted, the
 * converted part when some payments were left out. Never 0 or 1:1 for a missing rate.
 */
export function PlanMoney({
  amount,
  currency,
  quality,
  signedNegative = false,
  className,
}: {
  amount: string | null;
  currency: string;
  quality?: Pick<ConversionQuality, 'missingCurrencies'>;
  /** Write a negative value with the true minus ("− 1 200 Kč"). */
  signedNegative?: boolean;
  className?: string;
}) {
  const { t } = useTranslation('common');
  const fmt = useFormat();
  const missing = quality?.missingCurrencies ?? [];
  const title =
    missing.length > 0
      ? t('fxQuality.missing', { currency, currencies: missing.join(', ') })
      : undefined;
  if (amount === null) {
    return (
      <span className={cn(className, 'text-loss')} title={title}>
        —
      </span>
    );
  }
  const value = parseFloat(amount);
  const text = fmt.money(value, currency, { signed: signedNegative && value < 0 });
  return (
    <span className={cn(className, title && 'text-loss')} title={title}>
      {text}
    </span>
  );
}
