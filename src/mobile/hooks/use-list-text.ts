import { useCallback } from 'react';
import { useFormat } from '@/lib/use-format';
import { listText } from '@/mobile/lib/list-text';

/** `listText` in the UI's language: a list joined as the language writes it (B9). */
export function useListText() {
  const { locale } = useFormat();
  return useCallback((items: readonly string[]) => listText(locale, items), [locale]);
}
