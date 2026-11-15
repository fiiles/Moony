import { CreditCard, Landmark, LineChart, PiggyBank, type LucideIcon } from 'lucide-react';
import type { AccountType } from '@shared/schema';
import { RowChip } from '@/mobile/components/EntityRow';

const ICONS: Record<AccountType, LucideIcon> = {
  checking: Landmark,
  savings: PiggyBank,
  credit_card: CreditCard,
  investment: LineChart,
};

/**
 * The avatar of an account row, one for every account (README §13, "Transactions", audit
 * 2026-11-15 C3): its type's icon in the neutral 36 pt circle. Initials of the bank made two
 * accounts at one bank look alike and differed from every other row's avatar.
 */
export function AccountChip({ type }: { type: AccountType | null | undefined }) {
  const Icon = (type && ICONS[type]) || Landmark;
  return (
    <RowChip>
      <Icon strokeWidth={1.75} />
    </RowChip>
  );
}
