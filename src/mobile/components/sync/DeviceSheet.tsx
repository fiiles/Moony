import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useServerGate } from '@/hooks/use-server-gate';
import { Button } from '@/components/ui/button';
import type { SyncDevice } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { sentences } from '@/utils/sentences';
import {
  canKeepDevice,
  canRemoveDevice,
  checkReasonKey,
  deviceLabelKey,
  devicePlatformKey,
  formerDeviceIds,
  formerLineKey,
  inactiveDays,
  joinedViaKey,
} from '@/utils/sync-devices';
import { EntityRow, EntityRows, RowChip } from '@/mobile/components/EntityRow';
import { KeyValueRow } from '@/mobile/components/KeyValueRow';
import { deviceIcon } from '@/mobile/lib/device-icon';
import { Sheet } from '@/mobile/components/Sheet';

/** The 48 pt buttons of a sheet's body. */
const BUTTON =
  'h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-2 text-center text-m-row';

/** The card a group of label-value rows sits in (`.m-card`), rows `line-soft` apart. */
const CARD =
  'mb-3.5 divide-y divide-line-soft rounded-r4 border border-line bg-paper px-3 shadow-e1';

interface DeviceSheetProps {
  /** The device shown (read live from the list); `null` closes the sheet. */
  device: SyncDevice | null;
  /** Devices can be removed from this sync now (`removalAvailable`). */
  removalAvailable: boolean;
  keeping: boolean;
  onOpenChange: (open: boolean) => void;
  onKeep: (device: SyncDevice) => void;
  onRemove: (device: SyncDevice) => void;
}

/**
 * Another device's sheet (spec §6.1): its details as label-value rows, why it says "Zkontrolujte"
 * with "Je moje" where that helps, and "Odebrat zařízení" as a danger text button. Rows wrap, the
 * sheet scrolls.
 */
export function DeviceSheet({
  device,
  removalAvailable,
  keeping,
  onOpenChange,
  onKeep,
  onRemove,
}: DeviceSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { date, dateTime, locale } = useFormat();
  const shown = device;
  const label = shown ? deviceLabelKey(shown) : null;
  const reasonKey = shown ? checkReasonKey(shown) : null;
  const viaKey = shown ? joinedViaKey(shown) : null;
  const inactive = shown ? inactiveDays(shown) : null;
  const removable = shown !== null && removalAvailable && canRemoveDevice(shown);
  // Removing needs the server; the details and "Je moje" do not
  const server = useServerGate();
  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'medium' });

  return (
    <Sheet
      open={device !== null}
      onOpenChange={onOpenChange}
      title={shown ? (label ? t(label, { name: shown.name }) : shown.name) : ''}
    >
      {shown && (
        <div>
          {reasonKey && (
            <p className="mx-0.5 mb-3.5 mt-0 text-m-lead text-ink-2">
              {sentences(locale, [
                t(reasonKey, { by: shown.addedBy ?? '' }),
                canKeepDevice(shown) ? t('devices.check.keepNote') : null,
              ])}
            </p>
          )}
          <div className={CARD}>
            <KeyValueRow
              label={t('devices.sheet.platform')}
              value={t(devicePlatformKey(shown.platform))}
              wrap
            />
            <KeyValueRow
              label={t('devices.sheet.activity')}
              value={
                shown.lastActiveAt === null
                  ? t('devices.noChange')
                  : dateTime(shown.lastActiveAt, { dateStyle: 'medium', timeStyle: 'short' })
              }
              sub={inactive !== null ? t('devices.inactive', { count: inactive }) : undefined}
              wrap
            />
            <KeyValueRow
              label={t('devices.sheet.version')}
              value={t('devices.version', { version: shown.appVersion })}
              wrap
            />
            {viaKey && (
              <KeyValueRow
                label={t('devices.sheet.joined')}
                value={t(viaKey, { date: day(shown.joinedAt), name: shown.addedBy ?? '' })}
                stack
              />
            )}
          </div>
          {inactive !== null && (
            <p className="mx-0.5 mb-3.5 mt-0 text-m-sub text-ink-3">{t('devices.inactiveHint')}</p>
          )}
          <div className="grid gap-1">
            {canKeepDevice(shown) && (
              <Button
                variant="secondary"
                className={BUTTON}
                loading={keeping}
                onClick={() => onKeep(shown)}
              >
                {t('devices.check.keep')}
              </Button>
            )}
            {removable && (
              <Button
                variant="ghost"
                className={`${BUTTON} font-semibold text-loss hover:text-loss`}
                onClick={() => onRemove(shown)}
                {...server.props}
              >
                {t('remove.confirm')}
              </Button>
            )}
            {removable && server.blocked && (
              <p id={server.noteId} className="mx-0.5 my-0 text-center text-m-sub text-ink-3">
                {t('status.needsServer')}
              </p>
            )}
          </div>
        </div>
      )}
    </Sheet>
  );
}

