import { Fragment, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { List, Pencil, Plus } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import type { ExpenseRow, LimitState } from '@/utils/cashflow-budget-rows';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { EntityRow } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import type { LimitMode } from '@/mobile/lib/budget-limit';
import type { RowLimitAction } from '@/mobile/lib/budget-rows';
import { CategoryChip } from './CategoryChip';
import { ProgressBar } from './ProgressBar';

/** The desktop's words for each limit state (`ExpensesByCategoryCard`). */
const STATE_KEYS: Record<LimitState, string> = {
  over: 'table.state.overShort',
  exhausted: 'table.state.exhausted',
  near: 'table.state.near',
  ok: 'table.state.ok',
  unavailable: 'readiness.usageUnavailable',
};

interface ExpenseCategoryRowProps {
  row: ExpenseRow;
  /** The translated name ("Potraviny", "Nezařazeno"). */
  title: string;
  /** The category's icon and hue; `null` for the uncategorized group. */
  category: TransactionCategory | null;
  /** A report amount at the currency's minor units; a net refund reads with its minus sign. */
  money: (amount: number) => string;
  /** An amount in its own currency (the limit's), at its minor units. */
  native: (amount: number, currency: string, opts?: { signed?: boolean }) => string;
  /** The category's transactions in the period; `undefined` when it has none (a limit alone). */
  href: string | undefined;
  /** The limit covers only some months of the quarter or year ("Jen část období"). */
  partial: boolean;
  /** How far a running period has got, 0–1: the pace tick on the usage bar. */
  pace?: number;
  /** "Nastavit limit" or the limit that edits it; `null` for a row that cannot have a limit. */
  action: RowLimitAction | null;
  /**
   * The list is filtered to "Bez limitu": a row without a limit shows the compact "Nastavit limit"
   * line. Elsewhere it is reached through Spravovat limity and the long press.
   */
  offerSetLimit: boolean;
  /** The row still belongs to the previous period, while the next loads: nothing opens. */
  busy: boolean;
  onLimit: (mode: LimitMode) => void;
}

/**
 * An expense category of the Cashflow report (the desktop's `ExpensesByCategoryCard` row, spec
 * `2026-11-07-mobile-cashflow-parity-design.md`): chip, name, what was spent in the report currency
 * ("—" when no payment could be converted; never red, as on the desktop) and "47 % výdajů · 3
 * transakce". A row with a limit has the usage bar (red when over, the pace tick in a running
 * period) and, under the row and outside its tap target, the limit line: the state word and the
 * usage, what remains (or how far over) in the limit's own currency, "Jen část období" — or
 * "Čerpání nelze vyhodnotit" — and the limit amount as a 44 pt button that edits it. A row without
 * a limit has the share bar; only in the "Bez limitu" filter (`offerSetLimit`) does it get the line
 * "Bez limitu · Nastavit limit". The long press keeps its menu, "Zobrazit transakce" and the same
 * limit action. Colour is never the only signal: the state is worded and "over" has its arrow.
 */
export function ExpenseCategoryRow({
  row,
  title,
  category,
  money,
  native,
  href,
  partial,
  pace,
  action,
  offerSetLimit,
  busy,
  onLimit,
}: ExpenseCategoryRowProps) {
  const { t: tr } = useTranslation('reports');
  const { t: tb } = useTranslation('budgeting');
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const [, setLocation] = useLocation();
  const { limit } = row;
  const evaluated = limit && limit.state !== 'unavailable' && limit.percentage !== null;
  const over = limit?.state === 'over';
  const canOpen = !busy && href !== undefined;
  const limitMode = busy ? null : (action?.mode ?? null);

  const sub = `${tr('actuals.expenses.shareOfExpenses', { percent: fmt.percent(row.share, 0) })} · ${tr(
    'actuals.transactions',
    { count: row.count }
  )}`;
  const fxUnavailable = tr('readiness.fxUnavailable');
  const value = row.amountUnavailable ? (
    <span title={fxUnavailable}>
      —<span className="sr-only">{fxUnavailable}</span>
    </span>
  ) : (
    money(row.amount)
  );

  // The limit line's sentence, its parts joined by " · "
  const parts: ReactNode[] = [];
  if (limit && evaluated) {
    parts.push(
      <span className={over ? 'text-loss' : undefined}>
        {over && <span aria-hidden>↑ </span>}
        {tb(STATE_KEYS[limit.state])} {fmt.percent((limit.percentage ?? 0) / 100, 0)}
      </span>
    );
    if (limit.remaining !== null && limit.currency) {
      parts.push(
        limit.remaining < 0 ? (
          <span className="text-loss">
            {native(limit.remaining, limit.currency, { signed: true })}{' '}
            {tr('actuals.expenses.overLimit')}
          </span>
        ) : (
          `${tr('actuals.expenses.remaining')} ${native(limit.remaining, limit.currency)}`
        )
      );
    }
  } else if (limit) {
    parts.push(tb(STATE_KEYS.unavailable));
  } else {
    parts.push(tb('noBudget'));
  }
  if (limit && partial) parts.push(tr('actuals.expenses.partialPeriod'));

  const limitLabel = action?.hasLimit
    ? limit?.limit != null && limit.currency
      ? native(limit.limit, limit.currency)
      : tb('actions.editLimit')
    : tb('actions.setLimit');

  const limitLine = (row.hasLimit || (offerSetLimit && action)) && (
    <div className="-mt-2 flex min-h-11 items-center justify-between gap-3 pb-0.5 pl-[62px] pr-3.5">
      <p className="m-0 min-w-0 py-1 text-m-caption font-semibold leading-[1.35] text-ink-3 num">
        {parts.map((part, index) => (
          <Fragment key={index}>
            {index > 0 && ' · '}
            {part}
          </Fragment>
        ))}
      </p>
      {action && (
        <button
          type="button"
          disabled={limitMode === null}
          aria-label={tr(
            action.hasLimit ? 'actuals.expenses.editLimitFor' : 'actuals.expenses.setLimitFor',
            { name: title }
          )}
          onClick={() => limitMode && onLimit(limitMode)}
          className={cn(
            'inline-flex min-h-11 flex-none items-center whitespace-nowrap text-m-sub font-semibold text-ink [-webkit-touch-callout:none] active:text-ink-3 disabled:opacity-45',
            action.hasLimit &&
              'underline decoration-line-strong decoration-dotted underline-offset-[3px] num'
          )}
        >
          {limitLabel}
        </button>
      )}
    </div>
  );

  const hasMenu = !busy && (href !== undefined || action !== null);

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub}
      value={value}
      bar={
        limit && evaluated ? (
          <ProgressBar
            value={(limit.percentage ?? 0) / 100}
            tone={over ? 'over' : 'limit'}
            mark={pace}
          />
        ) : !row.hasLimit ? (
          <ProgressBar value={row.share} tone="share" />
        ) : undefined
      }
      href={canOpen ? href : undefined}
      menu={
        hasMenu
          ? {
              label: t('cashflow.rowMenu', { title }),
              // Nastavit / Upravit limit is nowhere else when the limit line is not drawn
              shown: !!action && !limitLine,
              children: (
                <>
                  <RowMenuItem disabled={!canOpen} onSelect={() => href && setLocation(href)}>
                    <List aria-hidden strokeWidth={1.75} />
                    {tb('actions.showTransactions')}
                  </RowMenuItem>
                  {action && (
                    <RowMenuItem
                      disabled={limitMode === null}
                      onSelect={() => limitMode && onLimit(limitMode)}
                    >
                      {action.hasLimit ? (
                        <Pencil aria-hidden strokeWidth={1.75} />
                      ) : (
                        <Plus aria-hidden strokeWidth={1.75} />
                      )}
                      {tb(action.hasLimit ? 'actions.editLimit' : 'actions.setLimit')}
                    </RowMenuItem>
                  )}
                </>
              ),
            }
          : undefined
      }
      below={limitLine || undefined}
    />
  );
}
