import { useId, useMemo, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import {
  ChevronDown,
  Clock,
  Download,
  EyeOff,
  FileCheck,
  House,
  Landmark,
  Percent,
  RefreshCw,
  Tag,
  Upload,
  type LucideIcon,
} from 'lucide-react';
import { MilestoneMenuItems } from '@/components/milestones/MilestoneRows';

import { useMilestoneActions } from '@/hooks/use-milestone-mutations';
import { useMilestoneSubLine } from '@/hooks/use-milestone-sub-line';
import { useFormat } from '@/lib/use-format';
import { cn } from '@/lib/utils';
import { utcDayFloor } from '@/utils/chart-axis';
import { useImportSheet } from '@/mobile/hooks/use-import-sheet';
import { useRemembered } from '@/mobile/hooks/use-remembered';
import { useUpkeep } from '@/mobile/hooks/use-upkeep';
import { useUpkeepDismissals } from '@/mobile/hooks/use-upkeep-dismissals';
import { queuePath } from '@/mobile/lib/categorize-queue';
import { shortDayOptions } from '@/mobile/lib/format-day';
import { loanEventPath } from '@/mobile/lib/loan-event-param';
import type { UpkeepKind, UpkeepRow } from '@/mobile/lib/upkeep';
import { dismissKey, hideRow, restoreRow, snoozeRow } from '@/mobile/lib/upkeep-dismissals';
import { propertyPath, routes, settingsPath } from '@/mobile/shell/routes';
import { RecordBalancesSheet } from '@/mobile/sheets/RecordBalancesSheet';
import { EntityRow, RowChip } from './EntityRow';
import { ROW_MENU_ITEM, RowMenuItem } from './RowMenu';
import { SubFacts } from './SubFacts';

const ICONS: Record<UpkeepKind, LucideIcon> = {
  balances_stale: Landmark,
  uncategorized: Tag,
  statement_due: Upload,
  backup_stale: Download,
  sync_stalled: RefreshCw,
  sync_attachments: RefreshCw,
  valuation_stale: House,
  // The desktop's upkeep icons (`MilestoneRows`)
  loan_balance_check: FileCheck,
  loan_fixation_expired: Percent,
};

const VERBS: Record<UpkeepKind, string> = {
  balances_stale: 'upkeep.actions.record',
  uncategorized: 'upkeep.actions.categorize',
  statement_due: 'upkeep.actions.import',
  backup_stale: 'upkeep.actions.backup',
  sync_stalled: 'upkeep.actions.open',
  sync_attachments: 'upkeep.actions.open',
  valuation_stale: 'upkeep.actions.revalue',
  loan_balance_check: 'upkeep.actions.enterBalance',
  loan_fixation_expired: 'upkeep.actions.enterRate',
};

/**
 * Where a verb that leads to a screen leads. Zálohovat opens Záloha a přenos, Přecenit the
 * property (its detail has the Přecenit button; the Majetek tab, which lists the properties, for a
 * row that does not name one), the loan verbs the loan with its event dialog open (Finance for a
 * row without a loan); Zařadit opens the categorize queue (`queuePath`: of one account when
 * exactly one holds the payments). Each screen's back button then names Přehled (`backState`), as
 * back returns here. Zaznamenat and Importovat open sheets instead (`UpkeepItem`).
 */
function targetOf(row: UpkeepRow): string {
  switch (row.kind) {
    case 'uncategorized':
      return queuePath(row.accountIds ?? []);
    case 'valuation_stale':
      return row.sourceId ? propertyPath(row.sourceId) : routes.wealth;
    case 'loan_balance_check':
      return row.sourceId ? loanEventPath(row.sourceId, 'balance_check') : routes.finance;
    case 'loan_fixation_expired':
      return row.sourceId ? loanEventPath(row.sourceId, 'rate_change') : routes.finance;
    case 'sync_stalled':
    case 'sync_attachments':
      return settingsPath('sync');
    default:
      // `backup_stale`: the other two kinds open a sheet and never ask
      return settingsPath('backup');
  }
}

/**
 * Údržba (spec §8.3 item 1, prototype frame 02, audit 2026-11-15 D3 and D4): an action card on
 * `well`, right under the net-worth hero on Přehled, present only while something is due. Unfolded
 * at first; the head (a button: title, count, chevron) folds it to one line naming what is due.
 * Unfolded it has one entity row per item (`UpkeepItem`): the whole row is the tap target of its
 * verb, with a one-line title, a short sub-line and a chevron; hiding and reminding later are the
 * row's swipes and its long press menu — the desktop's milestone menu on a row that stands for a
 * desktop milestone, Skrýt and Připomenout za měsíc kept on this device on the phone's own statement
 * and uncategorized rows. Folded or not is kept for the app session. The card leaves when the last
 * row is done; the record-balances sheet it opens is kept apart from it, because saving removes the
 * row that opened the sheet while the sheet is still sliding away.
 */
export function UpkeepCard() {
  const { t } = useTranslation(['mobile', 'milestones', 'deviceSync']);
  const { rows, accounts, lastImportByAccount } = useUpkeep();
  const { openImport } = useImportSheet();
  // Unfolded at first (C11): every row has a verb, and folded each was a tap further away. The
  // person's fold is kept for the app session
  const [open, setOpen] = useRemembered('overview.upkeepOpen', true);
  const listId = useId();
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
          className="relative mb-3.5 overflow-hidden rounded-r4 border border-line bg-well px-3.5 py-3.5"
        >
          <h2 className="m-0 text-m-section text-ink">
            <button
              type="button"
              aria-expanded={open}
              aria-controls={listId}
              onClick={() => setOpen(!open)}
              className="relative flex w-full items-baseline justify-between gap-3 text-left after:absolute after:-inset-y-3.5 after:inset-x-0 after:content-['']"
            >
              <span className="min-w-0 break-words">{t('upkeep.title')}</span>
              <span className="flex flex-none items-center gap-1 self-center whitespace-nowrap text-m-sub font-semibold text-ink-3">
                {t('upkeep.count', { count: rows.length })}
                <ChevronDown
                  aria-hidden
                  strokeWidth={1.75}
                  className={cn(
                    'size-[18px] transition-transform duration-base ease-moony motion-reduce:transition-none',
                    open && 'rotate-180'
                  )}
                />
              </span>
            </button>
          </h2>
          {!open && (
            <p className="m-0 mt-1 break-words text-m-sub text-ink-3">
              {rows.map((row) => t(row.title.key, row.title.params)).join(' · ')}
            </p>
          )}
          {/* Full-bleed rows: a swipe uncovers its panel to the card's edges */}
          <ul id={listId} hidden={!open} className="-mx-3.5 -mb-3.5 mt-2 list-none p-0">
            {rows.map((row) => (
              <UpkeepItem
                key={row.id}
                row={row}
                accounts={accounts}
                lastImportByAccount={lastImportByAccount}
                onRecord={() => setSheet({ open: true, accountIds: row.accountIds ?? [] })}
                onImport={() =>
                  openImport({
                    accountId: row.accountIds?.[0],
                    source: 'upkeep',
                    from: t('tabs.overview'),
                  })
                }
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
  accounts: ReturnType<typeof useUpkeep>['accounts'];
  lastImportByAccount: Record<string, number | null>;
  onRecord: () => void;
  onImport: () => void;
}

/**
 * One row of the card (audit D3): the whole row is the verb's tap target — an icon, a title on one
 * line, a short sub-line (a count, a date, never a list of names) and a chevron, so every row ends
 * on the same edge whatever the verb is called in the language. Odložit and Skrýt are the swipes
 * (right: remind in a month, left: hide) and the long press's menu, the gestures of every row.
 */
function UpkeepItem({ row, accounts, lastImportByAccount, onRecord, onImport }: UpkeepItemProps) {
  const { t } = useTranslation(['mobile', 'milestones', 'deviceSync']);
  const fmt = useFormat();
  const subLine = useMilestoneSubLine();
  const milestoneActions = useMilestoneActions();
  const { setDismissals } = useUpkeepDismissals();
  const Icon = ICONS[row.kind];
  const byId = useMemo(() => new Map(accounts.map((a) => [a.id, a])), [accounts]);

  const shortDay = (unixSeconds: number) => fmt.day(unixSeconds, shortDayOptions(unixSeconds));

  const names = (row.accountIds ?? []).flatMap((id) => byId.get(id)?.name ?? []);
  let sub: ReactNode = null;
  switch (row.kind) {
    case 'balances_stale':
      sub = t('upkeep.olderThanMonth');
      break;
    case 'uncategorized': {
      const ids = row.accountIds ?? [];
      if (ids.length === 1) {
        const last = lastImportByAccount[ids[0]];
        sub = (
          <SubFacts
            wrap
            lead={names[0]}
            tail={last != null ? t('upkeep.fromStatement', { date: shortDay(last) }) : null}
          />
        );
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
    case 'loan_balance_check':
      sub = row.milestone?.sinceDay
        ? t('upkeep.loanBalanceFrom', { date: shortDay(row.milestone.sinceDay) })
        : null;
      break;
    case 'loan_fixation_expired':
      sub = row.milestone?.dueDay
        ? t('upkeep.loanFixationEnded', { date: shortDay(row.milestone.dueDay) })
        : null;
      break;
    case 'sync_stalled':
      sub = row.milestone ? subLine(row.milestone) : null;
      break;
    default:
      // The row's own second line (the backup row with sync on)
      sub = row.sub ? t(row.sub.key, row.sub.params) : null;
  }

  // A loan row names the loan and says what is due under it; the folded line keeps the short title
  const title =
    (row.kind === 'loan_balance_check' || row.kind === 'loan_fixation_expired') &&
    row.milestone?.title
      ? row.milestone.title
      : t(row.title.key, row.title.params);
  const verb = t(VERBS[row.kind]);

  // Zaznamenat and Importovat open a sheet; the other verbs go to a screen
  const opensSheet = row.kind === 'balances_stale' || row.kind === 'statement_due';
  const target = opensSheet
    ? { onClick: row.kind === 'balances_stale' ? onRecord : onImport }
    : { href: targetOf(row), from: t('tabs.overview') };

  // Odložit and Skrýt: the desktop milestone's own state, or the phone's dismissal kept here
  const { milestone } = row;
  const phoneDismiss = !milestone && dismissKey(row) !== null;
  const undo = {
    label: t('milestones:actions.undo'),
    onClick: () => setDismissals((d) => restoreRow(d, row)),
  };
  const hide = milestone
    ? () => milestoneActions.hide(milestone)
    : phoneDismiss
      ? () => {
          setDismissals((d) => hideRow(d, row));
          toast(t('milestones:toast.hidden'), { action: undo });
        }
      : undefined;
  const remind = milestone
    ? () => milestoneActions.remindLater(milestone)
    : phoneDismiss
      ? () => {
          setDismissals((d) => snoozeRow(d, row, utcDayFloor(Date.now() / 1000)));
          toast(t('milestones:toast.snoozedMonth'), { action: undo });
        }
      : undefined;

  return (
    <EntityRow
      surface="well"
      chip={
        <RowChip square>
          <Icon strokeWidth={1.75} />
        </RowChip>
      }
      title={
        <>
          {title}
          {/* What the tap does, for VoiceOver: the row is the verb's button */}
          <span className="sr-only">, {verb}</span>
        </>
      }
      sub={sub ?? undefined}
      wrap
      chevron
      {...target}
      onSwipeRight={remind}
      swipeRightLabel={t('milestones:actions.remindMonth')}
      swipeRightIcon={Clock}
      onSwipeLeft={hide}
      swipeLeftLabel={t('milestones:actions.hide')}
      swipeLeftIcon={EyeOff}
      menu={
        milestone
          ? {
              label: t('milestones:card.rowMenu', { title }),
              children: (
                <MilestoneMenuItems
                  m={milestone}
                  actions={milestoneActions}
                  itemClassName={ROW_MENU_ITEM}
                />
              ),
            }
          : hide && remind
            ? {
                label: t('milestones:card.rowMenu', { title }),
                children: (
                  <>
                    <RowMenuItem onSelect={hide}>
                      <EyeOff />
                      {t('milestones:actions.hide')}
                    </RowMenuItem>
                    <RowMenuItem onSelect={remind}>
                      <Clock />
                      {t('milestones:actions.remindMonth')}
                    </RowMenuItem>
                  </>
                ),
              }
            : undefined
      }
    />
  );
}
