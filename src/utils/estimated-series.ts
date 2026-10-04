/**
 * Helpers for charts that draw reconstructed ("backfilled") days dashed
 *. On a backfilled day the static classes — bank accounts, bonds,
 * real estate, loans — are carried from the nearest live snapshot, so the
 * chart must not present them as authentic records.
 *
 * A chart draws one solid and one dashed series per metric. The dashed series
 * covers every estimated day plus the live day on either side of it, so every
 * line segment that touches an estimate is dashed and the line stays
 * connected; the solid series covers the live days only.
 */

/** History rows with this `source` were reconstructed, not recorded. */
export function isEstimatedSource(source: string | undefined): boolean {
  return source === 'backfill';
}

export interface EstimatedFlag {
  /** True when the row is a reconstruction (see `isEstimatedSource`). */
  estimated?: boolean;
}

export function anyEstimated(rows: readonly EstimatedFlag[]): boolean {
  return rows.some((r) => r.estimated === true);
}

/** Split one series into its solid (live) and dashed (estimated) parts. */
export function splitEstimated(
  values: readonly number[],
  estimated: readonly boolean[]
): { solid: (number | null)[]; dashed: (number | null)[] } {
  const solid: (number | null)[] = [];
  const dashed: (number | null)[] = [];
  values.forEach((value, i) => {
    const isEst = estimated[i] === true;
    const touchesEstimate = isEst || estimated[i - 1] === true || estimated[i + 1] === true;
    solid.push(isEst ? null : value);
    dashed.push(touchesEstimate ? value : null);
  });
  return { solid, dashed };
}

type Split<K extends string> = { [P in K as `${P}Solid`]: number | null } & {
  [P in K as `${P}Est`]: number | null;
};

/**
 * Add `<key>Solid` and `<key>Est` series next to each of `keys`; the original
 * `<key>` column stays untouched (it feeds fills and tooltips).
 */
export function withEstimatedSplit<T extends EstimatedFlag, K extends keyof T & string>(
  rows: readonly T[],
  keys: readonly K[]
): Array<T & Split<K>> {
  const flags = rows.map((r) => r.estimated === true);
  const out = rows.map((r) => ({ ...r }) as T & Split<K>);
  for (const key of keys) {
    const { solid, dashed } = splitEstimated(
      rows.map((r) => Number(r[key]) || 0),
      flags
    );
    out.forEach((row, i) => {
      (row as Record<string, unknown>)[`${key}Solid`] = solid[i];
      (row as Record<string, unknown>)[`${key}Est`] = dashed[i];
    });
  }
  return out;
}

type Tops<K extends string> = { [P in K as `${P}Top`]: number };

/**
 * Add `<key>Top`: the running total of `keys` in the given (stacking) order,
 * i.e. where the top edge of that series sits in a stacked area chart.
 */
export function withCumulativeTops<T extends object, K extends keyof T & string>(
  rows: readonly T[],
  keys: readonly K[]
): Array<T & Tops<K>> {
  return rows.map((row) => {
    const out = { ...row } as T & Tops<K>;
    let running = 0;
    for (const key of keys) {
      running += Number(row[key]) || 0;
      (out as Record<string, unknown>)[`${key}Top`] = running;
    }
    return out;
  });
}
