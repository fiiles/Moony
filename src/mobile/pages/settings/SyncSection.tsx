import { useEffect, useState } from 'react';
import { Redirect, useLocation, useSearch } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ClipboardPaste,
  Globe,
  Eraser,
  Info,
  Plus,
  Link2,
  Smartphone,
  Unplug,
  X,
} from 'lucide-react';
import type {
  HeldGapView,
  ReplaceImpactView,
  ReplaceOutcomeView,
  SyncDevice,
} from '@shared/schema';
import {
  useDeviceSyncFeatureState,
  useLeftoverSyncProfile,
  useDeviceSyncStatus,
  useSyncDeviceGroups,
} from '@/hooks/use-device-sync';
import { syncIsOn } from '@/utils/safe-copy';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { consentOf } from '@/utils/replace-impact';
import { skipGapTexts } from '@/utils/device-sync-problem';
import { removalAvailable } from '@/utils/sync-removal';
import { useFormat } from '@/lib/use-format';
import {
  IMPACT_CHANGED_KEY,
  replaceOutcomeToast,
} from '@/components/settings/sync/replace-outcome';
import { backupFileName, secondsToDate } from '@/utils/sync-status';
import { Button } from '@/components/ui/button';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { Sheet } from '@/mobile/components/Sheet';
import { JoinChoiceSheet, type PhoneJoinMethod } from '@/mobile/components/sync/JoinChoiceSheet';
import { KitSection } from '@/mobile/components/sync/KitSection';
import { RemovedBlock } from '@/mobile/components/sync/RemovedNotice';
import { ReplaceConfirmSheet } from '@/mobile/components/sync/ReplaceConfirmSheet';
import { RecoveryJoinSheet } from '@/mobile/components/sync/RecoveryJoinSheet';
import { ChangeServerSheet } from '@/mobile/components/sync/ChangeServerSheet';
import { DeviceSheet, FormerDevicesSheet } from '@/mobile/components/sync/DeviceSheet';
import { RemoveDeviceSheet } from '@/mobile/components/sync/RemoveDeviceSheet';
import { RenameDeviceSheet } from '@/mobile/components/sync/RenameDeviceSheet';
import { ScanFlow } from '@/mobile/components/sync/ScanFlow';
import { SetupSheet } from '@/mobile/components/sync/SetupSheet';
import { SyncDeviceRows } from '@/mobile/components/sync/SyncDeviceRows';
import { SyncBackupSavedSheet } from '@/mobile/components/sync/SyncBackupSavedSheet';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { SyncStatusBlock } from '@/mobile/components/sync/SyncStatusBlock';
import type { ReplaceSaved } from '@/mobile/lib/replace-saved';
import { JOIN_PARAM, type ScanMode } from '@/mobile/lib/sync-scan';
import { routes, settingsPath } from '@/mobile/shell/routes';
import { sentences } from '@/mobile/lib/list-text';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

type Open = 'kitJoin' | 'server' | 'repair' | 'resync' | 'turnOff' | 'delete' | 'rejoin' | null;

/** A scan under way: joining (sync off) or adding a device (sync on); `paste`: the code is typed or pasted. */
type Flow = { mode: ScanMode; paste: boolean } | null;

/**
 * Nastavení → Sync (spec §11.3 and the mobile design §13): behind the feature flag, which sends the
 * page back to Nastavení when it is off. With sync off (or half set up) the page offers the ways in:
 * start syncing on this phone (or continue a setup that was started, or cancel it) and "Connect this
 * device", which opens a choice: scan a code from the computer or type a Recovery Kit code. With
 * sync on it shows the status and "Sync now", the devices (this one can be renamed), "Add a device"
 * (a scan), the Recovery Kit behind the device password, re-sync, turn off and delete. A
 * pairing code can also be entered manually, in every build. The phone never opens a checkout.
 */
