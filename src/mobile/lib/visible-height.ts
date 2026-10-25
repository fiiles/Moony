/**
 * The height `--m-vh` should publish for the visual viewport, or `null` when it must stay as it is.
 *
 * While the page is pinch-zoomed (`scale` above 1.01; a hair of overshoot while a zoom settles is
 * not zoom) the visual viewport is a smaller window onto the layout viewport, not a shorter screen:
 * following it would shrink the app box and strand the tab bar mid-page. Only the on-screen
 * keyboard shortens the box.
 */
export function visibleHeightFor(height: number, scale: number): number | null {
  if (scale > 1.01) return null;
  return Math.round(height);
}
