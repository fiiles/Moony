//! Hierarchical exact match engine for learned payee lookups
//!
//! This engine provides cascading lookups with prioritized matching:
//! 1. iban_only_default: iban only (general IBAN rule)
//! 2. payee_default: payee only (general payee rule)
//!
//! Payees are keyed by [`normalize_payee_key`] (reference numbers stripped) and
//! IBANs by [`normalize_iban`] (whitespace removed, uppercased), on both the
//! learning and the matching side.

use std::collections::HashMap;

use super::tokenizer::{normalize_iban, normalize_payee_key, strip_punctuation};
use super::types::{CategorizationResult, CategorizationSource, TransactionInput};

/// Key prefix for different lookup types
const KEY_PAYEE_DEFAULT: &str = "payee_default:";
/// IBAN-only default (no payee) - general rule for transactions with only IBAN
const KEY_IBAN_ONLY_DEFAULT: &str = "iban_only_default:";

/// The payee's lookup key, or `None` when it is too short (or only a reference
/// number) to identify anyone. See [`normalize_payee_key`].
fn payee_key(payee: &str) -> Option<String> {
    let key = normalize_payee_key(payee);
    (key.len() > 2).then_some(key)
}

/// Every key a payee may be stored under: the current key first, then its
/// punctuation-stripped form, which is how rows written before RUL-02 were
/// keyed ("Dr. Max" → "dr max").
fn payee_lookup_keys(key: &str) -> Vec<String> {
    let legacy = strip_punctuation(key);
    if legacy == key {
        vec![key.to_string()]
    } else {
        vec![key.to_string(), legacy]
    }
}

/// The IBAN's lookup key, or `None` when empty. See [`normalize_iban`].
fn iban_key(iban: &str) -> Option<String> {
    let key = normalize_iban(iban);
    (!key.is_empty()).then_some(key)
}

/// Hierarchical exact match engine with cascading defaults
///
/// Matching priority (most specific first):
/// 1. iban_only_default: iban only (IBAN takes priority over payee!)
/// 2. payee_default: payee only
///
/// Unknown key prefixes (e.g. the removed `iban_partial:` entries in old
/// exports) are carried by `from_raw_map`/`merge` but never matched.
pub struct ExactMatchEngine {
    /// Unified map with prefixed keys for different lookup types
    payee_map: HashMap<String, String>,
}

impl ExactMatchEngine {
    /// Create an empty exact match engine
    pub fn new() -> Self {
        Self {
            payee_map: HashMap::new(),
        }
    }

    /// Create from an existing payee map (backward compatibility)
    /// Old format entries become payee defaults
    pub fn from_map(payee_map: HashMap<String, String>) -> Self {
        let mut engine = Self::new();
        for (payee, category) in payee_map {
            // Old format: normalized_payee -> category
            // Treat as payee default (payee + null)
            engine.learn_hierarchical(Some(&payee), None, &category);
        }
        engine
    }

    /// Create from raw internal map format (with prefixed keys)
    ///
    /// This is used when importing the exact format returned by export(),
    /// preserving the hierarchical key structure.
    pub fn from_raw_map(payee_map: HashMap<String, String>) -> Self {
        Self { payee_map }
    }

    /// Learn from user's manual categorization with hierarchical key
    ///
    /// Logic:
    /// 1. NEW CATEGORIZATION (no existing rule matches):
    ///    - If IBAN available → create iban_only_default (IBAN takes priority!)
    ///    - If only payee (no IBAN) → create payee_default
    ///
    /// 2. RECATEGORIZATION (existing rule was matched):
    ///    - Overwrite the existing rule with new category
    ///
    /// # Arguments
    /// * `payee` - Normalized payee name (counterparty_name)
    /// * `iban` - Counterparty IBAN/account number
    /// * `category_id` - Category to assign
    pub fn learn_hierarchical(
        &mut self,
        payee: Option<&str>,
        iban: Option<&str>,
        category_id: &str,
    ) {
        let norm_payee = payee.and_then(payee_key);
        let norm_iban = iban.and_then(iban_key);

        // Find which existing rule would match this transaction
        let existing_rule = self.find_matching_rule_key(&norm_payee, &norm_iban);

        match existing_rule {
            // NO EXISTING RULE → New categorization
            None => {
                // IBAN takes priority over payee!
                if let Some(ref i) = norm_iban {
                    // Create iban_only_default (regardless of whether payee is present)
                    let key = format!("{KEY_IBAN_ONLY_DEFAULT}{}", i);
                    self.payee_map.insert(key, category_id.to_string());
                } else if let Some(ref p) = norm_payee {
                    // No IBAN, only payee → create payee_default
                    let key = format!("{KEY_PAYEE_DEFAULT}{}", p);
                    self.payee_map.insert(key, category_id.to_string());
                }
                // else: no payee and no iban → do nothing
            }

            // EXISTING RULE → Recategorization
            Some(existing_key) => {
                // Check what type of rule matched
                let is_generic_payee = existing_key.starts_with(KEY_PAYEE_DEFAULT);

                if let Some(ref i) = norm_iban {
                    if is_generic_payee {
                        // Special case: payee_default matched, but transaction now has IBAN
                        // → delete payee_default and create iban_only_default (IBAN takes priority)
                        self.payee_map.remove(&existing_key);
                        let key = format!("{KEY_IBAN_ONLY_DEFAULT}{}", i);
                        self.payee_map.insert(key, category_id.to_string());
                    } else {
                        // Generic iban rule → overwrite existing rule
                        self.payee_map.insert(existing_key, category_id.to_string());
                    }
                } else {
                    // Generic rule → overwrite existing rule
                    self.payee_map.insert(existing_key, category_id.to_string());
                }
            }
        }
    }

