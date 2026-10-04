//! Amount parsing: decimal-separator consensus per column and a tolerant
//! single-cell parser.
//!
//! The separator is decided per *column*, not per cell: "1,234" is
//! 1234 in a column that otherwise reads "1,234.56", and 1.234 in a column that
//! otherwise reads "12,50". A cell that contradicts the column in a way that
//! would change its value by orders of magnitude (e.g. "12,50" in a column
//! whose decimal separator is "." ) is reported as unparseable instead of being
//! silently read as 1250.

/// Why a cell is not an amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountError {
    /// Nothing there (blank, or only a dash/currency).
    Empty,
    /// Something is there, but it is not a number we can read safely.
    Invalid,
}

/// Normalize the free-form text of an amount cell down to digits, `,` and `.`
/// and report the sign. Strips whitespace (incl. NBSP/thin space), currency
/// symbols and ISO codes at the edges, apostrophes, parentheses, trailing
/// minus, U+2212.
fn strip_decorations(value: &str) -> Result<(bool, String), AmountError> {
    fn is_noise(c: char) -> bool {
        // Letters ("EUR", "Kč"), currency symbols ($ € £ ¥ ₹ …) and spaces
        // around the number.
        c.is_alphabetic() || c.is_whitespace() || is_currency_symbol(c)
    }
    fn is_minus(c: char) -> bool {
        matches!(c, '-' | '\u{2212}' | '\u{2013}' | '\u{2012}')
    }

    // Blank or just a dash ("-", "—"): there is no amount, which is different
    // from text that is not an amount.
    if value
        .trim()
        .chars()
        .all(|c| c.is_whitespace() || is_minus(c) || c == '\u{2014}')
    {
        return Err(AmountError::Empty);
    }

    let mut s = value.trim();
    let mut negative = false;

    // Peel wrappers from the outside in until nothing changes: "EUR (12.34)",
    // "(EUR 12.34)", "-USD 12.34", "12.34- EUR" all converge.
    loop {
        let before = s;
        s = s.trim_matches(is_noise);
        if let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            negative = true;
            s = inner;
        }
        if let Some(rest) = s.strip_prefix(is_minus) {
            negative = true;
            s = rest;
        }
        if let Some(rest) = s.strip_suffix(is_minus) {
            negative = true;
            s = rest;
        }
        if let Some(rest) = s.strip_prefix('+') {
            s = rest;
        }
        if s == before {
            break;
        }
    }

    // Digit groups may be separated by spaces of any kind and apostrophes
    // (Swiss "1'234.56"); anything else left over is not a number.
    let mut core = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '0'..='9' | ',' | '.' => core.push(c),
            '\'' | '\u{2019}' => {}
            c if c.is_whitespace() => {}
            _ => return Err(AmountError::Invalid),
        }
    }

    if !core.chars().any(|c| c.is_ascii_digit()) {
        // "EUR", "n/a", a lone separator: text, but not a number.
        return Err(AmountError::Invalid);
    }
    Ok((negative, core))
}

/// Currency symbols without pulling in a Unicode-category crate: the Currency
/// Symbols block plus the few ASCII/Latin-1 ones and the Czech/Polish marks.
fn is_currency_symbol(c: char) -> bool {
    matches!(c, '$' | '£' | '¥' | '¤' | '€' | '\u{20A0}'..='\u{20CF}')
}

/// Is `core` a valid thousands-grouped integer for `separator`: a first group
/// of one to three digits (no leading zero) followed by groups of exactly
/// three digits?
fn valid_thousands_groups(core: &str, separator: char) -> bool {
    let mut parts = core.split(separator);
    let first = parts.next().unwrap_or("");
    if first.is_empty()
        || first.len() > 3
        || first.starts_with('0')
        || !first.chars().all(|c| c.is_ascii_digit())
    {
        return false;
    }
    parts.all(|g| g.len() == 3 && g.chars().all(|c| c.is_ascii_digit()))
}

