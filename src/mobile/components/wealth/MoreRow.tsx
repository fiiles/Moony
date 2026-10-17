interface MoreRowProps {
  /** "Zobrazit všech 12 pozic". */
  label: string;
  onClick: () => void;
}

/**
 * The last row of a folded list (README §13, `.m-rows__more`): 52 pt, the label centred 15 / 600
 * `ink-2`, its divider inset 14 pt like any row without a chip. A tap unfolds the list in place.
 */
export function MoreRow({ label, onClick }: MoreRowProps) {
  return (
    <li className="relative before:pointer-events-none before:absolute before:inset-x-0 before:left-3.5 before:top-0 before:border-t before:border-line-soft before:content-['']">
      <button
        type="button"
        className="flex min-h-[52px] w-full items-center justify-center text-m-row text-ink-2 [-webkit-touch-callout:none] active:bg-well"
        onClick={onClick}
      >
        {label}
      </button>
    </li>
  );
}
