import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';

interface InlineAlertAction {
  label: string;
  onClick: () => void;
  disabled?: boolean;
}

interface InlineAlertProps {
  icon: LucideIcon;
  /** The quiet variant: no border (a note that asks for nothing: a hand-entered balance holds). */
  muted?: boolean;
  /** A bold first line above the sentence, 12 / 600 `ink` (a pending split: "Split 10:1 k 3. 6."). */
  title?: string;
  /**
   * Text actions under the sentence, 44 pt targets: one, or a decision of two side by side that
   * wrap under each other when they do not fit (a pending split: the emphasised one first).
   */
  action?: InlineAlertAction | readonly InlineAlertAction[];
  children: ReactNode;
}

/**
 * An inline alert (README §13, `.m-alert`): a sentence next to its cause on `well`, radius 11, 12 /
 * 500 `ink-2`, a 15 pt glyph in `ink-3`, at least 44 pt tall, wrapping. It may start with a bold
 * title and end with one text action or two. A warning has the hairline border (the desktop's
 * alert), a note has none (`muted`). The loan's warnings use it, and a position's pending splits.
 */
export function InlineAlert({
  icon: Icon,
  muted = false,
  title,
  action,
  children,
}: InlineAlertProps) {
  const actions = action === undefined ? [] : 'label' in action ? [action] : action;
  return (
    <div
      // `status`, not `alert`: the alert is part of the page as it opens (a pending split, a loan's
      // fixation), not a sudden event, so VoiceOver reads it in turn instead of interrupting
      role={muted ? 'note' : 'status'}
      className={cn(
        'mb-2.5 flex min-h-11 items-start gap-2.5 rounded-[11px] border bg-well px-3 py-2.5 text-m-sub text-ink-2',
        muted ? 'border-transparent' : 'border-line'
      )}
    >
      <Icon aria-hidden className="mt-px size-[15px] flex-none text-ink-3" strokeWidth={1.75} />
      <div className="min-w-0 leading-[1.4]">
        {title && <p className="m-0 font-semibold text-ink">{title}</p>}
        <p className="m-0">{children}</p>
        {actions.length > 0 && (
          <div className="-mb-2.5 mt-0.5 flex flex-wrap gap-x-4">
            {actions.map((a) => (
              <button
                key={a.label}
                type="button"
                onClick={a.onClick}
                disabled={a.disabled}
                className="inline-flex min-h-11 items-center text-left text-m-sub font-semibold text-ink [-webkit-touch-callout:none] active:text-ink-3 disabled:pointer-events-none disabled:opacity-45"
              >
                {a.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
