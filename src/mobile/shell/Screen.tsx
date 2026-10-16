import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { cn } from '@/lib/utils';
import { NavBar } from './NavBar';

interface ScreenProps {
  title: string;
  /** Tab root: a 30 pt large title in the content that collapses into the nav bar on scroll. */
  large?: boolean;
  /** The one sentence under the large title (`.m-title-sub`, 13 / 450 `ink-3`). */
  subtitle?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
  /** False on screens outside the tabs (the lock's recovery): no room is kept for the tab bar. */
  tabBar?: boolean;
  children?: ReactNode;
}

/**
 * One phone screen (design system README §13, `.m-screen`): the nav bar over one scroll area
 * with the 16 pt gutter and room at the bottom for the fixed tab bar.
 */
export function Screen({
  title,
  large = false,
  subtitle,
  leading,
  trailing,
  tabBar = true,
  children,
}: ScreenProps) {
  const scrollRef = useRef<HTMLElement>(null);
  // The large title, or a 1 px marker at the top of a screen without one.
  const [marker, setMarker] = useState<HTMLElement | null>(null);
  const collapsed = useScrolledAway(scrollRef, marker);

  return (
    <div className="flex h-full flex-col overflow-hidden bg-canvas bg-[radial-gradient(520px_320px_at_100%_0%,var(--canvas-glow),transparent_70%)] text-m-body text-ink-2">
      <NavBar
        title={title}
        large={large}
        collapsed={collapsed}
        leading={leading}
        trailing={trailing}
      />
      <main
        ref={scrollRef}
        className={cn(
          'min-h-0 flex-1 overflow-y-auto overscroll-contain px-4',
          tabBar
            ? 'pb-[calc(var(--m-tabbar-h)+var(--safe-bottom)+24px)]'
            : 'pb-[calc(var(--safe-bottom)+24px)]'
        )}
      >
        {large ? (
          <h1
            ref={setMarker}
            className="mx-0.5 mb-[3px] mt-1.5 hyphens-auto break-words text-m-title-lg text-ink"
          >
            {title}
          </h1>
        ) : (
          <div ref={setMarker} aria-hidden className="h-px" />
        )}
        {large && subtitle && (
          <p className="mx-0.5 mb-3.5 mt-0 text-[13px] font-450 leading-[1.4] text-ink-3">
            {subtitle}
          </p>
        )}
        {children}
      </main>
    </div>
  );
}

/** True once `target` has scrolled out of the `root` scroll area. */
function useScrolledAway(root: RefObject<HTMLElement | null>, target: HTMLElement | null): boolean {
  const [away, setAway] = useState(false);
  useEffect(() => {
    if (!root.current || !target) return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) setAway(!entry.isIntersecting);
      },
      { root: root.current }
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [root, target]);
  return away;
}
