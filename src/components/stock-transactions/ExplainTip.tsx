import { useRef, useState, type ReactNode } from 'react';
import { Info } from 'lucide-react';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

type TipState = 'closed' | 'open' | 'pinned';

/**
 * The ⓘ beside a control or a label with an explanatory tooltip (spec D7, the wide wrapping
 * variant of `.tip`): it opens on hover and on keyboard focus, a click pins it open, and Esc, a
 * second click or a click anywhere else closes it. `label` names what it explains for assistive
 * tech; `children` is the explanation (`ExplainTitle` and `ExplainText`).
 */
export function ExplainTip({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  const [state, setState] = useState<TipState>('closed');
  const triggerRef = useRef<HTMLButtonElement>(null);

  return (
    <Tooltip
      open={state !== 'closed'}
      // Hover, focus, leave and blur: a pinned tip ignores them
      onOpenChange={(open) => setState((s) => (s === 'pinned' ? s : open ? 'open' : 'closed'))}
    >
      <TooltipTrigger asChild>
        <button
          ref={triggerRef}
          type="button"
          aria-label={label}
          aria-expanded={state === 'pinned'}
          // Radix closes a tooltip on pointer down and on click; the click toggles the pin instead
          onPointerDown={(event) => event.preventDefault()}
          onClick={(event) => {
            event.preventDefault();
            setState((s) => (s === 'pinned' ? 'closed' : 'pinned'));
          }}
          className={cn(
            'inline-grid size-[18px] shrink-0 cursor-help place-items-center rounded-full align-[-4px] text-ink-4 transition-colors duration-fast hover:bg-ink-hover hover:text-ink focus-visible:outline-none focus-visible:shadow-focus aria-expanded:bg-ink-hover aria-expanded:text-ink',
            className
          )}
        >
          <Info aria-hidden className="size-[14px]" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="bottom"
        collisionPadding={12}
        onEscapeKeyDown={() => setState('closed')}
        onPointerDownOutside={(event) => {
          // The trigger's own click toggles the pin
          if (triggerRef.current?.contains(event.target as Node)) {
            event.preventDefault();
            return;
          }
          setState('closed');
        }}
        className="w-[304px] max-w-[calc(100vw-24px)] whitespace-normal rounded-[9px] px-[13px] py-3 text-caption font-500 leading-[1.5] text-ink-inverse-2"
      >
        <div>{children}</div>
      </TooltipContent>
    </Tooltip>
  );
}

/** The bold first line of an explanation. */
export function ExplainTitle({ children }: { children: ReactNode }) {
  return <span className="mb-1 block text-table font-650 text-ink-inverse">{children}</span>;
}

/** One paragraph of an explanation. */
export function ExplainText({ children }: { children: ReactNode }) {
  return <span className="mb-[7px] block last:mb-0">{children}</span>;
}

/** The emphasis inside an explanation's text: `components={{ em: <ExplainEm /> }}` of a `<Trans>`. */
export function ExplainEm({ children }: { children?: ReactNode }) {
  return <em className="font-600 not-italic text-ink-inverse">{children}</em>;
}
