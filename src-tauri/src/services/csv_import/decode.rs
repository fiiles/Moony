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

/// Legacy encodings tried, in order, when the bytes are not valid UTF-8.
/// Windows-1250 first: it is what Czech/Slovak banks write. (It never reports
/// errors, so the later entries only matter if that ever changes; a bank that
/// really uses Windows-1252 or ISO-8859-2 should pin it in its preset.)
const LEGACY_FALLBACKS: [&encoding_rs::Encoding; 3] = [
    encoding_rs::WINDOWS_1250,
    encoding_rs::WINDOWS_1252,
    encoding_rs::ISO_8859_2,
];

/// Decode CSV bytes. Order: UTF-16 (only with a BOM) → UTF-8 (BOM stripped) →
/// `preferred_encoding` (a preset's legacy encoding) → Windows-1250 →
/// Windows-1252 → ISO-8859-2.
///
/// A valid-UTF-8 file always wins, even when `preferred_encoding` says
/// otherwise: a bank that moved from Windows-1250 to UTF-8 must not have its
/// new exports mangled by an out-of-date preset. Never fails; the last resort
/// is a lossy Windows-1252 decode.
pub fn decode_csv_content(bytes: &[u8], preferred_encoding: Option<&str>) -> DecodedCsv {
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
    for encoding in preferred.into_iter().chain(LEGACY_FALLBACKS) {
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
