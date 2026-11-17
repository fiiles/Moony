import { Laptop, Smartphone, type LucideIcon } from 'lucide-react';

const PHONES: ReadonlySet<string> = new Set(['ios', 'android']);

/** The platform's glyph in the device lists: a phone or a computer. */
export function deviceIcon(platform: string): LucideIcon {
  return PHONES.has(platform) ? Smartphone : Laptop;
}
