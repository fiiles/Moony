/**
 * Tauri API Client
 *
 * Wraps Tauri invoke calls to provide a consistent API for the frontend.
 * Replaces the HTTP-based API client from the Express.js version.
 */

import { invoke } from '@tauri-apps/api/core';
import i18n from '@/i18n';
import { toApiError } from '@/lib/api-error';
import type {
  AssetValuation,
  InsertAssetValuation,
  LoanEvent,
  InsertLoanEvent,
  OnboardingProgress,
  BackupManifest,
  BackupInspection,
  DataLocation,
  ExportSummary,
  IntegrityReport,
  UserProfile,
  SavingsAccountZone,
  InvestmentTransaction,
  InsertInvestmentTransaction,
  CryptoTransaction,
  Bond,
  InsertBond,
  Loan,
  InsertLoan,
  LoanSchedule,
  RealEstate,
  InsertRealEstate,
  RealEstateOneTimeCost,
  InsertRealEstateOneTimeCost,
  RealEstatePhotoBatch,
  RealEstatePhoto,
  RealEstateDocument,
  InsurancePolicy,
  InsertInsurancePolicy,
  InsuranceDocument,
  OtherAsset,
  InsertOtherAsset,
  OtherAssetTransaction,
  InsertOtherAssetTransaction,
  PortfolioMetrics,
  PortfolioMetricsHistory,
  PriceStatus,
  CashflowReport,
  CashflowActuals,
  CashflowRange,
  CashflowItem,
  ProjectionSettings,
  PortfolioProjection,
  // Bank account types
  BankAccountWithInstitution,
  BankAccount,
  InsertBankAccount,
  Institution,
  BankTransaction,
  InsertBankTransaction,
  TransactionCategory,
  InsertTransactionCategory,
  UpdateTransactionCategory,
  CategoryUsage,
  TransactionFilters,
  TransactionQueryResult,
  CsvPreset,
  CsvPreviewResult,
  CsvImportPreview,
  CsvImportResult,
  CsvImportConfigInput,
  CsvImportBatch,
  // Stock tags types
  StockTag,
  InsertStockTag,
  StockTagGroup,
  InsertStockTagGroup,
  StockInvestmentWithTags,
  TagMetrics,
  TwrSeries,
  McpServerStatus,
  // Stock Monitor types
  WatchedStock,
  WatchedStockRow,
  StockMonitorDetail,
  StockPricePoint,
  // Company data types
  StockCompanyInfo,
  // Categorization types
  CategorizationResult,
  TransactionInput,
  CategorizationRule,
  CategorizationStats,
  LearnedPayeeEntry,
  CustomRule,
  CustomRuleInput,
  RuleHitCount,
  RuleMatchPreview,
  CategorizationOverview,
  RulePackInfo,
  PackRuleInfo,
} from '../../shared/schema';
import type {
  StockInvestmentWithPrice,
  CryptoInvestmentWithPrice,
  InvestmentWithDetails,
} from '../../shared/types/extended-types';

// Import result from backend
interface ImportResult {
  success: number;
  imported: string[];
  errors: string[];
}

// Per-ticker value history record
export interface TickerValueHistory {
  ticker: string;
  recordedAt: number;
  valueCzk: string;
  quantity: string;
  price: string;
  currency: string;
}

// Backend error keys (`validation.*`, `auth.*`) and the generic `errors.*` messages live in common.json.
const translateCommon = (key: string, options?: Record<string, unknown>): string =>
  i18n.t(key, { ...options, ns: 'common' }) as string;

// Generic invoke wrapper with error handling. Every failure is thrown as an `ApiError` whose
// `message` is already localized (see `src/lib/api-error.ts`); the raw backend text stays in
// `error.raw` and in the console.
async function tauriInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.error(`Tauri command ${command} failed:`, error);
    throw toApiError(error, translateCommon);
  }
}

// ============================================================================
// Auth API
// ============================================================================

export const authApi = {
  checkSetup: () => tauriInvoke<boolean>('check_setup'),

  // 2-Phase Setup
  prepareSetup: () =>
    tauriInvoke<{ recoveryKey: string; masterKeyHex: string; salt: number[] }>('prepare_setup'),

  confirmSetup: (data: {
    name: string;
    surname: string;
    email: string;
    password: string;
    masterKeyHex: string;
    recoveryKey: string;
    salt: number[];
    language?: string;
    currency?: string;
  }) => tauriInvoke<UserProfile>('confirm_setup', { data }),

  unlock: (password: string) => tauriInvoke<UserProfile>('unlock', { password }),

  // 2-Phase Recovery (password reset using recovery key)
  prepareRecover: (data: { recoveryKey: string; newPassword: string }) =>
    tauriInvoke<{ recoveryKey: string }>('prepare_recover', { data }),

  confirmRecover: (data: { oldRecoveryKey: string; newPassword: string; newRecoveryKey: string }) =>
    tauriInvoke<UserProfile>('confirm_recover', { data }),

  logout: () => tauriInvoke<void>('logout'),

  isAuthenticated: () => tauriInvoke<boolean>('is_authenticated'),

  getProfile: () => tauriInvoke<UserProfile | null>('get_user_profile'),

  updateProfile: (updates: Partial<UserProfile>) =>
    tauriInvoke<UserProfile>('update_user_profile', { updates }),

  // 2-Phase Change Password
  prepareChangePassword: (data: { currentPassword: string }) =>
    tauriInvoke<{ recoveryKey: string }>('prepare_change_password', { data }),

  confirmChangePassword: (data: {
    currentPassword: string;
    newPassword: string;
    recoveryKey: string;
  }) => tauriInvoke<void>('confirm_change_password', { data }),

  deleteAccount: () => tauriInvoke<void>('delete_account'),

  setMcpServerEnabled: (enabled: boolean) =>
    tauriInvoke<UserProfile>('set_mcp_server_enabled', { enabled }),

  getMcpServerStatus: () => tauriInvoke<McpServerStatus>('get_mcp_server_status'),

  getMcpServerToken: () => tauriInvoke<string | null>('get_mcp_server_token'),

  setMcpServerPort: (port: number) => tauriInvoke<void>('set_mcp_server_port', { port }),

  regenerateMcpToken: () => tauriInvoke<string>('regenerate_mcp_token'),
};

