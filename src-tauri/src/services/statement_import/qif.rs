//! QIF parser, field by field as in the design spec §6.4.
//!
//! A QIF file is a list of sections (`!Type:Bank`, `!Account`, ...), each a list of records
//! that end with `^`; every line of a record is a field code and its value. The file is read in
//! two passes. [`scan`] sorts the records into the accounts they belong to (investment records
//! are only counted); the dates and the amounts of the whole file are then looked at together,
//! because neither the order of the date parts nor the decimal separator is written down in the
//! file: both are decided once, over every record, and every record is read with the result.

use chrono::{Datelike, NaiveDate, Utc};

use crate::error::{AppError, Result};
use crate::services::csv_import::amounts::{
    amount_to_text, clean_and_parse_amount, detect_decimal_separator,
};
use crate::services::csv_import::DeclaredBalance;
use crate::services::date_parser::detect_date_format;

use super::model::{malformed, utc_day, AccountRef, Entry, Statement};
use super::types::{DateOrder, StatementWarning};

/// Warning key of a file with `!Type:Invst` sections (`detail` = the number of records).
const INVESTMENT_IGNORED: &str = "statementImport.investmentIgnored";

/// The statements of one QIF file, what was left out of them, and how its dates were read.
#[derive(Debug)]
pub struct QifParse {
    pub statements: Vec<Statement>,
    pub warnings: Vec<StatementWarning>,
    /// `true` when the order of the day and month cannot be told from the dates.
    pub date_order_ambiguous: bool,
    /// The order the dates of the file point to (the European one when they do not decide).
    pub detected_date_order: Option<DateOrder>,
}

/// Parses a QIF file into one statement per account that has bank, cash, card or "other"
/// transactions, in the order the accounts first have one. `date_order` is the user's choice;
/// `None` = the detected order. Investment sections are counted into a warning; a file with
/// nothing else is `validation.statementInvestmentOnly`.
pub fn parse(text: &str, date_order: Option<DateOrder>) -> Result<QifParse> {
    let scan = scan(text);
    if scan.accounts.is_empty() {
        return Err(AppError::Validation(
            if scan.investment_records > 0 {
                "validation.statementInvestmentOnly"
            } else {
                "validation.statementNoStatements"
            }
            .into(),
        ));
    }
    let records = || scan.accounts.iter().flat_map(|account| &account.records);

    let dates: Vec<&str> = records().filter_map(|record| record.first('D')).collect();
    if !dates.is_empty() && dates.iter().all(|date| split_date(date).is_none()) {
        return Err(malformed("D"));
    }
    let normalized: Vec<String> = dates.iter().map(|date| normalize_date(date)).collect();
    let samples: Vec<&str> = normalized.iter().map(String::as_str).collect();
    let detected = detect_date_format(&samples);
    let detected_order = order_of(&detected.format);

    let amounts: Vec<&str> = records().filter_map(Record::amount).collect();
    let reader = Reader {
        order: date_order.unwrap_or(detected_order),
        decimal: detect_decimal_separator(&amounts),
        current_yy: Utc::now().year().rem_euclid(100) as u32,
    };

    let statements = scan
        .accounts
        .iter()
        .map(|account| statement(account, &reader))
        .collect::<Result<Vec<_>>>()?;
    let warnings = if scan.investment_records > 0 {
        vec![StatementWarning {
            key: INVESTMENT_IGNORED.to_string(),
            detail: Some(scan.investment_records.to_string()),
        }]
    } else {
        Vec::new()
    };
    Ok(QifParse {
        statements,
        warnings,
        date_order_ambiguous: detected.ambiguous,
        detected_date_order: Some(detected_order),
    })
}

// ---------------------------------------------------------------------------------------------
// Pass 1: sections and records
// ---------------------------------------------------------------------------------------------

/// One record of a transaction section: its lines in file order, as field code and value.
struct Record {
    /// 1-based position among the transaction records of the file (for error messages).
    ordinal: usize,
    fields: Vec<(char, String)>,
}

impl Record {
    /// The first non-empty value of a field.
    fn first(&self, code: char) -> Option<&str> {
        self.all(code).find(|value| !value.is_empty())
    }

