import { useTranslation } from 'react-i18next';
import { Landmark, Plus, RefreshCw } from 'lucide-react';
import type { BankAccountWithInstitution, DateOrder, StatementInspection } from '@shared/schema';
import type { Unreadable } from '@/mobile/hooks/use-import-flow';
import { FileBar } from '@/mobile/components/import/FileBar';
import { ImportNote } from '@/mobile/components/import/ImportNote';
import { NeutralChip, PickGroup, PickRow } from '@/mobile/components/PickRow';

const DATE_ORDERS: readonly DateOrder[] = ['dmy', 'mdy', 'ymd'];

/** What the sheet can read, in the one line every stage that turns a file away repeats. */
function ReadsLine() {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  return (
    <p className="mx-1 mb-0 mt-2 text-m-sub text-ink-3">
      {t('import.reads', { formats: tb('statementImport.supportedFormats') })}
    </p>
  );
}

/** No file yet: one sentence and what can be read; "Vybrat soubor" is the footer's primary. */
export function PickBody() {
  const { t } = useTranslation('mobile');
  return (
    <div className="pt-1">
      <p className="mx-1 my-0 text-m-body text-ink-2">{t('import.pick.lead')}</p>
      <ReadsLine />
    </div>
  );
}

/** The file is being read: the file bar and a line, never an empty sheet. */
export function ReadingBody({ fileName }: { fileName: string }) {
  const { t } = useTranslation('mobile');
  return (
    <div aria-live="polite">
      <FileBar name={fileName} />
      <p className="mx-1 my-0 flex items-center gap-2 text-m-sub text-ink-3">
        <RefreshCw aria-hidden className="size-3.5 animate-spin motion-reduce:animate-none" />
        {t('import.reading')}
      </p>
    </div>
  );
}

/**
 * A file the sheet cannot read says so plainly (the reason, what it can read) and never shows a
 * raw error: a read error is translated before it gets here. "Vybrat jiný soubor" is the footer's
 * primary.
 */
export function UnreadableBody({
  fileName,
  unreadable,
}: {
  fileName: string;
  unreadable: Unreadable;
}) {
  const { t } = useTranslation('mobile');
  return (
    <div role="alert">
      <FileBar name={fileName} />
      <p className="mx-1 my-0 text-m-body text-ink-2">
        {t(`import.unreadable.${unreadable.reason}`)}
      </p>
      {unreadable.detail && (
        <p className="mx-1 mb-0 mt-1.5 text-m-sub text-ink-3">{unreadable.detail}</p>
      )}
      <ReadsLine />
    </div>
  );
}

/**
 * Which account the payments go into, when the file does not say: the open accounts as radio rows
 * (a tap chooses and goes on) and "Nový účet…" (the desktop's account form). Without any open
 * account that is the only row.
 */
export function AccountBody({
  fileName,
  accounts,
  chosen,
  onChoose,
  onNew,
}: {
  fileName: string;
  accounts: readonly BankAccountWithInstitution[];
  chosen: string | null;
  onChoose: (accountId: string) => void;
  onNew: () => void;
}) {
  const { t } = useTranslation('mobile');
  return (
    <>
      <FileBar name={fileName} />
      {accounts.length === 0 ? (
        <p className="mx-1 mb-2 mt-0 text-m-body text-ink-2">{t('import.account.none')}</p>
      ) : (
        <PickGroup title={t('import.account.title')}>
          {accounts.map((account) => (
            <PickRow
              key={account.id}
              icon={<NeutralChip icon={<Landmark />} />}
              label={account.name}
              sub={[account.institution?.name, account.currency].filter(Boolean).join(' · ')}
              checked={account.id === chosen}
              onSelect={() => onChoose(account.id)}
            />
          ))}
        </PickGroup>
      )}
      <PickRow
        icon={<NeutralChip icon={<Plus />} />}
        label={t('import.account.new')}
        onSelect={onNew}
      />
    </>
  );
}

/**
 * A statement file asks before its preview: which of its statements (one account each), and — for
 * a QIF file whose dates read both ways — how they are written. Both are radio rows; "Pokračovat"
 * (the footer's primary) confirms them, so a date order is never taken without being seen.
 */
export function StatementBody({
  fileName,
  inspection,
  statementIndex,
  onStatementChange,
  dateOrder,
  onDateOrderChange,
}: {
  fileName: string;
  inspection: StatementInspection;
  statementIndex: number;
  onStatementChange: (index: number) => void;
  dateOrder: DateOrder | null;
  onDateOrderChange: (order: DateOrder) => void;
}) {
  const { t: tb } = useTranslation('bank_accounts');
  return (
    <>
      <FileBar name={fileName} meta={tb(`statementImport.format.${inspection.format}`)} />
      {inspection.statements.length > 1 && (
        <PickGroup title={tb('statementImport.pickStatement')}>
          {inspection.statements.map((statement) => (
            <PickRow
              key={statement.index}
              icon={<NeutralChip icon={<Landmark />} />}
              label={statement.accountIdentifier ?? statement.accountName ?? '—'}
              sub={[
                statement.currency,
                tb('statementImport.entries', { count: statement.entryCount }),
              ]
                .filter(Boolean)
                .join(' · ')}
              checked={statement.index === statementIndex}
              onSelect={() => onStatementChange(statement.index)}
            />
          ))}
        </PickGroup>
      )}
      {dateOrder !== null && (
        <>
          <PickGroup title={tb('statementImport.dateOrder.label')}>
            {DATE_ORDERS.map((order) => (
              <PickRow
                key={order}
                label={tb(`statementImport.dateOrder.${order}`)}
                checked={order === dateOrder}
                onSelect={() => onDateOrderChange(order)}
              />
            ))}
          </PickGroup>
          <ImportNote>{tb('statementImport.dateOrder.hint')}</ImportNote>
        </>
      )}
    </>
  );
}
