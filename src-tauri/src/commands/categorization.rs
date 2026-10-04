//! Tauri commands for the Smart Categorization Engine
//!
//! These commands provide the IPC interface for:
//! - Categorizing individual and batch transactions
//! - Learning from user corrections
//! - Managing categorization rules

use std::sync::Arc;

use rusqlite::OptionalExtension;
use tauri::State;
use uuid::Uuid;

use crate::db::Database;
use crate::services::categorization::learn;
use crate::services::categorization::{
    CategorizationEngine, CategorizationResult, CategorizationRule, TransactionInput,
};

/// State wrapper for thread-safe engine access
pub struct CategorizationState(pub Arc<CategorizationEngine>);

/// Categorize a single transaction using the waterfall approach
#[tauri::command]
pub async fn categorize_transaction(
    state: State<'_, CategorizationState>,
    transaction: TransactionInput,
) -> Result<CategorizationResult, String> {
    // Run categorization on background thread to avoid blocking
    let engine = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || engine.categorize(&transaction))
        .await
        .map_err(|e| format!("Categorization task panicked: {}", e))
}

/// Categorize multiple transactions in batch
#[tauri::command]
pub async fn categorize_batch(
    state: State<'_, CategorizationState>,
    transactions: Vec<TransactionInput>,
) -> Result<Vec<CategorizationResult>, String> {
    let engine = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || engine.categorize_batch(&transactions))
        .await
        .map_err(|e| format!("Batch categorization panicked: {}", e))
}

/// Learn from user's manual categorization with hierarchical matching
///
/// This stores the payee/iban → category mapping for instant future lookups
/// AND persists it to the database for future app sessions.
///
/// Hierarchical priority:
/// - iban = IBAN default (catches any payee for this iban)
/// - payee = payee default (catches any iban for this payee)
#[tauri::command]
pub async fn learn_categorization(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    payee: Option<String>,
    counterparty_iban: Option<String>,
    category_id: String,
) -> Result<(), String> {
    db.with_conn(|conn| {
        crate::services::categorization::learn::learn_payee(
            conn,
            &state.0,
            payee.as_deref(),
            counterparty_iban.as_deref(),
            &category_id,
        )
    })
    .map_err(|e| format!("Failed to persist learned payee: {}", e))?;

    log::debug!(
        "Learned and persisted: payee={:?}, iban={:?} → {}",
        payee,
        counterparty_iban,
        category_id
    );
    Ok(())
}

