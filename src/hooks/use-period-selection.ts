import { useMemo, useState } from 'react';
import { useFormat } from '@/lib/use-format';
import { getUtcPeriodRange, type PeriodTimeframe, type UtcPeriodRange } from '@/utils/period';

export interface PeriodSelection {
  timeframe: PeriodTimeframe;
  /** Switches the granularity and returns to the current period. */
  setTimeframe: (next: PeriodTimeframe) => void;
  /** Periods back from the current one (0 = current, -1 = previous). */
  offset: number;
  setOffset: (update: (offset: number) => number) => void;
  range: UtcPeriodRange;
  /** "Říjen 2026", "Q4 2026" or "2026". */
  label: string;
  /** The same period inside a sentence: "septiembre de 2026", "září 2026", "Q4 2026", "2026". */
  labelInSentence: string;
}

/**
 * A calendar month, quarter or year on the UTC calendar (booking dates are UTC
 * midnight, ADR 0008), navigated backwards from the current one.
 */
export function usePeriodSelection(initial: PeriodTimeframe): PeriodSelection {
  const fmt = useFormat();
  const [timeframe, setTimeframeState] = useState<PeriodTimeframe>(initial);
  const [offset, setOffsetState] = useState(0);
  const range = useMemo(() => getUtcPeriodRange(timeframe, offset), [timeframe, offset]);
  const label = useMemo(() => {
    if (timeframe === 'monthly') return fmt.month(range.startDate);
    if (timeframe === 'quarterly') {
      return `Q${Math.floor(range.startDate.getUTCMonth() / 3) + 1} ${range.startDate.getUTCFullYear()}`;
    }
    return String(range.startDate.getUTCFullYear());
  }, [timeframe, range, fmt]);
  const labelInSentence = useMemo(
    () => (timeframe === 'monthly' ? fmt.monthInSentence(range.startDate) : label),
    [timeframe, range, fmt, label]
  );
  return {
    timeframe,
    setTimeframe: (next) => {
      setTimeframeState(next);
      setOffsetState(0);
    },
    offset,
    setOffset: (update) => setOffsetState((current) => Math.min(0, update(current))),
    range,
    label,
    labelInSentence,
  };
}
