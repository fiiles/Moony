import { useLayoutEffect } from 'react';
import { typeScaleFrom } from '@/mobile/lib/type-scale';

/**
 * Dynamic Type (spec §10.2): publishes the iOS text size as `--m-type-scale` on `<html>`, the raw
 * factor of the default (1 at "Large"). `index.css` clamps it into `--m-text-k` (text roles, up to
 * 130 %) and `--m-value-k` (display and stat values, up to 110 %), and every `--m-fs-*` token
 * multiplies its design size by one of them.
 *
 * WKWebView exposes the size through the system font keyword `font: -apple-system-body` (17 px at
 * the default), so a hidden fixed probe is measured. Android's WebView and a desktop browser do not
 * know the keyword (`CSS.supports` is false): nothing is set and the factor stays at its default 1.
 * Neither platform sends an event when the user changes the size in Settings, so the probe is read
 * again when the app comes back to the foreground (`visibilitychange`, window `focus`).
 */
export function useTypeScale(): void {
  useLayoutEffect(() => {
    if (typeof CSS === 'undefined' || !CSS.supports('font', '-apple-system-body')) return;

    const root = document.documentElement;
    const probe = document.createElement('div');
    probe.setAttribute('aria-hidden', 'true');
    probe.style.cssText =
      'position:fixed;top:0;left:0;visibility:hidden;pointer-events:none;font:-apple-system-body;';
    document.body.appendChild(probe);

    const measure = () => {
      const bodyPx = parseFloat(getComputedStyle(probe).fontSize);
      root.style.setProperty('--m-type-scale', String(typeScaleFrom(bodyPx)));
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') measure();
    };

    measure();
    document.addEventListener('visibilitychange', onVisibility);
    window.addEventListener('focus', measure);
    return () => {
      document.removeEventListener('visibilitychange', onVisibility);
      window.removeEventListener('focus', measure);
      probe.remove();
      root.style.removeProperty('--m-type-scale');
    };
  }, []);
}