/// Accept a pending MCP category suggestion on a transaction.
///
/// Atomically promotes `suggested_category_id` to `category_id` (stamping
/// `categorization_source = 'mcp'` — the category is LLM-derived and
/// human-confirmed), clears the suggestion, and teaches the learned-payee
/// layer from the row's counterparty. Returns the accepted category id.
#[tauri::command]
pub async fn accept_category_suggestion(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    transaction_id: String,
) -> Result<String, String> {
    let accepted = db
        .with_conn(|conn| {
            let row: Option<(String, Option<String>, Option<String>)> = conn
                .query_row(
                    "SELECT suggested_category_id, counterparty_name, counterparty_iban
                     FROM bank_transactions
                     WHERE id = ?1 AND suggested_category_id IS NOT NULL",
                    [&transaction_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;

            let Some((category_id, payee, iban)) = row else {
                return Ok(None);
            };

            conn.execute(
                "UPDATE bank_transactions
                 SET category_id = suggested_category_id,
                     categorization_source = 'mcp',
                     suggested_category_id = NULL
                 WHERE id = ?1",
                [&transaction_id],
            )?;

            // Confirming is the human correction that trains the engine
            // (spec 2026-08-27 D5).
            crate::services::categorization::learn::learn_payee(
                conn,
                &state.0,
                payee.as_deref(),
                iban.as_deref(),
                &category_id,
            )?;
            Ok(Some(category_id))
        })
        .map_err(|e| format!("Failed to accept suggestion: {}", e))?;

    accepted.ok_or_else(|| "No pending suggestion on this transaction".to_string())
}

/// Decline a pending MCP category suggestion (clears it, learns nothing).
#[tauri::command]
pub async fn decline_category_suggestion(
    db: State<'_, Database>,
    transaction_id: String,
) -> Result<(), String> {
    db.with_conn(|conn| {
        conn.execute(
            "UPDATE bank_transactions SET suggested_category_id = NULL WHERE id = ?1",
            [&transaction_id],
        )?;
        Ok(())
    })
    .map_err(|e| format!("Failed to decline suggestion: {}", e))?;
    Ok(())
}

/// Forget a learned payee mapping
///
/// Removes from both in-memory engine AND database.
/// Supports hierarchical forget: specify all attributes that were used during learn.
#[tauri::command]
pub async fn forget_payee(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    payee: Option<String>,
    counterparty_iban: Option<String>,
) -> Result<bool, String> {
    // Remove from in-memory engine
    let forgotten = state
        .0
        .forget_payee(payee.as_deref(), counterparty_iban.as_deref());

    if forgotten {
        // Also remove from database: every row that normalizes to the same key
        db.with_conn(|conn| {
            learn::forget_learned_payee(conn, payee.as_deref(), counterparty_iban.as_deref())
        })
        .map_err(|e| format!("Failed to delete from database: {}", e))?;

        log::debug!(
            "Forgot and removed from DB: payee={:?}, iban={:?}",
            payee,
            counterparty_iban
        );
    }
    Ok(forgotten)
}

/// Update categorization rules (after user edits)
#[tauri::command]
pub async fn update_categorization_rules(
    state: State<'_, CategorizationState>,
    rules: Vec<CategorizationRule>,
) -> Result<(), String> {
    state.0.update_rules(rules);
    log::info!("Updated categorization rules");
    Ok(())
}

/// Get engine statistics
#[tauri::command]
pub async fn get_categorization_stats(
    state: State<'_, CategorizationState>,
) -> Result<CategorizationStats, String> {
    let stats = state.0.stats();
    Ok(CategorizationStats {
        active_rules: stats.active_rules,
        learned_payees: stats.learned_payees,
    })
}

/// Export learned payees for persistence
#[tauri::command]
pub async fn export_learned_payees(
    state: State<'_, CategorizationState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    Ok(state.0.export_learned_payees())
}

/// Import learned payees
#[tauri::command]
pub async fn import_learned_payees(
    state: State<'_, CategorizationState>,
    payees: std::collections::HashMap<String, String>,
) -> Result<usize, String> {
    let count = payees.len();
    state.0.import_learned_payees(payees);
    log::info!("Imported {} learned payees", count);
    Ok(count)
}

/// Load learned payees from the database
///
/// This should be called after app unlock to populate the in-memory engine
/// with previously learned payee → category mappings. Rows are normalized and
/// de-duplicated on load (newest wins), see `learn::load_learned_payees`.
#[tauri::command]
pub async fn load_learned_payees_from_db(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
) -> Result<usize, String> {
    let count = db
        .with_conn(|conn| learn::load_learned_payees(conn, &state.0))
        .map_err(|e| format!("Failed to load learned payees: {}", e))?;
    if count > 0 {
        log::info!("Loaded {} learned payees from database", count);
    }
    Ok(count)
}

/// Load user's own IBANs from bank accounts for internal transfer detection
///
/// This should be called after app unlock to enable automatic detection of
/// transactions between user's own accounts as "Internal Transfers".
#[tauri::command]
pub async fn load_own_ibans_from_db(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
) -> Result<usize, String> {
    let count = db
        .with_conn(|conn| {
            crate::services::categorization::own_accounts::refresh_own_ibans(conn, &state.0)
        })
        .map_err(|e| format!("Failed to load IBANs: {}", e))?;

    log::info!("Loaded {} own IBANs for internal transfer detection", count);
    Ok(count)
}

/// (Re)load the user's custom rules from the database into the engine.
/// Called after unlock and after every custom-rule write so a rule created
/// in the UI applies immediately instead of after the next restart.
pub(crate) fn reload_custom_rules(
    state: &CategorizationState,
    db: &Database,
) -> Result<usize, String> {
    use crate::services::categorization::types::RuleType;

    // Load custom rules from database (only non-system rules)
    let custom_rules = db
        .with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, rule_type, pattern, category_id, priority, is_active, stop_processing, iban_pattern
                 FROM categorization_rules
                 WHERE is_active = 1 AND is_system = 0
                 ORDER BY priority DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                let rule_type_str: String = row.get(2)?;
                let rule_type: RuleType = rule_type_str.parse().unwrap_or(RuleType::Contains);

                Ok(CategorizationRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    rule_type,
                    pattern: row.get(3)?,
                    category_id: row.get(4)?,
                    priority: row.get(5)?,
                    is_active: row.get::<_, i32>(6)? != 0,
                    stop_processing: row.get::<_, i32>(7)? != 0,
                    iban_pattern: row.get(8)?,
                })
            })?;
            let rules: Vec<CategorizationRule> = rows.filter_map(|r| r.ok()).collect();
            Ok(rules)
        })
        .map_err(|e| format!("Failed to load custom rules: {}", e))?;

    let count = custom_rules.len();

    // Update the custom rules engine (separate from default rules)
    // Custom rules take priority over learned payees in the waterfall
    state.0.update_custom_rules(custom_rules);

    log::info!("Loaded {} custom rules from database", count);
    Ok(count)
}

