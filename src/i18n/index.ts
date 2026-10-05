/**
 * i18n Configuration
 *
 * Supports English (en) and Czech (cs) languages with namespace-based organization.
 * Language preference is stored in localStorage for pre-auth state and synced to user profile after login.
 */
import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import LanguageDetector from 'i18next-browser-languagedetector';
import { getFormatters } from '@/lib/format';

// Import all translation namespaces
import enCommon from './locales/en/common.json';
import enDashboard from './locales/en/dashboard.json';
import enSettings from './locales/en/settings.json';
import enAuth from './locales/en/auth.json';
import enStocks from './locales/en/stocks.json';
import enLoans from './locales/en/loans.json';
import enCrypto from './locales/en/crypto.json';
import enBonds from './locales/en/bonds.json';
import enRealEstate from './locales/en/realEstate.json';
import enOtherAssets from './locales/en/otherAssets.json';
import enInsurance from './locales/en/insurance.json';
import enReports from './locales/en/reports.json';
import enCalculators from './locales/en/calculators.json';
import enBankAccounts from './locales/en/bank_accounts.json';
import enBudgeting from './locales/en/budgeting.json';
import enCategorization from './locales/en/categorization.json';
import enStockMonitor from './locales/en/stockMonitor.json';
import enMilestones from './locales/en/milestones.json';

import csCommon from './locales/cs/common.json';
import csDashboard from './locales/cs/dashboard.json';
import csSettings from './locales/cs/settings.json';
import csAuth from './locales/cs/auth.json';
import csStocks from './locales/cs/stocks.json';
import csLoans from './locales/cs/loans.json';
import csCrypto from './locales/cs/crypto.json';
import csBonds from './locales/cs/bonds.json';
import csRealEstate from './locales/cs/realEstate.json';
import csOtherAssets from './locales/cs/otherAssets.json';
import csInsurance from './locales/cs/insurance.json';
import csReports from './locales/cs/reports.json';
import csCalculators from './locales/cs/calculators.json';
import csBankAccounts from './locales/cs/bank_accounts.json';
import csBudgeting from './locales/cs/budgeting.json';
import csCategorization from './locales/cs/categorization.json';
import csStockMonitor from './locales/cs/stockMonitor.json';
import csMilestones from './locales/cs/milestones.json';

export const SUPPORTED_LANGUAGES = ['en', 'cs'] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

export const LANGUAGE_NAMES: Record<SupportedLanguage, { native: string; english: string }> = {
  en: { native: 'English', english: 'English (US)' },
  cs: { native: 'Čeština', english: 'Czech' },
};

// Namespace definitions for type safety
export const NAMESPACES = [
  'common',
  'dashboard',
  'settings',
  'auth',
  'stocks',
  'stockMonitor',
  'loans',
  'crypto',
  'bonds',
  'realEstate',
  'otherAssets',
  'insurance',
  'reports',
  'calculators',
  'bank_accounts',
  'budgeting',
  'categorization',
  'milestones',
] as const;

export type Namespace = (typeof NAMESPACES)[number];

const resources = {
  en: {
    common: enCommon,
    dashboard: enDashboard,
    settings: enSettings,
    auth: enAuth,
    stocks: enStocks,
    stockMonitor: enStockMonitor,
    loans: enLoans,
    crypto: enCrypto,
    bonds: enBonds,
    realEstate: enRealEstate,
    otherAssets: enOtherAssets,
    insurance: enInsurance,
    reports: enReports,
    calculators: enCalculators,
    bank_accounts: enBankAccounts,
    budgeting: enBudgeting,
    categorization: enCategorization,
    milestones: enMilestones,
  },
  cs: {
    common: csCommon,
    dashboard: csDashboard,
    settings: csSettings,
    auth: csAuth,
    stocks: csStocks,
    stockMonitor: csStockMonitor,
    loans: csLoans,
    crypto: csCrypto,
    bonds: csBonds,
    realEstate: csRealEstate,
    otherAssets: csOtherAssets,
    insurance: csInsurance,
    reports: csReports,
    calculators: csCalculators,
    bank_accounts: csBankAccounts,
    budgeting: csBudgeting,
    categorization: csCategorization,
    milestones: csMilestones,
  },
};

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: 'en',
    defaultNS: 'common',
    ns: NAMESPACES,

    // Language detection options
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: 'moony-language',
      caches: ['localStorage'],
    },

    interpolation: {
      escapeValue: false, // React already escapes values
    },

    react: {
      useSuspense: false, // Disable suspense for smoother UX
    },
  });

// `{{value, lower}}`: lower-cases a label used mid-sentence ("za říjen 2026" — `fmt.month`
// capitalises because period labels usually start a line).
i18n.services.formatter?.add('lower', (value, lng) => String(value).toLocaleLowerCase(lng));

export default i18n;

/**
 * Get the locale string for Intl APIs based on language
 */
export function getLocaleForLanguage(lang: SupportedLanguage): string {
  const localeMap: Record<SupportedLanguage, string> = {
    en: 'en-US',
    cs: 'cs-CZ',
  };
  return localeMap[lang] || 'en-US';
}

/**
 * Format a `Date` (local time zone) according to the current language.
 * Day-granular booking dates stored as UTC midnight belong in
 * `getFormatters(locale).day()` instead.
 */
export function formatDateForLanguage(
  date: Date | string,
  lang: SupportedLanguage,
  options?: Intl.DateTimeFormatOptions
): string {
  return getFormatters(getLocaleForLanguage(lang)).date(date, options);
}

/**
 * Format a number according to the current language
 */
export function formatNumberForLanguage(
  value: number,
  lang: SupportedLanguage,
  options?: Intl.NumberFormatOptions
): string {
  return getFormatters(getLocaleForLanguage(lang)).number(value, options);
}
