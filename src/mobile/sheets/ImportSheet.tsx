import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'wouter';
import type { CsvPreviewRow, InsertBankAccount } from '@shared/schema';
import { BankAccountFormDialog } from '@/components/bank-accounts/BankAccountFormDialog';
import { Button } from '@/components/ui/button';
import { useBankAccountMutations } from '@/hooks/use-bank-account-mutations';
import { useCategories } from '@/hooks/use-categories';
import { useFormat } from '@/lib/use-format';
import { CorrectStep } from '@/mobile/components/import/CorrectStep';
import { DoneStep } from '@/mobile/components/import/DoneStep';
import { FOOTER_GHOST, FOOTER_GHOST_WIDE, FOOTER_PRIMARY } from '@/mobile/components/import/styles';
import { PreviewBody } from '@/mobile/components/import/PreviewBody';
import { RecognizeStep } from '@/mobile/components/import/RecognizeStep';
import {
  AccountBody,
  PickBody,
  ReadingBody,
  StatementBody,
  UnreadableBody,
} from '@/mobile/components/import/StageBodies';
import { Sheet } from '@/mobile/components/Sheet';
import { useImportFlow } from '@/mobile/hooks/use-import-flow';
import { leaveAfterSheet } from '@/mobile/hooks/use-leave-after-sheet';
import { useRetained } from '@/mobile/hooks/use-retained';
import type { ImportRequest } from '@/mobile/lib/import-flow';
import { accountPath, routes } from '@/mobile/shell/routes';
import { CategoryChoiceSheet } from '@/mobile/sheets/CategoryChoiceSheet';