/// Load custom categorization rules from the database
///
/// This should be called after app unlock to populate the in-memory engine
/// with user-defined custom rules. Custom rules are loaded into separate engine
/// and take priority over learned payees in the waterfall.
#[tauri::command]
pub async fn load_custom_rules_from_db(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
) -> Result<usize, String> {
    reload_custom_rules(&state, &db)
}

// ==================== DTOs ====================

/// Statistics about the categorization engine
#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CategorizationStats {
    pub active_rules: usize,
    pub learned_payees: usize,
}

// ==================== Locale Rule Packs ====================

use crate::services::categorization::packs;

/// One shipped rule pack, with its enabled flag from app_config
#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RulePackInfo {
    pub pack_id: String,
    pub country: String,
    pub version: u32,
    pub rule_count: usize,
    pub enabled: bool,
}

/// One pack rule, with its disabled flag from app_config
#[derive(serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PackRuleInfo {
    pub id: String,
    pub name: String,
    pub rule_type: String,
    pub pattern: String,
    pub category_id: String,
    pub priority: i32,
    pub disabled: bool,
}

/// Rebuild the engine's default-rule layer from the persisted pack config.
fn apply_pack_config(
    engine: &CategorizationEngine,
    conn: &rusqlite::Connection,
) -> Result<usize, String> {
    let (enabled, disabled) =
        packs::read_config(conn).map_err(|e| format!("Failed to read pack config: {}", e))?;
    let rules = packs::rules_for(&enabled, &disabled);
    let count = rules.len();
    engine.update_rules(rules);
    Ok(count)
}

/// List all shipped rule packs with their enabled state
#[tauri::command]
pub async fn get_rule_packs(db: State<'_, Database>) -> Result<Vec<RulePackInfo>, String> {
    let (enabled, _) = db
        .with_conn(packs::read_config)
        .map_err(|e| format!("Failed to read pack config: {}", e))?;

    Ok(packs::available_packs()
        .iter()
        .map(|pack| RulePackInfo {
            pack_id: pack.pack_id.clone(),
            country: pack.country.clone(),
            version: pack.version,
            rule_count: pack.rules.len(),
            enabled: enabled.iter().any(|id| id == &pack.pack_id),
        })
        .collect())
}

/// Persist the enabled pack set and rebuild the engine's default rules
#[tauri::command]
pub async fn set_rule_packs_enabled(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    pack_ids: Vec<String>,
) -> Result<usize, String> {
    db.with_conn(|conn| packs::write_enabled(conn, &pack_ids))
        .map_err(|e| format!("Failed to persist pack selection: {}", e))?;
    let count = db
        .with_conn(|conn| Ok(apply_pack_config(&state.0, conn)))
        .map_err(|e| format!("Failed to open database: {}", e))??;
    log::info!("Enabled packs updated ({} rules active)", count);
    Ok(count)
}

/// List the rules of one pack with their disabled flags
#[tauri::command]
pub async fn get_pack_rules(
    db: State<'_, Database>,
    pack_id: String,
) -> Result<Vec<PackRuleInfo>, String> {
    let (_, disabled) = db
        .with_conn(packs::read_config)
        .map_err(|e| format!("Failed to read pack config: {}", e))?;

    let pack = packs::available_packs()
        .iter()
        .find(|p| p.pack_id == pack_id)
        .ok_or_else(|| format!("Unknown pack: {}", pack_id))?;

    Ok(pack
        .rules
        .iter()
        .map(|rule| PackRuleInfo {
            id: rule.id.clone(),
            name: rule.name.clone(),
            rule_type: rule.rule_type.to_string(),
            pattern: rule.pattern.clone(),
            category_id: rule.category_id.clone(),
            priority: rule.priority,
            disabled: disabled.contains(&rule.id),
        })
        .collect())
}

