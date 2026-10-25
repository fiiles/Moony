//! Pure Yahoo Finance symbol logic for crypto holdings (spec 2026-10-25, D2 / D4 / D5 / D6).
//!
//! No I/O lives here: the functions turn Yahoo symbols into display tickers and USD pairs,
//! parse a `v1/finance/search` body, merge the rows of two searches, and choose the one symbol
//! that matches a holding, or none (never a guess).

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One coin offered by the search: the Yahoo USD pair, its display ticker and a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CryptoSearchResult {
    pub symbol: String,
    pub ticker: String,
    pub name: String,
}

/// Fewer digits than this at the end of a base are part of the ticker (`BTC2`).
const MIN_ID_DIGITS: usize = 4;

/// The part of a Yahoo symbol before its last `-` (the whole symbol when it has none).
fn base_of(symbol: &str) -> &str {
    symbol.rsplit_once('-').map_or(symbol, |(base, _)| base)
}

/// The part after the last `-`, if any.
fn quote_of(symbol: &str) -> Option<&str> {
    symbol.rsplit_once('-').map(|(_, quote)| quote)
}

/// `"SUI20947-USD"` → `"SUI"`; `"W-USD"` → `"W"`; `"1INCH-USD"` → `"1INCH"`; `"GRT6719-USD"` → `"GRT"`.
///
/// A trailing run of four or more digits is a CoinMarketCap id and is removed when at least one
/// letter remains (D5).
pub fn display_ticker(symbol: &str) -> String {
    let base = base_of(symbol);
    let stem = base.trim_end_matches(|c: char| c.is_ascii_digit());
    let digits = base.len() - stem.len();
    if digits >= MIN_ID_DIGITS && stem.chars().any(|c| c.is_ascii_alphabetic()) {
        stem.to_string()
    } else {
        base.to_string()
    }
}

/// `"GRT6719-ETH"` → `"GRT6719-USD"`; `"BTC-USD"` unchanged; a symbol without `-` gets `-USD` appended.
pub fn usd_pair(symbol: &str) -> String {
    format!("{}-USD", base_of(symbol))
}

/// D2 rule; used by validate() and the MCP tool.
///
/// ASCII uppercase letters, digits, `.` and `-`, 3–40 characters, a non-empty base, ending in `-USD`.
pub fn is_valid_symbol(symbol: &str) -> bool {
    let allowed = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '.' || c == '-';
    (3..=40).contains(&symbol.len())
        && symbol.chars().all(allowed)
        && symbol
            .strip_suffix("-USD")
            .is_some_and(|base| !base.is_empty())
}

/// Trim + uppercase, then validate; Err(AppError::Validation("validation.yahooSymbolInvalid")).
pub fn normalize_symbol(input: &str) -> Result<String> {
    let symbol = input.trim().to_uppercase();
    if is_valid_symbol(&symbol) {
        Ok(symbol)
    } else {
        Err(AppError::Validation("validation.yahooSymbolInvalid".into()))
    }
}

#[derive(Deserialize)]
struct SearchBody {
    quotes: Option<Vec<SearchQuote>>,
}

#[derive(Deserialize)]
struct SearchQuote {
    symbol: String,
    shortname: Option<String>,
    longname: Option<String>,
    #[serde(rename = "quoteType")]
    quote_type: Option<String>,
}

