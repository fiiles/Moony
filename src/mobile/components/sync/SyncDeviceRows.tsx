import { useTranslation } from 'react-i18next';
import { History } from 'lucide-react';
import type { SyncDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { deviceLabelKey, devicePlatformKey, inactiveDays } from '@/utils/sync-devices';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { RowBadge } from '@/mobile/components/RowBadge';
import { deviceIcon } from '@/mobile/lib/device-icon';

interface SyncDeviceRowsProps {
  /** Active devices first (this one leading), then the "Zkontrolujte" ones. */
  devices: readonly SyncDevice[];
  /** The devices of the folded group ("Dřívější zařízení"): one row that opens them. */
  formerCount: number;
  /** Tapping this device's row opens its rename sheet. */
  onRename: (device: SyncDevice) => void;
  /** Tapping another device's row opens its device sheet. */
  onOpen: (device: SyncDevice) => void;
  onOpenFormer: () => void;
}

/**
 * The devices of the sync (README §13 entity rows): the name with its badges ("Zkontrolujte",
 * "Odebírá se…", "Neaktivní {n} dní"), and under it "This device" first, the platform and when the
 * device was last active. This device's row renames it; another device's row opens its device sheet
 * (details, "Je moje", "Odebrat zařízení"). Left or removed devices are one folded row. Labels wrap.
 */
export function SyncDeviceRows({
  devices,
  formerCount,
  onRename,
  onOpen,
  onOpenFormer,
}: SyncDeviceRowsProps) {
  const { t } = useTranslation('deviceSync');
  const { dateTime } = useFormat();

  return (
    <EntityRows>
      {devices.map((device) => {
        const Icon = deviceIcon(device.platform);
        const label = deviceLabelKey(device);
        const inactive = inactiveDays(device);
        const sub = [
          ...(device.thisDevice ? [t('devices.thisDevice')] : []),
          t(devicePlatformKey(device.platform)),
          device.lastActiveAt === null
            ? t('devices.noChange')
            : t('devices.activeAt', {
                when: dateTime(device.lastActiveAt, { dateStyle: 'medium', timeStyle: 'short' }),
              }),
          ...(device.held ? [t('devices.held')] : []),
        ].join(' · ');
        return (
          <EntityRow
            key={device.id}
            chip={
              <RowChip square>
                <Icon strokeWidth={1.75} />
              </RowChip>
            }
            title={
              <>
                {label ? t(label, { name: device.name }) : device.name}
                {device.status === 'check' && (
                  <RowBadge tone="loss">{t('devices.check.badge')}</RowBadge>
                )}
                {device.removalPending && <RowBadge>{t('devices.removing')}</RowBadge>}
                {inactive !== null && (
                  <RowBadge>{t('devices.inactive', { count: inactive })}</RowBadge>
                )}
              </>
            }
            sub={sub}
            onClick={device.thisDevice ? () => onRename(device) : () => onOpen(device)}
            chevron
            wrap
          />
        );
      })}
      {formerCount > 0 && (
        <EntityRow
          chip={
            <RowChip square>
              <History strokeWidth={1.75} />
            </RowChip>
          }
          title={t('devices.formerGroup', { count: formerCount })}
          onClick={onOpenFormer}
          chevron
          wrap
        />
      )}
    </EntityRows>
  );
}
