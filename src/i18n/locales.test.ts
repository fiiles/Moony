import { describe, expect, it } from 'vitest';
import { SUPPORTED_LANGUAGES } from './languages';
import { compareNamespace, pluralForms, type LocaleTree } from './locale-parity';

/**
 * Every locale carries every English string (docs/standards/i18n.md rule 1):
 * same keys, the plural forms of its language, the same {{variables}}.
 */
const files = import.meta.glob<LocaleTree>('./locales/*/*.json', {
  eager: true,
  import: 'default',
});

const namespaces = Object.keys(files)
  .filter((path) => path.startsWith('./locales/en/'))
  .map((path) => path.slice('./locales/en/'.length, -'.json'.length))
  .sort();

describe.each(SUPPORTED_LANGUAGES.filter((language) => language !== 'en'))(
  '%s locale',
  (language) => {
    it.each(namespaces)('%s matches English', (namespace) => {
      const target = files[`./locales/${language}/${namespace}.json`];
      expect(target, `${language}/${namespace}.json is missing`).toBeDefined();
      expect(compareNamespace(files[`./locales/en/${namespace}.json`], target, language)).toEqual(
        []
      );
    });

    it('has no namespace English lacks', () => {
      const own = Object.keys(files)
        .filter((path) => path.startsWith(`./locales/${language}/`))
        .map((path) => path.slice(`./locales/${language}/`.length, -'.json'.length))
        .sort();
      expect(own).toEqual(namespaces);
    });
  }
);

describe('compareNamespace', () => {
  const english = {
    title: 'Accounts',
    count_one: '{{count}} account',
    count_other: '{{count}} accounts',
    categories: { cat_other: 'Other' },
  };

  it('accepts the plural forms of the language', () => {
    expect(
      compareNamespace(
        english,
        {
          title: 'Konta',
          count_one: '{{count}} konto',
          count_few: '{{count}} konta',
          count_many: '{{count}} kont',
          count_other: '{{count}} konta',
          categories: { cat_other: 'Inne' },
        },
        'pl'
      )
    ).toEqual([]);
    expect(
      compareNamespace(
        english,
        { title: '口座', count_other: '{{count}} 件の口座', categories: { cat_other: 'その他' } },
        'ja'
      )
    ).toEqual([]);
  });

  it('reports missing keys, forms and variables', () => {
    expect(
      compareNamespace(
        english,
        { count_one: 'ein Konto', count_other: 'Konten', categories: { cat: 'x' } },
        'de'
      )
    ).toEqual([
      'extra key categories.cat',
      'missing key title',
      'count drops {{count}}',
      'missing key categories.cat_other',
    ]);
  });

  it('requires the same <Trans> components', () => {
    expect(
      compareNamespace({ note: 'Saves <b>{{amount}}</b>' }, { note: 'Spart {{amount}}' }, 'de')
    ).toEqual(['note drops <b>']);
  });

  it('knows each language’s plural categories', () => {
    expect([...pluralForms('pl').required].sort()).toEqual(['few', 'many', 'one', 'other']);
    expect(pluralForms('ja').required).toEqual(['other']);
    expect(pluralForms('cs')).toEqual({ required: ['one', 'few', 'other'], optional: ['many'] });
  });
});

/** What a person reads about sync, flattened: every `deviceSync` string and `common`'s `validation.sync*` ones. */
function syncStrings(language: string): string[] {
  const out: string[] = [];
  const walk = (node: unknown) => {
    if (typeof node === 'string') out.push(node);
    else if (node && typeof node === 'object') Object.values(node).forEach(walk);
  };
  walk(files[`./locales/${language}/deviceSync.json`]);
  const validation = (files[`./locales/${language}/common.json`] as LocaleTree)['validation'];
  for (const [key, value] of Object.entries((validation ?? {}) as LocaleTree)) {
    if (key.startsWith('sync')) walk(value);
  }
  return out;
}

describe.each(SUPPORTED_LANGUAGES)('%s sync copy', (language) => {
  it('no locale calls the sync server a relay or names ALLOWED_ACCOUNTS', () => {
    const strings = syncStrings(language);
    expect(strings.length).toBeGreaterThan(100);
    for (const value of strings) {
      const text = value.toLowerCase();
      expect(text, value).not.toContain('relay');
      expect(text, value).not.toContain('allowed_accounts');
    }
  });

  it('keeps Moony cloud as the one untranslated brand name', () => {
    const files_ = files[`./locales/${language}/deviceSync.json`] as LocaleTree;
    const server = (files_['setup'] as LocaleTree)['server'] as LocaleTree;
    expect(server['cloud']).toBe('Moony cloud');
    const pay = (files_['setup'] as LocaleTree)['pay'] as LocaleTree;
    expect(pay['ownServer']).toContain('Moony cloud');
  });
});
