//! Master categorization engine orchestrating the waterfall flow
//!
//! This engine combines:
//! 1. Custom Rule Engine - User-defined rules (highest priority)
//! 2. Exact Match Engine - HashMap lookup for known payees
//! 3. Internal Transfer Detection - Own IBAN matching
//! 4. Default Rule Engine - System rules (locale packs)
//!
//! Flow: Custom Rules → Learned Payees → Internal Transfers → Default Rules

use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use super::exact_match::ExactMatchEngine;
use super::rules::RuleEngine;
use super::tokenizer::normalize_iban;
use super::types::{CategorizationResult, CategorizationRule, TransactionInput};

/// Main categorization engine orchestrating all classification methods
pub struct CategorizationEngine {
    /// User-defined custom rules (highest priority, checked first)
    custom_rule_engine: RwLock<RuleEngine>,
    /// System/default rules (checked after learned payees)
    default_rule_engine: RwLock<RuleEngine>,
    exact_match: RwLock<ExactMatchEngine>,
    /// User's own IBANs for internal transfer detection
    own_ibans: RwLock<HashSet<String>>,
}

impl CategorizationEngine {
    /// Create a new engine with the given default rules
    pub fn new(default_rules: Vec<CategorizationRule>) -> Self {
        Self {
            custom_rule_engine: RwLock::new(RuleEngine::empty()),
            default_rule_engine: RwLock::new(RuleEngine::new(default_rules)),
            exact_match: RwLock::new(ExactMatchEngine::new()),
            own_ibans: RwLock::new(HashSet::new()),
        }
    }

    /// Create an engine with no default rules. The locale packs are applied
    /// post-unlock via `update_rules` (`load_rule_packs_config`), because the
    /// pack configuration lives in the encrypted database.
    pub fn new_empty() -> Self {
        Self::new(Vec::new())
    }

    /// Set the user's own IBANs for internal transfer detection
    ///
    /// When a transaction's counterparty IBAN matches any of these,
    /// it will be categorized as "Internal Transfer" (unless overridden by learned payees).
    pub fn set_own_ibans(&self, ibans: Vec<String>) {
        let normalized: HashSet<String> = ibans
            .iter()
            .map(|s| normalize_iban(s))
            .filter(|s| !s.is_empty())
            .collect();
        if let Ok(mut own) = self.own_ibans.write() {
            *own = normalized;
        }
    }

    /// Check if counterparty IBAN matches user's own accounts
    fn is_internal_transfer(&self, counterparty_iban: Option<&str>) -> bool {
        if let Some(iban) = counterparty_iban {
            let normalized = normalize_iban(iban);
            if !normalized.is_empty() {
                if let Ok(own) = self.own_ibans.read() {
                    return own.contains(&normalized);
                }
            }
        }
        false
    }

    /// Categorize a single transaction using waterfall approach
    ///
    /// Flow (priority order):
    /// 1. Try CUSTOM rules FIRST (user-defined rules override everything)
    /// 2. Try exact payee match (user's learned categorizations)
    /// 3. Check if counterparty IBAN matches user's own accounts (internal transfer)
    /// 4. Try DEFAULT rules (system rules)
    /// 5. Return None if nothing matches
    pub fn categorize(&self, tx: &TransactionInput) -> CategorizationResult {
        // Step 1: Try CUSTOM rules FIRST (user-defined rules override everything)
        if let Ok(rules) = self.custom_rule_engine.read() {
            if let Some((result, _stop_processing)) = rules.apply(tx) {
                return result;
            }
        }

        // Step 2: Try exact payee match (learned payees)
        if let Ok(exact) = self.exact_match.read() {
            if let Some(result) = exact.apply(tx) {
                return result;
            }
        }

        // Step 3: Check own IBANs for internal transfer detection
        if self.is_internal_transfer(tx.counterparty_iban.as_deref()) {
            return CategorizationResult::Match {
                category_id: "cat_internal_transfers".to_string(),
                source: super::types::CategorizationSource::Rule {
                    rule_id: "own_account_iban".to_string(),
                    rule_name: "Internal Transfer (own account)".to_string(),
                },
            };
        }

        // Step 4: Try DEFAULT rules (system rules)
        if let Ok(rules) = self.default_rule_engine.read() {
            if let Some((result, _stop_processing)) = rules.apply(tx) {
                return result;
            }
        }

        CategorizationResult::None
    }

