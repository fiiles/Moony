//! Decoding of statement bytes into text (spec 7.2). Never fails: undecodable
//! bytes become U+FFFD, as in the CSV import.

use std::sync::LazyLock;

use encoding_rs::{Encoding, UTF_8};
use regex::Regex;

use crate::services::csv_import::decode::DecodedCsv;
use crate::services::csv_import::decode_csv_content_for_country;

use super::model::StatementFormat;

/// Bytes of an XML file searched for the declaration's `encoding`.
const XML_DECLARATION_BYTES: usize = 200;

/// Bytes of an OFX 1.x file searched for the `CHARSET`/`ENCODING` header lines.
const OFX_HEADER_BYTES: usize = 4096;

static XML_ENCODING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"encoding\s*=\s*["']([A-Za-z0-9._:-]+)["']"#).expect("static pattern is valid")
});

/// The text of a statement file of `format`. `country` (the profile's) only matters for the
/// legacy 8-bit encodings of MT940, QIF and OFX 1.x without a charset header.
pub fn decode(bytes: &[u8], format: StatementFormat, country: Option<&str>) -> DecodedCsv {
    match format {
        StatementFormat::Camt053 => decode_xml(bytes),
        StatementFormat::Ofx if starts_with_xml_declaration(bytes) => decode_xml(bytes),
        StatementFormat::Ofx => decode_ofx_sgml(bytes, country),
        StatementFormat::Mt940 | StatementFormat::Qif => {
            decode_csv_content_for_country(bytes, None, country)
        }
    }
}

/// XML: the declaration's `encoding` (UTF-8 when absent or unknown); a BOM wins.
fn decode_xml(bytes: &[u8]) -> DecodedCsv {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(XML_DECLARATION_BYTES)]);
    let encoding = XML_ENCODING
        .captures(&head)
        .and_then(|declared| Encoding::for_label(declared[1].as_bytes()))
        .unwrap_or(UTF_8);
    decode_with(encoding, bytes)
}

/// OFX 1.x (SGML): `ENCODING:UTF-8`, else `CHARSET:<codepage or label>`; `NONE`, nothing or a
/// label that is not known falls back to the CSV rule.
fn decode_ofx_sgml(bytes: &[u8], country: Option<&str>) -> DecodedCsv {
    match ofx_header_encoding(bytes) {
        Some(encoding) => decode_with(encoding, bytes),
        None => decode_csv_content_for_country(bytes, None, country),
    }
}

fn ofx_header_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let scanned = &bytes[..bytes.len().min(OFX_HEADER_BYTES)];
    let header_end = scanned
        .iter()
        .position(|&b| b == b'<')
        .unwrap_or(scanned.len());
    let header = String::from_utf8_lossy(&scanned[..header_end]);
    let (mut encoding, mut charset) = (None, None);
    for line in header.split(['\r', '\n']) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim().to_ascii_uppercase().as_str() {
            "ENCODING" => encoding = Some(value),
            "CHARSET" => charset = Some(value),
            _ => {}
        }
    }
    if encoding.is_some_and(|e| e.eq_ignore_ascii_case("UTF-8") || e.eq_ignore_ascii_case("UTF8")) {
        return Some(UTF_8);
    }
    let charset = charset.filter(|c| !c.eq_ignore_ascii_case("NONE"))?;
    if !charset.is_empty() && charset.bytes().all(|b| b.is_ascii_digit()) {
        return Encoding::for_label(format!("windows-{charset}").as_bytes());
    }
    Encoding::for_label(charset.as_bytes())
}

/// Does the file open with `<?xml` (after an optional UTF-8 BOM and whitespace)?
fn starts_with_xml_declaration(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace());
    start.is_some_and(|at| {
        bytes[at..]
            .get(..5)
            .is_some_and(|opening| opening.eq_ignore_ascii_case(b"<?xml"))
    })
}

