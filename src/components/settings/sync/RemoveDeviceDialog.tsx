import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RecoveryKit, RemovalPlanDevice } from '@shared/schema';
import { useRemovalPlan } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import {
  PLAN_CHANGED_KEY,
  canKeepDropping,
  dropReasonKey,
  kitLineKey,
  outcomeView,
  removalRequest,
  removalTitleName,
  toggleId,
  type OutcomeView,
} from '@/utils/sync-removal';
import { NewKitStep } from '@/components/settings/sync/NewKitStep';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

type Step =
  | { kind: 'plan' }
  | { kind: 'working'; name: string }
  | { kind: 'kit'; kit: RecoveryKit; view: OutcomeView; name: string };

/**
 * "Odebrat zařízení": one screen (what happens, who stays, who drops out, the devices the removed
 * one added, the recovery-kit line, the Moony password), then "Odebírám {name}…", then a toast, or
 * the Kit step when the removal made a new recovery kit. Every removal changes the vault key.
 * Mounted only while open; `onClose` runs once it is done.
 */
export function RemoveDeviceDialog({
  deviceIds,
  onClose,
}: {
  deviceIds: string[];
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { removeDevices, removeDevicesWithPassword, keepDevice } = useDeviceSyncMutations();
  const [alsoRemove, setAlsoRemove] = useState<string[]>([]);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: 'plan' });
  const field = useRef<HTMLInputElement>(null);

  const planning = step.kind === 'plan';
  const planQuery = useRemovalPlan(deviceIds, alsoRemove, false, planning);
  const plan = planQuery.data;
  const name = removalTitleName(plan, deviceIds) ?? '';
  const reload = planQuery.refetch;

  // The new kit's seed and the password live in the mutation (and a ref) only while the dialog is
  // open: leaving clears both.
  const resetRemove = removeDevices.reset;
  useEffect(() => resetRemove, [resetRemove]);

  // A tick the reloaded plan no longer offers is not sent: `removalRequest` prunes the ticks.

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
    resetRemove();
    onClose();
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!plan || password === '' || !planning || planQuery.isFetching) return;
    const removedName = name;
    setError(null);
    setStep({ kind: 'working', name: removedName });
    try {
      const outcome = await removeDevicesWithPassword(
        password,
        removalRequest(plan, deviceIds, alsoRemove)
      );
      setPassword('');
      const view = outcomeView(outcome);
      if (view.kit) setStep({ kind: 'kit', kit: view.kit, view, name: removedName });
      else finish(view, removedName);
    } catch (failure) {
      setStep({ kind: 'plan' });
      setError(translateApiError(failure as Error, tc));
      // The plan the person confirmed no longer holds: show the new one and ask again
      if (failure instanceof ApiError && failure.key === PLAN_CHANGED_KEY) void reload();
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

  const working = step.kind === 'working';
  const closing = () => {
    if (working || step.kind === 'kit') return;
    onClose();
  };

  return (
    <Dialog open onOpenChange={(open) => !open && closing()}>
      <DialogContent
        className="max-w-[560px]"
        hideCloseButton={step.kind !== 'plan'}
        onEscapeKeyDown={(event) => step.kind !== 'plan' && event.preventDefault()}
        onInteractOutside={(event) => step.kind !== 'plan' && event.preventDefault()}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          field.current?.focus();
        }}
      >
        {step.kind === 'kit' && (
          <NewKitStep
            kit={step.kit}
            pending={step.view.kitPending}
            onDone={() => finish(step.view, step.name)}
          />
        )}
        {working && (
          <div className="grid gap-2 py-2" role="status" aria-live="polite">
            <DialogTitle>{t('remove.progress', { name: step.name })}</DialogTitle>
            <DialogDescription>{t('remove.body', { name: step.name })}</DialogDescription>
          </div>
        )}
        {planning && (
          <form onSubmit={submit} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>{t('remove.title', { name })}</DialogTitle>
              <DialogDescription>{t('remove.body', { name })}</DialogDescription>
            </DialogHeader>

            {planQuery.isError && !plan && (
              <div className="grid gap-2">
                <p className="m-0 text-body text-loss" role="alert">
                  {t('remove.loadError')}
                </p>
                <div>
                  <Button type="button" variant="outline" size="sm" onClick={() => void reload()}>
                    {t('remove.retry')}
                  </Button>
                </div>
              </div>
            )}

            {plan && (
              <div className="grid gap-3.5 text-body text-ink-2">
                <section className="grid gap-1">
                  <p className="m-0 font-600 text-ink">{t('remove.stays')}</p>
                  <ul className="m-0 grid list-none gap-0.5 p-0">
                    {plan.staying.map((device) => (
                      <li key={device.id} className="min-w-0 break-words">
                        {device.thisDevice
                          ? t('remove.thisDevice', { name: device.name })
                          : device.name}
                      </li>
                    ))}
                  </ul>
                </section>

                {plan.droppingOut.length > 0 && (
                  <section className="grid gap-1.5">
                    <p className="m-0 font-600 text-ink">{t('remove.drops')}</p>
                    <ul className="m-0 grid list-none gap-1.5 p-0">
                      {plan.droppingOut.map(({ device, reason }) => (
                        <li
                          key={device.id}
                          className="flex flex-wrap items-center justify-between gap-2"
                        >
                          <span className="min-w-0 break-words">
                            {device.name} · {t(dropReasonKey(reason))}
                          </span>
                          {canKeepDropping(reason) && (
                            <Button
                              type="button"
                              variant="outline"
                              size="sm"
                              onClick={() => void keep(device)}
                              loading={keepDevice.isPending && keepDevice.variables === device.id}
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
                  <section className="grid gap-1.5">
                    <p className="m-0">{t('remove.added', { name })}</p>
                    <ul className="m-0 grid list-none gap-1.5 p-0">
                      {plan.addedByRemoving.map((device) => (
                        <li key={device.id} className="flex items-start gap-2">
                          <Checkbox
                            id={`remove-also-${device.id}`}
                            className="mt-0.5"
                            checked={alsoRemove.includes(device.id)}
                            onCheckedChange={() => setAlsoRemove((ids) => toggleId(ids, device.id))}
                          />
                          <Label
                            htmlFor={`remove-also-${device.id}`}
                            className="min-w-0 break-words font-500"
                          >
                            {t('remove.addedRow', {
                              name: device.name,
                              date: date(new Date(device.joinedAt * 1000), { dateStyle: 'medium' }),
                            })}
                          </Label>
                        </li>
                      ))}
                    </ul>
                  </section>
                )}

                <p className="m-0 text-micro font-500 text-ink-3">{t('remove.inHand', { name })}</p>
                {kitLineKey(plan) && (
                  <p className="m-0 font-600 text-ink">{t(kitLineKey(plan) ?? '', { name })}</p>
                )}
              </div>
            )}

            <div className="grid gap-1.5">
              <Label htmlFor="remove-device-password">{t('password.label')}</Label>
              <Input
                ref={field}
                id="remove-device-password"
                type="password"
                value={password}
                onChange={(event) => {
                  setPassword(event.target.value);
                  setError(null);
                }}
                onKeyDown={submitOnEnter}
                autoComplete="current-password"
                aria-invalid={error ? true : undefined}
              />
              {error && (
                <p className="m-0 text-micro font-500 text-loss" role="alert">
                  {error}
                </p>
              )}
            </div>
            <DialogFooter>
              <Button type="button" variant="ghost" onClick={onClose}>
                {tc('buttons.cancel')}
              </Button>
              <Button
                type="submit"
                variant="destructive"
                disabled={!plan || password === '' || planQuery.isFetching}
              >
                {t('remove.confirm')}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
