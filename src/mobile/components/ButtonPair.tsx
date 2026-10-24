import type { LucideIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

interface PairButton {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
  disabled?: boolean;
}

/** One button of the pair: 48 pt, radius 10, the label 13 px and allowed a second line. */
const PAIR_BUTTON =
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
  className,
}: {
  primary: PairButton;
  secondary: PairButton;
  className?: string;
}) {
  return (
    <div className={cn('mb-3.5 grid grid-cols-2 gap-2.5', className)}>
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
