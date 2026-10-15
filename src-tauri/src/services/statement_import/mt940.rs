//! MT940 (SWIFT customer statement) parser, field by field as in the design spec §6.2.
//! This file reads the structure (envelopes, fields, `:61:` lines, balances, years, the
//! German BLZ/Konto account); the `:86:` text goes through [`apply_details`].

use std::ops::RangeInclusive;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;

use crate::error::Result;
use crate::services::csv_import::DeclaredBalance;
use crate::services::dedup::canonical_amount;
use crate::services::financial_semantics::valid_iban;

use super::decimal::Dec;
use super::model::{join_description, malformed, utc_day, AccountRef, Entry, Statement};

/// SWIFT blocks 1, 2, 3 and 5 (one line each; block 3 and 5 hold one level of nested
/// `{…}`, e.g. `{3:{108:MT940}}`).
static SWIFT_HEADER_BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{[1235]:(?:[^{}\n]|\{[^{}\n]*\})*\}").expect("static pattern is valid")
});

/// The opener of block 4, the text block.
static SWIFT_TEXT_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{4:").expect("static pattern is valid"));

/// The closer of block 4: a `-` line ended by `}`. Kept as a `-` line, the end of a statement.
static SWIFT_TEXT_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^-\}").expect("static pattern is valid"));

/// A field starts with its tag at the beginning of a line (`:20:`, `:28C:`, `:NS:`).
static FIELD_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:(\d{2}[A-Z]?|NS):(.*)$").expect("static pattern is valid"));

/// `:60F:` / `:62F:` and the `M` variants: mark, `YYMMDD`, currency, amount with a decimal
/// comma. Only upper-case letters, as the standard writes them.
static BALANCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([CD])(\d{6})([A-Z]{3})([\d,]+)$").expect("static pattern is valid")
});

/// The first line of `:61:`: value date, optional entry date, mark, optional funds code,
/// amount, `N` + type, customer reference (up to the first `//`), optional bank reference.
static ENTRY_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{6})(\d{4})?(RC|RD|C|D)([A-Z])?(\d[\d,]*)([NSF][A-Z0-9]{3})(.*?)(?://(.*))?$")
        .expect("static pattern is valid")
});

/// A German bank code (8 digits) and a five to ten digit account number as `:25:` writes
/// them. A shorter number is not taken: `NNNNNNNN/BBBB` is a Czech account and its four digit
/// bank code.
static GERMAN_ACCOUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{8})/(\d{5,10})$").expect("static pattern is valid"));

/// The start of an `:86:` in the subfield dialect: a three digit code, a separator character
/// (usually `?`, sometimes `>`) and the two digits of the first subfield.
static SUBFIELD_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{3}(\S)[0-9]{2}").expect("static pattern is valid"));

/// The tags of the slash dialect, as `/TAG/`.
static SLASH_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/(?:EREF|ORDP|BENM|NAME|IBAN|BIC|ACCW|REMI|CNTP|TRTP|MARF|CSID|PURP|RTRN|ULTD|ULTC|ADDR|ISDT)/")
        .expect("static pattern is valid")
});

/// What the lines of a message are made of.
enum Item {
    /// A tag and its text; continuation lines follow after `\n`.
    Field { tag: String, value: String },
    /// A `-` line: the end of a statement.
    End,
}

/// The statement being read.
#[derive(Default)]
struct Builder {
    statement: Statement,
    has_20: bool,
    /// The last `:61:`, kept open while its `:86:` (and `:NS:` lines) may still follow. Every
    /// other field closes it, so a statement-level `:86:` after e.g. `:64:` is not its text.
    open: Option<OpenEntry>,
}

struct OpenEntry {
    entry: Entry,
    /// The second line of `:61:`, the description of an entry without `:86:` text.
    supplementary: Option<String>,
}

impl Builder {
    /// Moves the open entry into the statement.
    fn flush(&mut self) {
        if let Some(OpenEntry {
            mut entry,
            supplementary,
        }) = self.open.take()
        {
            if entry.description.is_none() {
                entry.description = supplementary;
            }
            self.statement.entries.push(entry);
        }
    }

    /// The statement, or `None` for a builder that read nothing of one: a stray or ignored
    /// field after a `-` line is no statement.
    fn finish(mut self) -> Option<Statement> {
        self.flush();
        let statement = &mut self.statement;
        if !self.has_20
            && statement.account.identifier.is_none()
            && statement.opening.is_none()
            && statement.closing.is_none()
            && statement.entries.is_empty()
        {
            return None;
        }
        statement.currency = statement
            .opening
            .iter()
            .chain(&statement.closing)
            .map(|balance| balance.currency.clone())
            .next();
        Some(self.statement)
    }
}

/// Parses MT940 text (a bare tag stream or SWIFT envelopes) into its statements, in file
/// order. A `:61:` that cannot be read is `validation.statementMalformed` (detail `:61: <n>`,
/// the 1-based position of the line in the file); an unreadable balance is no declared balance.
pub fn parse(text: &str) -> Result<Vec<Statement>> {
    let mut statements = Vec::new();
    let mut current: Option<Builder> = None;
    let mut entries_seen = 0usize;
    for item in items(&strip_envelopes(&normalize(text))) {
        let (tag, value) = match item {
            Item::End => {
                statements.extend(current.take().and_then(Builder::finish));
                continue;
            }
            Item::Field { tag, value } => (tag, value),
        };
        if tag == "20" && current.as_ref().is_some_and(|builder| builder.has_20) {
            statements.extend(current.take().and_then(Builder::finish));
        }
        let builder = current.get_or_insert_with(Builder::default);
        if !matches!(tag.as_str(), "86" | "NS") {
            builder.flush();
        }
        match tag.as_str() {
            "20" => builder.has_20 = true,
            "25" => builder.statement.account = account_ref(&value),
            "28C" => builder.statement.sequence = non_blank(first_line(&value)),
            "60F" | "60M" => builder.statement.opening = balance(&value),
            "62F" | "62M" => builder.statement.closing = balance(&value),
            "61" => {
                entries_seen += 1;
                builder.open = Some(
                    open_entry(&value).ok_or_else(|| malformed(&format!(":61: {entries_seen}")))?,
                );
            }
            "86" => {
                if let Some(open) = &mut builder.open {
                    apply_details(&mut open.entry, &value);
                }
            }
            _ => {}
        }
    }
    statements.extend(current.take().and_then(Builder::finish));
    Ok(statements)
}

