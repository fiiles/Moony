//! Date Parsing Service for CSV Imports
//!
//! Provides intelligent date parsing with:
//! - Time component stripping (e.g., "2024-01-15 14:30:00" → "2024-01-15")
//! - Multiple format support with fallback chain
//! - Smart format detection to distinguish DD-MM-YYYY from MM-DD-YYYY

use chrono::{Datelike, NaiveDate};

/// Detected date format family
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DateFormatFamily {
    /// YYYY-MM-DD, YYYY/MM/DD, YYYY.MM.DD (ISO standard)
    YearMonthDay,
    /// YYYY-DD-MM (non-standard year-first)
    YearDayMonth,
    /// DD-MM-YYYY, DD/MM/YYYY, DD.MM.YYYY (European)
    DayMonthYear,
    /// MM-DD-YYYY, MM/DD/YYYY, MM.DD.YYYY (US)
    MonthDayYear,
}

/// Strip time component from a date-time string.
/// Handles formats like:
/// - "2024-01-15 14:30:00"
/// - "2024-01-15T14:30:00Z"
/// - "2024-01-15T14:30:00+01:00"
/// - "15.01.2024 10:00"
fn strip_time_component(date_str: &str) -> &str {
    let trimmed = date_str.trim();

    // Find the position where time component starts
    // Common separators: space, 'T' (ISO 8601)
    if let Some(space_pos) = trimmed.find(' ') {
        // Check if what follows looks like a time (starts with digit AND contains ':')
        // This prevents "14. 1. 2026" (Czech date with spaces) from being misidentified
        // as a date+time string where " 1. 2026" is treated as the time component.
        let after_space = &trimmed[space_pos + 1..];
        let starts_with_digit = after_space
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false);
        let contains_colon = after_space.contains(':');
        if starts_with_digit && contains_colon {
            return &trimmed[..space_pos];
        }
    }

    if let Some(t_pos) = trimmed.find('T') {
        // ISO 8601 format with 'T' separator
        return &trimmed[..t_pos];
    }

    trimmed
}

/// Parse date components from a string, extracting numeric parts.
/// Returns (first, second, third) components or None if parsing fails.
fn parse_date_components(date_str: &str) -> Option<(u32, u32, u32)> {
    // Split by common separators: -, /, .
    let parts: Vec<&str> = date_str.split(['-', '/', '.']).collect();

    if parts.len() != 3 {
        return None;
    }

    let first: u32 = parts[0].trim().parse().ok()?;
    let second: u32 = parts[1].trim().parse().ok()?;
    let third: u32 = parts[2].trim().parse().ok()?;

    Some((first, second, third))
}

/// Classify one date string: its format family and whether the string itself
/// proves it. Year-first dates are ISO unless the second number cannot be a
/// month; year-last dates are only decisive when one of the two leading numbers
/// cannot be a month ("25.01.2024" is day-first, "01/25/2024" month-first).
/// "05/01/2024" is not decisive: it reads both ways, and falls back to the
/// European order.
fn classify_format(date_str: &str) -> Option<(DateFormatFamily, bool)> {
    let (first, second, _) = parse_date_components(date_str)?;

    // Check if year is first (4-digit number)
    if (1900..=2100).contains(&first) {
        // Year-first format: YYYY-??-??
        if second > 12 {
            // Second component > 12, must be day → YYYY-DD-MM
            Some((DateFormatFamily::YearDayMonth, true))
        } else {
            // ISO standard YYYY-MM-DD
            Some((DateFormatFamily::YearMonthDay, true))
        }
    } else {
        // Year-last (4-digit year, or a 2-digit year): ??-??-YY(YY)
        if first > 12 {
            // First component > 12, must be day → DD-MM-YYYY
            Some((DateFormatFamily::DayMonthYear, true))
        } else if second > 12 {
            // Second component > 12, must be day → MM-DD-YYYY
            Some((DateFormatFamily::MonthDayYear, true))
        } else {
            // Both ≤ 12: ambiguous, default to European DD-MM-YYYY
            Some((DateFormatFamily::DayMonthYear, false))
        }
    }
}

/// Detect the date format family from a single date string.
/// Uses heuristics to determine the most likely format.
fn detect_format_family(date_str: &str) -> Option<DateFormatFamily> {
    classify_format(date_str).map(|(family, _)| family)
}

