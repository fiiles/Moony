/**
 * The back button's label of a pushed screen (README §13, Navigation). A screen names its parent
 * ("‹ Finance") because that is where it is pushed from; a push that leaves its tab (a property's
 * mortgage, a Přehled agenda row) passes the label of the screen it leaves in the history state,
 * so the button names the page back really returns to. A deep link, a cold start or a push
 * without a label leaves no state, and the screen keeps its own label. The sheets' entries
 * spread the state they find (`sheet-history.ts`), so the label outlives a sheet on the target.
 * Pure: the state arrives as a value.
 */

/** Key of the history state that carries the label. */
const KEY = 'moonyBack';

/**
 * The history state of a push that leaves `label`'s screen, for `setLocation(path, { state })` or
 * a `Link`'s `state`; `undefined` without a label (the target keeps its own).
 */
export function backState(label: string | undefined): Record<string, string> | undefined {
  const trimmed = label?.trim();
  return trimmed ? { [KEY]: trimmed } : undefined;
}

/** The label the back button shows: the one the push left in `state`, else `fallback`. */
export function backLabel(state: unknown, fallback: string): string {
  if (state === null || typeof state !== 'object') return fallback;
  const label = (state as Record<string, unknown>)[KEY];
  return typeof label === 'string' && label.trim() !== '' ? label : fallback;
}