/// One line break style (`\n`), no byte order mark.
fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

/// Removes the SWIFT blocks around the text block, leaving the tag stream.
fn strip_envelopes(text: &str) -> String {
    let without_headers = SWIFT_HEADER_BLOCK.replace_all(text, "");
    let without_opener = SWIFT_TEXT_OPEN.replace_all(&without_headers, "");
    SWIFT_TEXT_CLOSE
        .replace_all(&without_opener, "-")
        .into_owned()
}

/// Splits the lines into fields and statement ends; a line that starts no field continues
/// the previous one (lines before the first field, or after an end, are dropped).
fn items(text: &str) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();
    for line in text.lines() {
        if let Some(start) = FIELD_START.captures(line) {
            let tag = start[1].to_string();
            let value = kept(&tag, &start[2]).to_string();
            items.push(Item::Field { tag, value });
        } else if line.trim_end() == "-" {
            items.push(Item::End);
        } else if !line.trim().is_empty() {
            if let Some(Item::Field { tag, value }) = items.last_mut() {
                value.push('\n');
                value.push_str(kept(tag, line));
            }
        }
    }
    items
}

/// What a field keeps of a line: its trailing whitespace goes, except for an `:86:`. Where
/// its text is cut at the end of a line the space belongs to it, and the subfield dialect
/// counts the characters of a segment.
fn kept<'a>(tag: &str, line: &'a str) -> &'a str {
    if tag == "86" {
        line
    } else {
        line.trim_end()
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or_default().trim()
}

fn non_blank(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

/// `:25:` whole, except a German BLZ/Konto, which is written as its IBAN (R4).
fn account_ref(value: &str) -> AccountRef {
    let account = first_line(value);
    AccountRef {
        identifier: german_iban(account).or_else(|| non_blank(account)),
        ..AccountRef::default()
    }
}

/// R4: a German BLZ/Konto as an IBAN; None for anything else.
pub(crate) fn german_iban(account: &str) -> Option<String> {
    let parts = GERMAN_ACCOUNT.captures(account)?;
    let bban = format!("{}{:0>10}", &parts[1], &parts[2]);
    // "DE" is 13 14 and the check digits start out as 00.
    let check = 98 - mod97(&format!("{bban}131400"));
    let iban = format!("DE{check:02}{bban}");
    valid_iban(&iban).then_some(iban)
}

/// The remainder of a digit string divided by 97, one digit at a time.
fn mod97(digits: &str) -> u32 {
    digits
        .bytes()
        .fold(0, |r, b| (r * 10 + u32::from(b - b'0')) % 97)
}

/// `YYMMDD`; 79 and below is 20YY, 80 and above 19YY.
fn yymmdd(text: &str) -> Option<NaiveDate> {
    let part = |range: std::ops::Range<usize>| text.get(range)?.parse::<u32>().ok();
    let year = part(0..2)?;
    let year = if year < 80 { 2000 + year } else { 1900 + year };
    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, part(2..4)?, part(4..6)?)
}

