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
    types.register::<crate::models::PortfolioMetricsHistory>();

    // Investment models
    types.register::<crate::models::StockInvestment>();
    types.register::<crate::models::EnrichedStockInvestment>();
    types.register::<crate::models::InsertStockInvestment>();
    types.register::<crate::models::InvestmentTransaction>();
    types.register::<crate::models::InsertInvestmentTransaction>();
    types.register::<crate::models::StockPriceOverride>();
    types.register::<crate::models::DividendOverride>();
    types.register::<crate::models::TickerValueHistory>();
    types.register::<crate::models::TwrDataPoint>();
    types.register::<crate::models::TwrSeries>();
    types.register::<crate::services::history_recalc::HistoryRecalcEvent>();

    // Crypto models
    types.register::<crate::models::CryptoInvestment>();
    types.register::<crate::models::EnrichedCryptoInvestment>();
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
    types.register::<crate::services::csv_import::CsvImportConfig>();
    types.register::<crate::services::csv_import::CsvCategoryOverride>();
    types.register::<crate::services::csv_import::CsvImportResult>();
    types.register::<crate::services::csv_import::CsvImportPreview>();
    types.register::<crate::services::csv_import::CsvPreviewRow>();
    types.register::<crate::services::csv_import::CsvPreviewResult>();
    types.register::<crate::services::csv_import::CsvRowMessage>();
    types.register::<crate::services::csv_import::CsvRowStatus>();
    types.register::<crate::services::csv_import::CsvDateRange>();

    // Stock CSV import
    types.register::<crate::services::stock_import::StockImportConfig>();
    types.register::<crate::services::stock_import::StockImportTransforms>();
    types.register::<crate::services::stock_import::StockTypeValueMapping>();
    types.register::<crate::services::stock_import::StockInstrumentOverride>();
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
    types.register::<crate::services::stock_import::StockImportResult>();
    types.register::<crate::services::stock_import::StockImportBatch>();
    types.register::<crate::services::stock_import::StockImportUndoResult>();
    types.register::<crate::services::stock_import::SavedStockImportFormat>();

    // Stock tags models
    types.register::<crate::models::StockTag>();
    types.register::<crate::models::InsertStockTag>();
    types.register::<crate::models::StockTagGroup>();
    types.register::<crate::models::InsertStockTagGroup>();

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
    types.register::<crate::models::CashflowMonth>();
    types.register::<crate::models::CashflowGroup>();
    types.register::<crate::models::CashflowActuals>();

    // Portfolio models
    types.register::<crate::commands::portfolio::PortfolioMetrics>();
    types.register::<crate::commands::portfolio::PriceStatus>();

    // Projection models
    types.register::<crate::models::ProjectionSettings>();

    // Onboarding
    types.register::<crate::models::OnboardingProgress>();

    // Backup / data models
    types.register::<crate::models::BackupFileEntry>();
    types.register::<crate::models::BackupManifest>();
    types.register::<crate::models::BackupInspection>();
    types.register::<crate::models::IntegrityReport>();
    types.register::<crate::models::DataLocation>();
    types.register::<crate::models::ExportSummary>();

    // MCP server models
    types.register::<crate::commands::auth::McpServerStatus>();
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
