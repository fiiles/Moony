import { useRef, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { useFitText } from '@/mobile/hooks/use-fit-text';

type HeroTone = 'gain' | 'loss' | 'neutral';

const TONES: Record<HeroTone, string> = {
  gain: 'text-gain',
  loss: 'text-loss',
  neutral: 'text-ink',
};

interface CashflowHeroProps {
  label: string;
  /** The period's figure; `null` when there is none (shown as "—"). */
  value: number | null;
  /** The shown text of a value (masked while amounts are hidden); a string, so the fit can follow it. */
  formatValue: (value: number) => string;
  /** Green for a surplus, red for a deficit; the sign is in the value too. */
  tone: HeroTone;
  /** The line under the number ("míra úspor 24 %"); the line keeps its height without one. */
  note?: ReactNode;
}

/**
 * The Cashflow hero (spec §8.7 item 1, README §13, `.m-card--hero`): the hero card of Přehled
 * without the period segment and the chart — label, the display number in the colour of its sign
 * and one note line. A long number shrinks to fit (floor `m-fs-title`), as the hero's does; the
 * decorative circle is at 60 %.
 */
export function CashflowHero({ label, value, formatValue, tone, note }: CashflowHeroProps) {
  const shownValue = value === null ? '—' : formatValue(value);
  const heroRef = useRef<HTMLElement>(null);
  const fit = useFitText(heroRef, shownValue);

  return (
    <section
      ref={heroRef}
      aria-label={label}
      className="relative mb-3.5 overflow-hidden rounded-r4 border border-line bg-[linear-gradient(180deg,var(--paper),var(--paper-2))] px-4 pb-3 pt-4 shadow-e1 after:pointer-events-none after:absolute after:-right-[76px] after:-top-[92px] after:size-[220px] after:rounded-full after:bg-hero-orb after:content-[''] [&>*]:relative [&>*]:z-[1]"
    >
      <div className="text-m-sub text-ink-3">{label}</div>
      <div
        data-fit-text
        className={cn(
          'mb-1.5 mt-[3px] overflow-hidden text-ellipsis whitespace-nowrap text-m-display num',
          value === null ? 'text-ink' : TONES[tone]
        )}
        style={
          fit === null
            ? undefined
            : { fontSize: `clamp(var(--m-fs-title), ${fit}px, var(--m-fs-display))` }
        }
      >
        <span>{shownValue}</span>
      </div>
      <div className="flex h-5 min-w-0 items-center whitespace-nowrap text-m-sub text-ink-3">
        {note && <span className="min-w-0 truncate">{note}</span>}
      </div>
    </section>
  );
}
