import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Info, TriangleAlert } from 'lucide-react';
import type { SyncNoticeKind, SyncStatus } from '@shared/schema';
import { useSyncDevices } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { useServerGate } from '@/hooks/use-server-gate';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import { sentences } from '@/utils/sentences';
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
  /** Their names, for the sheet's title before its plan loads. */
  names: string[];
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
  const { date, locale } = useFormat();
  const { dismissNotice, keepDevice, kitJoinDecided } = useDeviceSyncMutations();
  const { data: devices } = useSyncDevices();
  // The notice being dismissed (its key): its own action waits and a double tap is ignored
  const [dismissing, setDismissing] = useState<string | null>(null);
  const [removing, setRemoving] = useState<Removing | null>(null);
  const [makingKit, setMakingKit] = useState(false);
  const available = removalAvailable(status);
  // Removing devices and a new kit need the server; "Mám je", "Je moje" and closing do not
  const server = useServerGate();
  const waits = server.blocked ? <span className="block">{t('status.needsServer')}</span> : null;

  const day = (seconds: number) => date(new Date(seconds * 1000), { dateStyle: 'long' });
  const failed = (error: unknown) =>
    toast.error(tc('status.error'), { description: translateApiError(error as Error, tc) });

  /** Dismisses a notice; `true` when it is gone. */
  const dismiss = async (key: string, kind: SyncNoticeKind, ids?: string[]): Promise<boolean> => {
    if (dismissing === key) return false;
    setDismissing(key);
    try {
      await dismissNotice.mutateAsync(ids ? { kind, devices: ids } : kind);
      return true;
    } catch (error) {
      failed(error);
      return false;
    } finally {
      setDismissing(null);
    }
  };
  const keep = async (id: string) => {
    let kept: boolean;
    try {
      kept = await keepDevice.mutateAsync(id);
    } catch {
      toast.error(t('devices.check.keepFailed'));
      return;
    }
    // Nothing was kept (the row is gone or no longer in question): the list was reloaded, no success
    if (!kept) {
      toast(t('devices.check.keepFailed'));
      return;
    }
    await dismiss(`new-${id}`, 'newDevices', [id]);
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
    onClick: () => void dismiss(kind, kind),
    disabled: dismissing === kind,
  });

  const afterKit = status.afterKitJoin;
  const devicesLoaded = devices !== undefined;
  const heldDevices = afterKit ? afterKitJoinDevices(afterKit.devices, devices) : [];
  const removal = status.removalNotice;
  const notDone = status.removalNotDone;
  const overrun = status.keyChangeOverrun;

  const boxes: ReactNode[] = [];

  if (afterKit) {
    const names = afterKit.devices.map((device) => device.name).join(', ');
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
          !devicesLoaded
            ? {
                // The held devices are known once the list is: no choice before that
                label: t('afterKit.remove'),
                onClick: () => undefined,
                disabled: true,
              }
            : heldDevices.length > 0
              ? {
                  label: t('afterKit.remove'),
                  onClick: () =>
                    setRemoving({
                      ids: heldDevices.map((device) => device.id),
                      names: heldDevices.map((device) => device.name),
                      newKit: false,
                      decidesKitJoin: true,
                    }),
                  disabled: !available || server.blocked,
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
          {status.removalPending ? sentences(locale, [body, t('afterKit.pending')]) : body}
        </span>
        {devicesLoaded && heldDevices.length > 0 && waits}
      </InlineAlert>
    );
  }

  if (removal) {
    const text = removalNoticeText(removal);
    boxes.push(
      <InlineAlert key="removal" icon={Info} action={closeAction('removal')}>
        <span data-testid="notice-removal">
          {sentences(locale, [
            t(text.key, {
              by: removal.by,
              names: removal.removed.join(', '),
              date: day(removal.at),
            }),
            text.kitToo ? t('notice.removal.kitToo') : null,
          ])}
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
                  disabled: !available || server.blocked,
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
        {notDone.kitReplaced && waits}
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
      onClick: () => void dismiss(`new-${device.id}`, 'newDevices', [device.id]),
      disabled: dismissing === `new-${device.id}`,
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
                setRemoving({
                  ids: [device.id],
                  names: [device.name],
                  newKit: false,
                  decidesKitJoin: false,
                }),
              disabled: !available || server.blocked,
            },
          ]
        : kind === 'kit'
          ? [
              closeDevice,
              {
                label: t('notice.newDevice.removeAndNew'),
                onClick: () =>
                  setRemoving({
                    ids: [device.id],
                    names: [device.name],
                    newKit: true,
                    decidesKitJoin: false,
                  }),
                disabled: !available || server.blocked,
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
        {kind !== 'added' && waits}
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
          deviceNames={removing.names}
          newKit={removing.newKit}
          onRemoved={removing.decidesKitJoin ? () => void decideKitJoin(true) : undefined}
          onClose={() => setRemoving(null)}
        />
      )}
      {makingKit && <NewKitSheet onClose={() => setMakingKit(false)} />}
    </>
  );
}
