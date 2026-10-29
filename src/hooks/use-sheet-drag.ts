import { useEffect, useLayoutEffect, useRef } from 'react';
import {
  releaseVelocity,
  sheetDragCommits,
  sheetDragDirection,
  sheetDragOffset,
} from '@/lib/sheet-drag';

/** The slide away after a committed drag, and the settle back after one let go too early. */
const SETTLE_MS = 200;

/** Where a touch never drags the sheet: it edits, picks or slides something of its own. */
const OWN_GESTURE =
  'input, textarea, select, [contenteditable="true"], [role="slider"], [data-sheet-drag="off"]';

interface Drag {
  y: number;
  x: number;
  active: boolean;
  dy: number;
  lastY: number;
  lastT: number;
  velocity: number;
}

/** True when something between the touch and the sheet is scrolled down: that scroll goes first. */
function scrolledAbove(target: Element, sheet: HTMLElement): boolean {
  for (let node: Element | null = target; node; node = node.parentElement) {
    if (node.scrollTop > 0 && node.scrollHeight > node.clientHeight) return true;
    if (node === sheet) break;
  }
  return false;
}

/**
 * Close a bottom sheet by dragging it down (`sheet-drag.ts`), on the phone's `Sheet` and on every
 * desktop dialog the phone shows as a sheet. A drag down anywhere on the sheet takes it while
 * nothing under the finger is scrolled (a scrolled body scrolls back first, as on iOS); a drag up
 * or sideways stays with the content, and fields, sliders and `data-sheet-drag="off"` keep their
 * own gestures. Released past a quarter of the height or flicked, the sheet slides away and
 * `onClose` runs with its exit animation switched off (it has already left); otherwise it settles.
 * `sheet` is the element (state from a callback ref: the content mounts with the opening).
 */
export function useSheetDrag(sheet: HTMLElement | null, onClose: () => void, enabled = true) {
  const close = useRef(onClose);
  useLayoutEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    if (!sheet || !enabled) return;
    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let drag: Drag | null = null;
    let timer: number | null = null;

    const place = (dy: number, settle: boolean) => {
      sheet.style.transition =
        settle && !reduced ? `transform ${SETTLE_MS}ms cubic-bezier(0.2, 0.7, 0.2, 1)` : 'none';
      sheet.style.transform = dy === 0 ? '' : `translateY(${dy}px)`;
    };

    const onStart = (event: TouchEvent) => {
      if (event.touches.length !== 1 || timer !== null) return;
      const target = event.target;
      if (!(target instanceof Element) || target.closest(OWN_GESTURE)) return;
      if (scrolledAbove(target, sheet)) return;
      const touch = event.touches[0];
      drag = {
        x: touch.clientX,
        y: touch.clientY,
        active: false,
        dy: 0,
        lastY: touch.clientY,
        lastT: event.timeStamp,
        velocity: 0,
      };
    };

    // An interrupted drag (a second finger, a cancelled touch) settles back and never closes
    const cancel = () => {
      const was = drag;
      drag = null;
      if (was?.active) place(0, true);
    };

    const onMove = (event: TouchEvent) => {
      if (!drag) return;
      if (event.touches.length > 1) return cancel();
      const touch = event.touches[0];
      const dy = touch.clientY - drag.y;
      if (!drag.active) {
        const direction = sheetDragDirection(touch.clientX - drag.x, dy);
        if (direction === null) return;
        // Sideways, or the page is already scrolling (the move can no longer be held back)
        if (direction === 'none' || !event.cancelable) {
          drag = null;
          return;
        }
        drag.active = true;
      }
      if (event.cancelable) event.preventDefault();
      const elapsed = event.timeStamp - drag.lastT;
      if (elapsed > 0) drag.velocity = (touch.clientY - drag.lastY) / elapsed;
      drag.lastY = touch.clientY;
      drag.lastT = event.timeStamp;
      drag.dy = dy;
      place(sheetDragOffset(dy), false);
    };

    const onEnd = (event: TouchEvent) => {
      if (event.touches.length > 0) return cancel();
      const done = drag;
      drag = null;
      if (!done?.active) return;
      const height = sheet.getBoundingClientRect().height;
      const velocity = releaseVelocity(done.velocity, done.lastT, event.timeStamp);
      if (sheetDragCommits(done.dy, velocity, height)) {
        place(height, true);
        timer = window.setTimeout(
          () => {
            timer = null;
            // It has left already: closing must not play the slide out again from the top
            sheet.style.animation = 'none';
            close.current();
            // A sheet that refuses to close (it is saving) comes back up. Its rise is not
            // played again: the animation stays off while it is open.
            timer = window.setTimeout(() => {
              timer = null;
              if (sheet.isConnected && sheet.dataset.state !== 'closed') place(0, true);
            }, 60);
          },
          reduced ? 0 : SETTLE_MS
        );
      } else {
        place(0, true);
      }
    };

    sheet.addEventListener('touchstart', onStart, { passive: true });
    sheet.addEventListener('touchmove', onMove, { passive: false });
    sheet.addEventListener('touchend', onEnd);
    sheet.addEventListener('touchcancel', cancel);
    return () => {
      sheet.removeEventListener('touchstart', onStart);
      sheet.removeEventListener('touchmove', onMove);
      sheet.removeEventListener('touchend', onEnd);
      sheet.removeEventListener('touchcancel', cancel);
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [sheet, enabled]);
}