/// The booking day: the entry date `MMDD` with the year of the value date, moved a year when
/// the entry date lies on the other side of New Year (December booked for a January value
/// date, January for a December one). Without an entry date it is the value date.
fn booking_from(value: NaiveDate, entry: Option<(u32, u32)>) -> Option<NaiveDate> {
    let Some((month, day)) = entry else {
        return Some(value);
    };
    let year = match (month, value.month()) {
        (12, 1) => value.year() - 1,
        (1, 12) => value.year() + 1,
        _ => value.year(),
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

/// An unsigned amount with a decimal comma as canonical decimal text; `None` unless it is a
/// plain decimal.
fn amount_text(raw: &str) -> Option<String> {
    let dotted = raw.replace(',', ".");
    Dec::parse(&dotted)?;
    Some(canonical_amount(&dotted))
}

/// `:60F:` and friends; `D` makes the amount negative. `None` when the field cannot be read.
fn balance(value: &str) -> Option<DeclaredBalance> {
    let parts = BALANCE.captures(first_line(value))?;
    let amount = amount_text(&parts[4])?;
    Some(DeclaredBalance {
        amount: if &parts[1] == "D" && amount != "0" {
            format!("-{amount}")
        } else {
            amount
        },
        currency: parts[3].to_string(),
        as_of: utc_day(yymmdd(&parts[2])?),
    })
}

/// The entry of a `:61:` field. `None` when the first line, its dates or its amount cannot
/// be read.
fn open_entry(value: &str) -> Option<OpenEntry> {
    let (line, rest) = value.split_once('\n').unwrap_or((value, ""));
    let parts = ENTRY_LINE.captures(line)?;
    let value_date = yymmdd(&parts[1])?;
    let entry_date = match parts.get(2) {
        Some(mmdd) => Some((
            mmdd.as_str().get(..2)?.parse().ok()?,
            mmdd.as_str().get(2..)?.parse().ok()?,
        )),
        None => None,
    };
    let bank_reference = parts
        .get(8)
        .map(|reference| reference.as_str().trim())
        .filter(|reference| !reference.is_empty() && !reference.eq_ignore_ascii_case("NONREF"));
    let supplementary = rest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Some(OpenEntry {
        entry: Entry {
            booking_date: utc_day(booking_from(value_date, entry_date)?),
            value_date: Some(utc_day(value_date)),
            amount: amount_text(&parts[5])?,
            is_credit: matches!(&parts[3], "C" | "RD"),
            bank_id: bank_reference.map(str::to_string),
            ..Entry::default()
        },
        supplementary: non_blank(&supplementary),
    })
}

/// The longest a subfield segment of the German dialect is; a segment of exactly this length
/// was cut by the field width, so the next one continues it.
const SEGMENT_WIDTH: usize = 27;

/// What an `:86:` text says about its entry.
#[derive(Default)]
struct Details {
    description: Option<String>,
    counterparty_name: Option<String>,
    counterparty_account: Option<String>,
    counterparty_bank_code: Option<String>,
}

impl Details {
    fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.counterparty_name.is_none()
            && self.counterparty_account.is_none()
            && self.counterparty_bank_code.is_none()
    }
}

/// Reads the `:86:` text of an entry (spec §6.2). The dialect is chosen from the shape of the
/// text: subfields (`166?20…?32…`), `/TAG/value` pairs, else free text. What the text does not
/// say stays as it is, so an entry without `:86:` text keeps its `:61:` supplementary details
/// as the description.
fn apply_details(entry: &mut Entry, details: &str) {
    let text = details.trim_start();
    let read = subfield_details(text)
        .or_else(|| slash_details(text))
        .unwrap_or_else(|| free_text_details(text));
    entry.description = read.description.or_else(|| entry.description.take());
    entry.counterparty_name = read
        .counterparty_name
        .or_else(|| entry.counterparty_name.take());
    entry.counterparty_account = read
        .counterparty_account
        .or_else(|| entry.counterparty_account.take());
    entry.counterparty_bank_code = read
        .counterparty_bank_code
        .or_else(|| entry.counterparty_bank_code.take());
}

/// The subfield dialect (DE/AT and some CZ banks): a three digit code, then segments that each
/// start with a separator and two digits. `None` when the text is not of that shape, or says
/// none of what this dialect knows (a number like `100.50 EUR` is no code).
fn subfield_details(text: &str) -> Option<Details> {
    let separator = SUBFIELD_START
        .captures(text)?
        .get(1)?
        .as_str()
        .chars()
        .next()
        .filter(|c| !c.is_alphanumeric())?;
    let segments = split_subfields(text, separator);
    let in_codes = |codes: RangeInclusive<u8>| {
        segments
            .iter()
            .filter(move |(code, _)| codes.contains(code))
            .map(|(_, segment)| segment.as_str())
    };
    // ?20-?29 (purpose), then ?60-?63 (its continuation), behind the posting text ?00
    let body = join_segments(in_codes(20..=29).chain(in_codes(60..=63)));
    let posting_text = collapse(in_codes(0..=0).next().unwrap_or_default());
    let first = |code: u8| {
        in_codes(code..=code)
            .map(compact)
            .find(|value| !value.is_empty())
    };
    let details = Details {
        description: join_description([posting_text.as_str(), body.as_str()]),
        counterparty_name: non_blank(&join_segments(in_codes(32..=33))),
        counterparty_account: first(31),
        counterparty_bank_code: first(30),
    };
    (!details.is_empty()).then_some(details)
}

/// The text split at every separator followed by two digits: the number of the subfield and
/// its text with the line breaks removed (the lines are cut by their width, not by words).
fn split_subfields(text: &str, separator: char) -> Vec<(u8, String)> {
    let marks: Vec<(usize, usize, u8)> = text
        .char_indices()
        .filter(|&(_, c)| c == separator)
        .filter_map(|(at, c)| {
            let from = at + c.len_utf8();
            let digits = text.get(from..from + 2)?;
            let code = digits
                .bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| digits.parse().ok())??;
            Some((at, from + 2, code))
        })
        .collect();
    marks
        .iter()
        .enumerate()
        .map(|(n, &(_, from, code))| {
            let to = marks.get(n + 1).map_or(text.len(), |next| next.0);
            (code, text[from..to].replace('\n', ""))
        })
        .collect()
}

/// Joins subfield segments into one text. A segment of exactly [`SEGMENT_WIDTH`] characters was
/// cut by the field width, so the next one continues it; after a shorter one a space separates
/// the words. Whitespace is collapsed.
fn join_segments<'a>(segments: impl Iterator<Item = &'a str>) -> String {
    let mut joined = String::new();
    let mut previous_width = None;
    for segment in segments.filter(|segment| !segment.trim().is_empty()) {
        if previous_width.is_some_and(|width| width != SEGMENT_WIDTH) {
            joined.push(' ');
        }
        joined.push_str(segment);
        previous_width = Some(segment.chars().count());
    }
    collapse(&joined)
}

/// The slash dialect (NL/BE and others): `/TAG/value` pairs, as soon as a known tag shows up.
/// `None` for text that does not start with a slash or names no known tag.
fn slash_details(text: &str) -> Option<Details> {
    if !text.starts_with('/') {
        return None;
    }
    // The line breaks cut values and even tags (`/N` + `AME/`): put the text back together.
    let flat = text.replace('\n', "");
    let marks: Vec<regex::Match> = SLASH_TAG.find_iter(&flat).collect();
    if marks.is_empty() {
        return None;
    }
    // Every value runs up to the next known tag (the slashes of the tags are ASCII).
    let fields: Vec<(&str, &str)> = marks
        .iter()
        .enumerate()
        .map(|(n, mark)| {
            let to = marks.get(n + 1).map_or(flat.len(), |next| next.start());
            (
                &flat[mark.start() + 1..mark.end() - 1],
                flat[mark.end()..to].trim(),
            )
        })
        .collect();
    let first = |tag: &str| {
        fields
            .iter()
            .filter(|(name, _)| *name == tag)
            .map(|(_, value)| slash_value(value))
            .find(|value| !value.is_empty())
    };
    // /CNTP/ is account/BIC/name/city; the tags of their own win over it
    let cntp: Vec<&str> = first("CNTP")
        .map(|value| value.split('/').map(str::trim).collect())
        .unwrap_or_default();
    let part = |index: usize| cntp.get(index).copied().filter(|part| !part.is_empty());
    let remittance = fields
        .iter()
        .filter(|(name, _)| *name == "REMI")
        .map(|(_, value)| {
            let unstructured = value.strip_prefix("USTD//");
            let structured = value.strip_prefix("STRD/CUR/");
            collapse(slash_value(unstructured.or(structured).unwrap_or(value)))
        })
        .find(|text| !text.is_empty());
    Some(Details {
        description: remittance,
        counterparty_name: first("NAME").or(part(2)).map(collapse),
        counterparty_account: first("IBAN").or(first("ACCW")).or(part(0)).map(compact),
        counterparty_bank_code: first("BIC").or(part(1)).map(compact),
    })
}

