import { useEffect, useState } from 'react';
import { Redirect } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ClipboardPaste,
  Eraser,
  KeyRound,
  Plus,
  QrCode,
  RefreshCw,
  Smartphone,
  Type,
  Unplug,
  X,
} from 'lucide-react';
import type { ReplaceImpactView, ReplaceOutcomeView, SyncDevice } from '@shared/schema';
import {
  useDeviceSyncFeatureState,
  useDeviceSyncStatus,
  useSyncDevices,
} from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { consentOf } from '@/utils/replace-impact';
import {
  IMPACT_CHANGED_KEY,
  replaceOutcomeToast,
} from '@/components/settings/sync/replace-outcome';
import { backupFileName } from '@/utils/sync-status';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { ReplaceConfirmSheet } from '@/mobile/components/sync/ReplaceConfirmSheet';
import { RecoveryJoinSheet } from '@/mobile/components/sync/RecoveryJoinSheet';
import { RenameDeviceSheet } from '@/mobile/components/sync/RenameDeviceSheet';
import { ScanFlow } from '@/mobile/components/sync/ScanFlow';
import { SetupSheet } from '@/mobile/components/sync/SetupSheet';
import { SyncDeviceRows } from '@/mobile/components/sync/SyncDeviceRows';
import { SyncBackupSavedSheet } from '@/mobile/components/sync/SyncBackupSavedSheet';
import { SyncKitSheet } from '@/mobile/components/sync/SyncKitSheet';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { SyncStatusBlock } from '@/mobile/components/sync/SyncStatusBlock';
import type { ReplaceSaved } from '@/mobile/lib/replace-saved';
import type { ScanMode } from '@/mobile/lib/sync-scan';
import { routes } from '@/mobile/shell/routes';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

type Open = 'kit' | 'kitJoin' | 'repair' | 'resync' | 'turnOff' | 'delete' | null;

/** A scan under way: joining (sync off) or adding a device (sync on); `paste` is the developer tool. */
type Flow = { mode: ScanMode; paste: boolean } | null;

/**
 * Nastavení → Sync (spec §11.3 and the mobile design §13): behind the feature flag, which sends the
 * page back to Nastavení when it is off. With sync off (or half set up) the page offers the ways in:
 * set up sync on this phone (or continue a setup that was started, or cancel it), scan a code from
 * another device and type a Recovery Kit code. With sync on it shows the status and "Sync now", the devices (this one can be renamed), "Add a device" (a
 * scan), the Recovery Kit behind the device password, re-sync, turn off and delete. With the
 * developer tools on, a pairing code can also be pasted (the simulator cannot scan); release builds
 * say nothing about it. The phone never opens a checkout.
 */
