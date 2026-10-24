/** Share of a box a fitted figure may fill: text width is not exactly linear in the font size and a box is fractional. */
export const FIT_FILL = 0.98;
/** The smallest size a figure shrinks to, however long it is. */
export const FIT_MIN_PX = 8;
/** The resolution of a fit: sizes step down to half a pixel. */
export const FIT_STEP = 0.5;

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
  return Math.max(Math.floor(size / FIT_STEP) * FIT_STEP, FIT_MIN_PX);
}

/**
 * The fit to keep when `next` was just measured (`useFitText`): `current` is the fit now drawn and
 * `previous` the one before it. Text is not exactly linear in the font size, so measuring at A can
 * answer "A - 0.5" and measuring at A - 0.5 can answer "A": every figure box is observed, each
 * change of size is seen by the next measure, and the two half-pixel sizes would take turns for
 * ever. A grow of exactly one step back to the size just left is therefore ignored, and the figure
 * stays on the smaller of the two, which fits. A shrink is never ignored (a figure must never be
 * held above what fits), nor is a grow to any other size.
 */
export function settleFit(
  next: number | null,
  current: number | null,
  previous: number | null
): number | null {
  const growsBack =
    next !== null && current !== null && next === previous && next - current === FIT_STEP;
  return growsBack ? current : next;
}
