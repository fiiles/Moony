/*
 * Runs before the stylesheet applies so the first paint has the right theme (spec D10).
 * Mirrors `src/utils/theme.ts` resolveTheme; tested by `theme-boot.test.ts`.
 * Plain ES5 in one IIFE: it is loaded as a classic blocking script and cannot be bundled.
 */
(function () {
  var theme = 'light';
  try {
    var pref = localStorage.getItem('moony-theme');
    var dark =
      pref === 'dark' || (pref !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
    theme = dark ? 'dark' : 'light';
  } catch (e) {
    // Blocked storage or no matchMedia: fall back to the light theme.
  }
  document.documentElement.setAttribute('data-theme', theme);
})();
