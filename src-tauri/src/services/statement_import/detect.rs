//! Content-based format detection (D4) and the two file guards (D16). The
//! extension is never consulted: a file that is none of the four formats goes to
//! the CSV path unchanged.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::json;

use crate::error::{AppError, Result};

use super::model::StatementFormat;

/// Bytes of a file `detect` looks at.
pub const DETECT_BYTES: usize = 4096;

/// Larger statement files are refused.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

const UNSUPPORTED_VARIANT: &str = "validation.statementUnsupportedVariant";

/// One namespace declaration, with or without a prefix (`xmlns="…"`, `xmlns:ns2='…'`).
static XMLNS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"xmlns(?::[a-z0-9._-]+)?\s*=\s*["']([^"']*)["']"#)
        .expect("static pattern is valid")
});

/// The ISO 20022 statement-family message in a namespace value (lower-cased input).
static CAMT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"camt\.(05[234])\.").expect("static pattern is valid"));

/// The statement format of a file, from the first [`DETECT_BYTES`] bytes (§7.1).
///
/// `Ok(None)` means the file is not a statement (it takes the CSV path). `Err` is a
/// statement variant Moony recognizes and refuses (camt.052/054, MT942, MT950).
pub fn detect(head: &[u8]) -> Result<Option<StatementFormat>> {
    let head = &head[..head.len().min(DETECT_BYTES)];
    let lower = String::from_utf8_lossy(head).to_lowercase();
    let text = lower.trim_start_matches(|c: char| c == '\u{feff}' || c.is_whitespace());

    if text.contains("ofxheader:") || text.contains("<ofx>") {
        return Ok(Some(StatementFormat::Ofx));
    }
    if text.starts_with('<') {
        return detect_xml(text);
    }
    // The text starts with a non-blank character, so its first line is the first non-empty one.
    let first_line = text.lines().next().unwrap_or("");
    if ["!type:", "!account", "!option:"]
        .iter()
        .any(|marker| first_line.starts_with(marker))
    {
        return Ok(Some(StatementFormat::Qif));
    }
    detect_swift(text)
}

/// Any XML that is not a camt.053 document is not ours (`None`); camt.052/054 are refused.
fn detect_xml(text: &str) -> Result<Option<StatementFormat>> {
    for declaration in XMLNS.captures_iter(text) {
        if let Some(message) = CAMT.captures(&declaration[1]) {
            return match &message[1] {
                "053" => Ok(Some(StatementFormat::Camt053)),
                other => Err(unsupported_variant(&format!("camt.{other}"))),
            };
        }
    }
    Ok(None)
}

/// MT940: lines `:20:` and `:25:` plus a `:61:`/`:60F:`/`:60M:` line, the block optionally
/// opened by `{4:` (on its own line or in front of `:20:`).
fn detect_swift(text: &str) -> Result<Option<StatementFormat>> {
    let (mut reference, mut account, mut movement, mut interim) = (false, false, false, false);
    for line in text.lines() {
        let after_block = line.split_once("{4:").map(|(_, tail)| tail);
        for line in std::iter::once(line).chain(after_block) {
            reference |= line.starts_with(":20:");
            account |= line.starts_with(":25:");
            movement |=
                line.starts_with(":61:") || line.starts_with(":60f:") || line.starts_with(":60m:");
            interim |= line.starts_with(":34f:");
        }
    }
    if !(reference && account && movement) {
        return Ok(None);
    }
    if interim {
        return Err(unsupported_variant("MT942"));
    }
    if text.contains("{2:i950") || text.contains("{2:o950") {
        return Err(unsupported_variant("MT950"));
    }
    Ok(Some(StatementFormat::Mt940))
}

fn unsupported_variant(variant: &str) -> AppError {
    AppError::validation_with(UNSUPPORTED_VARIANT, json!({ "variant": variant }))
}

/// Refuses a statement file above [`MAX_FILE_BYTES`].
pub fn check_size(len: u64) -> Result<()> {
    if len > MAX_FILE_BYTES {
        return Err(AppError::validation_with(
            "validation.statementTooLarge",
            json!({ "limitMb": MAX_FILE_BYTES / (1024 * 1024) }),
        ));
    }
    Ok(())
}

