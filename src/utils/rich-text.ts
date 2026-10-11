/**
 * Translated sentences with a bold date or a linked name in the middle: the copy carries a plain
 * `{{placeholder}}`, the caller passes `slot('name')` for it and `<Rich>` swaps each slot for a
 * node. The slot text uses private-use characters, so i18next leaves it alone and no real copy can
 * contain it.
 */
const OPEN = '';
const CLOSE = '';

/** Text to interpolate where the node called `name` goes. */
export function slot(name: string): string {
  return `${OPEN}${name}${CLOSE}`;
}

export type RichPart = { text: string } | { slot: string };

/** Splits a sentence into plain text and slots, in order; empty text is dropped. */
export function splitSlots(text: string): RichPart[] {
  const parts: RichPart[] = [];
  let rest = text;
  for (;;) {
    const open = rest.indexOf(OPEN);
    const close = open < 0 ? -1 : rest.indexOf(CLOSE, open);
    if (open < 0 || close < 0) break;
    if (open > 0) parts.push({ text: rest.slice(0, open) });
    parts.push({ slot: rest.slice(open + 1, close) });
    rest = rest.slice(close + 1);
  }
  if (rest) parts.push({ text: rest });
  return parts;
}
