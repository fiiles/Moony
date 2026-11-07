/**
 * The height of a phone sheet (the phone's own `Sheet` and a desktop dialog shown as one). A sheet
 * is as tall as its content up to 93 % of the screen (95 % with `full`), or exactly that tall with
 * `fill`. The percentage alone runs under the status bar on a phone with a Dynamic Island (5 % of
 * an 874 pt screen is 44 pt, the top safe area is about 62 pt), so the height is also capped at the
 * screen less the top safe area and a 10 px gap: the top edge always sits below the island.
 *
 * Whole class strings, so Tailwind finds them in the source.
 */
export function sheetHeightClass({
  fill = false,
  full = false,
}: {
  fill?: boolean;
  full?: boolean;
}) {
  if (fill) {
    return full
      ? 'h-[min(95%,calc(100%_-_var(--safe-top)_-_10px))]'
      : 'h-[min(93%,calc(100%_-_var(--safe-top)_-_10px))]';
  }
  return full
    ? 'max-h-[min(95%,calc(100%_-_var(--safe-top)_-_10px))]'
    : 'max-h-[min(93%,calc(100%_-_var(--safe-top)_-_10px))]';
}
