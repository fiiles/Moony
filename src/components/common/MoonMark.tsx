import { cn } from '@/lib/utils';

/**
 * The brand mark (design system §5, `.moon`): a dark disc with the glow
 * crescent. 31 px in the sidebar, 36 px on the lock screen.
 */
export function MoonMark({ size = 31, className }: { size?: 31 | 36; className?: string }) {
  const large = size === 36;
  return (
    <span
      aria-hidden
      className={cn(
        'relative block shrink-0 rounded-full bg-dark shadow-[inset_0_1px_rgba(255,255,255,0.2),0_2px_5px_rgba(33,30,24,0.15)]',
        large ? 'size-9' : 'size-[31px]',
        className
      )}
    >
      <span
        className={cn(
          'absolute block rounded-full bg-canvas-glow',
          large
            ? 'left-[9px] top-[7px] size-4 shadow-[5px_1px_0_var(--dark)]'
            : 'left-2 top-1.5 size-3.5 shadow-[4px_1px_0_var(--dark)]'
        )}
      />
    </span>
  );
}
