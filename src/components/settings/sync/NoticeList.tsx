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
import { sentences } from '@/utils/sentences';
import { removalAvailable } from '@/utils/sync-removal';
import { NewKitDialog } from '@/components/settings/sync/NewKitDialog';
import { RemoveDeviceDialog } from '@/components/settings/sync/RemoveDeviceDialog';
import { WRAP_BUTTON } from '@/components/settings/sync/wrap-button';
import { Alert, AlertActions, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';

/** A removal the list opened: the devices, whether it also makes a new recovery kit, and what follows it. */
interface Removing {
  ids: string[];
  /** Their names, for the dialog's title before its plan loads. */
  names: string[];
  newKit: boolean;
  /** "Data jsou zpět": the hold is lifted once the devices are gone. */
  decidesKitJoin: boolean;
}

/**
 * The notices of Settings → Sync, one box each, in the order of §6.4: the Kit joiner's "Data jsou
 * zpět" (it holds this device's pushes until decided), another device's removal or new recovery
 * kit, a removal that did not happen, changes applied past a removal, and the devices that
 * appeared on the list. Each closes with its own button (`dismissNotice`; a new device by its ID,
 * so one that arrives meanwhile stays). The sync server's kit-read notice (`kitUsed`) stays in
 * the kit row, next to its two buttons.
 */
export function NoticeList({ status }: { status: SyncStatus }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date, locale } = useFormat();
  const { dismissNotice, keepDevice, kitJoinDecided } = useDeviceSyncMutations();
  const { data: devices } = useSyncDevices();
  // The notice being dismissed (its key), so only its own button waits and a double tap is ignored
  const [dismissing, setDismissing] = useState<string | null>(null);
  const [removing, setRemoving] = useState<Removing | null>(null);
  const [makingKit, setMakingKit] = useState(false);
  const available = removalAvailable(status);

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

  const close = (kind: SyncNoticeKind) => (
    <Button
      type="button"
      size="sm"
      variant="outline"
      className={WRAP_BUTTON}
      loading={dismissing === kind}
      onClick={() => void dismiss(kind, kind)}
    >
      {tc('buttons.close')}
    </Button>
  );

  const afterKit = status.afterKitJoin;
  const heldDevices = afterKit ? afterKitJoinDevices(afterKit.devices, devices) : [];
  // What the held devices are is known only once the device list is: no choice before that
  const devicesLoaded = devices !== undefined;
  const removal = status.removalNotice;
  const notDone = status.removalNotDone;
  const overrun = status.keyChangeOverrun;

  const boxes: ReactNode[] = [];

  if (afterKit) {
    const names = afterKit.devices.map((device) => device.name).join(', ');
    boxes.push(
      <NoticeBox
        key="afterKit"
        testId="notice-after-kit"
        title={t('afterKit.title')}
        actions={
          <>
            {!devicesLoaded ? (
              <Skeleton className="h-8 w-28" />
            ) : heldDevices.length > 0 ? (
              <Button
                type="button"
                size="sm"
                className={WRAP_BUTTON}
                disabled={!available}
                onClick={() =>
                  setRemoving({
                    ids: heldDevices.map((device) => device.id),
                    names: heldDevices.map((device) => device.name),
                    newKit: false,
                    decidesKitJoin: true,
                  })
                }
              >
                {t('afterKit.remove')}
              </Button>
            ) : (
              <Button
                type="button"
                size="sm"
                className={WRAP_BUTTON}
                disabled={status.removalPending}
                loading={kitJoinDecided.isPending}
                onClick={() => void decideKitJoin(true)}
              >
                {tc('buttons.continue')}
              </Button>
            )}
            <Button
              type="button"
              size="sm"
              variant="outline"
              className={WRAP_BUTTON}
              loading={kitJoinDecided.isPending}
              onClick={() => void decideKitJoin(false)}
            >
              {t('afterKit.keep')}
            </Button>
          </>
        }
      >
        <p className="m-0">
          {afterKit.newestChangeAt === null
            ? t('afterKit.bodyNoDate', { names })
            : t('afterKit.body', { date: day(afterKit.newestChangeAt), names })}
        </p>
        {status.removalPending && <p className="m-0 mt-1 font-600">{t('afterKit.pending')}</p>}
      </NoticeBox>
    );
  }

  if (removal) {
    const text = removalNoticeText(removal);
    boxes.push(
      <NoticeBox key="removal" testId="notice-removal" actions={close('removal')}>
        {sentences(locale, [
          t(text.key, {
            by: removal.by,
            names: removal.removed.join(', '),
            date: day(removal.at),
          }),
          text.kitToo ? t('notice.removal.kitToo') : null,
        ])}
      </NoticeBox>
    );
  }

  if (notDone) {
    boxes.push(
      <NoticeBox
        key="notDone"
        testId="notice-not-done"
        actions={
          <>
            {notDone.kitReplaced && (
              <Button
                type="button"
                size="sm"
                className={WRAP_BUTTON}
                disabled={!available}
                onClick={() => setMakingKit(true)}
              >
                {t('kit.makeNewKit')}
              </Button>
            )}
            {close('removalNotDone')}
          </>
        }
      >
        {t(removalNotDoneText(notDone), {
          by: notDone.by ?? '',
          devices: notDone.devices.join(', '),
        })}
      </NoticeBox>
    );
  }

  if (overrun) {
    boxes.push(
      <NoticeBox key="overrun" testId="notice-overrun" warn actions={close('keyChangeOverrun')}>
        {overrun.at === null
          ? t('notice.overrunNoDate')
          : t('notice.overrun', { date: day(overrun.at) })}
      </NoticeBox>
    );
  }

  for (const device of status.newDevices) {
    const kind = newDeviceKind(device);
    const values = { name: device.name, by: device.by, date: day(device.at) };
    const closeDevice = (
      <Button
        type="button"
        size="sm"
        variant="outline"
        className={WRAP_BUTTON}
        loading={dismissing === `new-${device.id}`}
        onClick={() => void dismiss(`new-${device.id}`, 'newDevices', [device.id])}
      >
        {kind === 'kit' ? t('notice.newDevice.mine') : tc('buttons.close')}
      </Button>
    );
    boxes.push(
      <NoticeBox
        key={`new-${device.id}`}
        testId={`notice-new-device-${kind}`}
        warn={kind !== 'added'}
        title={kind === 'kit' ? t('notice.newDevice.kitTitle') : undefined}
        actions={
          kind === 'lateVouch' ? (
            <>
              <Button
                type="button"
                size="sm"
                className={WRAP_BUTTON}
                loading={keepDevice.isPending && keepDevice.variables === device.id}
                onClick={() => void keep(device.id)}
              >
                {t('devices.check.keep')}
              </Button>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className={WRAP_BUTTON}
                disabled={!available}
                onClick={() =>
                  setRemoving({
                    ids: [device.id],
                    names: [device.name],
                    newKit: false,
                    decidesKitJoin: false,
                  })
                }
              >
                {t('devices.remove')}
              </Button>
            </>
          ) : kind === 'kit' ? (
            <>
              {closeDevice}
              <Button
                type="button"
                size="sm"
                className={WRAP_BUTTON}
                disabled={!available}
                onClick={() =>
                  setRemoving({
                    ids: [device.id],
                    names: [device.name],
                    newKit: true,
                    decidesKitJoin: false,
                  })
                }
              >
                {t('notice.newDevice.removeAndNew')}
              </Button>
            </>
          ) : (
            closeDevice
          )
        }
      >
        {t(`notice.newDevice.${kind}`, values)}
      </NoticeBox>
    );
  }

  return (
    <>
      {boxes}
      {removing && (
        <RemoveDeviceDialog
          deviceIds={removing.ids}
          deviceNames={removing.names}
          newKit={removing.newKit}
          onRemoved={removing.decidesKitJoin ? () => void decideKitJoin(true) : undefined}
          onClose={() => setRemoving(null)}
        />
      )}
      {makingKit && <NewKitDialog onClose={() => setMakingKit(false)} />}
    </>
  );
}

/** One notice: an alert with an optional title, a body that wraps and its buttons. */
function NoticeBox({
  title,
  warn = false,
  testId,
  actions,
  children,
}: {
  title?: string;
  warn?: boolean;
  testId: string;
  actions: ReactNode;
  children: ReactNode;
}) {
  const Icon = warn ? TriangleAlert : Info;
  return (
    <Alert data-testid={testId}>
      <Icon />
      {title && <AlertTitle>{title}</AlertTitle>}
      <AlertDescription className="break-words">{children}</AlertDescription>
      <AlertActions className="flex-wrap">{actions}</AlertActions>
    </Alert>
  );
}
