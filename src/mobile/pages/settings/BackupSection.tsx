import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { useBackup } from '@/mobile/hooks/use-backup';
import { RestoreBackupSheet } from '@/mobile/sheets/RestoreBackupSheet';
import type { BackupInspection } from '@shared/schema';

/** The card the date sits in (`.m-card`). */
const CARD = 'mb-3.5 rounded-r4 border border-line bg-paper px-3 shadow-e1';

/** The 48 pt buttons of the section's two actions. */
const BUTTON = 'h-12 w-full rounded-[10px] text-m-row';

/**
 * Nastavení → Záloha a přenos (spec §8.9): when the last backup was made, "Zálohovat nyní" and the
 * restore. A backup is one encrypted file the person keeps in Files (iCloud Drive or another
 * folder); the same file moves a profile from Moony on the computer to the phone and back, with
 * the password that was in use when it was made. There is no automatic backup: the "backup is due"
 * reminder says when to make one. Restoring asks first, in a sheet that names what the file holds.
 * Making a backup works on iOS only for now: elsewhere the button is off and a sentence says so,
 * while restoring (a picked file, read by Rust) is there on every phone.
 */
export default function BackupSection() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const fmt = useFormat();
  const { createBackup, pickRestore, restore, busy, lastBackupAt, canExport } = useBackup();
  const [inspection, setInspection] = useState<BackupInspection | null>(null);

  const chooseRestore = async () => {
    const picked = await pickRestore();
    if (picked) setInspection(picked);
  };

  const confirmRestore = () => {
    if (!inspection) return;
    // `restore` has told the person what went wrong: the sheet stays open to try again or cancel
    void restore(inspection).then(
      () => setInspection(null),
      () => undefined
    );
  };

  return (
    <SectionPage title={t('settings.rows.backup')} lead={t('backup.lead')}>
      <div className={CARD}>
        <KeyValueRow
          label={t('backup.last')}
          value={lastBackupAt !== null ? fmt.dateTime(lastBackupAt) : t('backup.never')}
        />
      </div>
      <Button
        className={BUTTON}
        loading={busy === 'backup'}
        disabled={busy === 'restore' || !canExport}
        onClick={() => void createBackup()}
      >
        {ts('data.backupNow')}
      </Button>
      <p className="mx-0.5 mb-0 mt-3 text-m-sub text-ink-3">
        {canExport ? t('backup.what') : t('backup.unavailableHere')}
      </p>

      <SectionHead title={t('backup.restoreTitle')} />
      <p className="mx-0.5 mb-3 mt-0 text-m-sub text-ink-3">{t('backup.restoreLead')}</p>
      <Button
        variant="secondary"
        className={BUTTON}
        loading={busy === 'restore'}
        disabled={busy === 'backup'}
        onClick={() => void chooseRestore()}
      >
        {ts('data.restore')}
      </Button>

      <p className="mx-0.5 mb-0 mt-6 text-m-caption text-ink-4">{t('backup.osBackupNote')}</p>

      <RestoreBackupSheet
        inspection={inspection}
        onOpenChange={(open) => {
          if (!open) setInspection(null);
        }}
        onConfirm={confirmRestore}
        isPending={busy === 'restore'}
      />
    </SectionPage>
  );
}
