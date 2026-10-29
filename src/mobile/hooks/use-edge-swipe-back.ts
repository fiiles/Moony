import { useEffect, type RefObject } from 'react';
import { releaseVelocity } from '@/lib/sheet-drag';
import { edgeSwipeCommits, edgeSwipeDirection, edgeSwipeStarts } from '@/mobile/lib/edge-swipe';

/** The slide out after a committed swipe, and the snap home after one let go too early. */
const SETTLE_MS = 200;

interface Drag {
  x: number;
  y: number;
  /** Decided to be a back swipe: the screen follows the finger. */
  active: boolean;
  dx: number;
  /** The last two samples, for the release velocity. */
  lastX: number;
  lastT: number;
  velocity: number;
}

/** The screen's back button (`NavTextButton` with `chevron`), when it has one that works. */
function backButton(screen: HTMLElement): HTMLButtonElement | null {
  const button = screen.querySelector<HTMLButtonElement>('[data-nav-back]');
  return button && !button.disabled ? button : null;
}

/**
 * Back by a swipe from the left edge (`edge-swipe.ts`) on a screen with a back button: the screen
 * follows the finger and, released far enough or flicked, slides out and presses its back button,
 * so the swipe goes exactly where the button goes. Touch events, not pointer events: the move
 * listener is not passive, so a drag that has been taken for a back swipe stops the page scroll.
 * A sheet is portalled outside the screen, so it never starts one.
 */
export function useEdgeSwipeBack(ref: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const screen = ref.current;
    if (!screen) return;
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let drag: Drag | null = null;
    let timer: number | null = null;

    const place = (dx: number, settle: boolean) => {
      screen.style.transition =
        settle && !reduced ? `transform ${SETTLE_MS}ms cubic-bezier(0.2, 0.8, 0.2, 1)` : 'none';
      screen.style.transform = dx === 0 ? '' : `translateX(${dx}px)`;
      screen.style.boxShadow = dx === 0 ? '' : '-8px 0 24px var(--scrim)';
    };
    const later = (run: () => void) => {
      timer = window.setTimeout(run, reduced ? 0 : SETTLE_MS);
    };

    const onStart = (event: TouchEvent) => {
      if (event.touches.length !== 1 || timer !== null) return;
      const touch = event.touches[0];
      if (!edgeSwipeStarts(touch.clientX) || !backButton(screen)) return;
      drag = {
        x: touch.clientX,
        y: touch.clientY,
        active: false,
        dx: 0,
        lastX: touch.clientX,
        lastT: event.timeStamp,
        velocity: 0,
      };
    };

    // An interrupted swipe (a second finger, a cancelled touch) settles back and never goes back
    const cancel = () => {
      const was = drag;
      drag = null;
      if (!was?.active) return;
      place(0, true);
      later(() => {
        timer = null;
        screen.style.transition = '';
      });
    };

    const onMove = (event: TouchEvent) => {
      if (!drag) return;
      if (event.touches.length > 1) return cancel();
      const touch = event.touches[0];
      const dx = touch.clientX - drag.x;
      if (!drag.active) {
        const direction = edgeSwipeDirection(dx, touch.clientY - drag.y);
        if (direction === null) return;
        // Not a back swipe, or the page is already scrolling (the move can no longer be held back)
        if (direction === 'none' || !event.cancelable) {
          drag = null;
          return;
        }
        drag.active = true;
      }
      if (event.cancelable) event.preventDefault();
      const elapsed = event.timeStamp - drag.lastT;
      if (elapsed > 0) drag.velocity = (touch.clientX - drag.lastX) / elapsed;
      drag.lastX = touch.clientX;
      drag.lastT = event.timeStamp;
      drag.dx = Math.max(dx, 0);
      place(drag.dx, false);
    };

    const onEnd = (event: TouchEvent) => {
      if (event.touches.length > 0) return cancel();
      const done = drag;
      drag = null;
      if (!done?.active) return;
      const width = screen.getBoundingClientRect().width;
      const velocity = releaseVelocity(done.velocity, done.lastT, event.timeStamp);
      if (edgeSwipeCommits(done.dx, velocity, width)) {
        place(width, true);
        later(() => {
          timer = null;
          backButton(screen)?.click();
          // A back that lands on the same screen component keeps this element: put it home
          requestAnimationFrame(() => place(0, false));
        });
      } else {
        place(0, true);
        later(() => {
          timer = null;
          screen.style.transition = '';
        });
      }
    };

    screen.addEventListener('touchstart', onStart, { passive: true });
    screen.addEventListener('touchmove', onMove, { passive: false });
    screen.addEventListener('touchend', onEnd);
    screen.addEventListener('touchcancel', cancel);
    return () => {
      screen.removeEventListener('touchstart', onStart);
      screen.removeEventListener('touchmove', onMove);
      screen.removeEventListener('touchend', onEnd);
      screen.removeEventListener('touchcancel', cancel);
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [ref]);
}
