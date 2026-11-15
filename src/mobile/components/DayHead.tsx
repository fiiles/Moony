import type { ReactNode } from 'react';

/**
 * Day head of the ledger (README §13, `.m-dayhead`): "Dnes", "Včera", "14. 10." in 11 / 600
 * uppercase `ink-3`, sticky at the top of the scroll area while its day's rows pass under it,
 * above the rows' dividers (z 2), below the nav bar. It reaches the screen edges as translucent
 * canvas material with a blur (audit C11): the page's glow shows through, so the head draws no
 * flat band over it, and a row sliding under it stays visible, blurred, instead of being cut off
 * by an opaque edge (the system bars' material).
 */
export function DayHead({ children }: { children: ReactNode }) {
  return (
    <h3 className="sticky top-0 z-[3] -mx-4 bg-[color-mix(in_srgb,var(--canvas)_78%,transparent)] px-[18px] pb-1.5 pt-3 text-m-caption font-semibold uppercase tracking-[0.06em] text-ink-3 backdrop-blur-md backdrop-saturate-150">
      {children}
    </h3>
  );
}
