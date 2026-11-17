import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { RecoveryKitCopy } from '@/utils/recovery-kit';

/** The kit's printed lines, in the language the app shows. */
export function useRecoveryKitCopy(): RecoveryKitCopy {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  return useMemo(
    () => ({
      title: t('setup.kit.file.title'),
      intro: t('setup.kit.file.intro'),
      codeLabel: t('setup.kit.file.codeLabel'),
      codeNote: t('setup.kit.file.codeNote'),
      qrLabel: t('setup.kit.file.qrLabel'),
      differsFromRecoveryKey: t('setup.kit.differs'),
      created: t('setup.kit.file.created', {
        date: date(new Date(), { dateStyle: 'long' }),
      }),
      relayLabel: t('setup.kit.file.relayLabel'),
      relayNote: t('setup.kit.file.relayNote'),
    }),
    [t, date]
  );
}
