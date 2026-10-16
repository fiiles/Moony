import { useState } from 'react';

/**
 * The last non-null value. A sheet's row goes back to `null` the moment the sheet is asked to
 * close, while the sheet is still sliding away: it keeps showing what it showed instead of
 * blanking out under the finger.
 */
export function useRetained<T>(value: T | null): T | null {
  const [kept, setKept] = useState(value);
  if (value !== null && value !== kept) setKept(value);
  return value ?? kept;
}