export default function SyncSection() {
  const { t } = useTranslation('deviceSync');
  const feature = useDeviceSyncFeatureState();
  const { leftover, pending: leftoverPending, loading: leftoverLoading } = useLeftoverSyncProfile();
  const { data: status } = useDeviceSyncStatus();
  const { data: groups } = useSyncDeviceGroups();
  const { repairDevice, resyncDevice, turnOff, deleteSyncedData, skipMissingChange, keepDevice } =
    useDeviceSyncMutations();
  const { t: tc } = useTranslation('common');
  const { date, locale } = useFormat();
  const renderDate = (seconds: number) => {
    const when = secondsToDate(seconds);
    return when ? date(when, { dateStyle: 'long' }) : '';
  };
  const [open, setOpen] = useState<Open>(null);
  /** The "What syncs" sheet is open. */
  const [whatSyncsOpen, setWhatSyncsOpen] = useState(false);
  const [flow, setFlow] = useState<Flow>(null);
  /** The "Connect this device" choice sheet is open. */
  const [choosing, setChoosing] = useState(false);
  // "Připojit znovu" from the sheet at unlock arrives with `?join=1`: the choice opens once
  const search = useSearch();
  const [, navigate] = useLocation();
  const joinOnArrival = new URLSearchParams(search).has(JOIN_PARAM);
  const [arrivalHandled, setArrivalHandled] = useState(false);
  if (joinOnArrival && !arrivalHandled) {
    setArrivalHandled(true);
    setChoosing(true);
  }
  if (!joinOnArrival && arrivalHandled) setArrivalHandled(false);
  useEffect(() => {
    if (joinOnArrival) navigate(settingsPath('sync'), { replace: true });
  }, [joinOnArrival, navigate]);
  /** The first-device setup sheet is up. */
  const [settingUp, setSettingUp] = useState(false);
  const [renaming, setRenaming] = useState<SyncDevice | null>(null);
  /** The device whose sheet is open (by ID: the sheet reads the live list), and the folded group's sheet. */
  const [viewing, setViewing] = useState<string | null>(null);
  const [formerOpen, setFormerOpen] = useState(false);
  /** The devices whose removal sheet is open. */
  const [removing, setRemoving] = useState<{ ids: string[]; names: string[] } | null>(null);
  /** A repair that could not send every change: re-syncing is offered next to it. */
  const [offerResync, setOfferResync] = useState(false);
  /** A replace that kept its backup: the sheet that says where it is (and offers a copy in Files). */
  const [saved, setSaved] = useState<ReplaceSaved | null>(null);
  /** The confirmation of cancelling a setup that was started is open. */
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  /** The held change whose skip confirmation is open. */
  const [skipping, setSkipping] = useState<HeldGapView | null>(null);

  // A password typed for repair, re-sync, turning off or delete is a mutation variable: it leaves
  // with the page
  const resetRepair = repairDevice.reset;
  const resetResync = resyncDevice.reset;
  const resetTurnOff = turnOff.reset;
  const resetDelete = deleteSyncedData.reset;
  const resetSkip = skipMissingChange.reset;
  useEffect(() => resetRepair, [resetRepair]);
  useEffect(() => resetResync, [resetResync]);
  useEffect(() => resetTurnOff, [resetTurnOff]);
  useEffect(() => resetDelete, [resetDelete]);
  useEffect(() => resetSkip, [resetSkip]);

  if (!feature.loading && !leftoverLoading && !feature.enabled && !leftover) {
    return <Redirect to={routes.settings} replace />;
  }

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

  // A wrong password (or a change not 14 days old yet) shows under the field and keeps the sheet
  const skipNow = async (password: string) => {
    if (!skipping) return;
    const skipped = await skipMissingChange.mutateAsync({ password, deviceId: skipping.deviceId });
    const texts = skipGapTexts(skipping, t, renderDate);
    toast(skipped ? texts.skipped : texts.unchanged);
  };

  // A wrong password shows under the field and keeps the sheet
  const turnOffNow = async (password: string) => {
    try {
      await turnOff.mutateAsync(password);
    } catch (error) {
      // The typed password is the mutation's variable: a failure must not leave it behind
      resetTurnOff();
      throw error;
    }
    toast(t('turnOff.success'));
  };

  // A setup that was started and not finished: the row resumes it, another cancels it
  const cancelSetup = async () => {
    if (turnOff.isPending) return;
    try {
      await turnOff.mutateAsync();
      setConfirmingCancel(false);
    } catch (error) {
      fail(error);
    }
  };

  // A build without sync: only a profile that has sync on gets a page, with the way out
  if (!feature.loading && !feature.enabled && leftover) {
    return (
      <SectionPage title={t('phone.title')} lead={t('leftover.text')}>
        <EntityRows>
          <SettingsRow
            icon={Unplug}
            title={t('leftover.button')}
            onClick={() => setOpen('turnOff')}
          />
        </EntityRows>
        <SyncPasswordSheet
          open={open === 'turnOff'}
          onOpenChange={close}
          title={t('turnOff.title')}
          description={sentences(locale, [t('leftover.phoneDescription'), t('leftover.noServer')])}
          note={
            leftoverPending > 0
              ? t('turnOff.pendingChanges', { count: leftoverPending })
              : undefined
          }
          confirmLabel={t('turnOff.confirm')}
          destructive
          onSubmit={turnOffNow}
        />
      </SectionPage>
    );
  }

  const viewedDevice =
    [...(groups?.active ?? []), ...(groups?.check ?? [])].find((d) => d.id === viewing) ?? null;
  // "Je moje" can find the row gone or no longer trusted: the list is re-read either way
  const keep = async (device: SyncDevice) => {
    try {
      await keepDevice.mutateAsync(device.id);
    } catch {
      toast.error(t('devices.check.keepFailed'));
    }
  };

  const syncOn = syncIsOn(feature.enabled, status);
  const pending = status?.pendingChanges ?? 0;
  const deleteAfterwards = t(
    status?.ownServer ? 'delete.afterwardsOwnServer' : 'delete.afterwards'
  );
  const joinWith = (method: PhoneJoinMethod) => {
    if (method === 'kit') setOpen('kitJoin');
    else setFlow({ mode: 'join', paste: method === 'paste' });
  };
  const enterCodeRow = (
    <SettingsRow
      icon={ClipboardPaste}
      title={t('phone.enterCodeTitle')}
      onClick={() => setFlow({ mode: syncOn ? 'add' : 'join', paste: true })}
    />
  );

  return (
    <SectionPage title={t('phone.title')} lead={t('description')}>
      {status && !syncOn && (
        <>
          {status.removedNotice && (
            <RemovedBlock notice={status.removedNotice} onRejoin={() => setChoosing(true)} />
          )}
          <p className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3">
            {sentences(locale, [t('intro.description'), t('intro.privacy')])}
          </p>
          <EntityRows>
            <SettingsRow
              icon={Smartphone}
              title={status.phase === 'settingUp' ? t('intro.resume') : t('intro.startTitle')}
              sub={status.phase === 'settingUp' ? undefined : t('intro.startLead')}
              onClick={() => setSettingUp(true)}
            />
            {status.phase === 'settingUp' && (
              <SettingsRow
                icon={X}
                title={t('intro.cancelSetup')}
                onClick={() => setConfirmingCancel(true)}
              />
            )}
            <SettingsRow
              icon={Link2}
              title={t('intro.joinTitle')}
              sub={t('intro.joinLead')}
              onClick={() => setChoosing(true)}
            />
          </EntityRows>
          <EntityRows>
            <SettingsRow
              icon={Info}
              title={t('whatSyncs.title')}
              onClick={() => setWhatSyncsOpen(true)}
            />
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
            onRejoin={() => setOpen('rejoin')}
            onSkipMissing={setSkipping}
          />

          <SectionHead title={t('devices.title')} />
          <p className="mx-0.5 mb-2.5 mt-0 text-m-sub text-ink-3">{t('devices.description')}</p>
          <SyncDeviceRows
            devices={[...(groups?.active ?? []), ...(groups?.check ?? [])]}
            formerCount={groups?.former.length ?? 0}
            onRename={setRenaming}
            onOpen={(device) => setViewing(device.id)}
            onOpenFormer={() => setFormerOpen(true)}
          />
          <p
            className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3"
            data-testid={status.removalSupported ? 'sync-limits' : 'sync-unsupported'}
          >
            {t(status.removalSupported ? 'devices.underList' : 'devices.unsupported')}
          </p>
          <EntityRows>
            <SettingsRow
              icon={Plus}
              title={t('phone.addDevice')}
              onClick={() => setFlow({ mode: 'add', paste: false })}
            />
            {enterCodeRow}
          </EntityRows>

          <EntityRows>
            {status.serverAddress && (
              <SettingsRow
                icon={Globe}
                title={t('server.change')}
                onClick={() => setOpen('server')}
              />
            )}
          </EntityRows>

          <KitSection status={status} />

          <SectionHead title={t('danger.title')} />
          <EntityRows>
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
          <EntityRows>
            <SettingsRow
              icon={Info}
              title={t('whatSyncs.title')}
              onClick={() => setWhatSyncsOpen(true)}
            />
          </EntityRows>
        </>
      )}

      <Sheet
        open={whatSyncsOpen}
        onOpenChange={setWhatSyncsOpen}
        title={t('whatSyncs.title')}
        description={t('whatSyncs.body')}
        footer={
          <Button
            className="h-auto min-h-12 flex-1 whitespace-normal rounded-[10px] py-2 text-center text-m-row"
            onClick={() => setWhatSyncsOpen(false)}
          >
            {tc('buttons.close')}
          </Button>
        }
      >
        <></>
      </Sheet>
      <RenameDeviceSheet
        device={renaming}
        onOpenChange={(next) => {
          if (!next) setRenaming(null);
        }}
      />
      <DeviceSheet
        device={viewedDevice}
        removalAvailable={removalAvailable(status)}
        keeping={keepDevice.isPending}
        onOpenChange={(next) => {
          if (!next) setViewing(null);
        }}
        onKeep={(device) => void keep(device)}
        onRemove={(device) => {
          setViewing(null);
          setRemoving({ ids: [device.id], names: [device.name] });
        }}
      />
      <FormerDevicesSheet
        devices={groups?.former ?? []}
        open={formerOpen}
        onOpenChange={setFormerOpen}
      />
      {/* Mounted only while open: unmounting forgets the typed password and a new kit */}
      {removing && (
        <RemoveDeviceSheet
          deviceIds={removing.ids}
          deviceNames={removing.names}
          onClose={() => setRemoving(null)}
        />
      )}
      <SyncBackupSavedSheet saved={saved} onClose={() => setSaved(null)} />
      <ConfirmSheet
        open={confirmingCancel}
        onOpenChange={(next) => {
          if (!turnOff.isPending) setConfirmingCancel(next);
        }}
        title={t('intro.cancelConfirm.title')}
        description={t('intro.cancelConfirm.description')}
        confirmLabel={t('intro.cancelSetup')}
        cancelLabel={t('intro.cancelConfirm.keep')}
        onConfirm={() => void cancelSetup()}
        isPending={turnOff.isPending}
      />
      <JoinChoiceSheet open={choosing} onOpenChange={setChoosing} onPick={joinWith} />
      {/* Mounted only while open: unmounting forgets the typed password and the server check */}
      {open === 'server' && <ChangeServerSheet onClose={() => setOpen(null)} />}
      <RecoveryJoinSheet open={open === 'kitJoin'} onOpenChange={close} onReplaced={replaced} />
      {/* Mounted only while a setup runs: unmounting forgets the Recovery Kit */}
      {settingUp && (
        <SetupSheet
          onClose={() => setSettingUp(false)}
          onAddDevice={() => {
            setSettingUp(false);
            setFlow({ mode: 'add', paste: false });
          }}
        />
      )}
      {/* Mounted only while a scan runs: unmounting cancels the camera and the pairing */}
      {flow && (
        <ScanFlow
          mode={flow.mode}
          paste={flow.paste}
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
        description={sentences(locale, [t('phone.turnOffBody'), t('turnOff.noCutOff')])}
        note={pending > 0 ? t('turnOff.pendingChanges', { count: pending }) : undefined}
        confirmLabel={t('turnOff.confirm')}
        destructive
        onSubmit={turnOffNow}
      />
      <SyncPasswordSheet
        open={open === 'rejoin'}
        onOpenChange={close}
        title={t('rejoin.title')}
        description={t('rejoin.description')}
        confirmLabel={t('problem.rejoin.button')}
        closeOnSuccess={false}
        onSubmit={async (password) => {
          try {
            await turnOff.mutateAsync(password);
          } catch (error) {
            resetTurnOff();
            throw error;
          }
          // The password was the mutation's variable: it goes before the next sheet opens
          resetTurnOff();
          // Sync is off here now (the data stay): the join choice follows
          setOpen(null);
          setChoosing(true);
        }}
      />
      <SyncPasswordSheet
        open={skipping !== null}
        onOpenChange={(next) => {
          if (next) return;
          setSkipping(null);
          // The typed password leaves with its sheet
          resetSkip();
        }}
        title={
          skipping ? skipGapTexts(skipping, t, renderDate).title : t('skipGap.title', { count: 1 })
        }
        description={skipping ? skipGapTexts(skipping, t, renderDate).description : ''}
        confirmLabel={t('skipGap.confirm')}
        destructive
        onSubmit={skipNow}
      />
      <SyncPasswordSheet
        open={open === 'delete'}
        onOpenChange={close}
        title={t('delete.title')}
        description={`${t('delete.description')} ${deleteAfterwards}`}
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
