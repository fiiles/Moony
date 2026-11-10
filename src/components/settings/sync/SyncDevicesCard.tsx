import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { SyncDevice } from '@shared/schema';
import { useSyncDevices } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useFormat } from '@/lib/use-format';
import { translateApiError } from '@/lib/translate-api-error';
import { devicePlatformKey } from '@/utils/sync-devices';
import { SettingsCard, SettingsRow } from '@/components/settings/SettingsCard';
import { AddDeviceDialog } from '@/components/settings/sync/AddDeviceDialog';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/** Settings → Sync, sync on: the devices of the sync account, renaming this one, adding another. */
export function SyncDevicesCard() {
  const { t } = useTranslation('deviceSync');
  const { date, dateTime } = useFormat();
  const { data: devices } = useSyncDevices();
  const [renaming, setRenaming] = useState<SyncDevice | null>(null);
  const [adding, setAdding] = useState(false);

  const hint = (device: SyncDevice) =>
    [
      t(devicePlatformKey(device.platform)),
      t('devices.joined', {
        date: date(new Date(device.joinedAt * 1000), { dateStyle: 'medium' }),
      }),
      device.lastActiveAt === null
        ? t('devices.noChange')
        : t('devices.lastChange', {
            when: dateTime(device.lastActiveAt, { dateStyle: 'medium', timeStyle: 'short' }),
          }),
      t('devices.version', { version: device.appVersion }),
    ].join(' · ');

  return (
    <>
      <SettingsCard
        title={t('devices.title')}
        description={t('devices.description')}
        action={
          <Button type="button" onClick={() => setAdding(true)}>
            {t('devices.add')}
          </Button>
        }
      >
        <div>
          {(devices ?? []).map((device) => (
            <SettingsRow
              key={device.id}
              label={
                <span className="inline-flex flex-wrap items-center gap-2">
                  {device.name}
                  {device.thisDevice && <Badge variant="dark">{t('devices.thisDevice')}</Badge>}
                  {device.held && <Badge variant="loss">{t('devices.held')}</Badge>}
                </span>
              }
              hint={hint(device)}
            >
              {device.thisDevice && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setRenaming(device)}
                >
                  {t('devices.rename')}
                </Button>
              )}
            </SettingsRow>
          ))}
        </div>
      </SettingsCard>
      {renaming && <RenameDeviceDialog device={renaming} onClose={() => setRenaming(null)} />}
      {adding && <AddDeviceDialog onClose={() => setAdding(false)} />}
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
