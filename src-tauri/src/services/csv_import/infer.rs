//! Content-based inference of the columns of a CSV no preset describes (mobile spec §8.6,
//! rung 3 "guided recognition"). Every column gets its role from what its cells hold; the
//! header synonyms of [`suggest_column_mappings`] only break ties between columns the content
//! already qualified, and a field only the content decided carries 0.1 less confidence. When
//! the content cannot decide the date or the amount, the field stays empty and the phone asks:
//! the app never guesses either silently.
//!
//! The rules, in the order they claim columns (a claimed column takes no later role):
//! - **date**: ≥ 90 % of the non-empty cells parse with the one format
//!   [`date_parser::detect_date_format`] finds, the column filled on ≥ half the rows. Several →
//!   a column that varies before one holding a single date (a statement date on every row),
//!   then the header synonym (booking date before an unnamed column before the value date),
//!   then the leftmost. Every date column is claimed, the chosen one or not. The chosen dates
//!   also tell the file's order (ascending = oldest first, descending = newest first).
//! - **counterparty account**: ≥ 90 % of the non-empty cells are an account identifier: a
//!   checksum-valid IBAN, or a Czech `[prefix-]number/bank` that passes the mod-11 checks and
//!   becomes one ([`canonical_account_identifier`] + [`valid_iban`]). A column holding one value
//!   on every row is the statement's own account, not the other party's.
//! - **currency**: ≥ 90 % of the non-empty cells are an ISO 4217 code in three capitals, at most
//!   five distinct, the column filled on ≥ half the rows. Without a currency header it is not
//!   taken when another signed money column sits beside the amount (an original or foreign
//!   amount, which that currency belongs to).
//! - **decimal separator**: [`detect_decimal_separator`] over the cells of the columns that read
//!   as numbers (≥ 90 % of the cells with either separator); then a column is *numeric* when
//!   ≥ 90 % of its cells parse with that separator ([`clean_and_parse_amount`]). A numeric column
//!   is *money* when it carries the decimal separator, or when no column of the file does (a
//!   whole-number file); codes and symbols never are.
//! - **amount**: a numeric column filled on ≥ half the rows with both signs; a one-signed money
//!   column only when a balance proves it (without one it may be magnitudes beside a direction
//!   word); or a *debit + credit pair*: two neighbouring one-signed money columns that are never
//!   both non-zero on a row, exactly one filled on ≥ 90 % of the rows. An unproven pair whose
//!   sides neither the signs nor the headers tell apart is not guessed.
//! - **balance**: a money column filled on ≥ half the rows with `balance[i] − balance[i−1] =
//!   amount[i]` on ≥ 80 % of the consecutive pairs where the balance moved or there is an amount,
//!   read in the file's order (both orders only when the dates do not tell), and no pair saying
//!   the opposite sign: `verified`; the mirror image: `inverted`; else no balance, `unchecked`.
//!   A proven reading of the amounts wins, then the header synonyms, then the reading that
//!   covers more rows; the balance decides which side of a pair is the debit.
//! - **transaction id**: a token (no spaces, a digit) unique and filled on every row, not money,
//!   not a column with both signs or a balance header, not a 1…n row counter.
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
use crate::services::financial_semantics::{canonical_account_identifier, valid_iban};

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

    // Date, and the file's order from it.
    let dates: Vec<(usize, DetectedDateFormat, f64)> = columns
        .iter()
        .enumerate()
        .filter_map(|(k, column)| {
            date_reading(column, row_count).map(|(format, share)| (k, format, share))
        })
        .collect();
    let some_date_varies = dates.iter().any(|(k, ..)| !columns[*k].is_constant());
    let date_rank = |column: &Column<'_>| {
        let synonym = if synonyms.names("date", column) {
            0
        } else if synonyms.names("valueDate", column) {
            2
        } else {
            1
        };
        (
            some_date_varies && column.is_constant(),
            synonym,
            column.index,
        )
    };
    let mut order = FileOrder::Unknown;
    if let Some((k, format, share)) = dates.iter().min_by_key(|(k, ..)| date_rank(&columns[*k])) {
        let column = &columns[*k];
        out.date = Some(column.field(*share, synonyms.names("date", column)));
        out.date_format = Some(format.format.clone());
        out.date_ambiguous = format.ambiguous;
        order = file_order(column, &format.format);
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

    // Currency columns (which one, if any, is the row currency is settled after the amounts).
    let currencies: Vec<(usize, f64)> = unclaimed(&columns, &claimed)
        .filter(|&k| columns[k].filled_share(row_count) >= MIN_FILLED)
        .map(|k| (k, columns[k].share_matching(is_currency_code)))
        .filter(|&(k, share)| share >= MATCH_SHARE && columns[k].distinct() <= MAX_CURRENCIES)
        .collect();
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
    let mut numerics: Vec<Numeric> = number_like
        .into_iter()
        .filter_map(|k| Numeric::read(k, &columns[k], separator, row_count))
        .collect();
    let file_has_decimals = numerics.iter().any(|n| n.carries_separator);
    for numeric in &mut numerics {
        numeric.money = numeric.carries_separator || !file_has_decimals;
        claimed[numeric.k] = true;
    }

    // Amount (single or debit + credit), balance and the sign check.
    let mut taken: Vec<usize> = Vec::new();
    let reader = AmountReader {
        numerics: &numerics,
        columns: &columns,
        synonyms: &synonyms,
        rows: row_count,
        order,
    };
    if let Some(reading) = reader.choose() {
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

    // The row currency: a currency column the header names, or any when no other signed money
    // column (an original amount) sits beside the amount.
    let second_amount = numerics
        .iter()
        .any(|n| n.money && n.negative && !taken.contains(&n.k));
    let currency = currencies
        .iter()
        .filter(|(k, _)| synonyms.names("currency", &columns[*k]) || !second_amount)
        .min_by_key(|(k, _)| {
            let column = &columns[*k];
            (!synonyms.names("currency", column), column.index)
        });
    if let Some((k, share)) = currency {
        let column = &columns[*k];
        out.currency = Some(column.field(*share, synonyms.names("currency", column)));
    }

    // Transaction id: never money, a signed column or a balance.
    let not_an_id: HashSet<usize> = numerics
        .iter()
        .filter(|n| {
            n.money || (n.negative && n.positive) || synonyms.names("balance", &columns[n.k])
        })
        .map(|n| n.k)
        .chain(dates.iter().map(|(k, ..)| *k))
        .chain(accounts.iter().map(|(k, _)| *k))
        .chain(currencies.iter().map(|(k, _)| *k))
        .chain(taken.iter().copied())
        .collect();
    let id = (0..columns.len())
        .filter(|k| !not_an_id.contains(k))
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
        .filter(|column| !column.is_constant())
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

    /// At least two values, all the same (a statement date, a product name).
    fn is_constant(&self) -> bool {
        self.filled >= 2 && self.distinct() == 1
    }

    /// One value on every row (of at least two): the statement's own account.
    fn is_constant_on_every_row(&self, rows: usize) -> bool {
        self.filled == rows && self.is_constant()
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

/// The order of the rows, as the dates show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOrder {
    /// Dates only ever rise.
    OldestFirst,
    /// Dates only ever fall.
    NewestFirst,
    /// No date column, one date throughout, or dates that rise and fall.
    Unknown,
}

fn file_order(column: &Column<'_>, format: &str) -> FileOrder {
    let dates: Vec<_> = column
        .values()
        .filter_map(|value| date_parser::parse_date_strict(value, format))
        .collect();
    let rises = dates.windows(2).any(|w| w[1] > w[0]);
    let falls = dates.windows(2).any(|w| w[1] < w[0]);
    match (rises, falls) {
        (true, false) => FileOrder::OldestFirst,
        (false, true) => FileOrder::NewestFirst,
        _ => FileOrder::Unknown,
    }
}

/// A checksum-valid IBAN (spaces allowed), or a Czech `[prefix-]number/bank` whose mod-11
/// checks pass, so that it has an IBAN.
fn is_account_identifier(cell: &str) -> bool {
    valid_iban(&canonical_account_identifier(cell, None))
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
    /// Some cell carries the decimal separator.
    carries_separator: bool,
    /// Money: it carries the decimal separator, or no column of the file does. A code or a
    /// symbol (VS, KS, bank code, id) never is in a file that writes money with decimals.
    money: bool,
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
            carries_separator: column.values().any(|cell| cell.contains(separator)),
            money: false,
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
    /// Rows with a non-zero amount under this reading.
    coverage: usize,
}

/// Does `balances` move by the amounts? Counted over the consecutive rows where the balance
/// moved or there is an amount (a move without an amount is a miss), in the file's order; both
/// orders only when the dates do not tell it. A pair that agrees with the opposite sign spoils
/// the proof: the column then holds magnitudes, not signed amounts.
fn balance_check(
    amounts: &[Option<f64>],
    balances: &[Option<f64>],
    order: FileOrder,
) -> Option<(SignCheck, f64)> {
    #[derive(Default)]
    struct Tally {
        pairs: usize,
        verified: usize,
        inverted: usize,
    }
    impl Tally {
        fn count(&mut self, step: f64, amount: Option<f64>) {
            let amount = amount.filter(|a| a.abs() >= CENT);
            if amount.is_none() && step.abs() < CENT {
                return;
            }
            self.pairs += 1;
            let Some(amount) = amount else {
                return;
            };
            if (step - amount).abs() < CENT {
                self.verified += 1;
            } else if (step + amount).abs() < CENT {
                self.inverted += 1;
            }
        }

        fn proof(&self) -> Option<(SignCheck, f64)> {
            let (verified, inverted) = (
                share(self.verified, self.pairs),
                share(self.inverted, self.pairs),
            );
            if verified >= BALANCE_SHARE && self.inverted == 0 {
                Some((SignCheck::Verified, verified))
            } else if inverted >= BALANCE_SHARE && self.verified == 0 {
                Some((SignCheck::Inverted, inverted))
            } else {
                None
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
    let readable = match order {
        FileOrder::OldestFirst => [Some(&oldest_first), None],
        FileOrder::NewestFirst => [None, Some(&newest_first)],
        FileOrder::Unknown => [Some(&oldest_first), Some(&newest_first)],
    };
    readable
        .into_iter()
        .flatten()
        .filter_map(Tally::proof)
        .max_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then((a.0 == SignCheck::Verified).cmp(&(b.0 == SignCheck::Verified)))
        })
}

/// Everything the amount choice looks at.
struct AmountReader<'r, 'a> {
    numerics: &'r [Numeric],
    columns: &'r [Column<'a>],
    synonyms: &'r Synonyms,
    rows: usize,
    order: FileOrder,
}

impl AmountReader<'_, '_> {
    fn column(&self, numeric: usize) -> &Column<'_> {
        &self.columns[self.numerics[numeric].k]
    }

    /// The signed amount of every row under `kind` (a pair holds magnitudes: credit − debit,
    /// as the import reads it).
    fn amounts_of(&self, kind: AmountKind) -> Vec<Option<f64>> {
        match kind {
            AmountKind::Single(a) => self.numerics[a].values.clone(),
            AmountKind::Pair { debit, credit } => self.numerics[debit]
                .values
                .iter()
                .zip(&self.numerics[credit].values)
                .map(|(d, c)| match (c.map(f64::abs), d.map(f64::abs)) {
                    (None, None) => None,
                    (Some(c), None) => Some(c),
                    (None, Some(d)) => Some(-d),
                    (Some(c), Some(d)) => Some(c - d),
                })
                .collect(),
        }
    }

    /// The balance column that best agrees with `kind`: a money column filled on ≥ half the
    /// rows; header synonym first, then the share of agreeing pairs, then the leftmost.
    fn prove(&self, kind: AmountKind) -> Option<BalanceProof> {
        let used = match kind {
            AmountKind::Single(a) => [a, a],
            AmountKind::Pair { debit, credit } => [debit, credit],
        };
        let amounts = self.amounts_of(kind);
        self.numerics
            .iter()
            .enumerate()
            .filter(|(b, numeric)| {
                !used.contains(b) && numeric.money && numeric.filled_share >= MIN_FILLED
            })
            .filter_map(|(b, numeric)| {
                balance_check(&amounts, &numeric.values, self.order).map(|(check, share)| {
                    BalanceProof {
                        balance: b,
                        check,
                        share,
                    }
                })
            })
            .max_by(|x, y| {
                let named =
                    |p: &BalanceProof| self.synonyms.names("balance", self.column(p.balance));
                named(x)
                    .cmp(&named(y))
                    .then(x.share.total_cmp(&y.share))
                    .then(
                        self.column(y.balance)
                            .index
                            .cmp(&self.column(x.balance).index),
                    )
            })
    }

    fn reading(&self, kind: AmountKind, share: f64, proof: Option<BalanceProof>) -> AmountReading {
        let coverage = self
            .amounts_of(kind)
            .iter()
            .filter(|a| a.is_some_and(|v| v.abs() > 0.0))
            .count();
        AmountReading {
            kind,
            share,
            proof,
            coverage,
        }
    }

    /// Choose how the amounts are read. Candidates: a numeric column with both signs filled on
    /// ≥ half the rows; a one-signed money column filled on ≥ half the rows that a balance
    /// proves; neighbouring one-signed money columns never both non-zero on a row and with
    /// exactly one non-zero on ≥ 90 % of the rows (a pair). A reading a balance proves wins;
    /// then the header synonyms; then the one covering more rows; then a single column over a
    /// pair; then money; then the leftmost.
    fn choose(&self) -> Option<AmountReading> {
        let numerics = self.numerics;
        let mut readings: Vec<AmountReading> = Vec::new();
        for (a, numeric) in numerics.iter().enumerate() {
            if numeric.filled_share < MIN_FILLED {
                continue;
            }
            let kind = AmountKind::Single(a);
            let both_signs = numeric.negative && numeric.positive;
            if !both_signs && !(numeric.one_signed() && numeric.money) {
                continue;
            }
            let proof = self.prove(kind);
            // A one-signed column without a balance may be magnitudes beside a direction word.
            if both_signs || proof.is_some() {
                readings.push(self.reading(kind, numeric.share, proof));
            }
        }

        let sides: Vec<usize> = (0..numerics.len())
            .filter(|&i| numerics[i].one_signed() && numerics[i].money)
            .collect();
        for pair in sides.windows(2) {
            if let Some(reading) = self.pair_reading(pair[0], pair[1]) {
                readings.push(reading);
            }
        }

        let header_named = |kind: AmountKind| match kind {
            AmountKind::Single(a) => self.synonyms.names("amount", self.column(a)),
            AmountKind::Pair { debit, credit } => [debit, credit].iter().any(|&i| {
                let column = self.column(i);
                self.synonyms.names("debit", column) || self.synonyms.names("credit", column)
            }),
        };
        let money = |kind: AmountKind| match kind {
            AmountKind::Single(a) => numerics[a].money,
            AmountKind::Pair { .. } => true,
        };
        let leftmost = |kind: AmountKind| match kind {
            AmountKind::Single(a) => self.column(a).index,
            AmountKind::Pair { debit, credit } => {
                self.column(debit).index.min(self.column(credit).index)
            }
        };
        readings.into_iter().max_by_key(|reading| {
            (
                reading.proof.is_some(),
                header_named(reading.kind),
                reading.coverage,
                matches!(reading.kind, AmountKind::Single(_)),
                money(reading.kind),
                Reverse(leftmost(reading.kind)),
            )
        })
    }

    /// `left` and `right` read as a debit + credit pair, when they are one: never both
    /// non-zero on a row, exactly one non-zero on ≥ 90 % of the rows, and sides that a balance,
    /// the signs or the headers tell apart.
    fn pair_reading(&self, left: usize, right: usize) -> Option<AmountReading> {
        let (l, r) = (&self.numerics[left], &self.numerics[right]);
        if (0..self.rows).any(|row| l.nonzero(row) && r.nonzero(row)) {
            return None;
        }
        let exactly_one = (0..self.rows)
            .filter(|&row| l.nonzero(row) != r.nonzero(row))
            .count();
        let pair_share = share(exactly_one, self.rows);
        if pair_share < MATCH_SHARE {
            return None;
        }
        let kept = AmountKind::Pair {
            debit: left,
            credit: right,
        };
        let swapped = AmountKind::Pair {
            debit: right,
            credit: left,
        };
        match self.prove(kept) {
            // The balance moves against this reading: the sides are the other way round.
            Some(proof) if proof.check == SignCheck::Inverted => Some(self.reading(
                swapped,
                pair_share,
                Some(BalanceProof {
                    check: SignCheck::Verified,
                    ..proof
                }),
            )),
            Some(proof) => Some(self.reading(kept, pair_share, Some(proof))),
            None => self
                .orient_unproven_pair(left, right)
                .map(|kind| self.reading(kind, pair_share, None)),
        }
    }

    /// The sides of a pair no balance proves: a column of negative values is the debit; else
    /// the header synonyms; else they cannot be told apart and the person is asked.
    fn orient_unproven_pair(&self, left: usize, right: usize) -> Option<AmountKind> {
        let kept = AmountKind::Pair {
            debit: left,
            credit: right,
        };
        let swapped = AmountKind::Pair {
            debit: right,
            credit: left,
        };
        let (l, r) = (&self.numerics[left], &self.numerics[right]);
        if r.negative && !l.negative {
            return Some(swapped);
        }
        if l.negative && !r.negative {
            return Some(kept);
        }
        let names = |role: &str, i: usize| self.synonyms.names(role, self.column(i));
        if names("debit", right) || names("credit", left) {
            Some(swapped)
        } else if names("debit", left) || names("credit", right) {
            Some(kept)
        } else {
            None
        }
    }
}
