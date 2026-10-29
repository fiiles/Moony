/**
 * i18n Configuration
 *
 * The languages of `./languages.ts`, one namespace per domain. English is bundled (it is the
 * fallback); every other locale is a lazy chunk loaded when the language is first used.
 * Language preference is stored in localStorage for pre-auth state and synced to user profile after login.
 */
import i18n, { type BackendModule, type ResourceKey } from 'i18next';
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
import enMobile from './locales/en/mobile.json';
import enSpendingPlans from './locales/en/spending_plans.json';

import {
  LANGUAGES,
  SUPPORTED_LANGUAGES,
  getLocaleForLanguage,
  isSupportedLanguage,
  matchLanguage,
  type SupportedLanguage,
} from './languages';

export { LANGUAGES, SUPPORTED_LANGUAGES, getLocaleForLanguage, isSupportedLanguage };
export type { SupportedLanguage };

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
  'mobile',
  'spending_plans',
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
    mobile: enMobile,
    spending_plans: enSpendingPlans,
  },
};

/** Every non-English namespace file, as a lazy chunk. */
const lazyBundles = import.meta.glob<ResourceKey>(['./locales/*/*.json', '!./locales/en/*.json'], {
  import: 'default',
});

/** Reads a language's namespace from its lazy chunk; English never gets here (bundled). */
const lazyBackend: BackendModule = {
  type: 'backend',
  init: () => {},
  read(language, namespace, callback) {
    const load = lazyBundles[`./locales/${language}/${namespace}.json`];
    if (!load) {
      callback(null, {});
      return;
    }
    load().then(
      (data) => callback(null, data),
      (error: Error) => callback(error, null)
    );
  },
};

// `<html lang>` picks the CJK glyph variants and the `:lang()` typography rules.
i18n.on('languageChanged', (language) => {
  if (typeof document !== 'undefined') document.documentElement.lang = language;
});

/** Resolves once the initial language is loaded; `main.tsx` renders after it. */
export const i18nReady = i18n
  .use(lazyBackend)
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    partialBundledLanguages: true,
    supportedLngs: [...SUPPORTED_LANGUAGES],
    load: 'currentOnly',
    fallbackLng: 'en',
    defaultNS: 'common',
    ns: NAMESPACES,

    // Language detection options: the stored choice, else the system language mapped onto a
    // shipped one ("de-AT" → "de", "nn" → "nb"), else English.
    detection: {
      order: ['localStorage', 'navigator'],
      lookupLocalStorage: 'moony-language',
      caches: ['localStorage'],
      convertDetectedLanguage: (language: string) => matchLanguage(language) ?? 'en',
    },

    interpolation: {
      escapeValue: false, // React already escapes values
    },

    react: {
      useSuspense: false, // Disable suspense for smoother UX
    },
  });

// `{{value, lower}}`: lower-cases a label used mid-sentence ("za říjen 2026"). Month names inside a
// sentence come from `fmt.monthInSentence` (Intl's own casing per language), not `fmt.month`, which
// capitalises because period labels start a line; a locale string may still use `{{x, lower}}`,
// which leaves text that is already lower case alone.
i18n.services.formatter?.add('lower', (value, lng) => String(value).toLocaleLowerCase(lng));

export default i18n;

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
  // No money is formatted here: no main currency needed.
  return getFormatters(getLocaleForLanguage(lang), undefined).date(date, options);
}

/**
 * Format a number according to the current language
 */
export function formatNumberForLanguage(
  value: number,
  lang: SupportedLanguage,
  options?: Intl.NumberFormatOptions
): string {
  // No money is formatted here: no main currency needed.
  return getFormatters(getLocaleForLanguage(lang), undefined).number(value, options);
}