// ============================================================================
// Investments API
// ============================================================================

export const investmentsApi = {
  getAll: () => tauriInvoke<StockInvestmentWithPrice[]>('get_all_investments'),

  get: (id: string) => tauriInvoke<StockInvestmentWithPrice>('get_investment', { id }),

  /** Get investment with all details (transactions + tags) in a single IPC call */
  getWithDetails: (id: string) =>
    tauriInvoke<InvestmentWithDetails>('get_investment_with_details', { id }),

  create: (
    data: { ticker: string; companyName: string },
    initialTransaction?: InsertInvestmentTransaction
  ) => tauriInvoke<StockInvestmentWithPrice>('create_investment', { data, initialTransaction }),

  delete: (id: string) => tauriInvoke<void>('delete_investment', { id }),

  updateName: (id: string, companyName: string) =>
    tauriInvoke<StockInvestmentWithPrice>('update_investment_name', { id, companyName }),

  getTransactions: (investmentId: string) =>
    tauriInvoke<InvestmentTransaction[]>('get_investment_transactions', { investmentId }),

  getAllTransactions: () => tauriInvoke<InvestmentTransaction[]>('get_all_stock_transactions'),

  createTransaction: (investmentId: string, data: InsertInvestmentTransaction) =>
    tauriInvoke<InvestmentTransaction>('create_investment_transaction', { investmentId, data }),

  deleteTransaction: (txId: string) => tauriInvoke<void>('delete_investment_transaction', { txId }),

  /** Several rows at once: one DB transaction, one history rebuild per ticker. */
  deleteTransactions: (txIds: string[]) =>
    tauriInvoke<void>('delete_investment_transactions', { txIds }),

  updateTransaction: (txId: string, data: Partial<InsertInvestmentTransaction>) =>
    tauriInvoke<InvestmentTransaction>('update_investment_transaction', { txId, data }),

  setManualPrice: (ticker: string, price: string, currency: string) =>
    tauriInvoke<void>('set_manual_price', { ticker, price, currency }),

  deleteManualPrice: (ticker: string) => tauriInvoke<void>('delete_manual_price', { ticker }),

  setManualDividend: (ticker: string, amount: string, currency: string) =>
    tauriInvoke<void>('set_manual_dividend', { ticker, amount, currency }),

  deleteManualDividend: (ticker: string) => tauriInvoke<void>('delete_manual_dividend', { ticker }),

  importTransactions: (
    transactions: Record<string, string | number | boolean | null | undefined>[],
    defaultCurrency: string
  ) =>
    tauriInvoke<ImportResult>('import_investment_transactions', { transactions, defaultCurrency }),

  /** Company data cached from Yahoo Finance; `refresh` fetches it first when it is over a day old. */
  getCompanyInfo: (ticker: string, refresh: boolean) =>
    tauriInvoke<StockCompanyInfo>('get_stock_company_info', { ticker, refresh }),

  getHistory: (ticker: string, startDate?: number, endDate?: number) =>
    tauriInvoke<TickerValueHistory[]>('get_stock_value_history', { ticker, startDate, endDate }),

  backfillHistory: (ticker: string) =>
    tauriInvoke<BackfillResult>('backfill_stock_ticker_history', { ticker }),

  /**
   * Time-weighted return per tag and for the whole portfolio. `investmentIds` narrows every tag
   * series to those positions; the whole-portfolio series is never narrowed.
   */
  getStockTwr: (
    tagIds: string[],
    includePortfolio: boolean,
    includeUntagged: boolean,
    fromTs: number,
    toTs: number,
    investmentIds?: string[]
  ) =>
    tauriInvoke<TwrSeries[]>('get_stock_twr', {
      tagIds,
      includePortfolio,
      includeUntagged,
      investmentIds,
      fromTs,
      toTs,
    }),
};

// ============================================================================
// Crypto API
// ============================================================================

export const cryptoApi = {
  getAll: () => tauriInvoke<CryptoInvestmentWithPrice[]>('get_all_crypto'),

  create: (
    data: { ticker: string; name: string; coingeckoId?: string },
    initialTransaction?: Omit<CryptoTransaction, 'id' | 'investmentId' | 'createdAt'>
  ) => tauriInvoke<CryptoInvestmentWithPrice>('create_crypto', { data, initialTransaction }),

  delete: (id: string) => tauriInvoke<void>('delete_crypto', { id }),

  getTransactions: (investmentId: string) =>
    tauriInvoke<CryptoTransaction[]>('get_crypto_transactions', { investmentId }),

  getAllTransactions: () => tauriInvoke<CryptoTransaction[]>('get_all_crypto_transactions'),

  createTransaction: (
    investmentId: string,
    data: Omit<CryptoTransaction, 'id' | 'investmentId' | 'createdAt'>
  ) => tauriInvoke<CryptoTransaction>('create_crypto_transaction', { investmentId, data }),

  deleteTransaction: (txId: string) => tauriInvoke<void>('delete_crypto_transaction', { txId }),

  updatePrice: (symbol: string, price: string, currency: string, coingeckoId?: string) =>
    tauriInvoke<void>('update_crypto_price', { symbol, price, currency, coingeckoId }),

  deleteManualPrice: (symbol: string) =>
    tauriInvoke<void>('delete_crypto_manual_price', { symbol }),

  getHistory: (ticker: string, startDate?: number, endDate?: number) =>
    tauriInvoke<TickerValueHistory[]>('get_crypto_value_history', { ticker, startDate, endDate }),

  backfillHistory: (ticker: string) =>
    tauriInvoke<BackfillResult>('backfill_crypto_ticker_history', { ticker }),
};

