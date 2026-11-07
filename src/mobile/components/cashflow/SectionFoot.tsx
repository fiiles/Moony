import type { ReactNode } from 'react';

/**
 * The quiet line under a breakdown's rows (the desktop cards' footer): 12 / 500 `ink-4`, each
 * child on its own line, wrapping. It says what the section counts ("Nepočítá se do příjmů ani
 * výdajů") and its totals.
 */
export function SectionFoot({ lines }: { lines: readonly ReactNode[] }) {
  const shown = lines.filter((line) => line !== null && line !== undefined && line !== false);
  if (shown.length === 0) return null;
  return (
    <div className="mx-0.5 -mt-1.5 mb-3.5 text-m-caption font-medium leading-[1.4] text-ink-4 num">
      {shown.map((line, index) => (
        <p key={index} className="m-0">
          {line}
        </p>
      ))}
    </div>
  );
}
