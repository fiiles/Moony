import type { ReactNode } from 'react';
import { Link } from 'wouter';

interface SectionHeadProps {
  title: string;
  /** The trailing action ("Zobrazit vše (7)", "Majetek →"); a link to `href`. */
  action?: { label: ReactNode; href: string };
}

/**
 * Section head (README §13, `.m-sec`): the 16 / 620 title and a 12 / 600 `ink-3` action whose hit
 * area reaches 44 pt (an `::after` extension, as the contract's small buttons do).
 */
export function SectionHead({ title, action }: SectionHeadProps) {
  return (
    <div className="mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3">
      <h2 className="m-0 min-w-0 truncate text-m-section text-ink">{title}</h2>
      {action && (
        <Link
          href={action.href}
          className="relative flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink"
        >
          {action.label}
        </Link>
      )}
    </div>
  );
}
