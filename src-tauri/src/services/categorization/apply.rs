//! Definitive-only resolver: the seam between the categorization engine and
//! the import write paths.
//!
//! The engine's four deterministic layers (custom rules → learned payee exact
//! match → own-account IBAN → default rule packs) return
//! `CategorizationResult::Match`, which an import may apply unattended;
//! anything else stays uncategorized for a human or an LLM to decide. This
//! module is the only place that draws that line, so no write path has to
//! remember it.

use super::types::{CategorizationResult, CategorizationSource};
use super::{CategorizationEngine, TransactionInput};
use crate::models::InsertBankTransaction;

/// `bank_transactions.categorization_source` for a custom or system rule match.
pub const SOURCE_RULE: &str = "rule";
/// `bank_transactions.categorization_source` for a learned-payee exact match.
pub const SOURCE_EXACT_MATCH: &str = "exact_match";
/// `bank_transactions.categorization_source` for an own-account IBAN match.
pub const SOURCE_OWN_ACCOUNT: &str = "own_account";
/// `bank_transactions.categorization_source` for a category an MCP client
/// assigned. Not produced here — the engine never returns it — but kept with
/// the others so the column's whole vocabulary is in one place (spec D3).
/// NULL remains the fourth value and means "no recorded provenance".
pub const SOURCE_MCP: &str = "mcp";

/// The engine reports its own-account IBAN layer as a `Rule` carrying this
/// synthetic rule id (see `engine::CategorizationEngine::categorize`), so the
/// id is the only way to tell that layer apart from a real rule.
const OWN_ACCOUNT_IBAN_RULE_ID: &str = "own_account_iban";

/// Map a categorization result to the `(category_id, categorization_source)`
/// pair an import may auto-apply, or `None` when the row must stay
/// uncategorized for a human or an LLM to decide.
///
/// Split out from [`resolve_definitive`] so the mapping is unit-testable on
/// hand-built results without an engine.
pub fn definitive_match(result: &CategorizationResult) -> Option<(String, &'static str)> {
    let CategorizationResult::Match {
        category_id,
        source,
    } = result
    else {
        return None;
    };
    let label = match source {
        CategorizationSource::Rule { rule_id, .. } if rule_id == OWN_ACCOUNT_IBAN_RULE_ID => {
            SOURCE_OWN_ACCOUNT
        }
        CategorizationSource::Rule { .. } => SOURCE_RULE,
        CategorizationSource::ExactMatch { .. } => SOURCE_EXACT_MATCH,
        // Unreachable from `categorize()`: Manual is only ever set by the
        // user. Treated as non-definitive rather than guessed at, so a future
        // engine change cannot silently start auto-applying it.
        CategorizationSource::Manual => return None,
    };
    Some((category_id.clone(), label))
}

/// Run the engine's waterfall over one transaction and return only what an
/// import is allowed to apply unattended.
pub fn resolve_definitive(
    engine: &CategorizationEngine,
    tx: &TransactionInput,
) -> Option<(String, &'static str)> {
    definitive_match(&engine.categorize(tx))
}

/// Build the engine's input from a row about to be written. Mirrors the
/// frontend's `BankTransaction` → `TransactionInput` mapping
/// (`src/hooks/useCategorization.ts`), so a row categorized at import time
/// gets the same answer the app's own auto-categorize button would give.
pub fn transaction_input_from_insert(data: &InsertBankTransaction) -> TransactionInput {
    TransactionInput {
        // The row has no id yet. The waterfall never reads it — it matches on
        // text and IBAN only.
        id: String::new(),
        description: data.description.clone(),
        counterparty: data.counterparty_name.clone(),
        counterparty_iban: data.counterparty_iban.clone(),
        // Amounts are stored positive with the direction in tx_type. `validate()`
        // also parses this field, but its ordering relative to this call is not
        // identical on both import paths (compare services/csv_import.rs, which
        // validates each row before enrichment, with services/mcp/bank_accounts.rs,
        // where the specific struct this call enriches isn't validated until
        // afterwards, inside create_transaction_with_origin) — so
        // `unwrap_or(0.0)` is a real fallback, not dead code. It is harmless
        // regardless: no `RuleType` matches on amount.
        amount: data.amount.parse().unwrap_or(0.0),
        is_credit: data.tx_type.eq_ignore_ascii_case("credit"),
        bank_account_id: Some(data.bank_account_id.clone()),
    }
}

