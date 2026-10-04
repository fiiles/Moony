import { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import type { SavedStockImportFormat } from '@shared/schema';
import { isSourceId } from './import-config';

/**
 * The name of a source for display: the broker, "Vlastní tabulka (vzor Moony)",
 * "Jiný broker", or the name the user gave a remembered format.
 */
export function useSourceLabel(formats: readonly SavedStockImportFormat[]) {
  const { t } = useTranslation('stocks');
  return useCallback(
    (source: string): string => {
      if (isSourceId(source)) return t(`importWizard.sources.${source}.name`);
      return (
        formats.find((format) => format.id === source)?.name ??
        t('importWizard.sources.savedFallback')
      );
    },
    [t, formats]
  );
}
