import { useId, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'wouter';
import { Download, House, Landmark, Tag, Upload, type LucideIcon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { useUpkeep } from '@/mobile/hooks/use-upkeep';
import type { UpkeepKind, UpkeepRow } from '@/mobile/lib/upkeep';
import { routes } from '@/mobile/shell/routes';
import { RecordBalancesSheet } from '@/mobile/sheets/RecordBalancesSheet';

const ICONS: Record<UpkeepKind, LucideIcon> = {
  balances_stale: Landmark,
  uncategorized: Tag,
  statement_due: Upload,
  backup_stale: Download,
  valuation_stale: House,
};

const VERBS: Record<UpkeepKind, string> = {
  balances_stale: 'upkeep.actions.record',
  uncategorized: 'upkeep.actions.categorize',
  statement_due: 'upkeep.actions.import',
  backup_stale: 'upkeep.actions.backup',
  valuation_stale: 'upkeep.actions.revalue',
};

/**
 * Where a verb leads. Zařadit, Importovat and Zálohovat arrive with their flows (Phases 2, 5 and
 * 4); until then they open the tab that will hold them.
 */
const TARGETS: Record<Exclude<UpkeepKind, 'balances_stale'>, string> = {
  uncategorized: routes.finance,
  statement_due: routes.finance,
  backup_stale: routes.settings,
  valuation_stale: routes.wealth,
};

/**
 * Údržba (spec §8.3 item 1, prototype frame 02 and note 2): an action card on `well`, first on
 * Přehled, present only while something is due. One row per item — a 34 pt icon, the title, a
 * sub-line and the verb as a small button (the first row's is primary, the rest secondary). The
 * card leaves when the last row is done; the record-balances sheet it opens is kept apart from it,
 * because saving removes the row that opened the sheet while the sheet is still sliding away.
 */
export function UpkeepCard() {
  const { t } = useTranslation('mobile');
  const { rows, accounts, lastImportByAccount } = useUpkeep();
  // The accounts are frozen at opening: saving removes them from the card while the sheet closes.
  const [sheet, setSheet] = useState<{ open: boolean; accountIds: string[] }>({
    open: false,
    accountIds: [],
  });

  return (
    <>
      {rows.length > 0 && (
        <section
          aria-label={t('upkeep.title')}
          className="relative mb-3.5 rounded-r4 border border-line bg-well px-4 py-3.5"
        >
          <div className="mb-1 flex items-baseline justify-between gap-3">
            <h2 className="m-0 min-w-0 truncate text-m-section text-ink">{t('upkeep.title')}</h2>
            <span className="flex-none whitespace-nowrap text-m-sub font-semibold text-ink-3">
              {t('upkeep.count', { count: rows.length })}
            </span>
          </div>
          <ul className="m-0 list-none p-0">
            {rows.map((row, index) => (
              <UpkeepItem
                key={row.id}
                row={row}
                primary={index === 0}
                accounts={accounts}
                lastImportByAccount={lastImportByAccount}
                onRecord={() => setSheet({ open: true, accountIds: row.accountIds ?? [] })}
              />
            ))}
          </ul>
        </section>
      )}
      <RecordBalancesSheet
        open={sheet.open}
        onOpenChange={(open) => setSheet((current) => ({ ...current, open }))}
        accountIds={sheet.accountIds}
      />
    </>
  );
}

interface UpkeepItemProps {
  row: UpkeepRow;
  primary: boolean;
  accounts: ReturnType<typeof useUpkeep>['accounts'];
  lastImportByAccount: Record<string, number | null>;
  onRecord: () => void;
}

/** One row of the card: icon, title and sub-line, and the verb. */
function UpkeepItem({ row, primary, accounts, lastImportByAccount, onRecord }: UpkeepItemProps) {
  const { t } = useTranslation('mobile');
  const fmt = useFormat();
  const titleId = useId();
  const Icon = ICONS[row.kind];
  const byId = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts]);

  // "10. 10.", with the year when it is not this one
  const shortDay = (unixSeconds: number) =>
    fmt.day(unixSeconds, {
      day: 'numeric',
      month: 'numeric',
      year:
        new Date(unixSeconds * 1000).getUTCFullYear() !== new Date().getUTCFullYear()
          ? 'numeric'
          : undefined,
    });

  const names = (row.accountIds ?? []).flatMap((id) => byId.get(id)?.name ?? []);
  let sub: string | null = null;
  switch (row.kind) {
    case 'balances_stale':
      sub = names.join(' · ');
      break;
    case 'uncategorized': {
      const ids = row.accountIds ?? [];
      if (ids.length === 1) {
        const last = lastImportByAccount[ids[0]];
        sub =
          last != null
            ? t('upkeep.uncategorizedFrom', { account: names[0] ?? '', date: shortDay(last) })
            : (names[0] ?? null);
      } else if (ids.length > 1) {
        sub = t('upkeep.accounts', { count: ids.length });
      }
      break;
    }
    case 'statement_due':
      sub =
        row.sinceDay !== undefined
          ? t('upkeep.lastImport', { date: shortDay(row.sinceDay) })
          : null;
      break;
    default:
      sub = null;
  }

  const verb = t(VERBS[row.kind]);
  const buttonClass = cn(
    'relative h-[34px] rounded-[9px] px-[11px] text-m-sub font-semibold after:absolute after:inset-x-0 after:-inset-y-[5px] after:content-[""]'
  );
  const variant = primary ? 'default' : 'secondary';

  let action: ReactNode;
  if (row.kind === 'balances_stale') {
    action = (
      <Button
        variant={variant}
        className={buttonClass}
        aria-describedby={titleId}
        onClick={onRecord}
      >
        {verb}
      </Button>
    );
  } else {
    action = (
      <Button asChild variant={variant} className={buttonClass}>
        <Link href={TARGETS[row.kind]} aria-describedby={titleId}>
          {verb}
        </Link>
      </Button>
    );
  }

  return (
    <li className="grid grid-cols-[34px_minmax(0,1fr)_auto] items-center gap-3 border-t border-line-soft py-2.5 first:border-t-0">
      <span
        aria-hidden
        className="flex size-[34px] items-center justify-center rounded-[10px] bg-well text-ink-2 [&_svg]:size-[17px]"
      >
        <Icon strokeWidth={1.75} />
      </span>
      <div className="min-w-0">
        <div id={titleId} className="text-[14px] font-semibold tracking-[-0.01em] text-ink">
          {t(row.title.key, row.title.params)}
        </div>
        {sub && <div className="mt-px truncate text-m-sub text-ink-4">{sub}</div>}
      </div>
      {action}
    </li>
  );
}
