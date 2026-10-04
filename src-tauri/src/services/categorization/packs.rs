//! Locale rule packs: data-driven default categorization rules.
//!
//! Packs are JSON files under `src-tauri/resources/rules/`, embedded at
//! compile time via `include_str!` (no runtime path resolution — a missing
//! file is a build error, not a silent no-op). One pack per country plus a
//! `global` pack with international brands. The validation test in this
//! module is the contribution gate for new packs.

use std::collections::HashSet;
use std::sync::LazyLock;

use serde::Deserialize;

use super::types::{CategorizationRule, RuleType};

/// Raw pack file shape (`schemaVersion: 1`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RulePackFile {
    schema_version: u32,
    pack_id: String,
    country: String,
    version: u32,
    rules: Vec<PackRuleFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackRuleFile {
    id: String,
    name: String,
    rule_type: String,
    pattern: String,
    category_id: String,
    priority: i32,
    #[serde(default)]
    iban_pattern: Option<String>,
}

/// A parsed, ready-to-use rule pack.
pub struct RulePack {
    pub pack_id: String,
    pub country: String,
    pub version: u32,
    pub rules: Vec<CategorizationRule>,
}

/// Every embedded pack source. Adding a new pack = add the JSON file and one
/// line here; the `all_packs_are_valid` test enforces the schema.
pub const PACK_SOURCES: &[&str] = &[
    include_str!("../../../resources/rules/global.json"),
    include_str!("../../../resources/rules/ae.json"),
    include_str!("../../../resources/rules/at.json"),
    include_str!("../../../resources/rules/au.json"),
    include_str!("../../../resources/rules/be.json"),
    include_str!("../../../resources/rules/bh.json"),
    include_str!("../../../resources/rules/ca.json"),
    include_str!("../../../resources/rules/ch.json"),
    include_str!("../../../resources/rules/cl.json"),
    include_str!("../../../resources/rules/cn.json"),
    include_str!("../../../resources/rules/cy.json"),
    include_str!("../../../resources/rules/cz.json"),
    include_str!("../../../resources/rules/de.json"),
    include_str!("../../../resources/rules/dk.json"),
    include_str!("../../../resources/rules/ee.json"),
    include_str!("../../../resources/rules/es.json"),
    include_str!("../../../resources/rules/fi.json"),
    include_str!("../../../resources/rules/fr.json"),
    include_str!("../../../resources/rules/gb.json"),
    include_str!("../../../resources/rules/gr.json"),
    include_str!("../../../resources/rules/hk.json"),
    include_str!("../../../resources/rules/hr.json"),
    include_str!("../../../resources/rules/hu.json"),
    include_str!("../../../resources/rules/ie.json"),
    include_str!("../../../resources/rules/il.json"),
    include_str!("../../../resources/rules/is.json"),
    include_str!("../../../resources/rules/it.json"),
    include_str!("../../../resources/rules/jp.json"),
    include_str!("../../../resources/rules/kr.json"),
    include_str!("../../../resources/rules/kw.json"),
    include_str!("../../../resources/rules/lt.json"),
    include_str!("../../../resources/rules/lu.json"),
    include_str!("../../../resources/rules/lv.json"),
    include_str!("../../../resources/rules/mt.json"),
    include_str!("../../../resources/rules/nl.json"),
    include_str!("../../../resources/rules/no.json"),
    include_str!("../../../resources/rules/nz.json"),
    include_str!("../../../resources/rules/om.json"),
    include_str!("../../../resources/rules/pa.json"),
    include_str!("../../../resources/rules/pl.json"),
    include_str!("../../../resources/rules/pt.json"),
    include_str!("../../../resources/rules/qa.json"),
    include_str!("../../../resources/rules/ro.json"),
    include_str!("../../../resources/rules/sa.json"),
    include_str!("../../../resources/rules/se.json"),
    include_str!("../../../resources/rules/sg.json"),
    include_str!("../../../resources/rules/si.json"),
    include_str!("../../../resources/rules/sk.json"),
    include_str!("../../../resources/rules/tw.json"),
    include_str!("../../../resources/rules/us.json"),
    include_str!("../../../resources/rules/uy.json"),
];

/// The category ids seeded by the baseline migration — the only valid
/// `categoryId` targets for pack rules.
pub const SEEDED_CATEGORY_IDS: &[&str] = &[
    "cat_dining",
    "cat_entertainment",
    "cat_groceries",
    "cat_health",
    "cat_housing",
    "cat_income",
    "cat_insurance",
    "cat_internal_transfers",
    "cat_investments",
    "cat_loan_payments",
    "cat_other",
    "cat_savings",
    "cat_shopping",
    "cat_taxes",
    "cat_transport",
    "cat_travel",
    "cat_utilities",
];

