//! Raw-file handling: text encoding, delimiter and header-row detection.
//!
//! Everything here works on bytes/`&str` and has no database or Tauri
//! dependency, so it is unit-tested directly (see `tests`).

use super::columns::suggest_column_mappings;

/// How many lines at the top of a file are inspected when looking for the
/// delimiter and the header row. Bank preambles (Fio, ING, DKB) are < 15 lines.
pub const HEADER_SCAN_LINES: usize = 40;

/// Text decoded from a file plus the encoding that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedCsv {
    pub text: String,
    /// Canonical encoding name (`UTF-8`, `windows-1250`, `UTF-16LE`, …).
    pub encoding: &'static str,
}

/// The legacy encoding files of a country are usually written in when they are not UTF-8.
/// A single-byte code page accepts every byte sequence, so the one chosen here is the answer; the
/// multi-byte ones (Shift_JIS, GBK, EUC-KR, Big5) report errors on a file of another encoding and
/// the search then falls through to Windows-1252. Unknown or missing countries get Windows-1252.
pub fn fallback_encoding_for_country(country: Option<&str>) -> &'static encoding_rs::Encoding {
    match country.map(|c| c.trim().to_ascii_uppercase()).as_deref() {
        Some("CZ" | "SK" | "PL" | "HU" | "HR" | "SI" | "RO") => encoding_rs::WINDOWS_1250,
        Some("RU" | "UA" | "BG" | "BY") => encoding_rs::WINDOWS_1251,
        Some("GR" | "CY") => encoding_rs::WINDOWS_1253,
        Some("TR") => encoding_rs::WINDOWS_1254,
        Some("LT" | "LV" | "EE") => encoding_rs::WINDOWS_1257,
        Some("JP") => encoding_rs::SHIFT_JIS,
        Some("CN") => encoding_rs::GBK,
        Some("KR") => encoding_rs::EUC_KR,
        Some("TW" | "HK") => encoding_rs::BIG5,
        _ => encoding_rs::WINDOWS_1252,
    }
}

/// Decode CSV bytes for a caller that does not know the user's country (the stock import): the
/// historical order, Windows-1250 first, as before. Bank imports use
/// [`decode_csv_content_for_country`].
pub fn decode_csv_content(bytes: &[u8], preferred_encoding: Option<&str>) -> DecodedCsv {
    decode_csv_content_for_country(bytes, preferred_encoding, Some("CZ"))
}

/// Decode CSV bytes. Order: UTF-16 (only with a BOM) → UTF-8 (BOM stripped) → `preferred_encoding`
/// (a preset's legacy encoding) → the encoding usual in `country`
/// ([`fallback_encoding_for_country`]) → Windows-1252.
///
/// A valid-UTF-8 file always wins, even when `preferred_encoding` says otherwise: a bank that
/// moved from Windows-1250 to UTF-8 must not have its new exports mangled by an out-of-date
/// preset. Never fails; the last resort is a lossy Windows-1252 decode.
pub fn decode_csv_content_for_country(
    bytes: &[u8],
    preferred_encoding: Option<&str>,
    country: Option<&str>,
) -> DecodedCsv {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        // decode() sniffs the BOM and picks LE or BE.
        let (text, encoding, _) = encoding_rs::UTF_16LE.decode(bytes);
        return DecodedCsv {
            text: text.into_owned(),
            encoding: encoding.name(),
        };
    }

    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    if let Ok(text) = std::str::from_utf8(bytes) {
        return DecodedCsv {
            text: text.to_string(),
            encoding: encoding_rs::UTF_8.name(),
        };
    }

    let preferred = preferred_encoding
        .and_then(|label| encoding_rs::Encoding::for_label(label.trim().as_bytes()))
        .filter(|enc| *enc != encoding_rs::UTF_8);
    let by_country = fallback_encoding_for_country(country);
    for encoding in preferred
        .into_iter()
        .chain([by_country, encoding_rs::WINDOWS_1252])
    {
        let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
        if !had_errors {
            return DecodedCsv {
                text: text.into_owned(),
                encoding: encoding.name(),
            };
        }
    }

    let (text, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
    DecodedCsv {
        text: text.into_owned(),
        encoding: encoding_rs::WINDOWS_1252.name(),
    }
}

