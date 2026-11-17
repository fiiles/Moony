import { useEffect, useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RecoveryKit, RemovalPlanDevice } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRemovalPlan } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import {
  canKeepDropping,
  dropReasonKey,
  kitLineKey,
  outcomeView,
  planNeedsReload,
  removalErrorKey,
  removalRequest,
  removalTitleNames,
  toggleId,
  type OutcomeView,
  withoutKit,
} from '@/utils/sync-removal';
import { Sheet } from '@/mobile/components/Sheet';
import { AlsoRemoveList } from '@/mobile/components/sync/AlsoRemoveList';
import { NewKitStep } from '@/mobile/components/sync/NewKitStep';
import { PasswordField } from '@/mobile/components/sync/PasswordField';
import { useServerGate } from '@/hooks/use-server-gate';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

type Step =
  | { kind: 'plan' }
  | { kind: 'working'; name: string }
  | { kind: 'kit'; kit: RecoveryKit | null; view: OutcomeView; name: string };

/**
 * "Odebrat zařízení" on the phone, the counterpart of the desktop's `RemoveDeviceDialog`: one sheet
 * (what happens, who stays, who drops out, the devices the removed one added, the recovery-kit
 * line, the Moony password), then "Odebírám {name}…", then a toast, or the Kit step when the
 * removal made a new recovery kit. Every removal changes the vault key. Mounted only while open;
 * `onClose` runs once it is done. The password and a new kit's seed live in a ref and the mutation
 * (gcTime 0) only while the sheet is mounted.
 */