interface ImportSheetProps {
  request: ImportRequest;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * The import sheet (spec §8.6, README §13 "Import sheet"): one tall sheet whose stages swap its
 * title, sentence, body and footer — pick a file, read it, which statement and how its dates
 * read, how an unknown CSV is understood ("Takhle tomu rozumím") and the per-field correction of
 * it, which account, the preview with the category each payment would get, the write, and the
 * done step. The flow (`useImportFlow`) walks the file through the desktop's parsers and commands;
 * this component only draws the stage it is in. The footer has one primary at most. A file the
 * sheet cannot read says why and offers another. The category choice and the new-account form
 * stack over it. Closing in any stage (except while the write runs) discards the copy of the file
 * that a share or the picker made.
 */
export function ImportSheet({ request, open, onOpenChange }: ImportSheetProps) {
  const { t } = useTranslation('mobile');
  const { t: tb } = useTranslation('bank_accounts');
  const { t: tc } = useTranslation('common');
  const fmt = useFormat();
  const [, setLocation] = useLocation();
  const flow = useImportFlow(request, open);
  const { categories } = useCategories();
  const { createAccount } = useBankAccountMutations();
  const [pickLine, setPickLine] = useState<number | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const choosing = useRetained(pickLine);

  const { stage } = flow;
  const close = () => {
    // The write is running: closing now would hide what it did
    if (stage === 'importing') return;
    flow.finish();
    onOpenChange(false);
  };

  // Before the account is chosen the recognition only knows the one the entry point named
  const target = flow.target ?? (flow.recognition.csv ? flow.requestedAccount : undefined);
  const accountLine = target
    ? [
        t('import.intoAccount', { account: target.name }),
        target.institution?.name,
        request.source === 'shared' && t('import.fromShare'),
      ]
        .filter(Boolean)
        .join(' · ')
    : request.source === 'shared'
      ? t('import.fromShare')
      : undefined;
  const result = flow.result;

  const recognition = flow.recognition;
  const view = recognition.view;
  let title = t('import.title');
  let description = accountLine;
  switch (stage) {
    case 'correct':
      if (view.kind === 'field') {
        // A date whose day and month cannot be told is the same question the QIF import asks
        const order = view.field === 'date' && recognition.options.some((o) => o.kind === 'order');
        title = order ? t('import.statement.dateTitle') : t(`import.recognize.ask.${view.field}`);
        description = order
          ? tb('statementImport.dateOrder.hint')
          : t(`import.recognize.lead.${view.field}`);
      } else {
        title = t('import.recognize.fixTitle');
        description = t('import.recognize.fixLead');
      }
      break;
    case 'account':
      title = t('import.account.sheetTitle');
      description = flow.openAccounts.length > 0 ? t('import.account.lead') : undefined;
      break;
    case 'statement':
      title =
        (flow.statement?.statements.length ?? 0) > 1
          ? t('import.statement.title')
          : t('import.statement.dateTitle');
      description = undefined;
      break;
    case 'importing':
      description = t('import.importing');
      break;
    case 'done':
      title = tb('csvImport.done.title');
      description = result?.dateRange
        ? tb('csvImport.done.summary', {
            count: result.importedCount,
            from: fmt.day(result.dateRange.from),
            to: fmt.day(result.dateRange.to),
          })
        : tb('csvImport.done.nothing');
      break;
    case 'unreadable':
      title = t('import.unreadable.title');
      description = undefined;
      break;
    default:
      break;
  }

  const cancel = (wide: boolean) => (
    <Button variant="ghost" className={wide ? FOOTER_GHOST_WIDE : FOOTER_GHOST} onClick={close}>
      {tc('buttons.cancel')}
    </Button>
  );
  const primary = (
    label: string,
    onClick: () => void,
    state: { disabled?: boolean; loading?: boolean } = {}
  ) => (
    <Button
      className={FOOTER_PRIMARY}
      onClick={onClick}
      disabled={state.disabled}
      loading={state.loading}
    >
      {label}
    </Button>
  );

  let footer;
  switch (stage) {
    case 'pick':
      footer = (
        <>
          {cancel(false)}
          {primary(t('import.pick.choose'), () => void flow.chooseFile())}
        </>
      );
      break;
    case 'statement':
      footer = (
        <>
          {cancel(false)}
          {primary(t('import.statement.continue'), () => void flow.continueFromStatement())}
        </>
      );
      break;
    case 'preview':
    case 'importing':
      footer = (
        <>
          {cancel(false)}
          {primary(
            flow.counts
              ? tb('csvImport.importButton', { count: flow.counts.willImport })
              : tb('csvImport.importButtonIdle'),
            () => void flow.runImport(),
            {
              disabled: !flow.canImport,
              loading: stage === 'importing' || flow.previewUpdating,
            }
          )}
        </>
      );
      break;
    case 'done':
      footer = (
        <Button variant="ghost" className={FOOTER_GHOST_WIDE} onClick={close}>
          {tb('csvImport.next.done')}
        </Button>
      );
      break;
    case 'unreadable':
      footer = (
        <>
          {cancel(false)}
          {primary(t('import.pick.chooseOther'), () => void flow.chooseFile())}
        </>
      );
      break;
    case 'recognize':
      footer = (
        <>
          {/* Expenses the balance proves positive cannot be imported yet: "Opravit…" moves into the
              red box that says so (the action that resolves it), the person can cancel here, and
              the primary waits disabled — three buttons do not fit one row at 360 */}
          {recognition.signsBlocked ? (
            cancel(false)
          ) : (
            <Button variant="ghost" className={FOOTER_GHOST} onClick={recognition.openFields}>
              {t('import.recognize.fix')}
            </Button>
          )}
          {primary(t('import.recognize.looksRight'), () => void flow.continueFromRecognition(), {
            disabled: recognition.signsBlocked,
          })}
        </>
      );
      break;
    case 'correct':
      footer =
        view.kind === 'field' ? (
          <>
            {/* A question the recognition asked itself has nothing to go back to: the person can only leave */}
            {view.start ? (
              cancel(false)
            ) : (
              <Button variant="ghost" className={FOOTER_GHOST} onClick={recognition.back}>
                {tc('buttons.back')}
              </Button>
            )}
            {primary(t('import.recognize.use'), recognition.apply, {
              disabled: !recognition.canApply,
            })}
          </>
        ) : (
          <Button variant="ghost" className={FOOTER_GHOST_WIDE} onClick={recognition.back}>
            {tc('buttons.back')}
          </Button>
        );
      break;
    default:
      footer = cancel(true);
  }

  const toQueue = (accountId: string) =>
    leaveAfterSheet(close, () => setLocation(`${routes.uncategorized}?account=${accountId}`));
  const toAccount = (accountId: string) =>
    leaveAfterSheet(close, () => setLocation(accountPath(accountId)));

  return (
    <>
      <Sheet
        open={open}
        onOpenChange={(next) => !next && close()}
        title={title}
        description={description}
        footer={footer}
        full
      >
        {stage === 'pick' && <PickBody guide={flow.accountGuide} />}
        {stage === 'reading' && <ReadingBody fileName={flow.fileName} />}
        {stage === 'unreadable' && flow.unreadable && (
          <UnreadableBody
            fileName={flow.fileName}
            unreadable={flow.unreadable}
            guides={flow.guides}
          />
        )}
        {stage === 'account' && (
          <AccountBody
            fileName={flow.fileName}
            accounts={flow.openAccounts}
            chosen={flow.targetId}
            onChoose={(id) => void flow.selectAccount(id)}
            onNew={() => setAddOpen(true)}
          />
        )}
        {stage === 'recognize' && (
          <RecognizeStep fileName={flow.fileName} recognition={recognition} />
        )}
        {stage === 'correct' && <CorrectStep recognition={recognition} />}
        {stage === 'statement' && flow.statement && (
          <StatementBody
            fileName={flow.fileName}
            inspection={flow.statement}
            statementIndex={flow.statementIndex}
            onStatementChange={flow.setStatementIndex}
            dateOrder={flow.statement.dateOrderAmbiguous ? flow.dateOrder.dateOrder : null}
            onDateOrderChange={flow.pickDateOrder}
          />
        )}
        {(stage === 'preview' || stage === 'importing') && (
          <PreviewBody flow={flow} onPick={(row: CsvPreviewRow) => setPickLine(row.line)} />
        )}
        {stage === 'done' && result && flow.importedId && (
          <DoneStep
            result={result}
            accountId={flow.importedId}
            fileName={flow.fileName}
            onQueue={() => toQueue(flow.importedId as string)}
            onShow={() => toAccount(flow.importedId as string)}
          />
        )}
      </Sheet>

      <CategoryChoiceSheet
        open={pickLine !== null}
        onOpenChange={(next) => !next && setPickLine(null)}
        categories={categories}
        value={
          choosing === null
            ? null
            : (flow.overrides.find((o) => o.line === choosing)?.categoryId ?? null)
        }
        onChoose={(categoryId) => choosing !== null && flow.setOverride(choosing, categoryId)}
      />
      {/* The desktop's account form, as Finance's "+" uses it; the new account is chosen at once */}
      <BankAccountFormDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSubmit={(data, zones) =>
          createAccount.mutate(
            { data: data as InsertBankAccount, zones },
            {
              onSuccess: (account) => {
                setAddOpen(false);
                void flow.selectAccount(account.id);
              },
            }
          )
        }
        isLoading={createAccount.isPending}
      />
    </>
  );
}
