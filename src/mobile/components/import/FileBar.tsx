import { FileText } from 'lucide-react';
import { cn } from '@/lib/utils';

interface FileBarProps {
  /** The file's name; it ends with an ellipsis when it is long. */
  name: string;
  /** One line under it: the format and the account it names ("CAMT.053 · CZ65 0800 …"). */
  meta?: string;
  /** The line wraps instead of ending in an ellipsis: what it says is all news (the recognition's "format zatím neznám"). */
  wrapMeta?: boolean;
}

/**
 * The file the import sheet is working on (README §13, `.m-filebar`): a 36 pt paper chip with the
 * file glyph, the name 15 / 600 and a 12 / 500 `ink-4` line under it, on `well`, radius 12, 52 pt
 * at least. It tops every stage after the picker, so the person always sees which file this is.
 */
export function FileBar({ name, meta, wrapMeta = false }: FileBarProps) {
  return (
    <div className="mb-3 flex min-h-[52px] items-center gap-3 rounded-[12px] bg-well px-3 py-2">
      <span
        aria-hidden
        className="grid size-9 flex-none place-items-center rounded-[10px] bg-paper text-ink-3 shadow-e1 [&_svg]:size-4"
      >
        <FileText strokeWidth={1.75} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-m-row text-ink">{name}</span>
        {meta && (
          <span
            className={cn(
              'block text-m-sub text-ink-4 num',
              wrapMeta ? '[overflow-wrap:anywhere]' : 'truncate'
            )}
          >
            {meta}
          </span>
        )}
      </span>
    </div>
  );
}
