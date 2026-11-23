import type {
  StockReportBucket,
  StockReportLedgerRow,
  StockReportMatch,
  StockReportOpenLot,
  StockReportSale,
} from '@shared/schema';

export type SaleClass = 'all' | 'taxable' | 'exempt';
export type LedgerKind = 'all' | 'buy' | 'sell' | 'dividend';

export interface SaleView {
  sale: StockReportSale;
  /** The matches the class filter keeps. */
  matches: StockReportMatch[];
  quantity: number;
  proceeds: number;
  cost: number;
  fees: number;
  resultAfterFees: number;
  /** `null` when the holding test is off (or the sale has no matches). */
  kind: 'taxable' | 'exempt' | 'mixed' | null;
  exemptQuantity: number;
}

const DAY_SECONDS = 86_400;

/** Decimal TEXT from the wire as a number; `null` and unparsable text add nothing. */
function amount(text: string | null): number {
  if (text === null) return 0;
  const value = Number(text);
  return Number.isFinite(value) ? value : 0;
}

function matchesSearch(search: string, ticker: string, companyName: string): boolean {
  const needle = search.trim().toLowerCase();
  if (needle === '') return true;
  return ticker.toLowerCase().includes(needle) || companyName.toLowerCase().includes(needle);
}

function inBucket(day: number, bucket: StockReportBucket | null): boolean {
  return bucket === null || (bucket.fromDay <= day && day <= bucket.toDay);
}

function keepsMatch(match: StockReportMatch, saleClass: SaleClass): boolean {
  if (saleClass === 'all') return true;
  return saleClass === 'exempt' ? match.exempt === true : match.exempt === false;
}

function kindOf(matches: StockReportMatch[]): SaleView['kind'] {
  if (matches.length === 0 || matches.some((m) => m.exempt === null)) return null;
  const exempt = matches.filter((m) => m.exempt === true).length;
  if (exempt === 0) return 'taxable';
  return exempt === matches.length ? 'exempt' : 'mixed';
}

/** The sales of the report after the search, bucket and class filters, with sums of the kept matches. */
export function saleViews(
  sales: StockReportSale[],
  filter: { search: string; bucket: StockReportBucket | null; saleClass: SaleClass }
): SaleView[] {
  const views: SaleView[] = [];
  for (const sale of sales) {
    if (!matchesSearch(filter.search, sale.ticker, sale.companyName)) continue;
    if (!inBucket(sale.day, filter.bucket)) continue;
    const matches = sale.matches.filter((m) => keepsMatch(m, filter.saleClass));
    if (filter.saleClass !== 'all' && matches.length === 0) continue;
    const exemptQuantity = matches
      .filter((m) => m.exempt === true)
      .reduce((sum, m) => sum + amount(m.quantity), 0);
    const view: SaleView = {
      sale,
      matches,
      quantity: 0,
      proceeds: 0,
      cost: 0,
      fees: 0,
      resultAfterFees: 0,
      kind: kindOf(matches),
      exemptQuantity,
    };
    if (sale.counted) {
      for (const m of matches) {
        view.quantity += amount(m.quantity);
        view.proceeds += amount(m.proceeds);
        view.cost += amount(m.cost);
        view.fees += amount(m.fees);
        view.resultAfterFees += amount(m.resultAfterFees);
      }
    }
    views.push(view);
  }
  return views;
}

export function totalOf(views: SaleView[]): {
  proceeds: number;
  cost: number;
  fees: number;
  resultAfterFees: number;
} {
  const total = { proceeds: 0, cost: 0, fees: 0, resultAfterFees: 0 };
  for (const v of views) {
    total.proceeds += v.proceeds;
    total.cost += v.cost;
    total.fees += v.fees;
    total.resultAfterFees += v.resultAfterFees;
  }
  return total;
}

