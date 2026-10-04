/**
 * Time-weighted return on the stocks analysis page. The backend sends the cumulative return in
 * percent (`TwrDataPoint.twr`, 12.4 = +12.4 %); the lines chart plots it as an index where 100
 * is the start of the period (100 + twr), so every series starts on the same line.
 */

/**
 * The return an index value stands for, as the ratio `fmt.percent` expects: index 112.4 gives
 * 0.124 (shown as "+12.4 %"), 100 gives 0, 90 gives -0.1.
 */
export function twrPercent(index: number): number {
  return index / 100 - 1;
}

/**
 * Fraction digits the axis tick at `index` needs: a whole percentage reads "+5 %", the half or
 * quarter steps of a tight axis "+2.5 %" (rounding them to "+3 %" would put a wrong number on
 * the axis). Up to three digits: the nice steps of an axis that spans about 0.1 percentage points
 * are 0.025 apart.
 */
export function twrTickDigits(index: number): number {
  const percent = index - 100;
  for (const digits of [0, 1, 2]) {
    const scaled = percent * 10 ** digits;
    if (Math.abs(scaled - Math.round(scaled)) < 1e-6) return digits;
  }
  return 3;
}
