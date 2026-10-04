//! XTB: the Cash operations export (`;`, `dd.MM.yyyy HH:mm:ss`).
//!
//! `ID;Type;Time;Symbol;Comment;Amount`, one row per cash movement: deposits,
//! dividends, taxes, interest and the trades (`Stocks/ETF purchase`,
//! `Stocks/ETF sale`). A trade's quantity and price are only in its comment
//! (`OPEN BUY 34/42.5658 @ 11.7480`: 34 shares at 11.748, `CLOSE BUY 1 @
//! 26.280`: one sold), the symbol carries an XTB suffix (`AAPL.US`,
//! `VUSA.UK`) and there is no currency column. The comment decides the
//! direction; the type column only tells a trade from the rest.
//!
//! The headers are translated in the other languages of the platform; they are
//! found by name, in any position.

use std::sync::LazyLock;

use regex::Regex;

use crate::services::csv_import::amounts::{clean_and_parse_amount, detect_decimal_separator};
use crate::services::stock_import::types::{CurrencyMode, TradeDirection, SOURCE_XTB};

use super::{base_config, find_header, AdapterConfig, StockAdapter};

pub const ADAPTER: StockAdapter = StockAdapter {
    source: SOURCE_XTB,
    detect,
    config,
};

// Normalised header names per column, in the languages of the platform.
const ID: &[&str] = &["id"];
const TYPE: &[&str] = &["type", "typ", "tipo", "rodzaj", "tip", "tipus"];
const TIME: &[&str] = &[
    "time", "cas", "czas", "hora", "zeit", "heure", "ora", "data", "ido",
];
const SYMBOL: &[&str] = &[
    "symbol",
    "simbolo",
    "symbole",
    "simbol",
    "szimbolum",
    "sembol",
];
const COMMENT: &[&str] = &[
    "comment",
    "komentar",
    "komentarz",
    "kommentar",
    "comentario",
    "commentaire",
    "commento",
    "comentariu",
    "megjegyzes",
];
const AMOUNT: &[&str] = &[
    "amount", "castka", "kwota", "betrag", "importe", "montante", "montant", "importo", "suma",
    "osszeg",
];

/// Positions of the columns the adapter reads.
struct Columns {
    id: usize,
    kind: usize,
    time: usize,
    symbol: usize,
    comment: usize,
}

/// The six columns of the export; the amount is required (it is part of the
/// signature) but not read.
fn columns(headers: &[String]) -> Option<Columns> {
    find_header(headers, AMOUNT)?;
    Some(Columns {
        id: find_header(headers, ID)?,
        kind: find_header(headers, TYPE)?,
        time: find_header(headers, TIME)?,
        symbol: find_header(headers, SYMBOL)?,
        comment: find_header(headers, COMMENT)?,
    })
}

/// `OPEN BUY 34/42.5658 @ 11.7480`, `CLOSE BUY 1 @ 26.280`: the direction,
/// the quantity (the number before an optional `/total`) and the price.
static TRADE_COMMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(OPEN|CLOSE)\s+BUY\s+(\d[\d.,]*)\s*(?:/\s*\d[\d.,]*)?\s*@\s*(\d[\d.,]*)")
        .expect("Invalid XTB comment regex")
});

/// XTB cash operations with a trade among the first rows.
pub fn detect(headers: &[String], rows: &[Vec<String>]) -> bool {
    let Some(cols) = columns(headers) else {
        return false;
    };
    rows.iter()
        .any(|row| row.get(cols.comment).is_some_and(|c| is_trade_comment(c)))
}

pub fn config(headers: &[String], rows: &[Vec<String>]) -> Option<AdapterConfig> {
    let cols = columns(headers)?;
    let mut config = base_config(SOURCE_XTB);
    config.date_column = cols.time;
    config.date_format = "%d.%m.%Y".to_string();
    config.symbol_column = Some(cols.symbol);
    // The comment holds both numbers.
    config.quantity_column = cols.comment;
    config.price_column = cols.comment;
    config.currency_mode = CurrencyMode::Instrument;
    config.type_column = Some(cols.kind);
    config.external_id_column = Some(cols.id);
    config.transforms.xtb_comment = true;
    config.transforms.xtb_symbols = true;

    let numbers: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get(cols.comment))
        .filter_map(|comment| comment_numbers(comment))
        .flat_map(|(quantity, price)| [quantity, price])
        .collect();
    config.decimal_separator = detect_decimal_separator(&numbers).to_string();

    Some(AdapterConfig {
        config,
        date_ambiguous: false,
    })
}