    /// Every value of a field, empty ones included.
    fn all(&self, code: char) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .filter(move |(field, _)| *field == code)
            .map(|(_, value)| value.as_str())
    }

    /// The amount: `T`, else `U`.
    fn amount(&self) -> Option<&str> {
        self.first('T').or_else(|| self.first('U'))
    }
}

/// The records of one account, which become one statement.
struct Account {
    name: Option<String>,
    records: Vec<Record>,
}

#[derive(Default)]
struct Scan {
    accounts: Vec<Account>,
    /// Records of `!Type:Invst` sections.
    investment_records: usize,
}

/// What the records after a `!` header are.
#[derive(Clone, Copy, Default)]
enum Section {
    /// `!Account`: records that name an account.
    Account,
    /// `!Type:Bank`, `Cash`, `CCard`, `Oth A`, `Oth L`.
    Transactions,
    /// `!Type:Invst`.
    Investment,
    /// Everything else (`!Type:Cat`, `Class`, `Memorized`, `Prices`, `!Option:*`, `!Clear:*`),
    /// and the lines before the first header.
    #[default]
    Ignored,
}

fn section_of(header: &str) -> Section {
    let header = header.trim().to_lowercase();
    if header == "account" {
        return Section::Account;
    }
    match header.strip_prefix("type:").map(str::trim) {
        Some("bank" | "cash" | "ccard" | "oth a" | "oth l") => Section::Transactions,
        Some("invst") => Section::Investment,
        _ => Section::Ignored,
    }
}

/// Sorts the records of a file into accounts. Lines end with CR, LF or both; a record the file
/// ends in (or a header interrupts) without `^` still counts.
fn scan(text: &str) -> Scan {
    let mut scanner = Scanner::default();
    for line in text.trim_start_matches('\u{feff}').split(['\r', '\n']) {
        scanner.line(line.trim_end());
    }
    scanner.end_record();
    scanner.scan
}

#[derive(Default)]
struct Scanner {
    scan: Scan,
    section: Section,
    /// The name of the account the transactions belong to (`None` = the one unnamed account).
    current: Option<String>,
    /// Index into `scan.accounts` of the current account, once it has a record.
    active: Option<usize>,
    fields: Vec<(char, String)>,
    ordinal: usize,
}

impl Scanner {
    fn line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if let Some(header) = line.strip_prefix('!') {
            self.end_record();
            self.section = section_of(header);
        } else if line.starts_with('^') {
            self.end_record();
        } else if !matches!(self.section, Section::Ignored) {
            let mut chars = line.chars();
            if let Some(code) = chars.next() {
                self.fields.push((code, chars.as_str().trim().to_string()));
            }
        }
    }

    fn end_record(&mut self) {
        let fields = std::mem::take(&mut self.fields);
        if fields.is_empty() {
            return;
        }
        match self.section {
            Section::Account => {
                self.current = fields
                    .into_iter()
                    .find(|(code, value)| *code == 'N' && !value.is_empty())
                    .map(|(_, name)| name);
                self.active = None;
            }
            Section::Transactions => {
                self.ordinal += 1;
                let index = match self.active {
                    Some(index) => index,
                    None => self.open_account(),
                };
                self.scan.accounts[index].records.push(Record {
                    ordinal: self.ordinal,
                    fields,
                });
            }
            Section::Investment => self.scan.investment_records += 1,
            Section::Ignored => {}
        }
    }

    /// The index of the current account, added (as the last one) when it has no record yet.
    fn open_account(&mut self) -> usize {
        let index = self
            .scan
            .accounts
            .iter()
            .position(|account| account.name == self.current)
            .unwrap_or_else(|| {
                self.scan.accounts.push(Account {
                    name: self.current.clone(),
                    records: Vec::new(),
                });
                self.scan.accounts.len() - 1
            });
        self.active = Some(index);
        index
    }
}

// ---------------------------------------------------------------------------------------------
// Pass 2: records to statements
// ---------------------------------------------------------------------------------------------

/// What the whole file decided for how a record is read.
struct Reader {
    order: DateOrder,
    decimal: char,
    /// The current year modulo 100, for two-digit years.
    current_yy: u32,
}

/// A record's day and signed amount.
struct Movement {
    day: i64,
    /// Unsigned canonical decimal text.
    amount: String,
    negative: bool,
}

