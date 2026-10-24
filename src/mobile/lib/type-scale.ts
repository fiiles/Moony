/** The iOS body text size (`font: -apple-system-body`) at the default "Large" setting, in px. */
export const DEFAULT_BODY_PX = 17;

/**
 * The Dynamic Type factor of the phone (spec §10.2): the measured body size over the default 17 px,
 * rounded to three decimals. `1` for anything that is not a positive, finite size, so a failed
 * measurement leaves the text at its design size. The factor is published raw as `--m-type-scale`;
 * `--m-text-k` and `--m-value-k` in `index.css` clamp it to 130 % and 110 %.
 */
export function typeScaleFrom(bodyPx: number): number {
  if (!Number.isFinite(bodyPx) || bodyPx <= 0) return 1;
  return Math.round((bodyPx / DEFAULT_BODY_PX) * 1000) / 1000;
}
