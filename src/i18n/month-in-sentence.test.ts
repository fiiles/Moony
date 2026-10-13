import { createInstance } from 'i18next';
import { describe, expect, it } from 'vitest';
import { createFormatters } from '@/lib/format';
import { LANGUAGES } from './languages';

/**
 * The sentences that receive a month name from `fmt.monthInSentence` (docs/standards/i18n.md
 * rule 4): the month must come out exactly as Intl writes it for the language, whether or not
 * the locale string runs it through `{{x, lower}}` ("septiembre de 2026", "září 2026",
 * "September 2026", "2026年9月"). Add a key here when a new call site interpolates a month.
 */
const SENTENCES = [
  ['realEstate', 'detail.loans.principalUntil', 'date'],
  ['realEstate', 'detail.loans.fixedUntil', 'date'],
  ['insurance', 'calendar.peakNote', 'month'],
  ['budgeting', 'readiness.scheduledLimit', 'month'],
  ['budgeting', 'readiness.scheduledRemoval', 'month'],
  ['budgeting', 'readiness.effectiveMonthPrefill', 'month'],
  ['bank_accounts', 'flows.netIn', 'month'],
] as const;

const files = import.meta.glob<Record<string, unknown>>('./locales/*/*.json', {
  eager: true,
  import: 'default',
});

const september = new Date(Date.UTC(2026, 8, 1));

describe.each(LANGUAGES)('$code: a month name inside a sentence', ({ code, locale }) => {
  it.each(SENTENCES)('%s %s keeps the casing Intl uses', async (namespace, key, variable) => {
    const i18n = createInstance();
    await i18n.init({
      lng: code,
      fallbackLng: false,
      ns: [namespace],
      defaultNS: namespace,
      resources: { [code]: { [namespace]: files[`./locales/${code}/${namespace}.json`] } },
      interpolation: { escapeValue: false },
    });
    // The same one-liner as src/i18n/index.ts (that module also starts the app's language detector).
    i18n.services.formatter?.add('lower', (value, lng) => String(value).toLocaleLowerCase(lng));

    const month = createFormatters(locale, undefined).monthInSentence(september);
    const text = i18n.t(key, { [variable]: month, amount: '100' });

    expect(text, `${code}/${namespace}.json ${key}`).toContain(month);
  });
});
