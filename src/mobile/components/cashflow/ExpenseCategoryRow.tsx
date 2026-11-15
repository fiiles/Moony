import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import { List, Pencil, Plus } from 'lucide-react';
import type { TransactionCategory } from '@shared/schema';
import type { ExpenseRow } from '@/utils/cashflow-budget-rows';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { EntityRow } from '@/mobile/components/EntityRow';
import { RowMenuItem } from '@/mobile/components/RowMenu';
import { useApprox } from '@/mobile/hooks/use-approx';
import type { LimitMode } from '@/mobile/lib/budget-limit';
import { expenseRowMoney, limitStatus, type RowLimitAction } from '@/mobile/lib/budget-rows';
import { CategoryChip } from './CategoryChip';
import { ProgressBar } from './ProgressBar';

/** The desktop's words for a limit whose usage cannot be worked out (`ExpensesByCategoryCard`). */
const UNAVAILABLE_KEY = 'readiness.usageUnavailable';

/** Stands for the limit amount inside the translated sentence, which is then split around it. */
const SLOT = '\u2063';

function withSlot(text: string): { before: string; after: string } {
  const at = text.indexOf(SLOT);
  return at < 0
    ? { before: text, after: '' }
    : { before: text.slice(0, at), after: text.slice(at + 1) };
}

interface ExpenseCategoryRowProps {
  row: ExpenseRow;
  /** The translated name ("Potraviny", "Nezařazeno"). */
  title: string;
  /** The category's icon and hue; `null` for the uncategorized group. */
  category: TransactionCategory | null;
  /** The currency of the report: the row's amount, unless its limit is in another one. */
  reportCurrency: string;
  /** A whole amount in the given currency (the report's or the limit's). */
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
 * `2026-11-07-mobile-cashflow-parity-design.md`, audit 2026-11-15 D18): chip, name, what was spent
 * ("—" when no payment could be converted; never red, as on the desktop) and "47 % výdajů · 3
 * transakce". Every row ends on the same edge: no visible `···`. A row with a limit has the usage
 * bar (red when over, the pace tick in a running period) and, under the row and outside its tap
 * target, one sentence (`limitStatus`): "Zbývá 1 229 Kč z 12 500 Kč", "Přečerpáno o 2 796 Kč ·
 * limit 12 500 Kč" (red, no minus) or "Vyčerpáno · limit 12 500 Kč", whose limit amount is the
 * dotted button that edits it, then "Jen část období" when it applies — or "Čerpání nelze
 * vyhodnotit · Upravit limit". A row without a limit has no bar: its share of spending is the
 * percentage in the sub-line. Every amount of a row is in one currency (`expenseRowMoney`, README
 * §13): the report's, or — when the limit is in another one — the limit's, with the report amount
 * as the "≈" under what was spent. All amounts are whole (a summary). Only in the "Bez limitu"
 * filter (`offerSetLimit`) does a row without a limit get the line "Bez limitu · Nastavit limit".
 * Swipe right sets or edits the limit; the long press keeps its menu, "Zobrazit transakce" and the
 * same limit action. Colour is never the only signal: the sentence says the state.
 */
