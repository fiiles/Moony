import {
  readSetting,
  writeSetting,
  type ReadableStorage,
  type WritableStorage,
} from '@/utils/device-storage';
import {
  DAY,
  PERIOD_PRESETS,
  dayFromIso,
  isoDay,
  type DayRange,
  type PeriodPreset,
} from '@/utils/stock-performance';

/** What the analysis page remembers per device (`localStorage`, never the profile). */
export const PERIOD_KEY = 'stocks-analysis.period';
export const CUSTOM_KEY = 'stocks-analysis.custom';
export const GROUP_KEY = 'stocks-analysis.group';

/** The default period: this year. */
const DEFAULT_PERIOD: PeriodPreset = 'ytd';

export interface PeriodChoice {
  period: PeriodPreset;
  /** The last custom range; `period === 'custom'` implies it is set. */
  custom: DayRange | null;
}

/** `"2026-01-05|2026-09-30"` to a range; `null` when it is not two valid days at least two apart. */
export function parseStoredRange(text: string | null): DayRange | null {
  if (!text) return null;
  const [from, to] = text.split('|');
  const start = dayFromIso(from ?? '');
  const end = dayFromIso(to ?? '');
  if (start === null || end === null || start > end - 2 * DAY) return null;
  return { from: start, to: end };
}

/** The remembered period and custom range; an unknown or unusable value falls back to "Letos". */
export function loadPeriod(storage?: ReadableStorage): PeriodChoice {
  const custom = parseStoredRange(readSetting(CUSTOM_KEY, storage));
  const stored = readSetting(PERIOD_KEY, storage);
  const preset = PERIOD_PRESETS.find((p) => p === stored);
  if (!preset) return { period: DEFAULT_PERIOD, custom };
  if (preset === 'custom' && !custom) return { period: DEFAULT_PERIOD, custom: null };
  return { period: preset, custom };
}

/** Remembers the period; the custom range is written whenever there is one. */
export function savePeriod(choice: PeriodChoice, storage?: WritableStorage): void {
  writeSetting(PERIOD_KEY, choice.period, storage);
  if (choice.custom) {
    writeSetting(CUSTOM_KEY, `${isoDay(choice.custom.from)}|${isoDay(choice.custom.to)}`, storage);
  }
}

/** The remembered "Rozdělit podle" key (`group:<id>`, `sector`, `currency`); the page validates it. */
export function loadGroupKey(storage?: ReadableStorage): string | null {
  return readSetting(GROUP_KEY, storage);
}

export function saveGroupKey(key: string, storage?: WritableStorage): void {
  writeSetting(GROUP_KEY, key, storage);
}
