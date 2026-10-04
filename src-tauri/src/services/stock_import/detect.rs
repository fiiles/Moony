//! Header-row detection for stock exports, the records after the header and
//! the small cell readers every stage shares.
//!
//! Brokers write a title, a period or a header record before the column names
//! (Interactive Brokers' optional `BOF` line), so the header is found by what
//! its cells mean, with the stock column roles instead of the bank's.

use crate::services::csv_import::decode::{
    csv_reader, detect_header_row_by, slice_from_line, LineIndex,
};

use super::columns::{suggest_stock_columns, StockRole};

/// Whether `fields` (the trimmed cells of a line) are a stock header: at least
/// three non-empty cells, matching at least two stock roles, one of them the
/// date, the quantity or the price. Preamble lines have fewer cells or none of
/// those words; a data line has no role words at all.
pub fn is_stock_header(fields: &[String]) -> bool {
    if fields.iter().filter(|f| !f.is_empty()).count() < 3 {
        return false;
    }
    let roles = suggest_stock_columns(fields);
    let has_core = roles.iter().any(|s| {
        [StockRole::Date, StockRole::Quantity, StockRole::Price]
            .iter()
            .any(|r| r.key() == s.role)
    });
    roles.len() >= 2 && has_core
}

/// 0-based line index of the header row; line 0 when nothing qualifies (the
/// bank import's fallback).
pub fn detect_stock_header_row(content: &str, delimiter: char) -> usize {
    detect_header_row_by(content, delimiter, is_stock_header)
}

/// The date of a date cell. What follows a `;` is a time (Interactive
/// Brokers' `20230522;093000`); a time after a space or a `T` is left to the
/// date parser, which drops it.
pub fn date_part(cell: &str) -> &str {
    let cell = cell.trim();
    cell.split_once(';').map_or(cell, |(date, _)| date).trim()
}

/// Whether `value` has the shape of an ISIN: two letters, nine letters or
/// digits and a check digit (the digit itself is not verified).
pub fn is_isin(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 12
        && bytes[..2].iter().all(u8::is_ascii_alphabetic)
        && bytes[2..11].iter().all(u8::is_ascii_alphanumeric)
        && bytes[11].is_ascii_digit()
}

