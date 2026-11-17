import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown } from 'lucide-react';
import type { SyncDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import {
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
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';

interface DeviceRowProps {
  device: SyncDevice;
  /** Devices can be removed from this sync (`removalSupported`, no key change of ours waiting). */
  removalAvailable: boolean;
  onRename: (device: SyncDevice) => void;
  onRemove: (device: SyncDevice) => void;
  onKeep: (device: SyncDevice) => void;
  keeping: boolean;
}

/**
 * One device of the list (design system: a settings row). Active rows open for their details and
 * "Odebrat…"; a "Zkontrolujte" row says why in the open and carries its actions; a former row is a
 * line with no action. Labels wrap, never truncate.
 */
export function DeviceRow({
  device,
  removalAvailable,
  onRename,
  onRemove,
  onKeep,
  keeping,
}: DeviceRowProps) {
  const { t } = useTranslation('deviceSync');
  const { date, dateTime } = useFormat();
  const [open, setOpen] = useState(false);

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'medium' });
  const label = deviceLabelKey(device);
  const inactive = inactiveDays(device);
  const reasonKey = checkReasonKey(device);
  const viaKey = joinedViaKey(device);
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

  const details = (
    <dl className="m-0 mt-2 grid gap-1 text-caption text-ink-3">
      {viaKey && (
        <dd className="m-0">
          {t(viaKey, {
            date: day(device.joinedAt),
            name: device.addedBy ?? '',
          })}
        </dd>
      )}
      {inactive !== null && <dd className="m-0">{t('devices.inactiveHint')}</dd>}
    </dl>
  );

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className="border-b border-line-soft py-3 last:border-b-0"
      data-testid="sync-device-row"
    >
      <div className="flex flex-wrap items-start justify-between gap-x-6 gap-y-2">
        <div className="min-w-0 flex-1 basis-[220px]">
          <b className="block break-words text-body font-600 text-ink">
            <span className="inline-flex flex-wrap items-center gap-2">
              {label ? t(label, { name: device.name }) : device.name}
              {device.thisDevice && <Badge variant="dark">{t('devices.thisDevice')}</Badge>}
              {device.status === 'check' && (
                <Badge variant="loss">{t('devices.check.badge')}</Badge>
              )}
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
          {reasonKey && (
            <p className="m-0 mt-1.5 text-caption text-ink-2">
              {t(reasonKey, { by: device.addedBy ?? '' })}
              {canKeepDevice(device) && ` ${t('devices.check.keepNote')}`}
            </p>
          )}
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {device.thisDevice && (
            <Button type="button" variant="outline" size="sm" onClick={() => onRename(device)}>
              {t('devices.rename')}
            </Button>
          )}
          {device.status === 'check' && canKeepDevice(device) && (
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
          {device.status === 'check' && removable && (
            <Button type="button" variant="outline" size="sm" onClick={() => onRemove(device)}>
              {t('devices.remove')}
            </Button>
          )}
          {device.status === 'active' && !device.thisDevice && (
            <CollapsibleTrigger asChild>
              <Button type="button" variant="ghost" size="sm" aria-expanded={open}>
                {open ? t('devices.hideDetails') : t('devices.details')}
                <ChevronDown className={open ? 'rotate-180' : undefined} />
              </Button>
            </CollapsibleTrigger>
          )}
        </div>
      </div>
      {device.status === 'active' && !device.thisDevice && (
        <CollapsibleContent>
          {details}
          {removable && (
            <div className="mt-2.5">
              <Button type="button" variant="outline" size="sm" onClick={() => onRemove(device)}>
                {t('devices.remove')}
              </Button>
            </div>
          )}
        </CollapsibleContent>
      )}
    </Collapsible>
  );
}
