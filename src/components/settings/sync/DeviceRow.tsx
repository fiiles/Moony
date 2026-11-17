import { useTranslation } from 'react-i18next';
import type { SyncDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { sentences } from '@/utils/sentences';
import {
  canHideDevice,
  canKeepDevice,
  canRemoveDevice,
  checkReasonKey,
  deviceLabelKey,
  devicePlatformKey,
  formerLineKey,
  inactiveDays,
  joinedViaKey,
} from '@/utils/sync-devices';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';

interface DeviceRowProps {
  device: SyncDevice;
  /** Devices can be removed from this sync (`removalSupported`, no key change of ours waiting). */
  removalAvailable: boolean;
  /** The sync server is out of reach: "Odebrat…" waits, described by the card's line (`useServerGate`). */
  serverProps?: { 'aria-disabled'?: true; 'aria-describedby'?: string };
  onRename: (device: SyncDevice) => void;
  onRemove: (device: SyncDevice) => void;
  onKeep: (device: SyncDevice) => void;
  keeping: boolean;
  /** "Odstranit ze seznamu" of a former row. */
  onHide: (device: SyncDevice) => void;
  hiding: boolean;
}

/**
 * One device of the list (design system: a settings row): the name with its badges, under it the
 * platform, last activity and app version, then how it joined; the actions sit at the right edge
 * ("Přejmenovat" for this device, "Je moje" and "Odebrat…" where they apply, "Odstranit ze
 * seznamu" on a former row). A "Zkontrolujte" row says why in the open. Labels wrap, never
 * truncate: the actions drop under the text when the row is narrow.
 */
export function DeviceRow({
  device,
  removalAvailable,
  serverProps,
  onRename,
  onRemove,
  onKeep,
  keeping,
  onHide,
  hiding,
}: DeviceRowProps) {
  const { t } = useTranslation('deviceSync');
  const { date, dateTime, locale } = useFormat();

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'medium' });
  const label = deviceLabelKey(device);
  const inactive = inactiveDays(device);
  const reasonKey = checkReasonKey(device);
  const viaKey = device.status === 'former' ? null : joinedViaKey(device);
  const removable = removalAvailable && canRemoveDevice(device);

  const line =
    device.status === 'former'
      ? [
          t(devicePlatformKey(device.platform)),
          t(formerLineKey(device), {
            date: device.stateAt === null ? '' : day(device.stateAt),
            by: device.stateBy ?? '',
          }),
        ]
      : [
          t(devicePlatformKey(device.platform)),
          device.lastActiveAt === null
            ? t('devices.noChange')
            : t('devices.activeAt', {
                when: dateTime(device.lastActiveAt, { dateStyle: 'medium', timeStyle: 'short' }),
              }),
          t('devices.version', { version: device.appVersion }),
        ];
  const joined = [
    viaKey ? t(viaKey, { date: day(device.joinedAt), name: device.addedBy ?? '' }) : null,
    inactive !== null ? t('devices.inactiveHint') : null,
  ].filter((part): part is string => part !== null);

  return (
    <div
      className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2 border-b border-line-soft py-3 last:border-b-0"
      data-testid="sync-device-row"
    >
      <div className="min-w-0 flex-1 basis-[280px]">
        <b className="block break-words text-body font-600 text-ink">
          <span className="inline-flex flex-wrap items-center gap-2">
            {label ? t(label, { name: device.name }) : device.name}
            {device.thisDevice && <Badge variant="dark">{t('devices.thisDevice')}</Badge>}
            {device.status === 'check' && <Badge variant="loss">{t('devices.check.badge')}</Badge>}
            {device.removalPending && <Badge variant="default">{t('devices.removing')}</Badge>}
            {device.held && <Badge variant="loss">{t('devices.held')}</Badge>}
            {inactive !== null && (
              <Badge variant="default">{t('devices.inactive', { count: inactive })}</Badge>
            )}
          </span>
        </b>
        <small className="mt-0.5 block break-words text-caption text-ink-4">
          {line.join(' · ')}
        </small>
        {joined.length > 0 && (
          <small className="mt-0.5 block break-words text-caption text-ink-4">
            {joined.join(' · ')}
          </small>
        )}
        {reasonKey && (
          <p className="m-0 mt-1.5 text-caption text-ink-2">
            {sentences(locale, [
              t(reasonKey, { by: device.addedBy ?? '' }),
              canKeepDevice(device) ? t('devices.check.keepNote') : null,
            ])}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">
        {device.thisDevice && (
          <Button type="button" variant="outline" size="sm" onClick={() => onRename(device)}>
            {t('devices.rename')}
          </Button>
        )}
        {canKeepDevice(device) && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            loading={keeping}
            onClick={() => onKeep(device)}
          >
            {t('devices.check.keep')}
          </Button>
        )}
        {removable && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onRemove(device)}
            {...serverProps}
          >
            {t('devices.remove')}
          </Button>
        )}
        {canHideDevice(device) && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            loading={hiding}
            onClick={() => onHide(device)}
          >
            {t('devices.hide')}
          </Button>
        )}
      </div>
    </div>
  );
}
