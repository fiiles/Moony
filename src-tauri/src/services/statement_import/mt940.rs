//! MT940 (SWIFT customer statement) parser, field by field as in the design spec §6.2.
//! This file reads the structure (envelopes, fields, `:61:` lines, balances, years, the
//! German BLZ/Konto account); the `:86:` text goes through [`apply_details`].

use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;

use crate::error::Result;
use crate::services::csv_import::DeclaredBalance;
use crate::services::dedup::canonical_amount;
use crate::services::financial_semantics::valid_iban;

use super::decimal::Dec;
use super::model::{utc_day, AccountRef, Entry, Statement};
use super::xml_tree::malformed;

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
    /// The last `:61:`, kept open until the next field that is not its `:86:`.
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

    fn finish(mut self) -> Statement {
        self.flush();
        let statement = &mut self.statement;
        statement.currency = statement
            .opening
            .iter()
            .chain(&statement.closing)
            .map(|balance| balance.currency.clone())
            .next();
        self.statement
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
                statements.extend(current.take().map(Builder::finish));
                continue;
            }
            Item::Field { tag, value } => (tag, value),
        };
        if tag == "20" && current.as_ref().is_some_and(|builder| builder.has_20) {
            statements.extend(current.take().map(Builder::finish));
        }
        let builder = current.get_or_insert_with(Builder::default);
        match tag.as_str() {
            "20" => builder.has_20 = true,
            "25" => builder.statement.account = account_ref(&value),
            "28C" => builder.statement.sequence = non_blank(first_line(&value)),
            "60F" | "60M" => builder.statement.opening = balance(&value),
            "62F" | "62M" => {
                builder.flush();
                builder.statement.closing = balance(&value);
            }
            "61" => {
                entries_seen += 1;
                builder.flush();
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
    statements.extend(current.take().map(Builder::finish));
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
    for line in text.lines().map(str::trim_end) {
        if let Some(start) = FIELD_START.captures(line) {
            items.push(Item::Field {
                tag: start[1].to_string(),
                value: start[2].to_string(),
            });
        } else if line == "-" {
            items.push(Item::End);
        } else if !line.is_empty() {
            if let Some(Item::Field { value, .. }) = items.last_mut() {
                value.push('\n');
                value.push_str(line);
            }
        }
    }
    items
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

/// Reads the `:86:` text of an entry. Not yet read in dialects: the text, with its line
/// breaks as spaces, is the description. Leaves the entry alone when the text is blank (the
/// `:61:` supplementary details then become the description).
fn apply_details(entry: &mut Entry, details: &str) {
    let text = details
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !text.is_empty() {
        entry.description = Some(text);
    }
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
    fn fixture() -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/statements/mt940/basic.sta",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
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
}
