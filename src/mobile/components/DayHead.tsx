import type { ReactNode } from 'react';

/**
 * Day head of the ledger (README §13, `.m-dayhead`): "Dnes", "Včera", "14. 10." in 11 / 600
 * uppercase `ink-3`, sticky at the top of the scroll area while its day's rows pass under it. It
 * reaches the screen edges on the canvas, which hides the rows sliding under it; above the rows'
 * dividers (z 2), below the nav bar.
 */
export function DayHead({ children }: { children: ReactNode }) {
  return (
    <h3 className="sticky top-0 z-[3] -mx-4 bg-canvas px-[18px] pb-1.5 pt-3 text-m-caption font-semibold uppercase tracking-[0.06em] text-ink-3">
      {children}
    </h3>
  );
}
