import { useEffect, useState } from 'react';
import { Redirect } from 'wouter';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Eraser, KeyRound, Plus, QrCode, RefreshCw, Smartphone, Type, Unplug } from 'lucide-react';
import type { SyncDevice } from '@shared/schema';
import {
  useDeviceSyncFeatureState,
  useDeviceSyncStatus,
  useSyncDevices,
} from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { backupFileName } from '@/utils/sync-status';
import { EntityRows } from '@/mobile/components/EntityRow';
import { SectionHead } from '@/mobile/components/SectionHead';
import { SectionPage } from '@/mobile/components/SectionPage';
import { SettingsRow } from '@/mobile/components/SettingsRow';
import { RenameDeviceSheet } from '@/mobile/components/sync/RenameDeviceSheet';
import { SyncDeviceRows } from '@/mobile/components/sync/SyncDeviceRows';
import { SyncKitSheet } from '@/mobile/components/sync/SyncKitSheet';
import { SyncPasswordSheet } from '@/mobile/components/sync/SyncPasswordSheet';
import { SyncStatusBlock } from '@/mobile/components/sync/SyncStatusBlock';
import { ConfirmSheet } from '@/mobile/sheets/ConfirmSheet';
import { routes } from '@/mobile/shell/routes';

/** The key the backend sends when a repair could not send every change and replaced nothing. */
const DRAIN_INCOMPLETE_KEY = 'validation.syncRepairDrainIncomplete';

type Open = 'kit' | 'repair' | 'resync' | 'turnOff' | 'delete' | null;

/**
 * Nastavení → Sync (spec §11.3 and the mobile design §13): behind the feature flag, which sends the
 * page back to Nastavení when it is off. With sync off (or half set up) the page offers the ways in:
 * set up sync on this phone, scan a code from another device and type a Recovery Kit code. With sync
 * on it shows the status and "Sync now", the devices (this one can be renamed), the Recovery Kit
 * behind the device password, re-sync, turn off and delete. The phone never opens a checkout.
 */
export default function SyncSection() {
  const { t } = useTranslation('deviceSync');
  const feature = useDeviceSyncFeatureState();
  const { data: status } = useDeviceSyncStatus();
  const { data: devices } = useSyncDevices();
  const { repairDevice, resyncDevice, turnOff, deleteSyncedData } = useDeviceSyncMutations();
  const { t: tc } = useTranslation('common');
  const [open, setOpen] = useState<Open>(null);
  const [renaming, setRenaming] = useState<SyncDevice | null>(null);
  /** A repair that could not send every change: re-syncing is offered next to it. */
  const [offerResync, setOfferResync] = useState(false);

  // A password typed for re-sync or delete is a mutation variable: it leaves with the page
  const resetResync = resyncDevice.reset;
  const resetDelete = deleteSyncedData.reset;
  useEffect(() => resetResync, [resetResync]);
  useEffect(() => resetDelete, [resetDelete]);

  if (!feature.loading && !feature.enabled) return <Redirect to={routes.settings} replace />;

  const close = (next: boolean) => {
    if (next) return;
    setOpen(null);
    // The password of a re-sync or a delete leaves with its sheet
    resetResync();
    resetDelete();
  };
  const fail = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  const repair = async () => {
    setOpen(null);
    try {
      const path = await repairDevice.mutateAsync();
      toast(t('repair.success', { file: backupFileName(path) }));
    } catch (error) {
      fail(error);
      if (error instanceof ApiError && error.key === DRAIN_INCOMPLETE_KEY) setOfferResync(true);
    }
  };

  // As the repair: the sheet goes first, and with it the typed password; the busy cover is over
  // the shell while the data is replaced, and a failure (a wrong password too) is a toast. The
  // mutation's own copy of the password goes once it has settled.
  const resync = async (password: string) => {
    setOpen(null);
    try {
      const path = await resyncDevice.mutateAsync(password);
      toast(t('resync.success', { file: backupFileName(path) }));
    } catch (error) {
      fail(error);
    } finally {
      resetResync();
    }
  };

  const turnOffNow = async () => {
    try {
      await turnOff.mutateAsync();
      toast(t('turnOff.success'));
      setOpen(null);
    } catch (error) {
      fail(error);
    }
  };

  const syncOn = status !== undefined && status.phase !== 'off' && status.phase !== 'settingUp';
  const pending = status?.pendingChanges ?? 0;

  return (
    <SectionPage title={t('phone.title')} lead={t('description')}>
      {status && !syncOn && (
        <>
          <p className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3">
            {t('intro.description')} {t('intro.privacy')}
          </p>
          {/* The ways in are Task 11 (scan, type a code) and Task 12 (set up): their rows wait here */}
          <EntityRows>
            <SettingsRow disabled icon={Smartphone} title={t('phone.setUp')} />
            <SettingsRow disabled icon={QrCode} title={t('phone.scan')} />
            <SettingsRow disabled icon={Type} title={t('phone.typeCode')} />
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
            {/* Task 11 turns this on: pairing another device from the phone */}
            <SettingsRow disabled icon={Plus} title={t('phone.addDevice')} />
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
      <SyncKitSheet open={open === 'kit'} onOpenChange={close} />
      <ConfirmSheet
        open={open === 'repair'}
        onOpenChange={close}
        title={t('repair.title')}
        description={t('repair.description')}
        confirmLabel={t('repair.confirm')}
        onConfirm={() => void repair()}
      />
      <SyncPasswordSheet
        open={open === 'resync'}
        onOpenChange={close}
        title={t('resync.title')}
        description={t('resync.description')}
        note={pending > 0 ? t('resync.pendingChanges', { count: pending }) : undefined}
        confirmLabel={t('resync.confirm')}
        destructive
        closeOnSuccess={false}
        onSubmit={(password) => {
          // Closes the sheet itself before the replace starts (`resync`)
          void resync(password);
          return Promise.resolve();
        }}
      />
      <ConfirmSheet
        open={open === 'turnOff'}
        onOpenChange={close}
        title={t('turnOff.title')}
        description={
          pending > 0
            ? `${t('phone.turnOffBody')} ${t('turnOff.pendingChanges', { count: pending })}`
            : t('phone.turnOffBody')
        }
        confirmLabel={t('turnOff.confirm')}
        onConfirm={() => void turnOffNow()}
        isPending={turnOff.isPending}
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
