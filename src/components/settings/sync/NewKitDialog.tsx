import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RecoveryKit } from '@shared/schema';
import { useRemovalPlan } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import { useFormat } from '@/lib/use-format';
import {
  PLAN_CHANGED_KEY,
  outcomeView,
  removalRequest,
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
  { kind: 'plan' } | { kind: 'working' } | { kind: 'kit'; kit: RecoveryKit; view: OutcomeView };

/**
 * "Vytvořit novou sadu": a key change with a new recovery kit and nobody removed (spec §2.7, §6.2).
 * One screen (what happens, the devices that joined with the old kit with "Odebrat také" ticks, the
 * Moony password), then "Vytvářím novou sadu…", then the Kit step (`NewKitStep`) or a toast.
 * `missing`: sync has no usable kit at all (the Kit row's "Vytvořit sadu", or right after a setup
 * the server's own kit beat), so the dialog speaks of making one, not replacing one. The new kit's
 * seed and the password live in the mutation and a ref only while the dialog is open. Mounted only
 * while open; `onClose` runs once it is done.
 */
export function NewKitDialog({
  missing = false,
  onClose,
}: {
  missing?: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { date } = useFormat();
  const { removeDevices, removeDevicesWithPassword } = useDeviceSyncMutations();
  const [alsoRemove, setAlsoRemove] = useState<string[]>([]);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: 'plan' });
  const field = useRef<HTMLInputElement>(null);

  const planning = step.kind === 'plan';
  const planQuery = useRemovalPlan([], alsoRemove, true, planning);
  const plan = planQuery.data;
  const reload = planQuery.refetch;

  // Leaving clears the new kit's seed (the mutation's result) and the password.
  const resetRemove = removeDevices.reset;
  useEffect(() => resetRemove, [resetRemove]);

  const finish = (view: OutcomeView, kitShown: boolean) => {
    if (view.toast === 'removed') {
      toast(kitShown ? t('newKitDialog.toast.done') : t('kit.notShown'));
    } else if (view.toast === 'pending') toast(t('remove.toast.pending'));
    else if (view.toast === 'alreadyRemoved') {
      toast(
        view.by === null
          ? t('newKitDialog.toast.alreadyRemovedUnknown')
          : t('newKitDialog.toast.alreadyRemoved', { by: view.by })
      );
    } else toast(t('remove.toast.removedHere'));
    resetRemove();
    onClose();
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!plan || password === '' || !planning || planQuery.isFetching) return;
    setError(null);
    setStep({ kind: 'working' });
    try {
      const outcome = await removeDevicesWithPassword(
        password,
        removalRequest(plan, [], alsoRemove, true)
      );
      setPassword('');
      const view = outcomeView(outcome);
      if (view.kit) setStep({ kind: 'kit', kit: view.kit, view });
      else finish(view, false);
    } catch (failure) {
      setStep({ kind: 'plan' });
      setError(translateApiError(failure as Error, tc));
      // The plan the person confirmed no longer holds: show the new one and ask again
      if (failure instanceof ApiError && failure.key === PLAN_CHANGED_KEY) void reload();
    }
  };

  const closing = () => {
    if (step.kind !== 'plan') return;
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
            onDone={() => finish(step.view, true)}
          />
        )}
        {step.kind === 'working' && (
          <div className="grid gap-2 py-2" role="status" aria-live="polite">
            <DialogTitle>{t('newKitDialog.progress')}</DialogTitle>
            <DialogDescription>{t('newKitDialog.body')}</DialogDescription>
          </div>
        )}
        {planning && (
          <form onSubmit={submit} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>{t('newKitDialog.title')}</DialogTitle>
              <DialogDescription>
                {missing ? t('kit.none') : t('newKitDialog.body')}
              </DialogDescription>
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

            {plan && plan.addedByRemoving.length > 0 && (
              <section className="grid gap-1.5 text-body text-ink-2">
                <p className="m-0">{t('newKitDialog.joined')}</p>
                <ul className="m-0 grid list-none gap-1.5 p-0">
                  {plan.addedByRemoving.map((device) => (
                    <li key={device.id} className="flex items-start gap-2">
                      <Checkbox
                        id={`new-kit-also-${device.id}`}
                        className="mt-0.5"
                        checked={alsoRemove.includes(device.id)}
                        onCheckedChange={() => setAlsoRemove((ids) => toggleId(ids, device.id))}
                      />
                      <Label
                        htmlFor={`new-kit-also-${device.id}`}
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

            <div className="grid gap-1.5">
              <Label htmlFor="new-kit-password">{t('password.label')}</Label>
              <Input
                ref={field}
                id="new-kit-password"
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
                className="h-auto min-h-control whitespace-normal py-1.5"
                disabled={!plan || password === '' || planQuery.isFetching}
              >
                {missing ? t('kit.make') : t('newKitDialog.confirm')}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
