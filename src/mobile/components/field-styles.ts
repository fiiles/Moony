/** The classes of a plain text field of a sheet (the name, the note): 48 pt, the field role. */
export const TEXT_FIELD =
  'h-12 w-full rounded-[10px] border border-line-strong bg-paper px-3.5 text-m-field text-ink transition-[border-color,box-shadow] duration-fast placeholder:text-ink-5 focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring';

/** The frame of a picker field (`.m-field__i`): the amount field's look, the focus ring, `loss` when invalid. */
export const PICKER_FRAME =
  'flex h-12 w-full min-w-0 items-center rounded-[10px] border border-line-strong bg-paper px-3.5 text-left text-m-field transition-[border-color,box-shadow] duration-fast peer-focus:border-focus peer-focus:shadow-focus peer-aria-[invalid=true]:border-loss';

/**
 * The native picker over the frame: transparent and as large as the field, so a tap anywhere opens
 * the system's wheel (iOS, Android) or calendar; a browser that only opens it from its icon is
 * asked to by `showPicker()`.
 */
export const PICKER_INPUT =
  'peer absolute inset-0 z-[1] m-0 h-full w-full min-w-0 cursor-pointer appearance-none opacity-0';

/** Opens the platform's picker on a tap where the browser would only focus a segment. */
export function openPicker(input: HTMLInputElement) {
  try {
    input.showPicker?.();
  } catch {
    // Not allowed here (no user gesture) or not supported: the tap itself still focuses the field
  }
}