/// Detect the most likely date format from multiple sample strings.
///
/// Only samples that prove their own order vote ("25.01.2024" cannot be
/// month-first); a column of "01/02/2024"-style values that all read both ways
/// must not outvote the one "09/13/2026" that shows the file is month-first.
/// When no sample is decisive the result is the European default.
pub fn detect_date_format_from_samples(samples: &[&str]) -> DateFormatFamily {
    let mut decisive = [0usize; 4];
    let mut ambiguous = [0usize; 4];
    let slot = |family: DateFormatFamily| match family {
        DateFormatFamily::YearMonthDay => 0,
        DateFormatFamily::YearDayMonth => 1,
        DateFormatFamily::DayMonthYear => 2,
        DateFormatFamily::MonthDayYear => 3,
    };

    for sample in samples {
        let stripped = strip_time_component(sample);
        if let Some((family, is_decisive)) = classify_format(stripped) {
            if is_decisive {
                decisive[slot(family)] += 1;
            } else {
                ambiguous[slot(family)] += 1;
            }
        }
    }

    let counts = if decisive.iter().sum::<usize>() > 0 {
        decisive
    } else {
        ambiguous
    };
    let max_count = counts.iter().copied().max().unwrap_or(0);
    if max_count == 0 {
        return DateFormatFamily::DayMonthYear; // Default
    }

    // Ties keep the historical priority: ISO, year-day-month, US, European.
    if counts[0] == max_count {
        DateFormatFamily::YearMonthDay
    } else if counts[1] == max_count {
        DateFormatFamily::YearDayMonth
    } else if counts[3] == max_count {
        DateFormatFamily::MonthDayYear
    } else {
        DateFormatFamily::DayMonthYear
    }
}

/// Parse a date string with a specific chrono format string.
/// Handles time components by stripping them first.
pub fn parse_date_with_format(date_str: &str, format: &str) -> Option<NaiveDate> {
    let stripped = strip_time_component(date_str);

    // First try the exact format
    if let Ok(date) = NaiveDate::parse_from_str(stripped, format) {
        return Some(date);
    }

    // If format includes time but our stripped version doesn't have it, try without time
    // Extract just the date portion of the format
    let date_only_format = format.split_whitespace().next().unwrap_or(format);
    if date_only_format != format {
        if let Ok(date) = NaiveDate::parse_from_str(stripped, date_only_format) {
            return Some(date);
        }
    }

    None
}

/// Parse a date string using the detected format family. Only for dates with a
/// four-digit year: a two-digit year puts the year first or last, which the
/// family cannot tell (see `two_digit_layouts`).
fn parse_with_format_family(date_str: &str, family: DateFormatFamily) -> Option<NaiveDate> {
    let (first, second, third) = parse_date_components(date_str)?;

    let (year, month, day) = match family {
        DateFormatFamily::YearMonthDay => (first, second, third),
        DateFormatFamily::YearDayMonth => (first, third, second),
        DateFormatFamily::DayMonthYear => (third, second, first),
        DateFormatFamily::MonthDayYear => (third, first, second),
    };
    if !(1900..=2100).contains(&year) {
        return None;
    }

    NaiveDate::from_ymd_opt(year as i32, month, day)
}

/// Field order of a numeric date whose year has two digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YearLayout {
    /// YY-MM-DD
    YearMonthDay,
    /// DD-MM-YY
    DayMonthYear,
    /// MM-DD-YY
    MonthDayYear,
}

const LAYOUTS: [YearLayout; 3] = [
    YearLayout::YearMonthDay,
    YearLayout::DayMonthYear,
    YearLayout::MonthDayYear,
];

/// Which layouts the numbers `a-b-c` (all two-digit) can be read as — the ones
/// that give a month in 1..=12 and a day in 1..=31. With a four-digit year the
/// position of the year is obvious; with two digits "24-01-15" is both
/// 2015-01-24 and 2024-01-15, and only a number that rules a layout out (a
/// "day" above 31, a "month" above 12) can decide.
fn two_digit_layouts(a: u32, b: u32, c: u32) -> [bool; 3] {
    let day = |n: u32| (1..=31).contains(&n);
    let month = |n: u32| (1..=12).contains(&n);
    [
        month(b) && day(c), // YY-MM-DD
        day(a) && month(b), // DD-MM-YY
        month(a) && day(b), // MM-DD-YY
    ]
}

