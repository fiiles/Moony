//! Content-based inference of the columns of a CSV no preset describes (mobile spec §8.6,
//! rung 3 "guided recognition"). Every column gets its role from what its cells hold; the
//! header synonyms of [`suggest_column_mappings`] only break ties between columns the content
//! already qualified, and a field only the content decided carries 0.1 less confidence.
//!
//! The rules, in the order they claim columns (a claimed column takes no later role):
//! - **date**: ≥ 90 % of the non-empty cells parse with the one format
//!   [`date_parser::detect_date_format`] finds, the column filled on ≥ half the rows. Several →
//!   the header synonym (booking date before an unnamed column before the value date), else the
//!   leftmost. Every date column is claimed, the chosen one or not.
//! - **counterparty account**: ≥ 90 % of the non-empty cells are a checksum-valid IBAN
//!   ([`valid_iban`]) or a Czech domestic `[prefix-]number/bank`. A column holding one value on
//!   every row is the statement's own account, not the other party's.
//! - **currency**: ≥ 90 % of the non-empty cells are an ISO 4217 code in three capitals, at most
//!   five distinct, the column filled on ≥ half the rows.
//! - **decimal separator**: [`detect_decimal_separator`] over the cells of the columns that read
//!   as numbers (≥ 90 % of the cells with either separator); then a column is *numeric* when
//!   ≥ 90 % of its cells parse with that separator ([`clean_and_parse_amount`]).
//! - **amount**: a numeric column filled on ≥ half the rows with both signs, or a *debit + credit
//!   pair*: two one-signed numeric columns, neighbours among those, exactly one of them filled
//!   (non-zero) on ≥ 90 % of the rows. A one-signed column next to a direction word is not
//!   handled (left to the person's correction).
//! - **balance**: a numeric column filled on ≥ half the rows with `balance[i] − balance[i−1] =
//!   amount[i]` on ≥ 80 % of the consecutive pairs, in file order or reversed (newest-first
//!   files): `verified`; `= −amount[i]`: `inverted`; neither: no balance, `unchecked`. A proven
//!   reading of the amounts wins over an unproven one, and the balance decides which side of a
//!   pair is the debit.
//! - **transaction id**: a token (no spaces, a digit) unique and filled on every row, not a
//!   money-like numeric column (one with a decimal separator), not a 1…n row counter.
//! - **counterparty**: a text column with 2 ≤ distinct values ≤ 90 % of its filled cells and a
//!   mean length of 3–40 characters; several → the header synonym, else the most distinct.
//! - **description**: the remaining text columns that vary, longest mean length first, at most
//!   three.
//!
//! Pure and bounded: at most [`SAMPLE_ROWS`] rows are read, every rule is one pass over the
//! cells of the columns it looks at.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::models::currency::is_iso_4217;
use crate::services::date_parser::{self, DetectedDateFormat};
use crate::services::financial_semantics::valid_iban;

use super::amounts::{clean_and_parse_amount, detect_decimal_separator};
use super::columns::suggest_column_mappings;
use super::decode::{detect_csv_delimiter, detect_header_row, read_headers};
use super::inspect::decode_for_import_in;
use super::rows::{data_records, SAMPLE_ROWS};
use super::types::{InferredColumns, InferredField, SignCheck};

/// Share of a column's non-empty cells that must read as the role (date, account, currency,
/// number), and share of the rows on which exactly one side of a debit/credit pair is filled.
const MATCH_SHARE: f64 = 0.9;
/// Share of the consecutive row pairs on which a balance must agree with the amounts.
const BALANCE_SHARE: f64 = 0.8;
/// Share of the rows a date, amount, balance or currency column must fill.
const MIN_FILLED: f64 = 0.5;
/// Confidence taken off a field no header synonym agrees with.
const CONTENT_ONLY_PENALTY: f64 = 0.1;
/// Most distinct codes a currency column holds.
const MAX_CURRENCIES: usize = 5;
/// Most distinct counterparties, as a share of the column's filled cells: a column whose every
/// value is new is a description, not a list of parties.
const COUNTERPARTY_DISTINCT_SHARE: f64 = 0.9;
/// Mean length (characters) of a counterparty name.
const COUNTERPARTY_MEAN_LENGTH: std::ops::RangeInclusive<f64> = 3.0..=40.0;
const MAX_DESCRIPTION_COLUMNS: usize = 3;
/// Two amounts closer than this are the same (the balance chain of `mod.rs` uses it too).
const CENT: f64 = 0.005;