/// Parse one amount cell with the column's decimal separator
/// (`,` or `.`). Never returns NaN/inf.
///
/// - `"(12.34)"`, `"12.34-"`, `"−12.34"` (U+2212), `"-12.34"` are negative;
/// - spaces (NBSP, thin, narrow), apostrophes, ISO codes and symbols at the
///   edges are ignored: `"1 234,56 CZK"`, `"$1,234.56"`, `"Kč 1 234,50"`;
/// - a cell with both `,` and `.` reads the *last* one as the decimal mark,
///   whatever the column says;
/// - a cell with one kind of separator reads it as the decimal mark when it
///   matches `decimal_separator`, otherwise as a thousands mark — which then
///   must be followed by exactly three digits per group (`"1,234"` → 1234 in a
///   `.` column, `"12,50"` → `Invalid`).
///
/// A zero is `Ok(0.0)`; the caller decides to report it as skipped.
pub fn clean_and_parse_amount(value: &str, decimal_separator: char) -> Result<f64, AmountError> {
    let (negative, core) = strip_decorations(value)?;
    let decimal = if decimal_separator == ',' { ',' } else { '.' };
    let thousands = if decimal == ',' { '.' } else { ',' };

    let has_comma = core.contains(',');
    let has_dot = core.contains('.');

    let normalized: String = if has_comma && has_dot {
        let last_comma = core.rfind(',').unwrap_or(0);
        let last_dot = core.rfind('.').unwrap_or(0);
        let (dec, thou) = if last_comma > last_dot {
            (',', '.')
        } else {
            ('.', ',')
        };
        // The decimal mark must occur once and after every thousands mark.
        if core.matches(dec).count() != 1 {
            return Err(AmountError::Invalid);
        }
        core.replace(thou, "").replace(dec, ".")
    } else if has_comma || has_dot {
        let sep = if has_comma { ',' } else { '.' };
        if sep == decimal {
            if core.matches(sep).count() != 1 {
                return Err(AmountError::Invalid);
            }
            core.replace(sep, ".")
        } else {
            // `sep` is the column's thousands mark.
            debug_assert_eq!(sep, thousands);
            if !valid_thousands_groups(&core, sep) {
                return Err(AmountError::Invalid);
            }
            core.replace(sep, "")
        }
    } else {
        core
    };

    // ".5" / "5." are fine for f64; an empty integer part with no digits was
    // rejected above.
    let parsed: f64 = normalized.parse().map_err(|_| AmountError::Invalid)?;
    if !parsed.is_finite() {
        return Err(AmountError::Invalid);
    }
    Ok(if negative { -parsed } else { parsed })
}

/// What one sample says about the column's decimal separator.
enum Evidence {
    /// This character is the decimal mark.
    Decimal(char),
    /// This character is a thousands mark (so the other one is the decimal).
    Thousands(char),
    /// "1,234"-style: one separator, exactly three digits after it. Could be
    /// either; counts as thousands only if nothing else is decisive.
    LooksLikeThousands(char),
    None,
}

fn evidence(sample: &str) -> Evidence {
    let Ok((_, core)) = strip_decorations(sample) else {
        return Evidence::None;
    };
    let has_comma = core.contains(',');
    let has_dot = core.contains('.');
    match (has_comma, has_dot) {
        (true, true) => {
            let last_comma = core.rfind(',').unwrap_or(0);
            let last_dot = core.rfind('.').unwrap_or(0);
            Evidence::Decimal(if last_comma > last_dot { ',' } else { '.' })
        }
        (false, false) => Evidence::None,
        _ => {
            let sep = if has_comma { ',' } else { '.' };
            let occurrences = core.matches(sep).count();
            if occurrences > 1 {
                return Evidence::Thousands(sep);
            }
            let after = core.rsplit(sep).next().unwrap_or("");
            let before = core.split(sep).next().unwrap_or("");
            if after.len() == 3
                && !before.is_empty()
                && before.len() <= 3
                && !before.starts_with('0')
            {
                Evidence::LooksLikeThousands(sep)
            } else {
                Evidence::Decimal(sep)
            }
        }
    }
}

