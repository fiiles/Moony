interface StepProgressProps {
  /** The 1-based place of the current step. */
  index: number;
  total: number;
  /** The caption above the bar: "2 / 7 · Účet". */
  label: string;
}

/**
 * Where the person is in a flow of steps (the first run): the caption in `ink-3` over a 3 pt bar
 * on `well-2` whose `dark` fill is `index / total` long. The caption says it all, so the bar is
 * decoration for a screen reader.
 */
export function StepProgress({ index, total, label }: StepProgressProps) {
  return (
    <div className="mb-4">
      <p className="m-0 mb-1.5 text-m-caption text-ink-3">{label}</p>
      <div aria-hidden className="h-[3px] overflow-hidden rounded-full bg-well-2">
        <div
          className="h-full rounded-full bg-dark transition-[width] duration-base motion-reduce:transition-none"
          style={{ width: `${Math.min(100, Math.max(0, (index / total) * 100))}%` }}
        />
      </div>
    </div>
  );
}