/// What the caller already knows about the file (what `parse_csv_file` reported); `None`
/// fields are detected the way the inspection detects them.
#[derive(Debug, Clone, Default)]
pub struct InferOptions<'a> {
    pub delimiter: Option<char>,
    /// 0-based line of the header row.
    pub header_row: Option<usize>,
    /// Force a text encoding.
    pub encoding: Option<&'a str>,
    /// ISO 3166-1 alpha-2 country of the profile: picks the legacy encoding of a file that is
    /// not UTF-8.
    pub country: Option<&'a str>,
}

/// Infer the columns of a file: the inspection's decode, delimiter and header row, then
/// [`infer_columns`] over the first [`SAMPLE_ROWS`] non-empty data rows. Never fails: a file
/// with nothing to read infers nothing.
pub fn infer_csv(bytes: &[u8], options: &InferOptions<'_>) -> InferredColumns {
    let decoded = decode_for_import_in(bytes, options.encoding, None, options.country);
    let content = &decoded.text;
    let delimiter = options
        .delimiter
        .unwrap_or_else(|| detect_csv_delimiter(content));
    let header_row = options
        .header_row
        .unwrap_or_else(|| detect_header_row(content, delimiter));
    let headers = read_headers(content, delimiter, header_row);
    let rows: Vec<Vec<String>> = data_records(content, delimiter, header_row, 0)
        .filter_map(|(_, record)| record.ok())
        .filter(|record| !record.iter().all(|f| f.trim().is_empty()))
        .take(SAMPLE_ROWS)
        .map(|record| record.iter().map(str::to_string).collect())
        .collect();
    infer_columns(&headers, &rows)
}