/// Persist one pack rule's disabled flag and rebuild the engine's default rules
#[tauri::command]
pub async fn set_pack_rule_disabled(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    rule_id: String,
    disabled: bool,
) -> Result<(), String> {
    db.with_conn(|conn| packs::write_rule_disabled(conn, &rule_id, disabled))
        .map_err(|e| format!("Failed to persist rule flag: {}", e))?;
    db.with_conn(|conn| Ok(apply_pack_config(&state.0, conn)))
        .map_err(|e| format!("Failed to open database: {}", e))??;
    Ok(())
}

/// Load the persisted pack configuration into the engine (call after unlock)
#[tauri::command]
pub async fn load_rule_packs_config(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
) -> Result<usize, String> {
    let count = db
        .with_conn(|conn| Ok(apply_pack_config(&state.0, conn)))
        .map_err(|e| format!("Failed to open database: {}", e))??;
    log::info!("Loaded {} pack rules into the engine", count);
    Ok(count)
}

// ==================== Learned Payees Management ====================

/// Learned payee entry for display in the management UI
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LearnedPayeeEntry {
    pub id: String,
    /// Rule type: "payee_default", "iban_only_default"
    pub rule_type: String,
    pub normalized_payee: Option<String>,
    pub original_payee: Option<String>,
    pub counterparty_iban: Option<String>,
    pub category_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Get all learned payees for management UI
///
/// One entry per normalized payee + IBAN (the newest row), carrying the
/// normalized key — the text that is actually matched.
#[tauri::command]
pub async fn get_learned_payees_list(
    db: State<'_, Database>,
) -> Result<Vec<LearnedPayeeEntry>, String> {
    db.with_conn(learn::list_learned_payees)
        .map(|rows| {
            rows.into_iter()
                .map(|row| {
                    // Determine rule type based on which fields are present
                    let rule_type = match (&row.normalized_payee, &row.counterparty_iban) {
                        (Some(_), Some(_)) => "iban_default",
                        (Some(_), None) => "payee_default",
                        (None, Some(_)) => "iban_only_default",
                        (None, None) => "unknown",
                    };
                    LearnedPayeeEntry {
                        id: row.id,
                        rule_type: rule_type.to_string(),
                        normalized_payee: row.normalized_payee,
                        original_payee: row.original_payee,
                        counterparty_iban: row.counterparty_iban,
                        category_id: row.category_id,
                        created_at: row.created_at,
                        updated_at: row.updated_at,
                    }
                })
                .collect()
        })
        .map_err(|e| format!("Failed to load learned payees: {}", e))
}

/// Delete a learned payee by ID
///
/// Removes every stored row that shares the listed rule's normalized key, so
/// stale duplicates cannot bring it back at the next start.
#[tauri::command]
pub async fn delete_learned_payee(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    id: String,
) -> Result<(), String> {
    let forgotten = db
        .with_conn(|conn| learn::delete_learned_payee(conn, &id))
        .map_err(|e| format!("Failed to delete payee: {}", e))?;

    if let Some(key) = forgotten {
        // Remove from in-memory engine
        state
            .0
            .forget_payee(key.payee.as_deref(), key.iban.as_deref());
        log::info!("Deleted learned payee: {}", id);
    }

    Ok(())
}

/// Bulk delete learned payees by IDs
#[tauri::command]
pub async fn delete_learned_payees_bulk(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    ids: Vec<String>,
) -> Result<usize, String> {
    let mut count = 0;
    for id in &ids {
        // A listed rule stands for every stored row with the same normalized
        // key, so an id deleted by an earlier iteration is simply unknown now.
        let forgotten = db
            .with_conn(|conn| learn::delete_learned_payee(conn, id))
            .map_err(|e| format!("Failed to delete payees: {}", e))?;
        if let Some(key) = forgotten {
            state
                .0
                .forget_payee(key.payee.as_deref(), key.iban.as_deref());
            count += 1;
        }
    }

    log::info!("Bulk deleted {} learned payees", count);
    Ok(count)
}

/// Update the category of an existing learned payee
#[tauri::command]
pub async fn update_learned_payee_category(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    id: String,
    category_id: String,
) -> Result<(), String> {
    let key = db
        .with_conn(|conn| learn::set_learned_payee_category(conn, &id, &category_id))
        .map_err(|e| format!("Failed to update payee category: {}", e))?;

    if let Some(key) = key {
        // Update in-memory engine
        state
            .0
            .learn_from_user(key.payee.as_deref(), key.iban.as_deref(), &category_id);
        log::info!("Updated learned payee {} to category {}", id, category_id);
    }

    Ok(())
}

// ==================== Custom Rules Management ====================

/// Custom rule entry from database
#[derive(Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomRule {
    pub id: String,
    pub name: String,
    pub rule_type: String,
    pub pattern: String,
    pub category_id: String,
    pub priority: i32,
    pub is_active: bool,
    pub stop_processing: bool,
    pub is_system: bool,
    pub created_at: i64,
    pub iban_pattern: Option<String>,
}

