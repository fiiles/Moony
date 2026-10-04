import { forwardRef, useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Input } from '@/components/ui/input';
import { cn } from '@/lib/utils';

type PasswordInputProps = Omit<React.ComponentProps<typeof Input>, 'type'>;

/**
 * Password field with a show/hide toggle (design system §6 Field).
 * Drop-in replacement for `<Input type="password" />` inside react-hook-form
 * `FormControl`s.
 */
export const PasswordInput = forwardRef<HTMLInputElement, PasswordInputProps>(
  function PasswordInput({ className, ...props }, ref) {
    const { t } = useTranslation('auth');
    const [visible, setVisible] = useState(false);
    return (
      <div className="relative">
        <Input
          ref={ref}
          type={visible ? 'text' : 'password'}
          className={cn('pr-10 tracking-[0.08em] num', visible && 'tracking-normal', className)}
          {...props}
        />
        <button
          type="button"
          tabIndex={-1}
          onClick={() => setVisible((v) => !v)}
          className="absolute right-1.5 top-1/2 grid size-7 -translate-y-1/2 place-items-center rounded-[6px] text-ink-4 transition-colors duration-fast hover:bg-well hover:text-ink-2"
          aria-label={visible ? t('password.hide') : t('password.show')}
          title={visible ? t('password.hide') : t('password.show')}
        >
          {visible ? <EyeOff className="size-[15px]" /> : <Eye className="size-[15px]" />}
        </button>
      </div>
    );
  }
);
