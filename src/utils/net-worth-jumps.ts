/**
 * Big steps of the net-worth line and what stands behind them (design system §9: an aggregate
 * line carries only the events that explain its jumps). Pure: no DOM, no locale, no hooks. The
 * dashboard passes the plotted days with the value every class contributes to net worth, and the
 * dated facts of the period; the result is the marks to draw, each with the classes that moved
 * and the facts of that day per class. Wording is the caller's.
 *
 * - A jump is a day-over-day change of at least 5 % of the largest |net worth| of the period.
 *   A day whose previous day is not plotted (no rate, no row) is never compared: a gap is not a
 *   change.
 * - A step the dated facts do not explain (under half of it) and whose largest class is stocks or
 *   crypto without a trade that day is the market, not something that happened to the user: it
 *   needs 10 % to be marked and is titled as a market move. A loan with a dated fact (a start, a
 *   manual balance, an extra payment) that moved a tenth of the step keeps it a plain jump.
 * - At most 12 marks per period, the largest ones.
 */

import type { DatedFact } from '@shared/schema';
import { utcDayFloor } from './chart-axis';
import type { ChartEvent } from './chart-scale';

/** The classes of a history day; `loans` counts against net worth. */
export const JUMP_CLASSES = [
  'savings',
  'investments',
  'crypto',
  'bonds',
  'realEstate',
  'loans',
  'otherAssets',
] as const;
export type JumpClass = (typeof JUMP_CLASSES)[number];

/** A step of at least this share of the period's largest |net worth| is a jump. */
export const JUMP_MIN_SHARE = 0.05;
/** A market move (stocks or crypto alone, nothing dated) must reach this share to be marked. */
export const MARKET_MOVE_MIN_SHARE = 0.1;
/** Below this share of the step the dated facts do not explain it. */
export const EXPLAINED_MIN_SHARE = 0.5;
/** A class is named when it moved at least this share of the step. */
export const PART_MIN_SHARE = 0.1;
/** Marks per period: the largest steps. */
export const MAX_JUMPS = 12;

const DAY_SECONDS = 86400;

/** The classes whose moves without a trade are the market's. */
const MARKET_CLASSES: readonly JumpClass[] = ['investments', 'crypto'];

/** One plotted day. */
export interface JumpDay {
  /** Unix seconds of the plotted point. */
  t: number;
  netWorth: number;
  /** What each class contributes to `netWorth`: assets as they are, `loans` as minus the debt. */
  classes: Record<JumpClass, number>;
}

/** One class that moved in a jump and the facts of the day that belong to it. */
export interface JumpPart {
  cls: JumpClass;
  /** Change of the class's contribution to net worth since the day before. */
  delta: number;
  facts: DatedFact[];
}

export interface NetWorthJump {
  /** Stable key of the mark. */
  id: string;
  /** Unix seconds of the plotted point. */
  t: number;
  /** Net-worth change since the previous day (signed). */
  delta: number;
  /** `jump`: something happened (or the cause is not dated); `market`: stocks or crypto moved alone. */
  kind: 'jump' | 'market';
  /** The classes behind the step, largest first. */
  parts: JumpPart[];
}

const isFiniteNumber = (value: number) => Number.isFinite(value);

function factsByDayAndClass(facts: readonly DatedFact[]): Map<number, Map<string, DatedFact[]>> {
  const out = new Map<number, Map<string, DatedFact[]>>();
  for (const fact of facts) {
    let byClass = out.get(fact.day);
    if (!byClass) {
      byClass = new Map();
      out.set(fact.day, byClass);
    }
    const list = byClass.get(fact.class);
    if (list) list.push(fact);
    else byClass.set(fact.class, [fact]);
  }
  return out;
}

/** The marks of the plotted days (oldest first, as plotted), oldest mark first. */
export function detectNetWorthJumps(
  days: readonly JumpDay[],
  facts: readonly DatedFact[]
): NetWorthJump[] {
  const largest = days.reduce(
    (max, day) => (isFiniteNumber(day.netWorth) ? Math.max(max, Math.abs(day.netWorth)) : max),
    0
  );
  if (!(largest > 0)) return [];
  const byDay = factsByDayAndClass(facts);

  const found: NetWorthJump[] = [];
  for (let i = 1; i < days.length; i++) {
    const previous = days[i - 1];
    const current = days[i];
    const day = utcDayFloor(current.t);
    // Only a step between consecutive days is a change; a missing day is a gap
    if (day - utcDayFloor(previous.t) !== DAY_SECONDS) continue;
    const delta = current.netWorth - previous.netWorth;
    if (!isFiniteNumber(delta) || Math.abs(delta) < JUMP_MIN_SHARE * largest) continue;

    const dated = byDay.get(day);
    const parts = JUMP_CLASSES.map((cls) => ({
      cls,
      delta: current.classes[cls] - previous.classes[cls],
      facts: dated?.get(cls) ?? [],
    })).filter((part) => isFiniteNumber(part.delta));

    if (parts.length === 0) continue;
    const explained =
      parts.filter((p) => p.facts.length > 0).reduce((sum, p) => sum + Math.abs(p.delta), 0) /
      Math.abs(delta);
    const top = parts.reduce((a, b) => (Math.abs(b.delta) > Math.abs(a.delta) ? b : a));
    // A loan that was drawn, repaid or restated that day is something that happened to the user,
    // whatever the stocks did meanwhile
    const loanExplained = parts.some(
      (p) =>
        p.cls === 'loans' &&
        p.facts.length > 0 &&
        Math.abs(p.delta) >= PART_MIN_SHARE * Math.abs(delta)
    );
    const market =
      explained < EXPLAINED_MIN_SHARE &&
      MARKET_CLASSES.includes(top.cls) &&
      top.facts.length === 0 &&
      !loanExplained;
    if (market && Math.abs(delta) < MARKET_MOVE_MIN_SHARE * largest) continue;

    const named = parts
      .filter((p) => Math.abs(p.delta) >= PART_MIN_SHARE * Math.abs(delta))
      .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta));
    found.push({
      id: `jump-${day}`,
      t: current.t,
      delta,
      kind: market ? 'market' : 'jump',
      parts: named.length > 0 ? named : [top],
    });
  }

  return found
    .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta))
    .slice(0, MAX_JUMPS)
    .sort((a, b) => a.t - b.t);
}

/** A mark on the net-worth line: the neutral ring, carrying the jump it explains. */
export interface JumpEvent extends ChartEvent {
  type: 'mark';
  jump: NetWorthJump;
}

export function jumpEvents(jumps: readonly NetWorthJump[]): JumpEvent[] {
  return jumps.map((jump) => ({ id: jump.id, t: jump.t, type: 'mark', jump }));
}