    /// Find the key of an existing rule that would match a transaction with given attributes
    /// Returns None if no rule matches (new categorization scenario)
    fn find_matching_rule_key(
        &self,
        norm_payee: &Option<String>,
        norm_iban: &Option<String>,
    ) -> Option<String> {
        // Priority 1: iban_only_default (iban only)
        if let Some(ref i) = norm_iban {
            let key = format!("{KEY_IBAN_ONLY_DEFAULT}{}", i);
            if self.payee_map.contains_key(&key) {
                return Some(key);
            }
        }

        // Priority 2: payee_default (payee only)
        if let Some(ref p) = norm_payee {
            for candidate in payee_lookup_keys(p) {
                let key = format!("{KEY_PAYEE_DEFAULT}{candidate}");
                if self.payee_map.contains_key(&key) {
                    return Some(key);
                }
            }
        }

        None
    }

    /// Legacy learn function - creates payee default
    pub fn learn(&mut self, payee: &str, category_id: &str) {
        self.learn_hierarchical(Some(payee), None, category_id);
    }

    /// Forget a learned payee - removes exact key based on provided attributes
    pub fn forget_hierarchical(&mut self, payee: Option<&str>, iban: Option<&str>) -> bool {
        let norm_payee = payee.and_then(payee_key);
        let norm_iban = iban.and_then(iban_key);

        // Try to find and remove the most specific matching rule

        // 1. Try generic IBAN rule first (most specific)
        if let Some(ref i) = norm_iban {
            let key = format!("{KEY_IBAN_ONLY_DEFAULT}{}", i);
            if self.payee_map.remove(&key).is_some() {
                return true;
            }
        }

        // 2. Try generic Payee rule (under the current key and the legacy one)
        if let Some(ref p) = norm_payee {
            let mut removed = false;
            for candidate in payee_lookup_keys(p) {
                let key = format!("{KEY_PAYEE_DEFAULT}{candidate}");
                removed |= self.payee_map.remove(&key).is_some();
            }
            if removed {
                return true;
            }
        }

        false
    }

    /// Legacy forget function - removes payee default
    pub fn forget(&mut self, payee: &str) -> bool {
        self.forget_hierarchical(Some(payee), None)
    }

    /// Try hierarchical match lookup for a transaction
    ///
    /// Cascading priority (most specific first):
    /// 1. iban_only_default: iban only (general IBAN rule)
    /// 2. payee_default: payee only (general payee rule)
    pub fn apply(&self, tx: &TransactionInput) -> Option<CategorizationResult> {
        let payee = tx.counterparty.as_deref().and_then(payee_key);
        let iban = tx.counterparty_iban.as_deref().and_then(iban_key);

        // Priority 1: iban_only_default (iban only) - general IBAN rule
        if let Some(ref i) = iban {
            let key = format!("{KEY_IBAN_ONLY_DEFAULT}{}", i);
            if let Some(cat) = self.payee_map.get(&key) {
                return Some(CategorizationResult::Match {
                    category_id: cat.clone(),
                    source: CategorizationSource::ExactMatch {
                        payee: tx
                            .counterparty
                            .clone()
                            .or_else(|| tx.counterparty_iban.clone())
                            .unwrap_or_default(),
                    },
                });
            }
        }

        // Priority 2: payee_default (payee only) - general payee rule
        if let Some(ref p) = payee {
            for candidate in payee_lookup_keys(p) {
                let key = format!("{KEY_PAYEE_DEFAULT}{candidate}");
                if let Some(cat) = self.payee_map.get(&key) {
                    return Some(CategorizationResult::Match {
                        category_id: cat.clone(),
                        source: CategorizationSource::ExactMatch {
                            payee: tx.counterparty.clone().unwrap_or_default(),
                        },
                    });
                }
            }
        }

        None
    }

