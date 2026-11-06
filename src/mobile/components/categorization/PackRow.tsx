import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { RulePackInfo } from '@shared/schema';
import { Switch } from '@/components/ui/switch';

interface PackRowProps {
  pack: RulePackInfo;
  label: string;
  /** The switch waits while a pack write is in flight (the next list is computed from the server's state). */
  disabled: boolean;
  onToggle: (enabled: boolean) => void;
  onOpen: () => void;
}

/** One rule pack: the text opens its rules, the switch turns the pack on or off. */
export function PackRow({ pack, label, disabled, onToggle, onOpen }: PackRowProps) {
  const { t: ts } = useTranslation('settings');
  const { t } = useTranslation('mobile');
  return (
    <li className="flex min-h-[52px] list-none items-center gap-1 border-t border-line-soft first:border-t-0">
      <button
        type="button"
        onClick={onOpen}
        aria-label={t('settings.categorization.openPack', { name: label })}
        className="flex min-h-11 min-w-0 flex-1 items-center gap-2 py-2 pl-3.5 text-left [-webkit-touch-callout:none] active:bg-well focus-visible:outline-none focus-visible:shadow-focus"
      >
        <span className="min-w-0 flex-1">
          <span className="block truncate text-m-row text-ink">{label}</span>
          <span className="mt-0.5 block text-m-sub text-ink-4">
            {ts('rulePacks.rulesCount', { count: pack.ruleCount })}
          </span>
        </span>
        <ChevronRight aria-hidden className="size-4 flex-none text-ink-5" strokeWidth={1.75} />
      </button>
      <span className="flex size-11 flex-none items-center justify-center pr-1.5">
        <Switch
          checked={pack.enabled}
          disabled={disabled}
          onCheckedChange={onToggle}
          aria-label={t('settings.categorization.packSwitch', { name: label })}
        />
      </span>
    </li>
  );
}