    /// Categorize multiple transactions (batch)
    pub fn categorize_batch(&self, transactions: &[TransactionInput]) -> Vec<CategorizationResult> {
        transactions.iter().map(|tx| self.categorize(tx)).collect()
    }

    /// Learn from user's manual categorization with hierarchical matching
    ///
    /// This adds the payee/iban combination to the exact match engine.
    /// The engine uses cascading defaults:
    /// - iban = IBAN default (catches any payee)
    /// - payee = payee default (catches any iban)
    pub fn learn_from_user(&self, payee: Option<&str>, iban: Option<&str>, category_id: &str) {
        if let Ok(mut exact) = self.exact_match.write() {
            exact.learn_hierarchical(payee, iban, category_id);
        }
    }

    /// Legacy learn - creates payee default
    pub fn learn_from_user_simple(&self, payee: &str, category_id: &str) {
        self.learn_from_user(Some(payee), None, category_id);
    }

    /// Forget a learned payee combination
    pub fn forget_payee(&self, payee: Option<&str>, iban: Option<&str>) -> bool {
        if let Ok(mut exact) = self.exact_match.write() {
            exact.forget_hierarchical(payee, iban)
        } else {
            false
        }
    }

    /// Legacy forget - removes payee default
    pub fn forget_payee_simple(&self, payee: &str) -> bool {
        self.forget_payee(Some(payee), None)
    }

    /// Update default rules (system rules)
    pub fn update_rules(&self, rules: Vec<CategorizationRule>) {
        if let Ok(mut engine) = self.default_rule_engine.write() {
            *engine = RuleEngine::new(rules);
        }
    }

    /// Update custom rules (user-defined rules, checked first in waterfall)
    pub fn update_custom_rules(&self, rules: Vec<CategorizationRule>) {
        if let Ok(mut engine) = self.custom_rule_engine.write() {
            *engine = RuleEngine::new(rules);
        }
    }

    /// A category was deleted and everything that referenced it moved to
    /// `to`: keep the in-memory layers in step with the database so no rule or
    /// learned payee keeps answering with the dead id. Returns how many rules
    /// and learned entries changed.
    pub fn reassign_category(&self, from: &str, to: &str) -> usize {
        let mut changed = 0;
        if let Ok(mut engine) = self.custom_rule_engine.write() {
            changed += engine.reassign_category(from, to);
        }
        if let Ok(mut engine) = self.default_rule_engine.write() {
            changed += engine.reassign_category(from, to);
        }
        if let Ok(mut exact) = self.exact_match.write() {
            changed += exact.reassign_category(from, to);
        }
        changed
    }

    /// Export learned payees for persistence
    pub fn export_learned_payees(&self) -> HashMap<String, String> {
        self.exact_match
            .read()
            .map(|e| e.export())
            .unwrap_or_default()
    }

    /// Import learned payees (raw format with prefixed keys)
    ///
    /// This imports the exact format returned by export_learned_payees(),
    /// which includes the internal key prefixes for hierarchical matching.
    pub fn import_learned_payees(&self, payees: HashMap<String, String>) {
        if let Ok(mut exact) = self.exact_match.write() {
            exact.merge(&super::exact_match::ExactMatchEngine::from_raw_map(payees));
        }
    }

    /// Get statistics about the engine
    pub fn stats(&self) -> EngineStats {
        let custom_rules = self
            .custom_rule_engine
            .read()
            .map(|r| r.active_rule_count())
            .unwrap_or(0);

        let default_rules = self
            .default_rule_engine
            .read()
            .map(|r| r.active_rule_count())
            .unwrap_or(0);

        let learned_payees = self.exact_match.read().map(|e| e.len()).unwrap_or(0);

        EngineStats {
            active_rules: custom_rules + default_rules,
            learned_payees,
        }
    }
}

