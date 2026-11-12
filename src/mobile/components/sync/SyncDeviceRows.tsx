import { useTranslation } from 'react-i18next';
import { Laptop, Smartphone, type LucideIcon } from 'lucide-react';
import type { SyncDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { devicePlatformKey, quietDays } from '@/utils/sync-devices';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';

interface SyncDeviceRowsProps {
  devices: readonly SyncDevice[];
  /** Tapping this device's row opens its rename sheet. */
  onRename: (device: SyncDevice) => void;
}

const PHONES: ReadonlySet<string> = new Set(['ios', 'android']);

/**
 * The devices of the sync account (README §13 entity rows): the name, and under it the platform,
 * when the device last changed something and, for this one, "This device" first; a device whose
 * changes are held says so last. Only this device's row does anything (a tap renames it, the
 * chevron says so); the others cannot be changed from here.
 */
export function SyncDeviceRows({ devices, onRename }: SyncDeviceRowsProps) {
  const { t } = useTranslation('deviceSync');
  const { dateTime } = useFormat();
  const now = Math.floor(Date.now() / 1000);

  return (
    <EntityRows>
      {devices.map((device) => {
        const Icon: LucideIcon = PHONES.has(device.platform) ? Smartphone : Laptop;
        const sub = [
          ...(device.thisDevice ? [t('devices.thisDevice')] : []),
          t(devicePlatformKey(device.platform)),
          device.lastActiveAt === null
            ? t('devices.noChange')
            : t('devices.lastChange', {
                when: dateTime(device.lastActiveAt, { dateStyle: 'medium', timeStyle: 'short' }),
              }),
          ...(device.held ? [t('devices.held')] : []),
          ...(quietDays(device.lastActiveAt, now) !== null
            ? [t('devices.quiet', { count: quietDays(device.lastActiveAt, now) ?? 0 })]
            : []),
        ].join(' · ');
        return (
          <EntityRow
            key={device.id}
            chip={
              <RowChip square>
                <Icon strokeWidth={1.75} />
              </RowChip>
            }
            title={device.name}
            sub={sub}
            onClick={device.thisDevice ? () => onRename(device) : undefined}
            chevron={device.thisDevice}
          />
        );
      })}
    </EntityRows>
  );
}