/// A trade read from an XTB comment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XtbTrade {
    /// `OPEN` is a purchase, `CLOSE` a sale.
    pub direction: TradeDirection,
    pub quantity: f64,
    pub price: f64,
}

/// The quantity and price texts of a trade comment.
fn comment_numbers(text: &str) -> Option<(&str, &str)> {
    let caps = TRADE_COMMENT.captures(text)?;
    Some((caps.get(2)?.as_str(), caps.get(3)?.as_str()))
}

/// Does the comment have the shape of a trade (the numbers are not read)?
pub fn is_trade_comment(text: &str) -> bool {
    TRADE_COMMENT.is_match(text)
}

/// Read a trade from a comment; the numbers follow the file's `decimal`
/// separator. `None` for any other comment and for numbers that do not read.
pub fn parse_comment(text: &str, decimal: char) -> Option<XtbTrade> {
    let caps = TRADE_COMMENT.captures(text)?;
    let direction = if caps.get(1)?.as_str().eq_ignore_ascii_case("open") {
        TradeDirection::Buy
    } else {
        TradeDirection::Sell
    };
    Some(XtbTrade {
        direction,
        quantity: clean_and_parse_amount(caps.get(2)?.as_str(), decimal).ok()?,
        price: clean_and_parse_amount(caps.get(3)?.as_str(), decimal).ok()?,
    })
}

/// XTB exchange suffix → Yahoo Finance suffix (`.US` has none).
const SUFFIXES: [(&str, &str); 18] = [
    ("US", ""),
    ("UK", ".L"),
    ("DE", ".DE"),
    ("FR", ".PA"),
    ("NL", ".AS"),
    ("BE", ".BR"),
    ("ES", ".MC"),
    ("IT", ".MI"),
    ("PT", ".LS"),
    ("CH", ".SW"),
    ("DK", ".CO"),
    ("SE", ".ST"),
    ("NO", ".OL"),
    ("FI", ".HE"),
    ("PL", ".WA"),
    ("CZ", ".PR"),
    ("AT", ".VI"),
    ("IE", ".IR"),
];