/// Decide the decimal separator of a column from its sample cells.
///
/// - a cell with both `,` and `.` → the last one is the decimal mark;
/// - a separator that repeats ("1.234.567") is a thousands mark;
/// - a lone separator followed by exactly three digits ("1,234") is a
///   thousands mark *only* when no other cell says otherwise;
/// - any other lone separator ("12,50", "0.5") is the decimal mark.
///
/// Decisive cells vote; the majority wins, ties go to `.`. With no decisive
/// cell: all-"1,234"-style columns read the *other* character as the decimal
/// mark; a column of plain integers defaults to `.`.
pub fn detect_decimal_separator(samples: &[&str]) -> char {
    let mut votes_comma = 0usize;
    let mut votes_dot = 0usize;
    let mut looks_thousands_comma = 0usize;
    let mut looks_thousands_dot = 0usize;

    for sample in samples {
        match evidence(sample) {
            Evidence::Decimal(',') | Evidence::Thousands('.') => votes_comma += 1,
            Evidence::Decimal(_) | Evidence::Thousands(_) => votes_dot += 1,
            Evidence::LooksLikeThousands(',') => looks_thousands_comma += 1,
            Evidence::LooksLikeThousands(_) => looks_thousands_dot += 1,
            Evidence::None => {}
        }
    }

    if votes_comma > votes_dot {
        return ',';
    }
    if votes_dot > votes_comma {
        return '.';
    }
    if votes_comma == 0 {
        // No decisive cell at all.
        if looks_thousands_dot > looks_thousands_comma {
            // "1.234", "2.500" …: dots are thousands marks → decimal is ','.
            return ',';
        }
        return '.';
    }
    // Equal, non-zero decisive votes: stay with the dot.
    '.'
}

