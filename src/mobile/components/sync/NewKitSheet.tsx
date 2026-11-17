import { useEffect, useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { useRemovalPlan } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { translateApiError } from '@/lib/translate-api-error';
import {
  planNeedsReload,
  outcomeView,
  removalRequest,
  toggleId,
  withoutKit,
  type OutcomeView,
} from '@/utils/sync-removal';
import { Sheet } from '@/mobile/components/Sheet';
import { AlsoRemoveList } from '@/mobile/components/sync/AlsoRemoveList';
import { NewKitStep } from '@/mobile/components/sync/NewKitStep';
import { PasswordField } from '@/mobile/components/sync/PasswordField';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

type Step =
  | { kind: 'plan' }
  | { kind: 'working' }
  | { kind: 'kit'; kit: RecoveryKit | null; view: OutcomeView };

/**
 * "Vytvořit novou sadu" on the phone, the counterpart of the desktop's `NewKitDialog`: a key change
 * with a new recovery kit and nobody removed (spec §2.7, §6.2). One sheet (what happens, the devices
 * that joined with the old kit with "Odebrat také" ticks, the Moony password), then "Vytvářím novou
 * sadu…", then the Kit step or a toast. `missing`: sync has no usable kit at all, so the sheet speaks
 * of making one, not replacing one. The new kit's seed and the password live in the mutation and a
 * ref only while the sheet is mounted. Mounted only while open; `onClose` runs once it is done.
 */
export function NewKitSheet({
  missing = false,
  onClose,
}: {
  missing?: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const fieldId = useId();
  const { removeDevices, removeDevicesWithPassword } = useDeviceSyncMutations();
  const [alsoRemove, setAlsoRemove] = useState<string[]>([]);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: 'plan' });

  const planning = step.kind === 'plan';
  const planQuery = useRemovalPlan([], alsoRemove, true, planning);
  const plan = planQuery.data;
  const reload = planQuery.refetch;

  // Leaving clears the new kit's seed (the mutation's result) and the password.
  const resetRemove = removeDevices.reset;
  useEffect(() => resetRemove, [resetRemove]);

  // The kit expired or the password is asked for again: it leaves the step and the mutation result.
  const kitGone = () => {
    resetRemove();
    setStep((current) =>
      current.kind === 'kit' ? { ...current, kit: null, view: withoutKit(current.view) } : current
    );
  };

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
      if (failure instanceof ApiError && planNeedsReload(failure.key)) void reload();
    }
  };

  if (step.kind === 'kit') {
    return (
      <NewKitStep
        kit={step.kit}
        pending={step.view.kitPending}
        onGone={kitGone}
        onDone={() => finish(step.view, true)}
      />
    );
  }

  const working = step.kind === 'working';
  const formId = `${fieldId}-form`;

  return (
    <Sheet
      open
      full
      onOpenChange={(open) => {
        if (!open && planning) onClose();
      }}
      title={working ? t('newKitDialog.progress') : t('newKitDialog.title')}
      description={missing ? t('kit.none') : t('newKitDialog.body')}
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
            className={`${BUTTON} flex-1`}
            disabled={!plan || password === '' || planQuery.isFetching}
            loading={working}
          >
            {missing ? t('kit.make') : t('newKitDialog.confirm')}
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

        {plan && plan.addedByRemoving.length > 0 && (
          <section className="space-y-1 text-m-lead text-ink-2">
            <p className="m-0">{t('newKitDialog.joined')}</p>
            <AlsoRemoveList
              idPrefix={`${fieldId}-also`}
              devices={plan.addedByRemoving}
              ticked={alsoRemove}
              onToggle={(id) => setAlsoRemove((ids) => toggleId(ids, id))}
            />
          </section>
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
      </form>
    </Sheet>
  );
}