/// A slash dialect value without the `/` that closes it.
fn slash_value(value: &str) -> &str {
    value.trim().trim_end_matches('/').trim()
}

/// Free text: the whole `:86:` as one line, and its IBAN when it names exactly one.
fn free_text_details(text: &str) -> Details {
    let description = collapse(text);
    if description.is_empty() {
        return Details::default();
    }
    Details {
        counterparty_account: single_iban(&description),
        description: Some(description),
        ..Details::default()
    }
}

/// The one IBAN a text names. An IBAN is written in groups, so a token that starts like one
/// (two letters, two digits) is tried alone and joined with up to eight tokens after it; the
/// words after an IBAN are never part of it because only checksum-valid joins count. `None`
/// when the text has no IBAN, or several different ones.
fn single_iban(text: &str) -> Option<String> {
    let tokens: Vec<&str> = text
        .split_whitespace()
        .map(|token| token.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|token| !token.is_empty())
        .collect();
    let mut found: Vec<String> = Vec::new();
    for (start, token) in tokens.iter().enumerate() {
        let bytes = token.as_bytes();
        let starts_like_iban = bytes.len() >= 4
            && bytes[..2].iter().all(u8::is_ascii_alphabetic)
            && bytes[2..4].iter().all(u8::is_ascii_digit);
        if !starts_like_iban {
            continue;
        }
        let mut candidate = String::new();
        for next in &tokens[start..tokens.len().min(start + 9)] {
            candidate.push_str(next);
            let upper = candidate.to_ascii_uppercase();
            if valid_iban(&upper) && !found.contains(&upper) {
                found.push(upper);
            }
        }
    }
    (found.len() == 1).then(|| found.remove(0))
}

