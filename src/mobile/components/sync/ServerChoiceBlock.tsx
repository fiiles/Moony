import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Check, ScanLine } from 'lucide-react';
import type { ServerChoiceState } from '@/hooks/use-server-choice';
import { hostOf } from '@/utils/sync-setup';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { InlineAlert } from '@/mobile/components/InlineAlert';
import { field48 } from '@/mobile/pages/first-run/styles';

/** The plain-http warning and the acknowledgement that must be ticked before going on (44 pt row). */
export function InsecureBlock({
  acknowledged,
  onChange,
  body,
}: {
  acknowledged: boolean;
  onChange: (value: boolean) => void;
  /** Replaces the setup wording (a join names the server it is about to use). */
  body?: string;
}) {
  const { t } = useTranslation('deviceSync');
  const id = useId();
  return (
    <div className="mt-3">
      <InlineAlert icon={AlertTriangle} title={t('setup.server.insecureTitle')}>
        {body ?? t('setup.server.insecureBody')}
      </InlineAlert>
      <div className="flex min-h-11 items-center gap-3">
        <Checkbox
          id={id}
          checked={acknowledged}
          onCheckedChange={(checked) => onChange(checked === true)}
        />
        <Label htmlFor={id} className="text-m-row">
          {t('setup.server.insecureAck')}
        </Label>
      </div>
    </div>
  );
}

/**
 * One line under the server field: the problem, what the server answered, "Looking for the
 * server…", or the hint of what the field takes.
 */
export function ServerStatusLine({ state, id }: { state: ServerChoiceState; id: string }) {
  const { t } = useTranslation('deviceSync');
  const { check, checking, problem } = state;
  if (problem) {
    return (
      <p id={id} role="alert" className="m-0 text-m-sub text-loss">
        {problem}
      </p>
    );
  }
  if (check) {
    return (
      <p id={id} className="m-0 flex items-center gap-1.5 text-m-sub text-ink-2">
        <Check aria-hidden className="size-3.5 flex-none text-ink-3" />
        {t('setup.server.found', { version: check.version })}
      </p>
    );
  }
  const hint =
    state.accepts === 'addressOrCode' ? t('join.serverHint') : t('setup.server.codeHint');
  return (
    <p id={id} className={`m-0 text-m-sub ${checking ? 'text-ink-3' : 'text-ink-4'}`}>
      {checking ? t('setup.server.checking') : hint}
    </p>
  );
}

/**
 * The server a Recovery Kit QR names, which the person did not type: its host, what it answered
 * and, for plain http, the warning to acknowledge before the join sends anything to it.
 */
export function ServerCheckBlock({ state }: { state: ServerChoiceState }) {
  const { t } = useTranslation('deviceSync');
  const id = useId();
  const { check, problem } = state;
  return (
    <div className="space-y-1.5">
      <p className="m-0 text-m-row text-ink">
        {t('join.kitServer', { host: hostOf(state.input) })}
      </p>
      <ServerStatusLine state={state} id={`${id}-status`} />
      {check?.insecure && !problem && (
        <InsecureBlock acknowledged={state.acknowledged} onChange={state.setAcknowledged} />
      )}
    </div>
  );
}

interface ServerChoiceBlockProps {
  state: ServerChoiceState;
  /** Whether the Moony cloud card is shown (disabled, "Coming soon") above the own server. */
  showCloud: boolean;
  /** Opens the camera for a connection code; without it there is no "Scan code" button. */
  onScan?: () => void;
}

/**
 * The phone's "where should your data sync?": the Moony cloud (not open yet) and the person's own
 * server, with the connection code field (typed, pasted or scanned), what the server answered and,
 * for plain http, the warning to acknowledge. The desktop's `ServerChoice` in the phone's type
 * scale, 48 pt fields and 44 pt targets.
 */
export function ServerChoiceBlock({ state, showCloud, onScan }: ServerChoiceBlockProps) {
  const { t } = useTranslation('deviceSync');
  const fieldId = useId();
  const hintId = `${fieldId}-hint`;
  const { check, problem } = state;
  // Joining takes an address or a code, with its own label and hint
  const own = state.accepts === 'addressOrCode';

  return (
    <div className="space-y-3">
      <p className="m-0 text-m-sub text-ink-3">{t('setup.server.encrypted')}</p>
      {showCloud && (
        <div
          aria-disabled
          className="rounded-[11px] border border-line-strong bg-paper px-3.5 py-3 opacity-60"
        >
          <div className="flex items-center justify-between gap-2 text-m-row text-ink">
            <span>{t('setup.server.cloud')}</span>
            <Badge>{t('setup.server.soon')}</Badge>
          </div>
          <p className="m-0 mt-1 text-m-sub text-ink-3">{t('setup.server.cloudHint')}</p>
        </div>
      )}
      <div className="rounded-[11px] border border-focus bg-paper px-3.5 py-3 shadow-focus">
        <p className="m-0 text-m-row text-ink">{t('setup.server.own')}</p>
        <div className="mt-3 space-y-1.5">
          <Label htmlFor={fieldId}>
            {own ? t('join.serverLabel') : t('setup.server.codeLabel')}
          </Label>
          <Input
            id={fieldId}
            value={state.input}
            onChange={(event) => state.setInput(event.target.value)}
            aria-invalid={problem ? true : undefined}
            aria-describedby={hintId}
            className={`${field48} font-mono`}
            autoCapitalize="off"
            autoCorrect="off"
            autoComplete="off"
            spellCheck={false}
            enterKeyHint="go"
          />
          <ServerStatusLine state={state} id={hintId} />
          {onScan && (
            <Button
              variant="secondary"
              className="h-auto min-h-12 w-full whitespace-normal rounded-[10px] py-2 text-center text-m-row [&_svg]:size-[18px]"
              onClick={onScan}
            >
              <ScanLine aria-hidden />
              {t('setup.server.scan')}
            </Button>
          )}
        </div>
        {check?.insecure && !problem && (
          <InsecureBlock acknowledged={state.acknowledged} onChange={state.setAcknowledged} />
        )}
      </div>
    </div>
  );
}
