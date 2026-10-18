import type { LucideIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface PairButton {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
  disabled?: boolean;
}

/** One button of the pair: 48 pt, radius 10, the label 13 px and allowed a second line. */
const PAIR_BUTTON =
  'h-12 min-w-0 whitespace-normal rounded-[10px] px-2.5 text-center text-[13px] font-semibold leading-tight hyphens-auto [&_svg]:size-4';

/**
 * Two buttons sharing a row (README §13, `.m-btn-pair`): the account detail's "Zaznamenat
 * zůstatek" (primary) and "Importovat výpis" (secondary) under the nav bar — the spec's kept
 * exception to one primary per screen. A label that does not fit wraps to a second line instead
 * of overflowing (German at 360).
 */
export function ButtonPair({ primary, secondary }: { primary: PairButton; secondary: PairButton }) {
  return (
    <div className="mb-3.5 grid grid-cols-2 gap-2.5">
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