impl Reader {
    /// The day (`D`) and amount (`T`, else `U`) of a record; `validation.statementMalformed`
    /// (`record <n>`) when either is missing or cannot be read.
    fn movement(&self, record: &Record) -> Result<Movement> {
        let unreadable = || malformed(&format!("record {}", record.ordinal));
        let day = record
            .first('D')
            .and_then(|date| parse_date(date, self.order, self.current_yy))
            .ok_or_else(unreadable)?;
        let value = record
            .amount()
            .and_then(|amount| clean_and_parse_amount(amount, self.decimal).ok())
            .ok_or_else(unreadable)?;
        Ok(Movement {
            day,
            amount: amount_to_text(value),
            negative: value < 0.0,
        })
    }
}

fn statement(account: &Account, reader: &Reader) -> Result<Statement> {
    let mut statement = Statement {
        account: AccountRef {
            name: account.name.clone(),
            ..AccountRef::default()
        },
        ..Statement::default()
    };
    for (position, record) in account.records.iter().enumerate() {
        let movement = reader.movement(record)?;
        if position == 0 && is_opening_balance(record, account.name.as_deref()) {
            // QIF names no currency; the statement's is the account's.
            statement.opening = Some(DeclaredBalance {
                amount: signed(&movement),
                currency: String::new(),
                as_of: movement.day,
            });
            continue;
        }
        statement.entries.push(entry(record, movement));
    }
    Ok(statement)
}

/// Quicken starts an account with a record that is a transfer to the account itself: its
/// amount is the balance the account had then.
fn is_opening_balance(record: &Record, account_name: Option<&str>) -> bool {
    match (record.first('L'), account_name) {
        (Some(category), Some(name)) => {
            category.to_lowercase() == format!("[{name}]").to_lowercase()
        }
        _ => false,
    }
}

fn signed(movement: &Movement) -> String {
    if movement.negative {
        format!("-{}", movement.amount)
    } else {
        movement.amount.clone()
    }
}

fn entry(record: &Record, movement: Movement) -> Entry {
    let payee = record.first('P');
    Entry {
        booking_date: movement.day,
        is_credit: !movement.negative,
        amount: movement.amount,
        description: record.first('M').or(payee).map(str::to_string),
        counterparty_name: payee.map(str::to_string),
        source_category: source_category(record),
        ..Entry::default()
    }
}

/// The category of the splits when they all share one (a record with splits is not
/// categorized by its `L` line), else `None`; without splits, the `L` category.
fn source_category(record: &Record) -> Option<String> {
    let mut splits = record.all('S').map(category).peekable();
    if splits.peek().is_none() {
        return record.first('L').and_then(category);
    }
    let first = splits.next()?;
    splits
        .all(|other| other == first)
        .then_some(first)
        .flatten()
}

/// A category line without its `/class`; `None` for none and for `[Account]` (a transfer).
fn category(text: &str) -> Option<String> {
    let name = text.split('/').next().unwrap_or_default().trim();
    (!name.is_empty() && !name.starts_with('[')).then(|| name.to_string())
}

// ---------------------------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------------------------

/// A QIF date as `date_parser` reads it: `'` is a `/`, and padding spaces are dropped
/// (`" 9/ 5'26"` → `9/5/26`).
fn normalize_date(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '\'' { '/' } else { c })
        .collect()
}

/// The date order a chrono format of `detect_date_format` starts with.
fn order_of(format: &str) -> DateOrder {
    if format.starts_with("%d") {
        DateOrder::Dmy
    } else if format.starts_with("%m") {
        DateOrder::Mdy
    } else {
        DateOrder::Ymd
    }
}

/// A date taken apart: the three numbers, their digit counts and the two separators.
struct DateParts {
    numbers: [u32; 3],
    digits: [usize; 3],
    separators: [char; 2],
}