export default function SyncSection() {
  const { t } = useTranslation('deviceSync');
  const feature = useDeviceSyncFeatureState();
  const { data: status } = useDeviceSyncStatus();
  const { data: devices } = useSyncDevices();
  const { repairDevice, resyncDevice, turnOff, deleteSyncedData } = useDeviceSyncMutations();
  const { t: tc } = useTranslation('common');
  const [open, setOpen] = useState<Open>(null);
  const [flow, setFlow] = useState<Flow>(null);
  /** The first-device setup sheet is up. */
  const [settingUp, setSettingUp] = useState(false);
  const [renaming, setRenaming] = useState<SyncDevice | null>(null);
  /** A repair that could not send every change: re-syncing is offered next to it. */
  const [offerResync, setOfferResync] = useState(false);
  /** A replace that kept its backup: the sheet that says where it is (and offers a copy in Files). */
  const [saved, setSaved] = useState<ReplaceSaved | null>(null);

  // A password typed for repair, re-sync, turning off or delete is a mutation variable: it leaves
  // with the page
  const resetRepair = repairDevice.reset;
  const resetResync = resyncDevice.reset;
  const resetTurnOff = turnOff.reset;
  const resetDelete = deleteSyncedData.reset;
  useEffect(() => resetRepair, [resetRepair]);
  useEffect(() => resetResync, [resetResync]);
  useEffect(() => resetTurnOff, [resetTurnOff]);
  useEffect(() => resetDelete, [resetDelete]);

  if (!feature.loading && !feature.enabled) return <Redirect to={routes.settings} replace />;

  const close = (next: boolean) => {
    if (next) return;
    setOpen(null);
    // The password of a repair, a re-sync, turning off or a delete leaves with its sheet
    resetRepair();
    resetResync();
    resetTurnOff();
    resetDelete();
  };
  /**
   * After a join, repair or re-sync: a kept backup gets the sheet (where it is, a copy in Files on
   * iOS); a backup that is not kept only a toast.
   */
  const replaced = (headline: string, outcome: ReplaceOutcomeView) => {
    if (outcome.kept) setSaved({ headline, outcome });
    else replaceOutcomeToast(t, headline, outcome);
  };
  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  // As the re-sync below: the sheet goes first, and with it the typed password
  const repair = async (password: string) => {
    setOpen(null);
    try {
      const outcome = await repairDevice.mutateAsync(password);
      replaced(t('repair.success', { file: backupFileName(outcome.backupPath) }), outcome);
    } catch (error) {
      fail(error);
      if (error instanceof ApiError && error.key === DRAIN_INCOMPLETE_KEY) setOfferResync(true);
    } finally {
      resetRepair();
    }
  };

  // As the repair: the sheet goes first, and with it the typed password; the busy cover is over
  // the shell while the data is replaced, and a failure (a wrong password too) is a toast. The
  // mutation's own copy of the password goes once it has settled.
  const resync = async (password: string, impact: ReplaceImpactView) => {
    setOpen(null);
    try {
      const outcome = await resyncDevice.mutateAsync({
        password,
        // The consent of the numbers the sheet showed
        consent: consentOf('resync', impact),
      });
      replaced(t('resync.success', { file: backupFileName(outcome.backupPath) }), outcome);
    } catch (error) {
      fail(error);
      // What the sheet showed no longer holds: it opens again and reads the new numbers
      if (error instanceof ApiError && error.key === IMPACT_CHANGED_KEY) setOpen('resync');
    } finally {
      resetResync();
    }
  };

  // A wrong password shows under the field and keeps the sheet
  const turnOffNow = async (password: string) => {
    await turnOff.mutateAsync(password);
    toast(t('turnOff.success'));
  };

  // A setup that was started and not finished: the row resumes it, another cancels it
  const cancelSetup = async () => {
    if (turnOff.isPending) return;
    try {
      await turnOff.mutateAsync();
    } catch (error) {
      fail(error);
    }
  };

  const syncOn = status !== undefined && status.phase !== 'off' && status.phase !== 'settingUp';
  const pending = status?.pendingChanges ?? 0;
  const devTools = status?.devTools === true;
  const devPasteRow = devTools && (
    <SettingsRow
      icon={ClipboardPaste}
      title={t('phone.devPasteTitle')}
      onClick={() => setFlow({ mode: syncOn ? 'add' : 'join', paste: true })}
    />
  );

  return (
    <SectionPage title={t('phone.title')} lead={t('description')}>
      {status && !syncOn && (
        <>
          <p className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3">
            {t('intro.description')} {t('intro.privacy')}
          </p>
          <EntityRows>
            <SettingsRow
              icon={Smartphone}
              title={status.phase === 'settingUp' ? t('intro.resume') : t('phone.setUp')}
              onClick={() => setSettingUp(true)}
            />
            {status.phase === 'settingUp' && (
              <SettingsRow
                icon={X}
                title={t('intro.cancelSetup')}
                onClick={() => void cancelSetup()}
              />
            )}
            <SettingsRow
              icon={QrCode}
              title={t('phone.scan')}
              onClick={() => setFlow({ mode: 'join', paste: false })}
            />
            <SettingsRow
              icon={Type}
              title={t('phone.typeCode')}
              onClick={() => setOpen('kitJoin')}
            />
            {devPasteRow}
          </EntityRows>
        </>
      )}

      {status && syncOn && (
        <>
          <SyncStatusBlock
            status={status}
            offerResync={offerResync}
            onRepair={() => setOpen('repair')}
            onResync={() => setOpen('resync')}
            onTurnOff={() => setOpen('turnOff')}
          />

          <SectionHead title={t('devices.title')} />
          <SyncDeviceRows devices={devices ?? []} onRename={setRenaming} />
          <EntityRows>
            <SettingsRow
              icon={Plus}
              title={t('phone.addDevice')}
              onClick={() => setFlow({ mode: 'add', paste: false })}
            />
            {devPasteRow}
          </EntityRows>

          <EntityRows>
            <SettingsRow
              icon={KeyRound}
              title={t('status.showKit')}
              onClick={() => setOpen('kit')}
            />
          </EntityRows>

          <SectionHead title={t('danger.title')} />
          <EntityRows>
            <SettingsRow
              icon={RefreshCw}
              title={t('resync.button')}
              sub={t('resync.hint')}
              onClick={() => setOpen('resync')}
            />
            <SettingsRow
              icon={Unplug}
              title={t('turnOff.button')}
              sub={t('turnOff.hint')}
              onClick={() => setOpen('turnOff')}
            />
            <SettingsRow
              icon={Eraser}
              danger
              title={t('delete.button')}
              sub={t('delete.hint')}
              onClick={() => setOpen('delete')}
            />
          </EntityRows>
        </>
      )}

      <RenameDeviceSheet
        device={renaming}
        onOpenChange={(next) => {
          if (!next) setRenaming(null);
        }}
      />
      <SyncBackupSavedSheet saved={saved} onClose={() => setSaved(null)} />
      <SyncKitSheet open={open === 'kit'} onOpenChange={close} />
      <RecoveryJoinSheet open={open === 'kitJoin'} onOpenChange={close} onReplaced={replaced} />
      {/* Mounted only while a setup runs: unmounting forgets the Recovery Kit */}
      {settingUp && <SetupSheet onClose={() => setSettingUp(false)} />}
      {/* Mounted only while a scan runs: unmounting cancels the camera and the pairing */}
      {flow && (
        <ScanFlow
          mode={flow.mode}
          paste={flow.paste}
          devTools={devTools}
          onDone={() => setFlow(null)}
          onReplaced={replaced}
        />
      )}
      <ReplaceConfirmSheet
        open={open === 'repair'}
        onOpenChange={close}
        flow="repair"
        title={t('repair.title')}
        description={t('repair.description')}
        confirmLabel={t('repair.confirm')}
        onConfirm={(password) => {
          // Closes the sheet itself before the repair starts (`repair`)
          void repair(password);
          return Promise.resolve();
        }}
      />
      <ReplaceConfirmSheet
        open={open === 'resync'}
        onOpenChange={close}
        flow="resync"
        title={t('resync.title')}
        description={t('resync.description')}
        confirmLabel={t('resync.confirm')}
        onConfirm={(password, impact) => {
          // Closes the sheet itself before the replace starts (`resync`)
          void resync(password, impact);
          return Promise.resolve();
        }}
      />
      <SyncPasswordSheet
        open={open === 'turnOff'}
        onOpenChange={close}
        title={t('turnOff.title')}
        description={t('phone.turnOffBody')}
        note={pending > 0 ? t('turnOff.pendingChanges', { count: pending }) : undefined}
        confirmLabel={t('turnOff.confirm')}
        destructive
        onSubmit={turnOffNow}
      />
      <SyncPasswordSheet
        open={open === 'delete'}
        onOpenChange={close}
        title={t('delete.title')}
        description={t('delete.description')}
        confirmLabel={t('delete.confirm')}
        destructive
        onSubmit={async (password) => {
          await deleteSyncedData.mutateAsync(password);
          toast(t('delete.success'));
        }}
      />
    </SectionPage>
  );
}