/// Decode with exactly this encoding (the user picked it): no sniffing, a BOM of the encoding
/// itself is removed, invalid sequences become U+FFFD.
pub fn decode_forced(bytes: &[u8], encoding: &'static encoding_rs::Encoding) -> DecodedCsv {
    let (text, _had_errors) = encoding.decode_with_bom_removal(bytes);
    DecodedCsv {
        text: text.into_owned(),
        encoding: encoding.name(),
    }
}

/// Reader over `content` with no header handling and ragged rows allowed;
/// callers decide which record is the header.
pub fn csv_reader(content: &str, delimiter: char) -> csv::Reader<&[u8]> {
    csv::ReaderBuilder::new()
        .delimiter(delimiter as u8)
        .has_headers(false)
        .flexible(true)
        .from_reader(content.as_bytes())
}

/// Number of non-empty fields `line` splits into with `delimiter` (quotes
/// respected).
fn count_fields(line: &str, delimiter: char) -> usize {
    csv_reader(line, delimiter)
        .records()
        .next()
        .and_then(|r| r.ok())
        .map(|record| record.iter().filter(|s| !s.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Pick the delimiter that splits the top of the file into the most columns
/// (`,`, `;` or tab). Looks at several lines, not just the first: Fio-style
/// preambles start with a two-field line.
pub fn detect_csv_delimiter(content: &str) -> char {
    let lines: Vec<&str> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(HEADER_SCAN_LINES)
        .collect();

    let mut best = (';', 0usize);
    for delimiter in [',', ';', '\t'] {
        let widest = lines
            .iter()
            .map(|line| count_fields(line, delimiter))
            .max()
            .unwrap_or(0);
        if widest > best.1 {
            best = (delimiter, widest);
        }
    }
    best.0
}

/// Maps record positions to physical line numbers. The csv crate reports a
/// record's position *before* the blank lines it skips and its line counter
/// ignores them (Fio has a blank line between the preamble and the header), so
/// lines are derived from byte offsets, skipping leading line terminators.
pub struct LineIndex<'a> {
    content: &'a str,
    /// Byte offset of every `\n`.
    newlines: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub fn new(content: &'a str) -> Self {
        LineIndex {
            content,
            newlines: content.match_indices('\n').map(|(i, _)| i).collect(),
        }
    }

    /// 0-based line on which the record reported at byte `offset` really starts.
    pub fn line_of_record(&self, offset: usize) -> usize {
        let bytes = self.content.as_bytes();
        let mut start = offset.min(bytes.len());
        while start < bytes.len() && matches!(bytes[start], b'\n' | b'\r') {
            start += 1;
        }
        self.newlines.partition_point(|&nl| nl < start)
    }
}

/// Bytes of `content` after its first `line` lines (0 → the whole text).
pub fn slice_from_line(content: &str, line: usize) -> &str {
    if line == 0 {
        return content;
    }
    let mut remaining = line;
    for (idx, _) in content.match_indices('\n') {
        remaining -= 1;
        if remaining == 0 {
            return &content[idx + 1..];
        }
    }
    ""
}

/// 0-based line index of the first line (within the first
/// [`HEADER_SCAN_LINES`]) whose cells `is_header` accepts. `is_header` gets the
/// trimmed cells of the line, blank cells included. Falls back to line 0 when
/// nothing qualifies.
///
/// The scan is shared by the bank import ([`detect_header_row`]) and the stock
/// import, which decide differently what a header looks like.
pub fn detect_header_row_by(
    content: &str,
    delimiter: char,
    is_header: impl Fn(&[String]) -> bool,
) -> usize {
    let index = LineIndex::new(content);
    let mut reader = csv_reader(content, delimiter);
    for record in reader.records() {
        let Ok(record) = record else { continue };
        let Some(line) = record
            .position()
            .map(|p| index.line_of_record(p.byte() as usize) + 1)
        else {
            continue;
        };
        if line > HEADER_SCAN_LINES {
            break;
        }
        let fields: Vec<String> = record.iter().map(|f| f.trim().to_string()).collect();
        if is_header(&fields) {
            return line.saturating_sub(1);
        }
    }
    0
}

/// 0-based line index of the header row.
///
/// The header is the first line that splits into ≥ 3 fields and whose cells
/// match ≥ 2 known column roles, at least one of them a date or an amount
/// (debit/credit). Bank preambles (account number, period, balances) have two
/// fields per line or no date/amount words, so they are skipped. Falls back to
/// line 0 when nothing qualifies, which is the pre-v2 behaviour.
pub fn detect_header_row(content: &str, delimiter: char) -> usize {
    detect_header_row_by(content, delimiter, |fields| {
        if fields.iter().filter(|f| !f.is_empty()).count() < 3 {
            return false;
        }
        let roles = suggest_column_mappings(fields);
        let has_core = ["date", "amount", "debit", "credit"]
            .iter()
            .any(|k| roles.contains_key(*k));
        roles.len() >= 2 && has_core
    })
}

/// The header cells of the record at `header_row`, BOM and whitespace trimmed.
pub fn read_headers(content: &str, delimiter: char, header_row: usize) -> Vec<String> {
    let tail = slice_from_line(content, header_row);
    csv_reader(tail, delimiter)
        .records()
        .next()
        .and_then(|r| r.ok())
        .map(|record| {
            record
                .iter()
                .map(|h| h.trim().trim_start_matches('\u{feff}').trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_with_bom_is_decoded_and_stripped() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Datum;Částka".as_bytes());
        let decoded = decode_csv_content(&bytes, None);
        assert_eq!(decoded.text, "Datum;Částka");
        assert_eq!(decoded.encoding, "UTF-8");
    }

    #[test]
    fn valid_utf8_beats_a_legacy_preset_encoding() {
        let decoded = decode_csv_content("Částka".as_bytes(), Some("Windows-1250"));
        assert_eq!(decoded.text, "Částka");
        assert_eq!(decoded.encoding, "UTF-8");
    }

    #[test]
    fn windows_1250_bytes_decode_as_czech() {
        // "Částka" in Windows-1250: Č=0xC8 á=0xE1 t=0x74 k=0x6B a=0x61 + s=0x73
        let bytes = [0xC8, 0xE1, 0x73, 0x74, 0x6B, 0x61];
        let decoded = decode_csv_content(&bytes, None);
        assert_eq!(decoded.text, "Částka");
        assert_eq!(decoded.encoding, "windows-1250");
    }

    #[test]
    fn preset_encoding_wins_over_the_default_fallback_order() {
        // 0xF8 is "ø" in Windows-1252 but "ř" in Windows-1250.
        let bytes = [0x4B, 0xF8, 0x65];
        assert_eq!(decode_csv_content(&bytes, None).text, "Kře");
        let decoded = decode_csv_content(&bytes, Some("Windows-1252"));
        assert_eq!(decoded.text, "Køe");
        assert_eq!(decoded.encoding, "windows-1252");
        // An unknown label is ignored, not an error.
        assert_eq!(decode_csv_content(&bytes, Some("klingon")).text, "Kře");
    }

    fn bytes_in(encoding: &'static encoding_rs::Encoding, text: &str) -> Vec<u8> {
        let (bytes, _, unmappable) = encoding.encode(text);
        assert!(
            !unmappable,
            "{text:?} must be representable in {}",
            encoding.name()
        );
        bytes.into_owned()
    }

    #[test]
    fn a_windows_1252_file_keeps_pound_and_o_slash_for_british_nordic_and_unknown_profiles() {
        let text = "Beskrivelse;Beløp\nKøbenhavn £5";
        let bytes = bytes_in(encoding_rs::WINDOWS_1252, text);
        for country in [Some("GB"), Some("NO"), None] {
            let decoded = decode_csv_content_for_country(&bytes, None, country);
            assert_eq!(decoded.text, text, "{country:?}");
            assert_eq!(decoded.encoding, "windows-1252");
        }
        // The Central European guess reads the same bytes wrongly: ø becomes ř, £ becomes Ł.
        let czech = decode_csv_content_for_country(&bytes, None, Some("CZ"));
        assert!(
            czech.text.contains('ř') && czech.text.contains('Ł'),
            "{}",
            czech.text
        );
    }

    #[test]
    fn central_european_cyrillic_and_cjk_countries_get_their_own_code_page() {
        let cases: [(&'static encoding_rs::Encoding, &str, &str); 7] = [
            (encoding_rs::WINDOWS_1250, "Částka;Příjemce", "CZ"),
            (encoding_rs::WINDOWS_1250, "Kwota;Wypłata", "PL"),
            (encoding_rs::WINDOWS_1251, "Дата;Сумма", "RU"),
            (encoding_rs::SHIFT_JIS, "日付,摘要,金額", "JP"),
            (encoding_rs::GBK, "日期,摘要,金额", "CN"),
            (encoding_rs::EUC_KR, "거래일자,적요,금액", "KR"),
            (encoding_rs::BIG5, "日期,摘要,金額", "TW"),
        ];
        for (encoding, text, country) in cases {
            let bytes = bytes_in(encoding, text);
            let decoded = decode_csv_content_for_country(&bytes, None, Some(country));
            assert_eq!(decoded.text, text, "{country}");
            assert_eq!(decoded.encoding, encoding.name(), "{country}");
        }
    }

    #[test]
    fn a_baltic_file_is_read_as_windows_1257_for_lithuania() {
        let text = "Suma;Gavėjas";
        let bytes = bytes_in(encoding_rs::WINDOWS_1257, text);
        let decoded = decode_csv_content_for_country(&bytes, None, Some("LT"));
        assert_eq!(decoded.text, text);
        assert_eq!(decoded.encoding, "windows-1257");
    }

    #[test]
    fn utf8_still_wins_over_the_country_and_a_failing_multibyte_guess_falls_back() {
        let utf8 = decode_csv_content_for_country("Částka".as_bytes(), None, Some("JP"));
        assert_eq!((utf8.text.as_str(), utf8.encoding), ("Částka", "UTF-8"));
        // 0x81 0x20 is not a Shift_JIS pair: the guess for a Japanese profile is rejected.
        let decoded = decode_csv_content_for_country(&[0x41, 0x81, 0x20, 0x42], None, Some("JP"));
        assert_eq!(decoded.encoding, "windows-1252");
    }

    #[test]
    fn the_fallback_table_is_case_and_space_insensitive() {
        use encoding_rs::{
            BIG5, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252, WINDOWS_1253, WINDOWS_1254,
            WINDOWS_1257,
        };
        for (country, expected) in [
            (Some("cz"), WINDOWS_1250),
            (Some(" SK "), WINDOWS_1250),
            (Some("HU"), WINDOWS_1250),
            (Some("UA"), WINDOWS_1251),
            (Some("GR"), WINDOWS_1253),
            (Some("TR"), WINDOWS_1254),
            (Some("LT"), WINDOWS_1257),
            (Some("lv"), WINDOWS_1257),
            (Some("EE"), WINDOWS_1257),
            (Some("HK"), BIG5),
            (Some("DE"), WINDOWS_1252),
            (Some(""), WINDOWS_1252),
            (None, WINDOWS_1252),
        ] {
            assert_eq!(
                fallback_encoding_for_country(country),
                expected,
                "{country:?}"
            );
        }
    }

    #[test]
    fn utf16_with_bom_is_supported() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "Date;Amount".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let decoded = decode_csv_content(&bytes, None);
        assert_eq!(decoded.text, "Date;Amount");
        assert_eq!(decoded.encoding, "UTF-16LE");
    }

    #[test]
    fn delimiter_detection_uses_the_widest_line_not_the_first() {
        assert_eq!(detect_csv_delimiter("a,b,c\n1,2,3\n"), ',');
        assert_eq!(detect_csv_delimiter("a;b;c\n1;2;3\n"), ';');
        assert_eq!(detect_csv_delimiter("a\tb\tc\n1\t2\t3\n"), '\t');
        // Fio-style: two-field preamble first, comma decimals in the data.
        let fio = "accountId;2900000001\n\nID;Datum;Objem;Měna\n1;01.09.2026;25000,00;CZK\n";
        assert_eq!(detect_csv_delimiter(fio), ';');
        // Quoted commas do not count as delimiters.
        assert_eq!(detect_csv_delimiter("\"a, b\";c;d\n"), ';');
        assert_eq!(detect_csv_delimiter(""), ';');
    }

    #[test]
    fn slice_from_line_skips_whole_lines() {
        let text = "l0\nl1\nl2\n";
        assert_eq!(slice_from_line(text, 0), text);
        assert_eq!(slice_from_line(text, 1), "l1\nl2\n");
        assert_eq!(slice_from_line(text, 2), "l2\n");
        assert_eq!(slice_from_line(text, 3), "");
        assert_eq!(slice_from_line(text, 9), "");
    }

    #[test]
    fn header_on_first_line_is_row_zero() {
        let content = "Date,Description,Amount\n2026-09-01,Coffee,-3.50\n";
        assert_eq!(detect_header_row(content, ','), 0);
    }

    #[test]
    fn fio_preamble_is_skipped() {
        let content = "accountId;2900000001\nbankId;2010\ncurrency;CZK\niban;CZ79\nbic;FIOBCZPPXXX\nopeningBalance;10 000,00\nclosingBalance;12 345,67\ndateStart;01.09.2026\ndateEnd;30.09.2026\n\nID operace;Datum;Objem;Měna;Protiúčet\n26000000001;01.09.2026;25000,00;CZK;123\n";
        assert_eq!(detect_header_row(content, ';'), 10);
        let headers = read_headers(content, ';', 10);
        assert_eq!(headers[0], "ID operace");
        assert_eq!(headers[1], "Datum");
    }

    #[test]
    fn preamble_lines_with_three_fields_but_no_known_roles_are_skipped() {
        let content = "Report;generated;by bank\nDate;Description;Amount\n2026-09-01;x;1\n";
        assert_eq!(detect_header_row(content, ';'), 1);
    }

    #[test]
    fn no_recognisable_header_falls_back_to_the_first_line() {
        assert_eq!(detect_header_row("foo,bar,baz\n1,2,3\n", ','), 0);
        assert_eq!(detect_header_row("", ','), 0);
    }

    #[test]
    fn header_row_is_a_physical_line_index_even_with_multiline_quoted_cells() {
        let content = "\"Report\nsplit over lines\";x;y\nDate;Amount;Description\n2026-09-01;1;a\n";
        assert_eq!(detect_header_row(content, ';'), 2);
        assert_eq!(
            read_headers(content, ';', 2),
            vec!["Date", "Amount", "Description"]
        );
    }

    #[test]
    fn header_row_by_uses_the_callers_predicate() {
        let content = "Report;generated;by broker\nDate;Ticker;Qty\n2026-09-01;AAPL;3\n";
        // A predicate that wants a "Ticker" cell skips the preamble line.
        let row = detect_header_row_by(content, ';', |fields| fields.iter().any(|f| f == "Ticker"));
        assert_eq!(row, 1);
        // The first line wins when the predicate accepts it.
        assert_eq!(detect_header_row_by(content, ';', |_| true), 0);
    }

    #[test]
    fn header_row_by_receives_trimmed_cells_with_blanks_kept() {
        let content = "x;y\n A ; ;C\n";
        let mut seen: Vec<Vec<String>> = Vec::new();
        let seen_cell = std::cell::RefCell::new(&mut seen);
        let row = detect_header_row_by(content, ';', |fields| {
            seen_cell.borrow_mut().push(fields.to_vec());
            fields.len() == 3
        });
        assert_eq!(row, 1);
        assert_eq!(seen[1], vec!["A", "", "C"]);
    }

    #[test]
    fn header_row_by_falls_back_to_the_first_line_and_stops_scanning() {
        assert_eq!(detect_header_row_by("a;b;c\n1;2;3\n", ';', |_| false), 0);
        assert_eq!(detect_header_row_by("", ';', |_| true), 0);
        // A match beyond the scanned lines is not found.
        let mut content = "p;q;r\n".repeat(HEADER_SCAN_LINES);
        content.push_str("Date;Amount;Description\n");
        assert_eq!(
            detect_header_row_by(&content, ';', |f| f.first().is_some_and(|c| c == "Date")),
            0
        );
    }

    #[test]
    fn bank_header_detection_is_the_generic_helper_with_the_bank_rule() {
        // Same answers as before the refactor, including the < 3 fields rule.
        let content = "accountId;2900000001\nDate;Description;Amount\n2026-09-01;x;1\n";
        assert_eq!(detect_header_row(content, ';'), 1);
        assert_eq!(detect_header_row("Date;Amount\n2026-09-01;1\n", ';'), 0);
    }

    #[test]
    fn read_headers_trims_bom_and_whitespace() {
        let content = "\u{feff} Date ; Amount ;Description\n";
        assert_eq!(
            read_headers(content, ';', 0),
            vec!["Date", "Amount", "Description"]
        );
    }
}
