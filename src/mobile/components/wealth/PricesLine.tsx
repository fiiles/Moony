import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { priceStamp } from '@/mobile/lib/position-rows';

interface PricesLineProps {
  /** The count, already worded ("3 pozice"). */
  count: string;
  /** The rows of the list: the newest `fetchedAt` is the age of its prices. */
  rows: readonly { fetchedAt: number | null }[];
}

/**
 * The caption under a class page's title: the count and the age of the prices, "3 pozice · ceny
 * 9:40" today, "ceny 12. 10." on an older day, "ceny nenačteny" for never. The nav bar keeps its
 * one title, so this is the first line of the page.
 */
export function PricesLine({ count, rows }: PricesLineProps) {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('stocks');
  const fmt = useFormat();

  // The newest price of the list: "ceny 9:40" today, "ceny 12. 10." on an older day
  const fetchedAt = rows.reduce<number | null>(
    (newest, row) =>
      row.fetchedAt !== null && (newest === null || row.fetchedAt > newest)
        ? row.fetchedAt
        : newest,
    null
  );
  const stamp = priceStamp(fetchedAt, Date.now() / 1000);
  const line = [
    count,
    stamp.kind === 'today'
      ? t('wealth.stocks.pricesAt', { time: fmt.time(stamp.at) })
      : stamp.kind === 'day'
        ? t('wealth.stocks.pricesOn', { date: fmt.day(stamp.at, shortDayOptions(stamp.at)) })
        : ts('summary.notUpdated'),
  ].join(' · ');

  return (
    <p className="mx-0.5 -mt-0.5 mb-2.5 truncate text-center text-m-caption text-ink-3">{line}</p>
  );
}
