import type { ReactNode } from 'react';
import { Link } from 'wouter';
import { ArrowLeft } from 'lucide-react';
import { cn } from '@/lib/utils';

interface PageHeadProps {
  /** 10 px uppercase line above the title; segments joined with " · ". */
  eyebrow?: ReactNode | ReactNode[];
  title: ReactNode;
  /** One sentence under the title. */
  description?: ReactNode;
  /** Right side: actions with at most one primary button, or a period switch. */
  actions?: ReactNode;
  className?: string;
}

/** Page head (design system §4): eyebrow, H1, one sentence, actions right. */
export function PageHead({ eyebrow, title, description, actions, className }: PageHeadProps) {
  const segments = Array.isArray(eyebrow) ? eyebrow.filter(Boolean) : eyebrow ? [eyebrow] : [];
  return (
    <div className={cn('mb-[26px] flex items-end justify-between gap-6', className)}>
      <div className="min-w-0">
        {segments.length > 0 && (
          <div className="mb-[10px] text-eyebrow uppercase text-ink-4">
            {segments.map((segment, i) => (
              <span key={i}>
                {i > 0 && (
                  <span aria-hidden className="mx-1.5 text-ink-5">
                    ·
                  </span>
                )}
                {segment}
              </span>
            ))}
          </div>
        )}
        <h1 className="m-0 text-h1 text-ink">{title}</h1>
        {description && <p className="mt-2 text-body text-ink-3">{description}</p>}
      </div>
      {actions && <div className="flex flex-none items-center gap-2">{actions}</div>}
    </div>
  );
}

/** `.back`: "← Zpět na akcie" above the page head of a detail page. */
export function BackLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link
      href={href}
      className="mb-[18px] inline-flex items-center gap-1.5 rounded-r1 text-table font-600 text-ink-3 transition-colors duration-fast hover:text-ink focus-visible:outline-none focus-visible:shadow-focus"
    >
      <ArrowLeft className="size-3.5" aria-hidden />
      {children}
    </Link>
  );
}

interface SectionHeadProps {
  title: ReactNode;
  /** Text link at the right ("Otevřít investice →"). */
  link?: { href: string; label: ReactNode };
  children?: ReactNode;
  className?: string;
}

/** `.section-head`: 17 px title for a section outside a card, optional link right. */
export function SectionHead({ title, link, children, className }: SectionHeadProps) {
  return (
    <div className={cn('mx-0.5 mb-[13px] flex items-baseline justify-between gap-4', className)}>
      <h2 className="m-0 text-h2-section text-ink">{title}</h2>
      {link && (
        <Link
          href={link.href}
          className="text-caption font-650 text-ink-2 underline-offset-[3px] hover:text-ink hover:underline"
        >
          {link.label} →
        </Link>
      )}
      {children}
    </div>
  );
}