/// Assign roles to the columns of `rows` (the data rows under `headers`) from their content;
/// see the module documentation for the rules. Only the first [`SAMPLE_ROWS`] rows are read.
pub fn infer_columns(headers: &[String], rows: &[Vec<String>]) -> InferredColumns {
    let rows = &rows[..rows.len().min(SAMPLE_ROWS)];
    let mut out = InferredColumns {
        date: None,
        date_format: None,
        date_ambiguous: false,
        amount: None,
        debit: None,
        credit: None,
        balance: None,
        sign_check: SignCheck::Unchecked,
        currency: None,
        counterparty_iban: None,
        counterparty: None,
        transaction_id: None,
        description: Vec::new(),
        decimal_separator: ".".to_string(),
    };
    let row_count = rows.len();
    let columns: Vec<Column<'_>> = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| !header.trim().is_empty())
        .map(|(index, header)| Column::new(index, header, rows))
        .filter(|column| column.filled > 0)
        .collect();
    if columns.is_empty() {
        return out;
    }
    let synonyms = Synonyms(suggest_column_mappings(headers));
    // Columns a role has claimed (by position in `columns`).
    let mut claimed = vec![false; columns.len()];

    // Date.
    let dates: Vec<(usize, DetectedDateFormat, f64)> = columns
        .iter()
        .enumerate()
        .filter_map(|(k, column)| {
            date_reading(column, row_count).map(|(format, share)| (k, format, share))
        })
        .collect();
    let date_rank = |column: &Column<'_>| {
        if synonyms.names("date", column) {
            0
        } else if synonyms.names("valueDate", column) {
            2
        } else {
            1
        }
    };
    if let Some((k, format, share)) = dates
        .iter()
        .min_by_key(|(k, ..)| (date_rank(&columns[*k]), columns[*k].index))
    {
        let column = &columns[*k];
        out.date = Some(column.field(*share, synonyms.names("date", column)));
        out.date_format = Some(format.format.clone());
        out.date_ambiguous = format.ambiguous;
    }
    for (k, ..) in &dates {
        claimed[*k] = true;
    }

    // Counterparty account (IBAN or Czech domestic number).
    let accounts: Vec<(usize, f64)> = unclaimed(&columns, &claimed)
        .map(|k| (k, columns[k].share_matching(is_account_identifier)))
        .filter(|(_, share)| *share >= MATCH_SHARE)
        .collect();
    let counterparty_account = accounts
        .iter()
        .filter(|(k, _)| !columns[*k].is_constant_on_every_row(row_count))
        .min_by_key(|(k, _)| {
            let column = &columns[*k];
            (!synonyms.names("counterparty_iban", column), column.index)
        });
    if let Some((k, share)) = counterparty_account {
        let column = &columns[*k];
        out.counterparty_iban =
            Some(column.field(*share, synonyms.names("counterparty_iban", column)));
    }
    for (k, _) in &accounts {
        claimed[*k] = true;
    }

    // Currency.
    let currencies: Vec<(usize, f64)> = unclaimed(&columns, &claimed)
        .filter(|&k| columns[k].filled_share(row_count) >= MIN_FILLED)
        .map(|k| (k, columns[k].share_matching(is_currency_code)))
        .filter(|&(k, share)| share >= MATCH_SHARE && columns[k].distinct() <= MAX_CURRENCIES)
        .collect();
    if let Some((k, share)) = currencies.iter().min_by_key(|(k, _)| {
        let column = &columns[*k];
        (!synonyms.names("currency", column), column.index)
    }) {
        let column = &columns[*k];
        out.currency = Some(column.field(*share, synonyms.names("currency", column)));
    }
    for (k, _) in &currencies {
        claimed[*k] = true;
    }

    // Decimal separator, then the numeric columns.
    let number_like: Vec<usize> = unclaimed(&columns, &claimed)
        .filter(|&k| columns[k].share_matching(reads_as_number) >= MATCH_SHARE)
        .collect();
    let number_cells: Vec<&str> = number_like
        .iter()
        .flat_map(|&k| columns[k].values())
        .collect();
    let separator = detect_decimal_separator(&number_cells);
    out.decimal_separator = separator.to_string();
    let numerics: Vec<Numeric> = number_like
        .into_iter()
        .filter_map(|k| Numeric::read(k, &columns[k], separator, row_count))
        .collect();
    for numeric in &numerics {
        claimed[numeric.k] = true;
    }

    // Amount (single or debit + credit), balance and the sign check.
    let mut taken: Vec<usize> = Vec::new();
    if let Some(reading) = choose_amounts(&numerics, &columns, &synonyms, row_count) {
        match reading.kind {
            AmountKind::Single(a) => {
                let numeric = &numerics[a];
                let column = &columns[numeric.k];
                out.amount = Some(column.field(numeric.share, synonyms.names("amount", column)));
                taken.push(numeric.k);
            }
            AmountKind::Pair { debit, credit } => {
                let (d, c) = (numerics[debit].k, numerics[credit].k);
                let (debit, credit) = (&columns[d], &columns[c]);
                out.debit = Some(debit.field(reading.share, synonyms.names("debit", debit)));
                out.credit = Some(credit.field(reading.share, synonyms.names("credit", credit)));
                taken.extend([d, c]);
            }
        }
        if let Some(proof) = reading.proof {
            let column = &columns[numerics[proof.balance].k];
            out.balance = Some(column.field(proof.share, synonyms.names("balance", column)));
            out.sign_check = proof.check;
            taken.push(numerics[proof.balance].k);
        }
    }

    // Transaction id.
    let money_like: HashSet<usize> = numerics
        .iter()
        .filter(|n| n.money_like)
        .map(|n| n.k)
        .collect();
    let date_columns: HashSet<usize> = dates.iter().map(|(k, ..)| *k).collect();
    let account_like: HashSet<usize> = accounts.iter().map(|(k, _)| *k).collect();
    let currency_like: HashSet<usize> = currencies.iter().map(|(k, _)| *k).collect();
    let id = (0..columns.len())
        .filter(|k| {
            !taken.contains(k)
                && !money_like.contains(k)
                && !date_columns.contains(k)
                && !account_like.contains(k)
                && !currency_like.contains(k)
        })
        .filter(|&k| columns[k].is_transaction_id(row_count))
        .min_by_key(|&k| {
            let column = &columns[k];
            (!synonyms.names("transactionId", column), column.index)
        });
    if let Some(k) = id {
        let column = &columns[k];
        out.transaction_id = Some(column.field(1.0, synonyms.names("transactionId", column)));
        claimed[k] = true;
    }

    // Counterparty and description: the text columns nobody claimed.
    let texts: Vec<(usize, f64)> = unclaimed(&columns, &claimed)
        .map(|k| (k, columns[k].share_matching(has_letter)))
        .filter(|(_, share)| *share >= MATCH_SHARE)
        .collect();
    let counterparty = texts
        .iter()
        .filter(|(k, _)| columns[*k].is_counterparty())
        .min_by_key(|(k, _)| {
            let column = &columns[*k];
            (
                !synonyms.names("counterparty", column),
                Reverse(column.distinct()),
                column.index,
            )
        });
    if let Some((k, share)) = counterparty {
        let column = &columns[*k];
        out.counterparty = Some(column.field(*share, synonyms.names("counterparty", column)));
    }
    let mut descriptions: Vec<&Column<'_>> = texts
        .iter()
        .filter(|(k, _)| counterparty.is_none_or(|(c, _)| c != k))
        .map(|(k, _)| &columns[*k])
        .filter(|column| column.filled < 2 || column.distinct() > 1)
        .collect();
    descriptions.sort_by(|a, b| b.mean_length().total_cmp(&a.mean_length()));
    out.description = descriptions
        .into_iter()
        .take(MAX_DESCRIPTION_COLUMNS)
        .map(|column| column.header.to_string())
        .collect();

    out
}