export function ExpenseCategoryRow({
  row,
  title,
  category,
  reportCurrency,
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
  const approx = useApprox();
  const [, setLocation] = useLocation();
  const { limit } = row;
  const amounts = expenseRowMoney(row, reportCurrency);
  const evaluated = limit && limit.state !== 'unavailable' && limit.percentage !== null;
  const over = limit?.state === 'over';
  const canOpen = !busy && href !== undefined;
  const limitMode = busy ? null : (action?.mode ?? null);

  const sub = `${tr('actuals.expenses.shareOfExpenses', { percent: fmt.percent(row.share, 0) })} · ${tr(
    'actuals.transactions',
    { count: row.count }
  )}`;
  const fxUnavailable = tr('readiness.fxUnavailable');
  const value =
    amounts.spent === null ? (
      <span title={fxUnavailable}>
        —<span className="sr-only">{fxUnavailable}</span>
      </span>
    ) : (
      // A net refund reads "− 100 Kč" with the minus sign of the signed amounts
      native(amounts.spent, amounts.currency, amounts.spent < 0 ? { signed: true } : undefined)
    );
  // Written in the limit's currency: the report amount (the one the total adds up) is its "≈"
  const valueSub =
    amounts.currency === reportCurrency || amounts.spent === null
      ? undefined
      : amounts.approx !== null
        ? approx(amounts.spent, amounts.currency, { converted: amounts.approx, pending: false })
        : row.amountUnavailable
          ? fxUnavailable
          : undefined;

  // The limit line (D18): one sentence of what remains or how far over, in the limit's currency,
  // whose limit amount is the dotted button that edits it; no state word, no minus
  const status = limit ? limitStatus(limit, amounts.limitAmounts) : null;
  const editLabel = tr(
    action?.hasLimit ? 'actuals.expenses.editLimitFor' : 'actuals.expenses.setLimitFor',
    { name: title }
  );
  const limitButton = (text: string, amount: boolean) =>
    action ? (
      <button
        type="button"
        disabled={limitMode === null}
        aria-label={editLabel}
        onClick={() => limitMode && onLimit(limitMode)}
        className={cn(
          "relative whitespace-nowrap font-semibold text-ink [-webkit-touch-callout:none] after:absolute after:-inset-x-1 after:-inset-y-3 after:content-[''] active:text-ink-3 disabled:opacity-45",
          amount && 'underline decoration-line-strong decoration-dotted underline-offset-[3px] num'
        )}
      >
        {text}
      </button>
    ) : (
      <span className="whitespace-nowrap num">{text}</span>
    );
  let sentence: ReactNode;
  if (status && status.kind !== 'unavailable') {
    const words = withSlot(
      t(`cashflow.limit.${status.kind}`, {
        amount: native(status.amount, status.currency),
        limit: SLOT,
      })
    );
    sentence = (
      <span className={cn('whitespace-nowrap', status.kind === 'over' && 'text-loss')}>
        {words.before}
        {limitButton(native(status.limit, status.currency), true)}
        {words.after}
      </span>
    );
  } else if (limit) {
    sentence = (
      <>
        <span>{tb(UNAVAILABLE_KEY)}</span>
        {action?.hasLimit && (
          <>
            {' · '}
            {limitButton(tb('actions.editLimit'), false)}
          </>
        )}
      </>
    );
  } else {
    sentence = (
      <>
        <span>{tb('noBudget')}</span>
        {action && (
          <>
            {' · '}
            {limitButton(tb('actions.setLimit'), false)}
          </>
        )}
      </>
    );
  }

  const limitLine = (row.hasLimit || (offerSetLimit && action)) && (
    <p className="m-0 -mt-2 flex min-h-11 flex-wrap items-center gap-x-1 pb-0.5 pl-[62px] pr-3.5 text-m-caption font-semibold leading-[1.35] text-ink-3">
      {sentence}
      {limit && partial && (
        <span className="whitespace-nowrap">· {tr('actuals.expenses.partialPeriod')}</span>
      )}
    </p>
  );

  const hasMenu = !busy && (href !== undefined || action !== null);

  return (
    <EntityRow
      chip={<CategoryChip category={category} />}
      title={title}
      sub={sub}
      value={value}
      valueSub={valueSub}
      bar={
        limit && evaluated ? (
          <ProgressBar
            value={(limit.percentage ?? 0) / 100}
            tone={over ? 'over' : 'limit'}
            mark={pace}
          />
        ) : undefined
      }
      href={canOpen ? href : undefined}
      onSwipeRight={limitMode ? () => onLimit(limitMode) : undefined}
      swipeRightLabel={tb(action?.hasLimit ? 'actions.editLimit' : 'actions.setLimit')}
      swipeRightIcon={action?.hasLimit ? Pencil : Plus}
      menu={
        hasMenu
          ? {
              label: t('cashflow.rowMenu', { title }),
              // No visible `···` (D18): every row ends on the same edge; the limit is the swipe,
              // the long press and "Spravovat limity"
              shown: false,
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
