import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useEdgeSwipeBack } from '@/mobile/hooks/use-edge-swipe-back';
import { PULL_INDICATOR_PX, usePullToRefresh } from '@/mobile/hooks/use-pull-to-refresh';
import { navScroll, saveNavScroll, useNavEntry } from '@/mobile/hooks/use-nav-stack';
import { SCROLL_TOP_EVENT } from '@/mobile/hooks/use-tab-memory';
import { NavBar } from './NavBar';

interface ScreenProps {
  title: string;
  /**
   * The bar's title once the large one has scrolled away, when it is not the large title itself:
   * Přehled's greeting is a sentence, its bar says the tab's name (audit 2026-11-15 C9, B4).
   */
  barTitle?: string;
  /**
   * A 30 pt large title in the content that collapses into the nav bar on scroll: a tab root, and a
   * pushed detail screen (README §13, "Detail screens"), whose name may be long and so wraps here
   * instead of being cut between the back button and `···`.
   */
  large?: boolean;
  /**
   * The one sentence under the large title (`.m-title-sub`, 13 / 450 `ink-3`); a detail screen
   * draws its sub-line with `DetailSub` instead, under its loaded record.
   */
  subtitle?: ReactNode;
  leading?: ReactNode;
  trailing?: ReactNode;
  /** False on screens outside the tabs (the lock's recovery): no room is kept for the tab bar. */
  tabBar?: boolean;
  /**
   * Pull to refresh (spec §8.8): what a pull from the top does — refetch what the screen shows, or
   * refresh prices. The spinner stays until its promise settles. Without it the screen does not pull.
   */
  onRefresh?: () => Promise<unknown> | unknown;
  children?: ReactNode;
}

/**
 * One phone screen (design system README §13, `.m-screen`): the nav bar over one scroll area
 * with the 16 pt gutter and room at the bottom for the fixed tab bar. A screen with a back button
 * also goes back by a swipe from the left edge (`useEdgeSwipeBack`), and a screen comes back
 * scrolled where it was left (`useKeptScroll`). A tap on the tab a root belongs to scrolls it to
 * its top (`SCROLL_TOP_EVENT`). A screen given `onRefresh` refreshes by a pull from its top
 * (`usePullToRefresh`), with the indicator in the room the content leaves and a live region that
 * says it is refreshing.
 */
export function Screen({
  title,
  barTitle,
  large = false,
  subtitle,
  leading,
  trailing,
  tabBar = true,
  onRefresh,
  children,
}: ScreenProps) {
  const { t } = useTranslation('mobile');
  const rootRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLElement>(null);
  const indicatorRef = useRef<HTMLDivElement>(null);
  const underlay = useNavEntry()?.underlay ?? false;
  const refreshing = usePullToRefresh({
    scroll: scrollRef,
    indicator: indicatorRef,
    onRefresh,
    enabled: !underlay,
  });
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
        title={barTitle ?? title}
        large={large}
        collapsed={collapsed}
        leading={leading}
        trailing={trailing}
      />
      <div className="relative flex min-h-0 flex-1 flex-col">
        {onRefresh && (
          <>
            {/* Rides in the room the pulled content leaves; turns with the pull, spins while it runs */}
            <div
              ref={indicatorRef}
              aria-hidden
              className="pointer-events-none absolute left-1/2 top-0 -ml-[14px] flex items-center justify-center text-ink-3 opacity-0 data-[refreshing=true]:[&>svg]:animate-spin motion-reduce:data-[refreshing=true]:[&>svg]:animate-none"
              style={{ width: PULL_INDICATOR_PX, height: PULL_INDICATOR_PX }}
            >
              <LoaderCircle className="size-[22px]" strokeWidth={2} />
            </div>
            <span role="status" className="sr-only">
              {refreshing ? t('pull.refreshing') : ''}
            </span>
          </>
        )}
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
              // Holds its line while a detail's record loads, so the content does not jump
              // Wraps between words, never hyphenated or cut (B4)
              className="mx-0.5 mb-[3px] mt-1.5 min-h-[1.2em] break-words text-m-title-lg text-ink [hyphens:manual]"
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

    // A tap on the tab shown, at its root: back to the top (the swipe's underlay stays put)
    const toTop = () => {
      if (underlay) return;
      settled = true;
      const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      el.scrollTo({ top: 0, behavior: reduced ? 'auto' : 'smooth' });
    };
    window.addEventListener(SCROLL_TOP_EVENT, toTop);
    return () => {
      window.removeEventListener(SCROLL_TOP_EVENT, toTop);
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
