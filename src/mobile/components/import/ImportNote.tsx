import type { ReactNode } from 'react';
import { Check, Info, TriangleAlert } from 'lucide-react';
import { cn } from '@/lib/utils';

interface ImportNoteProps {
  /** `info` and `ok` are the quiet `.m-note`; `warn` is the `loss` box (`.m-box--loss`). */
  tone?: 'info' | 'ok' | 'warn';
  children: ReactNode;
  /**
   * What resolves a warning: a text button and the like, under the sentence. On a quiet note it is
   * one text button (44 pt) at the end of the line.
   */
  actions?: ReactNode;
}

/**
 * A sentence under the preview's counts: the balance check, whose account the statement is, a
 * currency that differs. A quiet `.m-note` (with a green check for "it adds up") or, when the
 * person has to look before importing, the red box of the contract (`.m-box--loss`) with the
 * action that resolves it. The text wraps; it never ends in an ellipsis. A quiet note may carry
 * one text button at its end (the learned format's "Změnit").
 */
export function ImportNote({ tone = 'info', children, actions }: ImportNoteProps) {
  if (tone === 'warn') {
    return (
      <div
        role="alert"
        className="mb-3 rounded-r2 border border-loss-line bg-loss-soft px-3 py-2.5 text-m-sub text-loss"
      >
        <p className="m-0 flex items-start gap-2">
          <TriangleAlert aria-hidden className="mt-px size-[15px] flex-none" strokeWidth={1.75} />
          <span className="min-w-0">{children}</span>
        </p>
        {actions}
      </div>
    );
  }
  const Icon = tone === 'ok' ? Check : Info;
  return (
    <div
      className={cn(
        'mb-3.5 flex min-h-11 items-center gap-2 rounded-[11px] bg-well px-3 text-m-sub text-ink-2',
        actions ? 'py-0 pr-1.5' : 'py-2'
      )}
    >
      <Icon
        aria-hidden
        className={cn('size-[15px] flex-none', tone === 'ok' ? 'text-gain' : 'text-ink-3')}
        strokeWidth={1.75}
      />
      <span className={cn('min-w-0', actions && 'flex-1 break-words py-2')}>{children}</span>
      {actions}
    </div>
  );
}