/// Whitespace runs (and line breaks) as single spaces, no whitespace at either end.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text without any whitespace (IBANs, BICs and account numbers have none).
fn compact(text: &str) -> String {
    text.split_whitespace().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::statement_import::model::StatementFormat;
    use crate::services::statement_import::{
        merge::{merge, BalanceCheck},
        model::utc_day,
    };
    use chrono::NaiveDate;

    fn day(y: i32, m: u32, d: u32) -> i64 {
        utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }
    fn fixture_text(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/statements/mt940/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }
    fn fixture() -> String {
        fixture_text("basic.sta")
    }

    #[test]
    fn crlf_pages_entries_and_balances() {
        let text = fixture();
        assert!(text.contains("\r\n"), "fixture must keep CRLF");
        let statements = parse(&text).unwrap();
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].sequence.as_deref(), Some("00009/001"));
        let merged = merge(statements);
        assert_eq!(merged[0].balance_check, BalanceCheck::AddsUp);
        let e = &merged[0].statement.entries;
        assert_eq!(
            (
                e[0].amount.as_str(),
                e[0].is_credit,
                e[0].bank_id.as_deref()
            ),
            ("500.5", true, Some("BANKREF001"))
        );
        assert_eq!(e[0].booking_date, day(2026, 9, 5));
        assert_eq!((e[1].amount.as_str(), e[1].is_credit), ("250", false));
        assert_eq!(e[1].description.as_deref(), Some("Platba kartou Lidl"));
        assert!(e[2].is_credit, "RD is a reversal of a debit: a credit");
        assert_eq!(e[2].bank_id, None, "NONREF is never an ID");
    }

    #[test]
    fn supplementary_details_when_86_is_missing() {
        let text = fixture().replace(":86:Platba kartou Lidl\r\n", "");
        let e = &parse(&text).unwrap()[1].entries[0];
        assert_eq!(e.description.as_deref(), Some("Card 1234"));
    }

    #[test]
    fn entry_date_across_new_year() {
        let s = ":20:X\n:25:CZ6508000000192000145399\n:60F:C251231CZK0,\n:61:2601021231D5,NMSCNONREF\n:62F:D260102CZK5,\n";
        let e = &parse(s).unwrap()[0].entries[0];
        assert_eq!(
            (e.booking_date, e.value_date),
            (day(2025, 12, 31), Some(day(2026, 1, 2)))
        );
        let s2 = s.replace("2601021231", "2512310102");
        let e2 = &parse(&s2).unwrap()[0].entries[0];
        assert_eq!(e2.booking_date, day(2026, 1, 2));
    }

    #[test]
    fn swift_envelope_and_german_account() {
        let s = "{1:F01DEUTDEFFAXXX0000000000}{2:O9401200261001DEUTDEFFAXXX00000000002610011200N}{4:\n:20:X\n:25:37040044/532013000\n:60F:C260901EUR0,\n:61:2609010901C1,NTRFNONREF//R1\n:86:x\n:62F:C260901EUR1,\n-}";
        let st = &parse(s).unwrap()[0];
        assert_eq!(
            st.account.identifier.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(st.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn czech_domestic_account_is_kept_whole() {
        let s = ":20:X\n:25:19-2000145399/0800\n:60F:C260901CZK0,\n:62F:C260901CZK0,\n";
        assert_eq!(
            parse(s).unwrap()[0].account.identifier.as_deref(),
            Some("19-2000145399/0800")
        );
    }

    #[test]
    fn decoded_windows_1250_keeps_diacritics() {
        // Review Focus 5, end to end with Task 4's decoder
        let (bytes, _, _) = encoding_rs::WINDOWS_1250.encode(
            ":20:X\n:25:CZ6508000000192000145399\n:60F:C260901CZK0,\n:61:2609010901C1,NTRFNONREF\n:86:Žluťoučký kůň\n:62F:C260901CZK1,\n",
        );
        let text = crate::services::statement_import::decode::decode(
            &bytes,
            StatementFormat::Mt940,
            Some("CZ"),
        )
        .text;
        assert_eq!(
            parse(&text).unwrap()[0].entries[0].description.as_deref(),
            Some("Žluťoučký kůň")
        );
    }

    #[test]
    fn broken_61_is_malformed() {
        let s = ":20:X\n:25:Y\n:60F:C260901CZK0,\n:61:garbage\n";
        assert!(parse(s)
            .unwrap_err()
            .to_string()
            .contains("validation.statementMalformed"));
    }

    const ACCOUNT: &str = ":20:X\n:25:CZ6508000000192000145399\n";

    #[test]
    fn german_iban_is_built_from_blz_and_konto() {
        assert_eq!(
            german_iban("37040044/532013000").as_deref(),
            Some("DE89370400440532013000")
        );
        // the account number is padded to ten digits
        assert_eq!(
            german_iban("10010010/12345").as_deref(),
            german_iban("10010010/0000012345").as_deref()
        );
        assert!(german_iban("10010010/12345").unwrap().starts_with("DE"));
        for other in [
            "19-2000145399/0800",
            "CZ6508000000192000145399",
            "3704004/532013000",
            "37040044/53201300012",
            "37040044",
            "",
        ] {
            assert_eq!(german_iban(other), None, "{other}");
        }
    }

    #[test]
    fn czech_account_and_bank_code_is_not_a_german_account() {
        // Czech bank codes are always four digits: NNNNNNNN/BBBB is account/bank, kept whole
        for account in [
            "12345678/0800",
            "37040044/1234",
            "12345678/1",
            "12345678/123",
        ] {
            assert_eq!(german_iban(account), None, "{account}");
            let s = format!(":20:X\n:25:{account}\n:60F:C260901CZK0,\n:62F:C260901CZK0,\n");
            assert_eq!(
                parse(&s).unwrap()[0].account.identifier.as_deref(),
                Some(account)
            );
        }
        // five to ten digit Kontonummern still convert
        assert_eq!(
            german_iban("37040044/532013000").as_deref(),
            Some("DE89370400440532013000")
        );
        assert!(german_iban("12345678/12345").is_some());
        assert!(german_iban("12345678/1234567890").is_some());
    }

    #[test]
    fn balances_are_signed_and_dated_as_written() {
        let s = format!("{ACCOUNT}:60F:D800101EUR1.250,5\n:62F:D791231eur0,\n");
        // a thousands separator is no decimal: the balance is unreadable, not 1.2505
        let st = &parse(&s).unwrap()[0];
        assert_eq!((&st.opening, &st.closing), (&None, &None));

        let s = format!("{ACCOUNT}:60F:D800101EUR1250,50\n:62F:D791231CZK0,\n");
        let st = &parse(&s).unwrap()[0];
        let opening = st.opening.as_ref().unwrap();
        assert_eq!(
            (
                opening.amount.as_str(),
                opening.currency.as_str(),
                opening.as_of
            ),
            ("-1250.5", "EUR", day(1980, 1, 1))
        );
        let closing = st.closing.as_ref().unwrap();
        assert_eq!(
            (closing.amount.as_str(), closing.as_of),
            ("0", day(2079, 12, 31))
        );
        assert_eq!(st.currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn currency_falls_back_to_the_closing_balance() {
        let s = format!("{ACCOUNT}:62F:C260901SEK1,\n");
        assert_eq!(parse(&s).unwrap()[0].currency.as_deref(), Some("SEK"));
        assert_eq!(parse(ACCOUNT).unwrap()[0].currency, None);
    }

    #[test]
    fn entry_without_entry_date_and_with_funds_code() {
        let s = format!("{ACCOUNT}:61:260915RCR12,5NCHKREF 1//BK 7\n:61:260916DR3,NTRFREF\n");
        let entries = &parse(&s).unwrap()[0].entries;
        assert_eq!(entries[0].booking_date, day(2026, 9, 15));
        assert_eq!(entries[0].value_date, Some(day(2026, 9, 15)));
        assert_eq!(
            (
                entries[0].amount.as_str(),
                entries[0].is_credit,
                entries[0].bank_id.as_deref()
            ),
            ("12.5", false, Some("BK 7")),
            "RC is a reversal of a credit: a debit"
        );
        assert_eq!(
            (
                entries[1].amount.as_str(),
                entries[1].is_credit,
                entries[1].bank_id.as_deref()
            ),
            ("3", false, None)
        );
    }

    #[test]
    fn bank_id_is_only_the_reference_after_the_double_slash() {
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFCUSTOMER 1\n:61:2609050905C1,NTRFCUSTOMER 2//nonref\n:61:2609050905C1,NTRFC//\n");
        let entries = &parse(&s).unwrap()[0].entries;
        assert_eq!(
            entries
                .iter()
                .map(|e| e.bank_id.clone())
                .collect::<Vec<_>>(),
            vec![None, None, None]
        );
    }

    #[test]
    fn a_new_20_starts_a_statement_and_a_dash_ends_one() {
        let s = ":20:A\n:25:AAA\n:61:2609050905C1,NTRFX\n:20:B\n:25:BBB\n:61:2609050905C2,NTRFX\n-\n:20:C\n:25:CCC\n";
        let statements = parse(s).unwrap();
        let shape: Vec<_> = statements
            .iter()
            .map(|st| (st.account.identifier.clone().unwrap(), st.entries.len()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("AAA".to_string(), 1),
                ("BBB".to_string(), 1),
                ("CCC".to_string(), 0)
            ]
        );
    }

    #[test]
    fn ns_and_unknown_tags_are_ignored_and_do_not_continue_the_details() {
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX\n:86:Platba\ndruhy radek\n:NS:22Some note\n:64:C260905CZK1,\n:65:C260906CZK1,\n:62F:C260905CZK1,\n:86:statement level info\n");
        let st = &parse(&s).unwrap()[0];
        assert_eq!(st.entries.len(), 1);
        assert_eq!(
            st.entries[0].description.as_deref(),
            Some("Platba druhy radek")
        );
    }

    #[test]
    fn several_swift_messages_with_all_blocks() {
        let one = |n: u8| {
            format!("{{1:F01BANKDEFFAXXX0000000000}}{{2:O9401200260901BANKDEFFAXXX00000000002609011200N}}{{3:{{108:MT940}}}}{{4:\r\n:20:S{n}\r\n:25:CZ6508000000192000145399\r\n:28C:{n}/1\r\n:60F:C260901CZK0,\r\n:61:2609010901C{n},NTRFNONREF\r\n:62F:C260901CZK{n},\r\n-}}{{5:{{CHK:123456789ABC}}}}\r\n")
        };
        let statements = parse(&format!("{}{}", one(1), one(2))).unwrap();
        assert_eq!(statements.len(), 2);
        let sequences: Vec<_> = statements
            .iter()
            .map(|st| st.sequence.clone().unwrap())
            .collect();
        assert_eq!(sequences, vec!["1/1", "2/1"]);
        assert_eq!(statements[1].entries[0].amount, "2");
        assert_eq!(statements[1].closing.as_ref().unwrap().amount, "2");
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_field() {
        let s = format!("\u{feff}{ACCOUNT}");
        assert_eq!(
            parse(&s).unwrap()[0].account.identifier.as_deref(),
            Some("CZ6508000000192000145399")
        );
    }

    #[test]
    fn details_of_several_lines_become_one_line_and_a_blank_one_keeps_the_supplement() {
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX//Y\nSupp\n:86:\n:61:2609050905C1,NTRFX\n:86:  Line one  \n  line two \n");
        let entries = &parse(&s).unwrap()[0].entries;
        assert_eq!(entries[0].description.as_deref(), Some("Supp"));
        assert_eq!(entries[1].description.as_deref(), Some("Line one line two"));
    }

    #[test]
    fn broken_61_names_its_position_in_the_file() {
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX\n:61:2609050905C1,NTRFX\n-\n:20:Y\n:61:260905C1,XOPE\n");
        let (key, params) = parse(&s).unwrap_err().keyed().unwrap();
        assert_eq!(key, "validation.statementMalformed");
        assert_eq!(params, serde_json::json!({ "detail": ":61: 3" }));
    }

    #[test]
    fn unreadable_dates_and_amounts_in_61_are_malformed() {
        for line in [
            "2613050905C1,NTRFX",    // month 13
            "2609050931C1,NTRFX",    // 31 September
            "2702280229C1,NTRFX",    // 29 February of a common year
            "2609050905C1,2,3NTRFX", // two decimal commas
            "2609050905C,NTRFX",     // no digit in the amount
            "2609050905X1,NTRFX",    // not a mark
            "2609050905C1,XTRFX",    // not a type
        ] {
            let s = format!("{ACCOUNT}:61:{line}\n");
            let (key, params) = parse(&s).unwrap_err().keyed().unwrap();
            assert_eq!(key, "validation.statementMalformed", "{line}");
            assert_eq!(params, serde_json::json!({ "detail": ":61: 1" }), "{line}");
        }
    }

    #[test]
    fn a_leap_day_entry_date_is_read_in_a_leap_year() {
        let s = format!("{ACCOUNT}:61:2802290229C1,NTRFX\n");
        assert_eq!(
            parse(&s).unwrap()[0].entries[0].booking_date,
            day(2028, 2, 29)
        );
    }

    #[test]
    fn german_subfields() {
        let mut e = Entry::default();
        // ?20 and ?32 are full 27-character lines cut mid-word
        apply_details(&mut e, "166?00GUTSCHRIFT?100599?20Rechnung 2026-17 Vielen Dan?21k fuer die Zahlung?30COBADEFFXXX?31DE89370400440532013000?32Maximilian Mustermann-Schmi?33dt");
        assert_eq!(
            e.description.as_deref(),
            Some("GUTSCHRIFT | Rechnung 2026-17 Vielen Dank fuer die Zahlung")
        );
        assert_eq!(
            e.counterparty_name.as_deref(),
            Some("Maximilian Mustermann-Schmidt")
        );
        assert_eq!(
            e.counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(e.counterparty_bank_code.as_deref(), Some("COBADEFFXXX"));
    }

    #[test]
    fn german_subfields_short_segments_get_a_space() {
        let mut e = Entry::default();
        apply_details(&mut e, "166?20Miete?21Oktober");
        assert_eq!(e.description.as_deref(), Some("Miete Oktober"));
    }

    #[test]
    fn subfield_separator_other_than_question_mark() {
        let mut e = Entry::default();
        apply_details(&mut e, "166>20Miete>32Vermieter GmbH");
        assert_eq!(
            (e.description.as_deref(), e.counterparty_name.as_deref()),
            (Some("Miete"), Some("Vermieter GmbH"))
        );
    }

    #[test]
    fn subfield_lines_are_joined_in_the_order_of_the_codes_groups() {
        let mut e = Entry::default();
        // ?60-?63 follow ?20-?29 whatever their place in the text; ?00 is only a prefix
        apply_details(
            &mut e,
            "166?60Zeile sechs?20Zeile eins?00BUCHUNG?22Zeile drei",
        );
        assert_eq!(
            e.description.as_deref(),
            Some("BUCHUNG | Zeile eins Zeile drei Zeile sechs")
        );
    }

    #[test]
    fn a_wrapped_subfield_keeps_the_space_before_the_break() {
        // ruling 1: a segment that continues on the next line keeps the trailing space
        let s = format!(
            "{ACCOUNT}:61:2609050905D1,NTRFX\n:86:105?20Miete Oktober \nWohnung 5?21Strasse 1\n"
        );
        let e = &parse(&s).unwrap()[0].entries[0];
        assert_eq!(
            e.description.as_deref(),
            Some("Miete Oktober Wohnung 5 Strasse 1")
        );
        // a full 27-character segment cut inside, from the file's lines
        let s = format!(
            "{ACCOUNT}:61:2609050905D1,NTRFX\n:86:166?20Rechnung 2026-17 Vielen \nDan?21k fuer die Zahlung\n"
        );
        assert_eq!(
            parse(&s).unwrap()[0].entries[0].description.as_deref(),
            Some("Rechnung 2026-17 Vielen Dank fuer die Zahlung")
        );
        // the other fields are still trimmed
        let s = format!("{ACCOUNT}:61:2609050905D1,NTRFX   \n:86:one \ntwo \n");
        assert_eq!(
            parse(&s).unwrap()[0].entries[0].description.as_deref(),
            Some("one two")
        );
    }

    #[test]
    fn a_segment_cut_by_the_line_width_keeps_its_27_characters() {
        // ?20 is "Rechnung 2026-17 Vielen Dan", 27 characters, with the break after the space
        // inside it: trimmed it would be 26 and ?21 would not continue "Dan" into "Dank".
        let mut e = Entry::default();
        apply_details(
            &mut e,
            "166?20Rechnung 2026-17 Vielen \nDan?21k fuer die Zahlung",
        );
        assert_eq!(
            e.description.as_deref(),
            Some("Rechnung 2026-17 Vielen Dank fuer die Zahlung")
        );
    }

    #[test]
    fn text_that_only_looks_like_a_subfield_code_stays_free_text() {
        let mut e = Entry::default();
        apply_details(&mut e, "100.50 EUR Rechnung 2026");
        assert_eq!(e.description.as_deref(), Some("100.50 EUR Rechnung 2026"));
        // a code with subfields of no known kind is no subfield text either
        apply_details(&mut e, "166?100599?50Something");
        assert_eq!(e.description.as_deref(), Some("166?100599?50Something"));
    }

    #[test]
    fn dutch_slash_tags() {
        let mut e = Entry::default();
        apply_details(&mut e, "/TRTP/SEPA OVERBOEKING/IBAN/NL91ABNA0417164300/BIC/ABNANL2A/NAME/J. Jansen/REMI/USTD//Huur oktober/EREF/NOTPROVIDED");
        assert_eq!(e.counterparty_name.as_deref(), Some("J. Jansen"));
        assert_eq!(
            e.counterparty_account.as_deref(),
            Some("NL91ABNA0417164300")
        );
        assert_eq!(e.counterparty_bank_code.as_deref(), Some("ABNANL2A"));
        assert_eq!(e.description.as_deref(), Some("Huur oktober"));
    }

    #[test]
    fn dutch_cntp() {
        let mut e = Entry::default();
        apply_details(
            &mut e,
            "/CNTP/NL91ABNA0417164300/ABNANL2A/J. Jansen/Amsterdam/REMI/USTD//Huur/",
        );
        assert_eq!(
            (
                e.counterparty_account.as_deref(),
                e.counterparty_bank_code.as_deref(),
                e.counterparty_name.as_deref()
            ),
            (
                Some("NL91ABNA0417164300"),
                Some("ABNANL2A"),
                Some("J. Jansen")
            )
        );
        assert_eq!(e.description.as_deref(), Some("Huur"));
    }

    #[test]
    fn slash_tags_beat_cntp_and_accw_is_an_account_fallback() {
        let mut e = Entry::default();
        apply_details(
            &mut e,
            "/CNTP/NL02ABNA0123456789/ABNANL2A/Other/Rotterdam/NAME/J. Jansen/IBAN/NL91ABNA0417164300/BIC/INGBNL2A",
        );
        assert_eq!(
            (
                e.counterparty_account.as_deref(),
                e.counterparty_bank_code.as_deref(),
                e.counterparty_name.as_deref()
            ),
            (
                Some("NL91ABNA0417164300"),
                Some("INGBNL2A"),
                Some("J. Jansen")
            )
        );
        let mut accw = Entry::default();
        apply_details(&mut accw, "/ACCW/NL02ABNA0123456789/NAME/Shop");
        assert_eq!(
            accw.counterparty_account.as_deref(),
            Some("NL02ABNA0123456789")
        );
    }

    #[test]
    fn slash_remittance_prefixes_and_empty_values() {
        let remi = |text: &str| {
            let mut e = Entry::default();
            apply_details(&mut e, text);
            e.description
        };
        assert_eq!(
            remi("/REMI/STRD/CUR/1234567890/EREF/X").as_deref(),
            Some("1234567890")
        );
        assert_eq!(remi("/REMI/Plain text/").as_deref(), Some("Plain text"));
        assert_eq!(remi("/REMI/USTD///EREF/X"), None);
        // known tags without a remittance leave the description to the :61: supplement
        assert_eq!(remi("/TRTP/SEPA/EREF/NOTPROVIDED"), None);
    }

    #[test]
    fn slash_dialect_needs_a_known_tag() {
        let mut e = Entry::default();
        apply_details(&mut e, "/payment/for rent");
        assert_eq!(e.description.as_deref(), Some("/payment/for rent"));
    }

    #[test]
    fn slash_line_breaks_cut_values_and_tags() {
        let mut e = Entry::default();
        apply_details(
            &mut e,
            "/TRTP/SEPA/N\nAME/J. Jan\nsen/REMI/USTD//Huur okt\nober/EREF/X",
        );
        assert_eq!(e.counterparty_name.as_deref(), Some("J. Jansen"));
        assert_eq!(e.description.as_deref(), Some("Huur oktober"));
    }

    #[test]
    fn free_text_takes_a_single_valid_iban() {
        let mut e = Entry::default();
        apply_details(
            &mut e,
            "PLATBA NA UCET CZ55 0800 0000 0012 3456 7899\nNAJEM RIJEN",
        );
        assert_eq!(
            e.description.as_deref(),
            Some("PLATBA NA UCET CZ55 0800 0000 0012 3456 7899 NAJEM RIJEN")
        );
        assert_eq!(
            e.counterparty_account.as_deref(),
            Some("CZ5508000000001234567899")
        );
        let mut two = Entry::default();
        apply_details(
            &mut two,
            "CZ5508000000001234567899 a CZ6508000000192000145399",
        );
        assert_eq!(two.counterparty_account, None);
    }

    #[test]
    fn free_text_iban_edge_cases() {
        let account = |text: &str| {
            let mut e = Entry::default();
            apply_details(&mut e, text);
            e.counterparty_account
        };
        // the same IBAN twice is one IBAN; punctuation around it does not hide it
        assert_eq!(
            account("na CZ55 0800 0000 0012 3456 7899, viz CZ5508000000001234567899.").as_deref(),
            Some("CZ5508000000001234567899")
        );
        // words after the IBAN are not swallowed, a bad checksum is no IBAN
        assert_eq!(
            account("CZ5508000000001234567899 JAN NOVAK").as_deref(),
            Some("CZ5508000000001234567899")
        );
        assert_eq!(account("CZ5508000000001234567890 JAN NOVAK"), None);
        assert_eq!(account("Platba bez uctu CZ12"), None);
    }

    #[test]
    fn blank_details_leave_the_entry_alone() {
        let mut e = Entry {
            description: Some("Supp".into()),
            ..Entry::default()
        };
        apply_details(&mut e, " \n  ");
        assert_eq!(e.description.as_deref(), Some("Supp"));
    }

    #[test]
    fn a_statement_level_86_after_another_field_does_not_overwrite_the_last_entry() {
        // ruling 2: every field tag but :86: and :NS: closes the open entry
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX\n:86:Platba\n:64:C260905CZK1,\n:86:statement level info\n");
        let st = &parse(&s).unwrap()[0];
        assert_eq!(st.entries[0].description.as_deref(), Some("Platba"));
        // :NS: lines between the :61: and its :86: do not close it
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX\n:NS:22Some note\n:86:Platba\n");
        assert_eq!(
            parse(&s).unwrap()[0].entries[0].description.as_deref(),
            Some("Platba")
        );
    }

    #[test]
    fn a_stray_field_after_the_end_of_a_statement_is_no_statement() {
        // ruling 3
        let s = format!("{ACCOUNT}:61:2609050905C1,NTRFX\n-\n:64:C260905CZK1,\n:86:stray\n-\n");
        assert_eq!(parse(&s).unwrap().len(), 1);
        assert!(parse(":64:C260905CZK1,\n:NS:22note\n").unwrap().is_empty());
        // a statement that has only a balance or an account is still one
        assert_eq!(parse(":60F:C260905CZK1,\n").unwrap().len(), 1);
        assert_eq!(parse(":25:CZ6508000000192000145399\n").unwrap().len(), 1);
        assert_eq!(parse(":20:ONLY\n").unwrap().len(), 1);
    }

    #[test]
    fn german_fixture() {
        let text = fixture_text("de-subfields.sta");
        assert!(text.contains("\r\n"), "fixture must keep CRLF");
        assert!(
            text.contains("Oktober \r\n"),
            "fixture must keep the trailing space of a wrapped segment"
        );
        let statements = parse(&text).unwrap();
        let st = &statements[0];
        assert_eq!(
            st.account.identifier.as_deref(),
            Some("DE02120300000000202051")
        );
        let e = &st.entries[0];
        assert_eq!(
            (
                e.counterparty_name.as_deref(),
                e.counterparty_account.as_deref(),
                e.counterparty_bank_code.as_deref(),
                e.description.as_deref()
            ),
            (
                Some("Maximilian Mustermann-Schmidt"),
                Some("DE89370400440532013000"),
                Some("COBADEFFXXX"),
                Some("GUTSCHRIFT | Rechnung 2026-17 Vielen Dank fuer die Zahlung")
            )
        );
        let second = &st.entries[1];
        assert_eq!(
            (
                second.counterparty_name.as_deref(),
                second.counterparty_account.as_deref(),
                second.description.as_deref(),
                second.is_credit
            ),
            (
                Some("Vermieter Beispiel GmbH"),
                Some("DE75512108001245126199"),
                Some("LASTSCHRIFT | Miete Oktober Wohnung 5 Mustergasse 12"),
                false
            )
        );
        assert_eq!(merge(statements)[0].balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn dutch_fixture() {
        let statements = parse(&fixture_text("nl-slash.sta")).unwrap();
        let e = &statements[0].entries[0];
        assert_eq!(
            (
                e.counterparty_name.as_deref(),
                e.counterparty_account.as_deref(),
                e.counterparty_bank_code.as_deref(),
                e.description.as_deref()
            ),
            (
                Some("J. Jansen"),
                Some("NL86INGB0002445588"),
                Some("INGBNL2A"),
                Some("Huur oktober")
            )
        );
        // the second entry has a tag and a word cut by the 65-character lines
        let second = &statements[0].entries[1];
        assert_eq!(
            (
                second.counterparty_name.as_deref(),
                second.counterparty_account.as_deref(),
                second.description.as_deref()
            ),
            (
                Some("Energie Voorbeeld B.V."),
                Some("NL02ABNA0123456789"),
                Some("Termijn oktober")
            )
        );
        assert_eq!(merge(statements)[0].balance_check, BalanceCheck::AddsUp);
    }

    #[test]
    fn czech_free_text_fixture() {
        let statements = parse(&fixture_text("cz-free-text.sta")).unwrap();
        let e = &statements[0].entries[0];
        assert_eq!(
            (
                e.counterparty_name.as_deref(),
                e.counterparty_account.as_deref(),
                e.description.as_deref()
            ),
            (
                None,
                Some("CZ5508000000001234567899"),
                Some("PLATBA NA UCET CZ55 0800 0000 0012 3456 7899 NAJEM RIJEN")
            )
        );
        assert_eq!(
            statements[0].entries[1].description.as_deref(),
            Some("Mzda za září 2026 Acme s.r.o.")
        );
        assert_eq!(merge(statements)[0].balance_check, BalanceCheck::AddsUp);
    }
}
