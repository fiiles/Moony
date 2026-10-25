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
use tauri::Manager;

/// Marker in the app data dir: the one-time temp cleanup already ran.
const TEMP_CLEANUP_MARKER: &str = "legacy-temp-cleanup.done";

/// Remove the plaintext photo copies that Moony up to 0.12.2 left in the OS temp dir, once, off
/// the main thread. Best effort: any problem is logged and never blocks startup.
fn spawn_legacy_temp_cleanup(app: tauri::AppHandle) {
    let (temp_dir, data_dir) = match (app.path().temp_dir(), app.path().app_data_dir()) {
        (Ok(temp_dir), Ok(data_dir)) => (temp_dir, data_dir),
        (temp_dir, data_dir) => {
            log::warn!(
                "[TEMP] Leftover photo cleanup skipped: temp dir ok = {}, app data dir ok = {}",
                temp_dir.is_ok(),
                data_dir.is_ok()
            );
            return;
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        services::temp_cleanup::run_once_and_log(&temp_dir, &data_dir.join(TEMP_CLEANUP_MARKER));
    });
}

/// Phones only: remove what an export that was interrupted (the app killed while the Files picker
/// was open) left in Documents, a staged backup archive or the recovery key's text, once at
/// startup and off the main thread. Best effort: a problem is logged and never blocks startup.
#[cfg(mobile)]
fn spawn_staged_export_sweep(app: tauri::AppHandle) {
    let document_dir = match app.path().document_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[BACKUP] Staged export sweep skipped: no documents dir: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        match services::backup::sweep_staged_exports(&document_dir) {
            Ok(count) => log::info!("[BACKUP] Staged export sweep removed {count} file(s)"),
            Err(e) => log::warn!("[BACKUP] Staged export sweep failed: {e}"),
        }
    });
}

/// Phones only: remove WebKit's plaintext copies of the files picked in the upload sheets (the
/// `WKFileUploadPanel-*` folders in the container's tmp), once at startup and off the main thread.
/// It never runs after an upload (another picked file may still be waiting in an open sheet); the
/// lock runs it again (`logout`). Best effort: a problem is logged and never blocks startup.
#[cfg(mobile)]
fn spawn_upload_copy_sweep(app: tauri::AppHandle) {
    let temp_dir = match app.path().temp_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[UPLOAD] Upload copy sweep skipped: no temp dir: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let removed = services::upload_copies::sweep(&temp_dir);
        log::info!("[UPLOAD] Upload copy sweep removed {removed} file(s)");
    });
}

/// Phones only: keep the profile folder, the webview's storage and the received files folder out of
/// iCloud and computer backups, at every start and off the main thread (the folders are created
/// first on a first launch). Best effort: a problem is logged and never blocks startup.
#[cfg(mobile)]
fn spawn_device_backup_exclusion(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        commands::device_backup::exclude_at_start(&app).await;
    });
}

/// Desktop `main` and the generated iOS / Android entry points all start here. The body runs
/// inside Tauri's own tokio runtime (`async_runtime::block_on`), the same multi-thread runtime
/// the commands and `tokio::spawn` calls use, so nothing changes for the desktop; a
/// `#[tokio::main]` wrapper would hide the future from `mobile_entry_point`, which expects
/// either a plain or an `async` function.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::async_runtime::block_on(run_app());
}