/// Format a parsed amount for storage: the sign is carried by the transaction
/// type, so this is the shortest round-trip decimal of the absolute value
/// (`250.5`, not `250.50`), exactly as the pre-v2 importer stored it — the
/// dedup rule compares these strings.
pub fn amount_to_text(amount: f64) -> String {
    amount.abs().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(value: &str, sep: char) -> Result<f64, AmountError> {
        clean_and_parse_amount(value, sep)
    }

    #[test]
    fn parse_table() {
        // (input, decimal separator, expected)
        let cases: &[(&str, char, Result<f64, AmountError>)] = &[
            // Czech with NBSP / thin space / narrow NBSP thousands
            ("-300\u{00a0}000,00", ',', Ok(-300000.0)),
            ("-300 000,00", ',', Ok(-300000.0)),
            ("1\u{2009}234,56", ',', Ok(1234.56)),
            ("1\u{202f}234,56", ',', Ok(1234.56)),
            ("1 234,56", ',', Ok(1234.56)),
            ("1234,56", ',', Ok(1234.56)),
            // European / US mixed separators: the last one is the decimal
            ("1.234,56", ',', Ok(1234.56)),
            ("1.234,56", '.', Ok(1234.56)),
            ("1,234.56", '.', Ok(1234.56)),
            ("1,234.56", ',', Ok(1234.56)),
            ("1.234.567,89", ',', Ok(1234567.89)),
            ("1,234,567.89", '.', Ok(1234567.89)),
            ("1234.56", '.', Ok(1234.56)),
            // Integers and signs
            ("1234", '.', Ok(1234.0)),
            ("-1234", '.', Ok(-1234.0)),
            ("+5.00", '.', Ok(5.0)),
            ("-7.00 ", '.', Ok(-7.0)),
            // Accounting negatives and unicode minus / trailing minus
            ("(12.34)", '.', Ok(-12.34)),
            ("(1,234.56)", '.', Ok(-1234.56)),
            ("12.34-", '.', Ok(-12.34)),
            ("12,34-", ',', Ok(-12.34)),
            ("\u{2212}12.34", '.', Ok(-12.34)),
            ("\u{2013}12.34", '.', Ok(-12.34)),
            // Currency symbols and ISO codes
            ("Kč 1 234,50", ',', Ok(1234.5)),
            ("1 234,56 CZK", ',', Ok(1234.56)),
            ("$1,234.56", '.', Ok(1234.56)),
            ("€ 12,00", ',', Ok(12.0)),
            ("EUR -7.00", '.', Ok(-7.0)),
            ("-USD 12.34", '.', Ok(-12.34)),
            ("(EUR 12.34)", '.', Ok(-12.34)),
            ("12.34 EUR-", '.', Ok(-12.34)),
            ("1'234.56", '.', Ok(1234.56)),
            ("1’234.56", '.', Ok(1234.56)),
            // a lone separator followed by three digits is a
            // thousands mark when it is not the column's decimal mark
            ("1,234", '.', Ok(1234.0)),
            ("1.234", ',', Ok(1234.0)),
            ("12,345,678", '.', Ok(12345678.0)),
            ("1.234", '.', Ok(1.234)),
            ("1,234", ',', Ok(1.234)),
            // Zero is a number; reporting it as skipped is the caller's job
            ("0.00", '.', Ok(0.0)),
            ("0,00", ',', Ok(0.0)),
            // Nothing / garbage
            ("", '.', Err(AmountError::Empty)),
            ("   ", '.', Err(AmountError::Empty)),
            ("-", '.', Err(AmountError::Empty)),
            ("\u{2014}", '.', Err(AmountError::Empty)),
            ("EUR", '.', Err(AmountError::Invalid)),
            ("n/a", '.', Err(AmountError::Invalid)),
            ("abc", '.', Err(AmountError::Invalid)),
            ("12abc34", '.', Err(AmountError::Invalid)),
            (".", '.', Err(AmountError::Invalid)),
            ("1.2.3", '.', Err(AmountError::Invalid)),
            // A comma that is not a valid thousands mark in a '.' column is
            // reported, never read as ×100
            ("12,50", '.', Err(AmountError::Invalid)),
            ("1,23,456", '.', Err(AmountError::Invalid)),
            ("1,5", '.', Err(AmountError::Invalid)),
            // "0,500" is 0.5, never "0" thousands "500"
            ("0,500", ',', Ok(0.5)),
            ("0,500", '.', Err(AmountError::Invalid)),
        ];
        for (input, sep, expected) in cases {
            assert_eq!(p(input, *sep), *expected, "parse {input:?} with {sep:?}");
        }
    }

    #[test]
    fn huge_amounts_keep_their_digits() {
        let value = p("123456789012.99", '.').unwrap();
        assert_eq!(amount_to_text(value), "123456789012.99");
    }

    #[test]
    fn amount_text_is_the_shortest_round_trip_of_the_absolute_value() {
        assert_eq!(amount_to_text(-250.5), "250.5");
        assert_eq!(amount_to_text(1000.0), "1000");
        assert_eq!(amount_to_text(-12.34), "12.34");
    }

    #[test]
    fn detect_separator_from_mixed_samples() {
        assert_eq!(detect_decimal_separator(&["1,234.56", "12.00"]), '.');
        assert_eq!(detect_decimal_separator(&["1.234,56", "12,00"]), ',');
        assert_eq!(detect_decimal_separator(&["1 234,56", "-300 000,00"]), ',');
        assert_eq!(detect_decimal_separator(&["-56.78", "3100.00"]), '.');
    }

    #[test]
    fn detect_separator_three_digit_rule() {
        // Only "1,234"-looking cells: the comma is thousands, decimal is '.'.
        assert_eq!(detect_decimal_separator(&["1,234", "2,500"]), '.');
        assert_eq!(detect_decimal_separator(&["1.234", "2.500"]), ',');
        // A repeating separator is thousands.
        assert_eq!(detect_decimal_separator(&["1.234.567"]), ',');
        assert_eq!(detect_decimal_separator(&["1,234,567"]), '.');
        // Another cell showing the comma is the decimal mark wins over the
        // "1,234" look-alike.
        assert_eq!(detect_decimal_separator(&["1,234", "12,50"]), ',');
        assert_eq!(detect_decimal_separator(&["1.234", "12.50"]), '.');
        // "1234,5" is decisive on its own (one digit after the separator).
        assert_eq!(detect_decimal_separator(&["1234,5"]), ',');
        // A leading zero cannot start a thousands group.
        assert_eq!(detect_decimal_separator(&["0,500"]), ',');
    }

    #[test]
    fn detect_separator_defaults() {
        assert_eq!(detect_decimal_separator(&[]), '.');
        assert_eq!(detect_decimal_separator(&["100", "250", ""]), '.');
        assert_eq!(detect_decimal_separator(&["abc", "n/a"]), '.');
        // Currency decorations do not confuse the vote.
        assert_eq!(detect_decimal_separator(&["12,50 CZK", "(3,00)"]), ',');
    }

    #[test]
    fn detected_separator_round_trips_the_column() {
        // A US column containing "1,234" parses all cells consistently.
        let column = ["1,234", "56.78", "(9.99)"];
        let sep = detect_decimal_separator(&column);
        assert_eq!(sep, '.');
        assert_eq!(p(column[0], sep), Ok(1234.0));
        assert_eq!(p(column[1], sep), Ok(56.78));
        assert_eq!(p(column[2], sep), Ok(-9.99));
        // A Czech column with the same first cell reads it as 1.234.
        let column = ["1,234", "56,78"];
        let sep = detect_decimal_separator(&column);
        assert_eq!(sep, ',');
        assert_eq!(p(column[0], sep), Ok(1.234));
    }
}
