import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Check, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { portfolioApi, priceApi } from '@/lib/tauri-api';
import { useCurrency } from '@/lib/currency';
import { useSyncStatus } from '@/hooks/sync-context';
import { hasKnownRate } from '@shared/currencies';
import { cn } from '@/lib/utils';
import type { ShellStatus, ShellStatusTone } from '@/components/shell/shell-context';

const toneClass: Record<ShellStatusTone, string> = {
  neutral: 'text-ink-3 before:bg-ink-5',
  fresh: 'text-gain before:bg-gain',
  stale: 'text-loss before:bg-loss',
};

/** `.status`: a 6 px dot and 11/600 text. */
export function StatusText({
  tone = 'neutral',
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & { tone?: ShellStatusTone }) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-[7px] text-caption font-600 before:block before:size-1.5 before:shrink-0 before:rounded-full before:content-[""]',
        toneClass[tone],
        className
      )}
      {...props}
    >
      {children}
    </span>
  );
}

/**
 * Data status in the top bar (design system §5): green when prices are
 * fresh, red-brown when anything is stale, neutral while historical prices
 * load. Clicking opens the detail (stock, crypto and FX age) with a refresh
 * action. A page can replace the text through `useShellPage`.
 */
export function DataStatus({ override }: { override?: ShellStatus }) {
  const { t } = useTranslation('common');
  const queryClient = useQueryClient();
  const [isRefreshing, setIsRefreshing] = useState(false);
  const { isSyncing } = useSyncStatus();

  const { data: priceStatus, refetch } = useQuery({
    queryKey: ['price-status'],
    queryFn: () => portfolioApi.getPriceStatus(),
    refetchInterval: 60000,
    staleTime: 30000,
  });

  // Currencies with no known exchange rate (internally counted 1:1 against
  // CZK, the base currency, so totals that include them are inaccurate). The
  // backend reports the ones it met in a conversion; the display
  // currency is converted on the frontend, so a non-ECB display currency is
  // added here once the rates have loaded.
  const { currencyCode: displayCurrency, ratesTimestamp } = useCurrency();
  const reportedMissing = priceStatus?.missingCurrencies ?? [];
  const displayMissing =
    ratesTimestamp > 0 &&
    !hasKnownRate(displayCurrency) &&
    !reportedMissing.includes(displayCurrency);
  const missingCurrencies = displayMissing
    ? [...reportedMissing, displayCurrency]
    : reportedMissing;

  const isStale =
    !!priceStatus &&
    (priceStatus.stocksStale ||
      priceStatus.cryptoStale ||
      priceStatus.exchangeRatesStale ||
      missingCurrencies.length > 0);

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await Promise.allSettled([
        portfolioApi.refreshExchangeRates(),
        priceApi.refreshStockPrices(),
        priceApi.refreshCryptoPrices(),
      ]);
      await refetch();
      queryClient.invalidateQueries({ queryKey: ['portfolio-metrics'] });
      queryClient.invalidateQueries({ queryKey: ['investments'] });
      queryClient.invalidateQueries({ queryKey: ['crypto'] });
    } catch (error) {
      console.error('Failed to refresh prices:', error);
    } finally {
      setIsRefreshing(false);
    }
  };

  const formatAge = (hours: number | null): string => {
    if (hours === null) return t('priceStatus.never');
    if (hours < 1) return t('priceStatus.lessThanHour');
    if (hours < 24) return t('priceStatus.hoursAgo', { count: hours });
    return t('priceStatus.daysAgo', { count: Math.floor(hours / 24) });
  };

  let tone: ShellStatusTone;
  let text: string;
  if (override) {
    tone = override.tone ?? 'neutral';
    text = override.text;
  } else if (isSyncing) {
    tone = 'neutral';
    text = t('sync.loadingHistorical');
  } else if (isStale) {
    tone = 'stale';
    text = t('priceStatus.stale');
  } else {
    tone = 'fresh';
    text = t('priceStatus.fresh');
  }

  const rows: { label: string; stale: boolean; text: string }[] = priceStatus
    ? [
        {
          label: t('priceStatus.stocks'),
          stale: priceStatus.stocksStale,
          text:
            (priceStatus.stocksMissingPrice > 0
              ? `${priceStatus.stocksMissingPrice} ${t('priceStatus.missing')} · `
              : '') + formatAge(priceStatus.oldestStockPriceAgeHours),
        },
        {
          label: t('priceStatus.crypto'),
          stale: priceStatus.cryptoStale,
          text:
            (priceStatus.cryptoMissingPrice > 0
              ? `${priceStatus.cryptoMissingPrice} ${t('priceStatus.missing')} · `
              : '') + formatAge(priceStatus.oldestCryptoPriceAgeHours),
        },
        {
          label: t('priceStatus.exchangeRates'),
          stale: priceStatus.exchangeRatesStale,
          text: formatAge(priceStatus.exchangeRatesAgeHours),
        },
      ]
    : [];

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="rounded-r1 px-1 py-0.5 focus-visible:outline-none focus-visible:shadow-focus"
          data-testid="data-status"
          aria-label={t('priceStatus.title')}
        >
          <StatusText tone={tone}>{text}</StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-80" align="end">
        <div className="space-y-4">
          <div>
            <h4 className="text-h3 text-ink">{t('priceStatus.title')}</h4>
            <p className="mt-1 text-caption text-ink-3">{t('priceStatus.description')}</p>
          </div>

          <div className="divide-y divide-line-soft text-table">
            {rows.map((row) => (
              <div key={row.label} className="flex items-center justify-between gap-4 py-2">
                <span className="text-ink-3">{row.label}</span>
                {row.stale ? (
                  <span className="font-600 text-loss">{row.text}</span>
                ) : (
                  <span className="inline-flex items-center gap-1 font-600 text-gain">
                    <Check className="size-3.5" strokeWidth={2.25} />
                    {row.text}
                  </span>
                )}
              </div>
            ))}
            {missingCurrencies.length > 0 && (
              <p className="py-2 text-caption text-loss">
                {t('priceStatus.unknownRates', { currencies: missingCurrencies.join(', ') })}
              </p>
            )}
          </div>

          <Button
            variant="outline"
            size="sm"
            className="w-full"
            onClick={handleRefresh}
            loading={isRefreshing}
            disabled={isRefreshing}
          >
            <RefreshCw />
            {isRefreshing ? t('priceStatus.refreshing') : t('priceStatus.refresh')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
