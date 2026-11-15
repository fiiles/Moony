/**
 * The currency picker's short "Časté" group (audit A10): the chosen currency, then the euro and
 * the dollar, each once and only where the picker offers it. Pure.
 */
export function frequentCurrencies<T extends string>(current: T, options: readonly T[]): T[] {
  const wanted = [current, 'EUR', 'USD'] as string[];
  return wanted.filter(
    (code, at): code is T =>
      wanted.indexOf(code) === at && (options as readonly string[]).includes(code)
  );
}
