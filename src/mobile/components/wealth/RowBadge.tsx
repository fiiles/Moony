import type { ReactNode } from 'react';

/** The badge after a row's title ("ručně", "uzavřená", "do roka"): 18 pt, 11 / 600 `ink-3` on `well-2` (`.m-row__badge`). */
const BADGE =
  'ml-1.5 inline-flex min-h-[18px] items-center rounded-r1 bg-well-2 px-1.5 py-px align-[2px] text-m-caption font-semibold leading-[1.3] text-ink-3';

/**
 * A short word after the title of a class row (README §13, `.m-row__badge`): Akcie's "ručně" and
 * "uzavřená", Dluhopisy' "do roka" and "splaceno". It sits inside the title, which still ends
 * with an ellipsis.
 */
export function RowBadge({ children }: { children: ReactNode }) {
  return <span className={BADGE}>{children}</span>;
}
