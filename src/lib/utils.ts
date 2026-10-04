import { clsx, type ClassValue } from 'clsx';
import { extendTailwindMerge } from 'tailwind-merge';

/**
 * tailwind-merge taught the design tokens from `tailwind.config.ts`. Without
 * this, a custom type role such as `text-micro` is read as a text *color* and
 * silently drops `text-ink-inverse` from the same class list (which is how a
 * small primary button lost its label).
 */
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: [
        'display',
        'h1',
        'modal',
        'stat',
        'h2',
        'h2-section',
        'h3',
        'nav',
        'body',
        'table',
        'caption',
        'micro',
        'eyebrow',
        'thead',
      ],
      'font-weight': ['450', '480', '500', '550', '570', '580', '600', '620', '650', '700', '750'],
      shadow: [
        'sidebar',
        'e1',
        'e2',
        'e3',
        'pop',
        'modal',
        'dark',
        'btn',
        'btn-hover',
        'seg',
        'destructive',
        'focus',
        'loss-ring',
      ],
      radius: ['r1', 'r2', 'r3', 'r4', 'r5'],
      spacing: ['sidebar', 'control', 'control-sm', 'input', 'row'],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