/**
 * "Dřívější zařízení ({n})": the devices that left or were removed (listed for 30 days), one row
 * each with when and by whom. A row opens the device's own sheet with "Odstranit ze seznamu";
 * "Odstranit vše" under the list takes the whole group off this device's list. Neither unblocks
 * anything. The sheet closes once the group is empty.
 */
export function FormerDevicesSheet({
  devices,
  open,
  onOpenChange,
}: {
  devices: readonly SyncDevice[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { hideFormerDevices } = useDeviceSyncMutations();
  const [viewingId, setViewingId] = useState<string | null>(null);
  // `null` while "Odstranit vše" runs
  const [hidingId, setHidingId] = useState<string | null>(null);
  const viewing = devices.find((device) => device.id === viewingId) ?? null;
  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'medium' });
  const subOf = (device: SyncDevice) =>
    [
      t(devicePlatformKey(device.platform)),
      t(formerLineKey(device), {
        date: device.stateAt === null ? '' : day(device.stateAt),
        by: device.stateBy ?? '',
      }),
    ].join(' · ');

  // Nothing left to show: the group's row is gone from the list, so the sheet goes too
  const empty = devices.length === 0;
  useEffect(() => {
    if (open && empty) onOpenChange(false);
  }, [open, empty, onOpenChange]);

  const hide = async (ids: string[], one: string | null) => {
    setHidingId(one);
    try {
      await hideFormerDevices.mutateAsync(ids);
      if (one !== null) setViewingId(null);
    } catch (error) {
      toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });
    }
  };

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={onOpenChange}
        title={t('devices.formerGroup', { count: devices.length })}
      >
        <EntityRows>
          {devices.map((device) => {
            const Icon = deviceIcon(device.platform);
            return (
              <EntityRow
                key={device.id}
                chip={
                  <RowChip square>
                    <Icon strokeWidth={1.75} />
                  </RowChip>
                }
                title={device.name}
                sub={subOf(device)}
                onClick={() => setViewingId(device.id)}
                chevron
                wrap
              />
            );
          })}
        </EntityRows>
        <p className="mx-0.5 mb-2 mt-3.5 text-m-sub text-ink-3">{t('devices.hideNote')}</p>
        {/* One former device has its own button in its sheet: "all" only for several */}
        {devices.length > 1 && (
          <Button
            variant="ghost"
            className={BUTTON}
            loading={hideFormerDevices.isPending && hidingId === null}
            onClick={() => void hide(formerDeviceIds(devices), null)}
          >
            {t('devices.hideAll')}
          </Button>
        )}
      </Sheet>
      <Sheet
        open={open && viewing !== null}
        onOpenChange={(next) => {
          if (!next) setViewingId(null);
        }}
        title={viewing?.name ?? ''}
        description={viewing ? subOf(viewing) : undefined}
      >
        {viewing && (
          <div className="grid gap-1">
            <p className="mx-0.5 mb-2.5 mt-0 text-m-sub text-ink-3">{t('devices.hideNote')}</p>
            <Button
              variant="secondary"
              className={BUTTON}
              loading={hideFormerDevices.isPending && hidingId === viewing.id}
              onClick={() => void hide([viewing.id], viewing.id)}
            >
              {t('devices.hide')}
            </Button>
          </div>
        )}
      </Sheet>
    </>
  );
}
