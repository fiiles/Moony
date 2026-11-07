/**
 * The intent of a main row's menu item (README §13, Gestures): "Koupit" on a position row opens the
 * position with its buy dialog already open. The push carries the intent in the history state,
 * beside the back label (`back-label.ts`); the detail reads it once (`useRowIntent`) and drops it
 * from the entry, so back, forward and a return from another tab never open the dialog again.
 * Pure: the state arrives as a value.
 */

/** Key of the history state that carries the intent. */
const KEY = 'moonyIntent';

/** Every act a main row's menu can open on its detail. */
export const ROW_INTENTS = [
  'record',
  'addTransaction',
  'balanceCheck',
  'rateChange',
  'edit',
  'end',
  'buy',
  'sell',
  'target',
  'revalue',
] as const;

export type RowIntent = (typeof ROW_INTENTS)[number];

/** The history state of a push that opens `intent` on its target, keeping `base` (the back label). */
export function intentState(
  base: Record<string, string> | undefined,
  intent: RowIntent
): Record<string, string> {
  return { ...base, [KEY]: intent };
}

/** The intent `state` carries, or null (none, or one this build does not know). */
export function readIntent(state: unknown): RowIntent | null {
  if (state === null || typeof state !== 'object') return null;
  const value = (state as Record<string, unknown>)[KEY];
  return typeof value === 'string' && (ROW_INTENTS as readonly string[]).includes(value)
    ? (value as RowIntent)
    : null;
}

/** `state` without its intent, every other key kept (the entry's back label, a sheet's mark). */
export function withoutIntent(state: unknown): unknown {
  if (state === null || typeof state !== 'object') return state;
  const rest = { ...(state as Record<string, unknown>) };
  delete rest[KEY];
  return rest;
}
