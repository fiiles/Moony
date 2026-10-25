//! Type bindings generation for TypeScript
//!
//! Run with: cargo test generate_bindings -- --ignored
//!
//! This module collects all types that need to be exported to TypeScript
//! for frontend use. The generated types ensure type safety across the
//! Rust backend and TypeScript frontend.

use specta::TypeCollection;

/// Collect all types that should be exported to TypeScript.
/// Each type registered here will be included in the generated TypeScript file.
pub fn collect_types() -> TypeCollection {
    let mut types = specta::TypeCollection::default();

    // User models
    types.register::<crate::models::UserProfile>();
    types.register::<crate::models::MenuPreferences>();
    types.register::<crate::models::UpdateUserProfile>();
    types.register::<crate::models::InsertUserProfile>();

    // Biometric unlock (phone lock screen)
    types.register::<crate::models::BiometricStatus>();
    types.register::<crate::models::BiometryType>();

    // Investment models
    types.register::<crate::models::StockInvestment>();
    types.register::<crate::models::EnrichedStockInvestment>();
    types.register::<crate::models::PositionTotals>();
    types.register::<crate::models::StockPositions>();
    types.register::<crate::models::StockPosition>();
    types.register::<crate::models::InvestmentWithDetails>();
    types.register::<crate::models::InsertStockInvestment>();
    types.register::<crate::models::InvestmentTransaction>();
    types.register::<crate::models::InsertInvestmentTransaction>();
    types.register::<crate::models::StockPriceOverride>();
    types.register::<crate::models::DividendOverride>();
    types.register::<crate::models::TickerValueHistory>();
    types.register::<crate::models::PerformanceSeriesSpec>();
    types.register::<crate::models::StockPerformanceRequest>();
    types.register::<crate::models::PerformanceSeries>();
    types.register::<crate::models::PositionPerformance>();
    types.register::<crate::models::StockPerformance>();
    types.register::<crate::models::StockBenchmark>();
    types.register::<crate::models::TickerValueHistoryReport>();
    types.register::<crate::services::history_recalc::HistoryRecalcEvent>();

    // Crypto models
    types.register::<crate::models::CryptoInvestment>();
    types.register::<crate::models::EnrichedCryptoInvestment>();
    types.register::<crate::models::CryptoPositions>();
    types.register::<crate::models::InsertCryptoInvestment>();
    types.register::<crate::models::CryptoTransaction>();
    types.register::<crate::models::InsertCryptoTransaction>();

    // Bank account interest-rate zones
    types.register::<crate::models::SavingsAccountZone>();

    // Bond models
    types.register::<crate::models::Bond>();
    types.register::<crate::models::InsertBond>();

    // Loan models
    types.register::<crate::models::Loan>();
    types.register::<crate::models::InsertLoan>();
    types.register::<crate::models::LoanSchedule>();
    types.register::<crate::models::LoanScheduleRow>();
    types.register::<crate::models::LoanEvent>();
    types.register::<crate::models::InsertLoanEvent>();

    // Real estate models
    types.register::<crate::models::RealEstate>();
    types.register::<crate::models::InsertRealEstate>();
    types.register::<crate::models::RecurringCost>();
    types.register::<crate::models::RealEstateOneTimeCost>();
    types.register::<crate::models::AssetValuation>();
    types.register::<crate::models::InsertAssetValuation>();
    types.register::<crate::models::RealEstatePhotoBatch>();
    types.register::<crate::models::RealEstatePhoto>();
    types.register::<crate::models::RealEstateDocument>();

    // Insurance models
    types.register::<crate::models::InsurancePolicy>();
    types.register::<crate::models::InsertInsurancePolicy>();
    types.register::<crate::models::InsuranceLimit>();
    types.register::<crate::models::InsuranceDocument>();

    // Attachment models
    types.register::<crate::models::AttachmentKind>();
    types.register::<crate::models::AttachmentStatus>();

    // Other assets models
    types.register::<crate::models::OtherAsset>();
    types.register::<crate::models::InsertOtherAsset>();
    types.register::<crate::models::OtherAssetTransaction>();
    types.register::<crate::models::InsertOtherAssetTransaction>();

    // Bank account models
    types.register::<crate::models::AccountType>();
    types.register::<crate::models::DataSource>();
    types.register::<crate::models::BankAccount>();
    types.register::<crate::models::InsertBankAccount>();
    types.register::<crate::models::Institution>();
    types.register::<crate::models::BankAccountWithInstitution>();

    // Bank transaction models
    types.register::<crate::models::TransactionType>();
    types.register::<crate::models::TransactionStatus>();
    types.register::<crate::models::FlowKind>();
    types.register::<crate::models::BankTransaction>();
    types.register::<crate::models::InsertBankTransaction>();
    types.register::<crate::models::TransactionCategory>();
    types.register::<crate::models::InsertTransactionCategory>();
    types.register::<crate::models::UpdateTransactionCategory>();
    types.register::<crate::models::CategoryUsage>();
    types.register::<crate::models::TransactionFilters>();
    types.register::<crate::models::TransactionQueryResult>();

    // CSV import models
    types.register::<crate::services::csv_presets::CsvPreset>();
    types.register::<crate::services::csv_presets::CsvRowFilter>();
    types.register::<crate::services::csv_presets::ExportRoute>();
    types.register::<crate::services::csv_presets::MobileExportGuide>();
    types.register::<crate::services::csv_import::CsvImportConfig>();
    types.register::<crate::services::csv_import::CsvCategoryOverride>();
    types.register::<crate::services::csv_import::CsvImportResult>();
    types.register::<crate::services::csv_import::CsvImportPreview>();
    types.register::<crate::services::csv_import::types::StatementBalance>();
    types.register::<crate::services::csv_import::types::CsvImportBatch>();
    types.register::<crate::models::bank_accounts::BankBalanceRecord>();
    types.register::<crate::models::bank_accounts::InsertBalanceRecord>();
    types.register::<crate::services::csv_import::CsvPreviewRow>();
    types.register::<crate::services::csv_import::CsvPreviewResult>();
    types.register::<crate::services::csv_import::InferredColumns>();
    types.register::<crate::services::csv_import::InferredField>();
    types.register::<crate::services::csv_import::SignCheck>();
    types.register::<crate::services::csv_import::CsvRowMessage>();
    types.register::<crate::services::csv_import::CsvRowStatus>();
    types.register::<crate::services::csv_import::CsvDateRange>();
    types.register::<crate::services::csv_import::ImportOverlap>();
    types.register::<crate::services::csv_import::ImportOverlapBatch>();
    types.register::<crate::services::csv_import::DeclaredBalance>();
    types.register::<crate::services::csv_import::SourceCategoryMapping>();

    // Statement import (CAMT.053, MT940, OFX, QIF)
    types.register::<crate::services::statement_import::StatementFormat>();
    types.register::<crate::services::statement_import::BalanceCheck>();
    types.register::<crate::services::statement_import::AccountMatch>();
    types.register::<crate::services::statement_import::DateOrder>();
    types.register::<crate::services::statement_import::SourceCategoryCount>();
    types.register::<crate::services::statement_import::StatementWarning>();
    types.register::<crate::services::statement_import::StatementSummary>();
    types.register::<crate::services::statement_import::StatementInspection>();
    types.register::<crate::services::statement_import::StatementImportOptions>();

    // Stock CSV import
    types.register::<crate::services::stock_import::StockImportConfig>();
    types.register::<crate::services::stock_import::StockImportTransforms>();
    types.register::<crate::services::stock_import::StockTypeValueMapping>();
    types.register::<crate::services::stock_import::StockInstrumentOverride>();
    types.register::<crate::services::stock_import::StockSplitOverride>();
    types.register::<crate::services::stock_import::StockSplitAction>();
    types.register::<crate::services::stock_import::StockCsvInspectOptions>();
    types.register::<crate::services::stock_import::StockCsvInspection>();
    types.register::<crate::services::stock_import::StockColumnSuggestion>();
    types.register::<crate::services::stock_import::StockColumnValues>();
    types.register::<crate::services::stock_import::StockTypeValueStat>();
    types.register::<crate::services::stock_import::StockImportPreview>();
    types.register::<crate::services::stock_import::StockPreviewRow>();
    types.register::<crate::services::stock_import::StockRowMessage>();
    types.register::<crate::services::stock_import::StockImportInstrument>();
    types.register::<crate::services::stock_import::StockImportCounts>();
    types.register::<crate::services::stock_import::StockInstrumentQuery>();
    types.register::<crate::services::stock_import::StockInstrumentCandidate>();
    types.register::<crate::services::stock_import::StockInstrumentResolution>();
    types.register::<crate::services::stock_import::StockSplitCheck>();
    types.register::<crate::services::stock_import::StockSplitCheckSplit>();
    types.register::<crate::services::stock_import::StockSplitCheckTrade>();
    types.register::<crate::services::stock_import::StockImportResult>();
    types.register::<crate::services::stock_import::StockImportBatch>();
    types.register::<crate::services::stock_import::StockImportUndoResult>();
    types.register::<crate::services::stock_import::SavedStockImportFormat>();

    // Stock splits
    types.register::<crate::services::stock_splits::StockSplit>();
    types.register::<crate::services::stock_splits::InsertStockSplit>();
    types.register::<crate::services::stock_splits::StockSplitVerdict>();
    types.register::<crate::services::stock_splits::StockSplitApplyResult>();
    types.register::<crate::services::stock_splits::StockSplitStatus>();
    types.register::<crate::services::stock_splits::StockSplitSource>();

    // Stock tags models
    types.register::<crate::models::StockTag>();
    types.register::<crate::models::InsertStockTag>();
    types.register::<crate::models::StockTagGroup>();
    types.register::<crate::models::InsertStockTagGroup>();
    types.register::<crate::models::StockInvestmentWithTags>();
    types.register::<crate::models::StocksAnalysis>();

    // Stock Monitor models
    types.register::<crate::models::WatchedStock>();
    types.register::<crate::models::WatchedStockRow>();
    types.register::<crate::models::StockMonitorDetail>();
    types.register::<crate::models::InsertWatchedStock>();
    types.register::<crate::models::StockPricePoint>();

    // Company data models
    types.register::<crate::models::company_info::StockCompanyInfo>();

    // Cashflow models
    types.register::<crate::models::CashflowItem>();
    types.register::<crate::models::InsertCashflowItem>();
    types.register::<crate::models::SetCashflowPlanInclusion>();
    types.register::<crate::models::CashflowMonth>();
    types.register::<crate::models::CashflowGroup>();
    types.register::<crate::models::CashflowActuals>();

    types.register::<crate::models::FlowSource>();
    types.register::<crate::models::FxQuality>();
    types.register::<crate::models::ReportQuality>();
    types.register::<crate::models::fx::ConversionQuality>();
    types.register::<crate::models::fx::FxSnapshot>();
    types.register::<crate::models::fx::FxDayQuotes>();
    types.register::<crate::models::fx::FxHistoryQuotes>();
    types.register::<crate::models::ReportTransaction>();
    types.register::<crate::models::EffectiveTransactionFlow>();
    types.register::<crate::models::BudgetGoal>();
    types.register::<crate::models::BudgetGoalVersion>();
    types.register::<crate::models::BudgetGoalInterval>();
    types.register::<crate::models::InsertBudgetGoal>();
    types.register::<crate::models::CategorySpendingSummary>();
    types.register::<crate::models::BudgetingReport>();
    types.register::<crate::models::CashflowReportItem>();
    types.register::<crate::models::CashflowOriginalComponent>();
    types.register::<crate::models::CashflowCategory>();
    types.register::<crate::models::CashflowSection>();
    types.register::<crate::models::CashflowReport>();
    // Portfolio models
    types.register::<crate::models::PortfolioMetrics>();
    types.register::<crate::models::PortfolioHistoryPoint>();
    types.register::<crate::models::PortfolioHistoryResponse>();
    types.register::<crate::models::DatedFact>();
    types.register::<crate::commands::portfolio::PriceStatus>();

    // Projection models
    types.register::<crate::models::ProjectionSettings>();
    types.register::<crate::models::ProjectionTimelinePoint>();
    types.register::<crate::models::CalculatedDefaults>();
    types.register::<crate::models::PortfolioProjection>();

    // Onboarding
    types.register::<crate::models::OnboardingProgress>();

    // Milestones
    types.register::<crate::models::Milestone>();

    // Backup / data models
    types.register::<crate::models::BackupFileEntry>();
    types.register::<crate::models::BackupManifest>();
    types.register::<crate::models::BackupInspection>();
    types.register::<crate::models::IntegrityReport>();
    types.register::<crate::models::DataLocation>();
    types.register::<crate::models::ExportSummary>();

    // MCP server models
    types.register::<crate::commands::auth::McpServerStatus>();
    types.register::<crate::models::categorization::InsertScopedLearning>();
    types.register::<crate::models::categorization::LearnedPayeeEntry>();
    types.register::<crate::models::categorization::CategoryCount>();
    types.register::<crate::models::categorization::ChangedRow>();
    types.register::<crate::models::categorization::GoverningRule>();
    types.register::<crate::models::categorization::RecategorizationPreview>();
    types.register::<crate::models::categorization::SuggestionSource>();
    types.register::<crate::models::categorization::CategorySuggestion>();
    types.register::<crate::models::categorization::CategorizeQueueRow>();
    types.register::<crate::models::categorization::CategorizeQueue>();
    types.register::<crate::models::categorization::SnoozeCategorization>();
    types.register::<crate::commands::categorization::RulePackInfo>();
    types.register::<crate::commands::categorization::PackRuleInfo>();
    types.register::<crate::services::categorization::hits::RuleHitCount>();
    types.register::<crate::services::categorization::hits::RuleMatchSample>();
    types.register::<crate::services::categorization::hits::RuleMatchPreview>();
    types.register::<crate::services::categorization::hits::SourceCount>();
    types.register::<crate::services::categorization::hits::CategorizationOverview>();

    types
}

#[cfg(test)]
mod tests {
    use super::*;
    use specta_typescript::{BigIntExportBehavior, Typescript};
    use std::path::Path;

    #[test]
    #[ignore] // Run manually: cargo test generate_bindings -- --ignored
    fn generate_bindings() {
        let types = collect_types();
        let output_path = Path::new("../shared/generated-types.ts");

        let typescript = Typescript::new()
            .header("// AUTO-GENERATED FILE - DO NOT EDIT\n// Generated by: cargo test generate_bindings -- --ignored\n")
            .bigint(BigIntExportBehavior::Number); // Use number for i64 timestamps

        typescript
            .export_to(output_path, &types)
            .expect("Failed to generate TypeScript bindings");

        println!("Generated TypeScript bindings at {:?}", output_path);
    }
}