/// Parse and validate one pack source. Returns a descriptive error for the
/// validation test; runtime callers go through [`available_packs`].
pub fn parse_pack(source: &str) -> Result<RulePack, String> {
    let file: RulePackFile =
        serde_json::from_str(source).map_err(|e| format!("invalid pack JSON: {e}"))?;
    if file.schema_version != 1 {
        return Err(format!(
            "pack {}: unsupported schemaVersion {}",
            file.pack_id, file.schema_version
        ));
    }

    let mut rules = Vec::with_capacity(file.rules.len());
    for raw in &file.rules {
        let rule_type: RuleType = raw
            .rule_type
            .parse()
            .map_err(|e| format!("pack {} rule {}: {e}", file.pack_id, raw.id))?;
        rules.push(CategorizationRule {
            id: raw.id.clone(),
            name: raw.name.clone(),
            rule_type,
            pattern: raw.pattern.clone(),
            category_id: raw.category_id.clone(),
            priority: raw.priority,
            is_active: true,
            stop_processing: false,
            iban_pattern: raw.iban_pattern.clone(),
        });
    }

    Ok(RulePack {
        pack_id: file.pack_id,
        country: file.country,
        version: file.version,
        rules,
    })
}

static PACKS: LazyLock<Vec<RulePack>> = LazyLock::new(|| {
    PACK_SOURCES
        .iter()
        .filter_map(|src| match parse_pack(src) {
            Ok(pack) => Some(pack),
            Err(e) => {
                // Unreachable when the validation test is green; never panic
                // in production over a bad data file.
                log::error!("Skipping invalid rule pack: {e}");
                None
            }
        })
        .collect()
});

/// All packs shipped with the app, parsed once.
pub fn available_packs() -> &'static [RulePack] {
    &PACKS
}

/// The rules of the enabled packs, minus individually disabled rule ids —
/// ready for `CategorizationEngine::update_rules`.
pub fn rules_for(
    enabled: &[String],
    disabled_rule_ids: &HashSet<String>,
) -> Vec<CategorizationRule> {
    available_packs()
        .iter()
        .filter(|pack| enabled.iter().any(|id| id == &pack.pack_id))
        .flat_map(|pack| pack.rules.iter())
        .filter(|rule| !disabled_rule_ids.contains(&rule.id))
        .cloned()
        .collect()
}

// ==================== Persisted configuration (app_config) ====================

use crate::error::Result as AppResult;

const ENABLED_KEY: &str = "rule_packs_enabled";
const DISABLED_RULES_KEY: &str = "rule_packs_disabled_rules";

fn read_json_array(conn: &rusqlite::Connection, key: &str) -> AppResult<Option<Vec<String>>> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()))
}

fn write_json_array(conn: &rusqlite::Connection, key: &str, values: &[String]) -> AppResult<()> {
    let json = serde_json::to_string(values)
        .map_err(|e| crate::error::AppError::Internal(format!("serialize {key}: {e}")))?;
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        rusqlite::params![key, json],
    )?;
    Ok(())
}

/// Which pack a UI language maps to when the user has not chosen packs yet.
fn language_pack(language: &str) -> Option<&'static str> {
    match language {
        "cs" => Some("cz"),
        _ => None,
    }
}

