import { monthOffsetOf, type PeriodTimeframe } from '@/utils/period';

/**
 * What the Cashflow tab shows: a calendar month, quarter or year (`offset` periods back from the
 * current one, never forward) of the chosen accounts (`undefined`: all of them). It lives in the
 * URL search of the tab root, so a detail pushed on the tab and the way back keep the same period.
 */
export interface CashflowState {
  timeframe: PeriodTimeframe;
  /** Periods back from the current one: 0 is the current period, -1 the previous. */
  offset: number;
  accountIds: string[] | undefined;
}

export const DEFAULT_CASHFLOW_STATE: CashflowState = {
  timeframe: 'monthly',
  offset: 0,
  accountIds: undefined,
};

const TIMEFRAMES: readonly PeriodTimeframe[] = ['monthly', 'quarterly', 'yearly'];

/**
 * Reads `?tf=monthly|quarterly|yearly&o=<int ≤ 0>&acc=<id,id>`. A field that is missing or cannot
 * be read falls back to its default: an unknown timeframe is monthly, an offset that is not a whole
 * number is 0 and a positive one (the future) is 0 too, and no accounts means all of them.
 */
export function parseCashflowSearch(search: string): CashflowState {
  const params = new URLSearchParams(search);

  const tf = params.get('tf');
  const timeframe =
    TIMEFRAMES.find((candidate) => candidate === tf) ?? DEFAULT_CASHFLOW_STATE.timeframe;

  const o = params.get('o');
  const parsed = o !== null && /^[+-]?\d+$/.test(o) ? Number(o) : 0;
  // Zero, "-0" and the future all mean the current period.
  const offset = parsed < 0 ? parsed : 0;

  const ids = (params.get('acc') ?? '').split(',').filter((id) => id !== '');
  return { timeframe, offset, accountIds: ids.length > 0 ? ids : undefined };
}

/**
 * The search of a state: `''` for the default one, else `?` and only the fields that differ from
 * the default. `parseCashflowSearch(cashflowSearch(state))` is the state again (an empty account
 * list reads back as `undefined`, which means the same: all accounts).
 */
export function cashflowSearch(state: CashflowState): string {
  const params = new URLSearchParams();
  if (state.timeframe !== DEFAULT_CASHFLOW_STATE.timeframe) params.set('tf', state.timeframe);
  if (state.offset !== DEFAULT_CASHFLOW_STATE.offset) params.set('o', String(state.offset));
  if (state.accountIds && state.accountIds.length > 0) {
    params.set('acc', state.accountIds.join(','));
  }
  const query = params.toString();
  return query ? `?${query}` : '';
}

/**
 * Tapping a bar of the chart opens that month: the monthly view, the month's offset back from now
 * (`monthStart` is the UTC midnight of its first day, as the report's months are).
 */
export function selectMonth(
  monthStart: number,
  now?: Date
): Pick<CashflowState, 'timeframe' | 'offset'> {
  return { timeframe: 'monthly', offset: monthOffsetOf(monthStart, now) };
}

/** The period is not over yet: its last second (`periodEnd`, inclusive) is still ahead of `nowSeconds`. */
export function isPartialPeriod(periodEnd: number, nowSeconds: number): boolean {
  return periodEnd > nowSeconds;
}