impl Default for CategorizationEngine {
    fn default() -> Self {
        Self::new_empty()
    }
}

/// Statistics about the categorization engine
#[derive(Debug, Clone)]
pub struct EngineStats {
    pub active_rules: usize,
    pub learned_payees: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::categorization::types::{CategorizationSource, RuleType};

    fn make_tx(description: &str, counterparty: Option<&str>) -> TransactionInput {
        TransactionInput {
            id: "tx1".into(),
            description: Some(description.into()),
            counterparty: counterparty.map(|s| s.into()),
            counterparty_iban: None,
            amount: -500.0,
            is_credit: false,
            bank_account_id: None,
        }
    }

    #[test]
    fn test_rule_match() {
        let rules = vec![CategorizationRule {
            id: "r1".into(),
            name: "Albert".into(),
            rule_type: RuleType::Contains,
            pattern: "albert".into(),
            category_id: "cat_groceries".into(),
            priority: 50,
            is_active: true,
            stop_processing: false,
            iban_pattern: None,
        }];

        let engine = CategorizationEngine::new(rules);
        let tx = make_tx("Platba kartou Albert Hypermarket", None);

        let result = engine.categorize(&tx);
        match result {
            CategorizationResult::Match {
                category_id,
                source,
            } => {
                assert_eq!(category_id, "cat_groceries");
                assert!(matches!(source, CategorizationSource::Rule { .. }));
            }
            _ => panic!("Expected rule Match"),
        }
    }

    #[test]
    fn test_exact_match_after_learning() {
        let engine = CategorizationEngine::new(vec![]);
        engine.learn_from_user_simple("Uber Eats", "cat_dining");

        let tx = make_tx("", Some("Uber Eats"));
        let result = engine.categorize(&tx);

        match result {
            CategorizationResult::Match {
                category_id,
                source,
            } => {
                assert_eq!(category_id, "cat_dining");
                assert!(matches!(source, CategorizationSource::ExactMatch { .. }));
            }
            _ => panic!("Expected exact Match"),
        }
    }

    #[test]
    fn test_waterfall_order() {
        // Learned payees (exact match) now take precedence over rules
        let rules = vec![CategorizationRule {
            id: "r1".into(),
            name: "Albert Rule".into(),
            rule_type: RuleType::Contains,
            pattern: "albert".into(),
            category_id: "cat_groceries".into(),
            priority: 50,
            is_active: true,
            stop_processing: false,
            iban_pattern: None,
        }];

        let engine = CategorizationEngine::new(rules);
        // Learn "Albert" as dining (should WIN over rule since learned payees have highest priority)
        engine.learn_from_user_simple("Albert", "cat_dining");

        // Transaction has BOTH description (for rule match) AND counterparty (for exact match)
        let tx = make_tx("Albert Praha", Some("Albert"));
        let result = engine.categorize(&tx);

        match result {
            CategorizationResult::Match {
                category_id,
                source,
            } => {
                assert_eq!(category_id, "cat_dining"); // Learned payee wins over rule
                assert!(matches!(source, CategorizationSource::ExactMatch { .. }));
            }
            _ => panic!("Expected exact Match"),
        }
    }

    #[test]
    fn test_no_match() {
        let engine = CategorizationEngine::new(vec![]);
        let tx = make_tx("Unknown merchant XYZ", None);

        let result = engine.categorize(&tx);
        assert!(matches!(result, CategorizationResult::None));
    }

    #[test]
    fn test_pack_rules_load_into_engine() {
        let enabled = vec!["global".to_string(), "cz".to_string()];
        let engine = CategorizationEngine::new(super::super::packs::rules_for(
            &enabled,
            &std::collections::HashSet::new(),
        ));
        let stats = engine.stats();
        assert!(stats.active_rules > 0);

        let tx = make_tx("Platba kartou Albert Hypermarket", None);
        assert!(engine.categorize(&tx).has_category());
    }

