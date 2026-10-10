import { Badge } from '@/components/ui/badge';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { CHANGE_MARK, changeDirection, splitOnMark } from '@/utils/change';

interface ChangeBadgeProps {
  /** Change as a ratio: 0.0042 is shown as "▲ 0,42 %" with `digits={2}`. */
  ratio: number;
  digits?: number;
  className?: string;
}

/**
 * Signed percentage change as a badge (design system §6): green with ▲ for a rise, red with ▼ for
 * a fall, neutral without a glyph when it rounds to zero. The glyph carries the direction, so the
 * number is unsigned; assistive tech gets the sign.
 */
export function ChangeBadge({ ratio, digits = 1, className }: ChangeBadgeProps) {
  const fmt = useFormat();
  const dir = changeDirection(ratio, digits);
  const variant = dir === 'up' ? 'gain' : dir === 'down' ? 'loss' : 'default';
  return (
    <Badge variant={variant} className={cn('num gap-1', className)}>
      {dir !== 'flat' && <span aria-hidden>{dir === 'up' ? '▲' : '▼'}</span>}
      {dir !== 'flat' && <span className="sr-only">{dir === 'up' ? '+' : '−'}</span>}
      {fmt.percent(Math.abs(ratio), digits)}
    </Badge>
  );
}

/**
 * A translated note with a change badge in place of its percent ("▲ 9,8 % oproti vloženému").
 * `render` receives the placeholder to pass as the percent; if a translation drops it, the note
 * falls back to the plain signed percent.
 */
export function ChangeNote({
  ratio,
  digits = 1,
  render,
}: {
  ratio: number;
  digits?: number;
  render: (percent: string) => string;
}) {
  const fmt = useFormat();
  const parts = splitOnMark(render(CHANGE_MARK));
  if (!parts) return <>{render(fmt.percent(ratio, digits, { signed: true }))}</>;
  return (
    <span className="inline-flex flex-wrap items-center gap-x-1.5 gap-y-1">
      {parts[0] && <span>{parts[0]}</span>}
      <ChangeBadge ratio={ratio} digits={digits} />
      {parts[1] && <span>{parts[1]}</span>}
    </span>
  );
}
