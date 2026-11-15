import type { ReactNode } from 'react';

/**
 * The sub-line of a pushed detail screen (README §13, "Detail screens"): the one line under the
 * large title that says what the record is ("Fio · CZK · 2100123456/2010", "Apple Inc. · USD"),
 * 13 / 450 `ink-3` like a tab root's sentence. It wraps rather than ending in an ellipsis; `icon`
 * (an instrument's logo) leads it.
 */
export function DetailSub({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return (
    <p className="mx-0.5 mb-3.5 mt-0 flex items-start gap-1.5 text-m-lead text-ink-3">
      {icon && <span className="flex h-[1.4em] flex-none items-center">{icon}</span>}
      <span className="min-w-0 break-words">{children}</span>
    </p>
  );
}