    // ==================== Generic multilingual keyword rules ====================

    /// An engine with only the `global` pack — the generic keyword rules live
    /// there, so they work for every user whatever country packs are enabled.
    fn global_pack_engine() -> CategorizationEngine {
        let enabled = vec!["global".to_string()];
        CategorizationEngine::new(super::super::packs::rules_for(
            &enabled,
            &std::collections::HashSet::new(),
        ))
    }

    fn category_of(engine: &CategorizationEngine, text: &str) -> Option<String> {
        engine
            .categorize(&make_tx(text, None))
            .category_id()
            .map(String::from)
    }

    #[test]
    fn generic_keywords_categorize_the_audit_examples() {
        // Salary / Miete / Strom stayed uncategorized after the first import.
        let engine = global_pack_engine();
        assert_eq!(
            category_of(&engine, "Salary September").as_deref(),
            Some("cat_income")
        );
        assert_eq!(
            category_of(&engine, "Miete 09/2026").as_deref(),
            Some("cat_housing")
        );
        assert_eq!(
            category_of(&engine, "Vattenfall Strom").as_deref(),
            Some("cat_utilities")
        );
    }

    #[test]
    fn generic_keywords_cover_every_language_and_topic() {
        let engine = global_pack_engine();
        let cases = [
            // English
            ("Monthly salary ACME", "cat_income"),
            ("Payroll ACME Ltd", "cat_income"),
            ("Rent October", "cat_housing"),
            ("Savings interest credited", "cat_income"),
            ("Electricity bill", "cat_utilities"),
            ("British Gas", "cat_utilities"),
            ("Thames Water", "cat_utilities"),
            ("Car insurance premium", "cat_insurance"),
            ("Loan repayment", "cat_loan_payments"),
            ("Mortgage payment", "cat_loan_payments"),
            ("Boots Pharmacy", "cat_health"),
            // Czech
            ("Mzda za září", "cat_income"),
            ("Nájem říjen", "cat_housing"),
            ("Úrok z vkladu", "cat_income"),
            ("Elektřina záloha", "cat_utilities"),
            ("Záloha plyn", "cat_utilities"),
            ("Voda", "cat_utilities"),
            ("Pojištění auta", "cat_insurance"),
            ("Splátka úvěru", "cat_loan_payments"),
            ("Hypotéka splátka", "cat_loan_payments"),
            ("Lékárna U Anděla", "cat_health"),
            // German
            ("Gehalt Oktober", "cat_income"),
            ("Lohn 10/2026", "cat_income"),
            ("Zinsen Sparbuch", "cat_income"),
            ("Wasser Abschlag", "cat_utilities"),
            ("Versicherung Haftpflicht", "cat_insurance"),
            ("Darlehen Tilgung", "cat_loan_payments"),
            ("Apotheke am Markt", "cat_health"),
            // French
            ("Salaire mars", "cat_income"),
            ("Loyer avril", "cat_housing"),
            ("Intérêts créditeurs", "cat_income"),
            ("Facture électricité", "cat_utilities"),
            ("Assurance habitation", "cat_insurance"),
            ("Pharmacie du centre", "cat_health"),
            // Spanish
            ("Nómina marzo", "cat_income"),
            ("Alquiler piso", "cat_housing"),
            ("Intereses", "cat_income"),
            ("Factura electricidad", "cat_utilities"),
            ("Seguro de coche", "cat_insurance"),
            ("Préstamo personal", "cat_loan_payments"),
            ("Hipoteca cuota", "cat_loan_payments"),
            ("Farmacia López", "cat_health"),
            // Italian
            ("Stipendio marzo", "cat_income"),
            ("Affitto aprile", "cat_housing"),
            ("Interessi attivi", "cat_income"),
            ("Bolletta elettricità", "cat_utilities"),
            ("Assicurazione auto", "cat_insurance"),
            ("Rata mutuo", "cat_loan_payments"),
            ("Farmacia Rossi", "cat_health"),
            // Dutch
            ("Salaris maart", "cat_income"),
            ("Huur april", "cat_housing"),
            ("Elektriciteit", "cat_utilities"),
            ("Zorgverzekering", "cat_insurance"),
            ("Persoonlijke lening", "cat_loan_payments"),
            ("Hypotheek", "cat_loan_payments"),
            ("Apotheek centrum", "cat_health"),
            // Polish
            ("Wynagrodzenie za marzec", "cat_income"),
            ("Pensja", "cat_income"),
            ("Czynsz", "cat_housing"),
            ("Odsetki", "cat_income"),
            ("Prąd", "cat_utilities"),
            ("Ubezpieczenie OC", "cat_insurance"),
            ("Kredyt hipoteczny", "cat_loan_payments"),
            ("Apteka Gemini", "cat_health"),
        ];
        for (text, expected) in cases {
            assert_eq!(
                category_of(&engine, text).as_deref(),
                Some(expected),
                "{text:?} should be {expected}"
            );
        }
    }

