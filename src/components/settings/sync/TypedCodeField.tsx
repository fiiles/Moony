import { useEffect, useId, useRef, type FormEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { cn } from '@/lib/utils';
import { isCompleteCode, normalizeTypedCode } from '@/utils/pairing-code';

interface TypedCodeFieldProps {
  /** The digits typed so far (already normalised: `normalizeTypedCode`). */
  value: string;
  onChange: (digits: string) => void;
  /** Enter (or the form's submit button) with six digits typed. */
  onSubmit: (digits: string) => void;
  /** Wrong codes the person may still type; shown while the backend says. */
  attemptsLeft: number | null;
  /** The last confirm's answer (a wrong code, a momentary failure), already translated. */
  errorMessage: string | null;
  disabled?: boolean;
  /** The phone's sheet puts its submit button in the footer and names this form there. */
  formId?: string;
  /** The phone's field: 16 px text, taller. */
  phone?: boolean;
  /** The inline Confirm button (desktop). */
  children?: ReactNode;
}

/**
 * The holder's six digits: typed from the joining device's screen. Digits only, at most six
 * (`normalizeTypedCode`: spaces, dashes and full-width digits are handled); text that is not six
 * digits never reaches the backend, so it never spends an attempt.
 */
export function TypedCodeField({
  value,
  onChange,
  onSubmit,
  attemptsLeft,
  errorMessage,
  disabled = false,
  formId,
  phone = false,
  children,
}: TypedCodeFieldProps) {
  const { t } = useTranslation('deviceSync');
  const id = useId();
  const input = useRef<HTMLInputElement>(null);
  // A disabled field loses focus: when it is enabled again after a confirm, the person types on.
  const wasDisabled = useRef(disabled);
  useEffect(() => {
    if (wasDisabled.current && !disabled) input.current?.focus();
    wasDisabled.current = disabled;
  }, [disabled]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!disabled && isCompleteCode(value)) onSubmit(normalizeTypedCode(value));
  };

  return (
    <form id={formId} onSubmit={submit} noValidate className="grid w-full gap-2">
      <Label htmlFor={id} className={cn('text-center', phone && 'text-m-lead')}>
        {t('pairing.typePrompt')}
      </Label>
      <Input
        id={id}
        ref={input}
        type="text"
        inputMode="numeric"
        autoComplete="one-time-code"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        autoFocus={!phone}
        value={value}
        disabled={disabled}
        placeholder="000000"
        aria-invalid={errorMessage ? true : undefined}
        aria-describedby={`${id}-note`}
        data-testid="pairing-typed-code"
        className={cn(
          'mx-auto text-center font-650 tracking-[0.3em] num',
          phone ? 'h-14 max-w-[260px] rounded-[10px] text-m-display' : 'max-w-[220px] text-[22px]'
        )}
        onChange={(event) => onChange(normalizeTypedCode(event.target.value))}
      />
      <div id={`${id}-note`} className="grid gap-1 text-center">
        {errorMessage && (
          <p role="alert" className={cn('m-0 text-loss', phone ? 'text-m-sub' : 'text-micro')}>
            {errorMessage}
          </p>
        )}
        {attemptsLeft !== null && (
          <p className={cn('m-0 text-ink-3', phone ? 'text-m-sub' : 'text-micro')}>
            {t('pairing.attemptsLeft', { count: attemptsLeft })}
          </p>
        )}
      </div>
      {children}
    </form>
  );
}
