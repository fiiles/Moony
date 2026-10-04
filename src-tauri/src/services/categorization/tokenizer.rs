//! Text normalization for payee matching
//!
//! This module provides text processing for the categorization engine:
//! - Diacritics stripping (ř → r, ě → e, etc.)
//! - Payee name normalization (company suffixes, bank payment prefixes)

use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

static WHITESPACE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("Invalid whitespace regex"));

/// Runs of three or more ASCII digits: transaction numbers, card and
/// reference ids. Shorter runs are usually part of a name ("O2", "7-Eleven").
static DIGIT_RUN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9]{3,}").expect("Invalid digit-run regex"));

/// Separators that can dangle at the end of a payee once a reference is cut.
const TRAILING_SEPARATORS: &[char] = &['-', '_', '/', '#', '*', ':', ';', ',', '\\', '|'];

/// Fold text for matching: lowercase, NFD-decompose and strip combining
/// marks (Košík → kosik). Every other character — CJK, Hangul, Hebrew,
/// Arabic, digits, punctuation — is preserved, so native-script rule
/// patterns keep working.
pub fn fold_text(text: &str) -> String {
    text.to_lowercase()
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect()
}

/// Simple tokenization without stopword removal (for exact matching).
///
/// Built on [`fold_text`], keeping letters and digits of any script (a
/// Chinese payee no longer normalizes to an empty string) while dropping
/// punctuation. Pure-ASCII inputs normalize exactly as before, so existing
/// `learned_payees` keys stay valid.
pub fn simple_normalize(text: &str) -> String {
    let folded = fold_text(text);
    // Punctuation is dropped without a replacement space — "s.r.o." → "sro" —
    // exactly as the previous ASCII-only implementation did, so existing
    // learned_payees keys stay stable.
    let normalized: String = folded
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    WHITESPACE_REGEX
        .replace_all(&normalized, " ")
        .trim()
        .to_string()
}

/// Czech company suffixes to strip for better payee matching
const CZECH_COMPANY_SUFFIXES: &[&str] = &[
    "s r o", "sro", "a s", "as", "spol", "k s", "ks", "v o s", "vos", "o p s", "ops", "se", "z s",
    "zs", "inc", "ltd", "gmbh", "sp z o o", "sp zoo",
];

/// Common payment prefixes often added by banks
const PAYMENT_PREFIXES: &[&str] = &[
    "platba",
    "platba kartou",
    "bezhotovostni platba",
    "prichozi platba",
    "odchozi platba",
    "prevod",
    "inkaso",
    "trvaly prikaz",
];

/// Normalize payee name for matching
///
/// This function provides more aggressive normalization for payee matching:
/// 1. Uses simple_normalize for basic normalization
/// 2. Strips common Czech company suffixes (s.r.o., a.s., etc.)
/// 3. Strips common payment prefixes from banks
///
/// # Example
/// ```
/// use moony_lib::services::categorization::tokenizer::normalize_payee;
///
/// assert_eq!(normalize_payee("ABC Company s.r.o."), "abc company");
/// assert_eq!(normalize_payee("XYZ a.s."), "xyz");
/// assert_eq!(normalize_payee("Albert CZ spol. s r.o."), "albert cz spol s ro");
/// ```
pub fn normalize_payee(text: &str) -> String {
    let mut normalized = simple_normalize(text);

    // Strip payment prefixes from the start
    for prefix in PAYMENT_PREFIXES {
        if normalized.starts_with(prefix) {
            normalized = normalized[prefix.len()..].trim_start().to_string();
        }
    }

    // Strip company suffixes from the end
    for suffix in CZECH_COMPANY_SUFFIXES {
        if normalized.ends_with(suffix) {
            normalized = normalized[..normalized.len() - suffix.len()]
                .trim_end()
                .to_string();
            break; // Only strip one suffix
        }
    }

    normalized.trim().to_string()
}

