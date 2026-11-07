import { useEffect, useRef, useState, type RefObject } from 'react';
import { selectionTick } from '@/mobile/lib/haptics';
import { edgeSwipeStarts } from '@/mobile/lib/edge-swipe';
import {
  PULL_IDLE,
  pullOffset,
  pullProgress,
  pullReducer,
  type PullEvent,
  type PullState,
} from '@/mobile/lib/pull-refresh';

/** The settle back and the move to the waiting place. */
const SETTLE_MS = 220;
/** A refresh shows its spinner at least this long, so a quick one is still seen to happen. */
const MIN_REFRESH_MS = 450;
/** The indicator's size; it rides just above the content's top edge. */
export const PULL_INDICATOR_PX = 28;

interface PullOptions {
  /** The screen's scroll area: the gesture starts only when it is at its top. */
  scroll: RefObject<HTMLElement | null>;
  /** The indicator drawn in the room the content leaves. */
  indicator: RefObject<HTMLElement | null>;
  /** What a pull does; `undefined` turns the gesture off. Its promise ends the spinner. */
  onRefresh: (() => Promise<unknown> | unknown) | undefined;
  /** False for a screen that only shows under the back swipe. */
  enabled: boolean;
}

/**
 * Pull to refresh on a screen's scroll area (`pull-refresh.ts`): at the top, a downward drag moves
 * the scroll area down against a growing resistance with the indicator turning in the room above
 * it; a tick says the release will refresh; a release past the trigger keeps the content a little
 * lower with a spinning indicator until `onRefresh` has finished. The styles are set on the
 * elements directly, so a drag does not render the screen on every move. Touch events, not
 * pointer events: the move listener is not passive, so a drag taken for a pull stops the
 * WebView's own bounce. A drag from the left edge is the back swipe's (`useEdgeSwipeBack`), and a
 * sheet is portalled outside the screen, so neither starts a pull. Returns whether a refresh runs
 * (for the live region).
 */
export function usePullToRefresh({ scroll, indicator, onRefresh, enabled }: PullOptions): boolean {
  const [refreshing, setRefreshing] = useState(false);
  // The handler of the render the touch ends in, without re-binding the listeners
  const refresh = useRef(onRefresh);
  useEffect(() => {
    refresh.current = onRefresh;
  });
  const on = enabled && onRefresh !== undefined;

  useEffect(() => {
    const el = scroll.current;
    if (!el || !on) return;
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let state: PullState = PULL_IDLE;
    let armed = false;
    let alive = true;

    const paint = (settle: boolean) => {
      const offset = pullOffset(state);
      const progress = pullProgress(state);
      el.style.transition =
        settle && !reduced ? `transform ${SETTLE_MS}ms cubic-bezier(0.2, 0.8, 0.2, 1)` : 'none';
      el.style.transform = offset === 0 ? '' : `translateY(${offset}px)`;
      const mark = indicator.current;
      if (!mark) return;
      mark.style.transition = el.style.transition.replace('transform', 'transform, opacity');
      mark.style.opacity = String(progress);
      // Centred in the room above the content; it turns with the pull and spins while refreshing
      const y = Math.max(0, (offset - PULL_INDICATOR_PX) / 2);
      mark.style.transform = `translateY(${y}px) rotate(${Math.round(progress * 300)}deg)`;
      mark.dataset.refreshing = state.kind === 'refreshing' ? 'true' : 'false';
    };
    const dispatch = (event: PullEvent) => {
      state = pullReducer(state, event);
    };

    const onStart = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (event.touches.length !== 1 || !touch || edgeSwipeStarts(touch.clientX)) {
        // A second finger or the back swipe: no pull until every finger has lifted
        if (state.kind !== 'refreshing') {
          const wasPulling = state.kind === 'pulling';
          state = { kind: 'ignored' };
          if (wasPulling) paint(true);
        }
        return;
      }
      armed = false;
      dispatch({ type: 'start', x: touch.clientX, y: touch.clientY, atTop: el.scrollTop <= 0 });
    };

    const onMove = (event: TouchEvent) => {
      const touch = event.touches[0];
      if (!touch || (state.kind !== 'tracking' && state.kind !== 'pulling')) return;
      dispatch({ type: 'move', x: touch.clientX, y: touch.clientY });
      if (state.kind !== 'pulling') return;
      if (event.cancelable) event.preventDefault();
      if (state.armed && !armed) selectionTick();
      armed = state.armed;
      paint(false);
    };

    const onEnd = (event: TouchEvent) => {
      if (event.touches.length > 0) return;
      const before = state.kind;
      dispatch({ type: 'end' });
      if (state.kind !== 'refreshing') {
        if (before === 'pulling') paint(true);
        return;
      }
      if (before === 'refreshing') return;
      paint(true);
      setRefreshing(true);
      const started = Date.now();
      Promise.resolve()
        .then(() => refresh.current?.())
        .catch(() => undefined)
        .then(
          () =>
            new Promise((resolve) => setTimeout(resolve, MIN_REFRESH_MS - (Date.now() - started)))
        )
        .then(() => {
          if (!alive) return;
          dispatch({ type: 'done' });
          paint(true);
          setRefreshing(false);
        });
    };

    const onCancel = () => {
      if (state.kind === 'refreshing') return;
      const wasPulling = state.kind === 'pulling';
      state = PULL_IDLE;
      if (wasPulling) paint(true);
    };

    el.addEventListener('touchstart', onStart, { passive: true });
    el.addEventListener('touchmove', onMove, { passive: false });
    el.addEventListener('touchend', onEnd, { passive: true });
    el.addEventListener('touchcancel', onCancel, { passive: true });
    return () => {
      alive = false;
      el.removeEventListener('touchstart', onStart);
      el.removeEventListener('touchmove', onMove);
      el.removeEventListener('touchend', onEnd);
      el.removeEventListener('touchcancel', onCancel);
      el.style.transform = '';
      el.style.transition = '';
    };
  }, [scroll, indicator, on]);

  return refreshing;
}
