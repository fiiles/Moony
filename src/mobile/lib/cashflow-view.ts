/**
 * The two views of the Cashflow tab root: the period `report` and the year's spending `plans`.
 * The view lives in the same query string as the period (`?tf=…&o=…&acc=…`): `?view=plans` (with
 * `&year=N` for the plans year) is the plans view, no `view` is the report. The helpers only touch
 * `view` and `year`, so the period survives going back and forth.
 */
export type CashflowView = 'report' | 'plans';

/** The view the search names; `undefined` when it names none (or one that is not known). */
export function viewFromSearch(search: string): CashflowView | undefined {
  const view = new URLSearchParams(search).get('view');
  return view === 'plans' ? 'plans' : undefined;
}

/**
 * The search for `view` on top of `search`: every other parameter is kept. The plans view writes
 * `view=plans` and, when `year` is given, `year=` (else it keeps the year the search has); the
 * report drops both. `''` when nothing is left, else `?…`.
 */
export function cashflowViewSearch(search: string, view: CashflowView, year?: number): string {
  const params = new URLSearchParams(search);
  if (view === 'plans') {
    params.set('view', 'plans');
    if (year !== undefined) params.set('year', String(year));
  } else {
    params.delete('view');
    params.delete('year');
  }
  const query = params.toString();
  return query ? `?${query}` : '';
}
