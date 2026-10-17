import type { ReactNode } from 'react';
import { Switch } from '@/components/ui/switch';
import { cn } from '@/lib/utils';

interface SwitchRowProps {
  id: string;
  label: string;
  /** What the switch does, or why it cannot be turned on; wraps. */
  sub?: ReactNode;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}

/**
 * A setting with a switch (README §13, `.m-switchrow`): the 15 / 450 label and a 12 `ink-4`
 * sentence under it on the left, the switch at the right. The whole row, 44 pt at least, is the
 * `<label>`, so a tap anywhere toggles it; the sentence describes the switch to a screen reader.
 */
export function SwitchRow({
  id,
  label,
  sub,
  checked,
  onCheckedChange,
  disabled = false,
}: SwitchRowProps) {
  return (
    <label
      htmlFor={id}
      className={cn(
        'flex min-h-11 w-full items-center gap-3 px-1 py-2 [-webkit-touch-callout:none]',
        disabled ? 'cursor-default' : 'cursor-pointer'
      )}
    >
      <span className="min-w-0 flex-1">
        <span className="block text-m-body text-ink">{label}</span>
        {sub && (
          <span id={`${id}-sub`} className="mt-0.5 block text-m-sub text-ink-4">
            {sub}
          </span>
        )}
      </span>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        aria-describedby={sub ? `${id}-sub` : undefined}
      />
    </label>
  );
}
