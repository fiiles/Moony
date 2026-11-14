import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useReplaceImpact } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { consentOf } from '@/utils/replace-impact';
import { translateApiError } from '@/lib/translate-api-error';
import { useServerChoice } from '@/hooks/use-server-choice';
import { scannedCodeKind } from '@/utils/pairing-code';
import { payloadServer } from '@/utils/sync-server';
import { kitServerNeed } from '@/utils/sync-setup';
import { defaultDeviceKey } from '@/utils/sync-devices';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { ReplaceImpactNotice } from '@/components/settings/sync/ReplaceImpactNotice';
import {
  IMPACT_CHANGED_KEY,
  replaceOutcomeToast,
} from '@/components/settings/sync/replace-outcome';
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
  const impact = useReplaceImpact();
  const resetJoin = joinWithRecoveryKit.reset;
  // The password stays in the mutation state until it succeeds: leaving clears it.
  useEffect(() => resetJoin, [resetJoin]);

  const [code, setCode] = useState('');
  const [account, setAccount] = useState('');
  const [name, setName] = useState(() => t(`setup.defaultName.${defaultDeviceKey()}`));
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  const kind = scannedCodeKind(code);
  const needsAccount = kind === 'recoveryCode';
  // A typed code asks for its sync server; a Kit QR brings its own, which is checked (and its
  // plain http acknowledged) before the join sends anything to it
  const qrServer = payloadServer(code);
  const need = kitServerNeed(kind, qrServer);
  const server = useServerChoice(
    'addressOrCode',
    'server',
    need === 'typed' ? undefined : need === 'qr' ? (qrServer ?? '') : ''
  );
  const busy = joinWithRecoveryKit.isPending;
  const ready =
    code.trim() !== '' &&
    (need === 'none' || server.ready) &&
    password !== '' &&
    impact.data !== undefined &&
    !impact.isFetching &&
    name.trim() !== '' &&
    (!needsAccount || account.trim() !== '');

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy || !impact.data) return;
    setError(null);
    try {
      const outcome = await joinWithRecoveryKit.mutateAsync({
        code: code.trim(),
        account: needsAccount ? account.trim() : undefined,
        relayUrl: need === 'typed' ? server.check?.address : undefined,
        deviceName: name.trim(),
        password,
        consent: consentOf('join', impact.data),
      });
      replaceOutcomeToast(t, t('pairing.joined', { path: outcome.backupPath }), outcome);
      onClose();
    } catch (failure) {
      joinWithRecoveryKit.reset();
      if (failure instanceof ApiError && failure.key === IMPACT_CHANGED_KEY) await impact.refetch();
      setError(translateApiError(failure as Error, tc));
    }
  };

  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent className="max-w-[560px]">
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{t('join.kitTitle')}</DialogTitle>
            <DialogDescription>{t('join.kitHint')}</DialogDescription>
          </DialogHeader>
          <JoinWarning />
          <ReplaceImpactNotice
            flow="join"
            impact={impact.data}
            error={impact.isError ? (impact.error as Error) : null}
          />
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
            server={server}
            serverMode={need === 'typed' ? 'field' : need === 'qr' ? 'fixed' : 'hidden'}
            error={error}
          />
          <DialogFooter>
            <Button type="button" variant="ghost" disabled={busy} onClick={onClose}>
              {tc('buttons.cancel')}
            </Button>
            <Button type="submit" loading={busy} disabled={!ready}>
              {t('join.kitTitle')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
