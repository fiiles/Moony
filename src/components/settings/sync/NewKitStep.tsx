import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { RecoveryKit } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { RecoveryKitPanel } from '@/components/settings/sync/RecoveryKitPanel';

/**
 * The step that shows a new recovery kit once (after a removal that replaced the kit, or after
 * "Vytvořit novou sadu"): print, save or copy it, then "Sadu mám, hotovo". The kit is on screen for
 * five minutes at most (the panel's lifetime) and cannot be shown again; when it expires, or the
 * ten-minute window of the backend does, the step says so and offers only to close.
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

  if (kit === null) {
    return (
      <div className="grid gap-4">
        <p className="m-0 text-body text-ink-2" role="status">
          {t('newKit.gone')}
        </p>
        <div className="flex justify-end">
          <Button type="button" onClick={onDone}>
            {tc('buttons.close')}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="grid gap-4">
      <div className="grid gap-1">
        <h3 className="m-0 text-[15px] font-650 text-ink">{t('newKit.title')}</h3>
        <p className="m-0 text-body text-ink-2">{t('newKit.body')}</p>
        {pending && <p className="m-0 text-body font-600 text-ink-2">{t('newKit.pending')}</p>}
      </div>
      <RecoveryKitPanel
        kit={kit}
        onKept={() => setKept(true)}
        onPasswordRequired={onGone}
        onExpired={onGone}
      />
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="m-0 min-w-0 flex-1 text-micro font-500 text-ink-3">
          {kept ? '' : t('newKit.keepFirst')}
        </p>
        <Button type="button" disabled={!kept} onClick={onDone}>
          {t('newKit.done')}
        </Button>
      </div>
    </div>
  );
}
