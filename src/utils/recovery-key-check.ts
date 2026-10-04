/**
 * Recovery-key verification for the first-run wizard: the user retypes two
 * of the six groups before the account is created.
 */

/** Split "ABCD-EFGH-…" into its groups (dashes/whitespace tolerant). */
export function recoveryKeyGroups(key: string): string[] {
  return key
    .toUpperCase()
    .split(/[\s-]+/)
    .filter((g) => g.length > 0);
}

/** Two distinct group indexes (0-based) to ask for, in ascending order. */
export function pickVerificationGroups(
  groupCount: number,
  random: () => number = Math.random
): [number, number] {
  if (groupCount < 2) return [0, 0];
  const first = Math.min(groupCount - 1, Math.floor(random() * groupCount));
  let second = Math.min(groupCount - 1, Math.floor(random() * (groupCount - 1)));
  if (second >= first) second += 1;
  return first < second ? [first, second] : [second, first];
}

/** Normalize a typed group: uppercase, no spaces/dashes. */
export function normalizeGroupInput(input: string): string {
  return input.replace(/[\s-]+/g, '').toUpperCase();
}

/** True when both typed groups match the key at the requested indexes. */
export function verifyGroups(
  key: string,
  indexes: [number, number],
  typed: [string, string]
): boolean {
  const groups = recoveryKeyGroups(key);
  const [a, b] = indexes;
  if (a >= groups.length || b >= groups.length) return false;
  return normalizeGroupInput(typed[0]) === groups[a] && normalizeGroupInput(typed[1]) === groups[b];
}

/** Text for the "Save to file" action: the key plus how it is meant to be used. */
export function recoveryKeyFileContents(
  key: string,
  lines: { title: string; note: string }
): string {
  return `${lines.title}\n\n${key}\n\n${lines.note}\n`;
}