export function ledgerRows(
  rows: StockReportLedgerRow[],
  filter: { search: string; bucket: StockReportBucket | null; kind: LedgerKind }
): StockReportLedgerRow[] {
  return rows.filter(
    (row) =>
      (filter.kind === 'all' || row.kind === filter.kind) &&
      matchesSearch(filter.search, row.ticker, row.companyName) &&
      inBucket(row.day, filter.bucket)
  );
}

/** Both ends included, UTC midnight, unix seconds. */
export function yearPeriod(year: number): { fromDay: number; toDay: number } {
  return {
    fromDay: Date.UTC(year, 0, 1) / 1000,
    toDay: Date.UTC(year, 11, 31) / 1000,
  };
}

/** Whole months between the two days; days only when the span is under one month. */
export function holdingSpan(
  buyDay: number,
  sellDay: number
): { years: number; months: number; days: number } {
  const from = new Date(buyDay * 1000);
  const to = new Date(sellDay * 1000);
  let totalMonths =
    (to.getUTCFullYear() - from.getUTCFullYear()) * 12 + (to.getUTCMonth() - from.getUTCMonth());
  if (to.getUTCDate() < from.getUTCDate()) totalMonths -= 1;
  if (totalMonths < 1) {
    return { years: 0, months: 0, days: Math.max(0, Math.round((sellDay - buyDay) / DAY_SECONDS)) };
  }
  return { years: Math.floor(totalMonths / 12), months: totalMonths % 12, days: 0 };
}

/** A whole number of months 1..600, otherwise `null`. */
export function parseHoldingMonths(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= 1 && value <= 600 ? value : null;
}

/**
 * Open lots split by whether the holding test is already passed, with the value of each group
 * (priced lots only): `waitingValue` + `passedValue` is the value of every valued open lot.
 */
export function openLotGroups(
  lots: StockReportOpenLot[],
  today: number
): {
  waiting: StockReportOpenLot[];
  passed: StockReportOpenLot[];
  waitingValue: number;
  passedValue: number;
} {
  const waiting: StockReportOpenLot[] = [];
  const passed: StockReportOpenLot[] = [];
  let waitingValue = 0;
  let passedValue = 0;
  for (const lot of lots) {
    const value = amount(lot.value);
    if (lot.passDay !== null && lot.passDay <= today) {
      passed.push(lot);
      passedValue += value;
    } else {
      waiting.push(lot);
      waitingValue += value;
    }
  }
  waiting.sort((a, b) => {
    const x = a.passDay ?? Infinity;
    const y = b.passDay ?? Infinity;
    return x === y ? 0 : x < y ? -1 : 1;
  });
  passed.sort((a, b) => (b.passDay ?? 0) - (a.passDay ?? 0));
  return { waiting, passed, waitingValue, passedValue };
}

/** The open lots of one position, with what they add up to. */
export interface OpenLotGroup {
  investmentId: string;
  ticker: string;
  companyName: string;
  lots: StockReportOpenLot[];
  /** Open shares of all its lots. */
  quantity: number;
  /** Sum over the priced lots; null when none of them is priced. */
  value: number | null;
  unrealized: number | null;
}

/**
 * Open lots grouped by position. The groups come in the order of their first lot and keep the
 * order of the lots, so a list sorted by pass day gives the position with the nearest day first.
 */
export function openLotsByTicker(lots: StockReportOpenLot[]): OpenLotGroup[] {
  const groups = new Map<string, OpenLotGroup>();
  for (const lot of lots) {
    let group = groups.get(lot.investmentId);
    if (!group) {
      group = {
        investmentId: lot.investmentId,
        ticker: lot.ticker,
        companyName: lot.companyName,
        lots: [],
        quantity: 0,
        value: null,
        unrealized: null,
      };
      groups.set(lot.investmentId, group);
    }
    group.lots.push(lot);
    group.quantity += amount(lot.quantity);
    if (lot.value !== null) group.value = (group.value ?? 0) + amount(lot.value);
    if (lot.unrealized !== null) {
      group.unrealized = (group.unrealized ?? 0) + amount(lot.unrealized);
    }
  }
  return [...groups.values()];
}
