/**
 * The UI languages Moony ships, in picker order: English first, then the rest
 * alphabetically by native name. Each language carries the Intl locale its
 * numbers, dates and money are formatted with ("Language and formats").
 */
export const LANGUAGES = [
  { code: 'en', native: 'English', english: 'English', locale: 'en-US' },
  { code: 'cs', native: 'Čeština', english: 'Czech', locale: 'cs-CZ' },
  { code: 'de', native: 'Deutsch', english: 'German', locale: 'de-DE' },
  { code: 'es', native: 'Español', english: 'Spanish', locale: 'es-ES' },
  { code: 'it', native: 'Italiano', english: 'Italian', locale: 'it-IT' },
  { code: 'nb', native: 'Norsk bokmål', english: 'Norwegian', locale: 'nb-NO' },
  { code: 'pl', native: 'Polski', english: 'Polish', locale: 'pl-PL' },
  { code: 'sv', native: 'Svenska', english: 'Swedish', locale: 'sv-SE' },
  { code: 'ja', native: '日本語', english: 'Japanese', locale: 'ja-JP' },
  { code: 'zh', native: '简体中文', english: 'Chinese (Simplified)', locale: 'zh-CN' },
] as const;

export type LanguageInfo = (typeof LANGUAGES)[number];
export type SupportedLanguage = LanguageInfo['code'];

export const SUPPORTED_LANGUAGES: readonly SupportedLanguage[] = LANGUAGES.map((l) => l.code);

export function isSupportedLanguage(value: unknown): value is SupportedLanguage {
  return typeof value === 'string' && (SUPPORTED_LANGUAGES as readonly string[]).includes(value);
}

export function languageInfo(code: SupportedLanguage): LanguageInfo {
  return LANGUAGES.find((l) => l.code === code) ?? LANGUAGES[0];
}

/** Intl locale of a UI language ("de" → "de-DE"). */
export function getLocaleForLanguage(code: SupportedLanguage): string {
  return languageInfo(code).locale;
}

/** Primary subtags that name a supported language under another code. */
const ALIASES: Record<string, SupportedLanguage> = { no: 'nb', nn: 'nb' };

/**
 * The supported language a BCP 47 tag asks for ("de-AT" → "de", "nn-NO" → "nb",
 * "zh-Hant-TW" → "zh"), or null when Moony does not ship it.
 */
export function matchLanguage(tag: string | null | undefined): SupportedLanguage | null {
  const primary = tag?.trim().toLowerCase().split(/[-_]/)[0];
  if (!primary) return null;
  const code = ALIASES[primary] ?? primary;
  return isSupportedLanguage(code) ? code : null;
}
