import { useState } from 'react';
import { invalidateAfterPriceRefresh } from '@/lib/price-refresh';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Check, RefreshCw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { SyncRemovedSection, SyncStatusSection } from '@/components/shell/SyncStatusSection';
import { portfolioApi, priceApi } from '@/lib/tauri-api';
import { useSyncStatus } from '@/hooks/sync-context';
import { useDeviceSyncFeature, useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useHistoryRecalculation } from '@/hooks/use-history-recalculation';
import { FX_HISTORY_KEY, FX_SNAPSHOT_KEY } from '@/hooks/use-fx';
import { cn } from '@/lib/utils';
import { dataStatus } from '@/utils/data-status';
import { syncState } from '@/utils/sync-state';
import type { ShellStatus, ShellStatusTone } from '@/components/shell/shell-context';

/** Age after which a held currency's newest stored rate counts as stale (mirrors the backend). */
const STALE_RATE_DAYS = 14;

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
 * "Stav dat" (design system §5): the one top-bar indicator for prices, sync and history, at the
 * right edge. Its label follows `dataStatus()`: a problem is named (or counted) on every page, a
 * page's own status (`useShellPage`) shows when nothing is wrong, work in progress is neutral and
 * the calm state is a green "Data aktuální". The popover has a section per source, each with its
 * own action under it.
 */
export function DataStatus({ override }: { override?: ShellStatus }) {
  const { t } = useTranslation('common');
  const { t: tSync } = useTranslation('deviceSync');
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const { isSyncing: loadingHistory } = useSyncStatus();
  const recalculating = useHistoryRecalculation().length > 0;
  const syncFeature = useDeviceSyncFeature();
  const { data: deviceSync } = useDeviceSyncStatus();
  const now = Math.floor(Date.now() / 1000);
  const syncView = syncFeature ? syncState(deviceSync, now) : null;

  const { data: priceStatus, refetch } = useQuery({
    queryKey: ['price-status'],
    queryFn: () => portfolioApi.getPriceStatus(),
    refetchInterval: 60000,
    staleTime: 30000,
  });

  // Currencies in the user's data (the profile currency included) without a
  // current quote: their amounts are excluded from converted totals, never
  // valued 1:1.
  const missingCurrencies = priceStatus?.missingCurrencies ?? [];

  // Held currencies whose newest stored rate is more than two weeks old.
  const staleCurrencies = priceStatus?.staleCurrencies ?? [];

  const pricesStale =
    !!priceStatus &&
    (priceStatus.stocksStale ||
      priceStatus.cryptoStale ||
      priceStatus.exchangeRatesStale ||
      missingCurrencies.length > 0 ||
      staleCurrencies.length > 0);

  const { tone, label } = dataStatus({
    pricesStale,
    sync: syncView,
    page: override,
    loadingHistory,
    recalculating,
  });

  let text: string;
  switch (label.kind) {
    case 'problems':
      text = t('dataStatus.problems', { count: label.count });
      break;
    case 'pricesStale':
      text = t('priceStatus.stale');
      break;
    case 'sync':
      text = tSync(label.sync.labelKey, { count: label.sync.days });
      break;
    case 'page':
      text = label.text;
      break;
    case 'loadingHistory':
      text = t('sync.loadingHistorical');
      break;
    case 'recalculating':
      text = t('sync.recalculatingHistory');
      break;
    case 'fresh':
      text = t('priceStatus.fresh');
      break;
  }

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await Promise.allSettled([
        portfolioApi.refreshExchangeRates().then(() => {
          queryClient.invalidateQueries({ queryKey: FX_SNAPSHOT_KEY });
          queryClient.invalidateQueries({ queryKey: FX_HISTORY_KEY });
        }),
        priceApi.refreshStockPrices(),
        priceApi.refreshCryptoPrices(true),
      ]);
      await refetch();
      void invalidateAfterPriceRefresh(queryClient);
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
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="min-w-0 rounded-r1 px-1 py-0.5 focus-visible:outline-none focus-visible:shadow-focus"
          data-testid="data-status"
          aria-label={t('dataStatus.title')}
        >
          <StatusText tone={tone}>
            <span className="truncate">{text}</span>
          </StatusText>
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-[340px]" align="end">
        <h4 className="text-h3 text-ink">{t('dataStatus.title')}</h4>

        <section className="mt-3">
          <div className="text-eyebrow uppercase text-ink-4">{t('dataStatus.prices')}</div>
          {pricesStale && (
            <p className="mt-1 text-caption text-ink-3">{t('priceStatus.description')}</p>
          )}
          <div className="mt-1 divide-y divide-line-soft text-table">
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
            {staleCurrencies.length > 0 && (
              <p className="py-2 text-caption text-loss">
                {t('priceStatus.staleRates', {
                  currencies: staleCurrencies.join(', '),
                  days: STALE_RATE_DAYS,
                })}
              </p>
            )}
            {loadingHistory && (
              <p className="py-2 text-caption text-ink-3">{t('sync.loadingHistorical')}</p>
            )}
            {recalculating && (
              <p className="py-2 text-caption text-ink-3">{t('sync.recalculatingHistory')}</p>
            )}
          </div>
          <div className="mt-2.5">
            <Button
              variant="outline"
              size="sm"
              onClick={handleRefresh}
              loading={isRefreshing}
              disabled={isRefreshing}
            >
              <RefreshCw />
              {isRefreshing ? t('priceStatus.refreshing') : t('priceStatus.refresh')}
            </Button>
          </div>
        </section>

        {syncView && deviceSync && (
          <div className="mt-4 border-t border-line-soft pt-3.5">
            <SyncStatusSection
              status={deviceSync}
              indicator={syncView}
              now={now}
              onNavigate={() => setOpen(false)}
            />
          </div>
        )}
        {syncFeature && !syncView && deviceSync?.removedNotice && (
          <div className="mt-4 border-t border-line-soft pt-3.5">
            <SyncRemovedSection status={deviceSync} onNavigate={() => setOpen(false)} />
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
