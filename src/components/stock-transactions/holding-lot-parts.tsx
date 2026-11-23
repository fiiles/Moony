import { useTranslation } from 'react-i18next';
import type { StockReportOpenLot } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';

const DAY_SECONDS = 86_400;

/**
 * The date chip of an open lot (the milestone agenda's chip, spec D12): the day its sale passes
 * the holding test over the distance from today. Dark material only for a day at most 7 days
 * ahead; `compact` writes day and month, the dialog the full day.
 */
export function PassChip({
  passDay,
  today,
  compact = false,
  className,
}: {
  passDay: number;
  today: number;
  compact?: boolean;
  className?: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const days = Math.round((passDay - today) / DAY_SECONDS);
  const urgent = days >= 0 && days <= 7;
  // The compact chip drops the year only for a day in the current (UTC) year
  const otherYear =
    new Date(passDay * 1000).getUTCFullYear() !== new Date(today * 1000).getUTCFullYear();

  let relative: string;
  if (days === 0) relative = t('upcoming.relative.today');
  else if (days === 1) relative = t('upcoming.relative.tomorrow');
  else if (days > 0) {
    relative =
      days <= 365
        ? t('upcoming.relative.inDays', { count: days })
        : t('upcoming.relative.inMonths', { count: Math.round(days / 30.44) });
  } else {
    const ago = -days;
    relative =
      ago <= 365
        ? t('upcoming.relative.daysAgo', { count: ago })
        : t('upcoming.relative.monthsAgo', { count: Math.round(ago / 30.44) });
  }

  return (
    <span
      className={cn(
        'grid h-[42px] min-w-[60px] place-content-center rounded-r2 px-1 text-center leading-tight',
        urgent ? 'bg-dark-grad text-ink-inverse shadow-dark' : 'bg-well text-ink',
        className
      )}
    >
      <b className="block whitespace-nowrap text-table font-650 num">
        {compact
          ? fmt.day(passDay, {
              day: 'numeric',
              month: 'numeric',
              ...(otherYear ? { year: 'numeric' } : {}),
            })
          : fmt.day(passDay)}
      </b>
      <small
        className={cn(
          'block whitespace-nowrap text-micro font-500',
          urgent ? 'text-ink-inverse-2' : 'text-ink-4'
        )}
      >
        {relative}
      </small>
    </span>
  );
}

/** The unrealized result of a lot with its caption under it; "—" when the lot has no value. */
export function UnrealizedCell({ lot, currency }: { lot: StockReportOpenLot; currency: string }) {
  const { t } = useTranslation('stockTransactions');
  const { signedMoney } = useReportMoney(currency);
  if (lot.unrealized === null) return <span className="text-ink-4">—</span>;
  const value = Number(lot.unrealized);
  return (
    <>
      <span
        className={cn(
          'block whitespace-nowrap text-table font-650 num',
          value > 0 && 'text-gain',
          value < 0 && 'text-loss'
        )}
      >
        {signedMoney(value)}
      </span>
      <small className="mt-[3px] block text-micro font-500 text-ink-4">
        {t(value < 0 ? 'upcoming.loss' : 'upcoming.gain')}
      </small>
    </>
  );
}
