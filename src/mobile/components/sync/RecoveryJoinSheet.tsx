import { useEffect, useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import { PasswordInput } from '@/components/auth/PasswordInput';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useReplaceImpact } from '@/hooks/use-device-sync';
import { useDeviceSyncMutations } from '@/hooks/use-device-sync-mutations';
import { ApiError } from '@/lib/api-error';
import { consentOf } from '@/utils/replace-impact';
import { IMPACT_CHANGED_KEY } from '@/components/settings/sync/replace-outcome';
import { ReplaceImpactBlock } from '@/mobile/components/sync/ReplaceImpactBlock';
import { translateApiError } from '@/lib/translate-api-error';
import { scannedCodeKind } from '@/utils/pairing-code';
import { backupFileName } from '@/utils/sync-status';
import { Sheet } from '@/mobile/components/Sheet';
import type { ReplaceOutcomeView } from '@shared/schema';
import { kitCodeProblem, phoneDeviceKey } from '@/mobile/lib/sync-scan';
import { field48, fieldMessage } from '@/mobile/pages/first-run/styles';

interface RecoveryJoinSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The join is done: its headline and outcome (the page says where the backup is). */
  onReplaced: (headline: string, outcome: ReplaceOutcomeView) => void;
}

/** A field that takes a code or an address never feeds the keyboard's corrections. */
const PLAIN_FIELD = { autoCapitalize: 'none', autoCorrect: 'off', spellCheck: false } as const;

/** The 48 pt buttons of a sheet's footer. */
const BUTTON = 'h-auto min-h-12 whitespace-normal py-2 text-center rounded-[10px] text-m-row';

/**
 * "Type a Recovery Kit code" (sync off): the recovery code, typed or pasted, or the text of the
 * Kit's QR; the account number when a bare code was typed; the device name and password; and under
 * Advanced the sync server address. It reads Recovery Kit codes only (review PF2): a pairing code
 * is "This is not a Recovery Kit code". The join replaces this phone's data after a backup: the sheet
 * steps aside for the busy cover while it runs and comes back with the error when it fails, the
 * fields as they were (the mutation's copy of the password is reset).
 */