/// `shortname` with a trailing ` <QUOTE>` stripped, else `longname`, else the symbol.
fn result_name(quote: &SearchQuote) -> String {
    let stripped = quote.shortname.as_deref().map(|short| {
        let short = short.trim();
        quote_of(&quote.symbol)
            .and_then(|q| short.strip_suffix(&format!(" {q}")))
            .unwrap_or(short)
            .trim()
    });
    stripped
        .filter(|s| !s.is_empty())
        .or_else(|| {
            quote
                .longname
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(&quote.symbol)
        .to_string()
}

/// Parse a Yahoo `v1/finance/search` body (D6): CRYPTOCURRENCY rows only, USD pair, deduped by
/// base in Yahoo's order; `name` = shortname with a trailing " <QUOTE>" stripped, else longname, else symbol.
pub fn parse_search_response(body: &str) -> Result<Vec<CryptoSearchResult>> {
    let parsed: SearchBody = serde_json::from_str(body)
        .map_err(|e| AppError::ExternalApi(format!("Yahoo search parse error: {e}")))?;
    let rows = parsed
        .quotes
        .unwrap_or_default()
        .into_iter()
        .filter(|q| q.quote_type.as_deref() == Some("CRYPTOCURRENCY"))
        .map(|q| {
            let symbol = usd_pair(&q.symbol);
            CryptoSearchResult {
                ticker: display_ticker(&symbol),
                name: result_name(&q),
                symbol,
            }
        })
        .collect();
    Ok(merge_search_results(rows, Vec::new()))
}

/// The rows of the ` USD` response followed by the plain response, deduplicated by base (first wins).
pub fn merge_search_results(
    first: Vec<CryptoSearchResult>,
    second: Vec<CryptoSearchResult>,
) -> Vec<CryptoSearchResult> {
    let mut merged: Vec<CryptoSearchResult> = Vec::new();
    for row in first.into_iter().chain(second) {
        if !merged
            .iter()
            .any(|kept| base_of(&kept.symbol) == base_of(&row.symbol))
        {
            merged.push(row);
        }
    }
    merged
}

/// D4 chooser over merged rows: candidates = display-ticker match (case-insensitive); exactly one → it;
/// several → those whose name equals `name` (case-insensitive); exactly one → it; else None.
pub fn choose_symbol(name: &str, ticker: &str, rows: &[CryptoSearchResult]) -> Option<String> {
    let candidates: Vec<&CryptoSearchResult> = rows
        .iter()
        .filter(|row| row.ticker.eq_ignore_ascii_case(ticker.trim()))
        .collect();
    match candidates.as_slice() {
        [only] => Some(only.symbol.clone()),
        [] => None,
        several => {
            let by_name: Vec<&&CryptoSearchResult> = several
                .iter()
                .filter(|row| row.name.eq_ignore_ascii_case(name.trim()))
                .collect();
            match by_name.as_slice() {
                [only] => Some(only.symbol.clone()),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEPE_BODY: &str = r#"{"quotes":[
        {"symbol":"PEPE24478-USD","shortname":"Pepe USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"PEP-USD","shortname":"Pepecoin USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"PEPECOIN-USD","shortname":"PepeCoin USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"PURPE-USD","shortname":"Purple Pepe USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"PEPE24549-USD","shortname":"Arbi Pepe USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"PORK29220-USD","shortname":"PepeFork USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"DOPE42609-USD","shortname":"Doge Pepe (dogepepeeth.lol) USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"}
    ]}"#;

    const GRAPH_BODY: &str = r#"{"quotes":[
        {"symbol":"GRT6719-ETH","shortname":"The Graph ETH","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-AUD","shortname":"The Graph AUD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-INR","shortname":"The Graph INR","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-GBP","shortname":"The Graph GBP","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-JPY","shortname":"The Graph JPY","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-RUB","shortname":"The Graph RUB","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"GRT6719-EUR","shortname":"The Graph EUR","quoteType":"CRYPTOCURRENCY","exchange":"CCC"}
    ]}"#;

    const ARBITRUM_BODY: &str = r#"{"quotes":[
        {"symbol":"AARB.AS","shortname":"Amundi ARB","quoteType":"ETF","exchange":"AMS"},
        {"symbol":"ARB11841-USD","shortname":"Arbitrum USD","longname":"Arbitrum USD","quoteType":"CRYPTOCURRENCY","exchange":"CCC"},
        {"symbol":"AARB-EUR.PA","shortname":"Amundi ARB EUR","longname":"Amundi ARB EUR Acc","quoteType":"ETF","exchange":"PAR"},
        {"symbol":"CH1304867463.SG","shortname":"ARB Fund","longname":"ARB Fund Ltd","quoteType":"MUTUALFUND","exchange":"SGO"},
        {"symbol":"VIRARB.ST","shortname":"Virtune ARB","longname":"Virtune Arbitrum ETP","quoteType":"ETF","exchange":"STO"}
    ]}"#;

    fn row(symbol: &str, ticker: &str, name: &str) -> CryptoSearchResult {
        CryptoSearchResult {
            symbol: symbol.into(),
            ticker: ticker.into(),
            name: name.into(),
        }
    }

    #[test]
    fn display_ticker_strips_a_coinmarketcap_suffix() {
        assert_eq!(display_ticker("SUI20947-USD"), "SUI");
        assert_eq!(display_ticker("GRT6719-USD"), "GRT");
        assert_eq!(display_ticker("BTC-USD"), "BTC");
        assert_eq!(display_ticker("W-USD"), "W");
        assert_eq!(display_ticker("1INCH-USD"), "1INCH");
        assert_eq!(display_ticker("BTC2-USD"), "BTC2"); // fewer than four digits stay
    }

    #[test]
    fn usd_pair_replaces_the_quote_currency() {
        assert_eq!(usd_pair("GRT6719-ETH"), "GRT6719-USD");
        assert_eq!(usd_pair("POL28321-EUR"), "POL28321-USD");
        assert_eq!(usd_pair("BTC-USD"), "BTC-USD");
        assert_eq!(usd_pair("BTC"), "BTC-USD");
    }

    #[test]
    fn symbol_validation_follows_d2() {
        assert!(is_valid_symbol("BTC-USD"));
        assert!(is_valid_symbol("SUI20947-USD"));
        assert!(!is_valid_symbol("btc-usd"));
        assert!(!is_valid_symbol("BTC-EUR"));
        assert!(!is_valid_symbol("-USD"));
        assert!(!is_valid_symbol(&format!("{}-USD", "A".repeat(40))));
        assert_eq!(normalize_symbol(" btc-usd ").unwrap(), "BTC-USD");
        assert!(normalize_symbol("bitcoin").is_err());
    }

    #[test]
    fn search_parsing_keeps_coins_as_usd_pairs_in_yahoo_order() {
        // PEPE fixture (spec §1 / the probe): 7 CRYPTOCURRENCY rows
        let rows = parse_search_response(PEPE_BODY).unwrap();
        assert_eq!(
            rows[0],
            CryptoSearchResult {
                symbol: "PEPE24478-USD".into(),
                ticker: "PEPE".into(),
                name: "Pepe".into()
            }
        );
        assert_eq!(rows[1].symbol, "PEP-USD");
        assert_eq!(rows.len(), 7);
        // GRAPH fixture: seven non-USD pairs of one base collapse to one USD pair
        let rows = parse_search_response(GRAPH_BODY).unwrap();
        assert_eq!(
            rows,
            vec![CryptoSearchResult {
                symbol: "GRT6719-USD".into(),
                ticker: "GRT".into(),
                name: "The Graph".into()
            }]
        );
        // ARBITRUM fixture: ETF and MUTUALFUND rows are dropped
        let rows = parse_search_response(ARBITRUM_BODY).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].symbol, "ARB11841-USD");
        assert!(parse_search_response("not json").is_err());
        assert!(parse_search_response(r#"{"quotes":[]}"#)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn the_chooser_takes_a_unique_ticker_match_then_a_unique_name_match_else_nothing() {
        let arb = vec![
            row("ARB-USD", "ARB", "ARbit"),
            row("ARB11841-USD", "ARB", "Arbitrum"),
            row("ARCT-USD", "ARCT", "ArbitrageCT"),
        ];
        assert_eq!(
            choose_symbol("Arbitrum", "ARB", &arb).as_deref(),
            Some("ARB11841-USD")
        );
        assert_eq!(
            choose_symbol("arbitrum", "arb", &arb).as_deref(),
            Some("ARB11841-USD")
        );
        assert_eq!(choose_symbol("ARB", "ARB", &arb), None); // two ticker matches, no name match: no guess
        let btc = vec![
            row("BTC-USD", "BTC", "Bitcoin"),
            row("WBTC-USD", "WBTC", "Wrapped Bitcoin"),
        ];
        assert_eq!(
            choose_symbol("anything", "BTC", &btc).as_deref(),
            Some("BTC-USD")
        ); // unique ticker match wins without the name
        let jup = vec![
            row("JUP-USD", "JUP", "Jupiter"),
            row("JUP29210-USD", "JUP", "Jupiter"),
        ];
        assert_eq!(choose_symbol("Jupiter", "JUP", &jup), None); // identical names: no guess
        assert_eq!(choose_symbol("Bitcoin", "BTC", &arb), None);
    }

    #[test]
    fn merged_search_results_keep_the_first_occurrence_of_a_base() {
        let merged = merge_search_results(
            vec![row("ADA-USD", "ADA", "Cardano")],
            vec![
                row("ADA-USD", "ADA", "Cardano"),
                row("POL28321-USD", "POL", "Polygon (prev. MATIC)"),
            ],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1].symbol, "POL28321-USD");
    }
}
