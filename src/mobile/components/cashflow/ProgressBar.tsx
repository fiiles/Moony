import { cn } from '@/lib/utils';

type ProgressTone = 'share' | 'limit' | 'over';

const FILLS: Record<ProgressTone, string> = {
  share: 'bg-s3',
  limit: 'bg-s1',
  over: 'bg-loss',
};

interface ProgressBarProps {
  /** How full the bar is, 0–1; more than 1 (an overspent limit) is a full bar. */
  value: number;
  /** `share`: a category's share of the movement; `limit`: used part of a limit; `over`: past it. */
  tone: ProgressTone;
}

/**
 * The thin bar under a category row (README §13, `.m-pbar`): a 6 px `well-3` track, radius 3, the
 * fill in `s3` (share), `s1` (limit) or `loss` (over). Hidden from assistive technology: the text
 * beside it says the same number.
 */
export function ProgressBar({ value, tone }: ProgressBarProps) {
  const width = Number.isFinite(value) ? Math.min(100, Math.max(0, value * 100)) : 0;
  return (
    <span aria-hidden className="block h-1.5 overflow-hidden rounded-[3px] bg-well-3">
      <span
        className={cn('block h-full rounded-[3px]', FILLS[tone])}
        style={{ width: `${width}%` }}
      />
    </span>
  );
}