/// Input for creating/updating custom rules
#[derive(Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CustomRuleInput {
    pub name: String,
    pub rule_type: String,
    pub pattern: String,
    pub category_id: String,
    pub priority: i32,
    pub is_active: bool,
    pub stop_processing: bool,
    pub iban_pattern: Option<String>,
}

/// Get all custom categorization rules
#[tauri::command]
pub async fn get_custom_rules(db: State<'_, Database>) -> Result<Vec<CustomRule>, String> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, rule_type, pattern, category_id, priority, is_active, stop_processing, is_system, created_at, iban_pattern
             FROM categorization_rules
             ORDER BY priority DESC, created_at ASC"
        )?;

        let entries = stmt.query_map([], |row| {
            Ok(CustomRule {
                id: row.get(0)?,
                name: row.get(1)?,
                rule_type: row.get(2)?,
                pattern: row.get(3)?,
                category_id: row.get(4)?,
                priority: row.get(5)?,
                is_active: row.get::<_, i32>(6)? != 0,
                stop_processing: row.get::<_, i32>(7)? != 0,
                is_system: row.get::<_, i32>(8)? != 0,
                created_at: row.get(9)?,
                iban_pattern: row.get(10)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

        Ok(entries)
    }).map_err(|e| format!("Failed to load custom rules: {}", e))
}

/// Create a new custom categorization rule
#[tauri::command]
pub async fn create_custom_rule(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    data: CustomRuleInput,
) -> Result<CustomRule, String> {
    let id = Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id, priority, is_active, stop_processing, is_system, created_at, iban_pattern)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
            rusqlite::params![
                &id,
                &data.name,
                &data.rule_type,
                &data.pattern,
                &data.category_id,
                data.priority,
                data.is_active as i32,
                data.stop_processing as i32,
                now,
                &data.iban_pattern,
            ],
        )?;
        Ok(())
    }).map_err(|e| format!("Failed to create rule: {}", e))?;

    log::debug!("Created custom rule: {} ({})", data.name, id);
    reload_custom_rules(&state, &db)?;

    Ok(CustomRule {
        id,
        name: data.name,
        rule_type: data.rule_type,
        pattern: data.pattern,
        category_id: data.category_id,
        priority: data.priority,
        is_active: data.is_active,
        stop_processing: data.stop_processing,
        is_system: false,
        created_at: now,
        iban_pattern: data.iban_pattern,
    })
}

/// Update an existing custom categorization rule
#[tauri::command]
pub async fn update_custom_rule(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    id: String,
    data: CustomRuleInput,
) -> Result<CustomRule, String> {
    // Check if rule is system rule (cannot be edited)
    let is_system = db
        .with_conn(|conn| {
            let val = conn.query_row(
                "SELECT is_system FROM categorization_rules WHERE id = ?1",
                [&id],
                |row| row.get::<_, i32>(0),
            )?;
            Ok(val)
        })
        .map_err(|e| format!("Failed to find rule: {}", e))?;

    if is_system != 0 {
        return Err("Cannot modify system rules".to_string());
    }

    db.with_conn(|conn| {
        conn.execute(
            "UPDATE categorization_rules
             SET name = ?1, rule_type = ?2, pattern = ?3, category_id = ?4, priority = ?5, is_active = ?6, stop_processing = ?7, iban_pattern = ?8
             WHERE id = ?9 AND is_system = 0",
            rusqlite::params![
                &data.name,
                &data.rule_type,
                &data.pattern,
                &data.category_id,
                data.priority,
                data.is_active as i32,
                data.stop_processing as i32,
                &data.iban_pattern,
                &id,
            ],
        )?;
        Ok(())
    }).map_err(|e| format!("Failed to update rule: {}", e))?;

    let created_at = db
        .with_conn(|conn| {
            let val = conn.query_row(
                "SELECT created_at FROM categorization_rules WHERE id = ?1",
                [&id],
                |row| row.get::<_, i64>(0),
            )?;
            Ok(val)
        })
        .unwrap_or(0);

    log::debug!("Updated custom rule: {} ({})", data.name, id);

    reload_custom_rules(&state, &db)?;

    Ok(CustomRule {
        id,
        name: data.name,
        rule_type: data.rule_type,
        pattern: data.pattern,
        category_id: data.category_id,
        priority: data.priority,
        is_active: data.is_active,
        stop_processing: data.stop_processing,
        is_system: false,
        created_at,
        iban_pattern: data.iban_pattern,
    })
}

