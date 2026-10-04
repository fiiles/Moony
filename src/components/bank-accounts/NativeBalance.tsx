import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { useRatesUpdatedAt } from '@/hooks/use-rates-updated-at';
import { cn } from '@/lib/utils';
import { hasKnownRate, type CurrencyCode } from '@shared/currencies';

interface NativeBalanceProps {
  /** Amount in the account's own currency. */
  amount: number;
  /** The account's currency. */
  currency: string;
  /** Text alignment of the two lines. */
  align?: 'start' | 'end';
  /** Classes for the primary (native) figure. */
  className?: string;
  /** Classes for the secondary (converted) line. */
  secondaryClassName?: string;
}

/**
 * Native-first money for an entity held in its own currency: the account's
 * currency is the primary figure, the display-currency
 * equivalent a muted "≈ 30 196 Kč" with the rate in a tooltip. An amount already
 * in the display currency renders as a single plain figure; a currency without
 * a known rate says so instead of showing a made-up 1:1 conversion.
 */
export function NativeBalance({
  amount,
  currency,
  align = 'end',
  className,
  secondaryClassName,
}: NativeBalanceProps) {
  const { t } = useTranslation('bank_accounts');
  const fmt = useFormat();
  const {
    currencyCode: displayCurrency,
    formatCurrencyRaw,
    convert,
    ratesTimestamp,
  } = useCurrency();
  const updatedAt = useRatesUpdatedAt();

  if (currency === displayCurrency) {
    return <span className={className}>{formatCurrencyRaw(amount)}</span>;
  }

  const primary = fmt.money(amount, currency);
  const from = currency as CurrencyCode;
  const to = displayCurrency;
  const ratesLoaded = ratesTimestamp > 0;
  const rateKnown = hasKnownRate(from) && hasKnownRate(to);

  let secondary: string | null = null;
  let tooltip: string | null = null;
  let warn = false;
  if (ratesLoaded && rateKnown) {
    secondary = t('nativeBalance.approx', { amount: formatCurrencyRaw(convert(amount, from, to)) });
    const rate = convert(1, from, to);
    const rateText = fmt.number(rate, {
      minimumFractionDigits: 2,
      maximumFractionDigits: rate >= 1 ? 2 : 4,
    });
    tooltip = updatedAt
      ? t('nativeBalance.rateTooltip', {
          from,
          to,
          rate: rateText,
          date: fmt.dateTime(updatedAt, { hour: undefined, minute: undefined }),
        })
      : t('nativeBalance.rateTooltipNoDate', { from, to, rate: rateText });
  } else if (ratesLoaded) {
    secondary = t('nativeBalance.noRate');
    tooltip = t('nativeBalance.noRateTooltip', { currency });
    warn = true;
  }

  return (
    <span className={cn('flex flex-col', align === 'end' ? 'items-end' : 'items-start')}>
      <span className={className}>{primary}</span>
      {secondary && tooltip && (
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              tabIndex={0}
              className={cn(
                // tracking-normal: a hero parent's tight letter-spacing is inherited as
                // pixels and would squash this small line
                'cursor-help text-xs font-normal tracking-normal',
                warn ? 'text-loss' : 'text-ink-3',
                secondaryClassName
              )}
            >
              {secondary}
            </span>
          </TooltipTrigger>
          <TooltipContent>{tooltip}</TooltipContent>
        </Tooltip>
      )}
    </span>
  );
}
