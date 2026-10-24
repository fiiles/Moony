/**
 * The head of one group inside a property's section (README §13, `.m-sec--group`): its name (12 /
 * 600 `ink-3`) and, at the right, the action that adds to it — a text button whose hit area reaches
 * 44 pt. Under "Finance" a group is the loans, the policies and the yearly balance; under
 * "Náklady" the recurring and the one-time costs.
 */
export function GroupHead({
  title,
  action,
}: {
  title: string;
  action?: { label: string; onClick: () => void };
}) {
  return (
    <div className="mx-0.5 mb-2 mt-4 flex items-baseline justify-between gap-3">
      <h3 className="m-0 min-w-0 truncate text-m-sub font-semibold text-ink-3">{title}</h3>
      {action && (
        <button
          type="button"
          onClick={action.onClick}
          className="relative flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3 [-webkit-touch-callout:none] after:absolute after:-inset-x-2 after:-inset-y-[14px] after:content-[''] active:text-ink"
        >
          {action.label}
        </button>
      )}
    </div>
  );
}