/// The header synonyms: role key → the header [`suggest_column_mappings`] gave it.
struct Synonyms(HashMap<String, (String, f32)>);

impl Synonyms {
    /// Does the header of `column` name `role` (a `suggestedMappings` key)?
    fn names(&self, role: &str, column: &Column<'_>) -> bool {
        self.0
            .get(role)
            .is_some_and(|(header, _)| header == column.header)
    }
}

/// One column of the sample, cells trimmed.
struct Column<'a> {
    /// Position in the file (leftmost wins ties).
    index: usize,
    header: &'a str,
    /// One cell per sampled row, `""` when the row is short.
    cells: Vec<&'a str>,
    filled: usize,
}

impl<'a> Column<'a> {
    fn new(index: usize, header: &'a str, rows: &'a [Vec<String>]) -> Self {
        let cells: Vec<&str> = rows
            .iter()
            .map(|row| row.get(index).map_or("", |cell| cell.trim()))
            .collect();
        let filled = cells.iter().filter(|cell| !cell.is_empty()).count();
        Column {
            index,
            header,
            cells,
            filled,
        }
    }

    fn values(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.cells.iter().copied().filter(|cell| !cell.is_empty())
    }

    fn filled_share(&self, rows: usize) -> f64 {
        share(self.filled, rows)
    }

    /// Share of the non-empty cells that pass `test`.
    fn share_matching(&self, test: impl Fn(&str) -> bool) -> f64 {
        share(self.values().filter(|cell| test(cell)).count(), self.filled)
    }

    fn distinct(&self) -> usize {
        self.values().collect::<HashSet<_>>().len()
    }

    fn mean_length(&self) -> f64 {
        let total: usize = self.values().map(|cell| cell.chars().count()).sum();
        share(total, self.filled)
    }

    /// One value on every row (of at least two): the statement's own account, a product name.
    fn is_constant_on_every_row(&self, rows: usize) -> bool {
        rows >= 2 && self.filled == rows && self.distinct() == 1
    }

    /// A bank-side id: a token with a digit, filled and unique on every row (of at least two),
    /// not a 1…n row counter.
    fn is_transaction_id(&self, rows: usize) -> bool {
        if rows < 2 || self.filled != rows {
            return false;
        }
        let tokens = self.cells.iter().all(|cell| {
            !cell.chars().any(char::is_whitespace) && cell.chars().any(|c| c.is_ascii_digit())
        });
        tokens && self.distinct() == rows && !is_row_counter(&self.cells)
    }

    /// Names that repeat: 2 ≤ distinct ≤ 90 % of the filled cells, a mean length of 3–40.
    fn is_counterparty(&self) -> bool {
        let distinct = self.distinct();
        distinct >= 2
            && distinct as f64 <= COUNTERPARTY_DISTINCT_SHARE * self.filled as f64
            && COUNTERPARTY_MEAN_LENGTH.contains(&self.mean_length())
    }

    /// The field for this column: `share` of its cells matched, less the penalty when no
    /// header synonym agrees.
    fn field(&self, share: f64, synonym: bool) -> InferredField {
        let confidence = if synonym {
            share
        } else {
            share - CONTENT_ONLY_PENALTY
        };
        let rounded = (confidence.clamp(0.0, 1.0) * 100.0).round() / 100.0;
        InferredField {
            column: self.header.to_string(),
            confidence: rounded as f32,
        }
    }
}

/// `part / whole`, 0 for an empty whole.
fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