    /// Export the payee map for persistence
    /// Returns the raw internal map (with prefixed keys)
    pub fn export(&self) -> HashMap<String, String> {
        self.payee_map.clone()
    }

    /// Export in legacy format (payee -> category for payee defaults only)
    pub fn export_legacy(&self) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for (key, category) in &self.payee_map {
            if let Some(payee) = key.strip_prefix(KEY_PAYEE_DEFAULT) {
                result.insert(payee.to_string(), category.clone());
            }
        }
        result
    }

    /// Point every learned entry for `from` at `to` instead (a category was
    /// deleted and its references moved). Returns how many entries changed.
    pub fn reassign_category(&mut self, from: &str, to: &str) -> usize {
        let mut changed = 0;
        for category in self.payee_map.values_mut() {
            if category == from {
                *category = to.to_string();
                changed += 1;
            }
        }
        changed
    }

    /// Get the number of learned entries
    pub fn len(&self) -> usize {
        self.payee_map.len()
    }

    /// Check if the engine has no learned entries
    pub fn is_empty(&self) -> bool {
        self.payee_map.is_empty()
    }

    /// Check if a specific payee is known (checks payee default)
    pub fn knows_payee(&self, payee: &str) -> bool {
        self.get_category(payee).is_some()
    }

    /// Get the category for a known payee (from payee default)
    pub fn get_category(&self, payee: &str) -> Option<&str> {
        let key = payee_key(payee)?;
        payee_lookup_keys(&key).into_iter().find_map(|candidate| {
            self.payee_map
                .get(&format!("{KEY_PAYEE_DEFAULT}{candidate}"))
                .map(|s| s.as_str())
        })
    }

    /// Merge another engine's payee map into this one
    /// Existing entries are not overwritten.
    pub fn merge(&mut self, other: &ExactMatchEngine) {
        for (key, category) in &other.payee_map {
            self.payee_map
                .entry(key.clone())
                .or_insert_with(|| category.clone());
        }
    }

    /// Merge with overwrite (other's entries take precedence)
    pub fn merge_overwrite(&mut self, other: &ExactMatchEngine) {
        for (key, category) in &other.payee_map {
            self.payee_map.insert(key.clone(), category.clone());
        }
    }
}

