/**
 * I18n Provider Component
 *
 * Provides internationalization context to the application.
 * - Syncs language preference with user profile after authentication
 * - Persists language to localStorage for pre-auth state
 * - Provides a hook for language switching in components
 */
import React, { createContext, useContext, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from '@/hooks/use-auth';
import { getFormatters } from '@/lib/format';
import './index'; // Initialize i18next
import {
  SUPPORTED_LANGUAGES,
  LANGUAGE_NAMES,
  type SupportedLanguage,
  getLocaleForLanguage,
} from './index';

interface I18nContextValue {
  /** Current language code */
  language: SupportedLanguage;
  /** Change the current language */
  setLanguage: (lang: SupportedLanguage) => void;
  /** List of supported languages */
  supportedLanguages: typeof SUPPORTED_LANGUAGES;
  /** Language display names */
  languageNames: typeof LANGUAGE_NAMES;
  /** Get locale string for Intl APIs */
  getLocale: () => string;
  /**
   * Format a local-time `Date` / ISO string according to current language.
   * Calendar days stored as UTC midnight (booking, transaction and maturity
   * dates) must go through `useFormat().day(ts)` so the day cannot shift.
   */
  formatDate: (date: Date | string, options?: Intl.DateTimeFormatOptions) => string;
  /** Format number according to current language */
  formatNumber: (value: number, options?: Intl.NumberFormatOptions) => string;
}

const I18nContext = createContext<I18nContextValue | undefined>(undefined);

interface I18nProviderProps {
  children: React.ReactNode;
}

export function I18nProvider({ children }: I18nProviderProps) {
  const { i18n } = useTranslation();
  const { user } = useAuth();

  // Sync language with user profile when it changes
  useEffect(() => {
    if (user?.language && SUPPORTED_LANGUAGES.includes(user.language as SupportedLanguage)) {
      if (i18n.language !== user.language) {
        i18n.changeLanguage(user.language);
      }
    }
  }, [user?.language, i18n]);

  const currentLanguage = (
    SUPPORTED_LANGUAGES.includes(i18n.language as SupportedLanguage) ? i18n.language : 'en'
  ) as SupportedLanguage;

  const setLanguage = useCallback(
    (lang: SupportedLanguage) => {
      if (SUPPORTED_LANGUAGES.includes(lang)) {
        i18n.changeLanguage(lang);
        // Also persist to localStorage for pre-auth state
        localStorage.setItem('moony-language', lang);
      }
    },
    [i18n]
  );

  const getLocale = useCallback(() => {
    return getLocaleForLanguage(currentLanguage);
  }, [currentLanguage]);

  const formatDate = useCallback(
    (date: Date | string, options?: Intl.DateTimeFormatOptions) => {
      return getFormatters(getLocaleForLanguage(currentLanguage)).date(date, options);
    },
    [currentLanguage]
  );

  const formatNumber = useCallback(
    (value: number, options?: Intl.NumberFormatOptions) => {
      if (value === null || value === undefined || isNaN(value)) return '';
      return getFormatters(getLocaleForLanguage(currentLanguage)).number(value, options);
    },
    [currentLanguage]
  );

  const value: I18nContextValue = {
    language: currentLanguage,
    setLanguage,
    supportedLanguages: SUPPORTED_LANGUAGES,
    languageNames: LANGUAGE_NAMES,
    getLocale,
    formatDate,
    formatNumber,
  };

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/**
 * Hook to access i18n context
 */
// The provider and its hook live together by design (every consumer imports both from here).
// eslint-disable-next-line react-refresh/only-export-components
export function useLanguage() {
  const context = useContext(I18nContext);
  if (!context) {
    throw new Error('useLanguage must be used within an I18nProvider');
  }
  return context;
}
