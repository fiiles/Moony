import { describe, expect, it } from 'vitest';
import { LANGUAGES, getLocaleForLanguage, isSupportedLanguage, matchLanguage } from './languages';

describe('LANGUAGES', () => {
  it('lists English first, then the rest alphabetically by native name', () => {
    const [first, ...rest] = LANGUAGES.map((l) => l.native);
    expect(first).toBe('English');
    expect(rest).toEqual([...rest].sort((a, b) => a.localeCompare(b, 'en')));
  });

  it('formats each language with its own region', () => {
    expect(getLocaleForLanguage('nb')).toBe('nb-NO');
    expect(getLocaleForLanguage('zh')).toBe('zh-CN');
    expect(getLocaleForLanguage('cs')).toBe('cs-CZ');
  });
});

describe('isSupportedLanguage', () => {
  it('accepts shipped codes only', () => {
    expect(isSupportedLanguage('pl')).toBe(true);
    expect(isSupportedLanguage('ar')).toBe(false);
    expect(isSupportedLanguage(undefined)).toBe(false);
  });
});

describe('matchLanguage', () => {
  it.each([
    ['de-AT', 'de'],
    ['es-MX', 'es'],
    ['nn-NO', 'nb'],
    ['no', 'nb'],
    ['nb_NO', 'nb'],
    ['zh-Hant-TW', 'zh'],
    ['ja', 'ja'],
    ['EN-gb', 'en'],
  ])('%s → %s', (tag, code) => {
    expect(matchLanguage(tag)).toBe(code);
  });

  it.each(['pt-BR', 'ar', '', null, undefined])('%s → null', (tag) => {
    expect(matchLanguage(tag)).toBeNull();
  });
});
