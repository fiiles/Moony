import type { ReactNode } from 'react';
import { Link } from 'wouter';
import { ChevronRight } from 'lucide-react';

interface SectionHeadProps {
  title: string;
  /** The trailing link ("Zobrazit vše (7)", "Majetek"), drawn with its chevron; a link to `href`. */
  action?: { label: ReactNode; href: string };
  /** The section's one number ("nesplaceno 2 418 760 Kč", `.m-sec__n`); not tappable, so never with an `action`. */
  note?: ReactNode;
  /** A short line under the head that says once what every row's figure measures ("Změna za 30 dní"). */
  caption?: string;
}

/**
 * Section head (README §13, `.m-sec`): the 16 / 620 title and a 12 / 600 `ink-3` link with a
 * chevron, whose hit area reaches 44 pt (an `::after` extension, as the contract's small buttons
 * do) — or, when there is nothing to open, the section's one number (`.m-sec__n`). One form for
 * every head that leads to a fuller list or screen (audit 2026-11-15 C8): never a text arrow.
 */
export function SectionHead({ title, action, note, caption }: SectionHeadProps) {
  const head = (
    <div
      className={
        caption
          ? 'flex items-baseline justify-between gap-3'
          : 'mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3'
      }
    >
      <h2 className="m-0 min-w-0 truncate text-m-section text-ink">{title}</h2>
      {action && (
        <Link
          href={action.href}
          className="relative flex flex-none items-center gap-0.5 whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink"
        >
          {action.label}
          <ChevronRight aria-hidden strokeWidth={2.25} className="size-3.5 flex-none" />
        </Link>
      )}
      {!action && note && (
        <span className="flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 num">
          {note}
        </span>
      )}
    </div>
  );
  if (!caption) return head;
  return (
    <div className="mx-0.5 mb-2 mt-4">
      {head}
      <p className="m-0 mt-0.5 truncate text-m-caption text-ink-4">{caption}</p>
    </div>
  );
}
