import { useEffect, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useReplaceImpact } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { consentOf } from '@/utils/replace-impact';
import { translateApiError } from '@/lib/translate-api-error';
import { useServerChoice } from '@/hooks/use-server-choice';
import { joinPassword, joinPresentation } from '@/utils/join-context';
import { scannedCodeKind } from '@/utils/pairing-code';
import { payloadServer } from '@/utils/sync-server';
import { kitCodeProblem, kitServerNeed } from '@/utils/sync-setup';
import { JoinFields, JoinWarning } from '@/components/settings/sync/JoinFields';
import { IMPACT_CHANGED_KEY } from '@/components/settings/sync/replace-outcome';
import { submitOnEnter } from '@/components/settings/sync/submit-on-enter';
import { JoinImpactNotice } from '@/components/settings/sync/join/JoinImpactNotice';
import type { JoinFlowProps } from '@/components/settings/sync/join/PairingJoinFlow';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

/**
 * Joining with a Sync Recovery Kit: its recovery code (typed, pasted, or the kit QR's text) and,
 * for a typed code, the account number. Replaces this device's data (in Settings after a backup);
 * the busy cover shows through the shared mutation key. Mounted only while it runs.
 */
export function RecoveryJoinFlow({
  context,
  deviceName,
  onJoined,
  onCancel,
  footer,
  dismissRef,
  onBusyChange,
}: JoinFlowProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const presentation = joinPresentation(context);
  const { joinWithRecoveryKit } = useDeviceSyncMutations();
  // Read in the first run too, where it is not shown: the join sends it as its consent.
  const impact = useReplaceImpact();
  const resetJoin = joinWithRecoveryKit.reset;
  // The password stays in the mutation state until it succeeds: leaving clears it.
  useEffect(() => resetJoin, [resetJoin]);

  const [code, setCode] = useState('');
  const [account, setAccount] = useState('');
  const [password, setPassword] = useState('');
  const devicePassword = joinPassword(context, password);
  const [error, setError] = useState<string | null>(null);

  const kind = scannedCodeKind(code);
  const codeProblem = kitCodeProblem(code);
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
  useEffect(() => onBusyChange?.(busy), [onBusyChange, busy]);
  const ready =
    code.trim() !== '' &&
    codeProblem === null &&
    (need === 'none' || server.ready) &&
    devicePassword !== '' &&
    impact.data !== undefined &&
    !impact.isFetching &&
    deviceName !== '' &&
    (!needsAccount || account.trim() !== '');

  // The caller's way out (Settings: a dismissal; first run: leaving the step): not while the join runs
  useEffect(() => {
    if (!dismissRef) return;
    dismissRef.current = () => {
      if (busy) return false;
      onCancel();
      return true;
    };
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy || !impact.data) return;
    setError(null);
    try {
      const outcome = await joinWithRecoveryKit.mutateAsync({
        code: code.trim(),
        account: needsAccount ? account.trim() : undefined,
        relayUrl: need === 'typed' ? server.check?.address : undefined,
        deviceName,
        password: devicePassword,
        consent: consentOf('join', impact.data),
      });
      onJoined(outcome);
    } catch (failure) {
      joinWithRecoveryKit.reset();
      if (failure instanceof ApiError && failure.key === IMPACT_CHANGED_KEY) await impact.refetch();
      setError(translateApiError(failure as Error, tc));
    }
  };

  return (
    <form onSubmit={submit} className="grid gap-4">
      {presentation.showReplaceWarning && <JoinWarning />}
      <JoinImpactNotice
        show={presentation.showImpact}
        impact={impact.data}
        error={impact.isError ? (impact.error as Error) : null}
      />
      <div className="grid gap-1.5">
        <Label htmlFor="join-kit-code">{t('join.codeLabel')}</Label>
        <Input
          id="join-kit-code"
          value={code}
          onChange={(event) => {
            setCode(event.target.value);
            setError(null);
          }}
          onKeyDown={submitOnEnter}
          aria-invalid={codeProblem ? true : undefined}
          className="font-mono"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        {codeProblem && (
          <p className="m-0 text-micro font-500 text-loss" role="alert">
            {t(codeProblem)}
          </p>
        )}
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
        askPassword={presentation.askPassword}
        password={password}
        onPassword={(value) => {
          setPassword(value);
          setError(null);
        }}
        server={server}
        serverMode={need === 'typed' ? 'field' : need === 'qr' ? 'fixed' : 'hidden'}
        error={error}
      />
      {footer(
        <>
          <Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>
            {tc(presentation.cancelKey)}
          </Button>
          <Button type="submit" loading={busy} disabled={!ready}>
            {context.kind === 'firstRun' ? t('join.connect') : t('join.kitTitle')}
          </Button>
        </>
      )}
    </form>
  );
}