/// Positions of the columns no role has claimed yet.
fn unclaimed<'c>(
    columns: &'c [Column<'_>],
    claimed: &'c [bool],
) -> impl Iterator<Item = usize> + 'c {
    (0..columns.len()).filter(move |&k| !claimed[k])
}

/// The column's date format and the share of its cells that parse with it, when it is a
/// date column.
fn date_reading(column: &Column<'_>, rows: usize) -> Option<(DetectedDateFormat, f64)> {
    if column.filled_share(rows) < MIN_FILLED {
        return None;
    }
    let values: Vec<&str> = column.values().collect();
    let detected = date_parser::detect_date_format(&values);
    let parsed = values
        .iter()
        .filter(|value| date_parser::parse_date_strict(value, &detected.format).is_some())
        .count();
    let matched = share(parsed, values.len());
    (matched >= MATCH_SHARE).then_some((detected, matched))
}

/// A checksum-valid IBAN (spaces allowed) or a Czech domestic `[prefix-]number/bank`.
fn is_account_identifier(cell: &str) -> bool {
    let compact: String = cell
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    valid_iban(&compact) || is_czech_domestic_account(cell)
}

/// `[prefix-]number/bank`: a prefix of up to six digits, a number of 2–10 digits, a four-digit
/// bank code (`19-2000145399/0800`, `2101234567/2010`).
fn is_czech_domestic_account(cell: &str) -> bool {
    let digits = |part: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
    };
    let Some((account, bank)) = cell.trim().rsplit_once('/') else {
        return false;
    };
    if !digits(bank, 4..=4) {
        return false;
    }
    match account.split_once('-') {
        Some((prefix, number)) => digits(prefix, 1..=6) && digits(number, 2..=10),
        None => digits(account, 2..=10),
    }
}

/// An ISO 4217 code written as three capitals (`CZK`; `POS` is three capitals but no code).
fn is_currency_code(cell: &str) -> bool {
    cell.len() == 3 && cell.bytes().all(|b| b.is_ascii_uppercase()) && is_iso_4217(cell)
}

/// Reads as a number with either decimal separator.
fn reads_as_number(cell: &str) -> bool {
    clean_and_parse_amount(cell, '.').is_ok() || clean_and_parse_amount(cell, ',').is_ok()
}

fn has_letter(cell: &str) -> bool {
    cell.chars().any(char::is_alphabetic)
}

/// Whole numbers stepping by one from 0 or 1 (or down to it, newest first): a row number.
fn is_row_counter(cells: &[&str]) -> bool {
    let numbers: Option<Vec<u64>> = cells
        .iter()
        .map(|cell| {
            if cell.bytes().all(|b| b.is_ascii_digit()) {
                cell.parse().ok()
            } else {
                None
            }
        })
        .collect();
    let Some(numbers) = numbers else {
        return false;
    };
    let up = numbers
        .windows(2)
        .all(|w| w[0].checked_add(1) == Some(w[1]));
    let down = numbers
        .windows(2)
        .all(|w| w[1].checked_add(1) == Some(w[0]));
    (up && numbers.first().is_some_and(|&n| n <= 1))
        || (down && numbers.last().is_some_and(|&n| n <= 1))
}

/// A numeric column: its cells parsed with the file's decimal separator.
struct Numeric {
    /// Position of the column in the list of inferred columns.
    k: usize,
    /// One value per sampled row; `None` for an empty or unreadable cell.
    values: Vec<Option<f64>>,
    /// Share of the non-empty cells that parsed.
    share: f64,
    filled_share: f64,
    negative: bool,
    positive: bool,
    /// Some cell carries the decimal separator: money, not a code or an id.
    money_like: bool,
}

impl Numeric {
    fn read(k: usize, column: &Column<'_>, separator: char, rows: usize) -> Option<Self> {
        let values: Vec<Option<f64>> = column
            .cells
            .iter()
            .map(|cell| {
                if cell.is_empty() {
                    None
                } else {
                    clean_and_parse_amount(cell, separator).ok()
                }
            })
            .collect();
        let parsed_share = share(values.iter().flatten().count(), column.filled);
        if parsed_share < MATCH_SHARE {
            return None;
        }
        Some(Numeric {
            k,
            negative: values.iter().flatten().any(|&v| v < 0.0),
            positive: values.iter().flatten().any(|&v| v > 0.0),
            money_like: column.values().any(|cell| cell.contains(separator)),
            filled_share: column.filled_share(rows),
            values,
            share: parsed_share,
        })
    }

