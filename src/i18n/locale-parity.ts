/**
 * Compares a locale with English, namespace by namespace: the gate behind
 * `locales.test.ts`. `fallbackLng: 'en'` hides a missing key at runtime, so
 * nothing else would notice one.
 */

export type LocaleTree = { [key: string]: unknown };

const PLURAL = /_(zero|one|two|few|many|other)$/;
const VARIABLE = /\{\{\s*([a-zA-Z0-9_]+)\s*(?:,[^}]*)?\}\}/g;
/** `<b>…</b>` / `<link>…</link>`: the components a `<Trans>` key hands to React. */
const TAG = /<([a-z][a-zA-Z0-9]*)>/g;

function flatten(tree: LocaleTree, prefix = '', out: Record<string, unknown> = {}) {
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      flatten(value as LocaleTree, path, out);
    } else {
      out[path] = value;
    }
  }
  return out;
}

/**
 * Plural forms a language must give. Czech keeps its hand-written one/few/other
 * (`many` is the decimal form, optional); the rest follow `Intl.PluralRules`.
 */
export function pluralForms(language: string): { required: string[]; optional: string[] } {
  if (language === 'cs') return { required: ['one', 'few', 'other'], optional: ['many'] };
  return {
    required: [...new Intl.PluralRules(language).resolvedOptions().pluralCategories],
    optional: [],
  };
}

interface Entry {
  forms: Set<string>;
  values: unknown[];
}

function matches(values: unknown[], pattern: RegExp): Set<string> {
  return new Set(
    values.flatMap((v) => (typeof v === 'string' ? [...v.matchAll(pattern)].map((m) => m[1]) : []))
  );
}

/** Every difference between one namespace of `language` and English, as readable lines. */
export function compareNamespace(
  english: LocaleTree,
  target: LocaleTree,
  language: string
): string[] {
  const en = flatten(english);
  const tr = flatten(target);

  // A plural has at least two suffixed forms in English; a lone `cat_other` is a plain key.
  const formCount = new Map<string, number>();
  for (const key of Object.keys(en)) {
    if (PLURAL.test(key)) {
      const base = key.replace(PLURAL, '');
      formCount.set(base, (formCount.get(base) ?? 0) + 1);
    }
  }
  const plurals = new Set([...formCount].filter(([, n]) => n >= 2).map(([base]) => base));

  const group = (flat: Record<string, unknown>) => {
    const entries = new Map<string, Entry>();
    for (const [key, value] of Object.entries(flat)) {
      const stripped = key.replace(PLURAL, '');
      const plural = PLURAL.test(key) && plurals.has(stripped);
      const base = plural ? stripped : key;
      const entry = entries.get(base) ?? { forms: new Set<string>(), values: [] };
      if (plural) entry.forms.add(key.match(PLURAL)![1]);
      entry.values.push(value);
      entries.set(base, entry);
    }
    return entries;
  };

  const source = group(en);
  const translated = group(tr);
  const { required, optional } = pluralForms(language);
  const problems: string[] = [];

  for (const base of translated.keys()) {
    if (!source.has(base)) problems.push(`extra key ${base}`);
  }
  for (const [base, s] of source) {
    const t = translated.get(base);
    if (!t) {
      problems.push(`missing key ${base}`);
      continue;
    }
    if (plurals.has(base)) {
      const want = new Set(required);
      if (s.forms.has('zero')) want.add('zero');
      for (const form of want) if (!t.forms.has(form)) problems.push(`${base} lacks _${form}`);
      for (const form of t.forms) {
        if (!want.has(form) && !optional.includes(form))
          problems.push(`${base} has unexpected _${form}`);
      }
    }
    const sv = matches(s.values, VARIABLE);
    const tv = matches(t.values, VARIABLE);
    // A plural's one-form may spell the number out ("1 hour ago"), but some form keeps {{count}}.
    for (const v of sv) if (!tv.has(v)) problems.push(`${base} drops {{${v}}}`);
    for (const v of tv) if (!sv.has(v)) problems.push(`${base} adds unknown {{${v}}}`);
    const st = matches(s.values, TAG);
    const tt = matches(t.values, TAG);
    for (const tag of st) if (!tt.has(tag)) problems.push(`${base} drops <${tag}>`);
    for (const tag of tt) if (!st.has(tag)) problems.push(`${base} adds unknown <${tag}>`);
    for (const value of t.values) {
      if (typeof value !== 'string') problems.push(`${base} is not a string`);
      else if (value.trim() === '') problems.push(`${base} is empty`);
    }
  }
  return problems;
}
