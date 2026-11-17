import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useFormat } from '@/lib/use-format';
import type { RecoveryKit } from '@shared/schema';
import type { RecoveryKitCopy } from '@/utils/recovery-kit';

/**
 * The kit's printed lines, in the language the app shows. The "created" line names the day the kit
 * was made and the device that made it (spec §2.1); without a kit (the phone's, until its task) it
 * says today.
 */
export function useRecoveryKitCopy(
  kit?: Pick<RecoveryKit, 'createdAt' | 'createdBy'>
): RecoveryKitCopy {
  const { t } = useTranslation('deviceSync');
  const { date } = useFormat();
  const createdAt = kit?.createdAt;
  const createdBy = kit?.createdBy ?? null;
  return useMemo(() => {
    const day = date(createdAt === undefined ? new Date() : new Date(createdAt * 1000), {
      dateStyle: 'long',
    });
    const createdLine = () =>
      createdBy
        ? t('setup.kit.file.createdBy', { date: day, name: createdBy })
        : t('setup.kit.file.created', { date: day });
    return {
      title: t('setup.kit.file.title'),
      intro: t('setup.kit.file.intro'),
      codeLabel: t('setup.kit.file.codeLabel'),
      codeNote: t('setup.kit.file.codeNote'),
      qrLabel: t('setup.kit.file.qrLabel'),
      differsFromRecoveryKey: t('setup.kit.differs'),
      created: createdLine(),
      relayLabel: t('setup.kit.file.relayLabel'),
      relayNote: t('setup.kit.file.relayNote'),
    };
  }, [t, date, createdAt, createdBy]);
}
