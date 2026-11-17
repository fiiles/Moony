import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useDeviceSyncFeature, useDeviceSyncStatus, useSyncDevices } from '@/hooks/use-device-sync';
import { protectionLine, syncIsOn } from '@/utils/safe-copy';
import { useFormat } from '@/lib/use-format';
import { notInBackup } from '@/utils/sync-attachments';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { useBackup } from '@/mobile/hooks/use-backup';
import { SyncBackupRows } from '@/mobile/components/sync/SyncBackupRows';
import { RestoreBackupSheet } from '@/mobile/sheets/RestoreBackupSheet';
import { DataTools } from './DataTools';
import type { BackupInspection } from '@shared/schema';
import { phoneCopyKey } from '@/mobile/lib/platform-copy';

/** The card the date sits in (`.m-card`). */
const CARD = 'mb-3.5 rounded-r4 border border-line bg-paper px-3 shadow-e1';

/** The 48 pt buttons of the section's two actions. */
const BUTTON =
  'h-auto min-h-12 whitespace-normal py-2 text-center w-full rounded-[10px] text-m-row';

/**
 * Nastavení → Záloha a přenos (spec §8.9): when the last backup was made, "Zálohovat nyní", the
 * restore, and the rest of the desktop's data card (`DataTools`: the JSON export, the database check
 * and the attachments' status). A backup is one encrypted file the person keeps in Files (iCloud Drive or another
 * folder); the same file moves a profile from Moony on the computer to the phone and back, with
 * the password that was in use when it was made. There is no automatic backup: the "backup is due"
 * reminder says when to make one. Restoring asks first, in a sheet that names what the file holds.
 * Making a backup works on iOS only for now: elsewhere the button is off and a sentence says so.
 * Restoring (a picked file, read by Rust) is verified on iOS only too: Android's picker returns
 * `content://` URIs that Rust cannot read yet, which is a follow-up.
 */
export default function BackupSection() {
  const { t } = useTranslation('mobile');
  const { t: ts } = useTranslation('settings');
  const { t: tSync } = useTranslation('deviceSync');
  const fmt = useFormat();
  const { createBackup, pickRestore, inspectPath, restore, busy, lastBackupAt, canExport } =
    useBackup();
  const [inspection, setInspection] = useState<BackupInspection | null>(null);
  // Sync on: documents and photos not downloaded here are not in a backup made here (spec §5.7).
  // The status exists only with the feature on, and counts nothing while sync is off.
  const { data: syncStatus } = useDeviceSyncStatus();
  const featureOn = useDeviceSyncFeature();
  const syncOn = syncIsOn(featureOn, syncStatus);
  const missing = syncOn && syncStatus ? notInBackup(syncStatus.attachments) : 0;
  const { data: syncDevices } = useSyncDevices();
  const protection = protectionLine(featureOn, syncStatus, syncDevices, Date.now() / 1000);

  const chooseRestore = async () => {
    const picked = await pickRestore();
    if (picked) setInspection(picked);
  };

  /** The listed backup being read: its own Restore button spins, not the file picker's. */
  const [listedBusy, setListedBusy] = useState<string | null>(null);
  const restoreListed = async (path: string) => {
    setListedBusy(path);
    try {
      const read = await inspectPath(path);
      if (read) setInspection(read);
    } finally {
      setListedBusy(null);
    }
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
    <SectionPage
      title={t('settings.rows.backup')}
      lead={t(syncOn ? 'backup.leadSync' : 'backup.lead')}
    >
      {protection && (
        <p className="mx-0.5 mb-3 mt-0 text-m-sub text-ink-3">
          {tSync('protection.line', {
            count: protection.devices,
            age: tSync(`age.${protection.age.key}`, { count: protection.age.count }),
          })}
        </p>
      )}
      <div className={CARD}>
        <KeyValueRow
          label={t('backup.last')}
          wrap
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
        {canExport ? t(phoneCopyKey('backup.what')) : t('backup.unavailableHere')}
      </p>
      {missing > 0 && (
        <p className="mx-0.5 mb-0 mt-2 text-m-sub text-ink-3">
          {t('backup.notHere', { count: missing })}
        </p>
      )}

      <SectionHead title={t('backup.restoreTitle')} />
      <p className="mx-0.5 mb-3 mt-0 text-m-sub text-ink-3">
        {t(syncOn ? 'backup.restoreLeadSync' : 'backup.restoreLead')}
      </p>
      <Button
        variant="secondary"
        className={BUTTON}
        loading={busy === 'restore' && listedBusy === null}
        disabled={busy === 'backup'}
        onClick={() => void chooseRestore()}
      >
        {ts('data.restore')}
      </Button>

      <SyncBackupRows
        onRestore={(path) => void restoreListed(path)}
        disabled={busy !== null}
        restoring={listedBusy}
      />

      <DataTools />

      <p className="mx-0.5 mb-0 mt-6 text-m-caption text-ink-4">
        {t(phoneCopyKey(syncOn ? 'backup.osBackupNoteSync' : 'backup.osBackupNote'))}
      </p>

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
