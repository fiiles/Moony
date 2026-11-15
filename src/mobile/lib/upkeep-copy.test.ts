import { describe, expect, it } from 'vitest';

const LOCALES = ['en', 'cs', 'de', 'es', 'it', 'nb', 'pl', 'sv', 'ja', 'zh'];
/** The sub-line of the backup row is one truncated line on a 360 pt phone. */
const MAX_SUB_LINE = 38;

type Upkeep = Record<string, string>;
const files = import.meta.glob<{ upkeep: Upkeep }>('../../i18n/locales/*/mobile.json', {
  eager: true,
  import: 'default',
});
const upkeepOf = (locale: string): Upkeep =>
  files[`../../i18n/locales/${locale}/mobile.json`].upkeep;

describe.each(LOCALES)('the phone backup row copy (%s)', (locale) => {
  const upkeep = upkeepOf(locale);

  it('has a sub-line that fits one line', () => {
    expect(upkeep.bothWaysOut.length).toBeLessThanOrEqual(MAX_SUB_LINE);
  });

  it('leads with the number in every plural form (a figure is never cut)', () => {
    const forms = Object.entries(upkeep).filter(([key]) => key.startsWith('safeCopyStale_'));
    expect(forms.length).toBeGreaterThan(0);
    for (const [, text] of forms) expect(text.startsWith('{{count}}')).toBe(true);
  });
});
