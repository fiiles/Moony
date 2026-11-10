import { useEffect, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncStatus } from '@/hooks/use-device-sync';
import { useDevicePairing } from '@/hooks/use-device-pairing';
import { translateApiError } from '@/lib/translate-api-error';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { PairingSteps } from '@/components/settings/sync/PairingSteps';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';

/**
 * Joining from another device's QR (the joiner): the device name, address and password first, then
 * the QR, the six digits to compare and, once the sync details arrive, the join itself (the busy
 * cover shows through the shared mutation key). Mounted only while open; closing gives up.
 */
export function JoinSyncDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const status = useDeviceSyncStatus().data;
  const pairing = useDevicePairing();
  const { error, isError, start, confirm, cancel, join } = pairing;
  // A successful join clears the pairing before the dialog closes: keep showing its last step.
  const lastView = useRef(pairing.view);
  if (pairing.view) lastView.current = pairing.view;
  const view = pairing.view ?? lastView.current;
  const [name, setName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));
  const [password, setPassword] = useState('');
  const [relayUrl, setRelayUrl] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [joinError, setJoinError] = useState<string | null>(null);

  const close = async () => {
    try {
      await cancel();
    } catch {
      // Gone on this device either way.
    }
    onClose();
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (password === '' || name.trim() === '' || start.isPending) return;
    setFormError(null);
    try {
      await start.mutateAsync({
        password,
        deviceName: name.trim(),
        relayUrl: relayUrl.trim() === '' ? undefined : relayUrl.trim(),
      });
      setPassword('');
    } catch (failure) {
      start.reset();
      setFormError(translateApiError(failure as Error, tc));
    }
  };

  const runJoin = async () => {
    setJoinError(null);
    try {
      const path = await join.mutateAsync();
      toast(t('pairing.joined', { path }));
      onClose();
    } catch (failure) {
      setJoinError(translateApiError(failure as Error, tc));
    }
  };

  // The sync details arrived: join at once, once (a failure offers "Try again").
  const joining = useRef(false);
  const stage = view?.stage;
  useEffect(() => {
    if (stage !== 'received' || joining.current) return;
    joining.current = true;
    void runJoin();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stage]);

  const handleMatch = async () => {
    try {
      await confirm.mutateAsync();
    } catch (failure) {
      toast.error(tc('status.error'), { description: translateApiError(failure as Error, tc) });
    }
  };

  const retry = async () => {
    await runJoin();
  };

  if (!view) {
    return (
      <Dialog open onOpenChange={(open) => !open && !start.isPending && void close()}>
        <DialogContent className="max-w-[480px]">
          <form onSubmit={submit} className="grid gap-4">
            <DialogHeader>
              <DialogTitle>{t('join.pairTitle')}</DialogTitle>
              <DialogDescription>{t('join.pairHint')}</DialogDescription>
            </DialogHeader>
            <JoinWarning />
            <JoinFields
              name={name}
              onName={setName}
              password={password}
              onPassword={(value) => {
                setPassword(value);
                setFormError(null);
              }}
              relayUrl={relayUrl}
              onRelayUrl={setRelayUrl}
              error={formError}
            />
            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                disabled={start.isPending}
                onClick={() => void close()}
              >
                {tc('buttons.cancel')}
              </Button>
              <Button
                type="submit"
                loading={start.isPending}
                disabled={password === '' || name.trim() === ''}
              >
                {t('join.submit')}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    );
  }

  const failed = isError || view.contested;
  return (
    <Dialog open onOpenChange={(open) => !open && !join.isPending && void close()}>
      <DialogContent className="max-w-[480px]">
        <DialogHeader>
          <DialogTitle>{t('join.pairTitle')}</DialogTitle>
          <DialogDescription>
            {view.stage === 'compare' ? t('pairing.compareTitle') : t('join.warning')}
          </DialogDescription>
        </DialogHeader>
        <PairingSteps
          view={view}
          scanHint={t('join.pairHint')}
          devTools={status?.devTools === true}
          error={isError ? (error as Error) : null}
          onMatch={handleMatch}
          matching={confirm.isPending}
          onNoMatch={() => void close()}
        />
        {joinError && (
          <p className="m-0 text-body text-loss" role="alert">
            {joinError}
          </p>
        )}
        <DialogFooter>
          {view.stage === 'received' && joinError && !failed && (
            <Button type="button" onClick={retry} loading={join.isPending}>
              {t('pairing.tryAgain')}
            </Button>
          )}
          <Button
            type="button"
            variant="ghost"
            disabled={join.isPending}
            onClick={() => void close()}
          >
            {tc('buttons.cancel')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