export function RemoveDeviceSheet({
  deviceIds,
  deviceNames,
  newKit = false,
  onRemoved,
  onClose,
}: {
  deviceIds: string[];
  /** Their names, for the title until the plan names them (it loads after the dialog opens). */
  deviceNames?: string[];
  /** Also makes a new recovery kit (a device that joined with the kit: "Odebrat a vytvořit novou sadu"). */
  newKit?: boolean;
  /** Runs once the devices are gone (done, or already removed by another device), before the sheet closes. */
  onRemoved?: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const { removeDevices, removeDevicesWithPassword, keepDevice } = useDeviceSyncMutations();
  const [alsoRemove, setAlsoRemove] = useState<string[]>([]);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: 'plan' });

  const planning = step.kind === 'plan';
  // The server drops while the sheet is open: it stays, and the confirm waits, saying why
  const server = useServerGate();
  const planQuery = useRemovalPlan(deviceIds, alsoRemove, newKit, planning);
  const plan = planQuery.data;
  const name = removalTitleNames(plan, deviceIds) ?? deviceNames?.join(', ') ?? '';
  const several = deviceIds.length > 1;
  const reload = planQuery.refetch;

  // The new kit's seed (the mutation's result) and the password leave with the sheet.
  const resetRemove = removeDevices.reset;
  useEffect(() => resetRemove, [resetRemove]);

  // The kit expired or the password is asked for again: it leaves the step and the mutation result.
  const kitGone = () => {
    resetRemove();
    setStep((current) =>
      current.kind === 'kit' ? { ...current, kit: null, view: withoutKit(current.view) } : current
    );
  };

  const finish = (view: OutcomeView, removedName: string) => {
    const by = view.by ?? '';
    if (view.toast === 'removed') toast(t('remove.toast.removed', { name: removedName }));
    else if (view.toast === 'pending') toast(t('remove.toast.pending'));
    else if (view.toast === 'alreadyRemoved') {
      toast(
        view.by === null
          ? t('remove.toast.alreadyRemovedUnknown', { name: removedName })
          : t('remove.toast.alreadyRemoved', { name: removedName, by })
      );
    } else toast(t('remove.toast.removedHere'));
    if (view.toast === 'removed' || view.toast === 'alreadyRemoved') onRemoved?.();
    resetRemove();
    onClose();
  };

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!plan || password === '' || !planning || planQuery.isFetching || server.blocked) return;
    const removedName = name;
    setError(null);
    setStep({ kind: 'working', name: removedName });
    try {
      const outcome = await removeDevicesWithPassword(
        password,
        removalRequest(plan, deviceIds, alsoRemove, newKit)
      );
      setPassword('');
      const view = outcomeView(outcome);
      if (view.kit) setStep({ kind: 'kit', kit: view.kit, view, name: removedName });
      else finish(view, removedName);
    } catch (failure) {
      setStep({ kind: 'plan' });
      const own = removalErrorKey(failure instanceof ApiError ? failure.key : undefined);
      setError(own ? t(own) : translateApiError(failure as Error, tc));
      // The plan the person confirmed no longer holds: show the new one and ask again
      if (failure instanceof ApiError && planNeedsReload(failure.key)) void reload();
    }
  };

  const keep = async (device: RemovalPlanDevice) => {
    try {
      await keepDevice.mutateAsync(device.id);
    } catch {
      // The row is no longer there to keep (or is no longer trusted): say so, the list reloads
      toast.error(t('devices.check.keepFailed'));
    }
    void reload();
  };

  if (step.kind === 'kit') {
    return (
      <NewKitStep
        kit={step.kit}
        pending={step.view.kitPending}
        onGone={kitGone}
        onDone={() => finish(step.view, step.name)}
      />
    );
  }

  const working = step.kind === 'working';
  const formId = `${fieldId}-form`;
  const kitLine = plan ? kitLineKey(plan) : null;

  return (
    <Sheet
      open
      full
      // While it works, only the result closes it (not the scrim, a drag or Android back)
      locked={!planning}
      onOpenChange={(open) => {
        if (!open && planning) onClose();
      }}
      title={working ? t('remove.progress', { name: step.name }) : t('remove.title', { name })}
      description={t(several ? 'remove.bodyMany' : 'remove.body', {
        name: working ? step.name : name,
      })}
      footer={
        <>
          <Button
            variant="ghost"
            className={`${BUTTON} px-[18px]`}
            disabled={working}
            onClick={onClose}
          >
            {tc('buttons.cancel')}
          </Button>
          <Button
            type="submit"
            form={formId}
            variant="destructive"
            className={`${BUTTON} flex-1`}
            disabled={!plan || password === '' || planQuery.isFetching}
            loading={working}
            {...server.props}
          >
            {t('remove.confirm')}
          </Button>
        </>
      }
    >
      <form
        id={formId}
        onSubmit={(event) => void submit(event)}
        aria-busy={working}
        noValidate
        className="space-y-3.5"
      >
        {planQuery.isError && !plan && (
          <div className="space-y-2">
            <p role="alert" className="m-0 text-m-lead text-loss">
              {t('remove.loadError')}
            </p>
            <Button
              type="button"
              variant="secondary"
              className={`${BUTTON} w-full`}
              onClick={() => void reload()}
            >
              {t('remove.retry')}
            </Button>
          </div>
        )}

        {plan && (
          <div className="space-y-3.5 text-m-lead text-ink-2">
            <section className="space-y-1">
              <p className="m-0 font-semibold text-ink">{t('remove.stays')}</p>
              <ul className="m-0 list-none space-y-0.5 p-0">
                {plan.staying.map((device) => (
                  <li key={device.id} className="break-words">
                    {device.thisDevice
                      ? t('remove.thisDevice', { name: device.name })
                      : device.name}
                  </li>
                ))}
              </ul>
            </section>

            {plan.droppingOut.length > 0 && (
              <section className="space-y-1">
                <p className="m-0 font-semibold text-ink">{t('remove.drops')}</p>
                <ul className="m-0 list-none p-0">
                  {plan.droppingOut.map(({ device, reason }) => (
                    <li key={device.id} className="py-1">
                      <span className="break-words">
                        {device.name} · {t(dropReasonKey(reason))}
                      </span>
                      {canKeepDropping(reason) && (
                        <Button
                          type="button"
                          variant="ghost"
                          className="mt-0.5 block h-auto min-h-11 px-0 text-m-lead font-semibold text-ink-2 underline"
                          loading={keepDevice.isPending && keepDevice.variables === device.id}
                          onClick={() => void keep(device)}
                        >
                          {t('remove.keep')}
                        </Button>
                      )}
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {plan.addedByRemoving.length > 0 && (
              <section className="space-y-1">
                <p className="m-0">{t('remove.added', { name })}</p>
                <AlsoRemoveList
                  idPrefix={`${fieldId}-also`}
                  devices={plan.addedByRemoving}
                  ticked={alsoRemove}
                  onToggle={(id) => setAlsoRemove((ids) => toggleId(ids, id))}
                />
              </section>
            )}

            <p className="m-0 text-m-sub text-ink-3">{t('remove.inHand', { name })}</p>
            {kitLine && <p className="m-0 font-semibold text-ink">{t(kitLine, { name })}</p>}
          </div>
        )}

        <PasswordField
          id={`${fieldId}-password`}
          value={password}
          onChange={(value) => {
            setPassword(value);
            setError(null);
          }}
          error={error}
          readOnly={working}
        />
        {server.blocked && (
          <p id={server.noteId} className="m-0 text-m-sub text-ink-3">
            {t('status.needsServer')}
          </p>
        )}
      </form>
    </Sheet>
  );
}