/// Key for learned-payee lookups (RUL-02): the counterparty text with
/// everything that varies per transaction removed, so "AUSTRIAN AIR2572170483655"
/// and "AUSTRIAN AIR2572170484001" are one payee.
///
/// 1. [`fold_text`]: lowercase, diacritics folded.
/// 2. Runs of three or more digits (transaction numbers, card/reference ids)
///    are removed wherever they sit, glued to letters or not.
/// 3. Trailing reference tokens — digits with separators such as `03`, `/05`,
///    `#12` — are dropped, along with trailing separators.
/// 4. Whitespace is collapsed and trimmed.
///
/// Punctuation inside the name is kept ("DR. MAX 03" → "dr. max"); lookups
/// also try [`strip_punctuation`] of the key so rows written before this
/// function existed (punctuation already stripped) keep matching. The result
/// is idempotent: normalizing a key returns the key.
pub fn normalize_payee_key(text: &str) -> String {
    let folded = fold_text(text);
    let without_runs = DIGIT_RUN_REGEX.replace_all(&folded, " ");
    let mut tokens: Vec<&str> = without_runs.split_whitespace().collect();

    loop {
        // Separators dangling after a removed run ("tesco stores-" → "tesco stores").
        while let Some(last) = tokens.last().copied() {
            let trimmed = last.trim_end_matches(TRAILING_SEPARATORS);
            if trimmed.is_empty() {
                tokens.pop();
            } else if trimmed.len() != last.len() {
                // Keep the token, minus its dangling separators.
                let len = tokens.len();
                tokens[len - 1] = trimmed;
                break;
            } else {
                break;
            }
        }
        match tokens.last() {
            Some(last) if is_reference_token(last) => {
                tokens.pop();
            }
            _ => break,
        }
    }

    tokens.join(" ")
}

/// A trailing token that is only a reference: it holds a digit and nothing
/// but digits and separators, or it is a `#`-prefixed id.
fn is_reference_token(token: &str) -> bool {
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let only_digits_and_separators = token
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '#' | '/' | '-' | '_' | '.' | ':' | '*'));
    (has_digit && only_digits_and_separators)
        || (token.starts_with('#') && token.chars().count() > 1)
}

/// The key with punctuation removed the way [`simple_normalize`] does
/// ("dr. max" → "dr max", "t-mobile" → "tmobile"): how `learned_payees` keys
/// were written before [`normalize_payee_key`] kept punctuation.
pub fn strip_punctuation(key: &str) -> String {
    simple_normalize(key)
}

