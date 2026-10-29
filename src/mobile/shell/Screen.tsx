import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { cn } from '@/lib/utils';
import { useEdgeSwipeBack } from '@/mobile/hooks/use-edge-swipe-back';
import { navScroll, saveNavScroll, useNavEntry } from '@/mobile/hooks/use-nav-stack';
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
 * with the 16 pt gutter and room at the bottom for the fixed tab bar. A screen with a back button
 * also goes back by a swipe from the left edge (`useEdgeSwipeBack`), and a screen comes back
 * scrolled where it was left (`useKeptScroll`).
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
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLElement>(null);
  // The large title, or a 1 px marker at the top of a screen without one.
  const [marker, setMarker] = useState<HTMLElement | null>(null);
  const collapsed = useScrolledAway(scrollRef, marker);
  useEdgeSwipeBack(rootRef);
  useKeptScroll(scrollRef);

  return (
    <div
      ref={rootRef}
      className="flex h-full flex-col overflow-hidden bg-canvas bg-[radial-gradient(520px_320px_at_100%_0%,var(--canvas-glow),transparent_70%)] text-m-body text-ink-2"
    >
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
          <p className="mx-0.5 mb-3.5 mt-0 text-m-lead text-ink-3">{subtitle}</p>
        )}
        {children}
      </main>
    </div>
  );
}

/** How long a screen keeps moving to its kept offset while its content is still arriving. */
const RESTORE_MS = 1500;

/**
 * Keeps the scroll offset of the screen's stack entry (`use-nav-stack.ts`) and puts it back when
 * the screen is drawn again — after back, or under the swipe that goes back to it. Content that
 * arrives after the first paint (a query not yet in the cache) moves it on as it grows, until the
 * offset is reached, the finger touches the screen or 1.5 s pass. The swipe's underlay only reads.
 */
function useKeptScroll(scroll: RefObject<HTMLElement | null>) {
  const entry = useNavEntry();
  const index = entry?.index;
  const underlay = entry?.underlay ?? false;

  useLayoutEffect(() => {
    const el = scroll.current;
    if (!el || index === undefined) return;
    const target = navScroll(index);
    let settled = target <= 0;
    const apply = () => {
      if (settled) return;
      el.scrollTop = target;
      if (Math.abs(el.scrollTop - target) < 1) settled = true;
    };
    apply();
    const stop = () => {
      settled = true;
    };
    // Sections that arrive with their data are new nodes in the scroll area
    const observer = new MutationObserver(apply);
    observer.observe(el, { childList: true, subtree: true });
    const timer = window.setTimeout(stop, RESTORE_MS);
    el.addEventListener('touchstart', stop, { passive: true });
    el.addEventListener('wheel', stop, { passive: true });

    const save = () => {
      if (!underlay) saveNavScroll(index, el.scrollTop);
    };
    el.addEventListener('scroll', save, { passive: true });
    return () => {
      observer.disconnect();
      window.clearTimeout(timer);
      el.removeEventListener('touchstart', stop);
      el.removeEventListener('wheel', stop);
      el.removeEventListener('scroll', save);
    };
  }, [scroll, index, underlay]);
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
