import * as React from 'react';

import { useTranslation } from 'react-i18next';

import { cn } from '@/lib/utils';
import { isMobileShell } from '@/lib/platform';
import { decimalSeparator, decimalTextToDot, shownText } from '@/mobile/lib/amount-input';

const PHONE = isMobileShell(import.meta.env);

/**
 * Text input (design system §6, `.input`): 39 px, radius 8, paper surface,
 * focus = focus-border + 3 px ring, invalid (aria-invalid from FormControl)
 * = loss border. Units sit inside the field on the right via <InputWrap>.
 */
const inputClassName =
  'flex h-input w-full rounded-r2 border border-line-strong bg-paper px-[11px] text-body text-ink transition-[border-color,box-shadow] duration-fast file:border-0 file:bg-transparent file:text-table file:font-600 file:text-ink placeholder:text-ink-5 hover:border-line-hover focus:border-focus focus:shadow-focus focus-visible:outline-none disabled:cursor-not-allowed disabled:bg-well disabled:text-ink-4 aria-[invalid=true]:border-loss aria-[invalid=true]:focus:shadow-loss-ring';

type InputProps = React.ComponentProps<'input'> & {
  /** A field that may be negative: the phone shell gives it the full keyboard (the decimal pad has no minus). No effect on desktop. */
  signed?: boolean;
};

/**
 * An event whose `target` reads `value` as the dot text. Everything else (preventDefault,
 * stopPropagation, currentTarget, the element's other properties) is the real event's and the real
 * element's; the methods are bound to them.
 */
function withDotValue<E extends React.SyntheticEvent<HTMLInputElement>>(event: E, dot: string): E {
  const element = event.target as HTMLInputElement;
  const target = new Proxy(element, {
    get(el, prop) {
      if (prop === 'value') return dot;
      const value = Reflect.get(el, prop, el);
      return typeof value === 'function' ? value.bind(el) : value;
    },
  });
  return new Proxy(event, {
    get(ev, prop) {
      if (prop === 'target') return target;
      const value = Reflect.get(ev, prop, ev);
      return typeof value === 'function' ? value.bind(ev) : value;
    },
  });
}

/**
 * In the phone shell a decimal `type="number"` field becomes a text field with the decimal keypad:
 * WKWebView refuses a comma in a number field when the keypad language differs from the page's, and
 * shows no decimal pad. The field keeps the text as typed ("1 234,5"), while `value` and the
 * `target.value` that `onChange` and `onBlur` read keep speaking dot text ("1234.5"), so
 * react-hook-form and the zod schemas see what a desktop number input gives them. What the field
 * shows after its `value` changes is decided by `shownText`. Integer fields (`inputMode="numeric"`)
 * stay number inputs: digits only.
 */
const PhoneDecimalInput = React.forwardRef<HTMLInputElement, InputProps>(
  (
    {
      className,
      type: _type,
      min: _min,
      max: _max,
      step: _step,
      signed,
      inputMode,
      placeholder,
      onChange,
      onBlur,
      ...props
    },
    ref
  ) => {
    const { i18n } = useTranslation();
    const locale = i18n.language;
    const controlled = Object.hasOwn(props, 'value');
    const { value, defaultValue } = props;
    const [text, setText] = React.useState(
      () => shownText('', controlled ? value : defaultValue, locale) ?? ''
    );
    React.useEffect(() => {
      if (!controlled) return;
      setText((current) => shownText(current, value, locale) ?? current);
    }, [controlled, value, locale]);

    return (
      <input
        {...props}
        ref={ref}
        type="text"
        inputMode={signed ? 'text' : (inputMode ?? 'decimal')}
        autoComplete="off"
        placeholder={
          placeholder && /^[0-9.]+$/.test(placeholder)
            ? placeholder.replace('.', decimalSeparator(locale))
            : placeholder
        }
        value={controlled ? text : undefined}
        defaultValue={controlled ? undefined : text}
        className={cn(inputClassName, className)}
        onChange={(event) => {
          const typed = event.target.value;
          setText(typed);
          onChange?.(withDotValue(event, decimalTextToDot(typed)));
        }}
        onBlur={(event) => onBlur?.(withDotValue(event, decimalTextToDot(event.target.value)))}
      />
    );
  }
);
PhoneDecimalInput.displayName = 'PhoneDecimalInput';

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, signed: _signed, ...props }, ref) => {
    if (
      PHONE &&
      type === 'number' &&
      (props.inputMode === undefined || props.inputMode === 'decimal')
    ) {
      return <PhoneDecimalInput className={className} ref={ref} signed={_signed} {...props} />;
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
