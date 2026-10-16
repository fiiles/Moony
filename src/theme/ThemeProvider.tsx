/**
 * Theme Provider Component
 *
 * Owns the theme the app renders in.
 * - Resolves the stored preference (`system` follows the OS) into `light` or `dark`
 * - Stamps `data-theme` on <html>, which switches the token block in src/index.css
 * - Resets the chart token cache on every switch and bumps `themeVersion`, so charts that read
 *   their colors through `useChartTokens()` re-render in the new palette
 * - Drives the native window theme and background, and shows the window after the first stamp
 *
 * The preference is a device setting (localStorage, read before the profile is unlocked); its
 * profile sync comes in a later task and gets its own effect below.
 */
import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { chartTokens, resetTokenCache, type ChartTokens } from '@/lib/tokens';
import {
  readStoredPreference,
  resolveTheme,
  writeStoredPreference,
  type ResolvedTheme,
  type ThemePreference,
} from '@/utils/theme';
import {
  applyWindowBackground,
  applyWindowTheme,
  onSystemThemeChange,
  showWindow,
  systemPrefersDark,
} from './window-theme';

interface ThemeContextValue {
  /** The user's choice: follow the OS, or light, or dark */
  preference: ThemePreference;
  /** What is on screen right now */
  resolved: ResolvedTheme;
  /** Increments on every theme switch; the dependency of values read from the stylesheet */
  themeVersion: number;
  /** Change the preference; persists it on this device */
  setPreference: (pref: ThemePreference) => void;
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

interface ThemeProviderProps {
  children: React.ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  const [preference, setPreferenceState] = useState<ThemePreference>(() => readStoredPreference());
  const [systemDark, setSystemDark] = useState(systemPrefersDark);
  const [themeVersion, setThemeVersion] = useState(0);
  const windowShown = useRef(false);

  const resolved = resolveTheme(preference, systemDark);

  // Native window theme first (`system` hands the appearance back to the OS), then re-read the
  // media query: an earlier explicit choice made the webview report the override, not the OS.
  useEffect(() => {
    let stale = false;
    void applyWindowTheme(preference).then(() => {
      if (!stale) setSystemDark(systemPrefersDark());
    });
    return () => {
      stale = true;
    };
  }, [preference]);

  // Follow the OS appearance only while the preference says `system`.
  useEffect(() => {
    if (preference !== 'system') return;
    return onSystemThemeChange(() => setSystemDark(systemPrefersDark()));
  }, [preference]);

  // Stamp, then reset the cache, then read the new palette: this order keeps the window
  // background (and every chart) from picking up the old theme's colors.
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', resolved);
    resetTokenCache();
    // The bump is the point of this effect: it re-renders the charts after the stamp and the reset.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setThemeVersion((v) => v + 1);
    void applyWindowBackground(resolved);
  }, [resolved]);

  // The window starts hidden; show it once, after the first stamp above (a second call under
  // React strict mode would be harmless, the flag only saves the IPC round trip).
  useEffect(() => {
    if (windowShown.current) return;
    windowShown.current = true;
    void showWindow();
  }, []);

  const setPreference = useCallback((pref: ThemePreference) => {
    setPreferenceState(pref);
    writeStoredPreference(pref);
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({ preference, resolved, themeVersion, setPreference }),
    [preference, resolved, themeVersion, setPreference]
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

/**
 * Hook to access the theme context
 */
// The provider and its hooks live together by design (every consumer imports them from here).
// eslint-disable-next-line react-refresh/only-export-components
export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useTheme must be used within a ThemeProvider');
  }
  return context;
}

/**
 * The chart palette as strings for Recharts and canvas code. Re-reads the stylesheet when the
 * theme switches, so a chart that takes its colors from here repaints in the new palette.
 */
// eslint-disable-next-line react-refresh/only-export-components
export function useChartTokens(): ChartTokens {
  const { themeVersion } = useTheme();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- themeVersion is the invalidation key
  return useMemo(() => chartTokens(), [themeVersion]);
}
