import { Link } from 'wouter';
import { useTranslation } from 'react-i18next';
import {
  ArrowDownLeft,
  ArrowUpRight,
  Bitcoin,
  TrendingDown,
  TrendingUp,
  type LucideIcon,
} from 'lucide-react';
import { useCurrency } from '@/lib/currency';
import { useFormat } from '@/lib/use-format';
import { utcDayFloor } from '@/utils/chart-axis';
import { cn } from '@/lib/utils';
import type { MoveKind, RecentMove } from '@/hooks/use-recent-moves';
import type { CurrencyCode } from '@shared/currencies';

const ICONS: Record<MoveKind, LucideIcon> = {
  bankIn: ArrowDownLeft,
  bankOut: ArrowUpRight,
  buy: TrendingUp,
  sell: TrendingDown,
  cryptoBuy: Bitcoin,
  cryptoSell: Bitcoin,
};

/**
 * "Poslední pohyby" rows (design system §7 Overview): icon in a `well` square,
 * title + "Dnes · Spořicí účet", signed amount right. Only a bank credit is
 * green with a plus; a debit carries a minus in ink; buys and sells are
 * neutral amounts whose direction is in the title.
 */
export function RecentMoves({ moves, isLoading }: { moves: RecentMove[]; isLoading: boolean }) {
  const { t } = useTranslation('dashboard');
  const fmt = useFormat();
  const { convert, currencyCode, formatCurrencyRaw } = useCurrency();
  const today = utcDayFloor(Date.now() / 1000);

  const dayLabel = (date: number) => {
    const day = utcDayFloor(date);
    if (day === today) return t('common:time.today');
    if (day === today - 86400) return t('common:time.yesterday');
    return fmt.day(date, { day: 'numeric', month: 'short' });
  };

  if (isLoading && moves.length === 0) {
    return (
      <ul className="m-0 list-none p-0" aria-hidden>
        {Array.from({ length: 4 }, (_, i) => (
          <li
            key={i}
            className="grid grid-cols-[30px_1fr_auto] items-center gap-[11px] border-b border-line-soft py-[11px] last:border-0"
          >
            <div className="skeleton size-[29px] rounded-r2" />
            <div>
              <div className="skeleton h-3 w-2/5" />
              <div className="skeleton mt-1.5 h-2.5 w-1/4" />
            </div>
            <div className="skeleton h-3 w-16" />
          </li>
        ))}
      </ul>
    );
  }

  if (moves.length === 0) {
    return <p className="m-0 py-6 text-center text-table text-ink-3">{t('moves.empty')}</p>;
  }

  return (
    <ul className="m-0 list-none p-0">
      {moves.map((move) => {
        const Icon = ICONS[move.kind];
        const amount = convert(move.amount, move.currency as CurrencyCode, currencyCode);
        const signed =
          move.kind === 'bankIn'
            ? formatCurrencyRaw(amount).replace(/^/, '+ ')
            : move.kind === 'bankOut'
              ? formatCurrencyRaw(amount).replace(/^/, '− ')
              : formatCurrencyRaw(amount);
        const title = move.title || t(`moves.kind.${move.kind}`);
        const kindLabel = t(`moves.kind.${move.kind}`);
        const source =
          move.kind === 'bankIn' || move.kind === 'bankOut'
            ? move.source
            : `${kindLabel} · ${move.source}`;
        return (
          <li key={move.id} className="border-b border-line-soft last:border-0">
            <Link
              href={move.href}
              className="grid grid-cols-[30px_1fr_auto] items-center gap-[11px] py-[11px] text-ink hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
            >
              <i className="grid size-[29px] place-items-center rounded-r2 bg-well text-ink-2">
                <Icon className="size-[15px]" strokeWidth={1.75} aria-hidden />
              </i>
              <span className="min-w-0">
                <b className="block truncate text-table font-650">{title}</b>
                <small className="mt-[3px] block truncate text-micro font-500 text-ink-4">
                  {dayLabel(move.date)} · {source}
                </small>
              </span>
              <strong
                className={cn(
                  'text-table font-650 num',
                  move.kind === 'bankIn' && 'text-gain',
                  (move.kind === 'buy' ||
                    move.kind === 'sell' ||
                    move.kind === 'cryptoBuy' ||
                    move.kind === 'cryptoSell') &&
                    'text-ink-2'
                )}
              >
                {signed}
              </strong>
            </Link>
          </li>
        );
      })}
    </ul>
  );
}