/// Three numbers between `/`, `.`, `-` or `'`; spaces are ignored. `None` for anything else.
fn split_date(raw: &str) -> Option<DateParts> {
    let mut parts = vec![String::new()];
    let mut separators = Vec::new();
    for c in raw.chars().filter(|c| !c.is_whitespace()) {
        match c {
            '0'..='9' => parts.last_mut()?.push(c),
            '/' | '.' | '-' | '\'' => {
                separators.push(c);
                parts.push(String::new());
            }
            _ => return None,
        }
    }
    let [first, second, third] = <[String; 3]>::try_from(parts).ok()?;
    let [first_separator, second_separator] = <[char; 2]>::try_from(separators).ok()?;
    let number = |part: &String| {
        (!part.is_empty())
            .then(|| part.parse::<u32>().ok())
            .flatten()
    };
    Some(DateParts {
        numbers: [number(&first)?, number(&second)?, number(&third)?],
        digits: [first.len(), second.len(), third.len()],
        separators: [first_separator, second_separator],
    })
}

/// The calendar day of a date written in `order`, as UTC midnight. A four-digit year must be
/// 1900 to 2100. A two-digit year follows [`expand_year`], with `'` before the year (the
/// separator that precedes it when it comes last) as the apostrophe.
fn parse_date(raw: &str, order: DateOrder, current_yy: u32) -> Option<i64> {
    let parts = split_date(raw)?;
    let (year_slot, month_slot, day_slot, year_last) = match order {
        DateOrder::Dmy => (2, 1, 0, true),
        DateOrder::Mdy => (2, 0, 1, true),
        DateOrder::Ymd => (0, 1, 2, false),
    };
    let year_number = parts.numbers[year_slot];
    let year = match parts.digits[year_slot] {
        4 => i32::try_from(year_number)
            .ok()
            .filter(|year| (1900..=2100).contains(year))?,
        2 => expand_year(
            year_number,
            year_last && parts.separators[1] == '\'',
            current_yy,
        ),
        _ => return None,
    };
    NaiveDate::from_ymd_opt(year, parts.numbers[month_slot], parts.numbers[day_slot]).map(utc_day)
}

