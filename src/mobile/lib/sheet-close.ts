/**
 * What a phone `Sheet` does when it is asked to close by anything but its own buttons (the scrim,
 * Escape, a drag down, the edge swipe or Android back): a `locked` sheet stays (it is busy, or it
 * shows what cannot be shown again, such as a new Recovery Kit), a `dirty` one asks "Zahodit
 * změny?" first, any other closes. Locked wins over dirty: nothing is asked while a sheet must stay.
 */
export type SheetCloseVerdict = 'close' | 'ask' | 'stay';

export function sheetCloseVerdict({
  locked,
  dirty,
}: {
  locked?: boolean;
  dirty?: boolean;
}): SheetCloseVerdict {
  if (locked === true) return 'stay';
  if (dirty === true) return 'ask';
  return 'close';
}
