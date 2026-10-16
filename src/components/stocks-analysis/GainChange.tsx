import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { toneOf } from '@/utils/performance-format';
import { ChangeBadge } from '@/components/common/ChangeBadge';

/**
 * A gain the way the analysis tables show it: the signed amount in the gain or loss color, then
 * its percent as a change badge. A part that is unknown is left out; without both it is a dash.
 * In a narrow window the badge wraps under the amount, so the tables fit their 720 px.
 */
export function GainChange({
  amount,
  percent,
  currency,
}: {
  /** In `currency`. */
  amount: number | null;
  /** In percent (12.5 is 12,5 %). */
  percent: number | null;
  currency: string;
}) {
  const fmt = useFormat();
  if (amount === null && percent === null) return <span className="text-ink-5">—</span>;
  const tone = toneOf(amount);
  return (
    <span className="inline-flex flex-wrap items-center justify-end gap-x-2 gap-y-1">
      {amount !== null && (
        <span
          className={cn(
            'whitespace-nowrap font-650',
            tone === 'gain' && 'text-gain',
            tone === 'loss' && 'text-loss'
          )}
        >
          {fmt.money(amount, currency, { decimals: 0, signed: true })}
        </span>
      )}
      {percent !== null && <ChangeBadge ratio={percent / 100} />}
    </span>
  );
}
