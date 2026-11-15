import { useRef, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';
import { useFitText } from '@/mobile/hooks/use-fit-text';

export interface StatItem {
  label: string;
  /** The shown text; a string, so the fit can follow it. */
  value: string;
  /** 10 / 500 line under the value (a change, "na účtech"). */
  note?: ReactNode;
  /** Colours the value for a signed result (`.m-stat__v--gain` / `--loss`); the sign stays in the text. */
  tone?: 'gain' | 'loss';
  /** The value is still being computed: a skeleton bar stands in its place, never a bare "—". */
  loading?: boolean;
}

/**
 * The floor of a stat value: a fixed 12 px, never one that grows with the text size. A floor above
 * the fit is a cut (an ellipsis on a money value), and a stat that fits at the default size must
 * fit at every size.
 */
const STAT_FLOOR = '12px';

/** The contract's rule (`.m-stat__v`): 17 px down to the 86 px box of a third, then with the card. */
const CONTAINER_SIZE = `clamp(${STAT_FLOOR}, 19.7cqi, var(--m-fs-stat))`;

/**
 * Stat row (spec §10.3, README §13, `.m-stats`): three (or two, or one) tiles on one fixed grid
 * (audit 2026-11-15 C2): label 11 on one line, value 17, note 10 on one line. The tiles share the
 * row's three tracks (`grid-template-rows: subgrid`), so the values of a row stand on one baseline and the
 * notes on another whatever a tile holds; a label that wraps at a large text size moves the whole
 * row's values together. A label never breaks inside a word (no hyphenation, B2): the phone's
 * labels are short (`mobile:stat`, `mobile:statWide`, measured by `locales.test.ts`). A note is
 * one line; what does not fit there belongs to the detail, not to the tile. A value never wraps:
 * the contract's container rule shrinks it with a narrow card, and a value longer than its card
 * shrinks the whole row to fit (every stat in a row shares one size, floor `STAT_FLOOR`).
 */
export function StatRow({ stats }: { stats: StatItem[] }) {
  const { t } = useTranslation('common');
  const rowRef = useRef<HTMLDivElement>(null);
  // The fit follows the shown values' text (a "—" is narrower than an amount).
  const fit = useFitText(rowRef, stats.map((stat) => (stat.loading ? '' : stat.value)).join('\n'));
  const notes = stats.some((stat) => stat.note !== undefined && stat.note !== null);

  return (
    <div
      ref={rowRef}
      className={cn(
        'mb-3.5 grid gap-x-2.5',
        notes ? 'grid-rows-[auto_auto_auto]' : 'grid-rows-[auto_auto]',
        stats.length === 1 ? 'grid-cols-1' : stats.length === 2 ? 'grid-cols-2' : 'grid-cols-3'
      )}
    >
      {stats.map((stat) => (
        <div
          key={stat.label}
          className={cn(
            'grid min-w-0 rounded-r3 [grid-template-rows:subgrid] border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-2.5 py-2.5 shadow-e1',
            notes ? 'row-span-3' : 'row-span-2'
          )}
        >
          <span className="mb-[3px] min-w-0 break-normal text-m-caption text-ink-3 [hyphens:manual] [text-wrap:balance]">
            {stat.label}
          </span>
          {/* The value's own query container: on the tile itself the containment would stop it
              from being a subgrid (layout containment makes a grid a grid of its own) */}
          <div className="min-w-0 self-end [container-type:inline-size]">
            {stat.loading ? (
              <span aria-busy="true" className="flex h-[1.25em] items-center text-m-stat">
                <span aria-hidden className="skeleton block h-[0.8em] w-4/5" />
                <span className="sr-only">{t('status.loading')}</span>
              </span>
            ) : (
              <span
                data-fit-text
                className={cn(
                  'block overflow-hidden text-ellipsis whitespace-nowrap text-m-stat num',
                  stat.tone === 'gain'
                    ? 'text-gain'
                    : stat.tone === 'loss'
                      ? 'text-loss'
                      : 'text-ink'
                )}
                style={{
                  fontSize:
                    fit === null
                      ? CONTAINER_SIZE
                      : `clamp(${STAT_FLOOR}, min(${fit}px, 19.7cqi), var(--m-fs-stat))`,
                }}
              >
                <span>{stat.value}</span>
              </span>
            )}
          </div>
          {notes && (
            <span className="mt-[3px] min-w-0 truncate text-m-note text-ink-4">
              {stat.note ?? null}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
