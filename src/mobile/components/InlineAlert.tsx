import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

interface InlineAlertProps {
  icon: LucideIcon;
  /** The quiet variant: no border (a note that asks for nothing: a hand-entered balance holds). */
  muted?: boolean;
  /** A text action under the sentence: a 44 pt target. */
  action?: { label: string; onClick: () => void };
  children: ReactNode;
}

/**
 * An inline alert (README §13, `.m-alert`): a sentence next to its cause on `well`, radius 11, 12 /
 * 500 `ink-2`, a 15 pt glyph in `ink-3`, at least 44 pt tall, wrapping. It may end with one text
 * action. A warning has the hairline border (the desktop's alert), a note has none (`muted`). The
 * loan's warnings use it; the policy and the property detail reuse it.
 */
export function InlineAlert({ icon: Icon, muted = false, action, children }: InlineAlertProps) {
  return (
    <div
      role={muted ? 'note' : 'alert'}
      className={cn(
        'mb-2.5 flex min-h-11 items-start gap-2.5 rounded-[11px] border bg-well px-3 py-2.5 text-m-sub text-ink-2',
        muted ? 'border-transparent' : 'border-line'
      )}
    >
      <Icon aria-hidden className="mt-px size-[15px] flex-none text-ink-3" strokeWidth={1.75} />
      <div className="min-w-0 leading-[1.4]">
        <p className="m-0">{children}</p>
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="-mb-2.5 mt-0.5 inline-flex min-h-11 items-center text-m-sub font-semibold text-ink [-webkit-touch-callout:none] active:text-ink-3"
          >
            {action.label}
          </button>
        )}
      </div>
    </div>
  );
}