/// The default enabled set: `global` plus the pack matching the profile
/// language, when such a pack exists. Computed at read time, not persisted,
/// so a language change keeps working until the user picks packs explicitly.
fn default_enabled(conn: &rusqlite::Connection) -> Vec<String> {
    use rusqlite::OptionalExtension;
    let language: Option<String> = conn
        .query_row(
            "SELECT language FROM user_profile WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();

    let mut enabled = vec!["global".to_string()];
    if let Some(pack) = language.as_deref().and_then(language_pack) {
        if available_packs().iter().any(|p| p.pack_id == pack) {
            enabled.push(pack.to_string());
        }
    }
    enabled
}

/// Read the persisted pack configuration: (enabled pack ids, disabled rule ids).
pub fn read_config(conn: &rusqlite::Connection) -> AppResult<(Vec<String>, HashSet<String>)> {
    let enabled = match read_json_array(conn, ENABLED_KEY)? {
        Some(ids) => ids,
        None => default_enabled(conn),
    };
    let disabled: HashSet<String> = read_json_array(conn, DISABLED_RULES_KEY)?
        .unwrap_or_default()
        .into_iter()
        .collect();
    Ok((enabled, disabled))
}

/// Persist the enabled pack ids (unknown ids are dropped; `global` and the
/// order of `available_packs` are not enforced — an empty list is legal).
pub fn write_enabled(conn: &rusqlite::Connection, pack_ids: &[String]) -> AppResult<()> {
    let known: Vec<String> = pack_ids
        .iter()
        .filter(|id| available_packs().iter().any(|p| &p.pack_id == *id))
        .cloned()
        .collect();
    write_json_array(conn, ENABLED_KEY, &known)
}

/// Persist a single pack rule's disabled flag.
pub fn write_rule_disabled(
    conn: &rusqlite::Connection,
    rule_id: &str,
    disabled: bool,
) -> AppResult<()> {
    let mut set: HashSet<String> = read_json_array(conn, DISABLED_RULES_KEY)?
        .unwrap_or_default()
        .into_iter()
        .collect();
    if disabled {
        set.insert(rule_id.to_string());
    } else {
        set.remove(rule_id);
    }
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    write_json_array(conn, DISABLED_RULES_KEY, &list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    /// CJK text has no word boundaries, so `\b`-based Word rules can never
    /// match inside it. Alphabetic scripts with spaces (Greek, Cyrillic, …)
    /// work fine with the regex crate's Unicode-aware `\b`.
    fn contains_cjk(s: &str) -> bool {
        s.chars().any(|c| {
            matches!(c as u32,
                0x3040..=0x30FF   // Hiragana + Katakana
                | 0x3400..=0x4DBF // CJK Extension A
                | 0x4E00..=0x9FFF // CJK Unified Ideographs
                | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            )
        })
    }

    /// The contribution gate: every embedded pack must fully validate.
    #[test]
    fn all_packs_are_valid() {
        let mut seen_pack_ids: HashSet<String> = HashSet::new();
        let mut seen_rule_ids: HashSet<String> = HashSet::new();
        let categories: HashSet<&str> = SEEDED_CATEGORY_IDS.iter().copied().collect();

        assert!(!PACK_SOURCES.is_empty());

        for source in PACK_SOURCES {
            let pack = parse_pack(source).expect("pack must parse");
            assert!(
                seen_pack_ids.insert(pack.pack_id.clone()),
                "duplicate pack id {}",
                pack.pack_id
            );
            assert!(!pack.rules.is_empty(), "pack {} has no rules", pack.pack_id);

            for rule in &pack.rules {
                let ctx = format!("pack {} rule {}", pack.pack_id, rule.id);
                assert!(
                    rule.id.starts_with(&format!("{}/", pack.pack_id)),
                    "{ctx}: id must be prefixed with the pack id"
                );
                assert!(
                    seen_rule_ids.insert(rule.id.clone()),
                    "{ctx}: duplicate rule id"
                );
                assert!(
                    categories.contains(rule.category_id.as_str()),
                    "{ctx}: unknown category {}",
                    rule.category_id
                );
                assert!(
                    (1..=100).contains(&rule.priority),
                    "{ctx}: priority {} out of range",
                    rule.priority
                );
                assert!(!rule.pattern.trim().is_empty(), "{ctx}: empty pattern");
                assert_eq!(
                    rule.pattern,
                    rule.pattern.to_lowercase(),
                    "{ctx}: pattern must be lowercase"
                );

                match rule.rule_type {
                    RuleType::Regex => {
                        Regex::new(&rule.pattern)
                            .unwrap_or_else(|e| panic!("{ctx}: invalid regex: {e}"));
                    }
                    RuleType::Word => {
                        assert!(
                            !contains_cjk(&rule.pattern),
                            "{ctx}: word rules cannot match CJK text (no word boundaries) — use contains"
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn rules_for_respects_enabled_and_disabled_sets() {
        let enabled = vec!["global".to_string(), "cz".to_string()];
        let mut disabled = HashSet::new();

        let all = rules_for(&enabled, &disabled);
        assert!(all.iter().any(|r| r.id == "global/netflix"));
        assert!(all.iter().any(|r| r.id == "cz/albert"));
        assert!(!all.iter().any(|r| r.id.starts_with("de/")));

        disabled.insert("cz/albert".to_string());
        let filtered = rules_for(&enabled, &disabled);
        assert!(!filtered.iter().any(|r| r.id == "cz/albert"));
        assert_eq!(filtered.len(), all.len() - 1);
    }

    #[test]
    fn packs_parse_into_the_registry() {
        // Every source parses (the validation test proves it), so the lazy
        // registry must expose all of them.
        assert_eq!(available_packs().len(), PACK_SOURCES.len());
    }

    fn setup_config_db(language: &str) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(&format!(
            r#"
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY, language TEXT NOT NULL);
            INSERT INTO user_profile (id, language) VALUES (1, '{language}');
            "#
        ))
        .expect("schema");
        conn
    }

    #[test]
    fn default_config_follows_profile_language() {
        let conn = setup_config_db("cs");
        let (enabled, disabled) = read_config(&conn).unwrap();
        assert_eq!(enabled, vec!["global".to_string(), "cz".to_string()]);
        assert!(disabled.is_empty());

        let conn = setup_config_db("en");
        let (enabled, _) = read_config(&conn).unwrap();
        assert_eq!(enabled, vec!["global".to_string()]);
    }

    #[test]
    fn enabled_packs_round_trip_and_drop_unknown_ids() {
        let conn = setup_config_db("en");
        write_enabled(&conn, &["global".into(), "de".into(), "atlantis".into()]).unwrap();
        let (enabled, _) = read_config(&conn).unwrap();
        assert_eq!(enabled, vec!["global".to_string(), "de".to_string()]);
    }

    #[test]
    fn rule_disabling_round_trips() {
        let conn = setup_config_db("cs");
        write_rule_disabled(&conn, "cz/albert", true).unwrap();
        let (_, disabled) = read_config(&conn).unwrap();
        assert!(disabled.contains("cz/albert"));

        write_rule_disabled(&conn, "cz/albert", false).unwrap();
        let (_, disabled) = read_config(&conn).unwrap();
        assert!(disabled.is_empty());
    }
}