    #[test]
    fn a_merchant_rule_beats_a_generic_keyword() {
        let engine = global_pack_engine();
        // "booking.com" (travel, priority 50) and "rent" (housing, priority 30) both match.
        assert_eq!(
            category_of(&engine, "Booking.com apartment rent").as_deref(),
            Some("cat_travel")
        );
        // Without the merchant, the generic keyword decides.
        assert_eq!(
            category_of(&engine, "Apartment rent").as_deref(),
            Some("cat_housing")
        );
    }

    #[test]
    fn generic_keywords_stay_away_from_look_alikes() {
        let engine = global_pack_engine();
        for text in [
            "Lohnsteuer 2025",        // tax, not salary
            "Kreditkarte Abrechnung", // credit card statement, not a loan
            "Pret A Manger",          // a sandwich shop, not French "prêt"
            "Waterstones Bookshop",   // "water" only as a whole word
            "Výplata z bankomatu",    // ATM withdrawal, not salary
            "Rentenversicherung",     // "rente" is pension in German, interest in Dutch: left out
        ] {
            let category = category_of(&engine, text);
            assert!(
                !matches!(
                    category.as_deref(),
                    Some("cat_income" | "cat_loan_payments" | "cat_utilities")
                ),
                "{text:?} must not be caught by a generic keyword, got {category:?}"
            );
        }
    }

    #[test]
    fn generic_keyword_guards_win_over_the_words_they_protect() {
        let engine = global_pack_engine();
        // US "gas" is fuel.
        assert_eq!(
            category_of(&engine, "Gas Station 5th Avenue").as_deref(),
            Some("cat_transport")
        );
        // Interest you pay is not income; loan rules also outrank the bare word.
        assert_eq!(
            category_of(&engine, "Overdraft interest charged").as_deref(),
            Some("cat_loan_payments")
        );
        assert_eq!(
            category_of(&engine, "Loan interest").as_deref(),
            Some("cat_loan_payments")
        );
    }

    #[test]
    fn generic_keywords_rank_below_every_merchant_rule() {
        let rules = super::super::packs::available_packs()
            .iter()
            .flat_map(|pack| pack.rules.iter())
            .collect::<Vec<_>>();
        let generic: Vec<_> = rules
            .iter()
            .filter(|r| r.id.starts_with("global/kw-"))
            .collect();
        assert!(generic.len() >= 70, "expected the generic keyword set");
        let highest_generic = generic.iter().map(|r| r.priority).max().unwrap();
        let lowest_merchant = rules
            .iter()
            .filter(|r| !r.id.starts_with("global/kw-"))
            .map(|r| r.priority)
            .min()
            .unwrap();
        assert!(
            highest_generic < lowest_merchant,
            "generic keywords ({highest_generic}) must rank below merchant rules ({lowest_merchant})"
        );
    }

    // ==================== Own-account IBAN normalization ====================