export function RecoveryJoinSheet({ open, onOpenChange, onReplaced }: RecoveryJoinSheetProps) {
  const { t } = useTranslation('deviceSync');
  const { t: tc } = useTranslation('common');
  const { joinWithRecoveryKit } = useDeviceSyncMutations();
  // What the join puts at stake, read each time the sheet opens; the join sends its consent
  const impact = useReplaceImpact(open);
  const shownImpact = impact.isFetching ? undefined : impact.data;
  const resetJoin = joinWithRecoveryKit.reset;
  // The password stays in the mutation state until it succeeds: leaving clears it
  useEffect(() => resetJoin, [resetJoin]);
  const fieldId = useId();

  const [code, setCode] = useState('');
  const [account, setAccount] = useState('');
  const [name, setName] = useState(() =>
    t(`setup.defaultName.${phoneDeviceKey(import.meta.env.TAURI_ENV_PLATFORM ?? '')}`)
  );
  const [password, setPassword] = useState('');
  const [relayUrl, setRelayUrl] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Closed: the code, the account and the password go with it (the name and the server stay)
  if (!open && (code !== '' || account !== '' || password !== '' || error !== null)) {
    setCode('');
    setAccount('');
    setPassword('');
    setError(null);
  }

  const busy = joinWithRecoveryKit.isPending;
  const codeProblem = kitCodeProblem(code);
  const needsAccount = scannedCodeKind(code) === 'recoveryCode';
  const ready =
    code.trim() !== '' &&
    codeProblem === null &&
    password !== '' &&
    shownImpact !== undefined &&
    name.trim() !== '' &&
    (!needsAccount || account.trim() !== '');

  const change = (next: boolean) => {
    if (!next && busy) return;
    onOpenChange(next);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy || !shownImpact) return;
    setError(null);
    try {
      const outcome = await joinWithRecoveryKit.mutateAsync({
        code: code.trim(),
        account: needsAccount ? account.trim() : undefined,
        relayUrl: relayUrl.trim() === '' ? undefined : relayUrl.trim(),
        deviceName: name.trim(),
        password,
        consent: consentOf('join', shownImpact),
      });
      onOpenChange(false);
      onReplaced(t('pairing.joined', { path: backupFileName(outcome.backupPath) }), outcome);
    } catch (failure) {
      joinWithRecoveryKit.reset();
      // What the sheet showed no longer holds: read again, the new numbers replace the old
      if (failure instanceof ApiError && failure.key === IMPACT_CHANGED_KEY) await impact.refetch();
      setError(translateApiError(failure as Error, tc));
    }
  };

  const formId = `${fieldId}-form`;
  return (
    <Sheet
      // Out of the way of the busy cover while the data is replaced
      open={open && !busy}
      onOpenChange={change}
      title={t('join.kitTitle')}
      description={t('join.warning')}
      dirty={code !== '' || account !== ''}
      footer={
        <>
          <Button variant="ghost" className={`${BUTTON} px-[18px]`} onClick={() => change(false)}>
            {tc('buttons.cancel')}
          </Button>
          <Button type="submit" form={formId} className={`${BUTTON} flex-1`} disabled={!ready}>
            {t('join.confirm')}
          </Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate className="space-y-3">
        <p className="m-0 text-m-sub text-ink-3">{t('join.kitHint')}</p>
        <ReplaceImpactBlock
          flow="join"
          impact={shownImpact}
          error={impact.isError ? (impact.error as Error) : null}
        />
        <div className="space-y-1.5">
          <Label htmlFor={`${fieldId}-code`}>{t('join.codeLabel')}</Label>
          <textarea
            id={`${fieldId}-code`}
            value={code}
            rows={3}
            onChange={(event) => {
              setCode(event.target.value);
              setError(null);
            }}
            aria-invalid={codeProblem ? true : undefined}
            className="m-0 block w-full resize-none rounded-[10px] border border-line-strong bg-paper px-3.5 py-3 font-mono text-m-field text-ink transition-[border-color,box-shadow] duration-fast focus:border-focus focus:shadow-focus focus-visible:outline-none aria-[invalid=true]:border-loss"
            {...PLAIN_FIELD}
          />
          {codeProblem && (
            <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
              {t(codeProblem)}
            </p>
          )}
        </div>
        {needsAccount && (
          <div className="space-y-1.5">
            <Label htmlFor={`${fieldId}-account`}>{t('join.accountLabel')}</Label>
            <Input
              id={`${fieldId}-account`}
              value={account}
              onChange={(event) => setAccount(event.target.value)}
              inputMode="numeric"
              autoComplete="off"
              enterKeyHint="next"
              className={field48}
            />
          </div>
        )}
        <div className="space-y-1.5">
          <Label htmlFor={`${fieldId}-name`}>{t('join.nameLabel')}</Label>
          <Input
            id={`${fieldId}-name`}
            value={name}
            maxLength={60}
            onChange={(event) => setName(event.target.value)}
            autoComplete="off"
            enterKeyHint="next"
            className={field48}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor={`${fieldId}-password`}>{t('join.passwordLabel')}</Label>
          <PasswordInput
            id={`${fieldId}-password`}
            touch
            value={password}
            onChange={(event) => {
              setPassword(event.target.value);
              setError(null);
            }}
            autoComplete="current-password"
            enterKeyHint="go"
            className={field48}
            aria-invalid={error ? true : undefined}
            {...PLAIN_FIELD}
          />
          {error && (
            <p role="alert" className={`m-0 text-loss ${fieldMessage}`}>
              {error}
            </p>
          )}
        </div>
        <Collapsible>
          <CollapsibleTrigger className="group flex min-h-11 items-center gap-1 text-m-sub font-semibold text-ink-2">
            <ChevronRight
              aria-hidden
              className="size-4 transition-transform group-data-[state=open]:rotate-90"
            />
            {t('join.advanced')}
          </CollapsibleTrigger>
          <CollapsibleContent className="space-y-1.5 pt-1">
            <Label htmlFor={`${fieldId}-server`}>{t('join.serverLabel')}</Label>
            <Input
              id={`${fieldId}-server`}
              value={relayUrl}
              onChange={(event) => setRelayUrl(event.target.value)}
              placeholder={t('setup.advanced.placeholder')}
              inputMode="url"
              className={field48}
              {...PLAIN_FIELD}
            />
            <p className="m-0 text-m-sub text-ink-4">{t('setup.advanced.hint')}</p>
          </CollapsibleContent>
        </Collapsible>
      </form>
    </Sheet>
  );
}
