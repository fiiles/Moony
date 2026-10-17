import type { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@/lib/utils';
import { EntityRow, RowChip } from '@/mobile/components/EntityRow';

interface SettingsRowProps {
  /** The 16 pt glyph in the chip. */
  icon: LucideIcon;
  title: string;
  /** What the setting is now ("Odemykání obličejem · zámek po 1 minutě"). */
  sub?: ReactNode;
  /** The row opens a page … */
  href?: string;
  /** … or runs an action (a sheet, the lock). */
  onClick?: () => void;
  /** A destructive row: the chip and the title in `loss`. */
  danger?: boolean;
}

/**
 * A row of Nastavení (spec §8.9): the entity row with a rounded-square icon chip, the setting's
 * name, its state in the sub-line and the disclosure chevron (the contract's chevron is for rows
 * without a trailing value, which these are).
 */
export function SettingsRow({
  icon: Icon,
  title,
  sub,
  href,
  onClick,
  danger = false,
}: SettingsRowProps) {
  return (
    <EntityRow
      chip={
        <RowChip square tone={danger ? 'loss' : 'default'}>
          <Icon strokeWidth={1.75} />
        </RowChip>
      }
      title={<span className={cn(danger && 'text-loss')}>{title}</span>}
      sub={sub}
      href={href}
      onClick={onClick}
      chevron
    />
  );
}