    /// All non-zero values share one sign (and there is one).
    fn one_signed(&self) -> bool {
        self.negative != self.positive
    }

    fn nonzero(&self, row: usize) -> bool {
        self.values[row].is_some_and(|v| v.abs() > 0.0)
    }
}

/// How the amounts are read: indexes into the numeric columns.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AmountKind {
    Single(usize),
    Pair { debit: usize, credit: usize },
}

/// A balance column that agrees with the amounts.
#[derive(Debug, Clone, Copy)]
struct BalanceProof {
    /// Index into the numeric columns.
    balance: usize,
    check: SignCheck,
    share: f64,
}

/// One way to read the amounts, with its evidence.
struct AmountReading {
    kind: AmountKind,
    /// Share of the cells (single) or rows (pair) that matched.
    share: f64,
    proof: Option<BalanceProof>,
}

/// The signed amount of every row under `kind` (a pair holds magnitudes: credit − debit, as
/// the import reads it).
fn amounts_of(kind: AmountKind, numerics: &[Numeric]) -> Vec<Option<f64>> {
    match kind {
        AmountKind::Single(a) => numerics[a].values.clone(),
        AmountKind::Pair { debit, credit } => numerics[debit]
            .values
            .iter()
            .zip(&numerics[credit].values)
            .map(|(d, c)| match (c.map(f64::abs), d.map(f64::abs)) {
                (None, None) => None,
                (Some(c), None) => Some(c),
                (None, Some(d)) => Some(-d),
                (Some(c), Some(d)) => Some(c - d),
            })
            .collect(),
    }
}

/// Does `balances` move by the amounts? Counted over the consecutive rows that have both
/// balances and a non-zero amount, in file order (oldest first) and reversed (newest first).
fn balance_check(amounts: &[Option<f64>], balances: &[Option<f64>]) -> Option<(SignCheck, f64)> {
    #[derive(Default)]
    struct Tally {
        pairs: usize,
        verified: usize,
        inverted: usize,
    }
    impl Tally {
        fn count(&mut self, step: f64, amount: Option<f64>) {
            let Some(amount) = amount.filter(|a| a.abs() >= CENT) else {
                return;
            };
            self.pairs += 1;
            if (step - amount).abs() < CENT {
                self.verified += 1;
            } else if (step + amount).abs() < CENT {
                self.inverted += 1;
            }
        }
    }
    let (mut oldest_first, mut newest_first) = (Tally::default(), Tally::default());
    for i in 1..amounts.len().min(balances.len()) {
        let (Some(previous), Some(current)) = (balances[i - 1], balances[i]) else {
            continue;
        };
        // Oldest first: this row's balance is the previous one plus this row's amount.
        oldest_first.count(current - previous, amounts[i]);
        // Newest first: the previous row's balance is this one plus the previous row's amount.
        newest_first.count(previous - current, amounts[i - 1]);
    }
    let best = |of: fn(&Tally) -> usize| {
        [&oldest_first, &newest_first]
            .iter()
            .map(|tally| share(of(tally), tally.pairs))
            .fold(0.0, f64::max)
    };
    let (verified, inverted) = (best(|t| t.verified), best(|t| t.inverted));
    if verified >= BALANCE_SHARE {
        Some((SignCheck::Verified, verified))
    } else if inverted >= BALANCE_SHARE {
        Some((SignCheck::Inverted, inverted))
    } else {
        None
    }
}

