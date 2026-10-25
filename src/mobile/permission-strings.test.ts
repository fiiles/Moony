import { describe, expect, it } from 'vitest';

// The two iOS permission prompts (Face ID, camera) are shown from the app bundle's
// `<lang>.lproj/InfoPlist.strings`, not from the React i18n files: iOS reads them before any of the
// app's code runs. They live next to the xcodegen project, one folder per app language. This gate
// keeps the ten folders complete and wired into the project, and the English text equal to the
// fallback in `Info.ios.plist`, which the Tauri CLI merges into the generated Info.plist.
//
// The files sit outside src/ and the frontend tsconfig has no Node typings, so fs is imported
// through a variable specifier and typed by hand (the pattern of src/index.test.ts).
type Fs = { readFileSync(url: URL, encoding: 'utf8'): string };
const fsSpecifier = 'node:fs';
const { readFileSync } = (await import(/* @vite-ignore */ fsSpecifier)) as Fs;

const APPLE = new URL('../../src-tauri/gen/apple/', import.meta.url);
const read = (url: URL) => readFileSync(url, 'utf8');

/** The app's ten UI languages as iOS region folders (`zh` is Simplified Chinese, `zh-Hans`). */
const REGIONS = ['en', 'cs', 'de', 'es', 'it', 'nb', 'pl', 'sv', 'ja', 'zh-Hans'];
const KEYS = ['NSFaceIDUsageDescription', 'NSCameraUsageDescription'];

/**
 * The entries of a `.strings` file, in order. Throws when anything but comments, whitespace and
 * `"key" = "value";` entries is left, which is what an unescaped `"` inside a value produces.
 */
function parseStrings(text: string): [string, string][] {
  const body = text.replace(/\/\*[\s\S]*?\*\//g, '');
  const entry = /"((?:[^"\\]|\\.)*)"\s*=\s*"((?:[^"\\]|\\.)*)"\s*;/g;
  const entries = [...body.matchAll(entry)].map((m): [string, string] => [m[1], m[2]]);
  expect(body.replace(entry, '').trim(), 'text outside the entries').toBe('');
  return entries;
}

/** The `<string>` value that follows `<key>name</key>` in the plist. */
function plistString(plist: string, name: string): string {
  const m = plist.match(new RegExp(`<key>${name}</key>\\s*<string>([^<]*)</string>`));
  expect(m, `${name} in Info.ios.plist`).not.toBeNull();
  return m![1];
}

const stringsOf = (region: string) =>
  read(new URL(`Localizations/${region}.lproj/InfoPlist.strings`, APPLE));

describe('InfoPlist.strings (the permission prompts)', () => {
  it.each(REGIONS)('%s holds exactly the two permission texts', (region) => {
    const text = stringsOf(region);
    const entries = parseStrings(text);
    expect(entries.map(([key]) => key)).toEqual(KEYS);
    for (const [key, value] of entries) {
      expect(value.trim(), `${region} ${key}`).not.toBe('');
      // Parsed, so every quote in a value is escaped; a bare one would have failed above
      expect(value.replace(/\\./g, ''), `${region} ${key}`).not.toContain('"');
    }
    // Each entry follows a comment that says what it is for
    expect(text.match(/\/\*[\s\S]*?\*\/\s*"NS/g)?.length, `${region} comments`).toBe(2);
  });

  it('English equals the texts of Info.ios.plist', () => {
    const plist = read(new URL('../../src-tauri/Info.ios.plist', import.meta.url));
    const en = new Map(parseStrings(stringsOf('en')));
    for (const key of KEYS) {
      expect(en.get(key), key).toBe(plistString(plist, key));
    }
  });

  it('keeps Face ID untranslated in every language', () => {
    for (const region of REGIONS) {
      const face = new Map(parseStrings(stringsOf(region))).get('NSFaceIDUsageDescription');
      expect(face, region).toContain('Face ID');
    }
  });

  it('project.yml puts the Localizations folder into the moony_iOS target', () => {
    const yml = read(new URL('project.yml', APPLE));
    const target = yml.slice(yml.indexOf('\n  moony_iOS:'));
    expect(target).toMatch(/^\s+- path: Localizations\s*$/m);
  });

  it('the generated project names every region and bundles InfoPlist.strings', () => {
    const pbx = read(new URL('moony.xcodeproj/project.pbxproj', APPLE));
    const regions = pbx.match(/knownRegions = \(([^)]*)\);/);
    expect(regions, 'knownRegions').not.toBeNull();
    const known = regions![1]
      .split(',')
      .map((r) => r.trim().replace(/^"|"$/g, ''))
      .filter(Boolean);
    for (const region of REGIONS) expect(known, region).toContain(region);
    expect(pbx).toContain('InfoPlist.strings in Resources');
  });
});
