/*
 * Runs before the stylesheet applies so the first paint has the right theme (spec D10).
 * Mirrors `src/utils/theme.ts` resolveTheme; tested by `theme-boot.test.ts`.
 * Plain ES5 in one IIFE: it is loaded as a classic blocking script and cannot be bundled.
 */
(function () {
  var pref = null;
  try {
    pref = localStorage.getItem('moony-theme');
  } catch (e) {
    // Blocked storage: the preference is unknown, which follows the OS like System does.
  }
  var dark = pref === 'dark';
  if (!dark && pref !== 'light') {
    try {
      dark = matchMedia('(prefers-color-scheme: dark)').matches;
    } catch (e) {
      // No matchMedia: fall back to the light theme.
    }
  }
  document.documentElement.setAttribute('data-theme', dark ? 'dark' : 'light');
})();
