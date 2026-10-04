//! Moony - Personal Finance Manager
//!
//! Tauri v2 application with Rust backend

pub mod bindings;
pub mod commands;
pub mod constants;
pub mod db;
pub mod error;
pub mod models;
pub mod services;

use std::sync::Arc;

use commands::categorization::CategorizationState;
use db::Database;
use services::categorization::CategorizationEngine;
use services::local_api::LocalApiServer;
use services::logging;
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[tokio::main]
pub async fn run() {
    // Panics go to the log file too (there is no console in a release build)
    logging::install_panic_hook();

    // Initialize categorization engine with default rules
    let categorization_engine = Arc::new(CategorizationEngine::new_empty());

    // The log plugin goes first so every other plugin's `log` output is captured.
    tauri::Builder::default()
        .plugin(logging::plugin())
        .setup(|_app| {
            logging::log_startup_banner();
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Database::new())
        .manage(services::history_recalc::HistoryRecalc::new())
        .manage(CategorizationState(categorization_engine))
        .manage(LocalApiServer::new())
        .invoke_handler(tauri::generate_handler![
            // Onboarding (services::onboarding)
            commands::onboarding::get_onboarding_progress,
            commands::onboarding::set_onboarding_flag,
            // Backup / restore / export (services::backup)
            commands::backup::create_backup,
            commands::backup::inspect_backup,
            commands::backup::restore_backup,
            commands::backup::export_all_data,
            commands::backup::verify_database,
            commands::backup::get_data_location,
            commands::backup::open_data_folder,
            // System (services::logging)
            commands::system::open_logs_folder,
            // Auth commands
            commands::auth::check_setup,
            commands::auth::prepare_setup,
            commands::auth::confirm_setup,
            commands::auth::unlock,
            commands::auth::prepare_recover,
            commands::auth::confirm_recover,
            commands::auth::logout,
            commands::auth::is_authenticated,
            commands::auth::get_user_profile,
            commands::auth::update_user_profile,
            commands::auth::prepare_change_password,
            commands::auth::confirm_change_password,
            commands::auth::delete_account,
            commands::auth::set_mcp_server_enabled,
            commands::auth::get_mcp_server_status,
            commands::auth::get_mcp_server_token,
            commands::auth::set_mcp_server_port,
            commands::auth::regenerate_mcp_token,
            // Investment commands
            commands::investments::get_all_investments,
            commands::investments::get_investment,
            commands::investments::get_investment_with_details,
            commands::investments::create_investment,
            commands::investments::delete_investment,
            commands::investments::update_investment_name,
            commands::investments::get_investment_transactions,
            commands::investments::get_all_stock_transactions,
            commands::investments::create_investment_transaction,
            commands::investments::delete_investment_transaction,
            commands::investments::delete_investment_transactions,
            commands::investments::update_investment_transaction,
            commands::investments::import_investment_transactions,
            commands::investments::set_manual_price,
            commands::investments::delete_manual_price,
            commands::investments::set_manual_dividend,
            commands::investments::delete_manual_dividend,
            commands::investments::get_stock_value_history,
            commands::investments::get_stock_twr,
            // Crypto commands
            commands::crypto::get_all_crypto,
            commands::crypto::create_crypto,
            commands::crypto::delete_crypto,
            commands::crypto::get_crypto_transactions,
            commands::crypto::get_all_crypto_transactions,
            commands::crypto::create_crypto_transaction,
            commands::crypto::delete_crypto_transaction,
            commands::crypto::update_crypto_price,
            commands::crypto::delete_crypto_manual_price,
            commands::crypto::get_crypto_value_history,
            // Bond commands
            commands::bonds::get_all_bonds,
            commands::bonds::create_bond,
            commands::bonds::update_bond,
            commands::bonds::delete_bond,
            // Loan commands
            commands::loans::get_all_loans,
            commands::loans::get_loan_schedule,
            commands::loans::create_loan,
            commands::loans::update_loan,
            commands::loans::delete_loan,
            commands::loans::get_loan_real_estate,
            commands::loans::get_available_loans,
            commands::loans::get_loan_events,
            commands::loans::add_loan_event,
            commands::loans::delete_loan_event,
            // Real estate commands
            commands::real_estate::get_all_real_estate,
            commands::real_estate::get_real_estate,
            commands::real_estate::create_real_estate,
            commands::real_estate::update_real_estate,
            commands::real_estate::delete_real_estate,
            commands::real_estate::get_real_estate_costs,
            commands::real_estate::create_real_estate_cost,
            commands::real_estate::delete_real_estate_cost,
            commands::real_estate::update_real_estate_cost,
            commands::real_estate::get_real_estate_valuations,
            commands::real_estate::add_real_estate_valuation,
            commands::real_estate::delete_real_estate_valuation,
            commands::real_estate::link_loan_to_real_estate,
            commands::real_estate::unlink_loan_from_real_estate,
            commands::real_estate::get_real_estate_loans,
            // Real estate insurance linking commands
            commands::real_estate::link_insurance_to_real_estate,
            commands::real_estate::unlink_insurance_from_real_estate,
            commands::real_estate::get_real_estate_insurances,
            // Real estate photo commands
            commands::real_estate::get_real_estate_photo_batches,
            commands::real_estate::create_photo_batch,
            commands::real_estate::add_photos_to_batch,
            commands::real_estate::update_photo_batch,
            commands::real_estate::delete_photo_batch,
            commands::real_estate::delete_real_estate_photo,
            // Real estate document commands
            commands::real_estate::get_real_estate_documents,
            commands::real_estate::add_real_estate_document,
            commands::real_estate::delete_real_estate_document,
            commands::real_estate::open_real_estate_document,
            // Insurance commands
            commands::insurance::get_all_insurance,
            commands::insurance::get_insurance,
            commands::insurance::create_insurance,
            commands::insurance::update_insurance,
            commands::insurance::delete_insurance,
            commands::insurance::get_insurance_documents,
            commands::insurance::add_insurance_document,
            commands::insurance::delete_insurance_document,
            commands::insurance::open_insurance_document,
            // Insurance real estate linking commands
            commands::insurance::get_insurance_real_estate,
            commands::insurance::get_available_insurances,
            // Other assets commands
            commands::other_assets::get_all_other_assets,
            commands::other_assets::create_other_asset,
            commands::other_assets::update_other_asset,
            commands::other_assets::delete_other_asset,
            commands::other_assets::get_other_asset_transactions,
            commands::other_assets::create_other_asset_transaction,
            commands::other_assets::delete_other_asset_transaction,
            commands::other_assets::get_other_asset_valuations,
            commands::other_assets::add_other_asset_valuation,
            commands::other_assets::delete_other_asset_valuation,
            // Portfolio commands
            commands::portfolio::get_portfolio_metrics,
            commands::portfolio::get_portfolio_history,
            commands::portfolio::record_portfolio_snapshot,
            commands::portfolio::refresh_exchange_rates,
            commands::portfolio::get_exchange_rates,
            commands::portfolio::get_exchange_rates_for_date,
            commands::portfolio::get_exchange_rates_for_date_range,
            commands::portfolio::get_price_status,
            commands::portfolio::start_snapshot_backfill,
            commands::portfolio::recalculate_all_portfolio_history,
            commands::portfolio::backfill_stock_ticker_history,
            commands::portfolio::backfill_crypto_ticker_history,
            commands::portfolio::backfill_currency_breakdowns,
            commands::portfolio::trigger_import_recalculation,
            // Price API commands
            commands::price_api::get_api_keys,
            commands::price_api::set_api_keys,
            commands::price_api::refresh_stock_prices,
            commands::price_api::refresh_crypto_prices,
            commands::price_api::search_crypto,
            commands::price_api::refresh_dividends,
            commands::price_api::search_stock_tickers,
            // Stock Monitor commands
            commands::stock_monitor::get_watched_stocks,
            commands::stock_monitor::follow_stock,
            commands::stock_monitor::get_portfolio_follow_candidates,
            commands::stock_monitor::follow_portfolio_stocks,
            commands::stock_monitor::unfollow_stock,
            commands::stock_monitor::set_watched_target_price,
            commands::stock_monitor::update_watched_notes,
            commands::stock_monitor::refresh_watched_stock_prices,
            commands::stock_monitor::get_stock_monitor_detail,
            commands::stock_monitor::get_stock_price_range,
            // Cashflow commands
            commands::cashflow::get_cashflow_report,
            commands::cashflow::get_cashflow_actuals,
            commands::cashflow::get_all_cashflow_items,
            commands::cashflow::create_cashflow_item,
            commands::cashflow::update_cashflow_item,
            commands::cashflow::delete_cashflow_item,
            // Budgeting commands
            commands::budgeting::get_budgeting_report,
            commands::budgeting::get_category_transactions,
            commands::budgeting::get_budget_goals,
            commands::budgeting::upsert_budget_goal,
            commands::budgeting::delete_budget_goal,
            // Projection commands
            commands::projection::get_projection_settings,
            commands::projection::save_projection_settings,
            commands::projection::calculate_portfolio_projection,
            // Export commands
            commands::export::export_stock_transactions,
            commands::export::export_crypto_transactions,
            commands::export::export_bonds,
            commands::export::export_real_estate,
            commands::export::export_real_estate_costs,
            commands::export::export_insurance_policies,
            commands::export::export_loans,
            commands::export::export_other_assets,
            commands::export::export_other_asset_transactions,
            // Bank account commands
            commands::bank_accounts::get_all_bank_accounts,
            commands::bank_accounts::get_bank_account,
            commands::bank_accounts::create_bank_account,
            commands::bank_accounts::update_bank_account,
            commands::bank_accounts::delete_bank_account,
            commands::bank_accounts::get_all_institutions,
            commands::bank_accounts::create_institution,
            // Interest-rate zone commands
            commands::bank_accounts::get_account_zones,
            commands::bank_accounts::create_account_zone,
            commands::bank_accounts::delete_account_zone,
            // Bank transaction commands
            commands::bank_accounts::get_bank_transactions,
            commands::bank_accounts::create_bank_transaction,
            commands::bank_accounts::delete_bank_transaction,
            commands::bank_accounts::update_transaction_category,
            // Transaction category commands
            commands::categories::get_transaction_categories,
            commands::categories::create_transaction_category,
            commands::categories::update_transaction_category_definition,
            commands::categories::delete_transaction_category,
            commands::categories::get_category_usage,
            // CSV import commands
            commands::bank_accounts::get_csv_presets,
            commands::bank_accounts::get_csv_preset_by_institution,
            commands::bank_accounts::parse_csv_file,
            commands::bank_accounts::preview_csv_import,
            commands::bank_accounts::import_csv_transactions,
            commands::bank_accounts::get_import_batches,
            commands::bank_accounts::delete_import_batch,
            // Stock CSV import commands (services::stock_import)
            commands::stock_import::inspect_stock_csv,
            commands::stock_import::resolve_stock_instruments,
            commands::stock_import::preview_stock_csv_import,
            commands::stock_import::import_stock_csv,
            commands::stock_import::list_stock_import_batches,
            commands::stock_import::undo_stock_import_batch,
            commands::stock_import::list_stock_import_formats,
            commands::stock_import::save_stock_import_format,
            commands::stock_import::delete_stock_import_format,
            // Stock tags commands
            commands::stock_tags::get_all_stock_tags,
            commands::stock_tags::create_stock_tag,
            commands::stock_tags::update_stock_tag,
            commands::stock_tags::delete_stock_tag,
            commands::stock_tags::get_investment_tags,
            commands::stock_tags::set_investment_tags,
            commands::stock_tags::get_stocks_analysis,
            commands::stock_tags::get_tag_metrics,
            // Stock tag group commands
            commands::stock_tags::get_all_stock_tag_groups,
            commands::stock_tags::create_stock_tag_group,
            commands::stock_tags::update_stock_tag_group,
            commands::stock_tags::delete_stock_tag_group,
            // Categorization commands
            commands::categorization::categorize_transaction,
            commands::categorization::categorize_batch,
            commands::categorization::learn_categorization,
            commands::categorization::forget_payee,
            commands::categorization::update_categorization_rules,
            commands::categorization::get_categorization_stats,
            commands::categorization::export_learned_payees,
            commands::categorization::import_learned_payees,
            commands::categorization::load_learned_payees_from_db,
            commands::categorization::load_own_ibans_from_db,
            commands::categorization::load_custom_rules_from_db,
            commands::categorization::get_rule_packs,
            commands::categorization::set_rule_packs_enabled,
            commands::categorization::get_pack_rules,
            commands::categorization::set_pack_rule_disabled,
            commands::categorization::get_rule_hit_counts,
            commands::categorization::preview_rule_matches,
            commands::categorization::apply_rule_to_uncategorized,
            commands::categorization::get_categorization_overview,
            commands::categorization::load_rule_packs_config,
            commands::categorization::accept_category_suggestion,
            commands::categorization::decline_category_suggestion,
            // Categorization rules management commands
            commands::categorization::get_learned_payees_list,
            commands::categorization::delete_learned_payee,
            commands::categorization::delete_learned_payees_bulk,
            commands::categorization::update_learned_payee_category,
            commands::categorization::get_custom_rules,
            commands::categorization::create_custom_rule,
            commands::categorization::update_custom_rule,
            commands::categorization::delete_custom_rule,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
