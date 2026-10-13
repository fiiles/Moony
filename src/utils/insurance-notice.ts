/** Longest cancellation notice the profile setting accepts (days). */
export const MAX_NOTICE_DAYS = 365;

/**
 * The whole number of days typed into the "Insurance cancellation notice" field, or null for text
 * that is not 0 to 365. An empty field means 0: no anniversary reminder.
 */
export function parseNoticeDays(text: string): number | null {
  const trimmed = text.trim();
  if (trimmed === '') return 0;
  if (!/^\d{1,3}$/.test(trimmed)) return null;
  const days = Number(trimmed);
  return days <= MAX_NOTICE_DAYS ? days : null;
}
