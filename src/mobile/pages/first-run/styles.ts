/**
 * Sizes shared by the first run's steps. The lock page keeps its own copies of the first three
 * (`Lock.tsx` does not export them): the same 48 pt controls, the same 44 pt text links.
 */

/** The phone's primary button (README §13): 48 pt, radius 10, 15 / 600. */
export const primaryButton = 'h-12 w-full rounded-[10px] text-m-row';

/**
 * A field: 48 pt, 15 pt text. iOS would zoom into a field under 16 px on focus; `maximum-scale=1`
 * in `index.html` is what stops that (see the note in `Lock.tsx`).
 */
export const field48 = 'h-12 rounded-[10px] text-m-body';

/** A form error under its field. */
export const fieldMessage = 'text-left text-m-sub';

/** The text link under the primary ("Zpět", "Teď ne", "Zatím přeskočit"): 44 pt tall, centred. */
export const textLink =
  'mx-auto mt-2.5 block h-11 px-3 text-[13px] font-650 text-ink-2 underline underline-offset-[3px] [-webkit-touch-callout:none] active:text-ink disabled:pointer-events-none disabled:opacity-45';

/** The label of a field that is not a form control of the shadcn `Form` (the first account's name). */
export const fieldLabel = 'mx-0.5 mb-1.5 block text-m-sub font-semibold text-ink-2';

/** The card a group of rows sits in (`.m-card`), rows `line-soft` apart. */
export const rowCard =
  'mb-4 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

/** A 12 pt sentence under a row or a button. */
export const caption = 'm-0 px-1 text-m-sub text-ink-4';
