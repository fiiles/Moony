/**
 * Czech vocative of a first name for the greeting on the overview
 * ("Dobré ráno, Filipe."). Covers the regular patterns of Czech given names
 * and the common irregular ones; anything it is unsure about comes back
 * unchanged (nominative), which is still correct enough for a greeting.
 * Other languages return the name as is.
 */

/** Names whose vocative is not predictable from the ending. */
const IRREGULAR: Record<string, string> = {
  Pavel: 'Pavle',
  Karel: 'Karle',
  Daniel: 'Danieli',
  Zdeněk: 'Zdeňku',
  Vojtěch: 'Vojtěchu',
  Oldřich: 'Oldřichu',
  Jindřich: 'Jindřichu',
  Bedřich: 'Bedřichu',
  Jiří: 'Jiří',
  Petr: 'Petře',
  Igor: 'Igore',
  Viktor: 'Viktore',
  Marek: 'Marku',
  Radek: 'Radku',
  Zbyněk: 'Zbyňku',
  Hynek: 'Hynku',
  Kryštof: 'Kryštofe',
  Josef: 'Josefe',
  Mikuláš: 'Mikuláši',
  Jan: 'Jane',
  Adam: 'Adame',
  Roman: 'Romane',
  Milan: 'Milane',
  Martin: 'Martine',
  Alex: 'Alexi',
  Max: 'Maxi',
  Felix: 'Felixi',
  Nikolas: 'Nikolasi',
  Lukas: 'Lukasi',
  Matyas: 'Matyasi',
  Dagmar: 'Dagmar',
  Miriam: 'Miriam',
  Ester: 'Ester',
  Ingrid: 'Ingrid',
  Ruth: 'Ruth',
  Karin: 'Karin',
  Nikol: 'Nikol',
};

const VOWELS = new Set(['a', 'e', 'i', 'o', 'u', 'y', 'á', 'é', 'í', 'ó', 'ú', 'ů', 'ý', 'ě']);
const SOFT = new Set(['š', 'ž', 'č', 'ř', 'c', 'j', 'ď', 'ť', 'ň']);

function vocativeCs(name: string): string {
  const irregular = IRREGULAR[name];
  if (irregular) return irregular;

  const last = name.slice(-1);
  const lastLower = last.toLowerCase();
  const prev = name.slice(-2, -1).toLowerCase();

  // Feminine -a → -o (Jana → Jano, Petra → Petro); -ie/-e/-í/-y/-o/-u unchanged
  if (lastLower === 'a') return `${name.slice(0, -1)}o`;
  if (VOWELS.has(lastLower)) return name;

  // -ec → -če (Vavřinec → Vavřinče); -ek → -ku (Marek → Marku) only when a vowel precedes the k
  if (name.endsWith('ec')) return `${name.slice(0, -2)}če`;
  if (name.endsWith('ek') && !VOWELS.has(name.slice(-3, -2).toLowerCase())) {
    // e-elision: Zdeněk → Zdeňku handled in IRREGULAR; generic -ek → -ku
    return `${name.slice(0, -2)}ku`;
  }
  if (name.endsWith('ek')) return `${name.slice(0, -1)}ku`;

  // Velars and h: -k/-g/-h/-ch → -u (Patrik → Patriku, Vojtěch → Vojtěchu)
  if (lastLower === 'k' || lastLower === 'g' || lastLower === 'h') return `${name}u`;

  // Soft consonants → -i (Lukáš → Lukáši, Tomáš → Tomáši, Ondřej → Ondřeji, Matěj → Matěji)
  if (SOFT.has(lastLower)) return `${name}i`;

  // -r after a consonant → -ře (Petr → Petře); after a vowel → -re (Igor → Igore)
  if (lastLower === 'r') return VOWELS.has(prev) ? `${name}e` : `${name.slice(0, -1)}ře`;

  // Hard consonants → -e (Filip → Filipe, Tomáš handled above, David → Davide, Robert → Roberte)
  if (/[bdfmnpstvzlx]/.test(lastLower)) return `${name}e`;

  return name;
}

/**
 * Vocative for the greeting. `locale` is the UI locale ("cs", "cs-CZ", "en", …);
 * only Czech declines, every other language returns the trimmed name.
 */
export function vocative(name: string, locale: string): string {
  const trimmed = name.trim();
  if (!trimmed) return trimmed;
  if (!/^cs\b/i.test(locale)) return trimmed;
  // Only the first word is declined (double names keep the second as is)
  const [first, ...rest] = trimmed.split(/\s+/);
  return [vocativeCs(first), ...rest].join(' ');
}