    fn tx_with_iban(description: &str, iban: &str) -> TransactionInput {
        TransactionInput {
            id: "tx1".into(),
            description: Some(description.into()),
            counterparty: None,
            counterparty_iban: Some(iban.into()),
            amount: 3200.0,
            is_credit: true,
            bank_account_id: None,
        }
    }

    #[test]
    fn own_iban_matches_across_spacing_case_and_exotic_whitespace() {
        // The audit's "Salary September" with a counterparty IBAN equal to the
        // account's own IBAN: both sides are normalized before comparing.
        let engine = global_pack_engine();
        engine.set_own_ibans(vec!["DE89 3704 0044 0532 0130 00".into()]);

        for counterparty in [
            "de89370400440532013000",
            "DE89 3704 0044 0532 0130 00",
            "de89\u{a0}3704\u{a0}0044 0532\t0130 00",
            "  DE89370400440532013000  ",
        ] {
            let result = engine.categorize(&tx_with_iban("Salary September", counterparty));
            assert_eq!(
                result.category_id(),
                Some("cat_internal_transfers"),
                "{counterparty:?} must be recognised as the user's own account"
            );
        }
    }

    #[test]
    fn own_iban_is_stored_normalized_so_either_side_may_carry_the_noise() {
        let engine = global_pack_engine();
        engine.set_own_ibans(vec!["de89370400440532013000".into(), "  ".into()]);
        let tx = tx_with_iban("Transfer to savings", "DE89 3704 0044 0532 0130 00");
        assert_eq!(
            engine.categorize(&tx).category_id(),
            Some("cat_internal_transfers")
        );
        // A different account is not an internal transfer.
        let other = tx_with_iban("Salary September", "DE89 3704 0044 0532 0130 01");
        assert_eq!(engine.categorize(&other).category_id(), Some("cat_income"));
    }

    #[test]
    fn test_export_import_payees() {
        let engine1 = CategorizationEngine::new(vec![]);
        engine1.learn_from_user_simple("Test Payee", "cat_other");

        let exported = engine1.export_learned_payees();
        assert!(!exported.is_empty());

        let engine2 = CategorizationEngine::new(vec![]);
        engine2.import_learned_payees(exported);

        let tx = make_tx("", Some("Test Payee"));
        assert!(engine2.categorize(&tx).has_category());
    }

    #[test]
    fn reassign_category_moves_custom_rules_and_learned_payees() {
        let engine = CategorizationEngine::new(vec![]);
        engine.update_custom_rules(vec![CategorizationRule {
            id: "r1".into(),
            name: "Zoo".into(),
            rule_type: RuleType::Contains,
            pattern: "zoo".into(),
            category_id: "custom-pets".into(),
            priority: 50,
            is_active: true,
            stop_processing: false,
            iban_pattern: None,
        }]);
        engine.learn_from_user_simple("Happy Vet", "custom-pets");
        engine.learn_from_user_simple("Albert", "cat_groceries");

        let moved = engine.reassign_category("custom-pets", "cat_other");
        assert_eq!(moved, 2, "one custom rule and one learned payee");

        assert_eq!(
            engine.categorize(&make_tx("Zoo Praha", None)).category_id(),
            Some("cat_other")
        );
        assert_eq!(
            engine
                .categorize(&make_tx("", Some("Happy Vet")))
                .category_id(),
            Some("cat_other")
        );
        // Entries for other categories are left alone.
        assert_eq!(
            engine
                .categorize(&make_tx("", Some("Albert")))
                .category_id(),
            Some("cat_groceries")
        );
    }

    #[test]
    fn test_forget_payee() {
        let engine = CategorizationEngine::new(vec![]);
        engine.learn_from_user_simple("Test Payee", "cat_other");

        let tx = make_tx("", Some("Test Payee"));
        assert!(engine.categorize(&tx).has_category());

        engine.forget_payee_simple("Test Payee");
        assert!(!engine.categorize(&tx).has_category());
    }
}
