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
  /**
   * Where today sits in a period that is still running, 0–1 (the desktop's pace mark): a 2 pt tick
   * across the track. The text around the bars names it ("uplynulo 23 % období").
   */
  mark?: number;
}

const clampPercent = (value: number) =>
  Number.isFinite(value) ? Math.min(100, Math.max(0, value * 100)) : 0;

/**
 * The thin bar under a category row (README §13, `.m-pbar`): a 6 px `well-3` track, radius 3, the
 * fill in `s3` (share), `s1` (limit) or `loss` (over), and an optional `ink-3` pace tick. Hidden from
 * assistive technology: the text beside it says the same number.
 */
export function ProgressBar({ value, tone, mark }: ProgressBarProps) {
  return (
    <span aria-hidden className="relative block h-1.5 overflow-hidden rounded-[3px] bg-well-3">
      <span
        className={cn('block h-full rounded-[3px]', FILLS[tone])}
        style={{ width: `${clampPercent(value)}%` }}
      />
      {mark !== undefined && (
        <span
          className="absolute inset-y-0 w-0.5 -translate-x-1/2 bg-ink-3"
          style={{ left: `${clampPercent(mark)}%` }}
        />
      )}
    </span>
  );
}