async fn run_app() {
    // Panics go to the log file too (there is no console in a release build)
    logging::install_panic_hook();

    // Initialize categorization engine with default rules
    let categorization_engine = Arc::new(CategorizationEngine::new_empty());

    // The log plugin goes first so every other plugin's `log` output is captured.
    let mut builder = tauri::Builder::default()
        .plugin(logging::plugin())
        .setup(|app| {
            logging::log_startup_banner();
            spawn_legacy_temp_cleanup(app.handle().clone());
            #[cfg(mobile)]
            spawn_staged_export_sweep(app.handle().clone());
            #[cfg(mobile)]
            spawn_upload_copy_sweep(app.handle().clone());
            #[cfg(mobile)]
            spawn_device_backup_exclusion(app.handle().clone());
            // The window starts hidden and the frontend shows it once the theme is stamped (spec D10);
            // if the frontend never does, show it anyway so a broken bundle is not an invisible app.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if let Some(window) = handle.get_webview_window("main") {
                    // A window the user minimized in those 3 s also reports not visible on macOS.
                    let minimized = window.is_minimized().unwrap_or(false);
                    if !minimized && !window.is_visible().unwrap_or(true) {
                        let _ = window.show();
                    }
                }
            });
            // Decrypted copies opened in other apps never outlive a run (D3).
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                commands::attachments::wipe_open_copies(&handle)
            });
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());

    // Updates and the relaunch after one exist on the desktop only.
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    // Phone-only hardware: haptic feedback, local notifications, and the keychain (biometric
    // unlock: the DB key behind biometry; privacy screen on from launch).
    #[cfg(mobile)]
    {
        builder = builder
            .plugin(tauri_plugin_haptics::init())
            // The phone's reminder notifications (Nastavení → Oznámení) are scheduled through it.
            .plugin(tauri_plugin_notification::init())
            .plugin(moony_keychain::init())
            // The files the system opened in Moony, until the webview asks for them (a cold start
            // from the share sheet delivers them before the webview listens).
            .manage(services::received_files::ReceivedFiles::default());
    }

    builder
        .manage(Database::new())
        .manage(services::auth::PendingSetup::default())
        .manage(services::session_key::SessionKey::default())
        .manage(services::history_recalc::HistoryRecalc::new())
        .manage(CategorizationState(categorization_engine))
        .manage(LocalApiServer::new())
        .manage(commands::attachments::AttachmentConversion::default())
        // Attachment bytes reach the webview only through this scheme (ADR 0017).
        .register_asynchronous_uri_scheme_protocol(
            services::attachment_protocol::SCHEME,
            |ctx, request, responder| {
                let app = ctx.app_handle().clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let origin = request
                        .headers()
                        .get("Origin")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned);
                    let response = match app.path().app_data_dir() {
                        Ok(data_dir) => services::attachment_protocol::respond(
                            &app.state::<Database>(),
                            &data_dir,
                            request.method().as_str(),
                            request.uri().path(),
                            origin.as_deref(),
                        ),
                        Err(e) => services::attachment_protocol::response_for(
                            Err(error::AppError::Internal(e.to_string())),
                            origin.as_deref(),
                        ),
                    };
                    responder.respond(response);
                });
            },
        )
        .invoke_handler(tauri::generate_handler![
            // Attachments (services::attachments)
            commands::attachments::export_attachment,
            commands::attachments::export_photo_batch,
            commands::attachments::get_attachment_status,
            commands::attachments::reveal_attachment_leftover,
            // Onboarding (services::onboarding)
            commands::onboarding::get_onboarding_progress,
            commands::onboarding::set_onboarding_flag,
            // Milestones (services::milestones)
            commands::milestones::get_milestones,
            commands::milestones::set_milestone_state,
            commands::milestones::clear_milestone_state,
            commands::milestones::get_milestone_muted_kinds,
            commands::milestones::set_milestone_muted_kinds,
            // Stock splits (services::stock_splits)
            commands::stock_splits::list_stock_splits,
            commands::stock_splits::apply_stock_split,
            commands::stock_splits::revert_stock_split,
            commands::stock_splits::dismiss_stock_split,
            commands::stock_splits::record_stock_split,
            commands::stock_splits::delete_stock_split,
            // Backup / restore / export (services::backup)
            commands::backup::create_backup,
            commands::backup::inspect_backup,
            commands::backup::restore_backup,
            commands::backup::export_all_data,
            commands::backup::verify_database,
            commands::backup::get_data_location,
            commands::backup::open_data_folder,
            // Exports through the phone's Files picker (staged in Documents, removed afterwards)
            #[cfg(mobile)]
            commands::backup::stage_backup_export,
            #[cfg(mobile)]
            commands::backup::finish_backup_export,
            #[cfg(mobile)]
            commands::backup::stage_text_export,
            #[cfg(mobile)]
            commands::backup::discard_text_export,
            // Statement files the system opened in Moony (services::received_files)
            #[cfg(mobile)]
            commands::received_files::take_received_files,
            #[cfg(mobile)]
            commands::received_files::discard_received_file,
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
            // Biometric unlock on the phone (services::biometric_unlock)
            #[cfg(mobile)]
            commands::biometric::biometric_unlock_status,
            #[cfg(mobile)]
            commands::biometric::enable_biometric_unlock,
            #[cfg(mobile)]
            commands::biometric::disable_biometric_unlock,
            #[cfg(mobile)]
            commands::biometric::unlock_with_biometrics,
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
            commands::investments::set_manual_price,
            commands::investments::delete_manual_price,
            commands::investments::set_manual_dividend,
            commands::investments::delete_manual_dividend,
            commands::investments::get_stock_value_history,
            commands::investments::get_stock_performance,
            commands::investments::get_stock_benchmark,
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
            commands::real_estate::get_all_real_estate_valuations,
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
            commands::real_estate::add_photo_to_batch,
            commands::real_estate::update_photo_batch,
            commands::real_estate::delete_photo_batch,
            commands::real_estate::delete_real_estate_photo,
            // Real estate document commands
            commands::real_estate::get_real_estate_documents,
            commands::real_estate::add_real_estate_document,
            commands::real_estate::add_real_estate_document_bytes,
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
            commands::insurance::add_insurance_document_bytes,
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
            commands::portfolio::get_dated_facts,
            commands::portfolio::record_portfolio_snapshot,
            commands::portfolio::refresh_exchange_rates,
            commands::portfolio::get_price_status,
            commands::portfolio::get_fx_snapshot,
            commands::portfolio::get_fx_history,
            commands::portfolio::start_snapshot_backfill,
            commands::portfolio::recalculate_all_portfolio_history,
            commands::portfolio::backfill_stock_ticker_history,
            commands::portfolio::backfill_crypto_ticker_history,
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
            // Company data (services::company_info)
            commands::company_info::get_stock_company_info,
            // Cashflow commands
            commands::cashflow::get_cashflow_report,
            commands::cashflow::get_cashflow_actuals,
            commands::cashflow::get_report_transactions,
            commands::cashflow::get_effective_transaction_flows,
            commands::cashflow::get_all_cashflow_items,
            commands::cashflow::create_cashflow_item,
            commands::cashflow::update_cashflow_item,
            commands::cashflow::delete_cashflow_item,
            commands::cashflow::set_cashflow_plan_inclusion,
            // Budgeting commands
            commands::budgeting::get_budgeting_report,
            commands::budgeting::get_category_transactions,
            commands::budgeting::get_budget_goals,
            commands::budgeting::get_budget_goal_history,
            commands::budgeting::delete_budget_goal_intervals,
            commands::budgeting::get_latest_transaction_date,
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
            commands::bank_accounts::update_transaction_flow,
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
            commands::bank_accounts::infer_csv_columns,
            commands::bank_accounts::list_learned_csv_formats,
            commands::bank_accounts::find_learned_csv_format,
            commands::bank_accounts::save_learned_csv_format,
            commands::bank_accounts::rename_learned_csv_format,
            commands::bank_accounts::delete_learned_csv_format,
            commands::bank_accounts::mark_learned_csv_format_used,
            commands::bank_accounts::preview_csv_import,
            commands::bank_accounts::import_csv_transactions,
            commands::bank_accounts::get_import_batches,
            commands::bank_accounts::delete_import_batch,
            commands::bank_accounts::get_balance_records,
            commands::bank_accounts::record_bank_account_balance,
            commands::bank_accounts::delete_balance_record,
            // Statement import commands (services::statement_import)
            commands::statement_import::inspect_statement_file,
            commands::statement_import::preview_statement_import,
            commands::statement_import::import_statement_file,
            // Stock CSV import commands (services::stock_import)
            commands::stock_import::inspect_stock_csv,
            commands::stock_import::resolve_stock_instruments,
            commands::stock_import::preview_stock_csv_import,
            commands::stock_import::check_stock_splits,
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
            commands::categorization::preview_recategorization,
            commands::categorization::retarget_custom_rule,
            commands::categorization::preview_learned_rule_history,
            commands::categorization::apply_learned_rule_to_history,
            commands::categorization::restore_transaction_categories,
            commands::categorization::get_categorize_queue,
            commands::categorization::snooze_categorization,
            commands::categorization::get_custom_rules,
            commands::categorization::create_custom_rule,
            commands::categorization::update_custom_rule,
            commands::categorization::delete_custom_rule,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            tauri::RunEvent::Exit => commands::attachments::wipe_open_copies(app),
            // A statement shared or opened into the phone app (iOS and Android)
            #[cfg(mobile)]
            tauri::RunEvent::Opened { urls } => commands::received_files::on_opened(app, &urls),
            _ => {}
        });
}
