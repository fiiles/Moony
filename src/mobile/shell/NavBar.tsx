import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface NavBarProps {
  title: string;
  /** Tab root: the large title sits in the content, the bar title waits until it has scrolled away. */
  large?: boolean;
  /** Content has moved under the bar (on a tab root: past the large title). */
  collapsed: boolean;
  /** At most one item: a back button or the hide-amounts eye. */
  leading?: ReactNode;
  /** At most two 44 pt buttons. */
  trailing?: ReactNode;
}

/**
 * Nav bar (design system README §13, `.m-nav`): 44 pt under the top safe area, the 20 pt title
 * centred between the slots. A collapsed bar shows its title and a hairline.
 */
export function NavBar({ title, large = false, collapsed, leading, trailing }: NavBarProps) {
  // On a tab root the large title is the page heading; the bar copy only mirrors it.
  const Title = large ? 'div' : 'h1';
  return (
    <header
      className={cn(
        'grid h-[calc(var(--m-nav-h)+var(--safe-top))] flex-none select-none grid-cols-[1fr_minmax(0,auto)_1fr] items-center gap-1 border-b border-transparent px-2.5 pt-[var(--safe-top)] transition-colors duration-base ease-moony',
        collapsed && 'border-line'
      )}
    >
      <div className="flex items-center gap-0.5">{leading}</div>
      <Title
        aria-hidden={large || undefined}
        className={cn(
          'min-w-0 truncate text-center text-m-title text-ink transition-opacity duration-base ease-moony',
          large && !collapsed && 'opacity-0'
        )}
      >
        {title}
      </Title>
      <div className="flex items-center justify-end gap-0.5">{trailing}</div>
    </header>
  );
}