/// Decode as `encoding`, unless the bytes open with a BOM, which wins.
fn decode_with(encoding: &'static Encoding, bytes: &[u8]) -> DecodedCsv {
    let (text, used, _had_errors) = encoding.decode(bytes);
    DecodedCsv {
        text: text.into_owned(),
        encoding: used.name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt940_in_windows_1250_with_cz_country() {
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(":86:Žluťoučký kůň\n");
        assert_eq!(
            decode(&bytes, StatementFormat::Mt940, Some("CZ")).text,
            ":86:Žluťoučký kůň\n"
        );
    }

    #[test]
    fn ofx_charset_header_wins() {
        let (body, _, _) = encoding_rs::WINDOWS_1252.encode("<NAME>Café");
        let mut bytes = b"OFXHEADER:100\nCHARSET:1252\n\n".to_vec();
        bytes.extend_from_slice(&body);
        assert!(decode(&bytes, StatementFormat::Ofx, Some("CZ"))
            .text
            .ends_with("<NAME>Café"));
    }

    #[test]
    fn camt_uses_the_xml_declaration() {
        let (body, _, _) = encoding_rs::WINDOWS_1250
            .encode("<?xml version=\"1.0\" encoding=\"windows-1250\"?><Nm>Kůň</Nm>");
        assert!(decode(&body, StatementFormat::Camt053, None)
            .text
            .contains("<Nm>Kůň</Nm>"));
    }

    fn windows_1250(text: &str) -> Vec<u8> {
        encoding_rs::WINDOWS_1250.encode(text).0.into_owned()
    }

    #[test]
    fn ofx_xml_follows_its_declaration() {
        let bytes = windows_1250(
            "<?xml version=\"1.0\" encoding=\"windows-1250\"?><?OFX OFXHEADER=\"200\"?><OFX><NAME>Kůň</NAME></OFX>",
        );
        let decoded = decode(&bytes, StatementFormat::Ofx, Some("DE"));
        assert!(decoded.text.contains("<NAME>Kůň</NAME>"));
        assert_eq!(decoded.encoding, "windows-1250");
    }

    #[test]
    fn xml_without_a_declaration_is_utf8_and_a_bom_wins() {
        let plain = decode("<Nm>Kůň</Nm>".as_bytes(), StatementFormat::Camt053, None);
        assert_eq!(plain.text, "<Nm>Kůň</Nm>");
        assert_eq!(plain.encoding, "UTF-8");

        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(
            "<?xml version=\"1.0\" encoding=\"windows-1250\"?><Nm>Kůň</Nm>".as_bytes(),
        );
        let decoded = decode(&bom, StatementFormat::Camt053, None);
        assert!(decoded.text.ends_with("<Nm>Kůň</Nm>"));
        assert!(!decoded.text.starts_with('\u{feff}'));

        // encoding_rs encodes only to UTF-8 (UTF-16 is decode-only), so build the bytes by hand.
        let mut with_bom = vec![0xFF, 0xFE];
        with_bom.extend("<Nm>Kůň</Nm>".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(
            decode(&with_bom, StatementFormat::Camt053, None).text,
            "<Nm>Kůň</Nm>"
        );
    }

    #[test]
    fn ofx_header_encoding_rules() {
        let utf8 = "OFXHEADER:100\nENCODING:UTF-8\nCHARSET:1252\n\n<NAME>Kůň";
        assert!(decode(utf8.as_bytes(), StatementFormat::Ofx, Some("CZ"))
            .text
            .ends_with("<NAME>Kůň"));

        let mut latin = b"OFXHEADER:100\r\nCHARSET:ISO-8859-1\r\n\r\n".to_vec();
        latin.extend_from_slice(&[b'<', b'N', b'>', 0xE9]);
        assert!(decode(&latin, StatementFormat::Ofx, Some("CZ"))
            .text
            .ends_with("<N>é"));

        // NONE, an absent header and an unknown label use the country fallback.
        for header in ["CHARSET:NONE\n", "", "CHARSET:no-such-charset\n"] {
            let mut bytes = format!("OFXHEADER:100\n{header}\n").into_bytes();
            bytes.extend_from_slice(&windows_1250("<NAME>Žluťoučký"));
            assert!(
                decode(&bytes, StatementFormat::Ofx, Some("CZ"))
                    .text
                    .ends_with("<NAME>Žluťoučký"),
                "header {header:?}"
            );
        }
    }

    #[test]
    fn utf8_statements_are_read_as_utf8_and_a_bom_is_removed() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("!Type:Bank\nPKůň\n".as_bytes());
        for format in [StatementFormat::Mt940, StatementFormat::Qif] {
            assert_eq!(
                decode(&bytes, format, Some("CZ")).text,
                "!Type:Bank\nPKůň\n"
            );
        }
    }
}
