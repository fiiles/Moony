import { forwardRef, useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

type PasswordInputProps = Omit<React.ComponentProps<typeof Input>, 'type'> & {
  /** Phone screens: the show/hide toggle gets a 44 pt hit area and the field room for it. */
  touch?: boolean;
};

/**
 * Password field with a show/hide toggle (design system §6 Field).
 * Drop-in replacement for `<Input type="password" />` inside react-hook-form
 * `FormControl`s.
 */
export const PasswordInput = forwardRef<HTMLInputElement, PasswordInputProps>(
  function PasswordInput({ className, touch = false, ...props }, ref) {
    const { t } = useTranslation('auth');
    const [visible, setVisible] = useState(false);
    const iconSize = touch ? 'size-5' : 'size-[15px]';
    return (
      <div className="relative">
        <Input
          ref={ref}
          type={visible ? 'text' : 'password'}
          className={cn(
            'pr-10 tracking-[0.08em] num',
            touch && 'pr-12',
            visible && 'tracking-normal',
            className
          )}
          {...props}
        />
        <button
          type="button"
          tabIndex={-1}
          onClick={() => setVisible((v) => !v)}
          className={cn(
            'absolute top-1/2 grid -translate-y-1/2 place-items-center text-ink-4 transition-colors duration-fast hover:bg-well hover:text-ink-2',
            touch
              ? 'right-0.5 size-11 rounded-r3 active:bg-well-2'
              : 'right-1.5 size-7 rounded-[6px]'
          )}
          aria-label={visible ? t('password.hide') : t('password.show')}
          title={visible ? t('password.hide') : t('password.show')}
        >
          {visible ? <EyeOff className={iconSize} /> : <Eye className={iconSize} />}
        </button>
      </div>
    );
  }
);