impl Default for ExactMatchEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tx(description: Option<&str>, counterparty: Option<&str>) -> TransactionInput {
        TransactionInput {
            id: "tx1".into(),
            description: description.map(|s| s.into()),
            counterparty: counterparty.map(|s| s.into()),
            counterparty_iban: None,
            amount: -500.0,
            is_credit: false,
            bank_account_id: None,
        }
    }

    fn make_tx_full(counterparty: Option<&str>, iban: Option<&str>) -> TransactionInput {
        TransactionInput {
            id: "tx1".into(),
            description: None,
            counterparty: counterparty.map(|s| s.into()),
            counterparty_iban: iban.map(|s| s.into()),
            amount: -500.0,
            is_credit: false,
            bank_account_id: None,
        }
    }

    #[test]
    fn test_learn_and_apply() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("Uber Eats", "cat_dining");

        let tx = make_tx(None, Some("Uber Eats"));
        let result = engine.apply(&tx);

        assert!(result.is_some());
        match result.unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_dining");
            }
            _ => panic!("Expected Match"),
        }
    }

    // ==================== NEW CATEGORIZATION TESTS ====================

    #[test]
    fn test_new_categorization_payee_only() {
        // Rule a) - platba má jen payee → vytvořit pravidlo s payee
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(Some("Albert"), None, "cat_groceries");

        // Should create payee_default
        let tx = make_tx_full(Some("Albert"), None);
        match engine.apply(&tx).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_groceries");
            }
            _ => panic!("Expected Match"),
        }
    }

    #[test]
    fn test_new_categorization_iban_only() {
        // Rule b) - platba má jen iban → vytvořit pravidlo s iban
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(None, Some("CZ123"), "cat_utilities");

        // Should create iban_only_default
        let tx = make_tx_full(None, Some("CZ123"));
        match engine.apply(&tx).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_utilities");
            }
            _ => panic!("Expected Match"),
        }
    }

    #[test]
    fn test_new_categorization_iban_plus_payee_creates_iban_rule() {
        // Rule c) - platba má iban + payee → vytvořit pravidlo JEN s iban
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(Some("Albert"), Some("CZ123"), "cat_groceries");

        // Should create iban_only_default, NOT payee_default
        // Check: same IBAN, DIFFERENT payee should still match
        let tx = make_tx_full(Some("Different Payee"), Some("CZ123"));
        match engine.apply(&tx).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_groceries");
            }
            _ => panic!("Expected Match - iban_only_default should match any payee with same IBAN"),
        }

        // Same payee, DIFFERENT IBAN should NOT match
        let tx2 = make_tx_full(Some("Albert"), Some("CZ999"));
        assert!(
            engine.apply(&tx2).is_none(),
            "Different IBAN should not match"
        );
    }

    // ==================== RECATEGORIZATION TESTS ====================

    #[test]
    fn test_recategorization_generic_without_vs_overwrites() {
        // Existing generic rule + transaction → overwrite existing
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(Some("Albert"), None, "cat_groceries");

        // Recategorization: should overwrite
        engine.learn_hierarchical(Some("Albert"), None, "cat_dining");

        let tx = make_tx_full(Some("Albert"), None);
        match engine.apply(&tx).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_dining");
            }
            _ => panic!("Expected Match"),
        }
    }

    #[test]
    fn test_recategorization_payee_default_replaced_by_iban_when_iban_present() {
        // Existing payee_default + transaction now has IBAN
        // → delete payee_default and create iban_only_default
        let mut engine = ExactMatchEngine::new();

        // First categorization (payee only): creates payee_default
        engine.learn_hierarchical(Some("Albert"), None, "cat_groceries");

        // Verify payee_default was created
        let tx_payee_only = make_tx_full(Some("Albert"), None);
        assert!(engine.apply(&tx_payee_only).is_some());

        // Recategorization: same payee but NOW with IBAN
        // This should DELETE payee_default and CREATE iban_only_default
        engine.learn_hierarchical(Some("Albert"), Some("CZ123"), "cat_utilities");

        // Check: payee_default should be GONE
        let tx_payee_only2 = make_tx_full(Some("Albert"), None);
        assert!(
            engine.apply(&tx_payee_only2).is_none(),
            "payee_default should have been deleted"
        );

        // Check: iban_only_default should now exist
        let tx_with_iban = make_tx_full(Some("Albert"), Some("CZ123"));
        match engine.apply(&tx_with_iban).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_utilities");
            }
            _ => panic!("Expected Match from iban_only_default"),
        }

        // Same IBAN, different payee should also match (because it's iban_only_default)
        let tx_different_payee = make_tx_full(Some("Different"), Some("CZ123"));
        match engine.apply(&tx_different_payee).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_utilities");
            }
            _ => panic!("Expected Match - iban_only_default should match any payee"),
        }
    }

    // ==================== MATCHING PRIORITY TESTS ====================

    #[test]
    fn test_iban_rule_takes_priority_over_payee() {
        let mut engine = ExactMatchEngine::new();
        // Create payee general rule
        engine.payee_map.insert(
            format!("{}albert", KEY_PAYEE_DEFAULT),
            "cat_payee".to_string(),
        );
        // Create IBAN general rule
        engine.payee_map.insert(
            format!("{}CZ123", KEY_IBAN_ONLY_DEFAULT),
            "cat_iban".to_string(),
        );

        // IBAN rule should take priority when transaction has both
        let tx = make_tx_full(Some("Albert"), Some("CZ123"));
        match engine.apply(&tx).unwrap() {
            CategorizationResult::Match { category_id, .. } => {
                assert_eq!(category_id, "cat_iban");
            }
            _ => panic!("Expected Match - IBAN rule should have priority"),
        }
    }

    // ==================== PAYEE KEY NORMALIZATION (RUL-02) ====================

    #[test]
    fn a_learned_payee_matches_the_next_transaction_with_a_different_reference() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("AUSTRIAN AIR2572170483655", "cat_travel");

        // A different reference number, with and without a separating space.
        for counterparty in ["AUSTRIAN AIR9999", "Austrian Air 123456789", "austrian air"] {
            let tx = make_tx(None, Some(counterparty));
            match engine.apply(&tx) {
                Some(CategorizationResult::Match { category_id, .. }) => {
                    assert_eq!(category_id, "cat_travel", "{counterparty:?}")
                }
                _ => panic!("{counterparty:?} should hit the learned payee"),
            }
        }
        assert_eq!(
            engine.len(),
            1,
            "sixteen flights are one learned payee, not sixteen"
        );
    }

    #[test]
    fn sixteen_references_learned_one_by_one_stay_one_entry() {
        let mut engine = ExactMatchEngine::new();
        for i in 0..16 {
            engine.learn(
                &format!("AUSTRIAN AIR25721704836{:02}", 55 + i),
                "cat_travel",
            );
        }
        assert_eq!(engine.len(), 1);
    }

    #[test]
    fn punctuation_stripped_legacy_keys_still_match() {
        // Rows written before RUL-02 keyed on simple_normalize(), which dropped
        // punctuation: "Dr. Max" was stored as "dr max".
        let mut raw = HashMap::new();
        raw.insert(
            format!("{KEY_PAYEE_DEFAULT}dr max"),
            "cat_health".to_string(),
        );
        let engine = ExactMatchEngine::from_raw_map(raw);

        let tx = make_tx(None, Some("DR. MAX 03"));
        match engine.apply(&tx) {
            Some(CategorizationResult::Match { category_id, .. }) => {
                assert_eq!(category_id, "cat_health")
            }
            _ => panic!("legacy punctuation-less key must still match"),
        }
    }

    #[test]
    fn forgetting_a_payee_removes_it_whatever_reference_it_was_learned_with() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("AUSTRIAN AIR2572170483655", "cat_travel");
        assert!(engine.forget("Austrian Air 777"));
        assert!(engine
            .apply(&make_tx(None, Some("AUSTRIAN AIR9999")))
            .is_none());
    }

    #[test]
    fn a_payee_made_only_of_a_reference_is_not_learned() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("2572170483655", "cat_travel");
        assert!(engine.is_empty());
    }

    // ==================== IBAN NORMALIZATION ====================

    #[test]
    fn learned_iban_matches_regardless_of_spacing_and_case() {
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(None, Some("DE89 3704 0044 0532 0130 00"), "cat_savings");

        let tx = make_tx_full(None, Some("de89370400440532013000"));
        match engine.apply(&tx) {
            Some(CategorizationResult::Match { category_id, .. }) => {
                assert_eq!(category_id, "cat_savings")
            }
            _ => panic!("IBAN keys must be normalized on both sides"),
        }
    }

    // ==================== LEGACY TESTS (still valid) ====================

    #[test]
    fn test_case_insensitive() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("UBER EATS", "cat_dining");

        let tx = make_tx(None, Some("uber eats"));
        assert!(engine.apply(&tx).is_some());

        let tx = make_tx(None, Some("Uber Eats"));
        assert!(engine.apply(&tx).is_some());
    }

    #[test]
    fn test_forget() {
        let mut engine = ExactMatchEngine::new();
        engine.learn("Uber Eats", "cat_dining");

        let tx = make_tx(None, Some("Uber Eats"));
        assert!(engine.apply(&tx).is_some());

        engine.forget("Uber Eats");
        assert!(engine.apply(&tx).is_none());
    }

    #[test]
    fn test_forget_hierarchical() {
        let mut engine = ExactMatchEngine::new();
        engine.learn_hierarchical(None, Some("CZ123"), "cat_utilities");

        let tx = make_tx_full(None, Some("CZ123"));
        assert!(engine.apply(&tx).is_some());

        engine.forget_hierarchical(None, Some("CZ123"));
        assert!(engine.apply(&tx).is_none());
    }

    #[test]
    fn test_unknown_key_prefixes_are_carried_but_never_matched() {
        // Old exports may contain the removed `iban_partial:` keys.
        let mut raw = HashMap::new();
        raw.insert(
            "iban_partial:CZ123".to_string(),
            "cat_utilities".to_string(),
        );
        let engine = ExactMatchEngine::from_raw_map(raw);

        let tx = make_tx_full(None, Some("CZ123"));
        assert!(engine.apply(&tx).is_none());
        // ...but the entry round-trips through export unchanged.
        assert_eq!(
            engine
                .export()
                .get("iban_partial:CZ123")
                .map(String::as_str),
            Some("cat_utilities")
        );
    }

    #[test]
    fn test_export_import() {
        let mut engine1 = ExactMatchEngine::new();
        engine1.learn("Test Payee", "cat_other");
        engine1.learn_hierarchical(None, Some("CZ123"), "cat_utilities");

        let exported = engine1.export();
        assert!(!exported.is_empty());

        let engine2 = ExactMatchEngine::from_raw_map(exported);

        let tx_payee = make_tx(None, Some("Test Payee"));
        assert!(engine2.apply(&tx_payee).is_some());

        let tx_iban = make_tx_full(None, Some("CZ123"));
        assert!(engine2.apply(&tx_iban).is_some());
    }
}