// ============================================================================
// Bonds API
// ============================================================================

export const bondsApi = {
  getAll: () => tauriInvoke<Bond[]>('get_all_bonds'),

  create: (data: InsertBond) => tauriInvoke<Bond>('create_bond', { data }),

  update: (id: string, data: Partial<InsertBond>) => tauriInvoke<Bond>('update_bond', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_bond', { id }),
};

// ============================================================================
// Loans API
// ============================================================================

export const loansApi = {
  getAll: () => tauriInvoke<Loan[]>('get_all_loans'),

  /** Repayment schedule and totals of one loan as of today. */
  getSchedule: (id: string) => tauriInvoke<LoanSchedule>('get_loan_schedule', { id }),

  create: (data: InsertLoan) => tauriInvoke<Loan>('create_loan', { data }),

  update: (id: string, data: Partial<InsertLoan>) => tauriInvoke<Loan>('update_loan', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_loan', { id }),

  // Real estate linking
  getRealEstate: (loanId: string) =>
    tauriInvoke<RealEstate | null>('get_loan_real_estate', { loanId }),

  getAvailable: () => tauriInvoke<Loan[]>('get_available_loans'),

  /** Extra payments, rate changes and balance checks, oldest first (loan detail time trace). */
  getEvents: (loanId: string) => tauriInvoke<LoanEvent[]>('get_loan_events', { loanId }),

  /** Records an event and re-anchors the amortization on its day. */
  addEvent: (data: InsertLoanEvent) => tauriInvoke<LoanEvent>('add_loan_event', { data }),

  deleteEvent: (eventId: string) => tauriInvoke<void>('delete_loan_event', { eventId }),
};

// ============================================================================
// Real Estate API
// ============================================================================

export const realEstateApi = {
  getAll: () => tauriInvoke<RealEstate[]>('get_all_real_estate'),

  get: (id: string) => tauriInvoke<RealEstate | null>('get_real_estate', { id }),

  create: (data: InsertRealEstate) => tauriInvoke<RealEstate>('create_real_estate', { data }),

  update: (id: string, data: Partial<InsertRealEstate>) =>
    tauriInvoke<RealEstate>('update_real_estate', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_real_estate', { id }),

  getCosts: (realEstateId: string) =>
    tauriInvoke<RealEstateOneTimeCost[]>('get_real_estate_costs', { realEstateId }),

  /** Dated valuations of a property, oldest first; the newest one is the market price. */
  getValuations: (realEstateId: string) =>
    tauriInvoke<AssetValuation[]>('get_real_estate_valuations', { realEstateId }),

  addValuation: (data: InsertAssetValuation) =>
    tauriInvoke<AssetValuation>('add_real_estate_valuation', { data }),

  deleteValuation: (valuationId: string) =>
    tauriInvoke<void>('delete_real_estate_valuation', { valuationId }),

  createCost: (data: InsertRealEstateOneTimeCost) =>
    tauriInvoke<RealEstateOneTimeCost>('create_real_estate_cost', { data }),

  deleteCost: (costId: string) => tauriInvoke<void>('delete_real_estate_cost', { costId }),

  updateCost: (costId: string, data: Partial<InsertRealEstateOneTimeCost>) =>
    tauriInvoke<RealEstateOneTimeCost>('update_real_estate_cost', { costId, data }),

  getLoans: (realEstateId: string) =>
    tauriInvoke<Loan[]>('get_real_estate_loans', { realEstateId }),

  linkLoan: (realEstateId: string, loanId: string) =>
    tauriInvoke<void>('link_loan_to_real_estate', { realEstateId, loanId }),

  unlinkLoan: (realEstateId: string, loanId: string) =>
    tauriInvoke<void>('unlink_loan_from_real_estate', { realEstateId, loanId }),

  // Insurance linking
  getInsurances: (realEstateId: string) =>
    tauriInvoke<InsurancePolicy[]>('get_real_estate_insurances', { realEstateId }),

  linkInsurance: (realEstateId: string, insuranceId: string) =>
    tauriInvoke<void>('link_insurance_to_real_estate', { realEstateId, insuranceId }),

  unlinkInsurance: (realEstateId: string, insuranceId: string) =>
    tauriInvoke<void>('unlink_insurance_from_real_estate', { realEstateId, insuranceId }),

  // Photo batches
  getPhotoBatches: (realEstateId: string) =>
    tauriInvoke<RealEstatePhotoBatch[]>('get_real_estate_photo_batches', { realEstateId }),

  createPhotoBatch: (realEstateId: string, data: { photoDate: number; description?: string }) =>
    tauriInvoke<RealEstatePhotoBatch>('create_photo_batch', { realEstateId, data }),

  addPhotosToBatch: (batchId: string, filePaths: string[]) =>
    tauriInvoke<RealEstatePhoto[]>('add_photos_to_batch', { batchId, filePaths }),

  updatePhotoBatch: (batchId: string, data: { photoDate?: number; description?: string }) =>
    tauriInvoke<RealEstatePhotoBatch>('update_photo_batch', { batchId, data }),

  deletePhotoBatch: (batchId: string) => tauriInvoke<void>('delete_photo_batch', { batchId }),

  deletePhoto: (photoId: string) => tauriInvoke<void>('delete_real_estate_photo', { photoId }),

  // Document management
  getDocuments: (realEstateId: string) =>
    tauriInvoke<RealEstateDocument[]>('get_real_estate_documents', { realEstateId }),

  addDocument: (
    realEstateId: string,
    filePath: string,
    data: { name: string; description?: string; fileType?: string }
  ) =>
    tauriInvoke<RealEstateDocument>('add_real_estate_document', { realEstateId, filePath, data }),

  deleteDocument: (documentId: string) =>
    tauriInvoke<void>('delete_real_estate_document', { documentId }),

  openDocument: (documentId: string) =>
    tauriInvoke<void>('open_real_estate_document', { documentId }),
};

// ============================================================================
// Insurance API
// ============================================================================

export const insuranceApi = {
  getAll: () => tauriInvoke<InsurancePolicy[]>('get_all_insurance'),

  get: (id: string) => tauriInvoke<InsurancePolicy | null>('get_insurance', { id }),

  create: (data: InsertInsurancePolicy) =>
    tauriInvoke<InsurancePolicy>('create_insurance', { data }),

  update: (id: string, data: Partial<InsertInsurancePolicy>) =>
    tauriInvoke<InsurancePolicy>('update_insurance', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_insurance', { id }),

  // Document management
  getDocuments: (insuranceId: string) =>
    tauriInvoke<InsuranceDocument[]>('get_insurance_documents', { insuranceId }),

  addDocument: (
    insuranceId: string,
    filePath: string,
    data: { name: string; description?: string; fileType?: string }
  ) => tauriInvoke<InsuranceDocument>('add_insurance_document', { insuranceId, filePath, data }),

  deleteDocument: (documentId: string) =>
    tauriInvoke<void>('delete_insurance_document', { documentId }),

  openDocument: (documentId: string) =>
    tauriInvoke<void>('open_insurance_document', { documentId }),

  // Real estate linking
  getRealEstate: (insuranceId: string) =>
    tauriInvoke<RealEstate | null>('get_insurance_real_estate', { insuranceId }),

  getAvailable: () => tauriInvoke<InsurancePolicy[]>('get_available_insurances'),
};

// ============================================================================
// Other Assets API
// ============================================================================

export const otherAssetsApi = {
  getAll: () => tauriInvoke<OtherAsset[]>('get_all_other_assets'),

  create: (data: InsertOtherAsset, initialTransaction?: InsertOtherAssetTransaction) =>
    tauriInvoke<OtherAsset>('create_other_asset', { data, initialTransaction }),

  update: (id: string, data: Partial<InsertOtherAsset>) =>
    tauriInvoke<OtherAsset>('update_other_asset', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_other_asset', { id }),

  getTransactions: (assetId: string) =>
    tauriInvoke<OtherAssetTransaction[]>('get_other_asset_transactions', { assetId }),

  createTransaction: (assetId: string, data: InsertOtherAssetTransaction) =>
    tauriInvoke<OtherAssetTransaction>('create_other_asset_transaction', { assetId, data }),

  deleteTransaction: (txId: string) =>
    tauriInvoke<void>('delete_other_asset_transaction', { txId }),

  /** Dated price-per-unit estimates, oldest first; the newest one is the market price. */
  getValuations: (assetId: string) =>
    tauriInvoke<AssetValuation[]>('get_other_asset_valuations', { assetId }),

  addValuation: (data: InsertAssetValuation) =>
    tauriInvoke<AssetValuation>('add_other_asset_valuation', { data }),

  deleteValuation: (valuationId: string) =>
    tauriInvoke<void>('delete_other_asset_valuation', { valuationId }),
};

// ============================================================================
// Portfolio API
// ============================================================================

export interface BackfillResult {
  days_processed: number;
  total_days: number;
  completed: boolean;
  message: string;
}

export const portfolioApi = {
  getMetrics: (excludePersonalRealEstate: boolean = false) =>
    tauriInvoke<PortfolioMetrics>('get_portfolio_metrics', { excludePersonalRealEstate }),

  getHistory: (startDate?: number, endDate?: number) =>
    tauriInvoke<PortfolioMetricsHistory[]>('get_portfolio_history', { startDate, endDate }),

  recordSnapshot: () => tauriInvoke<void>('record_portfolio_snapshot'),

  refreshExchangeRates: () => tauriInvoke<Record<string, number>>('refresh_exchange_rates'),

  getExchangeRates: () => tauriInvoke<Record<string, number>>('get_exchange_rates'),

  getExchangeRatesForDate: (date: number) =>
    tauriInvoke<Record<string, number>>('get_exchange_rates_for_date', { date }),

  getExchangeRatesForDateRange: (startDate: number, endDate: number) =>
    tauriInvoke<Record<number, Record<string, number>>>('get_exchange_rates_for_date_range', {
      startDate,
      endDate,
    }),

  getPriceStatus: () => tauriInvoke<PriceStatus>('get_price_status'),

  startBackfill: () => tauriInvoke<BackfillResult>('start_snapshot_backfill'),

  recalculateAllHistory: () => tauriInvoke<BackfillResult>('recalculate_all_portfolio_history'),

  backfillCurrencyBreakdowns: () => tauriInvoke<number>('backfill_currency_breakdowns'),

  // Historical recalculation after an MCP bulk import of retroactive
  // transactions (see mcp-data-changed listener in SyncProvider.tsx).
  triggerImportRecalculation: (
    assetType: string,
    tickers: [string, string | null][],
    earliestDate: number
  ) => tauriInvoke<void>('trigger_import_recalculation', { assetType, tickers, earliestDate }),
};

// ============================================================================
// Price API (Yahoo Finance for stocks, CoinGecko for crypto)
// ============================================================================

export interface ApiKeys {
  coingecko?: string;
}

export interface StockPriceResult {
  ticker: string;
  price: number;
  currency: string;
}

// Result from the Yahoo Finance stock price refresh with rate limit info
export interface StockPriceRefreshResult {
  updated: StockPriceResult[];
  remaining_tickers: string[];
  rate_limit_hit: boolean;
}

export interface CryptoPriceResult {
  ticker: string;
  price: number;
  currency: string;
}

export interface CoinGeckoSearchResult {
  id: string;
  symbol: string;
  name: string;
  market_cap_rank: number | null;
  thumb: string | null;
}

export interface DividendResult {
  ticker: string;
  yearly_sum: number;
  currency: string;
}

export interface StockSearchResult {
  symbol: string;
  shortname: string;
  exchange: string;
}

export const priceApi = {
  getApiKeys: () => tauriInvoke<ApiKeys>('get_api_keys'),

  setApiKeys: (keys: ApiKeys) => tauriInvoke<void>('set_api_keys', { keys }),

  refreshStockPrices: (forceRefresh?: boolean) =>
    tauriInvoke<StockPriceRefreshResult>('refresh_stock_prices', {
      forceRefresh: forceRefresh ?? false,
    }),

  refreshCryptoPrices: () => tauriInvoke<CryptoPriceResult[]>('refresh_crypto_prices'),

  searchCrypto: (query: string) => tauriInvoke<CoinGeckoSearchResult[]>('search_crypto', { query }),

  refreshDividends: () => tauriInvoke<DividendResult[]>('refresh_dividends'),

  searchStockTickers: (query: string) =>
    tauriInvoke<StockSearchResult[]>('search_stock_tickers', { query }),
};

// ============================================================================
// Stock Monitor API
// ============================================================================

export const stockMonitorApi = {
  getWatched: () => tauriInvoke<WatchedStockRow[]>('get_watched_stocks'),

  follow: (ticker: string) => tauriInvoke<WatchedStock>('follow_stock', { ticker }),

  unfollow: (ticker: string) => tauriInvoke<void>('unfollow_stock', { ticker }),
  getPortfolioCandidates: () => tauriInvoke<string[]>('get_portfolio_follow_candidates'),

  followPortfolio: () => tauriInvoke<string[]>('follow_portfolio_stocks'),

  setTargetPrice: (ticker: string, targetPrice: string | null) =>
    tauriInvoke<WatchedStock>('set_watched_target_price', { ticker, targetPrice }),

  updateNotes: (ticker: string, notes: string) =>
    tauriInvoke<WatchedStock>('update_watched_notes', { ticker, notes }),

  refreshPrices: (forceRefresh?: boolean) =>
    tauriInvoke<StockPriceRefreshResult>('refresh_watched_stock_prices', {
      forceRefresh: forceRefresh ?? false,
    }),

  getDetail: (ticker: string, forceRefresh?: boolean) =>
    tauriInvoke<StockMonitorDetail>('get_stock_monitor_detail', {
      ticker,
      forceRefresh: forceRefresh ?? false,
    }),

  getPriceRange: (ticker: string, period: string) =>
    tauriInvoke<StockPricePoint[]>('get_stock_price_range', { ticker, period }),
};

// ============================================================================
// Cashflow API
// ============================================================================

export const cashflowApi = {
  getReport: (viewType: 'monthly' | 'yearly') =>
    tauriInvoke<CashflowReport>('get_cashflow_report', { viewType }),

  /** Actual flows from bank transactions for the Cashflow report page (design system README §11). */
  getActuals: (range: CashflowRange) =>
    tauriInvoke<CashflowActuals>('get_cashflow_actuals', { range }),

  getAllItems: () => tauriInvoke<CashflowItem[]>('get_all_cashflow_items'),

  createItem: (data: {
    name: string;
    amount: string;
    currency?: string;
    frequency: 'monthly' | 'yearly';
    itemType: 'income' | 'expense';
    category: string;
  }) => tauriInvoke<CashflowItem>('create_cashflow_item', { data }),

  updateItem: (
    id: string,
    data: {
      name: string;
      amount: string;
      currency?: string;
      frequency: 'monthly' | 'yearly';
      itemType: 'income' | 'expense';
      category: string;
    }
  ) => tauriInvoke<CashflowItem>('update_cashflow_item', { id, data }),

  deleteItem: (id: string) => tauriInvoke<void>('delete_cashflow_item', { id }),
};

// ============================================================================
// Projection API
// ============================================================================

export interface ProjectionInput {
  horizonYears: number;
  viewType: 'monthly' | 'yearly';
  excludePersonalRealEstate?: boolean;
  /** Percentage points added to every active class's annual rate (scenario band). */
  rateShift?: number;
}

export const projectionApi = {
  getSettings: () => tauriInvoke<ProjectionSettings[]>('get_projection_settings'),

  saveSettings: (settings: ProjectionSettings[]) =>
    tauriInvoke<void>('save_projection_settings', { settings }),

  calculate: (input: ProjectionInput) =>
    tauriInvoke<PortfolioProjection>('calculate_portfolio_projection', { input }),
};

// ============================================================================
// Export API
// ============================================================================

export interface ExportResult {
  csv: string;
  filename: string;
  count: number;
}

export const exportApi = {
  stockTransactions: () => tauriInvoke<ExportResult>('export_stock_transactions'),
  cryptoTransactions: () => tauriInvoke<ExportResult>('export_crypto_transactions'),
  bonds: () => tauriInvoke<ExportResult>('export_bonds'),
  realEstate: () => tauriInvoke<ExportResult>('export_real_estate'),
  realEstateCosts: () => tauriInvoke<ExportResult>('export_real_estate_costs'),
  insurancePolicies: () => tauriInvoke<ExportResult>('export_insurance_policies'),
  loans: () => tauriInvoke<ExportResult>('export_loans'),
  otherAssets: () => tauriInvoke<ExportResult>('export_other_assets'),
  otherAssetTransactions: () => tauriInvoke<ExportResult>('export_other_asset_transactions'),
};

// ============================================================================
// Onboarding API — first-run checklist progress and flags
// ============================================================================

export const onboardingApi = {
  getProgress: () => tauriInvoke<OnboardingProgress>('get_onboarding_progress'),
  setFlag: (
    key: 'onboarding.completedAt' | 'onboarding.checklistDismissed',
    value: string | null
  ) => tauriInvoke<void>('set_onboarding_flag', { key, value }),
};

// ============================================================================
// Data API — backup, restore, full export, integrity, data folder
// ============================================================================

export const dataApi = {
  getDataLocation: () => tauriInvoke<DataLocation>('get_data_location'),
  openDataFolder: () => tauriInvoke<void>('open_data_folder'),
  createBackup: (destPath: string) => tauriInvoke<BackupManifest>('create_backup', { destPath }),
  inspectBackup: (path: string) => tauriInvoke<BackupInspection>('inspect_backup', { path }),
  restoreBackup: (path: string) => tauriInvoke<void>('restore_backup', { path }),
  exportAllData: (destPath: string) => tauriInvoke<ExportSummary>('export_all_data', { destPath }),
  verifyDatabase: () => tauriInvoke<IntegrityReport>('verify_database'),
};

// ============================================================================
// System API
// ============================================================================

export const systemApi = {
  /** Opens the application log folder in the OS file manager. */
  openLogsFolder: () => tauriInvoke<void>('open_logs_folder'),
};

// ============================================================================
// Bank Accounts API
// ============================================================================

export const bankAccountsApi = {
  // Bank accounts
  getAll: () => tauriInvoke<BankAccountWithInstitution[]>('get_all_bank_accounts'),
  get: (id: string) => tauriInvoke<BankAccountWithInstitution | null>('get_bank_account', { id }),
  create: (data: InsertBankAccount) => tauriInvoke<BankAccount>('create_bank_account', { data }),
  update: (id: string, data: InsertBankAccount) =>
    tauriInvoke<BankAccount>('update_bank_account', { id, data }),
  delete: (id: string) => tauriInvoke<void>('delete_bank_account', { id }),

  // Institutions
  getInstitutions: () => tauriInvoke<Institution[]>('get_all_institutions'),
  createInstitution: (name: string) => tauriInvoke<Institution>('create_institution', { name }),

  // Interest-rate zones (tiers) of an account
  getZones: (accountId: string) =>
    tauriInvoke<SavingsAccountZone[]>('get_account_zones', { accountId }),
  createZone: (data: Omit<SavingsAccountZone, 'id' | 'createdAt'>) =>
    tauriInvoke<SavingsAccountZone>('create_account_zone', { data }),
  deleteZone: (zoneId: string) => tauriInvoke<void>('delete_account_zone', { zoneId }),

  // Transactions
  getTransactions: (accountId: string, filters?: TransactionFilters) =>
    tauriInvoke<TransactionQueryResult>('get_bank_transactions', { accountId, filters }),
  createTransaction: (data: InsertBankTransaction) =>
    tauriInvoke<BankTransaction>('create_bank_transaction', { data }),
  deleteTransaction: (id: string) => tauriInvoke<void>('delete_bank_transaction', { id }),
  updateTransactionCategory: (transactionId: string, categoryId: string | null) =>
    tauriInvoke<void>('update_transaction_category', { transactionId, categoryId }),

  // Categories
  getCategories: () => tauriInvoke<TransactionCategory[]>('get_transaction_categories'),
  createCategory: (data: InsertTransactionCategory) =>
    tauriInvoke<TransactionCategory>('create_transaction_category', { data }),
  updateCategoryDefinition: (id: string, data: UpdateTransactionCategory) =>
    tauriInvoke<TransactionCategory>('update_transaction_category_definition', { id, data }),
  /** Refused while the category is in use unless `reassignTo` names its replacement. */
  deleteCategory: (id: string, reassignTo?: string | null) =>
    tauriInvoke<void>('delete_transaction_category', { id, reassignTo: reassignTo ?? null }),
  getCategoryUsage: () => tauriInvoke<CategoryUsage[]>('get_category_usage'),

  // CSV Import
  getCsvPresets: () => tauriInvoke<CsvPreset[]>('get_csv_presets'),
  getCsvPresetByInstitution: (institutionId: string) =>
    tauriInvoke<CsvPreset | null>('get_csv_preset_by_institution', { institutionId }),
  /**
   * Inspect a file. Every argument after `filePath` overrides a detection:
   * `delimiter`, `skipRows` (data rows after the header), `headerRow` (0-based line),
   * `presetId` (apply this bank preset instead of auto-detecting), `encoding`.
   */
  parseCsvFile: (
    filePath: string,
    delimiter?: string,
    skipRows?: number,
    headerRow?: number,
    presetId?: string,
    encoding?: string
  ) =>
    tauriInvoke<CsvPreviewResult>('parse_csv_file', {
      filePath,
      delimiter, // Let backend auto-detect if undefined
      skipRows: skipRows || 0,
      headerRow,
      presetId,
      encoding,
    }),
  /** Dry run: parsed rows with ok / error / duplicate / skipped status; writes nothing. */
  previewCsvImport: (accountId: string, filePath: string, config: CsvImportConfigInput) =>
    tauriInvoke<CsvImportPreview>('preview_csv_import', { accountId, filePath, config }),
  importCsvTransactions: (accountId: string, filePath: string, config: CsvImportConfigInput) =>
    tauriInvoke<CsvImportResult>('import_csv_transactions', { accountId, filePath, config }),
  getImportBatches: (accountId: string) =>
    tauriInvoke<CsvImportBatch[]>('get_import_batches', { accountId }),
  deleteImportBatch: (batchId: string) => tauriInvoke<void>('delete_import_batch', { batchId }),
};

// ============================================================================
// Stock Tags API
// ============================================================================

export const stockTagsApi = {
  getAll: () => tauriInvoke<StockTag[]>('get_all_stock_tags'),

  create: (data: InsertStockTag) => tauriInvoke<StockTag>('create_stock_tag', { data }),

  update: (id: string, data: InsertStockTag) =>
    tauriInvoke<StockTag>('update_stock_tag', { id, data }),

  delete: (id: string) => tauriInvoke<void>('delete_stock_tag', { id }),

  getForInvestment: (investmentId: string) =>
    tauriInvoke<StockTag[]>('get_investment_tags', { investmentId }),

  setForInvestment: (investmentId: string, tagIds: string[]) =>
    tauriInvoke<void>('set_investment_tags', { investmentId, tagIds }),

  getAnalysis: () => tauriInvoke<StockInvestmentWithTags[]>('get_stocks_analysis'),

  getTagMetrics: (tagIds: string[] = []) =>
    tauriInvoke<TagMetrics[]>('get_tag_metrics', { tagIds }),

  // Tag Group operations
  getAllGroups: () => tauriInvoke<StockTagGroup[]>('get_all_stock_tag_groups'),

  createGroup: (data: InsertStockTagGroup) =>
    tauriInvoke<StockTagGroup>('create_stock_tag_group', { data }),

  updateGroup: (id: string, data: InsertStockTagGroup) =>
    tauriInvoke<StockTagGroup>('update_stock_tag_group', { id, data }),

  deleteGroup: (id: string) => tauriInvoke<void>('delete_stock_tag_group', { id }),
};

// ============================================================================
// Categorization API
// ============================================================================

// Wire types live in shared/schema.ts (the canonical contract); re-exported
// here so existing imports keep working.
export type {
  CategorizationSource,
  CategorizationResult,
  TransactionInput,
  RuleType,
  CategorizationRule,
  CategorizationStats,
  LearnedPayeeEntry,
  CustomRule,
  CustomRuleInput,
  RulePackInfo,
  PackRuleInfo,
} from '../../shared/schema';

export const categorizationApi = {
  // Categorize a single transaction
  categorize: (transaction: TransactionInput) =>
    tauriInvoke<CategorizationResult>('categorize_transaction', { transaction }),

  // Categorize multiple transactions in batch
  categorizeBatch: (transactions: TransactionInput[]) =>
    tauriInvoke<CategorizationResult[]>('categorize_batch', { transactions }),

  // Learn from user's manual categorization with hierarchical matching
  // Supports: payee + iban (iban default), payee only (payee default)
  learn: (payee: string | null, counterpartyIban: string | null, categoryId: string) =>
    tauriInvoke<void>('learn_categorization', {
      payee,
      counterpartyIban,
      categoryId,
    }),

  // Forget a learned payee combination
  forget: (payee: string | null, counterpartyIban: string | null) =>
    tauriInvoke<boolean>('forget_payee', {
      payee,
      counterpartyIban,
    }),

  // Update categorization rules
  updateRules: (rules: CategorizationRule[]) =>
    tauriInvoke<void>('update_categorization_rules', { rules }),

  // Get engine statistics
  getStats: () => tauriInvoke<CategorizationStats>('get_categorization_stats'),

  // Export learned payees for backup/persistence
  exportLearnedPayees: () => tauriInvoke<Record<string, string>>('export_learned_payees'),

  // Import learned payees
  importLearnedPayees: (payees: Record<string, string>) =>
    tauriInvoke<number>('import_learned_payees', { payees }),

  // Load learned payees from database (call after app unlock)
  loadFromDb: () => tauriInvoke<number>('load_learned_payees_from_db'),

  // Load user's own IBANs for internal transfer detection (call after app unlock)
  loadOwnIbans: () => tauriInvoke<number>('load_own_ibans_from_db'),

  // Load custom rules from database into the categorization engine (call after app unlock)
  loadCustomRulesFromDb: () => tauriInvoke<number>('load_custom_rules_from_db'),

  // ==================== Locale Rule Packs ====================

  // List all shipped rule packs with their enabled state
  getRulePacks: () => tauriInvoke<RulePackInfo[]>('get_rule_packs'),

  // Persist the enabled pack set and rebuild the engine's default rules
  setRulePacksEnabled: (packIds: string[]) =>
    tauriInvoke<number>('set_rule_packs_enabled', { packIds }),

  // List the rules of one pack with their disabled flags
  getPackRules: (packId: string) => tauriInvoke<PackRuleInfo[]>('get_pack_rules', { packId }),

  // Persist one pack rule's disabled flag
  setPackRuleDisabled: (ruleId: string, disabled: boolean) =>
    tauriInvoke<void>('set_pack_rule_disabled', { ruleId, disabled }),

  // Load the persisted pack configuration into the engine (call after app unlock)
  loadRulePacksConfig: () => tauriInvoke<number>('load_rule_packs_config'),

  // ==================== MCP Category Suggestions ====================

  // Accept a pending MCP suggestion: sets the category, learns the payee,
  // clears the suggestion. Returns the accepted category id.
  acceptSuggestion: (transactionId: string) =>
    tauriInvoke<string>('accept_category_suggestion', { transactionId }),

  // Decline a pending MCP suggestion (clears it, learns nothing)
  declineSuggestion: (transactionId: string) =>
    tauriInvoke<void>('decline_category_suggestion', { transactionId }),

  // ==================== Learned Payees Management ====================

  // Get all learned payees for management UI
  getLearnedPayees: () => tauriInvoke<LearnedPayeeEntry[]>('get_learned_payees_list'),

  // Delete a learned payee by ID
  deleteLearnedPayee: (id: string) => tauriInvoke<void>('delete_learned_payee', { id }),

  // Bulk delete learned payees
  deleteLearnedPayeesBulk: (ids: string[]) =>
    tauriInvoke<number>('delete_learned_payees_bulk', { ids }),

  // Update learned payee category
  updateLearnedPayeeCategory: (id: string, categoryId: string) =>
    tauriInvoke<void>('update_learned_payee_category', { id, categoryId }),

  // ==================== Custom Rules Management ====================

  // Get all custom rules
  getCustomRules: () => tauriInvoke<CustomRule[]>('get_custom_rules'),

  // Create a new custom rule
  createCustomRule: (data: CustomRuleInput) =>
    tauriInvoke<CustomRule>('create_custom_rule', { data }),

  // Update an existing custom rule
  updateCustomRule: (id: string, data: CustomRuleInput) =>
    tauriInvoke<CustomRule>('update_custom_rule', { id, data }),

  // Delete a custom rule
  deleteCustomRule: (id: string) => tauriInvoke<void>('delete_custom_rule', { id }),

  // ==================== Rule statistics (redesign phase 2) ====================

  /** Hits per rule (custom, enabled packs, learned) over the last `days` days. */
  getRuleHitCounts: (days: number) => tauriInvoke<RuleHitCount[]>('get_rule_hit_counts', { days }),

  /** What a draft rule would match today (the rule editor's live preview). */
  previewRuleMatches: (rule: CustomRuleInput, days: number) =>
    tauriInvoke<RuleMatchPreview>('preview_rule_matches', { rule, days }),

  /** Give a stored rule's category to its uncategorized matches of the period. */
  applyRuleToUncategorized: (ruleId: string, days: number) =>
    tauriInvoke<number>('apply_rule_to_uncategorized', { ruleId, days }),

  /** Automation rate of the last `days` days. */
  getCategorizationOverview: (days: number) =>
    tauriInvoke<CategorizationOverview>('get_categorization_overview', { days }),
};

// ============================================================================
// Budgeting API
// ============================================================================

// Budget goal entity
export interface BudgetGoal {
  id: string;
  categoryId: string;
  timeframe: string; // "monthly" | "quarterly" | "yearly"
  amount: string;
  currency: string;
  createdAt: number;
  updatedAt: number;
}

// Data for creating/updating a budget goal
export interface InsertBudgetGoal {
  categoryId: string;
  timeframe: string;
  amount: string;
  currency?: string;
}

// Category spending summary
export interface CategorySpendingSummary {
  categoryId: string;
  categoryName: string;
  categoryIcon?: string;
  categoryColor?: string;
  totalAmount: string;
  transactionCount: number;
  budgetGoal?: BudgetGoal;
  budgetPercentage?: number;
}

// Transaction for budgeting view
export interface BudgetingTransaction {
  id: string;
  bookingDate: number;
  amount: string;
  currency: string;
  description?: string;
  counterpartyName?: string;
  counterpartyIban?: string;
  categoryId?: string;
  bankAccountId: string;
  bankAccountName: string;
  txType: string;
}

// Full budgeting report
export interface BudgetingReport {
  periodStart: number;
  periodEnd: number;
  timeframe: string;
  totalIncome: string;
  totalExpenses: string;
  netBalance: string;
  incomeCategories: CategorySpendingSummary[];
  expenseCategories: CategorySpendingSummary[];
  uncategorizedIncome: string;
  uncategorizedExpenses: string;
  uncategorizedTransactionCount: number;
}

export const budgetingApi = {
  // Get the budgeting report for a time period
  getReport: (startDate: number, endDate: number, timeframe: string) =>
    tauriInvoke<BudgetingReport>('get_budgeting_report', { startDate, endDate, timeframe }),

  // Get transactions for a specific category
  getCategoryTransactions: (categoryId: string, startDate: number, endDate: number) =>
    tauriInvoke<BudgetingTransaction[]>('get_category_transactions', {
      categoryId,
      startDate,
      endDate,
    }),

  // Get all budget goals
  getBudgetGoals: () => tauriInvoke<BudgetGoal[]>('get_budget_goals'),

  // Create or update a budget goal
  upsertBudgetGoal: (data: InsertBudgetGoal) =>
    tauriInvoke<BudgetGoal>('upsert_budget_goal', { data }),

  // Delete a budget goal
  deleteBudgetGoal: (id: string) => tauriInvoke<void>('delete_budget_goal', { id }),
};

// ============================================================================
// Combined API export
// ============================================================================

export const api = {
  auth: authApi,
  investments: investmentsApi,
  crypto: cryptoApi,
  bonds: bondsApi,
  loans: loansApi,
  realEstate: realEstateApi,
  insurance: insuranceApi,
  otherAssets: otherAssetsApi,
  portfolio: portfolioApi,
  price: priceApi,
  stockMonitor: stockMonitorApi,
  cashflow: cashflowApi,
  projection: projectionApi,
  export: exportApi,
  bankAccounts: bankAccountsApi,
  stockTags: stockTagsApi,
  categorization: categorizationApi,
  budgeting: budgetingApi,
};

export default api;
