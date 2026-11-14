import { useTranslation } from 'react-i18next';
import { AlertTriangle, Check } from 'lucide-react';
import type { ServerChoiceState } from '@/hooks/use-server-choice';
import { hostOf } from '@/utils/sync-setup';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { ChoiceCard } from '@/components/ui/choice-card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RadioGroup } from '@/components/ui/radio-group';

interface ServerChoiceProps {
  state: ServerChoiceState;
  /** Prefix of the element ids; unique on the page. */
  id: string;
  /** Whether the Moony cloud card is shown (disabled, "Coming soon") above the own server. */
  showCloud: boolean;
}

/** The plain-http warning and the acknowledgement that must be ticked before going on. */
export function InsecureNotice({
  id,
  acknowledged,
  onChange,
  body,
}: {
  id: string;
  acknowledged: boolean;
  onChange: (value: boolean) => void;
  /** Replaces the setup wording (a join names the server it is about to use). */
  body?: string;
}) {
  const { t } = useTranslation('deviceSync');
  return (
    <Alert className="mt-2.5 text-caption">
      <AlertTriangle />
      <AlertTitle>{t('setup.server.insecureTitle')}</AlertTitle>
      <AlertDescription>
        {body ?? t('setup.server.insecureBody')}
        <div className="mt-2.5 flex items-center gap-2">
          <Checkbox
            id={`${id}-ack`}
            checked={acknowledged}
            onCheckedChange={(checked) => onChange(checked === true)}
          />
          <Label htmlFor={`${id}-ack`} className="text-caption font-650 text-ink">
            {t('setup.server.insecureAck')}
          </Label>
        </div>
      </AlertDescription>
    </Alert>
  );
}

/**
 * One line under the server field: the problem, what the server answered, "Looking for the
 * server…", or the hint of what the field takes.
 */
export function ServerStatus({ state, id }: { state: ServerChoiceState; id: string }) {
  const { t } = useTranslation('deviceSync');
  const { check, checking, problem } = state;
  if (problem) {
    return (
      <p id={id} className="m-0 text-micro font-500 text-loss" role="alert">
        {problem}
      </p>
    );
  }
  if (check) {
    return (
      <p id={id} className="m-0 flex items-center gap-1 text-micro font-500 text-ink-2">
        <Check className="size-3 text-ink-3" aria-hidden />
        {t('setup.server.found', { version: check.version })}
      </p>
    );
  }
  if (checking) {
    return (
      <p id={id} className="m-0 text-micro font-500 text-ink-3">
        {t('setup.server.checking')}
      </p>
    );
  }
  return (
    <p id={id} className="m-0 text-micro font-500 text-ink-4">
      {state.accepts === 'addressOrCode' ? t('join.serverHint') : t('setup.server.codeHint')}
    </p>
  );
}

/**
 * The server a Recovery Kit QR names, which the person did not type: its host, what it answered
 * and, for plain http, the warning to acknowledge before the join sends anything to it.
 */
export function ServerCheckNotice({ state, id }: { state: ServerChoiceState; id: string }) {
  const { t } = useTranslation('deviceSync');
  const { check, problem } = state;
  return (
    <div className="grid gap-1.5">
      <p className="m-0 text-caption font-650 text-ink">
        {t('join.kitServer', { host: hostOf(state.input) })}
      </p>
      <ServerStatus state={state} id={`${id}-status`} />
      {check?.insecure && !problem && (
        <InsecureNotice
          id={id}
          acknowledged={state.acknowledged}
          onChange={state.setAcknowledged}
        />
      )}
    </div>
  );
}

/**
 * Where the data syncs: the Moony cloud (not open yet) or the person's own server, with the
 * connection code field, what the server answered and, for plain http, the warning that must be
 * acknowledged before going on.
 */
export function ServerChoice({ state, id, showCloud }: ServerChoiceProps) {
  const { t } = useTranslation('deviceSync');
  const { check, problem } = state;
  const fieldId = `${id}-code`;
  const hintId = `${fieldId}-hint`;

  // Joining takes an address or a code, with its own label and hint
  const own = state.accepts === 'addressOrCode';
  return (
    <div className="grid gap-2.5">
      <p className="m-0 text-micro font-500 text-ink-3">{t('setup.server.encrypted')}</p>
      <RadioGroup
        value="own"
        aria-label={t('setup.server.title')}
        className="gap-2.5"
        onValueChange={() => undefined}
      >
        {showCloud && (
          <ChoiceCard
            value="cloud"
            id={`${id}-cloud`}
            disabled
            title={t('setup.server.cloud')}
            badge={<Badge>{t('setup.server.soon')}</Badge>}
            description={t('setup.server.cloudHint')}
          />
        )}
        <ChoiceCard value="own" id={`${id}-own`} selected title={t('setup.server.own')}>
          <div className="mt-3.5 grid gap-1.5">
            <Label htmlFor={fieldId}>
              {own ? t('join.serverLabel') : t('setup.server.codeLabel')}
            </Label>
            <Input
              id={fieldId}
              className="font-mono"
              value={state.input}
              onChange={(event) => state.setInput(event.target.value)}
              aria-invalid={problem ? true : undefined}
              aria-describedby={hintId}
              autoCapitalize="off"
              autoCorrect="off"
              autoComplete="off"
              spellCheck={false}
            />
            <ServerStatus state={state} id={hintId} />
          </div>
          {check?.insecure && !problem && (
            <InsecureNotice
              id={id}
              acknowledged={state.acknowledged}
              onChange={state.setAcknowledged}
            />
          )}
        </ChoiceCard>
      </RadioGroup>
    </div>
  );
}
