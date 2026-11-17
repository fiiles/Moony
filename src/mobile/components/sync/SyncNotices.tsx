import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Info, TriangleAlert } from 'lucide-react';
import type { SyncNoticeKind, SyncStatus } from '@shared/schema';
import { useSyncDevices } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import {
  afterKitJoinDevices,
  newDeviceKind,
  removalNoticeText,
  removalNotDoneText,
} from '@/utils/sync-notices';
import { removalAvailable } from '@/utils/sync-removal';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { NewKitSheet } from '@/mobile/components/sync/NewKitSheet';
import { RemoveDeviceSheet } from '@/mobile/components/sync/RemoveDeviceSheet';

/** A removal a notice opened: the devices, whether it also makes a new recovery kit, and what follows it. */
interface Removing {
  ids: string[];
  newKit: boolean;
  /** "Data jsou zpět": the hold is lifted once the devices are gone. */
  decidesKitJoin: boolean;
}

/**
 * The notices of Nastavení → Sync, one `InlineAlert` each, in the order of §6.4 (the phone's
 * counterpart of the desktop's `NoticeList`): the Kit joiner's "Data jsou zpět" (it holds this
 * device's pushes until decided), another device's removal or new recovery kit, a removal that did
 * not happen, changes applied past a removal, and the devices that appeared on the list. Each closes
 * with its own text action (`dismissNotice`; a new device by its ID, so one that arrives meanwhile
 * stays). The sync server's kit-read notice (`kitUsed`) stays with the kit.
 */
