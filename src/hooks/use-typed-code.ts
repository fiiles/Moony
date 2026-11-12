import { useState } from 'react';
import { ApiError } from '@/lib/api-error';
import { CODE_MISMATCH_KEY } from '@/utils/pairing-code';

/**
 * The holder's typed digits. A wrong code is typed again (the field empties); any other answer,
 * a momentary failure included, keeps the digits so the same confirm can be tried once more.
 */
export function useTypedCode(confirmError: Error | null): [string, (digits: string) => void] {
  const [typed, setTyped] = useState('');
  // The answer the field last reacted to: a new answer (a new error object) is reacted to once.
  const [seen, setSeen] = useState<Error | null>(null);
  if (confirmError !== seen) {
    setSeen(confirmError);
    if (confirmError instanceof ApiError && confirmError.key === CODE_MISMATCH_KEY) setTyped('');
  }
  return [typed, setTyped];
}