/// Delete a custom categorization rule
#[tauri::command]
pub async fn delete_custom_rule(
    state: State<'_, CategorizationState>,
    db: State<'_, Database>,
    id: String,
) -> Result<(), String> {
    // Check if rule is system rule (cannot be deleted)
    let is_system = db
        .with_conn(|conn| {
            let val = conn.query_row(
                "SELECT is_system FROM categorization_rules WHERE id = ?1",
                [&id],
                |row| row.get::<_, i32>(0),
            )?;
            Ok(val)
        })
        .map_err(|e| format!("Failed to find rule: {}", e))?;

    if is_system != 0 {
        return Err("Cannot delete system rules".to_string());
    }

    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM categorization_rules WHERE id = ?1 AND is_system = 0",
            [&id],
        )?;
        Ok(())
    })
    .map_err(|e| format!("Failed to delete rule: {}", e))?;

    log::info!("Deleted custom rule: {}", id);
    reload_custom_rules(&state, &db)?;
    Ok(())
}

// ==================== Rule statistics (redesign phase 2, backend D) ====================

use crate::services::categorization::hits;

/// A stored or drafted rule as the engine sees it.
fn rule_from_input(id: &str, data: &CustomRuleInput) -> crate::error::Result<CategorizationRule> {
    let rule_type = data
        .rule_type
        .parse()
        .map_err(|_| crate::error::AppError::Validation("validation.ruleTypeInvalid".into()))?;
    Ok(CategorizationRule {
        id: id.to_string(),
        name: data.name.clone(),
        rule_type,
        pattern: data.pattern.clone(),
        category_id: data.category_id.clone(),
        priority: data.priority,
        is_active: data.is_active,
        stop_processing: data.stop_processing,
        iban_pattern: data.iban_pattern.clone().filter(|p| !p.trim().is_empty()),
    })
}

/// Hits per rule (custom, enabled packs, learned) over the last `days` days.
#[tauri::command]
pub async fn get_rule_hit_counts(
    db: State<'_, Database>,
    days: i64,
) -> crate::error::Result<Vec<hits::RuleHitCount>> {
    db.with_conn(|conn| hits::rule_hit_counts(conn, days))
}

/// What a draft rule would match today (the rule editor's live preview).
#[tauri::command]
pub async fn preview_rule_matches(
    db: State<'_, Database>,
    rule: CustomRuleInput,
    days: i64,
) -> crate::error::Result<hits::RuleMatchPreview> {
    let rule = rule_from_input("preview", &rule)?;
    db.with_conn(|conn| hits::preview_rule_matches(conn, &rule, days))
}

/// Give a stored rule's category to the matching uncategorized transactions
/// of the period ("Zařadit nezařazené hned"). Returns the rows changed.
#[tauri::command]
pub async fn apply_rule_to_uncategorized(
    db: State<'_, Database>,
    rule_id: String,
    days: i64,
) -> crate::error::Result<usize> {
    db.with_conn(|conn| {
        let rule = conn
            .query_row(
                "SELECT name, rule_type, pattern, category_id, priority, is_active, stop_processing, iban_pattern
                 FROM categorization_rules WHERE id = ?1",
                [&rule_id],
                |row| {
                    Ok(CustomRuleInput {
                        name: row.get(0)?,
                        rule_type: row.get(1)?,
                        pattern: row.get(2)?,
                        category_id: row.get(3)?,
                        priority: row.get(4)?,
                        is_active: row.get::<_, i32>(5)? != 0,
                        stop_processing: row.get::<_, i32>(6)? != 0,
                        iban_pattern: row.get(7)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| crate::error::AppError::NotFound("Rule not found".into()))?;
        let rule = rule_from_input(&rule_id, &rule)?;
        hits::apply_rule_to_uncategorized(conn, &rule, days)
    })
}

/// Automation rate of the last `days` days.
#[tauri::command]
pub async fn get_categorization_overview(
    db: State<'_, Database>,
    days: i64,
) -> crate::error::Result<hits::CategorizationOverview> {
    db.with_conn(|conn| hits::categorization_overview(conn, days))
}