/// Refuses XML with a document type declaration (entity definitions); case-insensitive.
pub fn reject_doctype(text: &str) -> Result<()> {
    const DOCTYPE: &[u8] = b"<!doctype";
    if text
        .as_bytes()
        .windows(DOCTYPE.len())
        .any(|window| window.eq_ignore_ascii_case(DOCTYPE))
    {
        return Err(AppError::Validation("validation.statementDoctype".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(s: &str) -> Option<StatementFormat> {
        detect(s.as_bytes()).unwrap()
    }
    fn refused(s: &str) -> String {
        detect(s.as_bytes()).unwrap_err().to_string()
    }

    #[test]
    fn detects_each_format() {
        assert_eq!(
            det("OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>"),
            Some(StatementFormat::Ofx)
        );
        assert_eq!(
            det("<?xml version=\"1.0\"?><?OFX OFXHEADER=\"200\"?><OFX>"),
            Some(StatementFormat::Ofx)
        );
        assert_eq!(
            det("\u{feff}  <?xml version=\"1.0\"?>\n<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.08\">"),
            Some(StatementFormat::Camt053)
        );
        assert_eq!(det("!Type:Bank\nD1/2'26\n^"), Some(StatementFormat::Qif));
        assert_eq!(det("!account\nNChecking\n^"), Some(StatementFormat::Qif));
        assert_eq!(
            det(":20:X\r\n:25:CZ6508000000192000145399\r\n:28C:1\r\n:60F:C260831CZK1,00\r\n"),
            Some(StatementFormat::Mt940)
        );
        assert_eq!(
            det("{1:F01BANKXXXX}{2:O940}{4:\n:20:X\n:25:Y\n:61:2609050905C1,NTRFNONREF\n-}"),
            Some(StatementFormat::Mt940)
        );
    }

    #[test]
    fn csv_and_other_text_are_not_statements() {
        assert_eq!(det("Date;Amount;Note:20:x\n2026-01-01;5;a"), None);
        assert_eq!(det("<html><body>"), None);
        assert_eq!(det(""), None);
    }

    #[test]
    fn refuses_other_variants() {
        assert!(refused(
            "<?xml?><Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.052.001.02\">"
        )
        .contains("validation.statementUnsupportedVariant"));
        assert!(
            refused(":20:X\n:25:Y\n:34F:CZK0,\n:61:2609050905C1,NTRFX\n")
                .contains("validation.statementUnsupportedVariant")
        );
    }

    #[test]
    fn guards() {
        assert!(check_size(MAX_FILE_BYTES).is_ok());
        assert!(check_size(MAX_FILE_BYTES + 1)
            .unwrap_err()
            .to_string()
            .contains("validation.statementTooLarge"));
        assert!(reject_doctype("<?xml?><!DOCTYPE x [<!ENTITY a \"b\">]><x/>").is_err());
    }

    fn variant(s: &str) -> serde_json::Value {
        let (key, params) = detect(s.as_bytes()).unwrap_err().keyed().expect("keyed");
        assert_eq!(key, "validation.statementUnsupportedVariant");
        params["variant"].clone()
    }

    #[test]
    fn names_the_refused_variant() {
        let camt = |n: &str| {
            format!("<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.{n}.001.02\">")
        };
        assert_eq!(variant(&camt("052")), "camt.052");
        assert_eq!(variant(&camt("054")), "camt.054");
        assert_eq!(
            variant(":20:X\n:25:Y\n:34F:CZK0,\n:61:2609050905C1,NTRFX\n"),
            "MT942"
        );
        assert_eq!(
            variant("{1:F01BANKXXXX}{2:I950BANKYYYY}{4:\n:20:X\n:25:Y\n:60F:C260831CZK1,00\n-}"),
            "MT950"
        );
    }

    #[test]
    fn camt_namespace_may_be_prefixed_or_follow_another_declaration() {
        let ns = "urn:iso:std:iso:20022:tech:xsd:camt.053.001.02";
        assert_eq!(
            det(&format!("<ns2:Document xmlns:ns2=\"{ns}\">")),
            Some(StatementFormat::Camt053)
        );
        assert_eq!(
            det(&format!(
                "<Document xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance' xmlns='{ns}'>"
            )),
            Some(StatementFormat::Camt053)
        );
        assert_eq!(
            det("<?xml version=\"1.0\"?><root xmlns=\"urn:other\"><a/>"),
            None
        );
    }

    #[test]
    fn block_four_may_share_a_line_with_the_first_field() {
        assert_eq!(
            det("{1:F01BANKXXXX}{2:O940BANKYYYY}{4::20:X\n:25:Y\n:60F:C260831CZK1,00\n-}"),
            Some(StatementFormat::Mt940)
        );
        // `:20:` and `:25:` without any balance or movement line are not a statement.
        assert_eq!(det(":20:X\n:25:Y\n:28C:1\n"), None);
    }

    #[test]
    fn qif_marker_is_the_first_non_empty_line() {
        assert_eq!(
            det("\n\n  !Option:AutoSwitch\n!Account\n"),
            Some(StatementFormat::Qif)
        );
        assert_eq!(det("Date,Amount\n!Type:Bank\n"), None);
    }

    #[test]
    fn bytes_that_are_not_utf8_or_are_cut_mid_character_do_not_fail() {
        let (head, _, _) =
            encoding_rs::WINDOWS_1250.encode(":20:Žluťoučký\n:25:Y\n:60F:C260831CZK1,00\n");
        assert_eq!(detect(&head).unwrap(), Some(StatementFormat::Mt940));
        let mut cut = "!Type:Bank\nPŘÍJEM".as_bytes().to_vec();
        cut.pop();
        assert_eq!(detect(&cut).unwrap(), Some(StatementFormat::Qif));
    }

    #[test]
    fn only_the_first_detect_bytes_count() {
        let mut late = " ".repeat(DETECT_BYTES);
        late.push_str("!Type:Bank\n");
        // The window is cut first, so a marker past it is never seen.
        assert_eq!(det(&late), None);
    }

    #[test]
    fn guard_details() {
        let (key, params) = check_size(MAX_FILE_BYTES + 1)
            .unwrap_err()
            .keyed()
            .expect("keyed");
        assert_eq!(key, "validation.statementTooLarge");
        assert_eq!(params, serde_json::json!({ "limitMb": 50 }));
        assert!(check_size(0).is_ok());
        assert!(reject_doctype("<x><!doctype y></x>").is_err());
        assert!(
            reject_doctype("<?xml version=\"1.0\"?><Document><Nm>a &amp; b</Nm></Document>")
                .is_ok()
        );
        assert_eq!(
            reject_doctype("<!DocType x>")
                .unwrap_err()
                .keyed()
                .expect("keyed")
                .0,
            "validation.statementDoctype"
        );
    }
}
