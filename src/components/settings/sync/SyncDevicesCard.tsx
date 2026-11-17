import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncDevice } from '@shared/schema';
import { useDeviceSyncStatus, useSyncDeviceGroups } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerGate } from '@/hooks/use-server-gate';
import { translateApiError } from '@/lib/translate-api-error';
import { removalAvailable } from '@/utils/sync-removal';
import { formerDeviceIds } from '@/utils/sync-devices';
import { SettingsCard } from '@/components/settings/SettingsCard';
import { AddDeviceDialog } from '@/components/settings/sync/AddDeviceDialog';
import { DeviceRow } from '@/components/settings/sync/DeviceRow';
import { RemoveDeviceDialog } from '@/components/settings/sync/RemoveDeviceDialog';
import { ServerNeededNote } from '@/components/settings/sync/ServerNeededNote';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/** Settings → Sync, sync on: the devices of the sync account, renaming this one, adding or removing another. */
export function SyncDevicesCard() {
  const { t } = useTranslation('deviceSync');
  const { data: groups } = useSyncDeviceGroups();
  const { data: status } = useDeviceSyncStatus();
  const { t: tc } = useTranslation('common');
  const { keepDevice, hideFormerDevices } = useDeviceSyncMutations();
  const [renaming, setRenaming] = useState<SyncDevice | null>(null);
  const [removing, setRemoving] = useState<SyncDevice | null>(null);
  const [adding, setAdding] = useState(false);
  const available = removalAvailable(status);
  const unsupported = status !== undefined && !status.removalSupported;
  // Adding and removing a device need the server; renaming this one, "Je moje" and taking a former
  // device off the list do not
  const server = useServerGate();

  // "Je moje" can find the row gone or no longer trusted: the list is re-read either way
  const keep = async (device: SyncDevice) => {
    try {
      await keepDevice.mutateAsync(device.id);
    } catch {
      toast.error(t('devices.check.keepFailed'));
    }
  };

  // Off this device's list only: the device stays blocked. `null` hides every former device.
  const [hidingId, setHidingId] = useState<string | null>(null);
  const hide = async (ids: string[], one: string | null) => {
    setHidingId(one);
    try {
      await hideFormerDevices.mutateAsync(ids);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };
  const hidingOne = (device: SyncDevice) => hideFormerDevices.isPending && hidingId === device.id;

  const row = (device: SyncDevice) => (
    <DeviceRow
      key={device.id}
      device={device}
      removalAvailable={available}
      serverProps={server.props}
      onRename={setRenaming}
      onRemove={setRemoving}
      onKeep={keep}
      keeping={keepDevice.isPending && keepDevice.variables === device.id}
      onHide={(d) => void hide([d.id], d.id)}
      hiding={hidingOne(device)}
    />
  );
  const former = groups?.former ?? [];

  return (
    <>
      <SettingsCard
        title={t('devices.title')}
        description={t('devices.description')}
        action={
          <Button type="button" onClick={() => setAdding(true)} {...server.props}>
            {t('devices.add')}
          </Button>
        }
      >
        <div>
          {(groups?.active ?? []).map(row)}
          {(groups?.check ?? []).map(row)}
        </div>
        {server.blocked && <ServerNeededNote id={server.noteId} className="mt-3" />}
        {unsupported ? (
          <p className="m-0 mt-3 text-micro font-500 text-ink-3" data-testid="sync-unsupported">
            {t('devices.unsupported')}
          </p>
        ) : (
          <p className="m-0 mt-3 text-micro font-500 text-ink-3" data-testid="sync-limits">
            {t('devices.underList')}
          </p>
        )}
        {former.length > 0 && (
          <Collapsible className="mt-3">
            <CollapsibleTrigger className="cursor-pointer border-0 bg-transparent p-0 text-micro font-500 text-ink-3 underline">
              {t('devices.formerGroup', { count: former.length })}
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="mt-2 flex flex-wrap items-center justify-between gap-x-6 gap-y-1">
                <p className="m-0 min-w-0 flex-1 basis-[280px] text-caption text-ink-3">
                  {t('devices.hideNote')}
                </p>
                {/* One former device has its own button in its row: "all" only for several */}
                {former.length > 1 && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    loading={hideFormerDevices.isPending && hidingId === null}
                    onClick={() => void hide(formerDeviceIds(former), null)}
                  >
                    {t('devices.hideAll')}
                  </Button>
                )}
              </div>
              {former.map(row)}
            </CollapsibleContent>
          </Collapsible>
        )}
      </SettingsCard>
      {renaming && <RenameDeviceDialog device={renaming} onClose={() => setRenaming(null)} />}
      {adding && <AddDeviceDialog onClose={() => setAdding(false)} />}
      {removing && (
        <RemoveDeviceDialog
          deviceIds={[removing.id]}
          deviceNames={[removing.name]}
          onClose={() => setRemoving(null)}
        />
      )}
    </>
  );
}

function RenameDeviceDialog({ device, onClose }: { device: SyncDevice; onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { renameDevice } = useDeviceSyncMutations();
  const [name, setName] = useState(device.name);
  const trimmed = name.trim();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (trimmed === '' || renameDevice.isPending) return;
    try {
      await renameDevice.mutateAsync(trimmed);
      onClose();
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && !renameDevice.isPending && onClose()}>
      <DialogContent className="max-w-[440px]">
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t('devices.renameTitle')}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-1.5">
            <Label htmlFor="sync-rename-device">{t('setup.device.name')}</Label>
            <Input
              id="sync-rename-device"
              value={name}
              maxLength={60}
              onChange={(event) => setName(event.target.value)}
            />
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={onClose}
              disabled={renameDevice.isPending}
            >
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" loading={renameDevice.isPending} disabled={trimmed === ''}>
              {tc('buttons.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
