import { Fragment, type ReactNode } from 'react';
import { splitSlots } from '@/utils/rich-text';

/**
 * A translated sentence whose `slot('name')` placeholders are replaced by nodes (a bold date, a
 * name); see `src/utils/rich-text.ts`.
 */
export function Rich({
  text,
  slots,
}: {
  text: string;
  slots: Readonly<Record<string, ReactNode>>;
}) {
  return (
    <>
      {splitSlots(text).map((part, i) => (
        <Fragment key={i}>{'slot' in part ? (slots[part.slot] ?? null) : part.text}</Fragment>
      ))}
    </>
  );
}
