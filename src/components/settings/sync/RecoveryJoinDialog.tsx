import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { translateApiError } from '@/lib/translate-api-error';
import { scannedCodeKind } from '@/utils/pairing-code';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/**
 * Joining with a Sync Recovery Kit: its recovery code (typed, pasted, or the kit QR's text) and,
 * for a typed code, the account number. Replaces this device's data after a backup; the busy
 * cover shows through the shared mutation key. Mounted only while open.
 */
export function RecoveryJoinDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { joinWithRecoveryKit } = useDeviceSyncMutations();
  const resetJoin = joinWithRecoveryKit.reset;
  // The password stays in the mutation state until it succeeds: leaving clears it.
  useEffect(() => resetJoin, [resetJoin]);

  const [code, setCode] = useState('');
  const [account, setAccount] = useState('');
  const [name, setName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));
  const [password, setPassword] = useState('');
  const [relayUrl, setRelayUrl] = useState('');
  const [error, setError] = useState<string | null>(null);

  const needsAccount = scannedCodeKind(code) === 'recoveryCode';
  const busy = joinWithRecoveryKit.isPending;
  const ready =
    code.trim() !== '' &&
    password !== '' &&
    name.trim() !== '' &&
    (!needsAccount || account.trim() !== '');

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setError(null);
    try {
      const path = await joinWithRecoveryKit.mutateAsync({
        code: code.trim(),
        account: needsAccount ? account.trim() : undefined,
        relayUrl: relayUrl.trim() === '' ? undefined : relayUrl.trim(),
        deviceName: name.trim(),
        password,
      });
      toast(t('pairing.joined', { path }));
      onClose();
    } catch (failure) {
      joinWithRecoveryKit.reset();
      setError(translateApiError(failure as Error, tc));
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent className="max-w-[520px]">
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t('join.kitTitle')}</DialogTitle>
            <DialogDescription>{t('join.kitHint')}</DialogDescription>
          </DialogHeader>
          <JoinWarning />
          <div className="grid gap-1.5">
            <Label htmlFor="join-kit-code">{t('join.codeLabel')}</Label>
            <Input
              id="join-kit-code"
              value={code}
              onChange={(event) => setCode(event.target.value)}
              onKeyDown={submitOnEnter}
              className="font-mono"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>
          {needsAccount && (
            <div className="grid gap-1.5">
              <Label htmlFor="join-kit-account">{t('join.accountLabel')}</Label>
              <Input
                id="join-kit-account"
                value={account}
                onChange={(event) => setAccount(event.target.value)}
                onKeyDown={submitOnEnter}
                inputMode="numeric"
                autoComplete="off"
              />
            </div>
          )}
          <JoinFields
            name={name}
            onName={setName}
            password={password}
            onPassword={(value) => {
              setPassword(value);
              setError(null);
            }}
            relayUrl={relayUrl}
            onRelayUrl={setRelayUrl}
            error={error}
          />
          <DialogFooter>
            <Button type="button" variant="ghost" disabled={busy} onClick={onClose}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" loading={busy} disabled={!ready}>
              {t('join.submit')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