/// Data records of `content` after the header row and `skip_rows`, with the
/// 1-based file line each starts on. Blank lines are skipped by the reader; the
/// line comes from the byte offset, because the csv crate's own counter
/// ignores them and a quoted cell can span lines.
pub fn data_records(
    content: &str,
    delimiter: char,
    header_row: usize,
    skip_rows: usize,
) -> impl Iterator<Item = (usize, std::result::Result<csv::StringRecord, csv::Error>)> + '_ {
    let tail = slice_from_line(content, header_row);
    let index = LineIndex::new(tail);
    csv_reader(tail, delimiter)
        .into_records()
        .skip(1 + skip_rows)
        .map(move |result| {
            let byte = match &result {
                Ok(record) => record.position().map(|p| p.byte() as usize),
                Err(e) => e.position().map(|p| p.byte() as usize),
            };
            // The tail starts on 0-based line `header_row`.
            let line = byte.map_or(0, |b| header_row + index.line_of_record(b) + 1);
            (line, result)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;

    #[test]
    fn the_header_rows_of_every_source_are_headers() {
        for list in [
            &XTB_HEADERS[..],
            &TRADING212_HEADERS[..],
            &DEGIRO_HEADERS[..],
            &DEGIRO_AUTOFX_HEADERS[..],
            &IBKR_HEADERS[..],
            &MOONY_HEADERS_EN[..],
            &MOONY_HEADERS_CS[..],
            &[
                "Trade Date",
                "Action",
                "Symbol",
                "Shares",
                "Unit Price",
                "CCY",
            ][..],
        ] {
            assert!(is_stock_header(&headers(list)), "{list:?}");
        }
    }

    #[test]
    fn a_header_needs_three_cells_two_roles_and_a_date_quantity_or_price() {
        // Two cells only.
        assert!(!is_stock_header(&headers(&["Date", "Price"])));
        // Blank cells do not count.
        assert!(!is_stock_header(&headers(&["Date", "", "Price", ""])));
        // One role.
        assert!(!is_stock_header(&headers(&["Date", "Foo", "Bar"])));
        // Roles, but none of date, quantity, price.
        assert!(!is_stock_header(&headers(&["Type", "Symbol", "Name"])));
        // A data line.
        assert!(!is_stock_header(&headers(&["15.01.2024", "buy", "AAPL"])));
        // Quantity or price is as good as a date.
        assert!(is_stock_header(&headers(&["Symbol", "Quantity", "Foo"])));
        assert!(is_stock_header(&headers(&["Ticker", "Price", "Foo"])));
    }

    #[test]
    fn the_header_is_on_the_first_line_when_there_is_no_preamble() {
        let content =
            "Date,Type,Ticker,Quantity,Price,Currency\n2025-01-15,buy,AAPL,10,180.50,USD\n";
        assert_eq!(detect_stock_header_row(content, ','), 0);
    }

    #[test]
    fn a_preamble_is_skipped() {
        // Interactive Brokers' optional header record.
        let ibkr = "\"BOF\",\"U1234567\",\"Activity Flex Query\",\"20230101\",\"20231231\"\n\
                    \"Symbol\",\"ISIN\",\"TradeDate\",\"Quantity\",\"TradePrice\",\"CurrencyPrimary\"\n\
                    \"AAPL\",\"US0378331005\",\"20230522\",\"3\",\"172.4\",\"USD\"\n";
        assert_eq!(detect_stock_header_row(ibkr, ','), 1);

        // A statement with a title, a period and a blank line.
        let statement = "Výpis obchodů;;\nObdobí;01.01.2024;31.12.2024\n\nDatum;Typ;Symbol;Počet;Cena;Měna\n15.01.2024;nákup;AAPL;10;180,50;USD\n";
        assert_eq!(detect_stock_header_row(statement, ';'), 3);
    }

    #[test]
    fn nothing_that_looks_like_a_header_falls_back_to_the_first_line() {
        assert_eq!(detect_stock_header_row("foo,bar,baz\n1,2,3\n", ','), 0);
        assert_eq!(detect_stock_header_row("", ','), 0);
    }

    #[test]
    fn the_date_of_a_cell_is_what_comes_before_a_semicolon() {
        // Interactive Brokers' DateTime: date;time.
        assert_eq!(date_part("20230522;093000"), "20230522");
        assert_eq!(date_part("  20230522 ; 093000 "), "20230522");
        // Everything else is left to the date parser, which drops a time.
        assert_eq!(
            date_part(" 2023-12-18 11:45:06.326 "),
            "2023-12-18 11:45:06.326"
        );
        assert_eq!(date_part("12.04.2024 13:01:45"), "12.04.2024 13:01:45");
        assert_eq!(date_part(""), "");
        assert_eq!(date_part(";x"), "");
    }

    #[test]
    fn an_isin_is_two_letters_nine_alphanumerics_and_a_digit() {
        for isin in [
            "US0378331005",
            "IE00B3XXRP09",
            "GB00BH4HKS39",
            "us0378331005",
        ] {
            assert!(is_isin(isin), "{isin}");
        }
        for not_isin in [
            "",
            "US037833100",   // eleven characters
            "US03783310055", // thirteen
            "0S0378331005",  // starts with a digit
            "US037833100X",  // does not end in a digit
            "US0378-31005",  // punctuation
            "AAPL",
            "ÚS0378331005",
        ] {
            assert!(!is_isin(not_isin), "{not_isin}");
        }
    }

    fn lines(content: &str, header_row: usize, skip_rows: usize) -> Vec<(usize, Vec<String>)> {
        data_records(content, ';', header_row, skip_rows)
            .map(|(line, record)| {
                let record = record.expect("readable record");
                (line, record.iter().map(str::to_string).collect())
            })
            .collect()
    }

    #[test]
    fn records_carry_their_physical_one_based_line() {
        let content = "Date;Qty;Price\n01.01.2024;1;2\n02.01.2024;3;4\n";
        let rows = lines(content, 0, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 2);
        assert_eq!(rows[0].1, vec!["01.01.2024", "1", "2"]);
        assert_eq!(rows[1].0, 3);
    }

    #[test]
    fn blank_lines_and_the_preamble_do_not_shift_the_line_numbers() {
        // The csv crate skips blank lines and its own counter ignores them.
        let content = "pre;amble\n\nDate;Qty;Price\n01.01.2024;1;2\n\n02.01.2024;3;4\n";
        let rows = lines(content, 2, 0);
        assert_eq!(
            rows.iter().map(|(line, _)| *line).collect::<Vec<_>>(),
            vec![4, 6]
        );
        // Skipped rows still count as lines.
        let rows = lines(content, 2, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, 6);
    }

    #[test]
    fn a_quoted_cell_over_several_lines_is_one_record() {
        let content = "H1;H2;H3\n\"a\nb\";1;2\n3;4;5\n";
        let rows = lines(content, 0, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (2, vec!["a\nb".into(), "1".into(), "2".into()]));
        assert_eq!(rows[1].0, 4);
    }

    #[test]
    fn a_header_past_the_end_leaves_no_records() {
        assert!(lines("a;b;c\n", 5, 0).is_empty());
        assert!(lines("", 0, 0).is_empty());
    }
}