/// The year of a two-digit one: after `'` always 20YY (Quicken's marker for 2000 on), else 20YY
/// up to the year after the current one, and 19YY beyond.
fn expand_year(yy: u32, apostrophe: bool, current_yy: u32) -> i32 {
    let century = if apostrophe || yy <= current_yy + 1 {
        2000
    } else {
        1900
    };
    century + yy as i32
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::super::model::{utc_day, StatementFormat};
    use super::*;
    use crate::error::AppError;

    fn day(y: i32, m: u32, d: u32) -> i64 {
        utc_day(NaiveDate::from_ymd_opt(y, m, d).unwrap())
    }
    fn fixture_bytes(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/statements/qif/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }
    fn fixture_text(name: &str) -> String {
        String::from_utf8(fixture_bytes(name)).unwrap()
    }
    fn key(error: &AppError) -> String {
        error.keyed().expect("keyed").0.to_string()
    }
    fn detail(error: &AppError) -> String {
        let (key, params) = error.keyed().expect("keyed");
        assert_eq!(key, "validation.statementMalformed");
        params["detail"].as_str().expect("detail").to_string()
    }
    /// The entries of the first statement of a one-account file with these records.
    fn entries(records: &str) -> Vec<super::super::model::Entry> {
        parse(&format!("!Type:Bank\n{records}"), None)
            .unwrap()
            .statements
            .remove(0)
            .entries
    }

    #[test]
    fn quicken_us_file() {
        let text = fixture_text("quicken-us.qif");
        assert!(text.contains("\r\n"));
        let p = parse(&text, None).unwrap();
        assert!(!p.date_order_ambiguous);
        assert_eq!(p.detected_date_order, Some(DateOrder::Mdy));
        let s = &p.statements[0];
        assert_eq!(s.account.name.as_deref(), Some("Checking"));
        assert_eq!(
            s.opening.as_ref().map(|b| (b.amount.as_str(), b.as_of)),
            Some(("1000", day(2026, 8, 31)))
        );
        assert_eq!(s.closing, None);
        assert_eq!(
            s.entries.len(),
            3,
            "the opening balance record is not an entry"
        );
        assert_eq!(
            (s.entries[0].amount.as_str(), s.entries[0].is_credit),
            ("500.5", true)
        );
        assert_eq!(
            s.entries[0].source_category.as_deref(),
            Some("Income:Salary")
        );
        assert_eq!(s.entries[0].description.as_deref(), Some("Invoice 17"));
        assert_eq!(
            s.entries[1].source_category, None,
            "splits with different categories"
        );
        assert_eq!(s.entries[1].amount, "250");
        assert_eq!(
            s.entries[2].source_category, None,
            "a transfer has no category"
        );
        assert_eq!(s.entries[2].booking_date, day(2026, 9, 21));
    }

    #[test]
    fn european_file_in_windows_1250() {
        let bytes = fixture_bytes("european.qif");
        let text = crate::services::statement_import::decode::decode(
            &bytes,
            StatementFormat::Qif,
            Some("CZ"),
        )
        .text;
        let p = parse(&text, None).unwrap();
        assert_eq!(p.detected_date_order, Some(DateOrder::Dmy));
        let e = &p.statements[0].entries[0];
        assert_eq!(
            (e.amount.as_str(), e.is_credit, e.booking_date),
            ("1234.56", false, day(2026, 10, 15))
        );
        assert_eq!(e.counterparty_name.as_deref(), Some("Žluťoučký kůň"));
        assert_eq!(e.source_category.as_deref(), Some("Jídlo"));
    }

    #[test]
    fn ambiguous_dates_need_a_choice() {
        let text = "!Type:Bank\nD03/04/2026\nT5\n^\nD05/06/2026\nT6\n^\n";
        let detected = parse(text, None).unwrap();
        assert!(detected.date_order_ambiguous);
        let chosen = parse(text, Some(DateOrder::Mdy)).unwrap();
        assert_eq!(
            chosen.statements[0].entries[0].booking_date,
            day(2026, 3, 4)
        );
        let dmy = parse(text, Some(DateOrder::Dmy)).unwrap();
        assert_eq!(dmy.statements[0].entries[0].booking_date, day(2026, 4, 3));
    }

    #[test]
    fn two_accounts_and_investments() {
        let p = parse(&fixture_text("two-accounts.qif"), None).unwrap();
        assert_eq!(
            p.statements
                .iter()
                .map(|s| s.account.name.clone().unwrap())
                .collect::<Vec<_>>(),
            vec!["Checking", "Savings"]
        );
        assert_eq!(
            (p.warnings[0].key.as_str(), p.warnings[0].detail.as_deref()),
            ("statementImport.investmentIgnored", Some("2"))
        );
    }

    #[test]
    fn two_digit_years() {
        let p = parse("!Type:Bank\nD12/31/99\nT1\n^\nD1/1/27\nT1\n^\n", None).unwrap();
        // 99 > current YY + 1 → 1999; 27 ≤ current YY + 1 (in 2026) → 2027
        assert_eq!(p.statements[0].entries[0].booking_date, day(1999, 12, 31));
        assert_eq!(p.statements[0].entries[1].booking_date, day(2027, 1, 1));
    }

    #[test]
    fn the_opening_balance_is_the_first_record_and_matches_the_name_in_any_case() {
        let text = "!Account\nNChecking\n^\n!Type:Bank\nD1/31/2026\nT-12.50\nL[CHECKING]\n^\n\
                    D2/1/2026\nT5\nL[checking]\n^\n";
        let statement = parse(text, None).unwrap().statements.remove(0);
        let opening = statement.opening.unwrap();
        assert_eq!(
            (opening.amount.as_str(), opening.as_of),
            ("-12.5", day(2026, 1, 31)),
            "a negative balance keeps its sign"
        );
        assert_eq!(statement.entries.len(), 1, "only the first record counts");
        assert_eq!(statement.entries[0].source_category, None);
        // an unnamed account has no name to match
        let unnamed = parse("!Type:Bank\nD1/31/2026\nT1\nL[Checking]\n^\n", None).unwrap();
        assert_eq!(unnamed.statements[0].opening, None);
        assert_eq!(unnamed.statements[0].entries.len(), 1);
    }

    #[test]
    fn unreadable_records_name_their_position_in_the_file() {
        let record = |text: &str| detail(&parse(text, None).unwrap_err());
        // no amount
        assert_eq!(
            record("!Type:Bank\nD1/2/2026\nT5\n^\nD1/3/2026\n^\n"),
            "record 2"
        );
        // no date
        assert_eq!(record("!Type:Bank\nD1/2/2026\nT5\n^\nT6\n^\n"), "record 2");
        // a date that is none: month 13 in every order
        assert_eq!(record("!Type:Bank\nD13/13/2026\nT5\n^\n"), "record 1");
        // an amount that is none
        assert_eq!(record("!Type:Bank\nD1/2/2026\nTabc\n^\n"), "record 1");
        // positions run over the accounts of the file
        let second_account = "!Account\nNA\n^\n!Type:Bank\nD1/2/2026\nT5\n^\n!Account\nNB\n^\n\
                              !Type:Bank\nD1/2/2026\n^\n";
        assert_eq!(record(second_account), "record 2");
        // dates that no order can read at all
        assert_eq!(record("!Type:Bank\nDyesterday\nT5\n^\n"), "D");
    }

    #[test]
    fn a_chosen_order_that_does_not_fit_a_date_fails_that_record() {
        let text = "!Type:Bank\nD09/20/2026\nT5\n^\n";
        assert_eq!(
            detail(&parse(text, Some(DateOrder::Dmy)).unwrap_err()),
            "record 1"
        );
        assert!(parse(text, Some(DateOrder::Mdy)).is_ok());
    }

    #[test]
    fn a_choice_does_not_change_what_was_detected() {
        let text = "!Type:Bank\nD03/04/2026\nT5\n^\n";
        let chosen = parse(text, Some(DateOrder::Mdy)).unwrap();
        assert!(chosen.date_order_ambiguous);
        assert_eq!(chosen.detected_date_order, Some(DateOrder::Dmy));
    }

    #[test]
    fn year_first_dates_are_detected() {
        let p = parse("!Type:Bank\nD2026-09-05\nT1\n^\nD2026-10-20\nT1\n^\n", None).unwrap();
        assert_eq!(p.detected_date_order, Some(DateOrder::Ymd));
        assert!(!p.date_order_ambiguous);
        assert_eq!(p.statements[0].entries[0].booking_date, day(2026, 9, 5));
    }

    #[test]
    fn a_file_without_bank_records_is_refused_by_what_it_has() {
        let none = |text: &str| key(&parse(text, None).unwrap_err());
        assert_eq!(none(""), "validation.statementNoStatements");
        assert_eq!(
            none("!Type:Bank\n"),
            "validation.statementNoStatements",
            "a section without records"
        );
        assert_eq!(
            none("!Type:Cat\nNFood\n^\n"),
            "validation.statementNoStatements"
        );
        assert_eq!(
            none("!Type:Invst\nD1/2/2026\nNBuy\nT5\n^\n"),
            "validation.statementInvestmentOnly"
        );
    }

    #[test]
    fn sections_are_matched_in_any_case_and_continue_the_account() {
        let text = "!Account\nNMain\n^\n\
                    !TYPE:BANK\nD1/20/2026\nT1\n^\n\
                    !Type:Cash\nD1/21/2026\nT2\n^\n\
                    !type:ccard\nD1/22/2026\nT3\n^\n\
                    !Type:Oth A\nD1/23/2026\nT4\n^\n\
                    !Type:Oth L\nD1/24/2026\nT5\n^\n";
        let p = parse(text, None).unwrap();
        assert_eq!(p.statements.len(), 1);
        assert_eq!(p.statements[0].entries.len(), 5);
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn other_sections_are_ignored() {
        let text = "!Option:AutoSwitch\n!Account\nNA\nTBank\n^\nNB\nTBank\n^\n!Clear:AutoSwitch\n\
                    !Type:Cat\nNFood\nE\n^\n!Type:Class\nNHome\n^\n!Type:Memorized\nT9\n^\n\
                    !Account\nNA\n^\n!Type:Bank\nD1/20/2026\nT1\n^\n\
                    !Type:Prices\n\"ACME\",10,\"1/2'26\"\n^\n";
        let p = parse(text, None).unwrap();
        assert_eq!(
            p.statements
                .iter()
                .map(|s| (s.account.name.as_deref(), s.entries.len()))
                .collect::<Vec<_>>(),
            vec![(Some("A"), 1)],
            "B is only listed, A's categories and prices are not transactions"
        );
    }

    #[test]
    fn an_account_met_again_keeps_one_statement_in_the_order_of_its_first_record() {
        let text = "!Account\nNA\n^\n!Type:Bank\nD1/20/2026\nT1\n^\n\
                    !Account\nNB\n^\n!Type:Bank\nD1/21/2026\nT2\n^\n\
                    !Account\nNA\n^\n!Type:Bank\nD1/22/2026\nT3\n^\n";
        let p = parse(text, None).unwrap();
        assert_eq!(
            p.statements
                .iter()
                .map(|s| (s.account.name.as_deref(), s.entries.len()))
                .collect::<Vec<_>>(),
            vec![(Some("A"), 2), (Some("B"), 1)]
        );
    }

    #[test]
    fn fields_are_read_as_the_format_defines_them() {
        let e = entries(
            "D 9/ 5'26\nU1,234.50\nPShop\nCX\nN101\nAa street\nAa town\nF\n%5\n^\n\
             D9/6'26\nT-7.00\nM\nPDinner\n^\n\
             D9/25'26\nT1\n^\n",
        );
        assert_eq!(
            e[0].booking_date,
            day(2026, 9, 5),
            "space padding is dropped (the 25th settles month-first)"
        );
        assert_eq!(
            (e[0].amount.as_str(), e[0].is_credit),
            ("1234.5", true),
            "U is the amount"
        );
        assert_eq!(
            e[0].description.as_deref(),
            Some("Shop"),
            "no memo: the payee"
        );
        assert_eq!(e[0].bank_id, None);
        assert_eq!(
            e[1].description.as_deref(),
            Some("Dinner"),
            "an empty memo is no memo"
        );
        assert_eq!(e[1].counterparty_name.as_deref(), Some("Dinner"));
        assert_eq!(e[1].currency, None);
    }

    #[test]
    fn categories_lose_their_class_and_transfers_have_none() {
        let category = |lines: &str| {
            entries(&format!("D1/2/2026\nT-1\n{lines}\n^\n"))
                .remove(0)
                .source_category
        };
        assert_eq!(
            category("LFood:Groceries/Home"),
            Some("Food:Groceries".into())
        );
        assert_eq!(category("L[Savings]/Home"), None);
        assert_eq!(category("L/Home"), None);
        assert_eq!(category(""), None);
        // splits: one shared category wins over `L`, different ones leave none
        assert_eq!(
            category("LFood\nSRent/A\n$-1\nSRent/B\n$-1"),
            Some("Rent".into())
        );
        assert_eq!(category("LFood\nSRent\n$-1\nS[Savings]\n$-1"), None);
        assert_eq!(category("LFood\nS[Savings]\n$-1\nS[Cash]\n$-1"), None);
    }

    #[test]
    fn line_endings_bom_and_a_missing_last_caret() {
        let expected = vec![day(2026, 1, 20), day(2026, 1, 21)];
        for text in [
            "\u{feff}!Type:Bank\r\nD1/20/2026\r\nT1\r\n^\r\nD1/21/2026\r\nT2",
            "!Type:Bank\rD1/20/2026\rT1\r^\rD1/21/2026\rT2\r^\r",
            "!Type:Bank  \nD1/20/2026  \nT1  \n^  \n\nD1/21/2026\nT2\n^\n",
        ] {
            let p = parse(text, None).unwrap();
            let days: Vec<i64> = p.statements[0]
                .entries
                .iter()
                .map(|e| e.booking_date)
                .collect();
            assert_eq!(days, expected, "{text:?}");
        }
    }

    #[test]
    fn amounts_use_one_separator_for_the_whole_file() {
        // "1,234" alone could be a thousand or a decimal comma: the other records decide
        let e = entries("D1/2/2026\nT1,234\n^\nD1/3/2026\nT0.50\n^\n");
        assert_eq!(e[0].amount, "1234");
        let e = entries("D1/2/2026\nT1,234\n^\nD1/3/2026\nT0,50\n^\n");
        assert_eq!(e[0].amount, "1.234");
    }

    #[test]
    fn two_digit_year_centuries_follow_the_current_year() {
        // an apostrophe before the year always means 20YY
        assert_eq!(expand_year(99, true, 26), 2099);
        // a slash: up to next year is 20YY, anything later 19YY
        assert_eq!(expand_year(27, false, 26), 2027);
        assert_eq!(expand_year(28, false, 26), 1928);
        assert_eq!(expand_year(0, false, 99), 2000);
    }
}
