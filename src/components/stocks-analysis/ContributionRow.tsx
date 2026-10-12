import { ChevronRight } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { formatPoints, toneOf } from '@/utils/performance-format';
import { AssetLogo } from '@/components/common/AssetLogo';
import type { ContributionItem } from './contribution-model';

/**
 * Name, bar, value. In the card: compact below 1360 px (it is 351 px wide at the 1080 px
 * minimum), the design's widths from there.
 */
export const CARD_GRID =
  'grid grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)_72px] items-center gap-x-3 px-[21px] min-[1360px]:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)_84px] min-[1360px]:gap-x-[14px]';

/** The same columns in the 560 px dialog, padded like the dialog's content. */
export const DIALOG_GRID =
  'grid grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)_84px] items-center gap-x-[14px] px-[27px]';

/** Colour of a signed value: gain, loss, or neutral when it rounds to zero. */
function toneClass(value: number): string {
  const tone = toneOf(value);
  return tone === 'gain' ? 'text-gain' : tone === 'loss' ? 'text-loss' : 'text-ink-2';
}

interface ContributionRowProps {
  row: ContributionItem;
  /** Position of the zero line, percent of the track width (`barLayout`). */
  zero: number;
  /** Position and width of this row's bar, percent of the track width (`barLayout`). */
  bar: { left: number; width: number };
  /** Columns and horizontal padding: `CARD_GRID` or `DIALOG_GRID`. */
  grid: string;
  /** Makes the whole row a button that opens what the row folds (the "rest" row). */
  onOpen?: () => void;
  /** Accessible name and title of that button. */
  openLabel?: string;
}

/**
 * One row of the contribution list, in the card and in the dialog that lists what the card
 * folded: logo and name (or name and "N pozic" for a part), a bar from the shared zero line
 * (positive `s2`, negative `s3`) and the value in percentage points.
 */
export function ContributionRow({ row, zero, bar, grid, onOpen, openLabel }: ContributionRowProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const pp = t('stocksAnalysis.pp');

  const content = (
    <>
      <span className="flex min-w-0 items-center gap-[10px]">
        {row.ticker !== null && (
          <AssetLogo ticker={row.ticker} type="stock" className="size-6 text-[8.5px]" />
        )}
        <span className="min-w-0 py-[7px]">
          <b
            className={cn(
              'block truncate text-table',
              row.muted ? 'font-600 text-ink-3' : 'font-650 text-ink'
            )}
          >
            {row.name}
          </b>
          {row.positions !== null && (
            <small className="mt-0.5 block truncate text-micro font-500 text-ink-4">
              {t('stocksAnalysis.board.positions', { count: row.positions })}
            </small>
          )}
        </span>
        {onOpen && <ChevronRight aria-hidden className="ml-auto size-[14px] shrink-0 text-ink-4" />}
      </span>
      <span aria-hidden className="relative block h-4">
        <i className="absolute -bottom-3 -top-3 w-px bg-line-strong" style={{ left: `${zero}%` }} />
        <i
          className={cn('absolute top-1 h-2 rounded-[2px]', row.points < 0 ? 'bg-s3' : 'bg-s2')}
          style={{ left: `${bar.left}%`, width: `${bar.width}%` }}
        />
      </span>
      <span
        className={cn('text-right font-650 num whitespace-nowrap', toneClass(row.points))}
        title={
          row.periodReturn === null
            ? undefined
            : t('stocksAnalysis.contribution.periodReturn', {
                value: fmt.percent(row.periodReturn / 100, 1, { signed: true }),
              })
        }
      >
        {formatPoints(fmt.number, row.points, pp)}
      </span>
    </>
  );

  // The li's hairline counts towards the 40 px of a row, as it did when the li was the grid
  return (
    <li className="border-t border-line-soft text-table">
      {onOpen ? (
        <button
          type="button"
          onClick={onOpen}
          title={openLabel}
          aria-label={openLabel}
          className={cn(
            grid,
            'min-h-[39px] w-full text-left transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:shadow-[inset_0_0_0_2px_var(--focus-border)]'
          )}
        >
          {content}
        </button>
      ) : (
        <div className={cn(grid, 'min-h-[39px]')}>{content}</div>
      )}
    </li>
  );
}

interface ContributionTotalRowProps {
  label: string;
  points: number;
  grid: string;
  className?: string;
}

/** Footer of the list: what the rows add up to, on `well`. */
export function ContributionTotalRow({
  label,
  points,
  grid,
  className,
}: ContributionTotalRowProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  return (
    <li className={cn(grid, 'min-h-[40px] border-t border-line bg-well text-table', className)}>
      <b className="truncate font-700 text-ink">{label}</b>
      <span />
      <span className={cn('text-right font-650 num whitespace-nowrap', toneClass(points))}>
        {formatPoints(fmt.number, points, t('stocksAnalysis.pp'))}
      </span>
    </li>
  );
}
