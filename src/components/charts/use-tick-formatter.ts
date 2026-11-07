import { useCallback, useMemo } from 'react';
import { useFormat } from '@/lib/use-format';
import { distinctTickLabels } from '@/utils/chart-axis';

/**
 * The formatter of a chart's value ticks: the caller's own `formatTick`, else the locale's compact
 * number with as many decimals as the axis needs for no two ticks to read alike
 * (`distinctTickLabels`).
 */
export function useTickFormatter(
  ticks: readonly number[],
  formatTick?: (value: number) => string
): (value: number) => string {
  const fmt = useFormat();
  const compact = useCallback(
    (value: number, digits: number) =>
      fmt.number(value, { notation: 'compact', maximumFractionDigits: digits }),
    [fmt]
  );
  const labels = useMemo(
    () => (formatTick ? null : distinctTickLabels(ticks, compact)),
    [ticks, compact, formatTick]
  );
  return useCallback(
    (value: number) => formatTick?.(value) ?? labels?.get(value) ?? compact(value, 1),
    [formatTick, labels, compact]
  );
}
