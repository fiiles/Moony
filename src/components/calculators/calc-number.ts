/** "3 500 000" or "5,1" → number; anything unreadable → 0. */
export function parseCalcNumber(value: string): number {
  const n = parseFloat(value.replace(/\s/g, '').replace(',', '.'));
  return isFinite(n) ? n : 0;
}