/// Century of a two-digit year, the way chrono's `%y` reads it (69–99 → 19xx,
/// 00–68 → 20xx), so the lenient and the strict parser agree.
fn expand_two_digit_year(yy: u32) -> i32 {
    if yy >= 69 {
        1900 + yy as i32
    } else {
        2000 + yy as i32
    }
}

/// The `(a, b, c)` of a numeric date whose parts are all two-digit numbers.
fn two_digit_components(date_str: &str) -> Option<(u32, u32, u32)> {
    parse_date_components(date_str).filter(|&(a, b, c)| a <= 99 && b <= 99 && c <= 99)
}

/// Parse a date with a two-digit year, or `None` when the string does not decide
/// where the year is (or is no date).
fn parse_two_digit_year(a: u32, b: u32, c: u32) -> Option<NaiveDate> {
    let possible = two_digit_layouts(a, b, c);
    let mut candidates = LAYOUTS.iter().zip(possible).filter(|(_, ok)| *ok);
    let (layout, _) = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let (year, month, day) = match layout {
        YearLayout::YearMonthDay => (expand_two_digit_year(a), b, c),
        YearLayout::DayMonthYear => (expand_two_digit_year(c), b, a),
        YearLayout::MonthDayYear => (expand_two_digit_year(c), a, b),
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

/// `true` when `date_str` is a numeric date with a two-digit year that can be
/// read in more than one order.
fn is_ambiguous_two_digit_date(date_str: &str) -> bool {
    two_digit_components(strip_time_component(date_str))
        .map(|(a, b, c)| two_digit_layouts(a, b, c).iter().filter(|ok| **ok).count() > 1)
        .unwrap_or(false)
}

/// Parse a date string, automatically detecting the format.
/// This is the main entry point for date parsing.
///
/// # Arguments
/// * `date_str` - The date string to parse
/// * `preferred_format` - Optional chrono format string to try first
///
/// # Returns
/// * `Some(NaiveDate)` if parsing succeeds
/// * `None` if the date cannot be parsed
pub fn parse_date(date_str: &str, preferred_format: Option<&str>) -> Option<NaiveDate> {
    let stripped = strip_time_component(date_str);

    if stripped.is_empty() {
        return None;
    }

    // Try preferred format first
    if let Some(format) = preferred_format {
        if let Some(date) = parse_date_with_format(stripped, format) {
            return Some(date);
        }
    }

    // Try common chrono formats
    let formats = [
        "%Y-%m-%d", // ISO: 2024-01-15
        "%d.%m.%Y", // European: 15.01.2024
        "%d/%m/%Y", // European slash: 15/01/2024
        "%d-%m-%Y", // European dash: 15-01-2024
        "%m/%d/%Y", // US: 01/15/2024
        "%m-%d-%Y", // US dash: 01-15-2024
        "%Y/%m/%d", // ISO slash: 2024/01/15
        "%Y.%m.%d", // ISO dot: 2024.01.15
    ];

    for format in &formats {
        if let Ok(date) = NaiveDate::parse_from_str(stripped, format) {
            // Validate that the year is plausible (1900-2100)
            // This prevents 2-digit year strings from being misinterpreted
            // e.g., "15-01-24" should not be parsed as year 15
            let year = date.year();
            if (1900..=2100).contains(&year) {
                return Some(date);
            }
        }
    }

    // Two-digit years: the year can be first or last. Parse only when the
    // numbers decide it; otherwise an explicit format is required.
    if let Some((a, b, c)) = two_digit_components(stripped) {
        return parse_two_digit_year(a, b, c);
    }

    // Fallback to intelligent format detection
    let family = detect_format_family(stripped)?;
    parse_with_format_family(stripped, family)
}

/// Parse a date string to a Unix timestamp (seconds since epoch, midnight UTC).
/// This is a convenience function for the common use case in CSV imports.
///
/// # Arguments
/// * `date_str` - The date string to parse
/// * `preferred_format` - Optional chrono format string to try first
///
/// # Returns
/// * `Ok(i64)` - Unix timestamp if parsing succeeds
/// * `Err(String)` - Error message if parsing fails
pub fn parse_date_to_timestamp(
    date_str: &str,
    preferred_format: Option<&str>,
) -> Result<i64, String> {
    parse_date(date_str, preferred_format)
        .map(|d| {
            d.and_hms_opt(0, 0, 0)
                .expect("Valid date should have valid midnight time")
                .and_utc()
                .timestamp()
        })
        .ok_or_else(|| {
            if preferred_format.is_none() && is_ambiguous_two_digit_date(date_str) {
                format!(
                    "Ambiguous date '{}': a two-digit year can be first or last — use a four-digit year or give the format",
                    date_str
                )
            } else {
                format!("Cannot parse date '{}'", date_str)
            }
        })
}

/// Parse `date_str` with exactly `format` — no fallback to other formats and no
/// day/month guessing. A time component in the value is ignored, a time part in
/// the format is tolerated (`%Y-%m-%d %H:%M:%S` reads "2026-09-02 08:00:01").
///
/// This is what the CSV import uses: one format per file, decided up front
///, so a US export can never be read as DD/MM in some rows and MM/DD
/// in others. Years outside 1900–2100 are rejected (a 2-digit year read with
/// `%Y` would otherwise become year 26).
pub fn parse_date_strict(date_str: &str, format: &str) -> Option<NaiveDate> {
    let stripped = strip_time_component(date_str);
    if stripped.is_empty() {
        return None;
    }
    let date = parse_date_with_format(stripped, format)?;
    (1900..=2100).contains(&date.year()).then_some(date)
}

/// `parse_date_strict` as a unix timestamp (midnight UTC, ADR 0008).
pub fn parse_date_to_timestamp_strict(date_str: &str, format: &str) -> Option<i64> {
    parse_date_strict(date_str, format).map(|d| {
        d.and_hms_opt(0, 0, 0)
            .expect("midnight is a valid time")
            .and_utc()
            .timestamp()
    })
}

/// The date part of a chrono format: everything from the first time directive
/// on is cut (`%Y-%m-%d %H:%M:%S` → `%Y-%m-%d`, `%d.%m.%Y %H:%M` → `%d.%m.%Y`).
pub fn date_only_format(format: &str) -> String {
    const TIME_DIRECTIVES: [&str; 14] = [
        "%H", "%I", "%k", "%l", "%M", "%S", "%T", "%R", "%p", "%P", "%f", "%z", "%Z", "%X",
    ];
    let cut = TIME_DIRECTIVES
        .iter()
        .filter_map(|d| format.find(d))
        .min()
        .unwrap_or(format.len());
    format[..cut]
        .trim_end_matches(|c: char| c.is_whitespace() || c == 'T' || c == ',')
        .to_string()
}

/// A date format inferred from sample values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedDateFormat {
    /// chrono format, date part only (`%d.%m.%Y`, `%Y-%m-%d`, `%m/%d/%y`).
    pub format: String,
    /// `true` when day/month order cannot be decided: every sample has both
    /// numbers ≤ 12 (or no sample could be read at all). The format is then the
    /// European default and the user should confirm it.
    pub ambiguous: bool,
}

/// One numeric date sample taken apart: the three numbers, the separator
/// between them and the digit count of the year part.
struct SampleParts {
    separator: String,
    first: u32,
    second: u32,
    third: u32,
    year_digits: usize,
}

fn sample_parts(sample: &str) -> Option<SampleParts> {
    let stripped = strip_time_component(sample);
    let (first, second, third) = parse_date_components(stripped)?;
    // Separator = the non-digit run after the first number ("." , "/", "-", ". ").
    let after_first = stripped
        .trim_start()
        .trim_start_matches(|c: char| c.is_ascii_digit());
    let separator: String = after_first
        .chars()
        .take_while(|c| !c.is_ascii_digit())
        .collect();
    if separator.is_empty() {
        return None;
    }
    let year_part_first = (1900..=2100).contains(&first);
    let year_digits = if year_part_first {
        stripped
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .count()
    } else {
        stripped
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    Some(SampleParts {
        separator,
        first,
        second,
        third,
        year_digits,
    })
}

/// Infer one date format for a whole column. Builds on
/// `detect_date_format_from_samples` for the field order and adds the
/// separator and the year width. Samples that cannot be read are ignored; with
/// none left the result is `%Y-%m-%d` flagged ambiguous.
pub fn detect_date_format(samples: &[&str]) -> DetectedDateFormat {
    let parts: Vec<SampleParts> = samples.iter().filter_map(|s| sample_parts(s)).collect();
    if parts.is_empty() {
        return DetectedDateFormat {
            format: "%Y-%m-%d".to_string(),
            ambiguous: true,
        };
    }

    // Most common separator wins.
    let mut separator_counts: Vec<(&str, usize)> = Vec::new();
    for p in &parts {
        match separator_counts.iter_mut().find(|(s, _)| *s == p.separator) {
            Some((_, n)) => *n += 1,
            None => separator_counts.push((p.separator.as_str(), 1)),
        }
    }
    let separator = separator_counts
        .iter()
        .max_by_key(|(_, n)| *n)
        .map(|(s, _)| *s)
        .unwrap_or("-");

    // Two-digit years: the year can be first or last, so the layout is decided
    // only by values that rule the others out.
    if parts
        .iter()
        .all(|p| p.year_digits == 2 && p.first <= 99 && p.third <= 99)
    {
        let mut possible = [true; 3];
        for p in &parts {
            for (slot, ok) in possible
                .iter_mut()
                .zip(two_digit_layouts(p.first, p.second, p.third))
            {
                *slot &= ok;
            }
        }
        let count = possible.iter().filter(|ok| **ok).count();
        if count > 0 {
            // European order is the guess when several layouts fit.
            let layout = [
                YearLayout::DayMonthYear,
                YearLayout::MonthDayYear,
                YearLayout::YearMonthDay,
            ]
            .into_iter()
            .find(|l| possible[LAYOUTS.iter().position(|x| x == l).unwrap_or(0)])
            .unwrap_or(YearLayout::DayMonthYear);
            let format = match layout {
                YearLayout::YearMonthDay => format!("%y{separator}%m{separator}%d"),
                YearLayout::DayMonthYear => format!("%d{separator}%m{separator}%y"),
                YearLayout::MonthDayYear => format!("%m{separator}%d{separator}%y"),
            };
            return DetectedDateFormat {
                format,
                ambiguous: count > 1,
            };
        }
    }

    let family = detect_date_format_from_samples(samples);

    // Two-digit years only when every sample uses them.
    let year = if parts.iter().all(|p| p.year_digits == 2) {
        "%y"
    } else {
        "%Y"
    };

    let format = match family {
        DateFormatFamily::YearMonthDay => format!("%Y{separator}%m{separator}%d"),
        DateFormatFamily::YearDayMonth => format!("%Y{separator}%d{separator}%m"),
        DateFormatFamily::DayMonthYear => format!("%d{separator}%m{separator}{year}"),
        DateFormatFamily::MonthDayYear => format!("%m{separator}%d{separator}{year}"),
    };

    // Year-first dates follow ISO unless a value proves otherwise (that is the
    // YearDayMonth family); only year-last dates can be genuinely ambiguous.
    let year_last = matches!(
        family,
        DateFormatFamily::DayMonthYear | DateFormatFamily::MonthDayYear
    );
    let decisive = parts.iter().any(|p| p.first > 12 || p.second > 12);
    DetectedDateFormat {
        format,
        ambiguous: year_last && !decisive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_time_component() {
        // Space separator
        assert_eq!(strip_time_component("2024-01-15 14:30:00"), "2024-01-15");
        assert_eq!(strip_time_component("15.01.2024 10:00:00"), "15.01.2024");

        // ISO 8601 with T
        assert_eq!(strip_time_component("2024-01-15T14:30:00"), "2024-01-15");
        assert_eq!(strip_time_component("2024-01-15T14:30:00Z"), "2024-01-15");
        assert_eq!(
            strip_time_component("2024-01-15T14:30:00+01:00"),
            "2024-01-15"
        );

        // No time component
        assert_eq!(strip_time_component("2024-01-15"), "2024-01-15");
        assert_eq!(strip_time_component("15.01.2024"), "15.01.2024");

        // With whitespace
        assert_eq!(strip_time_component("  2024-01-15  "), "2024-01-15");

        // Czech date format with spaces after dots — must NOT be stripped
        assert_eq!(strip_time_component("14. 1. 2026"), "14. 1. 2026");
        assert_eq!(strip_time_component("1. 12. 2025"), "1. 12. 2025");
    }

    #[test]
    fn test_parse_date_components() {
        assert_eq!(parse_date_components("2024-01-15"), Some((2024, 1, 15)));
        assert_eq!(parse_date_components("15.01.2024"), Some((15, 1, 2024)));
        assert_eq!(parse_date_components("01/15/2024"), Some((1, 15, 2024)));
        assert_eq!(parse_date_components("invalid"), None);
        assert_eq!(parse_date_components("2024-01"), None);
    }

    #[test]
    fn test_detect_format_family() {
        // Year first - ISO standard
        assert_eq!(
            detect_format_family("2024-01-15"),
            Some(DateFormatFamily::YearMonthDay)
        );
        assert_eq!(
            detect_format_family("2024/01/15"),
            Some(DateFormatFamily::YearMonthDay)
        );

        // Year first with day > 12 in second position
        assert_eq!(
            detect_format_family("2024-25-01"),
            Some(DateFormatFamily::YearDayMonth)
        );

        // European (day first) - detectable when day > 12
        assert_eq!(
            detect_format_family("15-01-2024"),
            Some(DateFormatFamily::DayMonthYear)
        );
        assert_eq!(
            detect_format_family("25.01.2024"),
            Some(DateFormatFamily::DayMonthYear)
        );

        // US (month first) - detectable when day > 12 in second position
        assert_eq!(
            detect_format_family("01-25-2024"),
            Some(DateFormatFamily::MonthDayYear)
        );
        assert_eq!(
            detect_format_family("01/31/2024"),
            Some(DateFormatFamily::MonthDayYear)
        );

        // Ambiguous - defaults to European
        assert_eq!(
            detect_format_family("01-05-2024"),
            Some(DateFormatFamily::DayMonthYear)
        );
        assert_eq!(
            detect_format_family("05/01/2024"),
            Some(DateFormatFamily::DayMonthYear)
        );
    }

    #[test]
    fn test_parse_date_with_time() {
        // Revolut format
        let result = parse_date("2024-01-15 14:30:00", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // ISO 8601
        let result = parse_date("2024-01-15T14:30:00Z", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // European with time
        let result = parse_date("15.01.2024 10:00", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));
    }

    #[test]
    fn test_parse_date_various_formats() {
        // ISO
        let result = parse_date("2024-01-15", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // European dot
        let result = parse_date("15.01.2024", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // European slash
        let result = parse_date("15/01/2024", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // European dash
        let result = parse_date("15-01-2024", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // Czech format with spaces after dots: "14. 1. 2026"
        let result = parse_date("14. 1. 2026", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2026, 1, 14).unwrap()));

        let result = parse_date("1. 12. 2025", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()));
    }

    #[test]
    fn test_parse_date_us_format_detected() {
        // US format with day > 12 in second position
        let result = parse_date("01/31/2024", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 31).unwrap()));

        // Unambiguous US date
        let result = parse_date("12/25/2024", None);
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 12, 25).unwrap()));
    }

    #[test]
    fn test_parse_date_with_preferred_format() {
        // Preferred format should be tried first
        let result = parse_date("15-01-2024", Some("%d-%m-%Y"));
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));

        // Even if input has time, should work with date-only format
        let result = parse_date("2024-01-15 14:30:00", Some("%Y-%m-%d"));
        assert_eq!(result, Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()));
    }

    #[test]
    fn test_parse_date_to_timestamp() {
        let result = parse_date_to_timestamp("2024-01-15", None);
        assert!(result.is_ok());
        // 2024-01-15 00:00:00 UTC = 1705276800
        assert_eq!(result.unwrap(), 1705276800);

        let result = parse_date_to_timestamp("invalid", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_format_from_samples() {
        // All ISO dates
        let samples = vec!["2024-01-15", "2024-02-20", "2024-03-25"];
        assert_eq!(
            detect_date_format_from_samples(&samples),
            DateFormatFamily::YearMonthDay
        );

        // Mix with unambiguous European
        let samples = vec!["15-01-2024", "25-02-2024", "31-03-2024"];
        assert_eq!(
            detect_date_format_from_samples(&samples),
            DateFormatFamily::DayMonthYear
        );

        // Mix with unambiguous US
        let samples = vec!["01-15-2024", "02-25-2024", "03-31-2024"];
        assert_eq!(
            detect_date_format_from_samples(&samples),
            DateFormatFamily::MonthDayYear
        );

        // Empty samples default to European
        let samples: Vec<&str> = vec![];
        assert_eq!(
            detect_date_format_from_samples(&samples),
            DateFormatFamily::DayMonthYear
        );
    }

    #[test]
    fn test_two_digit_year_is_not_guessed() {
        // "24-01-15" is 2015-01-24 as DD-MM-YY and 2024-01-15 as
        // YY-MM-DD; "15-01-24" the other way round. Nothing in the string says
        // which, so the lenient parser refuses instead of picking one.
        assert_eq!(parse_date("24-01-15", None), None);
        assert_eq!(parse_date("15-01-24", None), None);
        // ... and an explicit format settles it either way.
        assert_eq!(
            parse_date("24-01-15", Some("%y-%m-%d")),
            Some(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap())
        );
        assert_eq!(
            parse_date("24-01-15", Some("%d-%m-%y")),
            Some(NaiveDate::from_ymd_opt(2015, 1, 24).unwrap())
        );
        // The timestamp wrapper says why it gave up.
        let err = parse_date_to_timestamp("24-01-15", None).unwrap_err();
        assert!(err.contains("Ambiguous"), "{err}");
        assert!(parse_date_to_timestamp("garbage", None)
            .unwrap_err()
            .contains("Cannot parse"));
    }

    #[test]
    fn test_two_digit_year_with_proof_still_parses() {
        let d = |y, m, day| Some(NaiveDate::from_ymd_opt(y, m, day).unwrap());
        // a year above 31 cannot be a day: it is last (DD-MM-YY)
        assert_eq!(parse_date("31-12-99", None), d(1999, 12, 31));
        // a first number above 31 cannot be a day: the year is first (YY-MM-DD)
        assert_eq!(parse_date("99-12-31", None), d(1999, 12, 31));
        // a second number above 12 cannot be a month: MM-DD-YY
        assert_eq!(parse_date("12/25/24", None), d(2024, 12, 25));
        // an impossible date is no date
        assert_eq!(parse_date("31-02-99", None), None);
    }
    #[test]
    fn strict_parse_never_falls_back_to_another_order() {
        // US file: "01/05/2024" is January 5th, never May 1st.
        let d = parse_date_strict("01/05/2024", "%m/%d/%Y").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2024, 1, 5).unwrap());
        // 25 is not a month: strict means error, not "try DD/MM instead".
        assert_eq!(parse_date_strict("01/25/2024", "%d/%m/%Y"), None);
        assert_eq!(
            parse_date_strict("01/25/2024", "%m/%d/%Y"),
            Some(NaiveDate::from_ymd_opt(2024, 1, 25).unwrap())
        );
        // Wrong separator, empty, garbage
        assert_eq!(parse_date_strict("2024-01-15", "%d.%m.%Y"), None);
        assert_eq!(parse_date_strict("", "%Y-%m-%d"), None);
        assert_eq!(parse_date_strict("31-02-2026", "%d-%m-%Y"), None);
    }

    #[test]
    fn strict_parse_tolerates_time_and_checks_year_range() {
        let expected = NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();
        assert_eq!(
            parse_date_strict("2026-09-02 08:00:01", "%Y-%m-%d %H:%M:%S"),
            Some(expected)
        );
        assert_eq!(
            parse_date_strict("2026-09-02 08:00:01", "%Y-%m-%d"),
            Some(expected)
        );
        assert_eq!(
            parse_date_strict("2026-09-02T08:00:01Z", "%Y-%m-%d"),
            Some(expected)
        );
        // Two-digit year read with %Y would be year 26.
        assert_eq!(parse_date_strict("26/09/06", "%Y/%m/%d"), None);
        assert_eq!(
            parse_date_strict("01.09.26", "%d.%m.%y"),
            Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
        );
        assert_eq!(
            parse_date_strict("14. 1. 2026", "%d. %m. %Y"),
            Some(NaiveDate::from_ymd_opt(2026, 1, 14).unwrap())
        );
    }

    #[test]
    fn strict_timestamp_is_midnight_utc() {
        assert_eq!(
            parse_date_to_timestamp_strict("2024-01-15", "%Y-%m-%d"),
            Some(1705276800)
        );
        assert_eq!(parse_date_to_timestamp_strict("nope", "%Y-%m-%d"), None);
    }

    #[test]
    fn date_only_format_cuts_the_time_part() {
        assert_eq!(date_only_format("%Y-%m-%d %H:%M:%S"), "%Y-%m-%d");
        assert_eq!(date_only_format("%d.%m.%Y %H:%M"), "%d.%m.%Y");
        assert_eq!(date_only_format("%Y-%m-%dT%H:%M:%S"), "%Y-%m-%d");
        assert_eq!(date_only_format("%d/%m/%Y"), "%d/%m/%Y");
        assert_eq!(date_only_format("%d. %m. %Y"), "%d. %m. %Y");
    }

    fn detected(samples: &[&str]) -> (String, bool) {
        let d = detect_date_format(samples);
        (d.format, d.ambiguous)
    }

    #[test]
    fn detect_format_iso_us_and_european() {
        assert_eq!(
            detected(&["2026-09-01", "2026-09-30"]),
            ("%Y-%m-%d".into(), false)
        );
        assert_eq!(
            detected(&["2026-09-01 10:12:33", "2026-09-02 08:00:01"]),
            ("%Y-%m-%d".into(), false)
        );
        // US file: 09/02, 09/05, 09/12, 09/13 — the last one proves MM/DD.
        assert_eq!(
            detected(&["09/02/2026", "09/05/2026", "09/12/2026", "09/13/2026"]),
            ("%m/%d/%Y".into(), false)
        );
        // UK file: 15/09 proves DD/MM.
        assert_eq!(
            detected(&["01/09/2026", "02/09/2026", "03/09/2026", "15/09/2026"]),
            ("%d/%m/%Y".into(), false)
        );
        assert_eq!(
            detected(&["01.09.2026", "30.09.2026"]),
            ("%d.%m.%Y".into(), false)
        );
        assert_eq!(
            detected(&["01-09-2026", "25-09-2026"]),
            ("%d-%m-%Y".into(), false)
        );
        // ... while "01-09" and "10-09" alone read both ways.
        assert_eq!(
            detected(&["01-09-2026", "10-09-2026"]),
            ("%d-%m-%Y".into(), true)
        );
        // Two-digit years: the year may be first or last, so a file is only
        // decided when a value rules a layout out.
        assert_eq!(
            detected(&["01.09.26", "30.09.26"]),
            ("%d.%m.%y".into(), true),
            "30.09.26 reads as DD.MM.YY and as YY.MM.DD"
        );
        assert_eq!(
            detected(&["31.12.99", "30.09.98"]),
            ("%d.%m.%y".into(), false),
            "99 cannot be a day"
        );
        assert_eq!(
            detected(&["99-12-31", "98-11-30"]),
            ("%y-%m-%d".into(), false),
            "99 cannot be a day: the year is first"
        );
        assert_eq!(
            detected(&["12/25/24", "01/31/24"]),
            ("%m/%d/%y".into(), false),
            "25 cannot be a month"
        );
        assert_eq!(detected(&["24-01-15"]), ("%d-%m-%y".into(), true));
        // Czech dates with spaces after the dots
        assert_eq!(
            detected(&["14. 1. 2026", "1. 12. 2025"]),
            ("%d. %m. %Y".into(), false)
        );
    }

    #[test]
    fn detect_format_flags_ambiguity() {
        // 01/02, 03/04 … all ≤ 12 in both positions: European default, but a guess.
        assert_eq!(
            detected(&["01/02/2026", "03/04/2026", "05/06/2026"]),
            ("%d/%m/%Y".into(), true)
        );
        // ISO is never ambiguous.
        assert_eq!(detected(&["2026-01-02"]), ("%Y-%m-%d".into(), false));
        // Nothing readable: ISO guess, flagged.
        assert_eq!(detected(&[]), ("%Y-%m-%d".into(), true));
        assert_eq!(detected(&["", "n/a"]), ("%Y-%m-%d".into(), true));
    }

    #[test]
    fn detected_format_parses_its_own_samples() {
        for samples in [
            vec!["09/02/2026", "09/13/2026"],
            vec!["01.09.26", "30.09.26"],
            vec!["2026-09-01 10:12:33"],
            vec!["14. 1. 2026", "1. 12. 2025"],
        ] {
            let d = detect_date_format(&samples);
            for s in &samples {
                assert!(
                    parse_date_strict(s, &d.format).is_some(),
                    "{s:?} must parse with its own detected format {:?}",
                    d.format
                );
            }
        }
    }
}