/// Canonical form of an IBAN (or any account number) for comparisons and
/// keys: every whitespace character (including non-breaking and zero-width
/// spaces that bank exports love) removed, uppercased.
pub fn normalize_iban(iban: &str) -> String {
    iban.chars()
        .filter(|c| {
            !c.is_whitespace() && !matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}')
        })
        .flat_map(char::to_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== normalize_payee_key (RUL-02) ====================

    #[test]
    fn sixteen_flight_references_collapse_to_one_key() {
        // The audit's 16 separate learned rules for one airline.
        let variants: Vec<String> = (0..16)
            .map(|i| format!("AUSTRIAN AIR25721704836{:02}", 55 + i))
            .collect();
        let keys: std::collections::HashSet<String> =
            variants.iter().map(|v| normalize_payee_key(v)).collect();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys.into_iter().next().unwrap(), "austrian air");
        // A space before the number or a different length is the same payee.
        assert_eq!(normalize_payee_key("Austrian Air 9999"), "austrian air");
        assert_eq!(normalize_payee_key("AUSTRIAN AIR9999"), "austrian air");
    }

    #[test]
    fn branch_numbers_and_punctuation() {
        assert_eq!(normalize_payee_key("DR. MAX 03"), "dr. max");
        assert_eq!(normalize_payee_key("Studio 54"), "studio");
        assert_eq!(
            normalize_payee_key("Müller Drogerie #12"),
            "muller drogerie"
        );
        assert_eq!(normalize_payee_key("TESCO STORES-4821"), "tesco stores");
        assert_eq!(normalize_payee_key("UBER *TRIP 1234567"), "uber *trip");
        assert_eq!(normalize_payee_key("Albert 1234 Praha"), "albert praha");
        assert_eq!(normalize_payee_key("Košík.cz"), "kosik.cz");
        assert_eq!(
            normalize_payee_key("  Multiple   spaces  "),
            "multiple spaces"
        );
    }

    #[test]
    fn digits_that_belong_to_the_name_survive() {
        assert_eq!(normalize_payee_key("7-Eleven"), "7-eleven");
        assert_eq!(
            normalize_payee_key("O2 Czech Republic"),
            "o2 czech republic"
        );
        assert_eq!(normalize_payee_key("3M Company"), "3m company");
        assert_eq!(normalize_payee_key("Pizza 4 Ps Cafe"), "pizza 4 ps cafe");
    }

    #[test]
    fn a_payee_that_is_only_a_reference_has_no_key() {
        assert_eq!(normalize_payee_key("2572170483655"), "");
        assert_eq!(normalize_payee_key("  12 / 05 "), "");
        assert_eq!(normalize_payee_key(""), "");
    }

    #[test]
    fn payee_key_is_idempotent() {
        for text in [
            "AUSTRIAN AIR2572170483655",
            "DR. MAX 03",
            "abc 12 -",
            "TESCO STORES-4821 /05",
            "Müller Drogerie #12",
            "7-Eleven",
            "",
        ] {
            let once = normalize_payee_key(text);
            assert_eq!(
                normalize_payee_key(&once),
                once,
                "not idempotent for {text:?}"
            );
        }
    }

    #[test]
    fn strip_punctuation_reproduces_the_legacy_key() {
        assert_eq!(strip_punctuation("dr. max"), "dr max");
        assert_eq!(strip_punctuation("t-mobile"), "tmobile");
        assert_eq!(strip_punctuation("albert s.r.o."), "albert sro");
    }

    // ==================== normalize_iban ====================

    #[test]
    fn iban_normalization_ignores_case_and_every_kind_of_whitespace() {
        assert_eq!(
            normalize_iban("DE89 3704 0044 0532 0130 00"),
            normalize_iban("de89370400440532013000")
        );
        assert_eq!(
            normalize_iban("DE89 3704 0044 0532 0130 00"),
            "DE89370400440532013000"
        );
        // non-breaking space, tab, zero-width space
        assert_eq!(
            normalize_iban("  de89\u{a0}3704\t0044\u{200b}0532 0130 00 "),
            "DE89370400440532013000"
        );
        assert_eq!(normalize_iban(""), "");
    }

    #[test]
    fn test_simple_normalize() {
        let result = simple_normalize("  ALBERT  CZ  Praha  ");
        assert_eq!(result, "albert cz praha");
    }

    #[test]
    fn test_simple_normalize_strips_diacritics() {
        assert_eq!(simple_normalize("žluťoučký kůň"), "zlutoucky kun");
    }

    #[test]
    fn test_simple_normalize_keeps_non_latin_scripts() {
        assert_eq!(simple_normalize("美团 Meituan"), "美团 meituan");
    }

    #[test]
    fn test_fold_text() {
        assert_eq!(fold_text("Košík.cz"), "kosik.cz");
        assert_eq!(fold_text("永辉超市"), "永辉超市");
        assert_eq!(fold_text("MÜLLER"), "muller");
    }

    #[test]
    fn test_empty_input() {
        assert!(simple_normalize("").is_empty());
    }

    #[test]
    fn test_normalize_payee_strips_sro() {
        let result = normalize_payee("ABC Company s.r.o.");
        assert_eq!(result, "abc company");
    }

    #[test]
    fn test_normalize_payee_strips_as() {
        let result = normalize_payee("XYZ a.s.");
        assert_eq!(result, "xyz");
    }

    #[test]
    fn test_normalize_payee_strips_spol() {
        let result = normalize_payee("Albert CZ spol.");
        assert_eq!(result, "albert cz");
    }

    #[test]
    fn test_normalize_payee_strips_payment_prefix() {
        let result = normalize_payee("prevod Albert");
        assert_eq!(result, "albert");
    }

    #[test]
    fn test_normalize_payee_case_insensitive() {
        let result = normalize_payee("ALBERT HYPERMARKET S.R.O.");
        assert_eq!(result, "albert hypermarket");
    }
}
