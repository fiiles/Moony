import { useTranslation } from 'react-i18next';
import { CircleHelp } from 'lucide-react';
import { unpricedOpenCoins, type CryptoRow } from '@/utils/positions';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';

interface UnpricedCoinsBannerProps<T extends CryptoRow> {
  rows: T[];
  /** Open the picker for this coin (the first one listed; the rest go through the row menu). */
  onPick: (row: T) => void;
}

/**
 * Banner above the trend card naming the holdings Moony could not match to a Yahoo Finance
 * listing (`yahooSymbol` null; a sold-out coin needs none). It has no dismiss state: it goes when
 * every holding has a symbol.
 */
export function UnpricedCoinsBanner<T extends CryptoRow>({
  rows,
  onPick,
}: UnpricedCoinsBannerProps<T>) {
  const { t } = useTranslation('crypto');
  const unpriced = unpricedOpenCoins(rows);
  if (unpriced.length === 0) return null;

  return (
    <Alert role="status" className="mb-[17px]">
      <CircleHelp />
      <AlertTitle>{t('unpricedBanner.title', { count: unpriced.length })}</AlertTitle>
      <AlertDescription>
        {t('unpricedBanner.message', { tickers: unpriced.map((row) => row.ticker).join(', ') })}
      </AlertDescription>
      <AlertActions>
        <Button variant="outline" size="sm" onClick={() => onPick(unpriced[0])}>
          {t('unpricedBanner.pick')}
        </Button>
      </AlertActions>
    </Alert>
  );
}
