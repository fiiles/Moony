import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { KitBody } from '@/mobile/components/sync/KitBody';
import { Sheet } from '@/mobile/components/Sheet';

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * The step that shows a new recovery kit once (after a removal that replaced the kit, or after
 * "Vytvořit novou sadu"): copy it or save or share it, then "Sadu mám, hotovo". It is its own
 * sheet that cannot be dismissed: the kit is on screen for five minutes at most (`KitBody`) and
 * cannot be shown again. When that, or the backend's ten-minute window, runs out the step says so
 * and offers only to close.
 */
export function NewKitStep({
  kit,
  pending,
  onGone,
  onDone,
}: {
  /** `null` once the kit left the screen (`onGone`): the caller dropped it from its state. */
  kit: RecoveryKit | null;
  /** The removal has not finished (the sync server was out of reach): the kit starts working once it does. */
  pending: boolean;
  /** The kit expired or the backend asks for the password again: the caller drops it (state and mutation result). */
  onGone: () => void;
  onDone: () => void;
}) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const [kept, setKept] = useState(false);
  const gone = kit === null;

  return (
    <Sheet
      open
      full
      // Nothing closes this sheet but its button: the kit is shown only now
      onOpenChange={() => undefined}
      title={t('newKit.title')}
      description={gone ? undefined : t('newKit.body')}
      footer={
        gone ? (
          <Button className={`${BUTTON} flex-1`} onClick={onDone}>
            {tc('buttons.close')}
          </Button>
        ) : (
          <Button className={`${BUTTON} flex-1`} disabled={!kept} onClick={onDone}>
            {t('newKit.done')}
          </Button>
        )
      }
    >
      {gone || kit === null ? (
        <p role="status" className="m-0 text-m-lead text-ink-2">
          {t('newKit.gone')}
        </p>
      ) : (
        <div className="space-y-3.5">
          {pending && (
            <p className="m-0 text-m-lead font-semibold text-ink-2">{t('newKit.pending')}</p>
          )}
          <KitBody
            kit={kit}
            onKept={() => setKept(true)}
            onPasswordRequired={onGone}
            onExpired={onGone}
          />
          {!kept && <p className="m-0 text-m-sub text-ink-4">{t('newKit.keepFirst')}</p>}
        </div>
      )}
    </Sheet>
  );
}
