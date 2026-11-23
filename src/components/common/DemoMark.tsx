import { Avatar, AvatarFallback } from '@/components/ui/avatar';

/**
 * The demo profile's one motif (design system §6, "Sample mark"): a dashed outline means "sample,
 * not yours". The badge, the card around the sidebar's note and the top-bar button, and the
 * avatar carry it; no new color, green and red keep meaning gain and loss only.
 */

/** The dashed outline of a sample card or button (`line-hover`, 1 px). */
export const DEMO_OUTLINE = 'border border-dashed border-line-hover';

/** The avatar of a sample profile: initials in a dashed ring instead of the filled disc. */
export function DemoAvatar({ initials, className }: { initials: string; className?: string }) {
  return (
    <Avatar className={className}>
      <AvatarFallback className="border border-dashed border-ink-5 bg-transparent text-ink-2 shadow-none">
        {initials}
      </AvatarFallback>
    </Avatar>
  );
}
