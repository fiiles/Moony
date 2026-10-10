// Locale parity check for one language against English: `node scripts/check-locale.mjs de`.
// Same rules as src/i18n/locales.test.ts (the gate); this script is the quick loop for translators.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '../src/i18n/locales');
const lang = process.argv[2];
if (!lang) throw new Error('usage: node scripts/check-locale.mjs <lang>');

const PLURAL = /_(zero|one|two|few|many|other)$/;
const VAR = /\{\{\s*([a-zA-Z0-9_]+)\s*(?:,[^}]*)?\}\}/g;
const flatten = (tree, prefix = '', out = {}) => {
  for (const [key, value] of Object.entries(tree)) {
    const p = prefix ? `${prefix}.${key}` : key;
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) flatten(value, p, out);
    else out[p] = value;
  }
  return out;
};
const categories =
  lang === 'cs' ? ['one', 'few', 'other'] : new Intl.PluralRules(lang).resolvedOptions().pluralCategories;
const optional = lang === 'cs' ? ['many'] : [];

const problems = [];
for (const file of fs.readdirSync(path.join(root, 'en')).sort()) {
  const target = path.join(root, lang, file);
  if (!fs.existsSync(target)) {
    problems.push(`${file}: missing`);
    continue;
  }
  let tr;
  try {
    tr = flatten(JSON.parse(fs.readFileSync(target, 'utf8')));
  } catch (e) {
    problems.push(`${file}: invalid JSON (${e.message})`);
    continue;
  }
  const en = flatten(JSON.parse(fs.readFileSync(path.join(root, 'en', file), 'utf8')));
  // A plural base has at least two suffixed forms in English; a lone `cat_other` is a plain key.
  const forms = new Map();
  for (const k of Object.keys(en)) {
    if (!PLURAL.test(k)) continue;
    const base = k.replace(PLURAL, '');
    forms.set(base, (forms.get(base) ?? 0) + 1);
  }
  const pluralBases = new Set([...forms].filter(([, n]) => n >= 2).map(([b]) => b));
  const group = (flat) => {
    const m = new Map();
    for (const [k, v] of Object.entries(flat)) {
      const stripped = k.replace(PLURAL, '');
      const plural = PLURAL.test(k) && pluralBases.has(stripped);
      const base = plural ? stripped : k;
      if (!m.has(base)) m.set(base, { cats: new Set(), values: [] });
      if (plural) m.get(base).cats.add(k.match(PLURAL)[1]);
      m.get(base).values.push(v);
    }
    return m;
  };
  const ge = group(en);
  const gt = group(tr);
  for (const base of ge.keys()) if (!gt.has(base)) problems.push(`${file}: missing key ${base}`);
  for (const base of gt.keys()) if (!ge.has(base)) problems.push(`${file}: extra key ${base}`);
  for (const [base, e] of ge) {
    const t = gt.get(base);
    if (!t) continue;
    if (pluralBases.has(base)) {
      const want = new Set(categories);
      if (e.cats.has('zero')) want.add('zero');
      for (const c of want) if (!t.cats.has(c)) problems.push(`${file}: ${base} lacks _${c}`);
      for (const c of t.cats) if (!want.has(c) && !optional.includes(c)) problems.push(`${file}: ${base} has unexpected _${c}`);
    }
    const vars = (values) => new Set(values.flatMap((v) => (typeof v === 'string' ? [...v.matchAll(VAR)].map((m) => m[1]) : [])));
    const ve = vars(e.values);
    const vt = vars(t.values);
    for (const v of ve) if (!vt.has(v) && !(v === 'count' && pluralBases.has(base))) problems.push(`${file}: ${base} drops {{${v}}}`);
    for (const v of vt) if (!ve.has(v)) problems.push(`${file}: ${base} adds unknown {{${v}}}`);
    for (const v of t.values) {
      if (typeof v !== 'string') problems.push(`${file}: ${base} is not a string`);
      else if (v.trim() === '') problems.push(`${file}: ${base} is empty`);
    }
  }
}
if (problems.length) {
  console.log(problems.join('\n'));
  console.log(`\n${problems.length} problem(s) in ${lang}`);
  process.exit(1);
}
console.log(`${lang}: OK`);
