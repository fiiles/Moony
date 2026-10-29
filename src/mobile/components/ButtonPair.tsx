import type { LucideIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface PairButton {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
  disabled?: boolean;
}

/**
 * One button of the pair: 48 pt, radius 10, the label in the lead role and allowed a second line,
 * so a larger text makes the button taller, never wider. `CaptureButtons` draws its two with it.
 */
export const PAIR_BUTTON =
  'h-auto min-h-12 min-w-0 whitespace-normal rounded-[10px] px-2.5 py-2 text-center text-m-lead font-semibold leading-tight hyphens-auto [&_svg]:size-4';

/**
 * Two buttons sharing a row (README §13, `.m-btn-pair`): the account detail's "Zaznamenat
 * zůstatek" (primary) and "Importovat výpis" (secondary) under the nav bar — the spec's kept
 * exception to one primary per screen. A label that does not fit wraps to a second line instead
 * of overflowing (German at 360). A full-width button that belongs under the pair (an other
 * asset's Přecenit) takes the pair's own 10 pt gap: `className="mb-2.5"` (`.m-btn-pair--stack`).
 */
export function ButtonPair({
  primary,
  secondary,
  stacked = false,
  className,
}: {
  primary: PairButton;
  secondary: PairButton;
  /** One full-width button above the other (the account detail, under its chart). */
  stacked?: boolean;
  className?: string;
}) {
  return (
    <div className={cn('mb-3.5 grid gap-2.5', stacked ? 'grid-cols-1' : 'grid-cols-2', className)}>
      <Button className={PAIR_BUTTON} onClick={primary.onClick} disabled={primary.disabled}>
        <primary.icon strokeWidth={1.75} aria-hidden />
        {primary.label}
      </Button>
      <Button
        variant="secondary"
        className={PAIR_BUTTON}
        onClick={secondary.onClick}
        disabled={secondary.disabled}
      >
        <secondary.icon strokeWidth={1.75} aria-hidden />
        {secondary.label}
      </Button>
    </div>
  );
}