/// Pre-write enrichment for the import paths (spec D2): a row the caller left
/// uncategorized gets the app's own deterministic categorization. Returns the
/// `categorization_source` label to store alongside it, or `None` when the row
/// must stay uncategorized.
///
/// A row that arrives WITH a `category_id` is never touched — the caller was
/// explicit. `engine == None` (unit tests, or a missing engine state) disables
/// enrichment entirely rather than failing the import.
///
/// Not to be confused with `bank_accounts::enrich_with_rules`, its only
/// caller: that one is DB-checked (drops a match onto a since-deleted
/// category) and returns `Result`; this one is pure and returns `Option`.
/// `pub(crate)` (rather than the wider `pub` `enrich_uncategorized` used to
/// have) so `enrich_with_rules` — the checked wrapper — stays the easy thing
/// to reach for from outside this module.
pub(crate) fn resolve_and_apply(
    data: &mut InsertBankTransaction,
    engine: Option<&CategorizationEngine>,
) -> Option<&'static str> {
    if data.category_id.is_some() {
        return None;
    }
    let (category_id, source) = resolve_definitive(engine?, &transaction_input_from_insert(data))?;
    data.category_id = Some(category_id);
    Some(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::categorization::types::{CategorizationRule, RuleType};

    fn tx(description: &str, counterparty: Option<&str>, iban: Option<&str>) -> TransactionInput {
        TransactionInput {
            id: "tx1".into(),
            description: Some(description.into()),
            counterparty: counterparty.map(String::from),
            counterparty_iban: iban.map(String::from),
            amount: 500.0,
            is_credit: false,
            bank_account_id: None,
        }
    }

    fn albert_rule() -> CategorizationRule {
        CategorizationRule::new(
            "r1".into(),
            "Albert".into(),
            RuleType::Contains,
            "albert".into(),
            "cat_groceries".into(),
        )
    }

    #[test]
    fn resolves_rule_match_as_rule() {
        let engine = CategorizationEngine::new(vec![albert_rule()]);
        let resolved = resolve_definitive(&engine, &tx("Platba kartou Albert", None, None));
        assert_eq!(resolved, Some(("cat_groceries".to_string(), SOURCE_RULE)));
    }

    #[test]
    fn resolves_learned_payee_as_exact_match() {
        let engine = CategorizationEngine::new(vec![]);
        engine.learn_from_user_simple("Uber Eats", "cat_dining");
        let resolved = resolve_definitive(&engine, &tx("", Some("Uber Eats"), None));
        assert_eq!(
            resolved,
            Some(("cat_dining".to_string(), SOURCE_EXACT_MATCH))
        );
    }

    #[test]
    fn resolves_own_account_iban_as_own_account() {
        let engine = CategorizationEngine::new(vec![]);
        engine.set_own_ibans(vec!["CZ6508000000192000145399".into()]);
        let resolved = resolve_definitive(
            &engine,
            &tx("transfer", None, Some("CZ65 0800 0000 1920 0014 5399")),
        );
        assert_eq!(
            resolved,
            Some(("cat_internal_transfers".to_string(), SOURCE_OWN_ACCOUNT)),
            "the own-account layer must not be mislabeled as a plain rule"
        );
    }

    #[test]
    fn no_match_resolves_to_none() {
        let engine = CategorizationEngine::new(vec![]);
        assert_eq!(
            resolve_definitive(&engine, &tx("Unknown merchant XYZ", None, None)),
            None
        );
    }

    #[test]
    fn no_result_is_never_applied() {
        assert_eq!(definitive_match(&CategorizationResult::None), None);
    }

    fn insert(description: &str, tx_type: &str) -> InsertBankTransaction {
        InsertBankTransaction {
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: tx_type.into(),
            amount: "250.50".into(),
            currency: None,
            description: Some(description.into()),
            counterparty_name: None,
            counterparty_iban: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,
            status: None,
        }
    }

    #[test]
    fn transaction_input_mirrors_the_ui_mapping() {
        let input = transaction_input_from_insert(&insert("Albert", "CREDIT"));
        assert_eq!(input.description.as_deref(), Some("Albert"));
        assert_eq!(input.amount, 250.50, "stored amounts are positive");
        assert!(
            input.is_credit,
            "direction comes from tx_type, not the sign"
        );
        assert_eq!(input.bank_account_id.as_deref(), Some("acc-1"));
        assert!(!transaction_input_from_insert(&insert("Albert", "debit")).is_credit);
    }

    #[test]
    fn enrich_applies_a_rule_match_and_reports_its_label() {
        let engine = CategorizationEngine::new(vec![albert_rule()]);
        let mut data = insert("Platba kartou Albert", "debit");
        assert_eq!(resolve_and_apply(&mut data, Some(&engine)), Some("rule"));
        assert_eq!(data.category_id.as_deref(), Some("cat_groceries"));
    }

    #[test]
    fn enrich_leaves_unmatched_rows_uncategorized() {
        let engine = CategorizationEngine::new(vec![albert_rule()]);
        let mut data = insert("Unknown merchant XYZ", "debit");
        assert_eq!(resolve_and_apply(&mut data, Some(&engine)), None);
        assert!(data.category_id.is_none());
    }

    #[test]
    fn enrich_never_overrides_an_explicit_category() {
        let engine = CategorizationEngine::new(vec![albert_rule()]);
        let mut data = insert("Platba kartou Albert", "debit");
        data.category_id = Some("cat_caller_chose_this".into());
        assert_eq!(
            resolve_and_apply(&mut data, Some(&engine)),
            None,
            "an explicit category gets no provenance label"
        );
        assert_eq!(data.category_id.as_deref(), Some("cat_caller_chose_this"));
    }

    #[test]
    fn enrich_without_an_engine_is_a_no_op() {
        let mut data = insert("Platba kartou Albert", "debit");
        assert_eq!(resolve_and_apply(&mut data, None), None);
        assert!(data.category_id.is_none());
    }

    #[test]
    fn manual_source_inside_a_match_is_not_definitive() {
        // Not produced by today's `categorize()`, but the mapping must fail
        // closed rather than stamp an unknown provenance.
        assert_eq!(
            definitive_match(&CategorizationResult::Match {
                category_id: "cat_dining".into(),
                source: CategorizationSource::Manual,
            }),
            None
        );
    }
}
