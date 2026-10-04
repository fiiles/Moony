import type { StockInvestmentWithPrice } from '@shared/types';
import {
  calculateHoldingMetrics,
  calculatePortfolioTotals,
  findTopPerformer,
  calculateGainLoss,
  calculateGainLossPercent,
} from '@shared/calculations';

export interface HoldingData {
  id: string;
  ticker: string;
  companyName: string;
  quantity: number;
  avgCost: number;
  /** Currency of the average purchase price (determined by first transaction) */
  avgCostCurrency?: string;
  originalAvgCost?: number;
  originalAvgCostCurrency?: string;
  totalCost: number;
  currentPrice: number;
  /** Original price in its source currency (before conversion) */
  originalPrice?: number;
  originalCurrentPrice?: number;
  /** Currency of the current market price */
  currency?: string;
  originalCurrency?: string;
  fetchedAt?: string | Date;
  isManualPrice?: boolean;
  dividendYield?: number;
  /** Original dividend amount before conversion */
  originalDividendYield?: number;
  dividendCurrency?: string;
  isManualDividend?: boolean;
  /** Market value = quantity * currentPrice */
  marketValue: number;
  /** Gain/Loss = marketValue - totalCost */
  gainLoss: number;
  /** Gain/Loss as a percentage */
  gainLossPercent: number;
  /** Realized gain of this position (WAC), in the display currency; set by the list page */
  realizedGain?: number;
}

export interface InvestmentMetrics {
  totalValue: number;
  totalCost: number;
  overallGainLoss: number;
  overallGainLossPercent: number;
  estimatedDividendYield: number; // Yearly dividend yield in user's currency
  topPerformer: HoldingData | null;
}

export function mapInvestmentToHolding(investment: StockInvestmentWithPrice): HoldingData {
  const quantity = parseFloat(String(investment.quantity));
  const avgCost = parseFloat(String(investment.averagePrice));
  const currentPrice = investment.currentPrice;

  // Use shared calculation functions
  const metrics = calculateHoldingMetrics({
    quantity,
    averagePrice: avgCost,
    currentPrice,
  });

  return {
    id: investment.id,
    ticker: investment.ticker,
    companyName: investment.companyName,
    quantity,
    avgCost,
    avgCostCurrency: investment.averagePriceCurrency,
    totalCost: metrics.totalCost,
    currentPrice,
    originalPrice: investment.originalPrice,
    marketValue: metrics.marketValue,
    gainLoss: metrics.gainLoss,
    gainLossPercent: metrics.gainLossPercent,
    currency: investment.currency,
    fetchedAt: investment.fetchedAt,
    isManualPrice: investment.isManualPrice,
    dividendYield: investment.dividendYield,
    originalDividendYield: investment.originalDividendYield,
    dividendCurrency: investment.dividendCurrency,
    isManualDividend: investment.isManualDividend,
  };
}

export function calculateMetrics(
  holdings: HoldingData[],
  dividendYield: number = 0
): InvestmentMetrics {
  // Use shared calculation functions
  const totals = calculatePortfolioTotals(holdings);
  const topPerformer = findTopPerformer(holdings);

  return {
    totalValue: totals.totalValue,
    totalCost: totals.totalCost,
    overallGainLoss: totals.overallGainLoss,
    overallGainLossPercent: totals.overallGainLossPercent,
    estimatedDividendYield: dividendYield,
    topPerformer,
  };
}

export function groupHoldingsByTicker(holdings: HoldingData[]): HoldingData[] {
  const groups = new Map<string, HoldingData[]>();

  // Group by ticker
  holdings.forEach((h) => {
    const existing = groups.get(h.ticker) || [];
    existing.push(h);
    groups.set(h.ticker, existing);
  });

  // Aggregate
  return Array.from(groups.values()).map((group) => {
    const first = group[0];
    if (group.length === 1) return first;

    const quantity = group.reduce((sum, h) => sum + h.quantity, 0);
    const totalCost = group.reduce((sum, h) => sum + h.totalCost, 0);
    const marketValue = group.reduce((sum, h) => sum + h.marketValue, 0);
    // Use shared calculation functions
    const gainLoss = calculateGainLoss(marketValue, totalCost);
    const gainLossPercent = calculateGainLossPercent(gainLoss, totalCost);
    const avgCost = quantity > 0 ? totalCost / quantity : 0;

    return {
      ...first,
      id: `group-${first.ticker}`, // Use a synthetic ID for the group
      quantity,
      totalCost,
      marketValue,
      gainLoss,
      gainLossPercent,
      avgCost,
    };
  });
}

export function getInstrumentIcon(ticker: string): string {
  const colors = [
    'bg-orange-500',
    'bg-gray-800',
    'bg-teal-600',
    'bg-black',
    'bg-blue-600',
    'bg-purple-600',
    'bg-red-600',
  ];
  const hash = ticker.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
  return colors[hash % colors.length];
}
