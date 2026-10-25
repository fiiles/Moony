import { Search, X } from 'lucide-react';

interface SearchFieldProps {
  value: string;
  onChange: (value: string) => void;
  /** Says what is searched; also the field's accessible name. */
  placeholder: string;
  /** Accessible name of the clear button. */
  clearLabel: string;
}

/**
 * Search field (README §13, `.m-search`): 44 pt on `well-2`, radius 12, the magnifier on the left
 * and a 44 pt clear button on the right while there is text. The keyboard's key says "Search";
 * no autocorrect (names of counterparties).
 */
export function SearchField({ value, onChange, placeholder, clearLabel }: SearchFieldProps) {
  return (
    <div className="relative mb-2.5">
      <Search
        aria-hidden
        className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-ink-4"
        strokeWidth={1.75}
      />
      <input
        type="search"
        inputMode="search"
        enterKeyHint="search"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        aria-label={placeholder}
        className="block h-11 w-full appearance-none rounded-[12px] border-0 bg-well-2 pl-10 pr-10 text-m-field text-ink placeholder:text-ink-4 focus-visible:outline-none focus-visible:shadow-focus [&::-webkit-search-cancel-button]:appearance-none [&::-webkit-search-decoration]:appearance-none"
      />
      {value && (
        <button
          type="button"
          aria-label={clearLabel}
          onClick={() => onChange('')}
          className="absolute right-0 top-0 inline-flex size-11 items-center justify-center rounded-[12px] text-ink-4 [-webkit-touch-callout:none] active:text-ink"
        >
          <X aria-hidden className="size-4" strokeWidth={1.75} />
        </button>
      )}
    </div>
  );
}
