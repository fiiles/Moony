import * as React from 'react';

import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import { isMobileShell } from '@/lib/platform';
import { amountInputText, decimalTextToDot } from '@/mobile/lib/amount-input';

const PHONE = isMobileShell(import.meta.env);

/**
 * Text input (design system §6, `.input`): 39 px, radius 8, paper surface,
 * focus = focus-border + 3 px ring, invalid (aria-invalid from FormControl)
 * = loss border. Units sit inside the field on the right via <InputWrap>.
 */
const inputClassName =
  'flex h-input w-full rounded-r2 border border-line-strong bg-paper px-[11px] text-body text-ink transition-[border-color,box-shadow] duration-fast file:border-0 file:bg-transparent file:text-table file:font-600 file:text-ink placeholder:text-ink-5 hover:border-line-hover focus:border-focus focus:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:bg-well disabled:text-ink-4 aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring';

/**
 * In the phone shell a decimal `type="number"` field becomes a text field with the decimal keypad:
 * WKWebView refuses a comma in a number field when the keypad language differs from the page's, and
 * shows no decimal pad. The field keeps the text as typed ("1 234,5"), while `value` and `onChange`
 * keep speaking dot text ("1234.5"), so react-hook-form and the zod schemas see what a desktop
 * number input gives them. Integer fields (`inputMode="numeric"`) stay number inputs: digits only.
 * `inputMode="text"` opts a signed field (a balance) into the full keyboard, which has a minus.
 */
const PhoneDecimalInput = React.forwardRef<HTMLInputElement, React.ComponentProps<'input'>>(
  ({ className, type: _type, min: _min, max: _max, step: _step, inputMode, ...props }, ref) => {
    const { i18n } = useTranslation();
    const { value, defaultValue, onChange } = props;
    const external = value === undefined ? undefined : value === 0 ? '' : String(value);
    const [text, setText] = React.useState(() => {
      const initial = value === undefined ? defaultValue : value;
      return typeof initial === 'number' || typeof initial === 'string'
        ? initial === 0
          ? ''
          : amountInputText(initial, i18n.language)
        : '';
    });
    // A value changed from outside (a reset, a prefill) replaces the text; the parent echoing what
    // was just typed does not (it reads the same number).
    React.useEffect(() => {
      if (external === undefined) return;
      setText((current) =>
        decimalTextToDot(current) === external ? current : amountInputText(external, i18n.language)
      );
    }, [external, i18n.language]);

    return (
      <input
        {...props}
        ref={ref}
        type="text"
        inputMode={inputMode ?? 'decimal'}
        autoComplete="off"
        value={value === undefined ? undefined : text}
        defaultValue={value === undefined ? defaultValue : undefined}
        className={cn(inputClassName, className)}
        onChange={(event) => {
          const typed = event.target.value;
          setText(typed);
          onChange?.({
            ...event,
            target: {
              name: event.target.name,
              id: event.target.id,
              value: decimalTextToDot(typed),
            },
            currentTarget: event.currentTarget,
          } as unknown as React.ChangeEvent<HTMLInputElement>);
        }}
      />
    );
  }
);
PhoneDecimalInput.displayName = 'PhoneDecimalInput';

const Input = React.forwardRef<HTMLInputElement, React.ComponentProps<'input'>>(
  ({ className, type, ...props }, ref) => {
    if (
      PHONE &&
      type === 'number' &&
      (props.inputMode === undefined || props.inputMode === 'decimal' || props.inputMode === 'text')
    ) {
      return <PhoneDecimalInput className={className} ref={ref} {...props} />;
    }
    return <input type={type} className={cn(inputClassName, className)} ref={ref} {...props} />;
  }
);
Input.displayName = 'Input';

interface InputWrapProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Unit shown inside the field on the right ("Kč", "ks", "USD", "%"). */
  unit?: React.ReactNode;
  /** Leading icon (search fields). */
  icon?: React.ReactNode;
}

/** `.input-wrap`: positions a unit on the right or an icon on the left of an <Input>. */
const InputWrap = React.forwardRef<HTMLDivElement, InputWrapProps>(
  ({ className, unit, icon, children, ...props }, ref) => (
    <div
      ref={ref}
      className={cn(
        'relative',
        icon && '[&>input]:pl-[33px]',
        unit && '[&>input]:pr-[42px]',
        className
      )}
      {...props}
    >
      {icon && (
        <span className="pointer-events-none absolute left-[11px] top-1/2 -translate-y-1/2 text-ink-4 [&_svg]:size-3.5">
          {icon}
        </span>
      )}
      {children}
      {unit && (
        <span className="pointer-events-none absolute right-[11px] top-1/2 -translate-y-1/2 text-caption font-600 text-ink-4">
          {unit}
        </span>
      )}
    </div>
  )
);
InputWrap.displayName = 'InputWrap';

export { Input, InputWrap, inputClassName };
