import { useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react';
import type { StockReportCostMethod, StockReportMatch, StockReportSale } from '@shared/schema';
import { useFormat } from '@/lib/use-format';
import { useReportMoney } from '@/lib/use-report-money';
import { cn } from '@/lib/utils';
import {
  holdingSpan,
  totalOf,
  type SaleClass,
  type SaleView,
} from '@/utils/stock-transactions-report';
import { ChangeBadge } from '@/components/common/ChangeBadge';
import { Badge } from '@/components/ui/badge';
import { ExplainText, ExplainTip } from './ExplainTip';
import {
  BODY_ROW,
  EntityCell,
  HEAD_ROW,
  NUM_CELL,
  SubLine,
  TableEmpty,
} from './transactions-table-parts';

/** Prototype grid: tighter columns under 1240 px so nothing overflows at the 1080 px minimum. */
const COLUMNS_TEST =
  'grid-cols-[18px_78px_minmax(0,1fr)_112px_112px_112px_124px] gap-x-2.5 max-[1239px]:grid-cols-[16px_78px_minmax(0,1fr)_94px_104px_98px_110px] max-[1239px]:gap-x-2';
const COLUMNS_PLAIN =
  'grid-cols-[18px_78px_minmax(0,1fr)_112px_112px_112px_84px] gap-x-2.5 max-[1239px]:grid-cols-[16px_78px_minmax(0,1fr)_94px_104px_98px_84px] max-[1239px]:gap-x-2';

type TestKind = 'taxable' | 'exempt' | 'mixed';

const toNumber = (text: string) => Number(text);

/**
 * "Výsledek prodejů" (spec D13): one row per sale, newest first, with its proceeds, Výdaje
 * (acquisition cost plus fees), result after fees and, with the holding test on, its class; with
 * the test off a plain holding-time column. A row is a button that unfolds the purchases the sale
 * consists of (on `well`). A sale that could not be converted (D15) shows "—" in its amount
 * cells; a sale with shares sold beyond the recorded buys carries a note (D16). The last row
 * totals what the filters leave.
 */
export function SaleResultsTable({
  views,
  saleClass,
  testOn,
  costMethod,
  currency,
}: {
  views: SaleView[];
  saleClass: SaleClass;
  testOn: boolean;
  costMethod: StockReportCostMethod;
  currency: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set());
  const columns = testOn ? COLUMNS_TEST : COLUMNS_PLAIN;

  const money = (value: number) => fmt.money(value, currency);
  const signed = (value: number) => fmt.money(value, currency, { signed: true });
  const total = totalOf(views);

  const toggle = (id: string) =>
    setOpen((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  if (views.length === 0) {
    return <TableEmpty title={t('sales.emptyTitle')} body={t('sales.emptyBody')} />;
  }

  return (
    <div>
      <div className={cn(HEAD_ROW, columns)}>
        <span />
        <span>{t('sales.colSold')}</span>
        <span>{t('sales.colTitle')}</span>
        <span className="text-right">{t('sales.colProceeds')}</span>
        <span className="flex items-center justify-end gap-1">
          {t('sales.colCosts')}
          <ExplainTip label={t('sales.costsTipLabel')}>
            <ExplainText>{t('sales.costsTip')}</ExplainText>
          </ExplainTip>
        </span>
        <span className="text-right">{t('sales.colResult')}</span>
        <span className="text-right">{t(testOn ? 'sales.colTest' : 'sales.colHeld')}</span>
      </div>
      {views.map((view) => (
        <SaleRows
          key={view.sale.id}
          view={view}
          saleClass={saleClass}
          testOn={testOn}
          costMethod={costMethod}
          currency={currency}
          columns={columns}
          open={open.has(view.sale.id)}
          onToggle={() => toggle(view.sale.id)}
        />
      ))}
      <div className={cn(BODY_ROW, columns, 'border-b-0 bg-well font-650 text-ink')}>
        <span />
        <span>{t('summary.total')}</span>
        <span className="min-w-0 truncate">{t('stats.sales', { count: views.length })}</span>
        <span className={NUM_CELL}>{money(total.proceeds)}</span>
        <span className={NUM_CELL}>{money(total.cost + total.fees)}</span>
        <span className={cn(NUM_CELL, gainClass(total.resultAfterFees))}>
          {signed(total.resultAfterFees)}
        </span>
        <span />
      </div>
    </div>
  );
}

function gainClass(value: number): string | false {
  return value > 0 ? 'text-gain' : value < 0 ? 'text-loss' : false;
}

/** "4 r 10 měs"; days only under one month. */
function useHeld() {
  const { t } = useTranslation('stockTransactions');
  return (buyDay: number, sellDay: number): string => {
    const { years, months, days } = holdingSpan(buyDay, sellDay);
    if (years === 0 && months === 0) return t('held.days', { count: days });
    const parts: string[] = [];
    if (years > 0) parts.push(t('held.years', { count: years }));
    if (months > 0) parts.push(t('held.months', { count: months }));
    return parts.join(' ');
  };
}

function TestBadge({ kind }: { kind: TestKind }) {
  const { t } = useTranslation('stockTransactions');
  if (kind === 'exempt') return <Badge variant="outline">{t('sales.badgeExempt')}</Badge>;
  return <Badge>{t(kind === 'mixed' ? 'sales.badgeMixed' : 'sales.badgeTaxable')}</Badge>;
}

function SaleRows({
  view,
  saleClass,
  testOn,
  costMethod,
  currency,
  columns,
  open,
  onToggle,
}: {
  view: SaleView;
  saleClass: SaleClass;
  testOn: boolean;
  costMethod: StockReportCostMethod;
  currency: string;
  columns: string;
  open: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { nativeMoney } = useReportMoney(currency);
  const held = useHeld();
  const { sale, matches } = view;

  const quantityText = (value: number) => fmt.number(value, { maximumFractionDigits: 6 });
  const money = (value: number) => fmt.money(value, currency);
  const signed = (value: number) => fmt.money(value, currency, { signed: true });

  // The pieces the class filter keeps, whether or not their amounts converted
  const shown = matches.reduce((sum, m) => sum + toNumber(m.quantity), 0);
  const whole = toNumber(sale.quantity);
  const partial = saleClass !== 'all' && matches.length < sale.matches.length;
  const sub = `${sale.ticker} · ${
    partial
      ? t('sales.partOf', { count: whole, part: quantityText(shown), whole: quantityText(whole) })
      : t('quantity', { count: whole, value: quantityText(whole) })
  }`;
  const uncovered = toNumber(sale.uncoveredQuantity);
  const note =
    uncovered > 0
      ? t('sales.uncovered', { count: uncovered, value: quantityText(uncovered) })
      : undefined;

  // Holding time of the sale: one span, or "shortest až longest" when the purchases differ
  const buyDays = matches.map((m) => m.buyDay);
  let heldText = '—';
  if (buyDays.length > 0) {
    const shortest = held(Math.max(...buyDays), sale.day);
    const longest = held(Math.min(...buyDays), sale.day);
    heldText =
      shortest === longest ? shortest : t('sales.heldRange', { from: shortest, to: longest });
  }

  const outlay = view.cost + view.fees;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onToggle();
    }
  };

  return (
    <>
      <div
        role="button"
        tabIndex={0}
        aria-expanded={open}
        onClick={onToggle}
        onKeyDown={onKeyDown}
        className={cn(
          BODY_ROW,
          columns,
          'cursor-pointer transition-colors duration-fast hover:bg-well focus-visible:bg-well focus-visible:outline-none focus-visible:shadow-[inset_0_0_0_2px_var(--focus-border)]'
        )}
      >
        <ChevronRight
          aria-hidden
          className={cn(
            'size-3.5 text-ink-4 transition-transform duration-fast',
            open && 'rotate-90'
          )}
        />
        <span className="whitespace-nowrap num">{fmt.day(sale.day)}</span>
        <EntityCell ticker={sale.ticker} name={sale.companyName} sub={sub} note={note} />
        <div className={NUM_CELL}>
          <span className="whitespace-nowrap">{sale.counted ? money(view.proceeds) : '—'}</span>
          <SubLine>
            {t('sales.times', {
              quantity: quantityText(shown),
              price: nativeMoney(toNumber(sale.price), sale.currency),
            })}
          </SubLine>
        </div>
        <div className={NUM_CELL}>
          {sale.counted ? (
            <>
              <span className="whitespace-nowrap">{money(outlay)}</span>
              <SubLine>{t('sales.feesPart', { amount: money(view.fees) })}</SubLine>
            </>
          ) : (
            '—'
          )}
        </div>
        <div className={NUM_CELL}>
          {sale.counted ? (
            <>
              <span
                className={cn('block whitespace-nowrap font-650', gainClass(view.resultAfterFees))}
              >
                {signed(view.resultAfterFees)}
              </span>
              {outlay > 0 && (
                <ChangeBadge ratio={view.resultAfterFees / outlay} className="mt-[3px]" />
              )}
            </>
          ) : (
            '—'
          )}
        </div>
        <div className={NUM_CELL}>
          {testOn ? (
            view.kind === null ? (
              '—'
            ) : (
              <>
                <TestBadge kind={view.kind} />
                <SubLine>
                  {view.kind === 'mixed' && sale.counted
                    ? t('sales.exemptPart', {
                        count: shown,
                        exempt: quantityText(view.exemptQuantity),
                        total: quantityText(shown),
                      })
                    : t('sales.heldFor', { span: heldText })}
                </SubLine>
              </>
            )
          ) : (
            heldText
          )}
        </div>
      </div>
      {open &&
        matches.map((match, i) => (
          <MatchRow
            key={`${sale.id}-${i}`}
            match={match}
            sale={sale}
            testOn={testOn}
            costMethod={costMethod}
            currency={currency}
            columns={columns}
            heldText={held(match.buyDay, sale.day)}
          />
        ))}
    </>
  );
}

/** One purchase of an unfolded sale, on `well`, 46 px. */
function MatchRow({
  match,
  sale,
  testOn,
  costMethod,
  currency,
  columns,
  heldText,
}: {
  match: StockReportMatch;
  sale: StockReportSale;
  testOn: boolean;
  costMethod: StockReportCostMethod;
  currency: string;
  columns: string;
  heldText: string;
}) {
  const { t } = useTranslation('stockTransactions');
  const fmt = useFormat();
  const { nativeMoney } = useReportMoney(currency);

  const quantity = toNumber(match.quantity);
  const quantityLabel = t('quantity', {
    count: quantity,
    value: fmt.number(quantity, { maximumFractionDigits: 6 }),
  });
  const rate = (text: string | null) =>
    text === null ? '—' : fmt.number(toNumber(text), { maximumFractionDigits: 3 });
  const money = (text: string | null) =>
    text === null ? '—' : fmt.money(toNumber(text), currency);

  const outlay =
    match.cost === null || match.fees === null ? null : toNumber(match.cost) + toNumber(match.fees);
  const result = match.resultAfterFees === null ? null : toNumber(match.resultAfterFees);

  let what: string;
  let whatSub: string | null;
  if (costMethod === 'fifo') {
    what = t('sales.times', {
      quantity: quantityLabel,
      price: nativeMoney(toNumber(match.buyPrice), match.buyCurrency),
    });
    whatSub =
      match.buyCurrency === currency
        ? t('table.noConversion')
        : t('sales.rates', { buy: rate(match.buyRate), sell: rate(sale.rate) });
  } else {
    what = quantityLabel;
    whatSub =
      match.averageCost === null
        ? null
        : t('sales.average', {
            price: fmt.money(toNumber(match.averageCost), currency, { decimals: 2 }),
          });
  }

  return (
    <div className={cn(BODY_ROW, columns, 'min-h-[46px] bg-well text-ink-3')}>
      <span />
      <span className="whitespace-nowrap num">
        {fmt.day(match.buyDay)}
        <SubLine>{t('sales.purchase')}</SubLine>
      </span>
      <div className="min-w-0 text-ink-2">
        <span className="block truncate num" title={what}>
          {what}
        </span>
        {whatSub && <SubLine>{whatSub}</SubLine>}
      </div>
      <span className={cn(NUM_CELL, 'whitespace-nowrap')}>{money(match.proceeds)}</span>
      <div className={NUM_CELL}>
        {outlay === null ? (
          '—'
        ) : (
          <>
            <span className="whitespace-nowrap">{fmt.money(outlay, currency)}</span>
            <SubLine>{t('sales.feesPart', { amount: money(match.fees) })}</SubLine>
          </>
        )}
      </div>
      <span
        className={cn(NUM_CELL, 'whitespace-nowrap font-650', result !== null && gainClass(result))}
      >
        {result === null ? '—' : fmt.money(result, currency, { signed: true })}
      </span>
      <div className={NUM_CELL}>
        {testOn && match.exempt !== null ? (
          <>
            <TestBadge kind={match.exempt ? 'exempt' : 'taxable'} />
            <SubLine>{t('sales.heldFor', { span: heldText })}</SubLine>
          </>
        ) : (
          heldText
        )}
      </div>
    </div>
  );
}