export function SyncNotices({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { dismissNotice, keepDevice, kitJoinDecided } = useDeviceSyncMutations();
  const { data: devices } = useSyncDevices();
  const [removing, setRemoving] = useState<Removing | null>(null);
  const [makingKit, setMakingKit] = useState(false);
  const available = removalAvailable(status);

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'long' });
  const failed = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  const dismiss = async (kind: SyncNoticeKind, ids?: string[]) => {
    try {
      await dismissNotice.mutateAsync(ids ? { kind, devices: ids } : kind);
    } catch (error) {
      failed(error);
    }
  };
  const keep = async (id: string) => {
    try {
      await keepDevice.mutateAsync(id);
      await dismissNotice.mutateAsync({ kind: 'newDevices', devices: [id] });
    } catch {
      toast.error(t('devices.check.keepFailed'));
    }
  };
  // "Mám je" (`false`), or the devices are gone (`true`); a removal still running refuses with its own text
  const decideKitJoin = async (removeThem: boolean) => {
    try {
      await kitJoinDecided.mutateAsync(removeThem);
    } catch (error) {
      failed(error);
    }
  };

  const closeAction = (kind: SyncNoticeKind) => ({
    label: tc('buttons.close'),
    onClick: () => void dismiss(kind),
    disabled: dismissNotice.isPending,
  });

  const afterKit = status.afterKitJoin;
  const heldDevices = afterKit ? afterKitJoinDevices(afterKit.names, devices) : [];
  const removal = status.removalNotice;
  const notDone = status.removalNotDone;
  const overrun = status.keyChangeOverrun;

  const boxes: ReactNode[] = [];

  if (afterKit) {
    const names = afterKit.names.join(', ');
    const body =
      afterKit.newestChangeAt === null
        ? t('afterKit.bodyNoDate', { names })
        : t('afterKit.body', { date: day(afterKit.newestChangeAt), names });
    boxes.push(
      <InlineAlert
        key="afterKit"
        icon={Info}
        title={t('afterKit.title')}
        action={[
          heldDevices.length > 0
            ? {
                label: t('afterKit.remove'),
                onClick: () =>
                  setRemoving({
                    ids: heldDevices.map((device) => device.id),
                    newKit: false,
                    decidesKitJoin: true,
                  }),
                disabled: !available,
              }
            : {
                label: tc('buttons.continue'),
                onClick: () => void decideKitJoin(true),
                disabled: status.removalPending || kitJoinDecided.isPending,
              },
          {
            label: t('afterKit.keep'),
            onClick: () => void decideKitJoin(false),
            disabled: kitJoinDecided.isPending,
          },
        ]}
      >
        <span data-testid="notice-after-kit">
          {status.removalPending ? `${body} ${t('afterKit.pending')}` : body}
        </span>
      </InlineAlert>
    );
  }

  if (removal) {
    const text = removalNoticeText(removal);
    boxes.push(
      <InlineAlert key="removal" icon={Info} action={closeAction('removal')}>
        <span data-testid="notice-removal">
          {t(text.key, {
            by: removal.by,
            names: removal.removed.join(', '),
            date: day(removal.at),
          })}
          {text.kitToo && ` ${t('notice.removal.kitToo')}`}
        </span>
      </InlineAlert>
    );
  }

  if (notDone) {
    boxes.push(
      <InlineAlert
        key="notDone"
        icon={Info}
        action={[
          ...(notDone.kitReplaced
            ? [
                {
                  label: t('kit.makeNewKit'),
                  onClick: () => setMakingKit(true),
                  disabled: !available,
                },
              ]
            : []),
          closeAction('removalNotDone'),
        ]}
      >
        <span data-testid="notice-not-done">
          {t(removalNotDoneText(notDone), {
            by: notDone.by ?? '',
            devices: notDone.devices.join(', '),
          })}
        </span>
      </InlineAlert>
    );
  }

  if (overrun) {
    boxes.push(
      <InlineAlert key="overrun" icon={TriangleAlert} action={closeAction('keyChangeOverrun')}>
        <span data-testid="notice-overrun">
          {overrun.at === null
            ? t('notice.overrunNoDate')
            : t('notice.overrun', { date: day(overrun.at) })}
        </span>
      </InlineAlert>
    );
  }

  for (const device of status.newDevices) {
    const kind = newDeviceKind(device);
    const values = { name: device.name, by: device.by, date: day(device.at) };
    const closeDevice = {
      label: kind === 'kit' ? t('notice.newDevice.mine') : tc('buttons.close'),
      onClick: () => void dismiss('newDevices', [device.id]),
      disabled: dismissNotice.isPending,
    };
    const actions =
      kind === 'lateVouch'
        ? [
            {
              label: t('devices.check.keep'),
              onClick: () => void keep(device.id),
              disabled: keepDevice.isPending && keepDevice.variables === device.id,
            },
            {
              label: t('devices.remove'),
              onClick: () =>
                setRemoving({ ids: [device.id], newKit: false, decidesKitJoin: false }),
              disabled: !available,
            },
          ]
        : kind === 'kit'
          ? [
              closeDevice,
              {
                label: t('notice.newDevice.removeAndNew'),
                onClick: () =>
                  setRemoving({ ids: [device.id], newKit: true, decidesKitJoin: false }),
                disabled: !available,
              },
            ]
          : [closeDevice];
    boxes.push(
      <InlineAlert
        key={`new-${device.id}`}
        icon={kind === 'added' ? Info : TriangleAlert}
        title={kind === 'kit' ? t('notice.newDevice.kitTitle') : undefined}
        action={actions}
      >
        <span data-testid={`notice-new-device-${kind}`}>
          {t(`notice.newDevice.${kind}`, values)}
        </span>
      </InlineAlert>
    );
  }

  return (
    <>
      {boxes}
      {/* Mounted only while open: unmounting forgets the typed password and a new kit */}
      {removing && (
        <RemoveDeviceSheet
          deviceIds={removing.ids}
          newKit={removing.newKit}
          onRemoved={removing.decidesKitJoin ? () => void decideKitJoin(true) : undefined}
          onClose={() => setRemoving(null)}
        />
      )}
      {makingKit && <NewKitSheet onClose={() => setMakingKit(false)} />}
    </>
  );
}
