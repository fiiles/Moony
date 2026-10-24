/** Share of a box a fitted figure may fill: text width is not exactly linear in the font size and a box is fractional. */
export const FIT_FILL = 0.98;
/** The smallest size a figure shrinks to, however long it is. */
export const FIT_MIN_PX = 8;

/**
 * The font size in px at which a figure just fits its box (`useFitText`): `room` is the box's
 * content width, `natural` the width of the text drawn at `current` px. Text width scales with the
 * size, so one measurement is enough; 2 % of the room is left over (WebKit ends a line in an
 * ellipsis when the text is wider than the box by a fraction of a pixel, and the old measure took
 * the box's width rounded to whole pixels), and the result steps down to half a pixel. `null`
 * while there is nothing to measure.
 */
export function fitSizeFor(room: number, natural: number, current: number): number | null {
  if (!(room > 0) || !(natural > 0) || !(current > 0)) return null;
  const size = (room * FIT_FILL * current) / natural;
  return Math.max(Math.floor(size * 2) / 2, FIT_MIN_PX);
}