/// The balance column that best agrees with `kind`: header synonym first, then the share of
/// agreeing pairs, then the leftmost.
fn prove(
    kind: AmountKind,
    numerics: &[Numeric],
    columns: &[Column<'_>],
    synonyms: &Synonyms,
) -> Option<BalanceProof> {
    let used = match kind {
        AmountKind::Single(a) => [a, a],
        AmountKind::Pair { debit, credit } => [debit, credit],
    };
    let amounts = amounts_of(kind, numerics);
    numerics
        .iter()
        .enumerate()
        .filter(|(b, numeric)| !used.contains(b) && numeric.filled_share >= MIN_FILLED)
        .filter_map(|(b, numeric)| {
            balance_check(&amounts, &numeric.values).map(|(check, share)| BalanceProof {
                balance: b,
                check,
                share,
            })
        })
        .max_by(|x, y| {
            let column = |p: &BalanceProof| &columns[numerics[p.balance].k];
            let named = |p: &BalanceProof| synonyms.names("balance", column(p));
            named(x)
                .cmp(&named(y))
                .then(x.share.total_cmp(&y.share))
                .then(column(y).index.cmp(&column(x).index))
        })
}

/// Choose how the amounts are read. Candidates: every numeric column with both signs filled on
/// ≥ half the rows, and every pair of neighbouring one-signed numeric columns of which exactly
/// one is non-zero on ≥ 90 % of the rows. A reading a balance proves wins; then the header
/// synonyms; then a single column over a pair; then money-like columns; then the leftmost.
fn choose_amounts(
    numerics: &[Numeric],
    columns: &[Column<'_>],
    synonyms: &Synonyms,
    rows: usize,
) -> Option<AmountReading> {
    let mut readings: Vec<AmountReading> = Vec::new();
    for (a, numeric) in numerics.iter().enumerate() {
        if numeric.negative && numeric.positive && numeric.filled_share >= MIN_FILLED {
            let kind = AmountKind::Single(a);
            readings.push(AmountReading {
                kind,
                share: numeric.share,
                proof: prove(kind, numerics, columns, synonyms),
            });
        }
    }
    let one_signed: Vec<usize> = (0..numerics.len())
        .filter(|&i| numerics[i].one_signed())
        .collect();
    for pair in one_signed.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let exactly_one = (0..rows)
            .filter(|&row| numerics[left].nonzero(row) != numerics[right].nonzero(row))
            .count();
        let pair_share = share(exactly_one, rows);
        if pair_share < MATCH_SHARE {
            continue;
        }
        let kind = AmountKind::Pair {
            debit: left,
            credit: right,
        };
        let reading = match prove(kind, numerics, columns, synonyms) {
            // The balance moves against this reading: the sides are the other way round.
            Some(proof) if proof.check == SignCheck::Inverted => AmountReading {
                kind: AmountKind::Pair {
                    debit: right,
                    credit: left,
                },
                share: pair_share,
                proof: Some(BalanceProof {
                    check: SignCheck::Verified,
                    ..proof
                }),
            },
            Some(proof) => AmountReading {
                kind,
                share: pair_share,
                proof: Some(proof),
            },
            None => AmountReading {
                kind: orient_unproven_pair(left, right, numerics, columns, synonyms),
                share: pair_share,
                proof: None,
            },
        };
        readings.push(reading);
    }

    let header_named = |kind: AmountKind| match kind {
        AmountKind::Single(a) => synonyms.names("amount", &columns[numerics[a].k]),
        AmountKind::Pair { debit, credit } => [debit, credit].iter().any(|&i| {
            let column = &columns[numerics[i].k];
            synonyms.names("debit", column) || synonyms.names("credit", column)
        }),
    };
    let money_like = |kind: AmountKind| match kind {
        AmountKind::Single(a) => numerics[a].money_like,
        AmountKind::Pair { debit, credit } => {
            numerics[debit].money_like && numerics[credit].money_like
        }
    };
    let leftmost = |kind: AmountKind| match kind {
        AmountKind::Single(a) => columns[numerics[a].k].index,
        AmountKind::Pair { debit, credit } => columns[numerics[debit].k]
            .index
            .min(columns[numerics[credit].k].index),
    };
    readings.into_iter().max_by_key(|reading| {
        (
            reading.proof.is_some(),
            header_named(reading.kind),
            matches!(reading.kind, AmountKind::Single(_)),
            money_like(reading.kind),
            Reverse(leftmost(reading.kind)),
        )
    })
}

/// The sides of a pair no balance proves: a column of negative values is the debit; else the
/// header synonyms; else the left column (Debit/Credit, Paid out/Paid in, Výdaj/Příjem).
fn orient_unproven_pair(
    left: usize,
    right: usize,
    numerics: &[Numeric],
    columns: &[Column<'_>],
    synonyms: &Synonyms,
) -> AmountKind {
    let swapped = AmountKind::Pair {
        debit: right,
        credit: left,
    };
    let kept = AmountKind::Pair {
        debit: left,
        credit: right,
    };
    let (l, r) = (&numerics[left], &numerics[right]);
    if r.negative && !l.negative {
        return swapped;
    }
    if l.negative && !r.negative {
        return kept;
    }
    let (left_column, right_column) = (&columns[l.k], &columns[r.k]);
    if synonyms.names("debit", right_column) || synonyms.names("credit", left_column) {
        swapped
    } else {
        kept
    }
}
