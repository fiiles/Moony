/**
 * Whole-number (or fixed-decimal) shares of a total that add up to exactly 100.
 *
 * Rounding each share on its own can give 99 % (33.4 / 33.3 / 33.3 → 33 + 33 +
 * 33) or 103 % (12.5 × 5 + 37.5 → 13 × 5 + 38). The largest-remainder method
 * rounds every share down and then hands the missing points, one each, to the
 * shares that lost the most by being rounded down. Ties go to the
 * larger value, then to the earlier position, so the result is deterministic.
 *
 * Negative and non-finite values count as 0. The shares are relative to the sum
 * of the given values; with nothing to allocate every share is 0.
 *
 * @param decimals digits after the decimal point of each share (default 0)
 */
export function allocationPercents(values: readonly number[], decimals = 0): number[] {
  const clean = values.map((v) => (Number.isFinite(v) && v > 0 ? v : 0));
  const total = clean.reduce((a, b) => a + b, 0);
  if (total <= 0) return clean.map(() => 0);

  const unit = 10 ** decimals;
  const target = 100 * unit;
  const exact = clean.map((v) => (v / total) * target);
  const floors = exact.map((x) => Math.floor(x));
  const missing = target - floors.reduce((a, b) => a + b, 0);

  const order = exact
    .map((x, index) => ({ index, remainder: x - floors[index] }))
    .filter(({ index }) => clean[index] > 0)
    .sort(
      (a, b) => b.remainder - a.remainder || clean[b.index] - clean[a.index] || a.index - b.index
    );
  for (let k = 0; k < missing && k < order.length; k++) floors[order[k].index] += 1;

  return floors.map((f) => f / unit);
}
