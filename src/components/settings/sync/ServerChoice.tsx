import { useTranslation } from 'react-i18next';
import { AlertTriangle, Check } from 'lucide-react';
import type { ServerChoiceState } from '@/hooks/use-server-choice';
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

/**
 * Where the data syncs: the Moony cloud (not open yet) or the person's own server, with the
 * connection code field, what the server answered and, for plain http, the warning that must be
 * acknowledged before going on.
 */
export function ServerChoice({ state, id, showCloud }: ServerChoiceProps) {
  const { t } = useTranslation('deviceSync');
  const { check, checking, problem } = state;
  const fieldId = `${id}-code`;
  const hintId = `${fieldId}-hint`;

  let status: React.ReactNode;
  if (problem) {
    status = (
      <p id={hintId} className="m-0 text-micro font-500 text-loss" role="alert">
        {problem}
      </p>
    );
  } else if (check) {
    status = (
      <p id={hintId} className="m-0 flex items-center gap-1 text-micro font-500 text-ink-2">
        <Check className="size-3 text-ink-3" aria-hidden />
        {t('setup.server.found', { version: check.version })}
      </p>
    );
  } else if (checking) {
    status = (
      <p id={hintId} className="m-0 text-micro font-500 text-ink-3">
        {t('setup.server.checking')}
      </p>
    );
  } else {
    status = (
      <p id={hintId} className="m-0 text-micro font-500 text-ink-4">
        {t('setup.server.codeHint')}
      </p>
    );
  }

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
            <Label htmlFor={fieldId}>{t('setup.server.codeLabel')}</Label>
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
            {status}
          </div>
          {check?.insecure && !problem && (
            <Alert className="mt-2.5 text-caption">
              <AlertTriangle />
              <AlertTitle>{t('setup.server.insecureTitle')}</AlertTitle>
              <AlertDescription>
                {t('setup.server.insecureBody')}
                <div className="mt-2.5 flex items-center gap-2">
                  <Checkbox
                    id={`${id}-ack`}
                    checked={state.acknowledged}
                    onCheckedChange={(checked) => state.setAcknowledged(checked === true)}
                  />
                  <Label htmlFor={`${id}-ack`} className="text-caption font-650 text-ink">
                    {t('setup.server.insecureAck')}
                  </Label>
                </div>
              </AlertDescription>
            </Alert>
          )}
        </ChoiceCard>
      </RadioGroup>
    </div>
  );
}