/// The Yahoo Finance symbol of an XTB symbol: `AAPL.US` → `AAPL`, `VUSA.UK` →
/// `VUSA.L`, `CEZ.CZ` → `CEZ.PR`. A suffix not on the list is kept, as is a
/// symbol without one. Uppercase.
pub fn yahoo_symbol(symbol: &str) -> String {
    let upper = symbol.trim().to_uppercase();
    match upper.rsplit_once('.') {
        Some((base, suffix)) if !base.is_empty() => {
            match SUFFIXES.iter().find(|(xtb, _)| *xtb == suffix) {
                Some((_, yahoo)) => format!("{base}{yahoo}"),
                None => upper,
            }
        }
        _ => upper,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::stock_import::adapters::test_support::*;
    use crate::services::stock_import::types::{CurrencyMode, DirectionMode};

    fn sample_rows() -> Vec<Vec<String>> {
        rows(&[
            &[
                "530692719",
                "Stocks/ETF purchase",
                "12.04.2024 13:01:45",
                "SPYL.DE",
                "OPEN BUY 34/42.5658 @ 11.7480",
                "-399.43",
            ],
            &[
                "530691593",
                "Deposit",
                "12.04.2024 13:00:22",
                "",
                "Bank transfer deposit",
                "500",
            ],
            &[
                "530690001",
                "Stocks/ETF sale",
                "15.04.2024 09:30:10",
                "AAPL.US",
                "CLOSE BUY 1 @ 26.280",
                "26.28",
            ],
        ])
    }

    #[test]
    fn the_cash_operations_export_is_detected() {
        assert!(detect(&headers(&XTB_HEADERS), &sample_rows()));
    }

    #[test]
    fn detection_needs_a_trade_comment() {
        let deposits = rows(&[&[
            "1",
            "Deposit",
            "12.04.2024 13:00:22",
            "",
            "Bank transfer deposit",
            "500",
        ]]);
        assert!(!detect(&headers(&XTB_HEADERS), &deposits));
        assert!(!detect(&headers(&XTB_HEADERS), &[]));
        // A dividend comment is no trade comment.
        let dividend = rows(&[&[
            "2",
            "Dividend",
            "15.02.2024 06:10:11",
            "AAPL.US",
            "AAPL.US USD 0.2400/ SHR",
            "0.72",
        ]]);
        assert!(!detect(&headers(&XTB_HEADERS), &dividend));
    }

    #[test]
    fn translated_headers_are_detected() {
        for list in [
            ["ID", "Typ", "Čas", "Symbol", "Komentář", "Částka"],
            ["ID", "Rodzaj", "Czas", "Symbol", "Komentarz", "Kwota"],
            ["ID", "Tipo", "Hora", "Símbolo", "Comentario", "Importe"],
            ["ID", "Tipo", "Hora", "Símbolo", "Comentário", "Montante"],
            ["ID", "Typ", "Zeit", "Symbol", "Kommentar", "Betrag"],
        ] {
            assert!(detect(&headers(&list), &sample_rows()), "{list:?}");
        }
    }

    #[test]
    fn other_exports_are_not_xtb() {
        let data = sample_rows();
        for list in [
            &TRADING212_HEADERS[..],
            &DEGIRO_HEADERS[..],
            &IBKR_HEADERS[..],
            &MOONY_HEADERS_EN[..],
        ] {
            assert!(!detect(&headers(list), &data), "{list:?}");
        }
        // Five of the six columns are not enough.
        let five = headers(&["ID", "Type", "Time", "Symbol", "Comment"]);
        assert!(!detect(&five, &data));
    }

    #[test]
    fn the_config_reads_the_trade_from_the_comment() {
        let adapter = config(&headers(&XTB_HEADERS), &sample_rows()).expect("config");
        let c = adapter.config;
        assert_eq!(c.source, "xtb");
        assert_eq!((c.date_column, c.date_format.as_str()), (2, "%d.%m.%Y"));
        assert_eq!(c.symbol_column, Some(3));
        assert_eq!(c.isin_column, None);
        assert_eq!((c.quantity_column, c.price_column), (4, 4));
        assert_eq!(c.currency_mode, CurrencyMode::Instrument);
        assert_eq!(c.direction_mode, DirectionMode::TypeColumn);
        assert_eq!(c.type_column, Some(1));
        assert_eq!(c.external_id_column, Some(0));
        assert!(c.transforms.xtb_comment && c.transforms.xtb_symbols);
        assert_eq!(c.decimal_separator, ".");
        assert!(
            !adapter.date_ambiguous,
            "the XTB format is known, not guessed"
        );
    }

    #[test]
    fn columns_are_found_by_name_wherever_they_are() {
        let h = headers(&[
            "Account",
            "ID",
            "Typ",
            "Čas",
            "Symbol",
            "Komentář",
            "Částka",
            "",
        ]);
        let c = config(&h, &[]).expect("config").config;
        assert_eq!(
            (c.external_id_column, c.type_column, c.date_column),
            (Some(1), Some(2), 3)
        );
        assert_eq!((c.symbol_column, c.quantity_column), (Some(4), 5));
        assert!(config(&headers(&MOONY_HEADERS_EN), &[]).is_none());
    }

    #[test]
    fn a_comma_decimal_file_is_noticed_in_the_comments() {
        let data = rows(&[&[
            "1",
            "Stocks/ETF purchase",
            "12.04.2024 13:01:45",
            "A.US",
            "OPEN BUY 0,5/1,5 @ 100,25",
            "-50",
        ]]);
        let c = config(&headers(&XTB_HEADERS), &data)
            .expect("config")
            .config;
        assert_eq!(c.decimal_separator, ",");
    }

    #[test]
    fn a_trade_comment_has_a_direction_a_quantity_and_a_price() {
        let t = parse_comment("OPEN BUY 34/42.5658 @ 11.7480", '.').expect("open");
        assert_eq!(
            (t.direction, t.quantity, t.price),
            (TradeDirection::Buy, 34.0, 11.748)
        );
        let t = parse_comment("CLOSE BUY 1 @ 26.280", '.').expect("close");
        assert_eq!(
            (t.direction, t.quantity, t.price),
            (TradeDirection::Sell, 1.0, 26.28)
        );
        // Fractional shares, case, spacing, trailing words.
        let t = parse_comment("  open buy  0.5 / 3  @  9.5 (partial)", '.').expect("loose");
        assert_eq!(
            (t.direction, t.quantity, t.price),
            (TradeDirection::Buy, 0.5, 9.5)
        );
        // Decimal commas and thousands separators follow the file.
        let t = parse_comment("OPEN BUY 0,5/1,5 @ 100,25", ',').expect("comma");
        assert_eq!((t.quantity, t.price), (0.5, 100.25));
        let t = parse_comment("OPEN BUY 1000 @ 1,234.50", '.').expect("thousands");
        assert_eq!((t.quantity, t.price), (1000.0, 1234.5));
    }

    #[test]
    fn other_comments_are_not_trades() {
        for text in [
            "",
            "Bank transfer deposit",
            "AAPL.US USD 0.2400/ SHR",
            "OPEN SELL 1 @ 5",
            "CLOSE SELL 1 @ 5",
            "OPEN BUY @ 5",
            "OPEN BUY 1 @",
            "OPEN BUY one @ 5",
            "BUY 1 @ 5",
        ] {
            assert!(parse_comment(text, '.').is_none(), "{text:?}");
            assert!(!is_trade_comment(text), "{text:?}");
        }
        // The shape alone decides is_trade_comment; the numbers are read later.
        assert!(is_trade_comment("OPEN BUY 1,5 @ 2,5"));
        assert!(
            parse_comment("OPEN BUY 1,5 @ 2,5", '.').is_none(),
            "1,5 is not a '.' number"
        );
    }

    #[test]
    fn xtb_suffixes_become_yahoo_symbols() {
        for (xtb, yahoo) in [
            ("AAPL.US", "AAPL"),
            ("VUSA.UK", "VUSA.L"),
            ("SPYL.DE", "SPYL.DE"),
            ("TTE.FR", "TTE.PA"),
            ("ASML.NL", "ASML.AS"),
            ("UCB.BE", "UCB.BR"),
            ("SAN.ES", "SAN.MC"),
            ("ENEL.IT", "ENEL.MI"),
            ("EDP.PT", "EDP.LS"),
            ("NESN.CH", "NESN.SW"),
            ("NOVO.DK", "NOVO.CO"),
            ("VOLV.SE", "VOLV.ST"),
            ("EQNR.NO", "EQNR.OL"),
            ("NOKIA.FI", "NOKIA.HE"),
            ("PKO.PL", "PKO.WA"),
            ("CEZ.CZ", "CEZ.PR"),
            ("VOE.AT", "VOE.VI"),
            ("RYA.IE", "RYA.IR"),
        ] {
            assert_eq!(yahoo_symbol(xtb), yahoo, "{xtb}");
        }
    }

    #[test]
    fn unknown_suffixes_and_plain_symbols_are_kept() {
        assert_eq!(yahoo_symbol("ABC.XX"), "ABC.XX");
        assert_eq!(yahoo_symbol("AAPL"), "AAPL");
        assert_eq!(yahoo_symbol("aapl.us"), "AAPL");
        assert_eq!(yahoo_symbol(" vusa.uk "), "VUSA.L");
        // Only the last suffix counts.
        assert_eq!(yahoo_symbol("BRK.B.US"), "BRK.B");
        assert_eq!(yahoo_symbol(".US"), ".US");
        assert_eq!(yahoo_symbol(""), "");
    }
}
