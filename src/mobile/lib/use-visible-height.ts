import { useEffect } from 'react';
import { visibleHeightFor } from '@/mobile/lib/visible-height';

/**
 * Keeps the phone page still while the on-screen keyboard comes and goes (spec §8.1; the lock and the
 * recovery forms are the first screens with fields).
 *
 * - Publishes the height of the part of the screen the keyboard leaves free as `--m-vh` on `<html>`.
 *   `index.css` makes `#root` a fixed box of that height and every phone screen fills it (`h-full`), so
 *   a field near the bottom scrolls above the keyboard inside its own container.
 * - Puts the document back at the top whenever it moves. WKWebView gives a `viewport-fit=cover` page a
 *   native scroll range of the safe-area insets even when nothing in it scrolls, and focusing a field
 *   (the lock focuses the password again after a wrong one) used to slide the whole page up, the nav
 *   bar under the status bar.
 * - Pinch zoom is allowed (Dynamic Type replaced the viewport lock), and a zoomed visual viewport is a
 *   smaller window onto the page, not a shorter screen: while `scale` is above 1 the box keeps its
 *   height and the page is not pinned, so the user can pan the zoomed page (`visibleHeightFor`).
 */
export function useVisibleHeight(): void {
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const root = document.documentElement;
    let frame = 0;

    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const height = visibleHeightFor(viewport.height, viewport.scale);
        // Zoomed: the box keeps the height it had.
        if (height === null) return;
        root.style.setProperty('--m-vh', `${height}px`);
        // The box has just shrunk: keep the focused field inside it.
        const focused = document.activeElement;
        if (focused instanceof HTMLElement && focused.matches('input, textarea, select')) {
          focused.scrollIntoView({ block: 'nearest' });
        }
      });
    };
    const pinToTop = () => {
      // Zoomed in, the document scrolls to follow the pan: leave it alone.
      if (visibleHeightFor(viewport.height, viewport.scale) === null) return;
      if (window.scrollX !== 0 || window.scrollY !== 0) window.scrollTo(0, 0);
    };

    viewport.addEventListener('resize', update);
    viewport.addEventListener('scroll', pinToTop);
    window.addEventListener('scroll', pinToTop);
    update();
    return () => {
      cancelAnimationFrame(frame);
      viewport.removeEventListener('resize', update);
      viewport.removeEventListener('scroll', pinToTop);
      window.removeEventListener('scroll', pinToTop);
      root.style.removeProperty('--m-vh');
    };
  }, []);
}
