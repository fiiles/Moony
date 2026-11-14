/** The file name of a path the backend returned (either separator). */
export function backupFileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** The last second of year 9999: later instants have no valid `Date` and make `Intl` throw. */
const MAX_DISPLAY_SECONDS = 253_402_300_799;

/** A `Date` for unix seconds from the sync server, or null when it is not a displayable date (a hostile or broken value). */
export function secondsToDate(seconds: number | null | undefined): Date | null {
  if (typeof seconds !== 'number' || !Number.isFinite(seconds)) return null;
  if (seconds < 0 || seconds > MAX_DISPLAY_SECONDS) return null;
  return new Date(seconds * 1000);
}
