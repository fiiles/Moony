import { useCallback, useMemo } from 'react';
import { useFormat } from '@/lib/use-format';
import { distinctTickLabels, tickGutter } from '@/utils/chart-axis';
import { AXIS_WIDTH } from './ChartAxis';

/**
 * The formatter of a chart's value ticks and the width of the gutter that holds them: the
 * caller's own `formatTick`, else the locale's compact number with as many decimals as the axis
 * needs for no two ticks to read alike (`distinctTickLabels`). The gutter is `AXIS_WIDTH`, wider
 * only when the longest label needs it (`tickGutter`), so an extra decimal is never clipped.
 */
export function useTickFormatter(
  ticks: readonly number[],
  formatTick?: (value: number) => string
): { tickFormatter: (value: number) => string; axisWidth: number } {
  const fmt = useFormat();
  const compact = useCallback(
    (value: number, digits: number) =>
      fmt.number(value, { notation: 'compact', maximumFractionDigits: digits }),
    [fmt]
  );
  const labels = useMemo(
    () =>
      formatTick
        ? new Map(ticks.map((value) => [value, formatTick(value)]))
        : distinctTickLabels(ticks, compact),
    [ticks, compact, formatTick]
  );
  const tickFormatter = useCallback(
    (value: number) => labels.get(value) ?? formatTick?.(value) ?? compact(value, 1),
    [formatTick, labels, compact]
  );
  const axisWidth = useMemo(() => tickGutter(labels.values(), AXIS_WIDTH), [labels]);
  return { tickFormatter, axisWidth };
}
