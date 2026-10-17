import { ChevronDown } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

interface SortPillProps<T extends string> {
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  /** Names the choice ("Řazení"): the menu's heading and the pill's accessible name. */
  label: string;
}

/**
 * The sort pill (README §13, `.m-sortpill`): 34 pt on `well-2`, radius 10, the chosen option 12 / 600
 * and a chevron; a 44 pt hit area reaches past it. It opens a radio menu of the options (Radix, so
 * the arrow keys and typeahead come with it) and says the choice at once.
 */
export function SortPill<T extends string>({ value, options, onChange, label }: SortPillProps<T>) {
  const current = options.find((option) => option.value === value);
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          aria-label={current ? `${label}: ${current.label}` : label}
          className="relative inline-flex h-[34px] items-center gap-1 rounded-[10px] bg-well-2 px-2.5 text-m-sub font-semibold text-ink-2 [-webkit-touch-callout:none] after:absolute after:-inset-y-[5px] after:inset-x-0 after:content-[''] active:bg-well-3"
        >
          <span>{current?.label}</span>
          <ChevronDown aria-hidden className="size-3.5 flex-none" strokeWidth={1.75} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="min-w-[200px]">
        <DropdownMenuLabel>{label}</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={value} onValueChange={(next) => onChange(next as T)}>
          {options.map((option) => (
            <DropdownMenuRadioItem
              key={option.value}
              value={option.value}
              className="min-h-11 text-m-body"
            >
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
