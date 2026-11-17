/** The overview's greeting by the local hour: morning until 11, day until 18, evening after. */
export function greetingKey(hour: number): 'morning' | 'day' | 'evening' {
  if (hour >= 5 && hour < 11) return 'morning';
  if (hour >= 11 && hour < 18) return 'day';
  return 'evening';
}

/**
 * The `dashboard` key of the overview's title and the name it greets: "Vítejte" on the first visit
 * after onboarding, else by the time of day. A profile without a name is greeted without one
 * ("Dobré ráno.", not "Dobré ráno, ."): the `NoName` keys take no name.
 */
export function greetingMessage(
  hour: number,
  firstVisit: boolean,
  name: string
): { key: string; name?: string } {
  const base = firstVisit ? 'welcomeFirst' : `greeting.${greetingKey(hour)}`;
  const trimmed = name.trim();
  return trimmed === '' ? { key: `${base}NoName` } : { key: base, name: trimmed };
}
