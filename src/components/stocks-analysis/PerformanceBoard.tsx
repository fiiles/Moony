import type { CSSProperties, ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { StockTag } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { maxDrawdownTo } from '@/utils/stock-performance';
import { formatPoints, toneOf } from '@/utils/performance-format';
import { fxEffect, type ModelRow } from './performance-model';

/** The kind of split a board belongs to; it decides the caption. */
export type BoardKind = 'none' | 'tags' | 'sector' | 'currency' | null;

interface PerformanceBoardProps {
  rows: readonly ModelRow[];
  /** Header of the first column: the name of the group. */
  groupLabel: string;
  kind: BoardKind;
  /** Report currency of every figure. */
  currency: string;
  /** Index of the point the return, drawdown and contribution are read at (the hovered day). */
  point: number;
  /** The period ends today: "Hodnota", else "K {date}". */
  endsToday: boolean;
  /** Last day of the period, unix seconds. */
  endDate: number;
  filterActive: boolean;
  highlighted: string | null;
  onHighlight: (id: string | null) => void;
  /** Click on a part that is a tag: filter the page by it. */
  onFilterTag: (tag: StockTag) => void;
}

/**
 * Columns of the board: swatch, name, return, vs portfolio, contribution, max drawdown, share,
 * value. Compact below 1360 px (the page is 720 px wide at the 1080 px minimum), the design's
 * widths from there.
 */
const GRID =
  'grid grid-cols-[14px_minmax(0,1.2fr)_80px_84px_72px_76px_minmax(0,1fr)_92px] items-center gap-x-3 px-6 min-[1360px]:grid-cols-[22px_minmax(0,1.25fr)_116px_112px_84px_92px_minmax(0,1fr)_118px] min-[1360px]:gap-x-[14px]';

/** Hatched square: a part the chart does not draw. */
const HATCH: CSSProperties = {
  background: 'repeating-linear-gradient(135deg, transparent 0 2px, var(--line-strong) 2px 3px)',
};

function Swatch({ row }: { row: ModelRow }) {
  // The portfolio is dashed as a reference next to parts; on its own (`main`) it is solid
  if (row.kind === 'portfolio' && !row.main) {
    return (
      <i
        aria-hidden
        className="block h-[2.5px] w-[14px] rounded-[1px]"
        style={{
          background:
            'repeating-linear-gradient(90deg, var(--chart-line) 0 3px, transparent 3px 5px)',
        }}
      />
    );
  }
  if (row.kind === 'index') {
    return (
      <i
        aria-hidden
        className="block h-[2px] w-[14px] rounded-[1px]"
        style={{
          background: 'repeating-linear-gradient(90deg, var(--ink-4) 0 2px, transparent 2px 4px)',
        }}
      />
    );
  }
  if (row.drawn && row.color) {
    return (
      <i
        aria-hidden
        className="block h-[2.5px] w-[14px] rounded-[1px]"
        style={{ background: row.color }}
      />
    );
  }
  return (
    <i aria-hidden className="block size-2 rounded-[2px] border border-line-strong" style={HATCH} />
  );
}

/**
 * The legend of the performance chart with numbers (design system §7 Analysis): one row per part
 * of the chosen group, then the portfolio (the reference line), the index and the part without a
 * tag or sector. Return, drawdown and contribution follow the hovered day; the value and the
 * share are as of the end of the period. Hovering a row highlights its line; a tag row filters the
 * page by that tag.
 */
export function PerformanceBoard({
  rows,
  groupLabel,
  kind,
  currency,
  point,
  endsToday,
  endDate,
  filterActive,
  highlighted,
  onHighlight,
  onFilterTag,
}: PerformanceBoardProps) {
  const { t } = useTranslation('reports');
  const fmt = useFormat();
  const pp = t('stocksAnalysis.pp');

  const percent = (value: number, digits = 1) => fmt.percent(value / 100, digits, { signed: true });
  const points = (value: number) => formatPoints(fmt.number, value, pp);
  const money = (value: number | null) =>
    value === null ? '—' : fmt.money(value, currency, { decimals: 0 });
  const dim = <span className="text-ink-4">—</span>;

  const portfolio = rows.find((row) => row.kind === 'portfolio');
  const portfolioReturn = portfolio?.twr[point] ?? null;

  const subLine = (row: ModelRow): ReactNode => {
    // On its own the portfolio is the main line, not a reference
    if (row.kind === 'portfolio') return row.main ? null : t('stocksAnalysis.board.reference');
    if (row.kind === 'index') return t('stocksAnalysis.board.index');
    if (kind === 'currency' && row.currency !== null) {
      if (row.currency === currency) return t('stocksAnalysis.board.noFx');
      const native = row.native?.[point];
      const effect = fxEffect(row, point);
      if (native !== undefined && effect !== null) {
        return t('stocksAnalysis.board.fx', {
          currency: row.currency,
          native: percent(native),
          effect: points(effect),
        });
      }
    }
    const count = t('stocksAnalysis.board.positions', { count: row.count ?? 0 });
    return !row.drawn && !row.isNone ? `${count} · ${t('stocksAnalysis.board.noLine')}` : count;
  };

  const renderRow = (row: ModelRow) => {
    const value = row.twr[point] ?? null;
    const tag = row.kind === 'part' ? row.tag : null;
    const clickable = tag !== null;
    const returnTone = row.quiet ? 'flat' : toneOf(value);
    const contribution = row.contribution?.[point];
    const drawdown = maxDrawdownTo(row.drawdown, point);
    const hot = highlighted === row.id;
    return (
      <div
        key={row.id}
        role="row"
        className={cn(
          GRID,
          'min-h-[40px] border-t border-line-soft text-table text-ink-2 transition-colors duration-fast',
          (clickable || !row.quiet) && 'hover:bg-well',
          hot && 'bg-well',
          clickable && 'cursor-pointer'
        )}
        onMouseEnter={() => row.drawn && onHighlight(row.id)}
        onMouseLeave={() => onHighlight(null)}
        onClick={tag ? () => onFilterTag(tag) : undefined}
      >
        <span role="cell" className="flex items-center">
          <Swatch row={row} />
        </span>
        <span role="cell" className="min-w-0 py-[7px]">
          {tag ? (
            <button
              type="button"
              title={t('stocksAnalysis.board.filterBy', { name: row.name })}
              className={cn(
                'block max-w-full truncate rounded-[3px] text-left text-table font-650 focus-visible:outline-none focus-visible:shadow-focus',
                row.quiet ? 'font-600 text-ink-3' : 'text-ink'
              )}
            >
              {row.name}
            </button>
          ) : (
            <b
              className={cn(
                'block truncate text-table',
                row.quiet ? 'font-600 text-ink-3' : 'font-650 text-ink'
              )}
            >
              {row.name}
            </b>
          )}
          {subLine(row) !== null && (
            <small className="mt-0.5 block truncate text-micro font-500 text-ink-4">
              {subLine(row)}
            </small>
          )}
        </span>
        <span
          role="cell"
          className={cn(
            'text-right num',
            row.quiet ? 'font-600 text-ink-3' : 'font-650',
            returnTone === 'gain' && 'text-gain',
            returnTone === 'loss' && 'text-loss'
          )}
        >
          {value === null ? dim : percent(value)}
        </span>
        <span role="cell" className="text-right num">
          {row.kind === 'portfolio' || value === null || portfolioReturn === null
            ? dim
            : points(value - portfolioReturn)}
        </span>
        <span role="cell" className="text-right num">
          {contribution === undefined ? dim : points(contribution)}
        </span>
        <span role="cell" className="text-right num">
          {fmt.percent(drawdown / 100, 1, { signed: true })}
        </span>
        <span role="cell" className="min-w-0">
          {row.share === null ? (
            dim
          ) : (
            <span className="flex items-center gap-[10px]">
              <i
                aria-hidden
                className="block h-[6px] flex-1 overflow-hidden rounded-[3px] bg-well-3"
              >
                <i
                  className="block h-full rounded-[3px]"
                  style={{
                    width: `${Math.min(100, Math.max(0, row.share * 100))}%`,
                    background: row.drawn && row.color ? row.color : 'var(--s-other)',
                  }}
                />
              </i>
              <span className="w-10 shrink-0 text-right font-600 text-ink-2 num">
                {fmt.percent(row.share, 0)}
              </span>
            </span>
          )}
        </span>
        <span role="cell" className="text-right num">
          {row.kind === 'index' ? (
            <span className="text-ink-4">
              {t('stocksAnalysis.performance.indexVia', { ticker: row.ticker })}
            </span>
          ) : (
            money(row.value)
          )}
        </span>
      </div>
    );
  };

  const caption = [
    kind === 'none' &&
      filterActive &&
      rows.some((row) => row.kind === 'part') &&
      t('stocksAnalysis.board.captionNone'),
    kind === 'tags' && t('stocksAnalysis.board.captionTags'),
    kind === 'sector' && t('stocksAnalysis.board.captionSector'),
    kind === 'currency' && t('stocksAnalysis.board.captionCurrency', { currency }),
    rows.some((row) => row.kind === 'part' && !row.drawn && !row.isNone) &&
      t('stocksAnalysis.board.captionMore'),
    kind !== null && kind !== 'none' && filterActive && t('stocksAnalysis.board.captionSelection'),
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div role="table" className="-mx-6 mt-4 border-t border-line-soft">
      <div
        role="row"
        className={cn(GRID, 'min-h-[34px] py-1.5 text-thead uppercase leading-[1.25] text-ink-4')}
      >
        <span role="columnheader" />
        <span role="columnheader" className="min-w-0 truncate">
          {groupLabel}
        </span>
        <span role="columnheader" className="text-right">
          {t('stocksAnalysis.board.return')}
        </span>
        <span role="columnheader" className="text-right">
          {t('stocksAnalysis.board.vsPortfolio')}
        </span>
        <span role="columnheader" className="text-right">
          {t('stocksAnalysis.board.contribution')}
        </span>
        <span role="columnheader" className="text-right">
          {t('stocksAnalysis.board.maxDrawdown')}
        </span>
        <span role="columnheader">
          {filterActive
            ? t('stocksAnalysis.board.shareSelection')
            : t('stocksAnalysis.board.share')}
        </span>
        <span role="columnheader" className="text-right">
          {endsToday
            ? t('stocksAnalysis.board.value')
            : t('stocksAnalysis.board.valueAt', { date: fmt.day(endDate) })}
        </span>
      </div>
      {rows.map(renderRow)}
      {caption && (
        <p className="m-0 border-t border-line-soft px-6 pb-[13px] pt-[10px] text-micro font-500 text-ink-4">
          {caption}
        </p>
      )}
    </div>
  );
}
