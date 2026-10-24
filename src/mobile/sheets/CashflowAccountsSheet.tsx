import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { BankAccountWithInstitution } from '@shared/schema';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Sheet } from '@/mobile/components/Sheet';

interface CashflowAccountsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  accounts: BankAccountWithInstitution[];
  /** The accounts in the report; `undefined` is all of them. */
  value: string[] | undefined;
  /** The chosen accounts, applied once, from "Hotovo". */
  onDone: (ids: string[]) => void;
}

/** The ids the report counts: `value`, or every account. */
function draftOf(value: string[] | undefined, accounts: BankAccountWithInstitution[]) {
  return new Set(value ?? accounts.map((account) => account.id));
}

/**
 * "Účty v přehledu" (spec §8.7, the `···` menu): which accounts the report counts, as the
 * desktop's account scope. One 52 pt row per account — a checkbox, the name, the bank under it —
 * and "Všechny účty" to tick them all. The sheet edits a draft that "Hotovo" (waiting while
 * nothing is ticked) hands over once: the screen behind writes the report's accounts to the URL,
 * and doing that while the sheet is open would replace the history entry the sheet owns. The
 * scrim, Escape and Android back close the sheet and drop the draft.
 */
export function CashflowAccountsSheet({
  open,
  onOpenChange,
  accounts,
  value,
  onDone,
}: CashflowAccountsSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tr } = useTranslation('reports');
  const [draft, setDraft] = useState(() => draftOf(value, accounts));

  // Every opening starts from the report's accounts
  const [openedFor, setOpenedFor] = useState(open);
  if (open !== openedFor) {
    setOpenedFor(open);
    if (open) setDraft(draftOf(value, accounts));
  }

  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t('cashflow.accounts.title')}
      description={tr('readiness.accountScopeHelp')}
      footer={
        <Button
          className="h-auto min-h-12 whitespace-normal py-2 text-center flex-1 rounded-[10px] text-m-row"
          disabled={draft.size === 0}
          onClick={() => onDone([...draft])}
        >
          {t('cashflow.accounts.done')}
        </Button>
      }
    >
      <Button
        variant="ghost"
        className="relative mb-1 h-auto min-h-[34px] rounded-[9px] px-3 py-1 text-m-sub font-semibold after:absolute after:inset-x-0 after:-inset-y-[5px] after:content-['']"
        onClick={() => setDraft(new Set(accounts.map((account) => account.id)))}
      >
        {tr('readiness.allAccounts')}
      </Button>
      <ul className="m-0 list-none p-0">
        {accounts.map((account) => (
          <li key={account.id} className="border-t border-line-soft first:border-t-0">
            <label className="flex min-h-[52px] items-center gap-3 px-1 py-2">
              <Checkbox
                checked={draft.has(account.id)}
                onCheckedChange={(next) =>
                  setDraft((current) => {
                    const ids = new Set(current);
                    if (next === true) ids.add(account.id);
                    else ids.delete(account.id);
                    return ids;
                  })
                }
              />
              <span className="min-w-0">
                <span className="block truncate text-m-row text-ink">{account.name}</span>
                {account.institution && (
                  <span className="block truncate text-m-sub text-ink-4">
                    {account.institution.name}
                  </span>
                )}
              </span>
            </label>
          </li>
        ))}
      </ul>
    </Sheet>
  );
}
