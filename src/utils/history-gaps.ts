/**
 * Class values of one recorded history day for a chart, with an unknown class (`null`:
 * nothing in it could be converted that day) counted as 0 — unless today's value has that
 * class. Then the day is left out (`null`): plotting it would drop the line by the whole
 * class for a day, a cliff that is a missing rate, not a change in value. A class unknown
 * today as well stays at 0 on every day, so the line stays comparable with today.
 */
export function bridgeUnknownClasses<K extends string>(
  day: Record<K, number | null>,
  knownToday: Record<K, boolean>
): Record<K, number> | null {
  const out = {} as Record<K, number>;
  for (const key of Object.keys(day) as K[]) {
    const value = day[key];
    if (value === null) {
      if (knownToday[key]) return null;
      out[key] = 0;
    } else {
      out[key] = value;
    }
  }
  return out;
}
